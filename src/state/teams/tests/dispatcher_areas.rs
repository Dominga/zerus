// The dispatcher's area tests that share these imports. A file, not an
// inline module: an inline module's #[path] resolves under
// teams/dispatcher/tests/, which does not exist, and Linux and macOS
// refuse the "../.." through it (Windows let it pass).
use super::super::ops::{self, Actor};
use super::*;
use super::support::*;

#[path = "dispatcher_replies.rs"]
mod dispatcher_replies;

#[path = "dispatcher_inbox_questions.rs"]
mod dispatcher_inbox_questions;

#[path = "dispatcher_handout.rs"]
mod dispatcher_handout;

#[path = "dispatcher_report_holds.rs"]
mod dispatcher_report_holds;

#[path = "dispatcher_handoff_brief.rs"]
mod dispatcher_handoff_brief;

#[path = "dispatcher_worktrees.rs"]
mod dispatcher_worktrees;

#[path = "deepseek_folder.rs"]
mod deepseek_folder;

#[path = "dispatcher_worker.rs"]
mod dispatcher_worker;

#[path = "dispatcher_stage_land.rs"]
mod dispatcher_stage_land;

#[path = "dispatcher_phase_start.rs"]
mod dispatcher_phase_start;

#[path = "dispatcher_starved.rs"]
mod dispatcher_starved;

#[path = "dispatcher_landing_wake.rs"]
mod dispatcher_landing_wake;

#[path = "dispatcher_revoked.rs"]
mod dispatcher_revoked;

#[path = "dispatcher_run_settled.rs"]
mod dispatcher_run_settled;

#[path = "dispatcher_outputs.rs"]
mod dispatcher_outputs;

#[path = "dispatcher_doorbell.rs"]
mod dispatcher_doorbell;

#[path = "dispatcher_cross_role.rs"]
mod dispatcher_cross_role;

#[path = "dispatcher_holds.rs"]
mod dispatcher_holds;

#[path = "dispatcher_urgent.rs"]
mod dispatcher_urgent;
