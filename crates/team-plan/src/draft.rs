//! Draft input shared by the service and the window. No execution identity is
//! accepted here: only approval may turn a draft into a task.
use serde::{Deserialize, Serialize};

use crate::model::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DraftInput {
    pub title: String,
    #[serde(default)]
    pub barrier: bool,
    pub nodes: Vec<DraftNode>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DraftNode {
    /// Existing draft ID, or the next ID from the revisioned phases read.
    pub id: NodeId,
    pub title: String,
    #[serde(default)]
    pub text: String,
    pub criteria: Vec<String>,
    pub role: String,
    pub pipeline: String,
    #[serde(default)]
    pub deps: Vec<Dep>,
    #[serde(default)]
    pub assignee: Option<MemberId>,
    #[serde(default)]
    pub writes: Vec<String>,
    #[serde(default)]
    pub estimate_minutes: u32,
    #[serde(default)]
    pub kind: NodeKind,
}

impl DraftNode {
    /// The existing graph/kernel shape, without a task or any runtime facts.
    pub fn node(&self, phase: u32) -> Node {
        Node {
            id: self.id,
            phase,
            task: None,
            title: self.title.clone(),
            text: self.text.clone(),
            source: NodeSource::Planned,
            deps: self.deps.clone(),
            assignee: self.assignee.clone(),
            role: self.role.clone(),
            pipeline: self.pipeline.clone(),
            writes: self.writes.clone(),
            estimate_minutes: self.estimate_minutes,
            kind: self.kind,
            earlier_implementers: Vec::new(),
            severity: Severity::None,
            origin: None,
            stage_of: None,
            stage_index: 0,
            round: 1,
            implementer: None,
            hold: false,
            admitted: true,
            state: NodeState::Draft,
            decided_by: None,
            integrated: false,
            rank: String::new(),
        }
    }
}
