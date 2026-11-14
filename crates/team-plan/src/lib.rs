//! Agent team types and the pure rules that decide what may run.
//!
//! A team is a lead session, member sessions with roles, and their work: a
//! plan of phases, a queue of tasks sent one by one, or both. A team task is a
//! Zerus task; this crate holds the structure a team adds around it.
//!
//! Nothing here reads a file, a clock or a session. Callers pass state in and
//! get decisions back, so every rule can be tested without a running agent.
//!
//! Agent Teams were designed and first built by WhiteWh in the `WhiteWh/zerus`
//! fork. See `docs/superpowers/specs/2026-11-11-agent-teams-design.md`.

pub mod accounts;
pub mod builtin;
pub mod catalog;
pub mod graph;
pub mod model;
pub mod overlap;
pub mod pipeline;
pub mod problem;
pub mod ready;
#[cfg(test)]
mod testkit;
pub mod validate;

pub use builtin::builtin;
pub use catalog::{
    AccountChoice, Catalog, LeadSpec, Pipeline, Role, Seat, Stage, StageKind, Template,
    TemplateBundle,
};
pub use model::{
    Acceptance, Approval, Caps, ContextMode, Decider, Delegations, Dep, DepKind, Integration,
    Member, MemberId, MemberState, Node, NodeId, NodeKind, NodeSource, NodeState, PersonId, Phase,
    PhaseKind, PhaseMode, PhaseState, Routing, SessionKey, Settings, Severity, TaskRef, Team,
    TeamFixture, TeamId, TeamStatus, Timers,
};
pub use problem::Problem;
pub use validate::{
    admission_problems, bundle, forbidden_char, id_problem, name_problem, validate_bundle,
    validate_catalog, validate_ids,
};

/// True when a value equals its type's default; used to keep JSON short.
pub(crate) fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}
