//! One computer's catalog and followed Activity, kept from its watch frames.
//!
//! [`CatalogState::apply`] turns each frame into keyed [`Change`]s: catalog
//! snapshot pages are staged until the page with `last` and then replace the
//! rows at once, so a `gap` never empties the list; a delta gives one upsert or
//! removal per item; Activity frames of followed sessions become
//! [`Change::Activity`]. Frame types and topics it does not know change nothing.
use crate::{view, ActivityUpdate, Change, ComputerId, ComputerStatus, ViewRow};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};
use zerus_watch_protocol::{reason, topic, Body, Frame, Hello};

/// Failures further apart than this start the restart delays over.
pub const FAILURE_WINDOW: Duration = Duration::from_secs(600);
/// Provider messages a merged summary keeps, newest by `at`.
pub const PROVIDER_MESSAGES: usize = 100;
/// Two provider messages of one type and text this close are one repeat.
pub const REPEAT_SECONDS: f64 = 3.0;
/// Event revisions remembered per followed session.
const EVENT_REVISIONS: usize = 1024;

/// The removal reasons a [`Change::Remove`] keeps; others become None.
const REASONS: [&str; 4] = [
    reason::REPLACED,
    reason::ARCHIVED,
    reason::FORGOTTEN,
    reason::ENDED,
];

/// A followed session: its event window position and latest summary.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ActivityState {
    /// The journal `seq` to resume after.
    pub cursor: i64,
    /// The latest `state` value with `event` frames merged into its
    /// `provider_messages`.
    pub state: Option<Value>,
    /// The `rev` of the latest `state` frame.
    pub rev: u64,
    staged: Option<Vec<Value>>,
    revisions: HashMap<String, u64>,
}

impl ActivityState {
    /// A `state` frame: it replaces the summary, provider messages included.
    pub fn replace_state(&mut self, value: Value, rev: u64) {
        self.state = Some(value);
        self.rev = rev;
    }

    /// Merges an activity `event` frame (a provider message, `AgentMessage` or
    /// `AgentThinking`) into the summary's `provider_messages`; true when the
    /// list changed. A frame whose `rev` is not higher than the last one of
    /// its `id` is stale. The message replaces the one with the same non-empty
    /// `message_id` and type; otherwise one of the same type and `detail` with
    /// `at` within 3 s makes it a repeat; otherwise it is inserted in `at`
    /// order and the newest 100 stay. Without a summary yet nothing merges:
    /// the first `state` carries the message.
    pub fn merge_event(&mut self, id: &str, rev: u64, event: Value) -> bool {
        let Some(Value::Object(state)) = self.state.as_mut() else {
            return false;
        };
        if self.revisions.get(id).is_some_and(|seen| *seen >= rev) {
            return false;
        }
        if self.revisions.len() >= EVENT_REVISIONS {
            self.revisions.clear();
        }
        self.revisions.insert(id.to_owned(), rev);
        merge_provider_message(state, event)
    }
}

fn field<'a>(value: &'a Value, name: &str) -> &'a str {
    value.get(name).and_then(Value::as_str).unwrap_or("")
}

fn at(value: &Value) -> f64 {
    value.get("at").and_then(Value::as_f64).unwrap_or(f64::MAX)
}

fn merge_provider_message(state: &mut Map<String, Value>, message: Value) -> bool {
    let entry = state
        .entry("provider_messages")
        .or_insert_with(|| Value::Array(Vec::new()));
    if !entry.is_array() {
        *entry = Value::Array(Vec::new());
    }
    let Value::Array(list) = entry else {
        return false;
    };
    let id = field(&message, "message_id");
    let kind = field(&message, "type");
    if !id.is_empty() {
        let same = list
            .iter()
            .position(|old| field(old, "message_id") == id && field(old, "type") == kind);
        if let Some(index) = same {
            list[index] = message;
            return true;
        }
    }
    let detail = message.get("detail");
    let when = at(&message);
    let repeat = list.iter().any(|old| {
        field(old, "type") == kind
            && old.get("detail") == detail
            && (at(old) - when).abs() <= REPEAT_SECONDS
    });
    if repeat {
        return false;
    }
    let index = list
        .iter()
        .position(|old| at(old) > when)
        .unwrap_or(list.len());
    list.insert(index, message);
    while list.len() > PROVIDER_MESSAGES {
        let oldest = (0..list.len())
            .min_by(|a, b| at(&list[*a]).total_cmp(&at(&list[*b])))
            .unwrap_or(0);
        list.remove(oldest);
    }
    true
}

/// The state of one computer's stream, kept across its restarts.
#[derive(Debug, Default)]
pub struct CatalogState {
    hello: Option<Hello>,
    rows: BTreeMap<String, ViewRow>,
    staged: Option<BTreeMap<String, ViewRow>>,
    meta: Option<Value>,
    activity: HashMap<String, ActivityState>,
    live: bool,
    failed: bool,
    failures: u32,
    last_failure: Option<Instant>,
}

impl CatalogState {
    pub fn new() -> Self {
        Self::default()
    }

    /// The changes one frame makes. A fatal `error` ends the stream:
    /// [`Self::failed`] is then true and the caller restarts it.
    pub fn apply(&mut self, computer: &ComputerId, frame: &Frame) -> Vec<Change> {
        let mut changes = Vec::new();
        match &frame.body {
            Body::Hello(hello) => {
                self.hello = Some(hello.clone());
                self.begin();
            }
            Body::Snapshot(snapshot) if snapshot.topic == topic::CATALOG => {
                let machine = self.machine(computer);
                if snapshot.page == 0 {
                    self.staged = Some(BTreeMap::new());
                }
                let staged = self.staged.get_or_insert_with(BTreeMap::new);
                for item in snapshot.items.iter().flatten() {
                    let (Some(key), Some(row)) =
                        (item.get("key").and_then(Value::as_str), item.get("row"))
                    else {
                        continue;
                    };
                    staged.insert(key.to_owned(), project(row, &machine));
                }
                if snapshot.last {
                    self.rows = self.staged.take().unwrap_or_default();
                    changes.push(Change::Replace {
                        computer: computer.clone(),
                        rows: self
                            .rows
                            .iter()
                            .map(|(key, row)| (key.clone(), row.clone()))
                            .collect(),
                    });
                    if !self.live {
                        self.live = true;
                        changes.push(Change::Computer {
                            computer: computer.clone(),
                            status: ComputerStatus::Live,
                        });
                    }
                }
            }
            Body::Snapshot(snapshot) if snapshot.topic == topic::CATALOG_META => {
                if snapshot.last {
                    self.meta = snapshot.value.clone();
                }
            }
            Body::Snapshot(snapshot) if snapshot.topic == topic::ACTIVITY => {
                let key = snapshot.key.as_deref().unwrap_or("");
                if let Some(activity) = self.activity.get_mut(key) {
                    if snapshot.page == 0 {
                        activity.staged = Some(Vec::new());
                    }
                    let staged = activity.staged.get_or_insert_with(Vec::new);
                    staged.extend(snapshot.items.iter().flatten().cloned());
                    if snapshot.last {
                        activity.cursor = snapshot.cursor;
                        let events = activity.staged.take().unwrap_or_default();
                        changes.push(activity_change(
                            computer,
                            key,
                            ActivityUpdate::Replace {
                                events,
                                cursor: snapshot.cursor,
                            },
                        ));
                    }
                }
            }
            Body::Delta(delta) if delta.topic == topic::CATALOG => {
                let machine = self.machine(computer);
                for item in &delta.upsert {
                    let row = project(&item.row, &machine);
                    if let Some(staged) = self.staged.as_mut() {
                        staged.insert(item.key.clone(), row.clone());
                    }
                    self.rows.insert(item.key.clone(), row.clone());
                    changes.push(Change::Upsert {
                        computer: computer.clone(),
                        key: item.key.clone(),
                        row,
                    });
                }
                for removal in &delta.remove {
                    if let Some(staged) = self.staged.as_mut() {
                        staged.remove(&removal.key);
                    }
                    self.rows.remove(&removal.key);
                    let reason = removal
                        .reason
                        .as_deref()
                        .filter(|reason| REASONS.contains(reason))
                        .map(str::to_owned);
                    changes.push(Change::Remove {
                        computer: computer.clone(),
                        key: removal.key.clone(),
                        reason,
                    });
                }
            }
            Body::Events(events) if events.topic == topic::ACTIVITY => {
                if let Some(activity) = self.activity.get_mut(&events.key) {
                    activity.cursor = events.cursor;
                    changes.push(activity_change(
                        computer,
                        &events.key,
                        ActivityUpdate::Events {
                            events: events.events.clone(),
                            cursor: events.cursor,
                        },
                    ));
                }
            }
            Body::Event(event) if event.topic == topic::ACTIVITY => {
                if let Some(activity) = self.activity.get_mut(&event.key) {
                    if activity.merge_event(&event.id, event.rev, event.event.clone()) {
                        let value = activity.state.clone().unwrap_or(Value::Null);
                        let rev = activity.rev;
                        changes.push(activity_change(
                            computer,
                            &event.key,
                            ActivityUpdate::State { value, rev },
                        ));
                    }
                }
            }
            Body::State(state) if state.topic == topic::ACTIVITY => {
                if let Some(activity) = self.activity.get_mut(&state.key) {
                    activity.replace_state(state.value.clone(), state.rev);
                    changes.push(activity_change(
                        computer,
                        &state.key,
                        ActivityUpdate::State {
                            value: state.value.clone(),
                            rev: state.rev,
                        },
                    ));
                }
            }
            Body::Gap(gap) if gap.topic == topic::CATALOG => {
                // The rows stay until the snapshot that follows is complete.
                self.staged = None;
            }
            Body::Gap(gap) if gap.topic == topic::ACTIVITY => {
                let key = gap.key.as_deref().unwrap_or("");
                if let Some(activity) = self.activity.get_mut(key) {
                    activity.staged = None;
                }
            }
            Body::Error(error) if error.fatal => {
                return self.end(computer, Instant::now());
            }
            Body::Error(error) => {
                let ended = matches!(
                    error.code.as_str(),
                    "subscription_ended" | "session_changed" | "too_many_subscriptions"
                );
                let key = error.key.as_deref().unwrap_or("");
                if ended
                    && error.topic.as_deref() == Some(topic::ACTIVITY)
                    && self.activity.remove(key).is_some()
                {
                    changes.push(activity_change(computer, key, ActivityUpdate::Ended));
                }
            }
            _ => {}
        }
        changes
    }

    /// The rows as last known, by key.
    pub fn rows(&self) -> &BTreeMap<String, ViewRow> {
        &self.rows
    }

    /// The latest `hello` of this computer.
    pub fn hello(&self) -> Option<&Hello> {
        self.hello.as_ref()
    }

    /// A new stream attempt starts (a probe or a spawn).
    pub fn begin(&mut self) {
        self.failed = false;
        self.live = false;
        self.staged = None;
        for activity in self.activity.values_mut() {
            activity.staged = None;
        }
    }

    /// The stream ended, failed or stayed silent: the computer reconnects and
    /// its rows stay as last known. A second end of the same attempt (its exit
    /// after a fatal error) changes nothing.
    pub fn end(&mut self, computer: &ComputerId, now: Instant) -> Vec<Change> {
        if self.failed {
            return Vec::new();
        }
        self.begin();
        self.failed = true;
        self.failures = match self.last_failure {
            Some(last) if now.saturating_duration_since(last) <= FAILURE_WINDOW => {
                self.failures.saturating_add(1)
            }
            _ => 1,
        };
        self.last_failure = Some(now);
        vec![Change::Computer {
            computer: computer.clone(),
            status: ComputerStatus::Reconnecting {
                attempt: self.failures,
            },
        }]
    }

    /// Whether this attempt has ended.
    pub fn failed(&self) -> bool {
        self.failed
    }

    /// Consecutive failures (failures more than 10 minutes apart start over).
    pub fn attempt(&self) -> u32 {
        self.failures
    }

    /// Whether the catalog snapshot of this attempt has arrived.
    pub fn live(&self) -> bool {
        self.live
    }

    /// The latest `catalog.meta` value: the listing without its sessions.
    pub fn meta(&self) -> Option<&Value> {
        self.meta.as_ref()
    }

    /// The SSH aliases of the peers this computer lists, None when its
    /// `catalog.meta` has no `peers` array (an older host): keep the old list.
    pub fn peers(&self) -> Option<Vec<String>> {
        let peers = self.meta.as_ref()?.get("peers")?.as_array()?;
        Some(
            peers
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
        )
    }

    /// Accepts Activity frames of this key from now on; a key followed before
    /// keeps its cursor and summary.
    pub fn follow(&mut self, key: &str) {
        self.activity.entry(key.to_owned()).or_default();
    }

    pub fn unfollow(&mut self, key: &str) {
        self.activity.remove(key);
    }

    pub fn is_followed(&self, key: &str) -> bool {
        self.activity.contains_key(key)
    }

    /// The journal `seq` to resume a followed key after.
    pub fn cursor(&self, key: &str) -> Option<i64> {
        self.activity.get(key).map(|activity| activity.cursor)
    }

    pub fn activity(&self, key: &str) -> Option<&ActivityState> {
        self.activity.get(key)
    }

    /// The name rows carry: a peer's alias, or this computer's host name.
    fn machine(&self, computer: &ComputerId) -> String {
        match computer.alias() {
            Some(alias) => alias.to_owned(),
            None => self
                .hello
                .as_ref()
                .map(|hello| hello.host.clone())
                .unwrap_or_default(),
        }
    }
}

fn project(row: &Value, machine: &str) -> ViewRow {
    let mut view = view::row(row);
    view.machine = machine.to_owned();
    view
}

fn activity_change(computer: &ComputerId, key: &str, update: ActivityUpdate) -> Change {
    Change::Activity {
        computer: computer.clone(),
        key: key.to_owned(),
        update,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use zerus_watch_protocol::decode;

    fn fixture(name: &str) -> Vec<Frame> {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(path)
            .unwrap()
            .lines()
            .filter_map(|line| decode(line).ok())
            .collect()
    }

    fn frame(value: Value) -> Frame {
        serde_json::from_value(value).unwrap()
    }

    fn apply_all(state: &mut CatalogState, computer: &ComputerId, frames: &[Frame]) -> Vec<Change> {
        frames
            .iter()
            .flat_map(|frame| state.apply(computer, frame))
            .collect()
    }

    fn keys(rows: &[(String, ViewRow)]) -> Vec<&str> {
        rows.iter().map(|(key, _)| key.as_str()).collect()
    }

    #[test]
    fn a_three_page_snapshot_gives_one_replace_after_the_last_page() {
        let frames = fixture("basic.ndjson");
        let local = ComputerId::local();
        let mut state = CatalogState::new();
        assert!(state.apply(&local, &frames[0]).is_empty(), "hello");
        assert!(state.apply(&local, &frames[1]).is_empty(), "page 0");
        assert!(state.apply(&local, &frames[2]).is_empty(), "page 1");
        let changes = state.apply(&local, &frames[3]);
        let replaces: Vec<_> = changes
            .iter()
            .filter_map(|change| match change {
                Change::Replace { computer, rows } => Some((computer, rows)),
                _ => None,
            })
            .collect();
        assert_eq!(replaces.len(), 1);
        let (computer, rows) = replaces[0];
        assert_eq!(computer, &local);
        assert_eq!(
            keys(rows),
            vec![
                "name:sh/sample-project",
                "run:run-draft-1",
                "run:run-lint-1",
                "run:run-outline-1",
                "run:run-review-1",
                "run:run-tests-1"
            ]
        );
        // The local computer's rows carry its hello host.
        assert!(rows.iter().all(|(_, row)| row.machine == "workstation"));
        assert!(changes.contains(&Change::Computer {
            computer: local.clone(),
            status: ComputerStatus::Live
        }));
        assert_eq!(state.rows().len(), 6);
    }

    #[test]
    fn a_gap_keeps_the_rows_until_the_new_snapshot_is_complete() {
        let frames = fixture("gap.ndjson");
        let local = ComputerId::local();
        let mut state = CatalogState::new();
        apply_all(&mut state, &local, &frames[..2]);
        assert_eq!(state.rows().len(), 2);
        // gap, then the first page of the replacement and a heartbeat
        for frame in &frames[2..5] {
            assert!(state.apply(&local, frame).is_empty(), "{frame:?}");
            assert_eq!(
                state.rows().keys().collect::<Vec<_>>(),
                vec!["run:run-a", "run:run-b"]
            );
        }
        let changes = state.apply(&local, &frames[5]);
        assert_eq!(changes.len(), 1, "{changes:?}");
        let Change::Replace { rows, .. } = &changes[0] else {
            panic!("{changes:?}")
        };
        assert_eq!(
            keys(rows),
            vec!["name:sh/sample-project", "run:run-b", "run:run-c"]
        );
        assert_eq!(rows[1].1.status, "working");
    }

    #[test]
    fn removals_keep_known_reasons_and_report_unknown_ones_as_none() {
        let frames = fixture("basic.ndjson");
        let local = ComputerId::local();
        let mut state = CatalogState::new();
        apply_all(&mut state, &local, &frames[..6]);
        let changes = state.apply(&local, &frames[6]);
        let removes: Vec<_> = changes
            .iter()
            .filter_map(|change| match change {
                Change::Remove { key, reason, .. } => Some((key.as_str(), reason.as_deref())),
                _ => None,
            })
            .collect();
        assert_eq!(
            removes,
            vec![
                ("run:run-tests-1", Some("replaced")),
                ("run:run-outline-1", Some("archived")),
                ("run:run-draft-1", Some("forgotten")),
                ("name:sh/sample-project", Some("ended")),
                ("run:run-lint-1", None),
            ]
        );
        let upserts: Vec<_> = changes
            .iter()
            .filter_map(|change| match change {
                Change::Upsert { key, row, .. } => Some((key.as_str(), row.status.as_str())),
                _ => None,
            })
            .collect();
        assert_eq!(
            upserts,
            vec![
                ("run:run-review-1", "ready"),
                ("run:run-tests-2", "starting")
            ]
        );
        assert_eq!(
            state.rows().keys().collect::<Vec<_>>(),
            vec!["run:run-review-1", "run:run-tests-2"]
        );
        // `replaced`: the new run lists under the same name.
        assert_eq!(
            state.rows()["run:run-tests-2"].name,
            "codex/sample-project/tests"
        );
    }

    #[test]
    fn a_replaced_run_removes_the_old_key_and_upserts_the_new_one() {
        let frames = fixture("replaced.ndjson");
        let local = ComputerId::local();
        let mut state = CatalogState::new();
        apply_all(&mut state, &local, &frames[..2]);
        let changes = state.apply(&local, &frames[2]);
        assert_eq!(changes.len(), 2);
        assert!(matches!(&changes[0], Change::Upsert { key, row, .. }
            if key == "run:run-new" && row.name == "claude/sample-project/review"));
        assert!(matches!(&changes[1], Change::Remove { key, reason, .. }
            if key == "run:run-old" && reason.as_deref() == Some("replaced")));
    }

    #[test]
    fn a_burst_frame_gives_one_upsert_per_item() {
        let frames = fixture("burst.ndjson");
        let local = ComputerId::local();
        let mut state = CatalogState::new();
        apply_all(&mut state, &local, &frames[..2]);
        let changes = state.apply(&local, &frames[2]);
        assert_eq!(changes.len(), 5000);
        assert!(changes
            .iter()
            .all(|change| matches!(change, Change::Upsert { .. })));
        assert_eq!(state.rows().len(), 50);
        assert_eq!(state.rows()["run:b49"].updated_at, Some(1787844999.0));
    }

    #[test]
    fn a_fatal_error_reconnects_and_keeps_the_rows() {
        let frames = fixture("fatal.ndjson");
        let local = ComputerId::local();
        let mut state = CatalogState::new();
        apply_all(&mut state, &local, &frames[..2]);
        let changes = state.apply(&local, &frames[2]);
        assert_eq!(
            changes,
            vec![Change::Computer {
                computer: local.clone(),
                status: ComputerStatus::Reconnecting { attempt: 1 }
            }]
        );
        assert!(state.failed());
        assert_eq!(state.attempt(), 1);
        assert_eq!(state.rows().len(), 1, "rows stay as last known");
        // The stream's exit after its fatal error is the same failure.
        assert!(state.end(&local, Instant::now()).is_empty());
        assert_eq!(state.attempt(), 1);
    }

    #[test]
    fn failures_within_ten_minutes_count_up_and_later_ones_start_over() {
        let local = ComputerId::local();
        let mut state = CatalogState::new();
        let start = Instant::now();
        let hello = fixture("basic.ndjson").remove(0);
        let mut attempts = Vec::new();
        for offset in [0, 1, 2, 3, 4] {
            state.apply(&local, &hello);
            let changes = state.end(&local, start + Duration::from_secs(offset));
            attempts.push(changes[0].clone());
        }
        let expected: Vec<_> = (1..=5)
            .map(|attempt| Change::Computer {
                computer: local.clone(),
                status: ComputerStatus::Reconnecting { attempt },
            })
            .collect();
        assert_eq!(attempts, expected);
        state.apply(&local, &hello);
        state.end(
            &local,
            start + Duration::from_secs(4) + FAILURE_WINDOW + Duration::from_secs(1),
        );
        assert_eq!(state.attempt(), 1);
    }

    #[test]
    fn unknown_topics_and_non_fatal_errors_change_nothing() {
        let frames = fixture("basic.ndjson");
        let local = ComputerId::local();
        let mut state = CatalogState::new();
        apply_all(&mut state, &local, &frames[..4]);
        for value in [
            json!({"v":1,"seq":30,"t":"state","topic":"git","key":"/home/example/src/x","value":{},"rev":1}),
            json!({"v":1,"seq":31,"t":"snapshot","topic":"tasks","items":[],"page":0,"last":true,"cursor":1}),
            json!({"v":1,"seq":32,"t":"error","code":"unsupported_topic","message":"no","topic":"processes","fatal":false}),
            json!({"v":1,"seq":33,"t":"error","code":"too_many_subscriptions","message":"no","topic":"activity","fatal":false}),
            json!({"v":1,"seq":34,"t":"pong","id":3}),
            json!({"v":1,"seq":35,"t":"heartbeat","at":1.0}),
            json!({"v":1,"seq":36,"t":"delta","topic":"later","upsert":[],"remove":[],"cursor":1}),
        ] {
            assert!(state.apply(&local, &frame(value)).is_empty());
        }
        assert!(!state.failed());
    }

    #[test]
    fn catalog_meta_lists_the_peers_only_when_it_has_an_array() {
        let frames = fixture("basic.ndjson");
        let local = ComputerId::local();
        let mut state = CatalogState::new();
        assert_eq!(state.peers(), None);
        apply_all(&mut state, &local, &frames[..5]);
        assert_eq!(
            state.peers(),
            Some(vec!["studio".to_owned(), "build-01".to_owned()])
        );
        let older = frame(json!({"v":1,"seq":40,"t":"snapshot","topic":"catalog.meta",
            "value":{"host":"workstation"},"page":0,"last":true,"cursor":9}));
        state.apply(&local, &older);
        assert_eq!(state.peers(), None);
        assert_eq!(state.meta().unwrap()["host"], "workstation");
    }

    #[test]
    fn peer_rows_carry_the_alias_never_the_reported_host() {
        let frames = fixture("basic.ndjson");
        let peer = ComputerId::from("studio");
        let mut state = CatalogState::new();
        let changes = apply_all(&mut state, &peer, &frames[..4]);
        let Some(Change::Replace { computer, rows }) = changes.first() else {
            panic!("{changes:?}")
        };
        assert_eq!(computer, &peer);
        assert!(rows.iter().all(|(_, row)| row.machine == "studio"));
    }

    fn followed(state: &mut CatalogState, frames: &[Frame]) -> Vec<ActivityUpdate> {
        let local = ComputerId::local();
        frames
            .iter()
            .flat_map(|frame| state.apply(&local, frame))
            .filter_map(|change| match change {
                Change::Activity { key, update, .. } => {
                    assert_eq!(key, "run:run-review-1");
                    Some(update)
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn activity_frames_of_followed_sessions_become_activity_changes() {
        let frames = fixture("basic.ndjson");
        let mut ignored = CatalogState::new();
        assert!(followed(&mut ignored, &frames).is_empty(), "not followed");

        let mut state = CatalogState::new();
        state.follow("run:run-review-1");
        let updates = followed(&mut state, &frames);
        assert_eq!(updates.len(), 6, "{updates:#?}");
        let ActivityUpdate::Events { events, cursor } = &updates[0] else {
            panic!("{:?}", updates[0])
        };
        assert_eq!((events.len(), *cursor), (2, 41));
        let ActivityUpdate::State { value, rev } = &updates[1] else {
            panic!("{:?}", updates[1])
        };
        assert_eq!(*rev, 3);
        assert_eq!(value["provider_messages"].as_array().unwrap().len(), 1);
        // Both `event` frames merge into the summary's provider messages.
        for (update, detail) in updates[2..4]
            .iter()
            .zip(["Partial answer", "Complete answer"])
        {
            let ActivityUpdate::State { value, rev } = update else {
                panic!("{update:?}")
            };
            assert_eq!(*rev, 3);
            let messages = value["provider_messages"].as_array().unwrap();
            assert_eq!(messages.len(), 2);
            assert_eq!(messages[1]["detail"], detail);
        }
        let ActivityUpdate::Replace { events, cursor } = &updates[4] else {
            panic!("{:?}", updates[4])
        };
        assert_eq!((events.len(), *cursor), (1, 45));
        assert_eq!(updates[5], ActivityUpdate::Ended);
        assert!(
            !state.is_followed("run:run-review-1"),
            "the subscription ended"
        );
    }

    #[test]
    fn a_reconnect_resumes_activity_after_the_last_cursor() {
        let frames = fixture("basic.ndjson");
        let local = ComputerId::local();
        let mut state = CatalogState::new();
        state.follow("run:run-review-1");
        apply_all(&mut state, &local, &frames[..9]);
        assert_eq!(state.cursor("run:run-review-1"), Some(41));
        state.end(&local, Instant::now());
        state.apply(&local, &frames[0]);
        assert_eq!(state.cursor("run:run-review-1"), Some(41));
        state.unfollow("run:run-review-1");
        assert_eq!(state.cursor("run:run-review-1"), None);
    }

    fn message(kind: &str, id: &str, detail: &str, at: f64) -> Value {
        json!({"type": kind, "message_id": id, "detail": detail, "at": at, "source": "hooks", "agent_id": ""})
    }

    fn details(state: &ActivityState) -> Vec<String> {
        state.state.as_ref().unwrap()["provider_messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|message| message["detail"].as_str().unwrap().to_owned())
            .collect()
    }

    #[test]
    fn provider_messages_merge_by_id_and_rev() {
        let mut activity = ActivityState::default();
        assert!(!activity.merge_event("m-1", 1, message("AgentMessage", "m-1", "early", 10.0)));
        activity.replace_state(
            json!({"provider_messages": [message("AgentThinking", "t-1", "thinking", 5.0)]}),
            7,
        );
        assert!(activity.merge_event("m-1", 1, message("AgentMessage", "m-1", "draft", 10.0)));
        assert!(activity.merge_event("m-1", 2, message("AgentMessage", "m-1", "final", 11.0)));
        assert_eq!(details(&activity), vec!["thinking", "final"]);
        // A lower or equal rev of the same id changes nothing.
        assert!(!activity.merge_event("m-1", 2, message("AgentMessage", "m-1", "stale", 12.0)));
        assert!(!activity.merge_event("m-1", 1, message("AgentMessage", "m-1", "older", 12.0)));
        assert_eq!(details(&activity), vec!["thinking", "final"]);
        // The same message_id with another type is another message.
        assert!(activity.merge_event(
            "m-1t",
            1,
            message("AgentThinking", "m-1", "more thinking", 12.0)
        ));
        assert_eq!(
            details(&activity),
            vec!["thinking", "final", "more thinking"]
        );
        assert_eq!(activity.rev, 7);
        // The next state replaces the list wholesale.
        activity.replace_state(json!({"provider_messages": []}), 8);
        assert!(details(&activity).is_empty());
    }

    #[test]
    fn provider_messages_drop_repeats_within_three_seconds() {
        let mut activity = ActivityState::default();
        activity.replace_state(
            json!({"provider_messages": [message("AgentMessage", "", "Done.", 100.0)]}),
            1,
        );
        assert!(!activity.merge_event("a", 1, message("AgentMessage", "", "Done.", 102.5)));
        assert!(!activity.merge_event("b", 1, message("AgentMessage", "other-id", "Done.", 97.0)));
        assert!(
            activity.merge_event("c", 1, message("AgentThinking", "", "Done.", 101.0)),
            "another type"
        );
        assert!(
            activity.merge_event("d", 1, message("AgentMessage", "", "Done.", 103.5)),
            "3.5 s later"
        );
        assert_eq!(details(&activity).len(), 3);
    }

    #[test]
    fn provider_messages_stay_in_time_order_and_keep_the_newest_hundred() {
        let mut activity = ActivityState::default();
        activity.replace_state(json!({"name": "claude/sample-project/review"}), 1);
        for at in [30.0, 10.0, 20.0] {
            activity.merge_event(
                &format!("m{at}"),
                1,
                message("AgentMessage", &format!("m{at}"), &format!("{at}"), at),
            );
        }
        assert_eq!(details(&activity), vec!["10", "20", "30"]);
        for n in 0..150 {
            let at = 1000.0 + f64::from(n) * 10.0;
            activity.merge_event(
                &format!("n{n}"),
                1,
                message("AgentMessage", &format!("n{n}"), &format!("n{n}"), at),
            );
        }
        let kept = details(&activity);
        assert_eq!(kept.len(), 100);
        assert_eq!(kept.first().map(String::as_str), Some("n50"));
        assert_eq!(kept.last().map(String::as_str), Some("n149"));
        assert_eq!(
            activity.state.as_ref().unwrap()["name"],
            "claude/sample-project/review"
        );
    }
}
