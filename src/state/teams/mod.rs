//! Agent teams. A lead session plans; members (any agent,
//! model and computer) work through dependency-ordered tasks that a dispatcher
//! hands out. See docs/superpowers/specs/2026-10-24-agent-teams-core-design.md.
pub(crate) mod commands;
pub(crate) mod dispatcher;
pub(crate) mod model;
pub(crate) mod ops;
pub(crate) mod plan;
pub(crate) mod store;
