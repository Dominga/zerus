//! Exact input for the service-owned deferred send (zerus-5wmh.66.4.4, slice
//! 3, its first kind): one original `hgs send <session> --json` with the exact
//! stdin the client built for the deferred-input checkpoint it reserved and
//! saved with its draft, then only scoped reads of its result. The result is
//! reconciled from the CLI's own `input_receipts` record of that request ID,
//! never from the child's exit or its reply.
//!
//! These closed values carry no current writer, frontend, profile, exclusion
//! or execution authority: only the same current native action that durably
//! reserves the checkpoint supplies a one-shot admission. An imported,
//! recovered, reconnected or offered-Unknown original gets the read-only
//! lookup only.
//!
//! v1 carries a plain send to a local session: no subagent target, no
//! compaction (`compact_to_send` is its own kind), no `@alias` or other
//! machine.
use crate::{creation, operation_recovery::deferred_input};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use super::creation_clone::Scope;

pub const ABI: &str = "desktop-deferred-send-v1";
pub const HELP_MARKER: &str = "Desktop deferred send ABI: desktop-deferred-send-v1";
pub const PROFILE: &str = "deferred-send-v1";
pub const REPLY_BYTES: usize = 24 << 10;
/// The receipt status the CLI writes after the input's Enter: a submission
/// to the terminal, never proof that the agent read it.
pub const SUBMITTED: &str = "submitted";
/// The closed code of an original child that never started: refused before
/// any child existed. Nothing ran, so the message counts as never sent.
pub const NOT_STARTED: &str = "deferred_send_not_started";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    DeferredSend,
}

/// One deferred send: the exact checkpoint the client saved with its draft
/// before this dispatch, and the exact stdin of `hgs send <session> --json`.
#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Intent {
    pub scope: Scope,
    /// The local session the send names.
    pub session: String,
    /// The request ID the checkpoint reserved, which the stdin carries.
    pub request_id: String,
    /// The checkpoint's exact JSON.
    pub checkpoint: String,
    /// The exact stdin.
    pub original: String,
}
impl std::fmt::Debug for Intent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeferredSendIntent")
            .field("session", &self.session)
            .field("request_id", &self.request_id)
            .finish_non_exhaustive()
    }
}

/// `hgs send`'s stdin attachment, exactly as the CLI reads it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Attachment {
    name: String,
    mime: String,
    data_base64: String,
    #[serde(default)]
    reference: String,
}
/// `hgs send`'s stdin, exactly as the CLI reads it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Send {
    request_id: String,
    text: String,
    expected_run_id: String,
    #[serde(default)]
    expected_conversation_id: String,
    #[serde(default)]
    attachments: Vec<Attachment>,
    #[serde(default)]
    expected_compaction_id: String,
}

/// The fixed effect: `hgs send <session> --json` with the stdin whose SHA-256
/// is `input_sha256`, which is also the digest the CLI's receipt records.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub session: String,
    pub request_id: String,
    pub run_id: String,
    pub conversation_id: String,
    pub message_sha256: String,
    pub input_sha256: String,
}
impl Plan {
    pub fn valid(&self) -> bool {
        local_session(&self.session)
            && uuid(&self.request_id)
            && identity(&self.run_id)
            && !self.run_id.is_empty()
            && identity(&self.conversation_id)
            && super::digest(&self.message_sha256)
            && super::digest(&self.input_sha256)
    }
    pub fn fingerprint(&self) -> Option<String> {
        self.valid()
            .then(|| hash(&serde_json::to_vec(self).expect("fixed send plan")))
    }
    /// The one original call's arguments.
    pub fn args(&self) -> [String; 3] {
        ["send".into(), self.session.clone(), "--json".into()]
    }
}

/// Persisted per-kind binding: the request, its target and the raw,
/// checkpoint, message and input digests, unrelated to every other kind.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub kind: Kind,
    pub schema: u32,
    pub configuration: String,
    pub session: String,
    pub request_id: String,
    pub run_id: String,
    pub conversation_id: String,
    pub source_sha256: String,
    pub checkpoint_sha256: String,
    pub message_sha256: String,
    pub input_sha256: String,
    pub plan_sha256: String,
    pub consent_sha256: String,
}
impl Binding {
    pub fn valid(&self) -> bool {
        self.kind == Kind::DeferredSend
            && self.schema == 1
            && super::digest(&self.configuration)
            && local_session(&self.session)
            && uuid(&self.request_id)
            && identity(&self.run_id)
            && !self.run_id.is_empty()
            && identity(&self.conversation_id)
            && [
                &self.source_sha256,
                &self.checkpoint_sha256,
                &self.message_sha256,
                &self.input_sha256,
                &self.plan_sha256,
                &self.consent_sha256,
            ]
            .into_iter()
            .all(|v| super::digest(v))
    }
    pub fn operation_id(&self) -> Option<String> {
        self.valid().then(|| {
            let mut bytes = b"desktop-deferred-send-key-v1\0".to_vec();
            bytes.extend(
                serde_json::to_vec(&(&self.configuration, &self.session, &self.request_id))
                    .expect("fixed send key"),
            );
            hash(&bytes)
        })
    }
    pub fn fingerprint(&self) -> Option<String> {
        self.valid()
            .then(|| hash(&serde_json::to_vec(self).expect("fixed send binding")))
    }
    /// Whether this binding names exactly `plan`: every field it repeats,
    /// and the plan's own fingerprint. A declared digest never overrides a
    /// conflicting original field.
    pub fn agrees(&self, plan: &Plan) -> bool {
        self.session == plan.session
            && self.request_id == plan.request_id
            && self.run_id == plan.run_id
            && self.conversation_id == plan.conversation_id
            && self.message_sha256 == plan.message_sha256
            && self.input_sha256 == plan.input_sha256
            && plan.fingerprint().as_deref() == Some(self.plan_sha256.as_str())
    }
}

pub struct Verified {
    pub intent: Intent,
    pub plan: Plan,
    pub binding: Binding,
}

/// A session name as the launch records hold it, on this computer only: v1
/// refuses any `@alias` or other-machine target.
fn local_session(value: &str) -> bool {
    super::creation_launch::session_name(value) && !value.contains('@')
}
fn uuid(value: &str) -> bool {
    creation::canonical_uuid(value) && value != "00000000-0000-0000-0000-000000000000"
}
/// A run, conversation or session identity as the CLI bounds it.
fn identity(value: &str) -> bool {
    value.len() <= 128 && !value.chars().any(char::is_control)
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

impl Intent {
    pub fn verify(self) -> Result<Verified, &'static str> {
        if !self.scope.valid()
            || !local_session(&self.session)
            || !uuid(&self.request_id)
            || self.checkpoint.is_empty()
            || self.original.is_empty()
        {
            return Err("invalid_deferred_send_intent");
        }
        // The complete service line must fit the existing control frame; a
        // larger original stays on the direct path, decided before any offer.
        if !super::ui_requests::fits_frontend_line(&super::ui_requests::Request::DeferredSend {
            intent: Box::new(self.clone()),
        }) {
            return Err("deferred_send_original_too_large");
        }
        // Typed, strict decoding: unknown and duplicate keys are refused, as
        // the CLI refuses them.
        let checkpoint: deferred_input::Checkpoint = serde_json::from_str(&self.checkpoint)
            .map_err(|_| "invalid_deferred_send_checkpoint")?;
        if !checkpoint.valid()
            || checkpoint.kind != deferred_input::Kind::Send
            || checkpoint.request_id != self.request_id
        {
            return Err("invalid_deferred_send_checkpoint");
        }
        let send: Send =
            serde_json::from_str(&self.original).map_err(|_| "invalid_deferred_send_original")?;
        if send.request_id != self.request_id
            || send.expected_run_id != checkpoint.run_id
            || send.expected_conversation_id != checkpoint.conversation_id
        {
            return Err("deferred_send_intent_changed");
        }
        if !send.expected_compaction_id.is_empty() {
            return Err("deferred_send_kind_refused");
        }
        let mut files = Vec::with_capacity(send.attachments.len());
        for attachment in &send.attachments {
            files.push(
                STANDARD
                    .decode(&attachment.data_base64)
                    .map_err(|_| "invalid_deferred_send_original")?,
            );
        }
        let message = deferred_input::message_sha256(
            &send.text,
            send.attachments.iter().zip(&files).map(|(a, bytes)| {
                (
                    a.name.as_str(),
                    a.mime.as_str(),
                    a.reference.as_str(),
                    bytes.as_slice(),
                )
            }),
        );
        if message != checkpoint.message_sha256 {
            return Err("deferred_send_message_changed");
        }
        let plan = Plan {
            session: self.session.clone(),
            request_id: self.request_id.clone(),
            run_id: checkpoint.run_id.clone(),
            conversation_id: checkpoint.conversation_id.clone(),
            message_sha256: message,
            input_sha256: hash(self.original.as_bytes()),
        };
        if !plan.valid() {
            return Err("invalid_deferred_send_plan");
        }
        let encoded = serde_json::to_vec(&plan).map_err(|_| "invalid_deferred_send_plan")?;
        let mut consent = b"desktop-deferred-send-consent-v1\0".to_vec();
        consent.extend_from_slice(self.checkpoint.as_bytes());
        consent.push(0);
        consent.extend_from_slice(self.original.as_bytes());
        let binding = Binding {
            kind: Kind::DeferredSend,
            schema: 1,
            configuration: self.scope.configuration.clone(),
            session: self.session.clone(),
            request_id: self.request_id.clone(),
            run_id: plan.run_id.clone(),
            conversation_id: plan.conversation_id.clone(),
            source_sha256: hash(self.original.as_bytes()),
            checkpoint_sha256: hash(self.checkpoint.as_bytes()),
            message_sha256: plan.message_sha256.clone(),
            input_sha256: plan.input_sha256.clone(),
            plan_sha256: hash(&encoded),
            consent_sha256: hash(&consent),
        };
        Ok(Verified {
            intent: self,
            plan,
            binding,
        })
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Admission {
    pub abi: String,
    pub request_id: String,
    pub scope: Scope,
    pub accepted: bool,
    pub durable: bool,
    pub code: String,
    pub receipt: Option<Receipt>,
}
impl Admission {
    pub fn matches(&self, intent: &Intent) -> bool {
        self.abi == ABI
            && self.request_id == intent.request_id
            && self.scope == intent.scope
            && self.accepted
            && self.durable
            && self.code.is_empty()
            && self.receipt.as_ref().is_some_and(|r| r.matches(intent))
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Queued,
    Running,
    /// The CLI's own receipt of this exact request says `submitted`: the
    /// input was submitted to the terminal. Never proof that the agent read
    /// or answered it.
    Recorded,
    /// Only at the closed no-spawn boundary ([`NOT_STARTED`]): nothing ran.
    Failed,
    /// Anything else after a possibly started child: an `in_progress`,
    /// missing, corrupt, mismatched, inaccessible or replaced receipt, a
    /// timeout or a lost worker. Never resent.
    Unknown,
}
impl Phase {
    pub fn terminal(self) -> bool {
        matches!(self, Self::Recorded | Self::Failed | Self::Unknown)
    }
}

/// The closed outcome a matching `submitted` receipt proves: no text, files
/// or terminal data.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    pub request_id: String,
    pub status: String,
}
impl Outcome {
    pub fn matches(&self, plan: &Plan) -> bool {
        self.request_id == plan.request_id && self.status == SUBMITTED
    }
}

#[derive(Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub binding: Binding,
    pub admission_epoch: String,
    pub revision: u64,
    pub phase: Phase,
    pub drained: bool,
    pub code: String,
    pub outcome: Option<Outcome>,
}
impl std::fmt::Debug for Receipt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeferredSendReceipt")
            .field("binding", &self.binding)
            .field("revision", &self.revision)
            .field("phase", &self.phase)
            .field("drained", &self.drained)
            .field("code", &self.code)
            .finish_non_exhaustive()
    }
}
impl Receipt {
    pub fn valid(&self) -> bool {
        self.binding.valid()
            && uuid(&self.admission_epoch)
            && (1..=i64::MAX as u64).contains(&self.revision)
            && self.code.len() <= 80
            && self
                .code
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            && (!self.drained || self.phase.terminal())
            && match self.phase {
                Phase::Recorded => self.drained && self.outcome.is_some(),
                // Only the closed no-spawn boundary fails.
                Phase::Failed => self.outcome.is_none() && self.code == NOT_STARTED,
                _ => self.outcome.is_none(),
            }
    }
    pub fn matches(&self, intent: &Intent) -> bool {
        self.valid()
            && intent
                .clone()
                .verify()
                .is_ok_and(|v| v.binding == self.binding)
    }
    /// Whether this exact original's drained receipt proves that its child
    /// never started. Nothing ran, so the message counts as never sent.
    pub fn not_started(&self, plan: &Plan) -> bool {
        self.valid() && self.drained && self.binding.agrees(plan) && self.phase == Phase::Failed
    }
    /// Only the exact original's drained receipt settles it: Recorded is the
    /// CLI's own submission receipt, Failed the closed no-spawn boundary.
    /// Queued, Running and Unknown never settle, and nothing is resent.
    pub fn result(&self, plan: &Plan) -> Option<Result<(), String>> {
        if !self.valid() || !self.drained || !self.binding.agrees(plan) {
            return None;
        }
        match self.phase {
            Phase::Recorded => self
                .outcome
                .as_ref()
                .is_some_and(|outcome| outcome.matches(plan))
                .then_some(Ok(())),
            Phase::Failed => Some(Err(self.code.clone())),
            _ => None,
        }
    }
}

/// A scoped read-only lookup of one original's receipt.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub scope: Scope,
    pub session: String,
    pub request_id: String,
    pub plan_sha256: String,
}
impl Query {
    pub fn valid(&self) -> bool {
        self.scope.valid()
            && local_session(&self.session)
            && uuid(&self.request_id)
            && super::digest(&self.plan_sha256)
    }
    pub fn for_plan(scope: Scope, plan: &Plan) -> Option<Self> {
        let query = Self {
            scope,
            session: plan.session.clone(),
            request_id: plan.request_id.clone(),
            plan_sha256: plan.fingerprint()?,
        };
        query.valid().then_some(query)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Lookup {
    pub abi: String,
    pub request_id: String,
    pub scope: Scope,
    pub read_only: bool,
    pub found: bool,
    pub receipt: Option<Receipt>,
}
impl Lookup {
    pub fn matches(&self, query: &Query) -> bool {
        self.abi == ABI
            && self.read_only
            && self.request_id == query.request_id
            && self.scope == query.scope
            && self.found == self.receipt.is_some()
            && self.receipt.as_ref().is_none_or(|receipt| {
                // The receipt's own immutable configuration is the queried
                // one; its admission epoch stays its own.
                receipt.valid()
                    && receipt.binding.configuration == query.scope.configuration
                    && receipt.binding.session == query.session
                    && receipt.binding.request_id == query.request_id
                    && receipt.binding.plan_sha256 == query.plan_sha256
            })
    }
}

#[cfg(test)]
pub(crate) mod tests;
