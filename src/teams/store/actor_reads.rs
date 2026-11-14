//! Additive service reads. No effects, migrations or task document access.
use super::*;
mod admission;
// Read by the teams actor, which is built on Linux and macOS only.
#[cfg_attr(not(any(target_os = "linux", target_os = "macos")), allow(unused_imports))]
pub use admission::Admission;
#[cfg_attr(not(any(target_os = "linux", target_os = "macos")), allow(unused_imports))]
pub(crate) use admission::BUSY as ADMISSION_BUSY;
use crate::teams::ops::ReportNote;
use rusqlite::params;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub member: String,
    pub text: String,
    pub commit: String,
    /// The service's statements about the report, apart from its text. A row
    /// from before notes has none.
    pub notes: Vec<ReportNote>,
}
const REPORT: &str = "SELECT e.member,r.text,r.commit_id,r.notes FROM reports r JOIN effects e INDEXED BY effects_attempt ON e.team=r.team AND e.node=r.node AND e.generation=r.generation WHERE r.team=?1 AND r.node=?2 AND r.generation=?3 AND e.state NOT IN ('refused','superseded') ORDER BY e.seq DESC LIMIT 1";
const ACCEPTS:&str="SELECT seq,node,generation,actor,kind,text,state FROM decisions INDEXED BY decision_pending WHERE team=?1 AND state<>'settled' AND state='ready' AND kind='accept' AND node>?2 AND node<=?3 ORDER BY node LIMIT ?4";
const REOPENS: &str = "SELECT seq,node,generation,actor,kind,text,state FROM decisions INDEXED BY decision_pending WHERE team=?1 AND state<>'settled' AND state='pending' AND kind IN ('return','reassign') AND node>?2 AND node<=?3 ORDER BY node LIMIT ?4";
const OPEN: &str = "SELECT seq,node,member,generation,request_id,actor,state,outcome,created_ms,kind FROM effects INDEXED BY effects_pending WHERE team=?1 AND state IN ('reserved','submitted','uncertain','cutting_over') AND seq>?2 AND seq<=?3 ORDER BY seq LIMIT ?4";
impl Store {
    /// Whether this exact permit still admits a new write. Facts about an
    /// earlier admitted write have their separate store authority.
    pub fn ready_accept(
        &self,
        team: &str,
        node: u32,
        generation: u32,
        decision: u64,
    ) -> Result<bool> {
        self.active()?;
        self.db.as_ref().map_or(Ok(false),|db|db.query_row(
            "SELECT EXISTS(SELECT 1 FROM decisions WHERE seq=?1 AND team=?2 AND node=?3 AND generation=?4 AND kind='accept' AND state='ready')",
            params![decision,team,node,generation],|r|r.get(0)).map_err(storage))
    }
    /// Whether the effect is still the reservation a delivery may act on:
    /// reserved, with no outcome, as the spawn admission and the task
    /// writer's own permit require. A removal's hand-back supersedes it
    /// (zerus-63rru.77); a point read of the unique request ID.
    pub fn effect_reserved(&self, team: &str, id: &str) -> Result<bool> {
        self.active()?;
        self.db.as_ref().map_or(Ok(false),|db|db.query_row("SELECT EXISTS(SELECT 1 FROM effects WHERE request_id=?1 AND team=?2 AND state='reserved' AND outcome IS NULL)",params![id,team],|r|r.get(0)).map_err(storage))
    }
    /// The removed member whose hand-out of this exact generation its removal
    /// settled: the generation's newest effect, superseded, of a member now
    /// removed (zerus-63rru.77). A report it writes later is not the team's.
    pub fn removed_reporter(
        &self,
        team: &str,
        node: u32,
        generation: u32,
    ) -> Result<Option<String>> {
        self.active()?;
        let removed = encode(&MemberState::Removed)?;
        self.db.as_ref().map_or(Ok(None),|db|db.query_row("SELECT e.member FROM (SELECT member,state FROM effects INDEXED BY effects_attempt WHERE team=?1 AND node=?2 AND generation=?3 ORDER BY seq DESC LIMIT 1) e JOIN members m ON m.team=?1 AND m.id=e.member WHERE e.state='superseded' AND m.state=?4",params![team,node,generation,removed],|r|r.get(0)).optional().map_err(storage))
    }
    /// Whether the lead was already told of that later report, once per node
    /// and generation (the inbox's own unique key).
    pub fn late_report_told(&self, team: &str, node: u32, generation: u32) -> Result<bool> {
        self.active()?;
        self.db.as_ref().map_or(Ok(false),|db|db.query_row("SELECT EXISTS(SELECT 1 FROM inbox WHERE team=?1 AND kind='removed_report' AND node=?2 AND generation=?3)",params![team,node,generation],|r|r.get(0)).map_err(storage))
    }
    /// Only a saved refusal proves that this request was refused.
    pub fn request_refused(&self, team: &str, id: &str) -> Result<bool> {
        self.active()?;
        self.db.as_ref().map_or(Ok(false),|db|db.query_row("SELECT COALESCE(json_type(result,'$.Refused')='object',0) FROM requests WHERE id=?1 AND team=?2",params![id,team],|r|r.get(0)).optional().map_err(storage).map(|v|v.unwrap_or(false)))
    }
    /// The immutable report and its holder in that exact attempt generation.
    pub fn report(&self, team: &str, node: NodeId, generation: u32) -> Result<Option<Report>> {
        self.active()?;
        self.db.as_ref().map_or(Ok(None), |db| {
            db.query_row(REPORT, params![team, node, generation], |r| {
                Ok(Report {
                    member: r.get(0)?,
                    text: r.get(1)?,
                    commit: r.get(2)?,
                    notes: sql_json(r, 3)?,
                })
            })
            .optional()
            .map_err(storage)
        })
    }
    /// Bounded ranges of the existing effects_pending(team,state,seq) index.
    pub fn open_effects(
        &self,
        team: &str,
        limit: u32,
        cursor: Option<&Cursor>,
    ) -> Result<Page<EffectIntent>> {
        self.active()?;
        if !(1..=MAX_PAGE).contains(&limit) {
            return Err(Problem::new(code::OVER_CAP, "Invalid page limit"));
        }
        let scope = format!("{team}:open_effects");
        let c = if let Some(c) = cursor {
            if c.scope != scope || c.after > c.ceiling {
                return Err(storage("Cursor belongs to another list"));
            }
            c.clone()
        } else {
            Cursor {
                scope,
                after: 0,
                rank: String::new(),
                ceiling: self.db.as_ref().map_or(Ok(0), |db| {
                    db.query_row(
                        "SELECT COALESCE(MAX(seq),0) FROM effects WHERE team=?1",
                        [team],
                        |r| r.get(0),
                    )
                    .map_err(storage)
                })?,
            }
        };
        let mut rows = Vec::new();
        if let Some(db) = &self.db {
            let mut s = db.prepare(OPEN).map_err(storage)?;
            rows = s
                .query_map(params![team, c.after, c.ceiling, limit], |r| {
                    Ok(EffectIntent {
                        seq: r.get(0)?,
                        node: r.get(1)?,
                        member: r.get(2)?,
                        generation: r.get(3)?,
                        request_id: r.get(4)?,
                        actor: sql_json(r, 5)?,
                        state: r.get(6)?,
                        outcome: r
                            .get::<_, Option<String>>(7)?
                            .map(|s| serde_json::from_str(&s))
                            .transpose()
                            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
                        created_ms: r.get(8)?,
                        kind: r.get(9)?,
                    })
                })
                .map_err(storage)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(storage)?;
        }
        Ok(pages::finish(rows, c, limit, |r| (r.seq, String::new())))
    }
    pub fn pending_reopens(
        &self,
        team: &str,
        limit: u32,
        cursor: Option<&Cursor>,
    ) -> Result<Page<Decision>> {
        self.actor_decisions(team, limit, cursor, "pending_reopens", REOPENS)
    }
    /// Ready accepts remain discoverable after integration, and also when a
    /// stage or no-merge team needs finishing without an integration fact.
    pub fn pending_accepts(
        &self,
        team: &str,
        limit: u32,
        cursor: Option<&Cursor>,
    ) -> Result<Page<Decision>> {
        self.actor_decisions(team, limit, cursor, "pending_accepts", ACCEPTS)
    }
    fn actor_decisions(
        &self,
        team: &str,
        limit: u32,
        cursor: Option<&Cursor>,
        kind: &str,
        sql: &str,
    ) -> Result<Page<Decision>> {
        self.active()?;
        if !(1..=MAX_PAGE).contains(&limit) {
            return Err(Problem::new(code::OVER_CAP, "Invalid page limit"));
        }
        let scope = format!("{team}:{kind}");
        let c = if let Some(c) = cursor {
            if c.scope != scope || c.after > c.ceiling {
                return Err(storage("Cursor belongs to another list"));
            }
            c.clone()
        } else {
            Cursor {
                scope,
                after: 0,
                rank: String::new(),
                ceiling: self.db.as_ref().map_or(Ok(0), |db| {
                    db.query_row(
                        "SELECT COALESCE(MAX(id),0) FROM node_identity WHERE team=?1",
                        [team],
                        |r| r.get(0),
                    )
                    .map_err(storage)
                })?,
            }
        };
        let mut rows = Vec::new();
        if let Some(db) = &self.db {
            let mut s = db.prepare(sql).map_err(storage)?;
            rows = s
                .query_map(params![team, c.after, c.ceiling, limit], |r| {
                    Ok(Decision {
                        seq: r.get(0)?,
                        node: r.get(1)?,
                        generation: r.get(2)?,
                        actor: sql_json(r, 3)?,
                        kind: r.get(4)?,
                        text: r.get(5)?,
                        state: r.get(6)?,
                    })
                })
                .map_err(storage)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(storage)?;
        }
        Ok(pages::finish(rows, c, limit, |r| {
            (u64::from(r.node), String::new())
        }))
    }
    /// Changes for commits by another connection, not this connection's writes.
    /// Missing stores return None and are never created by this read.
    pub fn change_token(&self) -> Result<Option<u64>> {
        self.active()?;
        self.db
            .as_ref()
            .map(|db| {
                db.query_row("PRAGMA data_version", [], |r| r.get(0))
                    .map_err(storage)
            })
            .transpose()
    }
}
#[cfg(test)]
mod tests;
