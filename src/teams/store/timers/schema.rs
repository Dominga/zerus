//! This package's additions to a store. The extension knows two stores:
//! one from before it (no mark), which it extends to the current layout in
//! one immediate transaction (the tables, indexes and triggers first, then
//! the backfills, then the mark), and one at a layout that reached the
//! integration branch, which it steps forward to the current one. A mark
//! that only an unmerged development build could have written is refused
//! with a sentence that says so, and nothing is written; a newer mark is
//! refused as a newer schema is. Opening a store at the current layout
//! reads one row and takes no write lock.
//!
//! The one-time cost: the backfills walk the teams once and every archived
//! node once, in pages of 200 keys, under the write lock; memory stays one
//! page. Measured here at about 33 SQLite VM steps and 6 microseconds per
//! archived node (10,000 in 60 ms), so a million archived nodes hold the
//! lock for several seconds, once. Another process opening the store
//! meanwhile waits for it, up to a minute, instead of the usual two seconds.
use super::*;
use std::time::Duration;
mod legacy_holds;
use legacy_holds::legacy_owners;

/// The layout the whole transition builds before any step: the first that
/// reached the integration branch, with the timers. Every lower value was
/// written only by unmerged development builds of this package.
pub(crate) const BASE: u32 = 8;
/// The current layout: the base and the steps after it.
pub(crate) const EXTENSION: u32 = 12;
/// Each layout that reached the integration branch, with the step that
/// brings a store marked with it to the next; the steps run in order up to
/// the current layout, after the base for a store from before the extension.
const STEPS: &[(u32, Step)] = &[
    (8, answers),
    (9, card_decisions),
    (10, hold_owners),
    (11, key_refusal_order),
];
/// One step from a merged layout to the next, inside the transition.
type Step = fn(&Connection) -> Result<()>;
/// Layout 8 to 9: the lead's answers reach their members.
fn answers(db: &Connection) -> Result<()> {
    db.execute_batch(include_str!("../answers.sql"))
        .map_err(storage)
}
/// Layout 9 to 10: a card keeps the decisions its opener listed; a row
/// without a list answers by the rule of its kind. The attempt holder
/// narrows in the same layout, so the Returns it strands are handed back.
fn card_decisions(db: &Connection) -> Result<()> {
    let present: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('cards') WHERE name='decisions')",
            [],
            |r| r.get(0),
        )
        .map_err(storage)?;
    if !present {
        db.execute_batch("ALTER TABLE cards ADD COLUMN decisions TEXT")
            .map_err(storage)?;
    }
    unheld_returns(db)
}
/// Layout 10 to 11: a hold has an owner (the plan, "A hold has an owner").
/// An explicit hold keeps its owner beside its flag, a hand-back has a flag
/// of its own, and a pending decision's hold is read from the decision and
/// never stored. A held node of an earlier layout belongs to whatever set its
/// hold last, as the store itself recorded it in order; where the store has
/// no exact record, the hold is the person's ([`legacy_owners`]):
/// - with an unsettled decision: the hold was the decision's, and the stored
///   flag goes;
/// - otherwise the last of its recorded events decides: an explicit Hold
///   makes the hold explicit, owned by its actor (the person or the lead); a
///   hand-back of its current generation makes it a hand-back, which the
///   lead's next decision or Release ends;
/// - a Release last, nothing recorded, or a hand-back that cannot be placed
///   among the records: the person's, which only the person's Release ends;
///   the lead's Release is refused with a sentence that says so.
///
/// The step reads no clock: only sequence numbers order the events.
fn hold_owners(db: &Connection) -> Result<()> {
    for table in ["nodes", "node_archive"] {
        for (column, definition) in [
            ("hold_owner", "TEXT"),
            ("hand_back", "INTEGER NOT NULL DEFAULT 0"),
        ] {
            let present: bool = db
                .query_row(
                    &format!("SELECT EXISTS(SELECT 1 FROM pragma_table_info('{table}') WHERE name='{column}')"),
                    [],
                    |r| r.get(0),
                )
                .map_err(storage)?;
            if !present {
                db.execute_batch(&format!(
                    "ALTER TABLE {table} ADD COLUMN {column} {definition}"
                ))
                .map_err(storage)?;
            }
        }
        db.execute_batch(&format!(
            "UPDATE {table} SET hold=0 WHERE hold AND hold_owner IS NULL AND EXISTS(SELECT 1 FROM decisions d WHERE d.team={table}.team AND d.node={table}.id AND d.state<>'settled');"
        ))
        .map_err(storage)?;
    }
    legacy_owners(db)?;
    for table in ["nodes", "node_archive"] {
        db.execute_batch(&format!(
            "UPDATE {table} SET hold_owner='person' WHERE hold AND hold_owner IS NULL;"
        ))
        .map_err(storage)?;
    }
    Ok(())
}
/// Layout 11 to 12: a team's key refusals are read newest first, for the
/// team's status, through their own partial index.
fn key_refusal_order(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE INDEX IF NOT EXISTS key_refusals_newest ON key_clears(team,at_ms,node) WHERE state='refused';")
        .map_err(storage)
}
/// The hand-back of a Return without a holder as layout 10 records it: the
/// decision settled, the node's one hold flag set, and the lead's item. The
/// step to layout 11 cannot place that item by an exact record, so the hold
/// becomes the person's (`legacy_holds`). The current writer
/// (`rows::hand_back`) writes a column this layout lacks.
fn hand_back_at_layout_10(
    db: &Connection,
    team: &str,
    node: u32,
    seq: u64,
    actor: &str,
    now: i64,
) -> Result<()> {
    rows::settle_decisions(db, team, Some(node), actor, false)?;
    db.execute(
        "UPDATE nodes SET hold=1 WHERE team=?1 AND id=?2",
        params![team, node],
    )
    .map_err(storage)?;
    let (kind, text) = return_without_holder(node, seq);
    rows::hand_back_item(db, team, node, actor, &kind, &text, now)?;
    Ok(())
}
/// Earlier layouts admitted a Return while its hand-out was only reserved.
/// Under the narrowed attempt holder ([`holder_of`]) such a Return has
/// nobody to go to, and nothing would ever carry it out. Each unstarted one
/// (pending, no reopen reserved) of a live team whose node's holder reads
/// nobody is handed back to the lead once, as a refused reopen is: the
/// decision settles, the node is held, and the lead gets one inbox item of
/// its own kind; the team's revision moves. Pages of 200 live teams, and per
/// team pages of 200 unsettled decisions through their partial index.
fn unheld_returns(db: &Connection) -> Result<()> {
    let teams = format!(
        "SELECT id FROM teams WHERE id>?1 AND status NOT IN {TERMINAL} ORDER BY id LIMIT 200"
    );
    let mut after = String::new();
    loop {
        let ids = column::<String>(db, &teams, params![after])?;
        for team in &ids {
            unheld_returns_of(db, team)?;
        }
        match ids.last() {
            Some(last) if ids.len() == 200 => after = last.clone(),
            _ => return Ok(()),
        }
    }
}
fn unheld_returns_of(db: &Connection, team: &str) -> Result<()> {
    let pending = format!(
        "SELECT d.seq,d.node FROM decisions d INDEXED BY decision_pending JOIN nodes n ON n.team=d.team AND n.id=d.node WHERE d.team=?1 AND d.state<>'{}' AND d.node>?2 AND d.kind='return' AND d.state='{}' ORDER BY d.node LIMIT 200",
        DecisionState::Settled.as_str(),
        DecisionState::Pending.as_str()
    );
    let Some(owner) = db
        .query_row("SELECT owner FROM teams WHERE id=?1", [team], |r| {
            r.get::<_, String>(0)
        })
        .optional()
        .map_err(storage)?
    else {
        return Ok(());
    };
    let actor = encode(&crate::teams::ops::Actor::Service {
        person: owner,
        team: team.into(),
    })?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64);
    let mut after = 0u32;
    let mut moved = false;
    loop {
        let rows = db
            .prepare(&pending)
            .and_then(|mut s| {
                s.query_map(params![team, after], |r| {
                    Ok((r.get::<_, u64>(0)?, r.get::<_, u32>(1)?))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()
            })
            .map_err(storage)?;
        for (seq, node) in &rows {
            if holder_of(db, team, *node)?.is_some() {
                continue;
            }
            hand_back_at_layout_10(db, team, *node, *seq, &actor, now)?;
            moved = true;
        }
        match rows.last() {
            Some((_, node)) if rows.len() == 200 => after = *node,
            _ => break,
        }
    }
    if moved {
        db.execute("UPDATE teams SET revision=revision+1 WHERE id=?1", [team])
            .map_err(storage)?;
    }
    Ok(())
}

#[cfg(test)]
type Hook = Box<dyn FnOnce() -> Result<()>>;
#[cfg(test)]
thread_local! {
    /// Runs inside the transition, after the backfills, before the marker.
    pub(crate) static BEFORE_MARK: std::cell::RefCell<Option<Hook>> =
        const { std::cell::RefCell::new(None) };
    /// Transitions completed on this thread.
    pub(crate) static TRANSITIONS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    /// Runs on the open path right after the first, unlocked fence.
    pub(crate) static AFTER_FENCE: std::cell::RefCell<Option<Hook>> =
        const { std::cell::RefCell::new(None) };
}
#[cfg(test)]
pub(in crate::teams::store) fn after_fence() -> Result<()> {
    match AFTER_FENCE.with(|slot| slot.borrow_mut().take()) {
        Some(hook) => hook(),
        None => Ok(()),
    }
}

/// Read first and read only, before any statement of the store's schema
/// extensions that can write: a store marked by a newer program or by an
/// unmerged development build is refused and nothing is written to it.
/// Every write of the open path reads the mark again under the lock it
/// writes with: the common batches, the read index conversion and the
/// transition.
pub(in crate::teams::store) fn fence(db: &Connection) -> Result<()> {
    marked(db).map(|_| ())
}
/// The preview-off disposition reader cannot migrate an earlier layout.
pub(in crate::teams::store) fn as_is(db: &Connection) -> Result<()> {
    if marked(db)? != Mark::Current {
        return Err(Problem::new(
            code::INVALID_STATE,
            "Teams store needs migration; no team data was changed",
        ));
    }
    Ok(())
}
pub(in crate::teams::store) fn extend(db: &Connection) -> Result<()> {
    if marked(db)? == Mark::Current {
        // The current layout's idempotent index, kept here instead of a
        // newer mark so an older build still opens the upgraded store
        // (zerus-63rru.52 round 2): a store at this layout that predates
        // the index gains it on open, and a base build ignores it.
        db.execute_batch("CREATE INDEX IF NOT EXISTS cancel_node_newest ON cancel_outcomes(team,node,decision DESC);")
            .map_err(storage)?;
        return Ok(());
    }
    let own = db.is_autocommit();
    let prior: u64 = db
        .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
        .map_err(storage)?;
    if own {
        db.busy_timeout(Duration::from_secs(60)).map_err(storage)?;
    }
    let begun = db.execute_batch(if own {
        "BEGIN IMMEDIATE;"
    } else {
        "SAVEPOINT timers;"
    });
    let result = begun.map_err(begin_error).and_then(|()| {
        let result = marked(db).and_then(|mark| match mark {
            Mark::Current => Ok(()),
            mark => transition(db, mark),
        });
        let end = match (&result, own) {
            (Ok(()), true) => "COMMIT;",
            (Ok(()), false) => "RELEASE timers;",
            (Err(_), true) => "ROLLBACK;",
            (Err(_), false) => "ROLLBACK TO timers; RELEASE timers;",
        };
        db.execute_batch(end).map_err(storage)?;
        result
    });
    if own {
        db.busy_timeout(Duration::from_millis(prior))
            .map_err(storage)?;
    }
    result
}
/// What the completion marker says about a store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mark {
    /// A store from before the extension.
    Absent,
    /// An earlier layout that reached the integration branch.
    Merged(u32),
    Current,
}
/// The store's mark. A newer one is a store written by a newer program and
/// is refused as a newer schema is; any other value was written only by an
/// unmerged development build and is refused too. Nothing is written.
fn marked(db: &Connection) -> Result<Mark> {
    match version(db)? {
        None => Ok(Mark::Absent),
        Some(EXTENSION) => Ok(Mark::Current),
        // The same refusal as a newer core schema's.
        Some(v) if v > EXTENSION => Err(Problem::new(
            code::INVALID_STATE,
            "store schema is newer than this version of Zerus",
        )),
        Some(v) if STEPS.iter().any(|(from, _)| *from == v) => Ok(Mark::Merged(v)),
        Some(_) => Err(Problem::new(
            code::INVALID_STATE,
            "store schema was written by an unmerged development build of Zerus, and this version does not open it",
        )),
    }
}
/// The layout version the completion marker holds, if any.
fn version(db: &Connection) -> Result<Option<u32>> {
    let table: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='timers_extension')",
            [],
            |r| r.get(0),
        )
        .map_err(storage)?;
    if !table {
        return Ok(None);
    }
    db.query_row("SELECT version FROM timers_extension WHERE id=1", [], |r| {
        r.get(0)
    })
    .optional()
    .map_err(storage)
}
fn transition(db: &Connection, mark: Mark) -> Result<()> {
    let from = match mark {
        Mark::Absent => {
            // Triggers exist before any backfill reads, so a write that
            // commits after this transaction is seen by them, and none can
            // commit inside it.
            db.execute_batch(include_str!("../timers.sql"))
                .map_err(storage)?;
            observed_attempts(db)?;
            backfill_teams(db)?;
            backfill_released(db)?;
            backfill_stopped(db)?;
            BASE
        }
        Mark::Merged(from) => from,
        Mark::Current => return Ok(()),
    };
    for (_, step) in STEPS.iter().filter(|(layout, _)| *layout >= from) {
        step(db)?;
    }
    #[cfg(test)]
    if let Some(hook) = BEFORE_MARK.with(|slot| slot.borrow_mut().take()) {
        hook()?;
    }
    db.execute(
        "INSERT INTO timers_extension(id,version) VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET version=excluded.version",
        [EXTENSION],
    )
    .map_err(storage)?;
    #[cfg(test)]
    TRANSITIONS.with(|n| n.set(n.get() + 1));
    Ok(())
}
fn backfill_teams(db: &Connection) -> Result<()> {
    let sql =
        format!("SELECT id FROM teams WHERE id>?1 AND status IN {TERMINAL} ORDER BY id LIMIT 200");
    let mut insert = db
        .prepare("INSERT OR IGNORE INTO key_clear_teams(team,pending) VALUES(?1,1)")
        .map_err(storage)?;
    let mut after = String::new();
    loop {
        let ids = column::<String>(db, &sql, params![after])?;
        for id in &ids {
            insert.execute([id]).map_err(storage)?;
        }
        match ids.last() {
            Some(last) if ids.len() == 200 => after = last.clone(),
            _ => return Ok(()),
        }
    }
}
/// A task observation keeps the attempt it shows beside its data: columns of
/// package 6's table, in its definition for a new store and added here to a
/// table a store from before the extension already has. Such a store holds
/// no attempt of any task, so its first observation of a pair is judged
/// against no attempt, as for a task that never recorded one.
fn observed_attempts(db: &Connection) -> Result<()> {
    for (column, kind) in [
        ("attempt", "INTEGER NOT NULL DEFAULT 0"),
        ("attempt_id", "TEXT NOT NULL DEFAULT ''"),
        ("last_attempt_id", "TEXT NOT NULL DEFAULT ''"),
        ("attempt_conflict", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        let present: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('task_observations') WHERE name=?1)",
                [column],
                |r| r.get(0),
            )
            .map_err(storage)?;
        if !present {
            let sql = format!("ALTER TABLE task_observations ADD COLUMN {column} {kind}");
            db.execute_batch(&sql).map_err(storage)?;
        }
    }
    Ok(())
}
/// Walk the archive once, in pages, for stages released before the frontier.
fn backfill_released(db: &Connection) -> Result<()> {
    let page =
        "SELECT team,id FROM node_archive WHERE (team,id)>(?1,?2) ORDER BY team,id LIMIT 200";
    let insert = format!("INSERT OR IGNORE INTO key_clear_work(team,node,kind) SELECT a.team,a.id,'stage' FROM node_archive a WHERE a.team=?1 AND a.id=?2 AND {RELEASED}");
    let mut insert = db.prepare(&insert).map_err(storage)?;
    let mut after = (String::new(), 0u32);
    loop {
        let keys = rows(db, page, params![after.0, after.1], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?))
        })?;
        for (team, id) in &keys {
            insert.execute(params![team, id]).map_err(storage)?;
        }
        match keys.last() {
            Some(last) if keys.len() == 200 => after = last.clone(),
            _ => return Ok(()),
        }
    }
}
/// A stopped or archived team's open nodes whose cancel settled before the
/// frontier existed; one statement per pending team, over its live nodes.
fn backfill_stopped(db: &Connection) -> Result<()> {
    let mut insert = db.prepare(STOPPED).map_err(storage)?;
    let mut after = String::new();
    loop {
        let teams = column::<String>(
            db,
            "SELECT team FROM key_clear_teams WHERE pending=1 AND team>?1 ORDER BY team LIMIT 200",
            params![after],
        )?;
        for team in &teams {
            insert.execute([team]).map_err(storage)?;
        }
        match teams.last() {
            Some(last) if teams.len() == 200 => after = last.clone(),
            _ => return Ok(()),
        }
    }
}
const STOPPED: &str = "INSERT OR IGNORE INTO key_clear_work(team,node,kind) SELECT n.team,n.id,'stop' FROM nodes n JOIN node_identity i ON i.team=n.team AND i.id=n.id LEFT JOIN stage_tasks s ON s.team=n.team AND s.node=n.id WHERE n.team=?1 AND (i.task_uuid<>'' OR s.request_id IS NOT NULL) AND NOT EXISTS(SELECT 1 FROM decisions d WHERE d.team=n.team AND d.node=n.id AND d.state<>'settled' AND d.kind='cancel') AND NOT EXISTS(SELECT 1 FROM task_observations o WHERE o.team=n.team AND o.node=n.id AND json_extract(o.data,'$.key') IS NOT NULL) AND NOT EXISTS(SELECT 1 FROM key_clears k WHERE k.team=n.team AND k.node=n.id)";
