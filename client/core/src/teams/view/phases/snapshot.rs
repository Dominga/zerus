//! The selected phase's live scope is explicit. A missing or invalid scope
//! never borrows draft/queue data or manufactures an empty complete graph.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use zerus_team_plan::model::Node;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhaseNodeSnapshot {
    pub phase: u32,
    pub revision: u64,
    pub nodes: Vec<Node>,
    pub complete: bool,
}

pub(super) fn read(value: Value, phase: u32, revision: u64) -> Result<PhaseNodeSnapshot, String> {
    if value
        .get("nodes")
        .and_then(Value::as_array)
        .is_none_or(|n| n.len() > 4096)
        || serde_json::to_vec(&value).map_err(|e| e.to_string())?.len() > 8 * 1024 * 1024
    {
        return Err("The live phase snapshot exceeds its bound or has no node list.".into());
    }
    let out: PhaseNodeSnapshot = serde_json::from_value(value).map_err(|e| e.to_string())?;
    let ids: BTreeSet<_> = out.nodes.iter().map(|n| n.id).collect();
    if out.phase != phase
        || out.revision != revision
        || ids.len() != out.nodes.len()
        || ids.contains(&0)
        || (out.complete && out.nodes.is_empty())
    {
        return Err("The live phase snapshot has incomplete or conflicting identities.".into());
    }
    for n in &out.nodes {
        if n.phase != phase
            || n.deps.len() > 200
            || n.writes.len() > 200
            || n.writes.iter().any(|p| p.len() > 4096)
            || n.title.len() > 200
            || n.text.len() > 80_000
        {
            return Err("A live phase node exceeds its scope or field bounds.".into());
        }
        let deps: BTreeSet<_> = n.deps.iter().map(|d| d.node).collect();
        if deps.len() != n.deps.len() || deps.contains(&0) || deps.contains(&n.id) {
            return Err("A live phase node has ambiguous dependencies.".into());
        }
        if n.task
            .as_ref()
            .is_some_and(|t| t.project.is_empty() || !crate::teams::read::valid_team(&t.uuid))
        {
            return Err("A live phase task has an invalid identity.".into());
        }
    }
    Ok(out)
}
