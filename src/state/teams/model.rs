//! WhiteWh fork: the agent team record (docs/superpowers/specs/2026-10-24-agent-teams-core-design.md).
//! One team is stored whole as JSON; these types are its schema.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TeamStatus {
    /// F5: a team the owner and the lead are still drafting, before Planning.
    Draft,
    #[default]
    Planning,
    Running,
    Paused,
    Stopped,
    Done,
    Archived,
}

/// F5: a draft team's editing state: who changed what since the other side last
/// looked, so the owner's edit always wins and the lead sees the changes.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct Draft {
    #[serde(default)]
    pub revision: u64,
    /// Fields the lead changed that the owner has not seen yet.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unseen: Vec<String>,
    /// Fields the owner edited after the lead's last read; a later lead edit of
    /// one is refused ("owner changed <field>").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub owner_edited: Vec<String>,
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
    /// The lead chooses per member: shared, a worktree or a clone (B6).
    Auto,
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
    /// How long a delivered hand-off may stay unstarted before the lead is
    /// told once that the member has not started (A1).
    #[serde(default = "default_start_timeout")]
    pub start_timeout_seconds: u64,
    /// How long a finished turn may stay blocked by an open question or by
    /// Background work before the lead is told once (N5).
    #[serde(default = "default_blocked_timeout")]
    pub blocked_timeout_seconds: u64,
    /// At most one inbox message reaches the lead per this many seconds (C2).
    #[serde(default = "default_inbox_interval")]
    pub inbox_interval_seconds: u64,
    /// Minutes an undecided report waits before the lead is reminded once;
    /// 0 turns the reminder off (C4).
    #[serde(default = "default_remind_lead")]
    pub remind_lead_minutes: u64,
    /// Minutes before an undecided report shows on the owner's attention list in
    /// the desktop; 0 turns it off (C4).
    #[serde(default = "default_remind_owner")]
    pub remind_owner_minutes: u64,
    /// U18: minutes a member's unanswered permission request waits before it
    /// reaches the owner's attention list; 0 turns only that off.
    #[serde(default = "default_remind_owner_permission")]
    pub remind_owner_permission_minutes: u64,
    /// Minutes a Background job without a declared budget may run before the
    /// lead hears it is red (C12).
    #[serde(default = "default_background_red")]
    pub background_red_minutes: u64,
    /// A project that forbids worktrees and clones (B6): members then work in
    /// the shared or their own existing folder only.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub worktrees_forbidden: bool,
    /// H1: resume a member's session after a usage-limit reset, even when the
    /// shared automatic-recovery policy is off.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub resume_after_limits: bool,
    /// N19 (spec 7, 12): folders whose images and HTML the desktop previews
    /// as report evidence, besides the team and member folders.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_folders: Vec<String>,
    /// Hierarchy (section 9.2), set on the master team: how deep sub-teams may
    /// go below it and how many sessions the whole tree may run.
    #[serde(default = "default_tree_depth", skip_serializing_if = "tree_depth_is_default")]
    pub tree_depth: u32,
    #[serde(default = "default_tree_sessions", skip_serializing_if = "tree_sessions_is_default")]
    pub tree_sessions: u32,
    /// Who approves a sub-team's plan and phases (section 9.3).
    #[serde(default, skip_serializing_if = "subteam_approval_is_default")]
    pub subteam_approval: SubteamApproval,
    /// U25: draft phases without errors are accepted at once, also the ones
    /// waiting when the owner turns it on.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub auto_accept: bool,
}

fn default_max_members() -> usize {
    6
}
/// Section 9.2: two levels of sub-teams below the master, and sixteen active
/// sessions in the whole tree.
fn default_tree_depth() -> u32 {
    2
}
fn default_tree_sessions() -> u32 {
    16
}
fn tree_depth_is_default(depth: &u32) -> bool {
    *depth == default_tree_depth()
}
fn tree_sessions_is_default(sessions: &u32) -> bool {
    *sessions == default_tree_sessions()
}
fn subteam_approval_is_default(approval: &SubteamApproval) -> bool {
    *approval == SubteamApproval::Parent
}
fn default_report_timeout() -> u64 {
    120
}
fn default_start_timeout() -> u64 {
    180
}
fn default_blocked_timeout() -> u64 {
    1800
}
fn default_inbox_interval() -> u64 {
    120
}
fn default_remind_lead() -> u64 {
    15
}
fn default_remind_owner() -> u64 {
    60
}
/// U18: a permission request reaches the owner sooner than an undecided report:
/// the member is blocked meanwhile.
fn default_remind_owner_permission() -> u64 {
    10
}
fn default_background_red() -> u64 {
    30
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
            start_timeout_seconds: default_start_timeout(),
            blocked_timeout_seconds: default_blocked_timeout(),
            inbox_interval_seconds: default_inbox_interval(),
            remind_lead_minutes: default_remind_lead(),
            remind_owner_minutes: default_remind_owner(),
            remind_owner_permission_minutes: default_remind_owner_permission(),
            background_red_minutes: default_background_red(),
            worktrees_forbidden: false,
            resume_after_limits: false,
            evidence_folders: Vec::new(),
            tree_depth: default_tree_depth(),
            tree_sessions: default_tree_sessions(),
            subteam_approval: SubteamApproval::default(),
            auto_accept: false,
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
    /// A full clone with its own `.git`, so commit, merge and fetch work
    /// inside a sandboxed member's folder (B4/B6).
    Clone,
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
    /// The launch permission mode (C3): auto or bypass; empty for the agent's
    /// default.
    #[serde(default)]
    pub permissions: String,
    /// The role's permissions profile (B5): read, write, build, commit or
    /// network, mapped to the agent's own mode where it has one.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub permissions_profile: String,
    /// The lead's or owner's answers to this member's permission requests,
    /// newest last, at most 20 (B1): {question_id, decision, by, at, reason}.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub permission_answers: Vec<serde_json::Value>,
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
    /// U32: how the member joined on the owner's verified word:
    /// `{by, owner_word, at}`; absent when the owner added it directly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added: Option<Value>,
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
    /// The permission request the lead's inbox names while it waits for an
    /// answer (C2): the member's session question as Zerus read it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending: Option<PendingQuestion>,
    /// The first tick that saw this member idle without a ready task while
    /// tasks were open (C7); cleared when it gets work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub starved_since: Option<f64>,
    /// The lead was told once about this starvation episode (C7).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub starved_told: bool,
    /// The member's declared time budget for its background work (C12).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<Budget>,
    /// The first tick that saw Background work in this member's session; the
    /// default red time counts from here (C12).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background_since: Option<f64>,
    /// The lead heard once that this Background work passed the default red
    /// time (C12).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub background_told: bool,
    /// The MCP servers and services the member's session runs: they live with
    /// the session and are not Background work (G7).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub services: Vec<MemberService>,
    /// The conversation whose notifications the dispatcher muted: a member
    /// works for the lead, so its replies do not call the owner. Muted once
    /// per conversation; the owner may unmute it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub quieted: String,
    /// The last liveness the dispatcher saw for this member's session: status
    /// derives `stopped` from it without a listing of its own (L3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seen: Option<MemberSeen>,
    /// The permission request the lead last heard about (B1): the exact
    /// command, where it came from and the agent's reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission: Option<serde_json::Value>,
    /// H1: the member's session is stopped by a usage limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limited: Option<MemberLimit>,
    /// H1: Zerus asked the recovery service to wait for this member's reset.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub limit_resume: bool,
    /// N29: the session that request names, so a swap or the end of the
    /// episode withdraws it for that session, not for the next one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_session: Option<SessionRef>,
    /// H6: a swapped member's new session starts even before its next task.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub launch_after_swap: bool,
    /// Spec 5.6 (U25): the member it reports to - the lead (empty) or a
    /// sub-lead of the same team.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub parent: String,
    /// Spec 5.6: a sub-lead hands out, reviews and accepts the tasks of the
    /// members that report to it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sub: bool,
    /// A sub-lead's own coalesced inbox, as the team's is the lead's (C2).
    #[serde(default, skip_serializing_if = "Inbox::is_empty")]
    pub inbox: Inbox,
    /// N10: the sub-lead brief has not been sent yet.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sub_brief: bool,
}

/// U21: one built-in command template Zerus ran for this attempt, outside the
/// member's sandbox.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct RunRecord {
    pub template: String,
    #[serde(default)]
    pub args: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit: Option<i32>,
    #[serde(default)]
    pub at: f64,
    #[serde(default)]
    pub ms: u64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub timed_out: bool,
}

/// One commit Zerus made for an attempt (B4).
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct CommitRef {
    pub sha: String,
    #[serde(default)]
    pub at: f64,
    #[serde(default)]
    pub by: String,
    #[serde(default)]
    pub files: usize,
}

/// A member's session as the dispatcher last saw it in the live listing.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct MemberSeen {
    pub at: f64,
    pub live: bool,
}

impl Member {
    /// A started DeepSeek member whose role chose a model or effort.
    pub(crate) fn needs_settings(&self) -> bool {
        self.agent == "dsh" && !self.settings_applied && (!self.model.is_empty() || !self.effort.is_empty())
    }
}

/// The owner's own sentence that let the lead approve a plan or a phase (F13):
/// the exact text, its time and the event it came from.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct OwnerWord {
    pub text: String,
    #[serde(default)]
    pub at: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub event: i64,
}

fn is_zero(value: &i64) -> bool {
    *value == 0
}

fn is_zero_u64(value: &u64) -> bool {
    *value == 0
}

/// An MCP server or service a member's session runs (G7): shown on the member
/// card and never counted as Background work.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct MemberService {
    pub name: String,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub since: f64,
}

/// What a sub-team's plan and phases wait for (section 9.3): the parent lead
/// by default, or the owner.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SubteamApproval {
    #[default]
    Parent,
    Owner,
}

/// The team and task a sub-team was delegated from (section 9.1).
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct Parent {
    pub team: String,
    pub task: u32,
    /// The parent team's lead, who approves this sub-team's plan by default.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub lead: String,
    /// The assignee whose session became the sub-team's lead.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub member: String,
}

/// A member's session stopped by a usage limit (H1); `resets_at` is present
/// only when the session reports a reset time.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct MemberLimit {
    pub at: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<f64>,
    #[serde(default)]
    pub detail: String,
}

/// What a swapped member's next attempt needs to continue the work (H6): the
/// previous attempt, when it began, its partial report and its open questions.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct Continuation {
    pub attempt: String,
    pub n: u32,
    pub since: f64,
    #[serde(default)]
    pub report: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub questions: Vec<String>,
}

/// A member's pending native question as Zerus read it (C2): the lead answers
/// it with `hgs team answer` by this exact id and hash, as before, but now the
/// inbox lists it with the rest instead of a message of its own.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct PendingQuestion {
    pub question_id: String,
    pub question_hash: String,
    #[serde(default)]
    pub approval: bool,
    #[serde(default)]
    pub at: f64,
    /// The card the answer command is built from: header, question and options.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub card: serde_json::Value,
}

/// The lead's last inbox message (C2): when it went and which items it named,
/// so one coalesced message replaces the per-item messages and a reminder can
/// still reach the lead later.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct Inbox {
    #[serde(default)]
    pub told_at: f64,
    /// Item keys already named, oldest first, bounded.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub told: Vec<String>,
}

impl Inbox {
    /// An empty record adds no key to an old team's JSON.
    pub(crate) fn is_empty(&self) -> bool {
        self.told_at == 0.0 && self.told.is_empty()
    }
}

/// The Background job a member's time budget is tied to (C12): its command and
/// when it started.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct JobRef {
    #[serde(default)]
    pub command: String,
    pub since: f64,
}

/// A member's declared time budget for its newest Background job (C12). The
/// budget never stops anything; it only says when the work is overdue.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct Budget {
    pub label: String,
    pub minutes: u32,
    pub red_minutes: u32,
    pub declared_at: f64,
    /// Filled when the dispatcher ties the budget to live Background work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job: Option<JobRef>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub amber_told: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub red_told: bool,
}

impl Budget {
    /// How long the job has run, from its start or from the declaration.
    pub(crate) fn elapsed(&self, now: f64) -> f64 {
        now - self.job.as_ref().map(|job| job.since).unwrap_or(self.declared_at)
    }

    /// `ok`, `amber` (past the estimate) or `red` (past the red time) (C12).
    pub(crate) fn state(&self, now: f64) -> &'static str {
        let elapsed = self.elapsed(now);
        if elapsed >= self.red_minutes as f64 * 60.0 {
            "red"
        } else if elapsed >= self.minutes as f64 * 60.0 {
            "amber"
        } else {
            "ok"
        }
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

/// A task of the plan's work, or a check that runs a command in the member's
/// own folder and is decided by its exit code (C5).
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TaskKind {
    #[default]
    Task,
    Check,
}

impl TaskKind {
    /// An ordinary task keeps the old JSON shape: `kind` is skipped.
    pub(crate) fn is_task(&self) -> bool {
        matches!(self, Self::Task)
    }
}

/// The command a check task runs, its expected exit code and its time limit.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct Check {
    pub command: String,
    #[serde(default)]
    pub expect_exit: i32,
    #[serde(default = "default_check_timeout")]
    pub timeout_minutes: u32,
}

fn default_check_timeout() -> u32 {
    30
}

/// What the member's check ran and printed: the last lines go to the report
/// and the decision is the exit code (C5).
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct CheckResult {
    pub exit: i32,
    #[serde(default)]
    pub tail: String,
    pub at: f64,
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

/// D2: a finding a report filed, kept until it is fixed or dropped.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct Finding {
    /// f1, f2, ... in filing order.
    pub id: String,
    pub from_task: u32,
    pub severity: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    pub scenario: String,
    /// The task that caused it, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<u32>,
    pub state: String,
    pub by: String,
    pub at: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    /// N17: the owner's replan proposal, kept with the finding it answers.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub proposal: String,
    /// N18: the draft phase a `fix_phase` finding is marked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<u32>,
}

impl Finding {
    /// N18: still to be dealt with - open, reopened, urgent, or waiting in a
    /// replan or a draft phase. These block a finish when critical and count
    /// as open in the summary.
    pub(crate) fn unresolved(&self) -> bool {
        matches!(self.state.as_str(), "open" | "reopened" | "urgent" | "replan" | "fix_phase")
    }

    /// N14: fixed and dropped findings are closed for good.
    pub(crate) fn closed(&self) -> bool {
        matches!(self.state.as_str(), "fixed" | "dropped")
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct Report {
    pub text: String,
    pub at: f64,
    /// Zerus took the member's last reply as the report (A2); the member may
    /// replace it while the task is undecided (A6).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub captured: bool,
    /// Captured texts this report replaced, the newest three (A6).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub replaced: Vec<ReplacedReport>,
    /// D1: the structured report object, validated and bounded; None for an
    /// unstructured report.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured: Option<serde_json::Value>,
    /// D1: a report without a structured object, shown as such in status.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unstructured: bool,
}

/// A report text that a member's real report replaced (A6).
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct ReplacedReport {
    pub text: String,
    pub at: f64,
}

/// A member's question to the lead about its open attempt (A3): one message
/// per question, answered once by the lead or the owner.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct Question {
    pub id: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub at: f64,
    /// Empty while the question waits for an answer.
    #[serde(default)]
    pub answer: String,
    #[serde(default)]
    pub answered_at: Option<f64>,
    /// The lead or you, who answered.
    #[serde(default)]
    pub by: String,
    /// The answer's message to the member: Pending until it reaches it (A3).
    #[serde(default, skip_serializing_if = "Notice::is_none")]
    pub notice: Notice,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notice_id: String,
    /// Section 9.6: the parent question this one was escalated as; the
    /// parent's answer is copied back here.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub forwarded: String,
}

impl Question {
    /// Still waiting for an answer. A question closed with its attempt or a
    /// finished task has `answered_at` and no answer.
    pub(crate) fn open(&self) -> bool {
        self.answer.is_empty() && self.answered_at.is_none()
    }
}

/// A task kept back from the dispatcher until the lead releases it (A5): a
/// rejection may return a task to a member who is not ready for it.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct Hold {
    /// The lead or you.
    pub by: String,
    pub at: f64,
    #[serde(default)]
    pub reason: String,
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
    /// The first dispatcher tick after delivery that showed the member working
    /// on it (A1); absent until it really started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<f64>,
    /// The lead was told once that the member did not start in time (A1).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub start_warned: bool,
    /// When the member's turn after the hand-off ended, for No report.
    #[serde(default)]
    pub turn_ended_at: Option<f64>,
    /// The first tick that saw the capture gate open: the report timeout counts
    /// from here, not from the reply (C2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_ready_at: Option<f64>,
    /// The first tick that saw the finished turn blocked by an open question or
    /// by Background work; the lead is told once it lasts too long (N5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_since: Option<f64>,
    /// The member's session reply id at `turn_ended_at`: a newer turn is never
    /// captured as this attempt's report (A2).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub turn_reply: String,
    /// The member's turn ended long enough ago without a report.
    #[serde(default)]
    pub no_report: bool,
    /// The lead was reminded once that this report still waits for a decision
    /// (C4); absent until it was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reminded_at: Option<f64>,
    /// The check this attempt ran, when its task is a check task (C5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<CheckResult>,
    #[serde(default)]
    pub report: Option<Report>,
    /// Questions this attempt asked the lead, oldest first (A3).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub questions: Vec<Question>,
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
    /// Commits Zerus made for this attempt (B4), newest last, at most 20.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commits: Vec<CommitRef>,
    /// U21: the built-in command templates Zerus ran for this attempt, newest
    /// last, at most 20.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runs: Vec<RunRecord>,
}

impl Attempt {
    /// The attempt's first question still waiting for an answer (A3).
    pub(crate) fn open_question(&self) -> Option<&Question> {
        self.questions.iter().find(|question| question.open())
    }
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
    /// A plan task, or a check that runs a command (C5).
    #[serde(default, skip_serializing_if = "TaskKind::is_task")]
    pub kind: TaskKind,
    /// The check a check task runs; absent on a plan task.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<Check>,
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
    /// Held back from the dispatcher until the lead releases it (A5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hold: Option<Hold>,
    #[serde(default)]
    pub attempts: Vec<Attempt>,
    #[serde(default)]
    pub decided_by: Option<Decider>,
    #[serde(default)]
    pub decision_reason: String,
    /// D2: what a reopen replaced, newest last (an accepted task's decision).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<Value>,
    /// D2: the finding text a reopened task hands out with its next attempt.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reopen_note: String,
    /// H6: the hand-out after a swap carries the previous attempt's partial
    /// report and open questions; cleared once the new attempt is delivered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation: Option<Continuation>,
    /// Section 9.4: the sub-team delegated from this task; its status is
    /// derived for `hgs team status`, not stored here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subteam: Option<String>,
}

impl Task {
    pub(crate) fn open_attempt(&self) -> Option<&Attempt> {
        self.attempts.iter().rev().find(|attempt| attempt.state.open())
    }
    pub(crate) fn open_attempt_mut(&mut self) -> Option<&mut Attempt> {
        self.attempts.iter_mut().rev().find(|attempt| attempt.state.open())
    }
    /// A check task is decided by the exit code of its command (C5).
    pub(crate) fn is_check(&self) -> bool {
        self.kind == TaskKind::Check
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
    /// U25: the owner sent the draft back with a comment; the lead reworks it
    /// into a draft again. Its tasks are not handed out.
    Rejected,
    /// U25: accepted while an earlier phase still runs; it starts (becomes
    /// approved) once every earlier phase is finished.
    Queued,
}

impl PhaseState {
    /// A draft or a draft sent back: not accepted yet, still editable by the
    /// lead, numbered with the drafts.
    pub(crate) fn is_draft(self) -> bool {
        matches!(self, Self::Draft | Self::Rejected)
    }
    /// Accepted: running, finished or queued behind an earlier phase.
    pub(crate) fn is_accepted(self) -> bool {
        matches!(self, Self::Approved | Self::Queued)
    }
}

/// U25: the owner's send-back of a draft phase.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct PhaseRejection {
    pub comment: String,
    #[serde(default)]
    pub by: String,
    #[serde(default)]
    pub at: f64,
    /// When the lead reworked the phase back into a draft.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reworked_at: Option<f64>,
}

/// One phase appended to the plan after the approved one. Phase 1 is implicit
/// and has no record.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct Phase {
    pub n: u32,
    /// The display and approval order (F10): drafts may be moved before each
    /// other. 0 on a team stored before the field means n order.
    #[serde(default)]
    pub order: u32,
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
    /// The owner's sentence when the lead approved this phase on its word (F13).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_word: Option<OwnerWord>,
    /// The lead's edits to this draft since it was added, newest last, for the
    /// owner's diff view (F10): {"kind":"added|removed|changed|moved", ...}.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changes: Vec<serde_json::Value>,
    /// D2: an urgent phase runs at once (no approval under review, no barrier)
    /// and its tasks are handed out before other pending ones.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub urgent: bool,
    /// Section 9.5: why a "Rework" draft was opened after the parent lead
    /// rejected the sub-team's report.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
    /// U25: the note shown under the phase header, such as "Reworked after
    /// your comment: ...".
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    /// U25: the owner's latest send-back; kept after the rework for the note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rejection: Option<PhaseRejection>,
}

impl Phase {
    /// The display and approval order; a team stored before the field keeps
    /// its phase numbers as the order (F10).
    pub(crate) fn order(&self) -> u32 {
        if self.order == 0 {
            self.n
        } else {
            self.order
        }
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
    /// Phases added by `extend`; the approved initial plan (phase 1) has none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub phases: Vec<Phase>,
    /// Permissions a plan widens beyond the role's default (B5): the owner
    /// confirms them before the plan can be approved.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profile_widening: Vec<serde_json::Value>,
    /// D2: findings filed by accepted reports, oldest first (at most 1000).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<Finding>,
    /// D2: the last finding number given; ids stay unique for the team's life.
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub findings_seq: u64,
    /// F5: the editing state while the team is a draft.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft: Option<Draft>,
    /// F7: the team this one continues, and the team that continues it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub previous: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub next: String,
    /// N7: why a stopped team stopped ("draft_discarded"), empty otherwise.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub stop_reason: String,
    /// F8: membership and next-goal proposals the lead made.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub proposals: Vec<serde_json::Value>,
    #[serde(default)]
    pub revision: u64,
    /// F11: the revision that last changed draft phase or task numbers; a
    /// caller holding an older revision is refused.
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub phase_numbers_revision: u64,
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
    /// The lead's coalesced inbox message (C2).
    #[serde(default, skip_serializing_if = "Inbox::is_empty")]
    pub inbox: Inbox,
    /// Who approved the plan: "You", or "Lead" on the owner's word (F13).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub approved_by: String,
    /// The owner's sentence the lead approved the plan on (F13).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_word: Option<OwnerWord>,
    /// Who finished the team and with what summary; set by `finish`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish: Option<Finish>,
    /// The template's lead protocol when the team was created, given to the
    /// lead with the planning brief.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub lead_protocol: String,
    /// H6: the sessions members were swapped away from, oldest first, at most
    /// 20: {member, old, at, by}. The old session keeps running for the owner.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub swapped: Vec<serde_json::Value>,
    /// Section 9.1: the parent team and task this sub-team was delegated
    /// from; absent on a master (or any flat) team.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<Parent>,
    /// Section 9.1: the sub-teams delegated from this team, at most 32.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<String>,
    /// U26: verified owner events already spent on a change, so one sentence
    /// authorizes one change: [{event, change}].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub used_owner_words: Vec<serde_json::Value>,
}

impl Team {
    /// U26: the change an already-spent owner word paid for, if any.
    pub(crate) fn owner_word_used(&self, event: i64) -> Option<String> {
        self.used_owner_words
            .iter()
            .find(|used| used["event"].as_i64() == Some(event))
            .map(|used| used["change"].as_str().unwrap_or_default().to_owned())
    }

    pub(crate) fn record_owner_word(&mut self, event: i64, change: &str) {
        if self.owner_word_used(event).is_none() {
            self.used_owner_words.push(serde_json::json!({"event":event,"change":change}));
            let excess = self.used_owner_words.len().saturating_sub(50);
            self.used_owner_words.drain(..excess);
        }
    }
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

    /// Spec 5.6: who reviews a member's work - its parent when that is a
    /// sub-lead still in the team, else the lead. The lead has no reviewer.
    pub(crate) fn reviewer_of(&self, member: &str) -> String {
        if member == self.lead {
            return String::new();
        }
        let parent = self.member(member).map(|m| m.parent.as_str()).unwrap_or_default();
        match self.member(parent) {
            Some(found) if found.sub && found.state != MemberState::Removed && found.id != member && found.id != self.lead => found.id.clone(),
            _ => self.lead.clone(),
        }
    }

    /// The members that report directly to `member` (spec 5.6), in roster
    /// order; the lead's are those without a sub-lead parent.
    pub(crate) fn reports_of(&self, member: &str) -> Vec<&Member> {
        self.members
            .iter()
            .filter(|m| m.id != self.lead && m.state != MemberState::Removed && self.reviewer_of(&m.id) == member)
            .collect()
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
        // H1/H6: the new optional fields stay out of an old team's file.
        assert!(value["settings"].get("resume_after_limits").is_none());
        assert!(value.get("swapped").is_none());
        // Section 9: a flat team has no parent, no children and the default
        // tree settings are not written.
        assert!(value.get("parent").is_none() && value.get("children").is_none());
        assert_eq!((team.settings.tree_depth, team.settings.tree_sessions), (2, 16));
        assert_eq!(team.settings.subteam_approval, SubteamApproval::Parent);
        assert!(value["settings"].get("tree_depth").is_none() && value["settings"].get("tree_sessions").is_none());
        assert!(value["settings"].get("subteam_approval").is_none());
        let back: Team = serde_json::from_value(value).unwrap();
        assert_eq!(back, team);
    }

    #[test]
    fn a_subteam_link_and_children_round_trip() {
        let mut team: Team = serde_json::from_value(old_team()).unwrap();
        team.parent = Some(Parent { team: "master".into(), task: 3, lead: "lead".into(), member: "dev".into() });
        team.children = vec!["sub1".into(), "sub2".into()];
        team.tasks[0].subteam = Some("sub1".into());
        let value = serde_json::to_value(&team).unwrap();
        assert_eq!(value["parent"]["team"], "master");
        assert_eq!(value["parent"]["task"], 3);
        assert_eq!(value["children"], json!(["sub1","sub2"]));
        assert_eq!(value["tasks"][0]["subteam"], "sub1");
        let back: Team = serde_json::from_value(value).unwrap();
        assert_eq!(back, team);
        // Settings beyond the defaults are written, and a changed approval.
        team.settings.tree_depth = 3;
        team.settings.tree_sessions = 24;
        team.settings.subteam_approval = SubteamApproval::Owner;
        let value = serde_json::to_value(&team).unwrap();
        assert_eq!(value["settings"]["tree_depth"], 3);
        assert_eq!(value["settings"]["tree_sessions"], 24);
        assert_eq!(value["settings"]["subteam_approval"], "owner");
    }

    #[test]
    fn the_limit_setting_and_member_episode_round_trip() {
        let mut team: Team = serde_json::from_value(old_team()).unwrap();
        assert!(!team.settings.resume_after_limits);
        team.settings.resume_after_limits = true;
        team.members[0].limited = Some(MemberLimit {
            at: 100.0,
            resets_at: Some(200.0),
            detail: "You've hit your session limit".into(),
        });
        team.members[0].limit_resume = true;
        let value = serde_json::to_value(&team).unwrap();
        assert_eq!(value["settings"]["resume_after_limits"], true);
        assert_eq!(value["members"][0]["limited"]["resets_at"], 200.0);
        let back: Team = serde_json::from_value(value).unwrap();
        assert!(back.settings.resume_after_limits && back.members[0].limit_resume);
        assert_eq!(back.members[0].limited.as_ref().and_then(|limit| limit.resets_at), Some(200.0));
        // An unknown reset time is optional.
        let limit: MemberLimit = serde_json::from_value(json!({"at":10.0})).unwrap();
        assert_eq!((limit.resets_at, limit.detail.as_str()), (None, ""));
        // H6: a task's continuation is optional.
        assert!(team.tasks.iter().all(|task| task.continuation.is_none()));
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

    /// An old attempt and report: no start, question, hold or capture fields.
    fn old_attempt_team() -> Value {
        json!({
            "id":"old","name":"Old team","lead":"lead","status":"running",
            "members":[{"id":"lead","role":"Lead","agent":"claude"},{"id":"dev","role":"Backend","agent":"codex"}],
            "tasks":[{"id":1,"title":"API","assignee":"dev","status":"submitted",
                "attempts":[{"n":1,"id":"00000000-0000-4000-8000-000000000001","member":"dev",
                    "state":"reported","sent_at":10.0,"delivered_at":11.0,"turn_ended_at":20.0,
                    "report":{"text":"Done","at":30.0}}]}]
        })
    }

    #[test]
    fn an_old_attempt_loads_with_the_new_fields_at_their_defaults() {
        let team: Team = serde_json::from_value(old_attempt_team()).unwrap();
        assert_eq!(team.settings.start_timeout_seconds, 180);
        // The lead's load settings (C2, C4, C12, U18) default; an old team has
        // no inbox.
        assert_eq!(
            (
                team.settings.inbox_interval_seconds,
                team.settings.remind_lead_minutes,
                team.settings.remind_owner_minutes,
                team.settings.remind_owner_permission_minutes
            ),
            (120, 15, 60, 10)
        );
        assert_eq!(team.settings.background_red_minutes, 30);
        assert!(team.inbox.told.is_empty() && team.inbox.told_at == 0.0);
        assert!(team.members.iter().all(|member| member.pending.is_none()));
        assert!(team.approved_by.is_empty() && team.owner_word.is_none());
        let attempt = &team.tasks[0].attempts[0];
        assert_eq!((attempt.started_at, attempt.start_warned), (None, false));
        assert!(attempt.reminded_at.is_none());
        assert!(attempt.check.is_none());
        assert!(attempt.turn_reply.is_empty());
        assert!(attempt.questions.is_empty());
        assert_eq!(attempt.open_question(), None);
        assert_eq!(team.tasks[0].hold, None);
        // An ordinary plan task keeps the old shape (C5: no kind, no check).
        assert_eq!(team.tasks[0].kind, TaskKind::Task);
        assert!(team.tasks[0].check.is_none());
        let report = attempt.report.as_ref().unwrap();
        assert!(!report.captured);
        assert!(report.replaced.is_empty());
        // Writing it back adds only the default timeouts to the settings; the
        // new fields stay absent so an old binary reads the file.
        let value = serde_json::to_value(&team).unwrap();
        assert_eq!(value["settings"]["start_timeout_seconds"], json!(180));
        assert_eq!(value["settings"]["background_red_minutes"], json!(30));
        assert!(value.get("inbox").is_none() && value.get("pending").is_none());
        assert!(value.get("approved_by").is_none() && value.get("owner_word").is_none());
        assert!(value["tasks"][0].get("kind").is_none() && value["tasks"][0].get("check").is_none());
        let attempt = &value["tasks"][0]["attempts"][0];
        for key in ["started_at", "start_warned", "turn_reply", "questions", "reminded_at"] {
            assert!(attempt.get(key).is_none(), "{key} should be skipped when empty");
        }
        assert!(value["members"][1].get("pending").is_none());
        assert!(value["tasks"][0].get("hold").is_none());
        let report = &attempt["report"];
        assert!(report.get("captured").is_none() && report.get("replaced").is_none());
        let back: Team = serde_json::from_value(value).unwrap();
        assert_eq!(back, team);
    }

    fn held_team() -> Team {
        Team {
            id: "held".into(),
            name: "Held".into(),
            lead: "lead".into(),
            status: TeamStatus::Running,
            settings: Settings { start_timeout_seconds: 300, ..Default::default() },
            tasks: vec![Task {
                id: 7,
                title: "API".into(),
                assignee: "dev".into(),
                status: TaskStatus::Pending,
                hold: Some(Hold { by: "Lead".into(), at: 1792940000.5, reason: "Busy with #9".into() }),
                attempts: vec![Attempt {
                    n: 1,
                    id: "00000000-0000-4000-8000-000000000007".into(),
                    member: "dev".into(),
                    state: AttemptState::Rejected,
                    sent_at: 1792939000.0,
                    delivered_at: Some(1792939001.0),
                    started_at: Some(1792939010.0),
                    start_warned: true,
                    turn_ended_at: Some(1792939500.0),
                    turn_reply: "r7".into(),
                    questions: vec![
                        Question { id: "q1".into(), text: "Keep the old field?".into(), at: 1792939100.0, answer: "Yes".into(), answered_at: Some(1792939200.0), by: "Lead".into(), ..Default::default() },
                        Question { id: "q2".into(), text: "Which JSON?".into(), at: 1792939300.0, ..Default::default() },
                    ],
                    report: Some(Report {
                        text: "Done".into(),
                        at: 1792939600.0,
                        captured: true,
                        replaced: vec![ReplacedReport { text: "Building and running...".into(), at: 1792939550.0 }],
                        ..Default::default()
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn the_attempt_and_hold_fields_round_trip() {
        let team = held_team();
        let text = serde_json::to_string(&team).unwrap();
        let back: Team = serde_json::from_str(&text).unwrap();
        assert_eq!(back, team);
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["settings"]["start_timeout_seconds"], json!(300));
        let task = &value["tasks"][0];
        assert_eq!(task["hold"], json!({"by":"Lead","at":1792940000.5,"reason":"Busy with #9"}));
        let attempt = &task["attempts"][0];
        assert_eq!(attempt["started_at"], json!(1792939010.0));
        assert_eq!(attempt["start_warned"], json!(true));
        assert_eq!(attempt["turn_reply"], json!("r7"));
        // Every field of a question has its own name, answers stay visible.
        assert_eq!(attempt["questions"][0], json!({
            "id":"q1","text":"Keep the old field?","at":1792939100.0,"answer":"Yes",
            "answered_at":1792939200.0,"by":"Lead"}));
        assert_eq!(attempt["questions"][1]["answered_at"], Value::Null);
        assert_eq!(attempt["questions"][1]["answer"], "");
        let report = &attempt["report"];
        assert_eq!(report["captured"], json!(true));
        assert_eq!(report["replaced"], json!([{"text":"Building and running...","at":1792939550.0}]));
        // The open question is the unanswered one.
        let attempt = &back.tasks[0].attempts[0];
        assert_eq!(attempt.open_question().map(|q| q.id.as_str()), Some("q2"));
        assert!(!attempt.questions[0].open());
    }
}
