//! Small builders for the kernel's unit tests. Values come from JSON so every
//! field a test does not name takes its serde default, as a stored row would.

use serde_json::{json, Value};

use crate::catalog::{Catalog, Pipeline};
use crate::model::{Integration, MemberState, Node, NodeId, Phase, LEAD, QUEUE_PHASE};
use crate::ready::{MemberView, PlanView};

/// Applies `fields` over `base` and parses the result.
fn build<T: serde::de::DeserializeOwned>(mut base: Value, fields: Value) -> T {
    let object = base.as_object_mut().expect("an object");
    for (key, value) in fields.as_object().expect("an object") {
        object.insert(key.clone(), value.clone());
    }
    serde_json::from_value(base).expect("a valid test value")
}

/// A pending developer task in planned phase 1 with the given fields over it.
pub fn node(id: NodeId, fields: Value) -> Node {
    build(
        json!({"id": id, "phase": 1, "title": format!("Task {id}"), "role": "developer", "state": "pending"}),
        fields,
    )
}

/// An approved planned phase with the given fields over it.
pub fn phase(n: u32, fields: Value) -> Phase {
    build(json!({"n": n, "kind": "plan", "state": "approved"}), fields)
}

/// The team's approved queue phase.
pub fn queue_phase() -> Phase {
    phase(QUEUE_PHASE, json!({"kind": "queue", "mode": "free"}))
}

/// A pending queue task with the given board rank.
pub fn queued(id: NodeId, rank: &str) -> Node {
    node(
        id,
        json!({"phase": QUEUE_PHASE, "source": "sent", "rank": rank,
               "task": {"project": "example-project", "uuid": format!("00000000-0000-4000-8000-{id:012}")}}),
    )
}

/// An active, free member.
pub fn member(id: &str, role: &str) -> MemberView {
    MemberView {
        id: id.to_owned(),
        role: role.to_owned(),
        state: MemberState::Active,
        free: true,
    }
}

/// A plan, its queue and a team to view them with.
pub struct Plan {
    pub phases: Vec<Phase>,
    pub nodes: Vec<Node>,
    pub members: Vec<MemberView>,
    pub integration: Integration,
}

impl Plan {
    /// Phase 1 approved and the queue open; a lead, developers `m1` and `m2`
    /// and reviewer `m3`, all free.
    pub fn new(nodes: Vec<Node>) -> Self {
        Self {
            phases: vec![queue_phase(), phase(1, json!({}))],
            nodes,
            members: vec![
                member(LEAD, LEAD),
                member("m1", "developer"),
                member("m2", "developer"),
                member("m3", "reviewer"),
            ],
            integration: Integration::Branch,
        }
    }

    pub fn view(&self) -> PlanView<'_> {
        PlanView {
            phases: &self.phases,
            nodes: &self.nodes,
            members: &self.members,
            lead: LEAD,
            integration: self.integration,
        }
    }

    pub fn node(&mut self, id: NodeId) -> &mut Node {
        self.nodes
            .iter_mut()
            .find(|node| node.id == id)
            .expect("a node")
    }

    pub fn phase(&mut self, n: u32) -> &mut Phase {
        self.phases
            .iter_mut()
            .find(|phase| phase.n == n)
            .expect("a phase")
    }

    pub fn member(&mut self, id: &str) -> &mut MemberView {
        self.members
            .iter_mut()
            .find(|member| member.id == id)
            .expect("a member")
    }
}

/// A built-in pipeline by ID, as the shared fixture defines it.
pub fn builtin_pipeline(id: &str) -> Pipeline {
    let catalog: Catalog =
        serde_json::from_str(include_str!("../fixtures/catalog-builtin.json")).expect("catalog");
    catalog
        .pipelines
        .into_iter()
        .find(|pipeline| pipeline.id == id)
        .expect("a built-in pipeline")
}
