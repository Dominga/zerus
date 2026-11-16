//! Rework keeps every publication and original task request. Only SendBack
//! activates this layout, atomically with its first held acceptance episode.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReworkRow {
    pub id: String,
    pub base_publication: String,
    pub comment: String,
    pub state: String,
    pub generation: Option<u64>,
    pub header: Option<Phase>,
    pub approval: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ReworkView {
    pub id: String,
    pub base_publication: String,
    pub comment: String,
    pub state: String,
    pub draft: Option<Box<drafts::PhaseDraft>>,
}

fn installed(db: &Connection) -> Result<bool> {
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='phase_reworks' AND type='table')",
        [],
        |r| r.get(0),
    )
    .map_err(storage)
}

pub(super) fn validate(db: &Connection) -> Result<()> {
    let version: u32 = db
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(storage)?;
    let present = installed(db)?;
    if version >= versions::CORE_REWORK {
        let objects: u32 = db.query_row("SELECT count(*) FROM sqlite_master WHERE name IN ('phase_reworks','phase_rework_current','phase_rework_open','phase_rework_identity','phase_publication_episodes')", [], |r| r.get(0)).map_err(storage)?;
        if !present || objects != 5 {
            return Err(storage(
                "The rework layout is incomplete; no team data was changed.",
            ));
        }
    } else if present {
        return Err(storage("Rework storage has no matching execution floor."));
    }
    Ok(())
}

pub(crate) fn activate(db: &Connection) -> Result<()> {
    if db.is_autocommit() {
        return Err(storage(
            "Rework activation requires the operation's writer transaction.",
        ));
    }
    execution_fence(db)?;
    if installed(db)? {
        return validate(db);
    }
    // Bound this one-time native table rewrite before changing the floor.
    // The SQL reads only blob lengths, never all publication bodies in RAM.
    let mut query=db.prepare("SELECT length(CAST(snapshot AS BLOB)) FROM phase_publications ORDER BY rowid LIMIT 65537").map_err(storage)?;
    let mut bytes = 0u64;
    for (count, row) in query
        .query_map([], |r| r.get::<_, u64>(0))
        .map_err(storage)?
        .enumerate()
    {
        bytes = bytes.saturating_add(row.map_err(storage)?);
        if count >= 65_536 || bytes > 256 * 1024 * 1024 {
            return Err(Problem::new(code::OVER_CAP,"The publication store exceeds the bounded rework migration; no phase gate was changed."));
        }
    }
    drop(query);
    raise_execution_floor(db, versions::CORE_REWORK)?;
    // Rebuild only this bounded publication parent. Deferred FK checks retain
    // the original child rows while the same IDs are inserted into its new
    // shape; no request JSON is decoded/re-encoded and no child is deleted.
    db.execute_batch(
        "PRAGMA defer_foreign_keys=ON;
         CREATE TEMP TABLE teams_rework_publications_backup AS SELECT * FROM phase_publications;
         DROP TABLE phase_publications;
         CREATE TABLE phase_publications (
          id TEXT PRIMARY KEY, team TEXT NOT NULL, phase INTEGER NOT NULL,
          generation INTEGER NOT NULL, snapshot TEXT NOT NULL, initial INTEGER NOT NULL,
          state TEXT NOT NULL CHECK(state IN ('reserved','published')), created_ms INTEGER NOT NULL,
          UNIQUE(team,phase,generation), FOREIGN KEY(team,phase) REFERENCES phases(team,n));
         INSERT INTO phase_publications SELECT * FROM teams_rework_publications_backup;
         DROP TABLE teams_rework_publications_backup;
         CREATE INDEX phase_publication_pending ON phase_publications(team,state,phase);
         CREATE INDEX phase_publication_episodes ON phase_publications(team,phase,generation DESC);",
    )
    .map_err(storage)?;
    db.execute_batch(include_str!("rework.sql"))
        .map_err(storage)?;
    validate(db)
}

pub(crate) fn latest(db: &Connection, team: &str, phase: u32) -> Result<Option<ReworkRow>> {
    if !installed(db)? {
        return Ok(None);
    }
    let row=db.query_row("SELECT id,base_publication,CASE WHEN length(CAST(comment AS BLOB))<=16384 THEN comment END,state,generation,CASE WHEN length(CAST(header AS BLOB))<=4096 THEN header END,approval FROM phase_reworks WHERE team=?1 AND phase=?2 ORDER BY seq DESC LIMIT 1", params![team,phase], |r| Ok(ReworkRow {
        id:r.get(0)?,base_publication:r.get(1)?,comment:r.get(2)?,state:r.get(3)?,generation:r.get(4)?,
        header:r.get::<_,Option<String>>(5)?.map(|v| serde_json::from_str(&v).map_err(|e|rusqlite::Error::FromSqlConversionFailure(5,rusqlite::types::Type::Text,Box::new(e)))).transpose()?,approval:r.get(6)?,
    })).optional().map_err(storage)?;
    if row.as_ref().is_some_and(|r| {
        (r.state == "drafting") != r.generation.is_none()
            || r.generation.is_none() != r.header.is_none()
            || matches!(r.state.as_str(), "publishing" | "published") != r.approval.is_some()
            || r.header.as_ref().is_some_and(|p| p.n != phase)
    }) {
        return Err(storage(
            "The saved correction header or generation is missing or conflicts with its episode.",
        ));
    }
    Ok(row)
}

pub(crate) fn open(db: &Connection, team: &str, phase: u32) -> Result<Option<ReworkRow>> {
    Ok(latest(db, team, phase)?.filter(|r| r.state != "published"))
}

pub(crate) fn view(
    db: &Connection,
    team: &str,
    row: ReworkRow,
    revision: u64,
    next_node: u32,
    active: bool,
) -> Result<ReworkView> {
    let draft = if row.state == "published" {
        None
    } else if let Some(phase) = &row.header {
        let publication = match row.approval.as_deref() {
            Some(id) => Some(publication::by_id(db, team, id)?.ok_or_else(|| {
                storage(
                    "The saved correction publication is missing; its draft is not substituted.",
                )
            })?),
            None => None,
        };
        let nodes = if let Some(p) = &publication {
            publication::snapshot(db, team, &p.id)?
        } else {
            drafts::nodes(db, team, phase.n)?
        };
        if nodes.is_empty() || row.generation.is_none() {
            return Err(storage("The correction proposal has no complete draft."));
        }
        Some(Box::new(drafts::PhaseDraft {
            phase: phase.clone(),
            revision,
            generation: row.generation,
            next_node,
            nodes,
            complete: true,
            editable: active && row.approval.is_none(),
            accept_available: false,
            publication,
            rejection: publication::rejection(db, team, phase.n)?,
            previous: publication::previous(db, team, phase.n)?,
            live: None,
            rework_available: false,
            rework: None,
        }))
    } else {
        None
    };
    Ok(ReworkView {
        id: row.id,
        base_publication: row.base_publication,
        comment: row.comment,
        state: row.state,
        draft,
    })
}
