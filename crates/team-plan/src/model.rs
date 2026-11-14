//! A team, its members, phases and plan nodes, and the settings that say what
//! the lead may decide.

use serde::{Deserialize, Serialize};

use crate::catalog::TemplateBundle;
use crate::is_default;

/// A lowercase UUID.
pub type TeamId = String;
/// `"lead"`, `"m1"`, `"m2"`: unique within a team.
pub type MemberId = String;
/// A counter per team, starting at 1.
pub type NodeId = u32;
/// The person behind a decision. `"local"` until several people share a team.
pub type PersonId = String;

/// The person identifier used while one person owns everything.
pub const LOCAL_PERSON: &str = "local";
/// The member ID of every team's lead.
pub const LEAD: &str = "lead";
/// The phase number of a team's queue; planned phases start at 1.
pub const QUEUE_PHASE: u32 = 0;

fn local_person() -> PersonId {
    LOCAL_PERSON.to_owned()
}

fn one() -> u32 {
    1
}

fn yes() -> bool {
    true
}

/// A Zerus task: its project and its UUID. A short task reference is never
/// stored, because it can differ between replicas.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskRef {
    pub project: String,
    pub uuid: String,
}

/// A session by identity. A session name is a label and never appears here.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionKey {
    /// The computer's UUID.
    pub machine: String,
    /// `launch_id:<uuid>` or `run_id:<id>`, as tasks identify a session.
    pub session_id: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamStatus {
    #[default]
    Draft,
    Planning,
    Running,
    Paused,
    Finished,
    Stopped,
    Archived,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberState {
    #[default]
    NotCreated,
    Active,
    Paused,
    Removed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhaseKind {
    #[default]
    Plan,
    /// The team's one open phase, which takes tasks sent one by one.
    Queue,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhaseMode {
    #[default]
    Standard,
    Free,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhaseState {
    #[default]
    Draft,
    Approved,
    /// Every task is decided and the person has not accepted the phase yet.
    AwaitingAcceptance,
    Done,
    Rejected,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Approval {
    /// The person approves.
    #[default]
    Review,
    Auto,
}

/// Who accepts a result. A phase is accepted by the lead or the person; a
/// pipeline may also accept on its own once every stage has passed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Acceptance {
    #[default]
    Lead,
    Person,
    Pipeline,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DepKind {
    /// The dependency must be accepted and merged.
    #[default]
    Hard,
    /// The dependency must have started.
    Soft,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    #[default]
    Task,
    Check,
    /// A review or a test of another node's result.
    Stage,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    #[default]
    None,
    Crit,
    Minor,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeSource {
    /// Drawn up by the lead in a plan.
    #[default]
    Planned,
    /// Sent to the team's queue.
    Sent,
}

/// Where a node stands. It is derived from the node's Zerus task and is never
/// the stored truth.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeState {
    /// No task exists yet.
    #[default]
    Draft,
    /// A queue node that is not admitted or not routed yet.
    Waiting,
    /// The task is in To do.
    Pending,
    /// The task is In progress.
    InProgress,
    /// The task is In review.
    Reported,
    /// The task is Done.
    Accepted,
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decider {
    Lead,
    Person,
    Pipeline,
}

/// How a task sent to the queue starts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Routing {
    /// Straight to the default pipeline and a free member.
    #[default]
    Direct,
    /// The lead sets the role and pipeline first.
    Lead,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Integration {
    /// Accepted work is merged into the team's integration branch.
    #[default]
    Branch,
    /// Each task's branch is its own result.
    None,
}

/// What happens to a member's conversation between tasks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextMode {
    #[default]
    Keep,
    Fresh,
}

/// What the person lets the team decide without them. All on for a new team.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Delegations {
    /// The lead moves a team task to Done.
    pub accept_results: bool,
    /// The lead answers members' questions and permission requests.
    pub answer_members: bool,
    /// The trust prompt of a folder Zerus created for the team is confirmed.
    pub confirm_trust: bool,
    /// A member whose agent exited by itself is resumed.
    pub resume_exited: bool,
    /// Accepting a result merges it into the integration branch.
    pub merge_accepted: bool,
}

impl Default for Delegations {
    fn default() -> Self {
        Self {
            accept_results: true,
            answer_members: true,
            confirm_trust: true,
            resume_exited: true,
            merge_accepted: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Caps {
    /// Members besides the lead.
    pub max_members: u32,
    pub max_phase_tasks: u32,
    /// Undecided tasks in a team. Decided queue tasks do not count.
    pub max_open_tasks: u32,
    /// Open questions of one attempt.
    pub max_open_questions: u32,
}

impl Default for Caps {
    fn default() -> Self {
        Self {
            max_members: 6,
            max_phase_tasks: 50,
            max_open_tasks: 500,
            max_open_questions: 3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Timers {
    /// A turn that ended without a report becomes a lead inbox item.
    pub report_timeout_seconds: u32,
    /// A hand-out that has not started becomes a lead inbox item.
    pub start_timeout_seconds: u32,
    /// At most one doorbell to the lead in this time.
    pub inbox_interval_seconds: u32,
    pub remind_lead_minutes: u32,
    pub remind_person_minutes: u32,
    /// A lead that leaves its inbox this long produces an owner card.
    pub lead_stall_minutes: u32,
    /// A member's request the lead leaves this long goes to the person.
    pub answer_escalate_minutes: u32,
    pub resume_limit_per_hour: u32,
}

impl Default for Timers {
    fn default() -> Self {
        Self {
            report_timeout_seconds: 120,
            start_timeout_seconds: 180,
            inbox_interval_seconds: 120,
            remind_lead_minutes: 15,
            remind_person_minutes: 60,
            lead_stall_minutes: 20,
            answer_escalate_minutes: 10,
            resume_limit_per_hour: 3,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub delegations: Delegations,
    pub caps: Caps,
    pub timers: Timers,
    /// The team takes tasks sent one by one.
    pub queue: bool,
    pub routing: Routing,
    pub plan_approval: Approval,
    /// `Lead` or `Person`; `Pipeline` is refused by validation.
    pub phase_acceptance: Acceptance,
    pub integration: Integration,
    /// A pipeline may end in a land stage. Only the person turns this on.
    pub land_allowed: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            delegations: Delegations::default(),
            caps: Caps::default(),
            timers: Timers::default(),
            queue: true,
            routing: Routing::Direct,
            plan_approval: Approval::Review,
            phase_acceptance: Acceptance::Lead,
            integration: Integration::Branch,
            land_allowed: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Team {
    pub id: TeamId,
    /// The Zerus project the team's tasks belong to.
    pub project: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub goal: String,
    /// The UUID of the computer where the lead runs.
    pub home: String,
    #[serde(default = "local_person")]
    pub owner: PersonId,
    pub status: TeamStatus,
    pub lead: MemberId,
    #[serde(default)]
    pub settings: Settings,
    /// The team's own copy of the template it was created from.
    pub bundle: TemplateBundle,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub integration_branch: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub base_commit: String,
    #[serde(default)]
    pub revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Member {
    pub id: MemberId,
    /// A role ID of the team's bundle, or `"lead"`.
    pub role: String,
    pub agent: String,
    /// The account the member was launched with; empty until then.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub account: String,
    #[serde(default, skip_serializing_if = "is_default")]
    pub state: MemberState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<SessionKey>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub worktree: String,
    /// The nonce of the worktree's Zerus ownership record.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ownership_nonce: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Phase {
    /// [`QUEUE_PHASE`] for the queue; planned phases count from 1.
    pub n: u32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub order: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    pub kind: PhaseKind,
    #[serde(default, skip_serializing_if = "is_default")]
    pub mode: PhaseMode,
    #[serde(default, skip_serializing_if = "is_default")]
    pub approval: Approval,
    pub state: PhaseState,
    #[serde(default, skip_serializing_if = "is_default")]
    pub acceptance: Acceptance,
    /// Its tasks wait for every task of earlier phases.
    #[serde(default, skip_serializing_if = "is_default")]
    pub barrier: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub paused: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dep {
    pub node: NodeId,
    #[serde(default, skip_serializing_if = "is_default")]
    pub kind: DepKind,
}

/// One task of a team with the structure the team adds around it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub id: NodeId,
    pub phase: u32,
    /// Absent while the node is a draft.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<TaskRef>,
    pub title: String,
    /// Every member who implemented any round of this task, read from the
    /// store's own records (zerus-63rru.20); a stage of the task never goes
    /// to any of them. Kept out of the serialized rows so an older build
    /// reads the store exactly as before.
    #[serde(default, skip)]
    pub earlier_implementers: Vec<MemberId>,
    /// The task text of a draft. Once the task exists, the task holds it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    #[serde(default, skip_serializing_if = "is_default")]
    pub source: NodeSource,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deps: Vec<Dep>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<MemberId>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub role: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub pipeline: String,
    /// Files or folders the task may change.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub writes: Vec<String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub estimate_minutes: u32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub kind: NodeKind,
    #[serde(default, skip_serializing_if = "is_default")]
    pub severity: Severity,
    /// The node a finding came from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<NodeId>,
    /// For a stage: the node whose result it checks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage_of: Option<NodeId>,
    /// For a stage: its position in the pipeline, from 0.
    #[serde(default, skip_serializing_if = "is_default")]
    pub stage_index: u32,
    /// 1 on the first pass; grows with each return.
    #[serde(default = "one")]
    pub round: u32,
    /// The member who implemented the task. A stage is never handed to it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub implementer: Option<MemberId>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub hold: bool,
    /// A queue node is admitted when the person or the lead let it in.
    #[serde(default = "yes")]
    pub admitted: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub state: NodeState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_by: Option<Decider>,
    /// The accepted result is merged into the integration branch.
    #[serde(default, skip_serializing_if = "is_default")]
    pub integrated: bool,
    /// The task's board rank: the order of the queue.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub rank: String,
}

/// A whole small team in one value, for fixtures and tests. Production reads
/// are rows and pages and never build this.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamFixture {
    pub team: Team,
    pub members: Vec<Member>,
    pub phases: Vec<Phase>,
    pub nodes: Vec<Node>,
}
