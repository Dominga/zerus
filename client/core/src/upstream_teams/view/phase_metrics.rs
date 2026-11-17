//! Estimated work/span for one explicitly complete phase, using the team's
//! graph kernel. This is a view only: it never changes readiness or validation.
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use zerus_team_plan::{graph, model::DepKind, Node};

/// The planned .161 live snapshot bound; drafts have a smaller producer cap.
pub const MAX_NODES: usize = 4096;
pub const MAX_DEPENDENCIES: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Unavailable {
    IncompletePhase,
    MissingEstimates,
    EmptyPhase,
    InvalidGraph,
    ScopeTooLarge,
    UnknownFields,
    LiveSnapshotUnavailable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PhaseMetrics {
    pub unavailable: Option<Unavailable>,
    pub critical_path_minutes: Option<u64>,
    /// Rounded to one decimal place, without floating-point wire values.
    pub average_parallelism_tenths: Option<u64>,
    pub known_estimates: usize,
    pub total_nodes: usize,
    pub external_dependencies: usize,
    /// Reserved for existing non-fatal engine findings. There are none in the
    /// current saved-draft contract; hard refusals are never made warnings.
    pub warnings: Vec<String>,
}

impl PhaseMetrics {
    pub fn unavailable(reason: Unavailable) -> Self {
        Self {
            unavailable: Some(reason),
            critical_path_minutes: None,
            average_parallelism_tenths: None,
            known_estimates: 0,
            total_nodes: 0,
            external_dependencies: 0,
            warnings: Vec::new(),
        }
    }
}

/// Intrinsic phase duration in estimated minutes and work/span parallelism.
/// A hard dependency waits for the prerequisite's finish; a soft dependency
/// waits only for its start, as in the scheduler. Origins are placement facts,
/// not execution dependencies. External waits are outside this phase's span
/// and are counted explicitly. No estimate is substituted for an unknown zero.
///
/// The caller owns the completeness proof, including archived nodes. A queue
/// page, even at EOF, is not such a proof. This accepts draft or live Nodes
/// unchanged; no store, task, wall-clock or member data is consulted.
pub fn project(nodes: &[Node], complete: bool) -> PhaseMetrics {
    let mut out = PhaseMetrics::unavailable(Unavailable::IncompletePhase);
    if !complete {
        return out;
    }
    if nodes.len() > MAX_NODES || nodes.iter().any(|node| node.deps.len() > MAX_DEPENDENCIES) {
        out.unavailable = Some(Unavailable::ScopeTooLarge);
        return out;
    }
    out.total_nodes = nodes.len();
    if nodes.is_empty() {
        out.unavailable = Some(Unavailable::EmptyPhase);
        return out;
    }
    let ids: BTreeSet<_> = nodes.iter().map(|node| node.id).collect();
    if ids.len() != nodes.len()
        || ids.contains(&0)
        || nodes.iter().any(|node| node.phase != nodes[0].phase)
    {
        out.unavailable = Some(Unavailable::InvalidGraph);
        return out;
    }
    out.known_estimates = nodes
        .iter()
        .filter(|node| node.estimate_minutes > 0)
        .count();
    if out.known_estimates != nodes.len() {
        out.unavailable = Some(Unavailable::MissingEstimates);
        return out;
    }
    if !graph::cycles(nodes).is_empty() {
        out.unavailable = Some(Unavailable::InvalidGraph);
        return out;
    }
    let layers = graph::layers(nodes);
    let mut ordered: Vec<_> = nodes.iter().collect();
    ordered.sort_by_key(|node| (layers[&node.id], node.id));
    let mut times: BTreeMap<u32, (u64, u64)> = BTreeMap::new();
    let mut work = 0u64;
    let mut span = 0u64;
    for node in ordered {
        let mut start = 0;
        for dep in &node.deps {
            if !ids.contains(&dep.node) {
                out.external_dependencies += 1;
                continue;
            }
            let Some(&(dep_start, dep_finish)) = times.get(&dep.node) else {
                // The placement kernel may break an origin loop. If that
                // leaves an execution dependency out of order, claim no span.
                out.unavailable = Some(Unavailable::InvalidGraph);
                return out;
            };
            start = start.max(if dep.kind == DepKind::Soft {
                dep_start
            } else {
                dep_finish
            });
        }
        // The row/dependency bounds make these u64 sums safe even for u32::MAX
        // estimates; the current draft writer additionally caps at one year.
        let finish = start + u64::from(node.estimate_minutes);
        work += u64::from(node.estimate_minutes);
        span = span.max(finish);
        times.insert(node.id, (start, finish));
    }
    out.unavailable = None;
    out.critical_path_minutes = Some(span);
    out.average_parallelism_tenths = Some((work * 10 + span / 2) / span);
    out
}

#[cfg(test)]
mod availability_tests;
// The fork keeps upstream's team client unwired (merge 2026-11-17): these
// tests drive it through the client-wide Change pipeline or the removed UI
// fixtures, which carry the fork's own teams.
#[cfg(any())]
mod tests;
