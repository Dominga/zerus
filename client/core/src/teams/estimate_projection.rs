//! Implementation estimates for one returned draft/publication cohort.
//! This neither measures actual work nor computes an elapsed time or ETA.
use super::view::phases::PhaseDraft;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use zerus_team_plan::{model::PhaseState, NodeKind, PhaseKind, TaskRef};

pub const MAX_ESTIMATE_MINUTES: u32 = 525_600;
pub const MAX_COHORT: usize = 200;
const MAX_LIVE: usize = 4096;
const JS_INTEGER: u64 = 9_007_199_254_740_991;

/// Read-model context, never a renderer-supplied label. Computer identity is
/// supplied by the enclosing ComputerId-bound Change and selected model epoch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EstimateContext {
    pub team: String,
    pub home: String,
    pub revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EstimateVersion {
    pub phase: u32,
    pub draft_generation: u64,
    pub publication: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EstimateBasis {
    UnapprovedPreview,
    FrozenPublication,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EstimateScope {
    pub team: String,
    pub home: String,
    pub revision: u64,
    pub version: EstimateVersion,
    pub basis: EstimateBasis,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EstimateUnknown {
    SourceIdentityUnavailable,
    StaleRevision,
    GenerationUnavailable,
    HistoryUnavailable,
    UnknownFields,
    IncompleteGraph,
    ScopeTooLarge,
    IncompleteDraft,
    UnsettledPublication,
    IncompleteLive,
    ConflictingMembership,
    MissingEstimate,
    EstimateOverBound,
    ExternalPrerequisite,
    NotImplementation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EstimateRow {
    pub node: u32,
    pub title: String,
    pub kind: NodeKind,
    pub task: Option<TaskRef>,
    pub estimate_minutes: Option<u32>,
    pub unknown: Option<EstimateUnknown>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PhaseEstimates {
    pub scope: Option<EstimateScope>,
    pub cohort_complete: bool,
    pub unknown: Option<EstimateUnknown>,
    pub rows: Vec<EstimateRow>,
}

impl PhaseEstimates {
    pub fn unavailable(reason: EstimateUnknown) -> Self {
        Self {
            scope: None,
            cohort_complete: false,
            unknown: Some(reason),
            rows: Vec::new(),
        }
    }
}

/// `requested` pins an existing version, not an instruction to retarget it to
/// whatever draft a later read returns. No history is fetched or cached here.
/// Approved estimates come only from the frozen publication's draft.nodes.
/// Live data establishes membership/task identity, never replacement estimates.
pub fn project(
    context: &EstimateContext,
    source: &PhaseDraft,
    requested: Option<&EstimateVersion>,
) -> PhaseEstimates {
    use EstimateUnknown as U;
    if !super::read::valid_team(&context.team)
        || !super::read::valid_team(&context.home)
        || context.revision == 0
        || context.revision > JS_INTEGER
        || source.phase.n == 0
        || source.phase.kind != PhaseKind::Plan
    {
        return PhaseEstimates::unavailable(U::SourceIdentityUnavailable);
    }
    if context.revision != source.revision {
        return PhaseEstimates::unavailable(U::StaleRevision);
    }
    let Some(generation) = source.generation.filter(|g| *g > 0 && *g <= JS_INTEGER) else {
        return PhaseEstimates::unavailable(U::GenerationUnavailable);
    };
    let version = EstimateVersion {
        phase: source.phase.n,
        draft_generation: generation,
        publication: source.publication.as_ref().map(|p| p.id.clone()),
    };
    if requested.is_some_and(|wanted| *wanted != version) {
        return PhaseEstimates::unavailable(U::HistoryUnavailable);
    }
    let published = matches!(
        source.phase.state,
        PhaseState::Approved | PhaseState::AwaitingAcceptance | PhaseState::Done
    );
    if published != source.publication.is_some()
        || source
            .publication
            .as_ref()
            .is_some_and(|p| p.generation != generation || !super::action::canonical_uuid(&p.id))
        || (!published && source.live.is_some())
    {
        return PhaseEstimates::unavailable(U::ConflictingMembership);
    }
    if source.nodes.len() > MAX_COHORT
        || source
            .nodes
            .iter()
            .any(|n| n.title.len() > 200 || n.deps.len() > 200)
    {
        return PhaseEstimates::unavailable(U::ScopeTooLarge);
    }
    let ids: BTreeSet<_> = source.nodes.iter().map(|n| n.id).collect();
    if ids.contains(&0)
        || ids.len() != source.nodes.len()
        || source.nodes.iter().any(|n| n.kind == NodeKind::Stage)
    {
        return PhaseEstimates::unavailable(U::ConflictingMembership);
    }
    let mut unknown = if source.unknown_fields {
        Some(U::UnknownFields)
    } else if !source.complete || source.nodes.is_empty() {
        Some(U::IncompleteDraft)
    } else if !source.graph.complete {
        Some(U::IncompleteGraph)
    } else {
        None
    };
    let mut tasks = BTreeMap::new();
    if published {
        let publication = source.publication.as_ref().unwrap();
        if publication.state != "published"
            || publication.total as usize != ids.len()
            || publication.created != publication.total
            || publication.blocked != 0
        {
            unknown.get_or_insert(U::UnsettledPublication);
        } else if let Some(live) = &source.live {
            if !live.complete
                || live.phase != source.phase.n
                || live.revision != source.revision
                || live.nodes.len() > MAX_LIVE
            {
                unknown.get_or_insert(U::IncompleteLive);
            } else {
                let by_id: BTreeMap<_, _> = live.nodes.iter().map(|n| (n.id, n)).collect();
                let mut task_ids = BTreeSet::new();
                if by_id.len() != live.nodes.len() {
                    unknown.get_or_insert(U::ConflictingMembership);
                }
                // Earlier accepted nodes/stages may remain in the complete
                // phase. Only this publication's original cohort is joined.
                for original in &source.nodes {
                    let live_node = by_id.get(&original.id).copied();
                    let task = live_node.and_then(|n| n.task.as_ref());
                    if live_node
                        .is_none_or(|n| n.phase != source.phase.n || n.kind != original.kind)
                        || task.is_none_or(|t| {
                            t.project.is_empty()
                                || t.project.len() > 256
                                || !super::read::valid_team(&t.uuid)
                                || !task_ids.insert((t.project.clone(), t.uuid.clone()))
                        })
                    {
                        unknown.get_or_insert(U::ConflictingMembership);
                    } else {
                        tasks.insert(original.id, task.unwrap().clone());
                    }
                }
            }
        } else {
            unknown.get_or_insert(U::IncompleteLive);
        }
    }
    let mut rows: Vec<_> = source
        .nodes
        .iter()
        .map(|node| {
            let reason = unknown.or_else(|| {
                if node.kind != NodeKind::Task {
                    Some(U::NotImplementation)
                } else if node.estimate_minutes == 0 {
                    Some(U::MissingEstimate)
                } else if node.estimate_minutes > MAX_ESTIMATE_MINUTES {
                    Some(U::EstimateOverBound)
                } else if node.deps.iter().any(|d| !ids.contains(&d.node)) {
                    Some(U::ExternalPrerequisite)
                } else {
                    None
                }
            });
            EstimateRow {
                node: node.id,
                title: node.title.clone(),
                kind: node.kind,
                task: unknown
                    .is_none()
                    .then(|| tasks.get(&node.id).cloned())
                    .flatten(),
                estimate_minutes: reason.is_none().then_some(node.estimate_minutes),
                unknown: reason,
            }
        })
        .collect();
    rows.sort_by_key(|n| n.node);
    PhaseEstimates {
        scope: Some(EstimateScope {
            team: context.team.clone(),
            home: context.home.clone(),
            revision: source.revision,
            version,
            basis: if published {
                EstimateBasis::FrozenPublication
            } else {
                EstimateBasis::UnapprovedPreview
            },
        }),
        cohort_complete: unknown.is_none(),
        unknown,
        rows,
    }
}

#[cfg(test)]
mod tests;
