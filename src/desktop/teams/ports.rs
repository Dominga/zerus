//! Effects reuse the maintained worktree and launch CLI, through the shared
//! service original-child runner. Each port settles only its original child, pipes and
//! custody. A missing or ambiguous result is Unknown, never a new admission.
use serde::{Deserialize, Serialize};
use zerus_team_plan::{accounts::AccountUsage, model::SessionKey, Problem};

pub(super) type Result<T> = std::result::Result<T, Problem>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Plan {
    pub id: String,
    pub team: String,
    pub person: String,
    pub machine: String,
    pub project: String,
    pub member: String,
    pub role: String,
    pub agent: String,
    pub account: String,
    pub model: String,
    pub effort: String,
    pub base_commit: String,
    pub placement: Placement,
    pub parent: Option<SessionKey>,
    pub launch_id: String,
    /// Chosen once for a new launch. Older plans retain their original label.
    /// This display value never selects or binds a session.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub session_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Placement {
    pub source: String,
    pub git_dir: String,
    pub branch: String,
    pub destination: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Worktree {
    pub path: String,
    pub ownership_nonce: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ObservedSession {
    pub machine: String,
    pub launch_id: String,
    pub agent: String,
    pub account: String,
    pub worktree: Worktree,
    pub parent: Option<SessionKey>,
    // Display data deliberately does not participate in binding.
    pub name: String,
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub run_id: String,
    #[serde(default)]
    pub conversation_id: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum LaunchRead {
    Session(Box<ObservedSession>),
    Absent { legacy: bool },
    Ended { code: i32 },
}

pub(super) enum Effect<T> {
    NotStarted,
    /// A read failed before the effect started; retry preparation with backoff.
    Deferred(Problem),
    Submitted(T),
    Refused(Problem),
    Unknown(Problem),
}

/// Called only by the service worker. Readers are bounded, and the native
/// adapters must independently validate current service admission, generated
/// ownership and the exact fixed plan before every effect. Return Submitted
/// only after original custody actually settles. A result read never admits
/// execution. No implementation is constructed from CLI input.
pub(super) trait Effects {
    fn hosted_ready(&mut self, _plan: &Plan, _bound: bool) -> Result<()> { Ok(()) }
    fn placement(&mut self, project: &str, launch: &str) -> Result<Placement>;
    fn branch_exists(&mut self, plan: &super::ledger::BranchPlan) -> Result<bool>;
    fn create_branch(&mut self, plan: &super::ledger::BranchPlan) -> Effect<()>;
    fn accounts(&mut self, agent: &str) -> Result<Vec<AccountUsage>>;
    /// None means creation settled successfully but ownership still needs a read.
    fn worktree(&mut self, plan: &Plan, original: &str) -> Effect<Option<Worktree>>;
    fn verify_worktree(&mut self, plan: &Plan) -> Result<Worktree>;
    fn launch(&mut self, plan: &Plan, worktree: &Worktree, original: &str) -> Effect<()>;
    /// Exact launch ID lookup; ambiguity must return an error, not one match.
    fn observe(&mut self, plan: &Plan) -> Result<LaunchRead>;
    fn parent(&mut self, plan: &Plan, session: &ObservedSession, original: &str) -> String;
}
