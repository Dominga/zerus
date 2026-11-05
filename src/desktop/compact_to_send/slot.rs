//! The session's own compaction slot, the condition for the send
//! (zerus-5wmh.66.4.8, slice 3). After the compaction child drained, and
//! only within the same live operation, the send may start once the
//! selected session's own record shows this compaction completed: the
//! record's `compact_context_request` names this request, run and
//! conversation, its status is `completed` and not superseded, and the
//! session is idle (activity and phase). That is exactly what the CLI's own
//! `check_continuation` checks again under its input lock before the send's
//! first byte.
//!
//! The slot is a continuation condition, never evidence about the
//! compaction child: hooks carry no request ID, so a `/compact` typed in
//! Terminal while this one is submitted completes it too. The compaction's
//! own result stays what its receipt proved.
//!
//! The record is read from the held original root, through the same proof
//! of exactly one record as the target check, with the root checked before
//! and after; a replaced root ends the wait.
use super::receipt::Roots;
use serde_json::Value;
use zerus_client_core::desktop::compact_to_send::Plan;

/// The slot no longer names this compaction, or ended without completing it
/// (cancelled, superseded, uncertain or another request).
pub(super) const SLOT_ENDED: &str = "compact_to_send_slot_ended";
/// No single record of the session names the plan's run and conversation.
pub(super) const SESSION_CHANGED: &str = "compact_to_send_session_changed";
/// The state path no longer names the held original root.
pub(super) const ROOT_CHANGED: &str = "compact_to_send_root_changed";
/// The read itself was lost (its worker ended without a result).
pub(super) const UNREAD: &str = "compact_to_send_slot_unread";

/// What one read of the slot says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Slot {
    /// This compaction completed and the session is idle: the send may start.
    Completed,
    /// Still submitted or compacting, or completed while the session is not
    /// yet idle: read again.
    Waiting,
    /// The wait ends without the send, for the named reason.
    Ended(&'static str),
}

/// Reads the slot from the held root.
pub(super) fn read(roots: &Roots, plan: &Plan) -> Slot {
    if !roots.root_current() {
        return Slot::Ended(ROOT_CHANGED);
    }
    let record = crate::desktop::original_store::session_record(
        roots,
        &plan.session,
        &plan.run_id,
        &plan.conversation_id,
    );
    if !roots.root_current() {
        return Slot::Ended(ROOT_CHANGED);
    }
    match record {
        Some(record) => classify(&record, plan),
        None => Slot::Ended(SESSION_CHANGED),
    }
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

/// The rule, on one session record the target check proved.
pub(super) fn classify(record: &Value, plan: &Plan) -> Slot {
    let request = &record["compact_context_request"];
    if text(request, "request_id") != plan.compaction_request_id
        || text(request, "run_id") != plan.run_id
        || text(request, "conversation_id") != plan.conversation_id
    {
        return Slot::Ended(SLOT_ENDED);
    }
    match text(request, "status") {
        "submitted" | "compacting" => Slot::Waiting,
        "completed" if !request["superseded_at"].is_null() => Slot::Ended(SLOT_ENDED),
        "completed" if text(record, "activity") == "idle" && text(record, "phase") == "idle" => {
            Slot::Completed
        }
        "completed" => Slot::Waiting,
        _ => Slot::Ended(SLOT_ENDED),
    }
}
