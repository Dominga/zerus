//! Transactional team decisions and service-recorded facts. No operation
//! performs an effect in a session, a task document, or Git.
#![allow(dead_code)] // Used by the following CLI and service packages.
mod apply;
mod authority;
mod common;
mod decisions;
mod delegation;
pub(super) mod handout;
pub(crate) use handout::handout_admissible;
mod integration;
mod journal;
mod members;
mod pipeline;
mod queue;
mod readiness;
mod stage_task;
mod talk;
mod team;
mod timers;
mod types;
mod unstaffed;
pub use unstaffed::{unstaffed_changes, unstaffed_of, unstaffed_stages};
mod validation;
use super::store::{self, rows, DecisionState, Store};
#[allow(unused_imports)]
pub use apply::apply;
pub use authority::authority;
use common::*;
#[allow(unused_imports)] // The teams actor consumes this read next.
pub use readiness::{readiness, Readiness};
use rusqlite::{params, Connection, OptionalExtension};
use store::{decode, encode, storage};
pub use types::*;
use zerus_team_plan::{model::*, problem::code, Problem};
type Result<T, E = Problem> = std::result::Result<T, E>;

#[cfg(test)]
pub(crate) mod tests;
