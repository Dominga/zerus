//! The shape of a plan: dependency cycles, layers, how much work each node
//! unblocks, and the order that follows from it.
//!
//! Imported from WhiteWh/zerus at `ca4cc4481` (`src/state/teams/plan.rs`:
//! `cycle_tasks`, `layers`, `dependants_count`, `dispatch_order`) and adapted
//! to [`Node`]. Edges run from a dependency to its dependant. A dependency on
//! a node that is not in the slice is not an edge here; readiness treats it
//! as unmet.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::{Node, NodeId};

/// The dependency edges between the nodes of one slice, by dense index. Each
/// walk below is iterative, so a long chain never deepens the stack.
struct Graph<'a> {
    nodes: Vec<&'a Node>,
    /// Node IDs, ascending and distinct; a position here is a node's index.
    ids: Vec<NodeId>,
    index: BTreeMap<NodeId, usize>,
    /// Direct dependants of each node, ascending.
    dependants: Vec<Vec<usize>>,
}

impl<'a> Graph<'a> {
    fn new(nodes: impl IntoIterator<Item = &'a Node>) -> Self {
        let nodes: Vec<&'a Node> = nodes.into_iter().collect();
        let ids: Vec<NodeId> = nodes
            .iter()
            .map(|node| node.id)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let index: BTreeMap<NodeId, usize> =
            ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
        let mut dependants: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); ids.len()];
        for node in &nodes {
            let to = index[&node.id];
            for dep in &node.deps {
                if let Some(&from) = index.get(&dep.node) {
                    dependants[from].insert(to);
                }
            }
        }
        Self {
            nodes,
            ids,
            index,
            dependants: dependants
                .into_iter()
                .map(|set| set.into_iter().collect())
                .collect(),
        }
    }

    fn len(&self) -> usize {
        self.ids.len()
    }

    /// Tarjan's strongly connected components, iteratively: every component
    /// larger than one node, and every node with an edge to itself.
    fn components(&self) -> Vec<Vec<NodeId>> {
        const UNSEEN: usize = usize::MAX;
        let n = self.len();
        let mut order = vec![UNSEEN; n];
        let mut low = vec![0; n];
        let mut on_stack = vec![false; n];
        let mut stack = Vec::new();
        let mut next_order = 0;
        let mut found = Vec::new();
        for root in 0..n {
            if order[root] != UNSEEN {
                continue;
            }
            // Each frame is a node and the position of its next dependant.
            let mut calls = vec![(root, 0usize)];
            order[root] = next_order;
            low[root] = next_order;
            next_order += 1;
            stack.push(root);
            on_stack[root] = true;
            while let Some(frame) = calls.last_mut() {
                let v = frame.0;
                if let Some(&w) = self.dependants[v].get(frame.1) {
                    frame.1 += 1;
                    if order[w] == UNSEEN {
                        order[w] = next_order;
                        low[w] = next_order;
                        next_order += 1;
                        stack.push(w);
                        on_stack[w] = true;
                        calls.push((w, 0));
                    } else if on_stack[w] {
                        low[v] = low[v].min(order[w]);
                    }
                    continue;
                }
                calls.pop();
                if let Some(&(parent, _)) = calls.last() {
                    low[parent] = low[parent].min(low[v]);
                }
                if low[v] == order[v] {
                    let mut component = Vec::new();
                    while let Some(w) = stack.pop() {
                        on_stack[w] = false;
                        component.push(self.ids[w]);
                        if w == v {
                            break;
                        }
                    }
                    if component.len() > 1 || self.dependants[v].contains(&v) {
                        component.sort_unstable();
                        found.push(component);
                    }
                }
            }
        }
        found.sort();
        found
    }

    /// The edges a depth-first walk meets as back edges, walking roots first
    /// and then by ID, as the fork orders its walk.
    fn back_edges(&self) -> BTreeSet<(usize, usize)> {
        const WHITE: u8 = 0;
        const GREY: u8 = 1;
        const BLACK: u8 = 2;
        let mut colour = vec![WHITE; self.len()];
        let mut back = BTreeSet::new();
        let mut starts: Vec<&Node> = self.nodes.clone();
        starts.sort_by_key(|node| (!node.deps.is_empty(), node.id));
        for start in starts {
            let root = self.index[&start.id];
            if colour[root] != WHITE {
                continue;
            }
            colour[root] = GREY;
            let mut calls = vec![(root, 0usize)];
            while let Some(frame) = calls.last_mut() {
                let v = frame.0;
                if let Some(&w) = self.dependants[v].get(frame.1) {
                    frame.1 += 1;
                    match colour[w] {
                        WHITE => {
                            colour[w] = GREY;
                            calls.push((w, 0));
                        }
                        GREY => {
                            back.insert((v, w));
                        }
                        _ => {}
                    }
                    continue;
                }
                colour[v] = BLACK;
                calls.pop();
            }
        }
        back
    }
}

/// Every dependency cycle: each a sorted list of node IDs, the list sorted by
/// its first ID. A node that depends on itself is a cycle of one.
pub fn cycles(nodes: &[Node]) -> Vec<Vec<NodeId>> {
    Graph::new(nodes).components()
}

/// Every node in a cycle among `nodes`, for readiness, which passes only the
/// nodes that still count.
pub(crate) fn cycle_members<'a>(nodes: impl IntoIterator<Item = &'a Node>) -> BTreeSet<NodeId> {
    Graph::new(nodes)
        .components()
        .into_iter()
        .flatten()
        .collect()
}

/// Each node's layer: the longest dependency path from a root. Back edges of
/// a cycle are ignored, so adding one does not reshuffle the graph. A finding
/// sits at least in the layer of the node it came from.
pub fn layers(nodes: &[Node]) -> BTreeMap<NodeId, u32> {
    let graph = Graph::new(nodes);
    let back = graph.back_edges();
    // What each node's depth is taken from, in the fork's order: its
    // dependencies one layer down, then its origin in the same layer.
    let mut inputs: Vec<Vec<(usize, u32)>> = vec![Vec::new(); graph.len()];
    for node in nodes {
        let v = graph.index[&node.id];
        for dep in &node.deps {
            if let Some(&u) = graph.index.get(&dep.node) {
                if !back.contains(&(u, v)) {
                    inputs[v].push((u, 1));
                }
            }
        }
        if let Some(&u) = node
            .origin
            .filter(|origin| *origin != node.id)
            .and_then(|origin| graph.index.get(&origin))
        {
            inputs[v].push((u, 0));
        }
    }
    // A node being measured reads as 0, which ends an origin loop as the
    // fork's memo does.
    let mut depth: Vec<Option<u32>> = vec![None; graph.len()];
    for node in nodes {
        let start = graph.index[&node.id];
        if depth[start].is_some() {
            continue;
        }
        depth[start] = Some(0);
        // Each frame is a node, the position of its next input and its depth so far.
        let mut calls = vec![(start, 0usize, 0u32)];
        while let Some(frame) = calls.last_mut() {
            let (v, next, so_far) = *frame;
            if let Some(&(u, step)) = inputs[v].get(next) {
                match depth[u] {
                    Some(d) => {
                        frame.1 += 1;
                        frame.2 = so_far.max(d + step);
                    }
                    None => {
                        depth[u] = Some(0);
                        calls.push((u, 0, 0));
                    }
                }
                continue;
            }
            depth[v] = Some(so_far);
            calls.pop();
        }
    }
    graph
        .ids
        .iter()
        .zip(depth)
        .map(|(id, d)| (*id, d.unwrap_or(0)))
        .collect()
}

/// For each node, how many nodes depend on it directly or through others.
pub fn dependants(nodes: &[Node]) -> BTreeMap<NodeId, u32> {
    let graph = Graph::new(nodes);
    let mut seen = vec![usize::MAX; graph.len()];
    let mut pending = Vec::new();
    let mut out = BTreeMap::new();
    for start in 0..graph.len() {
        let mut count = 0u32;
        pending.push(start);
        while let Some(v) = pending.pop() {
            for &w in &graph.dependants[v] {
                if seen[w] != start {
                    seen[w] = start;
                    count += 1;
                    pending.push(w);
                }
            }
        }
        out.insert(graph.ids[start], count);
    }
    out
}

/// `ready` in hand-out order: the node with the most transitive dependants
/// first, then the lowest ID, so the work that unblocks most never starves.
pub fn dispatch_order(nodes: &[Node], ready: &[NodeId]) -> Vec<NodeId> {
    let counts = dependants(nodes);
    let mut ordered = ready.to_vec();
    ordered.sort_by_key(|id| (std::cmp::Reverse(counts.get(id).copied().unwrap_or(0)), *id));
    ordered
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::testkit::node;

    /// Node `id` with hard dependencies on `deps`.
    fn task(id: NodeId, deps: &[NodeId]) -> Node {
        let deps: Vec<_> = deps.iter().map(|dep| json!({"node": dep})).collect();
        node(id, json!({"deps": deps}))
    }

    fn map(pairs: &[(NodeId, u32)]) -> BTreeMap<NodeId, u32> {
        pairs.iter().copied().collect()
    }

    #[test]
    fn a_node_that_depends_on_itself_is_a_cycle_of_one() {
        let nodes = [task(1, &[]), task(2, &[2]), task(3, &[1])];
        assert_eq!(cycles(&nodes), vec![vec![2]]);
    }

    #[test]
    fn a_cycle_of_two_is_found() {
        let nodes = [task(1, &[2]), task(2, &[1]), task(3, &[1])];
        assert_eq!(cycles(&nodes), vec![vec![1, 2]]);
    }

    #[test]
    fn a_cycle_of_three_is_found_and_the_rest_is_not() {
        let nodes = [task(1, &[3]), task(2, &[1]), task(3, &[2]), task(4, &[3])];
        assert_eq!(cycles(&nodes), vec![vec![1, 2, 3]]);
    }

    #[test]
    fn separate_cycles_come_in_order_of_their_lowest_node() {
        let nodes = [
            task(5, &[2]),
            task(2, &[5]),
            task(3, &[4]),
            task(4, &[3]),
            task(6, &[]),
        ];
        assert_eq!(cycles(&nodes), vec![vec![2, 5], vec![3, 4]]);
    }

    #[test]
    fn a_plan_without_cycles_has_none_and_a_missing_dependency_is_no_edge() {
        let nodes = [task(1, &[]), task(2, &[1, 9]), task(3, &[2])];
        assert!(cycles(&nodes).is_empty());
        assert_eq!(layers(&nodes), map(&[(1, 0), (2, 1), (3, 2)]));
    }

    #[test]
    fn a_diamond_has_three_layers() {
        let nodes = [task(1, &[]), task(2, &[1]), task(3, &[1]), task(4, &[2, 3])];
        assert_eq!(layers(&nodes), map(&[(1, 0), (2, 1), (3, 1), (4, 2)]));
    }

    #[test]
    fn layers_take_the_longest_path() {
        let nodes = [task(1, &[]), task(2, &[1]), task(3, &[1, 2])];
        assert_eq!(layers(&nodes)[&3], 2);
    }

    #[test]
    fn cycles_are_found_and_layers_ignore_their_back_edges() {
        let nodes = [task(1, &[]), task(2, &[1]), task(3, &[2, 4]), task(4, &[3])];
        assert_eq!(cycles(&nodes), vec![vec![3, 4]]);
        let found = layers(&nodes);
        assert_eq!(found, map(&[(1, 0), (2, 1), (3, 2), (4, 3)]));
        // Without the cycle the layers stay where they were.
        let acyclic = [task(1, &[]), task(2, &[1]), task(3, &[2]), task(4, &[3])];
        assert_eq!(layers(&acyclic), found);
    }

    #[test]
    fn a_finding_sits_at_least_in_the_layer_of_its_origin() {
        let mut finding = task(4, &[]);
        finding.origin = Some(3);
        let nodes = [task(1, &[]), task(2, &[1]), task(3, &[2]), finding];
        assert_eq!(layers(&nodes)[&4], 2);
        // An origin loop ends instead of recursing for ever.
        let mut a = task(1, &[]);
        a.origin = Some(2);
        let mut b = task(2, &[]);
        b.origin = Some(1);
        assert_eq!(layers(&[a, b]), map(&[(1, 0), (2, 0)]));
    }

    #[test]
    fn dependants_of_a_chain_count_everything_after_each_node() {
        let nodes = [task(1, &[]), task(2, &[1]), task(3, &[2]), task(4, &[3])];
        assert_eq!(dependants(&nodes), map(&[(1, 3), (2, 2), (3, 1), (4, 0)]));
    }

    #[test]
    fn dependants_count_a_shared_dependant_once() {
        let nodes = [task(1, &[]), task(2, &[1]), task(3, &[1]), task(4, &[2, 3])];
        assert_eq!(dependants(&nodes)[&1], 3);
    }

    #[test]
    fn the_node_with_the_most_transitive_dependants_goes_first() {
        // 1 unblocks nothing, 2 unblocks 4 and through it 5, 3 unblocks 6; 7
        // and 8 tie, so the lower ID goes first.
        let nodes = [
            task(1, &[]),
            task(2, &[]),
            task(3, &[]),
            task(4, &[2]),
            task(5, &[4]),
            task(6, &[3]),
            task(7, &[]),
            task(8, &[]),
        ];
        assert_eq!(
            dispatch_order(&nodes, &[1, 2, 3, 7, 8]),
            vec![2, 3, 1, 7, 8]
        );
        assert_eq!(dispatch_order(&nodes, &[8, 7]), vec![7, 8]);
    }

    #[test]
    fn a_long_chain_needs_no_deep_stack() {
        // Run on a small stack: a recursive walk of this chain would overflow.
        let worker = std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(|| {
                let count = 20_000;
                let nodes: Vec<Node> = (1..=count)
                    .map(|id| {
                        if id == 1 {
                            task(id, &[])
                        } else {
                            task(id, &[id - 1])
                        }
                    })
                    .collect();
                assert!(cycles(&nodes).is_empty());
                assert_eq!(layers(&nodes)[&count], count - 1);
                let mut looped = nodes;
                looped[0]
                    .deps
                    .push(serde_json::from_value(json!({"node": count})).unwrap());
                assert_eq!(cycles(&looped)[0].len(), count as usize);
            })
            .unwrap();
        worker.join().unwrap();
    }
}
