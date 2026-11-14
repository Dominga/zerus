//! Typed, bounded phase projection. Unknown draft input fields make editing
//! unavailable rather than being silently discarded by a whole-draft save.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use zerus_team_plan::{draft::DraftNode, model::Phase};

use super::graph::{self, PlanGraph};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhaseList {
    pub team: String,
    pub revision: u64,
    pub rows: Vec<Phase>,
    pub next: Option<String>,
    pub next_phase: u32,
    pub next_node: u32,
    pub drafts_available: bool,
    pub routes: Vec<DraftRoute>,
    pub default_pipeline: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftRoute {
    pub pipeline: String,
    pub role: String,
}

#[derive(Deserialize)]
struct DraftRead {
    phase: Phase,
    revision: u64,
    generation: Option<u64>,
    next_node: u32,
    nodes: Vec<DraftNode>,
    complete: bool,
    editable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PhaseDraft {
    pub phase: Phase,
    pub revision: u64,
    pub generation: Option<u64>,
    pub next_node: u32,
    pub nodes: Vec<DraftNode>,
    pub graph: PlanGraph,
    pub complete: bool,
    pub editable: bool,
    pub unknown_fields: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PhaseFailure {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct PhasePane {
    pub epoch: u64,
    pub list: Option<PhaseList>,
    pub draft: Option<PhaseDraft>,
    pub selected: Option<u32>,
    pub list_failure: Option<PhaseFailure>,
    pub draft_failure: Option<PhaseFailure>,
}

pub fn list(value: &Value) -> Result<PhaseList, String> {
    if value
        .get("rows")
        .and_then(Value::as_array)
        .is_none_or(|a| a.len() > 200)
    {
        return Err("The phase list exceeds its row bound or has no rows.".into());
    }
    if value
        .get("routes")
        .and_then(Value::as_array)
        .is_none_or(|a| a.len() > 200)
    {
        return Err("The draft routes exceed their read bound.".into());
    }
    let out: PhaseList = crate::teams::tolerant::read(value.clone()).map_err(|e| e.to_string())?;
    let ids: BTreeSet<_> = out.rows.iter().map(|p| p.n).collect();
    if ids.len() != out.rows.len() || out.next_phase == 0 || out.next_node == 0 {
        return Err("The phase list has ambiguous identities.".into());
    }
    if out
        .next
        .as_ref()
        .is_some_and(|s| s.is_empty() || s.len() > crate::teams::read::CURSOR_BYTES)
    {
        return Err("The phase list has an invalid cursor.".into());
    }
    Ok(out)
}

pub fn draft(value: &Value, wanted: u32) -> Result<PhaseDraft, String> {
    let data = value.get("draft").ok_or("The phase read gives no draft.")?;
    if data
        .get("nodes")
        .and_then(Value::as_array)
        .is_none_or(|a| a.len() > 200)
        || serde_json::to_vec(data).map_err(|e| e.to_string())?.len() > 300_000
    {
        return Err("The draft exceeds its row or byte bound.".into());
    }
    let (out, unknown): (DraftRead, Vec<String>) =
        crate::teams::tolerant::read_noting(data.clone()).map_err(|e| e.to_string())?;
    let ids: BTreeSet<_> = out.nodes.iter().map(|n| n.id).collect();
    if out.nodes.iter().any(|n| {
        n.title.len() > 200
            || n.text.len() > 16_384
            || n.criteria.is_empty()
            || n.criteria.len() > 32
            || n.criteria.iter().any(|c| c.len() > 2048)
            || n.deps.len() > 200
            || n.writes.len() > 200
            || n.writes.iter().any(|p| p.len() > 4096)
            || n.kind == zerus_team_plan::NodeKind::Stage
    }) {
        return Err("A draft task exceeds its field bounds or carries runtime state.".into());
    }
    if out.phase.n != wanted
        || ids.len() != out.nodes.len()
        || ids.contains(&0)
        || out.next_node == 0
        || (out.complete && out.generation.is_none())
        || (out.editable
            && (!out.complete
                || out.phase.kind != zerus_team_plan::PhaseKind::Plan
                || out.phase.state != zerus_team_plan::PhaseState::Draft))
    {
        return Err("The phase read has incomplete or conflicting draft identities.".into());
    }
    let nodes = out.nodes.iter().map(|n| n.node(wanted)).collect::<Vec<_>>();
    Ok(PhaseDraft {
        phase: out.phase,
        revision: out.revision,
        generation: out.generation,
        next_node: out.next_node,
        graph: graph::project(&nodes, out.complete),
        nodes: out.nodes,
        complete: out.complete,
        editable: out.editable && unknown.is_empty(),
        unknown_fields: !unknown.is_empty(),
    })
}

#[cfg(test)]
mod tests;
