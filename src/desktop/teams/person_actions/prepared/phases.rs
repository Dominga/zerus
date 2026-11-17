//! Separate bounded canonical references. Serialized hashes remain data;
//! only the live original can consume the resulting sealed facts digest.
use super::*;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use zerus_team_plan::draft::DraftNode;
mod controls;

enum Selection<'a> {
    Controls(&'a TeamAction),
    Draft {
        phase: u32,
        generation: Option<u64>,
    },
    Current {
        phase: u32,
        publication: &'a str,
    },
    Correction {
        phase: u32,
        base: &'a str,
        episode: &'a str,
        generation: Option<u64>,
    },
}

fn selection(action: &TeamAction) -> Option<Selection<'_>> {
    use TeamAction::*;
    // Deliberately exhaustive: adding a future control requires enrolling
    // its actual identities here, including any nested scope/relative/pause.
    Some(match action {
        SetPhaseBarrier { .. }
        | PausePhase { .. }
        | ResumePhase { .. }
        | MovePhase { .. }
        | SetPhaseAcceptance { .. } => Selection::Controls(action),
        SubmitDraftPhase { phase, .. } => Selection::Draft {
            phase: *phase,
            generation: None,
        },
        EditDraftPhase {
            phase, generation, ..
        }
        | ApprovePhase {
            phase, generation, ..
        }
        | RejectPhase {
            phase, generation, ..
        }
        | UndoRejectPhase {
            phase, generation, ..
        } => Selection::Draft {
            phase: *phase,
            generation: Some(*generation),
        },
        AcceptPhase { phase, publication }
        | SendBackPhase {
            phase, publication, ..
        } => Selection::Current {
            phase: *phase,
            publication,
        },
        SubmitPhaseCorrections {
            phase,
            publication,
            rework,
            ..
        } => Selection::Correction {
            phase: *phase,
            base: publication,
            episode: rework,
            generation: None,
        },
        EditPhaseCorrections {
            phase,
            publication,
            rework,
            generation,
            ..
        }
        | ApprovePhaseCorrections {
            phase,
            publication,
            rework,
            generation,
        }
        | RejectPhaseCorrections {
            phase,
            publication,
            rework,
            generation,
            ..
        }
        | UndoPhaseCorrectionRejection {
            phase,
            publication,
            rework,
            generation,
            ..
        } => Selection::Correction {
            phase: *phase,
            base: publication,
            episode: rework,
            generation: Some(*generation),
        },
        StartPlanning { .. }
        | Create { .. }
        | AddMember { .. }
        | SwapMember { .. }
        | RenameTeam { .. }
        | SetGoal { .. }
        | Pause { .. }
        | Resume { .. }
        | Stop { .. }
        | Archive { .. }
        | AdvanceBase { .. }
        | PauseMember { .. }
        | ResumeMember { .. }
        | StopMember { .. }
        | ArchiveMember { .. }
        | ValidationConfig { .. }
        | Settings { .. }
        | Decide { .. }
        | Admit { .. }
        | Route { .. }
        | SetDeps { .. }
        | Accept { .. }
        | Return { .. }
        | Reassign { .. }
        | Hold { .. }
        | Release { .. }
        | Cancel { .. }
        | Reply { .. }
        | Ask { .. } => return None,
    })
}

#[derive(Serialize)]
struct Reference {
    role: &'static str,
    key: Value,
    bytes: u64,
    sha256: String,
}

fn reference(role: &'static str, key: Value, bytes: &[u8]) -> Reference {
    Reference {
        role,
        key,
        bytes: bytes.len() as u64,
        sha256: format!("{:x}", Sha256::digest(bytes)),
    }
}
fn metadata(role: &'static str, key: Value, value: &impl Serialize) -> Result<Reference> {
    Ok(reference(
        role,
        key,
        &serde_json::to_vec(value).map_err(store::storage)?,
    ))
}
fn uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| !id.is_nil() && id.to_string() == value)
}
fn installed(db: &Connection, table: &str) -> Result<bool> {
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        [table],
        |r| r.get(0),
    )
    .map_err(store::storage)
}

#[derive(Serialize)]
struct Episode {
    seq: u64,
    id: String,
    base: String,
    state: String,
    generation: Option<u64>,
    header: Option<String>,
    approval: Option<String>,
    comment: String,
}
fn episode(db: &Connection, team: &str, phase: u32) -> Result<Option<Episode>> {
    if !installed(db, "phase_reworks")? {
        let floor: u32 = db
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(store::storage)?;
        return if floor < store::versions::CORE_REWORK {
            Ok(None)
        } else {
            Err(required())
        };
    }
    let result = db.query_row(
        "SELECT seq,CASE WHEN length(CAST(id AS BLOB))=36 THEN id END,CASE WHEN length(CAST(base_publication AS BLOB))=36 THEN base_publication END,CASE WHEN length(CAST(state AS BLOB))<=16 THEN state END,generation,CASE WHEN length(CAST(header AS BLOB))<=4096 THEN header END,CASE WHEN length(CAST(approval AS BLOB))=36 THEN approval END,CASE WHEN length(CAST(comment AS BLOB))<=16384 THEN comment END,length(CAST(header AS BLOB)),length(CAST(approval AS BLOB)) FROM phase_reworks WHERE team=?1 AND phase=?2 ORDER BY seq DESC LIMIT 1",
        params![team, phase], |r| {
            let header_bytes: Option<u64> = r.get(8)?;
            let approval_bytes: Option<u64> = r.get(9)?;
            if header_bytes.is_some_and(|n| n > 4096) || approval_bytes.is_some_and(|n| n != 36) {
                return Err(rusqlite::Error::InvalidQuery);
            }
            Ok(Episode { seq:r.get(0)?, id:r.get(1)?, base:r.get(2)?, state:r.get(3)?, generation:r.get(4)?, header:r.get(5)?, approval:r.get(6)?, comment:r.get(7)? })
        }).optional().map_err(store::storage)?;
    if let Some(row) = &result {
        if row.seq == 0
            || !uuid(&row.id)
            || !uuid(&row.base)
            || !matches!(
                row.state.as_str(),
                "drafting" | "review" | "rejected" | "publishing" | "published"
            )
            || (row.state == "drafting") != row.generation.is_none()
            || row.generation.is_none() != row.header.is_none()
            || row.generation == Some(0)
            || matches!(row.state.as_str(), "publishing" | "published") != row.approval.is_some()
            || row.approval.as_deref().is_some_and(|id| !uuid(id))
        {
            return Err(required());
        }
        if let Some(header) = &row.header {
            let header: zerus_team_plan::model::Phase =
                serde_json::from_str(header).map_err(store::storage)?;
            if header.n != phase {
                return Err(required());
            }
        }
    }
    Ok(result)
}

fn proposal(db: &Connection, team: &str, phase: u32, generation: Option<u64>) -> Result<Reference> {
    let (count, bytes): (u64, u64) = db.query_row("SELECT count(*),coalesce(sum(bytes),0) FROM (SELECT length(CAST(data AS BLOB)) AS bytes FROM phase_draft_nodes WHERE team=?1 AND phase=?2 ORDER BY id LIMIT 201)", params![team,phase], |r|Ok((r.get(0)?,r.get(1)?))).map_err(store::storage)?;
    if count > 200 || bytes > 262_144 {
        return Err(required());
    }
    let mut statement = db.prepare("SELECT id,data FROM phase_draft_nodes WHERE team=?1 AND phase=?2 ORDER BY id LIMIT 201").map_err(store::storage)?;
    let mut rows = statement
        .query(params![team, phase])
        .map_err(store::storage)?;
    let mut references = Vec::new();
    while let Some(row) = rows.next().map_err(store::storage)? {
        let id: u32 = row.get(0).map_err(store::storage)?;
        let body: String = row.get(1).map_err(store::storage)?;
        let node: DraftNode = serde_json::from_str(&body).map_err(store::storage)?;
        if id == 0 || node.id != id || references.len() >= 200 {
            return Err(required());
        }
        references.push(reference(
            "proposal_node",
            json!([team, phase, id, generation]),
            body.as_bytes(),
        ));
    }
    if references.len() as u64 != count {
        return Err(required());
    }
    metadata(
        "proposal",
        json!([team, phase, generation, count, bytes]),
        &references,
    )
}

fn current_publication(db: &Connection, team: &str, phase: u32) -> Result<Option<String>> {
    if !installed(db, "phase_publications")? {
        let floor: u32 = db
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(store::storage)?;
        return if floor < store::versions::CORE_PUBLICATION {
            Ok(None)
        } else {
            Err(required())
        };
    }
    db.query_row("SELECT CASE WHEN length(CAST(id AS BLOB))=36 THEN id END FROM phase_publications WHERE team=?1 AND phase=?2 ORDER BY generation DESC LIMIT 1", params![team,phase], |r|r.get(0)).optional().map_err(store::storage)
}

fn publication(
    db: &Connection,
    team: &str,
    phase: u32,
    id: &str,
    role: &'static str,
) -> Result<Value> {
    if !uuid(id) {
        return Err(required());
    }
    let header = point(
        db,
        "phase_publications",
        "team",
        team,
        Some(("id", &json!(id))),
        "id,phase,generation,state,initial,created_ms",
        1024,
    )?;
    let fields = header.as_array().ok_or_else(required)?;
    if fields.get(1).and_then(Value::as_u64) != Some(phase.into())
        || fields.get(2).and_then(Value::as_u64).is_none_or(|g| g == 0)
        || !matches!(
            fields.get(3).and_then(Value::as_str),
            Some("reserved" | "published")
        )
    {
        return Err(required());
    }
    let (bytes, body): (u64, Option<String>) = db.query_row("SELECT length(CAST(snapshot AS BLOB)),CASE WHEN length(CAST(snapshot AS BLOB))<=262144 THEN snapshot END FROM phase_publications WHERE team=?1 AND id=?2", params![team,id], |r|Ok((r.get(0)?,r.get(1)?))).map_err(store::storage)?;
    let body = body
        .filter(|b| b.len() as u64 == bytes)
        .ok_or_else(required)?;
    let nodes: Vec<DraftNode> = serde_json::from_str(&body).map_err(store::storage)?;
    if nodes.is_empty() || nodes.len() > 200 {
        return Err(required());
    }
    let by_id: BTreeMap<_, _> = nodes.iter().map(|n| (n.id, n)).collect();
    if by_id.len() != nodes.len() || by_id.contains_key(&0) {
        return Err(required());
    }
    let count: u64 = db
        .query_row(
            "SELECT count(*) FROM (SELECT node FROM phase_publication_nodes WHERE publication=?1 ORDER BY node LIMIT 201)",
            [id],
            |r| r.get(0),
        )
        .map_err(store::storage)?;
    if count != nodes.len() as u64 {
        return Err(required());
    }
    let mut statement = db.prepare("SELECT node,length(CAST(request AS BLOB)),CASE WHEN length(CAST(request AS BLOB))<=530000 THEN request END,CASE WHEN length(CAST(team AS BLOB))<=36 AND length(CAST(task_uuid AS BLOB))=36 AND length(CAST(state AS BLOB))<=16 AND length(CAST(reason AS BLOB))<=16384 AND coalesce(length(CAST(clear_outcome AS BLOB)),0)<=16 THEN json_array(team,task_uuid,state,reason,cleared,failures,clear_failures,clear_outcome) END FROM phase_publication_nodes WHERE publication=?1 ORDER BY node LIMIT 201").map_err(store::storage)?;
    let mut rows = statement.query([id]).map_err(store::storage)?;
    let mut requests = Vec::new();
    while let Some(row) = rows.next().map_err(store::storage)? {
        let node: u32 = row.get(0).map_err(store::storage)?;
        let length: u64 = row.get(1).map_err(store::storage)?;
        let request: Option<String> = row.get(2).map_err(store::storage)?;
        let request = request
            .filter(|r| r.len() as u64 == length)
            .ok_or_else(required)?;
        let state: Option<String> = row.get(3).map_err(store::storage)?;
        let state: Value =
            serde_json::from_str(&state.ok_or_else(required)?).map_err(store::storage)?;
        let task = state.get(1).and_then(Value::as_str).ok_or_else(required)?;
        if !matches!(
            state.get(2).and_then(Value::as_str),
            Some("pending" | "writing" | "created" | "refused" | "uncertain")
        ) || !matches!(state.get(4).and_then(Value::as_i64), Some(0 | 1))
            || state.get(5).and_then(Value::as_u64).is_none()
            || state.get(6).and_then(Value::as_u64).is_none()
            || !matches!(state.get(7), Some(Value::Null))
                && !matches!(
                    state.get(7).and_then(Value::as_str),
                    Some("cleared" | "refused" | "uncertain")
                )
        {
            return Err(required());
        }
        let saved: store::publication::PlannedTaskRequest =
            serde_json::from_str(&request).map_err(store::storage)?;
        if requests.len() >= 200
            || state.get(0).and_then(Value::as_str) != Some(team)
            || !uuid(task)
            || saved.publication != id
            || saved.team != team
            || saved.phase != phase
            || saved.task.uuid != task
            || by_id.get(&node).copied() != Some(&saved.node)
        {
            return Err(required());
        }
        requests.push(json!({"body":reference("original_request",json!([id,node,task]),request.as_bytes()),"state":metadata("publication_node",json!([id,node,task]),&state)?}));
    }
    if requests.len() as u64 != count {
        return Err(required());
    }
    Ok(
        json!({"role":role,"key":[team,phase,id],"header":header,"snapshot":reference(role,json!([team,phase,id]),body.as_bytes()),"originals":requests}),
    )
}

pub(super) fn capture(db: &Connection, team: &str, action: &TeamAction) -> Result<Option<Value>> {
    let Some(selection) = selection(action) else {
        return Ok(None);
    };
    if let Selection::Controls(action) = selection {
        return controls::capture(db, team, action).map(Some);
    }
    capture_selected(db, team, selection)
}

fn capture_selected(
    db: &Connection,
    team: &str,
    selection: Selection<'_>,
) -> Result<Option<Value>> {
    store::execution_fence(db)?;
    let phase = match &selection {
        Selection::Draft { phase, .. }
        | Selection::Current { phase, .. }
        | Selection::Correction { phase, .. } => *phase,
        Selection::Controls(_) => return Err(required()),
    };
    let header = point(
        db,
        "phases",
        "team",
        team,
        Some(("n", &json!(phase))),
        "data",
        262144,
    )?;
    let generation: Option<u64> = db
        .query_row(
            "SELECT generation FROM phase_drafts WHERE team=?1 AND phase=?2",
            params![team, phase],
            |r| r.get(0),
        )
        .optional()
        .map_err(store::storage)?;
    let episode = episode(db, team, phase)?;
    let mut snapshots = Vec::new();
    match selection {
        Selection::Controls(_) => return Err(required()),
        Selection::Correction {
            base,
            episode: expected,
            generation: wanted,
            ..
        } => {
            let current = episode.as_ref().ok_or_else(required)?;
            if current.id != expected
                || current.base != base
                || wanted.is_some_and(|g| current.generation != Some(g) || generation != Some(g))
            {
                return Err(required());
            }
            snapshots.push(publication(db, team, phase, base, "base")?);
            if let Some(id) = &current.approval {
                snapshots.push(publication(db, team, phase, id, "new_approval")?);
            }
        }
        Selection::Current {
            publication: expected,
            ..
        } => {
            let current = if let Some(row) = episode.as_ref().filter(|r| r.state != "published") {
                Some(row.base.clone())
            } else {
                current_publication(db, team, phase)?
            };
            if current.as_deref() != Some(expected) {
                return Err(required());
            }
            snapshots.push(publication(db, team, phase, expected, "current")?);
            if let Some(id) = episode
                .as_ref()
                .filter(|r| r.state != "published")
                .and_then(|r| r.approval.as_ref())
            {
                snapshots.push(publication(db, team, phase, id, "new_approval")?);
            }
        }
        Selection::Draft {
            generation: wanted, ..
        } => {
            if wanted.is_some_and(|g| generation != Some(g)) {
                return Err(required());
            }
            if let Some(id) = current_publication(db, team, phase)? {
                snapshots.push(publication(db, team, phase, &id, "current")?);
            }
        }
    }
    let proposal = proposal(db, team, phase, generation)?;
    Ok(Some(
        json!({"phase":[team,phase],"header":header,"generation":generation,"episode":episode,"proposal":proposal,"publications":snapshots}),
    ))
}
