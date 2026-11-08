//! The original `hgs compact-context <session> --json` child and the custody
//! it holds (zerus-5wmh.66.4.8, slice 3).
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
// macOS admission refuses every original until a qualified child seam attests
// the code its child runs, so only tests start this runner there.
#![cfg_attr(target_os = "macos", allow(dead_code))]
use crate::desktop::original_child::{self, Codes, Input, Output, Retain, Spec, Stdin};
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;
use zerus_client_core::run::custody::InheritedLock;

/// Names the inherited custody descriptor of a compact-to-send child.
pub(super) const CUSTODY_FD: &str = "HGS_COMPACT_TO_SEND_CUSTODY_FD";
/// The reply kept to bound the drain; its content settles nothing.
const STDOUT_BYTES: usize = 64 << 10;
/// This kind's reasons for an Unknown result.
const CODES: Codes = Codes {
    unobserved: "compact_to_send_unobserved",
    unreadable: "compact_to_send_output_unreadable",
    signalled: "compact_to_send_signalled",
    timed_out: "compact_to_send_timed_out",
    retained: "compact_to_send_output_retained",
};

/// Called first in `main`. A compact-to-send child started by the desktop
/// actor holds the actor's close-only custody lock for exactly its own
/// lifetime.
pub(crate) fn adopt_custody() -> Result<(), String> {
    original_child::adopt(
        CUSTODY_FD,
        "Invalid compact-to-send custody",
        "Compact-to-send custody is unavailable",
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
            unwritten: "compact_to_send_input_unwritten",
        },
        output: Output::Stdout {
            bytes: STDOUT_BYTES,
            too_large: "compact_to_send_reply_too_large",
        },
        codes: CODES,
        retain: Some(Retain {
            guard: resources,
            reader: "compact-to-send-output",
        }),
    };
    match original_child::run(program, args, spec, custody, deadline, final_check) {
        original_child::Ended::Exited { code, .. } => Ended::Exited { code },
        original_child::Ended::NotStarted => Ended::NotStarted,
        original_child::Ended::Unknown(code) => Ended::Unknown(code),
    }
}
