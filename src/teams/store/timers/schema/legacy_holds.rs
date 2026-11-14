//! The owner of each hold that an earlier layout stored as one flag, for the
//! step to layout 11 (the plan, "A hold has an owner"). A held node without
//! an unsettled decision belongs to whatever set its hold last, in the order
//! the store itself recorded: the journal's sequence numbers. No clock is
//! read. Where the store has no exact record, the hold is the person's: the
//! step may leave the lead unable to release a hold that was its own, never
//! give the lead a hold that could be the person's.
//!
//! Recorded exactly, by the journal row of the operation that made it:
//! - an explicit Hold (its actor, the person or the lead) and a Release, by
//!   the node in the row's references;
//! - a reopen that was refused, uncertain or cutting over, by the effect in
//!   the row's references;
//! - a refused cancel, an integration conflict or a failing verdict whose
//!   holder was removed, and a pipeline waiting on the person, by the item
//!   or card named in the saved reply of the kind of operation that files
//!   it (a verdict only on a stage of the node).
//!
//! Never placed among those records, so the person's whatever the rows say:
//! a removed member's item, a pipeline active on a stage, and the step to
//! layout 10's own Return item.
//!
//! The read is bounded: one pass over each such team's live journal (at most
//! [`LIVE_JOURNAL_CAP`] rows); the archive is not read, and a hold whose
//! deciding row could be there is the person's.
use super::*;
use crate::teams::store::LIVE_JOURNAL_CAP;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Event {
    Hold(&'static str),
    Release,
    HandBack,
}
#[derive(Default)]
struct Held {
    table: &'static str,
    /// A hand-back of a kind that cannot be placed, or an ambiguous link.
    unplaceable: bool,
    /// The hand-backs of its current generation, and those found in order.
    /// Each with the kind of operation that files it: only that
    /// operation's saved reply places it.
    traces: BTreeMap<String, &'static str>,
    placed: BTreeSet<String>,
    last: Option<(u64, Event)>,
}
impl Held {
    fn record(&mut self, seq: u64, event: Event) {
        if self.last.is_none_or(|(last, _)| seq > last) {
            self.last = Some((seq, event));
        }
    }
    /// What the step writes: the person's unless an exact record decides.
    fn owner(&self) -> Option<Event> {
        if self.unplaceable || !self.traces.keys().all(|k| self.placed.contains(k)) {
            return None;
        }
        match self.last {
            Some((_, event @ (Event::Hold(_) | Event::HandBack))) => Some(event),
            _ => None,
        }
    }
}

pub(super) fn legacy_owners(db: &Connection) -> Result<()> {
    let teams: Vec<String> = column(
        db,
        "SELECT team FROM nodes WHERE hold AND hold_owner IS NULL UNION SELECT team FROM node_archive WHERE hold AND hold_owner IS NULL",
        params![],
    )?;
    for team in teams {
        team_owners(db, &team)?;
    }
    Ok(())
}

fn column<T: rusqlite::types::FromSql>(
    db: &Connection,
    sql: &str,
    args: impl rusqlite::Params,
) -> Result<Vec<T>> {
    let mut statement = db.prepare(sql).map_err(storage)?;
    let rows = statement
        .query_map(args, |r| r.get(0))
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<T>>>()
        .map_err(storage)?;
    Ok(rows)
}

fn team_owners(db: &Connection, team: &str) -> Result<()> {
    let mut held: BTreeMap<u32, Held> = BTreeMap::new();
    for table in ["nodes", "node_archive"] {
        let mut statement = db
            .prepare(&format!(
                "SELECT id,generation FROM {table} WHERE team=?1 AND hold AND hold_owner IS NULL"
            ))
            .map_err(storage)?;
        let found = statement
            .query_map([team], |r| Ok((r.get::<_, u32>(0)?, r.get::<_, u32>(1)?)))
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)?;
        for (node, generation) in found {
            let mut h = Held {
                table,
                ..Held::default()
            };
            traces(db, team, node, generation, &mut h)?;
            held.insert(node, h);
        }
    }
    // Each trace's key, to the node it belongs to and the operation that
    // files it.
    let mut links: BTreeMap<String, Vec<(u32, &'static str)>> = BTreeMap::new();
    for (node, h) in &held {
        for (key, op) in &h.traces {
            links.entry(key.clone()).or_default().push((*node, *op));
        }
    }
    let filers: BTreeSet<&str> = links
        .values()
        .flatten()
        .map(|(_, op)| *op)
        .filter(|op| *op != "reopen_outcome")
        .collect();
    let mut statement = db
        .prepare("SELECT seq,kind,actor,request_id,refs FROM journal WHERE team=?1 ORDER BY seq DESC LIMIT ?2")
        .map_err(storage)?;
    let journal = statement
        .query_map(params![team, LIVE_JOURNAL_CAP], |r| {
            Ok((
                r.get::<_, u64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage)?;
    for (seq, kind, actor, request, refs) in journal {
        let refs: Vec<String> = serde_json::from_str(&refs).unwrap_or_default();
        match kind.as_str() {
            "hold" | "release" => {
                let Some(h) = refs
                    .iter()
                    .find_map(|r| r.strip_prefix("node:")?.parse::<u32>().ok())
                    .and_then(|node| held.get_mut(&node))
                else {
                    continue;
                };
                if kind == "release" {
                    h.record(seq, Event::Release);
                } else {
                    match owner(&actor) {
                        Some(owner) => h.record(seq, Event::Hold(owner)),
                        None => h.unplaceable = true,
                    }
                }
            }
            "reopen_outcome" => {
                for key in refs.iter().filter(|r| r.starts_with("effect:")) {
                    place(&mut held, &links, key, seq);
                }
            }
            filer if filers.contains(filer) => {
                let Some(saved) = saved(db, &request)? else {
                    continue;
                };
                let Some(item) = saved.item else {
                    continue;
                };
                // A reply names an item, card, effect or other row by its
                // number alone. The trace's own kind of operation says which:
                // a refused cancel or a conflict files its item, a verdict its
                // item or card, and a verdict must be on a stage of the node.
                let mut found: BTreeMap<u32, Vec<String>> = BTreeMap::new();
                for key in [format!("inbox:{item}"), format!("card:{item}")] {
                    for (node, op) in links.get(&key).into_iter().flatten() {
                        if *op == filer
                            && (filer != "verdict" || saved.on_stage_of(db, team, *node)?)
                        {
                            found.entry(*node).or_default().push(key.clone());
                        }
                    }
                }
                for (node, keys) in found {
                    let Some(h) = held.get_mut(&node) else {
                        continue;
                    };
                    if keys.len() > 1 {
                        // The number names both an item and a card of the node.
                        h.unplaceable = true;
                        continue;
                    }
                    h.placed.insert(keys[0].clone());
                    h.record(seq, Event::HandBack);
                }
            }
            _ => (),
        }
    }
    for (node, h) in &held {
        let set = match h.owner() {
            Some(Event::Hold(owner)) => format!("hold_owner='{owner}'"),
            Some(Event::HandBack) => "hold=0,hand_back=1".to_owned(),
            _ => continue,
        };
        db.execute(
            &format!("UPDATE {} SET {set} WHERE team=?1 AND id=?2", h.table),
            params![team, node],
        )
        .map_err(storage)?;
    }
    Ok(())
}

/// The node's hand-backs in its current generation, each by the key that
/// links it to the journal row of the operation that made it; a kind that
/// no record places makes the hold the person's.
fn traces(db: &Connection, team: &str, node: u32, generation: u32, h: &mut Held) -> Result<()> {
    h.unplaceable = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM inbox WHERE team=?1 AND node=?2 AND generation=?3 AND (kind LIKE 'member_removed:%' OR kind LIKE 'return_without_holder:%'))
              OR EXISTS(SELECT 1 FROM pipeline_runs WHERE team=?1 AND node=?2 AND generation=?3 AND state='active' AND stage IS NOT NULL)",
            params![team, node, generation],
            |r| r.get(0),
        )
        .map_err(storage)?;
    let mut statement = db
        .prepare("SELECT seq,kind FROM inbox WHERE team=?1 AND node=?2 AND generation=?3 AND (kind LIKE 'cancel_refused:%' OR kind LIKE 'conflict_holder_removed:%' OR kind LIKE 'verdict_holder_removed:%')")
        .map_err(storage)?;
    let items = statement
        .query_map(params![team, node, generation], |r| {
            Ok((r.get::<_, u64>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage)?;
    let cards: Vec<u64> = column(
        db,
        "SELECT c.seq FROM cards c WHERE c.team=?1 AND c.node=?2 AND c.kind='pipeline_acceptance' AND c.state='open' AND EXISTS(SELECT 1 FROM pipeline_runs p WHERE p.team=?1 AND p.node=?2 AND p.generation=?3 AND p.state='person')",
        params![team, node, generation],
    )?;
    let effects: Vec<String> = column(
        db,
        "SELECT request_id FROM effects WHERE team=?1 AND node=?2 AND generation=?3 AND kind='reopen' AND state IN ('refused','uncertain','cutting_over')",
        params![team, node, generation],
    )?;
    for (seq, kind) in items {
        let op = if kind.starts_with("cancel_refused:") {
            "record_cancel"
        } else if kind.starts_with("conflict_holder_removed:") {
            "record_integration_conflict"
        } else {
            "verdict"
        };
        h.traces.insert(format!("inbox:{seq}"), op);
    }
    h.traces
        .extend(cards.into_iter().map(|s| (format!("card:{s}"), "verdict")));
    h.traces.extend(
        effects
            .into_iter()
            .map(|id| (format!("effect:{id}"), "reopen_outcome")),
    );
    Ok(())
}

fn place(
    held: &mut BTreeMap<u32, Held>,
    links: &BTreeMap<String, Vec<(u32, &'static str)>>,
    key: &str,
    seq: u64,
) {
    for (node, _) in links.get(key).into_iter().flatten() {
        if let Some(h) = held.get_mut(node) {
            h.placed.insert(key.to_owned());
            h.record(seq, Event::HandBack);
        }
    }
}

/// Only the person and the lead could hold a node: the person's actor, or
/// the lead's session.
fn owner(actor: &str) -> Option<&'static str> {
    let actor: serde_json::Value = serde_json::from_str(actor).ok()?;
    match actor["kind"].as_str()? {
        "person" => Some("person"),
        "session" => Some("lead"),
        _ => None,
    }
}

/// What a saved request names: the row its reply filed, by number, and the
/// node its operation names.
struct Saved {
    item: Option<u64>,
    node: Option<u64>,
}
impl Saved {
    /// The operation is on a stage of the node.
    fn on_stage_of(&self, db: &Connection, team: &str, node: u32) -> Result<bool> {
        let Some(named) = self.node else {
            return Ok(false);
        };
        let target: Option<u32> = db
            .query_row(
                "SELECT target FROM stage_tasks WHERE team=?1 AND node=?2",
                params![team, named],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?;
        Ok(target == Some(node))
    }
}
fn saved(db: &Connection, request: &str) -> Result<Option<Saved>> {
    let row: Option<(String, String)> = db
        .query_row(
            "SELECT content,result FROM requests WHERE id=?1",
            [request],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(storage)?;
    Ok(row.map(|(content, result)| {
        let content: serde_json::Value = serde_json::from_str(&content).unwrap_or_default();
        let result: serde_json::Value = serde_json::from_str(&result).unwrap_or_default();
        Saved {
            item: result["Success"]["item"].as_u64(),
            node: content["node"].as_u64(),
        }
    }))
}
