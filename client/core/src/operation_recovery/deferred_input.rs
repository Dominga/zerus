//! The durable request identity and consent checkpoint of one deferred input
//! (zerus-5wmh.66.4.4, slice 2): a send, an answer, the compaction of a
//! compact-to-send, or the start of a resume-then-send. The client reserves it
//! and saves it with its draft BEFORE the dispatch it names, and that dispatch
//! uses its request id.
//!
//! It records the original intent and identity only. It binds the original
//! draft (its save revision and exact message), the run and conversation the
//! user consented against, the question for an answer, and the CLI receipt
//! family its request id is reconciled against. Restart, import or reconnect
//! never rebuild executable consent from it, and nothing sends from it: the
//! live consent stays process-local and is checked again before each call. An
//! unknown or imported record is never re-sent; reconciliation (a later slice)
//! may only look its original receipt up. This module is pure: no I/O.
use serde::{Deserialize, Serialize};
use serde_json::json;
use zerus_desktop_migration::{canonical_uuid, sha256};

/// The record version written now.
pub const VERSION: u32 = 1;
/// The version of a compact-to-send checkpoint that also reserves its send
/// (zerus-5wmh.66.4.8): both request IDs, saved together before the desktop
/// service is offered the pair. A reload never derives another send ID.
pub const COMPACT_TO_SEND_VERSION: u32 = 2;
/// The version of a resume-then-send checkpoint the desktop service resumes
/// through the receipted `hgs session-action` (zerus-5wmh.66.4.9): a
/// [`Start::Resume`], its receipt family `action_receipts`, and the send it
/// reserves with it, both saved together before the offer. A reload never
/// derives another send ID; version 1 keeps the direct `hgs resume` path.
pub const SESSION_RESUME_VERSION: u32 = 3;
/// The receipt family of a version 3 resume-then-send: the session action's
/// own `action_receipts/<request_id>.json`.
pub const ACTION_RECEIPTS: &str = "action_receipts";
/// The longest run, conversation, question or row identity kept.
const MAX_ID: usize = 512;

/// Which deferred input a checkpoint names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// `hgs send`: its request id names an `input_receipts` record.
    Send,
    /// `hgs answer`: a `question_receipts` record.
    Answer,
    /// The compaction before a send: a `compact_receipts` record.
    CompactToSend,
    /// The start before a send. `hgs resume` takes no request id; the
    /// session's `resume_input` binding is what reconciliation reads, and the
    /// request id is the client's own identity of that dispatch. A version 3
    /// checkpoint resumes through `hgs session-action` instead, whose
    /// `action_receipts` record is keyed by this request id.
    ResumeToSend,
}

impl Kind {
    /// The CLI receipt family this kind's request id is reconciled against.
    pub fn receipts(self) -> &'static str {
        match self {
            Self::Send => "input_receipts",
            Self::Answer => "question_receipts",
            Self::CompactToSend => "compact_receipts",
            Self::ResumeToSend => "resume_input",
        }
    }
}

/// How a resume-then-send starts its session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Start {
    Resume,
    ResumeCompact,
    Fresh,
}

/// The question an answer was consented against.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Question {
    pub id: String,
    /// The SHA-256 of the question's consent identity at the click.
    pub identity_sha256: String,
}

/// The checkpoint saved with a draft before its dispatch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    pub version: u32,
    pub kind: Kind,
    /// The client request UUID of the one dispatch this names, reserved here.
    pub request_id: String,
    /// The CLI receipt family, [`Kind::receipts`] of `kind`.
    pub receipts: String,
    /// The draft save that carries this checkpoint.
    pub draft_revision: u64,
    /// The exact message the dispatch carries ([`message_sha256`]).
    pub message_sha256: String,
    /// The run and conversation the user consented against.
    pub run_id: String,
    pub conversation_id: String,
    /// An answer's question; absent for every other kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question: Option<Question>,
    /// A resume-then-send's start; absent for every other kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<Start>,
    /// The failed local row a Retry or compaction sends again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_of: Option<String>,
    /// Whether the user gave the cold-cache consent at the click: recorded
    /// intent, never a permission.
    pub consent: bool,
    /// The send a version 2 compact-to-send reserved with its compaction, or
    /// a version 3 resume-then-send with its resume; absent in every version
    /// 1 record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub send_request_id: Option<String>,
}

/// What a checkpoint binds about its dispatch, besides a fresh request id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Intent {
    pub kind: Kind,
    pub draft_revision: u64,
    pub message_sha256: String,
    pub run_id: String,
    pub conversation_id: String,
    pub question: Option<Question>,
    pub start: Option<Start>,
    pub retry_of: Option<String>,
    pub consent: bool,
}

impl Checkpoint {
    /// Reserves a new request id for `intent`. The caller saves the result
    /// with its draft before the dispatch, which then uses `request_id`.
    pub fn reserve(intent: Intent) -> Self {
        Self {
            version: VERSION,
            kind: intent.kind,
            request_id: uuid::Uuid::new_v4().to_string(),
            receipts: intent.kind.receipts().into(),
            draft_revision: intent.draft_revision,
            message_sha256: intent.message_sha256,
            run_id: intent.run_id,
            conversation_id: intent.conversation_id,
            question: intent.question,
            start: intent.start,
            retry_of: intent.retry_of,
            consent: intent.consent,
            send_request_id: None,
        }
    }

    /// Reserves both request IDs of a compact-to-send the desktop service
    /// runs as one operation: `request_id` for the compaction and
    /// `send_request_id` for the send after it. `None` unless `intent` is a
    /// compact-to-send.
    pub fn reserve_compact_to_send(intent: Intent) -> Option<Self> {
        (intent.kind == Kind::CompactToSend).then(|| {
            let mut checkpoint = Self::reserve(intent);
            checkpoint.version = COMPACT_TO_SEND_VERSION;
            checkpoint.send_request_id = Some(uuid::Uuid::new_v4().to_string());
            checkpoint
        })
    }

    /// Reserves both request IDs of a resume-then-send the desktop service
    /// runs as one operation: `request_id` for the receipted
    /// `hgs session-action` resume and `send_request_id` for the send after
    /// it. `None` unless `intent` is a resume-then-send that resumes
    /// ([`Start::Resume`]); a fresh start or a resume with compaction stays
    /// on the direct path.
    pub fn reserve_session_resume(intent: Intent) -> Option<Self> {
        (intent.kind == Kind::ResumeToSend && intent.start == Some(Start::Resume)).then(|| {
            let mut checkpoint = Self::reserve(intent);
            checkpoint.version = SESSION_RESUME_VERSION;
            checkpoint.receipts = ACTION_RECEIPTS.into();
            checkpoint.send_request_id = Some(uuid::Uuid::new_v4().to_string());
            checkpoint
        })
    }

    /// The CLI receipt family this checkpoint's request id is reconciled
    /// against: the kind's own, but `action_receipts` for version 3.
    pub fn receipt_family(&self) -> &'static str {
        if self.version == SESSION_RESUME_VERSION {
            ACTION_RECEIPTS
        } else {
            self.kind.receipts()
        }
    }

    /// Every field consistent: the version, a canonical request id, the
    /// kind's own receipt family, digests, bounded identities, a question
    /// exactly for an answer and a start exactly for a resume-then-send.
    pub fn valid(&self) -> bool {
        let id = |value: &str| value.len() <= MAX_ID && !value.chars().any(char::is_control);
        let paired = |send: &str| canonical_uuid(send) && send != self.request_id;
        // Version 2 is exactly a compact-to-send with its own, distinct send;
        // version 3 exactly a resume-then-send that resumes, with its own.
        let version = match (self.version, self.send_request_id.as_deref()) {
            (VERSION, None) => true,
            (COMPACT_TO_SEND_VERSION, Some(send)) => {
                self.kind == Kind::CompactToSend && paired(send)
            }
            (SESSION_RESUME_VERSION, Some(send)) => {
                self.kind == Kind::ResumeToSend && self.start == Some(Start::Resume) && paired(send)
            }
            _ => false,
        };
        version
            && canonical_uuid(&self.request_id)
            && self.receipts == self.receipt_family()
            && digest(&self.message_sha256)
            && id(&self.run_id)
            && id(&self.conversation_id)
            && self
                .retry_of
                .as_deref()
                .is_none_or(|row| !row.is_empty() && id(row))
            && match (&self.question, self.kind) {
                (Some(question), Kind::Answer) => {
                    !question.id.is_empty() && id(&question.id) && digest(&question.identity_sha256)
                }
                (None, Kind::Answer) | (Some(_), _) => false,
                (None, _) => true,
            }
            && (self.start.is_some() == (self.kind == Kind::ResumeToSend))
    }
}

fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The exact message a dispatch carries: its text and, in order, each
/// attachment's name, type, reference and content SHA-256, domain-separated.
pub fn message_sha256<'a>(
    text: &str,
    attachments: impl IntoIterator<Item = (&'a str, &'a str, &'a str, &'a [u8])>,
) -> String {
    let files: Vec<_> = attachments
        .into_iter()
        .map(|(name, mime, reference, bytes)| json!([name, mime, reference, sha256(bytes)]))
        .collect();
    let body = json!({"text": text, "attachments": files});
    sha256(
        &[
            b"zerus-deferred-input-message-v1\0".as_slice(),
            body.to_string().as_bytes(),
        ]
        .concat(),
    )
}

/// The SHA-256 of a question's consent identity, as [`Question`] keeps it.
pub fn identity_sha256(identity: &serde_json::Value) -> String {
    sha256(
        &[
            b"zerus-deferred-input-question-v1\0".as_slice(),
            identity.to_string().as_bytes(),
        ]
        .concat(),
    )
}

#[cfg(test)]
mod tests;
