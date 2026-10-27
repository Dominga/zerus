//! Per-frame batches for the webview.
//!
//! The view asks for the next [`Batch`] once per display frame. Between two
//! requests a [`Coalescer`] folds every [`Change`] so that a burst of thousands
//! of deltas costs one batch with the latest row per key, and its memory stays
//! bounded by the number of distinct keys.
use crate::{ActivityUpdate, Change, ComputerId, ComputerStatus, ViewRow};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Events one followed session keeps pending; beyond it the oldest go and the
/// update becomes a window replacement.
pub const PENDING_EVENTS: usize = 1000;

/// One row of one computer.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RowOut {
    pub computer: ComputerId,
    pub key: String,
    pub row: ViewRow,
}

/// A removed key; `reason` is `replaced`, `archived`, `forgotten`, `ended` or null.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct KeyOut {
    pub computer: ComputerId,
    pub key: String,
    pub reason: Option<String>,
}

/// All rows of one computer, replacing what the view holds for it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ReplaceOut {
    pub computer: ComputerId,
    pub rows: Vec<RowOut>,
}

/// A computer's connection state: `probing`, `unsupported`, `live`,
/// `reconnecting` (with its `attempt`) or `ended`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ComputerOut {
    pub computer: ComputerId,
    pub status: String,
    pub attempt: u32,
}

/// One Activity update of a followed session. `kind` and `payload`:
/// `events` and `replace` carry `{events, cursor}`, `event` `{id, rev, event}`,
/// `state` `{value, rev}` and `ended` null.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ActivityOut {
    pub computer: ComputerId,
    pub key: String,
    pub kind: String,
    pub payload: Value,
}

/// Everything that changed since the previous batch. The view applies, in
/// order: `computers`, `replaces`, `removes`, `upserts`, `activity`. Row changes
/// of a computer with a replacement are already part of it; otherwise a key is
/// in at most one of `removes` and `upserts`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Batch {
    pub upserts: Vec<RowOut>,
    pub removes: Vec<KeyOut>,
    pub replaces: Vec<ReplaceOut>,
    pub computers: Vec<ComputerOut>,
    pub activity: Vec<ActivityOut>,
}

impl Batch {
    pub fn is_empty(&self) -> bool {
        self.upserts.is_empty()
            && self.removes.is_empty()
            && self.replaces.is_empty()
            && self.computers.is_empty()
            && self.activity.is_empty()
    }
}

// Most pending changes are upserts; boxing them would allocate per change.
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
enum RowChange {
    Upsert(ViewRow),
    Remove(Option<String>),
}

type SessionKey = (ComputerId, String);

/// Folds changes until the next [`Coalescer::take`]:
///
/// - the latest upsert of a `(computer, key)` wins; a removal cancels a
///   pending upsert and an upsert a pending removal;
/// - a replacement drops the computer's pending row changes, and later row
///   changes of that computer fold into it;
/// - the latest status of a computer wins;
/// - Activity updates of a key keep their order: consecutive events merge
///   (into a pending window replacement too), a replacement drops the events
///   before it, the latest summary and the highest `rev` of an event id win,
///   and an end drops what was pending before it.
///
/// Memory is bounded by the distinct keys, the computers and
/// [`PENDING_EVENTS`] per followed session.
#[derive(Debug, Default)]
pub struct Coalescer {
    computers: BTreeMap<ComputerId, ComputerStatus>,
    replaces: BTreeMap<ComputerId, BTreeMap<String, ViewRow>>,
    rows: BTreeMap<SessionKey, RowChange>,
    activity: BTreeMap<SessionKey, Vec<ActivityUpdate>>,
    /// Activity keys in the order they first changed.
    order: Vec<SessionKey>,
}

impl Coalescer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, change: Change) {
        match change {
            Change::Upsert { computer, key, row } => match self.replaces.get_mut(&computer) {
                Some(rows) => {
                    rows.insert(key, row);
                }
                None => {
                    self.rows.insert((computer, key), RowChange::Upsert(row));
                }
            },
            Change::Remove {
                computer,
                key,
                reason,
            } => match self.replaces.get_mut(&computer) {
                Some(rows) => {
                    rows.remove(&key);
                }
                None => {
                    self.rows.insert((computer, key), RowChange::Remove(reason));
                }
            },
            Change::Replace { computer, rows } => {
                let pending: Vec<SessionKey> = self
                    .rows
                    .range((computer.clone(), String::new())..)
                    .take_while(|((owner, _), _)| *owner == computer)
                    .map(|(key, _)| key.clone())
                    .collect();
                for key in pending {
                    self.rows.remove(&key);
                }
                self.replaces.insert(computer, rows.into_iter().collect());
            }
            Change::Computer { computer, status } => {
                self.computers.insert(computer, status);
            }
            Change::Activity {
                computer,
                key,
                update,
            } => {
                let id = (computer, key);
                let list = match self.activity.get_mut(&id) {
                    Some(list) => list,
                    None => {
                        self.order.push(id.clone());
                        self.activity.entry(id).or_default()
                    }
                };
                fold(list, update);
            }
        }
    }

    pub fn take(&mut self) -> Batch {
        let mut batch = Batch::default();
        for (computer, status) in std::mem::take(&mut self.computers) {
            batch.computers.push(ComputerOut {
                computer,
                status: status.name().to_owned(),
                attempt: status.attempt(),
            });
        }
        for (computer, rows) in std::mem::take(&mut self.replaces) {
            let rows = rows
                .into_iter()
                .map(|(key, row)| RowOut {
                    computer: computer.clone(),
                    key,
                    row,
                })
                .collect();
            batch.replaces.push(ReplaceOut { computer, rows });
        }
        for ((computer, key), change) in std::mem::take(&mut self.rows) {
            match change {
                RowChange::Upsert(row) => batch.upserts.push(RowOut { computer, key, row }),
                RowChange::Remove(reason) => batch.removes.push(KeyOut {
                    computer,
                    key,
                    reason,
                }),
            }
        }
        let mut activity = std::mem::take(&mut self.activity);
        for id in std::mem::take(&mut self.order) {
            let Some(updates) = activity.remove(&id) else {
                continue;
            };
            for update in updates {
                let (kind, payload) = out(update);
                batch.activity.push(ActivityOut {
                    computer: id.0.clone(),
                    key: id.1.clone(),
                    kind: kind.to_owned(),
                    payload,
                });
            }
        }
        batch
    }

    /// Pending entries: rows (a replacement counts each row), computers and
    /// Activity updates, with events counted one by one.
    pub fn pending(&self) -> usize {
        let replaced: usize = self.replaces.values().map(|rows| rows.len().max(1)).sum();
        let activity: usize =
            self.activity
                .values()
                .flatten()
                .map(|update| match update {
                    ActivityUpdate::Events { events, .. }
                    | ActivityUpdate::Replace { events, .. } => events.len().max(1),
                    _ => 1,
                })
                .sum();
        self.computers.len() + replaced + self.rows.len() + activity
    }

    pub fn is_empty(&self) -> bool {
        self.computers.is_empty()
            && self.replaces.is_empty()
            && self.rows.is_empty()
            && self.activity.is_empty()
    }
}

fn fold(list: &mut Vec<ActivityUpdate>, update: ActivityUpdate) {
    // Updates before an end belong to the ended subscription.
    let start = list
        .iter()
        .rposition(|old| matches!(old, ActivityUpdate::Ended))
        .map_or(0, |index| index + 1);
    match update {
        ActivityUpdate::Ended => {
            list.clear();
            list.push(ActivityUpdate::Ended);
        }
        ActivityUpdate::Replace { .. } => {
            let mut index = start;
            while index < list.len() {
                if matches!(
                    list[index],
                    ActivityUpdate::Events { .. } | ActivityUpdate::Replace { .. }
                ) {
                    list.remove(index);
                } else {
                    index += 1;
                }
            }
            list.push(update);
        }
        ActivityUpdate::Events {
            events: mut more,
            cursor: next,
        } => {
            let window = list[start..].iter_mut().rev().find(|old| {
                matches!(
                    old,
                    ActivityUpdate::Events { .. } | ActivityUpdate::Replace { .. }
                )
            });
            match window {
                Some(ActivityUpdate::Events { events, cursor })
                | Some(ActivityUpdate::Replace { events, cursor }) => {
                    events.append(&mut more);
                    *cursor = next;
                }
                _ => list.push(ActivityUpdate::Events {
                    events: more,
                    cursor: next,
                }),
            }
            bound(list);
        }
        ActivityUpdate::State { .. } => {
            match list[start..]
                .iter_mut()
                .find(|old| matches!(old, ActivityUpdate::State { .. }))
            {
                Some(slot) => *slot = update,
                None => list.push(update),
            }
        }
        ActivityUpdate::Event { ref id, rev, .. } => {
            let slot = list[start..]
                .iter_mut()
                .find(|old| matches!(old, ActivityUpdate::Event { id: other, .. } if other == id));
            match slot {
                Some(slot) => {
                    if matches!(slot, ActivityUpdate::Event { rev: old, .. } if *old < rev) {
                        *slot = update;
                    }
                }
                None => list.push(update),
            }
        }
    }
}

/// Keeps the newest [`PENDING_EVENTS`] events; a window that lost events
/// becomes a replacement, since the view can no longer append it.
fn bound(list: &mut [ActivityUpdate]) {
    for update in list.iter_mut() {
        let (ActivityUpdate::Events { events, cursor }
        | ActivityUpdate::Replace { events, cursor }) = update
        else {
            continue;
        };
        if events.len() <= PENDING_EVENTS {
            continue;
        }
        let excess = events.len() - PENDING_EVENTS;
        events.drain(..excess);
        *update = ActivityUpdate::Replace {
            events: std::mem::take(events),
            cursor: *cursor,
        };
    }
}

fn out(update: ActivityUpdate) -> (&'static str, Value) {
    match update {
        ActivityUpdate::Events { events, cursor } => {
            ("events", json!({"events": events, "cursor": cursor}))
        }
        ActivityUpdate::Replace { events, cursor } => {
            ("replace", json!({"events": events, "cursor": cursor}))
        }
        ActivityUpdate::Event { id, rev, event } => {
            ("event", json!({"id": id, "rev": rev, "event": event}))
        }
        ActivityUpdate::State { value, rev } => ("state", json!({"value": value, "rev": rev})),
        ActivityUpdate::Ended => ("ended", Value::Null),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CatalogState;
    use zerus_watch_protocol::decode;

    fn local() -> ComputerId {
        ComputerId::local()
    }

    fn row(name: &str, updated: f64) -> ViewRow {
        ViewRow {
            name: name.to_owned(),
            updated_at: Some(updated),
            ..ViewRow::default()
        }
    }

    fn upsert(computer: &ComputerId, key: &str, updated: f64) -> Change {
        Change::Upsert {
            computer: computer.clone(),
            key: key.to_owned(),
            row: row(key, updated),
        }
    }

    fn remove(computer: &ComputerId, key: &str, reason: Option<&str>) -> Change {
        Change::Remove {
            computer: computer.clone(),
            key: key.to_owned(),
            reason: reason.map(str::to_owned),
        }
    }

    fn activity(key: &str, update: ActivityUpdate) -> Change {
        Change::Activity {
            computer: local(),
            key: key.to_owned(),
            update,
        }
    }

    fn events(range: std::ops::Range<i64>) -> ActivityUpdate {
        ActivityUpdate::Events {
            events: range.clone().map(|seq| json!({"seq": seq})).collect(),
            cursor: range.end - 1,
        }
    }

    #[test]
    fn a_burst_of_five_thousand_upserts_of_fifty_keys_is_fifty_upserts() {
        let path = format!("{}/tests/fixtures/burst.ndjson", env!("CARGO_MANIFEST_DIR"));
        let mut state = CatalogState::new();
        let mut coalescer = Coalescer::new();
        let text = std::fs::read_to_string(path).unwrap();
        let frames: Vec<_> = text.lines().map(|line| decode(line).unwrap()).collect();
        // hello and the empty snapshot go out in an earlier display frame
        for frame in &frames[..2] {
            for change in state.apply(&local(), frame) {
                coalescer.push(change);
            }
        }
        assert_eq!(coalescer.take().replaces.len(), 1);
        let changes = state.apply(&local(), &frames[2]);
        assert_eq!(changes.len(), 5000);
        for change in changes {
            coalescer.push(change);
        }
        assert_eq!(coalescer.pending(), 50);
        let batch = coalescer.take();
        assert_eq!(batch.upserts.len(), 50);
        assert!(batch.replaces.is_empty() && batch.removes.is_empty());
        for (index, out) in batch.upserts.iter().enumerate() {
            assert_eq!(out.key, format!("run:b{index:02}"));
            assert_eq!(
                out.row.updated_at,
                Some(1787840000.0 + 4950.0 + index as f64)
            );
        }
        assert!(coalescer.take().is_empty());
    }

    #[test]
    fn upserts_of_the_same_key_keep_the_latest_row() {
        let mut coalescer = Coalescer::new();
        for n in 0..5000 {
            coalescer.push(upsert(
                &local(),
                &format!("run:k{:02}", n % 50),
                f64::from(n),
            ));
        }
        assert_eq!(coalescer.pending(), 50);
        let batch = coalescer.take();
        assert_eq!(batch.upserts.len(), 50);
        assert_eq!(batch.upserts[0].key, "run:k00");
        assert_eq!(batch.upserts[0].row.updated_at, Some(4950.0));
        assert!(batch.removes.is_empty() && batch.replaces.is_empty());
    }

    #[test]
    fn a_remove_cancels_a_pending_upsert_and_an_upsert_a_pending_remove() {
        let mut coalescer = Coalescer::new();
        coalescer.push(upsert(&local(), "run:a", 1.0));
        coalescer.push(remove(&local(), "run:a", Some("archived")));
        coalescer.push(remove(&local(), "run:b", None));
        coalescer.push(upsert(&local(), "run:b", 2.0));
        let batch = coalescer.take();
        assert_eq!(
            batch.removes,
            vec![KeyOut {
                computer: local(),
                key: "run:a".into(),
                reason: Some("archived".into())
            }]
        );
        assert_eq!(batch.upserts.len(), 1);
        assert_eq!(batch.upserts[0].key, "run:b");
    }

    #[test]
    fn a_replace_drops_pending_row_changes_of_its_computer_only() {
        let peer = ComputerId::from("studio");
        let mut coalescer = Coalescer::new();
        coalescer.push(upsert(&local(), "run:a", 1.0));
        coalescer.push(remove(&local(), "run:gone", Some("ended")));
        coalescer.push(upsert(&peer, "run:p", 1.0));
        coalescer.push(Change::Replace {
            computer: local(),
            rows: vec![
                ("run:b".into(), row("run:b", 2.0)),
                ("run:c".into(), row("run:c", 2.0)),
            ],
        });
        let batch = coalescer.take();
        assert_eq!(batch.replaces.len(), 1);
        assert_eq!(batch.replaces[0].computer, local());
        let keys: Vec<_> = batch.replaces[0]
            .rows
            .iter()
            .map(|out| out.key.as_str())
            .collect();
        assert_eq!(keys, vec!["run:b", "run:c"]);
        assert!(batch.removes.is_empty());
        assert_eq!(batch.upserts.len(), 1);
        assert_eq!(batch.upserts[0].computer, peer);
    }

    #[test]
    fn row_changes_after_a_replace_fold_into_it() {
        let mut coalescer = Coalescer::new();
        coalescer.push(Change::Replace {
            computer: local(),
            rows: vec![("run:b".into(), row("run:b", 2.0))],
        });
        coalescer.push(upsert(&local(), "run:c", 3.0));
        coalescer.push(upsert(&local(), "run:b", 4.0));
        coalescer.push(remove(&local(), "run:c", Some("ended")));
        coalescer.push(Change::Replace {
            computer: local(),
            rows: vec![("run:d".into(), row("run:d", 5.0))],
        });
        coalescer.push(upsert(&local(), "run:e", 6.0));
        let batch = coalescer.take();
        assert_eq!(batch.replaces.len(), 1);
        let rows: Vec<_> = batch.replaces[0]
            .rows
            .iter()
            .map(|out| (out.key.as_str(), out.row.updated_at))
            .collect();
        assert_eq!(rows, vec![("run:d", Some(5.0)), ("run:e", Some(6.0))]);
        assert!(batch.upserts.is_empty() && batch.removes.is_empty());
    }

    #[test]
    fn an_empty_coalescer_takes_an_empty_batch() {
        let mut coalescer = Coalescer::new();
        assert!(coalescer.is_empty());
        let batch = coalescer.take();
        assert!(batch.is_empty());
        assert_eq!(batch, Batch::default());
    }

    #[test]
    fn the_latest_computer_status_wins() {
        let peer = ComputerId::from("studio");
        let mut coalescer = Coalescer::new();
        for status in [
            ComputerStatus::Probing,
            ComputerStatus::Live,
            ComputerStatus::Reconnecting { attempt: 1 },
            ComputerStatus::Reconnecting { attempt: 2 },
        ] {
            coalescer.push(Change::Computer {
                computer: peer.clone(),
                status,
            });
        }
        coalescer.push(Change::Computer {
            computer: local(),
            status: ComputerStatus::Live,
        });
        let batch = coalescer.take();
        assert_eq!(
            batch.computers,
            vec![
                ComputerOut {
                    computer: local(),
                    status: "live".into(),
                    attempt: 0
                },
                ComputerOut {
                    computer: peer,
                    status: "reconnecting".into(),
                    attempt: 2
                },
            ]
        );
    }

    fn kinds(batch: &Batch) -> Vec<(&str, &str)> {
        batch
            .activity
            .iter()
            .map(|out| (out.key.as_str(), out.kind.as_str()))
            .collect()
    }

    #[test]
    fn activity_keeps_its_order_and_merges_events() {
        let mut coalescer = Coalescer::new();
        coalescer.push(activity("run:a", events(1..3)));
        coalescer.push(activity("run:b", events(1..2)));
        coalescer.push(activity(
            "run:a",
            ActivityUpdate::State {
                value: json!({"phase": "tool"}),
                rev: 1,
            },
        ));
        coalescer.push(activity("run:a", events(3..5)));
        coalescer.push(activity(
            "run:a",
            ActivityUpdate::State {
                value: json!({"phase": "idle"}),
                rev: 2,
            },
        ));
        let batch = coalescer.take();
        assert_eq!(
            kinds(&batch),
            vec![("run:a", "events"), ("run:a", "state"), ("run:b", "events")]
        );
        assert_eq!(batch.activity[0].payload["cursor"], 4);
        let seqs: Vec<_> = batch.activity[0].payload["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|event| event["seq"].as_i64().unwrap())
            .collect();
        assert_eq!(seqs, vec![1, 2, 3, 4]);
        assert_eq!(
            batch.activity[1].payload,
            json!({"value": {"phase": "idle"}, "rev": 2})
        );
    }

    #[test]
    fn a_window_replacement_absorbs_earlier_and_later_events() {
        let mut coalescer = Coalescer::new();
        coalescer.push(activity("run:a", events(1..3)));
        coalescer.push(activity(
            "run:a",
            ActivityUpdate::Replace {
                events: vec![json!({"seq": 10})],
                cursor: 10,
            },
        ));
        coalescer.push(activity("run:a", events(11..12)));
        let batch = coalescer.take();
        assert_eq!(kinds(&batch), vec![("run:a", "replace")]);
        assert_eq!(
            batch.activity[0].payload,
            json!({"events": [{"seq": 10}, {"seq": 11}], "cursor": 11})
        );
    }

    #[test]
    fn an_ended_subscription_drops_what_it_had_pending() {
        let mut coalescer = Coalescer::new();
        coalescer.push(activity("run:a", events(1..3)));
        coalescer.push(activity("run:a", ActivityUpdate::Ended));
        coalescer.push(activity("run:a", events(1..2)));
        let batch = coalescer.take();
        assert_eq!(kinds(&batch), vec![("run:a", "ended"), ("run:a", "events")]);
        assert_eq!(batch.activity[0].payload, Value::Null);
        assert_eq!(
            batch.activity[1].payload["events"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn event_replacements_keep_the_highest_rev_per_id() {
        let mut coalescer = Coalescer::new();
        for rev in [1, 3, 2] {
            coalescer.push(activity(
                "run:a",
                ActivityUpdate::Event {
                    id: "m-1".into(),
                    rev,
                    event: json!({"rev": rev}),
                },
            ));
        }
        let batch = coalescer.take();
        assert_eq!(kinds(&batch), vec![("run:a", "event")]);
        assert_eq!(
            batch.activity[0].payload,
            json!({"id": "m-1", "rev": 3, "event": {"rev": 3}})
        );
    }

    #[test]
    fn pending_memory_stays_bounded() {
        let mut coalescer = Coalescer::new();
        for n in 0..100_000 {
            coalescer.push(upsert(&local(), &format!("run:k{}", n % 50), f64::from(n)));
            coalescer.push(Change::Computer {
                computer: local(),
                status: ComputerStatus::Live,
            });
        }
        assert_eq!(coalescer.pending(), 51);
        for seq in 0..5000 {
            coalescer.push(activity("run:a", events(seq..seq + 1)));
        }
        assert!(coalescer.pending() <= 51 + PENDING_EVENTS);
        let batch = coalescer.take();
        let update = &batch.activity[0];
        // Too many pending events become a window of the newest ones.
        assert_eq!(update.kind, "replace");
        let kept = update.payload["events"].as_array().unwrap();
        assert_eq!(kept.len(), PENDING_EVENTS);
        assert_eq!(kept[0]["seq"], 5000 - PENDING_EVENTS as i64);
        assert_eq!(update.payload["cursor"], 4999);
        assert!(coalescer.is_empty());
    }

    #[test]
    fn batches_serialize_as_snake_case_json() {
        let mut coalescer = Coalescer::new();
        coalescer.push(upsert(&ComputerId::from("studio"), "run:a", 2.5));
        coalescer.push(remove(&local(), "run:b", Some("replaced")));
        coalescer.push(Change::Replace {
            computer: ComputerId::from("build-01"),
            rows: vec![("run:c".into(), row("run:c", 1.0))],
        });
        coalescer.push(Change::Computer {
            computer: local(),
            status: ComputerStatus::Reconnecting { attempt: 3 },
        });
        coalescer.push(activity("run:a", ActivityUpdate::Ended));
        let value = serde_json::to_value(coalescer.take()).unwrap();
        assert_eq!(value["upserts"][0]["computer"], "studio");
        assert_eq!(value["upserts"][0]["key"], "run:a");
        assert_eq!(value["upserts"][0]["row"]["updated_at"], 2.5);
        assert_eq!(
            value["removes"][0],
            json!({"computer": "", "key": "run:b", "reason": "replaced"})
        );
        assert_eq!(value["replaces"][0]["computer"], "build-01");
        assert_eq!(value["replaces"][0]["rows"][0]["key"], "run:c");
        assert_eq!(
            value["computers"][0],
            json!({"computer": "", "status": "reconnecting", "attempt": 3})
        );
        assert_eq!(
            value["activity"][0],
            json!({"computer": "", "key": "run:a", "kind": "ended", "payload": null})
        );
        let back: Batch = serde_json::from_value(value).unwrap();
        assert_eq!(back.computers[0].attempt, 3);
    }
}
