//! Exact input for the service-owned creation worktree effect. These closed
//! values carry no current writer, frontend, profile, exclusion or execution
//! authority. Clone and task-start records stay separate.
use crate::{
    creation,
    paths::{self, Style},
    worktrees, ComputerId, Exit, Invocation,
};
use serde::{de::IgnoredAny, Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashSet;

pub use super::creation_clone::Scope;

pub const ABI: &str = "desktop-creation-worktree-v1";
pub const CONTROL_HELP_MARKER: &str = "Desktop creation worktree receipt control: future-fence-v1";
pub const HELP_MARKER: &str = "Desktop creation worktree ABI: desktop-creation-worktree-v1";
/// Leaves room for the authenticated envelope inside the existing control line.
pub const SOURCE_BYTES: usize = 32 << 10;
pub const PROFILE: &str = "creation-worktree-v1";
/// The fixed base of every reducer worktree, as the direct path passes it.
pub const BASE: &str = "HEAD";
const PATH_BYTES: usize = 4096;
const BRANCH_BYTES: usize = 1024;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    CreationWorktree,
}

/// Exact native reducer checkpoint. Imported recovery uses a separate route;
/// neither an import nor this value itself supplies fresh execution consent.
#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Intent {
    pub scope: Scope,
    pub creation_id: String,
    pub worktree_request_id: String,
    pub revision: u64,
    pub original: String,
}

/// The fixed effect: the existing `worktrees create --receipt` request.
#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub worktree_request_id: String,
    pub expected_machine: String,
    /// The working folder the reducer selected (its `target`).
    pub path: String,
    pub common_dir: String,
    pub branch: String,
    pub destination: String,
}
fn posix_path(path: &str) -> bool {
    paths::absolute(path, Style::Posix)
        && !paths::windows_shaped(path)
        && path.len() <= PATH_BYTES
        && !path.chars().any(char::is_control)
}
fn branch(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= BRANCH_BYTES
        && !name.starts_with('-')
        && !name.chars().any(|c| c.is_control() || c.is_whitespace())
}
impl Plan {
    /// Read-only recovery also accepts a well-formed later journal state. It
    /// supplies no fresh native admission or execution consent.
    pub fn from_operation(id: &str, value: &Value) -> Option<Self> {
        if !creation::valid_operation(id, value)
            || value["host"].as_str().is_some_and(|host| !host.is_empty())
        {
            return None;
        }
        let text = |key| value[key].as_str().map(str::to_owned);
        // Preparation moves `target` to the checkout; the admitted plan keeps
        // the request's own source folder.
        let plan = Self {
            worktree_request_id: text("worktree_id")?,
            expected_machine: text("expected_machine")?,
            path: text("worktree_source").or_else(|| text("target"))?,
            common_dir: text("worktree_common")?,
            branch: text("branch")?,
            destination: text("destination")?,
        };
        plan.valid().then_some(plan)
    }
    /// Correlation DATA for the owning computer's direct metadata command,
    /// including remote and Windows paths. This returns only a hash; it does
    /// not construct a local service plan or relax `valid`/`Intent::verify`.
    pub fn receipt_fingerprint(id: &str, value: &Value) -> Option<String> {
        if !creation::valid_operation(id, value) {
            return None;
        }
        let text = |key| value[key].as_str().map(str::to_owned);
        let plan = Self {
            worktree_request_id: text("worktree_id")?,
            expected_machine: text("expected_machine")?,
            path: text("worktree_source").or_else(|| text("target"))?,
            common_dir: text("worktree_common")?,
            branch: text("branch")?,
            destination: text("destination")?,
        };
        let absolute = |path: &String| {
            path.len() <= PATH_BYTES
                && !path.chars().any(char::is_control)
                && paths::absolute(
                    path,
                    if paths::windows_shaped(path) {
                        Style::Windows
                    } else {
                        Style::Posix
                    },
                )
        };
        (uuid(&plan.worktree_request_id)
            && uuid(&plan.expected_machine)
            && branch(&plan.branch)
            && [&plan.path, &plan.common_dir, &plan.destination]
                .iter()
                .all(|p| absolute(p)))
        .then(|| hash(&serde_json::to_vec(&plan.arguments()).expect("fixed receipt arguments")))
    }
    pub fn valid(&self) -> bool {
        uuid(&self.worktree_request_id)
            && uuid(&self.expected_machine)
            && [&self.path, &self.common_dir, &self.destination]
                .iter()
                .all(|path| posix_path(path))
            && branch(&self.branch)
    }
    pub fn fingerprint(&self) -> Option<String> {
        self.valid()
            .then(|| hash(&serde_json::to_vec(self).expect("fixed worktree plan")))
    }
    /// The exact `worktrees create` arguments after `create`, without
    /// `--receipt`: the existing CLI request fingerprint recipe hashes these.
    pub fn arguments(&self) -> Vec<String> {
        [
            "--path",
            &self.path,
            "--common-dir",
            &self.common_dir,
            "--branch",
            &self.branch,
            "--base",
            BASE,
            "--destination",
            &self.destination,
            "--request-id",
            &self.worktree_request_id,
            "--json",
        ]
        .map(str::to_owned)
        .to_vec()
    }
    /// The original CLI receipt's fingerprint. It identifies the original
    /// request and receipt; it is neither authority nor a checkout identity.
    pub fn request_fingerprint(&self) -> Option<String> {
        self.valid().then(|| {
            hash(&serde_json::to_vec(&self.arguments()).expect("fixed worktree arguments"))
        })
    }
    /// Whether every original receipt this plan can produce is representable:
    /// its [`project`]ion with this plan's own paths and the longest retained
    /// texts fits [`RESULT_BYTES`]. Admission refuses any other plan before
    /// anything starts.
    pub fn result_fits(&self) -> bool {
        let text = "x".repeat(TEXT_BYTES - 2);
        let longest = serde_json::json!({"schema":1,"request_id":self.worktree_request_id,
            "fingerprint":"0".repeat(64),"status":"unconfirmed","path":self.destination,
            "common_dir":self.common_dir,"branch":self.branch,"effects_started":true,
            "detail":text,"resource_error":text});
        serde_json::to_vec(&longest).is_ok_and(|bytes| bytes.len() <= RESULT_BYTES)
    }
    /// `worktrees status --request-id ID`: the read-only direct receipt.
    pub fn status_invocation(&self) -> Invocation {
        worktrees::receipt_invocation(
            &ComputerId::local(),
            "status",
            &self.worktree_request_id,
            &self.expected_machine,
        )
    }
}
// The checkpoint may contain a submitted prompt. Debug never prints its bytes.
impl std::fmt::Debug for Intent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CreationWorktreeIntent")
            .field("creation_id", &self.creation_id)
            .field("worktree_request_id", &self.worktree_request_id)
            .field("revision", &self.revision)
            .finish_non_exhaustive()
    }
}

/// Persisted per-kind binding, deliberately unrelated to clone and task-start.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub kind: Kind,
    pub schema: u32,
    pub configuration: String,
    pub creation_id: String,
    pub worktree_request_id: String,
    pub revision: u64,
    pub source_sha256: String,
    pub plan_sha256: String,
    pub consent_sha256: String,
}

pub struct Verified {
    pub intent: Intent,
    pub plan: Plan,
    pub binding: Binding,
}

fn uuid(value: &str) -> bool {
    creation::canonical_uuid(value) && value != "00000000-0000-0000-0000-000000000000"
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn flag(value: &Value, key: &str) -> bool {
    value[key].as_bool().unwrap_or(false)
}

// Check raw keys before Value can erase ambiguity. Opaque original fields are
// retained byte-for-byte; rejecting their duplicates loses no supported input.
fn unique(bytes: &[u8]) -> Result<(), &'static str> {
    struct Object;
    impl<'de> serde::de::Visitor<'de> for Object {
        type Value = ();
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("one unambiguous creation checkpoint")
        }
        fn visit_map<M: serde::de::MapAccess<'de>>(self, mut map: M) -> Result<(), M::Error> {
            let mut keys = HashSet::new();
            while let Some(key) = map.next_key::<String>()? {
                if !keys.insert(key) {
                    return Err(serde::de::Error::custom("duplicate creation field"));
                }
                map.next_value::<IgnoredAny>()?;
            }
            Ok(())
        }
    }
    let mut decoder = serde_json::Deserializer::from_slice(bytes);
    serde::de::Deserializer::deserialize_map(&mut decoder, Object)
        .map_err(|_| "invalid_creation_checkpoint")?;
    decoder.end().map_err(|_| "invalid_creation_checkpoint")
}

impl Intent {
    pub fn verify(self) -> Result<Verified, &'static str> {
        if !self.scope.valid()
            || !uuid(&self.creation_id)
            || !uuid(&self.worktree_request_id)
            || !(1..=i64::MAX as u64).contains(&self.revision)
            || self.original.is_empty()
            || self.original.len() > SOURCE_BYTES
        {
            return Err("invalid_creation_intent");
        }
        unique(self.original.as_bytes())?;
        let value: Value =
            serde_json::from_str(&self.original).map_err(|_| "invalid_creation_checkpoint")?;
        let text = |key| value[key].as_str().unwrap_or("");
        // A settled clone, or no clone at all, precedes the checkout.
        let clone_settled = if value.get("clone_id").is_some() {
            flag(&value, "repository") && flag(&value, "clone_done")
        } else {
            !flag(&value, "repository") && !flag(&value, "clone_done")
        };
        if !creation::valid_operation(&self.creation_id, &value)
            || value["revision"].as_u64() != Some(self.revision)
            || text("state") != "worktree"
            || !text("host").is_empty()
            || text("worktree_id") != self.worktree_request_id
            || !uuid(text("expected_machine"))
            || !flag(&value, "worktree_receipts")
            || ["worktree_done", "launch_dispatched", "stop_requested"]
                .iter()
                .any(|key| flag(&value, key))
            || ["bound_session", "creation_owner", "acquisition_confirmed"]
                .iter()
                .any(|key| value.get(*key).is_some())
            || value
                .get("resource_error")
                .is_some_and(|error| error.as_str() != Some(""))
            || !clone_settled
            || value
                .get("worktree_source")
                .is_some_and(|source| source.as_str() != Some(text("target")))
            || ["launch_id", "clone_id"].iter().any(|key| {
                value
                    .get(*key)
                    .is_some_and(|id| !id.as_str().is_some_and(uuid))
            })
        {
            return Err("creation_intent_changed");
        }
        let plan = Plan {
            worktree_request_id: self.worktree_request_id.clone(),
            expected_machine: text("expected_machine").into(),
            path: text("target").into(),
            common_dir: text("worktree_common").into(),
            branch: text("branch").into(),
            destination: text("destination").into(),
        };
        if !plan.valid() {
            return Err("invalid_worktree_plan");
        }
        if !plan.result_fits() {
            return Err("worktree_result_unrepresentable");
        }
        let encoded = serde_json::to_vec(&plan).map_err(|_| "invalid_worktree_plan")?;
        let mut consent = b"desktop-creation-worktree-consent-v1\0".to_vec();
        consent.extend_from_slice(self.original.as_bytes());
        let binding = Binding {
            kind: Kind::CreationWorktree,
            schema: 1,
            configuration: self.scope.configuration.clone(),
            creation_id: self.creation_id.clone(),
            worktree_request_id: plan.worktree_request_id.clone(),
            revision: self.revision,
            source_sha256: hash(self.original.as_bytes()),
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

impl Binding {
    pub fn valid(&self) -> bool {
        self.kind == Kind::CreationWorktree
            && self.schema == 1
            && super::digest(&self.configuration)
            && uuid(&self.creation_id)
            && uuid(&self.worktree_request_id)
            && (1..=i64::MAX as u64).contains(&self.revision)
            && [&self.source_sha256, &self.plan_sha256, &self.consent_sha256]
                .into_iter()
                .all(|v| super::digest(v))
    }
    pub fn operation_id(&self) -> Option<String> {
        self.valid().then(|| {
            let mut bytes = b"desktop-creation-worktree-key-v1\0".to_vec();
            bytes.extend(
                serde_json::to_vec(&(
                    &self.configuration,
                    &self.creation_id,
                    &self.worktree_request_id,
                ))
                .expect("fixed worktree key"),
            );
            hash(&bytes)
        })
    }
    pub fn fingerprint(&self) -> Option<String> {
        self.valid()
            .then(|| hash(&serde_json::to_vec(self).expect("fixed worktree binding")))
    }
    /// Whether this binding names exactly `plan`: the worktree request it repeats,
    /// and the plan's own fingerprint. A declared digest never overrides a
    /// conflicting original field.
    pub fn agrees(&self, plan: &Plan) -> bool {
        self.worktree_request_id == plan.worktree_request_id
            && plan.fingerprint().as_deref() == Some(self.plan_sha256.as_str())
    }
}

pub const REPLY_BYTES: usize = 24 << 10;
pub const RESULT_BYTES: usize = 16 << 10;
/// The retained bound of each free text in a result, as encoded JSON.
pub const TEXT_BYTES: usize = 2048;
/// The original receipt fields the creation reducer checks. Everything else,
/// such as the ownership record and its repeated paths, stays in the CLI
/// receipt store.
const RESULT_FIELDS: [&str; 12] = [
    "schema",
    "request_id",
    "fingerprint",
    "status",
    "path",
    "common_dir",
    "branch",
    "effects_started",
    "detail",
    "resource_error",
    "future_starts_fenced",
    "past_outcome",
];
/// The longest prefix of `text` whose JSON encoding fits `limit` bytes.
fn bounded_text(text: &str, limit: usize) -> String {
    let mut size = 2;
    let mut kept = String::new();
    let mut buffer = [0u8; 4];
    for c in text.chars() {
        let encoded =
            serde_json::to_string(&*c.encode_utf8(&mut buffer)).map_or(6, |e| e.len() - 2);
        if size + encoded > limit {
            break;
        }
        size += encoded;
        kept.push(c);
    }
    kept
}
/// The bounded projection of an original CLI worktree receipt that a result
/// carries: the fields the reducer checks, with free texts bounded. `None`
/// when even that does not fit [`RESULT_BYTES`]; such an original stays in
/// the CLI receipt store and its result is Unknown.
pub fn project(receipt: &Value) -> Option<Value> {
    let original = receipt.as_object()?;
    let mut kept = serde_json::Map::new();
    for key in RESULT_FIELDS {
        if let Some(value) = original.get(key) {
            kept.insert(key.into(), value.clone());
        }
    }
    for key in ["detail", "resource_error"] {
        if let Some(Value::String(text)) = kept.get_mut(key) {
            *text = bounded_text(text, TEXT_BYTES);
        }
    }
    let projected = Value::Object(kept);
    serde_json::to_vec(&projected)
        .is_ok_and(|bytes| bytes.len() <= RESULT_BYTES)
        .then_some(projected)
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
    /// This is a refusal reason, not an ID-wide no-effects witness. Generic
    /// service errors can occur after commit and retain the Unknown outcome.
    pub fn refusal_answer(&self, intent: &Intent) -> Option<crate::creation::Answer> {
        let reason = self.refusal(intent)?;
        Some(crate::creation::Answer {
            ok: false,
            data: serde_json::json!({"status":if self.code=="creation_worktree_unavailable" {"unconfirmed"}else{"refused"},"request_id":intent.worktree_request_id}),
            error: reason.into(),
        })
    }
    pub fn refusal(&self, intent: &Intent) -> Option<&'static str> {
        if self.abi != ABI
            || self.request_id != intent.worktree_request_id
            || self.scope != intent.scope
            || self.accepted
            || self.durable
            || self.receipt.is_some()
        {
            return None;
        }
        match self.code.as_str() {
            "creation_worktree_predecessor_unsettled" => Some("Worktree admission refused: the previous attempt is not a drained, confirmed failure. Stop setup and review its retained result before creating a new session."),
            "creation_worktree_capacity" => Some("Worktree admission refused: the desktop service's retained request capacity is full. No new attempt was admitted."),
            "creation_worktree_unavailable" => Some("The desktop service could not confirm this worktree request. Its result is unknown. Stop setup or check its original result; it will not be resent."),
            _ => None,
        }
    }
    pub fn matches(&self, intent: &Intent) -> bool {
        self.abi == ABI
            && self.request_id == intent.worktree_request_id
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
    Created,
    Failed,
    Unknown,
}
impl Phase {
    pub fn terminal(self) -> bool {
        matches!(self, Self::Created | Self::Failed | Self::Unknown)
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
    /// The original CLI worktree request receipt, never a replayed request.
    pub worktree: Option<Value>,
}
impl std::fmt::Debug for Receipt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CreationWorktreeReceipt")
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
            && self
                .worktree
                .as_ref()
                .is_none_or(|v| serde_json::to_vec(v).is_ok_and(|b| b.len() <= RESULT_BYTES))
            && (!matches!(self.phase, Phase::Created | Phase::Failed) || self.drained)
            && (!matches!(self.phase, Phase::Created | Phase::Failed) || self.worktree.is_some())
    }
    pub fn matches(&self, intent: &Intent) -> bool {
        self.valid()
            && intent
                .clone()
                .verify()
                .is_ok_and(|v| v.binding == self.binding)
    }
    /// Only the exact original CLI receipt after actual resource drain
    /// advances the native creation reducer, through the same `created` check
    /// as the direct path. Queued or Unknown never means success or failure.
    pub fn result(&self, plan: &Plan) -> Option<Result<Value, String>> {
        if !self.valid() || !self.drained || !self.binding.agrees(plan) {
            return None;
        }
        let expected = match self.phase {
            Phase::Created => "created",
            Phase::Failed => "failed",
            _ => return None,
        };
        let value = self.worktree.as_ref()?;
        if value["status"] != expected
            || value["request_id"] != plan.worktree_request_id.as_str()
            || value["fingerprint"] != plan.request_fingerprint()?.as_str()
        {
            return None;
        }
        let answer = worktrees::created(
            Exit::Code {
                code: 0,
                stdout: serde_json::to_vec(value).ok()?,
                stderr: String::new(),
            },
            &plan.worktree_request_id,
            &plan.branch,
            &plan.common_dir,
            true,
            &plan.destination,
        );
        match (self.phase, answer) {
            (Phase::Created, Ok(created))
                if created["path"]
                    .as_str()
                    .is_some_and(|path| paths::same(path, &plan.destination, Style::Posix)) =>
            {
                Some(Ok(created))
            }
            (Phase::Failed, Err(detail)) => Some(Err(detail)),
            _ => None,
        }
    }
}

/// Indexed read-only recovery, also usable by imported/recovered creations.
/// It deliberately contains no raw checkpoint or execution consent.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub request_id: String,
    pub scope: Scope,
    pub creation_id: String,
    pub worktree_request_id: String,
    pub plan_sha256: String,
}
impl Query {
    pub fn valid(&self) -> bool {
        uuid(&self.request_id)
            && self.scope.valid()
            && uuid(&self.creation_id)
            && uuid(&self.worktree_request_id)
            && super::digest(&self.plan_sha256)
    }
    pub fn for_plan(scope: Scope, creation_id: String, plan: &Plan) -> Option<Self> {
        let query = Self {
            request_id: uuid::Uuid::new_v4().to_string(),
            scope,
            creation_id,
            worktree_request_id: plan.worktree_request_id.clone(),
            plan_sha256: plan.fingerprint()?,
        };
        query.valid().then_some(query)
    }
}
#[derive(Clone, Deserialize, Serialize)]
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
        query.valid()
            && self.abi == ABI
            && self.request_id == query.request_id
            && self.scope == query.scope
            && self.read_only
            && self.found == self.receipt.is_some()
            && self.receipt.as_ref().is_none_or(|r| {
                r.valid()
                    && r.binding.configuration == query.scope.configuration
                    && r.binding.creation_id == query.creation_id
                    && r.binding.worktree_request_id == query.worktree_request_id
                    && r.binding.plan_sha256 == query.plan_sha256
            })
    }
}

/// A metadata-only observation or explicit future-start exclusion. This carries
/// the native reducer's plan, never a GUI-supplied receipt path or effect permit.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ControlAction {
    Status,
    Cancel,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ControlRequest {
    pub request_id: String,
    pub scope: Scope,
    pub creation_id: String,
    pub plan: Plan,
    pub action: ControlAction,
}
impl ControlRequest {
    pub fn valid(&self) -> bool {
        uuid(&self.request_id) && self.scope.valid() && uuid(&self.creation_id) && self.plan.valid()
    }
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ControlReply {
    pub abi: String,
    pub request_id: String,
    pub scope: Scope,
    pub action: ControlAction,
    pub receipt: Option<Value>,
}
impl ControlReply {
    pub fn matches(&self, request: &ControlRequest) -> bool {
        request.valid()
            && self.abi == ABI
            && self.request_id == request.request_id
            && self.scope == request.scope
            && self.action == request.action
            && self.receipt.as_ref().is_none_or(|v| {
                v["schema"] == 1
                    && v["request_id"] == request.plan.worktree_request_id
                    && v["fingerprint"].as_str() == request.plan.request_fingerprint().as_deref()
                    && serde_json::to_vec(v).is_ok_and(|v| v.len() <= RESULT_BYTES)
            })
    }
}
/// A future-start fence is deliberately NOT a failed/no-effects receipt. It
/// cannot settle historical uncertainty, reconstruct consent or release custody.
pub fn fenced_operation(id: &str, operation: &Value, value: &Value) -> bool {
    Plan::receipt_fingerprint(id, operation).is_some_and(|fingerprint| {
        fence_fields(
            value,
            operation["worktree_id"].as_str().unwrap_or(""),
            &fingerprint,
        )
    })
}
pub fn fenced(value: &Value, plan: &Plan) -> bool {
    plan.request_fingerprint()
        .is_some_and(|fingerprint| fence_fields(value, &plan.worktree_request_id, &fingerprint))
}
fn fence_fields(value: &Value, id: &str, fingerprint: &str) -> bool {
    value["schema"] == 1
        && value["request_id"] == id
        && value["fingerprint"].as_str() == Some(fingerprint)
        && value["status"] == "fenced"
        && value["future_starts_fenced"] == true
        && value["past_outcome"] == "unknown"
        && value.get("effects_started").is_none()
}

#[cfg(test)]
pub(super) fn test_intent() -> Intent {
    tests::checkpoint()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creation::{Answer, Call, Creations, Effect, Journal, Placement};
    use serde_json::json;
    use std::collections::VecDeque;

    const ID: &str = "11111111-1111-4111-8111-111111111111";
    const MACHINE: &str = "22222222-2222-4222-8222-222222222222";
    const EPOCH: &str = "33333333-3333-4333-8333-333333333333";
    /// Project placement is the reducer's own concern before the checkout.
    struct NeverPlace;
    impl Placement for NeverPlace {
        fn resolve(&mut self, _: &mut Value, _: &Value) -> Result<(), String> {
            Ok(())
        }
    }
    /// The real reducer's durable checkpoint, written just before it would
    /// start the direct `worktrees create --receipt` call.
    pub(super) fn checkpoint() -> Intent {
        let mut n = 10u64;
        let mut reducer = Creations::new(
            || 1,
            move || {
                n += 1;
                format!("00000000-0000-4000-8000-{n:012x}")
            },
        );
        reducer.load(Journal::Missing);
        let (admitted, effects) = reducer.submit_reserved(
            json!({
                "cmd":"codex", "tag":"example", "target":"/srv/repo", "parent":"/srv",
                "host":"", "auto":true, "branch":"zerus-example"
            }),
            ID,
        );
        assert_eq!(admitted.unwrap(), ID);
        let mut effects: VecDeque<_> = effects.into();
        let mut saved = None;
        while let Some(effect) = effects.pop_front() {
            match effect {
                Effect::Write { write, bytes, .. } => {
                    saved = Some(bytes);
                    effects.extend(reducer.written(write, true));
                }
                Effect::Call {
                    flight,
                    call: Call::CreationDirectory { .. },
                } => {
                    effects.extend(reducer.answered(
                        flight,
                        Answer {
                            ok: true,
                            data: json!({"creation_identity_v1":true,"computer_id":MACHINE,"path":"/srv/repo"}),
                            error: String::new(),
                        },
                        &mut NeverPlace,
                    ));
                }
                Effect::Call {
                    flight,
                    call: Call::Catalog { .. },
                } => {
                    effects.extend(reducer.answered(flight, Answer {
                        ok:true, data:json!({"state":"ok","stale":false,"common_dir":"/srv/repo/.git",
                            "selected_root":"/srv/repo","selected_branch":"main","creation_receipts_v1":true,"worktrees":[]}),
                        error:String::new()
                    }, &mut NeverPlace));
                }
                Effect::Call {
                    call:
                        Call::CreateWorktree {
                            request_id,
                            receipt,
                            ..
                        },
                    ..
                } => {
                    assert!(receipt);
                    let journal: Value = serde_json::from_slice(&saved.unwrap()).unwrap();
                    let value = &journal["operations"][ID];
                    assert_eq!(value["worktree_id"], request_id);
                    return Intent {
                        scope: Scope {
                            configuration: "a".repeat(64),
                            service_epoch: EPOCH.into(),
                        },
                        creation_id: ID.into(),
                        worktree_request_id: request_id.clone(),
                        revision: value["revision"].as_u64().unwrap(),
                        original: serde_json::to_string(value).unwrap(),
                    };
                }
                Effect::Changed { .. } => (),
                _ => panic!("unexpected effect before the original worktree request"),
            }
        }
        panic!("the real creation reducer did not request its worktree")
    }

    #[test]
    fn a_large_cached_catalog_does_not_fill_the_effect_checkpoint_or_change_its_plan() {
        let original = checkpoint();
        let expected = original.clone().verify().unwrap();
        let mut value: Value = serde_json::from_str(&original.original).unwrap();
        let worktrees: Vec<_> = (0..219)
            .map(|n| {
                json!({
                    "path":format!("/srv/repos/{}/work-{n}", "x".repeat(160)),
                    "branch":format!("work-{n}"),"available":true,
                })
            })
            .collect();
        value["catalog"] = json!({"common_dir":"/srv/repo/.git","state":"ok",
            "worktrees":worktrees});
        value["opaque_future"] = json!({"keep":"original fields remain bound"});
        let saved = value.clone();
        let mut intent = original.clone();
        intent.original = serde_json::to_string(&value).unwrap();
        assert!(intent.original.len() > SOURCE_BYTES);
        assert!(
            intent.clone().verify().is_err(),
            "the old full-card input is refused"
        );
        intent.original = creation::effect_checkpoint(&value).unwrap();
        assert!(intent.original.len() < SOURCE_BYTES);
        let verified = intent.verify().unwrap();
        assert!(verified.plan == expected.plan);
        let projected: Value = serde_json::from_str(&verified.intent.original).unwrap();
        let mut expected_checkpoint = saved.clone();
        expected_checkpoint["catalog"]
            .as_object_mut()
            .unwrap()
            .remove("worktrees");
        assert_eq!(
            projected, expected_checkpoint,
            "all other original fields are retained"
        );
        assert!(projected["catalog"].get("worktrees").is_none());
        assert_eq!(
            projected["catalog"]["common_dir"],
            saved["catalog"]["common_dir"]
        );
        assert_eq!(projected["opaque_future"], saved["opaque_future"]);
        assert_eq!(value, saved, "the journal/recovery catalog was not mutated");
        assert_eq!(
            verified.binding.source_sha256,
            hash(verified.intent.original.as_bytes())
        );
        assert_eq!(verified.binding.plan_sha256, expected.binding.plan_sha256);

        // Only this cached list is omitted. Do not truncate other input to fit,
        // raise a cap or let an oversized unknown field through the verifier.
        value["opaque_future"] = json!("x".repeat(SOURCE_BYTES + 1));
        let mut oversized = original;
        oversized.original = creation::effect_checkpoint(&value).unwrap();
        assert!(oversized.original.len() > SOURCE_BYTES);
        assert!(oversized.verify().is_err());
    }

    #[test]
    fn accepts_the_real_reducers_durable_worktree_checkpoint() {
        let intent = checkpoint();
        let original = intent.original.clone();
        let operation: Value = serde_json::from_str(&original).unwrap();
        let recovered = Plan::from_operation(ID, &operation).unwrap();
        let verified = intent.verify().unwrap();
        assert!(recovered == verified.plan, "recovery reads the same plan");
        let mut remote = operation.clone();
        remote["host"] = json!("peer");
        assert!(Plan::from_operation(ID, &remote).is_none());
        assert_eq!(verified.intent.original, original);
        assert_eq!(verified.binding.source_sha256, hash(original.as_bytes()));
        assert_eq!(verified.plan.path, "/srv/repo");
        assert_eq!(verified.plan.common_dir, "/srv/repo/.git");
        assert_eq!(verified.plan.branch, "zerus-example");
        assert_eq!(verified.plan.destination, "/srv/repo-zerus-example");
        assert_eq!(verified.plan.expected_machine, MACHINE);
        // The same argument recipe as the direct `worktrees create` call,
        // whose reducer path always passes the literal base HEAD.
        let direct = worktrees::create_invocation(
            &ComputerId::local(),
            &verified.plan.path,
            &verified.plan.common_dir,
            &verified.plan.branch,
            "HEAD",
            &verified.plan.destination,
            &verified.plan.worktree_request_id,
            true,
            MACHINE,
        );
        let at = direct.args.iter().position(|a| a == "create").unwrap();
        let after: Vec<_> = direct.args[at + 1..]
            .iter()
            .filter(|a| *a != "--receipt")
            .cloned()
            .collect();
        assert_eq!(after, verified.plan.arguments());
        assert_eq!(
            verified.plan.request_fingerprint().unwrap(),
            hash(&serde_json::to_vec(&after).unwrap())
        );
    }

    fn completed(verified: &Verified) -> Receipt {
        let plan = &verified.plan;
        Receipt {
            binding: verified.binding.clone(),
            admission_epoch: EPOCH.into(),
            revision: 3,
            phase: Phase::Created,
            drained: true,
            code: String::new(),
            worktree: Some(json!({"schema":1,"request_id":plan.worktree_request_id,
                "fingerprint":plan.request_fingerprint().unwrap(),"status":"created",
                "path":plan.destination,"common_dir":plan.common_dir,"branch":plan.branch,
                "effects_started":true,"started_at":1,"finished_at":2})),
        }
    }

    #[test]
    fn only_the_exact_drained_original_receipt_can_advance_a_creation() {
        let verified = checkpoint().verify().unwrap();
        let receipt = completed(&verified);
        assert!(receipt.matches(&verified.intent));
        let created = receipt.result(&verified.plan).unwrap().unwrap();
        assert_eq!(created["status"], "created");
        for phase in [Phase::Unknown, Phase::Queued, Phase::Running] {
            let mut unconfirmed = receipt.clone();
            unconfirmed.phase = phase;
            unconfirmed.drained = phase.terminal();
            assert!(unconfirmed.result(&verified.plan).is_none(), "{phase:?}");
        }
        let mut undrained = receipt.clone();
        undrained.drained = false;
        assert!(!undrained.valid());
        for (field, value) in [
            ("request_id", json!("changed")),
            ("path", json!("/srv/other")),
            ("common_dir", json!("/srv/other/.git")),
            ("branch", json!("other")),
            ("fingerprint", json!("b".repeat(64))),
            ("status", json!("failed")),
        ] {
            let mut changed = receipt.clone();
            changed.worktree.as_mut().unwrap()[field] = value;
            assert!(
                changed.result(&verified.plan).is_none(),
                "accepted changed {field}"
            );
        }
        let mut failed = receipt.clone();
        failed.phase = Phase::Failed;
        let worktree = failed.worktree.as_mut().unwrap();
        worktree["status"] = json!("failed");
        worktree["detail"] = json!("the destination already exists");
        assert_eq!(
            failed.result(&verified.plan).unwrap().unwrap_err(),
            "the destination already exists"
        );
        let mut changed = verified.intent.clone();
        changed.worktree_request_id = "44444444-4444-4444-8444-444444444444".into();
        assert!(changed.verify().is_err());
    }

    #[test]
    fn a_result_carries_a_bounded_projection_and_admission_matches_its_bound() {
        // Long but valid paths: the full receipt repeats them in its ownership
        // record and exceeds the wire bound; the projection fits.
        let long = |base: &str| {
            let mut path = String::from(base);
            while path.len() < 3400 {
                path.push_str("/segment-of-a-long-but-valid-worktree-path");
            }
            path
        };
        let mut intent = checkpoint();
        let mut raw: Value = serde_json::from_str(&intent.original).unwrap();
        let (target, destination) = (long("/srv/repo"), long("/srv/repo-zerus-example"));
        let common = format!("{target}/.git");
        raw["target"] = json!(target);
        raw["worktree_source"] = json!(target);
        raw["worktree_common"] = json!(common);
        raw["destination"] = json!(destination);
        intent.original = serde_json::to_string(&raw).unwrap();
        let verified = intent.verify().unwrap();
        let plan = &verified.plan;
        let mut original = completed(&verified).worktree.unwrap();
        original["creation_owner"] = json!({"version":1,"id":ID,"common_dir":common,
            "path":destination,"branch":plan.branch,"head":"a".repeat(40),
            "metadata":format!("{common}/worktrees/-"),"created_at":1});
        original["machine"] = json!("example");
        original["cleanup_note"] = Value::Null;
        assert!(serde_json::to_vec(&original).unwrap().len() > RESULT_BYTES);
        let projected = project(&original).unwrap();
        assert!(projected.get("creation_owner").is_none());
        let mut receipt = completed(&verified);
        receipt.worktree = Some(projected);
        assert!(receipt.valid());
        assert_eq!(
            receipt.result(plan).unwrap().unwrap()["path"],
            json!(destination)
        );
        // Free texts are bounded by their encoded size, on a character boundary.
        let mut failed = original.clone();
        failed["status"] = json!("failed");
        failed["detail"] = json!("\"é\n".repeat(4000));
        let projected = project(&failed).unwrap();
        let detail = projected["detail"].as_str().unwrap();
        assert!(serde_json::to_string(detail).unwrap().len() <= TEXT_BYTES);
        assert!("\"é\n".repeat(4000).starts_with(detail));
        // A plan whose longest result cannot be represented is refused before
        // anything starts; one that fits is admitted.
        let mut intent = checkpoint();
        let mut raw: Value = serde_json::from_str(&intent.original).unwrap();
        raw["destination"] = json!(format!("/srv/{}", "\"".repeat(4000)));
        raw["worktree_common"] = json!(format!("/srv/repo/{}", "\\".repeat(4000)));
        intent.original = serde_json::to_string(&raw).unwrap();
        assert_eq!(
            intent.verify().err(),
            Some("worktree_result_unrepresentable")
        );
        assert!(project(&json!({"path":"\"".repeat(9000)})).is_none());
    }

    #[test]
    fn the_admitted_plan_survives_preparation_moving_the_target() {
        let verified = checkpoint().verify().unwrap();
        let mut operation: Value = serde_json::from_str(&verified.intent.original).unwrap();
        assert_eq!(operation["worktree_source"], json!("/srv/repo"));
        // What prepared() leaves behind: the target is now the checkout.
        operation["worktree_done"] = json!(true);
        operation["target"] = json!(verified.plan.destination);
        operation["state"] = json!("verifying");
        let plan = Plan::from_operation(ID, &operation).unwrap();
        assert!(plan == verified.plan, "the same exact plan and hash");
        assert_eq!(plan.fingerprint(), verified.plan.fingerprint());
        // A fresh intent whose source and target disagree is refused.
        let mut intent = checkpoint();
        let mut raw: Value = serde_json::from_str(&intent.original).unwrap();
        raw["worktree_source"] = json!("/srv/other");
        intent.original = serde_json::to_string(&raw).unwrap();
        assert!(intent.verify().is_err());
    }

    #[test]
    fn recovery_lookup_is_scoped_and_never_turns_absence_into_execution() {
        let verified = checkpoint().verify().unwrap();
        let query =
            Query::for_plan(verified.intent.scope.clone(), ID.into(), &verified.plan).unwrap();
        let mut lookup = Lookup {
            abi: ABI.into(),
            request_id: query.request_id.clone(),
            scope: query.scope.clone(),
            read_only: true,
            found: true,
            receipt: Some(completed(&verified)),
        };
        assert!(lookup.matches(&query));
        lookup.receipt.as_mut().unwrap().admission_epoch =
            "44444444-4444-4444-8444-444444444444".into();
        assert!(
            lookup.matches(&query),
            "receipt epoch is historical, query scope is current"
        );
        lookup.scope.service_epoch = uuid::Uuid::new_v4().to_string();
        assert!(!lookup.matches(&query));
        lookup.scope = query.scope.clone();
        lookup.receipt.as_mut().unwrap().binding.plan_sha256 = "b".repeat(64);
        assert!(!lookup.matches(&query));
        lookup.receipt = None;
        lookup.found = false;
        assert!(lookup.matches(&query));
        lookup.read_only = false;
        assert!(!lookup.matches(&query));
        lookup.read_only = true;
        lookup.abi = super::super::creation_clone::ABI.into();
        assert!(
            !lookup.matches(&query),
            "a clone reply is not a worktree reply"
        );
    }

    #[test]
    fn unknown_original_fields_are_retained_and_bound_to_consent() {
        let first = checkpoint().verify().unwrap();
        let mut changed = checkpoint();
        let mut value: Value = serde_json::from_str(&changed.original).unwrap();
        value["opaque_future"] = json!({"keep":"original bytes"});
        changed.original = serde_json::to_string(&value).unwrap();
        let second = changed.verify().unwrap();
        assert!(second.intent.original.contains("opaque_future"));
        assert_eq!(first.binding.plan_sha256, second.binding.plan_sha256);
        assert_ne!(first.binding.source_sha256, second.binding.source_sha256);
        assert_ne!(first.binding.consent_sha256, second.binding.consent_sha256);
        assert_ne!(
            first.binding.operation_id(),
            None,
            "a valid binding has its own key"
        );
    }

    #[test]
    fn historical_changed_or_unsafe_checkpoints_never_become_fresh_worktree_input() {
        for (key, value) in [
            ("state", json!("unknown")),
            ("host", json!("peer")),
            ("launch_dispatched", json!(true)),
            ("worktree_done", json!(true)),
            ("worktree_receipts", json!(false)),
            ("stop_requested", json!(true)),
            ("bound_session", json!("codex/example")),
            ("creation_owner", json!({})),
            ("acquisition_confirmed", json!(true)),
            ("resource_error", json!("busy")),
            ("clone_done", json!(true)),
            ("expected_machine", json!("")),
            ("revision", json!(0)),
            ("worktree_id", json!("44444444-4444-4444-8444-444444444444")),
            ("destination", json!("relative/checkout")),
            ("destination", json!("C:\\checkout")),
            ("worktree_common", json!("")),
            ("branch", json!("-x")),
            ("branch", json!("two words")),
            ("target", json!("/srv/repo\n")),
            ("launch_id", json!("not-a-uuid")),
        ] {
            let mut intent = checkpoint();
            let mut raw: Value = serde_json::from_str(&intent.original).unwrap();
            raw[key] = value.clone();
            intent.original = serde_json::to_string(&raw).unwrap();
            assert!(intent.verify().is_err(), "accepted {key}={value}");
        }
        let mut intent = checkpoint();
        intent.original = intent.original.replacen('{', "{\"state\":\"worktree\",", 1);
        assert!(intent.verify().is_err(), "duplicate keys are ambiguous");
        let mut intent = checkpoint();
        intent.original = " ".repeat(SOURCE_BYTES + 1);
        assert!(intent.verify().is_err());
    }

    #[test]
    fn direct_receipt_hashes_preserve_remote_and_windows_identity_without_service_admission() {
        let intent = checkpoint();
        for (host, source, common, destination) in [
            ("peer", "/srv/repo", "/srv/repo/.git", "/srv/new"),
            (
                "peer",
                r"C:\fixture\repo",
                r"C:\fixture\repo\.git",
                r"C:\fixture\new",
            ),
        ] {
            let mut operation: Value = serde_json::from_str(&intent.original).unwrap();
            operation["host"] = json!(host);
            operation["target"] = json!(source);
            operation["worktree_source"] = json!(source);
            operation["worktree_common"] = json!(common);
            operation["destination"] = json!(destination);
            assert!(Plan::from_operation(&intent.creation_id, &operation).is_none());
            let invocation = worktrees::create_invocation(
                &ComputerId::from(host),
                source,
                common,
                operation["branch"].as_str().unwrap(),
                BASE,
                destination,
                &intent.worktree_request_id,
                true,
                MACHINE,
            );
            let after = invocation.args.iter().position(|s| s == "create").unwrap() + 1;
            let original: Vec<_> = invocation.args[after..]
                .iter()
                .filter(|s| *s != "--receipt")
                .cloned()
                .collect();
            let fingerprint = hash(&serde_json::to_vec(&original).unwrap());
            assert_eq!(
                Plan::receipt_fingerprint(&intent.creation_id, &operation),
                Some(fingerprint.clone())
            );
            let value = json!({"schema":1,"request_id":intent.worktree_request_id,"fingerprint":fingerprint,"status":"fenced","future_starts_fenced":true,"past_outcome":"unknown"});
            assert!(fenced_operation(&intent.creation_id, &operation, &value));
            let mut changed = operation.clone();
            changed["branch"] = json!("another");
            assert!(!fenced_operation(&intent.creation_id, &changed, &value));
        }
    }

    #[test]
    fn admission_refusals_remain_exact_and_fences_never_mean_no_past_effects() {
        let intent = checkpoint();
        let plan = intent.clone().verify().unwrap().plan;
        let mut refusal = Admission {
            abi: ABI.into(),
            request_id: intent.worktree_request_id.clone(),
            scope: intent.scope.clone(),
            accepted: false,
            durable: false,
            code: "creation_worktree_predecessor_unsettled".into(),
            receipt: None,
        };
        assert!(refusal.refusal(&intent).is_some());
        refusal.request_id = uuid::Uuid::new_v4().to_string();
        assert!(refusal.refusal(&intent).is_none());
        refusal.request_id = intent.worktree_request_id.clone();
        refusal.scope.service_epoch = uuid::Uuid::new_v4().to_string();
        assert!(refusal.refusal(&intent).is_none());
        let value = json!({"schema":1,"request_id":plan.worktree_request_id,"fingerprint":plan.request_fingerprint().unwrap(),"status":"fenced","future_starts_fenced":true,"past_outcome":"unknown"});
        assert!(fenced(&value, &plan));
        for (key, changed) in [
            ("effects_started", json!(false)),
            ("past_outcome", json!("failed")),
            ("future_starts_fenced", json!(false)),
            ("fingerprint", json!("0".repeat(64))),
        ] {
            let mut value = value.clone();
            value[key] = changed;
            assert!(!fenced(&value, &plan), "{key}");
        }
    }

    #[test]
    fn a_receipt_settles_only_when_its_binding_names_the_plan_request() {
        let verified = checkpoint().verify().unwrap();
        let receipt = completed(&verified);
        assert!(receipt.result(&verified.plan).is_some());
        // The declared plan digest never overrides a conflicting request.
        let mut changed = receipt.clone();
        changed.binding.worktree_request_id = "44444444-4444-4444-8444-444444444444".into();
        assert!(changed.valid(), "a valid shape");
        assert!(changed.result(&verified.plan).is_none());
    }
}
