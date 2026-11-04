//! The explicit review and release of the desktop service's charged
//! task message custody inputs (zerus-pi2k9, under G1's conditions).
//!
//! The service keeps one record per reference-form custody input. An
//! `orphaned` record (published, never admitted) or a `failed` one (its exact
//! removal failed after its original drained) stays charged until a person
//! reviews it here. A review observes the record's one canonical slot,
//! `task-message-inputs/<request_id>`, and a release names exactly what that
//! review saw:
//! - a present object is removed only while it is still that exact private
//!   object and its bytes are this request's frozen stdin (size and SHA-256 of
//!   the immutable record). This proves the bytes, not a historical inode: the
//!   deletion target is the object the review observed.
//! - an absent slot releases the record's charge only after a fresh, bounded
//!   check finds no copy of those bytes elsewhere in the folder. That is an
//!   accounting release: it proves neither that the input was destroyed
//!   everywhere nor any drain.
//!
//! Neither changes an execution outcome, a receipt or whether the request may
//! be sent again, and a released request ID never reserves again.
use super::Scope;
use serde::{Deserialize, Serialize};

/// The read-only listing operation.
pub const LIST_OP: &str = "task_message_inputs";
/// The most records one page carries.
pub const PAGE: usize = 16;
/// The record states a review may release.
pub const STATES: [&str; 2] = ["orphaned", "failed"];

/// One object's exact version, as a review observed it.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Version {
    pub device: u64,
    pub inode: u64,
    pub size: u64,
    pub mode: u32,
    pub links: u64,
    /// Seconds and nanoseconds of the last content change.
    pub modified: (i64, i64),
    /// Seconds and nanoseconds of the last status change.
    pub changed: (i64, i64),
}

/// What a review saw at a record's canonical slot.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Observation {
    /// Nothing at the name.
    Absent,
    /// A private regular file of this user with one link.
    Present { version: Version },
    /// Something else, or an object that could not be observed: never
    /// released from this review.
    Unsafe,
}

/// A stable position in the listing: records are ordered by creation time,
/// then request ID.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Cursor {
    pub created: i64,
    pub request_id: String,
}

/// The read-only listing request: one bounded page after `after`.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct List {
    pub op: String,
    pub scope: Scope,
    pub after: Option<Cursor>,
}
impl List {
    pub fn valid(&self) -> bool {
        self.op == LIST_OP
            && self.scope.valid()
            && self
                .after
                .as_ref()
                .is_none_or(|cursor| super::uuid(&cursor.request_id))
    }
}

/// One charged record and the fresh observation of its canonical slot.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub request_id: String,
    pub state: String,
    pub bytes: u64,
    pub input_sha256: String,
    pub created: i64,
    /// The identity the record names, only for a `failed` record.
    pub recorded: Option<(u64, u64)>,
    pub observed: Observation,
}

/// One listing page. `next` continues it; it is absent on the last page.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Page {
    pub abi: String,
    pub scope: Scope,
    pub read_only: bool,
    pub entries: Vec<Entry>,
    pub next: Option<Cursor>,
    pub code: String,
}
impl Page {
    pub fn matches(&self, list: &List) -> bool {
        self.abi == super::ABI
            && self.read_only
            && self.scope == list.scope
            && self.entries.len() <= PAGE
            && self.entries.iter().all(|entry| {
                super::uuid(&entry.request_id)
                    && STATES.contains(&entry.state.as_str())
                    && super::super::digest(&entry.input_sha256)
            })
    }
}

/// The explicit release of exactly what one review saw. `observed` is the
/// review's version of the present object, or `None` when it saw the slot
/// absent.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Release {
    pub scope: Scope,
    /// This release's own identity: one confirmation, sent once.
    pub release_id: String,
    pub request_id: String,
    pub state: String,
    pub bytes: u64,
    pub input_sha256: String,
    pub observed: Option<Version>,
}
impl Release {
    pub fn valid(&self) -> bool {
        self.scope.valid()
            && super::uuid(&self.release_id)
            && super::uuid(&self.request_id)
            && self.release_id != self.request_id
            && STATES.contains(&self.state.as_str())
            && (1..=64 << 20).contains(&self.bytes)
            && super::super::digest(&self.input_sha256)
            && self
                .observed
                .is_none_or(|version| version.size == self.bytes && version.links == 1)
    }
}

/// What one release did.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// This review removed its exact observed object and synced the folder;
    /// the record is `removed` and uncharged.
    Removed,
    /// The reviewed slot is still absent and no copy was found elsewhere: an
    /// accounting release, the record is `released` and uncharged.
    Released,
    /// Nothing changed: the slot, the object or the folder is not what the
    /// review saw, or a bound was reached. A fresh review decides.
    Kept,
    /// The effect could not be observed: a fresh review decides, and nothing
    /// is retried.
    Unknown,
}

/// The reply to a release, bound to its own identity.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Released {
    pub abi: String,
    pub release_id: String,
    pub request_id: String,
    pub scope: Scope,
    pub outcome: Outcome,
    pub code: String,
}
impl Released {
    pub fn matches(&self, release: &Release) -> bool {
        self.abi == super::ABI
            && self.release_id == release.release_id
            && self.request_id == release.request_id
            && self.scope == release.scope
    }
}
