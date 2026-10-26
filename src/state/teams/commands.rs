//! WhiteWh fork: `hgs [@host] team <operation> --json`. Every operation reads
//! one JSON object on stdin; changes carry a canonical `request_id` and replay
//! their first receipt.
use super::super::session_graph::caller_record;
use super::super::*;
use super::model::*;
use super::ops::{self, Actor};
use super::plan;
use super::store::{self, canonical_uuid, Store, PAGE};
use anyhow::{bail, ensure, Context};
use std::collections::BTreeSet;

pub(crate) const USAGE: &str = "usage: hgs [@host] team create|plan|extend|edit-draft|move-phase|approve-phase|discard-phase|finish|approve|answer|discard|pause|resume|stop|archive|status|inbox|list|events|current|report|ask|reply|hold|release|accept|reject|reassign|take-over|cancel|retry|check|use-last-reply|relaunch|add-member|pause-member|resume-member|remove-member --json (JSON stdin) | hgs [@host] team catalog get|set|propose|decide|apply|discard|route|designer --json | hgs team worker [--stop]";

const MUTATIONS: &[&str] = &[
    "plan", "extend", "edit-draft", "move-phase", "approve-phase", "discard-phase", "finish", "approve", "discard", "pause", "resume", "stop",
    "archive", "report", "ask", "reply", "hold", "release", "accept", "reject", "reassign", "take-over", "cancel",
    "retry", "check", "use-last-reply", "relaunch", "add-member", "pause-member", "resume-member", "remove-member",
];

fn text<'a>(input: &'a Value, key: &str) -> &'a str {
    input[key].as_str().unwrap_or_default()
}

/// How many active teams a status or an adoption looks at (C1): the indexed
/// Planning/Running/Paused query, bounded, never every stored team.
const ACTIVE_TEAMS: i64 = 1000;

/// The member whose session runs this CLI, proven by process ancestry.
fn calling_member(team: &Team, record: &Value) -> Option<String> {
    let name = record["name"].as_str()?;
    let run = record["run_id"].as_str().unwrap_or_default();
    team.members
        .iter()
        .find(|member| {
            member.session.as_ref().is_some_and(|session| {
                session.host.is_empty() && session.name == name && (session.run_id.is_empty() || session.run_id == run)
            })
        })
        .map(|member| member.id.clone())
}

/// The member whose dsh session carries this conversation id. A dsh session
/// has no process environment of an hgs session; its harness exports the
/// conversation id as `DSH_SESSION_ID` instead (A3, A4).
fn dsh_member(team: &Team, conversation: &str) -> Option<String> {
    if conversation.is_empty() {
        return None;
    }
    team.members
        .iter()
        .find(|member| {
            member.session.as_ref().is_some_and(|session| session.host.is_empty() && session.conversation_id == conversation)
        })
        .map(|member| member.id.clone())
}

/// Who asks. A team session is proven by ancestry; a dsh session by the
/// conversation its harness exports; a report, question or `current` from
/// elsewhere (a member on another computer) is authorized by its attempt id;
/// anything else must say it is you.
fn actor(team: &Team, op: &str, input: &Value) -> anyhow::Result<Actor> {
    resolve_actor(
        team,
        op,
        input,
        caller_record().as_ref(),
        nonempty_env("DSH_SESSION_ID").as_deref(),
        nonempty_env("HGS_SESSION").as_deref(),
    )
}

/// The proven identities decide who asks. A stale `HGS_SESSION` (a resumed host
/// leaks another session's into its members) never names the caller: a member
/// is proven by its ancestry, its dsh conversation or its attempt id, in that
/// order (H7).
fn resolve_actor(
    team: &Team,
    op: &str,
    input: &Value,
    record: Option<&Value>,
    dsh: Option<&str>,
    env: Option<&str>,
) -> anyhow::Result<Actor> {
    if let Some(record) = record {
        let member = calling_member(team, record).with_context(|| format!("This session is not a member of team {}", team.name))?;
        return Ok(Actor::Member(member));
    }
    if let Some(member) = dsh_member(team, dsh.unwrap_or_default()) {
        return Ok(Actor::Member(member));
    }
    if matches!(op, "report" | "ask") && text(input, "by") != "human" {
        let attempt = text(input, "attempt");
        if let Some(member) = attempt_member(team, attempt) {
            return Ok(Actor::Member(member));
        }
        // A report keeps the old empty-member path for a stale id; a question
        // names the attempt it belongs to.
        if op == "report" {
            return Ok(Actor::Member(String::new()));
        }
        bail!("The attempt is not in this team");
    }
    // A process outside every session acts as the owner; a session env that
    // does not own the calling process could name another one (H7).
    ensure!(env.is_none(), "Zerus could not verify the calling session");
    ensure!(text(input, "by") == "human", "Run this from the lead's session, or add \"by\":\"human\" to act as yourself");
    Ok(Actor::Human)
}

/// The member whose attempt id the request names, for a session with no local
/// identity of its own (N2).
fn attempt_member(team: &Team, attempt: &str) -> Option<String> {
    if attempt.is_empty() {
        return None;
    }
    team.tasks.iter().flat_map(|task| &task.attempts).find(|found| found.id == attempt).map(|found| found.member.clone())
}

/// A running team whose every task is decided waits for the lead to extend it
/// or finish it.
fn waiting_for_lead(team: &Team) -> bool {
    team.status == TeamStatus::Running && ops::all_decided(team)
}

/// The phase record still waiting for the owner, if any. A team that is over
/// shows none, even if an old record was left in draft.
fn pending_phase(team: &Team) -> Option<u32> {
    if !matches!(team.status, TeamStatus::Running | TeamStatus::Paused) {
        return None;
    }
    team.phases.iter().find(|phase| phase.state == PhaseState::Draft).map(|phase| phase.n)
}

/// Operations after which new work may be ready for the dispatcher, or a
/// question or answer waits to be delivered.
fn starts_worker(op: &str) -> bool {
    matches!(op, "approve" | "resume" | "extend" | "approve-phase" | "ask" | "reply" | "release")
}

/// Whether the attempt's member was last seen stopped: the dispatcher records
/// it every time the live listing changes (L3).
fn stopped(team: &Team, task: &Task) -> bool {
    task.attempts
        .last()
        .and_then(|attempt| team.member(&attempt.member))
        .and_then(|member| member.seen)
        .is_some_and(|seen| !seen.live)
}

/// A member's queue (A5, spec section 6): its open task first, then the tasks
/// the lead returned to it (they have attempts, are not finished and are Pending,
/// held or not). Nothing else.
fn member_queue(team: &Team, member: &str) -> Vec<Value> {
    let stopped = team.member(member).and_then(|member| member.seen).is_some_and(|seen| !seen.live);
    let entry = |task: &Task| json!({"task": task.id, "phase": ops::attempt_phase(task, stopped)});
    let mut open: Vec<u32> = team
        .tasks
        .iter()
        .filter(|task| task.open_attempt().is_some_and(|attempt| attempt.member == member))
        .map(|task| task.id)
        .collect();
    open.sort_unstable();
    let mut queue: Vec<Value> = open.iter().filter_map(|id| team.task(*id)).map(entry).collect();
    let mut returned: Vec<&Task> = team
        .tasks
        .iter()
        .filter(|task| {
            task.assignee == member && !task.status.finished() && task.status == TaskStatus::Pending && !task.attempts.is_empty()
        })
        .collect();
    returned.sort_by_key(|task| task.id);
    queue.extend(returned.into_iter().map(entry));
    queue
}

/// The members of a finished team whose sessions still run, from the live
/// listing (F7). The team's lead and any session that is the lead or a member
/// of a team still active are left out, so the owner cannot stop a working
/// session by mistake. Zerus never stops them.
fn left_running(team: &Team, teams: &[Team], sessions: &[super::dispatcher::Seen]) -> Vec<Value> {
    if !matches!(team.status, TeamStatus::Done | TeamStatus::Stopped) {
        return Vec::new();
    }
    let busy = |session: &SessionRef| {
        teams.iter().filter(|other| matches!(other.status, TeamStatus::Planning | TeamStatus::Running | TeamStatus::Paused)).any(|other| {
            let same = |other: &SessionRef| other.host == session.host && other.name == session.name;
            other.member(&other.lead).and_then(|lead| lead.session.as_ref()).is_some_and(same)
                || other.members.iter().any(|member| {
                    member.state != MemberState::Removed && member.session.as_ref().is_some_and(same)
                })
        })
    };
    team.members
        .iter()
        .filter(|member| member.state != MemberState::Removed && member.id != team.lead)
        .filter_map(|member| {
            let session = member.session.as_ref()?;
            let found = sessions.iter().find(|seen| seen.host == session.host && seen.name == session.name)?;
            (found.live && !busy(session)).then(|| json!({"member":member.id,"role":team.role_of(&member.id),"session":session}))
        })
        .collect()
}

/// The team with its live members when the caller listed sessions (F7).
fn view_with(team: &Team, teams: &[Team], sessions: Option<&[super::dispatcher::Seen]>) -> Value {
    let mut value = serde_json::to_value(team).unwrap_or_default();
    value["validation"] = json!(plan::validate(team, &BTreeSet::new()));
    value["layers"] = json!(plan::layers(&team.tasks));
    value["cycle"] = json!(plan::cycle_tasks(&team.tasks));
    value["ready"] = json!(plan::ready(team));
    value["waiting_for_lead"] = json!(waiting_for_lead(team));
    value["pending_phase"] = json!(pending_phase(team));
    // Each task's last attempt carries its derived phase (spec section 2). The
    // dispatcher records each member's last seen liveness, so `stopped` is
    // derived here without a listing of status's own (L3).
    if let Some(tasks) = value["tasks"].as_array_mut() {
        let ready: BTreeSet<u32> = plan::ready(team).into_iter().collect();
        for (task, value) in team.tasks.iter().zip(tasks) {
            if let Some(phase) = ops::attempt_phase(task, stopped(team, task)) {
                if let Some(attempt) = value["attempts"].as_array_mut().and_then(|attempts| attempts.last_mut()) {
                    attempt["phase"] = json!(phase);
                }
            }
            // C3: every open task says what blocks it and which action frees it.
            if let Some(waits) = super::inbox::waits(team, task, ready.contains(&task.id)) {
                value["waits"] = waits;
            }
        }
    }
    // C4: the reports the owner's attention list shows.
    value["attention"] = json!(super::inbox::attention(team, super::super::now()));
    // Each member's queue: its open task first, then the tasks returned to it
    // (A5, spec section 6).
    if let Some(members) = value["members"].as_array_mut() {
        for (member, value) in team.members.iter().zip(members) {
            value["queue"] = json!(member_queue(team, &member.id));
        }
    }
    // Done and Stopped teams say which of their members still run (F7).
    if let Some(sessions) = sessions {
        if matches!(team.status, TeamStatus::Done | TeamStatus::Stopped) {
            value["left_running"] = json!(left_running(team, teams, sessions));
        }
    }
    value
}

/// A team as the session list shows it: members with their sessions and each
/// task's state, without descriptions, reports or the feed. The list never
/// reads the live session listing: the desktop polls it often (C1).
fn summary(team: &Team) -> Value {
    let count = |status: TaskStatus| team.tasks.iter().filter(|task| task.status == status).count();
    let members: Vec<Value> = team
        .members
        .iter()
        .filter(|m| m.state != MemberState::Removed)
        .map(|m| {
            json!({"id":m.id,"role":team.role_of(&m.id),"agent":m.agent,"model":m.model,"computer":m.computer,
                "session":m.session,"state":m.state,"error":m.error,"queue":member_queue(team, &m.id)})
        })
        .collect();
    let tasks: Vec<Value> = team
        .tasks
        .iter()
        .map(|t| {
            let attempt = t.attempts.last();
            json!({"id":t.id,"title":t.title,"status":t.status,"assignee":t.assignee,"phase":t.phase,
                "attempt":attempt.map(|a| json!({"n":a.n,"state":a.state,"no_report":a.no_report,
                    "phase":ops::attempt_phase(t, stopped(team, t))}))})
        })
        .collect();
    // G8: the session row counts the phase in work and every phase, so it needs
    // each task's phase and the phase records (without their change history).
    let phases: Vec<Value> = team
        .phases
        .iter()
        .map(|p| json!({"n":p.n,"order":p.order,"title":p.title,"barrier":p.barrier,"state":p.state,"tasks":p.tasks}))
        .collect();
    json!({
        "id":team.id,"name":team.name,"status":team.status,"project":team.project,"folder":team.folder,
        "lead":team.lead,"members":members,"tasks":tasks,"phases":phases,
        "accepted":count(TaskStatus::Accepted),"submitted":count(TaskStatus::Submitted),
        "revision":team.revision,"updated":team.updated,
        "waiting_for_lead":waiting_for_lead(team),"pending_phase":pending_phase(team),
        // C4: the owner's attention list, also from the polled team list.
        "attention":super::inbox::attention(team, super::super::now()),
    })
}

fn create(input: &Value) -> anyhow::Result<Value> {
    let request = text(input, "request_id");
    ensure!(canonical_uuid(request), "A canonical request_id is required");
    let mut filled = input.clone();
    if let Some(record) = caller_record() {
        // A lead creates its own team: its session is the lead's.
        let lead = &mut filled["lead"];
        ensure!(lead.is_object(), "Describe the lead");
        let session = json!({"name":record["name"],"run_id":record["run_id"],"conversation_id":record["conversation_id"]});
        ensure!(lead["session"].is_null() || lead["session"]["name"] == record["name"], "A session creates only a team it leads");
        lead["session"] = session;
        if lead["agent"].is_null() {
            lead["agent"] = json!(record["agent"]);
        }
    } else {
        ensure!(nonempty_env("HGS_SESSION").is_none(), "Zerus could not verify the calling session");
        ensure!(text(input, "by") == "human", "Run this from the lead's session, or add \"by\":\"human\" to act as yourself");
    }
    let (team, events) = ops::create(&uuid::Uuid::new_v4().to_string(), &filled)?;
    let start = team.brief == Notice::Pending;
    // The request's identity is what the caller sent, so a replay matches.
    let receipt = Store::open(true)?.create(request, input, team, events)?;
    if start {
        if let Err(error) = super::dispatcher::ensure_worker() {
            eprintln!("hgs: the team dispatcher did not start: {error}");
        }
    }
    Ok(receipt)
}

/// The lead (or you) answers a member's question or permission request in the
/// member's session, by its exact id and hash.
fn answer(input: &Value) -> anyhow::Result<Value> {
    let team = Store::open(false)?.load(text(input, "team"))?.context("Unknown team")?;
    let by = actor(&team, "answer", input)?;
    ensure!(matches!(&by, Actor::Human) || matches!(&by, Actor::Member(id) if *id == team.lead), "Only the lead answers a member's question");
    let member = team.member(text(input, "member")).context("Unknown member")?;
    let session = member.session.as_ref().context("The member has no session")?;
    let (question, hash) = (text(input, "question_id"), text(input, "expected_question_hash"));
    ensure!(!question.is_empty() && !hash.is_empty() && input["answers"].is_array(), "question_id, expected_question_hash and answers are required");
    let inspected = super::dispatcher::hgs(&session.host, &["inspect", &session.name, "--skip-processes"], None, std::time::Duration::from_secs(45))?;
    ensure!(inspected.status.success(), "{}", String::from_utf8_lossy(&inspected.stderr).trim());
    let record: Value = serde_json::from_slice(&inspected.stdout).context("invalid inspect output")?;
    record["pending_questions"].as_array().into_iter().flatten()
        .find(|card| card["question_id"] == question && card["question_hash"] == hash)
        .context("This question changed or is no longer pending")?;
    let request = json!({"request_id":uuid::Uuid::new_v4().to_string(),"expected_run_id":session.run_id,
        "expected_conversation_id":session.conversation_id,"question_id":question,"expected_question_hash":hash,"answers":input["answers"]});
    let output = super::dispatcher::hgs(&session.host, &["answer", &session.name, "--json"], Some(&request), std::time::Duration::from_secs(120))?;
    ensure!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr).trim());
    Ok(serde_json::from_slice(&output.stdout).unwrap_or_else(|_| json!({"status":"answered"})))
}

/// What `hgs team current` answers for one member of a team (spec section 5):
/// the live attempt, its phase, its open questions and the report command.
/// Read only.
fn current_view(team: &Team, member: &str) -> anyhow::Result<Value> {
    let Some((task, attempt)) = team
        .tasks
        .iter()
        .rev()
        .filter_map(|task| task.open_attempt().map(|attempt| (task, attempt)))
        .find(|(_, attempt)| attempt.member == member)
    else {
        bail!("{}", no_attempt_text(team, member));
    };
    let phase = ops::attempt_phase(task, false).unwrap_or("unconfirmed");
    let open_questions: Vec<&str> =
        attempt.questions.iter().filter(|question| question.open()).map(|question| question.id.as_str()).collect();
    Ok(json!({
        "team": team.id,
        "task": task.id,
        "title": task.title,
        "attempt": attempt.id,
        "n": attempt.n,
        "phase": phase,
        "open_questions": open_questions,
        "report": {
            "request_id": uuid::Uuid::new_v4().to_string(),
            "team": team.id,
            "task": task.id,
            "attempt": attempt.id,
            "text": "..."
        }
    }))
}

/// Why a member has no open attempt: its last task and its state, such as
/// `#7 was rejected and is held by the lead` (spec section 5).
fn no_attempt_text(team: &Team, member: &str) -> String {
    let last = team
        .tasks
        .iter()
        .filter(|task| task.assignee == member || task.attempts.iter().any(|attempt| attempt.member == member))
        .max_by_key(|task| task.id);
    let Some(task) = last else { return "You have no open attempt".into() };
    let attempt = task.attempts.iter().rev().find(|attempt| attempt.member == member);
    let state = match attempt.map(|attempt| attempt.state) {
        Some(AttemptState::Rejected) if task.hold.is_some() => "was rejected and is held by the lead",
        Some(AttemptState::Rejected) => "was rejected and waits for another hand-out",
        Some(AttemptState::Reported) => "has a report waiting for the lead's decision",
        Some(AttemptState::Revoked) => "was returned to the lead",
        Some(AttemptState::Refused) => "was not delivered; Zerus tries it again",
        Some(AttemptState::Delivering | AttemptState::Delivered | AttemptState::Unconfirmed) => "is open to another attempt",
        None => match task.status {
            TaskStatus::Accepted => "was accepted",
            TaskStatus::Cancelled => "was cancelled",
            TaskStatus::Submitted => "has a report waiting for the lead's decision",
            TaskStatus::Draft => "is a draft",
            _ => "is waiting for a hand-out",
        },
    };
    format!("#{} {state}", task.id)
}

/// The team a `current` request names (N3): only one that still runs
/// (Planning, Running or Paused), never a finished team.
fn current_team<'a>(teams: &'a [Team], named: &str, mut member_of: impl FnMut(&Team) -> Option<String>) -> anyhow::Result<(&'a Team, String)> {
    let mut found: Vec<(&Team, String)> = Vec::new();
    for team in teams {
        if !matches!(team.status, TeamStatus::Planning | TeamStatus::Running | TeamStatus::Paused) {
            continue;
        }
        if !named.is_empty() && team.id != named {
            continue;
        }
        if let Some(member) = member_of(team) {
            found.push((team, member));
        }
    }
    match found.as_slice() {
        [] if named.is_empty() => bail!("This session is not a member of a running team"),
        [] => bail!("This session is not a member of team {named}"),
        [only] => Ok((only.0, only.1.clone())),
        many => bail!("This session is a member of {} teams; name one with \"team\"", many.len()),
    }
}

/// The member's live attempt from its own session; `team` is optional when the
/// session belongs to one team (spec section 5). Read only.
fn current(input: &Value) -> anyhow::Result<Value> {
    let named = text(input, "team");
    // A member on another computer has no local identity: its attempt id names
    // it, as report does (N2).
    let attempt = text(input, "attempt");
    if !attempt.is_empty() {
        let teams = Store::open(false)?.active_teams(ACTIVE_TEAMS)?;
        let found = teams.iter().find_map(|team| {
            if !named.is_empty() && team.id != named {
                return None;
            }
            if !matches!(team.status, TeamStatus::Planning | TeamStatus::Running | TeamStatus::Paused) {
                return None;
            }
            team.tasks
                .iter()
                .flat_map(|task| &task.attempts)
                .find(|found| found.id == attempt)
                .map(|found| (team, found.member.clone()))
        });
        let (team, member) = found.with_context(|| format!("No attempt {attempt} runs in a team"))?;
        return current_view(team, &member);
    }
    let record = caller_record();
    let conversation = nonempty_env("DSH_SESSION_ID").unwrap_or_default();
    if record.is_none() {
        ensure!(!conversation.is_empty(), "hgs team current runs in a member's session");
    }
    // Only the teams that still run, from the indexed query (N3, C1).
    let teams = Store::open(false)?.active_teams(ACTIVE_TEAMS)?;
    let (team, member) = current_team(&teams, named, |team| match &record {
        Some(record) => calling_member(team, record),
        None => dsh_member(team, &conversation),
    })?;
    current_view(team, &member)
}

/// Whether a status request reads the live listing: only a finished team, for
/// the members it left running (F7). The desktop polls the watched team's
/// status every 3 s, so a running team's status never lists sessions (C1, L3).
fn status_lists_sessions(status: TeamStatus) -> bool {
    matches!(status, TeamStatus::Done | TeamStatus::Stopped)
}

/// The live sessions of this computer and its peers, as the dispatcher reads
/// them (F7, F8).
fn live_sessions() -> anyhow::Result<Vec<super::dispatcher::Seen>> {
    let output = super::dispatcher::hgs("", &["ls", "--json", "--no-archives"], None, std::time::Duration::from_secs(15))?;
    ensure!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr).trim());
    let listing: Value = serde_json::from_slice(&output.stdout).context("invalid hgs ls output")?;
    Ok(super::dispatcher::seen(&listing))
}

/// How long a finished team's status reuses one live listing (C1, NEW-2): the
/// desktop polls the watched team every 3 s, and a full listing may take 15 s.
const LISTING_REUSE: f64 = 30.0;

/// The live sessions for a finished team's left_running, from a listing at
/// most LISTING_REUSE old: polling a Done team never runs hgs ls each time.
fn recent_sessions() -> anyhow::Result<Vec<super::dispatcher::Seen>> {
    let path = root().join("teams/left-running-listing.json");
    let now = super::super::now();
    if let Some(cached) = std::fs::read(&path).ok().and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok()) {
        if cached["at"].as_f64().is_some_and(|at| at <= now && now - at < LISTING_REUSE) {
            return Ok(super::dispatcher::seen(&cached["listing"]));
        }
    }
    let output = super::dispatcher::hgs("", &["ls", "--json", "--no-archives"], None, std::time::Duration::from_secs(15))?;
    ensure!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr).trim());
    let listing: Value = serde_json::from_slice(&output.stdout).context("invalid hgs ls output")?;
    // Best effort: a failed write only means the next poll lists again.
    let _ = std::fs::write(&path, serde_json::to_vec(&json!({"at":now,"listing":listing})).unwrap_or_default());
    Ok(super::dispatcher::seen(&listing))
}

/// Where a member works when it adopts an existing session (F8): the session's
/// own folder, stored as `directory`, and whether it keeps its worktree.
#[derive(Debug)]
struct Adopted {
    session: SessionRef,
    directory: String,
    worktree: bool,
}

/// The same git common dir: this folder already belongs to the team's
/// repository, so a worktree member may adopt the session in it (F8).
fn same_repository(folder: &str, team_folder: &str) -> bool {
    let common = |folder: &str| -> Option<std::path::PathBuf> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let output = super::super::worktrees::git(
            std::path::Path::new(folder),
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            deadline,
        )
        .ok()?;
        let text = String::from_utf8(output).ok()?;
        let text = text.trim();
        (!text.is_empty()).then(|| std::path::PathBuf::from(text))
    };
    match (common(folder), common(team_folder)) {
        (Some(folder), Some(team)) => std::fs::canonicalize(folder)
            .ok()
            .zip(std::fs::canonicalize(team).ok())
            .is_some_and(|(folder, team)| folder == team),
        _ => false,
    }
}

/// Checks the existing session a member adopts (F8): it must run on the
/// member's computer, be no lead or member of another active team, and a
/// worktree member needs it inside the team's repository. The member then
/// works in the session's own folder.
fn adopted_session(team: &Team, member: &Member, teams: &[Team], sessions: &[super::dispatcher::Seen]) -> anyhow::Result<Adopted> {
    let session = member.session.as_ref().context("A member has no session")?;
    ensure!(!session.name.is_empty(), "An existing session needs a name");
    let host = if session.host.is_empty() { member.computer.clone() } else { session.host.clone() };
    let here = if host.is_empty() { "this computer".to_owned() } else { host.clone() };
    ensure!(
        host == member.computer,
        "{} runs on {}, not on {}",
        session.name,
        here,
        if member.computer.is_empty() { "this computer" } else { member.computer.as_str() }
    );
    let found = sessions
        .iter()
        .find(|seen| seen.host == host && seen.name == session.name && (session.run_id.is_empty() || seen.run_id == session.run_id))
        .with_context(|| format!("{} is not running on {here}", session.name))?;
    ensure!(found.live, "{} is not running on {here}", session.name);
    ensure!(!found.cwd.is_empty(), "{} has no folder", session.name);
    // It may not be the lead or a member of another team that is still active.
    for other in teams.iter().filter(|other| other.id != team.id && matches!(other.status, TeamStatus::Planning | TeamStatus::Running | TeamStatus::Paused)) {
        if other
            .member(&other.lead)
            .and_then(|lead| lead.session.as_ref())
            .is_some_and(|lead| lead.name == session.name && lead.host == host)
        {
            bail!("{} is the lead of team {}", session.name, other.name);
        }
        if other.members.iter().any(|member| {
            member.state != MemberState::Removed
                && member.id != other.lead
                && member.session.as_ref().is_some_and(|member| member.name == session.name && member.host == host)
        }) {
            bail!("{} is a member of team {}", session.name, other.name);
        }
    }
    // Nor this team's lead or another of its members.
    if team
        .member(&team.lead)
        .and_then(|lead| lead.session.as_ref())
        .is_some_and(|lead| lead.name == session.name && lead.host == host)
    {
        bail!("{} is the lead of team {}", session.name, team.name);
    }
    if team.members.iter().any(|other| {
        other.id != member.id
            && other.state != MemberState::Removed
            && other.session.as_ref().is_some_and(|other| other.name == session.name && other.host == host)
    }) {
        bail!("{} is already a member of team {}", session.name, team.name);
    }
    let session = SessionRef {
        host: host.clone(),
        name: session.name.clone(),
        run_id: if session.run_id.is_empty() { found.run_id.clone() } else { session.run_id.clone() },
        conversation_id: if session.conversation_id.is_empty() { found.conversation_id.clone() } else { session.conversation_id.clone() },
    };
    let worktree = member.folder == MemberFolder::Worktree;
    if worktree {
        // Git runs on this computer; a session elsewhere cannot be checked, so
        // it is refused with that reason instead of a wrong "not a repository" (L5).
        ensure!(
            host.is_empty(),
            "{} runs on {}; a worktree member's session must be on this computer so its repository can be checked",
            session.name,
            here
        );
        ensure!(same_repository(&found.cwd, &team.folder), "{}'s folder {} is not the team's repository", session.name, found.cwd);
    }
    Ok(Adopted { session, directory: found.cwd.clone(), worktree })
}

/// Adopts the existing sessions a plan or a new member names (F8): the session
/// as `hgs ls` shows it and its own folder, or a clear refusal before the team
/// changes. A request that already has a receipt returns before any of it, so
/// a retry gives the stored receipt instead of listing sessions again (L4).
fn adopt_existing_sessions(store: &Store, request: &str, input: &Value, op: &str, filled: &mut Value) -> anyhow::Result<()> {
    if store.receipt(request, input)?.is_some() {
        return Ok(());
    }
    let Ok(Some(team)) = store.load(text(input, "team")) else { return Ok(()) };
    let planned = op == "plan" && team.status == TeamStatus::Planning;
    let joined = op == "add-member" && matches!(team.status, TeamStatus::Planning | TeamStatus::Running | TeamStatus::Paused);
    if planned {
        if let Some(members) = filled["members"].as_array_mut() {
            adopt_members(store, &team, members)?;
        }
    } else if joined {
        adopt_members(store, &team, std::slice::from_mut(&mut filled["member"]))?;
    }
    Ok(())
}

/// Checks and fills the members that name an existing session (F8).
fn adopt_members(store: &Store, team: &Team, members: &mut [Value]) -> anyhow::Result<()> {
    if !members.iter().any(|member| member["session"].is_object()) {
        return Ok(());
    }
    let sessions = live_sessions()?;
    // Only the teams that still run, from the indexed query (C1).
    let teams = store.active_teams(ACTIVE_TEAMS)?;
    for value in members.iter_mut() {
        if !value["session"].is_object() {
            continue;
        }
        let member: Member = serde_json::from_value(value.clone()).context("Invalid member")?;
        let adopted = adopted_session(team, &member, &teams, &sessions)?;
        value["session"] = serde_json::to_value(&adopted.session)?;
        value["directory"] = json!(adopted.directory);
        value["folder"] = serde_json::to_value(if adopted.worktree { MemberFolder::Worktree } else { MemberFolder::Shared })?;
    }
    Ok(())
}

/// The team's files refuse writes here: SQLite opened them read-only, or the
/// system denies access (in any language: Windows localizes its message).
fn read_only(error: &anyhow::Error) -> bool {
    let text = format!("{error:#}").to_lowercase();
    ["readonly", "read-only", "access is denied", "permission denied", "os error 5)", "os error 13)", "unable to open database"]
        .iter()
        .any(|sign| text.contains(sign))
}

fn drop_report(input: &Value) -> anyhow::Result<Value> {
    ensure!(!text(input, "attempt").is_empty() && !text(input, "team").is_empty(), "A report names its team and attempt");
    ensure!(!text(input, "text").trim().is_empty(), "A report needs text");
    let path = super::dispatcher::dropped_report(&std::env::current_dir()?, text(input, "attempt"));
    std::fs::create_dir_all(path.parent().context("no report folder")?)?;
    std::fs::write(&path, serde_json::to_vec(input)?)?;
    Ok(json!({"status":"queued","request_id":input["request_id"],"task":input["task"],
        "note":"The team's files are read-only here; Zerus records this report from your folder."}))
}

/// The teams a finished team's `left_running` looks at: only the ones that
/// still run, from the indexed query, never every stored team (C1).
fn active_team_records(store: &Store) -> anyhow::Result<Vec<Team>> {
    store.active_teams(ACTIVE_TEAMS)
}

/// The stored team list the desktop polls: no live session listing (C1).
fn list_items(teams: &[Team]) -> Value {
    json!({"items":teams.iter().map(summary).collect::<Vec<_>>()})
}

/// The lead's inbox (C2): every report, question, permission request, its own
/// task and stopped member, oldest first. Read only; the coalesced message the
/// dispatcher sends lists the new ones.
fn inbox_view(team: &Team) -> Value {
    json!({"items": super::inbox::items(team).iter().map(super::inbox::Item::value).collect::<Vec<_>>()})
}

fn run(op: &str, input: &Value) -> anyhow::Result<Value> {
    match op {
        "create" => create(input),
        "answer" => answer(input),
        "list" => {
            let path = store::path();
            if !path.exists() {
                return Ok(json!({"items":[]}));
            }
            let limit = input["limit"].as_i64().unwrap_or(200).clamp(1, 1000);
            let store = Store::open(false)?;
            let teams = store.teams(input["include_archived"].as_bool() == Some(true), limit)?;
            // The desktop lists teams while it shows them: a dispatcher that
            // stopped (an update, a crash) starts again while a team needs one.
            if teams.iter().any(|t| t.status == TeamStatus::Running || (t.status == TeamStatus::Planning && t.brief == Notice::Pending)) {
                if let Err(error) = super::dispatcher::ensure_worker() {
                    eprintln!("hgs: the team dispatcher did not start: {error}");
                }
            }
            // Stored teams only: a live session listing belongs to status, not
            // to this list the desktop polls (C1).
            Ok(list_items(&teams))
        }
        "status" => {
            let store = Store::open(false)?;
            let team = store.load(text(input, "team"))?.context("Unknown team")?;
            let finished = matches!(team.status, TeamStatus::Done | TeamStatus::Stopped);
            // Only a finished team reads the live listing, for its members left
            // running (F7); a watched running team is polled often and must not
            // spawn one (C1, L3).
            let sessions = if status_lists_sessions(team.status) { recent_sessions().ok() } else { None };
            // Only the teams that still run, from the indexed query, so a
            // finished team's status never reads every team (C1).
            let teams = if finished { active_team_records(&store)? } else { Vec::new() };
            Ok(view_with(&team, &teams, sessions.as_deref()))
        }
        "inbox" => {
            let team = Store::open(false)?.load(text(input, "team"))?.context("Unknown team")?;
            Ok(inbox_view(&team))
        }
        "events" => {
            let before = match &input["before"] {
                Value::Null => None,
                value => Some(value.as_i64().filter(|n| *n > 0).context("Invalid feed cursor")?),
            };
            Store::open(false)?.events(text(input, "team"), before, input["limit"].as_i64().unwrap_or(PAGE))
        }
        "current" => current(input),
        op if MUTATIONS.contains(&op) => {
            let request = text(input, "request_id");
            ensure!(canonical_uuid(request), "A canonical request_id is required");
            let team_id = text(input, "team");
            let revision = input["revision"].as_u64();
            // A member in a sandbox may not write the team's files: its report
            // waits in its own folder for the dispatcher, which checks it.
            // SQLite may open the store read-only without a word, so the
            // refusal can come with the write as well as with the open.
            let mut store = match Store::open(true) {
                Ok(store) => store,
                Err(_) if op == "report" => return drop_report(input),
                Err(error) => return Err(error),
            };
            let mut filled = input.clone();
            if op == "use-last-reply" && text(input, "attempt").is_empty() {
                // Without an attempt, the task's open one.
                let team = store.load(team_id)?.context("Unknown team")?;
                let task = team.task(input["task"].as_u64().unwrap_or_default() as u32).context("Unknown task")?;
                filled["attempt"] = json!(task.open_attempt().context("The task has no open attempt")?.id);
            }
            if op == "use-last-reply" && text(input, "text").trim().is_empty() {
                // The member's last reply becomes the report; reading it is not
                // part of the request's identity.
                let team = store.load(team_id)?.context("Unknown team")?;
                let task = team.task(input["task"].as_u64().unwrap_or_default() as u32).context("Unknown task")?;
                let attempt = task.attempts.iter().find(|a| a.id == text(&filled, "attempt")).context("Unknown attempt")?;
                let session = team.member(&attempt.member).and_then(|m| m.session.clone()).context("The member has no session")?;
                filled["text"] = json!(super::dispatcher::last_reply(&session)?);
            }
            if matches!(op, "plan" | "add-member") {
                // An existing session must run here and be free of another
                // active team; its own folder replaces the planned one (F8).
                // A retry returns here before any listing (L4).
                adopt_existing_sessions(&store, request, input, op, &mut filled)?;
            }
            let receipt = match store.mutate(Some((request, input)), team_id, revision, |team, events| {
                let actor = actor(team, op, &filled)?;
                ops::apply(team, op, &filled, &actor, &BTreeSet::new(), events)
            }) {
                Ok(receipt) => receipt,
                Err(error) if op == "report" && read_only(&error) => return drop_report(input),
                Err(error) => return Err(error),
            };
            if starts_worker(op) {
                // The dispatcher runs while any team runs; it exits by itself.
                if let Err(error) = super::dispatcher::ensure_worker() {
                    eprintln!("hgs: the team dispatcher did not start: {error}");
                }
            }
            if receipt["status"] == "stale" {
                bail!("This attempt is no longer current for task #{}; the report was not accepted", receipt["task"]);
            }
            Ok(receipt)
        }
        _ => bail!("Unknown team operation {op}"),
    }
}

pub(crate) fn dispatch(args: &[String]) -> Result<i32> {
    if args.len() == 1 && args[0] == "worker" {
        return super::dispatcher::worker();
    }
    // The desktop sends this on an explicit Quit: the worker writes its pid and
    // heartbeat, so it can be ended without touching agents (U2).
    if args.len() == 2 && args[0] == "worker" && args[1] == "--stop" {
        return super::dispatcher::stop_worker();
    }
    let catalog = args.first().map(String::as_str) == Some("catalog");
    let args = if catalog { &args[1..] } else { args };
    if args.len() != 2 || args[1] != "--json" {
        return Err(USAGE.into());
    }
    let mut bytes = Vec::new();
    io::stdin().take(256 * 1024 + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() > 256 * 1024 {
        return Err("Team request exceeds 256 KiB".into());
    }
    let input: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if !input.is_object() {
        return Err("A JSON object is required".into());
    }
    let result = if catalog {
        // The designer proposes from its session; only you decide and apply.
        let human = caller_record().is_none() && nonempty_env("HGS_SESSION").is_none() && text(&input, "by") == "human";
        super::catalog::run(&args[0], &input, human)
    } else {
        run(&args[0], &input)
    }
    .map_err(|e| e.to_string())?;
    println!("{result}");
    Ok(0)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_list_summary_carries_task_phases_and_phase_records() {
        let mut team = crate::state::teams::model::Team::default();
        team.tasks.push(crate::state::teams::model::Task { id: 1, phase: 1, ..Default::default() });
        team.tasks.push(crate::state::teams::model::Task { id: 2, phase: 2, ..Default::default() });
        team.phases.push(crate::state::teams::model::Phase { n: 2, title: "Fixes".into(), tasks: vec![2], ..Default::default() });
        let value = super::summary(&team);
        assert_eq!(value["tasks"][1]["phase"], 2);
        assert_eq!(value["phases"][0]["n"], 2);
        assert_eq!(value["phases"][0]["tasks"][0], 2);
        assert!(value["phases"][0].get("changes").is_none());
    }
    use super::super::dispatcher::Seen;
    use super::*;

    #[test]
    fn a_refused_write_is_recognised_in_any_language() {
        for message in [
            "attempt to write a readonly database",
            "Отказано в доступе. (os error 5)",
            "Permission denied (os error 13)",
            "unable to open database file: teams.sqlite3",
        ] {
            assert!(read_only(&anyhow::anyhow!(message.to_owned())), "{message}");
        }
        assert!(!read_only(&anyhow::anyhow!("This attempt is no longer current")));
    }

    #[test]
    fn the_phase_commands_are_mutations_named_in_the_usage() {
        for op in ["extend", "approve-phase", "discard-phase", "finish"] {
            assert!(MUTATIONS.contains(&op), "{op} is not a mutation");
            assert!(USAGE.contains(op), "{op} is not in the usage");
        }
    }

    #[test]
    fn status_shows_waiting_for_lead_and_the_pending_phase() {
        let team = Team {
            id: "t".into(),
            name: "T".into(),
            lead: "lead".into(),
            status: TeamStatus::Running,
            tasks: vec![Task { id: 1, title: "Done".into(), status: TaskStatus::Accepted, assignee: "lead".into(), ..Default::default() }],
            phases: vec![Phase { n: 2, state: PhaseState::Draft, ..Default::default() }],
            ..Default::default()
        };
        let value = view_with(&team, &[], None);
        assert_eq!(value["waiting_for_lead"], true);
        assert_eq!(value["pending_phase"], 2);
        // A paused team with every task decided is Paused, not waiting.
        let mut paused = team.clone();
        paused.status = TeamStatus::Paused;
        assert_eq!(view_with(&paused, &[], None)["waiting_for_lead"], false);
        // The session list carries both too.
        let listed = summary(&paused);
        assert_eq!((listed["waiting_for_lead"].as_bool(), listed["pending_phase"].as_u64()), (Some(false), Some(2)));
        // A team that is over shows no pending phase, even with a stale record.
        let mut done = team.clone();
        done.status = TeamStatus::Done;
        assert!(view_with(&done, &[], None)["pending_phase"].is_null());
        assert_eq!(pending_phase(&done), None);
    }

    #[test]
    fn ask_reply_hold_and_release_are_mutations_and_current_is_a_read_in_the_usage() {
        for op in ["ask", "reply", "hold", "release"] {
            assert!(MUTATIONS.contains(&op), "{op} is not a mutation");
            assert!(USAGE.contains(op), "{op} is not in the usage");
        }
        assert!(USAGE.contains("current"));
        assert!(!MUTATIONS.contains(&"current"));
    }

    /// A running team with an open hand-out to dev.
    fn running_team() -> Team {
        let (mut team, _) = ops::create(
            "t",
            &json!({"name":"T","lead":{"id":"lead","agent":"claude","session":{"name":"claude/x/lead","run_id":"r0"}}}),
        )
        .unwrap();
        let mut events = Vec::new();
        ops::apply(
            &mut team,
            "plan",
            &json!({"members":[{"id":"dev","role":"Backend","agent":"codex","session":{"name":"codex/x/dev","run_id":"r1"}}],
                "tasks":[{"id":1,"title":"API","assignee":"dev"}]}),
            &Actor::Human,
            &BTreeSet::new(),
            &mut events,
        )
        .unwrap();
        ops::apply(&mut team, "approve", &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
        {
            let task = team.task_mut(1).unwrap();
            task.attempts.push(Attempt {
                n: 1,
                id: "00000000-0000-4000-8000-000000000001".into(),
                member: "dev".into(),
                state: AttemptState::Delivered,
                sent_at: 10.0,
                delivered_at: Some(10.0),
                started_at: Some(11.0),
                ..Default::default()
            });
            task.status = TaskStatus::InProgress;
        }
        team
    }

    #[test]
    fn a_question_shows_on_its_attempt_in_status() {
        let mut team = running_team();
        let mut events = Vec::new();
        let asked = ops::apply(
            &mut team,
            "ask",
            &json!({"text":"Must I keep the old field?"}),
            &Actor::Member("dev".into()),
            &BTreeSet::new(),
            &mut events,
        )
        .unwrap();
        assert_eq!((asked["status"].as_str(), asked["question"].as_str()), (Some("asked"), Some("q1")));
        // The attempt stays open and status shows it waiting for an answer.
        assert!(team.task(1).unwrap().open_attempt().is_some());
        let value = view_with(&team, &[], None);
        assert_eq!(value["tasks"][0]["attempts"][0]["phase"], "asking");
        assert_eq!(value["tasks"][0]["attempts"][0]["questions"][0]["text"], "Must I keep the old field?");
        assert_eq!(value["tasks"][0]["attempts"][0]["questions"][0]["answer"], "");
        // The lead's answer closes it; the attempt is still open and working.
        ops::apply(&mut team, "reply", &json!({"question":"q1","text":"Yes"}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
        assert!(team.task(1).unwrap().open_attempt().is_some());
        assert_eq!(view_with(&team, &[], None)["tasks"][0]["attempts"][0]["phase"], "working");
        assert_eq!(view_with(&team, &[], None)["tasks"][0]["attempts"][0]["questions"][0]["answer"], "Yes");
    }

    #[test]
    fn current_names_the_live_attempt_and_its_report_command() {
        let mut team = running_team();
        let mut events = Vec::new();
        ops::apply(&mut team, "ask", &json!({"text":"Must I keep?"}), &Actor::Member("dev".into()), &BTreeSet::new(), &mut events).unwrap();
        let value = current_view(&team, "dev").unwrap();
        assert_eq!((value["team"].as_str(), value["task"].as_u64(), value["n"].as_u64()), (Some("t"), Some(1), Some(1)));
        assert_eq!((value["title"].as_str(), value["phase"].as_str()), (Some("API"), Some("asking")));
        assert_eq!(value["attempt"], "00000000-0000-4000-8000-000000000001");
        assert_eq!(value["open_questions"], json!(["q1"]));
        // The report command the member fills in and pipes to hgs team report.
        let report = &value["report"];
        assert_eq!((report["team"].as_str(), report["task"].as_u64()), (Some("t"), Some(1)));
        assert_eq!(report["attempt"], "00000000-0000-4000-8000-000000000001");
        assert_eq!(report["text"], "...");
        assert!(canonical_uuid(report["request_id"].as_str().unwrap()));
        // Every call offers a fresh request id.
        assert_ne!(current_view(&team, "dev").unwrap()["report"]["request_id"], report["request_id"]);
    }

    #[test]
    fn current_refuses_a_member_without_an_open_attempt() {
        let mut team = running_team();
        {
            let task = team.task_mut(1).unwrap();
            task.attempts[0].state = AttemptState::Rejected;
            task.status = TaskStatus::Pending;
            task.hold = Some(Hold { by: "Lead".into(), at: 12.0, reason: "Later".into() });
        }
        assert_eq!(current_view(&team, "dev").unwrap_err().to_string(), "#1 was rejected and is held by the lead");
        // A member without a task, and the lead with none.
        assert_eq!(current_view(&team, "qa").unwrap_err().to_string(), "You have no open attempt");
        assert_eq!(current_view(&team, "lead").unwrap_err().to_string(), "You have no open attempt");
        // A reported attempt waits for the lead, not for the member.
        let mut reported = running_team();
        {
            let attempt = &mut reported.task_mut(1).unwrap().attempts[0];
            attempt.state = AttemptState::Reported;
            attempt.report = Some(Report { text: "Done".into(), at: 20.0, ..Default::default() });
        }
        reported.task_mut(1).unwrap().status = TaskStatus::Submitted;
        assert_eq!(current_view(&reported, "dev").unwrap_err().to_string(), "#1 has a report waiting for the lead's decision");
    }

    #[test]
    fn an_attempt_id_names_a_member_away_from_its_computer() {
        let team = running_team();
        let id = team.task(1).unwrap().attempts[0].id.clone();
        // ask and current accept the attempt id where report already does (N2).
        assert_eq!(attempt_member(&team, &id).as_deref(), Some("dev"));
        assert_eq!(attempt_member(&team, "unknown"), None);
        assert_eq!(attempt_member(&team, ""), None);
    }

    #[test]
    fn a_recorded_stopped_session_makes_the_phase_stopped() {
        let mut team = running_team();
        assert_eq!(view_with(&team, &[], None)["tasks"][0]["attempts"][0]["phase"], "working");
        // The dispatcher saw the session stop: status derives the phase from
        // the record, without a listing of its own (L3).
        team.members.iter_mut().find(|member| member.id == "dev").unwrap().seen = Some(MemberSeen { at: 30.0, live: false });
        assert_eq!(view_with(&team, &[], None)["tasks"][0]["attempts"][0]["phase"], "stopped");
        assert_eq!(view_with(&team, &[], None)["members"][1]["queue"][0]["phase"], json!("stopped"));
        // Seen live again: working.
        team.members.iter_mut().find(|member| member.id == "dev").unwrap().seen = Some(MemberSeen { at: 40.0, live: true });
        assert_eq!(view_with(&team, &[], None)["tasks"][0]["attempts"][0]["phase"], "working");
    }

    #[test]
    fn status_of_a_running_team_reads_no_live_listing() {
        // The desktop polls the watched team's status every 3 s; a listing
        // there is the C1 load and process churn again (L3 regression).
        for status in [TeamStatus::Planning, TeamStatus::Running, TeamStatus::Paused] {
            assert!(!status_lists_sessions(status), "{status:?}");
        }
        for status in [TeamStatus::Done, TeamStatus::Stopped] {
            assert!(status_lists_sessions(status), "{status:?}");
        }
    }

    #[test]
    fn a_leaked_session_env_never_names_the_caller() {
        let mut team = running_team();
        let attempt = team.task(1).unwrap().attempts[0].id.clone();
        let human = json!({"request_id":"11111111-1111-4111-8111-111111111111","by":"human"});
        let foreign = Some("codex/AIBalanceTools/AI BalancerUpdate");
        // Its own session ancestry still proves the member, whatever the env says.
        let own = json!({"name":"codex/x/dev","run_id":"r1"});
        assert_eq!(resolve_actor(&team, "ask", &json!({}), Some(&own), None, foreign).unwrap(), Actor::Member("dev".into()));
        // A dsh member with a leaked env is proven by its conversation.
        team.members.iter_mut().find(|member| member.id == "dev").unwrap().session.as_mut().unwrap().conversation_id = "c1".into();
        assert_eq!(resolve_actor(&team, "ask", &json!({}), None, Some("c1"), foreign).unwrap(), Actor::Member("dev".into()));
        // A report by attempt id is proven too.
        assert_eq!(resolve_actor(&team, "report", &json!({"attempt":attempt}), None, None, foreign).unwrap(), Actor::Member("dev".into()));
        // The leaked env alone proves nothing: it cannot act as the owner.
        let error = resolve_actor(&team, "stop", &human, None, None, foreign).unwrap_err().to_string();
        assert_eq!(error, "Zerus could not verify the calling session");
        // Outside every session, the owner may act.
        assert_eq!(resolve_actor(&team, "stop", &human, None, None, None).unwrap(), Actor::Human);
    }

    #[test]
    fn a_dsh_session_is_known_by_the_conversation_its_harness_exports() {
        let mut team = running_team();
        {
            let dev = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
            dev.session = Some(SessionRef {
                name: "dsh/x/dev".into(),
                run_id: "r1".into(),
                conversation_id: "session-1".into(),
                ..Default::default()
            });
        }
        assert_eq!(dsh_member(&team, "session-1").as_deref(), Some("dev"));
        assert_eq!(dsh_member(&team, "session-2"), None);
        assert_eq!(dsh_member(&team, ""), None);
        // A member's session on another computer is not proven by this one.
        team.members.iter_mut().find(|member| member.id == "dev").unwrap().session.as_mut().unwrap().host = "msi".into();
        assert_eq!(dsh_member(&team, "session-1"), None);
    }

    #[test]
    fn a_captured_report_is_replaced_through_the_report_operation() {
        let mut team = running_team();
        {
            let attempt = &mut team.task_mut(1).unwrap().attempts[0];
            attempt.state = AttemptState::Reported;
            attempt.report = Some(Report { text: "Building and running...".into(), at: 20.0, captured: true, ..Default::default() });
        }
        team.task_mut(1).unwrap().status = TaskStatus::Submitted;
        let mut events = Vec::new();
        let receipt = ops::apply(
            &mut team,
            "report",
            &json!({"task":1,"attempt":"00000000-0000-4000-8000-000000000001","text":"Done: the tests pass."}),
            &Actor::Member("dev".into()),
            &BTreeSet::new(),
            &mut events,
        )
        .unwrap();
        assert_eq!(receipt["status"], "submitted");
        let report = team.task(1).unwrap().attempts[0].report.as_ref().unwrap();
        assert!(!report.captured);
        assert_eq!(report.text, "Done: the tests pass.");
        assert_eq!(report.replaced, vec![ReplacedReport { text: "Building and running...".into(), at: 20.0 }]);
        assert_eq!(events.last().unwrap().kind, "report_replaced");
    }

    #[test]
    fn status_shows_each_member_queue() {
        let mut team = running_team();
        // Task 2 came back to dev and the lead holds it.
        team.tasks.push(Task {
            id: 2,
            title: "Retry".into(),
            assignee: "dev".into(),
            status: TaskStatus::Pending,
            hold: Some(Hold { by: "Lead".into(), at: 12.0, reason: "Later".into() }),
            attempts: vec![Attempt { n: 1, id: "A2".into(), member: "dev".into(), state: AttemptState::Rejected, ..Default::default() }],
            ..Default::default()
        });
        // Assigned but never handed out, and a finished task: neither is queued.
        team.tasks.push(Task { id: 3, title: "Later".into(), assignee: "dev".into(), status: TaskStatus::Pending, ..Default::default() });
        team.tasks.push(Task {
            id: 4,
            title: "Done".into(),
            assignee: "dev".into(),
            status: TaskStatus::Accepted,
            attempts: vec![Attempt { n: 1, id: "A4".into(), member: "dev".into(), state: AttemptState::Reported, ..Default::default() }],
            ..Default::default()
        });
        let expected = json!([{"task":1,"phase":"working"},{"task":2,"phase":"held"}]);
        let value = view_with(&team, &[], None);
        let members = value["members"].as_array().unwrap();
        assert_eq!(members.iter().find(|member| member["id"] == "dev").unwrap()["queue"], expected);
        assert_eq!(members.iter().find(|member| member["id"] == "lead").unwrap()["queue"], json!([]));
        // The session list carries the same queue.
        let listed = summary(&team);
        assert_eq!(listed["members"].as_array().unwrap().iter().find(|member| member["id"] == "dev").unwrap()["queue"], expected);
    }

    fn found_session(name: &str, run: &str, live: bool, cwd: &str) -> Seen {
        Seen {
            name: name.into(),
            run_id: run.into(),
            conversation_id: "conversation-1".into(),
            cwd: cwd.into(),
            live,
            ..Default::default()
        }
    }

    fn existing_member() -> Member {
        Member {
            id: "dev2".into(),
            role: "Backend".into(),
            agent: "dsh".into(),
            session: Some(SessionRef { name: "dsh/x/dev2".into(), ..Default::default() }),
            ..Default::default()
        }
    }

    #[test]
    fn an_existing_session_is_adopted_with_its_own_folder() {
        let mut team = running_team();
        team.folder = "D:/work/shop".into();
        let sessions = vec![found_session("dsh/x/dev2", "r9", true, "D:/work/shop/module")];
        let adopted = adopted_session(&team, &existing_member(), &[], &sessions).unwrap();
        assert_eq!(adopted.directory, "D:/work/shop/module");
        assert!(!adopted.worktree);
        // The run and conversation come from the live session when omitted.
        assert_eq!((adopted.session.run_id.as_str(), adopted.session.conversation_id.as_str()), ("r9", "conversation-1"));
        // A session that is not running, or on another computer, is refused.
        let stopped = adopted_session(&team, &existing_member(), &[], &[found_session("dsh/x/dev2", "r9", false, "D:/w")]);
        assert_eq!(stopped.unwrap_err().to_string(), "dsh/x/dev2 is not running on this computer");
        let missing = adopted_session(&team, &existing_member(), &[], &[]);
        assert_eq!(missing.unwrap_err().to_string(), "dsh/x/dev2 is not running on this computer");
        let mut elsewhere = existing_member();
        elsewhere.computer = "msi".into();
        let remote = adopted_session(&team, &elsewhere, &[], &sessions);
        assert_eq!(remote.unwrap_err().to_string(), "dsh/x/dev2 is not running on msi");
        let mut mismatched = existing_member();
        mismatched.computer = "msi".into();
        mismatched.session.as_mut().unwrap().host = "peer".into();
        let wrong = adopted_session(&team, &mismatched, &[], &sessions);
        assert_eq!(wrong.unwrap_err().to_string(), "dsh/x/dev2 runs on peer, not on msi");
        // A worktree member needs the session inside the team's repository.
        let mut worktree = existing_member();
        worktree.folder = MemberFolder::Worktree;
        let refused = adopted_session(&team, &worktree, &[], &sessions);
        assert!(refused.unwrap_err().to_string().contains("is not the team's repository"));
        // A session on another computer joins in its own shared folder, but a
        // worktree member is refused clearly: git runs here, not there (L5).
        let mut remote = existing_member();
        remote.computer = "msi".into();
        remote.session.as_mut().unwrap().host = "msi".into();
        let remote_sessions = vec![Seen {
            host: "msi".into(),
            name: "dsh/x/dev2".into(),
            live: true,
            cwd: "/work/w".into(),
            ..Default::default()
        }];
        let adopted = adopted_session(&team, &remote, &[], &remote_sessions).unwrap();
        assert_eq!((adopted.directory.as_str(), adopted.worktree), ("/work/w", false));
        remote.folder = MemberFolder::Worktree;
        let refused = adopted_session(&team, &remote, &[], &remote_sessions);
        assert!(refused.unwrap_err().to_string().contains("must be on this computer"));
    }

    #[test]
    fn a_session_busy_in_another_active_team_is_refused() {
        let team = running_team();
        let mut other = running_team();
        other.id = "other".into();
        other.name = "Other".into();
        other.members.iter_mut().find(|member| member.id == "dev").unwrap().session =
            Some(SessionRef { name: "dsh/x/dev2".into(), run_id: "r9".into(), ..Default::default() });
        let sessions = vec![found_session("dsh/x/dev2", "r9", true, "D:/w")];
        let taken = adopted_session(&team, &existing_member(), &[team.clone(), other.clone()], &sessions);
        assert_eq!(taken.unwrap_err().to_string(), "dsh/x/dev2 is a member of team Other");
        // The lead of another active team is refused too.
        other.members.iter_mut().find(|member| member.id == "dev").unwrap().session = None;
        other.members.iter_mut().find(|member| member.id == "lead").unwrap().session =
            Some(SessionRef { name: "dsh/x/dev2".into(), ..Default::default() });
        let lead = adopted_session(&team, &existing_member(), &[team.clone(), other.clone()], &sessions);
        assert_eq!(lead.unwrap_err().to_string(), "dsh/x/dev2 is the lead of team Other");
        // A finished team does not keep the session.
        other.status = TeamStatus::Done;
        assert!(adopted_session(&team, &existing_member(), &[team.clone(), other], &sessions).is_ok());
    }

    fn live_session(session: &SessionRef) -> Seen {
        Seen {
            host: session.host.clone(),
            name: session.name.clone(),
            run_id: session.run_id.clone(),
            conversation_id: session.conversation_id.clone(),
            live: true,
            ..Default::default()
        }
    }

    #[test]
    fn a_finished_team_lists_the_members_whose_sessions_still_run() {
        let mut team = running_team();
        team.status = TeamStatus::Done;
        let dev = team.member("dev").unwrap().session.clone().unwrap();
        let lead = team.member("lead").unwrap().session.clone().unwrap();
        // Even the lead's live session is not left running: the owner must not
        // stop a working lead from this list.
        let sessions = vec![live_session(&dev), live_session(&lead)];
        let value = view_with(&team, &[], Some(&sessions));
        assert_eq!(value["left_running"], json!([{"member":"dev","role":"Backend","session":dev}]));
        // A running team has none, and no live session is an empty list.
        team.status = TeamStatus::Running;
        assert!(view_with(&team, &[], Some(&sessions)).get("left_running").is_none());
        team.status = TeamStatus::Done;
        assert_eq!(view_with(&team, &[], Some(&[]))["left_running"], json!([]));
        // The session list never carries a live listing: only status does (C1).
        assert!(summary(&team).get("left_running").is_none());
    }

    #[test]
    fn left_running_leaves_out_sessions_of_active_teams() {
        let mut done = running_team();
        done.status = TeamStatus::Done;
        let dev = done.member("dev").unwrap().session.clone().unwrap();
        let sessions = vec![live_session(&dev)];
        // The session still runs, but now leads another active team.
        let mut working = running_team();
        working.id = "working".into();
        working.name = "Working".into();
        working.members.iter_mut().find(|member| member.id == "lead").unwrap().session = Some(dev.clone());
        let value = view_with(&done, &[working.clone()], Some(&sessions));
        assert_eq!(value["left_running"], json!([]));
        // The same as a member of an active team.
        working.members.iter_mut().find(|member| member.id == "lead").unwrap().session =
            Some(SessionRef { name: "claude/x/other".into(), ..Default::default() });
        working.members.iter_mut().find(|member| member.id == "dev").unwrap().session = Some(dev.clone());
        let value = view_with(&done, &[working.clone()], Some(&sessions));
        assert_eq!(value["left_running"], json!([]));
        // A paused team keeps the session out too.
        working.status = TeamStatus::Paused;
        assert_eq!(view_with(&done, &[working.clone()], Some(&sessions))["left_running"], json!([]));
        // Once that team is over, the session is left running again.
        working.status = TeamStatus::Done;
        assert_eq!(view_with(&done, &[working], Some(&sessions))["left_running"], json!([{"member":"dev","role":"Backend","session":dev}]));
    }

    #[test]
    fn current_ignores_finished_teams() {
        let mut done = running_team();
        done.id = "done".into();
        done.status = TeamStatus::Done;
        let mut running = running_team();
        running.id = "running".into();
        running.name = "Running".into();
        let dev = |team: &Team| team.members.iter().find(|member| member.id == "dev").map(|member| member.id.clone());
        // Only a finished team is not "a running team" any more (N3).
        let error = current_team(&[done.clone()], "", dev).unwrap_err().to_string();
        assert_eq!(error, "This session is not a member of a running team");
        // The same session in a running team is found; the finished one is not.
        let teams = [done.clone(), running.clone()];
        let (team, member) = current_team(&teams, "", dev).unwrap();
        assert_eq!((team.id.as_str(), member.as_str()), ("running", "dev"));
        assert!(current_team(&[done], "done", dev).is_err());
    }

    #[test]
    fn a_retried_adopted_session_request_returns_its_receipt() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let mut team = running_team();
        team.status = TeamStatus::Planning;
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
        // The first plan records its receipt for a member with a session.
        const PLAN: &str = "44444444-4444-4444-8444-444444444444";
        let input = json!({"members":[{"id":"dev2","role":"Backend","agent":"dsh","session":{"name":"dsh/x/dev2"}}],
            "tasks":[{"id":1,"title":"A","assignee":"dev2"}]});
        let receipt = store
            .mutate(Some((PLAN, &input)), "t", None, |team, events| {
                ops::apply(team, "plan", &input, &Actor::Human, &BTreeSet::new(), events)
            })
            .unwrap();
        // The retry returns the receipt before listing sessions (the named
        // session is not live, so adoption would refuse it) - L4.
        let mut filled = input.clone();
        adopt_existing_sessions(&store, PLAN, &input, "plan", &mut filled).unwrap();
        assert_eq!(filled, input);
        assert_eq!(store.receipt(PLAN, &input).unwrap().unwrap(), receipt);
    }

    #[test]
    fn status_of_a_finished_team_reads_only_active_teams() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let mut done = running_team();
        done.id = "done".into();
        done.status = TeamStatus::Done;
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), done, vec![]).unwrap();
        for n in 0..20u32 {
            let mut old = running_team();
            old.id = format!("old{n}");
            old.status = TeamStatus::Done;
            store.create(&format!("22222222-2222-4222-8222-{n:012}"), &json!({}), old, vec![]).unwrap();
        }
        let mut active = running_team();
        active.id = "active".into();
        active.status = TeamStatus::Paused;
        store.create("33333333-3333-4333-8333-333333333333", &json!({}), active, vec![]).unwrap();
        // The status path reads the indexed active query, not the 21 finished
        // teams (C1).
        let teams = active_team_records(&store).unwrap();
        assert_eq!(teams.iter().map(|team| team.id.as_str()).collect::<Vec<_>>(), vec!["active"]);
    }

    #[test]
    fn the_team_list_is_stored_only() {
        let mut done = running_team();
        done.status = TeamStatus::Done;
        // A list item comes from the stored record alone: no left_running, no
        // live session listing (C1).
        let items = list_items(&[done]);
        assert_eq!(items["items"].as_array().unwrap().len(), 1);
        assert!(items["items"][0].get("left_running").is_none());
        assert!(items["items"][0]["members"][0]["queue"].is_array());
    }

    #[test]
    fn the_inbox_and_status_name_the_leads_next_actions() {
        let mut team = running_team();
        // The report waits for a decision: one inbox item, one wait.
        {
            let attempt = &mut team.task_mut(1).unwrap().attempts[0];
            attempt.state = AttemptState::Reported;
            attempt.report = Some(Report { text: "Done and checked".into(), at: 10.0, ..Default::default() });
        }
        team.task_mut(1).unwrap().status = TaskStatus::Submitted;
        let inbox = inbox_view(&team);
        assert_eq!(inbox["items"][0]["kind"], "report");
        assert_eq!((inbox["items"][0]["task"].as_u64(), inbox["items"][0]["member"].as_str()), (Some(1), Some("Backend")));
        assert!(inbox["items"][0]["command"].as_str().unwrap().contains("hgs team accept --json"));
        let value = view_with(&team, &[], None);
        assert_eq!(value["tasks"][0]["waits"], json!({"for":"lead","on":["Lead"],"unblock":"accept #1"}));
        // An old report shows on the owner's attention list; 0 turns it off.
        let mut old = team.clone();
        old.task_mut(1).unwrap().attempts[0].report.as_mut().unwrap().at = super::super::super::now() - 61.0 * 60.0;
        let value = view_with(&old, &[], None);
        assert_eq!(value["attention"][0]["kind"], "report");
        assert_eq!(value["attention"][0]["task"], 1);
        // The polled team list names the same attention items.
        assert_eq!(summary(&old)["attention"][0]["task"], 1);
        old.settings.remind_owner_minutes = 0;
        assert_eq!(view_with(&old, &[], None)["attention"], json!([]));
        // The settings are optional with the spec's defaults (C2, C4).
        assert_eq!(
            (team.settings.inbox_interval_seconds, team.settings.remind_lead_minutes, team.settings.remind_owner_minutes),
            (120, 15, 60)
        );
        // The inbox is a read, named in the usage.
        assert!(USAGE.contains("inbox"));
        assert!(!MUTATIONS.contains(&"inbox"));
    }

    #[test]
    fn the_commands_that_create_work_start_the_worker() {
        for op in ["approve", "resume", "extend", "approve-phase", "ask", "reply", "release"] {
            assert!(starts_worker(op), "{op}");
        }
        for op in ["report", "cancel", "finish", "discard-phase", "hold"] {
            assert!(!starts_worker(op), "{op}");
        }
    }
}
