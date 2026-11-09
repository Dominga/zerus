//! Exact input for the service-owned resume-then-send (zerus-5wmh.66.4.9,
//! slice 3, its fourth kind): one original `hgs session-action <session>
//! --json` that resumes the session and, only within the same live admitted
//! operation and only after the resumed agent is ready, one original `hgs send
//! <session> --json` bound to the run that resume returned. Each child is
//! reconciled from the CLI's own receipt of its request ID (`action_receipts`,
//! `input_receipts`), never from its exit, its stderr or its reply.
//!
//! The client's version 3 resume-then-send checkpoint, saved with the draft
//! before the offer, binds both request IDs, the run and conversation the
//! person consented against, and the frozen message. The send's stdin is
//! frozen as a template naming that original run: `hgs session-action` mints
//! the resumed run itself, so only [`bind`] derives the send's actual stdin,
//! from the template and the resume's own exact `result_target`, by replacing
//! that one field. These closed values carry no current writer, frontend,
//! profile, exclusion or execution authority: only the same current native
//! action that durably reserves the checkpoint supplies a one-shot admission.
//! A restored, imported, recovered or reconnected original never starts a
//! child: it gets the read-only lookup only.
//!
//! v1 carries a local Claude, Kimi or Codex session only, and a resume
//! only. A resumed Codex stays pending until its first message, and the CLI
//! shows its readiness only on its terminal, so the service asks the send's
//! own effect-free observation (`hgs send <session> --observe --json`), and
//! a client offers a Codex session only to a service that shows
//! [`OBSERVE_MARKER`] (zerus-5wmh.66.4.10). A fresh start, a resume with
//! compaction, DeepSeek and OpenCode, another computer and a subagent stay
//! direct, all decided before any offer.
use super::deferred_send::{self, Unsendable};
use crate::{creation, operation_recovery::deferred_input};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use super::creation_clone::Scope;

pub const ABI: &str = "desktop-deferred-session-resume-v1";
pub const HELP_MARKER: &str =
    "Desktop deferred session resume ABI: desktop-deferred-session-resume-v1";
pub const PROFILE: &str = "deferred-session-resume-v1";
/// The service also takes a Codex session, whose readiness only the send's
/// own effect-free observation shows (zerus-5wmh.66.4.10). A client offers a
/// Codex session only to a service whose help shows this marker; any other
/// service gets it the direct way, decided before any offer.
pub const OBSERVE_MARKER: &str =
    "Desktop deferred session resume Codex readiness: desktop-deferred-session-resume-codex-v1";
pub const REPLY_BYTES: usize = 24 << 10;
/// The session action this kind runs.
pub const ACTION: &str = "resume";
/// The action receipt status the CLI writes after the resumed agent's
/// terminal started: never proof that the agent is ready.
pub const COMPLETED: &str = "completed";
/// The closed code of a child the operation's own ledger proves never
/// crossed its durable reservation: nothing of it ran.
pub const NOT_STARTED: &str = "session_resume_not_started";
/// The service refused the resume-then-send before its ledger admission, with
/// its ledger proving no row for the request, and sealed the request for
/// its epoch, so no copy of it ever admits (zerus-tlb9q). The one refusal
/// that settles as never started; every other refusal leaves only the
/// read-only lookup.
pub const NOT_ADMITTED: &str = "session_resume_not_admitted";
/// The resume's exact `failed` action receipt: the CLI refused it before it
/// launched anything (it writes `failed` only before its launch began).
pub const REFUSED: &str = "session_resume_refused";
/// The largest action receipt read: the CLI's own bound.
pub const RECEIPT_BYTES: usize = 64 << 10;
/// The action receipt says `uncertain` (or still `claimed`): the resume may
/// have launched.
pub const RECEIPT_UNCERTAIN: &str = "session_resume_uncertain";
/// The CLI receipt is not this original's.
pub const RECEIPT_MISMATCH: &str = "session_resume_receipt_mismatch";
/// The CLI receipt is oversized or not a readable receipt.
pub const RECEIPT_INVALID: &str = "session_resume_receipt_invalid";
/// The session is not a Claude, Kimi or Codex session's: DeepSeek and
/// OpenCode stay direct, before any offer.
pub const AGENT_REFUSED: &str = "session_resume_agent_refused";
/// The agents this kind takes: terminal agents whose resume starts a
/// terminal and whose send types into it.
pub const AGENTS: [&str; 3] = ["claude", "kimi", "codex"];
/// Of [`AGENTS`], those whose resumed session shows only on its terminal
/// that it takes a message: a resumed Codex stays pending until its first
/// message. The send's own effect-free observation (`hgs send <session>
/// --observe --json`) decides their readiness, and a client offers them
/// only to a service that shows [`OBSERVE_MARKER`] (zerus-5wmh.66.4.10).
pub const OBSERVED: [&str; 1] = ["codex"];
/// The CLI's own bound on `hgs session-action`'s stdin.
const MAX_RESUME: usize = 8192;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    SessionResume,
}

/// One resume-then-send: the exact version 3 checkpoint the client saved
/// with its draft before this dispatch, the exact resume stdin and the
/// send's frozen template.
#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Intent {
    pub scope: Scope,
    /// The local session both children name.
    pub session: String,
    /// The resume's request ID: the operation's own.
    pub request_id: String,
    /// The send's request ID, reserved with it.
    pub send_request_id: String,
    /// The checkpoint's exact JSON.
    pub checkpoint: String,
    /// The exact stdin of `hgs session-action <session> --json`.
    pub resume: String,
    /// The send's frozen template: the exact stdin of `hgs send <session>
    /// --json` as the person consented to it, naming the original run.
    pub original: String,
}
impl std::fmt::Debug for Intent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionResumeIntent")
            .field("session", &self.session)
            .field("request_id", &self.request_id)
            .field("send_request_id", &self.send_request_id)
            .finish_non_exhaustive()
    }
}

/// `hgs session-action`'s stdin for a resume, exactly as the CLI reads it:
/// every optional key the CLI accepts is refused here, as the CLI refuses
/// it for a resume.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Resume {
    request_id: String,
    action: String,
    expected_run_id: String,
    expected_conversation_id: String,
}

/// The two fixed effects: `hgs session-action <session> --json` with the
/// stdin whose SHA-256 is `resume_sha256`, then `hgs send <session> --json`
/// with the template whose SHA-256 is `template_sha256`, bound to the
/// resumed run ([`bind`]).
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub session: String,
    pub resume_request_id: String,
    pub send_request_id: String,
    /// The original run the resume names; the send's run is the resumed one.
    pub run_id: String,
    pub conversation_id: String,
    pub message_sha256: String,
    pub resume_sha256: String,
    pub template_sha256: String,
}
impl Plan {
    pub fn valid(&self) -> bool {
        local_session(&self.session)
            && uuid(&self.resume_request_id)
            && uuid(&self.send_request_id)
            && self.resume_request_id != self.send_request_id
            && identity(&self.run_id)
            && !self.run_id.is_empty()
            && identity(&self.conversation_id)
            && !self.conversation_id.is_empty()
            && super::digest(&self.message_sha256)
            && super::digest(&self.resume_sha256)
            && super::digest(&self.template_sha256)
    }
    pub fn fingerprint(&self) -> Option<String> {
        self.valid()
            .then(|| hash(&serde_json::to_vec(self).expect("fixed session resume plan")))
    }
    /// The resume's arguments.
    pub fn resume_args(&self) -> [String; 3] {
        [
            "session-action".into(),
            self.session.clone(),
            "--json".into(),
        ]
    }
    /// The send's arguments.
    pub fn send_args(&self) -> [String; 3] {
        ["send".into(), self.session.clone(), "--json".into()]
    }
    /// Whether the send's own observation decides the resumed session's
    /// readiness ([`OBSERVED`]).
    pub fn observed(&self) -> bool {
        observed(&self.session)
    }
    /// The send's effect-free observation's arguments.
    pub fn observe_args(&self) -> [String; 4] {
        [
            "send".into(),
            self.session.clone(),
            "--observe".into(),
            "--json".into(),
        ]
    }
    /// The send as a deferred send's plan, once bound to the resumed run:
    /// its `input_receipts` record is read by exactly that kind's rule.
    pub fn send_plan(&self, resumed: &Resumed, input_sha256: &str) -> deferred_send::Plan {
        deferred_send::Plan {
            session: self.session.clone(),
            request_id: self.send_request_id.clone(),
            run_id: resumed.result_target.run_id.clone(),
            conversation_id: self.conversation_id.clone(),
            message_sha256: self.message_sha256.clone(),
            input_sha256: input_sha256.into(),
        }
    }
}

/// Persisted per-kind binding: both requests, their target and the
/// checkpoint, message, resume and template digests, unrelated to every
/// other kind.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub kind: Kind,
    pub schema: u32,
    pub configuration: String,
    pub session: String,
    pub resume_request_id: String,
    pub send_request_id: String,
    pub run_id: String,
    pub conversation_id: String,
    pub checkpoint_sha256: String,
    pub message_sha256: String,
    pub resume_sha256: String,
    pub template_sha256: String,
    pub plan_sha256: String,
    pub consent_sha256: String,
}
impl Binding {
    pub fn valid(&self) -> bool {
        self.kind == Kind::SessionResume
            && self.schema == 1
            && super::digest(&self.configuration)
            && local_session(&self.session)
            && uuid(&self.resume_request_id)
            && uuid(&self.send_request_id)
            && self.resume_request_id != self.send_request_id
            && identity(&self.run_id)
            && !self.run_id.is_empty()
            && identity(&self.conversation_id)
            && !self.conversation_id.is_empty()
            && [
                &self.checkpoint_sha256,
                &self.message_sha256,
                &self.resume_sha256,
                &self.template_sha256,
                &self.plan_sha256,
                &self.consent_sha256,
            ]
            .into_iter()
            .all(|v| super::digest(v))
    }
    /// The operation's key: its resume request in its configuration and
    /// session. Another send under the same resume is a conflict, never a
    /// second operation.
    pub fn operation_id(&self) -> Option<String> {
        self.valid().then(|| {
            let mut bytes = b"desktop-session-resume-key-v1\0".to_vec();
            bytes.extend(
                serde_json::to_vec(&(&self.configuration, &self.session, &self.resume_request_id))
                    .expect("fixed session resume key"),
            );
            hash(&bytes)
        })
    }
    pub fn fingerprint(&self) -> Option<String> {
        self.valid()
            .then(|| hash(&serde_json::to_vec(self).expect("fixed session resume binding")))
    }
    /// Whether this binding names exactly `plan`: every field it repeats,
    /// and the plan's own fingerprint.
    pub fn agrees(&self, plan: &Plan) -> bool {
        self.session == plan.session
            && self.resume_request_id == plan.resume_request_id
            && self.send_request_id == plan.send_request_id
            && self.run_id == plan.run_id
            && self.conversation_id == plan.conversation_id
            && self.message_sha256 == plan.message_sha256
            && self.resume_sha256 == plan.resume_sha256
            && self.template_sha256 == plan.template_sha256
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
/// Whether the session's agent is one whose readiness the send's own
/// observation decides ([`OBSERVED`]).
pub fn observed(session: &str) -> bool {
    session
        .split('/')
        .next()
        .is_some_and(|agent| OBSERVED.contains(&agent))
}
/// Whether the session's agent is one this kind takes ([`AGENTS`]): a
/// terminal agent whose resume starts a terminal and whose send types into
/// it.
fn terminal_agent(value: &str) -> bool {
    value
        .split('/')
        .next()
        .is_some_and(|agent| AGENTS.contains(&agent))
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
            || self.resume.is_empty()
            || self.original.is_empty()
        {
            return Err("invalid_session_resume_intent");
        }
        if !terminal_agent(&self.session) {
            return Err(AGENT_REFUSED);
        }
        // The CLI's own stdin bound and the existing control frame: a larger
        // original stays on the direct path, decided before any offer.
        if self.resume.len() > MAX_RESUME
            || !super::ui_requests::fits_frontend_line(
                &super::ui_requests::Request::SessionResume {
                    intent: Box::new(self.clone()),
                },
            )
        {
            return Err("session_resume_original_too_large");
        }
        // Typed, strict decoding: unknown and duplicate keys are refused, as
        // the CLI refuses them. Only a version 3 checkpoint resumes here.
        let checkpoint: deferred_input::Checkpoint = serde_json::from_str(&self.checkpoint)
            .map_err(|_| "invalid_session_resume_checkpoint")?;
        if !checkpoint.valid()
            || checkpoint.version != deferred_input::SESSION_RESUME_VERSION
            || checkpoint.kind != deferred_input::Kind::ResumeToSend
            || checkpoint.start != Some(deferred_input::Start::Resume)
            || checkpoint.request_id != self.request_id
            || checkpoint.send_request_id.as_deref() != Some(self.send_request_id.as_str())
        {
            return Err("invalid_session_resume_checkpoint");
        }
        let resume: Resume =
            serde_json::from_str(&self.resume).map_err(|_| "invalid_session_resume_resume")?;
        let send = deferred_send::frozen(&self.original).map_err(|error| match error {
            Unsendable::Invalid => "invalid_session_resume_original",
            Unsendable::Refused => "session_resume_message_refused",
        })?;
        // One target and one pair across the checkpoint, the resume and the
        // template. The template names the original run; [`bind`] replaces
        // only that with the resumed one. It names no compaction.
        if resume.request_id != self.request_id
            || resume.action != ACTION
            || resume.expected_run_id != checkpoint.run_id
            || resume.expected_conversation_id != checkpoint.conversation_id
            || send.request_id != self.send_request_id
            || send.run_id != checkpoint.run_id
            || send.conversation_id != checkpoint.conversation_id
            || !send.compaction_id.is_empty()
        {
            return Err("session_resume_intent_changed");
        }
        if send.message_sha256 != checkpoint.message_sha256 {
            return Err("session_resume_message_changed");
        }
        let plan = Plan {
            session: self.session.clone(),
            resume_request_id: self.request_id.clone(),
            send_request_id: self.send_request_id.clone(),
            run_id: checkpoint.run_id.clone(),
            conversation_id: checkpoint.conversation_id.clone(),
            message_sha256: send.message_sha256,
            // The raw frozen stdins, never reserialized equivalents.
            resume_sha256: hash(self.resume.as_bytes()),
            template_sha256: hash(self.original.as_bytes()),
        };
        if !plan.valid() {
            return Err("invalid_session_resume_plan");
        }
        let encoded = serde_json::to_vec(&plan).map_err(|_| "invalid_session_resume_plan")?;
        let mut consent = b"desktop-session-resume-consent-v1\0".to_vec();
        for part in [&self.checkpoint, &self.resume, &self.original] {
            consent.extend_from_slice(part.as_bytes());
            consent.push(0);
        }
        let binding = Binding {
            kind: Kind::SessionResume,
            schema: 1,
            configuration: self.scope.configuration.clone(),
            session: self.session.clone(),
            resume_request_id: self.request_id.clone(),
            send_request_id: self.send_request_id.clone(),
            run_id: plan.run_id.clone(),
            conversation_id: plan.conversation_id.clone(),
            checkpoint_sha256: hash(self.checkpoint.as_bytes()),
            message_sha256: plan.message_sha256.clone(),
            resume_sha256: plan.resume_sha256.clone(),
            template_sha256: plan.template_sha256.clone(),
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

/// The send's actual stdin: the frozen `template` with only its expected run
/// replaced by the run the resume's own exact receipt returned, and the
/// SHA-256 the CLI's input receipt will record for it. `None` unless
/// `template` is exactly the plan's, still a send the CLI takes, and
/// `resumed` this plan's own resume of the same session and conversation.
pub fn bind(plan: &Plan, template: &str, resumed: &Resumed) -> Option<(String, String)> {
    if !plan.valid() || hash(template.as_bytes()) != plan.template_sha256 || !resumed.matches(plan)
    {
        return None;
    }
    let bound =
        deferred_send::rebound(template, &plan.run_id, &resumed.result_target.run_id).ok()?;
    let digest = hash(bound.as_bytes());
    Some((bound, digest))
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
    /// The CLI's own exact receipt of this child proves it: for the resume,
    /// `completed` with the resumed run (its terminal started, never proof
    /// that the agent is ready); for the send, `submitted`.
    Recorded,
    /// The resume only: the CLI's own exact `failed` action receipt, written
    /// only before its launch began ([`REFUSED`]). Nothing was launched.
    Failed,
    /// The operation's own ledger proves it never crossed its reservation
    /// ([`NOT_STARTED`]): nothing of it ran.
    NotStarted,
    /// Anything else after a possible start: never resent.
    Unknown,
}
impl Step {
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Recorded | Self::Failed | Self::NotStarted | Self::Unknown
        )
    }
}

/// Where the resumed session lives, as the resume's own receipt names it:
/// the same session and conversation, and the run `hgs session-action`
/// minted for it.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ResultTarget {
    pub name: String,
    pub run_id: String,
    pub conversation_id: String,
}

/// The closed outcome an exact `completed` action receipt proves.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Resumed {
    pub request_id: String,
    pub status: String,
    pub result_target: ResultTarget,
}
/// The CLI's observation of `action_receipts/<request_id>.json`, as
/// `session_action::status` projects it; the private diagnostics it leaves
/// out never reach here. A duplicate key refuses.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Observation {
    request_id: String,
    name: String,
    run_id: String,
    conversation_id: String,
    status: String,
    #[serde(default)]
    at: Option<f64>,
    #[serde(default)]
    archive_id: Option<String>,
    #[serde(default)]
    result_target: Option<CliTarget>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CliTarget {
    name: String,
    run_id: String,
    conversation_id: String,
    #[serde(default)]
    archive_id: Option<String>,
}
impl Resumed {
    /// This plan's own resume: the same session and conversation, and a
    /// resumed run that is not the original one.
    pub fn matches(&self, plan: &Plan) -> bool {
        self.request_id == plan.resume_request_id
            && self.status == COMPLETED
            && self.result_target.name == plan.session
            && self.result_target.conversation_id == plan.conversation_id
            && !self.result_target.run_id.is_empty()
            && identity(&self.result_target.run_id)
            && self.result_target.run_id != plan.run_id
    }
    /// The one rule for the resume's CLI receipt, as the CLI's own
    /// observation projects it. It proves this original only when its
    /// request, session (`name`), run and conversation are all the plan's,
    /// it names no archive, and its status is `completed` with this plan's
    /// own resumed target. The exact `failed` receipt is [`REFUSED`]: the
    /// CLI writes it only before its launch began. `uncertain` (a `claimed`
    /// one included) and anything else never are.
    pub fn from_receipt(plan: &Plan, bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() > RECEIPT_BYTES {
            return Err(RECEIPT_INVALID);
        }
        let receipt: Observation = serde_json::from_slice(bytes).map_err(|_| RECEIPT_INVALID)?;
        let exact = receipt.request_id == plan.resume_request_id
            && receipt.name == plan.session
            && receipt.run_id == plan.run_id
            && receipt.conversation_id == plan.conversation_id
            && receipt.archive_id.is_none()
            && receipt.at.is_some_and(|at| at.is_finite() && at >= 0.0);
        if !exact {
            return Err(RECEIPT_MISMATCH);
        }
        match (receipt.status.as_str(), receipt.result_target) {
            (COMPLETED, Some(target)) if target.archive_id.is_none() => {
                let resumed = Self {
                    request_id: receipt.request_id,
                    status: receipt.status,
                    result_target: ResultTarget {
                        name: target.name,
                        run_id: target.run_id,
                        conversation_id: target.conversation_id,
                    },
                };
                if resumed.matches(plan) {
                    Ok(resumed)
                } else {
                    Err(RECEIPT_MISMATCH)
                }
            }
            ("failed", None) => Err(REFUSED),
            ("uncertain", None) => Err(RECEIPT_UNCERTAIN),
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
    fn valid(&self, may_fail: bool) -> bool {
        code(&self.code)
            && match self.step {
                Step::Recorded => self.outcome.is_some() && self.code.is_empty(),
                Step::Failed => may_fail && self.outcome.is_none() && self.code == REFUSED,
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
    pub resume: Component<Resumed>,
    /// The SHA-256 of the send's actual stdin ([`bind`]), fixed in the same
    /// write that records the resume and never changed after it.
    pub bound_sha256: Option<String>,
    pub send: Component<deferred_send::Outcome>,
    /// Both children are settled and every child, pipe and worker joined.
    pub drained: bool,
    /// The operation's closed reason, such as a wait that expired.
    pub code: String,
}
impl std::fmt::Debug for Receipt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionResumeReceipt")
            .field("binding", &self.binding)
            .field("revision", &self.revision)
            .field("resume", &self.resume.step)
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
    /// The send never started. The resume was recorded (the session was
    /// resumed), refused (nothing was launched) or may have launched.
    NotSent { resume: Step },
    /// The send may have started.
    Unknown,
}
impl Receipt {
    pub fn valid(&self) -> bool {
        self.binding.valid()
            && uuid(&self.admission_epoch)
            && (1..=i64::MAX as u64).contains(&self.revision)
            && code(&self.code)
            && self.resume.valid(true)
            && self.send.valid(false)
            && self.resume.outcome.as_ref().is_none_or(|o| {
                o.request_id == self.binding.resume_request_id
                    && o.status == COMPLETED
                    && o.result_target.name == self.binding.session
                    && o.result_target.conversation_id == self.binding.conversation_id
                    && !o.result_target.run_id.is_empty()
                    && identity(&o.result_target.run_id)
                    && o.result_target.run_id != self.binding.run_id
            })
            // The bound send exists exactly once the resume is recorded.
            && (self.bound_sha256.is_some() == (self.resume.step == Step::Recorded))
            && self.bound_sha256.as_deref().is_none_or(super::digest)
            && self.send.outcome.as_ref().is_none_or(|o| {
                o.request_id == self.binding.send_request_id
                    && o.status == deferred_send::SUBMITTED
                    && Some(&o.input_sha256) == self.bound_sha256.as_ref()
            })
            // The send may start only after a recorded resume: without the
            // resume's own receipt there is no resumed run to bind.
            && (!matches!(self.send.step, Step::Running | Step::Recorded | Step::Unknown)
                || self.resume.step == Step::Recorded)
            && (!self.drained || (self.resume.step.terminal() && self.send.step.terminal()))
    }
    pub fn matches(&self, intent: &Intent) -> bool {
        self.valid()
            && intent.clone().verify().is_ok_and(|v| {
                v.binding == self.binding
                    && self.resume.outcome.as_ref().is_none_or(|resumed| {
                        bind(&v.plan, &intent.original, resumed)
                            .is_some_and(|(_, digest)| Some(&digest) == self.bound_sha256.as_ref())
                    })
            })
    }
    /// Whether `self` may follow `earlier` in the same operation's ledger:
    /// the same binding and admission, a later revision, each child's own
    /// order, a bound send fixed once, and nothing after the drain.
    pub fn follows(&self, earlier: &Self) -> bool {
        self.valid()
            && earlier.valid()
            && self.binding == earlier.binding
            && self.admission_epoch == earlier.admission_epoch
            && self.revision > earlier.revision
            && !earlier.drained
            && self.resume.follows(&earlier.resume)
            && (earlier.bound_sha256.is_none() || self.bound_sha256 == earlier.bound_sha256)
            && self.send.follows(&earlier.send)
    }
    /// Only the exact operation's drained receipt settles it. Queued,
    /// Running and an undrained receipt never do, and nothing is resent.
    pub fn result(&self, plan: &Plan) -> Option<Settled> {
        if !self.valid() || !self.drained || !self.binding.agrees(plan) {
            return None;
        }
        Some(match (self.resume.step, self.send.step) {
            (_, Step::Recorded) => Settled::Sent(self.send.outcome.clone()?),
            (Step::NotStarted, Step::NotStarted) => Settled::NotStarted,
            (resume, Step::NotStarted) => Settled::NotSent { resume },
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
            request_id: plan.resume_request_id.clone(),
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
                    && receipt.binding.resume_request_id == query.request_id
                    && receipt.binding.plan_sha256 == query.plan_sha256
            })
    }
}

#[cfg(test)]
pub(crate) mod tests;
