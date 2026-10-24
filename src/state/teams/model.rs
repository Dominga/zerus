//! WhiteWh fork: the agent team record (docs/superpowers/specs/2026-10-24-agent-teams-core-design.md).
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
    /// How new phases are approved; absent means the plan's `approval`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase_approval: Option<Approval>,
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
fn default_phase() -> u32 {
    1
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            all_hard: false,
            folder_policy: FolderPolicy::default(),
            approval: Approval::default(),
            phase_approval: None,
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

impl Notice {
    fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }
}

/// Who finished the team, and with what summary.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct Finish {
    /// The lead or you.
    pub by: String,
    pub summary: String,
    pub at: f64,
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
    /// The phase of the plan: the approved initial plan is 1, `extend`
    /// appends later phases.
    #[serde(default = "default_phase")]
    pub phase: u32,
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

/// What happened to a phase appended by `extend`.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PhaseState {
    /// Waits for the owner's approval.
    #[default]
    Draft,
    /// The owner approved it, or its approval was `auto`.
    Approved,
    /// The owner discarded it; its ids stay used.
    Discarded,
}

/// One phase appended to the plan after the approved one. Phase 1 is implicit
/// and has no record.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct Phase {
    pub n: u32,
    #[serde(default)]
    pub title: String,
    /// No task of this phase starts before every lower phase is finished.
    #[serde(default)]
    pub barrier: bool,
    /// The effective phase approval when the phase was added.
    #[serde(default)]
    pub approval: Approval,
    #[serde(default)]
    pub state: PhaseState,
    /// The phase's task ids; a discarded phase keeps them, so they are never reused.
    #[serde(default)]
    pub tasks: Vec<u32>,
    #[serde(default)]
    pub added_by: String,
    #[serde(default)]
    pub added_at: f64,
    #[serde(default)]
    pub decided_by: String,
    #[serde(default)]
    pub decided_at: Option<f64>,
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
    /// Phases added by `extend`; the approved initial plan (phase 1) has none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub phases: Vec<Phase>,
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
    /// The "all tasks decided" message to the lead: sent once per such state.
    #[serde(default, skip_serializing_if = "Notice::is_none")]
    pub idle: Notice,
    /// Who finished the team and with what summary; set by `finish`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish: Option<Finish>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn old_team() -> Value {
        json!({
            "id":"old",
            "name":"Old team",
            "lead":"lead",
            "status":"running",
            "settings":{"approval":"auto"},
            "members":[{"id":"lead","role":"Lead","agent":"claude"}],
            "tasks":[{"id":1,"title":"Model","status":"accepted"},{"id":2,"title":"Tests","deps":[{"task":1}]}]
        })
    }

    #[test]
    fn old_team_json_loads_unchanged() {
        let team: Team = serde_json::from_value(old_team()).unwrap();
        assert!(team.phases.is_empty());
        // Absent phase approval means the plan's approval.
        assert_eq!(team.settings.phase_approval, None);
        assert_eq!(team.settings.phase_approval.unwrap_or(team.settings.approval), Approval::Auto);
        assert!(team.tasks.iter().all(|task| task.phase == 1 && !task.pinned));
        // Writing it back adds only the phase to each task.
        let value = serde_json::to_value(&team).unwrap();
        assert!(value.get("phases").is_none());
        assert!(value["settings"].get("phase_approval").is_none());
        assert!(value["tasks"].as_array().unwrap().iter().all(|task| task["phase"] == json!(1) && task.get("pinned").is_none()));
        assert_eq!(value["settings"]["approval"], "auto");
        assert_eq!(value["tasks"][1]["deps"][0], json!({"task":1,"kind":"hard"}));
        let back: Team = serde_json::from_value(value).unwrap();
        assert_eq!(back, team);
    }

    fn phased_team() -> Team {
        Team {
            id: "phased".into(),
            name: "Phased".into(),
            lead: "lead".into(),
            settings: Settings { approval: Approval::Review, phase_approval: Some(Approval::Auto), ..Default::default() },
            tasks: vec![Task { id: 3, phase: 2, title: "Fix".into(), pinned: true, ..Default::default() }],
            phases: vec![Phase {
                n: 2,
                title: "QA fixes".into(),
                barrier: true,
                approval: Approval::Auto,
                state: PhaseState::Draft,
                tasks: vec![3],
                added_by: "Lead".into(),
                added_at: 1792868401.5,
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn new_phase_fields_round_trip() {
        let team = phased_team();
        let text = serde_json::to_string(&team).unwrap();
        let back: Team = serde_json::from_str(&text).unwrap();
        assert_eq!(back, team);
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["tasks"][0]["phase"], json!(2));
        assert_eq!(value["tasks"][0]["pinned"], json!(true));
        assert_eq!(value["phases"][0]["n"], json!(2));
        assert_eq!(value["phases"][0]["state"], json!("draft"));
        assert_eq!(value["phases"][0]["approval"], json!("auto"));
        assert_eq!(value["phases"][0]["barrier"], json!(true));
        assert_eq!(value["phases"][0]["decided_at"], Value::Null);
        assert_eq!(value["settings"]["phase_approval"], json!("auto"));
    }

    #[test]
    fn the_waiting_and_finish_fields_round_trip() {
        let mut team = phased_team();
        team.idle = Notice::Pending;
        team.finish = Some(Finish { by: "Lead".into(), summary: "Shipped it".into(), at: 1.5 });
        let text = serde_json::to_string(&team).unwrap();
        let back: Team = serde_json::from_str(&text).unwrap();
        assert_eq!(back, team);
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["idle"], "pending");
        assert_eq!(value["finish"]["summary"], "Shipped it");
        // Without them, the old record still has neither key.
        let old: Value = serde_json::to_value(serde_json::from_value::<Team>(old_team()).unwrap()).unwrap();
        assert!(old.get("idle").is_none() && old.get("finish").is_none());
    }

    #[test]
    fn phase_approval_falls_back_to_the_plan_approval() {
        let review = Settings::default();
        assert_eq!(review.phase_approval, None);
        assert_eq!(review.phase_approval.unwrap_or(review.approval), Approval::Review);
        let auto = Settings { approval: Approval::Auto, ..Default::default() };
        assert_eq!(auto.phase_approval.unwrap_or(auto.approval), Approval::Auto);
        let review_phases = Settings { approval: Approval::Auto, phase_approval: Some(Approval::Review), ..Default::default() };
        assert_eq!(review_phases.phase_approval.unwrap_or(review_phases.approval), Approval::Review);
    }
}
