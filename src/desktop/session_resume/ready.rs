//! The resumed session's readiness, the condition for the send
//! (zerus-5wmh.66.4.9, slice 3). After the resume drained and its own exact
//! receipt recorded the resumed run, and only within the same live
//! operation, the send may start once the selected session's own record of
//! that run shows its agent takes a message: the CLI's own record gates
//! ([`crate::state::resumed_readiness`]), read without any effect. `hgs send`
//! checks every gate again, its terminal included, under its input lock
//! before its first byte.
//!
//! `hgs session-action` records the resume once the agent's terminal
//! started; the session's supervisor writes the resumed run's record after
//! that. Until it exists the wait reads again. The record is read from the
//! held original root, through the same proof of exactly one record as the
//! target check, with the root checked before and after; a replaced root
//! ends the wait.
use super::receipt::Roots;
use crate::state::ResumedReadiness;
use zerus_client_core::desktop::session_resume::Plan;

/// The resumed session failed, ended or left its conversation.
pub(super) const READY_ENDED: &str = "session_resume_ready_ended";
/// The state path no longer names the held original root.
pub(super) const ROOT_CHANGED: &str = "session_resume_root_changed";
/// The read itself was lost (its worker ended without a result).
pub(super) const UNREAD: &str = "session_resume_ready_unread";

/// What one read of the resumed session says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Readiness {
    /// The resumed run's record shows its agent takes a message.
    Ready,
    /// Not yet: its record is not written yet, or the agent is still
    /// starting or busy. Read again.
    Waiting,
    /// The wait ends without the send, for the named reason.
    Ended(&'static str),
}

/// Reads the resumed run's record from the held root.
pub(super) fn read(roots: &Roots, plan: &Plan, run: &str) -> Readiness {
    if !roots.root_current() {
        return Readiness::Ended(ROOT_CHANGED);
    }
    let record = crate::desktop::original_store::session_record(
        roots,
        &plan.session,
        run,
        &plan.conversation_id,
    );
    if !roots.root_current() {
        return Readiness::Ended(ROOT_CHANGED);
    }
    match record {
        Some(record) => classify(&record),
        None => Readiness::Waiting,
    }
}

/// The rule, on the one record of the resumed run the target check proved.
pub(super) fn classify(record: &serde_json::Value) -> Readiness {
    match crate::state::resumed_readiness(record) {
        ResumedReadiness::Ready => Readiness::Ready,
        ResumedReadiness::Starting => Readiness::Waiting,
        ResumedReadiness::Ended => Readiness::Ended(READY_ENDED),
    }
}
