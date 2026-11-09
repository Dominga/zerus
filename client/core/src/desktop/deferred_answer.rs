//! Exact input for the service-owned deferred answer (zerus-5wmh.66.4.7,
//! slice 3, its second kind): one original `hgs answer <session> --json` with
//! the exact stdin the client built for the deferred-input checkpoint it
//! reserved and saved with the question card, then only scoped reads of its
//! result. The result is reconciled from the CLI's own `question_receipts`
//! record of that request ID, never from the child's exit or its reply.
//!
//! The intent carries the question's consent identity as the person saw it
//! at the click (`pinned`, the closed [`CONSENT_FIELDS`] projection), and the
//! checkpoint binds its hash. The actor compares it again, before its child,
//! with the native current question read from the original store; that
//! fresh observation, never `pinned`, decides source and transport.
//!
//! These closed values carry no current writer, frontend, profile, exclusion
//! or execution authority: only the same current native action that durably
//! reserves the checkpoint supplies a one-shot admission. An imported,
//! recovered, reconnected or offered-Unknown original gets the read-only
//! lookup only. No answer is ever synthesized, and nothing here is a
//! standing approval.
use crate::questions::AnswerItem;
use crate::{creation, operation_recovery::deferred_input};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

pub use super::creation_clone::Scope;

pub const ABI: &str = "desktop-deferred-answer-v1";
pub const HELP_MARKER: &str = "Desktop deferred answer ABI: desktop-deferred-answer-v1";
/// The service also carries Codex's update phase in a Recorded outcome
/// (zerus-5wmh.66.4.7.2). Without this line a `codex_update` answer stays on
/// the direct path, decided before any offer; the ABI is unchanged.
pub const UPDATE_PHASE_MARKER: &str =
    "Desktop deferred answer update phase: desktop-deferred-answer-update-phase-v1";
pub const PROFILE: &str = "deferred-answer-v1";
pub const REPLY_BYTES: usize = 24 << 10;
/// The receipt statuses that record the answer: the provider acknowledged it
/// (`answered`), or Codex's asynchronous question took it (`submitted`, a
/// submission, not the provider's completion).
pub const ANSWERED: &str = "answered";
pub const SUBMITTED: &str = "submitted";
/// The closed code of an original child that never started: refused before
/// any child existed. Nothing ran, so the answer counts as never sent.
pub const NOT_STARTED: &str = "deferred_answer_not_started";
/// The service refused the answer before its ledger admission, with its
/// ledger proving no row for the request, and sealed the request for its
/// epoch, so no copy of it ever admits (zerus-tlb9q). The one refusal that
/// settles as never started; every other refusal leaves only the read-only
/// lookup.
pub const NOT_ADMITTED: &str = "deferred_answer_not_admitted";
/// The closed code of the CLI's own durable refusal: an exact `rejected`
/// receipt, which this kind's allowed sources write only before any
/// delivery. The answer counts as never sent.
pub const REJECTED: &str = "deferred_answer_rejected";
/// The largest CLI question receipt read.
pub const RECEIPT_BYTES: usize = 256 << 10;
/// The CLI receipt says `in_progress`: the answer may have been delivered.
pub const RECEIPT_IN_PROGRESS: &str = "deferred_answer_in_progress";
/// The CLI receipt is not this original's: another session, run,
/// conversation, question, stdin or status.
pub const RECEIPT_MISMATCH: &str = "deferred_answer_receipt_mismatch";
/// The CLI receipt is oversized or not a readable receipt.
pub const RECEIPT_INVALID: &str = "deferred_answer_receipt_invalid";
/// The CLI receipt's Codex update phase is malformed, or present where this
/// original cannot have one, or missing where it must be.
pub const RECEIPT_PHASE: &str = "deferred_answer_receipt_phase";
/// The question ID prefix of Codex's update question, the only question
/// whose recorded answer carries an update phase.
const UPDATE_QUESTION: &str = "codex-update:";
/// The session is not a terminal agent's: an OpenCode or DeepSeek session's
/// `hgs answer` goes to its own native backend. Direct, before any offer.
pub const AGENT_REFUSED: &str = "deferred_answer_agent_refused";
/// The question's source is not one this kind takes in v1. Direct, before
/// any offer.
pub const SOURCE_REFUSED: &str = "deferred_answer_source_refused";
/// The largest stdin: the client's own bound on one answer.
const MAX_INPUT: usize = 64 * 1024;

/// The fields of a question the person's consent was given for, exactly the
/// client's `questions::consent_identity` projection.
pub use crate::questions::CONSENT_FIELDS;

/// The closed v1 allowlist: the question sources `hgs answer` takes natively
/// whose `rejected` receipt guarantees that nothing was delivered. Each
/// drives the terminal only through the CLI's touched-flag helpers, so any
/// error after a key is "delivery uncertain", never `rejected`. A source not
/// listed, a new one included, stays on the direct path. Left out in v1:
/// `codex_async` (a Skip key or pending settings keys can precede a plain
/// error), Claude's `hook` questions and `claude_tool_approval` (a hook
/// offer can be visible before a plain error), and `codex_goal_resume`
/// (its question is read from a SQLite file that even a read-only open can
/// write to, so the actor cannot observe it without an effect).
pub const SOURCES: &[&str] = &[
    "claude_permission_mode",
    "claude_resume",
    "claude_usage_limit",
    "claude_folder_trust",
    "codex_hooks_trust",
    "codex_update",
    "codex_rate_limit",
    "codex_folder_trust",
    "kimi_folder_trust",
    "kimi_cache_hint",
    "kimi_plan_review",
    "kimi_tool_approval",
    "kimi_wire",
];

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    DeferredAnswer,
}

/// One deferred answer: the exact checkpoint the client saved with the
/// question card before this dispatch, the consent identity it binds, and the
/// exact stdin of `hgs answer <session> --json`.
#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Intent {
    pub scope: Scope,
    /// The local session the answer names.
    pub session: String,
    /// The request ID the checkpoint reserved, which the stdin carries.
    pub request_id: String,
    /// The checkpoint's exact JSON.
    pub checkpoint: String,
    /// The question's consent identity at the click: its exact JSON.
    pub pinned: String,
    /// The exact stdin.
    pub original: String,
}
impl std::fmt::Debug for Intent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeferredAnswerIntent")
            .field("session", &self.session)
            .field("request_id", &self.request_id)
            .finish_non_exhaustive()
    }
}

/// One answer of `hgs answer`'s stdin, exactly as the CLI reads it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Answer {
    question_id: String,
    #[serde(default)]
    selected_option_ids: Vec<String>,
    #[serde(default)]
    text: String,
    #[serde(default)]
    skip: bool,
}
/// `hgs answer`'s stdin, exactly as the CLI reads it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    request_id: String,
    expected_run_id: String,
    expected_conversation_id: String,
    question_id: String,
    expected_question_hash: String,
    answers: Vec<Answer>,
}

/// The fixed effect: `hgs answer <session> --json` with the stdin whose
/// SHA-256 is `input_sha256`, which is also the digest the CLI's receipt
/// records.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub session: String,
    pub request_id: String,
    pub run_id: String,
    pub conversation_id: String,
    pub question_id: String,
    pub question_hash: String,
    /// The SHA-256 of the pinned consent identity
    /// (`deferred_input::identity_sha256`).
    pub identity_sha256: String,
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
            && identity(&self.question_id)
            && !self.question_id.is_empty()
            && super::digest(&self.question_hash)
            && super::digest(&self.identity_sha256)
            && super::digest(&self.message_sha256)
            && super::digest(&self.input_sha256)
    }
    pub fn fingerprint(&self) -> Option<String> {
        self.valid()
            .then(|| hash(&serde_json::to_vec(self).expect("fixed answer plan")))
    }
    /// The one original call's arguments.
    pub fn args(&self) -> [String; 3] {
        ["answer".into(), self.session.clone(), "--json".into()]
    }
}

/// Persisted per-kind binding: the request, its target and question, and
/// the raw, checkpoint, identity, message and input digests, unrelated to
/// every other kind.
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
    pub question_id: String,
    pub question_hash: String,
    pub source_sha256: String,
    pub checkpoint_sha256: String,
    pub identity_sha256: String,
    pub message_sha256: String,
    pub input_sha256: String,
    pub plan_sha256: String,
    pub consent_sha256: String,
}
impl Binding {
    pub fn valid(&self) -> bool {
        self.kind == Kind::DeferredAnswer
            && self.schema == 1
            && super::digest(&self.configuration)
            && local_session(&self.session)
            && uuid(&self.request_id)
            && identity(&self.run_id)
            && !self.run_id.is_empty()
            && identity(&self.conversation_id)
            && identity(&self.question_id)
            && !self.question_id.is_empty()
            && [
                &self.question_hash,
                &self.source_sha256,
                &self.checkpoint_sha256,
                &self.identity_sha256,
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
            let mut bytes = b"desktop-deferred-answer-key-v1\0".to_vec();
            bytes.extend(
                serde_json::to_vec(&(&self.configuration, &self.session, &self.request_id))
                    .expect("fixed answer key"),
            );
            hash(&bytes)
        })
    }
    pub fn fingerprint(&self) -> Option<String> {
        self.valid()
            .then(|| hash(&serde_json::to_vec(self).expect("fixed answer binding")))
    }
    /// Whether this binding names exactly `plan`: every field it repeats,
    /// and the plan's own fingerprint. A declared digest never overrides a
    /// conflicting original field.
    pub fn agrees(&self, plan: &Plan) -> bool {
        self.session == plan.session
            && self.request_id == plan.request_id
            && self.run_id == plan.run_id
            && self.conversation_id == plan.conversation_id
            && self.question_id == plan.question_id
            && self.question_hash == plan.question_hash
            && self.identity_sha256 == plan.identity_sha256
            && self.message_sha256 == plan.message_sha256
            && self.input_sha256 == plan.input_sha256
            && plan.fingerprint().as_deref() == Some(self.plan_sha256.as_str())
    }
}

pub struct Verified {
    pub intent: Intent,
    pub plan: Plan,
    pub binding: Binding,
    /// The pinned consent identity, parsed: what the actor compares with the
    /// native current question before its child.
    pub pinned: Map<String, Value>,
}

/// A session name as the launch records hold it, on this computer only: v1
/// refuses any `@alias` or other-machine target.
fn local_name(value: &str) -> bool {
    super::creation_launch::session_name(value) && !value.contains('@')
}
/// Whether the session's agent is a terminal agent
/// ([`super::creation_launch::AGENTS`]: Codex, Claude, Kimi).
fn terminal_agent(value: &str) -> bool {
    value
        .split('/')
        .next()
        .is_some_and(|agent| super::creation_launch::AGENTS.contains(&agent))
}
/// A local terminal agent's session: the only target of v1.
fn local_session(value: &str) -> bool {
    local_name(value) && terminal_agent(value)
}
fn uuid(value: &str) -> bool {
    creation::canonical_uuid(value) && value != "00000000-0000-0000-0000-000000000000"
}
/// A run, conversation, session or question identity as the CLI bounds it.
fn identity(value: &str) -> bool {
    value.len() <= 128 && !value.chars().any(char::is_control)
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn text<'a>(map: &'a Map<String, Value>, key: &str) -> &'a str {
    map.get(key).and_then(Value::as_str).unwrap_or("")
}

/// The CLI's own exception for an empty expected conversation: a startup
/// question, before the agent's first conversation exists. Exactly the CLI's
/// `startup_question_id`, never a general empty-ID allowance.
fn startup_question_id(question_id: &str) -> bool {
    [
        "codex-hooks-trust:",
        "codex-update:",
        "codex-goal-resume:",
        "codex-trust:",
        "kimi-trust:",
        "claude-trust:",
        "claude-permissions:",
        "claude-resume:",
        "claude-usage-limit:",
    ]
    .iter()
    .any(|prefix| question_id.starts_with(prefix))
}

/// The answers as the client's own items (`questions::AnswerItem`): a Skip
/// carries only its question, a Choice never a skip.
fn items(answers: &[Answer]) -> Vec<AnswerItem> {
    answers
        .iter()
        .map(|answer| {
            if answer.skip {
                AnswerItem::Skip {
                    question_id: answer.question_id.clone(),
                    skip: true,
                }
            } else {
                AnswerItem::Choice {
                    question_id: answer.question_id.clone(),
                    selected_option_ids: answer.selected_option_ids.clone(),
                    text: answer.text.clone(),
                }
            }
        })
        .collect()
}
/// The digest the client's checkpoint binds: the JSON of its items.
fn message_sha256(items: &[AnswerItem]) -> String {
    let encoded = serde_json::to_string(items).expect("fixed answers");
    deferred_input::message_sha256(&encoded, Vec::<(&str, &str, &str, &[u8])>::new())
}

impl Intent {
    pub fn verify(self) -> Result<Verified, &'static str> {
        if !self.scope.valid()
            || !local_name(&self.session)
            || !uuid(&self.request_id)
            || self.checkpoint.is_empty()
            || self.pinned.is_empty()
            || self.original.is_empty()
        {
            return Err("invalid_deferred_answer_intent");
        }
        if !terminal_agent(&self.session) {
            return Err(AGENT_REFUSED);
        }
        // The complete service line must fit the existing control frame; a
        // larger original stays on the direct path, decided before any offer.
        if self.original.len() > MAX_INPUT
            || !super::ui_requests::fits_frontend_line(
                &super::ui_requests::Request::DeferredAnswer {
                    intent: Box::new(self.clone()),
                },
            )
        {
            return Err("deferred_answer_original_too_large");
        }
        // Typed, strict decoding: unknown and duplicate keys are refused, as
        // the CLI refuses them.
        let checkpoint: deferred_input::Checkpoint = serde_json::from_str(&self.checkpoint)
            .map_err(|_| "invalid_deferred_answer_checkpoint")?;
        let question = checkpoint
            .question
            .clone()
            .filter(|_| {
                checkpoint.valid()
                    && checkpoint.kind == deferred_input::Kind::Answer
                    && checkpoint.receipts == deferred_input::Kind::Answer.receipts()
                    && checkpoint.request_id == self.request_id
            })
            .ok_or("invalid_deferred_answer_checkpoint")?;
        // The pinned consent identity: exactly the closed projection, and
        // exactly the identity the checkpoint hashed.
        let pinned: Map<String, Value> =
            serde_json::from_str(&self.pinned).map_err(|_| "invalid_deferred_answer_consent")?;
        if pinned
            .keys()
            .any(|key| !CONSENT_FIELDS.contains(&key.as_str()))
            || deferred_input::identity_sha256(&Value::Object(pinned.clone()))
                != question.identity_sha256
            || text(&pinned, "question_id") != question.id
            || text(&pinned, "run_id") != checkpoint.run_id
            || text(&pinned, "conversation_id") != checkpoint.conversation_id
            || !super::digest(text(&pinned, "question_hash"))
        {
            return Err("deferred_answer_consent_changed");
        }
        if !SOURCES.contains(&text(&pinned, "source")) {
            return Err(SOURCE_REFUSED);
        }
        // Codex's update question, and only it, has its ID prefix: the plan's
        // question ID alone then says whether an update phase may follow.
        if (text(&pinned, "source") == "codex_update") != question.id.starts_with(UPDATE_QUESTION) {
            return Err("deferred_answer_consent_changed");
        }
        let request: Request =
            serde_json::from_str(&self.original).map_err(|_| "invalid_deferred_answer_original")?;
        if request.request_id != self.request_id
            || request.expected_run_id != checkpoint.run_id
            || request.expected_conversation_id != checkpoint.conversation_id
            || request.question_id != question.id
            || request.expected_question_hash != text(&pinned, "question_hash")
        {
            return Err("deferred_answer_intent_changed");
        }
        // The CLI's own rule: a run, a conversation unless the question is a
        // startup one, a 64-digit hash and at least one answer. The CLI skips
        // only an optional Codex asynchronous question, a source v1 never
        // takes, and refuses that before its receipt: a Skip here would end
        // Unknown although nothing was sent.
        if request.expected_run_id.is_empty()
            || (request.expected_conversation_id.is_empty()
                && !startup_question_id(&request.question_id))
            || request.answers.is_empty()
            || request.answers.iter().any(|answer| answer.skip)
        {
            return Err("deferred_answer_refused");
        }
        // The stdin's answers are exactly the client's items, nothing more,
        // and the checkpoint hashed exactly those.
        let items = items(&request.answers);
        let raw: Value =
            serde_json::from_str(&self.original).map_err(|_| "invalid_deferred_answer_original")?;
        if serde_json::to_value(&items).ok().as_ref() != raw.get("answers") {
            return Err("deferred_answer_message_changed");
        }
        let message = message_sha256(&items);
        if message != checkpoint.message_sha256 {
            return Err("deferred_answer_message_changed");
        }
        let plan = Plan {
            session: self.session.clone(),
            request_id: self.request_id.clone(),
            run_id: checkpoint.run_id.clone(),
            conversation_id: checkpoint.conversation_id.clone(),
            question_id: question.id.clone(),
            question_hash: request.expected_question_hash.clone(),
            identity_sha256: question.identity_sha256.clone(),
            message_sha256: message,
            // The raw frozen stdin, never a reserialized equivalent.
            input_sha256: hash(self.original.as_bytes()),
        };
        if !plan.valid() {
            return Err("invalid_deferred_answer_plan");
        }
        let encoded = serde_json::to_vec(&plan).map_err(|_| "invalid_deferred_answer_plan")?;
        let mut consent = b"desktop-deferred-answer-consent-v1\0".to_vec();
        consent.extend_from_slice(self.checkpoint.as_bytes());
        consent.push(0);
        consent.extend_from_slice(self.pinned.as_bytes());
        consent.push(0);
        consent.extend_from_slice(self.original.as_bytes());
        let binding = Binding {
            kind: Kind::DeferredAnswer,
            schema: 1,
            configuration: self.scope.configuration.clone(),
            session: self.session.clone(),
            request_id: self.request_id.clone(),
            run_id: plan.run_id.clone(),
            conversation_id: plan.conversation_id.clone(),
            question_id: plan.question_id.clone(),
            question_hash: plan.question_hash.clone(),
            source_sha256: hash(self.original.as_bytes()),
            checkpoint_sha256: hash(self.checkpoint.as_bytes()),
            identity_sha256: plan.identity_sha256.clone(),
            message_sha256: plan.message_sha256.clone(),
            input_sha256: plan.input_sha256.clone(),
            plan_sha256: hash(&encoded),
            consent_sha256: hash(&consent),
        };
        Ok(Verified {
            intent: self,
            plan,
            binding,
            pinned,
        })
    }
}

/// Whether the native current question still has exactly the consent
/// identity the person answered: the actor's fresh comparison before its
/// child. Missing fields count; nothing is defaulted.
pub fn consent_current(pinned: &Map<String, Value>, current: &Value) -> bool {
    crate::questions::consent_identity(current) == Value::Object(pinned.clone())
        && SOURCES.contains(&text(pinned, "source"))
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
    /// Whether this is exactly the closed [`NOT_ADMITTED`] refusal of
    /// `intent`: this kind's ABI, the same request and scope, refused, not
    /// durable, with no receipt. Any other refusal or reply is not.
    pub fn not_admitted(&self, intent: &Intent) -> bool {
        self.abi == ABI
            && self.request_id == intent.request_id
            && self.scope == intent.scope
            && !self.accepted
            && !self.durable
            && self.code == NOT_ADMITTED
            && self.receipt.is_none()
    }
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
    /// The CLI's own receipt of this exact request says `answered` or
    /// `submitted`: never proof that the agent acted on it.
    Recorded,
    /// The closed no-spawn boundary, or the CLI's exact durable `rejected`.
    Failed,
    Unknown,
}
impl Phase {
    pub fn terminal(self) -> bool {
        matches!(self, Self::Recorded | Self::Failed | Self::Unknown)
    }
}

/// What Codex's Update now did, as the CLI's own receipt of that answer
/// observed it: a closed two-value set. Absence is the only "none".
/// `installing` is that receipt's observation, never later progress, and
/// `exited` is not proof that the update succeeded.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UpdatePhase {
    Installing,
    Exited,
}

/// The closed outcome a matching recorded CLI receipt proves: its request
/// ID, status, stdin digest, whether the answer continues in Terminal (an
/// offer for an explicit Open Terminal action, never opened on its own) and,
/// for Codex's Update now only, the receipt's update phase.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    pub request_id: String,
    pub status: String,
    pub input_sha256: String,
    pub open_terminal: bool,
    /// Present exactly when [`Outcome::paired`] says so; an explicit null
    /// never decodes.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_phase"
    )]
    pub codex_update: Option<UpdatePhase>,
}
/// A phase key that is present must hold one of the two values.
fn present_phase<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<UpdatePhase>, D::Error> {
    UpdatePhase::deserialize(d).map(Some)
}
/// Any value of a present key, kept raw so that null and other shapes refuse.
fn present_value<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(d).map(Some)
}
/// The fields of the CLI's `question_receipts/<request_id>.json` record this
/// rule reads; others are kept by the CLI and ignored here. A duplicate of
/// any of these refuses.
#[derive(Deserialize)]
struct CliReceipt {
    request_id: String,
    name: String,
    run_id: String,
    conversation_id: String,
    question_id: String,
    question_hash: String,
    digest: String,
    status: String,
    #[serde(default)]
    open_terminal: bool,
    #[serde(default, deserialize_with = "present_value")]
    codex_update: Option<Value>,
}
/// What the CLI's own receipt of this original says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Read {
    /// Recorded: `answered` or `submitted`.
    Recorded(Outcome),
    /// The CLI's exact durable refusal before any delivery.
    Rejected,
    /// Anything else, for the named reason.
    Unknown(&'static str),
}
impl Outcome {
    pub fn matches(&self, plan: &Plan) -> bool {
        self.request_id == plan.request_id
            && (self.status == ANSWERED || self.status == SUBMITTED)
            && self.input_sha256 == plan.input_sha256
            && self.paired(&plan.question_id)
    }
    /// The strict pairing: a phase is present if and only if this is Codex's
    /// update question, answered, and continuing in Terminal (Update now).
    /// Its two Skip choices carry neither.
    pub fn paired(&self, question_id: &str) -> bool {
        let update_now = question_id.starts_with(UPDATE_QUESTION)
            && self.status == ANSWERED
            && self.open_terminal;
        self.codex_update.is_some() == update_now
    }
    /// The one rule for this original's CLI receipt. It proves this original
    /// only when its session (`name`), run, conversation, question, question
    /// hash and raw stdin digest are all the plan's. `answered` and
    /// `submitted` record it; an exact `rejected` is the CLI's durable
    /// refusal before delivery; `in_progress` and anything else never are.
    pub fn read(plan: &Plan, bytes: &[u8]) -> Read {
        if bytes.len() > RECEIPT_BYTES {
            return Read::Unknown(RECEIPT_INVALID);
        }
        let Ok(receipt) = serde_json::from_slice::<CliReceipt>(bytes) else {
            return Read::Unknown(RECEIPT_INVALID);
        };
        let exact = receipt.request_id == plan.request_id
            && receipt.name == plan.session
            && receipt.run_id == plan.run_id
            && receipt.conversation_id == plan.conversation_id
            && receipt.question_id == plan.question_id
            && receipt.question_hash == plan.question_hash
            && receipt.digest == plan.input_sha256;
        if !exact {
            return Read::Unknown(RECEIPT_MISMATCH);
        }
        // Only a recorded answer may carry a phase, and only one of the two
        // values: null, "none", other cases or types never pass.
        let codex_update = match receipt.codex_update {
            None => None,
            Some(value) => match serde_json::from_value::<UpdatePhase>(value) {
                Ok(phase) if receipt.status == ANSWERED || receipt.status == SUBMITTED => {
                    Some(phase)
                }
                _ => return Read::Unknown(RECEIPT_PHASE),
            },
        };
        match receipt.status.as_str() {
            ANSWERED | SUBMITTED => {
                let outcome = Self {
                    request_id: receipt.request_id,
                    status: receipt.status,
                    input_sha256: receipt.digest,
                    open_terminal: receipt.open_terminal,
                    codex_update,
                };
                if outcome.paired(&plan.question_id) {
                    Read::Recorded(outcome)
                } else {
                    Read::Unknown(RECEIPT_PHASE)
                }
            }
            "rejected" => Read::Rejected,
            "in_progress" => Read::Unknown(RECEIPT_IN_PROGRESS),
            _ => Read::Unknown(RECEIPT_MISMATCH),
        }
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
        f.debug_struct("DeferredAnswerReceipt")
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
                Phase::Recorded => {
                    self.drained
                        && self
                            .outcome
                            .as_ref()
                            .is_some_and(|outcome| outcome.paired(&self.binding.question_id))
                }
                // Only the closed no-spawn boundary and the CLI's exact
                // durable refusal fail.
                Phase::Failed => {
                    self.outcome.is_none() && (self.code == NOT_STARTED || self.code == REJECTED)
                }
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
    /// Whether this exact original's drained receipt proves that nothing was
    /// delivered: its child never started, or the CLI refused it durably
    /// before any delivery. The answer counts as never sent.
    pub fn not_delivered(&self, plan: &Plan) -> bool {
        self.valid() && self.drained && self.binding.agrees(plan) && self.phase == Phase::Failed
    }
    /// Only the exact original's drained receipt settles it: Recorded is the
    /// CLI's own receipt, Failed a proven non-delivery. Queued, Running and
    /// Unknown never settle, and nothing is resent.
    pub fn result(&self, plan: &Plan) -> Option<Result<Outcome, String>> {
        if !self.valid() || !self.drained || !self.binding.agrees(plan) {
            return None;
        }
        match self.phase {
            Phase::Recorded => self
                .outcome
                .as_ref()
                .filter(|outcome| outcome.matches(plan))
                .map(|outcome| Ok(outcome.clone())),
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
