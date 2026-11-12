//! Exact input for the service-owned creation launch effect: one local,
//! detached `hgs AGENT TARGET --new ... -d` launch. These closed values carry
//! no current writer, frontend, profile, exclusion or execution authority.
//! Clone, worktree and task-start records stay separate.
use crate::{
    actions::tag_problem,
    creation,
    launch::{self, Launch},
    paths::{self, Style},
    ComputerId, Exit,
};
use serde::{de::IgnoredAny, Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashSet;

pub use super::creation_clone::Scope;

pub const ABI: &str = "desktop-creation-launch-v1";
pub const HELP_MARKER: &str = "Desktop creation launch ABI: desktop-creation-launch-v1";
/// Leaves room for the authenticated envelope inside the existing control line.
pub const SOURCE_BYTES: usize = 32 << 10;
pub const PROFILE: &str = "creation-launch-v1";
/// The closed agent set this leaf launches. Each starts detached (`-d`);
/// DeepSeek hosts and OpenCode, external terminals and remote hosts keep
/// the direct path.
pub const AGENTS: [&str; 3] = ["codex", "claude", "kimi"];
const PATH_BYTES: usize = 4096;
const NAME_BYTES: usize = 512;
const ACCOUNT_BYTES: usize = 256;
/// The launch child's retained diagnostic, as the direct path keeps it.
pub const STDERR_BYTES: usize = 4096;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    CreationLaunch,
}

/// Exact native reducer checkpoint. Imported recovery uses a separate route;
/// neither an import nor this value itself supplies fresh execution consent.
#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Intent {
    pub scope: Scope,
    pub creation_id: String,
    pub launch_id: String,
    pub revision: u64,
    pub original: String,
}

/// The fixed effect: the existing detached launch, through the shared
/// `launch::launch_invocation`. This recovery leaf carries no model choice;
/// the general launch path supports OpenCode, Claude, Codex and Kimi.
#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub launch_id: String,
    pub expected_machine: String,
    pub agent: String,
    /// The working folder the reducer selected (its `target`).
    pub target: String,
    /// The session name the reducer passes with `-n` (its `tag`).
    pub name: String,
    pub account: String,
}
fn posix_path(path: &str) -> bool {
    paths::absolute(path, Style::Posix)
        && !paths::windows_shaped(path)
        && path.len() <= PATH_BYTES
        && !path.chars().any(char::is_control)
}
fn account(value: &str) -> bool {
    value.len() <= ACCOUNT_BYTES
        && !value.starts_with('-')
        && !value.chars().any(|c| c.is_control() || c.is_whitespace())
}
/// Whether this leaf launches `agent`: the closed set, each detached.
pub fn supported(agent: &str) -> bool {
    AGENTS.contains(&agent) && launch::detached(agent)
}
impl Plan {
    /// Read-only recovery also accepts a well-formed later journal state. It
    /// supplies no fresh native admission or execution consent.
    pub fn from_operation(id: &str, value: &Value) -> Option<Self> {
        if !creation::valid_operation(id, value)
            || value["host"].as_str().is_some_and(|host| !host.is_empty())
            || flag(value, "terminal")
        {
            return None;
        }
        let text = |key| value[key].as_str().map(str::to_owned);
        let plan = Self {
            launch_id: text("launch_id")?,
            expected_machine: text("expected_machine")?,
            agent: text("cmd")?,
            target: text("target")?,
            name: text("tag")?,
            account: value["account"].as_str().unwrap_or("").to_owned(),
        };
        plan.valid().then_some(plan)
    }
    pub fn valid(&self) -> bool {
        uuid(&self.launch_id)
            && uuid(&self.expected_machine)
            && supported(&self.agent)
            && posix_path(&self.target)
            && self.name.len() <= NAME_BYTES
            && tag_problem(&self.name).is_none()
            && account(&self.account)
    }
    pub fn fingerprint(&self) -> Option<String> {
        self.valid()
            .then(|| hash(&serde_json::to_vec(self).expect("fixed launch plan")))
    }
    /// The exact arguments after the `hgs` program, as the direct path runs
    /// them for this computer.
    pub fn arguments(&self) -> Vec<String> {
        launch::launch_invocation(
            &ComputerId::local(),
            &Launch {
                agent: self.agent.clone(),
                target: self.target.clone(),
                name: self.name.clone(),
                account: self.account.clone(),
                launch_id: self.launch_id.clone(),
                model: String::new(),
                effort: String::new(),
            },
            &self.expected_machine,
        )
        .args
    }
    /// Matches an original launch result to these exact arguments. It is
    /// neither execution authority nor a current target identity.
    pub fn launch_fingerprint(&self) -> Option<String> {
        self.valid()
            .then(|| hash(&serde_json::to_vec(&self.arguments()).expect("fixed launch arguments")))
    }
}
// The checkpoint may contain a submitted prompt. Debug never prints its bytes.
impl std::fmt::Debug for Intent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CreationLaunchIntent")
            .field("creation_id", &self.creation_id)
            .field("launch_id", &self.launch_id)
            .field("revision", &self.revision)
            .finish_non_exhaustive()
    }
}

/// Persisted per-kind binding, deliberately unrelated to clone, worktree
/// and task-start.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub kind: Kind,
    pub schema: u32,
    pub configuration: String,
    pub creation_id: String,
    pub launch_id: String,
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
            || !uuid(&self.launch_id)
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
        // A clone and a worktree, when the creation has them, are settled.
        let clone_settled = if value.get("clone_id").is_some() {
            flag(&value, "repository") && flag(&value, "clone_done")
        } else {
            !flag(&value, "repository") && !flag(&value, "clone_done")
        };
        let worktree_settled = value.get("worktree_id").is_none() || flag(&value, "worktree_done");
        if !creation::valid_operation(&self.creation_id, &value)
            || value["revision"].as_u64() != Some(self.revision)
            || text("state") != "launching"
            || !text("host").is_empty()
            || flag(&value, "terminal")
            || text("launch_id") != self.launch_id
            || !uuid(text("expected_machine"))
            || !flag(&value, "launch_dispatched")
            || flag(&value, "stop_requested")
            || value.get("bound_session").is_some()
            || value
                .get("resource_error")
                .is_some_and(|error| error.as_str() != Some(""))
            || !clone_settled
            || !worktree_settled
            || ["worktree_id", "clone_id"].iter().any(|key| {
                value
                    .get(*key)
                    .is_some_and(|id| !id.as_str().is_some_and(uuid))
            })
        {
            return Err("creation_intent_changed");
        }
        let plan = Plan::from_operation(&self.creation_id, &value).ok_or("invalid_launch_plan")?;
        let encoded = serde_json::to_vec(&plan).map_err(|_| "invalid_launch_plan")?;
        let mut consent = b"desktop-creation-launch-consent-v1\0".to_vec();
        consent.extend_from_slice(self.original.as_bytes());
        let binding = Binding {
            kind: Kind::CreationLaunch,
            schema: 1,
            configuration: self.scope.configuration.clone(),
            creation_id: self.creation_id.clone(),
            launch_id: plan.launch_id.clone(),
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
        self.kind == Kind::CreationLaunch
            && self.schema == 1
            && super::digest(&self.configuration)
            && uuid(&self.creation_id)
            && uuid(&self.launch_id)
            && (1..=i64::MAX as u64).contains(&self.revision)
            && [&self.source_sha256, &self.plan_sha256, &self.consent_sha256]
                .into_iter()
                .all(|v| super::digest(v))
    }
    pub fn operation_id(&self) -> Option<String> {
        self.valid().then(|| {
            let mut bytes = b"desktop-creation-launch-key-v1\0".to_vec();
            bytes.extend(
                serde_json::to_vec(&(&self.configuration, &self.creation_id, &self.launch_id))
                    .expect("fixed launch key"),
            );
            hash(&bytes)
        })
    }
    pub fn fingerprint(&self) -> Option<String> {
        self.valid()
            .then(|| hash(&serde_json::to_vec(self).expect("fixed launch binding")))
    }
    /// Whether this binding names exactly `plan`: the launch it repeats,
    /// and the plan's own fingerprint. A declared digest never overrides a
    /// conflicting original field.
    pub fn agrees(&self, plan: &Plan) -> bool {
        self.launch_id == plan.launch_id
            && plan.fingerprint().as_deref() == Some(self.plan_sha256.as_str())
    }
}

pub const REPLY_BYTES: usize = 24 << 10;
pub const RESULT_BYTES: usize = 8 << 10;
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
            && self.request_id == intent.launch_id
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
    Launched,
    Failed,
    Unknown,
}
impl Phase {
    pub fn terminal(self) -> bool {
        matches!(self, Self::Launched | Self::Failed | Self::Unknown)
    }
}
/// The original launch child's exit after its physical drain: exactly
/// what the direct path's `launch::launched` reads.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Exited {
    pub schema: u32,
    pub launch_id: String,
    pub fingerprint: String,
    pub code: i32,
    pub stderr: String,
}
impl Exited {
    /// The drained exit of the original launch child for `plan`, its stderr
    /// cut to [`STDERR_BYTES`] of encoded JSON on a character boundary, so
    /// storing it never fails on size.
    pub fn new(plan: &Plan, code: i32, stderr: &str) -> Option<Self> {
        let mut size = 2;
        let mut kept = String::new();
        let mut buffer = [0u8; 4];
        for c in stderr.chars() {
            let encoded =
                serde_json::to_string(&*c.encode_utf8(&mut buffer)).map_or(6, |e| e.len() - 2);
            if size + encoded > STDERR_BYTES {
                break;
            }
            size += encoded;
            kept.push(c);
        }
        Some(Self {
            schema: 1,
            launch_id: plan.launch_id.clone(),
            fingerprint: plan.launch_fingerprint()?,
            code,
            stderr: kept,
        })
    }
}
/// The closed code of an original launch child that never started: refused
/// before any child existed (its own image, its custody, or the spawn). After
/// the custody drain its receipt is Failed with this code and no exit.
pub const NOT_STARTED: &str = "creation_launch_not_started";

#[derive(Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub binding: Binding,
    pub admission_epoch: String,
    pub revision: u64,
    pub phase: Phase,
    pub drained: bool,
    pub code: String,
    /// The original launch child's drained exit, never a replayed launch.
    pub launch: Option<Exited>,
}
impl std::fmt::Debug for Receipt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CreationLaunchReceipt")
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
            && self.launch.as_ref().is_none_or(|exited| {
                exited.schema == 1
                    && exited.launch_id == self.binding.launch_id
                    && super::digest(&exited.fingerprint)
                    && serde_json::to_string(&exited.stderr)
                        .is_ok_and(|encoded| encoded.len() <= STDERR_BYTES)
            })
            && match self.phase {
                Phase::Launched => {
                    self.drained && self.launch.as_ref().is_some_and(|e| e.code == 0)
                }
                Phase::Failed => {
                    self.drained
                        && match &self.launch {
                            Some(exited) => exited.code != 0,
                            None => self.code == NOT_STARTED,
                        }
                }
                _ => self.launch.is_none(),
            }
    }
    /// Whether this exact original's drained receipt proves that its launch
    /// child never started: Failed, no exit and the closed [`NOT_STARTED`]
    /// code. Nothing ran, so the launch counts as never sent.
    pub fn not_started(&self, plan: &Plan) -> bool {
        self.valid()
            && self.drained
            && self.binding.agrees(plan)
            && self.phase == Phase::Failed
            && self.launch.is_none()
            && self.code == NOT_STARTED
    }
    pub fn matches(&self, intent: &Intent) -> bool {
        self.valid()
            && intent
                .clone()
                .verify()
                .is_ok_and(|v| v.binding == self.binding)
    }
    /// Only the exact original child's drained exit advances the native
    /// creation reducer, through the direct path's own `launch::launched`.
    /// Exit 0 is launch-command success, not a running or current agent;
    /// the session listing still binds it. Queued, Running or Unknown never
    /// means success or failure.
    pub fn result(&self, plan: &Plan) -> Option<Result<(), String>> {
        if !self.valid() || !self.drained || !self.binding.agrees(plan) {
            return None;
        }
        let exited = self.launch.as_ref()?;
        if exited.launch_id != plan.launch_id || exited.fingerprint != plan.launch_fingerprint()? {
            return None;
        }
        let exit = Exit::Code {
            code: exited.code,
            stdout: Vec::new(),
            stderr: exited.stderr.clone(),
        };
        matches!(self.phase, Phase::Launched | Phase::Failed).then(|| launch::launched(exit))
    }
}

/// Bounded evidence about an original launch whose ledger row is Unknown,
/// read from its own start and end records and its name's binding. It is
/// display evidence only: it never settles the row, releases a charge,
/// rotates the launch ID or authorizes another launch. An end record is
/// nonexclusive (`binding_unresolved` stays true), and a binding still
/// waits for a fresh exact listing.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum Observation {
    Unknown,
    ObservedBinding {
        name: String,
        agent: String,
        run_id: String,
        conversation_id: String,
    },
    ObservedEnd {
        name: String,
        code: i32,
        ended_at: f64,
        binding_unresolved: bool,
    },
}
/// A session name as the launch records hold it: two or three parts, each
/// passing the shared `tag_problem` rule of a launch's own tag, new sessions,
/// rename and fork. Internal spaces are valid.
pub fn session_name(value: &str) -> bool {
    let parts: Vec<_> = value.split('/').collect();
    value.len() <= NAME_BYTES
        && (2..=3).contains(&parts.len())
        && parts.iter().all(|part| tag_problem(part).is_none())
}
impl Observation {
    pub fn valid(&self) -> bool {
        match self {
            Self::Unknown => true,
            Self::ObservedBinding {
                name,
                agent,
                run_id,
                conversation_id,
            } => {
                session_name(name)
                    && name.split('/').next() == Some(agent.as_str())
                    && AGENTS.contains(&agent.as_str())
                    && uuid(run_id)
                    && uuid(conversation_id)
            }
            Self::ObservedEnd {
                name,
                ended_at,
                binding_unresolved,
                ..
            } => {
                session_name(name)
                    && ended_at.is_finite()
                    && *ended_at >= 0.0
                    && *binding_unresolved
            }
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
    pub launch_id: String,
    pub plan_sha256: String,
}
impl Query {
    pub fn valid(&self) -> bool {
        uuid(&self.request_id)
            && self.scope.valid()
            && uuid(&self.creation_id)
            && uuid(&self.launch_id)
            && super::digest(&self.plan_sha256)
    }
    pub fn for_plan(scope: Scope, creation_id: String, plan: &Plan) -> Option<Self> {
        let query = Self {
            request_id: uuid::Uuid::new_v4().to_string(),
            scope,
            creation_id,
            launch_id: plan.launch_id.clone(),
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
    /// Present only beside an Unknown original receipt.
    pub observation: Option<Observation>,
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
                    && r.binding.launch_id == query.launch_id
                    && r.binding.plan_sha256 == query.plan_sha256
            })
            && self.observation.as_ref().is_none_or(|observation| {
                observation.valid()
                    && self
                        .receipt
                        .as_ref()
                        .is_some_and(|r| r.phase == Phase::Unknown)
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
    const OTHER: &str = "44444444-4444-4444-8444-444444444444";
    /// Project placement is the reducer's own concern before the launch.
    struct NeverPlace;
    impl Placement for NeverPlace {
        fn resolve(&mut self, _: &mut Value, _: &Value) -> Result<(), String> {
            Ok(())
        }
    }
    /// The real reducer's durable checkpoint, written just before it would
    /// start the direct detached launch.
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
                "host":"", "auto":false
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
                Effect::Launch { id, operation } => {
                    assert_eq!(id, ID);
                    let journal: Value = serde_json::from_slice(&saved.unwrap()).unwrap();
                    let value = &journal["operations"][ID];
                    assert_eq!(
                        value, &operation,
                        "the launch follows its durable checkpoint"
                    );
                    return Intent {
                        scope: Scope {
                            configuration: "a".repeat(64),
                            service_epoch: EPOCH.into(),
                        },
                        creation_id: ID.into(),
                        launch_id: value["launch_id"].as_str().unwrap().into(),
                        revision: value["revision"].as_u64().unwrap(),
                        original: serde_json::to_string(value).unwrap(),
                    };
                }
                Effect::Changed { .. } => (),
                _ => panic!("unexpected effect before the original launch"),
            }
        }
        panic!("the real creation reducer did not request its launch")
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
    fn accepts_the_real_reducers_durable_launch_checkpoint() {
        let intent = checkpoint();
        let original = intent.original.clone();
        let verified = intent.verify().unwrap();
        assert_eq!(verified.intent.original, original);
        assert_eq!(verified.binding.source_sha256, hash(original.as_bytes()));
        // The reducer's first launch ID is the creation's own UUID.
        assert_eq!(verified.plan.launch_id, ID);
        assert_eq!(verified.plan.agent, "codex");
        assert_eq!(verified.plan.target, "/srv/repo");
        assert_eq!(verified.plan.name, "example");
        assert_eq!(verified.plan.expected_machine, MACHINE);
        let operation: Value = serde_json::from_str(&original).unwrap();
        assert!(Plan::from_operation(ID, &operation).as_ref() == Some(&verified.plan));
        // Exactly the direct path's detached launch for this computer.
        assert_eq!(
            verified.plan.arguments(),
            [
                "--expect-machine",
                MACHINE,
                "codex",
                "/srv/repo",
                "--new",
                "-n",
                "example",
                "--launch-id",
                ID,
                "-d"
            ]
        );
        assert_eq!(
            verified.plan.launch_fingerprint().unwrap(),
            hash(&serde_json::to_vec(&verified.plan.arguments()).unwrap())
        );
        let mut remote = operation.clone();
        remote["host"] = json!("peer");
        assert!(Plan::from_operation(ID, &remote).is_none());
    }

    #[test]
    fn only_the_closed_detached_agent_set_is_launched_here() {
        for agent in AGENTS {
            assert!(launch::AGENTS.contains(&agent) && launch::detached(agent));
        }
        for agent in launch::AGENTS {
            assert_eq!(supported(agent), launch::detached(agent), "{agent}");
        }
        for agent in ["dsh", "opencode", "unknown-agent", ""] {
            assert!(!supported(agent), "{agent}");
            let mut intent = checkpoint();
            let mut raw: Value = serde_json::from_str(&intent.original).unwrap();
            raw["cmd"] = json!(agent);
            intent.original = serde_json::to_string(&raw).unwrap();
            assert!(intent.verify().is_err(), "accepted {agent}");
        }
        let mut intent = checkpoint();
        let mut raw: Value = serde_json::from_str(&intent.original).unwrap();
        raw["account"] = json!("work");
        raw["model"] = json!("ignored-for-codex");
        intent.original = serde_json::to_string(&raw).unwrap();
        let verified = intent.verify().unwrap();
        let arguments = verified.plan.arguments();
        assert_eq!(arguments[arguments.len() - 2..], ["--account", "work"]);
        assert!(!arguments.iter().any(|a| a == "--model"));
    }

    fn exited(verified: &Verified, code: i32, stderr: &str) -> Receipt {
        let plan = &verified.plan;
        Receipt {
            binding: verified.binding.clone(),
            admission_epoch: EPOCH.into(),
            revision: 3,
            phase: if code == 0 {
                Phase::Launched
            } else {
                Phase::Failed
            },
            drained: true,
            code: String::new(),
            launch: Some(Exited {
                schema: 1,
                launch_id: plan.launch_id.clone(),
                fingerprint: plan.launch_fingerprint().unwrap(),
                code,
                stderr: stderr.into(),
            }),
        }
    }

    #[test]
    fn only_the_exact_drained_original_exit_can_advance_a_creation() {
        let verified = checkpoint().verify().unwrap();
        let receipt = exited(&verified, 0, "");
        assert!(receipt.matches(&verified.intent));
        assert_eq!(receipt.result(&verified.plan), Some(Ok(())));
        for phase in [
            Phase::Unknown,
            Phase::Queued,
            Phase::Running,
            Phase::Reserved,
        ] {
            let mut unconfirmed = receipt.clone();
            unconfirmed.phase = phase;
            unconfirmed.drained = phase.terminal();
            assert!(
                !unconfirmed.valid(),
                "an exit belongs only to a settled launch"
            );
            unconfirmed.launch = None;
            assert!(unconfirmed.valid());
            assert!(unconfirmed.result(&verified.plan).is_none(), "{phase:?}");
        }
        let mut undrained = receipt.clone();
        undrained.drained = false;
        assert!(!undrained.valid());
        let mut mislabelled = receipt.clone();
        mislabelled.launch.as_mut().unwrap().code = 1;
        assert!(!mislabelled.valid(), "a failed exit is never Launched");
        let mut changed = receipt.clone();
        changed.launch.as_mut().unwrap().fingerprint = "b".repeat(64);
        assert!(changed.result(&verified.plan).is_none());
        let mut changed = receipt.clone();
        changed.launch.as_mut().unwrap().launch_id = OTHER.into();
        assert!(!changed.valid());
        let failed = exited(&verified, 2, "hgs: the session already exists");
        assert_eq!(
            failed.result(&verified.plan),
            Some(launch::launched(Exit::Code {
                code: 2,
                stdout: Vec::new(),
                stderr: "hgs: the session already exists".into(),
            }))
        );
        assert!(failed.result(&verified.plan).unwrap().is_err());
        let mut changed = verified.intent.clone();
        changed.launch_id = OTHER.into();
        assert!(changed.verify().is_err());
    }

    #[test]
    fn only_the_exact_drained_not_started_original_counts_as_never_sent() {
        let verified = checkpoint().verify().unwrap();
        let mut receipt = exited(&verified, 1, "");
        receipt.launch = None;
        receipt.code = NOT_STARTED.into();
        assert!(receipt.valid());
        assert!(receipt.not_started(&verified.plan));
        assert!(receipt.result(&verified.plan).is_none(), "no exit to read");
        // A Failed receipt without an exit carries only this code.
        let mut other = receipt.clone();
        other.code = "creation_launch_timed_out".into();
        assert!(!other.valid() && !other.not_started(&verified.plan));
        // An exit, an Unknown or undrained receipt, or another plan is not it.
        let mut exit = exited(&verified, 1, "");
        exit.code = NOT_STARTED.into();
        assert!(exit.valid() && !exit.not_started(&verified.plan));
        let mut unknown = receipt.clone();
        unknown.phase = Phase::Unknown;
        assert!(unknown.valid() && !unknown.not_started(&verified.plan));
        let mut undrained = receipt.clone();
        undrained.drained = false;
        assert!(!undrained.not_started(&verified.plan));
        let mut plan = verified.plan.clone();
        plan.target = "/srv/other".into();
        assert!(!receipt.not_started(&plan));
    }

    #[test]
    fn a_drained_exit_keeps_a_bounded_stderr_that_always_stores() {
        let verified = checkpoint().verify().unwrap();
        let noisy = "\"é\u{1b}\n".repeat(4000);
        let drained = Exited::new(&verified.plan, 3, &noisy).unwrap();
        assert!(serde_json::to_string(&drained.stderr).unwrap().len() <= STDERR_BYTES);
        assert!(noisy.starts_with(&drained.stderr) && !drained.stderr.is_empty());
        let mut receipt = exited(&verified, 3, "");
        receipt.launch = Some(drained);
        assert!(receipt.valid());
        assert!(receipt.result(&verified.plan).unwrap().is_err());
    }

    #[test]
    fn recovery_lookup_is_scoped_and_evidence_never_settles_a_launch() {
        let verified = checkpoint().verify().unwrap();
        let query =
            Query::for_plan(verified.intent.scope.clone(), ID.into(), &verified.plan).unwrap();
        let mut lookup = Lookup {
            abi: ABI.into(),
            request_id: query.request_id.clone(),
            scope: query.scope.clone(),
            read_only: true,
            found: true,
            receipt: Some(exited(&verified, 0, "")),
            observation: None,
        };
        assert!(lookup.matches(&query));
        lookup.receipt.as_mut().unwrap().admission_epoch = OTHER.into();
        assert!(
            lookup.matches(&query),
            "receipt epoch is historical, query scope is current"
        );
        lookup.scope.service_epoch = uuid::Uuid::new_v4().to_string();
        assert!(!lookup.matches(&query));
        lookup.scope = query.scope.clone();
        lookup.receipt.as_mut().unwrap().binding.plan_sha256 = "b".repeat(64);
        assert!(!lookup.matches(&query));
        // Evidence rides only beside an Unknown original, which stays Unknown.
        let ended = Observation::ObservedEnd {
            name: "codex/example".into(),
            code: 0,
            ended_at: 2.0,
            binding_unresolved: true,
        };
        lookup.receipt = Some(exited(&verified, 0, ""));
        lookup.observation = Some(ended.clone());
        assert!(
            !lookup.matches(&query),
            "a settled launch carries no evidence"
        );
        let unknown = lookup.receipt.as_mut().unwrap();
        unknown.phase = Phase::Unknown;
        unknown.launch = None;
        assert!(lookup.matches(&query));
        assert!(lookup
            .receipt
            .as_ref()
            .unwrap()
            .result(&verified.plan)
            .is_none());
        lookup.observation = Some(Observation::ObservedEnd {
            name: "codex/example".into(),
            code: 0,
            ended_at: 2.0,
            binding_unresolved: false,
        });
        assert!(
            !lookup.matches(&query),
            "an end record never resolves the binding"
        );
        lookup.observation = Some(Observation::ObservedBinding {
            name: "codex/example".into(),
            agent: "codex".into(),
            run_id: OTHER.into(),
            conversation_id: EPOCH.into(),
        });
        assert!(lookup.matches(&query));
        lookup.observation = Some(Observation::ObservedBinding {
            name: "claude/example".into(),
            agent: "codex".into(),
            run_id: OTHER.into(),
            conversation_id: EPOCH.into(),
        });
        assert!(!lookup.matches(&query));
        lookup.receipt = None;
        lookup.found = false;
        lookup.observation = Some(Observation::Unknown);
        assert!(!lookup.matches(&query), "no evidence without an original");
        lookup.observation = None;
        assert!(lookup.matches(&query));
        lookup.read_only = false;
        assert!(!lookup.matches(&query));
        lookup.read_only = true;
        lookup.abi = super::super::creation_worktree::ABI.into();
        assert!(
            !lookup.matches(&query),
            "a worktree reply is not a launch reply"
        );
    }

    #[test]
    fn evidence_for_a_tag_with_internal_spaces_matches_its_launch() {
        let mut intent = checkpoint();
        let mut raw: Value = serde_json::from_str(&intent.original).unwrap();
        raw["tag"] = json!("my task");
        intent.original = serde_json::to_string(&raw).unwrap();
        let verified = intent.verify().unwrap();
        let arguments = verified.plan.arguments();
        let at = arguments.iter().position(|a| a == "-n").unwrap();
        assert_eq!(arguments[at + 1], "my task");
        let query =
            Query::for_plan(verified.intent.scope.clone(), ID.into(), &verified.plan).unwrap();
        let mut receipt = exited(&verified, 0, "");
        receipt.phase = Phase::Unknown;
        receipt.launch = None;
        for observation in [
            Observation::ObservedBinding {
                name: "codex/repo/my task".into(),
                agent: "codex".into(),
                run_id: OTHER.into(),
                conversation_id: EPOCH.into(),
            },
            Observation::ObservedEnd {
                name: "codex/repo/my task".into(),
                code: 0,
                ended_at: 2.0,
                binding_unresolved: true,
            },
        ] {
            assert!(observation.valid(), "{observation:?}");
            let lookup = Lookup {
                abi: ABI.into(),
                request_id: query.request_id.clone(),
                scope: query.scope.clone(),
                read_only: true,
                found: true,
                receipt: Some(receipt.clone()),
                observation: Some(observation),
            };
            assert!(lookup.matches(&query));
        }
        // The shared per-part rule still holds every other bound.
        for name in [
            "codex/repo/ my task",
            "codex/repo/my task ",
            "codex/repo/my.task",
            "codex/repo/my:task",
            "codex/repo/my\ttask",
            "codex/repo//task",
            "codex",
            "codex/a/b/c",
        ] {
            assert!(!session_name(name), "{name:?}");
        }
        assert!(!session_name(&format!(
            "codex/repo/{}",
            "x".repeat(NAME_BYTES)
        )));
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
        assert!(first.binding.operation_id().is_some());
    }

    #[test]
    fn historical_changed_or_unsafe_checkpoints_never_become_fresh_launch_input() {
        for (key, value) in [
            ("state", json!("unknown")),
            ("state", json!("confirming")),
            ("host", json!("peer")),
            ("terminal", json!(true)),
            ("launch_dispatched", json!(false)),
            ("stop_requested", json!(true)),
            ("bound_session", json!("codex/example")),
            ("resource_error", json!("busy")),
            ("clone_id", json!(OTHER)),
            ("clone_done", json!(true)),
            ("worktree_id", json!(OTHER)),
            ("worktree_id", json!("not-a-uuid")),
            ("expected_machine", json!("")),
            ("revision", json!(0)),
            ("launch_id", json!(OTHER)),
            ("launch_id", json!("not-a-uuid")),
            ("target", json!("relative/repo")),
            ("target", json!("C:\\repo")),
            ("target", json!("/srv/repo\n")),
            ("tag", json!("a/b")),
            ("tag", json!(" padded")),
            ("account", json!("--json")),
            ("account", json!("two words")),
        ] {
            let mut intent = checkpoint();
            let mut raw: Value = serde_json::from_str(&intent.original).unwrap();
            raw[key] = value.clone();
            intent.original = serde_json::to_string(&raw).unwrap();
            assert!(intent.verify().is_err(), "accepted {key}={value}");
        }
        let mut intent = checkpoint();
        intent.original = intent
            .original
            .replacen('{', "{\"state\":\"launching\",", 1);
        assert!(intent.verify().is_err(), "duplicate keys are ambiguous");
        let mut intent = checkpoint();
        intent.original = " ".repeat(SOURCE_BYTES + 1);
        assert!(intent.verify().is_err());
        // A settled worktree is accepted before its launch.
        let mut intent = checkpoint();
        let mut raw: Value = serde_json::from_str(&intent.original).unwrap();
        raw["worktree_id"] = json!(OTHER);
        raw["worktree_done"] = json!(true);
        intent.original = serde_json::to_string(&raw).unwrap();
        intent.verify().unwrap();
    }

    #[test]
    fn a_receipt_settles_only_when_its_binding_names_the_plan_launch() {
        let verified = checkpoint().verify().unwrap();
        let mut not_started = exited(&verified, 1, "");
        not_started.launch = None;
        not_started.code = NOT_STARTED.into();
        for receipt in [
            exited(&verified, 0, ""),
            exited(&verified, 2, "hgs: the session already exists"),
            not_started,
        ] {
            assert!(
                receipt.result(&verified.plan).is_some() || receipt.not_started(&verified.plan)
            );
            // The declared plan digest never overrides a conflicting launch,
            // even when the receipt's own exit agrees with its binding.
            let mut changed = receipt.clone();
            changed.binding.launch_id = OTHER.into();
            if let Some(launch) = changed.launch.as_mut() {
                launch.launch_id = OTHER.into();
            }
            assert!(changed.valid(), "a valid shape");
            assert!(changed.result(&verified.plan).is_none());
            assert!(!changed.not_started(&verified.plan));
        }
    }
}
