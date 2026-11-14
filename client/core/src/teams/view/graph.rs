//! Graph facts for the bounded page. Geometry remains in the webview; layer
//! ordering comes from the same kernel as the team's scheduler.
use super::{QueueCard, Value};
use zerus_team_plan::{
    graph,
    model::{Dep, Node},
};

/// A bounded read's graph. `complete` describes the caller's read scope
/// (one whole phase, or a partial queue page); it never drops external edges.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct PlanGraph {
    pub complete: bool,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct GraphNode {
    pub id: u32,
    pub phase: u32,
    pub layer: u32,
}

/// An arrow runs from the prerequisite to the task that depends on it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct GraphEdge {
    pub from: u32,
    pub to: u32,
    pub kind: zerus_team_plan::model::DepKind,
    /// The prerequisite lies outside this supplied read scope.
    pub external: bool,
}

/// Project already bounded typed nodes, including drafts (`task=None`), using
/// the scheduler's exact graph kernel. No store I/O, platform code or filtering
/// by state; the reader supplies the scope and its explicit completeness.
pub fn project(nodes: &[Node], complete: bool) -> PlanGraph {
    let layers = graph::layers(nodes);
    let ids: std::collections::BTreeSet<_> = nodes.iter().map(|node| node.id).collect();
    let ids = &ids;
    PlanGraph {
        complete,
        nodes: nodes
            .iter()
            .map(|node| GraphNode {
                id: node.id,
                phase: node.phase,
                layer: layers[&node.id],
            })
            .collect(),
        edges: nodes
            .iter()
            .flat_map(|node| {
                node.deps.iter().map(move |dep| GraphEdge {
                    from: dep.node,
                    to: node.id,
                    kind: dep.kind,
                    external: !ids.contains(&dep.node),
                })
            })
            .collect(),
    }
}

pub(super) fn dependencies(row: &Value) -> Vec<Dep> {
    super::array(row, "deps")
        .iter()
        .take(200)
        .filter_map(|value| serde_json::from_value(value.clone()).ok())
        .collect()
}

pub(super) fn layers(cards: &mut [QueueCard]) {
    let nodes: Vec<Node> = cards
        .iter()
        .filter_map(|card| {
            // Construct only the kernel's graph inputs, never decode the CLI's
            // additive view fields as a persisted Node.
            serde_json::from_value(serde_json::json!({
                "id":card.node, "phase":card.phase.unwrap_or(0),
                "title":"", "deps":card.deps, "origin":card.origin,
            }))
            .ok()
        })
        .collect();
    let layers: std::collections::BTreeMap<_, _> = project(&nodes, false)
        .nodes
        .into_iter()
        .map(|node| (node.id, node.layer))
        .collect();
    for card in cards {
        card.layer = u32::try_from(card.node)
            .ok()
            .and_then(|id| layers.get(&id).copied())
            .filter(|layer| *layer > 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Hand-written protocol rows: isolates projection from the CLI's additive
    // fields. The CLI and operation tests cover rows from the product writers.
    #[test]
    fn graph_projection_keeps_soft_and_off_page_edges_and_uses_kernel_layers() {
        let queue = json!({"rows":[
            {"id":1,"phase":0,"state":"pending"},
            {"id":2,"phase":0,"state":"pending","deps":[{"node":1,"kind":"soft"}]},
            {"id":3,"phase":0,"state":"pending","deps":[{"node":1},{"node":2},{"node":99}]},
            {"id":4,"phase":1,"state":"pending","origin":3}
        ]});
        let view = super::super::team(&json!({"team":{}}), &queue);
        let cards: Vec<_> = view.groups.iter().flat_map(|group| &group.cards).collect();
        assert_eq!(
            cards.iter().map(|card| card.layer).collect::<Vec<_>>(),
            vec![None, Some(1), Some(2), Some(2)]
        );
        assert_eq!(cards[1].deps[0].kind, zerus_team_plan::model::DepKind::Soft);
        assert_eq!(cards[2].deps[2].node, 99);
    }

    #[test]
    fn graph_projection_bounds_dependencies_and_tolerates_older_empty_rows() {
        assert!(dependencies(&json!({})).is_empty());
        let row = json!({"deps":(1..=201).map(|id| json!({"node":id})).collect::<Vec<_>>()});
        assert_eq!(dependencies(&row).len(), 200);
    }
}

#[cfg(test)]
#[test]
fn product_queue_page_projects_the_graph_fixture() {
    let queue: Value = serde_json::from_str(include_str!("../fixtures/graph-page.json")).unwrap();
    let status: Value =
        serde_json::from_str(include_str!("../fixtures/graph-status.json")).unwrap();
    let view = super::team(&status, &queue);
    println!("GRAPH_VIEW {}", serde_json::to_string(&view).unwrap());
    let pinned: Value = serde_json::from_str(include_str!("../fixtures/graph-view.json")).unwrap();
    assert_eq!(serde_json::to_value(view).unwrap(), pinned);
}

#[cfg(test)]
#[test]
fn draft_and_partial_scopes_share_layers_and_keep_external_edge_kinds() {
    // Typed synthetic draft input isolates the public projection contract.
    let nodes: Vec<Node> = serde_json::from_value(serde_json::json!([
        {"id":1,"phase":1,"title":"Draft"},
        {"id":2,"phase":1,"title":"Next","deps":[{"node":1},{"node":99,"kind":"soft"}]}
    ]))
    .unwrap();
    let full = project(&nodes, true);
    assert!(nodes.iter().all(|node| node.task.is_none()));
    assert_eq!(
        full.nodes.iter().map(|node| node.layer).collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert!(full.complete);
    assert_eq!(
        full.edges[0],
        GraphEdge {
            from: 1,
            to: 2,
            kind: zerus_team_plan::model::DepKind::Hard,
            external: false
        }
    );
    assert_eq!(
        full.edges[1],
        GraphEdge {
            from: 99,
            to: 2,
            kind: zerus_team_plan::model::DepKind::Soft,
            external: true
        }
    );
    let partial = project(&nodes, false);
    assert!(!partial.complete);
    assert_eq!(full.nodes, partial.nodes);
    assert_eq!(full.edges, partial.edges);
}
