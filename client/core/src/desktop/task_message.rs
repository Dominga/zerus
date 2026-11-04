//! Exact input for the service-owned task message effect (zerus-5wmh.66.4.4):
//! one original queue or note `hgs task message` call from its exact outbox
//! checkpoint, then only scoped reads of its result. These closed values carry
//! no current writer, frontend, profile, exclusion or execution authority.
//! No actor executes them yet: an authenticated request is refused. Send,
//! reopen and deferred native input stay review-only.
use crate::{creation, task_details, tasks_write};
use serde::{de::IgnoredAny, Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashSet;

pub use super::creation_clone::Scope;

pub const ABI: &str = "desktop-task-message-v1";
pub const HELP_MARKER: &str = "Desktop task message ABI: desktop-task-message-v1";
pub const PROFILE: &str = "task-message-v1";
/// The only modes this leaf carries: a queued message or a note. Their
/// receipts prove the exact CLI metadata outcome, never native delivery.
pub const MODES: [&str; 2] = ["queue", "note"];
pub const REPLY_BYTES: usize = 24 << 10;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    TaskMessage,
}

/// The exact outbox checkpoint of one native task message: the record bytes
/// as saved, inline. Its frozen stdin is the record's own (an exact `input`,
/// or the canonical serialization its `input_sha256` names). Neither an
/// import nor this value itself supplies fresh execution consent, and a
/// reopened or new reservation is durable data, never consent.
#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Intent {
    pub scope: Scope,
    pub project: String,
    pub task_id: String,
    pub request_id: String,
    pub original: String,
}
impl std::fmt::Debug for Intent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskMessageIntent")
            .field("project", &self.project)
            .field("task_id", &self.task_id)
            .field("request_id", &self.request_id)
            .finish_non_exhaustive()
    }
}

/// The fixed effect: the existing `hgs task message` call
/// (`tasks_write::message_call`), with the exact frozen stdin it names.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub project: String,
    pub task_id: String,
    pub request_id: String,
    pub mode: String,
    pub dispatch_id: String,
    pub input_sha256: String,
}
impl Plan {
    pub fn valid(&self) -> bool {
        task_details::document_call(&self.project, &self.task_id).is_ok()
            && uuid(&self.request_id)
            && MODES.contains(&self.mode.as_str())
            && (self.dispatch_id.is_empty() || uuid(&self.dispatch_id))
            && super::digest(&self.input_sha256)
    }
    pub fn fingerprint(&self) -> Option<String> {
        self.valid()
            .then(|| hash(&serde_json::to_vec(self).expect("fixed message plan")))
    }
}

/// Persisted per-kind binding: the original task/message UUID and the raw,
/// UI-snapshot and input digests, unrelated to every other kind.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub kind: Kind,
    pub schema: u32,
    pub configuration: String,
    pub project: String,
    pub task_id: String,
    pub request_id: String,
    pub source_sha256: String,
    pub ui_snapshot_sha256: String,
    pub input_sha256: String,
    pub plan_sha256: String,
    pub consent_sha256: String,
}
impl Binding {
    pub fn valid(&self) -> bool {
        self.kind == Kind::TaskMessage
            && self.schema == 1
            && super::digest(&self.configuration)
            && task_details::document_call(&self.project, &self.task_id).is_ok()
            && uuid(&self.request_id)
            && [
                &self.source_sha256,
                &self.ui_snapshot_sha256,
                &self.input_sha256,
                &self.plan_sha256,
                &self.consent_sha256,
            ]
            .into_iter()
            .all(|v| super::digest(v))
    }
    pub fn operation_id(&self) -> Option<String> {
        self.valid().then(|| {
            let mut bytes = b"desktop-task-message-key-v1\0".to_vec();
            bytes.extend(
                serde_json::to_vec(&(
                    &self.configuration,
                    &self.project,
                    &self.task_id,
                    &self.request_id,
                ))
                .expect("fixed message key"),
            );
            hash(&bytes)
        })
    }
    pub fn fingerprint(&self) -> Option<String> {
        self.valid()
            .then(|| hash(&serde_json::to_vec(self).expect("fixed message binding")))
    }
}

pub struct Verified {
    pub intent: Intent,
    pub plan: Plan,
    pub binding: Binding,
    /// The decoded original record, for building its one call.
    pub record: Value,
}
impl Verified {
    /// The one original call, or its read-only check (`--recover-only`),
    /// with the record's frozen stdin.
    pub fn call(&self, recovery: bool) -> Option<tasks_write::TaskCall> {
        let call = tasks_write::message_call(
            &self.plan.project,
            &self.plan.task_id,
            &self.record,
            recovery,
        )
        .ok()?;
        (hash(call.input.as_bytes()) == self.plan.input_sha256).then_some(call)
    }
}

fn uuid(value: &str) -> bool {
    creation::canonical_uuid(value) && value != "00000000-0000-0000-0000-000000000000"
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

// Check raw keys before Value can erase ambiguity, at the record and intent
// levels the plan reads.
fn unique(bytes: &[u8]) -> bool {
    struct Object;
    impl<'de> serde::de::Visitor<'de> for Object {
        type Value = Option<Box<serde_json::value::RawValue>>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("one unambiguous message checkpoint")
        }
        fn visit_map<M: serde::de::MapAccess<'de>>(
            self,
            mut map: M,
        ) -> Result<Self::Value, M::Error> {
            let mut keys = HashSet::new();
            let mut intent = None;
            while let Some(key) = map.next_key::<String>()? {
                if !keys.insert(key.clone()) {
                    return Err(serde::de::Error::custom("duplicate message field"));
                }
                if key == "intent" {
                    intent = Some(map.next_value::<Box<serde_json::value::RawValue>>()?);
                } else {
                    map.next_value::<IgnoredAny>()?;
                }
            }
            Ok(intent)
        }
    }
    let object = |bytes: &[u8]| {
        let mut decoder = serde_json::Deserializer::from_slice(bytes);
        let found = serde::de::Deserializer::deserialize_map(&mut decoder, Object).ok()?;
        decoder.end().ok()?;
        Some(found)
    };
    match object(bytes) {
        Some(Some(intent)) => object(intent.get().as_bytes()).is_some(),
        _ => false,
    }
}

impl Intent {
    pub fn verify(self) -> Result<Verified, &'static str> {
        if !self.scope.valid()
            || task_details::document_call(&self.project, &self.task_id).is_err()
            || !uuid(&self.request_id)
            || self.original.is_empty()
        {
            return Err("invalid_task_message_intent");
        }
        // The complete service line, not just this value, must fit the
        // existing control frame; an original that does not stays reviewable.
        if !super::ui_requests::fits_frontend_line(&super::ui_requests::Request::TaskMessage {
            intent: Box::new(self.clone()),
        }) {
            return Err("task_message_original_too_large");
        }
        if !unique(self.original.as_bytes()) {
            return Err("invalid_task_message_checkpoint");
        }
        let record = tasks_write::outbox::decode(self.original.as_bytes())
            .map_err(|_| "invalid_task_message_checkpoint")?;
        let intent = &record["intent"];
        let text = |key| intent[key].as_str().unwrap_or("");
        if record["id"].as_str() != Some(self.request_id.as_str()) {
            return Err("task_message_intent_changed");
        }
        // A legacy record froze no stdin: it stays review-only.
        if record.get("input").is_none() && record.get("input_sha256").is_none() {
            return Err("task_message_input_not_frozen");
        }
        let call = tasks_write::message_call(&self.project, &self.task_id, &record, false)
            .map_err(|_| "task_message_input_changed")?;
        let plan = Plan {
            project: self.project.clone(),
            task_id: self.task_id.clone(),
            request_id: self.request_id.clone(),
            mode: text("mode").into(),
            dispatch_id: text("dispatch").into(),
            input_sha256: hash(call.input.as_bytes()),
        };
        if !intent["mode"].is_string() || !intent["dispatch"].is_string() || !plan.valid() {
            return Err("task_message_mode_refused");
        }
        let encoded = serde_json::to_vec(&plan).map_err(|_| "invalid_task_message_plan")?;
        let mut consent = b"desktop-task-message-consent-v1\0".to_vec();
        consent.extend_from_slice(self.original.as_bytes());
        let binding = Binding {
            kind: Kind::TaskMessage,
            schema: 1,
            configuration: self.scope.configuration.clone(),
            project: self.project.clone(),
            task_id: self.task_id.clone(),
            request_id: self.request_id.clone(),
            source_sha256: hash(self.original.as_bytes()),
            ui_snapshot_sha256: tasks_write::outbox::snapshot(&record),
            input_sha256: plan.input_sha256.clone(),
            plan_sha256: hash(&encoded),
            consent_sha256: hash(&consent),
        };
        Ok(Verified {
            intent: self,
            plan,
            binding,
            record,
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
    Reserved,
    Running,
    /// The exact CLI metadata outcome of the original call: queued or noted.
    /// Never native delivery or consumption.
    Recorded,
    Failed,
    Unknown,
}
impl Phase {
    pub fn terminal(self) -> bool {
        matches!(self, Self::Recorded | Self::Failed | Self::Unknown)
    }
}

/// The closed metadata outcome of the original call, from its exact reply:
/// no body, files, prompt, errors or executor.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    pub request_id: String,
    pub mode: String,
    pub dispatch_id: String,
    /// `queued` for a queued message, `note` for a note.
    pub state: String,
}
/// The closed code of an original task message child that never started:
/// refused before any child existed. After the custody drain its receipt is
/// Failed with this code and no outcome.
pub const NOT_STARTED: &str = "task_message_not_started";
/// The closed code of an original that exited non-zero. It proves nothing about
/// the message; the original stays for the client's own recovery.
pub const EXITED: &str = "task_message_exited";

impl Outcome {
    /// The exact metadata outcome that `plan`'s original call proves through
    /// its whole stdout (exit 0), checked by the same rules as the recovery
    /// policy. Text, files, session and executor fields are never kept.
    pub fn from_reply(plan: &Plan, stdout: &[u8]) -> Option<Self> {
        let state = crate::operation_recovery::task_message::reply_state(
            &plan.project,
            &plan.task_id,
            &plan.request_id,
            &plan.mode,
            &plan.dispatch_id,
            stdout,
        )
        .ok()?;
        Some(Self {
            request_id: plan.request_id.clone(),
            mode: plan.mode.clone(),
            dispatch_id: plan.dispatch_id.clone(),
            state: state.into(),
        })
    }
    pub fn matches(&self, plan: &Plan) -> bool {
        self.request_id == plan.request_id
            && self.mode == plan.mode
            && self.dispatch_id == plan.dispatch_id
            && self.state
                == match plan.mode.as_str() {
                    "queue" => "queued",
                    _ => "note",
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
        f.debug_struct("TaskMessageReceipt")
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
    /// Only the exact original's drained outcome settles it: Recorded is the
    /// CLI's metadata outcome, Failed a definite refusal. Queued, Running and
    /// Unknown never settle, and nothing here is native delivery.
    pub fn result(&self, plan: &Plan) -> Option<Result<(), String>> {
        if !self.valid()
            || !self.drained
            || plan.fingerprint().as_deref() != Some(self.binding.plan_sha256.as_str())
        {
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
    pub project: String,
    pub task_id: String,
    pub request_id: String,
    pub plan_sha256: String,
}
impl Query {
    pub fn valid(&self) -> bool {
        self.scope.valid()
            && task_details::document_call(&self.project, &self.task_id).is_ok()
            && uuid(&self.request_id)
            && super::digest(&self.plan_sha256)
    }
    pub fn for_plan(scope: Scope, plan: &Plan) -> Option<Self> {
        let query = Self {
            scope,
            project: plan.project.clone(),
            task_id: plan.task_id.clone(),
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
                    && receipt.binding.project == query.project
                    && receipt.binding.task_id == query.task_id
                    && receipt.binding.request_id == query.request_id
                    && receipt.binding.plan_sha256 == query.plan_sha256
            })
    }
}

#[cfg(test)]
pub(super) fn test_intent() -> Intent {
    tests::frozen()
}

#[cfg(test)]
mod tests;
