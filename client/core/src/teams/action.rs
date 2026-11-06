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
    AddToPhase {
        phase: u64,
        confirm: String,
        #[serde(default)]
        confirmation: String,
        #[serde(default)]
        decline: bool,
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
            Self::AcceptPhase { .. } => "accept-phase",
            Self::PhaseMode { .. } => "phase-mode",
            Self::SendBackPhase { .. } => "send-back-phase",
            Self::DecideTool { .. } => "decide-tool",
            Self::AnswerDeferred { .. } => "answer-deferred",
            Self::Disk { .. } => "disk",
            Self::Release { .. } => "release",
            Self::Retry { .. } => "retry",
            Self::Finish { .. } => "finish",
            Self::Discard => "discard",
            Self::Hold { .. } => "hold",
            Self::SkipStage { .. } => "skip-stage",
            Self::SendBack { .. } => "send-back",
            Self::MoveTask { .. } => "move-task",
            Self::DraftStart { .. }
            | Self::DraftUpdate { .. }
            | Self::DraftSeen { .. }
            | Self::DraftCreate { .. }
            | Self::DraftDiscard { .. } => "draft",
            Self::Settings { .. } => "settings",
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
                ..
            } => {
                if *phase == 0 {
                    return Err("Not a phase number.".into());
                }
                word(confirm, "pending addition")?;
                plain(confirmation, "A confirmation", 200)
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
            Self::Disk { action, scope, paths, path, policy, .. } => {
                if !matches!(action.as_str(), "list" | "clean" | "policy" | "forget") {
                    return Err("List, clean, set the policy or forget.".into());
                }
                if !matches!(scope.as_str(), "" | "policy" | "all") {
                    return Err("Clean what the policy removes, or every folder of Zerus's.".into());
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
            } => {
                put("phase", json!(phase));
                put("confirm", json!(confirm));
                if *decline {
                    put("decline", json!(true));
                } else if !confirmation.is_empty() {
                    put("confirmation", json!(confirmation));
                }
            }
            Self::AcceptPhase { phase } => put("phase", json!(phase)),
            Self::PhaseMode { phase, mode, reason } => {
                put("phase", json!(phase));
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
            Self::Disk { action, scope, dry_run, policy, paths, path, refresh } => {
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
            }
            Self::AnswerDeferred { id, decision, text } => {
                put("id", json!(id));
                put("decision", json!(decision));
                if !text.trim().is_empty() {
                    put("text", json!(text.trim()));
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
                put("lead", lead.clone());
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
                put("set", set.clone());
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
                    .is_some_and(|id| super::read::valid_team(id)),
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
    let (invocation, request_id) = match prepare(call) {
        Ok(prepared) => prepared,
        Err(outcome) => return outcome,
    };
    classify(&call.action, &request_id, crate::run::run(hgs, &invocation))
}
