//! The read-only observation of one child's own CLI receipt
//! (zerus-5wmh.66.4.8, slice 3), from the original receipt store only.
//!
//! `hgs compact-context` writes `compact_receipts/<request_id>.json` under the
//! CLI's state root: an `in_progress` placeholder before the typed command,
//! then `submitted` after its Enter, or `uncertain`. Only an exact
//! `submitted` receipt of this original is a submission; it proves nothing
//! about whether the agent compacted.
//!
//! The store is held and read through [`crate::desktop::original_store`]: the
//! state root and the child's folder are opened before its child exists, and
//! the receipt is read after the physical drain from those exact objects.
//! Every other observation is Unknown and never authorizes a resend.
use crate::desktop::original_store::{self, Failure};
pub(super) use crate::desktop::original_store::{Evidence, Roots};
use zerus_client_core::desktop::{
    compact_to_send::{Compacted, Plan, RECEIPT_BYTES, RECEIPT_MISMATCH},
    deferred_send,
};

/// No receipt of the request exists in the original, unchanged store.
pub(super) const MISSING: &str = "compact_to_send_receipt_missing";
/// The receipt could not be read safely.
pub(super) const UNAVAILABLE: &str = "compact_to_send_receipt_unavailable";
/// The original root, folder or receipt was replaced, moved or changed
/// around the read.
pub(super) const CHANGED: &str = "compact_to_send_receipt_changed";
/// The read did not end within its one bounded deadline.
pub(super) const TIMED_OUT: &str = "compact_to_send_receipt_timed_out";
/// No original store observation is held for it (a restart): never read.
pub(super) const UNHELD: &str = "compact_to_send_receipt_root_unheld";
/// The compaction receipt folder's name under the CLI state root.
pub(super) const COMPACTION_FOLDER: &str = "compact_receipts";
/// The send receipt folder's name under the same root.
pub(super) const INPUT_FOLDER: &str = "input_receipts";

/// What a child's own receipt says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Observed {
    /// The exact compaction's receipt is `submitted`.
    Compacted(Compacted),
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

/// Reads the compaction's receipt from `roots` after the physical drain.
/// `between` runs after the bytes are read and before they are checked
/// again (tests change the store there; it does nothing otherwise).
pub(super) fn compaction(roots: &Roots, plan: &Plan, between: &dyn Fn()) -> (Observed, Evidence) {
    let Ok(request) = uuid::Uuid::parse_str(&plan.compaction_request_id) else {
        return (Observed::Unknown(RECEIPT_MISMATCH), Evidence::default());
    };
    let (bytes, evidence) = original_store::read(roots, &request, RECEIPT_BYTES, between);
    let observed = match bytes {
        // The wire owns the matching rule: the original's own `submitted`
        // record, or no outcome.
        Ok(bytes) => match Compacted::from_receipt(plan, &bytes) {
            Ok(outcome) => Observed::Compacted(outcome),
            Err(code) => Observed::Unknown(code),
        },
        Err(Failure::Missing) => Observed::Missing(Absence(())),
        Err(Failure::Unavailable) => Observed::Unknown(UNAVAILABLE),
        Err(Failure::Changed) => Observed::Unknown(CHANGED),
    };
    (observed, evidence)
}

/// Reads the send's receipt from `roots` (the same held root, its
/// `input_receipts` folder) after the physical drain, by the deferred send's
/// own rule for its plan.
pub(super) fn send(roots: &Roots, plan: &Plan, between: &dyn Fn()) -> (Observed, Evidence) {
    let send = plan.send_plan();
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
