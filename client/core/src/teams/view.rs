//! Compact view models of the team reads. Each is projected once in the core
//! from the checked reply and serialized as it is to the webview, which never
//! parses an `hgs` reply. Texts and lists are bounded; a view that left
//! something out says so (`truncated`).
use serde::Serialize;
use serde_json::{Map, Value};

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
    pub minutes_without_progress: u64,
    pub host_shared: bool,
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
    pub decided_by: String,
    pub pinned: bool,
    pub output: Vec<String>,
    /// What the task waits for, as hgs words it (`waits.unblock`).
    pub waits: String,
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
    pub lead: String,
    pub revision: u64,
    pub created: f64,
    pub updated: f64,
    pub waiting_for_lead: bool,
    pub pending_phase: Option<u64>,
    /// The team's settings object as hgs stores it (a few dozen fields).
    pub settings: Value,
    pub members: Vec<MemberView>,
    pub tasks: Vec<TaskView>,
    pub phases: Vec<PhaseView>,
    pub proposals: Vec<ProposalView>,
    /// The attention items as hgs words them (bounded).
    pub attention: Vec<Value>,
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
        minutes_without_progress: count(value, "minutes_without_progress"),
        host_shared: flag(value, "host_shared"),
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
        decided_by: name(value, "decided_by"),
        pinned: flag(value, "pinned"),
        output: lines(value, "output", LINE_CHARS),
        waits: value
            .get("waits")
            .map(|waits| text(waits, "unblock", LINE_CHARS))
            .unwrap_or_default(),
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
    }
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
        revision: count(value, "revision"),
        created: number(value, "created"),
        updated: number(value, "updated"),
        waiting_for_lead: flag(value, "waiting_for_lead"),
        pending_phase: value.get("pending_phase").and_then(Value::as_u64),
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
            .map(phase)
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
}

/// The first page of `team waiting`, with how many wait in all.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct WaitingView {
    pub items: Vec<WaitingItem>,
    pub total: u64,
    /// More items wait after this page.
    pub more: bool,
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
            })
            .collect(),
        total: count(value, "total"),
        more: value.get("next").is_some_and(|next| !next.is_null()),
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

/// `team profile`, kept as hgs gives it (a bounded summary: losses, roles,
/// models and one page of tasks).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ProfileView {
    pub team: String,
    pub profile: Value,
}

pub fn profile(team: &str, value: &Value) -> ProfileView {
    ProfileView {
        team: team.to_owned(),
        profile: value.clone(),
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
        builtin: value.get("builtin").and_then(Value::as_bool).unwrap_or(false),
        roles: array(catalog, "roles")
            .iter()
            .take(PROPOSALS_KEPT)
            .map(|role| CatalogRoleView {
                id: name(role, "id"),
                name: name(role, "name"),
                agent: name(role, "agent"),
                model: name(role, "model"),
                prompt: text(role, "prompt", TEXT_CHARS),
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
                roster: array(template, "roster")
                    .iter()
                    .take(MEMBERS_KEPT)
                    .map(|seat| CatalogSeatView {
                        role: name(seat, "role"),
                        count: seat.get("count").and_then(Value::as_u64).unwrap_or(1),
                        optional: seat.get("optional").and_then(Value::as_bool).unwrap_or(false),
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
                        command: text(stage.get("command").unwrap_or(&Value::Null), "command", TEXT_CHARS),
                    })
                    .collect(),
                max_rounds: pipeline.get("max_rounds").and_then(Value::as_u64).unwrap_or(0),
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
