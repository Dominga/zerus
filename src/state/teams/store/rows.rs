//! Task 288 (owner, 2026-11-08): a team's tasks and their attempts live in
//! their own rows, so a change writes kilobytes instead of the whole record.
//!
//! Team 33fbafeb's record was 5.3 MB, 4.68 MB of it tasks and 4.18 MB
//! attempts; every change parsed and rewrote all of it under the store's write
//! lock. Now:
//!
//! - `teams.payload` keeps the head: every field but the tasks (members,
//!   phases, settings, ...), with `"tasks"` a sentinel string, so an older hgs
//!   refuses the record ("Invalid team record") instead of writing it back
//!   without its tasks. `layout` is 2; `head_rev` is the revision that wrote
//!   the head. `revision` and `updated` are the columns' own.
//! - `team_tasks(team, task, ord, rev, payload)`: a task without its attempts,
//!   keyed by its id, `ord` its place in the plan.
//! - `team_attempts(team, task, ord, attempt, rev, payload)`: one attempt,
//!   keyed by its task and its place in the task.
//! - `rev` is the team revision that last wrote the row: a copy of the team
//!   at revision R is brought up to date by reading the rows written after R.
//!
//! A change of a copy the store already holds compares it with the copy and
//! writes only the head (when it changed), the tasks and the attempts that
//! changed. A team whose task ids are not unique keeps the whole-record layout
//! (`layout` 1), as every team did before; `layout` NULL is a team not yet
//! migrated. Each team migrates in its own transaction at a write open, so the
//! migration is idempotent and resumes where it stopped.
//!
//! Task #4 of team ede724c4 (owner, 2026-11-10): team 33fbafeb's head was
//! 2.4 MB, 2.2 MB of it the lead changes' `tasks_before`/`phases_before`
//! snapshots, rewritten by every head change. Layout 3 keeps them in
//! `team_lead_changes(team, id, rev, payload)`, written once when the change
//! is recorded and read on demand (a revert); the head keeps each change's
//! summary fields and the ids it changed (`changed`, `phases_changed`). A head
//! of layout 2 (task 288) is read the same and moves to layout 3 at a write
//! open, one transaction a team. An hgs of task 288 reads `layout` 3 as a whole
//! record and refuses it (the tasks sentinel), so it never rewrites the head
//! without the history.
use super::super::model::{Attempt, LeadChange, Phase, Task};
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The value `"tasks"` holds in a head: not a list, so an older hgs's
/// deserialization fails and it never rewrites the record without its tasks.
pub(super) const SENTINEL: &str = "stored in team_tasks (Zerus team store layout 3): this hgs is too old to read this team";
/// Rows with the lead changes' snapshots in `team_lead_changes` (task #4).
pub(super) const ROWS: i64 = 3;
/// Task 288's rows: the snapshots still in the head; read the same as
/// `ROWS`, migrated to it at a write open.
pub(super) const TASK_ROWS: i64 = 2;
pub(super) const JSON: i64 = 1;

/// Whether a team of this layout is stored in rows.
pub(super) fn is_rows(layout: Option<i64>) -> bool {
    matches!(layout, Some(ROWS) | Some(TASK_ROWS))
}

pub(super) const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS team_tasks(team TEXT NOT NULL,task INTEGER NOT NULL,ord INTEGER NOT NULL,
        rev INTEGER NOT NULL,payload TEXT NOT NULL,PRIMARY KEY(team,task));
    CREATE INDEX IF NOT EXISTS team_tasks_rev ON team_tasks(team,rev);
    CREATE TABLE IF NOT EXISTS team_attempts(team TEXT NOT NULL,task INTEGER NOT NULL,ord INTEGER NOT NULL,attempt TEXT NOT NULL,
        rev INTEGER NOT NULL,payload TEXT NOT NULL,PRIMARY KEY(team,task,ord));
    CREATE INDEX IF NOT EXISTS team_attempts_rev ON team_attempts(team,rev);
    CREATE TRIGGER IF NOT EXISTS team_rows_delete AFTER DELETE ON teams BEGIN
        DELETE FROM team_tasks WHERE team=OLD.id; DELETE FROM team_attempts WHERE team=OLD.id; END;
    CREATE TABLE IF NOT EXISTS team_lead_changes(team TEXT NOT NULL,id TEXT NOT NULL,rev INTEGER NOT NULL,payload TEXT NOT NULL,
        PRIMARY KEY(team,id));
    CREATE TRIGGER IF NOT EXISTS team_lead_changes_delete AFTER DELETE ON teams BEGIN
        DELETE FROM team_lead_changes WHERE team=OLD.id; END;";

/// A lead change's snapshots, as its `team_lead_changes` row holds them.
#[derive(Default, Deserialize, Serialize)]
pub(crate) struct History {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tasks_before: Vec<Task>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub phases_before: Vec<Phase>,
}

/// A copy of one team as the store last read or wrote it, with the texts its
/// rows hold, so a change finds what it changed without serializing the rest.
#[derive(Clone)]
pub(crate) struct Snapshot {
    pub revision: u64,
    pub layout: i64,
    pub team: Team,
    /// The head row's text (layout 2), empty otherwise.
    pub head: String,
    /// Each task row's text by task id (layout 2).
    pub tasks: HashMap<u32, String>,
}

/// What a write changed, to bring the copy along once it commits.
pub(crate) struct Saved {
    pub layout: i64,
    pub head: Option<String>,
    pub tasks: Vec<(u32, String)>,
    pub removed: Vec<u32>,
    /// Every row was written (a new team, a migration, a change of layout).
    pub full: bool,
}

/// Adds the row tables and the `layout` and `head_rev` columns, once.
pub(super) fn ensure_schema(db: &Connection) -> anyhow::Result<()> {
    let columns: Vec<String> = db.prepare("SELECT name FROM pragma_table_info('teams')")?.query_map([], |row| row.get(0))?.collect::<rusqlite::Result<_>>()?;
    let has_tables: bool = db.query_row(
        "SELECT (SELECT COUNT(*) FROM sqlite_master WHERE name IN ('team_attempts','team_lead_changes','team_lead_changes_delete'))=3",
        [],
        |row| row.get(0),
    )?;
    if has_tables && columns.iter().any(|name| name == "layout") && columns.iter().any(|name| name == "head_rev") {
        return Ok(());
    }
    db.execute_batch("BEGIN IMMEDIATE")?;
    let done = (|| -> rusqlite::Result<()> {
        let columns: Vec<String> = db.prepare("SELECT name FROM pragma_table_info('teams')")?.query_map([], |row| row.get(0))?.collect::<rusqlite::Result<_>>()?;
        if !columns.iter().any(|name| name == "layout") {
            db.execute_batch("ALTER TABLE teams ADD COLUMN layout INTEGER")?;
        }
        if !columns.iter().any(|name| name == "head_rev") {
            db.execute_batch("ALTER TABLE teams ADD COLUMN head_rev INTEGER")?;
        }
        db.execute_batch(SCHEMA)
    })();
    match done {
        Ok(()) => db.execute_batch("COMMIT")?,
        Err(error) => {
            let _ = db.execute_batch("ROLLBACK");
            return Err(error.into());
        }
    }
    Ok(())
}

/// Whether every task id of the team is distinct: the rows are keyed by it.
pub(super) fn keyable(team: &Team) -> bool {
    let mut seen = std::collections::HashSet::new();
    team.tasks.iter().all(|task| seen.insert(task.id))
}

/// The head row's text: the team without its tasks, revision and update time.
pub(super) fn head_text(team: &mut Team) -> anyhow::Result<String> {
    let tasks = std::mem::take(&mut team.tasks);
    let (revision, updated) = (team.revision, team.updated);
    team.revision = 0;
    team.updated = 0.0;
    let value = serde_json::to_value(&*team);
    team.tasks = tasks;
    team.revision = revision;
    team.updated = updated;
    let mut value = value?;
    value["tasks"] = json!(SENTINEL);
    Ok(value.to_string())
}

/// A task row's text: the task without its attempts.
fn task_text(task: &mut Task) -> anyhow::Result<String> {
    let attempts = std::mem::take(&mut task.attempts);
    let text = serde_json::to_string(&*task);
    task.attempts = attempts;
    Ok(text?)
}

fn parse_head(text: &str) -> anyhow::Result<Team> {
    let mut value: Value = serde_json::from_str(text).context("Invalid team record")?;
    value["tasks"] = json!([]);
    serde_json::from_value(value).context("Invalid team record")
}

/// The team's row, its layout and head revision, when it exists.
struct Head {
    revision: u64,
    updated: f64,
    layout: Option<i64>,
    head_rev: u64,
    payload: String,
}

fn head_row(db: &Connection, id: &str, with_payload: bool) -> anyhow::Result<Option<Head>> {
    let sql = if with_payload {
        "SELECT revision,updated,layout,COALESCE(head_rev,0),payload FROM teams WHERE id=?1"
    } else {
        "SELECT revision,updated,layout,COALESCE(head_rev,0),'' FROM teams WHERE id=?1"
    };
    Ok(db
        .query_row(sql, params![id], |row| {
            Ok(Head {
                revision: row.get::<_, i64>(0)? as u64,
                updated: row.get(1)?,
                layout: row.get(2)?,
                head_rev: row.get::<_, i64>(3)? as u64,
                payload: row.get(4)?,
            })
        })
        .optional()?)
}

/// The team as its rows hold it, with the texts, or None when it does not
/// exist. A record of the whole-record layout is parsed as before.
pub(crate) fn read(db: &Connection, id: &str) -> anyhow::Result<Option<Snapshot>> {
    let Some(head) = head_row(db, id, true)? else { return Ok(None) };
    if !is_rows(head.layout) {
        let mut team: Team = serde_json::from_str(&head.payload).context("Invalid team record")?;
        team.revision = head.revision;
        return Ok(Some(Snapshot { revision: head.revision, layout: head.layout.unwrap_or(0), team, head: String::new(), tasks: HashMap::new() }));
    }
    let layout = head.layout.unwrap_or(ROWS);
    let mut team = parse_head(&head.payload)?;
    let mut texts = HashMap::new();
    let rows: Vec<(u32, String)> = db
        .prepare("SELECT task,payload FROM team_tasks WHERE team=?1 ORDER BY ord")?
        .query_map(params![id], |row| Ok((row.get::<_, i64>(0)? as u32, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut index = HashMap::new();
    for (task_id, text) in rows {
        let task: Task = serde_json::from_str(&text).context("Invalid task row")?;
        index.insert(task_id, team.tasks.len());
        team.tasks.push(task);
        texts.insert(task_id, text);
    }
    let attempts: Vec<(u32, String)> = db
        .prepare("SELECT task,payload FROM team_attempts WHERE team=?1 ORDER BY task,ord")?
        .query_map(params![id], |row| Ok((row.get::<_, i64>(0)? as u32, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (task_id, text) in attempts {
        if let Some(&at) = index.get(&task_id) {
            team.tasks[at].attempts.push(serde_json::from_str(&text).context("Invalid attempt row")?);
        }
    }
    team.revision = head.revision;
    team.updated = head.updated;
    Ok(Some(Snapshot { revision: head.revision, layout, team, head: head.payload, tasks: texts }))
}

/// Brings `snapshot` up to the stored revision, reading only the rows written
/// after it (a head written after it, the changed tasks and attempts, the
/// order and the attempt counts). False when the team is gone.
pub(crate) fn refresh(db: &Connection, id: &str, snapshot: &mut Snapshot) -> anyhow::Result<bool> {
    let Some(head) = head_row(db, id, false)? else { return Ok(false) };
    if head.revision == snapshot.revision {
        return Ok(true);
    }
    if !is_rows(head.layout) || head.layout != Some(snapshot.layout) {
        let Some(fresh) = read(db, id)? else { return Ok(false) };
        *snapshot = fresh;
        return Ok(true);
    }
    let since = snapshot.revision as i64;
    let mut tasks = std::mem::take(&mut snapshot.team.tasks);
    if head.head_rev > snapshot.revision {
        let payload: String = db.query_row("SELECT payload FROM teams WHERE id=?1", params![id], |row| row.get(0))?;
        snapshot.team = parse_head(&payload)?;
        snapshot.head = payload;
    }
    let changed: Vec<(u32, String)> = db
        .prepare("SELECT task,payload FROM team_tasks WHERE team=?1 AND rev>?2")?
        .query_map(params![id, since], |row| Ok((row.get::<_, i64>(0)? as u32, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let order: Vec<u32> = db
        .prepare("SELECT task FROM team_tasks WHERE team=?1 ORDER BY ord")?
        .query_map(params![id], |row| Ok(row.get::<_, i64>(0)? as u32))?
        .collect::<rusqlite::Result<_>>()?;
    let attempts: Vec<(u32, usize, String)> = db
        .prepare("SELECT task,ord,payload FROM team_attempts WHERE team=?1 AND rev>?2 ORDER BY task,ord")?
        .query_map(params![id, since], |row| Ok((row.get::<_, i64>(0)? as u32, row.get::<_, i64>(1)? as usize, row.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let counts: HashMap<u32, usize> = db
        .prepare("SELECT task,COUNT(*) FROM team_attempts WHERE team=?1 GROUP BY task")?
        .query_map(params![id], |row| Ok((row.get::<_, i64>(0)? as u32, row.get::<_, i64>(1)? as usize)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut old: HashMap<u32, Task> = tasks.drain(..).map(|task| (task.id, task)).collect();
    for (task_id, text) in &changed {
        let mut task: Task = serde_json::from_str(text).context("Invalid task row")?;
        if let Some(previous) = old.remove(task_id) {
            task.attempts = previous.attempts;
        }
        old.insert(*task_id, task);
        snapshot.tasks.insert(*task_id, text.clone());
    }
    let mut rebuilt = Vec::with_capacity(order.len());
    for task_id in &order {
        // A task row missing from the copy and not among the changed rows
        // cannot be brought up to date: read the whole team.
        let Some(mut task) = old.remove(task_id) else {
            let Some(fresh) = read(db, id)? else { return Ok(false) };
            *snapshot = fresh;
            return Ok(true);
        };
        for (_, ord, text) in attempts.iter().filter(|(task, _, _)| task == task_id) {
            let attempt: Attempt = serde_json::from_str(text).context("Invalid attempt row")?;
            if *ord < task.attempts.len() {
                task.attempts[*ord] = attempt;
            } else if *ord == task.attempts.len() {
                task.attempts.push(attempt);
            } else {
                let Some(fresh) = read(db, id)? else { return Ok(false) };
                *snapshot = fresh;
                return Ok(true);
            }
        }
        task.attempts.truncate(counts.get(task_id).copied().unwrap_or(0));
        rebuilt.push(task);
    }
    for gone in old.keys() {
        snapshot.tasks.remove(gone);
    }
    snapshot.team.tasks = rebuilt;
    snapshot.team.revision = head.revision;
    snapshot.team.updated = head.updated;
    snapshot.revision = head.revision;
    Ok(true)
}

/// The team as read by a list query (its row's columns), assembled from its
/// rows in layout 2.
pub(super) fn decode(db: &Connection, id: &str, revision: i64, updated: f64, layout: Option<i64>, payload: &str) -> anyhow::Result<Team> {
    if !is_rows(layout) {
        let mut team: Team = serde_json::from_str(payload).context("Invalid team record")?;
        team.revision = revision as u64;
        return Ok(team);
    }
    match read(db, id)? {
        Some(snapshot) => Ok(snapshot.team),
        None => {
            let mut team = parse_head(payload)?;
            team.revision = revision as u64;
            team.updated = updated;
            Ok(team)
        }
    }
}

fn upsert_task(tx: &Connection, team: &str, task: u32, ord: usize, rev: u64, text: &str) -> rusqlite::Result<usize> {
    tx.execute(
        "INSERT INTO team_tasks(team,task,ord,rev,payload) VALUES(?1,?2,?3,?4,?5)
         ON CONFLICT(team,task) DO UPDATE SET ord=excluded.ord,rev=excluded.rev,payload=excluded.payload",
        params![team, task as i64, ord as i64, rev as i64, text],
    )
}

fn upsert_attempt(tx: &Connection, team: &str, task: u32, ord: usize, attempt: &Attempt, rev: u64) -> anyhow::Result<()> {
    tx.execute(
        "INSERT INTO team_attempts(team,task,ord,attempt,rev,payload) VALUES(?1,?2,?3,?4,?5,?6)
         ON CONFLICT(team,task,ord) DO UPDATE SET attempt=excluded.attempt,rev=excluded.rev,payload=excluded.payload",
        params![team, task as i64, ord as i64, attempt.id, rev as i64, serde_json::to_string(attempt)?],
    )?;
    Ok(())
}

fn write_head(tx: &Connection, team: &Team, head: &str, layout: i64) -> anyhow::Result<()> {
    let status = serde_json::to_value(team.status)?;
    tx.execute(
        "INSERT INTO teams(id,revision,status,updated,payload,members,members_rev,layout,head_rev) VALUES(?1,?2,?3,?4,?5,?6,?2,?7,?2)
         ON CONFLICT(id) DO UPDATE SET revision=excluded.revision,status=excluded.status,updated=excluded.updated,payload=excluded.payload,
             members=excluded.members,members_rev=excluded.members_rev,layout=excluded.layout,head_rev=excluded.head_rev",
        params![team.id, team.revision as i64, status.as_str().unwrap_or_default(), team.updated, head, serde_json::to_string(&team.members)?, layout],
    )?;
    Ok(())
}

/// Writes `team` (its revision already the new one) over `base`, the copy it
/// was changed from: the rows that changed, or every row when there is no
/// copy of layout 2. A team whose task ids repeat keeps the whole record.
pub(crate) fn save(tx: &Connection, base: Option<&Snapshot>, team: &mut Team) -> anyhow::Result<Saved> {
    let id = team.id.clone();
    let rev = team.revision;
    if !keyable(team) || legacy() {
        // The whole record carries the lead changes' snapshots again.
        fill_history(tx, team)?;
        let payload = serde_json::to_string(&*team)?;
        let status = serde_json::to_value(team.status)?;
        tx.execute(
            "INSERT INTO teams(id,revision,status,updated,payload,members,members_rev,layout,head_rev) VALUES(?1,?2,?3,?4,?5,?6,?2,?7,?2)
             ON CONFLICT(id) DO UPDATE SET revision=excluded.revision,status=excluded.status,updated=excluded.updated,payload=excluded.payload,
                 members=excluded.members,members_rev=excluded.members_rev,layout=excluded.layout,head_rev=excluded.head_rev",
            params![id, rev as i64, status.as_str().unwrap_or_default(), team.updated, payload, serde_json::to_string(&team.members)?,
                if keyable(team) { None } else { Some(JSON) }],
        )?;
        tx.execute("DELETE FROM team_tasks WHERE team=?1", params![id])?;
        tx.execute("DELETE FROM team_attempts WHERE team=?1", params![id])?;
        tx.execute("DELETE FROM team_lead_changes WHERE team=?1", params![id])?;
        return Ok(Saved { layout: if keyable(team) { 0 } else { JSON }, head: None, tasks: Vec::new(), removed: Vec::new(), full: true });
    }
    let base = base.filter(|base| base.layout == ROWS);
    save_lead_changes(tx, &id, rev, base.map(|base| base.team.lead_changes.as_slice()), team)?;
    let head = head_text(team)?;
    let Some(base) = base else {
        // Every row: a new team, a migration, a change of layout.
        write_head(tx, team, &head, ROWS)?;
        tx.execute("DELETE FROM team_tasks WHERE team=?1", params![id])?;
        tx.execute("DELETE FROM team_attempts WHERE team=?1", params![id])?;
        let mut tasks = Vec::with_capacity(team.tasks.len());
        for (ord, task) in team.tasks.iter_mut().enumerate() {
            let text = task_text(task)?;
            upsert_task(tx, &id, task.id, ord, rev, &text)?;
            for (at, attempt) in task.attempts.iter().enumerate() {
                upsert_attempt(tx, &id, task.id, at, attempt, rev)?;
            }
            tasks.push((task.id, text));
        }
        return Ok(Saved { layout: ROWS, head: Some(head), tasks, removed: Vec::new(), full: true });
    };
    let head_changed = head != base.head;
    if head_changed {
        write_head(tx, team, &head, ROWS)?;
    } else {
        let status = serde_json::to_value(team.status)?;
        tx.execute(
            "UPDATE teams SET revision=?2,members_rev=?2,updated=?3,status=?4 WHERE id=?1",
            params![id, rev as i64, team.updated, status.as_str().unwrap_or_default()],
        )?;
    }
    let index: HashMap<u32, usize> = base.team.tasks.iter().enumerate().map(|(at, task)| (task.id, at)).collect();
    let mut tasks = Vec::new();
    for (ord, task) in team.tasks.iter_mut().enumerate() {
        match index.get(&task.id) {
            Some(&at) => {
                let old = &base.team.tasks[at];
                if old != task {
                    let text = task_text(task)?;
                    if base.tasks.get(&task.id) != Some(&text) || at != ord {
                        upsert_task(tx, &id, task.id, ord, rev, &text)?;
                        tasks.push((task.id, text));
                    }
                    for (number, attempt) in task.attempts.iter().enumerate() {
                        if old.attempts.get(number) != Some(attempt) {
                            upsert_attempt(tx, &id, task.id, number, attempt, rev)?;
                        }
                    }
                    if old.attempts.len() > task.attempts.len() {
                        tx.execute(
                            "DELETE FROM team_attempts WHERE team=?1 AND task=?2 AND ord>=?3",
                            params![id, task.id as i64, task.attempts.len() as i64],
                        )?;
                    }
                } else if at != ord {
                    // Moved in the plan only: its place, not its text.
                    tx.execute("UPDATE team_tasks SET ord=?3,rev=?4 WHERE team=?1 AND task=?2", params![id, task.id as i64, ord as i64, rev as i64])?;
                }
            }
            None => {
                let text = task_text(task)?;
                upsert_task(tx, &id, task.id, ord, rev, &text)?;
                for (number, attempt) in task.attempts.iter().enumerate() {
                    upsert_attempt(tx, &id, task.id, number, attempt, rev)?;
                }
                tasks.push((task.id, text));
            }
        }
    }
    let kept: std::collections::HashSet<u32> = team.tasks.iter().map(|task| task.id).collect();
    let mut removed = Vec::new();
    for old in &base.team.tasks {
        if !kept.contains(&old.id) {
            tx.execute("DELETE FROM team_tasks WHERE team=?1 AND task=?2", params![id, old.id as i64])?;
            tx.execute("DELETE FROM team_attempts WHERE team=?1 AND task=?2", params![id, old.id as i64])?;
            removed.push(old.id);
        }
    }
    Ok(Saved { layout: ROWS, head: head_changed.then_some(head), tasks, removed, full: false })
}

/// Task #4 (team ede724c4): the lead changes' snapshots go to their own rows
/// and leave `team`, so the head keeps only the summary fields and the ids.
/// A change's row is written once, when it is recorded (it is not in `base`,
/// the copy the change was made from, or there is no copy); a change that left
/// the list (trimmed) loses its row. A change without snapshots in memory
/// keeps its stored row.
fn save_lead_changes(tx: &Connection, id: &str, rev: u64, base: Option<&[LeadChange]>, team: &mut Team) -> anyhow::Result<()> {
    let kept: std::collections::HashSet<&str> = team.lead_changes.iter().map(|change| change.id.as_str()).collect();
    let gone: Vec<String> = match base {
        Some(base) => base.iter().filter(|old| !kept.contains(old.id.as_str())).map(|old| old.id.clone()).collect(),
        None => tx
            .prepare("SELECT id FROM team_lead_changes WHERE team=?1")?
            .query_map(params![id], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?
            .into_iter()
            .filter(|stored| !kept.contains(stored.as_str()))
            .collect(),
    };
    for change in &gone {
        tx.execute("DELETE FROM team_lead_changes WHERE team=?1 AND id=?2", params![id, change])?;
    }
    let known: std::collections::HashSet<String> = base.unwrap_or_default().iter().map(|old| old.id.clone()).collect();
    for change in &mut team.lead_changes {
        let new = base.is_none() || !known.contains(&change.id);
        if change.tasks_before.is_empty() && change.phases_before.is_empty() {
            if new && base.is_some() {
                // A new change of this id: no row of an older one stays.
                tx.execute("DELETE FROM team_lead_changes WHERE team=?1 AND id=?2", params![id, change.id])?;
            }
            continue;
        }
        change.changed = change.tasks_before.iter().map(|task| task.id).collect();
        change.phases_changed = change.phases_before.iter().map(|phase| phase.n).collect();
        let history = History { tasks_before: std::mem::take(&mut change.tasks_before), phases_before: std::mem::take(&mut change.phases_before) };
        if new {
            tx.execute(
                "INSERT INTO team_lead_changes(team,id,rev,payload) VALUES(?1,?2,?3,?4)
                 ON CONFLICT(team,id) DO UPDATE SET rev=excluded.rev,payload=excluded.payload",
                params![id, change.id, rev as i64, serde_json::to_string(&history)?],
            )?;
        }
    }
    Ok(())
}

/// One lead change's snapshots, read on demand (a revert); None when the
/// store holds none for it.
pub(crate) fn lead_change_history(db: &Connection, team: &str, change: &str) -> anyhow::Result<Option<History>> {
    let text: Option<String> =
        db.query_row("SELECT payload FROM team_lead_changes WHERE team=?1 AND id=?2", params![team, change], |row| row.get(0)).optional()?;
    text.map(|text| serde_json::from_str(&text).context("Invalid lead change row")).transpose()
}

/// Puts the stored snapshots back into every lead change that lacks them, for
/// a write of the whole record.
fn fill_history(db: &Connection, team: &mut Team) -> anyhow::Result<()> {
    if !team.lead_changes.iter().any(LeadChange::history_missing) {
        return Ok(());
    }
    let rows: HashMap<String, String> = db
        .prepare("SELECT id,payload FROM team_lead_changes WHERE team=?1")?
        .query_map(params![team.id], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for change in team.lead_changes.iter_mut().filter(|change| change.history_missing()) {
        if let Some(text) = rows.get(&change.id) {
            let history: History = serde_json::from_str(text).context("Invalid lead change row")?;
            change.tasks_before = history.tasks_before;
            change.phases_before = history.phases_before;
        }
    }
    Ok(())
}

/// The copy after a committed write: `team` as written, with the texts.
pub(crate) fn after(base: Option<Snapshot>, team: Team, saved: Saved) -> Snapshot {
    match base.filter(|base| !saved.full && base.layout == ROWS) {
        Some(mut snapshot) => {
            if let Some(head) = saved.head {
                snapshot.head = head;
            }
            for (task, text) in saved.tasks {
                snapshot.tasks.insert(task, text);
            }
            for task in saved.removed {
                snapshot.tasks.remove(&task);
            }
            snapshot.revision = team.revision;
            snapshot.team = team;
            snapshot
        }
        None => Snapshot {
            revision: team.revision,
            layout: saved.layout,
            head: saved.head.unwrap_or_default(),
            tasks: saved.tasks.into_iter().collect(),
            team,
        },
    }
}

/// Migrates every team not yet migrated, one transaction each: a crash leaves
/// each team either migrated or as it was, and the next open goes on.
pub(super) fn migrate(db: &Connection) -> anyhow::Result<usize> {
    if legacy() {
        return Ok(0);
    }
    let ids: Vec<String> = db
        .prepare("SELECT id FROM teams WHERE layout IS NULL OR layout=?1")?
        .query_map(params![TASK_ROWS], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let mut migrated = 0;
    for id in ids {
        // Task #4 (team ede724c4): a team that cannot migrate now stays as it
        // was (this hgs reads it) and the next write open tries again; it
        // never fails the open, so nothing blocks creating a team.
        if let Ok(true) = migrate_one(db, &id) {
            migrated += 1;
        }
    }
    Ok(migrated)
}

/// One team's migration in its own transaction; whether it moved.
#[track_caller]
fn migrate_one(db: &Connection, id: &str) -> anyhow::Result<bool> {
    let write = begin(db, std::panic::Location::caller(), id)?;
    let Some(head) = head_row(&write, id, true)? else { return Ok(false) };
    if head.layout == Some(TASK_ROWS) {
        // Task #4: only the head and the lead changes' rows are written; the
        // task and attempt rows stay as they are.
        let Some(snapshot) = read(&write, id)? else { return Ok(false) };
        let mut team = snapshot.team;
        save_lead_changes(&write, id, team.revision, None, &mut team)?;
        let text = head_text(&mut team)?;
        write_head(&write, &team, &text, ROWS)?;
        commit(write)?;
        return Ok(true);
    }
    if head.layout.is_some() {
        return Ok(false);
    }
    let Ok(mut team) = serde_json::from_str::<Team>(&head.payload) else {
        // A record this hgs cannot read stays as it is, for the hgs that wrote it.
        write.execute("UPDATE teams SET layout=?2 WHERE id=?1", params![id, JSON])?;
        commit(write)?;
        return Ok(false);
    };
    team.revision = head.revision;
    team.updated = head.updated;
    let moved = keyable(&team);
    if moved {
        save(&write, None, &mut team)?;
    } else {
        write.execute("UPDATE teams SET layout=?2 WHERE id=?1", params![id, JSON])?;
    }
    commit(write)?;
    Ok(moved)
}

#[cfg(test)]
thread_local! {
    /// Tests: the whole-record layout, as before task 288 (the measurement's
    /// "before").
    static LEGACY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
pub(crate) fn set_legacy(on: bool) {
    LEGACY.with(|legacy| legacy.set(on));
}

pub(super) fn legacy() -> bool {
    #[cfg(test)]
    if LEGACY.with(std::cell::Cell::get) {
        return true;
    }
    false
}
