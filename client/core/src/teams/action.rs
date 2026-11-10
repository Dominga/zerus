//! The team actions the Qt Teams UI offers, as guarded `hgs [@host] team <op>
//! --json` calls (task #7).
//!
//! Every call carries a request identity (`request_id`, a canonical UUID) and
//! `"by": "human"`, as Qt's `TeamController::act` sends them. The team store
//! keeps the receipt of each request id, so a call sent again with the same id
//! answers its stored receipt instead of applying the change twice.
//!
//! [`classify`] turns the call's exit into a [`TeamOutcome`]:
//!
//! - `done`: hgs took the change and gave a receipt;
//! - `failed`: hgs refused it, or never ran; nothing was applied, and a new
//!   attempt may get a new request id;
//! - `uncertain`: the call timed out, ended abnormally or gave no valid
//!   receipt, so the change may have been applied. It is never sent again by
//!   itself: the view reads the team first, and only the owner may send it
//!   again, with the same request id ([`TeamCall::request_id`]), which the
//!   store answers with its receipt if it had applied it;
//! - `confirm`: a destructive action (stop, archive, cancel, take over,
//!   discard a draft) or an approval was asked for without `confirmed`;
//!   nothing ran, and the confirmation names what the owner is asked.
use crate::actions::{Exit, Invocation};
use crate::fleet::valid_alias;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::path::Path;
use std::time::Duration;

/// The longest reason, comment or reply text an action sends.
pub const TEXT_CHARS: usize = 20_000;
/// The longest stderr a failure keeps.
const DETAIL_CHARS: usize = 2000;

/// The task a new sub-team comes from (`create`'s parent).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Parent {
    pub team: String,
    pub task: u64,
}

/// One team action, as the webview asks for it (`{"kind": "...", ...}`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TeamAction {
    /// `approve` (the plan; `plan_mode` and `confirm_widening` as Qt's
    /// approval adds them) or, with `phase`, `approve-phase` (`alongside`: hand
    /// its tasks out while an earlier phase runs).
    Approve {
        #[serde(default)]
        phase: Option<u64>,
        #[serde(default)]
        plan_mode: Option<String>,
        #[serde(default)]
        confirm_widening: bool,
        #[serde(default)]
        alongside: bool,
    },
    /// Accepts a task's report; `manual` marks it done without one.
    Accept {
        task: u64,
        #[serde(default)]
        reason: String,
        #[serde(default)]
        manual: bool,
    },
    /// Rejects a task's report with the comment the member gets; `hold` keeps
    /// the task for a later release, `fresh` hands it to a free peer.
    Reject {
        task: u64,
        reason: String,
        #[serde(default)]
        hold: bool,
        #[serde(default)]
        fresh: bool,
    },
    /// Answers a member's question to the lead (`hgs team reply`).
    Reply {
        question: String,
        text: String,
    },
    /// Gives a task to another member.
    Reassign {
        task: u64,
        member: String,
        #[serde(default)]
        subteam: Option<String>,
    },
    /// The owner does the task: its attempt is revoked.
    TakeOver {
        task: u64,
    },
    /// Cancels a task.
    Cancel {
        task: u64,
        #[serde(default)]
        subteam: Option<String>,
    },
    Pause,
    Resume,
    /// Cancels every open task and hands nothing out again.
    Stop,
    /// Hides a done or stopped team, keeping its history.
    Archive,
    /// Answers the lead's proposal card with its content hash; a question's
    /// `answer` is an offered option or the owner's own words.
    DecideProposal {
        proposal: String,
        accept: bool,
        expected_hash: String,
        #[serde(default)]
        reason: String,
        #[serde(default)]
        answer: String,
    },
    /// Takes the member's last reply as the task's report.
    UseLastReply {
        task: u64,
        #[serde(default)]
        attempt: String,
    },
    /// Task 11: answers a member's pending permission request or question
    /// (`hgs team answer`) with its question id and hash and the session
    /// card's answers (`[{question_id, selected_option_ids, text}]`).
    Answer {
        member: String,
        question_id: String,
        expected_question_hash: String,
        answers: Value,
    },
    /// Task 11: confirms (`confirmation`, the typed word if any) or declines
    /// the addition `confirm` waiting for a running phase (`add-to-phase`).
    /// Task 196: with `tasks` (and no `confirm`) it is the request Qt's
    /// "Add task..." sends first; hgs answers with a warning and the pending
    /// addition the confirmation names.
    AddToPhase {
        phase: u64,
        #[serde(default)]
        confirm: String,
        #[serde(default)]
        confirmation: String,
        #[serde(default)]
        decline: bool,
        #[serde(default)]
        tasks: Vec<Value>,
    },
    /// Task 196: sends a draft phase back to the lead with what should change
    /// (`reject-phase`, against the revision it was read at).
    RejectPhase {
        phase: u64,
        comment: String,
        #[serde(default)]
        revision: Option<u64>,
    },
    /// Task 196: takes a send-back back; the draft waits for the owner again.
    UndoRejectPhase {
        phase: u64,
    },
    /// Task 196: discards a draft, or an accepted phase whose work has not
    /// started (`accepted` and `tasks` only word the confirmation).
    DiscardPhase {
        phase: u64,
        #[serde(default)]
        tasks: u64,
        #[serde(default)]
        accepted: bool,
    },
    /// Task 196: an accepted phase whose work has not started goes back to a
    /// draft that waits for the owner's approval again.
    ReturnToDraft {
        phase: u64,
    },
    /// Task 196: moves a phase before or after another (Move earlier/later).
    MovePhase {
        phase: u64,
        to: u64,
        position: String,
    },
    /// Task 196: pauses an accepted phase until the pause is removed; the
    /// optional reason is the lead's to read.
    PausePhase {
        phase: u64,
        #[serde(default)]
        reason: String,
    },
    /// Task 196: removes a phase's pause.
    UnpausePhase {
        phase: u64,
    },
    /// Task 196: whether a phase waits for the owner's acceptance once its
    /// tasks are decided ("Requires owner acceptance").
    PhaseAcceptance {
        phase: u64,
        owner: bool,
    },
    /// Task 196: edits a draft phase, or an accepted one that has not
    /// started: its title and its tasks' fields (`edit-draft`), against the
    /// revision it was read at.
    EditDraft {
        phase: u64,
        #[serde(default)]
        revision: Option<u64>,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        barrier: Option<bool>,
        #[serde(default)]
        tasks_update: Vec<Value>,
    },
    /// Task 196: an edit of the plan while the team plans (`plan`): core reads
    /// the whole team (`team status`), applies the change and sends the whole
    /// plan once at the revision the owner saw (super::plan_edit), never from
    /// the view's bounded summary.
    PlanEdit {
        revision: u64,
        change: super::plan_edit::PlanChange,
    },
    /// Task 11: accepts a phase that waits for the owner's acceptance.
    AcceptPhase {
        phase: u64,
    },
    /// Task #58: switches one phase between free and standard planning
    /// (`hgs team phase-mode`); the owner confirms it first.
    PhaseMode {
        phase: u64,
        mode: String,
        #[serde(default)]
        reason: String,
    },
    /// Task #12: the team's own plan mode, the default of every phase without
    /// a mode of its own (`hgs team phase-mode` without a phase). While the
    /// team plans it is the mode its approval starts in; while it runs, the
    /// phases that follow the team switch with it.
    TeamMode {
        mode: String,
        #[serde(default)]
        reason: String,
    },
    /// Task 11: sends a phase back to the lead with the owner's comment.
    SendBackPhase {
        phase: u64,
        comment: String,
    },
    /// Task 11: the owner's answer to a missing tool's offer: Allow installs
    /// it with the command shown; Deny (with a reason) is remembered.
    DecideTool {
        offer: String,
        allow: bool,
        #[serde(default)]
        reason: String,
    },
    /// Task 251: `hgs team propose --json` from the lead's tool_missing item:
    /// ask the owner to install the offered tool (`kind` is install-tool).
    Propose {
        proposal_kind: String,
        reason: String,
        #[serde(default)]
        offer: String,
    },
    /// Task 97: the owner's answer to a request the owner queue deferred:
    /// `allow` or `deny` a permission, or `answer` with the owner's text. The
    /// session gets one message; the tool is never replayed.
    AnswerDeferred {
        id: String,
        decision: String,
        #[serde(default)]
        text: String,
    },
    /// Task 108: the build folders Zerus holds on this computer: `list` (the
    /// Disk view), `clean` (a dry run, or Clean now after a confirmation),
    /// `policy` (the owner's cleanup policy) or `forget` (a folder the owner
    /// reviewed). No team's.
    Disk {
        action: String,
        #[serde(default)]
        scope: String,
        #[serde(default)]
        dry_run: bool,
        #[serde(default)]
        policy: Value,
        #[serde(default)]
        paths: Vec<String>,
        #[serde(default)]
        path: String,
        #[serde(default)]
        refresh: bool,
        /// Task 109: the open Disk view's live read: measures a few folders.
        #[serde(default)]
        measure: bool,
    },
    /// Task 88: the team's test configuration for its Tests settings:
    /// `get` reads it, `check` checks a candidate override (`tests`, an
    /// object shaped like .zerus/tests.toml, or null); neither writes. The
    /// owner's Save is `settings` with `tests`.
    TestConfig {
        action: String,
        #[serde(default)]
        tests: Value,
    },
    /// Task 11: releases a task the owner held.
    Release {
        task: u64,
    },
    /// Task 11: retries a task's step Zerus could not take.
    Retry {
        task: u64,
    },
    /// Task 11/40: finishes a team whose tasks are all decided, with the
    /// owner's summary for the lead (hgs requires one, at most 4000 chars).
    Finish {
        #[serde(default)]
        summary: String,
    },
    /// Task 40: drops a plan in Planning (Discard plan); the team stops.
    Discard,
    /// D2: the owner's answer to one finding (`hgs team finding`): Fixed,
    /// Dropped, Defer, Reopen, Urgent phase or Replan (with the proposal).
    Finding {
        finding: String,
        action: String,
        #[serde(default)]
        note: Option<String>,
        #[serde(default)]
        proposal: Option<String>,
        #[serde(default)]
        task: Option<u64>,
    },
    /// F7: a Done team starts the next goal with the same people.
    NextGoal {
        goal: String,
        #[serde(default)]
        name: Option<String>,
    },
    /// G1: creates a sub-team from a task; its lead is the assignee.
    Create {
        name: String,
        parent: Parent,
    },
    /// Task #308: proposes a working mode; the owner allows it on its card.
    ProposeWorkingMode {
        preset: String,
        #[serde(default)]
        reason: String,
    },
    /// Task #337: the owner keeps the team's settings for the project's current
    /// profile, so the working-mode block stays away until the profile changes.
    KeepWorkingMode,
    /// Task #230: the owner marks the lead's plan changes seen, or reverts one.
    SeenLeadChanges,
    RevertLeadChange {
        change: String,
    },
    /// Task 304 (hgs task #246): the owner dismisses a Waiting row in its
    /// current state; it returns when its state changes or on Restore.
    WaitingDismiss {
        id: String,
        state: String,
        #[serde(default)]
        team: String,
    },
    /// Task 304: Show dismissed > Restore.
    WaitingRestore {
        id: String,
    },
    /// Task 40: parks a pending task with its reason until it is released.
    Hold {
        task: u64,
        reason: String,
    },
    /// Task 40: ends a task's open pipeline stage (skip-stage) with a reason.
    SkipStage {
        task: u64,
        reason: String,
    },
    /// Task 40: sends a task in a pipeline back to its implementer.
    SendBack {
        task: u64,
        reason: String,
    },
    /// Task 40: moves a task that has not started to another phase, against
    /// the plan revision it was read at.
    MoveTask {
        task: u64,
        phase: u64,
        #[serde(default)]
        revision: Option<u64>,
    },
    /// Task #226: a task's lane in a planning or draft phase (`edit-draft`'s
    /// `tasks_update`); a null lane takes it out (lane 0).
    EditTasks {
        phase: u64,
        tasks: Vec<Value>,
    },
    /// Starts a draft team (no team id yet): its folder, name, lead and
    /// template as New team's Start planning sends them.
    DraftStart {
        folder: String,
        #[serde(default)]
        name: String,
        lead: Value,
        #[serde(default)]
        project: String,
        #[serde(default)]
        project_id: String,
        #[serde(default)]
        template: String,
    },
    /// The owner's edit of a draft against the revision it read.
    DraftUpdate {
        revision: u64,
        set: Value,
    },
    /// Marks fields the lead changed as seen.
    DraftSeen {
        fields: Vec<String>,
    },
    /// Creates the team from the draft.
    DraftCreate {
        #[serde(default)]
        approve: bool,
        #[serde(default)]
        confirm_widening: bool,
    },
    /// Discards the draft; `stop_lead` stops the lead's session too.
    DraftDiscard {
        #[serde(default)]
        stop_lead: bool,
        #[serde(default)]
        reason: String,
    },
    /// The owner's settings patch, merged by `hgs team settings`: only the
    /// fields that differ from the team's current settings, plus any member
    /// updates (their iron rules) sent in the same call.
    Settings {
        settings: Value,
        #[serde(default)]
        members_update: Vec<Value>,
    },
    /// Task 199: the member menu (Qt TeamWorkspace::memberMenu): pause or
    /// resume a member, relaunch one whose launch failed, drain it (or cancel
    /// the drain), remove it (asked first), who it reports to, sub-lead on
    /// or off, more members of its role, a swap, and the account move of
    /// every member of an agent (U54); "+ Member" adds one.
    PauseMember {
        member: String,
    },
    ResumeMember {
        member: String,
    },
    Relaunch {
        member: String,
    },
    Drain {
        member: String,
        #[serde(default)]
        role: String,
        #[serde(default)]
        cancel: bool,
    },
    RemoveMember {
        member: String,
        #[serde(default)]
        role: String,
    },
    ReportsTo {
        member: String,
        parent: String,
    },
    SubLead {
        member: String,
        sub: bool,
    },
    Scale {
        role: String,
        agent: String,
        count: u64,
        #[serde(default)]
        model: String,
        #[serde(default)]
        effort: String,
        #[serde(default)]
        account: String,
    },
    SwapMember {
        member: String,
        #[serde(default)]
        agent: String,
        #[serde(default)]
        model: String,
        #[serde(default)]
        effort: String,
        #[serde(default)]
        account: String,
        #[serde(default)]
        fresh: bool,
    },
    MoveAccount {
        provider: String,
        from_account: String,
        to_account: String,
    },
    AddMember {
        member: Value,
    },
    /// Task 199 (Qt TeamTests force): the suite goes to the test queue as
    /// forced: first at the worker's next tick, never a reused result.
    ForceTest {
        suite: String,
    },
    /// Task 199 (U51): a new intake key named `label`; hgs answers its
    /// secret once in the receipt (`key`). `revoke` removes one.
    IntakeKey {
        #[serde(default)]
        label: String,
        #[serde(default)]
        revoke: String,
    },
    /// The designer's change, left for the owner to keep or reject.
    CatalogPropose {
        change: Value,
    },
    /// The owner's decision on one catalog proposal: keep, reject or undo.
    CatalogDecide {
        proposal: String,
        decision: String,
    },
    /// Writes the kept proposals as the next catalog version.
    CatalogApply,
    /// Ends the catalog draft, undecided changes included.
    CatalogDiscard,
    /// Task #44: Save catalog: the edited catalog as the next version, over
    /// the version it was read at (`hgs team catalog set`), as Qt's editor
    /// saves it (`super::catalog_edit`).
    CatalogSet { version: u64, catalog: Value },
    /// Task #44: starts the Team designer's conversation for a project
    /// (`hgs team catalog designer`), with the owner's first message as its
    /// goal and the agent Qt's page offers.
    CatalogDesigner {
        folder: String,
        goal: String,
        #[serde(default)]
        agent: String,
        #[serde(default)]
        model: String,
        #[serde(default)]
        effort: String,
        #[serde(default)]
        project: String,
    },
}

/// What the owner is asked before a destructive action or an approval.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Confirmation {
    /// Qt's dialog name, for parity checks.
    pub name: String,
    pub title: String,
    pub text: String,
    /// The confirming button's text.
    pub action: String,
}

/// One call from the view: the computer ("" is this one), the team (empty
/// only for a draft start), its name for the confirmation text, the action,
/// whether the owner confirmed it, and the request id of an earlier attempt
/// the owner sends again (None: a new request).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TeamCall {
    #[serde(default)]
    pub computer: String,
    #[serde(default)]
    pub team: String,
    #[serde(default)]
    pub team_name: String,
    pub action: TeamAction,
    #[serde(default)]
    pub confirmed: bool,
    #[serde(default)]
    pub request_id: Option<String>,
}

/// How a call ended for the view.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum TeamOutcome {
    /// hgs applied it; `receipt` is its answer.
    Done { receipt: Value, request_id: String },
    /// Nothing was applied: hgs refused it (its words) or the call never ran.
    Failed { detail: String, request_id: String },
    /// It may have been applied. Never sent again by itself: read the team,
    /// then the owner may send it again with this `request_id`.
    Uncertain { detail: String, request_id: String },
    /// Not sent: the owner confirms first.
    Confirm { confirmation: Confirmation },
}

impl TeamOutcome {
    pub fn request_id(&self) -> Option<&str> {
        match self {
            Self::Done { request_id, .. }
            | Self::Failed { request_id, .. }
            | Self::Uncertain { request_id, .. } => Some(request_id),
            Self::Confirm { .. } => None,
        }
    }
}

/// A canonical UUID as the team store requires a request id: 8-4-4-4-12
/// lowercase hexadecimal digits.
pub fn canonical_uuid(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => byte == b'-',
            _ => byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte),
        })
}

/// A new request identity.
pub fn new_request_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn plain(text: &str, what: &str, chars: usize) -> Result<(), String> {
    if text.chars().count() > chars {
        return Err(format!("{what} is too long."));
    }
    if text
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err(format!("{what} contains control characters."));
    }
    Ok(())
}

fn word(text: &str, what: &str) -> Result<(), String> {
    if text.is_empty()
        || text.len() > 128
        || text.starts_with('-')
        || text.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return Err(format!("Not a {what}."));
    }
    Ok(())
}

fn phase_number(phase: u64) -> Result<(), String> {
    if phase == 0 || phase > u64::from(u32::MAX) {
        return Err("Not a phase number.".into());
    }
    Ok(())
}

/// Task 196: the tasks a request adds to a running phase: objects with a
/// title, at most 20 (hgs's own bound for one addition).
fn new_tasks(tasks: &[Value]) -> Result<(), String> {
    if tasks.len() > 20 {
        return Err("At most 20 tasks join a running phase at once.".into());
    }
    for task in tasks {
        let title = task.get("title").and_then(Value::as_str).unwrap_or_default();
        if title.trim().is_empty() {
            return Err("A new task needs a title.".into());
        }
        plain(title, "A task title", 200)?;
    }
    Ok(())
}

fn task(task: u64) -> Result<(), String> {
    if task == 0 || task > u64::from(u32::MAX) {
        return Err("Not a task number.".into());
    }
    Ok(())
}

impl TeamAction {
    /// The operation: `hgs team <op> --json`.
    pub fn op(&self) -> &'static str {
        match self {
            Self::Approve { phase: None, .. } => "approve",
            Self::Approve { phase: Some(_), .. } => "approve-phase",
            Self::Accept { .. } => "accept",
            Self::Reject { .. } => "reject",
            Self::Reply { .. } => "reply",
            Self::Reassign { .. } => "reassign",
            Self::TakeOver { .. } => "take-over",
            Self::Cancel { .. } => "cancel",
            Self::Pause => "pause",
            Self::Resume => "resume",
            Self::Stop => "stop",
            Self::Archive => "archive",
            Self::DecideProposal { .. } => "decide-proposal",
            Self::UseLastReply { .. } => "use-last-reply",
            Self::Answer { .. } => "answer",
            Self::AddToPhase { .. } => "add-to-phase",
            Self::RejectPhase { .. } => "reject-phase",
            Self::UndoRejectPhase { .. } => "undo-reject-phase",
            Self::DiscardPhase { .. } => "discard-phase",
            Self::ReturnToDraft { .. } => "return-to-draft",
            Self::MovePhase { .. } => "move-phase",
            Self::PausePhase { .. } => "pause-phase",
            Self::UnpausePhase { .. } => "unpause-phase",
            Self::PhaseAcceptance { .. } => "phase-acceptance",
            Self::EditDraft { .. } => "edit-draft",
            Self::PlanEdit { .. } => "plan",
            Self::AcceptPhase { .. } => "accept-phase",
            Self::PhaseMode { .. } | Self::TeamMode { .. } => "phase-mode",
            Self::SendBackPhase { .. } => "send-back-phase",
            Self::DecideTool { .. } => "decide-tool",
            Self::Propose { .. } => "propose",
            Self::AnswerDeferred { .. } => "answer-deferred",
            Self::Disk { .. } => "disk",
            Self::TestConfig { .. } => "test-config",
            Self::Release { .. } => "release",
            Self::Retry { .. } => "retry",
            Self::Finish { .. } => "finish",
            Self::Discard => "discard",
            Self::Finding { .. } => "finding",
            Self::NextGoal { .. } => "next-goal",
            Self::Create { .. } => "create",
            Self::ProposeWorkingMode { .. } => "propose",
            Self::KeepWorkingMode => "keep-working-mode",
            Self::SeenLeadChanges => "seen-lead-changes",
            Self::RevertLeadChange { .. } => "revert-lead-change",
            Self::WaitingDismiss { .. } | Self::WaitingRestore { .. } => "waiting",
            Self::Hold { .. } => "hold",
            Self::SkipStage { .. } => "skip-stage",
            Self::SendBack { .. } => "send-back",
            Self::MoveTask { .. } => "move-task",
            Self::EditTasks { .. } => "edit-draft",
            Self::DraftStart { .. }
            | Self::DraftUpdate { .. }
            | Self::DraftSeen { .. }
            | Self::DraftCreate { .. }
            | Self::DraftDiscard { .. } => "draft",
            Self::Settings { .. } => "settings",
            Self::PauseMember { .. } => "pause-member",
            Self::ResumeMember { .. } => "resume-member",
            Self::Relaunch { .. } => "relaunch",
            Self::Drain { .. } => "drain",
            Self::RemoveMember { .. } => "remove-member",
            Self::ReportsTo { .. } => "reports-to",
            Self::SubLead { .. } => "sub-lead",
            Self::Scale { .. } => "scale",
            Self::SwapMember { .. } | Self::MoveAccount { .. } => "swap-member",
            Self::AddMember { .. } => "add-member",
            Self::ForceTest { .. } => "test",
            Self::IntakeKey { .. } => "intake-key",
            Self::CatalogPropose { .. }
            | Self::CatalogDecide { .. }
            | Self::CatalogApply
            | Self::CatalogDiscard
            | Self::CatalogSet { .. }
            | Self::CatalogDesigner { .. } => "catalog",
        }
    }

    /// The `hgs team catalog <sub>` word of a catalog action.
    pub fn catalog_op(&self) -> Option<&'static str> {
        match self {
            Self::CatalogPropose { .. } => Some("propose"),
            Self::CatalogDecide { .. } => Some("decide"),
            Self::CatalogApply => Some("apply"),
            Self::CatalogDiscard => Some("discard"),
            Self::CatalogSet { .. } => Some("set"),
            Self::CatalogDesigner { .. } => Some("designer"),
            _ => None,
        }
    }

    /// Whether the call names an existing team (all but a draft start and a
    /// tool offer, which belongs to no team).
    pub fn names_team(&self) -> bool {
        !matches!(
            self,
            Self::DraftStart { .. }
                | Self::DecideTool { .. }
                | Self::AnswerDeferred { .. }
                | Self::Disk { .. }
                | Self::CatalogPropose { .. }
                | Self::CatalogDecide { .. }
                | Self::CatalogApply
                | Self::CatalogDiscard
                | Self::CatalogSet { .. }
                | Self::CatalogDesigner { .. }
                | Self::WaitingDismiss { .. }
                | Self::WaitingRestore { .. }
        )
    }

    /// Checks what the action sends before anything runs.
    pub fn check(&self) -> Result<(), String> {
        match self {
            Self::Approve {
                phase, plan_mode, ..
            } => {
                if phase.is_some_and(|phase| phase == 0) {
                    return Err("Not a phase number.".into());
                }
                if plan_mode
                    .as_deref()
                    .is_some_and(|mode| !matches!(mode, "standard" | "free"))
                {
                    return Err("A plan mode is standard or free.".into());
                }
                Ok(())
            }
            Self::Accept {
                task: id, reason, ..
            } => task(*id).and(plain(reason, "A reason", TEXT_CHARS)),
            Self::Reject {
                task: id, reason, ..
            } => {
                task(*id)?;
                if reason.trim().is_empty() {
                    return Err("A rejection needs a comment for the member.".into());
                }
                plain(reason, "A comment", TEXT_CHARS)
            }
            Self::Reply { question, text } => {
                word(question, "question id")?;
                if text.trim().is_empty() {
                    return Err("Write the answer.".into());
                }
                plain(text, "An answer", TEXT_CHARS)
            }
            Self::Reassign {
                task: id,
                member,
                subteam,
            } => {
                task(*id)?;
                word(member, "member id")?;
                subteam
                    .as_deref()
                    .map_or(Ok(()), |choice| word(choice, "sub-team choice"))
            }
            Self::TakeOver { task: id } | Self::UseLastReply { task: id, .. } => task(*id),
            Self::Cancel { task: id, subteam } => {
                task(*id)?;
                subteam
                    .as_deref()
                    .map_or(Ok(()), |choice| word(choice, "sub-team choice"))
            }
            Self::Pause | Self::Resume | Self::Stop | Self::Archive | Self::Discard => Ok(()),
            Self::Finish { summary } => {
                if summary.trim().is_empty() {
                    return Err("A summary is required.".into());
                }
                plain(summary, "A summary", 4000)
            }
            Self::Hold { task: id, reason }
            | Self::SkipStage { task: id, reason }
            | Self::SendBack { task: id, reason } => {
                task(*id)?;
                if reason.trim().is_empty() {
                    return Err("Say why.".into());
                }
                plain(reason, "A reason", TEXT_CHARS)
            }
            Self::MoveTask {
                task: id, phase, ..
            } => {
                task(*id)?;
                if *phase == 0 {
                    return Err("Not a phase number.".into());
                }
                Ok(())
            }
            Self::EditTasks { phase, tasks } => {
                if *phase == 0 {
                    return Err("Not a phase number.".into());
                }
                if tasks.is_empty() || tasks.len() > 500 {
                    return Err("An edit names 1 to 500 tasks.".into());
                }
                for value in tasks {
                    if value.get("id").and_then(Value::as_u64).is_none_or(|id| id == 0) {
                        return Err("A task id is required.".into());
                    }
                    match value.get("lane") {
                        None => return Err("A lane edit names each task's lane.".into()),
                        Some(Value::Null) => {}
                        Some(found) if found.is_object() => {
                            if found.get("n").and_then(Value::as_u64).is_none_or(|n| n == 0 || n > 999) {
                                return Err("A lane number is 1 to 999.".into());
                            }
                            let label = found.get("label").and_then(Value::as_str).unwrap_or_default();
                            if label.chars().count() > 40 {
                                return Err("A lane label is at most 40 characters.".into());
                            }
                        }
                        Some(_) => return Err("A lane is an object or null.".into()),
                    }
                }
                Ok(())
            }
            Self::Answer {
                member,
                question_id,
                expected_question_hash,
                answers,
            } => {
                word(member, "member id")?;
                word(question_id, "question id")?;
                if !expected_question_hash.is_empty() {
                    word(expected_question_hash, "question hash")?;
                }
                if !answers
                    .as_array()
                    .is_some_and(|answers| !answers.is_empty() && answers.len() <= 32)
                {
                    return Err("An answer names what was chosen.".into());
                }
                Ok(())
            }
            Self::AddToPhase {
                phase,
                confirm,
                confirmation,
                tasks,
                ..
            } => {
                if *phase == 0 {
                    return Err("Not a phase number.".into());
                }
                if !tasks.is_empty() {
                    // Task 196: the request names its tasks, never a pending one.
                    if !confirm.is_empty() {
                        return Err("A request to add tasks names no pending addition.".into());
                    }
                    return new_tasks(tasks);
                }
                word(confirm, "pending addition")?;
                plain(confirmation, "A confirmation", 200)
            }
            Self::RejectPhase { phase, comment, .. } => {
                phase_number(*phase)?;
                if comment.trim().is_empty() {
                    return Err("Say what the lead should change.".into());
                }
                plain(comment, "A comment", 2000)
            }
            Self::UndoRejectPhase { phase }
            | Self::DiscardPhase { phase, .. }
            | Self::ReturnToDraft { phase }
            | Self::UnpausePhase { phase }
            | Self::PhaseAcceptance { phase, .. } => phase_number(*phase),
            Self::MovePhase { phase, to, position } => {
                phase_number(*phase)?;
                phase_number(*to)?;
                if phase == to {
                    return Err("A phase moves before or after another phase.".into());
                }
                if !matches!(position.as_str(), "before" | "after") {
                    return Err("A phase moves before or after another.".into());
                }
                Ok(())
            }
            Self::PausePhase { phase, reason } => {
                phase_number(*phase)?;
                plain(reason, "A reason", 400)
            }
            Self::EditDraft { phase, title, tasks_update, .. } => {
                phase_number(*phase)?;
                if let Some(title) = title {
                    if title.trim().is_empty() {
                        return Err("A phase needs a title.".into());
                    }
                    plain(title, "A phase title", 120)?;
                }
                if tasks_update.len() > 200 {
                    return Err("At most 200 task changes at once.".into());
                }
                for change in tasks_update {
                    let id = change.get("id").and_then(Value::as_u64).unwrap_or(0);
                    task(id)?;
                    if let Some(title) = change.get("title") {
                        let title = title.as_str().ok_or("A task title is text.")?;
                        if title.trim().is_empty() {
                            return Err(format!("Task #{id} needs a title."));
                        }
                        plain(title, "A task title", 200)?;
                    }
                }
                Ok(())
            }
            Self::PlanEdit { change, .. } => change.check(),
            Self::PauseMember { member }
            | Self::ResumeMember { member }
            | Self::Relaunch { member }
            | Self::Drain { member, .. }
            | Self::RemoveMember { member, .. }
            | Self::SubLead { member, .. } => word(member, "member id"),
            Self::ReportsTo { member, parent } => {
                word(member, "member id")?;
                word(parent, "member id")?;
                if member == parent {
                    return Err("A member does not report to itself.".into());
                }
                Ok(())
            }
            Self::Scale { role, agent, count, model, effort, account } => {
                if role.trim().is_empty() {
                    return Err("Name the role to add more of.".into());
                }
                plain(role, "A role", 120)?;
                word(agent, "agent")?;
                if !(1..=20).contains(count) {
                    return Err("Add 1 to 20 members at once.".into());
                }
                for (value, what) in [(model, "model"), (effort, "effort"), (account, "account")] {
                    if !value.is_empty() {
                        word(value, what)?;
                    }
                }
                Ok(())
            }
            Self::SwapMember { member, agent, model, effort, account, fresh } => {
                word(member, "member id")?;
                let mut changes = false;
                for (value, what) in [(agent, "agent"), (model, "model"), (effort, "effort"), (account, "account")] {
                    if !value.is_empty() {
                        word(value, what)?;
                        changes = true;
                    }
                }
                if !changes && !*fresh {
                    return Err("The swap changes nothing: choose its agent, model, effort or account, or a fresh session.".into());
                }
                Ok(())
            }
            Self::ForceTest { suite } => word(suite, "suite"),
            // A revoke names the key's id (its label only words the question).
            Self::IntakeKey { label, revoke } if !revoke.is_empty() => word(revoke, "key id").and(plain(label, "A key's name", 60)),
            Self::IntakeKey { label, .. } => {
                if label.trim().is_empty() {
                    return Err("Name the new key.".into());
                }
                plain(label, "A key's name", 60)
            }
            Self::MoveAccount { provider, from_account, to_account } => {
                word(provider, "agent")?;
                word(from_account, "account")?;
                word(to_account, "account")?;
                if from_account == to_account {
                    return Err("Choose another account to move to.".into());
                }
                Ok(())
            }
            Self::AddMember { member } => {
                let Some(fields) = member.as_object() else { return Err("A member is an object.".into()) };
                for key in ["role", "agent"] {
                    let value = fields.get(key).and_then(Value::as_str).unwrap_or_default();
                    if value.trim().is_empty() {
                        return Err(format!("A new member needs its {key}."));
                    }
                }
                plain(fields.get("role").and_then(Value::as_str).unwrap_or_default(), "A role", 120)?;
                word(fields.get("agent").and_then(Value::as_str).unwrap_or_default(), "agent")?;
                if serde_json::to_vec(member).map_or(0, |bytes| bytes.len()) > 16 * 1024 {
                    return Err("The member is too large.".into());
                }
                Ok(())
            }
            Self::AcceptPhase { phase } => {
                if *phase == 0 {
                    return Err("Not a phase number.".into());
                }
                Ok(())
            }
            Self::PhaseMode { phase, mode, reason } => {
                if *phase == 0 || *phase > u64::from(u32::MAX) {
                    return Err("Not a phase number.".into());
                }
                if !matches!(mode.as_str(), "free" | "standard") {
                    return Err("A plan mode is standard or free.".into());
                }
                plain(reason, "A reason", 2000)
            }
            Self::TeamMode { mode, reason } => {
                if !matches!(mode.as_str(), "free" | "standard") {
                    return Err("A plan mode is standard or free.".into());
                }
                plain(reason, "A reason", 2000)
            }
            Self::SendBackPhase { phase, comment } => {
                if *phase == 0 {
                    return Err("Not a phase number.".into());
                }
                if comment.trim().is_empty() {
                    return Err("Say what the lead should change.".into());
                }
                plain(comment, "A comment", TEXT_CHARS)
            }
            Self::DecideTool { offer, reason, .. } => {
                word(offer, "tool offer")?;
                plain(reason, "A reason", 2000)
            }
            Self::Propose {
                proposal_kind,
                reason,
                offer,
            } => {
                if proposal_kind != "install-tool" {
                    return Err("The lead asks the owner to install a tool.".into());
                }
                if reason.trim().is_empty() {
                    return Err("A reason is required to ask the owner.".into());
                }
                word(offer, "tool offer")?;
                plain(reason, "A reason", 2000)
            }
            Self::TestConfig { action, tests } => {
                if !matches!(action.as_str(), "get" | "check") {
                    return Err("Read the test configuration or check an override.".into());
                }
                if !(tests.is_null() || tests.is_object()) {
                    return Err("The tests override is an object shaped like .zerus/tests.toml.".into());
                }
                if tests.to_string().len() > 64 * 1024 {
                    return Err("The tests override has at most 64 KiB.".into());
                }
                Ok(())
            }
            Self::Disk { action, scope, paths, path, policy, .. } => {
                if !matches!(action.as_str(), "list" | "clean" | "policy" | "forget") {
                    return Err("List, clean, set the policy or forget.".into());
                }
                // Task #364: one team's finished-task targets, `team:<id>`.
                if !matches!(scope.as_str(), "" | "policy" | "all") && !(scope.starts_with("team:") && scope.len() > "team:".len()) {
                    return Err("Clean what the policy removes, every folder of Zerus's, or one team's finished tasks.".into());
                }
                if action == "policy" && !policy.is_object() {
                    return Err("The policy is missing.".into());
                }
                if action == "forget" && path.trim().is_empty() {
                    return Err("Name the folder.".into());
                }
                if paths.len() > 500 {
                    return Err("At most 500 folders at once.".into());
                }
                for folder in paths.iter().chain(std::iter::once(path)) {
                    plain(folder, "A folder", 4096)?;
                }
                Ok(())
            }
            Self::AnswerDeferred { id, decision, text } => {
                word(id, "deferred request")?;
                if !matches!(decision.as_str(), "allow" | "deny" | "answer") {
                    return Err("Allow, deny or answer it.".into());
                }
                if decision == "answer" && text.trim().is_empty() {
                    return Err("Write the answer.".into());
                }
                plain(text, "An answer", 4000)
            }
            Self::Finding { finding, action, note, proposal, task: id } => {
                word(finding, "finding id")?;
                if !matches!(action.as_str(), "fixed" | "dropped" | "defer" | "reopen" | "urgent" | "replan") {
                    return Err("Fixed, dropped, deferred, reopened, urgent or replan.".into());
                }
                if action == "replan" && proposal.as_deref().unwrap_or("").trim().is_empty() {
                    return Err("A replan says what the lead should change.".into());
                }
                plain(note.as_deref().unwrap_or(""), "A note", 2000)?;
                plain(proposal.as_deref().unwrap_or(""), "A proposal", 4000)?;
                if let Some(id) = id {
                    task(*id)?;
                }
                Ok(())
            }
            Self::NextGoal { goal, name } => {
                if goal.trim().is_empty() {
                    return Err("Write the next goal.".into());
                }
                plain(goal, "The next goal", 4000)?;
                plain(name.as_deref().unwrap_or(""), "A name", 80)
            }
            Self::Create { name, parent } => {
                if name.trim().is_empty() {
                    return Err("Name the sub-team.".into());
                }
                plain(name, "A sub-team name", 80)?;
                word(&parent.team, "parent team")?;
                task(parent.task)
            }
            Self::SeenLeadChanges => Ok(()),
            Self::KeepWorkingMode => Ok(()),
            Self::RevertLeadChange { change } => word(change, "lead change id"),
            Self::WaitingDismiss { id, state, team } => {
                word(id, "Waiting item id")?;
                if state.trim().is_empty() {
                    return Err("Dismissing a Waiting row needs its state.".into());
                }
                plain(state, "A Waiting state", 400)?;
                if team.is_empty() { Ok(()) } else { word(team, "team id") }
            }
            Self::WaitingRestore { id } => word(id, "Waiting item id"),
            Self::ProposeWorkingMode { preset, reason } => {
                // Task #451: "auto" is the project's profile as it is now, which
                // the block's Propose the switch asks for.
                if !matches!(preset.as_str(), "auto" | "branch-pipeline" | "branch-per-task" | "shared-folder") {
                    return Err("The working mode is auto, branch-pipeline, branch-per-task or shared-folder.".into());
                }
                plain(reason, "A reason", 2000)
            }
            Self::Release { task: id } | Self::Retry { task: id } => task(*id),
            Self::DecideProposal {
                proposal,
                expected_hash,
                reason,
                answer,
                ..
            } => {
                word(proposal, "proposal id")?;
                word(expected_hash, "proposal hash")?;
                plain(reason, "A reason", 2000)?;
                plain(answer, "An answer", 4000)
            }
            Self::DraftStart {
                folder, name, lead, ..
            } => {
                if folder.trim().is_empty() {
                    return Err("Choose the team's folder.".into());
                }
                plain(folder, "A folder", 4096)?;
                plain(name, "A name", 80)?;
                if !lead.is_object() {
                    return Err("A draft names its lead.".into());
                }
                Ok(())
            }
            Self::DraftUpdate { set, .. } => {
                if !set.as_object().is_some_and(|fields| !fields.is_empty()) {
                    return Err("A draft update names the fields it sets.".into());
                }
                Ok(())
            }
            Self::DraftSeen { fields } => {
                if fields.is_empty() || fields.len() > 32 {
                    return Err("Name the fields seen.".into());
                }
                fields
                    .iter()
                    .try_for_each(|field| word(field, "draft field"))
            }
            Self::DraftCreate { .. } => Ok(()),
            Self::DraftDiscard { reason, .. } => plain(reason, "A reason", 2000),
            Self::Settings {
                settings,
                members_update,
            } => {
                if !settings.as_object().is_some_and(|fields| !fields.is_empty())
                    && members_update.is_empty()
                {
                    return Err("A settings change names the fields it sets.".into());
                }
                Ok(())
            }
            Self::CatalogPropose { change } => {
                if !change.as_object().is_some_and(|fields| !fields.is_empty()) {
                    return Err("A catalog proposal names its change.".into());
                }
                Ok(())
            }
            Self::CatalogDecide { proposal, decision } => {
                word(proposal, "proposal id")?;
                if !matches!(decision.as_str(), "keep" | "reject" | "undo") {
                    return Err("A catalog decision is keep, reject or undo.".into());
                }
                Ok(())
            }
            Self::CatalogApply | Self::CatalogDiscard => Ok(()),
            Self::CatalogSet { catalog, .. } => {
                if !catalog.as_object().is_some_and(|fields| fields.contains_key("roles") || fields.contains_key("templates")) {
                    return Err("A catalog to save has roles and templates.".into());
                }
                if catalog.to_string().len() > 240 * 1024 {
                    return Err("The catalog is too large to save.".into());
                }
                Ok(())
            }
            Self::CatalogDesigner { folder, goal, agent, model, effort, project } => {
                if folder.trim().is_empty() || folder.chars().any(char::is_control) || folder.chars().count() > 1000 {
                    return Err("Choose a project with a folder on this computer first.".into());
                }
                plain(goal, "The goal", TEXT_CHARS)?;
                if goal.trim().is_empty() {
                    return Err("Describe the work for the designer.".into());
                }
                if !agent.is_empty() && !matches!(agent.as_str(), "claude" | "codex" | "kimi" | "dsh" | "opencode") {
                    return Err("The designer runs as Claude, Codex, Kimi, DeepSeek or OpenCode.".into());
                }
                for (value, what) in [(model, "A model"), (effort, "An effort"), (project, "A project")] {
                    plain(value, what, 200)?;
                }
                Ok(())
            }
        }
    }

    /// The request's own fields (without `team`, `by` and `request_id`).
    pub fn input(&self) -> Map<String, Value> {
        let mut input = Map::new();
        let mut put = |key: &str, value: Value| {
            input.insert(key.to_owned(), value);
        };
        match self {
            Self::Approve {
                phase,
                plan_mode,
                confirm_widening,
                alongside,
            } => {
                match phase {
                    Some(phase) => {
                        put("phase", json!(phase));
                        if *alongside {
                            put("alongside", json!(true));
                        }
                    }
                    None => {
                        if let Some(mode) = plan_mode {
                            put("plan_mode", json!(mode));
                        }
                    }
                }
                if *confirm_widening {
                    put("confirm_widening", json!(true));
                }
            }
            Self::Accept {
                task,
                reason,
                manual,
            } => {
                put("task", json!(task));
                if !reason.is_empty() {
                    put("reason", json!(reason));
                }
                if *manual {
                    put("manual", json!(true));
                }
            }
            Self::Reject {
                task,
                reason,
                hold,
                fresh,
            } => {
                put("task", json!(task));
                put("reason", json!(reason));
                if *hold {
                    put("hold", json!(true));
                }
                if *fresh {
                    put("fresh", json!(true));
                }
            }
            Self::Reply { question, text } => {
                put("question", json!(question));
                put("text", json!(text));
            }
            Self::Reassign {
                task,
                member,
                subteam,
            } => {
                put("task", json!(task));
                put("member", json!(member));
                if let Some(choice) = subteam {
                    put("subteam", json!(choice));
                }
            }
            Self::TakeOver { task } => put("task", json!(task)),
            Self::Cancel { task, subteam } => {
                put("task", json!(task));
                if let Some(choice) = subteam {
                    put("subteam", json!(choice));
                }
            }
            Self::Pause | Self::Resume | Self::Stop | Self::Archive | Self::Discard => {}
            Self::Finish { summary } => put("summary", json!(summary.trim())),
            Self::Hold { task, reason }
            | Self::SkipStage { task, reason }
            | Self::SendBack { task, reason } => {
                put("task", json!(task));
                put("reason", json!(reason.trim()));
            }
            Self::MoveTask {
                task,
                phase,
                revision,
            } => {
                put("task", json!(task));
                put("phase", json!(phase));
                if let Some(revision) = revision {
                    put("revision", json!(revision));
                }
            }
            Self::EditTasks { phase, tasks } => {
                put("phase", json!(phase));
                put("tasks_update", json!(tasks));
            }
            Self::Answer {
                member,
                question_id,
                expected_question_hash,
                answers,
            } => {
                put("member", json!(member));
                put("question_id", json!(question_id));
                if !expected_question_hash.is_empty() {
                    put("expected_question_hash", json!(expected_question_hash));
                }
                put("answers", answers.clone());
            }
            Self::AddToPhase {
                phase,
                confirm,
                confirmation,
                decline,
                tasks,
            } => {
                put("phase", json!(phase));
                if !tasks.is_empty() {
                    put("tasks", Value::Array(tasks.clone()));
                } else {
                    put("confirm", json!(confirm));
                    if *decline {
                        put("decline", json!(true));
                    } else if !confirmation.is_empty() {
                        put("confirmation", json!(confirmation));
                    }
                }
            }
            Self::RejectPhase { phase, comment, revision } => {
                put("phase", json!(phase));
                put("comment", json!(comment.trim()));
                if let Some(revision) = revision {
                    put("revision", json!(revision));
                }
            }
            Self::UndoRejectPhase { phase }
            | Self::DiscardPhase { phase, .. }
            | Self::ReturnToDraft { phase }
            | Self::UnpausePhase { phase } => put("phase", json!(phase)),
            Self::MovePhase { phase, to, position } => {
                put("phase", json!(phase));
                put("to", json!(to));
                put("position", json!(position));
            }
            Self::PausePhase { phase, reason } => {
                put("phase", json!(phase));
                put("reason", json!(reason.trim()));
            }
            Self::PhaseAcceptance { phase, owner } => {
                put("phase", json!(phase));
                put("acceptance", json!(if *owner { "owner" } else { "lead" }));
            }
            Self::EditDraft { phase, revision, title, barrier, tasks_update } => {
                put("phase", json!(phase));
                if let Some(revision) = revision {
                    put("revision", json!(revision));
                }
                if let Some(title) = title {
                    put("title", json!(title.trim()));
                }
                if let Some(barrier) = barrier {
                    put("barrier", json!(barrier));
                }
                if !tasks_update.is_empty() {
                    put("tasks_update", Value::Array(tasks_update.clone()));
                }
            }
            // The whole plan is built by `run` from a fresh read of the team.
            Self::PlanEdit { revision, .. } => put("revision", json!(revision)),
            Self::PauseMember { member } | Self::ResumeMember { member } | Self::Relaunch { member } | Self::RemoveMember { member, .. } => {
                put("member", json!(member))
            }
            Self::Drain { member, cancel, .. } => {
                put("member", json!(member));
                if *cancel {
                    put("draining", json!(false));
                }
            }
            Self::ReportsTo { member, parent } => {
                put("member", json!(member));
                put("parent", json!(parent));
            }
            Self::SubLead { member, sub } => {
                put("member", json!(member));
                put("sub", json!(sub));
            }
            Self::Scale { role, agent, count, model, effort, account } => {
                put("role", json!(role.trim()));
                put("agent", json!(agent));
                put("count", json!(count));
                for (key, value) in [("model", model), ("effort", effort), ("account", account)] {
                    if !value.is_empty() {
                        put(key, json!(value));
                    }
                }
            }
            Self::SwapMember { member, agent, model, effort, account, fresh } => {
                put("member", json!(member));
                for (key, value) in [("agent", agent), ("model", model), ("effort", effort), ("account", account)] {
                    if !value.is_empty() {
                        put(key, json!(value));
                    }
                }
                if *fresh {
                    put("fresh", json!(true));
                }
            }
            Self::MoveAccount { provider, from_account, to_account } => {
                put("provider", json!(provider));
                put("from_account", json!(from_account));
                put("to_account", json!(to_account));
            }
            Self::AddMember { member } => put("members", json!([hgs_member(member)])),
            Self::IntakeKey { label, revoke } => {
                if revoke.is_empty() {
                    put("label", json!(label.trim()));
                } else {
                    put("revoke", json!(revoke));
                }
            }
            Self::ForceTest { suite } => {
                put("action", json!("submit"));
                put("suite", json!(suite));
                put("forced", json!(true));
                put("asked_by", json!("You"));
            }
            Self::AcceptPhase { phase } => put("phase", json!(phase)),
            Self::PhaseMode { phase, mode, reason } => {
                put("phase", json!(phase));
                put("mode", json!(mode));
                if !reason.trim().is_empty() {
                    put("reason", json!(reason.trim()));
                }
            }
            Self::TeamMode { mode, reason } => {
                put("mode", json!(mode));
                if !reason.trim().is_empty() {
                    put("reason", json!(reason.trim()));
                }
            }
            Self::SendBackPhase { phase, comment } => {
                put("phase", json!(phase));
                put("comment", json!(comment));
            }
            Self::DecideTool {
                offer,
                allow,
                reason,
            } => {
                put("offer", json!(offer));
                put("allow", json!(allow));
                if !allow && !reason.is_empty() {
                    put("reason", json!(reason));
                }
            }
            Self::Propose {
                proposal_kind,
                reason,
                offer,
            } => {
                put("kind", json!(proposal_kind));
                put("reason", json!(reason.trim()));
                if !offer.is_empty() {
                    put("offer", json!(offer));
                }
            }
            Self::TestConfig { action, tests } => {
                put("action", json!(action));
                if !tests.is_null() {
                    put("tests", tests.clone());
                }
            }
            Self::Disk { action, scope, dry_run, policy, paths, path, refresh, measure } => {
                put("action", json!(action));
                if !scope.is_empty() {
                    put("scope", json!(scope));
                }
                if action == "clean" {
                    put("dry_run", json!(dry_run));
                }
                if policy.is_object() {
                    put("policy", policy.clone());
                }
                if !paths.is_empty() {
                    put("paths", json!(paths));
                }
                if !path.is_empty() {
                    put("path", json!(path));
                }
                if *refresh {
                    put("refresh", json!(true));
                }
                if *measure {
                    put("measure", json!(true));
                }
            }
            Self::AnswerDeferred { id, decision, text } => {
                put("id", json!(id));
                put("decision", json!(decision));
                if !text.trim().is_empty() {
                    put("text", json!(text.trim()));
                }
            }
            Self::Finding { finding, action, note, proposal, task: id } => {
                put("finding", json!(finding));
                put("action", json!(action));
                if let Some(note) = note {
                    put("note", json!(note));
                }
                if let Some(proposal) = proposal {
                    put("proposal", json!(proposal));
                }
                if let Some(id) = id {
                    put("task", json!(id));
                }
            }
            Self::NextGoal { goal, name } => {
                put("goal", json!(goal));
                if let Some(name) = name {
                    put("name", json!(name));
                }
            }
            Self::Create { name, parent } => {
                put("name", json!(name));
                put("parent", json!({"team": parent.team, "task": parent.task}));
            }
            Self::SeenLeadChanges => put("by", json!("human")),
            Self::KeepWorkingMode => put("by", json!("human")),
            Self::RevertLeadChange { change } => {
                put("change", json!(change));
                put("by", json!("human"));
            }
            Self::WaitingDismiss { id, state, team } => {
                put("action", json!("dismiss"));
                put("id", json!(id));
                put("state", json!(state));
                if !team.is_empty() {
                    put("team", json!(team));
                }
            }
            Self::WaitingRestore { id } => {
                put("action", json!("undismiss"));
                put("id", json!(id));
            }
            Self::ProposeWorkingMode { preset, reason } => {
                put("kind", json!("working-mode"));
                put("preset", json!(preset));
                if !reason.trim().is_empty() {
                    put("reason", json!(reason));
                }
            }
            Self::Release { task } | Self::Retry { task } => put("task", json!(task)),
            Self::DecideProposal {
                proposal,
                accept,
                expected_hash,
                reason,
                answer,
            } => {
                put("proposal", json!(proposal));
                put("accept", json!(accept));
                put("expected_hash", json!(expected_hash));
                if !reason.is_empty() {
                    put("reason", json!(reason));
                }
                if !answer.is_empty() {
                    put("answer", json!(answer));
                }
            }
            Self::UseLastReply { task, attempt } => {
                put("task", json!(task));
                if !attempt.is_empty() {
                    put("attempt", json!(attempt));
                }
            }
            Self::DraftStart {
                folder,
                name,
                lead,
                project,
                project_id,
                template,
            } => {
                put("action", json!("start"));
                put("folder", json!(folder));
                put("lead", hgs_member(lead));
                for (key, value) in [
                    ("name", name),
                    ("project", project),
                    ("project_id", project_id),
                    ("template", template),
                ] {
                    if !value.is_empty() {
                        put(key, json!(value));
                    }
                }
            }
            Self::DraftUpdate { revision, set } => {
                put("action", json!("update"));
                put("revision", json!(revision));
                put("set", hgs_draft_set(set));
            }
            Self::DraftSeen { fields } => {
                put("action", json!("seen"));
                put("fields", json!(fields));
            }
            Self::DraftCreate {
                approve,
                confirm_widening,
            } => {
                put("action", json!("create"));
                if *approve {
                    put("approve", json!(true));
                }
                if *confirm_widening {
                    put("confirm_widening", json!(true));
                }
            }
            Self::DraftDiscard { stop_lead, reason } => {
                put("action", json!("discard"));
                put("stop_lead", json!(stop_lead));
                if !reason.is_empty() {
                    put("reason", json!(reason));
                }
            }
            Self::Settings {
                settings,
                members_update,
            } => {
                if settings.as_object().is_some_and(|fields| !fields.is_empty()) {
                    put("settings", settings.clone());
                }
                if !members_update.is_empty() {
                    put("members_update", json!(members_update));
                }
            }
            Self::CatalogPropose { change } => {
                put("change", change.clone());
            }
            Self::CatalogDecide { proposal, decision } => {
                put("proposal", json!(proposal));
                put("decision", json!(decision));
            }
            Self::CatalogApply | Self::CatalogDiscard => {}
            Self::CatalogSet { version, catalog } => {
                put("version", json!(version));
                put("catalog", catalog.clone());
            }
            Self::CatalogDesigner { folder, goal, agent, model, effort, project } => {
                put("folder", json!(folder));
                put("goal", json!(goal));
                for (key, value) in [("agent", agent), ("model", model), ("effort", effort), ("project", project)] {
                    if !value.is_empty() {
                        put(key, json!(value));
                    }
                }
            }
        }
        input
    }

    /// What the owner confirms first, Qt's dialogs' words: destructive
    /// actions and approvals. None: the action runs at once.
    pub fn confirmation(&self, team_name: &str) -> Option<Confirmation> {
        let name = if team_name.trim().is_empty() {
            "this team"
        } else {
            team_name.trim()
        };
        let ask = |dialog: &str, title: &str, text: String, action: &str| Confirmation {
            name: dialog.to_owned(),
            title: title.to_owned(),
            text,
            action: action.to_owned(),
        };
        match self {
            Self::Stop => Some(ask(
                "teamStopConfirm",
                "Stop team?",
                format!(
                    "Stop {name}? Every open task is cancelled and no task is handed out again. Member sessions keep running; their current turns finish."
                ),
                "Stop team",
            )),
            // Task 108: Clean now removes folders; a dry run and the rest ask nothing.
            Self::Disk { action, dry_run: false, scope, .. } if action == "clean" => Some(ask(
                "diskCleanConfirm",
                "Remove build folders?",
                if scope == "all" {
                    "Remove every build folder Zerus holds on this computer? Only Zerus's own build output goes; a folder in use, or one that is not Zerus's, is kept. Builds make them again.".to_owned()
                } else {
                    "Remove the build folders the cleanup policy marks now? Only Zerus's own build output goes; a folder in use, or one that is not Zerus's, is kept. Builds make them again.".to_owned()
                },
                "Remove",
            )),
            Self::Discard => Some(ask(
                "teamDiscardConfirm",
                "Discard plan?",
                format!("Discard the plan of {name}? The team stops before any member starts."),
                "Discard plan",
            )),
            Self::Finish { .. } => Some(ask(
                "teamFinishConfirm",
                "Finish team?",
                format!("Finish {name}? Every task is decided; the team is done and hands out nothing more."),
                "Finish team",
            )),
            Self::Archive => Some(ask(
                "teamArchiveConfirm",
                "Archive team?",
                format!("Archive {name}? Its history, tasks and reports stay; the team leaves the list of teams."),
                "Archive",
            )),
            // Task 196: Qt's phase confirmations, word for word.
            Self::Drain { role, cancel: false, member } => Some(ask(
                "teamDrainMemberConfirm",
                "Drain member?",
                format!(
                    "{} finishes its current task and review, gets nothing new, then its session is paused and it leaves the team.",
                    if role.trim().is_empty() { member.as_str() } else { role.trim() }
                ),
                "Drain",
            )),
            Self::IntakeKey { label, revoke } if !revoke.is_empty() => Some(ask(
                "teamRevokeIntakeConfirm",
                "Revoke intake key",
                format!("Revoke the key \"{}\"?", if label.trim().is_empty() { revoke.as_str() } else { label.trim() }),
                "Revoke",
            )),
            Self::RemoveMember { member, role } => Some(ask(
                "teamRemoveMemberConfirm",
                "Remove member?",
                format!(
                    "Remove {} from {name}? Its session keeps running; it gets no more tasks.",
                    if role.trim().is_empty() { member.as_str() } else { role.trim() }
                ),
                "Remove",
            )),
            Self::DiscardPhase { phase, tasks, accepted } => Some(ask(
                "teamDiscardPhaseConfirm",
                "Discard phase?",
                if *accepted {
                    format!("Phase {phase} is already accepted. Discard it and its {tasks} tasks? Only a phase whose work has not started can go; this cannot be undone.")
                } else {
                    format!("Discard phase {phase} and its {tasks} tasks?")
                },
                "Discard phase",
            )),
            Self::ReturnToDraft { phase } => Some(ask(
                "teamReturnPhaseConfirm",
                "Return phase to draft?",
                format!("Phase {phase} goes back to a draft: its tasks are not handed out until you approve it again. Only a phase whose work has not started can return."),
                "Return to draft",
            )),
            Self::Cancel { .. } => Some(ask(
                "teamCancelTaskConfirm",
                "Cancel task?",
                "Cancel this task? Tasks that depend on it hard never become ready.".to_owned(),
                "Cancel task",
            )),
            Self::TakeOver { .. } => Some(ask(
                "teamTakeOverConfirm",
                "Take over task?",
                "You do this task yourself; its current attempt is revoked and no member gets it.".to_owned(),
                "Take over",
            )),
            Self::DraftDiscard { stop_lead, .. } => Some(ask(
                "teamDiscardDraftConfirm",
                "Discard draft",
                if *stop_lead {
                    format!("Discard the draft \u{201c}{name}\u{201d}? Nothing of it is created, and the lead's session stops.")
                } else {
                    format!("Discard the draft \u{201c}{name}\u{201d}? Nothing of it is created.")
                },
                "Discard draft",
            )),
            Self::Approve { phase: Some(phase), .. } => Some(ask(
                "teamApprovePhaseConfirm",
                "Approve phase?",
                format!("Hand phase {phase}'s tasks to the members?"),
                "Approve phase",
            )),
            Self::Approve { phase: None, confirm_widening, .. } => Some(if *confirm_widening {
                ask(
                    "teamWideningConfirm",
                    "Widen permissions?",
                    format!("Approve the plan of {name}? It widens the permissions of its members beyond the template; approve it only if they need it."),
                    "Confirm and approve",
                )
            } else {
                ask(
                    "teamApproveConfirm",
                    "Approve plan and run?",
                    format!("Approve the plan of {name} and run it? Ready tasks are handed out at once."),
                    "Approve and run",
                )
            }),
            // Task #58: Qt's teamPhaseModeDialog.
            Self::PhaseMode { phase, mode, .. } if mode == "free" => Some(ask(
                "teamPhaseModeConfirm",
                "Switch to free planning?",
                format!(
                    "Switch phase {phase} to free planning? It stays open when its tasks are accepted: new tasks join it and run as they are added. The other phases keep their modes."
                ),
                "Switch to free",
            )),
            Self::TeamMode { mode, .. } => Some(ask(
                "teamModeConfirm",
                if mode == "free" { "Switch the team to free planning?" } else { "Switch the team to standard planning?" },
                format!(
                    "Switch the team to {mode} planning? Every phase without a mode of its own switches with it; a phase with its own mode keeps it."
                ),
                if mode == "free" { "Switch to free" } else { "Switch to standard" },
            )),
            Self::PhaseMode { phase, .. } => Some(ask(
                "teamPhaseModeConfirm",
                "Switch to standard planning?",
                format!(
                    "Switch phase {phase} to standard planning? Its tasks keep their state and run on; new work is planned in a new phase, approved by you. The other phases keep their modes."
                ),
                "Switch to standard",
            )),
            Self::CatalogApply => Some(ask(
                "teamCatalogApplyConfirm",
                "Apply the catalog draft?",
                "Write the kept changes as the next catalog version? Undecided changes are dropped with the draft."
                    .to_owned(),
                "Apply",
            )),
            Self::CatalogDiscard => Some(ask(
                "teamCatalogDiscardConfirm",
                "Discard draft",
                "Drop every proposal in the designer's draft? The catalog stays as it is."
                    .to_owned(),
                "Discard draft",
            )),
            _ => None,
        }
    }
}

fn outcome_failed(detail: impl Into<String>, request_id: &str) -> TeamOutcome {
    TeamOutcome::Failed {
        detail: detail.into(),
        request_id: request_id.to_owned(),
    }
}

fn outcome_uncertain(detail: impl Into<String>, request_id: &str) -> TeamOutcome {
    TeamOutcome::Uncertain {
        detail: detail.into(),
        request_id: request_id.to_owned(),
    }
}

/// The checked call: its command and its request id. A call that is not
/// sent gives the outcome instead: `confirm` without the owner's
/// confirmation, `failed` for a request that cannot be made.
pub fn prepare(call: &TeamCall) -> Result<(Invocation, String), TeamOutcome> {
    let request_id = match &call.request_id {
        Some(id) => id.clone(),
        None => new_request_id(),
    };
    if !canonical_uuid(&request_id) {
        return Err(outcome_failed("Not a request id.", &request_id));
    }
    if !call.computer.is_empty() && !valid_alias(&call.computer) {
        return Err(outcome_failed("Not a computer alias.", &request_id));
    }
    if call.action.names_team() && !super::read::valid_team(&call.team) {
        return Err(outcome_failed("Not a team id.", &request_id));
    }
    if let Err(detail) = call.action.check() {
        return Err(outcome_failed(detail, &request_id));
    }
    if !call.confirmed {
        if let Some(confirmation) = call.action.confirmation(&call.team_name) {
            return Err(TeamOutcome::Confirm { confirmation });
        }
    }
    let mut input = call.action.input();
    if call.action.names_team() {
        input.insert("team".into(), json!(call.team));
    }
    input.insert("by".into(), json!("human"));
    input.insert("request_id".into(), json!(request_id));
    let mut args: Vec<String> = (!call.computer.is_empty())
        .then(|| format!("@{}", call.computer))
        .into_iter()
        .collect();
    if let Some(sub) = call.action.catalog_op() {
        args.extend([
            "team".to_owned(),
            "catalog".to_owned(),
            sub.to_owned(),
            "--json".to_owned(),
        ]);
    } else {
        args.extend([
            "team".to_owned(),
            call.action.op().to_owned(),
            "--json".to_owned(),
        ]);
    }
    let invocation = Invocation {
        args,
        stdin: Some(Value::Object(input).to_string().into_bytes()),
        // Qt's requestTeam: 15 s here, 45 s on a peer. Task #44: the
        // designer starts an agent session, which hgs gives 120 s.
        timeout: Duration::from_secs(match (&call.action, call.computer.is_empty()) {
            (TeamAction::CatalogDesigner { .. }, _) => 150,
            // Owner, 2026-11-09 (package 169): Start planning starts or binds
            // the lead's agent session too, which hgs gives up to 120 s; 15 s
            // gave "hgs did not answer in time" and no team.
            (TeamAction::DraftStart { .. }, _) => 150,
            (_, true) => 15,
            (_, false) => 45,
        }),
        request_id: Some(request_id.clone()),
    };
    Ok((invocation, request_id))
}

fn stderr_text(stderr: &str) -> Option<String> {
    let text: String = stderr.trim().chars().take(DETAIL_CHARS).collect();
    (!text.is_empty()).then_some(text)
}

const READ_FIRST: &str = "Read the team before sending it again.";

/// The outcome of a call's exit. Only a refusal (an exit code) or a call that
/// never started is `failed`; everything that may have reached the store
/// without a valid receipt is `uncertain`.
pub fn classify(action: &TeamAction, request_id: &str, exit: Exit) -> TeamOutcome {
    match exit {
        Exit::StartFailed(error) => {
            outcome_failed(format!("hgs did not start: {error}"), request_id)
        }
        Exit::TimedOut => outcome_uncertain(
            format!("hgs did not answer in time; the change may have been made. {READ_FIRST}"),
            request_id,
        ),
        Exit::WriteFailed => outcome_uncertain(
            format!(
                "The request did not reach hgs whole; the change may have been made. {READ_FIRST}"
            ),
            request_id,
        ),
        Exit::Abnormal { stderr } => outcome_uncertain(
            stderr_text(&stderr).map_or_else(
                || format!("hgs ended abnormally. {READ_FIRST}"),
                |text| format!("{text} {READ_FIRST}"),
            ),
            request_id,
        ),
        Exit::Code { code, stderr, .. } if code != 0 => outcome_failed(
            stderr_text(&stderr).unwrap_or_else(|| format!("hgs exited with code {code}")),
            request_id,
        ),
        Exit::Code { stdout, .. } => {
            let receipt: Value = serde_json::from_slice(&stdout).unwrap_or(Value::Null);
            if !receipt.is_object() {
                return outcome_uncertain(format!("hgs gave no receipt. {READ_FIRST}"), request_id);
            }
            let matches = match action {
                // The new draft's id is the receipt.
                TeamAction::DraftStart { .. } => receipt["team"]
                    .as_str()
                    .is_some_and(super::read::valid_team),
                // The receipt names the proposal it decided (or its earlier
                // decision, on a replay).
                TeamAction::DecideProposal { proposal, .. } => receipt
                    .get("proposal")
                    .is_none_or(|id| id.as_str() == Some(proposal.as_str())),
                _ => true,
            };
            if matches {
                TeamOutcome::Done {
                    receipt,
                    request_id: request_id.to_owned(),
                }
            } else {
                outcome_uncertain(
                    format!("hgs's receipt does not match the request. {READ_FIRST}"),
                    request_id,
                )
            }
        }
    }
}

/// Runs one call through `hgs`: blocks for at most its timeout plus a few
/// seconds; callers run it off the UI thread. Never sends it twice.
pub fn run(hgs: &Path, call: &TeamCall) -> TeamOutcome {
    let (mut invocation, request_id) = match prepare(call) {
        Ok(prepared) => prepared,
        Err(outcome) => return outcome,
    };
    // Task 196: a plan edit sends the whole plan, built from a fresh read of
    // the whole team; a failed read sends nothing.
    if let TeamAction::PlanEdit { change, .. } = &call.action {
        let status = match read_status(hgs, call) {
            Ok(status) => status,
            Err(detail) => return outcome_failed(detail, &request_id),
        };
        let mut input: Map<String, Value> = serde_json::from_slice(invocation.stdin.as_deref().unwrap_or_default()).unwrap_or_default();
        match super::plan_edit::plan_input(&status, change) {
            Ok(plan) => input.extend(plan),
            Err(detail) => return outcome_failed(detail, &request_id),
        }
        invocation.stdin = Some(Value::Object(input).to_string().into_bytes());
    }
    classify(&call.action, &request_id, crate::run::run(hgs, &invocation))
}

/// Task 196: the whole team (`hgs team status --json`) a plan edit is built on.
fn read_status(hgs: &Path, call: &TeamCall) -> Result<Value, String> {
    let mut args: Vec<String> = (!call.computer.is_empty()).then(|| format!("@{}", call.computer)).into_iter().collect();
    args.extend(["team".to_owned(), "status".to_owned(), "--json".to_owned()]);
    let invocation = Invocation {
        args,
        stdin: Some(json!({"team": call.team}).to_string().into_bytes()),
        timeout: Duration::from_secs(if call.computer.is_empty() { 15 } else { 45 }),
        request_id: None,
    };
    match crate::run::run(hgs, &invocation) {
        Exit::Code { code: 0, stdout, .. } => serde_json::from_slice::<Value>(&stdout)
            .ok()
            .filter(Value::is_object)
            .ok_or_else(|| "The team could not be read; nothing was sent.".to_owned()),
        Exit::Code { stderr, .. } => Err(stderr_text(&stderr).unwrap_or_else(|| "The team could not be read; nothing was sent.".to_owned())),
        _ => Err("The team could not be read; nothing was sent.".to_owned()),
    }
}

/// Task #465: the member fields hgs team takes from the desktop, shared with
/// hgs's own test that checks them against its member model.
static MEMBER_FIELDS: std::sync::LazyLock<Vec<String>> = std::sync::LazyLock::new(|| {
    let list: Value = serde_json::from_str(include_str!("member_fields.json")).expect("member_fields.json");
    list["fields"]
        .as_array()
        .expect("member_fields.json lists its fields")
        .iter()
        .filter_map(|field| field.as_str().map(str::to_owned))
        .collect()
});

/// Task #465 (owner, package 168): New team's lead is the form's row, with
/// client-only keys (`lead`, a null `session`, an empty `folder`); hgs's strict
/// check refused it ("The lead has no field \"lead\""). A member goes to hgs
/// with only the fields hgs takes: a null value is no value, and an empty
/// `folder` is hgs's default (its folder kind has no empty value).
pub(crate) fn hgs_member(member: &Value) -> Value {
    let Some(fields) = member.as_object() else {
        return member.clone();
    };
    let kept = fields
        .iter()
        .filter(|(key, value)| {
            MEMBER_FIELDS.iter().any(|field| field == key.as_str())
                && !value.is_null()
                && !(key.as_str() == "folder" && value.as_str() == Some(""))
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    Value::Object(kept)
}

/// A draft update's `set`: a members list goes through [`hgs_member`]; the
/// object form (changes by member id, null removes one) and other fields stay.
fn hgs_draft_set(set: &Value) -> Value {
    let mut set = set.clone();
    if let Some(members) = set.get_mut("members").and_then(Value::as_array_mut) {
        for member in members.iter_mut() {
            *member = hgs_member(member);
        }
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Task #465: New team's lead and members reach hgs with only hgs's
    /// member fields (the shared list hgs checks against its model).
    #[test]
    fn draft_and_added_members_carry_only_the_fields_hgs_takes() {
        let row = json!({"id":"lead","role":"Lead","agent":"claude","model":"","effort":"","account":"",
            "permissions":"","computer":"","folder":"","prompt":"Plan it","lead":true,"session":null});
        let start = TeamAction::DraftStart {
            folder: "D:/example/team".into(),
            name: "Scratch".into(),
            lead: row.clone(),
            project: String::new(),
            project_id: String::new(),
            template: String::new(),
        };
        let input = start.input();
        let lead = input["lead"].as_object().unwrap();
        for key in lead.keys() {
            assert!(MEMBER_FIELDS.contains(key), "the lead sends {key}, which hgs does not take");
        }
        for gone in ["lead", "session", "folder"] {
            assert!(!lead.contains_key(gone), "{gone} stays out: {lead:?}");
        }
        assert_eq!(lead["agent"], "claude");
        assert_eq!(lead["prompt"], "Plan it");
        // An existing session as lead keeps its session.
        let mut this = row.clone();
        this["session"] = json!({"host":"","name":"claude/plan","run_id":"","conversation_id":""});
        assert_eq!(hgs_member(&this)["session"]["name"], "claude/plan");
        // A draft update's members list goes the same way; a worktree folder stays.
        let mut dev = row.clone();
        dev["id"] = json!("dev");
        dev["lead"] = json!(false);
        dev["folder"] = json!("worktree");
        let update = TeamAction::DraftUpdate { revision: 3, set: json!({"members":[row, dev],"goal":"Ship"}) };
        let input = update.input();
        let members = input["set"]["members"].as_array().unwrap();
        assert!(members.iter().all(|member| member.get("lead").is_none() && member.get("session").is_none()), "{members:?}");
        assert_eq!(members[1]["folder"], "worktree");
        assert_eq!(input["set"]["goal"], "Ship");
        let patches = TeamAction::DraftUpdate { revision: 3, set: json!({"members":{"dev":{"model":"opus"},"old":null}}) };
        assert_eq!(patches.input()["set"]["members"], json!({"dev":{"model":"opus"},"old":null}));
        let add = TeamAction::AddMember { member: json!({"role":"Backend","agent":"claude","lead":false}) };
        assert_eq!(add.input()["members"], json!([{"role":"Backend","agent":"claude"}]));
    }

    fn lane_edit() -> TeamAction {
        TeamAction::EditTasks {
            phase: 2,
            tasks: vec![
                json!({"id": 3, "lane": {"n": 1, "label": "First"}}),
                json!({"id": 4, "lane": null}),
            ],
        }
    }

    /// Task 304 (hgs #246): Dismiss and Restore are `hgs team waiting` calls
    /// the owner makes; a row without a team (a tool offer, a drive) works too.
    #[test]
    fn dismiss_and_restore_are_owner_waiting_calls() {
        let dismiss: TeamAction = serde_json::from_value(json!({"kind": "waiting_dismiss", "id": "t1/team_paused/1", "state": "paused:100.0", "team": "t1"})).unwrap();
        assert_eq!(dismiss.op(), "waiting");
        assert!(!dismiss.names_team());
        assert!(dismiss.check().is_ok());
        let call = TeamCall { computer: String::new(), team: String::new(), team_name: String::new(), action: dismiss, confirmed: false, request_id: Some("6f9619ff-8b86-4011-b42d-00c04fc964ff".into()) };
        let (invocation, _) = prepare(&call).unwrap();
        assert_eq!(invocation.args, ["team", "waiting", "--json"]);
        let sent: Value = serde_json::from_slice(invocation.stdin.as_deref().unwrap()).unwrap();
        assert_eq!(
            [&sent["action"], &sent["id"], &sent["state"], &sent["team"], &sent["by"]],
            [&json!("dismiss"), &json!("t1/team_paused/1"), &json!("paused:100.0"), &json!("t1"), &json!("human")]
        );
        let restore: TeamAction = serde_json::from_value(json!({"kind": "waiting_restore", "id": "tools/offer/cmake"})).unwrap();
        let input = restore.input();
        assert_eq!([&input["action"], &input["id"]], [&json!("undismiss"), &json!("tools/offer/cmake")]);
        assert!(input.get("team").is_none());
        // A dismissal names the state it hides.
        assert!(TeamAction::WaitingDismiss { id: "t1/x".into(), state: " ".into(), team: String::new() }.check().is_err());
        assert!(TeamAction::WaitingRestore { id: String::new() }.check().is_err());
    }

    #[test]
    fn an_edit_tasks_call_sends_the_lanes_through_edit_draft() {
        let action = lane_edit();
        assert_eq!(action.op(), "edit-draft");
        assert!(action.check().is_ok());
        let input = action.input();
        assert_eq!(input["phase"], json!(2));
        assert_eq!(
            input["tasks_update"],
            json!([{"id":3,"lane":{"n":1,"label":"First"}},{"id":4,"lane":null}])
        );
    }

    #[test]
    fn an_edit_tasks_call_refuses_a_bad_phase_task_or_lane() {
        let bad = [
            TeamAction::EditTasks { phase: 0, tasks: vec![json!({"id":1,"lane":null})] },
            TeamAction::EditTasks { phase: 1, tasks: Vec::new() },
            TeamAction::EditTasks { phase: 1, tasks: vec![json!({"lane":null})] },
            TeamAction::EditTasks { phase: 1, tasks: vec![json!({"id":1})] },
            TeamAction::EditTasks { phase: 1, tasks: vec![json!({"id":1,"lane":{"n":0}})] },
            TeamAction::EditTasks { phase: 1, tasks: vec![json!({"id":1,"lane":{"label":"x"}})] },
            TeamAction::EditTasks { phase: 1, tasks: vec![json!({"id":1,"lane":{"n":1,"label":"x".repeat(41)}})] },
            TeamAction::EditTasks { phase: 1, tasks: vec![json!({"id":1,"lane":"first"})] },
        ];
        for action in bad {
            assert!(action.check().is_err(), "{action:?}");
        }
    }

    #[test]
    fn task251_propose_asks_the_owner_for_the_tools_install() {
        let action = TeamAction::Propose {
            proposal_kind: "install-tool".into(),
            reason: "Dev's run needs cmake".into(),
            offer: "offer-1".into(),
        };
        assert_eq!(action.op(), "propose");
        assert!(action.check().is_ok());
        let input = action.input();
        assert_eq!(input["kind"], json!("install-tool"));
        assert_eq!(input["offer"], json!("offer-1"));
        assert_eq!(input["reason"], json!("Dev's run needs cmake"));
        assert!(TeamAction::Propose {
            proposal_kind: "add-member".into(),
            reason: "x".into(),
            offer: "offer-1".into(),
        }
        .check()
        .is_err());
        assert!(TeamAction::Propose {
            proposal_kind: "install-tool".into(),
            reason: "".into(),
            offer: "offer-1".into(),
        }
        .check()
        .is_err());
    }

    #[test]
    fn a_kept_working_mode_call_keeps_the_team_settings_for_the_owner() {
        let action = TeamAction::KeepWorkingMode;
        assert_eq!(action.op(), "keep-working-mode");
        assert!(action.check().is_ok());
        assert_eq!(Value::Object(action.input()), json!({"by": "human"}));
    }
}
