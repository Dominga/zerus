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

impl PauseOwner {
    /// Validate stored attribution; this does not verify a caller or binding.
    pub fn valid(&self) -> bool {
        let text = |s: &str, max: usize| {
            !s.trim().is_empty() && s.len() <= max && crate::forbidden_char(s).is_none()
        };
        match self {
            Self::Person { person } => text(person, 200),
            Self::Lead {
                person,
                member,
                session,
            } => {
                let identity = session
                    .session_id
                    .strip_prefix("launch_id:")
                    .is_some_and(canonical_uuid)
                    || session
                        .session_id
                        .strip_prefix("run_id:")
                        .is_some_and(|s| text(s, 256));
                text(person, 200)
                    && text(member, 200)
                    && canonical_uuid(&session.machine)
                    && identity
            }
        }
    }
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

    /// Shape only. Current-row identity and Actor admission remain writer facts.
    pub fn valid(&self) -> bool {
        const MAX_EXACT_JSON_INTEGER: u64 = (1 << 53) - 1;
        let generation = |g: u64| g > 0 && g <= MAX_EXACT_JSON_INTEGER;
        match self {
            Self::Draft {
                phase,
                generation: g,
            } => *phase > 0 && generation(*g),
            Self::Published {
                phase,
                generation: g,
                publication,
                rework,
            } => {
                *phase > 0
                    && generation(*g)
                    && canonical_uuid(publication)
                    && rework.as_ref().is_none_or(|r| {
                        canonical_uuid(&r.id)
                            && r.id != *publication
                            && r.generation.is_none_or(|v| generation(v) && v > *g)
                            && r.approval.as_ref().is_none_or(|id| {
                                r.generation.is_some()
                                    && canonical_uuid(id)
                                    && id != publication
                                    && *id != r.id
                            })
                    })
            }
        }
    }
}

fn canonical_uuid(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_digit() || (b'a'..=b'f').contains(&b),
        })
}
