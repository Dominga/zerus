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
        let plan = Self {
            worktree_request_id: text("worktree_id")?,
            expected_machine: text("expected_machine")?,
            path: text("target")?,
            common_dir: text("worktree_common")?,
            branch: text("branch")?,
            destination: text("destination")?,
        };
        plan.valid().then_some(plan)
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
}

pub const REPLY_BYTES: usize = 24 << 10;
pub const RESULT_BYTES: usize = 16 << 10;
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
        if !self.valid() || !self.drained || self.binding.plan_sha256 != plan.fingerprint()? {
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
}
