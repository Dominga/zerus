//! The read-only observation of one child's own CLI receipt
//! (zerus-5wmh.66.4.9, slice 3), from the original receipt store only.
//!
//! `hgs session-action` writes `action_receipts/<request_id>.json` under the
//! CLI's state root: `claimed` before the resume, then `completed` with the
//! resumed run once its terminal started, `failed` only before its launch
//! began, or `uncertain`. The resume's bytes are projected by the CLI's own
//! observation rule ([`crate::state::observe_held_lifecycle_receipt`], the
//! rule `session_action::status::observe_original` applies) and then matched
//! by the wire. `hgs send` writes `input_receipts/<request_id>.json`, read by
//! the deferred send's own rule for the send bound to the resumed run.
//!
//! The store is held and read through [`crate::desktop::original_store`]: the
//! state root and the child's folder are opened before its child exists, and
//! the receipt is read after the physical drain from those exact objects,
//! never by path. Every other observation is Unknown and never authorizes a
//! resend.
use crate::desktop::original_store::{self, Failure};
pub(super) use crate::desktop::original_store::{Evidence, Roots};
use zerus_client_core::desktop::{
    deferred_send,
    session_resume::{Plan, Resumed, RECEIPT_BYTES, RECEIPT_INVALID, RECEIPT_MISMATCH, REFUSED},
};

/// No receipt of the request exists in the original, unchanged store.
pub(super) const MISSING: &str = "session_resume_receipt_missing";
/// The receipt could not be read safely.
pub(super) const UNAVAILABLE: &str = "session_resume_receipt_unavailable";
/// The original root, folder or receipt was replaced, moved or changed
/// around the read.
pub(super) const CHANGED: &str = "session_resume_receipt_changed";
/// The read did not end within its one bounded deadline.
pub(super) const TIMED_OUT: &str = "session_resume_receipt_timed_out";
/// No original store observation is held for it (a restart): never read.
pub(super) const UNHELD: &str = "session_resume_receipt_root_unheld";
/// The resume receipt folder's name under the CLI state root.
pub(super) const RESUME_FOLDER: &str = "action_receipts";
/// The send receipt folder's name under the same root.
pub(super) const INPUT_FOLDER: &str = "input_receipts";

/// What a child's own receipt says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Observed {
    /// The exact resume's receipt is `completed`, with its resumed target.
    Resumed(Resumed),
    /// The exact resume's receipt is `failed`: the CLI refused it before its
    /// launch began, so nothing was launched.
    Refused,
    /// The exact send's receipt is `submitted`.
    Sent(deferred_send::Outcome),
    /// The original store this drain held was read and holds no receipt of
    /// the request. Only a read here constructs it, so no caller can claim
    /// an absence it never read.
    Missing(Absence),
    /// Anything else, for the named reason. Never resent.
    Unknown(&'static str),
}

/// A held store's read found no receipt. Its field is private to this
/// module: a path that never read a held store cannot make one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Absence(());

/// Reads the resume's receipt from `roots` after the physical drain.
/// `original` is the resume's exact frozen stdin, which the CLI's own rule
/// decodes again. `between` runs after the bytes are read and before they
/// are checked again (tests change the store there; it does nothing
/// otherwise).
pub(super) fn resume(
    roots: &Roots,
    plan: &Plan,
    original: &[u8],
    between: &dyn Fn(),
) -> (Observed, Evidence) {
    let Ok(request) = uuid::Uuid::parse_str(&plan.resume_request_id) else {
        return (Observed::Unknown(RECEIPT_MISMATCH), Evidence::default());
    };
    let (bytes, evidence) = original_store::read(roots, &request, RECEIPT_BYTES, between);
    let observed = match bytes {
        // The CLI's own observation rule projects the held bytes; the wire
        // owns the matching rule: the original's own `completed` record, its
        // exact `failed` one, or no outcome.
        Ok(bytes) => {
            match crate::state::observe_held_lifecycle_receipt(
                &plan.session,
                original,
                Some(&bytes),
            )
            .ok()
            .and_then(|value| serde_json::to_vec(&value).ok())
            {
                Some(projected) => match Resumed::from_receipt(plan, &projected) {
                    Ok(outcome) => Observed::Resumed(outcome),
                    Err(REFUSED) => Observed::Refused,
                    Err(code) => Observed::Unknown(code),
                },
                None => Observed::Unknown(RECEIPT_INVALID),
            }
        }
        Err(Failure::Missing) => Observed::Missing(Absence(())),
        Err(Failure::Unavailable) => Observed::Unknown(UNAVAILABLE),
        Err(Failure::Changed) => Observed::Unknown(CHANGED),
    };
    (observed, evidence)
}

/// Reads the send's receipt from `roots` (the same held root, its
/// `input_receipts` folder) after the physical drain, by the deferred send's
/// own rule for the send bound to `resumed` with the stdin digest
/// `input_sha256`.
pub(super) fn send(
    roots: &Roots,
    plan: &Plan,
    resumed: &Resumed,
    input_sha256: &str,
    between: &dyn Fn(),
) -> (Observed, Evidence) {
    let send = plan.send_plan(resumed, input_sha256);
    let Ok(request) = uuid::Uuid::parse_str(&send.request_id) else {
        return (Observed::Unknown(RECEIPT_MISMATCH), Evidence::default());
    };
    let (bytes, evidence) =
        original_store::read(roots, &request, deferred_send::RECEIPT_BYTES, between);
    let observed = match bytes {
        Ok(bytes) => match deferred_send::Outcome::from_receipt(&send, &bytes) {
            Ok(outcome) => Observed::Sent(outcome),
            Err(code) => Observed::Unknown(code),
        },
        Err(Failure::Missing) => Observed::Missing(Absence(())),
        Err(Failure::Unavailable) => Observed::Unknown(UNAVAILABLE),
        Err(Failure::Changed) => Observed::Unknown(CHANGED),
    };
    (observed, evidence)
}
