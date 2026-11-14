//! Service reads for the lead's doorbell, the timers and the cleanup after a
//! stop. Each read is one bounded page over live rows through an index; none
//! reads settled history or a whole team.
use super::*;
use rusqlite::params;
use zerus_team_plan::model::{MemberState, SessionKey, TaskRef};

const TERMINAL: &str = "('\"stopped\"','\"archived\"')";

/// Rows in one page of a team's status.
pub const STATUS_PAGE: u32 = 20;
/// The count of the rest stops here and reads as "1000 or more".
pub const STATUS_COUNT_CAP: u32 = 1000;
/// One page of a team's status list and how many more rows follow it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct StatusPage<T> {
    pub items: Vec<T>,
    /// Rows after this page, counted up to [`STATUS_COUNT_CAP`].
    pub more: u32,
}
impl<T> Default for StatusPage<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            more: 0,
        }
    }
}
impl<T> StatusPage<T> {
    fn of(items: Vec<T>, counted: u32) -> Self {
        let more = counted.saturating_sub(items.len() as u32);
        Self { items, more }
    }
}
/// A task key the team-clear child refused: final for the service.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct KeyRefusal {
    pub node: u32,
    pub reason: String,
    pub at_ms: i64,
}
/// A cancel the team-cancel child refused.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CancelRefusal {
    pub node: u32,
    pub decision: u64,
    pub reason: String,
}

/// One open inbox item, as a timer reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenItem {
    pub seq: u64,
    pub node: u32,
    pub generation: u32,
    pub created_ms: i64,
}
/// The lead's inbox as the doorbell counts it; `open` stops at 201.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InboxView {
    pub open: u32,
    pub newest: u64,
    pub oldest: Option<OpenItem>,
}
/// A delivered attempt of a live node, with the outcome time when known.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attempt {
    pub node: u32,
    pub generation: u32,
    pub member: String,
    pub outcome_ms: i64,
}
/// A report item that still waits for the lead, and which reminders fired.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Waiting {
    pub item: OpenItem,
    pub lead_reminded: bool,
    pub person_reminded: bool,
}
/// The task of a terminal team's node whose key has no final record yet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyedTask {
    pub node: u32,
    pub task: TaskRef,
}
/// The exact binding a doorbell was reserved for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bell {
    pub team: String,
    pub member: String,
    pub session: SessionKey,
    /// The mark of the previous doorbell of this binding.
    pub after: u64,
}

/// An archived stage `a` is released when its creation is no longer live,
/// and has a key to clear when a task was bound or its creation was saved.
pub(crate) const RELEASED: &str = "json_extract(a.data,'$.kind')='stage' AND NOT EXISTS(SELECT 1 FROM stage_tasks s WHERE s.team=a.team AND s.node=a.id AND s.state IN ('wanted','reserved','created')) AND (EXISTS(SELECT 1 FROM node_identity i WHERE i.team=a.team AND i.id=a.id AND i.task_uuid<>'') OR EXISTS(SELECT 1 FROM stage_tasks s WHERE s.team=a.team AND s.node=a.id AND s.request_id IS NOT NULL)) AND NOT EXISTS(SELECT 1 FROM key_clears k WHERE k.team=a.team AND k.node=a.id)";
fn column<T: rusqlite::types::FromSql>(
    db: &Connection,
    sql: &str,
    args: impl rusqlite::Params,
) -> Result<Vec<T>> {
    let mut s = db.prepare(sql).map_err(storage)?;
    let rows = s.query_map(args, |r| r.get(0)).map_err(storage)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(storage)
}
fn rows<T>(
    db: &Connection,
    sql: &str,
    args: impl rusqlite::Params,
    map: impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
) -> Result<Vec<T>> {
    let mut s = db.prepare(sql).map_err(storage)?;
    let rows = s.query_map(args, map).map_err(storage)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(storage)
}
fn item(r: &rusqlite::Row<'_>) -> rusqlite::Result<OpenItem> {
    Ok(OpenItem {
        seq: r.get(0)?,
        node: r.get(1)?,
        generation: r.get(2)?,
        created_ms: r.get(3)?,
    })
}
fn limit(value: u32) -> Result<u32> {
    if (1..=MAX_PAGE).contains(&value) {
        Ok(value)
    } else {
        Err(Problem::new(
            code::OVER_CAP,
            "Page limit must be between 1 and 200.",
        ))
    }
}

/// The reads, named so that their query plans are checked by tests.
pub(crate) mod sql {
    pub const NEWEST: &str =
        "SELECT seq FROM inbox WHERE team=?1 AND state='open' ORDER BY seq DESC LIMIT 1";
    pub const OPEN: &str =
        "SELECT count(*) FROM (SELECT 1 FROM inbox WHERE team=?1 AND state='open' LIMIT 201)";
    pub const OLDEST: &str = "SELECT seq,node,generation,created_ms FROM inbox WHERE team=?1 AND state='open' ORDER BY seq LIMIT 1";
    pub const UNREPORTED: &str = "SELECT n.id,n.generation,e.member,COALESCE(o.at_ms,e.created_ms) FROM nodes n JOIN effects e ON e.team=n.team AND e.node=n.id AND e.generation=n.generation AND e.state='submitted' LEFT JOIN effect_outcomes o ON o.team=e.team AND o.request_id=e.request_id WHERE n.team=?1 AND n.node_state='in_progress' AND n.id>?2 AND NOT EXISTS(SELECT 1 FROM timer_firings f WHERE f.team=n.team AND f.kind='no_report' AND f.node=n.id AND f.generation=n.generation AND f.item=0) ORDER BY n.id LIMIT ?3";
    pub const UNSTARTED: &str = "SELECT n.id,n.generation,e.member,o.at_ms FROM nodes n JOIN effects e ON e.team=n.team AND e.node=n.id AND e.generation=n.generation AND e.state='submitted' JOIN effect_outcomes o ON o.team=e.team AND o.request_id=e.request_id WHERE n.team=?1 AND n.node_state='pending' AND n.id>?2 AND NOT EXISTS(SELECT 1 FROM timer_firings f WHERE f.team=n.team AND f.kind='not_started' AND f.node=n.id AND f.generation=n.generation AND f.item=0) ORDER BY n.id LIMIT ?3";
    pub const UNDECIDED: &str = "SELECT i.seq,i.node,i.generation,i.created_ms,EXISTS(SELECT 1 FROM timer_firings f WHERE f.team=i.team AND f.kind='remind_lead' AND f.node=i.node AND f.generation=i.generation AND f.item=i.seq),EXISTS(SELECT 1 FROM timer_firings f WHERE f.team=i.team AND f.kind='remind_person' AND f.node=i.node AND f.generation=i.generation AND f.item=i.seq) FROM inbox i WHERE i.team=?1 AND i.state='open' AND i.seq>?2 AND i.kind='report' AND NOT EXISTS(SELECT 1 FROM decisions d WHERE d.team=i.team AND d.node=i.node AND d.generation=i.generation) ORDER BY i.seq LIMIT ?3";
    pub const FIRED: &str = "SELECT EXISTS(SELECT 1 FROM timer_firings WHERE team=?1 AND kind=?2 AND node=?3 AND generation=?4 AND item=?5)";
    pub const KEYED_TEAMS: &str =
        "SELECT team FROM key_clear_teams WHERE pending=1 AND team>?1 ORDER BY team LIMIT ?2";
    /// The work frontier of a team, in node order: each row names the bound
    /// task, or the task its saved stage creation names.
    pub const KEYED_PAGE: &str = "SELECT w.node,i.task_project,CASE WHEN i.task_uuid<>'' THEN i.task_uuid ELSE s.request_id END FROM key_clear_work w JOIN node_identity i ON i.team=w.team AND i.id=w.node LEFT JOIN stage_tasks s ON s.team=w.team AND s.node=w.node WHERE w.team=?1 AND w.node>?2 ORDER BY w.node LIMIT ?3";
    /// A team's closed tasks by identity, deduplicated, in keyset order
    /// (zerus-63rru.32): accepted or cancelled nodes still in `nodes`, and
    /// every archived node.
    pub const CLOSED_TASKS: &str = "SELECT DISTINCT i.task_project,i.task_uuid FROM node_identity i WHERE i.team=?1 AND (i.task_project>?2 OR (i.task_project=?2 AND i.task_uuid>?3)) AND i.task_uuid<>'' AND (EXISTS(SELECT 1 FROM nodes n WHERE n.team=i.team AND n.id=i.id AND (n.task_cancelled=1 OR n.task_stage='done')) OR EXISTS(SELECT 1 FROM node_archive a WHERE a.team=i.team AND a.id=i.id)) ORDER BY i.task_project,i.task_uuid LIMIT ?4";
    /// A team leaves the pending set when its frontier is empty and no cancel
    /// waits; both are prefix searches of what is still open.
    pub const SETTLE: &str = "UPDATE key_clear_teams SET pending=0 WHERE team=?1 AND pending=1 AND NOT EXISTS(SELECT 1 FROM key_clear_work w WHERE w.team=?1) AND NOT EXISTS(SELECT 1 FROM decisions d WHERE d.team=?1 AND d.state<>'settled' AND d.kind='cancel')";
    /// Whether a node waits in the frontier: the admission of its record.
    pub const WAITING_KEY: &str =
        "SELECT EXISTS(SELECT 1 FROM key_clear_work WHERE team=?1 AND node=?2)";
    /// Released stages pending a final key record, from the frontier only.
    pub const RELEASED_STAGES: &str = "SELECT w.node,i.task_project,CASE WHEN i.task_uuid<>'' THEN i.task_uuid ELSE s.request_id END FROM key_clear_work w JOIN node_identity i ON i.team=w.team AND i.id=w.node LEFT JOIN stage_tasks s ON s.team=w.team AND s.node=w.node WHERE w.team=?1 AND w.kind='stage' AND w.node>?2 ORDER BY w.node LIMIT ?3";
    pub const LATE: &str =
        "SELECT card FROM late_sessions WHERE team=?1 AND member=?2 AND launch=?3";
    /// A refused request is proved only by its saved receipt.
    pub const SAVED_REFUSAL: &str = "SELECT EXISTS(SELECT 1 FROM requests WHERE id=?1 AND team=?2 AND json_type(result,'$.Refused') IS NOT NULL)";
    pub const REFUSALS: &str = "SELECT node,reason FROM key_clears WHERE team=?1 AND state='refused' AND node>?2 ORDER BY node LIMIT ?3";
    /// A team's key refusals for its status, newest first.
    pub const REFUSALS_NEWEST: &str = "SELECT node,reason,at_ms FROM key_clears INDEXED BY key_refusals_newest WHERE team=?1 AND state='refused' ORDER BY at_ms DESC,node DESC LIMIT ?2";
    /// How many key refusals a team has, counted up to the cap.
    pub const REFUSALS_COUNT: &str = "SELECT COUNT(*) FROM (SELECT 1 FROM key_clears INDEXED BY key_refusals WHERE team=?1 AND state='refused' LIMIT ?2)";
    pub const WAITING: &str =
        "SELECT EXISTS(SELECT 1 FROM inbox WHERE team=?1 AND state='open' AND seq>?2)";
}

impl Store {
    fn read<T: Default>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        self.active()?;
        self.db.as_ref().map_or(Ok(T::default()), f)
    }
    /// The open lead inbox: newest and oldest item and a count bounded at 201.
    pub fn inbox_view(&self, team: &str) -> Result<Option<InboxView>> {
        self.read(|db| {
            let newest = db.query_row(sql::NEWEST, [team], |r| r.get(0));
            let Some(newest) = newest.optional().map_err(storage)? else {
                return Ok(None);
            };
            let open = db
                .query_row(sql::OPEN, [team], |r| r.get(0))
                .map_err(storage)?;
            let oldest = db
                .query_row(sql::OLDEST, [team], item)
                .optional()
                .map_err(storage)?;
            Ok(Some(InboxView {
                open,
                newest,
                oldest,
            }))
        })
    }
    /// Delivered attempts of live in-progress nodes without a report item
    /// yet, in node order after `after`, with the delivery's time.
    pub fn unreported(&self, team: &str, after: u32, page: u32) -> Result<Vec<Attempt>> {
        let page = limit(page)?;
        self.read(|db| rows(db, sql::UNREPORTED, params![team, after, page], attempt))
    }
    /// Submitted hand-outs of live nodes whose task is still To do, with the
    /// stored outcome time; an outcome without a stored time is never listed.
    pub fn unstarted(&self, team: &str, after: u32, page: u32) -> Result<Vec<Attempt>> {
        let page = limit(page)?;
        self.read(|db| rows(db, sql::UNSTARTED, params![team, after, page], attempt))
    }
    /// Open report items with no decision in their generation, after `after`.
    pub fn undecided(&self, team: &str, after: u64, page: u32) -> Result<Vec<Waiting>> {
        let page = limit(page)?;
        self.read(|db| {
            rows(db, sql::UNDECIDED, params![team, after, page], |r| {
                Ok(Waiting {
                    item: item(r)?,
                    lead_reminded: r.get(4)?,
                    person_reminded: r.get(5)?,
                })
            })
        })
    }
    /// Whether one timer key already fired.
    pub fn timer_fired(
        &self,
        team: &str,
        kind: &str,
        node: u32,
        generation: u32,
        item: u64,
    ) -> Result<bool> {
        self.read(|db| {
            db.query_row(
                sql::FIRED,
                params![team, kind, node, generation, item],
                |r| r.get(0),
            )
            .map_err(storage)
        })
    }
    /// Terminal teams with a task key that has no final record, by ID.
    pub fn keyed_teams(&self, after: &str, page: u32) -> Result<Vec<String>> {
        let page = limit(page)?;
        self.read(|db| column(db, sql::KEYED_TEAMS, params![after, page]))
    }
    /// A team's closed tasks (accepted or cancelled nodes, and the archive),
    /// by the task identity in keyset order after `after`, one page
    /// (zerus-63rru.32). The same task appears once, however many closed
    /// nodes it has.
    pub fn closed_tasks(
        &self,
        team: &str,
        after: Option<&(String, String)>,
        page: u32,
    ) -> Result<Vec<TaskRef>> {
        let page = limit(page)?;
        let (project, uuid) = after
            .cloned()
            .unwrap_or_else(|| (String::new(), String::new()));
        self.read(|db| {
            rows(db, sql::CLOSED_TASKS, params![team, project, uuid, page], |r| {
                Ok(TaskRef {
                    project: r.get(0)?,
                    uuid: r.get(1)?,
                })
            })
        })
    }
    /// Whether the team's closed-key sweep is recorded finished
    /// (zerus-63rru.32 round 2); only a new stop or archive reopens it.
    pub fn sweep_finished(&self, team: &str) -> Result<bool> {
        self.read(|db| {
            db.query_row(
                "SELECT EXISTS(SELECT 1 FROM key_clear_finished WHERE team=?1)",
                params![team],
                |r| r.get(0),
            )
            .map_err(storage)
        })
    }
    /// Records the sweep finished (zerus-63rru.32 round 2). The teams store
    /// writes happen through the operations; this marker is a plain row.
    pub fn mark_sweep_finished(&self, team: &str) -> Result<()> {
        if let Some(db) = self.db.as_ref() {
            db.execute(
                "INSERT OR REPLACE INTO key_clear_finished(team) VALUES (?1)",
                params![team],
            )
            .map_err(storage)?;
        }
        Ok(())
    }
    /// A terminal team's nodes whose task key is neither released nor finally
    /// recorded, in node order after `after`.
    pub fn keyed_tasks(&self, team: &str, after: u32, page: u32) -> Result<Vec<KeyedTask>> {
        let page = limit(page)?;
        self.read(|db| rows(db, sql::KEYED_PAGE, params![team, after, page], keyed))
    }
    /// The team's released stages whose task key has no final record, in
    /// node order after `after`, each with the task that team-clear names.
    /// Read through the outbox; settled history is never walked.
    pub fn released_stages(&self, team: &str, limit: u32, after: u32) -> Result<Vec<KeyedTask>> {
        let page = self::limit(limit)?;
        self.read(|db| rows(db, sql::RELEASED_STAGES, params![team, after, page], keyed))
    }
    /// Leave the pending set once no node of the team lacks a final record.
    pub fn settle_keyed(&self, team: &str) -> Result<bool> {
        self.read(|db| settle(db, team))
    }
    /// The task keys the team-clear child refused, with their reasons, for
    /// the team's status: final for the service, kept for the person.
    pub fn key_refusals(&self, team: &str, after: u32, page: u32) -> Result<Vec<(u32, String)>> {
        let page = limit(page)?;
        self.read(|db| {
            rows(db, sql::REFUSALS, params![team, after, page], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
        })
    }
    /// The newest key refusals of `team` for its status, and how many more
    /// there are, counted up to [`STATUS_COUNT_CAP`].
    pub fn key_refusal_page(&self, team: &str) -> Result<StatusPage<KeyRefusal>> {
        self.read(|db| {
            let items = rows(db, sql::REFUSALS_NEWEST, params![team, STATUS_PAGE], |r| {
                Ok(KeyRefusal {
                    node: r.get(0)?,
                    reason: r.get(1)?,
                    at_ms: r.get(2)?,
                })
            })?;
            let counted: u32 = db
                .query_row(
                    sql::REFUSALS_COUNT,
                    params![team, STATUS_PAGE + STATUS_COUNT_CAP],
                    |r| r.get(0),
                )
                .map_err(storage)?;
            Ok(StatusPage::of(items, counted))
        })
    }
    /// Whether `request` of `team` was refused, by its saved receipt: an
    /// error code alone never proves it, because a failed write carries the
    /// same code.
    pub fn saved_refusal(&self, team: &str, request: &str) -> Result<bool> {
        self.read(|db| {
            db.query_row(sql::SAVED_REFUSAL, params![request, team], |r| r.get(0))
                .map_err(storage)
        })
    }
    /// The card a late launch's provenance opened, if it was recorded.
    pub fn late_session(&self, team: &str, member: &str, launch: &str) -> Result<Option<u64>> {
        self.read(|db| {
            db.query_row(sql::LATE, params![team, member, launch], |r| r.get(0))
                .optional()
                .map_err(storage)
        })
    }
}
/// A team leaves the pending set when no node lacks a final record.
pub(crate) fn settle(db: &Connection, team: &str) -> Result<bool> {
    Ok(db.execute(sql::SETTLE, [team]).map_err(storage)? == 1)
}
fn keyed(r: &rusqlite::Row<'_>) -> rusqlite::Result<KeyedTask> {
    let task = TaskRef {
        project: r.get(1)?,
        uuid: r.get(2)?,
    };
    Ok(KeyedTask {
        node: r.get(0)?,
        task,
    })
}
fn attempt(r: &rusqlite::Row<'_>) -> rusqlite::Result<Attempt> {
    Ok(Attempt {
        node: r.get(0)?,
        generation: r.get(1)?,
        member: r.get(2)?,
        outcome_ms: r.get(3)?,
    })
}

/// What a doorbell's admission reads at its spawn: the lead's inbox, or a
/// member's answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Gate {
    Lead(Bell),
    Answer(AnswerBell),
}
/// The admission of either doorbell, inside the transaction held across its
/// spawn.
pub(crate) fn admit(db: &Connection, gate: &Gate) -> Result<()> {
    match gate {
        Gate::Lead(bell) => admit_bell(db, bell),
        Gate::Answer(bell) => admit_answer(db, bell),
    }
}
/// The doorbell's admission, read inside the immediate transaction held
/// across its spawn: the team still running, the lead's binding still the one
/// reserved, and an item beyond the previous mark still open.
pub(crate) fn admit_bell(db: &Connection, bell: &Bell) -> Result<()> {
    let team = super::rows::team(db, &bell.team)?.ok_or_else(|| refuse("The team is gone."))?;
    if team.status != zerus_team_plan::model::TeamStatus::Running {
        return Err(refuse("The team is not running."));
    }
    let lead = super::rows::member(db, &team.id, &team.lead)?;
    if team.lead != bell.member
        || !lead.is_some_and(|m| {
            m.state == MemberState::Active && m.session.as_ref() == Some(&bell.session)
        })
    {
        return Err(refuse("The lead's binding changed."));
    }
    let waiting: bool = db
        .query_row(sql::WAITING, params![team.id, bell.after], |r| r.get(0))
        .map_err(storage)?;
    if !waiting {
        return Err(refuse("No item beyond the last doorbell is open."));
    }
    Ok(())
}
fn refuse(text: &str) -> Problem {
    Problem::new(code::INVALID_STATE, text)
}

mod answers;
pub(crate) use answers::admit_answer;
// The member's doorbell step, built on Linux and macOS only, names the mark.
#[cfg_attr(
    not(any(target_os = "linux", target_os = "macos")),
    allow(unused_imports)
)]
pub use answers::AnswerMark;
pub use answers::{AnswerBell, Questions};
mod cancels;
mod reviewed;
mod schema;
#[allow(unused_imports)] // The actor's cancel step (package 8) reads this.
pub use cancels::PendingCancel;
#[cfg(test)]
pub(super) use schema::{after_fence, EXTENSION};
pub(super) use schema::{as_is, extend, fence};
#[cfg(test)]
mod tests;
