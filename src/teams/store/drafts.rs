//! Draft companions are additive: an older actor sees no live task to run.
use super::*;
use zerus_team_plan::draft::DraftNode;

pub(super) fn extend(db: &Connection) -> Result<()> {
    let present = |db: &Connection| -> Result<bool> {
        db.query_row("SELECT count(*)=4 FROM sqlite_master WHERE name IN ('phase_drafts','phase_draft_nodes','phase_draft_page','phase_draft_counters')", [], |r| r.get(0)).map_err(storage)
    };
    if present(db)? {
        return Ok(());
    }
    let own = db.is_autocommit();
    db.execute_batch(if own {
        "BEGIN IMMEDIATE;"
    } else {
        "SAVEPOINT phase_drafts;"
    })
    .map_err(begin_error)?;
    let result = timers::fence(db).and_then(|()| {
        if present(db)? {
            return Ok(());
        }
        db.execute_batch(include_str!("drafts.sql"))
            .map_err(storage)
    });
    let end = match (&result, own) {
        (Ok(()), true) => "COMMIT;",
        (Ok(()), false) => "RELEASE phase_drafts;",
        (Err(_), true) => "ROLLBACK;",
        (Err(_), false) => "ROLLBACK TO phase_drafts; RELEASE phase_drafts;",
    };
    db.execute_batch(end).map_err(storage)?;
    result
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PhaseDraft {
    pub phase: Phase,
    pub revision: u64,
    pub generation: Option<u64>,
    pub next_node: u32,
    pub nodes: Vec<DraftNode>,
    pub complete: bool,
    pub editable: bool,
    pub accept_available: bool,
    pub publication: Option<publication::Publication>,
    pub rejection: Option<publication::Rejection>,
    pub previous: Option<publication::PreviousDraft>,
    pub live: Option<phase_snapshot::PhaseNodeSnapshot>,
}

pub(crate) fn count(db: &Connection, team: &str) -> Result<u32> {
    db.query_row(
        "SELECT count(*) FROM phase_draft_nodes WHERE team=?1",
        [team],
        |r| r.get(0),
    )
    .map_err(storage)
}

pub(crate) fn next_phase(db: &Connection, team: &str) -> Result<u32> {
    db.query_row("SELECT COALESCE((SELECT next_phase FROM phase_draft_counters WHERE team=?1),(SELECT COALESCE(MAX(n),0)+1 FROM phases WHERE team=?1))", [team], |r| r.get(0)).map_err(storage)
}

pub(crate) fn nodes(db: &Connection, team: &str, phase: u32) -> Result<Vec<DraftNode>> {
    let (count,bytes):(u32,u64)=db.query_row("SELECT count(*),COALESCE(sum(length(CAST(data AS BLOB))),0) FROM phase_draft_nodes WHERE team=?1 AND phase=?2",params![team,phase],|r|Ok((r.get(0)?,r.get(1)?))).map_err(storage)?;
    if count > 200 || bytes > 262_144 {
        return Err(Problem::new(
            code::OVER_CAP,
            "The draft exceeds its row or byte bound.",
        ));
    }
    let mut stmt = db
        .prepare(
            "SELECT data FROM phase_draft_nodes WHERE team=?1 AND phase=?2 ORDER BY id LIMIT 201",
        )
        .map_err(storage)?;
    let out = stmt
        .query_map(params![team, phase], |r| sql_json(r, 0))
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage)?;
    if out.len() > 200 {
        return Err(Problem::new(
            code::OVER_CAP,
            "The draft exceeds its phase bound.",
        ));
    }
    Ok(out)
}

impl Store {
    /// A bounded complete draft, from one snapshot; the queue has no draft.
    pub fn phase_draft(&self, team: &str, phase: u32) -> Result<PhaseDraft> {
        self.active()?;
        let db = self
            .db
            .as_ref()
            .ok_or_else(|| Problem::new(code::NOT_FOUND, "Team does not exist."))?;
        let tx = db.unchecked_transaction().map_err(storage)?;
        let header = rows::team(&tx, team)?
            .ok_or_else(|| Problem::new(code::NOT_FOUND, "Team does not exist."))?;
        let record: Phase = tx
            .query_row(
                "SELECT data FROM phases WHERE team=?1 AND n=?2",
                params![team, phase],
                |r| sql_json(r, 0),
            )
            .optional()
            .map_err(storage)?
            .ok_or_else(|| Problem::new(code::NOT_FOUND, "Phase does not exist."))?;
        let generation: Option<u64> = tx
            .query_row(
                "SELECT generation FROM phase_drafts WHERE team=?1 AND phase=?2",
                params![team, phase],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?;
        let next_node = tx
            .query_row("SELECT next_node FROM teams WHERE id=?1", [team], |r| {
                r.get(0)
            })
            .map_err(storage)?;
        let mut nodes = nodes(&tx, team, phase)?;
        let publication = publication::publication(&tx, team, phase)?;
        if publication.is_some() {
            let bytes: usize = tx.query_row("SELECT length(CAST(snapshot AS BLOB)) FROM phase_publications WHERE team=?1 AND phase=?2",params![team,phase],|r|r.get(0)).map_err(storage)?;
            if bytes > 262_144 { return Err(Problem::new(code::OVER_CAP,"The saved publication exceeds its draft bound.")); }
            nodes = tx.query_row("SELECT snapshot FROM phase_publications WHERE team=?1 AND phase=?2",params![team,phase],|r|sql_json(r,0)).map_err(storage)?;
        }
        let rejection = publication::rejection(&tx, team, phase)?;
        let live = if publication.as_ref().is_some_and(|p| p.state == "published") {
            Some(phase_snapshot::load(&tx, team, phase, header.revision)?)
        } else { None };
        let editable = publication.is_none() && generation.is_some()
            && record.kind == PhaseKind::Plan
            && matches!(record.state, PhaseState::Draft | PhaseState::Rejected)
            && matches!(
                header.status,
                TeamStatus::Planning | TeamStatus::Running | TeamStatus::Paused
            );
        let accept_available = record.state == PhaseState::AwaitingAcceptance
            && matches!(header.status,TeamStatus::Running|TeamStatus::Paused)
            && crate::teams::ops::publication::complete(&tx,team,phase)?;
        let previous = publication::previous(&tx, team, phase)?;
        let complete = generation.is_some() && !nodes.is_empty();
        let answer = PhaseDraft {
            phase: record,
            revision: header.revision,
            generation,
            next_node,
            nodes,
            complete,
            editable,
            accept_available,
            publication,
            rejection,
            previous,
            live,
        };
        tx.commit().map_err(storage)?;
        Ok(answer)
    }
    pub fn next_draft_phase(&self, team: &str) -> Result<u32> {
        self.active()?;
        self.db.as_ref().map_or(Ok(1), |db| next_phase(db, team))
    }
    pub fn next_draft_node(&self, team: &str) -> Result<u32> {
        self.active()?;
        self.db
            .as_ref()
            .ok_or_else(|| Problem::new(code::NOT_FOUND, "Team does not exist."))?
            .query_row("SELECT next_node FROM teams WHERE id=?1", [team], |r| {
                r.get(0)
            })
            .map_err(storage)
    }
}
