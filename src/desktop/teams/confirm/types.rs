//! Frozen identities and effect receipts. Display names never grant authority.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zerus_team_plan::model::{Member, Team};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Binding {
    pub team: String,
    pub member: String,
    pub person: String,
    pub machine: String,
    pub role: String,
    pub agent: String,
    pub account: String,
    pub launch: String,
    pub session: String,
    pub run: String,
    pub conversation: String,
    pub folder: String,
    pub common: String,
    pub nonce: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ResumeWait {
    pub until: i64,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Snapshot {
    pub machine: String,
    pub launch: String,
    pub session: String,
    pub name: String,
    pub run: String,
    pub conversation: String,
    pub agent: String,
    pub account: String,
    pub folder: String,
    pub live: bool,
    pub ready: bool,
    pub paused: bool,
    pub ended: bool,
    pub self_exit: bool,
    pub exit_identity: String,
    pub archive: Option<String>,
    pub receipt_at: Option<i64>,
    /// When the archive's own inspection does not offer a restore, its reason
    /// (zerus-63rru.62). Absent otherwise; a stored row without it reads as
    /// absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restore_refusal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_wait: Option<ResumeWait>,
    pub questions: Vec<Value>,
}

pub(super) struct Facts {
    pub team: Team,
    pub member: Member,
    pub binding: Binding,
    pub preview: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Ownership {
    pub folder: String,
    pub common: String,
    pub nonce: String,
    pub launch: String,
    pub exists: bool,
    pub unchanged: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Kind {
    Trust,
    Resume,
    Brief,
    Notice,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Plan {
    pub key: String,
    pub binding: Binding,
    pub snapshot: Snapshot,
    pub kind: Kind,
    pub question: Value,
    pub text: String,
    pub parent: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Audit {
    pub id: i64,
    pub request: String,
    pub answers: Value,
    pub fresh: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Record {
    pub plan: Plan,
    pub original: String,
    pub epoch: String,
    pub state: String,
    pub audit: Option<Audit>,
    pub audit_done: bool,
    pub result: Option<Snapshot>,
    pub detail: String,
    pub card_request: String,
    pub card_done: bool,
    pub at: i64,
}

pub(super) enum Outcome {
    /// The bounded admission lock was busy; no child started.
    Retry,
    NotStarted(String),
    Refused(String),
    Approved(Option<Box<Snapshot>>),
    Unknown(String),
}

pub(super) struct Claim {
    pub record: Record,
    pub fresh: bool,
    pub limited: bool,
}
