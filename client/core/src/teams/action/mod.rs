//! The team actions the desktop offers, as guarded `hgs team <op> --json`
//! calls with a request identity.
//!
//! Every call names the team, the action, the revision the view was read at
//! and the intent's request id. The id is made once, when the person triggers
//! the action, and the core never generates or replaces it: a lost reply, a
//! reopened window or a retry after a refresh all keep one intent instead of
//! becoming new store requests. The store keeps the receipt of every id, so a
//! call sent again with the same id answers its stored receipt instead of
//! applying the change twice.
//!
//! [`TeamOutcome`] is what the view shows:
//!
//! - `done`: hgs applied it and gave a complete receipt (a revision, an actor
//!   and the call's team);
//! - `failed`: hgs refused it with a stable code, or the call never ran;
//!   nothing was applied, and a new attempt may get a new request id;
//! - `uncertain`: the call timed out, ended abnormally or gave no complete
//!   receipt, so the change may have been applied. The desktop never sends it
//!   again by itself; only the person may ask to check it, with the same
//!   request id, and the store answers its record;
//! - `confirm`: a destructive action was asked for without `confirmed`;
//!   nothing ran, and the confirmation names what the person is asked;
//! - `pending`: the same intent is still being sent; nothing new was sent.
//!
//! An uncertain intent has two ways out, both the person's: [`Intents::check_again`]
//! sends the same id and content again (the store answers from its record),
//! and [`Intents::dismiss`] settles it as abandoned without sending.
//!
//! [`Intents`] keeps a small bounded table of intents by request id with their
//! state, so a second call for a pending intent does nothing and a call for a
//! settled one returns its recorded outcome. The table is in memory only: a
//! restart forgets it, and the new gesture that follows carries a new id and
//! the revision the person saw. Only settled entries make room; pending and
//! uncertain intents are never evicted, and the table refuses a new intent
//! when every entry is unresolved.
pub mod editing;
mod input;
pub mod intents;
pub mod types;
pub mod wire;

#[cfg(test)]
mod tests;

pub use intents::{Admission, IntentState, Intents, Pending};
pub use types::{canonical_uuid, Confirmation, TeamAction, TeamCall, TeamOutcome};
pub use wire::{classify, prepare, run_action};
