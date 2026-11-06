//! The deferred answer's two children: the effect-free observation of the
//! native current question, and the one original `hgs answer <session>
//! --json` child with the custody it holds.
//!
//! Both are this computer's own pinned `hgs` image, each in its own process
//! group with exactly its frozen stdin, run by the shared runner
//! ([`crate::desktop::original_child`]). The observation, `hgs answer
//! <session> --observe --json`, holds no custody: it has no effect for a
//! drain to wait for. Its reply is kept within a bound and read only by the
//! actor's consent comparison; anything but a clean exit with a reply is no
//! observation, and the answer never starts.
//!
//! The answer child inherits the effect's close-only custody lock and,
//! before anything else, marks it close-on-exec ([`adopt_custody`]). Its
//! output is drained and discarded within a bound, because only the CLI's
//! own receipt settles the answer. Only that child is ever signalled, and
//! only at the deadline. The caller's resources guard (the custody lock and
//! the original's consent, with its lease and serve-owner loan) outlives
//! every original pipe: an output pipe still open on return keeps a guard
//! copy in its own reader until its real end.
// macOS admission refuses every original until a qualified child seam attests
// the code its child runs, so only tests start this runner there.
#![cfg_attr(target_os = "macos", allow(dead_code))]
use crate::desktop::original_child::{self, Codes, Input, Output, Retain, Spec, Stdin};
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;
use zerus_client_core::run::custody::InheritedLock;

/// Names the inherited custody descriptor of a deferred answer child.
pub(super) const CUSTODY_FD: &str = "HGS_DEFERRED_ANSWER_CUSTODY_FD";
/// The reply kept to bound the drain; its content settles nothing.
const STDOUT_BYTES: usize = 64 << 10;
/// The largest observation reply: the CLI's bound and its newline.
pub(super) const OBSERVATION_BYTES: usize = (64 << 10) + 1;
/// This kind's reasons for an Unknown result.
const CODES: Codes = Codes {
    unobserved: "deferred_answer_unobserved",
    unreadable: "deferred_answer_output_unreadable",
    signalled: "deferred_answer_signalled",
    timed_out: "deferred_answer_timed_out",
    retained: "deferred_answer_output_retained",
};
/// The observation's reasons; each one only means that it observed nothing.
const OBSERVATION_CODES: Codes = Codes {
    unobserved: "deferred_answer_observation_unobserved",
    unreadable: "deferred_answer_observation_unreadable",
    signalled: "deferred_answer_observation_signalled",
    timed_out: "deferred_answer_observation_timed_out",
    retained: "deferred_answer_observation_retained",
};

/// Called first in `main`. A deferred answer child started by the desktop
/// actor holds the actor's close-only custody lock for exactly its own
/// lifetime.
pub(crate) fn adopt_custody() -> Result<(), String> {
    original_child::adopt(
        CUSTODY_FD,
        "Invalid deferred answer custody",
        "Deferred answer custody is unavailable",
    )
}

/// How the original child ended. Its exit never settles the answer.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Ended {
    /// It read all of its stdin and exited with `code`, and both output pipes
    /// ended, within the deadline.
    Exited { code: i32 },
    /// It never started: refused before any child existed, the observation
    /// included. Nothing was answered.
    NotStarted,
    /// Its end is unknown, for the named reason.
    Unknown(&'static str),
}

/// Runs the observation child to its end, or to `deadline`: its reply after
/// a clean exit, or nothing. `final_check` runs inside its spawn turn.
pub(super) fn observe(
    program: &Path,
    args: &[String],
    input: &[u8],
    deadline: Instant,
    final_check: impl FnOnce(&mut Command) -> std::io::Result<()>,
) -> Option<Vec<u8>> {
    let spec = Spec {
        custody_env: CUSTODY_FD,
        input: Input::Frozen {
            stdin: Stdin::Bytes(input),
            unwritten: "deferred_answer_observation_input_unwritten",
        },
        output: Output::Stdout {
            bytes: OBSERVATION_BYTES,
            too_large: "deferred_answer_observation_too_large",
        },
        codes: OBSERVATION_CODES,
        retain: None,
    };
    match original_child::run_unheld(program, args, spec, deadline, final_check) {
        original_child::Ended::Exited { code: 0, output } => Some(output),
        _ => None,
    }
}

/// Runs the original child to its end, or to the deadline. `final_check` runs
/// inside the spawn turn, last before the spawn: its refusal means that no
/// child ever existed.
pub(super) fn run(
    program: &Path,
    args: &[String],
    input: &[u8],
    custody: &Arc<InheritedLock>,
    resources: &Arc<dyn Send + Sync>,
    deadline: Instant,
    final_check: impl FnOnce(&mut Command) -> std::io::Result<()>,
) -> Ended {
    let spec = Spec {
        custody_env: CUSTODY_FD,
        input: Input::Frozen {
            stdin: Stdin::Bytes(input),
            unwritten: "deferred_answer_input_unwritten",
        },
        output: Output::Stdout {
            bytes: STDOUT_BYTES,
            too_large: "deferred_answer_reply_too_large",
        },
        codes: CODES,
        retain: Some(Retain {
            guard: resources,
            reader: "deferred-answer-output",
        }),
    };
    match original_child::run(program, args, spec, custody, deadline, final_check) {
        original_child::Ended::Exited { code, .. } => Ended::Exited { code },
        original_child::Ended::NotStarted => Ended::NotStarted,
        original_child::Ended::Unknown(code) => Ended::Unknown(code),
    }
}
