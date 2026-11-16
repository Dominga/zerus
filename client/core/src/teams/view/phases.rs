//! Typed, bounded phase projection. Unknown draft input fields make editing
//! unavailable rather than being silently discarded by a whole-draft save.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use zerus_team_plan::{draft::DraftNode, model::Phase};

mod rework;
mod snapshot;
use super::graph::{self, PlanGraph};
use super::phase_metrics::{self, PhaseMetrics, Unavailable};
pub use rework::PhaseRework;
pub use snapshot::PhaseNodeSnapshot;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhaseList {
    pub team: String,
    pub revision: u64,
    pub rows: Vec<Phase>,
    pub next: Option<String>,
    pub next_phase: u32,
    pub next_node: u32,
    pub drafts_available: bool,
    #[serde(default)]
    pub approval_available: bool,
    pub routes: Vec<DraftRoute>,
    pub default_pipeline: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftRoute {
    pub pipeline: String,
    pub role: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Publication {
    pub id: String,
    pub generation: u64,
    pub state: String,
    pub total: u32,
    pub created: u32,
    pub blocked: u32,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rejection {
    pub id: String,
    pub generation: u64,
    pub comment: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviousDraft {
    pub generation: u64,
    pub draft: zerus_team_plan::draft::DraftInput,
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
    #[serde(default)]
    accept_available: bool,
    publication: Option<Publication>,
    rejection: Option<Rejection>,
    previous: Option<PreviousDraft>,
    live: Option<Value>,
    #[serde(default)]
    rework_available: bool,
    rework: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PhaseDraft {
    pub phase: Phase,
    pub revision: u64,
    pub generation: Option<u64>,
    pub next_node: u32,
    pub nodes: Vec<DraftNode>,
    pub graph: PlanGraph,
    pub metrics: PhaseMetrics,
    pub complete: bool,
    pub editable: bool,
    pub accept_available: bool,
    pub unknown_fields: bool,
    pub publication: Option<Publication>,
    pub rejection: Option<Rejection>,
    pub previous: Option<PreviousDraft>,
    pub live: Option<PhaseNodeSnapshot>,
    pub rework_available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rework: Option<PhaseRework>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PhaseFailure {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct PhasePane {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed: Option<crate::teams::observed_timing::ObservedPane>,
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
    draft_value(data, wanted, true)
}

fn draft_value(data: &Value, wanted: u32, allow_rework: bool) -> Result<PhaseDraft, String> {
    if !allow_rework && data.get("rework").is_some() {
        return Err("Correction proposals cannot contain another rework episode.".into());
    }
    let mut frozen = data.clone();
    if let Some(fields) = frozen.as_object_mut() {
        fields.remove("live");
        fields.remove("rework");
    }
    if data
        .get("nodes")
        .and_then(Value::as_array)
        .is_none_or(|a| a.len() > 200)
        || serde_json::to_vec(&frozen)
            .map_err(|e| e.to_string())?
            .len()
            > 600_000
        || serde_json::to_vec(data).map_err(|e| e.to_string())?.len() > 8 * 1024 * 1024
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
        || (out.complete && (out.generation.is_none() || out.nodes.is_empty()))
        || (out.editable
            && (!out.complete
                || out.phase.kind != zerus_team_plan::PhaseKind::Plan
                || !matches!(
                    out.phase.state,
                    zerus_team_plan::PhaseState::Draft | zerus_team_plan::PhaseState::Rejected
                )
                || out.publication.is_some()))
    {
        return Err("The phase read has incomplete or conflicting draft identities.".into());
    }
    if let Some(p) = &out.publication {
        if !crate::teams::action::canonical_uuid(&p.id)
            || out.generation != Some(p.generation)
            || !matches!(p.state.as_str(), "reserved" | "published")
            || p.total == 0
            || p.total > 200
            || p.total as usize != out.nodes.len()
            || p.created > p.total
            || p.blocked > p.total
            || p.reason.as_ref().is_some_and(|s| s.len() > 2048)
        {
            return Err("The phase publication has conflicting identities or counts.".into());
        }
    }
    if let Some(previous) = &out.previous {
        if out.generation.is_none_or(|g| previous.generation >= g) || previous.generation == 0 {
            return Err("The previous draft has a conflicting generation.".into());
        }
        crate::teams::action::TeamAction::SubmitDraftPhase {
            phase: wanted,
            draft: previous.draft.clone(),
        }
        .check()?;
    }
    if let Some(r) = &out.rejection {
        if !crate::teams::action::canonical_uuid(&r.id)
            || out.generation != Some(r.generation)
            || out.phase.state != zerus_team_plan::PhaseState::Rejected
            || r.comment.trim().is_empty()
            || r.comment.len() > 16_384
            || out.publication.is_some()
        {
            return Err("The phase rejection is no longer this draft's episode.".into());
        }
    }
    let published = matches!(
        out.phase.state,
        zerus_team_plan::PhaseState::Approved
            | zerus_team_plan::PhaseState::AwaitingAcceptance
            | zerus_team_plan::PhaseState::Done
    );
    let rework = out
        .rework
        .as_ref()
        .map(|r| rework::read(r, &out))
        .transpose()?;
    let live = out
        .live
        .map(|value| snapshot::read(value, wanted, out.revision))
        .transpose()?;
    if live.is_some()
        && (!published
            || !out
                .publication
                .as_ref()
                .is_some_and(|p| p.state == "published" && p.created == p.total && p.blocked == 0))
    {
        return Err("A live phase snapshot has no settled publication.".into());
    }
    if live.as_ref().is_some_and(|scope| {
        scope.complete
            && out
                .nodes
                .iter()
                .any(|original| !scope.nodes.iter().any(|n| n.id == original.id))
    }) {
        return Err("A complete live phase snapshot omits an approved task.".into());
    }
    let nodes = out.nodes.iter().map(|n| n.node(wanted)).collect::<Vec<_>>();
    let metrics = if !unknown.is_empty() {
        PhaseMetrics::unavailable(Unavailable::UnknownFields)
    } else if published {
        live.as_ref().map_or_else(
            || PhaseMetrics::unavailable(Unavailable::LiveSnapshotUnavailable),
            |scope| phase_metrics::project(&scope.nodes, scope.complete),
        )
    } else {
        phase_metrics::project(&nodes, out.complete)
    };
    let graph = if published {
        live.as_ref().map_or_else(
            || graph::project(&[], false),
            |scope| graph::project(&scope.nodes, scope.complete && unknown.is_empty()),
        )
    } else {
        graph::project(&nodes, out.complete && unknown.is_empty())
    };
    let accept_available = out.accept_available
        && unknown.is_empty()
        && live.as_ref().is_some_and(|s| s.complete)
        && out.phase.state == zerus_team_plan::PhaseState::AwaitingAcceptance;
    Ok(PhaseDraft {
        phase: out.phase,
        revision: out.revision,
        generation: out.generation,
        next_node: out.next_node,
        graph,
        metrics,
        nodes: out.nodes,
        complete: out.complete,
        editable: out.editable && unknown.is_empty(),
        accept_available,
        unknown_fields: !unknown.is_empty(),
        publication: out.publication,
        rejection: out.rejection,
        previous: out.previous,
        live,
        rework_available: out.rework_available,
        rework,
    })
}

#[cfg(test)]
mod tests;
