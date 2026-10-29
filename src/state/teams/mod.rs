//! WhiteWh fork: agent teams. A lead session plans; members (any agent,
//! model and computer) work through dependency-ordered tasks that a dispatcher
//! hands out. See docs/superpowers/specs/2026-10-24-agent-teams-core-design.md.
pub(crate) mod catalog;
pub(crate) mod commands;
pub(crate) mod dispatcher;
pub(crate) mod inbox;
pub(crate) mod model;
pub(crate) mod models;
pub(crate) mod ops;
pub(crate) mod outbox;
pub(crate) mod owner_word;
pub(crate) mod pipeline;
pub(crate) mod plan;
pub(crate) mod safe_actions;
pub(crate) mod stages;
pub(crate) mod store;
pub(crate) mod templates;
pub(crate) mod waiting;
