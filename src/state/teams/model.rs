//! The agent team record (docs/superpowers/specs/2026-10-24-agent-teams-core-design.md).
//! One team is stored whole as JSON; these types are its schema.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TeamStatus {
    #[default]
    Planning,
    Running,
    Paused,
    Stopped,
    Done,
    Archived,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FolderPolicy {
    /// One worktree per writing member.
    Worktree,
    /// Every member works in the team's folder.
    Shared,
    /// The lead chooses per member while planning.
    #[default]
    Lead,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Approval {
    /// The plan waits for Approve & Run.
    #[default]
    Review,
    /// The plan runs as soon as it has no errors.
    Auto,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub(crate) struct Settings {
    /// Every dependency is hard, whatever the lead chose.
    #[serde(default)]
    pub all_hard: bool,
    #[serde(default)]
    pub folder_policy: FolderPolicy,
    #[serde(default)]
    pub approval: Approval,
    #[serde(default = "default_max_members")]
    pub max_members: usize,
    /// Idle time after a delivered turn ends before a member shows No report.
    #[serde(default = "default_report_timeout")]
    pub report_timeout_seconds: u64,
}

fn default_max_members() -> usize {
    6
}
fn default_report_timeout() -> u64 {
    120
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            all_hard: false,
            folder_policy: FolderPolicy::default(),
            approval: Approval::default(),
            max_members: default_max_members(),
            report_timeout_seconds: default_report_timeout(),
        }
    }
}

/// A member's Zerus session, once it exists.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct SessionRef {
    /// SSH alias of the member's computer; empty for this computer.
    #[serde(default)]
    pub host: String,
    pub name: String,
    #[serde(default)]
    pub run_id: String,
    #[serde(default)]
    pub conversation_id: String,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MemberState {
    /// Planned; its session starts on approval.
    #[default]
    NotCreated,
    Active,
    Paused,
    Removed,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MemberFolder {
    Worktree,
    #[default]
    Shared,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct Member {
    /// Stable within the team, such as `backend`.
    pub id: String,
    #[serde(default)]
    pub role: String,
    pub agent: String,
    #[serde(default)]
    pub account: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub effort: String,
    #[serde(default)]
    pub permissions: String,
    /// SSH alias of the computer; empty for the lead's computer.
    #[serde(default)]
    pub computer: String,
    #[serde(default)]
    pub folder: MemberFolder,
    /// The folder on the member's computer; empty for the team's folder.
    #[serde(default)]
    pub directory: String,
    #[serde(default)]
    pub session: Option<SessionRef>,
    #[serde(default)]
    pub state: MemberState,
    /// Recorded before the dispatcher starts the member's session, which
    /// `hgs ls` then shows under the same id.
    #[serde(default)]
    pub launch_id: String,
    /// Why the session could not start; the dispatcher does not try again
    /// until you relaunch it.
    #[serde(default)]
    pub error: String,
    /// The role's instructions from the catalog, given with every task.
    #[serde(default)]
    pub prompt: String,
    /// DeepSeek takes its model and effort after it starts; once tried, the
    /// dispatcher does not try again.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub settings_applied: bool,
    /// The member's questions already given to the lead, as question id and
    /// hash; the most recent ones only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub asked: Vec<String>,
    /// The conversation whose notifications the dispatcher muted: a member
    /// works for the lead, so its replies do not call the owner. Muted once
    /// per conversation; the owner may unmute it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub quieted: String,
}

impl Member {
    /// A started DeepSeek member whose role chose a model or effort.
    pub(crate) fn needs_settings(&self) -> bool {
        self.agent == "dsh" && !self.settings_applied && (!self.model.is_empty() || !self.effort.is_empty())
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DependencyKind {
    /// The task waits until the dependency is Accepted.
    #[default]
    Hard,
    /// The task may start once the dependency is In progress.
    Soft,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct Dependency {
    pub task: u32,
    #[serde(default)]
    pub kind: DependencyKind,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TaskStatus {
    Draft,
    #[default]
    Pending,
    Assigned,
    InProgress,
    Submitted,
    Accepted,
    Rejected,
    Cancelled,
}

impl TaskStatus {
    pub(crate) fn finished(self) -> bool {
        matches!(self, Self::Accepted | Self::Rejected | Self::Cancelled)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Decider {
    Lead,
    Human,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AttemptState {
    /// Recorded before the hand-off is sent.
    #[default]
    Delivering,
    Delivered,
    /// The hand-off may or may not have reached the agent; a human decides.
    Unconfirmed,
    /// Refused before anything reached the agent.
    Refused,
    /// Replaced by a reassignment, retry or take-over.
    Revoked,
    Reported,
    Rejected,
}

impl AttemptState {
    /// The attempt still owns its task.
    pub(crate) fn open(self) -> bool {
        matches!(self, Self::Delivering | Self::Delivered | Self::Unconfirmed)
    }
}

/// Forwarding a report to the lead, like a hand-off, is never retried after
/// it may have arrived.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Notice {
    #[default]
    None,
    Pending,
    Sending,
    Sent,
    Unconfirmed,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct Report {
    pub text: String,
    pub at: f64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct Attempt {
    pub n: u32,
    /// The hand-off's request id.
    pub id: String,
    pub member: String,
    #[serde(default)]
    pub state: AttemptState,
    #[serde(default)]
    pub sent_at: f64,
    #[serde(default)]
    pub delivered_at: Option<f64>,
    /// When the member's turn after the hand-off ended, for No report.
    #[serde(default)]
    pub turn_ended_at: Option<f64>,
    /// The member's turn ended long enough ago without a report.
    #[serde(default)]
    pub no_report: bool,
    #[serde(default)]
    pub report: Option<Report>,
    /// The report's message to the lead.
    #[serde(default)]
    pub notice: Notice,
    #[serde(default)]
    pub notice_id: String,
    #[serde(default)]
    pub error: String,
    /// What the lead was told about this attempt's member on its own
    /// (stopped, no_report), once each.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub told_lead: Vec<String>,
    /// The member the task was assigned to when Zerus gave it to this
    /// attempt's member instead (a free member with the same role).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub rebalanced_from: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct Task {
    pub id: u32,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub criteria: Vec<String>,
    #[serde(default)]
    pub deps: Vec<Dependency>,
    #[serde(default)]
    pub assignee: String,
    /// The lead or you chose this member: Zerus does not give the task to
    /// another member with the same role.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
    #[serde(default)]
    pub status: TaskStatus,
    #[serde(default)]
    pub attempts: Vec<Attempt>,
    #[serde(default)]
    pub decided_by: Option<Decider>,
    #[serde(default)]
    pub decision_reason: String,
}

impl Task {
    pub(crate) fn open_attempt(&self) -> Option<&Attempt> {
        self.attempts.iter().rev().find(|attempt| attempt.state.open())
    }
    pub(crate) fn open_attempt_mut(&mut self) -> Option<&mut Attempt> {
        self.attempts.iter_mut().rev().find(|attempt| attempt.state.open())
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct Team {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub goal: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub folder: String,
    #[serde(default)]
    pub template: String,
    #[serde(default)]
    pub status: TeamStatus,
    #[serde(default)]
    pub settings: Settings,
    /// The member id of the lead.
    pub lead: String,
    #[serde(default)]
    pub members: Vec<Member>,
    #[serde(default)]
    pub tasks: Vec<Task>,
    #[serde(default)]
    pub revision: u64,
    #[serde(default)]
    pub created: f64,
    #[serde(default)]
    pub updated: f64,
    /// A team created from the desktop starts its lead and sends it this
    /// planning brief once, like a report to the lead.
    #[serde(default)]
    pub brief: Notice,
    #[serde(default)]
    pub brief_id: String,
    /// The template's lead protocol when the team was created, given to the
    /// lead with the planning brief.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub lead_protocol: String,
}

impl Team {
    pub(crate) fn member(&self, id: &str) -> Option<&Member> {
        self.members.iter().find(|member| member.id == id)
    }
    pub(crate) fn task(&self, id: u32) -> Option<&Task> {
        self.tasks.iter().find(|task| task.id == id)
    }
    pub(crate) fn task_mut(&mut self, id: u32) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|task| task.id == id)
    }
    /// The role name a hand-off or report header shows for a member.
    pub(crate) fn role_of(&self, member: &str) -> String {
        if member == self.lead {
            return "Lead".into();
        }
        self.member(member).map(|m| m.role.clone()).unwrap_or_else(|| member.to_owned())
    }
}
