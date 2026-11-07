//! Compact view models of the team reads. Each is projected once in the core
//! from the checked reply and serialized as it is to the webview, which never
//! parses an `hgs` reply. Texts and lists are bounded; a view that left
//! something out says so (`truncated`).
use serde::Serialize;
use serde_json::{json, Map, Value};

/// The longest name, title or short field kept.
pub const NAME_CHARS: usize = 200;
/// The longest free text kept (a goal, a reason, a role prompt).
pub const TEXT_CHARS: usize = 8000;
/// The longest line kept (a finding, a summary).
pub const LINE_CHARS: usize = 600;
/// Teams one list keeps.
pub const LIST_KEPT: usize = 1000;
/// Members one team keeps.
pub const MEMBERS_KEPT: usize = 256;
/// Tasks one team keeps.
pub const TASKS_KEPT: usize = 5000;
/// Open proposals one team keeps (hgs keeps at most 50).
pub const PROPOSALS_KEPT: usize = 50;
/// Items one inbox, Waiting page, attention list or finding list keeps.
pub const ITEMS_KEPT: usize = 200;
/// The largest proposal change or event detail kept as it is (bytes of JSON).
pub const DETAIL_BYTES: usize = 64 * 1024;

fn cut(text: &str, chars: usize) -> String {
    match text.char_indices().nth(chars) {
        Some((at, _)) => text[..at].to_owned(),
        None => text.to_owned(),
    }
}

fn text(value: &Value, key: &str, chars: usize) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(|text| cut(text, chars))
        .unwrap_or_default()
}

fn name(value: &Value, key: &str) -> String {
    text(value, key, NAME_CHARS)
}

fn number(value: &Value, key: &str) -> f64 {
    value.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

fn count(value: &Value, key: &str) -> u64 {
    value
        .get(key)
        .and_then(|found| {
            found
                .as_u64()
                .or_else(|| found.as_f64().map(|n| n.max(0.0) as u64))
        })
        .unwrap_or(0)
}

fn flag(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn array<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

/// A list of strings, or of objects with `text`, bounded.
fn lines(value: &Value, key: &str, chars: usize) -> Vec<String> {
    array(value, key)
        .iter()
        .take(ITEMS_KEPT)
        .filter_map(|item| {
            item.as_str()
                .map(str::to_owned)
                .or_else(|| item.get("text").and_then(Value::as_str).map(str::to_owned))
        })
        .map(|line| cut(&line, chars))
        .collect()
}

fn numbers(value: &Value, key: &str) -> Vec<u64> {
    array(value, key)
        .iter()
        .take(TASKS_KEPT)
        .filter_map(Value::as_u64)
        .collect()
}

/// A JSON value kept as it is when small, else null.
fn bounded(value: Option<&Value>) -> Value {
    match value {
        Some(found)
            if serde_json::to_string(found).is_ok_and(|json| json.len() <= DETAIL_BYTES) =>
        {
            found.clone()
        }
        _ => Value::Null,
    }
}

/// A member's session: the computer alias (empty: the team's computer) and
/// its name, with the run and conversation it was bound to.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct SessionView {
    pub host: String,
    pub name: String,
    pub run_id: String,
    pub conversation_id: String,
}

fn session(value: &Value) -> Option<SessionView> {
    let found = value.get("session").filter(|session| session.is_object())?;
    let view = SessionView {
        host: name(found, "host"),
        name: name(found, "name"),
        run_id: name(found, "run_id"),
        conversation_id: name(found, "conversation_id"),
    };
    (!view.name.is_empty()).then_some(view)
}

// ---------------------------------------------------------------- the list

/// One member of a listed team, enough to find a session's team.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ListedMember {
    pub id: String,
    pub role: String,
    pub agent: String,
    pub state: String,
    pub lead: bool,
    pub session: Option<SessionView>,
}

/// One team of `team list`.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct TeamSummary {
    pub id: String,
    pub name: String,
    pub status: String,
    pub project: String,
    pub folder: String,
    pub plan_mode: String,
    /// The lead's member id.
    pub lead: String,
    pub members: Vec<ListedMember>,
    pub tasks: u64,
    pub accepted: u64,
    pub submitted: u64,
    pub in_review: u64,
    pub in_test: u64,
    pub attention: u64,
    pub open_proposals: u64,
    pub waiting_for_lead: bool,
    pub pending_phase: Option<u64>,
    pub updated: f64,
    pub revision: u64,
    /// Task 52: the team whose task this sub-team came from ("" for a team
    /// with no parent), so a tree action counts the whole tree from the list.
    pub parent: String,
    /// Task #58: the phases that plan in free mode, from the list's
    /// `phase_modes` (empty from an older hgs).
    pub free_phases: Vec<u64>,
}

impl TeamSummary {
    pub fn draft(&self) -> bool {
        self.status == "draft"
    }
}

/// The teams of `team list`, newest first as the store lists them.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct TeamList {
    pub teams: Vec<TeamSummary>,
    pub truncated: bool,
}

pub fn list(value: &Value) -> TeamList {
    let items = array(value, "items");
    let teams = items
        .iter()
        .take(LIST_KEPT)
        .map(|item| {
            let lead = name(item, "lead");
            TeamSummary {
                id: name(item, "id"),
                name: name(item, "name"),
                status: name(item, "status"),
                project: name(item, "project"),
                folder: text(item, "folder", LINE_CHARS),
                plan_mode: name(item, "plan_mode"),
                members: array(item, "members")
                    .iter()
                    .take(MEMBERS_KEPT)
                    .map(|member| ListedMember {
                        id: name(member, "id"),
                        role: name(member, "role"),
                        agent: name(member, "agent"),
                        state: name(member, "state"),
                        lead: member.get("id").and_then(Value::as_str) == Some(lead.as_str()),
                        session: session(member),
                    })
                    .collect(),
                lead,
                tasks: array(item, "tasks").len() as u64,
                accepted: count(item, "accepted"),
                submitted: count(item, "submitted"),
                in_review: count(item, "in_review"),
                in_test: count(item, "in_test"),
                attention: array(item, "attention").len() as u64,
                open_proposals: array(item, "proposals")
                    .iter()
                    .filter(|proposal| proposal["state"] == "open")
                    .count() as u64,
                waiting_for_lead: flag(item, "waiting_for_lead"),
                pending_phase: item.get("pending_phase").and_then(Value::as_u64),
                updated: number(item, "updated"),
                revision: count(item, "revision"),
                parent: item.get("parent").map(|parent| name(parent, "team")).unwrap_or_default(),
                free_phases: item
                    .get("phase_modes")
                    .and_then(Value::as_object)
                    .map(|modes| {
                        modes
                            .iter()
                            .filter(|(_, mode)| mode.as_str() == Some("free"))
                            .filter_map(|(n, _)| n.parse::<u64>().ok())
                            .take(ITEMS_KEPT)
                            .collect()
                    })
                    .unwrap_or_default(),
            }
        })
        .collect();
    TeamList {
        teams,
        truncated: items.len() > LIST_KEPT,
    }
}

// --------------------------------------------------------------- one team

/// One member of a team, as the Team tab's members column shows it.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct MemberView {
    pub id: String,
    pub role: String,
    pub agent: String,
    pub model: String,
    pub effort: String,
    pub account: String,
    pub computer: String,
    pub folder: String,
    /// `not_created`, `active`, `paused`, `removed`...
    pub state: String,
    /// What its session does now (`busy`, `idle`...), and the hand-off phase.
    pub activity: String,
    pub phase: String,
    pub error: String,
    pub lead: bool,
    pub session: Option<SessionView>,
    /// The session was seen live at the last listing; None: never seen.
    pub live: Option<bool>,
    pub stuck: bool,
    /// The member waits on a question of its own.
    pub question: bool,
    /// Its open task first, then the tasks returned to it.
    pub queue: Vec<u64>,
    /// The owner's mandatory additions to the member's rules, as lines.
    pub iron_rules: Vec<String>,
    pub minutes_without_progress: u64,
    pub host_shared: bool,
    /// Task 98: hand-outs its Terminal refused in a row (a draft, a dialog or
    /// an unknown prompt), hgs's reason, and whether the lead and the owner
    /// were told.
    pub input_refusals: u64,
    pub input_reason: String,
    pub input_raised: bool,
    /// Task 134: the member's unread team messages (status's
    /// `members[].unread`), and how many of them are urgent.
    pub unread: u64,
    pub unread_urgent: u64,
    /// Task 11: the member's pending question or permission request as hgs
    /// sends it (`members[].question`, at most [`DETAIL_BYTES`]): its id,
    /// hash, tool, command, reason and the card's own allow and deny ids.
    pub permission: Value,
    /// Task 133: the member's latest pulse (`members[].pulse`): what it did,
    /// does next and is blocked on, in at most three short lines; None
    /// without one.
    pub pulse: Option<PulseView>,
    /// Task 209: the member's native context reading (`members[].context`):
    /// tokens used, the window and their percentage, so the card shows it
    /// without an inspect of its own. None when its agent keeps no meter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<ContextView>,
    /// Task 209: the member's context is being compacted (status reads its
    /// state `compacting`); the card shows Compacting instead of Working.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub compacting: bool,
}

/// Task 209: a member's native context reading as the member card shows it.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ContextView {
    pub used: u64,
    pub limit: u64,
    /// Whole percent of the window, rounded down by hgs.
    pub percent: u64,
    /// When hgs read it.
    pub at: f64,
}

fn context(value: &Value) -> Option<ContextView> {
    let context = value.get("context").filter(|context| context.is_object())?;
    Some(ContextView {
        used: count(context, "used"),
        limit: count(context, "limit"),
        percent: count(context, "percent").min(100),
        at: number(context, "at"),
    })
}

/// A member's pulse as the Team tab shows it.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct PulseView {
    pub task: Option<u64>,
    pub done: String,
    pub next: String,
    pub blocked: String,
    pub at: f64,
    /// `note` (the member's own) or `auto` (Zerus's summary of the turn).
    pub source: String,
}

fn pulse(value: &Value) -> Option<PulseView> {
    let pulse = value.get("pulse").filter(|pulse| pulse.is_object())?;
    Some(PulseView {
        task: pulse.get("task").and_then(Value::as_u64),
        done: text(pulse, "done", LINE_CHARS),
        next: text(pulse, "next", LINE_CHARS),
        blocked: text(pulse, "blocked", LINE_CHARS),
        at: pulse.get("at").and_then(Value::as_f64).unwrap_or_default(),
        source: name(pulse, "source"),
    })
}

/// One dependency: the task it waits for, hard or soft.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct DepView {
    pub task: u64,
    pub hard: bool,
}

/// One task, as the plan, its cards and the graph show it.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct TaskView {
    pub id: u64,
    pub title: String,
    pub status: String,
    pub assignee: String,
    pub phase: u64,
    pub deps: Vec<DepView>,
    pub estimate_minutes: f64,
    pub attempts: u64,
    /// The newest attempt's state (`delivered`, `reported`...), empty without one.
    pub attempt_state: String,
    /// Task 11: the newest attempt's id, which Use last reply names.
    pub attempt_id: String,
    /// Task 11: the newest attempt's member got no report in (`no_report`).
    pub no_report: bool,
    pub decided_by: String,
    pub pinned: bool,
    pub output: Vec<String>,
    /// What the task waits for, as hgs words it (`waits.unblock`).
    pub waits: String,
    /// Task 40: the owner holds it (`held`).
    pub held: bool,
    /// Task 160: "parked until <condition>, ETA HH:MM (in N min)", as hgs
    /// status words it; empty when the task is not parked.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub parked: String,
    /// Task 40: the pipeline chosen in the plan, and its run (stages and the
    /// open stage) when one started.
    pub pipeline: String,
    pub pipeline_run: Option<PipelineRunView>,
    /// Task 40: the task delegates to a sub-team (`subteam`).
    pub subteam: bool,
    /// Task 52: that sub-team's id, for Cancel's and Reassign's question.
    pub subteam_team: String,
    /// Task #60: the task this follow-up was added for ("spawned from"); it
    /// never blocks, unlike `deps`.
    pub origin: Option<u64>,
    /// Task 55: the newest attempt's open questions to the lead (A3): the
    /// owner answers them with `hgs team reply` too.
    pub questions: Vec<QuestionView>,
}

/// Task 55: one open question of a member to the lead.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct QuestionView {
    pub id: String,
    pub text: String,
    pub at: f64,
}

fn open_questions(value: &Value) -> Vec<QuestionView> {
    let Some(attempt) = array(value, "attempts").last() else {
        return Vec::new();
    };
    array(attempt, "questions")
        .iter()
        .filter(|question| {
            text(question, "answer", 1).is_empty()
                && question["answered_at"].as_f64().unwrap_or(0.0) <= 0.0
        })
        .take(ITEMS_KEPT)
        .map(|question| QuestionView {
            id: name(question, "id"),
            text: text(question, "text", TEXT_CHARS),
            at: number(question, "at"),
        })
        .filter(|question| !question.id.is_empty())
        .collect()
}

/// Task 40: a task's pipeline run (`pipeline_run`): its stage kinds in order,
/// how it is accepted, and the stage that is open now, if any.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct PipelineRunView {
    pub name: String,
    pub accept: String,
    pub round: u64,
    pub max_rounds: u64,
    pub stages: Vec<String>,
    /// The open run's stage index, kind and member (None: no stage runs).
    pub open: Option<StageRunView>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct StageRunView {
    pub stage: u64,
    pub kind: String,
    pub member: String,
    pub state: String,
}

fn pipeline_run(value: &Value) -> Option<PipelineRunView> {
    let run = value.get("pipeline_run").filter(|run| run.is_object())?;
    let last = array(run, "runs").last();
    // A run is open while its state is one hgs keeps open (Qt StageRun::open).
    let open = last
        .filter(|last| matches!(last["state"].as_str(), Some("waiting" | "delivered")))
        .map(|last| StageRunView {
            stage: count(last, "stage"),
            kind: name(last, "kind"),
            member: name(last, "member"),
            state: name(last, "state"),
        });
    Some(PipelineRunView {
        name: name(run, "name"),
        accept: name(run, "accept"),
        round: count(run, "round"),
        max_rounds: count(run, "max_rounds"),
        stages: array(run, "stages")
            .iter()
            .take(ITEMS_KEPT)
            .map(|stage| name(stage, "kind"))
            .collect(),
        open,
    })
}

/// One phase of the plan (`phase_summary`).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct PhaseView {
    pub n: u64,
    pub title: String,
    pub state: String,
    pub status: String,
    pub done: u64,
    pub total: u64,
    pub barrier: bool,
    pub auto: bool,
    pub acceptance: String,
    pub paused: bool,
    pub health: String,
    /// Task #58: the mode the phase plans in, "standard" or "free" (its own,
    /// else the team's); "" from an hgs that sends no phase modes.
    pub mode: String,
}

/// One open proposal, as its card shows it: hgs's own summary when the change
/// carries one (an owner-gated operation), else the kind.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ProposalView {
    pub id: String,
    pub kind: String,
    pub by: String,
    pub at: f64,
    pub reason: String,
    /// The content hash an answer names (`expected_hash`).
    pub hash: String,
    pub summary: String,
    pub lines: Vec<String>,
    /// The exact change (at most [`DETAIL_BYTES`]), for the kinds that list it.
    pub change: Value,
    pub member: Value,
    /// Task 11: a next goal's text and a plan mode switch's mode, which the
    /// proposal carries beside its change.
    pub goal: String,
    pub mode: String,
}

/// One preflight finding.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct FindingView {
    pub kind: String,
    pub text: String,
    pub task: Option<u64>,
    pub member: String,
}

fn findings(value: Option<&Value>, key: &str) -> Vec<FindingView> {
    value
        .map(|found| array(found, key))
        .unwrap_or(&[])
        .iter()
        .take(ITEMS_KEPT)
        .map(|finding| match finding {
            Value::String(line) => FindingView {
                text: cut(line, LINE_CHARS),
                ..Default::default()
            },
            _ => FindingView {
                kind: name(finding, "kind"),
                text: text(finding, "text", LINE_CHARS),
                task: finding.get("task").and_then(Value::as_u64),
                member: name(finding, "member"),
            },
        })
        .collect()
}

/// The preflight: what blocks Create team or Approve, and what only warns.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Validation {
    pub errors: Vec<FindingView>,
    pub warnings: Vec<FindingView>,
}

/// The plan's health line: its critical path, parallelism and warnings.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct HealthView {
    pub critical_path: u64,
    pub parallelism: f64,
    pub warnings: Vec<String>,
}

/// A draft's own state: its revision, the fields the lead changed that the
/// owner has not seen, and the ones the owner edited.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct DraftView {
    pub revision: u64,
    pub unseen: Vec<String>,
    pub owner_edited: Vec<String>,
}

/// `team status` for the Team tab.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct TeamView {
    pub id: String,
    pub name: String,
    pub status: String,
    pub project: String,
    pub folder: String,
    pub goal: String,
    pub plan_mode: String,
    /// Task #58: the phase that takes tasks as they come (`free_phase`), if any.
    pub free_phase: Option<u64>,
    /// Task #58: whether hgs sends each phase's mode (`phase_modes`), so the
    /// owner can switch one phase (`phase-mode`).
    pub phase_modes_known: bool,
    pub lead: String,
    pub revision: u64,
    pub created: f64,
    pub updated: f64,
    pub waiting_for_lead: bool,
    pub pending_phase: Option<u64>,
    /// Task 112: a paused team's pause ({at, by, reason}), or null.
    pub paused: Value,
    /// The team's settings object as hgs stores it (a few dozen fields).
    pub settings: Value,
    pub members: Vec<MemberView>,
    pub tasks: Vec<TaskView>,
    pub phases: Vec<PhaseView>,
    pub proposals: Vec<ProposalView>,
    /// The attention items as hgs words them (bounded).
    pub attention: Vec<Value>,
    /// Task 11: additions to a running phase that wait for the owner's
    /// confirmation (`pending_additions`, each bounded): id, phase, tasks.
    pub additions: Vec<Value>,
    /// Task 40: the plan's permission widenings ({member, role, from, to}),
    /// which Approve & Run confirms separately (Qt Team::widenings).
    pub widenings: Vec<Value>,
    /// Task 40: the team's direct sub-teams ({team, name, status}), for the
    /// tree confirmations of Pause and Stop.
    pub children: Vec<Value>,
    pub validation: Validation,
    pub health: HealthView,
    pub ready: Vec<u64>,
    pub cycle: Vec<u64>,
    pub draft: Option<DraftView>,
    /// Members, tasks or proposals over their caps were left out.
    pub truncated: bool,
}

fn member(value: &Value, lead: &str) -> MemberView {
    MemberView {
        id: name(value, "id"),
        role: name(value, "role"),
        agent: name(value, "agent"),
        model: name(value, "model"),
        effort: name(value, "effort"),
        account: name(value, "account"),
        computer: name(value, "computer"),
        folder: name(value, "folder"),
        state: name(value, "state"),
        activity: name(value, "activity"),
        phase: name(value, "phase"),
        error: text(value, "error", LINE_CHARS),
        lead: value.get("id").and_then(Value::as_str) == Some(lead),
        session: session(value),
        live: value
            .get("seen")
            .and_then(|seen| seen.get("live"))
            .and_then(Value::as_bool),
        stuck: value
            .get("stuck")
            .is_some_and(|stuck| !stuck.is_null() && stuck != &Value::Bool(false)),
        question: value
            .get("question")
            .is_some_and(|question| !question.is_null()),
        queue: array(value, "queue")
            .iter()
            .take(ITEMS_KEPT)
            .filter_map(|entry| {
                entry
                    .as_u64()
                    .or_else(|| entry.get("task").and_then(Value::as_u64))
            })
            .collect(),
        iron_rules: lines(value, "iron_rules", TEXT_CHARS),
        minutes_without_progress: count(value, "minutes_without_progress"),
        host_shared: flag(value, "host_shared"),
        input_refusals: value.get("input_block").map_or(0, |block| count(block, "count")),
        input_reason: value.get("input_block").map_or_else(String::new, |block| text(block, "reason", LINE_CHARS)),
        input_raised: value.get("input_block").is_some_and(|block| flag(block, "raised")),
        unread: value.get("unread").map_or(0, |unread| count(unread, "count")),
        unread_urgent: value.get("unread").map_or(0, |unread| count(unread, "urgent")),
        permission: bounded(
            value
                .get("question")
                .filter(|question| question.is_object()),
        ),
        pulse: pulse(value),
        context: context(value),
        compacting: value.get("state").and_then(Value::as_str) == Some("compacting")
            || value.get("compacting").is_some_and(Value::is_object),
    }
}

fn task(value: &Value) -> TaskView {
    let attempts = array(value, "attempts");
    TaskView {
        id: count(value, "id"),
        title: name(value, "title"),
        status: name(value, "status"),
        assignee: name(value, "assignee"),
        phase: count(value, "phase"),
        deps: array(value, "deps")
            .iter()
            .take(ITEMS_KEPT)
            .filter_map(|dep| {
                let task = dep
                    .as_u64()
                    .or_else(|| dep.get("task").and_then(Value::as_u64))?;
                Some(DepView {
                    task,
                    hard: dep.get("kind").and_then(Value::as_str) != Some("soft"),
                })
            })
            .collect(),
        estimate_minutes: number(value, "estimate_minutes"),
        attempts: attempts.len() as u64,
        attempt_state: attempts
            .last()
            .map(|attempt| name(attempt, "state"))
            .unwrap_or_default(),
        attempt_id: attempts
            .last()
            .map(|attempt| name(attempt, "id"))
            .unwrap_or_default(),
        no_report: attempts
            .last()
            .is_some_and(|attempt| flag(attempt, "no_report")),
        decided_by: name(value, "decided_by"),
        pinned: flag(value, "pinned"),
        output: lines(value, "output", LINE_CHARS),
        waits: value
            .get("waits")
            .map(|waits| text(waits, "unblock", LINE_CHARS))
            .unwrap_or_default(),
        held: flag(value, "held"),
        parked: text(value, "parked", LINE_CHARS),
        pipeline: name(value, "pipeline"),
        pipeline_run: pipeline_run(value),
        subteam: value.get("subteam").is_some_and(Value::is_object),
        subteam_team: value.get("subteam").map(|subteam| name(subteam, "team")).unwrap_or_default(),
        origin: value.get("origin").and_then(Value::as_u64).filter(|origin| *origin > 0),
        questions: open_questions(value),
    }
}

/// Task #58: phase `n`'s mode from status's `phase_modes`, else the team's
/// `plan_mode` for a phase it does not name; "" from an older hgs.
fn phase_mode(team: &Value, n: u64) -> String {
    let Some(modes) = team.get("phase_modes").and_then(Value::as_object) else {
        return String::new();
    };
    match modes.get(&n.to_string()).and_then(Value::as_str) {
        Some(mode @ ("free" | "standard")) => mode.to_owned(),
        _ if name(team, "plan_mode") == "free" => "free".into(),
        _ => "standard".into(),
    }
}

fn phase(value: &Value) -> PhaseView {
    PhaseView {
        n: count(value, "n"),
        title: name(value, "title"),
        state: name(value, "state"),
        status: name(value, "status"),
        done: count(value, "done"),
        total: count(value, "total"),
        barrier: flag(value, "barrier"),
        auto: flag(value, "auto"),
        acceptance: name(value, "acceptance"),
        paused: value.get("pause").is_some_and(|pause| !pause.is_null()),
        health: text(value, "health_line", LINE_CHARS),
        mode: String::new(),
    }
}

fn proposal(value: &Value) -> ProposalView {
    let change = value.get("change").unwrap_or(&Value::Null);
    let summary = text(change, "summary", LINE_CHARS);
    ProposalView {
        id: name(value, "id"),
        kind: name(value, "kind"),
        by: name(value, "by"),
        at: number(value, "at"),
        reason: text(value, "reason", TEXT_CHARS),
        hash: name(value, "hash"),
        summary: if summary.is_empty() {
            name(value, "kind")
        } else {
            summary
        },
        lines: lines(change, "lines", LINE_CHARS),
        change: bounded(value.get("change")),
        member: bounded(value.get("member")),
        goal: text(value, "goal", TEXT_CHARS),
        mode: name(value, "mode"),
    }
}

/// Task 40: the plan's permission widenings, as Qt reads them: each member's
/// `profile_widening`, then the team's own (an object or a list).
fn widenings(value: &Value) -> Vec<Value> {
    let one = |widening: &Value, member: &str, role: &str| {
        let from = name(widening, "from");
        let to = name(widening, "to");
        (!from.is_empty() || !to.is_empty()).then(|| {
            let named = name(widening, "member");
            json!({"member": if named.is_empty() { member.to_owned() } else { named },
                "role": if role.is_empty() { name(widening, "role") } else { role.to_owned() }, "from": from, "to": to})
        })
    };
    let mut out: Vec<Value> = array(value, "members")
        .iter()
        .filter_map(|member| {
            one(
                &member["profile_widening"],
                &name(member, "id"),
                &name(member, "role"),
            )
        })
        .collect();
    match value.get("profile_widening") {
        Some(Value::Array(list)) => {
            out.extend(list.iter().filter_map(|widening| one(widening, "", "")))
        }
        Some(widening) if widening.is_object() => out.extend(one(widening, "", "")),
        _ => {}
    }
    out.truncate(ITEMS_KEPT);
    out
}

/// Projects `team status` for the Team tab.
pub fn team(value: &Value) -> TeamView {
    let lead = name(value, "lead");
    let members = array(value, "members");
    let tasks = array(value, "tasks");
    let open: Vec<&Value> = array(value, "proposals")
        .iter()
        .filter(|proposal| proposal["state"] == "open")
        .collect();
    let validation = value.get("validation");
    let health = value
        .get("health")
        .or_else(|| validation.and_then(|found| found.get("health")));
    TeamView {
        id: name(value, "id"),
        name: name(value, "name"),
        status: name(value, "status"),
        project: name(value, "project"),
        folder: text(value, "folder", LINE_CHARS),
        goal: text(value, "goal", TEXT_CHARS),
        plan_mode: name(value, "plan_mode"),
        free_phase: value.get("free_phase").and_then(|free| free.get("phase")).and_then(Value::as_u64),
        phase_modes_known: value.get("phase_modes").is_some_and(Value::is_object),
        revision: count(value, "revision"),
        created: number(value, "created"),
        updated: number(value, "updated"),
        waiting_for_lead: flag(value, "waiting_for_lead"),
        pending_phase: value.get("pending_phase").and_then(Value::as_u64),
        paused: value.get("paused").filter(|paused| paused.is_object()).map_or(Value::Null, |paused| bounded(Some(paused))),
        settings: match value.get("settings") {
            Some(Value::Object(settings)) => Value::Object(settings.clone()),
            _ => Value::Object(Map::new()),
        },
        members: members
            .iter()
            .take(MEMBERS_KEPT)
            .map(|found| member(found, &lead))
            .collect(),
        tasks: tasks.iter().take(TASKS_KEPT).map(task).collect(),
        phases: array(value, "phase_summary")
            .iter()
            .take(ITEMS_KEPT)
            .map(|found| {
                let mut view = phase(found);
                view.mode = phase_mode(value, view.n);
                view
            })
            .collect(),
        proposals: open
            .iter()
            .take(PROPOSALS_KEPT)
            .map(|found| proposal(found))
            .collect(),
        attention: array(value, "attention")
            .iter()
            .take(ITEMS_KEPT)
            .map(|item| bounded(Some(item)))
            .collect(),
        additions: array(value, "pending_additions")
            .iter()
            .take(ITEMS_KEPT)
            .map(|item| bounded(Some(item)))
            .collect(),
        widenings: widenings(value),
        children: array(value, "children")
            .iter()
            .take(ITEMS_KEPT)
            .map(|child| json!({"team": name(child, "team"), "name": name(child, "name"), "status": name(child, "status")}))
            .collect(),
        validation: Validation {
            errors: findings(validation, "errors"),
            warnings: findings(validation, "warnings"),
        },
        health: HealthView {
            critical_path: health
                .map(|found| count(found, "critical_path"))
                .unwrap_or(0),
            parallelism: health
                .map(|found| number(found, "parallelism"))
                .unwrap_or(0.0),
            warnings: health
                .map(|found| lines(found, "warnings", LINE_CHARS))
                .unwrap_or_default(),
        },
        ready: numbers(value, "ready"),
        cycle: numbers(value, "cycle"),
        draft: value
            .get("draft")
            .filter(|draft| draft.is_object())
            .map(|draft| DraftView {
                revision: count(draft, "revision"),
                unseen: lines(draft, "unseen", NAME_CHARS),
                owner_edited: lines(draft, "owner_edited", NAME_CHARS),
            }),
        truncated: members.len() > MEMBERS_KEPT
            || tasks.len() > TASKS_KEPT
            || open.len() > PROPOSALS_KEPT,
        lead,
    }
}

// -------------------------------------------------------------- the graph

/// One task of the plan graph, in its layer.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct GraphNode {
    pub id: u64,
    pub title: String,
    pub status: String,
    pub assignee: String,
    /// The assignee's role, the lead's "Lead".
    pub role: String,
    pub phase: u64,
    /// The column the graph draws it in (hgs's `layers`).
    pub layer: u64,
    pub ready: bool,
    pub in_cycle: bool,
    /// Task #60: the task it was spawned from, when the graph has it; drawn
    /// as a link, never as a dependency edge.
    pub origin: Option<u64>,
}

/// One edge: `to` depends on `from`.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct GraphEdge {
    pub from: u64,
    pub to: u64,
    pub hard: bool,
}

/// The plan graph of one team: its tasks in layers and their dependencies.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct PlanGraph {
    pub team: String,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    /// How many layers the nodes take.
    pub layers: u64,
}

/// Projects the plan graph from `team status` (its `layers`) and its view.
pub fn graph(value: &Value, view: &TeamView) -> PlanGraph {
    let layers = value.get("layers").and_then(Value::as_object);
    let role = |id: &str| -> String {
        if id == view.lead {
            return "Lead".to_owned();
        }
        view.members
            .iter()
            .find(|member| member.id == id)
            .map(|member| member.role.clone())
            .unwrap_or_default()
    };
    let known: std::collections::BTreeSet<u64> = view.tasks.iter().map(|task| task.id).collect();
    let nodes: Vec<GraphNode> = view
        .tasks
        .iter()
        .map(|task| GraphNode {
            id: task.id,
            title: task.title.clone(),
            status: task.status.clone(),
            assignee: task.assignee.clone(),
            role: role(&task.assignee),
            phase: task.phase,
            layer: layers
                .and_then(|layers| layers.get(&task.id.to_string()))
                .and_then(Value::as_u64)
                .unwrap_or(0),
            ready: view.ready.contains(&task.id),
            in_cycle: view.cycle.contains(&task.id),
            origin: task.origin.filter(|origin| known.contains(origin)),
        })
        .collect();
    let edges = view
        .tasks
        .iter()
        .flat_map(|task| {
            task.deps
                .iter()
                .filter(|dep| known.contains(&dep.task))
                .map(move |dep| GraphEdge {
                    from: dep.task,
                    to: task.id,
                    hard: dep.hard,
                })
        })
        .collect();
    PlanGraph {
        team: view.id.clone(),
        layers: nodes.iter().map(|node| node.layer + 1).max().unwrap_or(0),
        nodes,
        edges,
    }
}

// ---------------------------------------------------------------- New team

/// One member of a draft, with what New team edits.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct DraftMember {
    pub id: String,
    pub role: String,
    pub agent: String,
    pub model: String,
    pub effort: String,
    pub account: String,
    pub permissions: String,
    pub computer: String,
    pub folder: String,
    pub prompt: String,
    pub lead: bool,
    pub session: Option<SessionView>,
}

/// A draft team, as the New team form shows it.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct NewTeamView {
    pub team: String,
    pub name: String,
    pub goal: String,
    /// The lead protocol addendum.
    pub addendum: String,
    pub folder: String,
    pub settings: Value,
    pub members: Vec<DraftMember>,
    pub tasks: Vec<TaskView>,
    pub draft: DraftView,
    pub validation: Validation,
    pub updated: f64,
}

/// Projects a draft's `team status` for New team; None for a team that is
/// not a draft.
pub fn new_team(value: &Value, view: &TeamView) -> Option<NewTeamView> {
    if view.status != "draft" {
        return None;
    }
    Some(NewTeamView {
        team: view.id.clone(),
        name: view.name.clone(),
        goal: view.goal.clone(),
        addendum: text(value, "lead_protocol", TEXT_CHARS),
        folder: view.folder.clone(),
        settings: view.settings.clone(),
        members: array(value, "members")
            .iter()
            .take(MEMBERS_KEPT)
            .map(|member| DraftMember {
                id: name(member, "id"),
                role: name(member, "role"),
                agent: name(member, "agent"),
                model: name(member, "model"),
                effort: name(member, "effort"),
                account: name(member, "account"),
                permissions: name(member, "permissions"),
                computer: name(member, "computer"),
                folder: name(member, "folder"),
                prompt: text(member, "prompt", TEXT_CHARS),
                lead: member.get("id").and_then(Value::as_str) == Some(view.lead.as_str()),
                session: session(member),
            })
            .collect(),
        tasks: view.tasks.clone(),
        draft: view.draft.clone().unwrap_or_default(),
        validation: view.validation.clone(),
        updated: view.updated,
    })
}

// ----------------------------------------------------------- feed and more

/// One journal event of the team's feed.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct FeedEvent {
    pub seq: i64,
    pub at: f64,
    pub kind: String,
    pub detail: Value,
}

/// One page of `team events`: its events, newest first, and the cursor of the
/// page before them (None: the journal's start).
pub fn feed_page(value: &Value) -> (Vec<FeedEvent>, Option<i64>) {
    let events = array(value, "items")
        .iter()
        .filter_map(|item| {
            Some(FeedEvent {
                seq: item.get("seq").and_then(Value::as_i64)?,
                at: number(item, "at"),
                kind: name(item, "kind"),
                detail: bounded(item.get("detail")),
            })
        })
        .collect();
    (
        events,
        value
            .get("next_cursor")
            .and_then(Value::as_i64)
            .filter(|cursor| *cursor > 0),
    )
}

/// The feed the view holds: the newest events first, at most
/// [`crate::teams::FEED_KEPT`].
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct FeedView {
    pub team: String,
    pub events: Vec<FeedEvent>,
    /// More events before the oldest held: Load older can ask for them.
    pub more: bool,
    /// Older events were left out to keep the feed bounded.
    pub capped: bool,
}

/// One item of the lead's inbox.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct InboxItem {
    pub kind: String,
    pub member: String,
    pub task: Option<u64>,
    pub summary: String,
    pub since: f64,
    pub command: String,
    pub detail: Value,
}

/// `team inbox`.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct InboxView {
    pub team: String,
    pub items: Vec<InboxItem>,
    pub truncated: bool,
}

pub fn inbox(team: &str, value: &Value) -> InboxView {
    let items = array(value, "items");
    InboxView {
        team: team.to_owned(),
        items: items
            .iter()
            .take(ITEMS_KEPT)
            .map(|item| InboxItem {
                kind: name(item, "kind"),
                member: name(item, "member"),
                task: item.get("task").and_then(Value::as_u64),
                summary: text(item, "summary", LINE_CHARS),
                since: number(item, "since"),
                command: text(item, "command", LINE_CHARS),
                detail: bounded(item.get("detail")),
            })
            .collect(),
        truncated: items.len() > ITEMS_KEPT,
    }
}

/// One thing that waits for the owner.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct WaitingItem {
    pub id: String,
    pub kind: String,
    pub team: String,
    pub team_name: String,
    pub title: String,
    pub action: String,
    pub since: f64,
    pub proposal: Value,
    /// Task 11: the whole item as hgs lists it (at most [`DETAIL_BYTES`]):
    /// its phase, task, member, member id or tool offer, which an answer in
    /// place names.
    pub detail: Value,
}

/// The first page of `team waiting`, with how many wait in all.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct WaitingView {
    pub items: Vec<WaitingItem>,
    pub total: u64,
    /// More items wait after this page.
    pub more: bool,
    /// The cursor the next older page sends, as hgs words it.
    pub next: Option<Value>,
}

pub fn waiting(value: &Value) -> WaitingView {
    WaitingView {
        items: array(value, "items")
            .iter()
            .take(ITEMS_KEPT)
            .map(|item| WaitingItem {
                id: name(item, "id"),
                kind: name(item, "kind"),
                team: name(item, "team"),
                team_name: name(item, "team_name"),
                title: text(item, "title", LINE_CHARS),
                action: name(item, "action"),
                since: number(item, "since"),
                proposal: bounded(item.get("proposal")),
                detail: bounded(Some(item)),
            })
            .collect(),
        total: count(value, "total"),
        more: value.get("next").is_some_and(|next| !next.is_null()),
        next: value
            .get("next")
            .filter(|next| next.is_object())
            .cloned(),
    }
}

/// One page of `team spans`: each task's spans as hgs gives them, and the
/// task after which the next page starts.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct SpansView {
    pub team: String,
    pub tasks: Vec<Value>,
    pub next: Option<u64>,
    pub truncated: bool,
}

pub fn spans(team: &str, value: &Value) -> SpansView {
    SpansView {
        team: team.to_owned(),
        tasks: array(value, "tasks")
            .iter()
            .take(ITEMS_KEPT)
            .map(|task| bounded(Some(task)))
            .collect(),
        next: value.get("next_cursor").and_then(Value::as_u64),
        truncated: value
            .get("events")
            .is_some_and(|events| flag(events, "truncated")),
    }
}

/// One loss the profile names (a rework, a wait for a member, the lead or a
/// dependency); `tokens` is null when hgs has no count, never zero.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ProfileLossView {
    pub kind: String,
    pub task: u64,
    pub seconds: f64,
    pub tokens: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ProfileRoleView {
    pub key: String,
    pub kind: String,
    pub accepted: u64,
    pub attempts: u64,
    pub returned: u64,
    pub tokens_per_accepted: Option<u64>,
}

/// `team profile`, projected: the summary, the top time losses and the roles
/// and models. Unknown token counts stay null.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ProfileView {
    pub team: String,
    pub elapsed_s: Option<u64>,
    pub worked_s: Option<u64>,
    pub tasks: u64,
    pub accepted: u64,
    pub tokens: Option<u64>,
    pub tokens_partial: bool,
    pub losses: Vec<ProfileLossView>,
    pub roles: Vec<ProfileRoleView>,
    /// Task 82: each task's lateness against the plan and overrun against
    /// its estimate, from the profile's first page of task rows.
    pub task_rows: Vec<ProfileTaskView>,
    /// More task rows follow the first page; their tasks show no heat.
    pub task_rows_more: bool,
}

/// Task 82: a task's lateness against the plan baseline. `known` is false
/// when the plan lacks a moment it needs (an estimate is missing); `from` is
/// the predecessor the inherited part comes from.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ProfileLatenessView {
    pub known: bool,
    pub unknown: String,
    pub planned_start: Option<f64>,
    pub planned_end: Option<f64>,
    pub actual_start: Option<f64>,
    pub actual_end: Option<f64>,
    pub start_late_s: i64,
    pub inherited_s: i64,
    pub own_s: i64,
    pub from: Option<u64>,
}

/// Task 82: a task's own work against its estimate; `known` is false
/// without an estimate, `started` false before its first attempt.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ProfileOverrunView {
    pub started: bool,
    pub known: bool,
    pub estimate_s: i64,
    pub work_s: i64,
    pub overrun_s: i64,
    pub ratio: f64,
    pub running: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ProfileTaskView {
    pub task: u64,
    /// Null when the task has no plan baseline (planned before it existed).
    pub lateness: Option<ProfileLatenessView>,
    pub overrun: Option<ProfileOverrunView>,
}

fn profile_task(row: &Value) -> Option<ProfileTaskView> {
    let task = row
        .get("task")
        .and_then(Value::as_u64)
        .filter(|task| *task > 0)?;
    let int = |value: &Value, key: &str| value.get(key).and_then(Value::as_i64);
    let moment = |value: &Value, key: &str| value.get(key).and_then(Value::as_f64);
    let lateness = row
        .get("lateness")
        .filter(|late| late.is_object())
        .map(|late| {
            let unknown = late
                .get("unknown")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            ProfileLatenessView {
                known: unknown.is_empty() && int(late, "start_late_s").is_some(),
                unknown,
                planned_start: moment(late, "planned_start"),
                planned_end: moment(late, "planned_end"),
                actual_start: moment(late, "actual_start"),
                actual_end: moment(late, "actual_end"),
                start_late_s: int(late, "start_late_s").unwrap_or(0),
                inherited_s: int(late, "inherited_s").unwrap_or(0),
                own_s: int(late, "own_s").unwrap_or(0),
                from: late.get("from").and_then(Value::as_u64),
            }
        });
    let overrun = row
        .get("overrun")
        .filter(|over| over.is_object())
        .map(|over| ProfileOverrunView {
            started: !flag(over, "not_started"),
            known: int(over, "overrun_s").is_some() && int(over, "estimate_s").is_some(),
            estimate_s: int(over, "estimate_s").unwrap_or(0),
            work_s: int(over, "work_s").unwrap_or(0),
            overrun_s: int(over, "overrun_s").unwrap_or(0),
            ratio: over.get("ratio").and_then(Value::as_f64).unwrap_or(0.0),
            running: flag(over, "running"),
        });
    Some(ProfileTaskView {
        task,
        lateness,
        overrun,
    })
}

pub fn profile(team: &str, value: &Value) -> ProfileView {
    let summary = value.get("summary").unwrap_or(&Value::Null);
    let tokens = summary.get("tokens").unwrap_or(&Value::Null);
    let total = tokens.get("total").and_then(Value::as_u64);
    let roles = [
        ("role", array(value, "roles")),
        ("model", array(value, "models")),
    ]
    .into_iter()
    .flat_map(|(kind, rows)| {
        rows.iter().take(PROPOSALS_KEPT).map(move |row| {
            let role = name(row, "role");
            let model = name(row, "model");
            ProfileRoleView {
                key: if !role.is_empty() { role } else if !model.is_empty() { model } else { name(row, "key") },
                kind: kind.to_owned(),
                accepted: row.get("accepted").and_then(Value::as_u64).unwrap_or(0),
                attempts: row.get("attempts").and_then(Value::as_u64).unwrap_or(0),
                returned: row.get("returned").and_then(Value::as_u64).unwrap_or(0),
                tokens_per_accepted: row.get("tokens_per_accepted").and_then(Value::as_u64),
            }
        })
    })
    .collect();
    ProfileView {
        team: team.to_owned(),
        elapsed_s: summary
            .get("totals")
            .and_then(|totals| totals.get("elapsed_s"))
            .and_then(Value::as_u64),
        worked_s: summary
            .get("totals")
            .and_then(|totals| totals.get("worked_s"))
            .and_then(Value::as_u64),
        tasks: summary.get("tasks").and_then(Value::as_u64).unwrap_or(0),
        accepted: summary.get("accepted").and_then(Value::as_u64).unwrap_or(0),
        tokens: total,
        tokens_partial: tokens
            .get("partial")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        losses: array(
            value
                .get("losses")
                .and_then(|losses| losses.get("by_time"))
                .unwrap_or(&Value::Null),
            "items",
        )
        .iter()
        .take(ITEMS_KEPT)
        .map(|loss| ProfileLossView {
            kind: name(loss, "kind"),
            task: loss.get("task").and_then(Value::as_u64).unwrap_or(0),
            seconds: number(loss, "s"),
            tokens: loss.get("tokens").and_then(Value::as_u64),
        })
        .collect(),
        roles,
        task_rows: array(value.get("tasks").unwrap_or(&Value::Null), "items")
            .iter()
            .take(ITEMS_KEPT)
            .filter_map(profile_task)
            .collect(),
        task_rows_more: value
            .get("tasks")
            .and_then(|tasks| tasks.get("next_cursor"))
            .is_some_and(|next| !next.is_null()),
    }
}

/// One test request still in the queue.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct TestPendingView {
    pub id: String,
    pub suite: String,
    pub asked_by: String,
    pub state: String,
    pub task: u64,
    pub forced: bool,
    pub started: f64,
    pub seconds: f64,
    pub reason: String,
}

/// One recorded test run.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct TestRunView {
    pub id: String,
    pub suite: String,
    pub status: String,
    pub asked_by: String,
    pub task: u64,
    pub at: f64,
    pub duration_ms: u64,
    pub recorded_ms: u64,
    pub runner: String,
    pub excerpt: String,
    pub flags: Vec<String>,
    pub reused_from: String,
    pub due: bool,
    pub over_budget: bool,
    /// Task 88: one result per test from the provider's report (counts,
    /// note and at most `REPORT_RESULTS` results, failures first), or null.
    pub report: Value,
}

/// Task 88: the results of one run the Tests mode shows; the counts cover the rest.
const REPORT_RESULTS: usize = 200;

/// Task 88: a run's report as the view carries it, every text bounded.
fn test_report(value: Option<&Value>) -> Value {
    let Some(report) = value.filter(|report| report.is_object()) else { return Value::Null };
    let results: Vec<Value> = report["results"]
        .as_array()
        .into_iter()
        .flatten()
        .take(REPORT_RESULTS)
        .map(|result| {
            json!({"suite":text(result, "suite", LINE_CHARS),"name":text(result, "name", LINE_CHARS),"status":name(result, "status"),
                "ms":result.get("ms").and_then(Value::as_u64).unwrap_or(0),"message":text(result, "message", LINE_CHARS)})
        })
        .collect();
    let count = |key: &str| report.get(key).and_then(Value::as_u64).unwrap_or(0);
    json!({"passed":count("passed"),"failed":count("failed"),"skipped":count("skipped"),"errors":count("errors"),
        "note":text(report, "note", LINE_CHARS),"results":results})
}

/// `team test` for the Tests mode: the queue and one page of results.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct TestsView {
    pub team: String,
    pub queue: Vec<TestPendingView>,
    pub runs: Vec<TestRunView>,
    pub next: Option<f64>,
}

pub fn tests(team: &str, queue: &Value, results: &Value) -> TestsView {
    TestsView {
        team: team.to_owned(),
        queue: array(queue, "items")
            .iter()
            .take(ITEMS_KEPT)
            .map(|item| TestPendingView {
                id: name(item, "id"),
                suite: name(item, "suite"),
                asked_by: name(item, "asked_by"),
                state: name(item, "state"),
                task: item.get("task").and_then(Value::as_u64).unwrap_or(0),
                forced: flag(item, "forced"),
                started: number(item, "started"),
                seconds: number(item, "seconds"),
                reason: text(item, "reason", LINE_CHARS),
            })
            .collect(),
        runs: array(results, "items")
            .iter()
            .take(ITEMS_KEPT)
            .map(|run| TestRunView {
                id: name(run, "id"),
                suite: name(run, "suite"),
                status: name(run, "status"),
                asked_by: name(run, "asked_by"),
                task: run.get("task").and_then(Value::as_u64).unwrap_or(0),
                at: number(run, "at"),
                duration_ms: run.get("duration_ms").and_then(Value::as_u64).unwrap_or(0),
                recorded_ms: run.get("recorded_ms").and_then(Value::as_u64).unwrap_or(0),
                runner: name(run, "runner"),
                excerpt: text(run, "excerpt", TEXT_CHARS),
                flags: lines(run, "flags", NAME_CHARS),
                reused_from: name(run, "reused_from"),
                due: flag(run, "due"),
                over_budget: flag(run, "over_budget"),
                report: test_report(run.get("report")),
            })
            .collect(),
        next: results.get("next_cursor").and_then(Value::as_f64),
    }
}
/// One catalog role as the editor lists it.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct CatalogRoleView {
    pub id: String,
    pub name: String,
    pub agent: String,
    pub model: String,
    pub prompt: String,
    /// Task 42: what New team copies into a member of this role.
    pub effort: String,
    pub account: String,
    pub permissions: String,
    pub computer: String,
    /// `worktree`, `shared` or `read_only`: the role's folder.
    pub folder: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct CatalogSeatView {
    pub role: String,
    pub count: u64,
    pub optional: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct CatalogTemplateView {
    pub id: String,
    pub name: String,
    pub description: String,
    pub planning: String,
    pub approval: String,
    pub max_members: u64,
    pub pipeline: String,
    pub roster: Vec<CatalogSeatView>,
    /// Task 42: the approval of later phases and the lead protocol New team
    /// takes from the template.
    pub phase_approval: String,
    pub lead_protocol: String,
    /// Task 42: the template's tasks (Use template tasks): title, the role
    /// that does it and the indexes of the tasks it waits for.
    pub tasks: Vec<CatalogTaskView>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct CatalogTaskView {
    pub title: String,
    pub role: String,
    pub deps: Vec<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct CatalogRuleView {
    pub id: String,
    pub when: String,
    pub template: String,
    pub r#use: String,
    pub approval: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct CatalogStageView {
    pub kind: String,
    pub role: String,
    pub member: String,
    pub command: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct CatalogPipelineView {
    pub id: String,
    pub name: String,
    pub stages: Vec<CatalogStageView>,
    pub max_rounds: u64,
    pub folder: String,
    pub accept: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct CatalogProposalView {
    pub id: String,
    pub kind: String,
    pub action: String,
    pub target: String,
    pub title: String,
    pub reason: String,
    pub state: String,
    pub value: Value,
    pub text: String,
    pub at: f64,
}

/// `team catalog get`: the saved catalog and the designer's draft. Roles,
/// templates, rules, pipelines and proposals are bounded; texts and changes
/// are capped, and the webview never parses an hgs reply.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct CatalogView {
    pub version: u64,
    pub builtin: bool,
    pub roles: Vec<CatalogRoleView>,
    pub templates: Vec<CatalogTemplateView>,
    pub routing: Vec<CatalogRuleView>,
    pub planning_rules: String,
    pub pipelines: Vec<CatalogPipelineView>,
    pub draft_base: u64,
    pub proposals: Vec<CatalogProposalView>,
}

pub fn catalog(value: &Value) -> CatalogView {
    let catalog = value.get("catalog").unwrap_or(&Value::Null);
    let draft = value.get("draft").unwrap_or(&Value::Null);
    CatalogView {
        version: catalog.get("version").and_then(Value::as_u64).unwrap_or(0),
        builtin: value
            .get("builtin")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        roles: array(catalog, "roles")
            .iter()
            .take(PROPOSALS_KEPT)
            .map(|role| CatalogRoleView {
                id: name(role, "id"),
                name: name(role, "name"),
                agent: name(role, "agent"),
                model: name(role, "model"),
                prompt: text(role, "prompt", TEXT_CHARS),
                effort: name(role, "effort"),
                account: name(role, "account"),
                permissions: name(role, "permissions"),
                computer: name(role, "computer"),
                folder: name(role, "folder"),
            })
            .collect(),
        templates: array(catalog, "templates")
            .iter()
            .take(PROPOSALS_KEPT)
            .map(|template| CatalogTemplateView {
                id: name(template, "id"),
                name: name(template, "name"),
                description: text(template, "description", TEXT_CHARS),
                planning: name(template, "planning"),
                approval: name(template, "approval"),
                max_members: template
                    .get("max_members")
                    .and_then(Value::as_u64)
                    .unwrap_or(6),
                pipeline: name(template, "pipeline"),
                phase_approval: name(template, "phase_approval"),
                lead_protocol: text(template, "lead_protocol", TEXT_CHARS),
                tasks: array(template, "tasks")
                    .iter()
                    .take(TASKS_KEPT)
                    .map(|task| CatalogTaskView {
                        title: text(task, "title", LINE_CHARS),
                        role: name(task, "role"),
                        deps: array(task, "deps")
                            .iter()
                            .filter_map(Value::as_u64)
                            .collect(),
                    })
                    .collect(),
                roster: array(template, "roster")
                    .iter()
                    .take(MEMBERS_KEPT)
                    .map(|seat| CatalogSeatView {
                        role: name(seat, "role"),
                        count: seat.get("count").and_then(Value::as_u64).unwrap_or(1),
                        optional: seat
                            .get("optional")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                    })
                    .collect(),
            })
            .collect(),
        routing: array(catalog, "routing")
            .iter()
            .take(PROPOSALS_KEPT)
            .map(|rule| CatalogRuleView {
                id: name(rule, "id"),
                when: text(rule, "when", TEXT_CHARS),
                template: name(rule, "template"),
                r#use: name(rule, "use"),
                approval: name(rule, "approval"),
            })
            .collect(),
        planning_rules: text(catalog, "planning_rules", TEXT_CHARS),
        pipelines: array(catalog, "pipelines")
            .iter()
            .take(PROPOSALS_KEPT)
            .map(|pipeline| CatalogPipelineView {
                id: name(pipeline, "id"),
                name: name(pipeline, "name"),
                stages: array(pipeline, "stages")
                    .iter()
                    .take(ITEMS_KEPT)
                    .map(|stage| CatalogStageView {
                        kind: name(stage, "kind"),
                        role: name(stage, "role"),
                        member: name(stage, "member"),
                        command: text(
                            stage.get("command").unwrap_or(&Value::Null),
                            "command",
                            TEXT_CHARS,
                        ),
                    })
                    .collect(),
                max_rounds: pipeline
                    .get("max_rounds")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                folder: name(pipeline, "folder"),
                accept: name(pipeline, "accept"),
            })
            .collect(),
        draft_base: draft.get("base").and_then(Value::as_u64).unwrap_or(0),
        proposals: array(draft, "proposals")
            .iter()
            .take(PROPOSALS_KEPT)
            .map(|proposal| {
                let change = proposal.get("value").cloned().unwrap_or(Value::Null);
                CatalogProposalView {
                    id: name(proposal, "id"),
                    kind: name(proposal, "kind"),
                    action: name(proposal, "action"),
                    target: name(proposal, "target"),
                    title: text(proposal, "title", TEXT_CHARS),
                    reason: text(proposal, "reason", TEXT_CHARS),
                    state: text(proposal, "state", NAME_CHARS),
                    text: change.as_str().unwrap_or_default().to_owned(),
                    value: bounded(Some(&change)),
                    at: number(proposal, "at"),
                }
            })
            .collect(),
    }
}
