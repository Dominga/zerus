//! The read-only observation of one original's own CLI input receipt
//! (zerus-5wmh.66.4.4, slice 3, G1 condition 2), from the original receipt
//! store only.
//!
//! `hgs send` writes `input_receipts/<request_id>.json` under the CLI's state
//! root, atomically: `in_progress` before the first input byte reaches the
//! terminal, then `submitted` after its Enter. Only an exact `submitted`
//! receipt of this original is a submission; it still proves nothing about
//! whether the agent read the message.
//!
//! The store is held and read through [`crate::desktop::original_store`]:
//! the state root and this kind's folder are opened before the original
//! child exists, and the receipt is read after the physical drain from those
//! exact objects ([`read`]). Every other observation is Unknown and never
//! authorizes a resend: `in_progress`, a missing, corrupt, oversized, unsafe,
//! replaced, changing or mismatched receipt, folder or root.
use crate::desktop::original_store::{self, Failure};
pub(super) use crate::desktop::original_store::{Evidence, Roots};
use zerus_client_core::desktop::deferred_send::{Outcome, Plan, RECEIPT_BYTES, RECEIPT_MISMATCH};

/// No receipt of the request exists in the original, unchanged store.
pub(super) const MISSING: &str = "deferred_send_receipt_missing";
/// The receipt could not be read safely: an unsafe folder or file, or an
/// I/O error.
pub(super) const UNAVAILABLE: &str = "deferred_send_receipt_unavailable";
/// The original root, folder or receipt was replaced, moved or changed
/// around the read.
pub(super) const CHANGED: &str = "deferred_send_receipt_changed";
/// The read did not end within its one bounded deadline.
pub(super) const TIMED_OUT: &str = "deferred_send_receipt_timed_out";
/// No original store observation is held for it (a restart): never read.
pub(super) const UNHELD: &str = "deferred_send_receipt_root_unheld";
/// The receipt folder's name under the CLI state root.
pub(super) const FOLDER: &str = "input_receipts";

/// What the original's own receipt says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Observed {
    /// The exact original's receipt is `submitted`.
    Submitted(Outcome),
    /// Anything else, for the named reason. Never resent.
    Unknown(&'static str),
}

/// Reads the original's receipt from `roots` after the physical drain.
/// `between` runs after the bytes are read and before they are checked
/// again (tests change the store there; it does nothing otherwise).
pub(super) fn read(roots: &Roots, plan: &Plan, between: &dyn Fn()) -> (Observed, Evidence) {
    let Ok(request) = uuid::Uuid::parse_str(&plan.request_id) else {
        return (Observed::Unknown(RECEIPT_MISMATCH), Evidence::default());
    };
    let (bytes, evidence) = original_store::read(roots, &request, RECEIPT_BYTES, between);
    let observed = match bytes {
        // The wire owns the matching rule: the original's own `submitted`
        // record, or no outcome.
        Ok(bytes) => match Outcome::from_receipt(plan, &bytes) {
            Ok(outcome) => Observed::Submitted(outcome),
            Err(code) => Observed::Unknown(code),
        },
        Err(Failure::Missing) => Observed::Unknown(MISSING),
        Err(Failure::Unavailable) => Observed::Unknown(UNAVAILABLE),
        Err(Failure::Changed) => Observed::Unknown(CHANGED),
    };
    (observed, evidence)
}
