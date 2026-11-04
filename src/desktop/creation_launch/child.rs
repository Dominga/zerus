//! The one original launch child and the custody it holds.
//!
//! The child is this computer's own `hgs` with the plan's exact arguments, in
//! its own process group, with closed input and drained output. It inherits
//! the effect's close-only custody lock and, before anything else, marks it
//! close-on-exec ([`adopt_custody`]): the lock lives exactly as long as the
//! child, and nothing it starts (tmux, its server or the agent) inherits it.
//! Only that child is ever signalled, and only at the deadline; its process
//! group, tmux and agents never are. The runner itself is shared
//! ([`crate::desktop::original_child`]); this kind closes its input, keeps a
//! bounded prefix of stderr and closes its pipes on return.
use crate::desktop::original_child::{self, Codes, Input, Output, Spec};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use zerus_client_core::run::custody::InheritedLock;

/// Names the inherited custody descriptor of a creation launch child.
pub(super) const CUSTODY_FD: &str = "HGS_CREATION_LAUNCH_CUSTODY_FD";
/// Standard error retained before the wire bounds its encoded text.
const STDERR_BYTES: usize = 64 << 10;
/// This kind's reasons for an Unknown result.
const CODES: Codes = Codes {
    unobserved: "creation_launch_unobserved",
    unreadable: "creation_launch_output_unreadable",
    signalled: "creation_launch_signalled",
    timed_out: "creation_launch_timed_out",
    retained: "creation_launch_output_retained",
};

/// Called first in `main`. A launch child started by the desktop actor holds
/// the actor's close-only custody lock for exactly its own lifetime: the
/// inherited descriptor is marked close-on-exec before anything else runs,
/// and the variable naming it is removed. A malformed or unusable value
/// refuses the launch before it starts anything.
pub(crate) fn adopt_custody() -> Result<(), String> {
    original_child::adopt(
        CUSTODY_FD,
        "Invalid creation launch custody",
        "Creation launch custody is unavailable",
    )
}

/// How the original launch child ended.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Ended {
    /// It exited with `code`, and both output pipes ended, within the deadline.
    Exited { code: i32, stderr: String },
    /// It never started: refused before any child existed (the own image, its
    /// custody, or the spawn itself, which reports a fork or exec failure
    /// before any program code runs). Nothing ran.
    NotStarted,
    /// Its result is unknown, for the named reason: a signal ended it, the
    /// deadline passed (the child alone was then killed), or another process
    /// kept its output open past the deadline.
    Unknown(&'static str),
}

#[cfg(test)]
pub(super) use original_child::{READ_ERROR, REFUSE_PRE_EXEC, SPAWNED, WAIT_ERROR};

/// Runs `program args` as the original launch child under `custody`.
pub(super) fn run(
    program: &Path,
    args: &[String],
    custody: &Arc<InheritedLock>,
    deadline: Instant,
) -> Ended {
    let spec = Spec {
        custody_env: CUSTODY_FD,
        input: Input::Closed,
        output: Output::StderrPrefix(STDERR_BYTES),
        codes: CODES,
        retain: None,
    };
    match original_child::run(program, args, spec, custody, deadline, |_| Ok(())) {
        original_child::Ended::Exited { code, output } => Ended::Exited {
            code,
            stderr: String::from_utf8_lossy(&output).into_owned(),
        },
        original_child::Ended::NotStarted => Ended::NotStarted,
        original_child::Ended::Unknown(code) => Ended::Unknown(code),
    }
}
