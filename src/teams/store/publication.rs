//! Immutable approval inputs and per-task creation witnesses. No session effect
//! is permitted by a draft row or by a partially created publication.
use super::*;
use serde::Deserialize;
use zerus_team_plan::draft::DraftNode;

pub(crate) const PUBLICATION_LAYOUT: u32 = versions::CORE_PUBLICATION;

pub(super) fn extend(db: &Connection) -> Result<()> {
    let complete = |db: &Connection| -> Result<bool> {
        db.query_row("SELECT count(*)=11 FROM sqlite_master WHERE name IN ('phase_reviews','phase_publications','phase_publication_pending','phase_publication_nodes','phase_publication_work','phase_nodes_live','phase_nodes_archive','phase_draft_previous','phase_cards','phase_draft_dependencies','phase_draft_dependants')", [], |r| r.get(0)).map_err(storage)
    };
    if complete(db)? {
        return Ok(());
    }
    let own = db.is_autocommit();
    db.execute_batch(if own {
        "BEGIN IMMEDIATE"
    } else {
        "SAVEPOINT phase_publication"
    })
    .map_err(begin_error)?;
    let result = execution_fence(db)
        .and_then(|()| timers::fence(db))
        .and_then(|()| {
            db.execute_batch(include_str!("publication.sql"))
                .map_err(storage)
        });
    let end = match (&result, own) {
        (Ok(()), true) => "COMMIT",
        (Ok(()), false) => "RELEASE phase_publication",
        (Err(_), true) => "ROLLBACK",
        (Err(_), false) => "ROLLBACK TO phase_publication; RELEASE phase_publication",
    };
    db.execute_batch(end).map_err(storage)?;
    result
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlannedTaskRequest {
    pub publication: String,
    pub team: String,
    pub phase: u32,
    pub node: DraftNode,
    pub task: TaskRef,
    pub lead: SessionKey,
    pub home: String,
    pub team_name: String,
    pub text: String,
    pub created_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Publication {
    pub id: String,
    pub generation: u64,
    pub state: String,
    pub total: u32,
    pub created: u32,
    pub blocked: u32,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Rejection {
    pub id: String,
    pub generation: u64,
    pub comment: String,
}

pub(crate) fn publication(db: &Connection, team: &str, phase: u32) -> Result<Option<Publication>> {
    db.query_row("SELECT p.id,p.generation,p.state,count(n.node),COALESCE(sum(n.state='created'),0),COALESCE(sum(n.state IN ('refused','uncertain') OR n.reason<>''),0),(SELECT NULLIF(reason,'') FROM phase_publication_nodes WHERE publication=p.id AND reason<>'' ORDER BY node LIMIT 1) FROM phase_publications p LEFT JOIN phase_publication_nodes n ON n.publication=p.id WHERE p.team=?1 AND p.phase=?2 GROUP BY p.id",params![team,phase],|r| Ok(Publication { id:r.get(0)?,generation:r.get(1)?,state:r.get(2)?,total:r.get(3)?,created:r.get(4)?,blocked:r.get(5)?,reason:r.get(6)? })).optional().map_err(storage)
}

pub(crate) fn rejection(db: &Connection, team: &str, phase: u32) -> Result<Option<Rejection>> {
    db.query_row("SELECT rejection,generation,comment FROM phase_reviews WHERE team=?1 AND phase=?2 AND rejection IS NOT NULL",params![team,phase],|r| Ok(Rejection{id:r.get(0)?,generation:r.get(1)?,comment:r.get(2)?})).optional().map_err(storage)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PreviousDraft {
    pub generation: u64,
    pub draft: zerus_team_plan::draft::DraftInput,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PhaseCardRef {
    pub phase: u32,
    pub generation: u64,
    pub purpose: String,
}

pub(crate) fn previous(db: &Connection, team: &str, phase: u32) -> Result<Option<PreviousDraft>> {
    let size:Option<usize>=db.query_row("SELECT length(CAST(draft AS BLOB)) FROM phase_draft_previous WHERE team=?1 AND phase=?2",params![team,phase],|r|r.get(0)).optional().map_err(storage)?;
    if size.is_some_and(|n| n > 262_144) {
        return Err(Problem::new(
            code::OVER_CAP,
            "The previous draft exceeds its bound.",
        ));
    }
    db.query_row(
        "SELECT generation,draft FROM phase_draft_previous WHERE team=?1 AND phase=?2",
        params![team, phase],
        |r| {
            Ok(PreviousDraft {
                generation: r.get(0)?,
                draft: sql_json(r, 1)?,
            })
        },
    )
    .optional()
    .map_err(storage)
}

impl Store {
    /// A pass holds at most four exact saved requests. Uncertain/refused work
    /// is visible in the phase read and never becomes an automatic resend.
    pub fn phase_card_ref(&self, team: &str, card: u64) -> Result<Option<PhaseCardRef>> {
        self.active()?;
        self.db.as_ref().map_or(Ok(None), |db| {
            db.query_row(
                "SELECT phase,generation,purpose FROM phase_cards WHERE card=?1 AND team=?2",
                params![card, team],
                |r| {
                    Ok(PhaseCardRef {
                        phase: r.get(0)?,
                        generation: r.get(1)?,
                        purpose: r.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(storage)
        })
    }
    pub fn phase_review_generation(&self, team: &str, phase: u32) -> Result<Option<u64>> {
        self.active()?;
        self.db.as_ref().map_or(Ok(None),|db| db.query_row("SELECT generation FROM phase_cards WHERE team=?1 AND phase=?2 AND purpose='review'",params![team,phase],|r|r.get(0)).optional().map_err(storage))
    }
    pub fn publication_work(&self, team: &str) -> Result<Vec<PlannedTaskRequest>> {
        self.active()?;
        let Some(db) = &self.db else {
            return Ok(vec![]);
        };
        let mut stmt = db.prepare("SELECT CASE WHEN length(CAST(n.request AS BLOB))<=530000 THEN n.request ELSE NULL END FROM phase_publications p JOIN phase_publication_nodes n ON n.publication=p.id WHERE p.team=?1 AND p.state='reserved' AND n.cleared=0 AND (n.state IN ('pending','writing') OR (n.state='created' AND n.reason<>'')) ORDER BY p.phase,n.node LIMIT 4").map_err(storage)?;
        let answer = stmt
            .query_map([team], |r| sql_json(r, 0))
            .map_err(storage)?
            .collect::<rusqlite::Result<_>>()
            .map_err(storage);
        answer
    }
    pub fn publication_cleanup(&self, team: &str) -> Result<Vec<PlannedTaskRequest>> {
        self.active()?;
        let Some(db) = &self.db else {
            return Ok(vec![]);
        };
        let mut stmt = db.prepare("SELECT CASE WHEN length(CAST(n.request AS BLOB))<=530000 THEN n.request ELSE NULL END FROM phase_publications p JOIN phase_publication_nodes n ON n.publication=p.id WHERE p.team=?1 AND p.state='reserved' AND n.cleared=0 AND n.clear_outcome IS NULL ORDER BY p.phase,n.node LIMIT 4").map_err(storage)?;
        let rows = stmt
            .query_map([team], |r| sql_json(r, 0))
            .map_err(storage)?
            .collect::<rusqlite::Result<_>>()
            .map_err(storage);
        rows
    }
    pub fn planned_task(
        &self,
        team: &str,
        node: u32,
        id: &str,
    ) -> Result<Option<PlannedTaskRequest>> {
        self.active()?;
        self.db.as_ref().map_or(Ok(None), |db| db.query_row("SELECT CASE WHEN length(CAST(request AS BLOB))<=530000 THEN request ELSE NULL END FROM phase_publication_nodes WHERE team=?1 AND node=?2 AND task_uuid=?3",params![team,node,id],|r|sql_json(r,0)).optional().map_err(storage))
    }
    pub fn publications_pending(&self, team: &str) -> Result<Vec<(u32, String)>> {
        self.active()?;
        let Some(db) = &self.db else {
            return Ok(vec![]);
        };
        let mut stmt = db.prepare("SELECT phase,id FROM phase_publications WHERE team=?1 AND state='reserved' ORDER BY phase LIMIT 200").map_err(storage)?;
        let answer = stmt
            .query_map([team], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(storage)?
            .collect::<rusqlite::Result<_>>()
            .map_err(storage);
        answer
    }
}
