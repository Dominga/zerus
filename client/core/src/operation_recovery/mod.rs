//! Recovery policies the live desktop uses: deferred input checkpoints, task
//! start and task message decisions, and the dormant creation STATUS
//! policies.
//!
//! Task #54 (team ede724c4, 2026-11-10): the review projection over the G3
//! switch import's records (G1 import records, observation of creation and
//! task outbox evidence) was removed with the Qt migration.
pub mod creation_clone;
pub mod creation_worktree;
pub mod deferred_input;
pub mod task_message;
pub mod task_start;
