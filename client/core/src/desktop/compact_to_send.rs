//! Exact input for the service-owned compact-to-send (zerus-5wmh.66.4.8,
//! slice 3, its third kind): one original `hgs compact-context <session>
//! --json` and, only within the same live admitted operation and only after
//! the session shows that compaction completed, one original `hgs send
//! <session> --json` whose frozen stdin names it (`expected_compaction_id`).
//! Each child is reconciled from the CLI's own receipt of its request ID
//! (`compact_receipts`, `input_receipts`), never from its exit, its stderr or
//! its reply.
//!
//! The client's version 2 compact-to-send checkpoint, saved with the draft
//! before the offer, binds both request IDs, the run and conversation, and
//! the frozen message. These closed values carry no current writer,
//! frontend, profile, exclusion or execution authority: only the same
//! current native action that durably reserves the checkpoint supplies a
//! one-shot admission. A restored, imported, recovered or reconnected
//! original never starts a child: it gets the read-only lookup only.
//!
//! v1 carries a local terminal agent's session only. DeepSeek and OpenCode,
//! another computer, a subagent and the start paths stay direct, decided
//! before any offer; a plain send stays [`super::deferred_send`]'s, which
//! keeps refusing `expected_compaction_id`.
use super::deferred_send::{self, Unsendable};
use crate::{creation, operation_recovery::deferred_input};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use super::creation_clone::Scope;

pub const ABI: &str = "desktop-deferred-compact-to-send-v1";
pub const HELP_MARKER: &str =
    "Desktop deferred compact to send ABI: desktop-deferred-compact-to-send-v1";
pub const PROFILE: &str = "deferred-compact-to-send-v1";
pub const REPLY_BYTES: usize = 24 << 10;
/// The compaction receipt status the CLI writes after the command's Enter:
/// a submission to the terminal, never proof that it compacted.
pub const SUBMITTED: &str = "submitted";
/// The closed code of a child the operation's own ledger proves never
/// crossed its durable reservation: nothing of it ran.
pub const NOT_STARTED: &str = "compact_to_send_not_started";
/// The service refused the compact-to-send before its ledger admission, with its
/// ledger proving no row for the request, and sealed the request for its
/// epoch, so no copy of it ever admits (zerus-tlb9q). The one refusal that
/// settles as never started; every other refusal leaves only the read-only
/// lookup.
pub const NOT_ADMITTED: &str = "compact_to_send_not_admitted";
/// The largest CLI receipt read.
pub const RECEIPT_BYTES: usize = 256 << 10;
/// The CLI receipt says `in_progress`: the input may have reached the
/// terminal without its Enter.
pub const RECEIPT_IN_PROGRESS: &str = "compact_to_send_in_progress";
/// The compaction receipt says `uncertain`: the CLI could not confirm its own
/// delivery.
pub const RECEIPT_UNCERTAIN: &str = "compact_to_send_uncertain";
/// The CLI receipt is not this original's.
pub const RECEIPT_MISMATCH: &str = "compact_to_send_receipt_mismatch";
/// The CLI receipt is oversized or not a readable receipt.
pub const RECEIPT_INVALID: &str = "compact_to_send_receipt_invalid";
/// The session is not a terminal agent's: DeepSeek and OpenCode compact and
/// send through their own backends and stay direct, before any offer.
pub const AGENT_REFUSED: &str = "compact_to_send_agent_refused";
/// The CLI's own bound on `hgs compact-context`'s stdin.
const MAX_COMPACTION: usize = 8192;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    CompactToSend,
}

/// One compact-to-send: the exact version 2 checkpoint the client saved with
/// its draft before this dispatch, and the exact stdin of each child.
#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Intent {
    pub scope: Scope,
    /// The local session both children name.
    pub session: String,
    /// The compaction's request ID: the operation's own.
    pub request_id: String,
    /// The send's request ID, reserved with it.
    pub send_request_id: String,
    /// The checkpoint's exact JSON.
    pub checkpoint: String,
    /// The exact stdin of `hgs compact-context <session> --json`.
    pub compaction: String,
    /// The exact stdin of `hgs send <session> --json`.
    pub original: String,
}
impl std::fmt::Debug for Intent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompactToSendIntent")
            .field("session", &self.session)
            .field("request_id", &self.request_id)
            .field("send_request_id", &self.send_request_id)
            .finish_non_exhaustive()
    }
}

/// `hgs compact-context`'s stdin, exactly as the CLI reads it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Compaction {
    request_id: String,
    expected_run_id: String,
    expected_conversation_id: String,
}

/// The two fixed effects: `hgs compact-context <session> --json` with the
/// stdin whose SHA-256 is `compaction_sha256`, then `hgs send <session>
/// --json` with the stdin whose SHA-256 is `input_sha256`.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub session: String,
    pub compaction_request_id: String,
    pub send_request_id: String,
    pub run_id: String,
    pub conversation_id: String,
    pub message_sha256: String,
    pub compaction_sha256: String,
    pub input_sha256: String,
}
impl Plan {
    pub fn valid(&self) -> bool {
        local_session(&self.session)
            && uuid(&self.compaction_request_id)
            && uuid(&self.send_request_id)
            && self.compaction_request_id != self.send_request_id
            && identity(&self.run_id)
            && !self.run_id.is_empty()
            && identity(&self.conversation_id)
            && !self.conversation_id.is_empty()
            && super::digest(&self.message_sha256)
            && super::digest(&self.compaction_sha256)
            && super::digest(&self.input_sha256)
    }
    pub fn fingerprint(&self) -> Option<String> {
        self.valid()
            .then(|| hash(&serde_json::to_vec(self).expect("fixed compact-to-send plan")))
    }
    /// The compaction's arguments.
    pub fn compaction_args(&self) -> [String; 3] {
        [
            "compact-context".into(),
            self.session.clone(),
            "--json".into(),
        ]
    }
    /// The send's arguments.
    pub fn send_args(&self) -> [String; 3] {
        ["send".into(), self.session.clone(), "--json".into()]
    }
    /// The send as a deferred send's plan: its `input_receipts` record is read
    /// by exactly that kind's rule.
    pub fn send_plan(&self) -> deferred_send::Plan {
        deferred_send::Plan {
            session: self.session.clone(),
            request_id: self.send_request_id.clone(),
            run_id: self.run_id.clone(),
            conversation_id: self.conversation_id.clone(),
            message_sha256: self.message_sha256.clone(),
            input_sha256: self.input_sha256.clone(),
        }
    }
}

/// Persisted per-kind binding: both requests, their target and the
/// checkpoint, message and input digests, unrelated to every other kind.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub kind: Kind,
    pub schema: u32,
    pub configuration: String,
    pub session: String,
    pub compaction_request_id: String,
    pub send_request_id: String,
    pub run_id: String,
    pub conversation_id: String,
    pub checkpoint_sha256: String,
    pub message_sha256: String,
    pub compaction_sha256: String,
    pub input_sha256: String,
    pub plan_sha256: String,
    pub consent_sha256: String,
}
impl Binding {
    pub fn valid(&self) -> bool {
        self.kind == Kind::CompactToSend
            && self.schema == 1
            && super::digest(&self.configuration)
            && local_session(&self.session)
            && uuid(&self.compaction_request_id)
            && uuid(&self.send_request_id)
            && self.compaction_request_id != self.send_request_id
            && identity(&self.run_id)
            && !self.run_id.is_empty()
            && identity(&self.conversation_id)
            && !self.conversation_id.is_empty()
            && [
                &self.checkpoint_sha256,
                &self.message_sha256,
                &self.compaction_sha256,
                &self.input_sha256,
                &self.plan_sha256,
                &self.consent_sha256,
            ]
            .into_iter()
            .all(|v| super::digest(v))
    }
    /// The operation's key: its compaction request in its configuration and
    /// session. Another send under the same compaction is a conflict, never
    /// a second operation.
    pub fn operation_id(&self) -> Option<String> {
        self.valid().then(|| {
            let mut bytes = b"desktop-compact-to-send-key-v1\0".to_vec();
            bytes.extend(
                serde_json::to_vec(&(
                    &self.configuration,
                    &self.session,
                    &self.compaction_request_id,
                ))
                .expect("fixed compact-to-send key"),
            );
            hash(&bytes)
        })
    }
    pub fn fingerprint(&self) -> Option<String> {
        self.valid()
            .then(|| hash(&serde_json::to_vec(self).expect("fixed compact-to-send binding")))
    }
    /// Whether this binding names exactly `plan`: every field it repeats,
    /// and the plan's own fingerprint.
    pub fn agrees(&self, plan: &Plan) -> bool {
        self.session == plan.session
            && self.compaction_request_id == plan.compaction_request_id
            && self.send_request_id == plan.send_request_id
            && self.run_id == plan.run_id
            && self.conversation_id == plan.conversation_id
            && self.message_sha256 == plan.message_sha256
            && self.compaction_sha256 == plan.compaction_sha256
            && self.input_sha256 == plan.input_sha256
            && plan.fingerprint().as_deref() == Some(self.plan_sha256.as_str())
    }
}

pub struct Verified {
    pub intent: Intent,
    pub plan: Plan,
    pub binding: Binding,
}

/// A session name as the launch records hold it, on this computer only.
fn local_name(value: &str) -> bool {
    super::creation_launch::session_name(value) && !value.contains('@')
}
/// Whether the session's agent is a terminal agent (Codex, Claude, Kimi),
/// whose compaction and send type into the terminal and write receipts.
fn terminal_agent(value: &str) -> bool {
    value
        .split('/')
        .next()
        .is_some_and(|agent| super::creation_launch::AGENTS.contains(&agent))
}
fn local_session(value: &str) -> bool {
    local_name(value) && terminal_agent(value)
}
fn uuid(value: &str) -> bool {
    creation::canonical_uuid(value) && value != "00000000-0000-0000-0000-000000000000"
}
fn identity(value: &str) -> bool {
    value.len() <= 128 && !value.chars().any(char::is_control)
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

impl Intent {
    pub fn verify(self) -> Result<Verified, &'static str> {
        if !self.scope.valid()
            || !local_name(&self.session)
            || !uuid(&self.request_id)
            || !uuid(&self.send_request_id)
            || self.request_id == self.send_request_id
            || self.checkpoint.is_empty()
            || self.compaction.is_empty()
            || self.original.is_empty()
        {
            return Err("invalid_compact_to_send_intent");
        }
        if !terminal_agent(&self.session) {
            return Err(AGENT_REFUSED);
        }
        // The CLI's own stdin bound and the existing control frame: a larger
        // original stays on the direct path, decided before any offer.
        if self.compaction.len() > MAX_COMPACTION
            || !super::ui_requests::fits_frontend_line(
                &super::ui_requests::Request::CompactToSend {
                    intent: Box::new(self.clone()),
                },
            )
        {
            return Err("compact_to_send_original_too_large");
        }
        // Typed, strict decoding: unknown and duplicate keys are refused, as
        // the CLI refuses them. Only a version 2 checkpoint carries a send.
        let checkpoint: deferred_input::Checkpoint = serde_json::from_str(&self.checkpoint)
            .map_err(|_| "invalid_compact_to_send_checkpoint")?;
        if !checkpoint.valid()
            || checkpoint.kind != deferred_input::Kind::CompactToSend
            || checkpoint.request_id != self.request_id
            || checkpoint.send_request_id.as_deref() != Some(self.send_request_id.as_str())
        {
            return Err("invalid_compact_to_send_checkpoint");
        }
        let compaction: Compaction = serde_json::from_str(&self.compaction)
            .map_err(|_| "invalid_compact_to_send_compaction")?;
        let send = deferred_send::frozen(&self.original).map_err(|error| match error {
            Unsendable::Invalid => "invalid_compact_to_send_original",
            Unsendable::Refused => "compact_to_send_message_refused",
        })?;
        // One target and one pair across the checkpoint and both stdins. The
        // send names this compaction: the CLI checks it again before its
        // first input byte.
        if compaction.request_id != self.request_id
            || compaction.expected_run_id != checkpoint.run_id
            || compaction.expected_conversation_id != checkpoint.conversation_id
            || send.request_id != self.send_request_id
            || send.run_id != checkpoint.run_id
            || send.conversation_id != checkpoint.conversation_id
            || send.compaction_id != self.request_id
        {
            return Err("compact_to_send_intent_changed");
        }
        if send.message_sha256 != checkpoint.message_sha256 {
            return Err("compact_to_send_message_changed");
        }
        let plan = Plan {
            session: self.session.clone(),
            compaction_request_id: self.request_id.clone(),
            send_request_id: self.send_request_id.clone(),
            run_id: checkpoint.run_id.clone(),
            conversation_id: checkpoint.conversation_id.clone(),
            message_sha256: send.message_sha256,
            // The raw frozen stdins, never reserialized equivalents.
            compaction_sha256: hash(self.compaction.as_bytes()),
            input_sha256: hash(self.original.as_bytes()),
        };
        if !plan.valid() {
            return Err("invalid_compact_to_send_plan");
        }
        let encoded = serde_json::to_vec(&plan).map_err(|_| "invalid_compact_to_send_plan")?;
        let mut consent = b"desktop-compact-to-send-consent-v1\0".to_vec();
        for part in [&self.checkpoint, &self.compaction, &self.original] {
            consent.extend_from_slice(part.as_bytes());
            consent.push(0);
        }
        let binding = Binding {
            kind: Kind::CompactToSend,
            schema: 1,
            configuration: self.scope.configuration.clone(),
            session: self.session.clone(),
            compaction_request_id: self.request_id.clone(),
            send_request_id: self.send_request_id.clone(),
            run_id: plan.run_id.clone(),
            conversation_id: plan.conversation_id.clone(),
            checkpoint_sha256: hash(self.checkpoint.as_bytes()),
            message_sha256: plan.message_sha256.clone(),
            compaction_sha256: plan.compaction_sha256.clone(),
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

/// Where one child of the operation stands.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    /// Not yet reserved.
    Queued,
    /// Durably reserved: it may have started.
    Running,
    /// The CLI's own exact receipt of this child says `submitted`: a
    /// submission to the terminal, never proof that the agent acted on it.
    Recorded,
    /// The operation's own ledger proves it never crossed its reservation
    /// ([`NOT_STARTED`]): nothing of it ran.
    NotStarted,
    /// Anything else after a possible start: never resent.
    Unknown,
}
impl Step {
    pub fn terminal(self) -> bool {
        matches!(self, Self::Recorded | Self::NotStarted | Self::Unknown)
    }
}

/// The closed outcome an exact `submitted` compaction receipt proves.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Compacted {
    pub request_id: String,
    pub status: String,
}
/// The fields of the CLI's `compact_receipts/<request_id>.json` record this
/// rule reads; others are kept by the CLI and ignored here. A duplicate of
/// any of these refuses. The CLI's `in_progress` placeholder holds only its
/// status.
#[derive(Deserialize)]
struct CliCompaction {
    #[serde(default)]
    request_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    run_id: Option<String>,
    #[serde(default)]
    conversation_id: Option<String>,
    status: String,
}
impl Compacted {
    pub fn matches(&self, plan: &Plan) -> bool {
        self.request_id == plan.compaction_request_id && self.status == SUBMITTED
    }
    /// The one rule for the compaction's CLI receipt. It proves this
    /// original only when its request, session (`name`), run and
    /// conversation are all the plan's and its status is `submitted`;
    /// `in_progress`, `uncertain` and anything else never are.
    pub fn from_receipt(plan: &Plan, bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() > RECEIPT_BYTES {
            return Err(RECEIPT_INVALID);
        }
        let receipt: CliCompaction = serde_json::from_slice(bytes).map_err(|_| RECEIPT_INVALID)?;
        if receipt.status == "in_progress" {
            return Err(RECEIPT_IN_PROGRESS);
        }
        let exact = receipt.request_id.as_deref() == Some(plan.compaction_request_id.as_str())
            && receipt.name.as_deref() == Some(plan.session.as_str())
            && receipt.run_id.as_deref() == Some(plan.run_id.as_str())
            && receipt.conversation_id.as_deref() == Some(plan.conversation_id.as_str());
        if !exact {
            return Err(RECEIPT_MISMATCH);
        }
        match receipt.status.as_str() {
            SUBMITTED => Ok(Self {
                request_id: plan.compaction_request_id.clone(),
                status: receipt.status,
            }),
            "uncertain" => Err(RECEIPT_UNCERTAIN),
            _ => Err(RECEIPT_MISMATCH),
        }
    }
}

/// One child's state: its step, the closed reason and, once Recorded, the
/// outcome its exact receipt proves.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Component<O> {
    pub step: Step,
    pub code: String,
    pub outcome: Option<O>,
}
impl<O> Component<O> {
    pub fn queued() -> Self {
        Self {
            step: Step::Queued,
            code: String::new(),
            outcome: None,
        }
    }
    fn valid(&self) -> bool {
        code(&self.code)
            && match self.step {
                Step::Recorded => self.outcome.is_some() && self.code.is_empty(),
                Step::NotStarted => self.outcome.is_none() && self.code == NOT_STARTED,
                Step::Unknown => self.outcome.is_none() && !self.code.is_empty(),
                Step::Queued | Step::Running => self.outcome.is_none() && self.code.is_empty(),
            }
    }
}
impl<O: PartialEq> Component<O> {
    /// Whether `self` may follow `earlier` for the same child: a settled
    /// child never changes (a Recorded one is never downgraded, an Unknown
    /// one never resolved), and nothing returns to Queued.
    fn follows(&self, earlier: &Self) -> bool {
        if earlier.step.terminal() {
            return self == earlier;
        }
        match earlier.step {
            Step::Queued => true,
            _ => self.step != Step::Queued,
        }
    }
}
fn code(value: &str) -> bool {
    value.len() <= 80
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

#[derive(Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub binding: Binding,
    pub admission_epoch: String,
    pub revision: u64,
    pub compaction: Component<Compacted>,
    pub send: Component<deferred_send::Outcome>,
    /// Both children are settled and every child, pipe and worker joined.
    pub drained: bool,
    /// The operation's closed reason, such as a wait that expired.
    pub code: String,
}
impl std::fmt::Debug for Receipt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompactToSendReceipt")
            .field("binding", &self.binding)
            .field("revision", &self.revision)
            .field("compaction", &self.compaction.step)
            .field("send", &self.send.step)
            .field("drained", &self.drained)
            .field("code", &self.code)
            .finish_non_exhaustive()
    }
}
/// How a drained operation ended, for the client's draft.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Settled {
    /// The send's exact `submitted` receipt: the message was submitted.
    Sent(deferred_send::Outcome),
    /// Neither child ever crossed its reservation: nothing ran.
    NotStarted,
    /// The send never started after a compaction that was submitted
    /// (`compacted`) or may have been.
    NotSent { compacted: bool },
    /// The send may have started.
    Unknown,
}
impl Receipt {
    pub fn valid(&self) -> bool {
        self.binding.valid()
            && uuid(&self.admission_epoch)
            && (1..=i64::MAX as u64).contains(&self.revision)
            && code(&self.code)
            && self.compaction.valid()
            && self.send.valid()
            && self
                .compaction
                .outcome
                .as_ref()
                .is_none_or(|o| o.request_id == self.binding.compaction_request_id && o.status == SUBMITTED)
            && self.send.outcome.as_ref().is_none_or(|o| {
                o.request_id == self.binding.send_request_id
                    && o.status == deferred_send::SUBMITTED
                    && o.input_sha256 == self.binding.input_sha256
            })
            // The send may start only after a compaction that ran or may
            // have; after one that never started it never does.
            && (!matches!(self.send.step, Step::Running | Step::Recorded | Step::Unknown)
                || matches!(self.compaction.step, Step::Recorded | Step::Unknown))
            && (!self.drained || (self.compaction.step.terminal() && self.send.step.terminal()))
    }
    pub fn matches(&self, intent: &Intent) -> bool {
        self.valid()
            && intent
                .clone()
                .verify()
                .is_ok_and(|v| v.binding == self.binding)
    }
    /// Whether `self` may follow `earlier` in the same operation's ledger:
    /// the same binding and admission, a later revision, each child's own
    /// order, and nothing after the drain.
    pub fn follows(&self, earlier: &Self) -> bool {
        self.valid()
            && earlier.valid()
            && self.binding == earlier.binding
            && self.admission_epoch == earlier.admission_epoch
            && self.revision > earlier.revision
            && !earlier.drained
            && self.compaction.follows(&earlier.compaction)
            && self.send.follows(&earlier.send)
    }
    /// Only the exact operation's drained receipt settles it. Queued,
    /// Running and an undrained receipt never do, and nothing is resent.
    pub fn result(&self, plan: &Plan) -> Option<Settled> {
        if !self.valid() || !self.drained || !self.binding.agrees(plan) {
            return None;
        }
        // A valid receipt's outcomes are its own binding's, which agrees
        // with `plan`.
        Some(match (self.compaction.step, self.send.step) {
            (_, Step::Recorded) => Settled::Sent(self.send.outcome.clone()?),
            (Step::NotStarted, Step::NotStarted) => Settled::NotStarted,
            (compaction, Step::NotStarted) => Settled::NotSent {
                compacted: compaction == Step::Recorded,
            },
            _ => Settled::Unknown,
        })
    }
}

/// A scoped read-only lookup of one operation's receipt.
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
            request_id: plan.compaction_request_id.clone(),
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
                    && receipt.binding.compaction_request_id == query.request_id
                    && receipt.binding.plan_sha256 == query.plan_sha256
            })
    }
}

#[cfg(test)]
pub(crate) mod tests;
