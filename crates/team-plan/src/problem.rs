//! One operation problem, with a stable code a caller can match on.

use serde::{Deserialize, Serialize};

pub const MAX_DETAIL_BYTES: usize = 4096;

/// A logical refusal or a storage failure whose result is not known.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Problem {
    /// One of the constants in [`code`]. Callers match on it; never on `message`.
    pub code: String,
    /// A short explanation for a person.
    pub message: String,
    /// Optional diagnostic cause, bounded by the producer and absent on old replies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// What the problem is about: IDs, field names, a reset time.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<String>,
}

impl Problem {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            message: message.into(),
            detail: None,
            refs: Vec::new(),
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        let mut detail = detail.into();
        let mut end = detail.len().min(MAX_DETAIL_BYTES);
        while !detail.is_char_boundary(end) {
            end -= 1;
        }
        detail.truncate(end);
        self.detail = Some(detail);
        self
    }

    pub fn with_refs<I, S>(mut self, refs: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.refs = refs.into_iter().map(Into::into).collect();
        self
    }
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for Problem {}

/// The stable error codes of Teams.
pub mod code {
    pub const TEAMS_INACTIVE: &str = "teams_inactive";
    pub const NOT_FOUND: &str = "not_found";
    pub const STALE_REVISION: &str = "stale_revision";
    pub const TEMPLATE_CHANGED: &str = "template_changed";
    pub const NOT_ALLOWED: &str = "not_allowed";
    pub const INVALID_STATE: &str = "invalid_state";
    pub const STORE_BUSY: &str = "store_busy";
    /// Storage failed or its result was lost; retry with the same request ID.
    pub const STORAGE_FAILED: &str = "storage_failed";
    pub const UNKNOWN_ROLE: &str = "unknown_role";
    pub const UNKNOWN_PIPELINE: &str = "unknown_pipeline";
    pub const UNKNOWN_MEMBER: &str = "unknown_member";
    pub const UNKNOWN_ACCOUNT: &str = "unknown_account";
    pub const NO_ACCOUNT_AVAILABLE: &str = "no_account_available";
    pub const NO_SEAT_FOR_STAGE: &str = "no_seat_for_stage";
    pub const DEPENDENCY_CYCLE: &str = "dependency_cycle";
    pub const OVER_CAP: &str = "over_cap";
    pub const DUPLICATE: &str = "duplicate";
    pub const UNKNOWN_FIELD: &str = "unknown_field";
    pub const INTEGRATION_CONFLICT: &str = "integration_conflict";
    pub const INTEGRATION_UNAVAILABLE: &str = "integration_unavailable";
    pub const DELIVERY_UNCERTAIN: &str = "delivery_uncertain";
    /// A reconcile of the unstaffed nodes found the store already as the
    /// kernel reads it; nothing was written. A race, settled, never a failure.
    pub const UNSTAFFED_UNCHANGED: &str = "unstaffed_unchanged";

    /// Every code, for tests that pin the list.
    pub const ALL: [&str; 22] = [
        TEAMS_INACTIVE,
        NOT_FOUND,
        STALE_REVISION,
        TEMPLATE_CHANGED,
        NOT_ALLOWED,
        INVALID_STATE,
        STORE_BUSY,
        STORAGE_FAILED,
        UNKNOWN_ROLE,
        UNKNOWN_PIPELINE,
        UNKNOWN_MEMBER,
        UNKNOWN_ACCOUNT,
        NO_ACCOUNT_AVAILABLE,
        NO_SEAT_FOR_STAGE,
        DEPENDENCY_CYCLE,
        OVER_CAP,
        DUPLICATE,
        UNKNOWN_FIELD,
        INTEGRATION_CONFLICT,
        INTEGRATION_UNAVAILABLE,
        DELIVERY_UNCERTAIN,
        UNSTAFFED_UNCHANGED,
    ];
}
