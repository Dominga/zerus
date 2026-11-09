//! The original `hgs session-action <session> --json` and `hgs send <session>
//! --json` children and the custody each holds (zerus-5wmh.66.4.9, slice 3).
//!
//! The child is this computer's own `hgs` with the plan's exact arguments, in
//! its own process group. Its stdin is exactly the original's frozen bytes;
//! its output is drained and discarded within a bound, because only the
//! CLI's own receipt settles it. It inherits the effect's close-only custody
//! lock and, before anything else, marks it close-on-exec
//! ([`adopt_custody`]). Only that child is ever signalled, and only at the
//! deadline.
//!
//! The caller's resources guard (the custody lock and the original's
//! consent, with its lease and serve-owner loan) outlives every original
//! pipe: an output pipe still open on return keeps a guard copy in its own
//! reader until its real end. The runner is shared
//! ([`crate::desktop::original_child`]).
//!
//! The send's effect-free observation of a resumed Codex
//! (zerus-5wmh.66.4.10), `hgs send <session> --observe --json`, runs the
//! same way in its own process group with its frozen stdin, but holds no
//! custody: it has no effect for a drain to wait for. Its reply is kept
//! within a bound, and anything but a clean exit with a reply is no
//! observation.
// macOS admission refuses every original until a qualified child seam attests
// the code its child runs, so only tests start this runner there.
#![cfg_attr(target_os = "macos", allow(dead_code))]
use crate::desktop::original_child::{self, Codes, Input, Output, Retain, Spec, Stdin};
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;
use zerus_client_core::run::custody::InheritedLock;

/// Names the inherited custody descriptor of a session resume child.
pub(super) const CUSTODY_FD: &str = "HGS_SESSION_RESUME_CUSTODY_FD";
/// The reply kept to bound the drain; its content settles nothing.
const STDOUT_BYTES: usize = 64 << 10;
/// This kind's reasons for an Unknown result.
const CODES: Codes = Codes {
    unobserved: "session_resume_unobserved",
    unreadable: "session_resume_output_unreadable",
    signalled: "session_resume_signalled",
    timed_out: "session_resume_timed_out",
    retained: "session_resume_output_retained",
};
/// The observation's reasons; each one only means that it observed nothing.
const OBSERVATION_CODES: Codes = Codes {
    unobserved: "session_resume_observation_unobserved",
    unreadable: "session_resume_observation_unreadable",
    signalled: "session_resume_observation_signalled",
    timed_out: "session_resume_observation_timed_out",
    retained: "session_resume_observation_retained",
};

/// Called first in `main`. A session resume child started by the desktop
/// actor holds the actor's close-only custody lock for exactly its own
/// lifetime.
pub(crate) fn adopt_custody() -> Result<(), String> {
    original_child::adopt(
        CUSTODY_FD,
        "Invalid session resume custody",
        "Session resume custody is unavailable",
    )
}

/// How an original child ended. Its exit never settles it.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Ended {
    /// It read all of its stdin and exited with `code`, and both output pipes
    /// ended, within the deadline.
    Exited { code: i32 },
    /// It never started: refused before any child existed. Nothing ran.
    NotStarted,
    /// Its end is unknown, for the named reason.
    Unknown(&'static str),
}

/// Runs the send's observation child to its end, or to `deadline`: its
/// reply after a clean exit, or nothing. `final_check` runs inside its
/// spawn turn.
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
            unwritten: "session_resume_observation_input_unwritten",
        },
        output: Output::Stdout {
            bytes: super::ready::OBSERVATION_BYTES,
            too_large: "session_resume_observation_too_large",
        },
        codes: OBSERVATION_CODES,
        retain: None,
    };
    match original_child::run_unheld(program, args, spec, deadline, final_check) {
        original_child::Ended::Exited { code: 0, output } => Some(output),
        _ => None,
    }
}

/// Runs one original child to its end, or to the deadline. `final_check`
/// runs inside the spawn turn, last before the spawn: its refusal means that
/// no child ever existed.
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
            unwritten: "session_resume_input_unwritten",
        },
        output: Output::Stdout {
            bytes: STDOUT_BYTES,
            too_large: "session_resume_reply_too_large",
        },
        codes: CODES,
        retain: Some(Retain {
            guard: resources,
            reader: "session resume-output",
        }),
    };
    match original_child::run(program, args, spec, custody, deadline, final_check) {
        original_child::Ended::Exited { code, .. } => Ended::Exited { code },
        original_child::Ended::NotStarted => Ended::NotStarted,
        original_child::Ended::Unknown(code) => Ended::Unknown(code),
    }
}
