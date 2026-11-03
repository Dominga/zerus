//! Bounded discovery over service-fleet observations. Neither a cached row nor
//! a display-only IPC feed can grant native approval permission.
use super::super::{feed, fleet, store::Store};
use anyhow::{ensure, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};
use zerus_client_core::{approvals::automatic as policy, view};
use zerus_desktop_catalog::{self as catalog, Cursor};
use zerus_desktop_ledger::reads;

const FRESH: Duration = Duration::from_secs(15);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Stamp {
    pub computer: String,
    pub incarnation: i64,
    pub generation: i64,
    pub visible: Option<i64>,
}
#[derive(Clone)]
pub(super) struct Candidate {
    pub target: policy::Candidate,
    pub source: Stamp,
    pub key: String,
}

/// One row of an indexed pass and, when the policy accepts it, its candidate.
/// The pass moves past it only once [`Sources::handled`] says so.
pub(super) struct Indexed {
    computer: String,
    generation: i64,
    id: i64,
    /// The last row of its source's pass.
    last: bool,
    pub candidate: Option<Candidate>,
}

/// One source's pass over its flagged rows: the generation it walks, the last
/// row it handled and the end it captured when it started.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Pass {
    generation: i64,
    after: i64,
    end: Option<i64>,
}

fn bounded_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<String> {
    let value = row.get_ref(0)?;
    let text = value.as_str().map_err(|_| rusqlite::Error::InvalidQuery)?;
    if text.is_empty() || text.len() > catalog::PAGE_BYTES {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(text.to_owned())
}

pub(super) struct Sources {
    epoch: String,
    native: BTreeMap<String, (Stamp, Instant)>,
    cursor: Option<Cursor>,
    computer: usize,
    read_after: i64,
    read_through: i64,
    /// Per native source: its pass over the approval-phase index. At most
    /// one entry per native source.
    approval_after: BTreeMap<String, Pass>,
    approval_next: usize,
}
impl Sources {
    pub fn new(epoch: &str) -> Self {
        Self {
            epoch: epoch.into(),
            native: BTreeMap::new(),
            cursor: None,
            computer: 0,
            read_after: 0,
            read_through: 0,
            approval_after: BTreeMap::new(),
            approval_next: 0,
        }
    }

    fn stamp(&self, db: &Connection, computer: &str) -> Result<Option<Stamp>> {
        Ok(db
            .query_row(
                "SELECT incarnation,generation,visible FROM desktop_catalog_sources
            WHERE computer=?1 AND epoch=?2 AND active=1 AND complete=1",
                params![computer, self.epoch],
                |r| {
                    Ok(Stamp {
                        computer: computer.into(),
                        incarnation: r.get(0)?,
                        generation: r.get(1)?,
                        visible: r.get(2)?,
                    })
                },
            )
            .optional()?)
    }

    /// Called only AFTER the actor successfully applies its own FleetEvent.
    /// FleetEvent has no public constructor or wire decoder. UI/feed JSON
    /// cannot register a native source through this interface.
    pub fn observe(&mut self, store: &Store, event: &fleet::FleetEvent) -> Result<()> {
        let Some((computer, generation)) = event.packet.scope() else {
            return Ok(());
        };
        if matches!(
            &event.packet,
            feed::Packet::Offline { .. } | feed::Packet::Remove { .. }
        ) {
            self.native.remove(computer);
            return Ok(());
        }
        let Some(stamp) = self.stamp(&store.db, computer)? else {
            self.native.remove(computer);
            return Ok(());
        };
        if u64::try_from(stamp.generation).ok() != Some(generation) {
            self.native.remove(computer);
            return Ok(());
        }
        ensure!(
            self.native.contains_key(computer) || self.native.len() < catalog::MAX_SOURCES as usize,
            "native discovery source capacity"
        );
        self.native.insert(computer.into(), (stamp, Instant::now()));
        Ok(())
    }

    fn current(&self, db: &Connection, expected: &Stamp) -> Result<bool> {
        let Some((native, at)) = self.native.get(&expected.computer) else {
            return Ok(false);
        };
        Ok(at.elapsed() <= FRESH
            && native == expected
            && self.stamp(db, &expected.computer)?.as_ref() == Some(expected))
    }

    pub fn eligible(&self, store: &Store) -> Result<Vec<String>> {
        let mut result = Vec::new();
        for (stamp, _) in self.native.values() {
            if self.current(&store.db, stamp)? {
                result.push(stamp.computer.clone());
            }
        }
        Ok(result)
    }

    pub fn same(&self, store: &Store, candidate: &Candidate) -> Result<bool> {
        if !self.current(&store.db, &candidate.source)? {
            return Ok(false);
        }
        let raw: Option<String> = store
            .db
            .query_row(
                "SELECT json FROM desktop_catalog_rows WHERE generation=?1 AND key=?2",
                params![candidate.source.visible, candidate.key],
                bounded_row,
            )
            .optional()?;
        let Some(raw) = raw else {
            return Ok(false);
        };
        Ok(policy::same(
            &candidate.target,
            &view::row(&serde_json::from_str(&raw)?),
        ))
    }

    /// Advance a bounded keyset walk of pending intents. One unacknowledged
    /// session cannot starve later sessions, nor can inserts extend this walk.
    pub fn observe_reads(&mut self, store: &mut Store) -> Result<()> {
        if self.read_after >= self.read_through {
            self.read_after = 0;
            self.read_through = reads::watermark(&store.db)?;
        }
        let pending = reads::page(&store.db, self.read_after, self.read_through, 16)?;
        for intent in &pending {
            let Some((stamp, _)) = self.native.get(&intent.mark.computer) else {
                continue;
            };
            if !self.current(&store.db, stamp)? {
                continue;
            }
            let mut query = store.db.prepare(
                "SELECT json FROM desktop_catalog_rows
                WHERE generation=?1 AND CASE WHEN json_valid(json) THEN json_extract(json,'$.name') END=?2 ORDER BY id LIMIT 2",
            )?;
            let mut rows = query.query(params![stamp.visible, intent.mark.name])?;
            let Some(raw) = rows.next()?.map(bounded_row).transpose()? else {
                continue;
            };
            // Ambiguous session identities never clear an intent.
            if rows.next()?.is_some() {
                continue;
            }
            drop(rows);
            drop(query);
            let row = view::row(&serde_json::from_str(&raw)?);
            reads::observe(
                &mut store.db,
                intent,
                &reads::Snapshot {
                    computer: &intent.mark.computer,
                    name: &row.name,
                    run: row.run_id.as_deref(),
                    conversation: row.conversation_id.as_deref(),
                    reply: row.reply_id.as_deref(),
                    signature: row.attention_signature.as_deref(),
                    read_reply: row.read_reply_id.as_deref(),
                    read_signature: row.read_attention_signature.as_deref(),
                },
            )?;
        }
        self.read_after = pending
            .last()
            .map_or(self.read_through, |intent| intent.seq);
        Ok(())
    }

    /// The approval-phase index (zerus-5wmh.66.3.33): at most `limit` flagged
    /// rows per scan over the current native sources, starting with a
    /// different source each scan. Each source walks a pass in id order, from
    /// the last row it handled to the highest flagged id when the pass began
    /// (ids only grow and an update keeps its id, so arrivals neither shift
    /// nor extend a pass; they and rows that entered the phase behind it come
    /// in the next one). Reading moves nothing: a pass moves past a row only
    /// once [`Self::handled`] says the scan decided it. Candidates go through
    /// the same policy, `same()`, native inspection and durable reservation
    /// as the catalog walk's.
    pub fn approvals(&mut self, store: &Store, limit: usize) -> Result<Vec<Indexed>> {
        self.approval_after
            .retain(|computer, _| self.native.contains_key(computer));
        let sources: Vec<Stamp> = self
            .native
            .values()
            .map(|(stamp, _)| stamp.clone())
            .collect();
        if sources.is_empty() {
            return Ok(Vec::new());
        }
        let first = self.approval_next % sources.len();
        self.approval_next = first + 1;
        let mut indexed = Vec::new();
        let mut budget = limit;
        for offset in 0..sources.len() {
            if budget == 0 {
                break;
            }
            let stamp = &sources[(first + offset) % sources.len()];
            let Some(visible) = stamp.visible else {
                continue;
            };
            if !self.current(&store.db, stamp)? {
                continue;
            }
            let pass = self
                .approval_after
                .entry(stamp.computer.clone())
                .or_insert(Pass {
                    generation: visible,
                    after: 0,
                    end: None,
                });
            if pass.generation != visible {
                *pass = Pass {
                    generation: visible,
                    after: 0,
                    end: None,
                };
            }
            let end = match pass.end {
                Some(end) => end,
                None => match catalog::approvals::pass_end(&store.db, visible)? {
                    Some(end) => *pass.end.insert(end),
                    // Nothing flagged: the next scan looks again.
                    None => continue,
                },
            };
            let rows = catalog::approvals::page(&store.db, visible, pass.after, end, budget)?;
            // A short page holds the rest of the pass.
            let short = rows.len() < budget;
            if rows.is_empty() {
                *pass = Pass {
                    generation: visible,
                    after: 0,
                    end: None,
                };
                continue;
            }
            budget -= rows.len();
            let count = rows.len();
            for (index, row) in rows.into_iter().enumerate() {
                let view = view::row(&serde_json::from_str(&row.json)?);
                let candidate = policy::candidate(&stamp.computer, &view).map(|target| Candidate {
                    target,
                    source: stamp.clone(),
                    key: row.key,
                });
                indexed.push(Indexed {
                    computer: stamp.computer.clone(),
                    generation: visible,
                    last: row.id >= end || (short && index + 1 == count),
                    id: row.id,
                    candidate,
                });
            }
        }
        Ok(indexed)
    }

    /// The scan decided `row` (inspected it, or found nothing to inspect):
    /// its pass moves past it, and starts again after its last row. A pass
    /// of another generation, or one already past the row, is left alone.
    pub fn handled(&mut self, row: &Indexed) {
        let Some(pass) = self.approval_after.get_mut(&row.computer) else {
            return;
        };
        if pass.generation != row.generation || pass.after >= row.id {
            return;
        }
        *pass = if row.last {
            Pass {
                generation: row.generation,
                after: 0,
                end: None,
            }
        } else {
            Pass {
                after: row.id,
                ..*pass
            }
        };
    }

    /// One bounded discovery page, never an all-session map. Every candidate
    /// still needs a fresh native question inspection and durable reservation.
    pub fn page(&mut self, store: &mut Store) -> Result<Vec<Candidate>> {
        if self.native.is_empty() {
            return Ok(Vec::new());
        }
        self.computer %= self.native.len();
        let (computer, (stamp, at)) = self
            .native
            .iter()
            .nth(self.computer)
            .expect("bounded source index");
        let (computer, stamp, at) = (computer.clone(), stamp.clone(), *at);
        if at.elapsed() > FRESH || !self.current(&store.db, &stamp)? {
            self.cursor = None;
            self.computer += 1;
            return Ok(Vec::new());
        }
        let cursor = self
            .cursor
            .take()
            .filter(|cursor| cursor.computer == computer);
        let page = match catalog::page(&store.db, &computer, cursor.as_ref()) {
            Ok(page) => page,
            Err(error) => {
                self.computer += 1;
                // A replaced generation invalidates an existing page cursor.
                // A failed first page is an actual catalog failure, not EOF.
                if cursor.is_some() {
                    return Ok(Vec::new());
                }
                return Err(error);
            }
        };
        self.cursor = page.next;
        if self.cursor.is_none() {
            self.computer += 1;
        }
        if !page.complete {
            return Ok(Vec::new());
        }
        let mut candidates = Vec::new();
        for item in page.rows {
            let row = view::row(&serde_json::from_str(&item.json)?);
            if let Some(target) = policy::candidate(&computer, &row) {
                candidates.push(Candidate {
                    target,
                    source: stamp.clone(),
                    key: item.key,
                });
            }
        }
        Ok(candidates)
    }
}

#[cfg(test)]
impl Sources {
    // Unit-test seam for the actor-owned FleetEvent notification. Applying
    // display-feed JSON alone deliberately never invokes this seam.
    pub(super) fn native_fixture(&mut self, store: &Store, computer: &str) {
        let stamp = self.stamp(&store.db, computer).unwrap().unwrap();
        self.native.insert(computer.into(), (stamp, Instant::now()));
    }
}
