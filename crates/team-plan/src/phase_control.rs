//! Exact identities for the S5 phase controls. These values carry no authority.
//! The operation writer resolves and compares them under its existing lock.
use crate::model::SessionKey;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PhaseControlScope {
    Draft {
        phase: u32,
        generation: u64,
    },
    Published {
        phase: u32,
        generation: u64,
        publication: String,
        rework: Option<CorrectionScope>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorrectionScope {
    pub id: String,
    pub generation: Option<u64>,
    /// The reserved NEW publication, distinct from the outer retained BASE.
    pub approval: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhasePlacement {
    Before,
    After,
}

/// A bounded selected-phase read. Availability reports facts, not permission.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhaseControls {
    pub version: u32,
    pub scope: PhaseControlScope,
    pub pause: PhasePause,
    pub unstarted: Availability,
    pub barrier: Availability,
    pub pause_change: Availability,
    pub resume: Availability,
    pub acceptance: Availability,
    pub earlier: PhaseMove,
    pub later: PhaseMove,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum PhasePause {
    Running,
    Paused { episode: Option<PauseEpisode> },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PauseEpisode {
    pub id: String,
    pub owner: PauseOwner,
    pub reason: String,
    pub created_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PauseOwner {
    Person {
        person: String,
    },
    Lead {
        person: String,
        member: String,
        session: SessionKey,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Availability {
    pub allowed: bool,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhaseMove {
    pub target: Option<PhaseControlScope>,
    pub availability: Availability,
}

impl PhaseControlScope {
    pub fn phase(&self) -> u32 {
        match self {
            Self::Draft { phase, .. } | Self::Published { phase, .. } => *phase,
        }
    }
}
