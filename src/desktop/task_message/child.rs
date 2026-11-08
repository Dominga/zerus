//! The one original `hgs task message` child and the custody it holds.
//!
//! The child is this computer's own `hgs` with the plan's exact arguments, in
//! its own process group. Its stdin is exactly the record's frozen input; its
//! output is drained, stdout kept within a bound for the exact reply and
//! stderr discarded. It inherits the effect's close-only custody lock and,
//! before anything else, marks it close-on-exec ([`adopt_custody`]): the lock
//! lives exactly as long as the child, and nothing it starts inherits it. Only
//! that child is ever signalled, and only at the deadline.
//!
//! The caller's resources guard (the same custody lock and the original's
//! consent, with its lease and serve-owner loan) outlives every original pipe:
//! an output pipe still open when this returns keeps a guard copy in its own
//! reader until its real end, whatever result was recorded. Nothing is
//! signalled for it. An end that can never be observed (a failed read or an
//! unverified wait) pins a guard copy until this process exits. The runner
//! itself is shared ([`crate::desktop::original_child`]).
// macOS admission refuses every original until a qualified child seam attests
// the code its child runs, so only tests start this runner there.
#![cfg_attr(target_os = "macos", allow(dead_code))]
use crate::desktop::original_child::{self, Codes, Input, Output, Retain, Spec};
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;
use zerus_client_core::run::custody::InheritedLock;

pub(super) use original_child::Stdin;

/// Names the inherited custody descriptor of a task message child.
pub(super) const CUSTODY_FD: &str = "HGS_TASK_MESSAGE_CUSTODY_FD";
/// The CLI reply retained to read its exact outcome; a larger one is Unknown.
pub(super) const STDOUT_BYTES: usize = 1 << 20;
/// This kind's reasons for an Unknown result.
const CODES: Codes = Codes {
    unobserved: "task_message_unobserved",
    unreadable: "task_message_output_unreadable",
    signalled: "task_message_signalled",
    timed_out: "task_message_timed_out",
    retained: "task_message_output_retained",
};

/// Called first in `main`, after the raw-input capture. A task message child
/// started by the desktop actor holds the actor's close-only custody lock for
/// exactly its own lifetime: the inherited descriptor is marked close-on-exec
/// before anything else runs, and the variable naming it is removed. A
/// malformed or unusable value refuses the call before it starts anything.
pub(crate) fn adopt_custody() -> Result<(), String> {
    original_child::adopt(
        CUSTODY_FD,
        "Invalid task message custody",
        "Task message custody is unavailable",
    )
}

/// How the original task message child ended.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Ended {
    /// It read all of its stdin and exited with `code`, and both output pipes
    /// ended, within the deadline; `stdout` is its whole reply.
    Exited { code: i32, stdout: Vec<u8> },
    /// It never started: refused before any child existed (the own image, its
    /// custody, or the spawn itself, which reports a fork or exec failure
    /// before any program code runs). Nothing ran.
    NotStarted,
    /// Its result is unknown, for the named reason: a signal ended it, the
    /// deadline passed (the child alone was then killed), its input or output
    /// was incomplete, or another process kept its output open.
    Unknown(&'static str),
}

#[cfg(test)]
pub(super) use original_child::{
    READ_ERROR, REFUSE_PRE_EXEC, SPAWNED, WAIT_ERROR, WAIT_ERROR_AFTER_OUTPUT,
};
// Only the Linux pinned-image tests rewrite an image after the final check.
#[cfg(all(test, target_os = "linux"))]
pub(super) use original_child::AFTER_FINAL_CHECK;

/// Runs the original child to its end, or to the deadline. `resources` is the
/// guard that every still-open original pipe keeps until its real end.
/// `final_check` runs inside the spawn turn, last before the spawn: its
/// refusal means that no child ever existed.
#[cfg(test)]
pub(super) fn run(
    program: &Path,
    args: &[String],
    input: &[u8],
    custody: &Arc<InheritedLock>,
    resources: &Arc<dyn Send + Sync>,
    deadline: Instant,
    final_check: impl FnOnce(&mut Command) -> std::io::Result<()>,
) -> Ended {
    run_stdin(
        program,
        args,
        Stdin::Bytes(input),
        custody,
        resources,
        deadline,
        final_check,
    )
}

/// [`run`] with either kind of stdin.
pub(super) fn run_stdin(
    program: &Path,
    args: &[String],
    input: Stdin<'_>,
    custody: &Arc<InheritedLock>,
    resources: &Arc<dyn Send + Sync>,
    deadline: Instant,
    final_check: impl FnOnce(&mut Command) -> std::io::Result<()>,
) -> Ended {
    let spec = Spec {
        custody_env: CUSTODY_FD,
        input: Input::Frozen {
            stdin: input,
            unwritten: "task_message_input_unwritten",
        },
        output: Output::Stdout {
            bytes: STDOUT_BYTES,
            too_large: "task_message_reply_too_large",
        },
        codes: CODES,
        retain: Some(Retain {
            guard: resources,
            reader: "task-message-output",
        }),
    };
    match original_child::run(program, args, spec, custody, deadline, final_check) {
        original_child::Ended::Exited { code, output } => Ended::Exited {
            code,
            stdout: output,
        },
        original_child::Ended::NotStarted => Ended::NotStarted,
        original_child::Ended::Unknown(code) => Ended::Unknown(code),
    }
}
