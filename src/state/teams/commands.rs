//! WhiteWh fork: `hgs [@host] team <operation> --json`. Every operation reads
//! one JSON object on stdin; changes carry a canonical `request_id` and replay
//! their first receipt.
use super::super::session_graph::caller_record;
use super::super::*;
use super::model::*;
use super::ops::{self, Actor};
use super::plan;
use super::store::{self, canonical_uuid, Event, Store, PAGE};
use anyhow::{bail, ensure, Context};
use std::collections::BTreeSet;

pub(crate) const USAGE: &str = "usage: hgs [@host] team create|plan|extend|edit-draft|move-phase|move-task|add-to-phase|approve-phase|discard-phase|return-to-draft|reject-phase|undo-reject-phase|rework-phase|finish|approve|answer|commit|discard|pause|resume|stop|archive|status|inbox|list|events|spans|profile|current|report|ask|reply|hold|release|accept|reject|skip-stage|send-back|reassign|take-over|cancel|retry|check|expect|swap-member|escalate|use-last-reply|relaunch|add-member|scale|drain|pause-member|resume-member|remove-member|settings|finding|draft|propose|decide-proposal|next-goal|leftovers|intake-key|intake|waiting|close-phase|integrate-build|start-ahead|test|run|templates|safe-action|reports-to|sub-lead --json (JSON stdin) | hgs [@host] team catalog get|set|propose|decide|apply|discard|route|designer --json | hgs team worker [--stop]";

const MUTATIONS: &[&str] = &[
    "plan", "extend", "edit-draft", "move-phase", "move-task", "add-to-phase", "approve-phase", "discard-phase", "return-to-draft", "reject-phase", "undo-reject-phase",
    "rework-phase", "reports-to", "sub-lead", "finish", "approve", "commit", "discard", "pause", "resume", "stop",
    "archive", "report", "ask", "reply", "hold", "release", "accept", "reject", "skip-stage", "send-back", "reassign", "take-over", "cancel",
    "retry", "check", "expect", "swap-member", "use-last-reply", "relaunch", "add-member", "scale", "drain", "pause-member", "resume-member", "remove-member", "settings", "finding",
    "draft", "propose", "close-phase", "integrate-build", "start-ahead",
];

fn text<'a>(input: &'a Value, key: &str) -> &'a str {
    input[key].as_str().unwrap_or_default()
}

/// How many active teams a status or an adoption looks at (C1): the indexed
/// Planning/Running/Paused query, bounded, never every stored team.
const ACTIVE_TEAMS: i64 = 1000;

/// C6: whether this CLI runs in the session of the sub-team's parent lead.
fn parent_lead_calls(store: &Store, team: &Team) -> anyhow::Result<bool> {
    let Some(link) = &team.parent else { return Ok(false) };
    let record = caller_record();
    let dsh = nonempty_env("DSH_SESSION_ID");
    if record.is_none() && dsh.is_none() {
        return Ok(false);
    }
    Ok(parent_lead_proven(team, store.load(&link.team)?.as_ref(), record.as_ref(), dsh.as_deref()))
}

/// The caller is the parent team's lead and no member of the sub-team itself:
/// proven by its process record, or for a dsh lead by its conversation.
fn parent_lead_proven(team: &Team, parent: Option<&Team>, record: Option<&Value>, dsh: Option<&str>) -> bool {
    let (Some(link), Some(parent)) = (&team.parent, parent) else { return false };
    let (here, there) = match record {
        Some(record) => (calling_member(team, record), calling_member(parent, record)),
        None => (dsh_member(team, dsh.unwrap_or_default()), dsh_member(parent, dsh.unwrap_or_default())),
    };
    link.team == parent.id && here.is_none() && there.as_deref() == Some(parent.lead.as_str())
}

/// The member whose session runs this CLI, proven by process ancestry. U54: a
/// renamed session stays the member: the run id and the conversation id match
/// as well as the stored name, so `hgs rename` never drops a member.
fn calling_member(team: &Team, record: &Value) -> Option<String> {
    let name = record["name"].as_str()?;
    let run = record["run_id"].as_str().unwrap_or_default();
    let conversation = record["conversation_id"].as_str().unwrap_or_default();
    team.members
        .iter()
        .find(|member| {
            member.session.as_ref().is_some_and(|session| {
                session.host.is_empty()
                    && (session.name == name
                        || (!run.is_empty() && session.run_id == run)
                        || (!conversation.is_empty() && session.conversation_id == conversation))
            })
        })
        .map(|member| member.id.clone())
}

/// U54: the team record follows a session's rename. A caller proven by its run
/// or conversation under a new name updates the member's stored session name
/// (and fills the identity it was missing), so every later command matches it
/// by name again.
fn follow_session_rename(store: &mut Store, team_id: &str, record: Option<&Value>) -> anyhow::Result<()> {
    let Some(record) = record else { return Ok(()) };
    let name = record["name"].as_str().unwrap_or_default();
    let run = record["run_id"].as_str().unwrap_or_default();
    let conversation = record["conversation_id"].as_str().unwrap_or_default();
    if name.is_empty() || (run.is_empty() && conversation.is_empty()) {
        return Ok(());
    }
    let Some(team) = store.load(team_id)? else { return Ok(()) };
    let matches = |session: &super::model::SessionRef| {
        session.host.is_empty()
            && session.name != name
            && ((!run.is_empty() && session.run_id == run)
                || (!conversation.is_empty() && session.conversation_id == conversation))
    };
    if !team.members.iter().any(|member| member.session.as_ref().is_some_and(matches)) {
        return Ok(());
    }
    store.mutate(None, team_id, None, |team, events| {
        let roles: Vec<(String, String)> = team.members.iter().map(|member| (member.id.clone(), team.role_of(&member.id))).collect();
        for member in &mut team.members {
            let Some(session) = member.session.as_mut() else { continue };
            if session.host.is_empty()
                && session.name != name
                && ((!run.is_empty() && session.run_id == run)
                    || (!conversation.is_empty() && session.conversation_id == conversation))
            {
                let old = session.name.clone();
                session.name = name.to_owned();
                if session.run_id.is_empty() {
                    session.run_id = run.to_owned();
                }
                if session.conversation_id.is_empty() {
                    session.conversation_id = conversation.to_owned();
                }
                let role = roles.iter().find(|(id, _)| *id == member.id).map(|(_, role)| role.clone()).unwrap_or_default();
                events.push(Event::new("member_renamed", json!({"member":role,"from":old,"to":name})));
            }
        }
        Ok(Value::Null)
    })?;
    Ok(())
}

/// U54: the members a bulk swap moves: those of `provider` (any when empty)
/// whose session really runs on `from` (the listing's account, not only the
/// configured one), the lead included.
fn bulk_swap_members(team: &Team, sessions: &[super::dispatcher::Seen], from: &str, provider: &str) -> Vec<String> {
    let native = default_account(from);
    team.members
        .iter()
        .filter(|member| member.state != MemberState::Removed)
        .filter(|member| provider.is_empty() || member.agent == provider)
        .filter(|member| {
            let Some(session) = member
                .session
                .as_ref()
                .filter(|session| session.host.is_empty() && !session.name.is_empty())
            else {
                return false;
            };
            sessions
                .iter()
                .find(|seen| seen.name == session.name && (session.run_id.is_empty() || session.run_id == seen.run_id))
                .is_some_and(|seen| if native { default_account(&seen.account_id) } else { seen.account_id == from })
        })
        .map(|member| member.id.clone())
        .collect()
}

/// The swap-member input of a bulk move: every member that leaves the old
/// account is swapped, the moved ones keeping their session and the fresh ones
/// getting a new one, so an all-fresh move never reads as a single swap.
fn fill_bulk_swap(filled: &mut Value, moved: &[String], fresh: &[String], notes: Vec<Value>, failed: Vec<Value>, to: &str) {
    filled["members"] = json!(moved.iter().chain(fresh).collect::<Vec<_>>());
    filled["fresh"] = json!(fresh);
    filled["notes"] = json!(notes);
    filled["bulk"] = json!(true);
    filled["account"] = json!(to);
    filled["failed"] = json!(failed);
}

/// A switch receipt's status as a word ("rolled_back"), not as JSON.
fn switch_status(receipt: &Value) -> &str {
    receipt["status"].as_str().unwrap_or("unknown")
}

/// U54: "", "native" and "native-<agent>" all name an agent's default
/// account; `hgs ls` names a default-account session "native-claude".
fn default_account(account: &str) -> bool {
    account.is_empty() || account == "native" || account.starts_with("native-")
}

/// U54: a same-agent account move goes through U53's switch, which keeps the
/// session, conversation and attempt; a test replaces the live switch.
#[cfg(test)]
pub(crate) type TestSwitch = fn(&str, &str, &str) -> anyhow::Result<Value>;

fn switch_member_account(name: &str, account: &str, request: &str) -> anyhow::Result<Value> {
    #[cfg(test)]
    if let Some(switch) = tests::test_switch() {
        return switch(name, account, request);
    }
    let config = crate::config::Config::load().map_err(|error| anyhow::anyhow!(error.message))?;
    crate::state::switch_account::switch_member(&config, name, account, request).map_err(|error| anyhow::anyhow!(error))
}

/// U54: the fresh-session fallback. The old session is paused once it is idle
/// (never killed; its conversation stays under Saved) and renamed to
/// `<name>-<old account>` so the member's new session of the same name can
/// start. A busy one is left alone (`Err` names the wait).
fn retire_old_session(name: &str, from: &str) -> std::result::Result<String, String> {
    let record = crate::state::storage::read(name)
        .map_err(|error| format!("the old session was not paused: {error}"))?;
    crate::state::lifecycle::pause_one_quiet(&record, None, true)
        .map_err(|_| "waiting for the old session to finish its turn".to_owned())?;
    let label = from.trim();
    let label = if label.is_empty() || label == "native" {
        "native".to_owned()
    } else {
        label.trim_start_matches("native-").to_owned()
    };
    let renamed = format!("{name}-{label}");
    crate::state::rename::dispatch(&[name.to_owned(), renamed.clone()], false)
        .map_err(|error| format!("the old session was paused but not renamed: {error}"))?;
    Ok(format!("the old session was paused and saved as {renamed}"))
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
    let (dsh, env, run) = caller_identities();
    resolve_actor(team, op, input, caller_record().as_ref(), dsh.as_deref(), env.as_deref(), run.as_deref())
}

/// The ambient session identities of this process (H7, U12): its dsh
/// conversation, its hgs session and its hgs run. A test driving the real
/// entry point replaces them, because the test process itself may run inside
/// a session.
pub(crate) type Identities = (Option<String>, Option<String>, Option<String>);

/// The ambient session identities this process carries, or the test's stand-in
/// for them. `crate::state::owner_request` reads the same three.
pub(crate) fn caller_identities() -> Identities {
    #[cfg(test)]
    if let Some(found) = TEST_CALLER_IDENTITIES.with(|value| value.borrow().clone()) {
        return found;
    }
    (nonempty_env("DSH_SESSION_ID"), nonempty_env("HGS_SESSION"), nonempty_env("HGS_RUN_ID"))
}

#[cfg(test)]
thread_local! {
    static TEST_CALLER_IDENTITIES: std::cell::RefCell<Option<Identities>> = const { std::cell::RefCell::new(None) };
}

/// Sets (or clears) this thread's ambient session identities; used with the
/// private state directory so a command test acts as the owner, not as the
/// session that happens to run the tests.
#[cfg(test)]
pub(crate) fn set_test_caller_identities(identities: Option<Identities>) {
    TEST_CALLER_IDENTITIES.with(|value| *value.borrow_mut() = identities);
}

/// The proven identities decide who asks. A stale `HGS_SESSION` (a resumed host
/// leaks another session's into its members) never names the caller: a member
/// is proven by its ancestry, its dsh conversation or its attempt id, in that
/// order (H7). An agent session marker (U12) never falls through to the owner.
fn resolve_actor(
    team: &Team,
    op: &str,
    input: &Value,
    record: Option<&Value>,
    dsh: Option<&str>,
    env: Option<&str>,
    run: Option<&str>,
) -> anyhow::Result<Actor> {
    if let Some(record) = record {
        let member = calling_member(team, record).with_context(|| format!("This session is not a member of team {}", team.name))?;
        return Ok(Actor::Member(member));
    }
    if let Some(member) = dsh_member(team, dsh.unwrap_or_default()) {
        return Ok(Actor::Member(member));
    }
    if matches!(op, "report" | "ask" | "commit") && text(input, "by") != "human" {
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
    // U12: a dsh session or any other agent session carries a marker; it can
    // never claim to be the owner, even with "by":"human".
    ensure!(
        dsh.is_none() && run.is_none(),
        "An agent session cannot act as the owner"
    );
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
    team.tasks
        .iter()
        .flat_map(|task| &task.attempts)
        .find(|found| found.id == attempt)
        .map(|found| found.member.clone())
        // U36: a stage member reports with its stage run's id.
        .or_else(|| super::pipeline::run_member(team, attempt))
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
/// U42 (#587): the lead's owner word for a phase discard or a return to a
/// draft, verified against its own session and spent on that one change. A
/// lead without a word passes through: ops refuses what needs one.
fn check_phase_change_word(
    team: &Team,
    op: &str,
    input: &Value,
    filled: &mut Value,
    caller: &Actor,
    verify: impl Fn(&Team, &Value, u32, super::owner_word::PhaseChange, f64) -> anyhow::Result<super::model::OwnerWord>,
) -> anyhow::Result<()> {
    if !matches!(caller, Actor::Member(member) if *member == team.lead) {
        return Ok(());
    }
    let Some(word) = input.get("owner_word").filter(|word| word.is_object()) else { return Ok(()) };
    let phase = input["phase"].as_u64().and_then(|phase| u32::try_from(phase).ok()).filter(|phase| *phase > 0).context("A phase number is required")?;
    let (change, what) = if op == "discard-phase" {
        (super::owner_word::PhaseChange::Discard, format!("the discard of phase {phase}"))
    } else {
        (super::owner_word::PhaseChange::ToDraft, format!("the return of phase {phase} to a draft"))
    };
    let verified = verify(team, word, phase, change, super::super::now())?;
    if let Some(used) = team.owner_word_used(verified.event) {
        bail!("That word was already used for {used}; ask the owner again");
    }
    filled["owner_word_checked"] = serde_json::to_value(&verified)?;
    filled["owner_word_event"] = json!(verified.event);
    filled["owner_word_change"] = json!(what);
    Ok(())
}

fn starts_worker(op: &str) -> bool {
    matches!(
        op,
        "approve" | "resume" | "extend" | "approve-phase" | "ask" | "reply" | "release" | "expect" | "swap-member"
            // U36: a send-back or a skipped stage leaves work to hand out.
            | "send-back" | "skip-stage"
            // U25: auto-accept may take a draft on enabling or after a rework.
            | "settings" | "rework-phase"
            // U49: the drain finishes when the member owes nothing; the worker
            // pauses its session and removes it.
            | "drain"
            // N6: a created draft team starts its lead and sends it the brief.
            | "draft"
            // U58: a task moved into the running phase is ready to hand out.
            | "move-task"
    )
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

/// A7: the task whose captured report holds the member back from new work,
/// shown on the member in status and list; absent when nothing holds it.
fn captured_hold(team: &Team, member: &str) -> Option<Value> {
    let task = team.captured_hold(member)?;
    Some(json!({"task":task,"text":format!("No new work: the captured report on #{task} awaits a decision")}))
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

/// F1.7: sessions still running for teams that are finished, at most 200.
/// Nothing is stopped here.
fn leftovers(teams: &[Team], sessions: &[super::dispatcher::Seen]) -> Vec<Value> {
    let busy = |session: &SessionRef| {
        teams.iter().filter(|other| matches!(other.status, TeamStatus::Planning | TeamStatus::Running | TeamStatus::Paused)).any(|other| {
            let same = |other: &SessionRef| other.host == session.host && other.name == session.name;
            other.member(&other.lead).and_then(|lead| lead.session.as_ref()).is_some_and(same)
                || other.members.iter().any(|member| {
                    member.state != MemberState::Removed && member.session.as_ref().is_some_and(same)
                })
        })
    };
    let mut items = Vec::new();
    for team in teams
        .iter()
        .filter(|team| matches!(team.status, TeamStatus::Done | TeamStatus::Stopped | TeamStatus::Archived))
    {
        for member in team.members.iter().filter(|member| member.state != MemberState::Removed && member.id != team.lead) {
            let Some(session) = member.session.as_ref() else { continue };
            let Some(found) = sessions.iter().find(|seen| seen.host == session.host && seen.name == session.name) else { continue };
            if !found.live || busy(session) {
                continue;
            }
            items.push(json!({"team":team.id,"name":team.name,"member":member.id,"role":team.role_of(&member.id),
                "session":session,"since":member.seen.as_ref().map(|seen| seen.at)}));
        }
    }
    items.truncate(200);
    items
}

/// H3: group dsh members by the host process serving them. A member is
/// `host_shared` when its host serves more than one; status keeps the host's
/// pid, start and members.
fn hosts_value(members: &[(String, Option<Value>)]) -> (Value, BTreeSet<String>) {
    let mut hosts: std::collections::BTreeMap<u32, (f64, Vec<String>)> = std::collections::BTreeMap::new();
    let mut shared = BTreeSet::new();
    for (id, info) in members {
        let Some(info) = info else { continue };
        let Some(pid) = info["pid"].as_u64() else { continue };
        let entry = hosts
            .entry(pid as u32)
            .or_insert_with(|| (info["started"].as_f64().unwrap_or(0.0), Vec::new()));
        entry.1.push(id.clone());
    }
    for (_, ids) in hosts.values() {
        if ids.len() > 1 {
            shared.extend(ids.iter().cloned());
        }
    }
    let value = json!(hosts
        .iter()
        .map(|(pid, (started, ids))| json!({"agent":"dsh","pid":pid,"started":started,"members":ids}))
        .collect::<Vec<_>>());
    (value, shared)
}

/// N31: when an attempt stopped being the task's current one: the time its
/// report came, or the end of its turn when it ended without one. An open
/// attempt has no end yet, so the answers arriving now still belong to it.
fn attempt_end(attempt: &Attempt) -> f64 {
    if attempt.state.open() {
        return f64::INFINITY;
    }
    attempt.report.as_ref().map(|report| report.at).or(attempt.turn_ended_at).unwrap_or(f64::INFINITY)
}

/// #538: a team stores the worker's stage facts (the report's file state, the
/// hold from the report and a closed stage's run), but they never reach a
/// client: the status wire carries the runs' `holds`, `changed` and states.
pub(crate) fn strip_stage_facts(value: &mut Value) {
    let Some(tasks) = value["tasks"].as_array_mut() else { return };
    for task in tasks {
        let Some(task) = task.as_object_mut() else { continue };
        if let Some(attempts) = task.get_mut("attempts").and_then(|attempts| attempts.as_array_mut()) {
            for attempt in attempts {
                if let Some(attempt) = attempt.as_object_mut() {
                    attempt.remove("report_files");
                }
            }
        }
        if let Some(pipeline) = task.get_mut("pipeline_run").and_then(|pipeline| pipeline.as_object_mut()) {
            pipeline.remove("hold_from_report");
            if let Some(runs) = pipeline.get_mut("runs").and_then(|runs| runs.as_array_mut()) {
                for run in runs {
                    if let Some(run) = run.as_object_mut() {
                        run.remove("awaiting_run");
                        run.remove("stalled_member");
                        run.remove("prepare");
                        run.remove("context");
                    }
                }
            }
        }
    }
}

/// The team with its live members when the caller listed sessions (F7).
#[cfg(test)]
pub(super) fn view_with(team: &Team, teams: &[Team], sessions: Option<&[super::dispatcher::Seen]>) -> Value {
    view_with_cancelled(team, teams, sessions, true)
}

/// U58: `hgs team status` hides cancelled tasks and the phases whose tasks are
/// all cancelled unless the caller asks for cancelled work; every other view
/// keeps the whole team.
pub(super) fn view_with_cancelled(team: &Team, teams: &[Team], sessions: Option<&[super::dispatcher::Seen]>, include_cancelled: bool) -> Value {
    let mut value = serde_json::to_value(team).unwrap_or_default();
    // #538: the worker's own stage facts are stored with the team but are not
    // part of the status wire (the desktop and Android read the runs and the
    // hold through `holds`, `changed` and the stages' states).
    strip_stage_facts(&mut value);
    // C7: the plan-health block, with the members starving on the plan now.
    let now = super::super::now();
    // Owner 2026-10-31: every task carries its time estimate and, while it
    // runs, the time spent against it; accepted tasks refresh the history.
    super::estimates::fill_status(&mut value, team, now);
    let mut validation = plan::validate(team, &BTreeSet::new());
    validation.health.starved = plan::starved(team, now);
    for starving in &validation.health.starved {
        validation.health.warnings.push(format!(
            "starved: {} idle {} min without a ready task; Open: {}",
            team.name_of(&starving.member),
            starving.minutes,
            starving.blocking.iter().map(|task| format!("#{task}")).collect::<Vec<_>>().join(", ")
        ));
    }
    value["validation"] = json!(&validation);
    value["health"] = json!(&validation.health);
    // C7 refinement: each phase's own health, at the top and on the phase.
    value["phases_health"] = json!(&validation.phases_health);
    // Section 10.2: the workspace header rows (G9).
    value["phase_summary"] = json!(plan::phase_summary(team));
    if let Some(phases) = value["phases"].as_array_mut() {
        for (phase, value) in team.phases.iter().zip(phases) {
            if let Some(health) = validation.phases_health.get(&phase.n.to_string()) {
                value["health"] = json!(health);
            }
        }
    }
    value["layers"] = json!(plan::layers(&team.tasks));
    value["cycle"] = json!(plan::cycle_tasks(&team.tasks));
    value["ready"] = json!(plan::ready(team));
    value["waiting_for_lead"] = json!(waiting_for_lead(team));
    value["pending_phase"] = json!(pending_phase(team));
    // U59: the plan mode (standard or free) the team's phase runs in, and the
    // free phase's own state for the plan header.
    value["plan_mode"] = json!(team.plan_mode);
    // U59: the lead's mode proposal at the start of planning; the owner's
    // approval chooses one of the two.
    value["plan_mode_offer"] = json!(team.plan_mode_offer);
    value["plan_mode_reason"] = json!(team.plan_mode_reason);
    if team.plan_mode == super::model::PlanMode::Free {
        let n = ops::open_phase(team);
        let tasks: Vec<&super::model::Task> = team.tasks.iter().filter(|task| task.phase == n).collect();
        let running: Vec<u32> = tasks.iter().filter(|task| !task.status.finished()).map(|task| task.id).collect();
        let accepted = tasks.iter().filter(|task| task.status == TaskStatus::Accepted).count();
        value["free_phase"] = json!({
            "phase":n,
            "state":if running.is_empty() { "idle" } else { "open" },
            "running":running,
            "accepted":accepted,
            "accepted_since":team.free_accepted_since_build,
            "last_build_at":team.last_build_at,
            "close_pending":team.free_close_pending,
        });
    }
    // U25: only the running phases' open tasks can make the team Stuck.
    value["stuck_scope"] = json!(plan::stuck_scope(team));
    // U21: the command templates Zerus runs outside a member's sandbox: the
    // built-ins (with the global switch) and the project's additions.
    value["templates"] = super::templates::listed();
    // C5: the plan approval view lists every check command with its expected
    // exit code, so the owner sees what will run and where.
    value["checks"] = json!(team
        .tasks
        .iter()
        .filter(|task| task.is_check())
        .map(|task| {
            let check = task.check.as_ref();
            json!({"task":task.id,"title":task.title,
                "command":check.map(|check| check.command.clone()).unwrap_or_default(),
                "expect_exit":check.map(|check| check.expect_exit).unwrap_or(0),
                "timeout_minutes":check.map(|check| check.timeout_minutes).unwrap_or(0)})
        })
        .collect::<Vec<_>>());
    // Each task's last attempt carries its derived phase (spec section 2). The
    // dispatcher records each member's last seen liveness, so `stopped` is
    // derived here without a listing of status's own (L3).
    if let Some(tasks) = value["tasks"].as_array_mut() {
        let ready: BTreeSet<u32> = plan::ready(team).into_iter().collect();
        let now = super::super::now();
        for (task, value) in team.tasks.iter().zip(tasks) {
            // U37: the task's working time and its waits, one number for every
            // client.
            value["timing"] = super::timing::timing(team, task, now);
            if let Some(phase) = ops::attempt_phase(task, stopped(team, task)) {
                if let Some(attempt) = value["attempts"].as_array_mut().and_then(|attempts| attempts.last_mut()) {
                    attempt["phase"] = json!(phase);
                }
            }
            // C3: every open task says what blocks it and which action frees it.
            if let Some(waits) = super::inbox::waits(team, task, ready.contains(&task.id)) {
                value["waits"] = waits;
            }
            // U36/U37: each stage run's waiting and working span.
            let spans = super::pipeline::spans(task, now);
            if !spans.is_empty() {
                value["stage_spans"] = json!(spans);
            }
            // U36: who may take the open stage (never the implementer).
            if task.status.in_stage() {
                value["stage_candidates"] = json!(super::pipeline::candidates(team, task.id));
            }
        }
    }
    // U17/B1: the answers stored on the member while an attempt ran belong to
    // it; the task card lists them with that attempt (newest shown first).
    if let Some(tasks) = value["tasks"].as_array_mut() {
        for (task, value) in team.tasks.iter().zip(tasks) {
            let Some(attempts) = value["attempts"].as_array_mut() else { continue };
            for (index, attempt) in attempts.iter_mut().enumerate() {
                let Some(recorded) = task.attempts.get(index) else { continue };
                // N31: an answer belongs to the member the attempt ran for, not
                // to whoever the task is assigned to now (a reassigned task).
                let Some(member) = team.member(&recorded.member) else { continue };
                if member.permission_answers.is_empty() {
                    continue;
                }
                // N31: the window ends with the attempt, not at the next one
                // only: a closed attempt never collects later answers.
                let end = task.attempts.get(index + 1).map(|next| next.sent_at).unwrap_or_else(|| attempt_end(recorded));
                let answers: Vec<&Value> = member
                    .permission_answers
                    .iter()
                    .filter(|answer| answer["at"].as_f64().is_some_and(|at| at >= recorded.sent_at && at < end))
                    .collect();
                if !answers.is_empty() {
                    attempt["permissions"] = json!(answers);
                }
            }
        }
    }
    // C4: the reports the owner's attention list shows.
    value["attention"] = json!(super::inbox::attention(team, super::super::now()));
    // Section 9.4: a delegated task shows its sub-team, and the team lists its
    // children with rolled-up counts; each level keeps its own tasks and plan
    // health.
    if let Some(tasks) = value["tasks"].as_array_mut() {
        for (task, value) in team.tasks.iter().zip(tasks) {
            let child = teams.iter().find(|child| {
                child.parent.as_ref().is_some_and(|parent| parent.team == team.id && parent.task == task.id)
            });
            if let Some(child) = child {
                let done = child.tasks.iter().filter(|task| task.status.finished()).count();
                value["subteam"] = json!({"team":child.id,"status":child.status,"done":done,"total":child.tasks.len()});
            }
        }
    }
    value["children"] = json!(teams
        .iter()
        .filter(|child| child.parent.as_ref().is_some_and(|parent| parent.team == team.id))
        .map(|child| {
            let done = child.tasks.iter().filter(|task| task.status.finished()).count();
            json!({"team":child.id,"name":child.name,"status":child.status,"done":done,"total":child.tasks.len()})
        })
        .collect::<Vec<_>>());
    // F12: a draft that cannot open yet says so plainly; the tooltip data names
    // the phase before it and that phase's open tasks.
    if let Some(phases) = value["phases"].as_array_mut() {
        for (phase, value) in team.phases.iter().zip(phases) {
            if phase.state != PhaseState::Draft {
                continue;
            }
            let target = phase.order();
            let mut numbers: Vec<(u32, u32)> =
                team.phases.iter().map(|other| (other.n, other.order())).filter(|(n, _)| *n != phase.n).collect();
            numbers.push((1, 1));
            let Some((previous, _)) = numbers.into_iter().filter(|(_, order)| *order < target).max_by_key(|(_, order)| *order) else {
                continue;
            };
            let drafted = team.phases.iter().any(|other| other.n == previous && other.state.is_draft());
            let open: Vec<u32> =
                team.tasks.iter().filter(|task| task.phase == previous && !task.status.finished()).map(|task| task.id).collect();
            // N40 (spec 2.6): behind an approved phase a draft can be approved
            // now (F12), whatever that phase still runs; only a draft before
            // it holds it.
            if drafted {
                value["waiting"] = json!({
                    "text":"Waiting for the previous phase to be approved",
                    "phase":previous,
                    "open":open
                });
            } else if !open.is_empty() {
                // F12/N40: an approved phase that is still running does not
                // block the draft - the owner may approve it to run alongside
                // (the desktop warns before sending). N6: a barrier phase's
                // tasks still wait for the earlier phase, so its text says so.
                let barrier = team.phases.iter().find(|other| other.n == phase.n).is_some_and(|other| other.barrier);
                value["waiting"] = json!({
                    "text": if barrier {
                        format!("Starts when phase {previous} is finished")
                    } else {
                        format!("Can run alongside phase {previous}")
                    },
                    "phase":previous,
                    "open":open,
                    "alongside":true
                });
            }
        }
    }
    // F12: an approved barrier phase starts only when the earlier phases are
    // finished; it says which one still holds it.
    if let Some(phases) = value["phases"].as_array_mut() {
        for (phase, value) in team.phases.iter().zip(phases) {
            let queued = phase.state == PhaseState::Queued;
            if !(queued || phase.state == PhaseState::Approved && phase.barrier) {
                continue;
            }
            let target = phase.order();
            let order_of = |n: u32| team.phases.iter().find(|other| other.n == n).map_or(n, |other| other.order());
            let mut open: Vec<u32> = team
                .tasks
                .iter()
                .filter(|task| order_of(task.phase) < target && !task.status.finished())
                .map(|task| task.id)
                .collect();
            open.sort_unstable();
            // The owner (2026-10-28): name the phase right ahead of this one in
            // the order, the one it queues behind, not the oldest phase that
            // still has a task open ("Phase 43 starts after phase 40" read
            // wrong while 41 and 42 were also ahead of it).
            // A draft ahead (or one sent back) does not hold it.
            let drafted = |n: u32| team.phases.iter().any(|other| other.n == n && other.state.is_draft());
            let Some(previous) = open
                .iter()
                .map(|id| team.task(*id).map(|task| task.phase).unwrap_or(1))
                .filter(|phase| !drafted(*phase))
                .max_by_key(|phase| order_of(*phase))
            else {
                continue;
            };
            // U25: an accepted phase queued behind a running one.
            let text = if queued { format!("Starts after phase {previous}") } else { format!("Starts when phase {previous} is finished") };
            value["waiting"] = json!({
                "text":text,
                "phase":previous,
                "open":open
            });
        }
    }
    // H3: the DeepSeek host(s) serving members, read only from their host pid
    // files; every member of a host with others is marked shared.
    let host_members: Vec<(String, Option<Value>)> = team
        .members
        .iter()
        .filter(|member| member.agent == "dsh")
        .filter_map(|member| {
            let session = member.session.as_ref()?;
            if !session.host.is_empty() {
                return None;
            }
            Some((member.id.clone(), super::super::dsh::host_of(&session.name)))
        })
        .collect();
    let (hosts, shared) = hosts_value(&host_members);
    value["hosts"] = hosts;
    // Each member's queue: its open task first, then the tasks returned to it
    // (A5, spec section 6).
    if let Some(members) = value["members"].as_array_mut() {
        for (member, value) in team.members.iter().zip(members) {
            value["host_shared"] = json!(shared.contains(&member.id));
            value["queue"] = json!(member_queue(team, &member.id));
            if let Some(held) = captured_hold(team, &member.id) {
                value["captured_hold"] = held;
            }
            // #635 N4: the task and stage whose hold keeps this member's folder,
            // so the desktop and Android need not derive the hold from text.
            if let Some((task, stage)) = super::stages::held_by(team, &member.id) {
                value["held_by"] = json!({"task":task,"stage":stage});
            }
            // U36: the stage it has, then the waiting stages it may take.
            value["stage_queue"] = json!(super::stages::queue(team, &member.id));
            // U32: how the member joined on the owner's word, with the quote.
            if let Some(added) = &member.added {
                value["added"] = added.clone();
            }
            // C12: the declared budget, its state and the job it is tied to.
            if let Some(budget) = &member.budget {
                let since = budget.job.as_ref().map(|job| job.since).unwrap_or(budget.declared_at);
                value["budget"] = json!({
                    "label":budget.label,"minutes":budget.minutes,"red_minutes":budget.red_minutes,
                    "since":since,"elapsed_minutes":((now - since).max(0.0) / 60.0).round() as u64,
                    "state":budget.state(now),
                    "command":budget.job.as_ref().map(|job| job.command.clone()).unwrap_or_default()
                });
            }
            if let Some(since) = member.background_since {
                value["background_minutes"] = json!(((now - since).max(0.0) / 60.0).round() as u64);
            }
            // U22/N17: a DeepSeek member's session that still uploads, or whose
            // running host was started before the block, says so on the member
            // and its card.
            if member.agent == "dsh" {
                if let Some(session) = &member.session {
                    if session.host.is_empty() {
                        if let Some(upload) = super::super::dsh::upload_state_for(&session.name).as_object() {
                            for (key, mark) in upload {
                                value[key] = mark.clone();
                            }
                        }
                    }
                }
            }
            // B1: the pending permission request and the answers so far.
            value["question"] = member.permission.clone().unwrap_or(Value::Null);
            value["permission_answers"] = json!(member.permission_answers);
        }
    }
    // Done and Stopped teams say which of their members still run (F7).
    if let Some(sessions) = sessions {
        if matches!(team.status, TeamStatus::Done | TeamStatus::Stopped) {
            value["left_running"] = json!(left_running(team, teams, sessions));
        }
    }
    // F1.6/N37: every member's folder facts, read from the folder the member
    // actually works in (a worktree member's own worktree, not the team folder)
    // and once per distinct folder; a member whose folder is another repository
    // is flagged, not silently accepted. The team's `project` is the GUI's group
    // name (N36), so the folders themselves decide what is outside.
    if let Some(members) = value["members"].as_array_mut() {
        let team_facts = super::dispatcher::folder_facts(&team.folder);
        let mut cache: std::collections::BTreeMap<String, (String, String)> = std::collections::BTreeMap::new();
        cache.insert(team.folder.clone(), team_facts.clone());
        for (member, entry) in team.members.iter().zip(members.iter_mut()) {
            if member.state == MemberState::Removed {
                continue;
            }
            // U54: the real account of the member's session, beside its
            // configured one.
            entry["session_account"] = json!(session_account(member));
            // U56: each member's session phase, activity and minutes without an
            // event, and whether Zerus marks it stuck - status without inspect.
            entry["phase"] = json!(member.seen_phase);
            entry["activity"] = json!(member.seen_activity);
            entry["minutes_without_progress"] =
                json!(member.seen_event_at.map(|at| ((crate::state::now() - at) / 60.0).max(0.0).round() as u64));
            entry["stuck"] = serde_json::to_value(&member.stuck).unwrap_or(Value::Null);
            let folder = super::dispatcher::member_folder_path(team, member).unwrap_or_default();
            // C11: a member on another computer works in a folder there; this
            // computer cannot read it, so its facts stay empty rather than
            // showing a local folder that happens to share the path.
            if !member.computer.is_empty() {
                entry["folder_path"] = json!(folder);
                entry["project"] = json!("");
                entry["branch"] = json!("");
                entry["outside_project"] = json!(false);
                continue;
            }
            let (project, branch) = cache
                .entry(folder.clone())
                .or_insert_with(|| super::dispatcher::folder_facts(&folder))
                .clone();
            let outside = super::dispatcher::outside_project(&team.folder, &folder, &project, &team_facts.0);
            entry["folder_path"] = json!(folder);
            entry["project"] = json!(&project);
            entry["branch"] = json!(branch);
            entry["outside_project"] = json!(outside);
        }
    }
    // D2: the findings summary and the newest page of findings (50).
    let open = |severity: &str| {
        team.findings
            .iter()
            .filter(|finding| finding.severity == severity && finding.unresolved())
            .count()
    };
    let deferred = team.findings.iter().filter(|finding| finding.state == "deferred").count();
    value["findings_summary"] = json!({"open":{"critical":open("critical"),"normal":open("normal"),"later":open("later")},"deferred":deferred});
    value["findings"] = findings_page(team, None);
    // U58: a phase whose tasks are all cancelled reads cancelled; the status
    // hides cancelled tasks and those phases unless the caller asks for them.
    if let Some(phases) = value["phases"].as_array_mut() {
        for (phase, value) in team.phases.iter().zip(phases.iter_mut()) {
            if plan::phase_cancelled(team, phase.n) {
                value["state"] = json!("cancelled");
            }
        }
    }
    if !include_cancelled {
        let cancelled: BTreeSet<u32> = team
            .phases
            .iter()
            .filter(|phase| plan::phase_cancelled(team, phase.n))
            .map(|phase| phase.n)
            .collect();
        if let Some(tasks) = value["tasks"].as_array_mut() {
            let kept: Vec<Value> = team
                .tasks
                .iter()
                .zip(std::mem::take(tasks))
                .filter(|(task, _)| task.status != TaskStatus::Cancelled)
                .map(|(_, value)| value)
                .collect();
            *tasks = kept;
        }
        if let Some(phases) = value["phases"].as_array_mut() {
            let kept: Vec<Value> = team
                .phases
                .iter()
                .zip(std::mem::take(phases))
                .filter(|(phase, _)| !cancelled.contains(&phase.n))
                .map(|(_, value)| value)
                .collect();
            *phases = kept;
        }
        if let Some(summary) = value["phase_summary"].as_array_mut() {
            summary.retain(|phase| phase["n"].as_u64().is_none_or(|n| !cancelled.contains(&(n as u32))));
        }
    }
    value
}

/// D2: one page of findings, newest first, with a stable cursor: `before` is a
/// finding number and the page holds the 50 newest older than it.
fn findings_page(team: &Team, before: Option<u64>) -> Value {
    let number = |id: &str| id.trim_start_matches('f').parse::<u64>().unwrap_or(0);
    let items: Vec<&super::model::Finding> = team
        .findings
        .iter()
        .filter(|finding| before.is_none_or(|before| number(&finding.id) < before))
        .rev()
        .take(50)
        .collect();
    let next_cursor = (items.len() == 50).then(|| items.last().map(|finding| number(&finding.id))).flatten();
    json!({"items":items,"next_cursor":next_cursor})
}

/// U54: the account a member's session really runs on, read from its record -
/// the configured account only while the session is remote or unknown, so the
/// roster never hides where a member really runs.
fn session_account(member: &Member) -> String {
    let Some(session) = member
        .session
        .as_ref()
        .filter(|session| session.host.is_empty() && !session.name.is_empty())
    else {
        return member.account.clone();
    };
    crate::state::storage::read(&session.name)
        .ok()
        .and_then(|record| record["account_id"].as_str().map(str::to_owned))
        .unwrap_or_else(|| member.account.clone())
}

/// A team as the session list shows it: members with their sessions and each
/// task's state, without descriptions, reports or the feed. The list never
/// reads the live session listing: the desktop polls it often (C1).
fn summary(team: &Team, include_cancelled: bool) -> Value {
    let count = |status: TaskStatus| team.tasks.iter().filter(|task| task.status == status).count();
    // U58: the dashboard hides cancelled tasks and the phases whose tasks are
    // all cancelled, unless it asks for them.
    let cancelled: BTreeSet<u32> = team
        .phases
        .iter()
        .filter(|phase| plan::phase_cancelled(team, phase.n))
        .map(|phase| phase.n)
        .collect();
    let members: Vec<Value> = team
        .members
        .iter()
        .filter(|m| m.state != MemberState::Removed)
        .map(|m| {
            let mut value = json!({"id":m.id,"role":team.role_of(&m.id),"agent":m.agent,"model":m.model,"computer":m.computer,
                "account":m.account,"session_account":session_account(m),
                "session":m.session,"state":m.state,"error":m.error,"queue":member_queue(team, &m.id),
                "stage_queue":super::stages::queue(team, &m.id),
                "services":m.services,"question":m.permission});
            if let Some(held) = captured_hold(team, &m.id) {
                value["captured_hold"] = held;
            }
            value
        })
        .collect();
    let tasks: Vec<Value> = team
        .tasks
        .iter()
        .filter(|task| include_cancelled || task.status != TaskStatus::Cancelled)
        .map(|t| {
            let attempt = t.attempts.last();
            json!({"id":t.id,"title":t.title,"status":t.status,"assignee":t.assignee,"phase":t.phase,
                "attempt":attempt.map(|a| json!({"n":a.n,"state":a.state,"no_report":a.no_report,
                    "phase":ops::attempt_phase(t, stopped(team, t))}))})
        })
        .collect();
    // G8: the session row counts the phase in work and every phase, so it needs
    // each task's phase and the phase records (without their change history).
    // U58: a phase whose tasks are all cancelled reads cancelled.
    let phases: Vec<Value> = team
        .phases
        .iter()
        .filter(|p| include_cancelled || !cancelled.contains(&p.n))
        .map(|p| json!({"n":p.n,"order":p.order,"title":p.title,"barrier":p.barrier,
            "state":if cancelled.contains(&p.n) { json!("cancelled") } else { json!(p.state) },"tasks":p.tasks,
            "note":p.note,"rejection":p.rejection}))
        .collect();
    json!({
        "id":team.id,"name":team.name,"status":team.status,"project":team.project,"folder":team.folder,
        "plan_mode":team.plan_mode,
        "lead":team.lead,"members":members,"tasks":tasks,"phases":phases,
        "accepted":count(TaskStatus::Accepted),"submitted":count(TaskStatus::Submitted),
        "in_review":count(TaskStatus::InReview),"in_test":count(TaskStatus::InTest),
        "revision":team.revision,"updated":team.updated,
        "waiting_for_lead":waiting_for_lead(team),"pending_phase":pending_phase(team),
        "stuck_scope":plan::stuck_scope(team),"auto_accept":team.settings.auto_accept,
        // C4: the owner's attention list, also from the polled team list.
        "attention":super::inbox::attention(team, super::super::now()),
        // U57: the lead's open proposals (at most 50), so the lead's session
        // window shows their cards from the polled list.
        "proposals":open_proposals(team),
    })
}

/// U57: the open proposals as the owner's card shows them, without the
/// stored request id their decision replays under.
fn open_proposals(team: &Team) -> Vec<Value> {
    team.proposals
        .iter()
        .filter(|proposal| proposal["state"] == "open")
        .map(|proposal| {
            let mut shown = proposal.clone();
            if let Some(object) = shown.as_object_mut() {
                object.remove("operation_request");
            }
            shown
        })
        .collect()
}

/// N7: stops the lead's session of a discarded draft, when the owner asked for
/// it (spec 3). The session is terminated, not forgotten: it stays in the
/// archive like any stopped session.
fn stop_lead_session(session: &SessionRef) -> anyhow::Result<()> {
    ensure!(!session.name.is_empty(), "The draft's lead has no session to stop");
    #[cfg(test)]
    if RECORD_LEAD_STOPS.with(std::cell::Cell::get) {
        LEAD_STOPS.with(|stops| stops.borrow_mut().push((session.host.clone(), session.name.clone())));
        return Ok(());
    }
    let output = super::dispatcher::hgs(&session.host, &["terminate", &session.name], None, std::time::Duration::from_secs(30))?;
    ensure!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr).trim());
    Ok(())
}

#[cfg(test)]
thread_local! {
    /// Tests record the lead stop instead of terminating a real session.
    static RECORD_LEAD_STOPS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static LEAD_STOPS: std::cell::RefCell<Vec<(String, String)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// N7: from now on, stop_lead_session records its argument instead of running
/// hgs terminate; `lead_stops` reads the record, `record_lead_stops(false)`
/// returns to the real route.
#[cfg(test)]
pub(crate) fn record_lead_stops(record: bool) {
    if record {
        LEAD_STOPS.with(|stops| stops.borrow_mut().clear());
    }
    RECORD_LEAD_STOPS.with(|flag| flag.set(record));
}

#[cfg(test)]
pub(crate) fn lead_stops() -> Vec<(String, String)> {
    LEAD_STOPS.with(|stops| stops.borrow().clone())
}

/// F5: the draft with its revision, unseen fields and preflight. N1/F5: a read
/// by the draft's own lead clears the owner-edited marks - the owner's edit wins
/// only over a lead edit made before the lead read it (spec 3) - so the lead can
/// change those fields after reading.
fn draft_get(input: &Value) -> anyhow::Result<Value> {
    // A lead's read writes the marks away; the owner's read does not. A store
    // the sandbox leaves read-only still reads the draft (the marks stay).
    let mut store = match Store::open(true) {
        Ok(store) => store,
        Err(error) if read_only(&error) => Store::open(false)?,
        Err(error) => return Err(super::store::write_error(error)),
    };
    draft_get_in(&mut store, input)
}

fn draft_get_in(store: &mut Store, input: &Value) -> anyhow::Result<Value> {
    let mut team = store.load(text(input, "team"))?.context("Unknown team")?;
    ensure!(team.status == TeamStatus::Draft, "The team is not a draft");
    let lead_reads = matches!(actor(&team, "draft", input), Ok(Actor::Member(member)) if member == team.lead);
    // N22: the lead must see which fields the owner changed in the answer that
    // clears their marks, or it never learns what the owner edited.
    let mut cleared: Vec<String> = Vec::new();
    let marked = team.draft.as_ref().is_some_and(|draft| !draft.owner_edited.is_empty());
    if lead_reads && marked {
        let marks = team.draft.as_ref().map(|draft| draft.owner_edited.clone()).unwrap_or_default();
        let label = team.role_of(&team.lead);
        let cleared_write = store.mutate(None, &team.id, None, |team, events| {
            if let Some(draft) = team.draft.as_mut() {
                draft.owner_edited.clear();
            }
            events.push(Event::new("draft_read", json!({"by":label})));
            Ok(Value::Null)
        });
        // A read that cannot write (a read-only store) still answers the draft,
        // and then names no cleared field: the marks are still there.
        if cleared_write.is_ok() {
            team = store.load(&team.id)?.context("Unknown team")?;
            cleared = marks;
        }
    }
    let draft = team.draft.clone().unwrap_or_default();
    let validation = super::plan::validate(&team, &BTreeSet::new());
    Ok(json!({"draft":draft,"revision":draft.revision,"unseen":draft.unseen,"owner_edited":cleared,"preflight":validation}))
}

/// F5: the owner starts a draft team; the lead session is adopted when given,
/// otherwise the dispatcher starts it in the team's folder (N6). Nothing exists
/// before this call.
fn draft_start(input: &Value) -> anyhow::Result<Value> {
    let request = text(input, "request_id").to_owned();
    ensure!(super::store::canonical_uuid(&request), "A canonical request_id is required");
    // C2: there is no team yet to prove a member by, so only you start one.
    let (dsh, session, run) = caller_identities();
    owner_without_team(input, caller_record().is_some(), dsh.is_some(), session.is_some(), run.is_some())?;
    let mut store = Store::open(true)?;
    let mut lead = input["lead"].clone();
    if store.receipt(&request, input)?.is_none() && lead["session"].is_object() {
        // C2: the lead's session runs here and leads or serves no other
        // active team, as an adopted member's must (F8).
        if lead.get("id").is_none() {
            lead["id"] = json!("lead");
        }
        let member: Member = serde_json::from_value(lead.clone()).context("Invalid lead")?;
        let adopted = adopted_session(&Team::default(), &member, &store.active_teams(ACTIVE_TEAMS)?, &live_sessions()?)?;
        lead["session"] = serde_json::to_value(&adopted.session)?;
    }
    let value = draft_start_in(&mut store, &request, input, lead)?;
    // N6: the draft's lead is launched and briefed by the dispatcher.
    if let Err(error) = super::dispatcher::ensure_worker() {
        eprintln!("hgs: the team dispatcher did not start: {error}");
    }
    Ok(value)
}

/// C1: the team revision a change must find. The draft, edit-draft,
/// move-phase, discard-phase and extend operations read "revision" as their
/// own (the draft's, or the phase numbers they were made against).
fn expected_revision(op: &str, input: &Value) -> Option<u64> {
    if matches!(op, "draft" | "edit-draft" | "move-phase" | "discard-phase" | "extend") {
        None
    } else {
        input["revision"].as_u64()
    }
}

/// U12/N4: one owner check for every path that can act as the owner. An agent
/// session carries DSH_SESSION_ID, HGS_SESSION or HGS_RUN_ID and is refused.
fn owner_caller(input: &Value, record: bool, dsh: bool, env: bool, run: bool) -> anyhow::Result<()> {
    ensure!(!record && !dsh && !env && !run, "An agent session cannot act as the owner");
    ensure!(text(input, "by") == "human", "Add \"by\":\"human\" to act as yourself");
    Ok(())
}

/// C2: an operation before any team exists is yours: from a process outside
/// every session, saying so.
fn owner_without_team(input: &Value, record: bool, dsh: bool, env: bool, run: bool) -> anyhow::Result<()> {
    owner_caller(input, record, dsh, env, run)
        .context("Only you start a draft team, not an agent session")
}

/// N4/N3: the catalog's owner paths (set, decide, apply, revert, import,
/// designer) run only on your word from outside every agent session. The
/// returned flag is what `catalog::run` takes: false leaves every owner path
/// refused, true lets that operation check its own contract.
fn catalog_owner(input: &Value) -> anyhow::Result<bool> {
    if text(input, "by") != "human" {
        return Ok(false);
    }
    let (dsh, session, run) = caller_identities();
    owner_caller(input, caller_record().is_some(), dsh.is_some(), session.is_some(), run.is_some())?;
    Ok(true)
}

fn draft_start_in(store: &mut Store, request: &str, input: &Value, lead: Value) -> anyhow::Result<Value> {
    if let Some(receipt) = store.receipt(request, input)? {
        return Ok(receipt);
    }
    let folder = text(input, "folder").trim().to_owned();
    ensure!(!folder.is_empty(), "A draft names its folder");
    let name = text(input, "name").trim().to_owned();
    ensure!(name.chars().count() <= 80, "A name has at most 80 characters");
    let mut lead_value = lead;
    if lead_value.get("id").is_none() {
        lead_value["id"] = json!("lead");
    }
    let mut lead: super::model::Member = serde_json::from_value(lead_value).context("Invalid lead")?;
    ensure!(lead.session.is_some() || !lead.agent.is_empty(), "A draft lead needs an agent or an existing session");
    lead.id = "lead".into();
    // N6/spec 3: the draft's lead starts in the team's folder, never a worktree.
    lead.folder = MemberFolder::Shared;
    lead.directory.clear();
    if lead.role.is_empty() {
        lead.role = "Lead".into();
    }
    lead.state = if lead.session.is_some() { MemberState::Active } else { MemberState::NotCreated };
    let mut team = Team {
        id: uuid::Uuid::new_v4().to_string(),
        name: if name.is_empty() { "New team".into() } else { name },
        folder,
        status: TeamStatus::Draft,
        lead: "lead".into(),
        draft: Some(super::model::Draft { revision: 1, ..Default::default() }),
        revision: 1,
        // N6: the dispatcher starts (or adopts) the lead in the folder and gives
        // it the draft brief once (spec 3).
        brief: Notice::Pending,
        ..Default::default()
    };
    team.members.push(lead);
    let id = team.id.clone();
    store.create(request, input, team, vec![Event::new("team_drafted", json!({"team":id}))])?;
    Ok(json!({"team":id,"revision":1}))
}

/// F7: a done team starts the next goal: the same folder, settings, lead and
/// members, their live sessions adopted, the old team kept.
fn next_goal(input: &Value) -> anyhow::Result<Value> {
    let request = text(input, "request_id").to_owned();
    ensure!(super::store::canonical_uuid(&request), "A canonical request_id is required");
    let mut store = Store::open(true)?;
    let value = next_goal_in(&mut store, &request, input)?;
    // N6/N8: the next team's lead gets its planning brief from the dispatcher.
    if let Err(error) = super::dispatcher::ensure_worker() {
        eprintln!("hgs: the team dispatcher did not start: {error}");
    }
    Ok(value)
}

fn next_goal_in(store: &mut Store, request: &str, input: &Value) -> anyhow::Result<Value> {
    let old = store.load(text(input, "team"))?.context("Unknown team")?;
    let caller = actor(&old, "next-goal", input)?;
    next_goal_as(store, request, input, &caller)
}

fn next_goal_as(store: &mut Store, request: &str, input: &Value, caller: &Actor) -> anyhow::Result<Value> {
    if let Some(receipt) = store.receipt(request, input)? {
        return Ok(receipt);
    }
    let old = store.load(text(input, "team"))?.context("Unknown team")?;
    ensure!(old.status == TeamStatus::Done, "Only a done team starts the next goal");
    // N3/N12: one next goal per done team, and the lead takes the accepted
    // proposal's goal rather than a new one.
    ensure!(old.next.is_empty(), "The next goal was already started for this team");
    let accepted_goal = old.proposals.iter().find_map(|proposal| {
        (proposal["kind"] == "next-goal" && proposal["state"] == "accepted")
            .then(|| proposal["goal"].as_str().unwrap_or_default().trim().to_owned())
    });
    ensure!(
        matches!(caller, Actor::Human) || (matches!(caller, Actor::Member(member) if *member == old.lead) && accepted_goal.is_some()),
        "Only you, or the lead with an accepted next-goal proposal, start the next goal"
    );
    let goal = match caller {
        Actor::Human => text(input, "goal").trim().to_owned(),
        _ => accepted_goal.unwrap_or_default(),
    };
    ensure!(!goal.is_empty(), "A next goal needs its goal");
    ensure!(goal.chars().count() <= 4000, "A goal has at most 4000 characters");
    let name = text(input, "name").trim().to_owned();
    ensure!(name.chars().count() <= 80, "A name has at most 80 characters");
    // N8: the members' live sessions are adopted; one that is gone or serves
    // another active team is not. A listing that fails adopts none (the
    // members start fresh when work is ready) rather than refusing the goal.
    let live = live_sessions().unwrap_or_default();
    let next = super::ops::next_team(&old, &goal, &name, &live, &store.active_teams(ACTIVE_TEAMS)?);
    let next_id = next.id.clone();
    store.create(request, input, next, vec![Event::new("next_goal", json!({"team":next_id,"previous":old.id}))])?;
    store.mutate(None, &old.id, None, |team, events| {
        team.next = next_id.clone();
        events.push(Event::new("next_goal_started", json!({"team":next_id})));
        Ok(Value::Null)
    })?;
    Ok(json!({"status":"planned","team":next_id,"previous":old.id}))
}

/// F8: the owner decides a proposal. An accepted one runs as the owner: an
/// accepted next goal creates the next team, an accepted swap calls the swap
/// operation. A decided proposal is idempotent: a replay returns its state.
fn decide_proposal(input: &Value) -> anyhow::Result<Value> {
    let store_path = text(input, "request_id").to_owned();
    ensure!(super::store::canonical_uuid(&store_path), "A canonical request_id is required");
    let mut store = Store::open(true)?;
    let team = store.load(text(input, "team"))?.context("Unknown team")?;
    let caller = actor(&team, "decide-proposal", input)?;
    let receipt = decide_proposal_in(&mut store, input, &caller)?;
    if receipt["status"] == "accepted"
        && matches!(receipt["kind"].as_str(), Some("add-member") | Some("swap-member") | Some("next-goal"))
    {
        // N4: an accepted membership change may bind or launch a session, and a
        // next goal's lead gets its brief; the dispatcher must run.
        if let Err(error) = super::dispatcher::ensure_worker() {
            eprintln!("hgs: the team dispatcher did not start: {error}");
        }
    }
    Ok(receipt)
}

/// C8/N3: the input an accepted add-member or swap-member proposal runs with.
/// A swap's member is its id, or an object with the id and the section 5
/// fields; the operation carries the proposal's own request id, so a retried
/// decision replays it.
fn proposal_input(team: &str, kind: &str, member: &Value, request: &str) -> anyhow::Result<Value> {
    let mut value = json!({"team":team,"request_id":request,"by":"human"});
    if kind == "add-member" {
        ensure!(member.is_object(), "The add-member proposal carries its member");
        value["member"] = member.clone();
        return Ok(value);
    }
    let id = member.as_str().or_else(|| member["id"].as_str()).unwrap_or_default();
    ensure!(!id.is_empty(), "The swap-member proposal names its member");
    value["member"] = json!(id);
    for key in ["agent", "model", "effort", "account", "session"] {
        if let Some(field) = member.get(key).filter(|field| !field.is_null()) {
            value[key] = field.clone();
        }
    }
    Ok(value)
}

/// The tree-budget key of one session, as `tree_session_count` counts it: the
/// live session when there is one, else the member that still needs it.
fn session_key(team: &str, member: &str, session: Option<(&str, &str)>) -> String {
    match session {
        Some((host, name)) => format!("session:{host}:{name}"),
        None => format!("member:{team}:{member}"),
    }
}

/// How many new sessions a change binds, for the tree budget (section 9.2).
fn added_sessions(team: &Team, op: &str, filled: &Value) -> usize {
    match op {
        // N26: an added member without a session is launched once work is
        // ready, so it counts now: leaving it out let a 17th session start at
        // 16/16. A member with an adopted session adds that session.
        "add-member" => 1,
        // U49: scale binds one session per member it adds.
        "scale" => filled["count"].as_u64().unwrap_or(0) as usize,
        "swap-member" => usize::from(
            filled["session"].is_object() && team.member(text(filled, "member")).is_none_or(|member| member.session.is_none()),
        ),
        // N26: a plan replaces the stored roster; only the members whose
        // session is not already counted are added, so a same-size re-plan fits.
        "plan" => {
            let old: BTreeSet<String> = team
                .members
                .iter()
                .filter(|member| member.id != team.lead && member.state != MemberState::Removed)
                .map(|member| {
                    session_key(&team.id, &member.id, member.session.as_ref().map(|session| (session.host.as_str(), session.name.as_str())))
                })
                .collect();
            let new: BTreeSet<String> = filled["members"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|member| {
                    let id = member["id"].as_str().unwrap_or_default();
                    let host = member["session"]["host"].as_str().unwrap_or_default();
                    let name = member["session"]["name"].as_str().unwrap_or_default();
                    if name.is_empty() { session_key(&team.id, id, None) } else { session_key(&team.id, id, Some((host, name))) }
                })
                .collect();
            new.difference(&old).count()
        }
        _ => 0,
    }
}

/// C8: an accepted proposal runs as your own change would: the same session
/// adoption, tree budget and request receipt.
fn owner_change(store: &mut Store, op: &str, value: &Value) -> anyhow::Result<Value> {
    let request = text(value, "request_id").to_owned();
    let team_id = text(value, "team").to_owned();
    let mut filled = value.clone();
    adopt_existing_sessions(store, &request, value, op, &mut filled)?;
    let team = store.load(&team_id)?.context("Unknown team")?;
    let tree = tree_around(&team, &mut |id| store.load(id))?;
    ops::ensure_tree_sessions(&team, &tree, added_sessions(&team, op, &filled))?;
    store.mutate(Some((&request, value)), &team_id, None, |team, events| {
        ops::apply(team, op, &filled, &Actor::Human, &BTreeSet::new(), events)
    })
}

/// N3: the request id an accepted proposal's change runs under. It is stored
/// with the proposal the first time it is needed (proposals made before this
/// field get one then), so a decision retried or interrupted between its writes
/// replays the change instead of running it twice.
fn proposal_operation_request(store: &mut Store, team_id: &str, proposal: &str, team: &Team) -> anyhow::Result<String> {
    if let Some(request) = team.proposals.iter().find(|found| found["id"] == proposal).and_then(|found| found["operation_request"].as_str()) {
        if super::store::canonical_uuid(request) {
            return Ok(request.to_owned());
        }
    }
    let request = uuid::Uuid::new_v4().to_string();
    let mut stored = request.clone();
    store.mutate(None, team_id, None, |team, _| {
        if let Some(found) = team.proposals.iter_mut().find(|found| found["id"] == proposal) {
            match found["operation_request"].as_str() {
                Some(existing) if super::store::canonical_uuid(existing) => stored = existing.to_owned(),
                _ => found["operation_request"] = json!(request),
            }
        }
        Ok(Value::Null)
    })?;
    Ok(stored)
}

fn decide_proposal_in(store: &mut Store, input: &Value, caller: &Actor) -> anyhow::Result<Value> {
    let team_id = text(input, "team").to_owned();
    let team = store.load(&team_id)?.context("Unknown team")?;
    ensure!(matches!(caller, Actor::Human), "Only you decide a proposal");
    let id = text(input, "proposal").to_owned();
    let at = team.proposals.iter().position(|proposal| proposal["id"] == id).context("Unknown proposal")?;
    let state = team.proposals[at]["state"].as_str().unwrap_or_default().to_owned();
    if state != "open" {
        return Ok(json!({"status":state,"proposal":id}));
    }
    // An answer is explicit: a missing or misspelled "accept" never becomes a
    // rejection (RogueDOD, 2026-10-30: "decision":"allow" rejected a proposal).
    let accept = input["accept"].as_bool().context(
        "decide-proposal needs \"accept\": true or false, with \"proposal\" and \"expected_hash\" from the proposal; nothing was decided",
    )?;
    let reason = text(input, "reason").trim().to_owned();
    ensure!(reason.chars().count() <= 2000, "A reason has at most 2000 characters");
    // U57: the owner's Allow is bound to this id and the proposal's content
    // hash; a changed proposal never applies. An older proposal without a hash
    // stays decidable.
    if let Some(hash) = team.proposals[at]["hash"].as_str().filter(|hash| !hash.is_empty()) {
        // The proposal's receipt names it "hash"; both spellings are read.
        let expected = Some(text(input, "expected_hash")).filter(|value| !value.is_empty()).unwrap_or_else(|| text(input, "hash"));
        ensure!(
            !expected.is_empty() && expected == hash,
            "The proposal changed; reload the card and answer again with its hash"
        );
    }
    if !accept {
        store.mutate(None, &team_id, None, |team, events| {
            if let Some(proposal) = team.proposals.iter_mut().find(|proposal| proposal["id"] == id) {
                proposal["state"] = json!("rejected");
                proposal["decided_at"] = json!(super::super::now());
                proposal["note"] = json!(reason);
            }
            events.push(Event::new("proposal_decided", json!({"proposal":id,"accept":false,"reason":reason})));
            if team.proposals.iter().any(|proposal| proposal["id"] == id && proposal["kind"] == "switch-plan-mode") {
                // U59: a Deny's reason reaches the lead with the team's feed.
                events.push(Event::new("plan_mode_denied", json!({"by":"You","proposal":id,"reason":reason})));
            }
            Ok(Value::Null)
        })?;
        return Ok(json!({"status":"rejected","proposal":id}));
    }
    let kind = team.proposals[at]["kind"].as_str().unwrap_or_default().to_owned();
    let member = team.proposals[at]["member"].clone();
    let change = team.proposals[at]["change"].clone();
    let goal = team.proposals[at]["goal"].as_str().unwrap_or_default().to_owned();
    // N3: the change runs under one request id stored with the proposal, so an
    // interrupted or retried decision replays it instead of applying it twice.
    let operation_request = proposal_operation_request(store, &team_id, &id, &team)?;
    let result = match kind.as_str() {
        "add-member" | "swap-member" => {
            let members: Vec<Value> = change["members"].as_array().cloned().unwrap_or_else(|| vec![member.clone()]);
            let mut applied = Vec::new();
            for (index, member) in members.into_iter().enumerate() {
                let request = if index == 0 {
                    operation_request.clone()
                } else {
                    // One request id per member: each change is its own replay.
                    use sha2::{Digest, Sha256};
                    let digest = Sha256::digest(format!("{operation_request}:{index}").as_bytes());
                    let mut bytes = [0u8; 16];
                    bytes.copy_from_slice(&digest[..16]);
                    uuid::Uuid::from_bytes(bytes).to_string()
                };
                let value = proposal_input(&team_id, &kind, &member, &request)?;
                applied.push(owner_change(store, &kind, &value)?);
            }
            json!({"applied":applied.len()})
        }
        "remove-member" => {
            let member_id = member["id"].as_str().or(member.as_str()).unwrap_or_default().to_owned();
            let value = json!({"team":team_id,"request_id":operation_request,"member":member_id,"by":"human"});
            store.mutate(Some((&operation_request, &value)), &team_id, None, |team, events| {
                super::ops::apply(team, "remove-member", &value, &Actor::Human, &BTreeSet::new(), events)
            })?
        }
        "add-to-phase" => {
            let phase = change["phase"].as_u64().context("The add-to-phase proposal carries its phase")?;
            let tasks = change["tasks"].clone();
            let value = json!({"team":team_id,"request_id":operation_request,"by":"human","phase":phase,"tasks":tasks});
            let first = store.mutate(Some((&operation_request, &value)), &team_id, None, |team, events| {
                super::ops::apply(team, "add-to-phase", &value, &Actor::Human, &BTreeSet::new(), events)
            })?;
            // The first step answers a running phase with a warning and its
            // pending id; the owner's Allow is the confirmation, so it is
            // applied at once (the receipt names it "pending").
            match first["confirm"].as_str().or_else(|| first["pending"].as_str()).filter(|confirm| !confirm.is_empty()) {
                Some(confirm) => {
                    // Its own request id, derived from the stored one, so a
                    // replayed decision replays this step too.
                    use sha2::{Digest, Sha256};
                    let digest = Sha256::digest(format!("{operation_request}:confirm").as_bytes());
                    let mut bytes = [0u8; 16];
                    bytes.copy_from_slice(&digest[..16]);
                    let confirm_request = uuid::Builder::from_random_bytes(bytes).into_uuid().to_string();
                    let value = json!({"team":team_id,"request_id":confirm_request,"by":"human","phase":phase,
                        "tasks":change["tasks"],"confirm":confirm,"confirmation":"add"});
                    store.mutate(Some((&confirm_request, &value)), &team_id, None, |team, events| {
                        super::ops::apply(team, "add-to-phase", &value, &Actor::Human, &BTreeSet::new(), events)
                    })?
                }
                None => first,
            }
        }
        "cancel-tasks" => {
            let mut cancelled = 0;
            for id in change["tasks"].as_array().into_iter().flatten().filter_map(Value::as_u64) {
                let request = uuid::Uuid::new_v4().to_string();
                let value = json!({"team":team_id,"request_id":request,"by":"human","task":id});
                store.mutate(Some((&request, &value)), &team_id, None, |team, events| {
                    super::ops::apply(team, "cancel", &value, &Actor::Human, &BTreeSet::new(), events)
                })?;
                cancelled += 1;
            }
            json!({"cancelled":cancelled})
        }
        "discard-phase" => {
            let phase = change["phase"].as_u64().context("The discard-phase proposal carries its phase")?;
            let value = json!({"team":team_id,"request_id":operation_request,"by":"human","phase":phase});
            store.mutate(Some((&operation_request, &value)), &team_id, None, |team, events| {
                super::ops::apply(team, "discard-phase", &value, &Actor::Human, &BTreeSet::new(), events)
            })?
        }
        "next-goal" => {
            ensure!(!goal.is_empty(), "The next-goal proposal carries its goal");
            // N2: it runs as your own next goal, not as the caller re-derived.
            let value = json!({"team":team_id,"goal":goal,"request_id":operation_request,"by":"human"});
            next_goal_as(store, &operation_request, &value, &Actor::Human)?
        }
        "switch-plan-mode" => {
            // U59: the desktop's dialog decides through this path; the switch
            // applies only the mode the proposal names.
            let mode = team.proposals[at]["mode"].as_str().unwrap_or_default().to_owned();
            let asked = team.proposals[at]["reason"].clone();
            store.mutate(None, &team_id, None, |team, events| {
                team.plan_mode = if mode == "free" { super::model::PlanMode::Free } else { super::model::PlanMode::Standard };
                events.push(Event::new("plan_mode_switched", json!({"by":"You","mode":mode,"proposal":id,"reason":asked})));
                Ok(Value::Null)
            })?;
            json!({"status":"switched","plan_mode":mode})
        }
        other => bail!("Unknown proposal kind {other}"),
    };
    store.mutate(None, &team_id, None, |team, events| {
        if let Some(proposal) = team.proposals.iter_mut().find(|proposal| proposal["id"] == id) {
            proposal["state"] = json!("accepted");
            proposal["decided_at"] = json!(super::super::now());
            proposal["note"] = json!(reason);
        }
        events.push(Event::new("proposal_decided", json!({"proposal":id,"accept":true,"kind":kind})));
        Ok(Value::Null)
    })?;
    Ok(json!({"status":"accepted","proposal":id,"kind":kind,"result":result}))
}

/// N8: a sub-team's create input cannot set an owner-only setting.
fn strip_subteam_owner_settings(settings: &mut Value) {
    if let Some(object) = settings.as_object_mut() {
        for key in ops::OWNER_SETTINGS {
            object.remove(key);
        }
    }
}

/// Who creates a sub-team for a parent task (section 9.1): the assignee's own
/// session, the parent lead on its behalf, or the owner.
#[derive(Clone, Copy, Debug, PartialEq)]
enum ParentCaller {
    Assignee,
    ParentLead,
    Owner,
}

/// The proven caller of a sub-team create: a session record (name and run),
/// else the parent member's dsh conversation, else the owner only when no
/// session env can name another caller and the request says it is you.
fn parent_caller(
    team: &Team,
    assignee: &str,
    record: Option<&Value>,
    dsh: Option<&str>,
    env: Option<&str>,
    run: Option<&str>,
    by: &str,
) -> anyhow::Result<ParentCaller> {
    let resolved = if let Some(record) = record {
        Some(
            calling_member(team, record)
                .with_context(|| format!("This session is not a member of team {}", team.name))?,
        )
    } else {
        dsh_member(team, dsh.unwrap_or_default())
    };
    match resolved.as_deref() {
        Some(id) if id == assignee => Ok(ParentCaller::Assignee),
        Some(id) if id == team.lead => Ok(ParentCaller::ParentLead),
        Some(_) => bail!("A sub-team is created from the parent task's assignee, the parent lead or you"),
        None => {
            // U12: an agent session never falls through to the owner.
            ensure!(dsh.is_none() && run.is_none(), "An agent session cannot act as the owner");
            ensure!(env.is_none(), "Zerus could not verify the calling session");
            ensure!(
                by == "human",
                "A sub-team is created from the parent task's assignee, the parent lead or you"
            );
            Ok(ParentCaller::Owner)
        }
    }
}

/// Loads the tree around a team by id: its ancestors (at most 32 links, enough
/// to find the master) and, from the master, its children down to the master's
/// declared depth, at most 32 per team. A flat team reads nothing extra, and
/// no call ever lists every team (C1).
fn tree_around(team: &Team, load: &mut dyn FnMut(&str) -> anyhow::Result<Option<Team>>) -> anyhow::Result<Vec<Team>> {
    let mut loaded = vec![team.clone()];
    let mut ids: BTreeSet<String> = [team.id.clone()].into_iter().collect();
    let mut current = team.clone();
    let mut links = 0;
    while let Some(parent) = current.parent.clone() {
        if links >= 32 || !ids.insert(parent.team.clone()) {
            break;
        }
        match load(&parent.team)? {
            Some(next) => {
                loaded.push(next.clone());
                current = next;
                links += 1;
            }
            None => {
                ids.remove(&parent.team);
                break;
            }
        }
    }
    // The topmost loaded team is the master; the depth budget is its own.
    let master = loaded
        .iter()
        .rev()
        .find(|candidate| candidate.parent.is_none())
        .cloned()
        .unwrap_or_else(|| team.clone());
    let mut queue: Vec<(Team, u32)> = vec![(master.clone(), 0)];
    while let Some((current, depth)) = queue.pop() {
        if depth >= master.settings.tree_depth.max(1) {
            continue;
        }
        for child in current.children.iter().take(32) {
            if !ids.insert(child.clone()) {
                continue;
            }
            match load(child)? {
                Some(next) => {
                    loaded.push(next.clone());
                    queue.push((next, depth + 1));
                }
                None => {
                    ids.remove(child);
                }
            }
        }
    }
    Ok(loaded)
}

/// Section 9.1: creates a sub-team from its parent task. The assignee's
/// session becomes the sub-team's lead; the caller must be that session, the
/// parent lead, or the owner. Returns (parent team, task) for the link.
fn parent_create(input: &Value, filled: &mut Value) -> anyhow::Result<Option<(String, u32)>> {
    if !input["parent"].is_object() {
        return Ok(None);
    }
    let store = Store::open(false)?;
    let parent = store
        .load(text(&input["parent"], "team"))?
        .context("Unknown parent team")?;
    ensure!(
        matches!(parent.status, TeamStatus::Running | TeamStatus::Paused),
        "Only a running or paused team delegates a sub-team"
    );
    let number = input["parent"]["task"]
        .as_u64()
        .and_then(|number| u32::try_from(number).ok())
        .filter(|number| *number > 0)
        .context("The parent task number is required")?;
    let task = parent.task(number).context("Unknown parent task")?;
    ensure!(!task.status.finished(), "The parent task is already decided");
    // N25: one sub-team per parent task; a second create must not overwrite it.
    ensure!(task.subteam.is_none(), "The parent task already has a sub-team");
    let member = parent
        .member(&task.assignee)
        .with_context(|| format!("The parent task #{} has no assignee", task.id))?
        .clone();
    let session = member
        .session
        .clone()
        .with_context(|| format!("{} has no session to lead a sub-team", member.id))?;
    // N3: the same caller identities every other gate reads (a test replaces
    // them, so an agent marker in the test process never decides the caller).
    let (dsh, env, run) = caller_identities();
    parent_caller(
        &parent,
        &member.id,
        caller_record().as_ref(),
        dsh.as_deref(),
        env.as_deref(),
        run.as_deref(),
        text(input, "by"),
    )?;
    // The assignee's session becomes the sub-team's lead.
    filled["lead"] = json!({
        "id":member.id,"role":if member.role.is_empty(){"Lead"}else{member.role.as_str()},"agent":member.agent,
        "account":member.account,"model":member.model,"effort":member.effort,"permissions":member.permissions,
        "permissions_profile":member.permissions_profile,"computer":member.computer,"folder":member.folder,
        "directory":if member.directory.is_empty(){parent.folder.clone()}else{member.directory.clone()},
        "session":session,"prompt":member.prompt
    });
    filled["parent"] = json!({"team":parent.id,"task":task.id,"lead":parent.lead,"member":member.id});
    if text(filled, "folder").is_empty() {
        filled["folder"] = json!(if member.directory.is_empty() { parent.folder.clone() } else { member.directory.clone() });
    }
    if text(filled, "project").is_empty() {
        filled["project"] = json!(parent.project.clone());
    }
    // N8: a sub-team's create input cannot carry an owner-only setting; the
    // parent's sub-team approval is inherited and the plan stays review.
    if filled["settings"].is_object() {
        let mut settings = filled["settings"].clone();
        strip_subteam_owner_settings(&mut settings);
        filled["settings"] = settings;
    }
    // Section 9.3: a sub-team inherits who approves its plan unless the
    // request says otherwise.
    if filled["settings"].is_null() {
        filled["settings"] = json!({"subteam_approval": parent.settings.subteam_approval});
    } else if filled["settings"].get("subteam_approval").is_none() {
        filled["settings"]["subteam_approval"] = json!(parent.settings.subteam_approval);
    }
    // N27: a sub-team's plan and phases are approved by its parent lead (or
    // you), never auto; its create input cannot turn approval off.
    filled["settings"]["approval"] = json!("review");
    filled["settings"]["phase_approval"] = json!("review");
    Ok(Some((parent.id.clone(), task.id)))
}

/// N25: a create whose child landed but whose parent update failed is repaired
/// on the replay: the reserved child id is already the request id.
fn repair_parent_link(child_id: &str, input: &Value) -> anyhow::Result<()> {
    let Some(parent_id) = input["parent"]["team"].as_str() else { return Ok(()) };
    let Some(number) = input["parent"]["task"].as_u64().and_then(|n| u32::try_from(n).ok()) else { return Ok(()) };
    let mut store = Store::open(true)?;
    if store.load(child_id)?.is_none() {
        return Ok(());
    }
    store.mutate(None, parent_id, None, |parent, events| {
        if let Some(task) = parent.tasks.iter_mut().find(|task| task.id == number) {
            task.subteam = Some(child_id.to_owned());
        }
        if !parent.children.iter().any(|id| id == child_id) {
            parent.children.push(child_id.to_owned());
        }
        events.push(Event::new("subteam_repaired", json!({"team":child_id,"task":number})));
        Ok(Value::Null)
    })?;
    Ok(())
}

/// N25: takes back the parent task's sub-team reservation after a create that
/// failed before the child existed, so a later create can delegate it again.
fn release_reservation(parent_link: &Option<(String, u32)>, child_id: &str) {
    let Some((parent_id, task)) = parent_link else { return };
    let Ok(mut store) = Store::open(true) else { return };
    let number = *task;
    let ours = child_id.to_owned();
    let _ = store.mutate(None, parent_id, None, |parent, _events| {
        if let Some(parent_task) = parent.tasks.iter_mut().find(|candidate| candidate.id == number) {
            if parent_task.subteam.as_deref() == Some(ours.as_str()) {
                parent_task.subteam = None;
            }
        }
        Ok(Value::Null)
    });
}

fn create(input: &Value) -> anyhow::Result<Value> {
    let request = text(input, "request_id");
    ensure!(canonical_uuid(request), "A canonical request_id is required");
    // A replay returns the first receipt; a parent link is repaired first
    // (N25: the child may exist from a create whose parent mutate failed).
    if let Some(receipt) = Store::open(false)?.receipt(request, input)? {
        if input["parent"].is_object() {
            repair_parent_link(request, input)?;
        }
        return Ok(receipt);
    }
    let mut filled = input.clone();
    let parent_link = parent_create(input, &mut filled)?;
    if parent_link.is_none() {
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
            // C2/N3: only you start a team from outside the lead's session,
            // through the same gate every owner path uses.
            let (dsh, session, run) = caller_identities();
            owner_caller(input, false, dsh.is_some(), session.is_some(), run.is_some())
                .context("Run this from the lead's session, or add \"by\":\"human\" to act as yourself")?;
        }
    }
    let (team, events, child_id) = {
        let mut child_id = uuid::Uuid::new_v4().to_string();
        if let Some((parent_id, task)) = &parent_link {
            // N25: reserve the parent link before the child exists. The request
            // id is the child id, so a retry after a failed create reuses it;
            // a different concurrent create sees the reservation and is refused.
            child_id = request.to_owned();
            let mut store = Store::open(true)?;
            let pid = parent_id.clone();
            let number = *task;
            store.mutate(None, &pid, None, |parent, events| {
                let parent_task = parent.tasks.iter_mut().find(|candidate| candidate.id == number).context("The parent task is missing")?;
                match &parent_task.subteam {
                    Some(existing) if existing != &child_id => bail!("The parent task already has a sub-team"),
                    _ => parent_task.subteam = Some(child_id.clone()),
                }
                events.push(Event::new("subteam_reserved", json!({"team":child_id,"task":number})));
                Ok(Value::Null)
            })?;
        }
        match ops::create(&child_id, &filled) {
            Ok(created) => (created.0, created.1, child_id),
            Err(error) => {
                // N25: a create refused before the child exists releases the
                // reservation, or no later delegation of the task could happen.
                release_reservation(&parent_link, &child_id);
                return Err(error);
            }
        }
    };
    // Section 9.2: depth, children and tree sessions are checked at create.
    if let Some((parent_id, _)) = &parent_link {
        let check = (|| -> anyhow::Result<()> {
            let store = Store::open(false)?;
            let parent = store.load(parent_id)?.context("The parent team is no longer active")?;
            // Bounded: the parent's chain by id and its children down to the
            // master's depth; this never lists every team.
            let tree = tree_around(&parent, &mut |id| store.load(id))?;
            let master = ops::root_of(&parent, &tree);
            ops::ensure_tree_create(&parent, &master, &tree, &team)
        })();
        if let Err(error) = check {
            // N25: a refused depth, children or session check releases it too.
            release_reservation(&parent_link, &child_id);
            return Err(error);
        }
    }
    let start = team.brief == Notice::Pending;
    // The request's identity is what the caller sent, so a replay matches.
    let receipt = match Store::open(true)?.create(request, input, team.clone(), events) {
        Ok(receipt) => receipt,
        Err(error) => {
            release_reservation(&parent_link, &child_id);
            return Err(error);
        }
    };
    if let Some((parent_id, task)) = parent_link {
        let mut store = Store::open(true)?;
        let child = team.id.clone();
        store.mutate(None, &parent_id, None, |parent, events| {
            if !parent.children.contains(&child) {
                parent.children.push(child.clone());
            }
            if let Some(parent_task) = parent.tasks.iter_mut().find(|candidate| candidate.id == task) {
                parent_task.subteam = Some(child.clone());
            }
            events.push(Event::new("subteam_created", json!({"team":child,"task":task})));
            Ok(Value::Null)
        })?;
    }
    if start {
        if let Err(error) = super::dispatcher::ensure_worker() {
            eprintln!("hgs: the team dispatcher did not start: {error}");
        }
    }
    Ok(receipt)
}

/// The lead (or you) answers a member's question or permission request in the
/// member's session, by its exact id and hash.
/// Whether a native option label is the deny answer (B2). The permission
/// record's `answer` picks its deny option with the same rule (C4).
pub(crate) fn denies(label: &str) -> bool {
    let label = label.trim().to_lowercase();
    label.starts_with("deny") || label.starts_with("no") || label == "reject"
}

fn answer(input: &Value) -> anyhow::Result<Value> {
    let team = Store::open(false)?.load(text(input, "team"))?.context("Unknown team")?;
    // Section 9.6: a sub-team's permission request may be answered one level
    // up by the parent lead, who is proven in the parent team.
    let parent = team.parent.clone().and_then(|link| Store::open(false).ok()?.load(&link.team).ok().flatten());
    let parent_authority = parent.as_ref().is_some_and(|parent| {
        caller_record()
            .as_ref()
            .is_some_and(|record| calling_member(parent, record).as_deref() == Some(parent.lead.as_str()))
    });
    let by = if parent_authority {
        Actor::Member(parent.as_ref().map(|parent| parent.lead.clone()).unwrap_or_default())
    } else {
        actor(&team, "answer", input)?
    };
    ensure!(
        matches!(&by, Actor::Human) || matches!(&by, Actor::Member(id) if *id == team.lead) || parent_authority,
        "Only the lead or the parent lead answers a member's question"
    );
    let member_id = text(input, "member").to_owned();
    let member = team.member(&member_id).context("Unknown member")?;
    // N20: the lead's own permission request goes to the owner, never into the
    // mailbox the lead answers; a lead that answered itself would also decide
    // the whole subagent group it was asked to confirm.
    ensure!(member_id != team.lead, "The lead's own permission request is the owner's to answer");
    let session = member.session.clone().context("The member has no session")?;
    let (question, hash) = (text(input, "question_id").to_owned(), text(input, "expected_question_hash").to_owned());
    ensure!(!question.is_empty() && !hash.is_empty() && input["answers"].is_array(), "question_id, expected_question_hash and answers are required");
    let inspected = super::dispatcher::hgs(&session.host, &["inspect", &session.name, "--skip-processes"], None, std::time::Duration::from_secs(45))?;
    ensure!(inspected.status.success(), "{}", String::from_utf8_lossy(&inspected.stderr).trim());
    let record: Value = serde_json::from_slice(&inspected.stdout).context("invalid inspect output")?;
    let card = record["pending_questions"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|card| card["question_id"] == question.as_str() && card["question_hash"] == hash.as_str())
        .cloned()
        .context("This question changed or is no longer pending")?;
    let request = json!({"request_id":uuid::Uuid::new_v4().to_string(),"expected_run_id":session.run_id,
        "expected_conversation_id":session.conversation_id,"question_id":question,"expected_question_hash":hash,"answers":input["answers"]});
    let output = super::dispatcher::hgs(&session.host, &["answer", &session.name, "--json"], Some(&request), std::time::Duration::from_secs(120))?;
    ensure!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr).trim());
    // B2: a denied permission's text reaches the member as one message, and
    // B1: the lead's or owner's answer is recorded with its reason.
    let selected: Vec<&str> = input["answers"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|answer| answer["selected_option_ids"].as_array().into_iter().flatten())
        .filter_map(|id| id.as_str())
        .collect();
    let deny = card["questions"].as_array().into_iter().flatten().any(|item| {
        item["options"].as_array().into_iter().flatten().any(|option| {
            option["id"].as_str().is_some_and(|id| selected.contains(&id))
                && option["label"].as_str().is_some_and(denies)
        })
    });
    let tool = card["questions"]
        .as_array()
        .and_then(|items| items.first())
        .and_then(|item| item["header"].as_str())
        .or_else(|| card["tool"].as_str())
        .unwrap_or_default()
        .to_owned();
    let reason = text(input, "text").trim().to_owned();
    // N5: the answer is recorded even when the denial message cannot be sent.
    let denial_error = if deny && !reason.is_empty() {
        super::dispatcher::send_denial(&session, &tool, &reason).err()
    } else {
        None
    };
    let decision = if deny { "deny" } else { "allow" };
    let who = if matches!(&by, Actor::Human) { "owner" } else { "lead" };
    let mut store = Store::open(true)?;
    store.mutate(None, &team.id, None, |team, events| {
        let member = team.members.iter_mut().find(|found| found.id == member_id).context("unknown member")?;
        // U17: the stored answer keeps the tool and the command the agent asked
        // about, so the desktop can list them beside the decision.
        let (asked_tool, asked_command) = member
            .permission
            .as_ref()
            .map(|pending| {
                (
                    pending["tool"].as_str().unwrap_or_default().to_owned(),
                    pending["command"].as_str().unwrap_or_default().to_owned(),
                )
            })
            .unwrap_or_default();
        if member.permission.as_ref().is_some_and(|pending| pending["question_id"] == question.as_str()) {
            member.permission = None;
        }
        // Answered here: the next question sync must not record it again as
        // answered by unknown.
        if member.pending.as_ref().is_some_and(|pending| pending.question_id == question) {
            member.pending = None;
        }
        member.permission_answers.push(json!({"question_id":question,"decision":decision,"by":who,"at":now(),"reason":reason,
            "tool":asked_tool,"command":asked_command}));
        let excess = member.permission_answers.len().saturating_sub(20);
        member.permission_answers.drain(..excess);
        events.push(Event::new(
            "permission_answered",
            json!({"member":team.role_of(&member_id),"question_id":question,"decision":decision,"by":who}),
        ));
        if let Some(error) = &denial_error {
            events.push(Event::new(
                "permission_denial_failed",
                json!({"member":team.role_of(&member_id),"question_id":question,"error":error.to_string()}),
            ));
        }
        Ok(Value::Null)
    })?;
    Ok(serde_json::from_slice(&output.stdout).unwrap_or_else(|_| json!({"status":"answered"})))
}

/// B4: commits the member's own work in its own folder, on its team branch.
/// Zerus runs git; it never pushes, amends or changes branches.
fn commit(input: &Value) -> anyhow::Result<Value> {
    // L1: a repeated request returns the stored receipt, never a second commit.
    let request = text(input, "request_id");
    ensure!(canonical_uuid(request), "A canonical request_id is required");
    let store = Store::open(false)?;
    if let Some(receipt) = store.receipt(request, input)? {
        return Ok(receipt);
    }
    let team = store.load(text(input, "team"))?.context("Unknown team")?;
    let by = actor(&team, "commit", input)?;
    let Actor::Member(member_id) = by else { bail!("Only a member commits its work") };
    ensure!(member_id != team.lead, "The lead does not commit a member's task");
    drop(store);
    // U20: prove the store is writable before git runs. A member whose sandbox
    // blocks it queues the whole commit instead (the worker runs it), so no
    // commit exists without its attempt recording it.
    Store::open(true)?;
    commit_in_team(&team, &member_id, input)
}

/// The commit itself, as the member's own (B4): the worker applies a queued
/// commit with the same code and checks.
pub(crate) fn commit_in_team(team: &Team, member_id: &str, input: &Value) -> anyhow::Result<Value> {
    let request = text(input, "request_id");
    ensure!(canonical_uuid(request), "A canonical request_id is required");
    if let Some(receipt) = Store::open(false)?.receipt(request, input)? {
        return Ok(receipt);
    }
    ensure!(member_id != team.lead, "The lead does not commit a member's task");
    let member = team.member(member_id).context("Unknown member")?;
    let message = text(input, "message").trim().to_owned();
    ensure!(!message.is_empty() && message.chars().count() <= 4000, "A commit message is required (at most 4000 characters)");
    let folder = super::dispatcher::member_folder_path(team, member)?;
    ensure!(!folder.is_empty() && folder != team.folder, "Zerus commits in the member's own folder, not the team's");
    // U48: with a branch per task, the member's open task's branch is its team
    // branch while that attempt is open.
    let attempt_id = text(input, "attempt");
    let open_task = team.tasks.iter().find(|task| {
        task.open_attempt().is_some_and(|attempt| attempt.member == member_id && (attempt_id.is_empty() || attempt.id == attempt_id))
    });
    let branch = open_task
        .and_then(|task| super::dispatcher::task_branch(team, task.id, member_id))
        .unwrap_or_else(|| format!("team/{}/{}", &team.id[..8.min(team.id.len())], super::dispatcher::tag(&member.id)));
    let paths: Vec<String> = input["paths"].as_array().into_iter().flatten().filter_map(|path| path.as_str()).map(str::to_owned).collect();
    for path in &paths {
        ensure!(!path.is_empty() && !path.starts_with('/') && !path.split(['/', '\\']).any(|part| part == ".."), "A commit path must stay inside the member's folder");
    }
    let result = super::dispatcher::commit_in(&folder, &branch, &message, &paths)?;
    if result["status"] != "committed" {
        // Nothing changed: the receipt still records this request as done.
        Store::open(true)?.mutate(Some((request, input)), &team.id, None, |_, _| Ok(result.clone()))?;
        return Ok(result);
    }
    let sha = result["sha"].as_str().unwrap_or_default().to_owned();
    let files = result["files"].as_u64().unwrap_or(0);
    let attempt_id = text(input, "attempt").to_owned();
    let mut store = Store::open(true)?;
    store.mutate(Some((request, input)), &team.id, None, |team, events| {
        let role = team.role_of(member_id);
        let task = team.tasks.iter_mut().find(|task| {
            task.open_attempt().is_some_and(|attempt| {
                (!attempt_id.is_empty() && attempt.id == attempt_id) || (attempt_id.is_empty() && attempt.member == member_id)
            })
        });
        let Some(task) = task else { bail!("The member has no open attempt to record the commit on") };
        let task_id = task.id;
        let attempt = task.open_attempt_mut().context("the attempt ended")?;
        let n = attempt.n;
        attempt.commits.push(CommitRef { sha: sha.clone(), at: now(), by: "Zerus".into(), files: files as usize });
        let excess = attempt.commits.len().saturating_sub(20);
        attempt.commits.drain(..excess);
        events.push(Event::new("committed", json!({"member":role,"task":task_id,"attempt":n,"sha":sha,"files":files})));
        Ok(Value::Null)
    })?;
    Ok(result)
}

/// What `hgs team current` answers for one member of a team (spec section 5):
/// the live attempt, its phase, its open questions and the report command.
/// Read only.
pub(crate) fn current_view(team: &Team, member: &str) -> anyhow::Result<Value> {
    let Some((task, attempt)) = team
        .tasks
        .iter()
        .rev()
        .filter_map(|task| task.open_attempt().map(|attempt| (task, attempt)))
        .find(|(_, attempt)| attempt.member == member)
    else {
        // U36: a stage member's current attempt is the open stage run given to
        // it; its answer is a verdict, not a report. A waiting stage is not
        // given out yet.
        if let Some((task, run)) = team.tasks.iter().rev().find_map(|task| {
            task.pipeline_run
                .as_ref()
                .and_then(PipelineRun::open_run)
                .filter(|run| run.state == StageState::Delivered && run.member == member)
                .map(|run| (task, run))
        }) {
            return Ok(json!({
                "team": team.id,
                "task": task.id,
                "title": task.title,
                "attempt": run.id,
                "n": run.attempt,
                "stage": run.kind,
                "round": run.round,
                "folder": run.folder,
                "phase": if run.kind == "test" { "in_test" } else { "in_review" },
                "open_questions": [],
                "report": {
                    "request_id": uuid::Uuid::new_v4().to_string(),
                    "team": team.id,
                    "task": task.id,
                    "attempt": run.id,
                    "verdict": "pass|return",
                    "findings": [],
                    "text": "..."
                }
            }));
        }
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
    // U36: a reported task in a stage of its pipeline says which.
    if task.status.in_stage() && attempt.is_some_and(|attempt| attempt.state == AttemptState::Reported) {
        let kind = if task.status == TaskStatus::InTest { "test" } else { "review" };
        return format!("#{} has a report in its {kind} stage", task.id);
    }
    // U46: a hand-out Zerus could not prepare is no delivery the member waits
    // for; the lead or the owner retries it.
    let state = if task.waits.as_ref().is_some_and(|wait| wait.state == "waiting") {
        "is being prepared by Zerus for its next hand-out"
    } else if task.waits.is_some() {
        "could not be prepared by Zerus; the lead or you retry it"
    } else {
        match attempt.map(|attempt| attempt.state) {
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
        }
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

/// U38: the timelines of one task (`task`), or a page of a phase's (`phase`)
/// or the plan's tasks by id after `after`, at most `limit` (50). One bounded,
/// indexed read of the journal serves the page. Read only.
fn spans_view(input: &Value) -> anyhow::Result<Value> {
    use super::spans;
    let store = Store::open(false)?;
    let team = store.load(text(input, "team"))?.context("Unknown team")?;
    let now = now();
    let limit = input["limit"].as_u64().map_or(spans::TASK_PAGE, |n| (n as usize).clamp(1, spans::TASK_PAGE));
    let mut chosen: Vec<&Task> = match input["task"].as_u64() {
        Some(id) => vec![team.task(id as u32).with_context(|| format!("Unknown task #{id}"))?],
        None => {
            let after = input["after"].as_u64().unwrap_or(0);
            let phase = input["phase"].as_u64();
            let mut list: Vec<&Task> = team
                .tasks
                .iter()
                .filter(|task| u64::from(task.id) > after && phase.is_none_or(|phase| u64::from(task.phase) == phase))
                .collect();
            list.sort_by_key(|task| task.id);
            list
        }
    };
    let more = chosen.len() > limit;
    chosen.truncate(limit);
    let (journal, cut) = match spans::window(&team, &chosen, now) {
        Some((from, to)) => store.span_events(&team.id, from, to, spans::EVENT_LIMIT)?,
        None => (Vec::new(), false),
    };
    let holders = spans::holders(&team, now);
    // Owner 2026-10-31: each timeline carries the task's estimate and, while it
    // runs, the time spent against it.
    let history = super::estimates::load();
    let tasks: Vec<Value> = chosen
        .iter()
        .map(|task| {
            let mut value = spans::spans(&team, task, &journal, &holders, now);
            let estimate = super::estimates::estimate_from(&history, &team, task);
            value["estimate_minutes"] = estimate.minutes.map_or(Value::Null, |minutes| json!(minutes));
            value["estimate_source"] = json!(estimate.source);
            value["estimate_text"] = json!(estimate.text());
            if let Some(spent) = super::estimates::spent_minutes(task, now) {
                value["estimate_spent_minutes"] = json!(spent.round());
            }
            if cut {
                let mut unknown: BTreeSet<String> = serde_json::from_value(value["unknown"].clone()).unwrap_or_default();
                unknown.insert("events".into());
                value["unknown"] = json!(unknown);
            }
            value
        })
        .collect();
    let next = if more { chosen.last().map(|task| task.id) } else { None };
    Ok(json!({"team":team.id,"tasks":tasks,"next_cursor":next,"events":{"read":journal.len(),"truncated":cut}}))
}

/// U38: the profile of one task (`task`), a phase (`phase`) or the plan: the
/// losses ranked by time and by tokens (pages of `limit` after `after_time` or
/// `after_tokens`), the planned and the real critical path, per member, role
/// and model the utilization and tokens, and a page of task rows after
/// `after_task`. `compare` [a, b] sets two phases side by side; `tokens`:false
/// reads no session. Read only; one bounded journal read per scope.
fn profile_view(input: &Value) -> anyhow::Result<Value> {
    use super::profile;
    let store = Store::open(false)?;
    let team = store.load(text(input, "team"))?.context("Unknown team")?;
    let now = now();
    let read = input["tokens"].as_bool() != Some(false);
    let page = profile::Page {
        limit: input["limit"].as_u64().map_or(profile::LOSS_PAGE, |n| n as usize),
        after_time: Some(input["after_time"].clone()).filter(Value::is_object),
        after_tokens: Some(input["after_tokens"].clone()).filter(Value::is_object),
        after_task: input["after_task"].as_u64().map_or(0, |n| n as u32),
    };
    // A draft has no timeline yet; cancelled work stays, its time was spent.
    let scope = |phase: Option<u64>| -> Vec<&Task> {
        let mut list: Vec<&Task> = team
            .tasks
            .iter()
            .filter(|task| task.status != TaskStatus::Draft && phase.is_none_or(|phase| u64::from(task.phase) == phase))
            .collect();
        list.sort_by_key(|task| task.id);
        list
    };
    if let Some(pair) = input["compare"].as_array() {
        let phases: Vec<u32> = pair.iter().filter_map(Value::as_u64).filter_map(|n| u32::try_from(n).ok()).collect();
        ensure!(phases.len() == 2, "compare names two phases");
        let first = profile_of(&store, &team, &scope(Some(u64::from(phases[0]))), &profile::Page { limit: 3, ..Default::default() }, read, now)?;
        let second = profile_of(&store, &team, &scope(Some(u64::from(phases[1]))), &profile::Page { limit: 3, ..Default::default() }, read, now)?;
        return Ok(json!({"team":team.id,"compare":profile::compare((phases[0], &first), (phases[1], &second))}));
    }
    let (chosen, label) = match (input["task"].as_u64(), input["phase"].as_u64()) {
        (Some(id), _) => (vec![team.task(id as u32).with_context(|| format!("Unknown task #{id}"))?], json!({"task":id})),
        (None, Some(phase)) => (scope(Some(phase)), json!({"phase":phase})),
        (None, None) => (scope(None), json!({"plan":true})),
    };
    let mut value = profile_of(&store, &team, &chosen, &page, read, now)?;
    value["team"] = json!(team.id);
    value["scope"] = label;
    Ok(value)
}

/// The profile of `chosen` tasks: their timelines from one bounded journal
/// read, the tokens of their spans (at most `profile::TOKEN_READS` reads) and
/// the ranked losses.
fn profile_of(store: &Store, team: &Team, chosen: &[&Task], page: &super::profile::Page, read: bool, now: f64) -> anyhow::Result<Value> {
    use super::{profile, spans};
    let (journal, cut) = match spans::window(team, chosen, now) {
        Some((from, to)) => store.span_events(&team.id, from, to, spans::EVENT_LIMIT)?,
        None => (Vec::new(), false),
    };
    let holders = spans::holders(team, now);
    let timelines: Vec<Value> = chosen.iter().map(|task| spans::spans(team, task, &journal, &holders, now)).collect();
    // #736: every phase's timelines decide whether a member was really idle:
    // work on another phase's task (or one moved between phases) is not idle.
    let all: Vec<Value> = team.tasks.iter().map(|task| spans::spans(team, task, &journal, &holders, now)).collect();
    let asks = profile::asks(&timelines, now);
    let (tokens, capped) = if read {
        profile::read_tokens(team, &asks)
    } else {
        (asks.iter().map(|ask| ((ask.task, ask.part.clone()), None)).collect(), false)
    };
    let mut value = profile::profile(team, chosen, &timelines, &all, &tokens, page, now);
    value["events"] = json!({"read":journal.len(),"truncated":cut});
    value["token_reads"] = json!({"asked":asks.len(),"read":if read { asks.len().min(profile::TOKEN_READS) } else { 0 },"truncated":capped});
    Ok(value)
}

/// #717: the suite's targets from the project's map for the task's diff against
/// the lead branch; an empty list when the project has no map or no change.
fn affected_targets(project: &std::path::Path, team: &str, suite: &str) -> anyhow::Result<Vec<String>> {
    let loaded = crate::test_discipline::load(project, Some(team))?;
    let config = &loaded.config;
    let text = std::fs::read_to_string(project.join(&config.map.file)).map_err(|error| anyhow::anyhow!("{error}"))?;
    let map: crate::test_discipline::map::TestMap = serde_json::from_str(&text)?;
    let durations = crate::test_discipline::durations::Durations::read(&project.join(&config.map.durations)).map_err(|error| anyhow::anyhow!("{error}"))?;
    let changed = super::dispatcher::git_at(&project.to_string_lossy(), &["diff", "--name-only", "teams-v2/lead"]).unwrap_or_default();
    let files: Vec<String> = changed.lines().map(|line| line.trim().to_owned()).filter(|line| !line.is_empty()).collect();
    let selection = crate::test_discipline::affected::select(project, config, &map, &durations, &files, &Default::default(), false);
    Ok(selection.runs.iter().filter(|run| run.provider == suite).flat_map(|run| run.targets.clone()).collect())
}

/// #717: `hgs team test submit|force|results` on the team's test database.
fn test_command(input: &Value) -> anyhow::Result<Value> {
    use super::test_queue::{self, Load, TestStore};
    let team_id = text(input, "team").to_owned();
    let store = Store::open(false)?;
    let team = store.load(&team_id)?.context("Unknown team")?;
    let action = text(input, "action");
    let database = TestStore::open(false)?;
    let project = std::path::PathBuf::from(&team.folder);
    let asked_by = if text(input, "by").is_empty() { input["asked_by"].as_str().unwrap_or_default().to_owned() } else { text(input, "by").to_owned() };
    let load = Load {
        cpu_free: input["cpu_free"].as_u64().map_or(16, |n| n as u32),
        ram_free_mb: input["ram_free_mb"].as_u64().map_or(32_000, |n| n as u32),
        gpu_free: input["gpu_free"].as_bool().unwrap_or(true),
        at_night: input["night"].as_bool().unwrap_or(false),
    };
    let classes = match crate::test_discipline::load(&project, Some(&team_id)) {
        Ok(loaded) => loaded.config.resources,
        Err(_) => Default::default(),
    };
    match action {
        "submit" | "force" => {
            let suite = text(input, "suite");
            ensure!(!suite.is_empty(), "A test request names a suite");
            let mut targets: Vec<String> = input["targets"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect();
            // #717: without listed targets the task's diff against the lead
            // branch names them through the project's test map.
            if targets.is_empty() {
                if let Ok(found) = affected_targets(&project, &team_id, suite) {
                    targets = found;
                }
            }
            let mut request = test_queue::suite_from_config(&project, Some(&team_id), suite, &targets)?;
            request.id = uuid::Uuid::new_v4().to_string();
            request.team = team_id.clone();
            request.task = input["task"].as_u64().and_then(|id| u32::try_from(id).ok());
            request.asked_by = if asked_by.is_empty() { "You".to_owned() } else { asked_by };
            request.forced = action == "force";
            request.commit = if text(input, "commit").is_empty() {
                super::dispatcher::git_at(&project.to_string_lossy(), &["rev-parse", "HEAD"]).unwrap_or_default()
            } else {
                text(input, "commit").to_owned()
            };
            let mut hash = sha2::Sha256::new();
            use sha2::Digest;
            hash.update(request.suite.as_bytes());
            hash.update(request.commit.as_bytes());
            for target in &request.targets {
                hash.update(target.as_bytes());
            }
            request.input_hash = format!("{:x}", hash.finalize());
            if !request.forced {
                if let Some(found) = database.reused(&request.suite, &request.commit, &request.input_hash)? {
                    return Ok(json!({"status":"reused","record":found,"suite":request.suite}));
                }
            }
            database.enqueue(&request)?;
            if action == "submit" {
                return Ok(json!({"status":"queued","request":request,"suite":request.suite}));
            }
            let queued = database.queued(&team_id)?;
            let chosen = test_queue::schedule(&queued, &classes, &load);
            let records = test_queue::run_due(&database, &project, &chosen, super::super::now())?;
            Ok(json!({"status":"ran","suite":request.suite,"results":records}))
        }
        "results" => {
            let limit = input["limit"].as_u64().map_or(20, |n| n as usize);
            let before = input["before"].as_f64();
            let records = database.results(Some(&team_id), before, limit)?;
            let next = records.last().map(|record| record.at);
            Ok(json!({"status":"ok","items":records,"next_cursor":next}))
        }
        other => anyhow::bail!("Unknown test action {other}; use submit, force or results"),
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
                .or_else(|| {
                    // U36: a stage run id names its member the same way.
                    team.tasks
                        .iter()
                        .filter_map(|task| task.pipeline_run.as_ref())
                        .flat_map(|run| &run.runs)
                        .find(|run| run.id == attempt && !run.member.is_empty())
                        .map(|run| (team, run.member.clone()))
                })
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
    #[cfg(test)]
    if let Some(found) = LIVE_SESSIONS.with(|sessions| sessions.borrow().clone()) {
        return Ok(found);
    }
    let output = super::dispatcher::hgs("", &["ls", "--json", "--no-archives"], None, std::time::Duration::from_secs(15))?;
    ensure!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr).trim());
    let listing: Value = serde_json::from_slice(&output.stdout).context("invalid hgs ls output")?;
    Ok(super::dispatcher::seen(&listing))
}

#[cfg(test)]
thread_local! {
    /// N3/N4: a test's own live listing instead of a real `hgs ls`.
    static LIVE_SESSIONS: std::cell::RefCell<Option<Vec<super::dispatcher::Seen>>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn live_sessions_override(sessions: Option<Vec<super::dispatcher::Seen>>) {
    LIVE_SESSIONS.with(|slot| *slot.borrow_mut() = sessions);
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
    // C3: a draft's members bind sessions under the same checks.
    let drafted = op == "draft" && text(input, "action") == "update" && team.status == TeamStatus::Draft;
    let joined = op == "add-member" && matches!(team.status, TeamStatus::Planning | TeamStatus::Running | TeamStatus::Paused);
    let swapped = op == "swap-member"
        && matches!(team.status, TeamStatus::Planning | TeamStatus::Running | TeamStatus::Paused)
        && filled["session"].is_object();
    if planned {
        if let Some(members) = filled["members"].as_array_mut() {
            adopt_members(store, &team, members)?;
        }
    } else if drafted {
        // The lead keeps its own session; the others are checked.
        for member in filled["set"]["members"].as_array_mut().into_iter().flatten().filter(|member| member["id"] != team.lead.as_str()) {
            adopt_members(store, &team, std::slice::from_mut(member))?;
        }
    } else if joined {
        adopt_members(store, &team, std::slice::from_mut(&mut filled["member"]))?;
    } else if swapped {
        // H6: the session named for the swap is adopted under the same checks
        // as a new member's existing session.
        let member = team.member(text(input, "member")).context("Unknown member")?.clone();
        let mut with_session = member;
        with_session.session = serde_json::from_value(filled["session"].clone()).ok();
        let sessions = live_sessions()?;
        let teams = store.active_teams(ACTIVE_TEAMS)?;
        let adopted = adopted_session(&team, &with_session, &teams, &sessions)?;
        filled["session"] = serde_json::to_value(&adopted.session)?;
        // N32: the adopted session's own folder, as for a new member's.
        filled["directory"] = json!(adopted.directory);
        filled["folder"] = serde_json::to_value(if adopted.worktree { MemberFolder::Worktree } else { MemberFolder::Shared })?;
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

/// U20/U42: the member writes a sandbox may not put in the store, queued in
/// its own folder: a report, a question, a commit or a budget (`expect`).
fn queues_instead(op: &str) -> bool {
    matches!(op, "report" | "ask" | "commit" | "reply" | "expect")
}

/// The team's files refuse writes here: SQLite opened them read-only, or the
/// system denies access (in any language: Windows localizes its message).
fn read_only(error: &anyhow::Error) -> bool {
    let text = format!("{error:#}").to_lowercase();
    ["readonly", "read-only", "access is denied", "permission denied", "os error 5)", "os error 13)", "unable to open database"]
        .iter()
        .any(|sign| text.contains(sign))
}

/// N5: where a member's sandboxed write queues - the folder the worker reads a
/// member's outbox from: the member's own folder as the team records it, never
/// this process's directory. A member that ran `cd tray` and then reports would
/// otherwise leave the request in `tray/.zerus-team/outbox`, where the worker
/// never looks.
fn member_outbox_folder(team: &Team, member: &Member) -> anyhow::Result<std::path::PathBuf> {
    match super::dispatcher::member_folder_path(team, member) {
        Ok(folder) if !folder.is_empty() => Ok(std::path::PathBuf::from(folder)),
        // A team without a folder has no member folder either; the session's own
        // directory is all there is.
        _ => std::env::current_dir().context("The member has no folder to queue in"),
    }
}

/// U20: a member in a sandbox may not write the team store, but it writes its
/// own folder. The report, ask, commit or budget waits there as
/// `<request_id>.json` for the worker, which applies it with the same identity
/// checks (folder + attempt id); `--wait` polls for the outcome (up to 30 s).
/// U42: one id never replaces a queued request with another: a poll that
/// carried a placeholder overwrote #512's whole report before the worker read
/// it. The first whole text stays, or nothing is queued.
fn queue_write(op: &str, input: &Value, wait: bool) -> anyhow::Result<Value> {
    let store = Store::open(false)?;
    let team = store.load(text(input, "team"))?.context("Unknown team")?;
    let Actor::Member(member_id) = actor(&team, op, input)? else {
        bail!("Only a member queues this request; a sandbox does not block the lead");
    };
    let request_id = text(input, "request_id").to_owned();
    let member = team.member(&member_id).context("Unknown member")?;
    let folder = member_outbox_folder(&team, member)?;
    let request = json!({"op":op, "request_id":request_id, "team":team.id, "member":member_id,
        "attempt":text(input, "attempt"), "input":input});
    // U42: a request already applied answers its outcome, and one still queued
    // answers its state (or waits, with `--wait`); a different request under the
    // same id is refused, so a placeholder can never replace the whole text that
    // was queued first.
    if let Some(result) = super::outbox::result(&folder, &request_id) {
        return Ok(result);
    }
    let existing = super::outbox::request(&folder, &request_id);
    if let Some(queued) = &existing {
        ensure!(
            queued["op"] == request["op"] && queued["input"] == request["input"],
            "request_id {request_id} already queues another request; use a new request_id for another one"
        );
    }
    let path = match &existing {
        Some(_) => super::outbox::request_path(&folder, &request_id),
        None => super::outbox::push(&folder, &request)?,
    };
    let queued = json!({
        "status":"queued", "request_id":request_id, "path":path.to_string_lossy(),
        "note": if existing.is_some() {
            "This request is already queued; ask again with the same request_id (or --wait) to read the outcome."
        } else {
            "The sandbox blocks the team store: the request waits in the member's folder and the team worker applies it. Run it again with --wait to read the outcome."
        }
    });
    if !wait {
        return Ok(queued);
    }
    Ok(super::outbox::wait(&folder, &request_id, 30.0).unwrap_or_else(|| json!({
        "status":"queued", "request_id":request_id, "path":path.to_string_lossy(),
        "note":"The team worker has not applied it in 30 s; it stays queued in the member's folder."
    })))
}

/// The teams a finished team's `left_running` looks at: only the ones that
/// still run, from the indexed query, never every stored team (C1).
fn active_team_records(store: &Store) -> anyhow::Result<Vec<Team>> {
    store.active_teams(ACTIVE_TEAMS)
}

/// The stored team list the desktop polls: no live session listing (C1).
fn list_items(teams: &[Team], include_cancelled: bool) -> Value {
    json!({"items":teams.iter().map(|team| summary(team, include_cancelled)).collect::<Vec<_>>()})
}

/// The lead's inbox (C2): every report, question, permission request, its own
/// task and stopped member, oldest first. Read only; the coalesced message the
/// dispatcher sends lists the new ones.
fn inbox_view_for(team: &Team, reviewer: &str) -> Value {
    json!({"items": super::inbox::items_for(team, reviewer).iter().map(super::inbox::Item::value).collect::<Vec<_>>()})
}

/// 9.5: files a finished sub-team's summary on its parent task.
fn file_subteam_report(store: &mut Store, child_id: &str) -> anyhow::Result<()> {
    let child = store.load(child_id)?.context("Unknown team")?;
    let Some(link) = child.parent.clone() else { return Ok(()) };
    if child.status != TeamStatus::Done || store.load(&link.team)?.is_none() {
        return Ok(());
    }
    if store.load(child_id)?.is_some_and(|team| team.status != TeamStatus::Done) {
        return Ok(());
    }
    store.mutate(None, &link.team, None, |parent, events| {
        let task = ops::file_subteam_report(&child, parent)?;
        events.push(Event::new("subteam_report", json!({"team":child_id,"task":task})));
        Ok(Value::Null)
    })
    .map(|_| ())
}

/// 9.5: a rejected sub-team report reopens the sub-team with a Rework draft.
/// N28: only a non-final reject of a finished sub-team reopens it; a stopped or
/// running child keeps its state instead of failing the stored reject.
fn reopen_rejected_subteam(store: &mut Store, parent_id: &str, input: &Value) -> anyhow::Result<()> {
    if input["final"].as_bool() == Some(true) {
        return Ok(());
    }
    let parent = store.load(parent_id)?.context("Unknown team")?;
    let Some(task) = parent.task(input["task"].as_u64().unwrap_or_default() as u32) else { return Ok(()) };
    let Some(child) = task.subteam.clone() else { return Ok(()) };
    let Some(finished) = store.load(&child)? else { return Ok(()) };
    if finished.status != TeamStatus::Done {
        return Ok(());
    }
    let reason = text(input, "reason").to_owned();
    store.mutate(None, &child, None, |team, events| {
        let phase = ops::reopen_subteam(team, &reason)?;
        events.push(Event::new("subteam_reopened", json!({"team":child,"phase":phase,"reason":reason})));
        Ok(Value::Null)
    })
    .map(|_| ())
}

/// 9.7: pause, resume or stop of a team reaches its sub-teams, level by level.
fn cascade_status(store: &mut Store, team_id: &str, op: &str, depth: usize) -> anyhow::Result<()> {
    if depth >= 8 {
        return Ok(());
    }
    let Some(team) = store.load(team_id)? else { return Ok(()) };
    for child_id in team.children.iter().take(32) {
        let Some(child) = store.load(child_id)? else { continue };
        let applies = match op {
            "pause" => child.status == TeamStatus::Running,
            "resume" => child.status == TeamStatus::Paused,
            "stop" => matches!(child.status, TeamStatus::Running | TeamStatus::Paused | TeamStatus::Planning),
            _ => false,
        };
        if applies {
            store.mutate(None, child_id, None, |child, events| {
                ops::apply(child, op, &json!({}), &Actor::Human, &BTreeSet::new(), events)?;
                events.push(Event::new("cascaded", json!({"parent":team_id,"op":op})));
                Ok(Value::Null)
            })?;
        }
        cascade_status(store, child_id, op, depth + 1)?;
    }
    Ok(())
}

/// U26/U32/N15: the rule the owner's sentence must meet for this roster
/// change; an empty rule when the request changes no roster or cap (so no word
/// is needed). A roster change names the member by its id or its role, and
/// says the operation in English or Russian, as an add names its member.
fn roster_word_rule(team: &Team, op: &str, input: &Value) -> super::owner_word::WordRule {
    use super::owner_word::{WordRule, MAX_VERBS, REMOVE_VERBS, ROSTER_PHRASES, SWAP_VERBS};
    let names_for = |id: &str| -> Vec<String> {
        let mut names = vec![id.to_owned()];
        if let Some(member) = team.member(id) {
            names.extend(super::owner_word::role_aliases(&member.role).iter().map(|alias| (*alias).to_owned()));
        }
        names
    };
    match op {
        "add-member" => {
            let member = &input["member"];
            if text(member, "id").is_empty() {
                WordRule::default()
            } else {
                // U32: the sentence says to add and names the member's role or
                // agent/model, never the generated id; that naming and the
                // count are checked once the sentence is verified.
                WordRule { phrases: &[], all: Vec::new(), add: true, expect: format!("add a {}", text(member, "role")), ..Default::default() }
            }
        }
        "remove-member" => {
            let id = text(input, "member");
            if id.is_empty() {
                WordRule::default()
            } else {
                WordRule { phrases: ROSTER_PHRASES, all: Vec::new(), add: false, verbs: REMOVE_VERBS, names: names_for(id), expect: format!("remove {id}") }
            }
        }
        "scale" => {
            // U49: the sentence says to add ("add"/"добав") and names the count;
            // the role it names is checked once the sentence is verified.
            match input["count"].as_u64() {
                Some(count) if (1..=32).contains(&count) && !text(input, "role").trim().is_empty() => {
                    WordRule { phrases: &[], all: vec![count.to_string()], add: true, expect: format!("add {count} {}", text(input, "role").trim()), ..Default::default() }
                }
                _ => WordRule::default(),
            }
        }
        "drain" => {
            // U49: a drain is a removal; cancelling one changes no roster.
            let id = text(input, "member");
            if id.is_empty() || input["draining"].as_bool() == Some(false) {
                WordRule::default()
            } else {
                WordRule { phrases: super::owner_word::DRAIN_PHRASES, all: vec![id.to_owned()], add: false, expect: format!("drain {id}"), ..Default::default() }
            }
        }
        "swap-member" => {
            // U54: a bulk swap names every member it moves.
            let mut ids: Vec<String> = input["members"]
                .as_array()
                .map(|ids| ids.iter().filter_map(|id| id.as_str()).map(str::to_owned).collect())
                .unwrap_or_default();
            if ids.is_empty() {
                ids.push(text(input, "member").to_owned());
            }
            if ids.iter().any(String::is_empty) {
                WordRule::default()
            } else {
                WordRule {
                    phrases: ROSTER_PHRASES,
                    all: Vec::new(),
                    add: false,
                    verbs: SWAP_VERBS,
                    names: ids.iter().flat_map(|id| names_for(id)).collect(),
                    expect: format!("swap {}", ids.join(", ")),
                }
            }
        }
        "relaunch" => {
            // U54: the lead relaunches a member's session on the owner's word.
            // #728: except a stuck member when the owner set lead_restarts_stuck;
            // that route is the lead's own decision.
            let id = text(input, "member");
            if id.is_empty()
                || (team.settings.lead_restarts_stuck && team.member(id).is_some_and(|member| member.stuck.is_some()))
            {
                WordRule::default()
            } else {
                WordRule {
                    phrases: ROSTER_PHRASES,
                    all: Vec::new(),
                    add: false,
                    verbs: super::owner_word::RELAUNCH_VERBS,
                    names: names_for(id),
                    expect: format!("relaunch {id}"),
                }
            }
        }
        "settings" => {
            let cap = input["settings"]["max_members"].as_u64();
            match cap {
                Some(cap) if cap as usize != team.settings.max_members => {
                    WordRule { phrases: ROSTER_PHRASES, all: vec![cap.to_string()], add: false, verbs: MAX_VERBS, names: Vec::new(), expect: format!("max members {cap}") }
                }
                _ => WordRule::default(),
            }
        }
        _ => WordRule::default(),
    }
}

/// U26: fills the verified roster word for a lead caller, or refuses. The
/// verifier is a parameter so a test can use a fixture events database.
fn check_roster_word(
    team: &Team,
    op: &str,
    input: &Value,
    filled: &mut Value,
    caller: &Actor,
    verify: impl Fn(&Team, &Value, &super::owner_word::WordRule, f64) -> anyhow::Result<super::model::OwnerWord>,
) -> anyhow::Result<()> {
    if !matches!(caller, Actor::Member(id) if *id == team.lead) {
        return Ok(());
    }
    let rule = roster_word_rule(team, op, filled);
    if !rule.needed() {
        return Ok(());
    }
    // `roster_word` itself is never the caller's to send: strip it first, so a
    // caller cannot assert a word that was not verified.
    if let Some(object) = filled.as_object_mut() {
        object.remove("roster_word");
    }
    // The sentence is the request's `owner_word` object, as `approve` takes it.
    let word_input = input.get("owner_word").filter(|word| word.is_object()).with_context(|| {
        format!("The lead changes the roster only on your word: pass \"owner_word\" with the owner's sentence, such as \"{}\"", rule.expect)
    })?;
    // U56: the refusal names a sentence that would pass.
    let word = verify(team, word_input, &rule, super::super::now()).with_context(|| {
        format!("The lead changes the roster only on your word: quote your own sentence of the last 30 minutes that says \"{}\"; without it, propose the change (F8)", rule.expect)
    })?;
    // U32/N7: a count word ("add two backends") authorizes that many adds and
    // the cap grows by one for each; every other word pays for one change.
    let (allowed, change) = if op == "add-member" {
        // The sentence names what is added by role or agent/model, in either
        // language; the generated id is not needed.
        let member = filled["member"].clone();
        let (role, agent, model) =
            (text(&member, "role").to_owned(), text(&member, "agent").to_owned(), text(&member, "model").to_owned());
        let described = [role.as_str(), agent.as_str(), model.as_str()]
            .into_iter()
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join(" / ");
        ensure!(
            super::owner_word::names_member(&word.text, &role, &agent, &model),
            "The owner's sentence does not name the member to add ({described}); quote a sentence naming its role or agent"
        );
        (super::owner_word::add_count(&word.text), format!("adding {role}"))
    } else if op == "scale" {
        // U49: the sentence names the role to add and the count it asks for.
        let (role, agent, model) =
            (text(filled, "role").to_owned(), text(filled, "agent").to_owned(), text(filled, "model").to_owned());
        let described = [role.as_str(), agent.as_str(), model.as_str()]
            .into_iter()
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join(" / ");
        ensure!(
            super::owner_word::names_member(&word.text, &role, &agent, &model),
            "The owner's sentence does not name the role to add ({described}); quote a sentence naming its role or agent"
        );
        let asked = filled["count"].as_u64().unwrap_or(0);
        let said = super::owner_word::add_count(&word.text) as u64;
        ensure!(said == asked, "The owner's sentence names {said} members, not {asked}; quote a sentence naming the count");
        (1, format!("scaling {role} by {asked}"))
    } else if op == "drain" {
        (1, "draining the member".to_owned())
    } else {
        (1, "the roster change".to_owned())
    };
    if !team.owner_word_allowed_again(word.event, allowed, &change) {
        let used = team.owner_word_used(word.event).unwrap_or_default();
        bail!("That word was already used for {used}; ask the owner again");
    }
    filled["owner_word_count"] = json!(allowed);
    filled["owner_word_change"] = json!(change);
    filled["owner_word_checked"] = serde_json::to_value(&word)?;
    filled["owner_word_event"] = json!(word.event);
    filled["roster_word"] = Value::Bool(true);
    Ok(())
}

/// U29: fills the verified owner word of an addition to a running phase for
/// a lead caller: the request, or the confirmation newer than the warning of
/// the pending addition it names. The verifier is a parameter for tests.
fn check_phase_word(
    team: &Team,
    input: &Value,
    filled: &mut Value,
    caller: &Actor,
    verify: impl Fn(&Team, &Value, u32, super::owner_word::PhaseWord, f64, f64) -> anyhow::Result<super::model::OwnerWord>,
) -> anyhow::Result<()> {
    if !matches!(caller, Actor::Member(id) if *id == team.lead) {
        return Ok(());
    }
    let phase = input["phase"].as_u64().and_then(|n| u32::try_from(n).ok()).filter(|n| *n > 0).context("A phase number is required")?;
    let word = input
        .get("owner_word")
        .filter(|word| word.is_object())
        .with_context(|| format!("The lead adds a task to a running phase only on your word: pass \"owner_word\" with the owner's sentence, such as \"add a task to phase {phase}\""))?;
    let (kind, after, change) = match input["confirm"].as_str().filter(|id| !id.is_empty()) {
        None => (super::owner_word::PhaseWord::Request, 0.0, "the request to add to a running phase"),
        Some(id) => {
            let pending = team.pending_additions.iter().find(|pending| pending.id == id).context("That addition is unknown or expired; ask again")?;
            (super::owner_word::PhaseWord::Confirm, pending.warned_at, "the confirmation of an addition to a running phase")
        }
    };
    let verified = verify(team, word, phase, kind, after, super::super::now())?;
    if let Some(used) = team.owner_word_used(verified.event) {
        // U58: one sentence naming a member and its tasks pays for both, the
        // member first: the roster add, then this request for its tasks.
        ensure!(
            team.owner_word_allows_its_tasks(verified.event),
            "That word was already used for {used}; ask the owner again"
        );
    }
    filled["owner_word_checked"] = serde_json::to_value(&verified)?;
    filled["owner_word_event"] = json!(verified.event);
    filled["owner_word_change"] = json!(change);
    Ok(())
}

/// 9.7: a task with a sub-team cannot be cancelled or reassigned without
/// saying whether the sub-team stops or keeps running.
fn choose_subteam(store: &mut Store, team: &Team, input: &Value) -> anyhow::Result<()> {
    let Some(task) = team.task(input["task"].as_u64().unwrap_or_default() as u32) else { return Ok(()) };
    if !ops::subteam_stop_choice(input, task)? {
        return Ok(());
    }
    let Some(child) = task.subteam.clone() else { return Ok(()) };
    let Some(found) = store.load(&child)? else { return Ok(()) };
    if matches!(found.status, TeamStatus::Running | TeamStatus::Paused | TeamStatus::Planning) {
        store.mutate(None, &child, None, |team, events| {
            ops::apply(team, "stop", &json!({}), &Actor::Human, &BTreeSet::new(), events)?;
            events.push(Event::new("subteam_stopped", json!({"task":task.id})));
            Ok(Value::Null)
        })?;        cascade_status(store, &child, "stop", 1)?;
    }
    Ok(())
}

/// 9.7: archive only after the children are stopped or done.
fn ensure_children_closed(store: &Store, team: &Team) -> anyhow::Result<()> {
    for child_id in team.children.iter().take(32) {
        let Some(child) = store.load(child_id)? else { continue };
        ensure!(
            matches!(child.status, TeamStatus::Done | TeamStatus::Stopped | TeamStatus::Archived),
            "The sub-team {} is not stopped or done; stop it before archiving",
            child.name
        );
    }
    Ok(())
}

/// 9.6: puts a forwarded question on the parent task (creating an attempt when
/// the parent task has none).
fn forward_to_parent(
    store: &mut Store,
    team: &Team,
    parent_question: &str,
    child_question: &str,
    text: &str,
    link: &Parent,
) -> anyhow::Result<()> {
    let child_id = team.id.clone();
    let child_name = team.name.clone();
    store.mutate(None, &link.team, None, |parent, events| {
        let task = parent.task_mut(link.task).context("The parent task is gone")?;
        ensure!(!task.status.finished(), "The parent task is already decided");
        if task.open_attempt().is_none() {
            let member = if link.member.is_empty() { task.assignee.clone() } else { link.member.clone() };
            let n = task.attempts.len() as u32 + 1;
            task.attempts.push(Attempt {
                n,
                id: uuid::Uuid::new_v4().to_string(),
                member,
                sent_at: now(),
                delivered_at: Some(now()),
                state: AttemptState::Delivered,
                ..Default::default()
            });
        }
        let attempt = task.attempts.iter_mut().rev().find(|attempt| attempt.state.open()).context("The parent task has no open attempt")?;
        attempt.questions.push(Question {
            id: parent_question.to_owned(),
            text: format!("From the sub-team {child_name}:\n{text}"),
            at: now(),
            // N24: the child question this one carries; `forwarded` stays for
            // the level above if this one is passed on again.
            source: child_question.to_owned(),
            ..Default::default()
        });
        events.push(Event::new("subteam_question", json!({"team":child_id,"question":parent_question,"task":link.task})));
        Ok(Value::Null)
    })?;
    Ok(())
}

/// 9.6: a sub-lead passes one of its open questions to the parent lead as a
/// question on the parent task; the parent's reply is copied back down.
/// 9.6: the parent question that carries a sub-team permission request with
/// the same command, source and reason as the lead's card.
fn permission_question(child: &Team, member: &str, pending: &PendingQuestion) -> Question {
    let tool = pending.card["questions"][0]["header"].as_str().or_else(|| pending.card["tool"].as_str()).unwrap_or_default();
    let command = super::dispatcher::permission_command_line(&pending.card);
    let reason: String = pending.card["questions"][0]["body"].as_str().unwrap_or_default().chars().take(2000).collect();
    Question {
        id: pending.question_id.clone(),
        text: format!(
            "Permission request from {member}: {tool}\n{command}Reason: {reason}\nAnswer the native card on the sub-team:\n  {{\"team\":\"{}\",\"member\":\"{member}\",\"question_id\":\"{}\",\"expected_question_hash\":\"{}\",\"answers\":[...]}} | hgs team answer --json",
            child.id, pending.question_id, pending.question_hash
        ),
        at: now(),
        ..Default::default()
    }
}

fn escalate(input: &Value) -> anyhow::Result<Value> {
    let request = text(input, "request_id");
    ensure!(canonical_uuid(request), "A canonical request_id is required");
    let mut store = Store::open(true)?;
    // N28: a replay returns the first receipt instead of duplicating the
    // parent question.
    if let Some(receipt) = store.receipt(request, input)? {
        return Ok(receipt);
    }
    let team = store.load(text(input, "team"))?.context("Unknown team")?;
    let link = team.parent.clone().context("Only a sub-team escalates a question")?;
    let by = actor(&team, "escalate", input)?;
    ensure!(matches!(by, Actor::Human) || by.is_lead(&team), "Only the sub-team lead or you escalate a question");
    let question_id = text(input, "question");
    ensure!(!question_id.is_empty(), "A question id is required");
    let found = team
        .tasks
        .iter()
        .flat_map(|task| &task.attempts)
        .flat_map(|attempt| &attempt.questions)
        .find(|question| question.id == question_id && question.open())
        .cloned();
    // Section 9.6: a permission request travels the same way; the parent lead
    // answers the native card on the sub-team, by its exact ids.
    let question = match found {
        Some(question) => question,
        None => {
            let (member, pending) = team
                .members
                .iter()
                .find_map(|member| {
                    member
                        .pending
                        .as_ref()
                        .filter(|pending| pending.question_id == input["question"].as_str().unwrap_or_default())
                        .map(|pending| (member.id.clone(), pending.clone()))
                })
                .context("No open question or permission request with that id in this sub-team")?;
            permission_question(&team, &member, &pending)
        }
    };
    let parent_question = ops::forward_question(&team, &question);
    let child_id = team.id.clone();
    let receipt = json!({"status":"escalated","team":child_id,"question":parent_question,"parent":link.team});
    // N27: a retry that finds the forward landed but its receipt lost writes
    // the receipt instead of refusing the question as already escalated.
    if question.forwarded == parent_question {
        store.mutate(Some((request, input)), &child_id, None, |_team, _events| Ok(receipt.clone()))?;
        return Ok(receipt);
    }
    // N24: one forward per level. A question the level below escalated (it
    // carries the child's id in `source`, or in `forwarded` on a team stored
    // before that field) may go one level up; one already passed on is refused.
    ensure!(!question.forwarded.starts_with("sub:"), "This question was already escalated");
    let child_name = team.name.clone();
    // N27: the child link is marked without a receipt; the receipt waits for
    // the parent question, so a retry after a failed forward escalates again
    // instead of returning an empty one.
    store.mutate(None, &child_id, None, |team, events| {
        for task in &mut team.tasks {
            for attempt in &mut task.attempts {
                for question in &mut attempt.questions {
                    if question.id == question_id {
                        // A question stored before `source` kept the child id
                        // in `forwarded`; keep it as the down-link.
                        if question.source.is_empty() && !question.forwarded.is_empty() {
                            question.source = question.forwarded.clone();
                        }
                        question.forwarded = parent_question.clone();
                    }
                }
            }
        }
        events.push(Event::new("escalated", json!({"question":question_id,"to":parent_question,"parent":link.team})));
        Ok(Value::Null)
    })?;
    let _ = child_name;
    if let Err(error) = forward_to_parent(&mut store, &team, &parent_question, question_id, &question.text, &link) {
        // N28: a failure upstream must not leave the child marked forwarded.
        let _ = store.mutate(None, &child_id, None, |team, _| {
            for task in &mut team.tasks {
                for attempt in &mut task.attempts {
                    for question in &mut attempt.questions {
                        if question.forwarded == parent_question {
                            question.forwarded.clear();
                        }
                    }
                }
            }
            Ok(Value::Null)
        });
        return Err(error);
    }
    // N27: the receipt is written only now, once the parent has the question.
    store.mutate(Some((request, input)), &child_id, None, |_team, _events| Ok(receipt.clone()))?;
    Ok(receipt)
}

/// 9.6/N24: the question `id` on `task`.
fn question_in(task: &Task, id: &str) -> Option<Question> {
    task.attempts.iter().flat_map(|attempt| &attempt.questions).find(|question| question.id == id).cloned()
}

/// 9.6/N24: the question `id` in `team` with the task that holds it.
fn question_with_task(team: &Team, id: &str) -> Option<(u32, Question)> {
    team.tasks.iter().find_map(|task| question_in(task, id).map(|question| (task.id, question)))
}

/// 9.6: copies the parent lead's answer into the sub-team's question. N23: the
/// inbox's reply names only the question, so its task is found here. N24: an
/// answer travels one level per call, on down the chain of escalations. A
/// queued reply from a sandboxed sub-lead is applied by the worker and routed
/// here too.
pub(super) fn route_reply_down(store: &mut Store, parent_id: &str, input: &Value) -> anyhow::Result<()> {
    let parent = store.load(parent_id)?.context("Unknown team")?;
    let question_id = text(input, "question");
    let asked_task = input["task"].as_u64().and_then(|task| u32::try_from(task).ok());
    let found = match asked_task {
        Some(id) => parent.task(id).and_then(|task| question_in(task, question_id)).map(|question| (id, question)),
        None => question_with_task(&parent, question_id),
    };
    let Some((task_id, question)) = found else { return Ok(()) };
    let Some(child) = parent.task(task_id).and_then(|task| task.subteam.clone()) else { return Ok(()) };
    // A question asked inside this team has no sub-team question to reach.
    if question.source.is_empty() && question.forwarded.is_empty() {
        return Ok(());
    }
    let answer = text(input, "text").trim().to_owned();
    if answer.is_empty() || store.load(&child)?.is_none() {
        return Ok(());
    }
    carry_answer_down(store, parent_id, question_id, &answer, 0)?;
    Ok(())
}

/// 9.6: the answer of `carrier` (a question in `team_id` that carries a
/// sub-team's question) goes to the child question it carries, and on down if
/// that child question carries one itself (N24).
fn carry_answer_down(store: &mut Store, team_id: &str, carrier: &str, answer: &str, depth: usize) -> anyhow::Result<bool> {
    if depth >= 8 {
        return Ok(false);
    }
    let Some(team) = store.load(team_id)? else { return Ok(false) };
    let Some((task_id, _)) = question_with_task(&team, carrier) else { return Ok(false) };
    let Some(child_id) = team.task(task_id).and_then(|task| task.subteam.clone()) else { return Ok(false) };
    let mut next: Option<String> = None;
    let mut delivered = false;
    store.mutate(None, &child_id, None, |child, events| {
        if let Some(answered) = ops::answer_forwarded(child, carrier, answer) {
            delivered = true;
            events.push(Event::new("answered_from_parent", json!({"question":carrier})));
            // The answered question may itself carry a deeper sub-team's one.
            let carries = child
                .tasks
                .iter()
                .flat_map(|task| &task.attempts)
                .flat_map(|attempt| &attempt.questions)
                .find(|question| question.id == answered)
                .is_some_and(|question| !question.source.is_empty());
            if carries {
                next = Some(answered);
            }
        }
        Ok(Value::Null)
    })?;
    if let Some(next) = next {
        carry_answer_down(store, &child_id, &next, answer, depth + 1)?;
    }
    Ok(delivered)
}

/// N4: how long `hgs team run --wait` waits by default: a short poll a member's
/// shell can afford, never the template's whole bound (up to an hour).
const RUN_WAIT_SECONDS: f64 = 90.0;

/// U21: a member asks Zerus to run one built-in command template. Zerus runs it
/// outside the member's sandbox (a detached runner, C1), so the request goes
/// through the U20 outbox in the member's own folder. N4: an answer already in
/// the folder is the answer and never queues the run again; the wait is short,
/// and asking again with the same request_id reads the result.
fn team_run(input: &Value, wait: bool) -> anyhow::Result<Value> {
    // N5: queue where the worker reads - the member's own folder, never this
    // process's directory.
    let store = Store::open(false)?;
    let team = store.load(text(input, "team"))?.context("Unknown team")?;
    let Actor::Member(member_id) = actor(&team, "run", input)? else {
        bail!("Only a member runs a template; the lead runs commands itself")
    };
    let folder = member_outbox_folder(&team, team.member(&member_id).context("Unknown member")?)?;
    team_run_in(&folder, input, wait, RUN_WAIT_SECONDS)
}

fn team_run_in(folder: &std::path::Path, input: &Value, wait: bool, wait_seconds: f64) -> anyhow::Result<Value> {
    let request_id = text(input, "request_id").to_owned();
    ensure!(canonical_uuid(&request_id), "A canonical request_id is required");
    let store = Store::open(false)?;
    let team = store.load(text(input, "team"))?.context("Unknown team")?;
    let Actor::Member(member_id) = actor(&team, "run", input)? else {
        bail!("Only a member runs a template; the lead runs commands itself")
    };
    ensure!(member_id != team.lead, "The lead runs commands itself");
    let attempt_id = text(input, "attempt").to_owned();
    let attempt = team
        .tasks
        .iter()
        .find(|task| task.open_attempt().is_some_and(|attempt| attempt.id == attempt_id && attempt.member == member_id));
    let Some(task) = attempt else { bail!("attempt {attempt_id} does not run for this member") };
    // N4: an outcome already there answers at once; the same request never runs twice.
    if let Some(result) = super::outbox::result(folder, &request_id) {
        return Ok(result);
    }
    if super::outbox::running(folder, &request_id).is_some() {
        return Ok(if wait {
            super::outbox::wait(folder, &request_id, wait_seconds).unwrap_or_else(|| running_note(&request_id))
        } else {
            running_note(&request_id)
        });
    }
    if let Some(queued) = super::outbox::request(folder, &request_id) {
        // A push of the same id is already queued: answer its state, do not push
        // a second file (the member retried because the first call did not wait).
        ensure!(
            queued["template"] == input["template"] && queued["args"] == input["args"],
            "request_id {request_id} is already used by another run; use a new UUID"
        );
        return Ok(json!({
            "status":"queued", "request_id":request_id,
            "path":super::outbox::request_path(folder, &request_id).to_string_lossy(),
            "note":"This run is already queued; ask again with the same request_id (or --wait) to read its result."
        }));
    }
    // Validate here too: a bad template never reaches the worker's queue.
    let template = text(input, "template").to_owned();
    super::templates::plan(&template, &input["args"], folder)?;
    let request = json!({"op":"run","request_id":request_id,"team":team.id,"member":member_id,"attempt":attempt_id,
        "template":template,"args":input["args"].clone(),"task":task.id,"input":input});
    let path = super::outbox::push(folder, &request)?;
    let queued = json!({
        "status":"queued", "request_id":request_id, "path":path.to_string_lossy(),
        "note":"Zerus runs the template outside your sandbox; ask again with the same request_id (or --wait) to read the exit code."
    });
    if !wait {
        return Ok(queued);
    }
    Ok(super::outbox::wait(folder, &request_id, wait_seconds).unwrap_or_else(|| json!({
        "status":"running", "request_id":request_id,
        "note":"The run has not finished yet; ask again with the same request_id to read its exit code."
    })))
}

/// N4: the answer while a runner still holds the claim.
fn running_note(request_id: &str) -> Value {
    json!({"status":"running","request_id":request_id,
        "note":"Zerus is running it; ask again with the same request_id to read the exit code."})
}

/// U21: the built-in set, the owner's additions and the global off switch.
/// Only the owner (the desktop, not an agent) changes them; they live in Zerus
/// state, never in a member's folder, so a sandboxed member cannot add one.
fn templates_command(input: &Value) -> anyhow::Result<Value> {
    match text(input, "action") {
        "" | "get" => Ok(super::templates::listed()),
        "set" => {
            // N3/C6: the same owner gate as every other owner path - no caller
            // record and no session marker (dsh conversation, hgs session or
            // hgs run), and the request says it is you. An added template runs
            // outside the member's sandbox (U21), so an agent never adds one.
            let (dsh, session, run) = caller_identities();
            owner_caller(input, caller_record().is_some(), dsh.is_some(), session.is_some(), run.is_some())?;
            if let Some(enabled) = input["enabled"].as_bool() {
                super::templates::set_enabled(enabled)?;
            }
            if let Some(list) = input["templates"].as_array() {
                super::templates::set_additions(list)?;
            }
            Ok(super::templates::listed())
        }
        other => bail!("templates action is get or set (got {other})"),
    }
}

/// The owner, 2026-10-28: the shapes of members' requests the lead allows
/// without being asked again (`safe_actions`). The lead or the owner lists,
/// adds and removes them; each change is in the team's journal.
fn safe_action_command(input: &Value) -> anyhow::Result<Value> {
    let team = Store::open(false)?.load(text(input, "team"))?.context("Unknown team")?;
    let by = actor(&team, "safe-action", input)?;
    ensure!(
        matches!(&by, Actor::Human) || matches!(&by, Actor::Member(id) if *id == team.lead),
        "Only the lead or the owner records safe actions"
    );
    let who = if matches!(&by, Actor::Human) { "owner" } else { "lead" };
    let listed = |team: &Team| json!({"actions":super::safe_actions::list().into_iter().filter(|action| action["team"] == team.id.as_str()).collect::<Vec<_>>()});
    let (event, detail) = match text(input, "action") {
        "" | "list" => return Ok(listed(&team)),
        "add" => {
            let added = super::safe_actions::add(&team, input, who)?;
            ("safe_action_added", json!({"action":added["id"],"shape":added["shape"],"member":added["member"],"role":added["role"],"by":who}))
        }
        "remove" => {
            super::safe_actions::remove(&team, text(input, "id"))?;
            ("safe_action_removed", json!({"action":text(input, "id"),"by":who}))
        }
        other => bail!("safe-action action is list, add or remove (got {other})"),
    };
    Store::open(true)?.mutate(None, &team.id, None, |_, events| {
        events.push(Event::new(event, detail.clone()));
        Ok(Value::Null)
    })?;
    Ok(listed(&team))
}

/// U51: the owner creates an intake key (shown once, never stored) or revokes
/// one by id. The team keeps only the key's hash.
fn intake_key(input: &Value) -> anyhow::Result<Value> {
    let request = text(input, "request_id");
    ensure!(canonical_uuid(request), "A canonical request_id is required");
    let team_id = text(input, "team");
    ensure!(!team_id.is_empty(), "Name the team");
    let mut secret = None;
    let mut store = Store::open(true)?;
    let mut receipt = store.mutate(Some((request, input)), team_id, input["revision"].as_u64(), |team, events| {
        let by = actor(team, "intake-key", input)?;
        let mut value = ops::apply(team, "intake-key", input, &by, &BTreeSet::new(), events)?;
        // The stored receipt names the key, never its secret.
        if let Some(object) = value.as_object_mut() {
            secret = object.remove("key").and_then(|key| key.as_str().map(str::to_owned));
        }
        Ok(value)
    })?;
    if receipt["status"] == "created" {
        if let Some(secret) = secret {
            receipt["key"] = json!(secret);
        }
    }
    Ok(receipt)
}

/// U51: a process presenting the owner's intake key adds one bounded urgent
/// phase. No session identity is needed: the key is the whole authority, and
/// the team names its label in the feed. The key never enters the stored
/// request or the receipt.
fn intake(input: &Value) -> anyhow::Result<Value> {
    let key = text(input, "key");
    ensure!(!key.is_empty(), "An intake request carries the owner's key");
    let team_id = text(input, "team");
    ensure!(!team_id.is_empty(), "Name the team");
    let request = text(input, "request_id");
    let generated = uuid::Uuid::new_v4().to_string();
    let request = if request.is_empty() { generated.as_str() } else { request };
    ensure!(canonical_uuid(request), "A canonical request_id is required when one is given");
    // The stored request keeps no secret; a retry with the same key replays.
    let mut stored = input.clone();
    if let Some(object) = stored.as_object_mut() {
        object["key"] = json!("");
    }
    let mut store = Store::open(true)?;
    store.mutate(Some((request, &stored)), team_id, input["revision"].as_u64(), |team, events| {
        let label = ops::intake_label(team, key).context("Unknown or revoked intake key for this team")?;
        ops::apply(team, "intake", input, &Actor::Intake(label), &BTreeSet::new(), events)
    })
}

fn run(op: &str, input: &Value, wait: bool) -> anyhow::Result<Value> {
    // U51: an intake key adds tasks; it is refused for every other operation,
    // and nothing else may present one.
    let presented = text(input, "key");
    ensure!(
        presented.is_empty() || op == "intake",
        "An intake key adds urgent tasks only: use hgs team intake"
    );
    if op == "intake" {
        return intake(input);
    }
    if op == "intake-key" {
        return intake_key(input);
    }
    // F5: the draft's read and its start are not mutations.
    if op == "draft" {
        match text(input, "action") {
            "get" => return draft_get(input),
            "start" => return draft_start(input),
            _ => {}
        }
    }
    // U21: a member's template run and the owner's off switch are not team
    // mutations: the worker runs the template outside the member's sandbox.
    if op == "run" {
        return team_run(input, wait);
    }
    if op == "templates" {
        return templates_command(input);
    }
    if op == "safe-action" {
        return safe_action_command(input);
    }
    // #717: the team's test queue and its results database; only the runner
    // writes a result, never a report.
    if op == "test" {
        return test_command(input);
    }
    // F8: a decision may create the next team, so it runs with the store.
    if op == "decide-proposal" {
        return decide_proposal(input);
    }
    match op {
        "create" => create(input),
        "answer" => answer(input),
        // U20: a sandbox that cannot write the store queues the whole commit, so
        // no commit exists without the attempt recording it.
        "commit" => match commit(input) {
            Err(error) if read_only(&error) => queue_write("commit", input, wait),
            result => result,
        },
        "next-goal" => next_goal(input),
        "leftovers" => {
            let store = Store::open(false)?;
            let teams = store.teams(true, 1000)?;
            let sessions = live_sessions()?;
            Ok(json!({"items":leftovers(&teams, &sessions)}))
        }
        // U50: what waits for the owner across every team, read only.
        "waiting" => {
            if !store::path().exists() {
                return Ok(super::owner_waiting::page(&[], None, None));
            }
            let teams = active_team_records(&Store::open(false)?)?;
            let items = super::owner_waiting::items(&teams, super::super::now());
            let before = Some(&input["before"]).filter(|cursor| !cursor.is_null());
            Ok(super::owner_waiting::page(&items, before, input["limit"].as_u64()))
        }
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
            Ok(list_items(&teams, input["include_cancelled"].as_bool() == Some(true)))
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
            let mut teams = if finished { active_team_records(&store)? } else { Vec::new() };
            // Section 9.4: the linked sub-teams are read by id (at most 32),
            // whatever their status, for the parent task card and the children
            // list; this is bounded and only for a team that has children.
            for child in team.children.iter().take(32) {
                if teams.iter().any(|team| &team.id == child) {
                    continue;
                }
                if let Some(child) = store.load(child)? {
                    teams.push(child);
                }
            }
            // U58: cancelled tasks and phases stay out of the status unless the
            // caller asks for them.
            let include_cancelled = input["include_cancelled"].as_bool() == Some(true);
            let mut value = view_with_cancelled(&team, &teams, sessions.as_deref(), include_cancelled);
            // D2: the findings page takes a before cursor.
            value["findings"] = findings_page(&team, input["before"].as_u64());
            // U38: the profile only on request, as hgs team profile gives it.
            if input["profile"].is_object() {
                let mut ask = input["profile"].clone();
                ask["team"] = json!(team.id);
                value["profile"] = profile_view(&ask)?;
            }
            Ok(value)
        }
        "inbox" => {
            let team = Store::open(false)?.load(text(input, "team"))?.context("Unknown team")?;
            // Spec 5.6: a sub-lead reads its own inbox; you may name one.
            let named = text(input, "member").to_owned();
            let reviewer = if !named.is_empty() {
                named
            } else {
                match actor(&team, "inbox", input) {
                    Ok(Actor::Member(id)) if team.member(&id).is_some_and(|member| member.sub) => id,
                    _ => team.lead.clone(),
                }
            };
            Ok(inbox_view_for(&team, &reviewer))
        }
        "events" => {
            let before = match &input["before"] {
                Value::Null => None,
                value => Some(value.as_i64().filter(|n| *n > 0).context("Invalid feed cursor")?),
            };
            Store::open(false)?.events(text(input, "team"), before, input["limit"].as_i64().unwrap_or(PAGE))
        }
        "spans" => spans_view(input),
        "profile" => profile_view(input),
        "current" => current(input),
        "escalate" => escalate(input),
        op if MUTATIONS.contains(&op) => {
            let request = text(input, "request_id");
            ensure!(canonical_uuid(request), "A canonical request_id is required");
            let team_id = text(input, "team");
            let revision = expected_revision(op, input);
            // A member in a sandbox may not write the team's files: its report
            // waits in its own folder for the dispatcher, which checks it.
            // SQLite may open the store read-only without a word, so the
            // refusal can come with the write as well as with the open.
            let mut store = match Store::open(true) {
                Ok(store) => store,
                Err(error) if queues_instead(op) && read_only(&error) => {
                    return queue_write(op, input, wait)
                }
                // U20: a write the sandbox blocked says what to do, not a bare
                // SQLite error.
                Err(error) => return Err(super::store::write_error(error)),
            };
            // U54: a renamed session's member keeps its team record in step, so
            // commands after the rename match it by name again.
            follow_session_rename(&mut store, team_id, caller_record().as_ref())?;
            let mut filled = input.clone();
            // C1: a caller can never hand in a verified owner word; only the
            // check below fills it, from the lead's own session.
            if let Some(object) = filled.as_object_mut() {
                object.remove("owner_word_checked");
                object.remove("owner_word_event");
                object.remove("owner_word_change");
                object.remove("roster_word");
            }
            // U54: one owner action moves every member whose session really
            // runs on from_account, of the requested agent, the lead included.
            // A same-provider move keeps the conversation through U53's switch;
            // a member whose switch does not confirm stays where it is and the
            // answer names why.
            if op == "swap-member" && !text(&filled, "from_account").is_empty() {
                let from = text(&filled, "from_account").to_owned();
                let to = text(&filled, "to_account").to_owned();
                ensure!(!to.is_empty(), "A bulk swap needs to_account");
                let provider = text(&filled, "provider").to_owned();
                let team = store.load(team_id)?.context("Unknown team")?;
                let sessions = live_sessions()?;
                let ids = bulk_swap_members(&team, &sessions, &from, &provider);
                ensure!(!ids.is_empty(), format!("No member of team {} runs on account {from}", team.name));
                let mut moved = Vec::new();
                let mut fresh = Vec::new();
                let mut notes = Vec::new();
                let mut failed = Vec::new();
                for id in &ids {
                    let member = team.member(id).context("Unknown member")?;
                    let Some(session) = member.session.as_ref().filter(|session| session.host.is_empty() && !session.name.is_empty()) else {
                        failed.push(json!({"member":id,"error":"the member has no local session to move"}));
                        continue;
                    };
                    // A deterministic request id makes a replayed bulk swap
                    // the same U53 request, never a second switch.
                    use sha2::{Digest, Sha256};
                    let digest = Sha256::digest(format!("{}:{}:{}", text(&filled, "request_id"), id, to).as_bytes());
                    let mut bytes = [0u8; 16];
                    bytes.copy_from_slice(&digest[..16]);
                    let request = uuid::Uuid::from_bytes(bytes).to_string();
                    match switch_member_account(&session.name, &to, &request) {
                        Ok(receipt) if receipt["status"] == "switched" => moved.push(id.clone()),
                        Ok(receipt) => match retire_old_session(&session.name, &from) {
                            Ok(note) => {
                                fresh.push(id.clone());
                                notes.push(json!({"member":id,"note":format!("the account switch is {}; {note}", switch_status(&receipt))}));
                            }
                            Err(wait) => failed.push(json!({"member":id,"error":format!("the account switch is {}; {wait}", switch_status(&receipt))})),
                        },
                        Err(error) => match retire_old_session(&session.name, &from) {
                            Ok(note) => {
                                fresh.push(id.clone());
                                notes.push(json!({"member":id,"note":format!("{error}; {note}")}));
                            }
                            Err(wait) => failed.push(json!({"member":id,"error":format!("{error}; {wait}")})),
                        },
                    }
                }
                ensure!(!moved.is_empty() || !fresh.is_empty(), format!("No member of team {} could move to account {to}", team.name));
                fill_bulk_swap(&mut filled, &moved, &fresh, notes, failed, &to);
            }
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
            if matches!(op, "plan" | "add-member" | "swap-member" | "draft") {
                // An existing session must run here and be free of another
                // active team; its own folder replaces the planned one (F8).
                // A retry returns here before any listing (L4).
                adopt_existing_sessions(&store, request, input, op, &mut filled)?;
            }
            // Section 9.2: a change that can bind a new session keeps the whole
            // tree within its master's budget (extend binds none, and is
            // checked for a tree already over it).
            if matches!(op, "plan" | "approve" | "add-member" | "scale" | "swap-member" | "extend") {
                let team = store.load(team_id)?.context("Unknown team")?;
                // Bounded: ancestors by id and children by id; a flat team
                // reads nothing beyond the team itself.
                let tree = tree_around(&team, &mut |id| store.load(id))?;
                ops::ensure_tree_sessions(&team, &tree, added_sessions(&team, op, &filled))?;
                if op == "add-member" {
                    // U32: a cap raise on the owner's word stays within the
                    // master's session budget.
                    filled["tree_sessions"] = json!(ops::root_of(&team, &tree).settings.tree_sessions);
                }
            }
            if op == "archive" {
                let team = store.load(team_id)?.context("Unknown team")?;
                ensure_children_closed(&store, &team)?;
            }
            // C6: a sub-team's plan and phases are decided by the parent lead,
            // whose session is proven in the parent team, not in this one.
            let parent_lead = matches!(op, "approve" | "approve-phase" | "discard-phase" | "return-to-draft" | "reject-phase" | "undo-reject-phase") && {
                let team = store.load(team_id)?.context("Unknown team")?;
                parent_lead_calls(&store, &team)?
            };
            if matches!(op, "approve" | "approve-phase") {
                // F13: the lead approves only on the owner's exact sentence,
                // typed in the lead's own session; the CLI checks it here and
                // records the verified quote for the state machine and the feed.
                let team = store.load(team_id)?.context("Unknown team")?;
                let caller = if parent_lead { Actor::ParentLead } else { actor(&team, op, &filled)? };
                let auto_plan = op == "approve" && team.settings.approval == Approval::Auto;
                if matches!(&caller, Actor::Member(member) if *member == team.lead) && !auto_plan {
                    // U56: a refusal names the sentence this approval expects.
                    let phase = (op == "approve-phase").then(|| input["phase"].as_u64().and_then(|n| u32::try_from(n).ok())).flatten();
                    let sentence = super::owner_word::approval_sentence(phase);
                    let word = input.get("owner_word").filter(|word| word.is_object()).with_context(|| {
                        format!("The lead approves only on the owner's word: pass \"owner_word\" with the owner's sentence, which says {sentence}")
                    })?;
                    let verified = super::owner_word::verify(&team, word, phase, super::super::now())?;
                    if let Some(used) = team.owner_word_used(verified.event) {
                        bail!("That word was already used for {used}; ask the owner again");
                    }
                    filled["owner_word_checked"] = serde_json::to_value(&verified)?;
                    filled["owner_word_event"] = json!(verified.event);
                    filled["owner_word_change"] = json!(if op == "approve" { "the plan approval" } else { "the phase approval" });
                }
            }
            // U42 (#587): the lead discards an accepted phase, or returns one to
            // a draft, on the owner's word naming the phase and the change; the
            // CLI checks it here as it does for an approval.
            if matches!(op, "discard-phase" | "return-to-draft") && !parent_lead {
                let team = store.load(team_id)?.context("Unknown team")?;
                let caller = actor(&team, op, &filled)?;
                check_phase_change_word(&team, op, input, &mut filled, &caller, super::owner_word::verify_phase_change)?;
            }
            // U26: a lead changes the roster (or max_members) only on the
            // owner's fresh word, and only for the change that word names.
            if matches!(op, "add-member" | "scale" | "drain" | "swap-member" | "remove-member" | "relaunch" | "settings") {
                let team = store.load(team_id)?.context("Unknown team")?;
                let caller = actor(&team, op, &filled)?;
                check_roster_word(&team, op, input, &mut filled, &caller, |team, input, rule, now| {
                    super::owner_word::verify_roster(team, input, rule, now)
                })?;
            }
            // U29: a task joins a running phase on the owner's word twice.
            if op == "add-to-phase" {
                let team = store.load(team_id)?.context("Unknown team")?;
                let caller = actor(&team, op, &filled)?;
                check_phase_word(&team, input, &mut filled, &caller, super::owner_word::verify_phase)?;
            }
            // N7: a replayed discard does not stop the lead's session twice.
            let replayed = op == "draft"
                && text(&filled, "action") == "discard"
                && store.receipt(request, input)?.is_some();
            let mut receipt = match store.mutate(Some((request, input)), team_id, revision, |team, events| {
                let actor = if parent_lead { Actor::ParentLead } else { actor(team, op, &filled)? };
                ops::apply(team, op, &filled, &actor, &BTreeSet::new(), events)
            }) {
                Ok(receipt) => receipt,
                Err(error) if queues_instead(op) && read_only(&error) => {
                    return queue_write(op, input, wait)
                }
                Err(error) => return Err(super::store::write_error(error)),
            };
            // N7: discarding a draft stops the lead's session when the owner
            // asked for it (spec 3); a failure leaves the discard done and names
            // it in the answer.
            if op == "draft" && text(&filled, "action") == "discard" && receipt["stop_lead"].as_bool() == Some(true) && !replayed {
                let team = store.load(team_id)?.context("Unknown team")?;
                match team.member(&team.lead).and_then(|member| member.session.clone()) {
                    Some(session) => match stop_lead_session(&session) {
                        Ok(()) => receipt["lead_stopped"] = json!(true),
                        Err(error) => receipt["lead_stop_error"] = json!(format!("{error:#}")),
                    },
                    None => receipt["lead_stopped"] = json!(false),
                }
            }
            if starts_worker(op) {
                // The dispatcher runs while any team runs; it exits by itself.
                if let Err(error) = super::dispatcher::ensure_worker() {
                    eprintln!("hgs: the team dispatcher did not start: {error}");
                }
            }
            if receipt["status"] == "stale" {
                bail!("This attempt is no longer current for task #{}; the report was not accepted", receipt["task"]);
            }
            // Sections 9.5 and 9.7: the tree reacts to a finished, rejected,
            // paused, resumed or stopped team, and to a parent answer. N28: the
            // change above is already stored, so a tree hook that fails says so
            // in the receipt instead of turning the stored change into an error.
            if op == "finish" {
                if let Err(error) = file_subteam_report(&mut store, team_id) {
                    receipt["subteam_report_error"] = json!(format!("{error:#}"));
                }
            }
            if op == "reject" {
                if let Err(error) = reopen_rejected_subteam(&mut store, team_id, &filled) {
                    receipt["subteam_reopen_error"] = json!(format!("{error:#}"));
                }
            }
            if matches!(op, "cancel" | "reassign") {
                // C5: the sub-team stops only once the cancel or reassign
                // itself went through for its caller.
                let team = store.load(team_id)?.context("Unknown team")?;
                if let Err(error) = choose_subteam(&mut store, &team, &filled) {
                    receipt["subteam_error"] = json!(format!("{error:#}"));
                }
            }
            if matches!(op, "pause" | "resume" | "stop") {
                if let Err(error) = cascade_status(&mut store, team_id, op, 0) {
                    receipt["cascade_error"] = json!(format!("{error:#}"));
                }
            }
            if op == "reply" {
                if let Err(error) = route_reply_down(&mut store, team_id, &filled) {
                    receipt["reply_down_error"] = json!(format!("{error:#}"));
                }
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
    // C1: the detached runner of one claimed template run; the worker starts it,
    // hands it the request on standard input and never waits for a command.
    if args.len() == 2 && args[0] == "worker" && args[1] == "--run" {
        return super::dispatcher::worker_run();
    }
    // H2: `--json` reads stdin; `--json-file PATH` reads the same request from
    // a file (PowerShell 5.1 turns Cyrillic in a pipe into "?").
    let mut args = args.to_vec();
    let wait = args.iter().any(|arg| arg == "--wait");
    args.retain(|arg| arg != "--wait");
    let file = if let Some(index) = args.iter().position(|arg| arg == "--json-file") {
        let path = args
            .get(index + 1)
            .cloned()
            .ok_or_else(|| "--json-file needs a path".to_owned())?;
        args.drain(index..=index + 1);
        Some(path)
    } else {
        None
    };
    let catalog = args.first().map(String::as_str) == Some("catalog");
    if catalog {
        args.remove(0);
    }
    if args.len() != 2 || (args[1] != "--json" && file.is_none()) {
        return Err(USAGE.into());
    }
    let operation = args[0].clone();
    let bytes = if let Some(path) = file {
        std::fs::read(&path).map_err(|e| format!("Cannot read {path}: {e}"))?
    } else {
        let mut bytes = Vec::new();
        io::stdin().take(256 * 1024 + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        bytes
    };
    if bytes.len() > 256 * 1024 {
        return Err("Team request exceeds 256 KiB".into());
    }
    let input: Value = super::super::json_request(&bytes)?;
    if !input.is_object() {
        return Err("A JSON object is required".into());
    }
    let result = if catalog {
        // N4/N3: the catalog's owner path applies the same agent-session rule.
        let human = catalog_owner(&input).map_err(|error| error.to_string())?;
        super::catalog::run(&operation, &input, human)
    } else {
        run(&operation, &input, wait)
    }
    .map_err(|e| e.to_string())?;
    println!("{result}");
    Ok(0)
}

#[cfg(test)]
mod tests {
    thread_local! {
        static TEST_SWITCH: std::cell::RefCell<Option<super::TestSwitch>> = const { std::cell::RefCell::new(None) };
    }

    pub(crate) fn set_test_switch(switch: Option<super::TestSwitch>) {
        TEST_SWITCH.with(|slot| *slot.borrow_mut() = switch);
    }

    pub(crate) fn test_switch() -> Option<super::TestSwitch> {
        TEST_SWITCH.with(|slot| *slot.borrow())
    }

    thread_local! {
        static SWITCH_CALLS: std::cell::RefCell<Vec<(String, String)>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    fn switched_switch(name: &str, account: &str, _request: &str) -> anyhow::Result<Value> {
        SWITCH_CALLS.with(|calls| calls.borrow_mut().push((name.to_owned(), account.to_owned())));
        Ok(json!({"status":"switched"}))
    }

    fn uncertain_switch(_name: &str, _account: &str, _request: &str) -> anyhow::Result<Value> {
        Ok(json!({"status":"uncertain"}))
    }

    /// U54: one owner action moves the members on the account through U53's
    /// switch (keeping the session) and reports a switch that does not confirm.
    #[test]
    fn a_bulk_swap_moves_the_lead_through_u53_and_reports_a_refusal() {
        use super::*;
        let dir = tempfile::tempdir().unwrap();
        crate::state::with_test_state_root(dir.path(), || {
            live_sessions_override(Some(vec![crate::state::teams::dispatcher::Seen {
                name: "claude/x/lead".into(),
                run_id: "r0".into(),
                account_id: "x".into(),
                ..Default::default()
            }]));
            let _worker = hold_worker_lock();
            let mut store = Store::open(true).unwrap();
            let team = running_team();
            let id = team.id.clone();
            store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
            set_test_switch(Some(switched_switch));
            let receipt = run(
                "swap-member",
                &json!({"request_id":"22222222-2222-4222-8222-222222222222","team":id,
                    "from_account":"x","to_account":"y","provider":"claude","by":"human"}),
                false,
            )
            .unwrap();
            assert_eq!(receipt["status"], "swapped", "{receipt}");
            assert_eq!(
                SWITCH_CALLS.with(|calls| calls.borrow().clone()),
                vec![("claude/x/lead".to_owned(), "y".to_owned())]
            );
            let stored = store.load(&id).unwrap().unwrap();
            let lead = stored.member("lead").unwrap();
            assert_eq!(lead.account, "y");
            assert!(lead.session.is_some());
            // A switch that does not confirm moves nobody and names the reason.
            set_test_switch(Some(uncertain_switch));
            let error = run(
                "swap-member",
                &json!({"request_id":"33333333-3333-4333-8333-333333333333","team":id,
                    "from_account":"x","to_account":"z","provider":"claude","by":"human"}),
                false,
            )
            .unwrap_err()
            .to_string();
            assert!(error.contains("could move to account z"), "{error}");
            set_test_switch(None);
            live_sessions_override(None);
        });
    }
    /// U54: a bulk swap selects the members of one provider whose session
    /// really runs on the account, the lead included, and the roster reads the
    /// real account from the session record.
    #[test]
    fn a_bulk_swap_selects_members_by_their_sessions_real_account() {
        use crate::state::teams::dispatcher::Seen;
        use crate::state::teams::model::{Member, SessionRef, Team, TeamStatus};
        let seen = |name: &str, account: &str| Seen {
            name: name.into(),
            run_id: format!("{name}-run"),
            account_id: account.into(),
            ..Default::default()
        };
        let mut team = Team { id: "t".into(), name: "T".into(), lead: "lead".into(), status: TeamStatus::Running, ..Default::default() };
        let member = |id: &str, agent: &str, name: &str, account: &str| Member {
            id: id.into(),
            role: id.into(),
            agent: agent.into(),
            account: account.into(),
            session: Some(SessionRef { name: name.into(), run_id: format!("{name}-run"), ..Default::default() }),
            ..Default::default()
        };
        team.members = vec![
            member("lead", "claude", "claude/x/lead", "x"),
            member("dev", "claude", "claude/x/dev", "x"),
            member("qa", "codex", "codex/x/qa", "x"),
        ];
        let sessions = vec![seen("claude/x/lead", "x"), seen("claude/x/dev", "x"), seen("codex/x/qa", "x")];
        assert_eq!(bulk_swap_members(&team, &sessions, "x", "claude"), ["lead".to_owned(), "dev".to_owned()]);
        assert_eq!(bulk_swap_members(&team, &sessions, "x", ""), ["lead".to_owned(), "dev".to_owned(), "qa".to_owned()]);
        let elsewhere = vec![seen("claude/x/lead", "y"), seen("claude/x/dev", "x"), seen("codex/x/qa", "x")];
        assert_eq!(bulk_swap_members(&team, &elsewhere, "x", "claude"), ["dev".to_owned()]);
        let native = vec![seen("claude/x/lead", ""), seen("claude/x/dev", "x")];
        assert_eq!(bulk_swap_members(&team, &native, "native-claude", "claude"), ["lead".to_owned()]);
        // `hgs ls` names a default-account session "native-claude": "", "native"
        // and "native-claude" match it alike.
        let listed = vec![seen("claude/x/lead", "native-claude"), seen("claude/x/dev", "native"), seen("codex/x/qa", "x")];
        for from in ["", "native", "native-claude"] {
            assert_eq!(bulk_swap_members(&team, &listed, from, "claude"), ["lead".to_owned(), "dev".to_owned()], "{from}");
        }
        assert!(bulk_swap_members(&team, &listed, "x", "claude").is_empty());
        let dir = tempfile::tempdir().unwrap();
        crate::state::with_test_state_root(dir.path(), || {
            let mut record = json!({"version":1,"name":"claude/x/dev","agent":"claude","account_id":"y"});
            crate::state::storage::write(&mut record).unwrap();
            assert_eq!(session_account(team.member("dev").unwrap()), "y");
            // A session without a readable record keeps its configured account.
            assert_eq!(session_account(team.member("qa").unwrap()), "x");
        });
    }

    /// U54: the members a bulk move takes are all swapped: the moved ones keep
    /// their session, the fresh ones get a new one on the target account, and
    /// an all-fresh move is not read as a single-member swap.
    #[test]
    fn a_bulk_swap_swaps_moved_fresh_and_mixed_members() {
        let swap = |moved: &[&str], fresh: &[&str]| {
            let mut team = running_team();
            let owned = |ids: &[&str]| ids.iter().map(|id| id.to_string()).collect::<Vec<_>>();
            let notes = fresh.iter().map(|id| json!({"member":id,"note":"the old session was paused and saved as x"})).collect();
            let mut input = json!({"from_account":"x","to_account":"y"});
            fill_bulk_swap(&mut input, &owned(moved), &owned(fresh), notes, Vec::new(), "y");
            let mut events = Vec::new();
            let receipt = ops::apply(&mut team, "swap-member", &input, &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
            (team, receipt, events)
        };
        // Moved only: the lead keeps its session and takes the new account.
        let (team, receipt, _) = swap(&["lead"], &[]);
        assert_eq!(receipt["status"], "swapped", "{receipt}");
        let lead = team.member("lead").unwrap();
        assert_eq!((lead.account.as_str(), lead.session.as_ref().map(|s| s.name.as_str())), ("y", Some("claude/x/lead")));
        assert!(!lead.launch_after_swap);
        // Fresh only: the member is swapped, not refused as a single swap.
        let (team, receipt, events) = swap(&[], &["dev"]);
        assert_eq!(receipt["fresh"], json!(["dev"]), "{receipt}");
        let dev = team.member("dev").unwrap();
        assert!(dev.session.is_none() && dev.launch_after_swap && dev.state == MemberState::NotCreated, "{dev:?}");
        assert_eq!(dev.account, "y");
        assert!(events.iter().any(|event| event.kind == "member_retired"));
        // Mixed: each one its own way.
        let (team, receipt, _) = swap(&["lead"], &["dev"]);
        assert_eq!(receipt["members"].as_array().map(Vec::len), Some(2), "{receipt}");
        assert!(team.member("lead").unwrap().session.is_some());
        assert!(team.member("dev").unwrap().session.is_none() && team.member("dev").unwrap().launch_after_swap);
    }

    /// U54: a bulk swap keeps each member's session, conversation and open
    /// attempt and only changes the account.
    #[test]
    fn a_bulk_swap_keeps_each_members_session_and_open_attempt() {
        use super::*;
        use crate::state::teams::model::{Attempt, AttemptState, Member, SessionRef, Task, TaskStatus, Team, TeamStatus};
        let mut team = Team { id: "t".into(), name: "T".into(), lead: "lead".into(), status: TeamStatus::Running, ..Default::default() };
        let member = |id: &str| Member {
            id: id.into(),
            role: id.into(),
            agent: "claude".into(),
            account: "x".into(),
            session: Some(SessionRef { name: format!("claude/x/{id}"), run_id: "r".into(), ..Default::default() }),
            ..Default::default()
        };
        team.members = vec![member("lead"), member("dev")];
        team.tasks.push(Task {
            id: 1,
            title: "A".into(),
            assignee: "dev".into(),
            status: TaskStatus::InProgress,
            attempts: vec![Attempt { n: 1, id: "a1".into(), member: "dev".into(), state: AttemptState::Delivered, ..Default::default() }],
            ..Default::default()
        });
        let mut events = Vec::new();
        let result = ops::apply(
            &mut team,
            "swap-member",
            &json!({"members":["lead","dev"],"bulk":true,"account":"y"}),
            &Actor::Human,
            &BTreeSet::new(),
            &mut events,
        )
        .unwrap();
        assert_eq!(result["status"], "swapped", "{result}");
        assert_eq!(result["members"].as_array().unwrap().len(), 2);
        for id in ["lead", "dev"] {
            let found = team.member(id).unwrap();
            assert_eq!(found.account, "y");
            assert!(found.session.is_some());
            assert!(!found.launch_after_swap);
        }
        let task = team.task(1).unwrap();
        assert_eq!(task.status, TaskStatus::InProgress);
        assert_eq!(task.attempts[0].state, AttemptState::Delivered);
        assert!(task.continuation.is_none());
        assert!(events.iter().any(|event| event.kind == "member_swapped"));
    }

    /// U54: a swap never interrupts a session that is busy or unreachable; the
    /// member waits with that reason and the session keeps running.
    #[test]
    fn a_busy_old_session_is_never_interrupted_by_a_swap() {
        use super::*;
        let dir = tempfile::tempdir().unwrap();
        crate::state::with_test_state_root(dir.path(), || {
            let mut record = json!({"version":1,"name":"claude/x/dev","agent":"claude","run_id":"r","conversation_id":"c"});
            crate::state::storage::write(&mut record).unwrap();
            let error = retire_old_session("claude/x/dev", "second").unwrap_err();
            assert!(error.contains("waiting for the old session"), "{error}");
            let saved = crate::state::storage::read("claude/x/dev").unwrap();
            assert!(saved.get("paused").is_none(), "a busy session is not paused");
        });
    }

    /// U54: the lead relaunches a member only on a word that names it.
    #[test]
    fn the_leads_relaunch_needs_a_word_that_names_the_member() {
        use super::*;
        use crate::state::teams::model::Member;
        let team = Team {
            id: "t".into(),
            name: "T".into(),
            lead: "lead".into(),
            members: vec![Member { id: "dev".into(), role: "Backend".into(), agent: "claude".into(), ..Default::default() }],
            ..Default::default()
        };
        let rule = roster_word_rule(&team, "relaunch", &json!({"member":"dev"}));
        assert!(rule.needed());
        assert!(rule.names.iter().any(|name| name == "dev"), "{:?}", rule.names);
        assert!(rule.names.iter().any(|name| name.eq_ignore_ascii_case("backend")), "{:?}", rule.names);
        // Another member's relaunch is not this word's.
        assert!(!roster_word_rule(&team, "relaunch", &json!({"member":""})).needed());
    }

    /// U54: a renamed session stays the member. The caller matches by its run
    /// id or conversation id as well as by name, and the team record follows the
    /// rename once (the next command matches by name again).
    #[test]
    fn a_renamed_session_stays_the_member_and_the_team_follows_it() {
        use crate::state::teams::model::{Member, SessionRef, Team, TeamStatus};
        let mut team = Team { id: "t".into(), name: "T".into(), lead: "lead".into(), status: TeamStatus::Running, ..Default::default() };
        team.members.push(Member {
            id: "lead".into(),
            role: "Lead".into(),
            session: Some(SessionRef {
                name: "claude/x/old".into(),
                run_id: "r1".into(),
                conversation_id: "c1".into(),
                ..Default::default()
            }),
            ..Default::default()
        });
        for record in [
            json!({"name":"claude/x/new","run_id":"r1"}),
            json!({"name":"claude/x/new","run_id":"","conversation_id":"c1"}),
            json!({"name":"claude/x/old","run_id":"r9"}),
        ] {
            assert_eq!(calling_member(&team, &record).as_deref(), Some("lead"), "{record}");
        }
        assert_eq!(calling_member(&team, &json!({"name":"claude/x/other","run_id":"r2","conversation_id":"c2"})), None);
        let dir = tempfile::tempdir().unwrap();
        crate::state::with_test_state_root(dir.path(), || {
            let mut store = Store::open(true).unwrap();
            store.create("11111111-1111-4111-8111-111111111111", &json!({}), team.clone(), vec![]).unwrap();
            let record = json!({"name":"claude/x/new","run_id":"r1","conversation_id":"c1"});
            follow_session_rename(&mut store, "t", Some(&record)).unwrap();
            let stored = store.load("t").unwrap().unwrap();
            let session = stored.member("lead").unwrap().session.clone().unwrap();
            assert_eq!((session.name.as_str(), session.run_id.as_str()), ("claude/x/new", "r1"));
            let before = store.events("t", None, 100).unwrap();
            follow_session_rename(&mut store, "t", Some(&record)).unwrap();
            assert_eq!(store.events("t", None, 100).unwrap(), before);
        });
    }
    /// N37/F1.6: status reads each member's own folder - a worktree member's
    /// worktree, not the team folder - one read per distinct folder, and flags
    /// a member in another repository. N36: the team's project label is a GUI
    /// group name and never decides the flag.
    #[test]
    fn status_reads_each_members_own_folder_once_and_flags_another_repository() {
        use crate::state::teams::dispatcher;
        use crate::state::teams::model::{Member, MemberFolder, Team, TeamStatus};
        let dir = tempfile::tempdir().unwrap();
        // A Windows runner's TEMP is an 8.3 short path (RUNNER~1); status names
        // folders in the long form git reports, so the test works in that form.
        let base = std::fs::canonicalize(dir.path()).unwrap();
        let base = std::path::PathBuf::from(base.to_string_lossy().trim_start_matches(r"\\?\").to_owned());
        dispatcher::set_git_ceiling(&base.display().to_string());
        // Two different repositories: distinct paths and distinct initial
        // commits. Identical content committed in the same second makes the same
        // root commit, and clone_of would rightly call them one repository, so
        // the foreign repo must carry its own history (asserted below).
        let make_repo = |path: &std::path::Path, name: &str| {
            std::fs::create_dir(path).unwrap();
            let git = |args: &[&str]| std::process::Command::new("git").arg("-C").arg(path).args(args).status().unwrap();
            assert!(git(&["init", "-q"]).success());
            assert!(git(&["config", "user.email", "t@example.com"]).success());
            assert!(git(&["config", "user.name", "T"]).success());
            std::fs::write(path.join(format!("{name}.txt")), name).unwrap();
            assert!(git(&["add", "."]).success());
            assert!(git(&["commit", "-qm", name]).success());
        };
        let root = |path: &std::path::Path| {
            String::from_utf8(
                std::process::Command::new("git").arg("-C").arg(path).args(["rev-list", "--max-parents=0", "HEAD"]).output().unwrap().stdout,
            )
            .unwrap()
            .trim()
            .to_owned()
        };
        let repo = base.join("project");
        make_repo(&repo, "project");
        let main = String::from_utf8(std::process::Command::new("git").arg("-C").arg(&repo).args(["rev-parse", "--abbrev-ref", "HEAD"]).output().unwrap().stdout).unwrap().trim().to_owned();
        let folder = repo.display().to_string();
        let mut team = Team {
            id: "abcdef12-0000-4000-8000-000000000000".into(),
            name: "T".into(),
            lead: "lead".into(),
            status: TeamStatus::Running,
            folder: folder.clone(),
            // The GUI's group name, not a project id or a path (N36).
            project: "Checkout".into(),
            ..Default::default()
        };
        team.members.push(Member { id: "lead".into(), role: "Lead".into(), agent: "claude".into(), ..Default::default() });
        team.members.push(Member { id: "dev".into(), role: "Backend".into(), agent: "dsh".into(), folder: MemberFolder::Worktree, ..Default::default() });
        team.members.push(Member { id: "qa".into(), role: "QA".into(), agent: "codex".into(), ..Default::default() });
        let foreign = base.join("sibling");
        make_repo(&foreign, "sibling");
        assert_ne!(root(&repo), root(&foreign), "the foreign repository must have its own history");
        team.members.push(Member { id: "out".into(), role: "Ops".into(), agent: "codex".into(), directory: foreign.display().to_string(), ..Default::default() });
        // A member on another computer works in a folder this one cannot read.
        team.members.push(Member {
            id: "far".into(),
            role: "Docs".into(),
            agent: "codex".into(),
            computer: "msi".into(),
            directory: foreign.display().to_string(),
            ..Default::default()
        });
        // The worktree the dispatcher would create for dev.
        let worktree = dispatcher::member_folder_path(&team, team.member("dev").unwrap()).unwrap();
        assert!(std::process::Command::new("git").arg("-C").arg(&repo).args(["worktree", "add", "--quiet", "-b", "team/abcdef12/dev", &worktree, "HEAD"]).status().unwrap().success());
        crate::state::with_test_state_root(dir.path(), || {
            let mut store = Store::open(true).unwrap();
            store.create("11111111-1111-4111-8111-111111111111", &json!({}), team.clone(), vec![]).unwrap();
            dispatcher::reset_folder_facts_reads();
            let value = run("status", &json!({"team":team.id}), false).unwrap();
            assert_eq!(value["members"].as_array().unwrap().len(), 5);
            let member = |id: &str| value["members"].as_array().unwrap().iter().find(|member| member["id"] == id).cloned().unwrap();
            // dev shows its own worktree's branch and folder, not the team folder's.
            let dev = member("dev");
            assert_eq!(dev["branch"], "team/abcdef12/dev");
            assert_eq!(dev["folder_path"], worktree);
            assert_eq!(dev["outside_project"], false);
            assert_eq!(dev["project"], member("lead")["project"]);
            assert_eq!(dev["project"].as_str(), Some(folder.replace('\\', "/").as_str()));
            // The shared members show the team folder's branch; the foreign one
            // is flagged and the group name plays no part.
            let lead = member("lead");
            assert_eq!(lead["branch"], main);
            assert_eq!(lead["outside_project"], false);
            let out = member("out");
            assert_eq!(out["outside_project"], true);
            assert_eq!(out["folder_path"], foreign.display().to_string());
            assert!(!out["branch"].as_str().unwrap_or_default().is_empty());
            // The remote member's folder is not read here (C11): its facts stay
            // empty even though a local folder has that path.
            let far = member("far");
            assert_eq!((far["project"].as_str(), far["branch"].as_str()), (Some(""), Some("")));
            assert_eq!(far["outside_project"], false);
            // One git read per distinct folder: the team's, dev's worktree and
            // the foreign one, for five members - not one per member.
            assert_eq!(dispatcher::folder_facts_reads(), 3);
        });
    }

    /// The dispatcher's worker lock, held so a command that would start a
    /// worker does not spawn one from the test binary.
    fn hold_worker_lock() -> crate::platform::FileLock {
        let path = crate::state::teams::dispatcher::lock_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let file = std::fs::OpenOptions::new().create(true).write(true).truncate(false).open(&path).unwrap();
        let lock = crate::platform::FileLock::exclusive(file).unwrap();
        assert!(crate::status::locked(&path), "the worker lock is held");
        lock
    }

    /// N39/F1.3: add-member checks the new member's folder against the team's
    /// policy, as plan and approve do, and files nothing on a mismatch.
    #[test]
    fn adding_a_member_whose_folder_breaks_the_policy_is_refused() {
        use crate::state::teams::model::FolderPolicy;
        let dir = tempfile::tempdir().unwrap();
        crate::state::with_test_state_root(dir.path(), || {
            let mut store = Store::open(true).unwrap();
            let mut team = running_team();
            team.settings.folder_policy = FolderPolicy::Shared;
            let id = team.id.clone();
            store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
            let error = run(
                "add-member",
                &json!({"request_id":"22222222-2222-4222-8222-222222222222","team":id,"by":"human",
                    "member":{"id":"ops","role":"Ops","agent":"codex","folder":"worktree"}}),
                false,
            )
            .unwrap_err()
            .to_string();
            assert!(error.contains("folder error(s) to add the member"), "{error}");
            assert!(error.contains("the shared folder policy leaves no own folder"), "{error}");
            let saved = Store::open(false).unwrap().load(&id).unwrap().unwrap();
            assert!(saved.member("ops").is_none(), "the refused member is not filed");
            // A member that fits the policy is added.
            run(
                "add-member",
                &json!({"request_id":"33333333-3333-4333-8333-333333333333","team":id,"by":"human",
                    "member":{"id":"ops","role":"Ops","agent":"codex"}}),
                false,
            )
            .unwrap();
            let saved = Store::open(false).unwrap().load(&id).unwrap().unwrap();
            assert!(saved.member("ops").is_some());
        });
    }

    /// N39/F1.3: extend keeps a folder or policy mismatch instead of dropping
    /// it (it names no task), so the phase that cannot run is not added.
    #[test]
    fn extending_a_team_whose_folders_break_the_policy_is_refused() {
        use crate::state::teams::model::FolderPolicy;
        let dir = tempfile::tempdir().unwrap();
        crate::state::with_test_state_root(dir.path(), || {
            let _worker = hold_worker_lock();
            let mut store = Store::open(true).unwrap();
            let mut team = running_team();
            team.settings.folder_policy = FolderPolicy::Worktree;
            let id = team.id.clone();
            store.create("11111111-1111-4111-8111-111111111112", &json!({}), team, vec![]).unwrap();
            let error = run(
                "extend",
                &json!({"request_id":"22222222-2222-4222-8222-222222222223","team":id,"by":"human",
                    "tasks":[{"id":2,"title":"More","assignee":"dev"}]}),
                false,
            )
            .unwrap_err()
            .to_string();
            assert!(error.contains("phase error(s) to extend"), "{error}");
            assert!(error.contains("the worktree policy gives every member its own folder"), "{error}");
            let saved = Store::open(false).unwrap().load(&id).unwrap().unwrap();
            assert!(saved.task(2).is_none(), "the refused phase is not added");
        });
    }

    /// N4/F1: `hgs team run` answers from the member's folder: a result already
    /// there never queues the run again, the wait is short, and the same
    /// request_id reads the outcome instead of running the command twice.
    #[test]
    fn a_run_answers_without_queueing_it_twice_and_waits_briefly() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("work");
        std::fs::create_dir_all(&folder).unwrap();
        crate::state::teams::templates::settings_override(Some(dir.path().join("templates.json")));
        crate::state::with_test_state_root(dir.path(), || {
            super::set_test_caller_identities(Some((Some("conv-dev".into()), None, None)));
            let mut store = Store::open(true).unwrap();
            let mut team = running_team();
            team.folder = folder.display().to_string();
            let member = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
            member.session = Some(SessionRef { name: "dsh/x/dev".into(), run_id: "r1".into(), conversation_id: "conv-dev".into(), ..Default::default() });
            let attempt = team.task(1).unwrap().open_attempt().unwrap().id.clone();
            let id = team.id.clone();
            store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
            let input = |request: &str| json!({"request_id":request,"team":id,"attempt":attempt,"template":"cargo-test","args":{"filter":"teams"}});
            // An answer already in the folder is the answer: no push, no wait.
            let request = "22222222-2222-4222-8222-222222222222";
            std::fs::create_dir_all(super::super::outbox::dir(&folder)).unwrap();
            let answer = json!({"status":"ran","request_id":request,"exit":0,"tail":"old"});
            std::fs::write(super::super::outbox::result_path(&folder, request), serde_json::to_vec(&answer).unwrap()).unwrap();
            let value = super::team_run_in(&folder, &input(request), false, 0.1).unwrap();
            assert_eq!((value["status"].as_str(), value["tail"].as_str()), (Some("ran"), Some("old")));
            assert!(!super::super::outbox::request_path(&folder, request).exists(), "the result queued the run again");
            // A short wait, not the template's 1800 s.
            let request = "33333333-3333-4333-8333-333333333333";
            let started = std::time::Instant::now();
            let value = super::team_run_in(&folder, &input(request), true, 0.2).unwrap();
            assert!(started.elapsed() < std::time::Duration::from_secs(10), "the wait took {:?}", started.elapsed());
            assert_eq!(value["status"], "running", "{value}");
            assert!(super::super::outbox::request(&folder, request).is_some(), "the queued request left");
            // Asking again with the same id reads the queue; another run cannot reuse it.
            let value = super::team_run_in(&folder, &input(request), false, 0.1).unwrap();
            assert_eq!(value["status"], "queued", "{value}");
            let mut other = input(request);
            other["template"] = json!("cargo-clippy");
            let error = super::team_run_in(&folder, &other, false, 0.1).unwrap_err().to_string();
            assert!(error.contains("already used by another run"), "{error}");
            // A result written later comes back without a wait.
            let done = json!({"status":"ran","request_id":request,"exit":0,"tail":"done"});
            super::super::outbox::settle(&folder, &json!({"request_id":request}), &done).unwrap();
            assert_eq!(super::team_run_in(&folder, &input(request), false, 0.1).unwrap()["tail"], "done");
        });
        crate::state::teams::templates::settings_override(None);
    }

    /// N5: a sandboxed write queues in the member's own folder, never in the
    /// process's directory: a member that ran `cd tray` before reporting would
    /// otherwise queue where the worker never looks.
    #[test]
    fn a_sandboxed_write_queues_in_the_members_folder_not_the_process_directory() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        crate::state::teams::templates::settings_override(Some(dir.path().join("templates.json")));
        crate::state::with_test_state_root(dir.path(), || {
            super::set_test_caller_identities(Some((Some("conv-dev".into()), None, None)));
            let mut store = Store::open(true).unwrap();
            let mut team = running_team();
            team.folder = project.display().to_string();
            let member = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
            member.session = Some(SessionRef { name: "dsh/x/dev".into(), run_id: "r1".into(), conversation_id: "conv-dev".into(), ..Default::default() });
            let attempt = team.task(1).unwrap().open_attempt().unwrap().id.clone();
            let id = team.id.clone();
            store.create("11111111-1111-4111-8111-111111111111", &json!({}), team.clone(), vec![]).unwrap();
            let member_folder = std::path::PathBuf::from(
                super::super::dispatcher::member_folder_path(&team, team.member("dev").unwrap()).unwrap(),
            );
            assert_eq!(member_folder, project);
            let report = "22222222-2222-4222-8222-222222222222";
            let input = json!({"request_id":report,"team":id,"attempt":attempt,"text":"Done and checked"});
            let value = super::queue_write("report", &input, false).unwrap();
            assert_eq!(value["status"], "queued", "{value}");
            assert!(super::super::outbox::request_path(&member_folder, report).exists(), "the report queued outside the member's folder");
            let run = "33333333-3333-4333-8333-333333333333";
            let input = json!({"request_id":run,"team":id,"attempt":attempt,"template":"cargo-test","args":{"filter":"teams"}});
            let value = super::team_run(&input, false).unwrap();
            assert_eq!(value["status"], "queued", "{value}");
            assert!(super::super::outbox::request_path(&member_folder, run).exists(), "the run queued outside the member's folder");
            // Not in the directory this test process happens to run in.
            let here = std::env::current_dir().unwrap();
            assert!(!super::super::outbox::request_path(&here, report).exists());
            assert!(!super::super::outbox::request_path(&here, run).exists());
        });
        crate::state::teams::templates::settings_override(None);
    }

    /// N5: a finished team's inbox still carries the owner's decision on the
    /// lead's proposal (spec 4), so the lead hears the answer.
    #[test]
    fn a_done_team_reports_the_owners_proposal_decision_in_its_inbox() {
        let dir = tempfile::tempdir().unwrap();
        crate::state::with_test_state_root(dir.path(), || {
            let mut store = Store::open(true).unwrap();
            let mut team = running_team();
            team.status = TeamStatus::Done;
            team.proposals.push(json!({"id":"p1","kind":"next-goal","by":"Lead","at":10.0,"state":"accepted",
                "decided_at":20.0,"note":"Go on","goal":"v2"}));
            let id = team.id.clone();
            store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
            let value = run("inbox", &json!({"team":id,"by":"human"}), false).unwrap();
            let items = value["items"].as_array().unwrap();
            let item = items.iter().find(|item| item["kind"] == "proposal_decided").expect("the decision is missing");
            assert!(
                item["summary"].as_str().unwrap_or_default().contains("accepted your next-goal proposal p1: Go on"),
                "{item}"
            );
            // A running team with the same proposal lists nothing stale twice.
            let mut running = running_team();
            running.proposals.push(json!({"id":"p2","kind":"next-goal","by":"Lead","at":10.0,"state":"rejected","decided_at":20.0,"note":"No"}));
            assert_eq!(super::super::inbox::items(&running).iter().filter(|item| item.kind == "proposal_decided").count(), 1);
        });
    }

    /// N3: a finished team starts one next goal, however many requests ask for
    /// it; a replay of the first answers its receipt.
    #[test]
    fn a_finished_team_starts_only_one_next_goal() {
        let dir = tempfile::tempdir().unwrap();
        crate::state::with_test_state_root(dir.path(), || {
            super::live_sessions_override(Some(Vec::new()));
            let _worker = hold_worker_lock();
            let mut store = Store::open(true).unwrap();
            let mut team = running_team();
            team.status = TeamStatus::Done;
            let id = team.id.clone();
            store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
            let first = run("next-goal", &json!({"team":id,"goal":"v2","by":"human","request_id":"22222222-2222-4222-8222-222222222222"}), false).unwrap();
            assert_eq!(first["status"], "planned", "{first}");
            let error = run("next-goal", &json!({"team":id,"goal":"v3","by":"human","request_id":"33333333-3333-4333-8333-333333333333"}), false)
                .unwrap_err()
                .to_string();
            assert!(error.contains("already started"), "{error}");
            let again = run("next-goal", &json!({"team":id,"goal":"v2","by":"human","request_id":"22222222-2222-4222-8222-222222222222"}), false).unwrap();
            assert_eq!(again["team"], first["team"]);
        });
        super::live_sessions_override(None);
    }

    /// N3: an accepted proposal's change runs under the proposal's own request
    /// id, so a decision interrupted between its writes replays instead of
    /// running the change twice.
    #[test]
    fn an_accepted_proposal_replays_instead_of_running_twice() {
        let dir = tempfile::tempdir().unwrap();
        crate::state::with_test_state_root(dir.path(), || {
            let mut store = Store::open(true).unwrap();
            let mut team = running_team();
            team.members.push(Member { id: "ops".into(), role: "Ops".into(), agent: "codex".into(), ..Default::default() });
            team.proposals.push(json!({"id":"p1","kind":"remove-member","by":"Lead","at":10.0,"state":"open","reason":"bye",
                "member":"ops","operation_request":"44444444-4444-4444-8444-444444444444"}));
            let id = team.id.clone();
            store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
            let decide = |store: &mut Store| {
                super::decide_proposal_in(
                    store,
                    &json!({"request_id":"55555555-5555-4555-8555-555555555555","team":id,"proposal":"p1","accept":true,"reason":"bye"}),
                    &Actor::Human,
                )
                .unwrap()
            };
            let first = decide(&mut store);
            assert_eq!(first["status"], "accepted", "{first}");
            let removals = |store: &Store| {
                let events = store.events(&id, None, 50).unwrap();
                events["items"].as_array().unwrap().iter().filter(|event| event["kind"] == "member_removed").count()
            };
            assert_eq!(removals(&store), 1);
            // The change went through but the acceptance was lost (a crash
            // between the two writes): the proposal is open again.
            store
                .mutate(None, &id, None, |team, _| {
                    if let Some(proposal) = team.proposals.iter_mut().find(|proposal| proposal["id"] == "p1") {
                        proposal["state"] = json!("open");
                    }
                    Ok(Value::Null)
                })
                .unwrap();
            let again = decide(&mut store);
            assert_eq!(again["status"], "accepted", "{again}");
            assert_eq!(removals(&store), 1, "the change ran twice");
        });
    }

    /// N4: an accepted add-member proposal runs the same adoption checks as the
    /// owner's own change (C8's owner_change), not a bare ops::apply.
    #[test]
    fn an_accepted_add_member_runs_the_adoption_checks() {
        let dir = tempfile::tempdir().unwrap();
        crate::state::with_test_state_root(dir.path(), || {
            super::live_sessions_override(Some(Vec::new()));
            let mut store = Store::open(true).unwrap();
            let mut team = running_team();
            team.proposals.push(json!({"id":"p1","kind":"add-member","by":"Lead","at":10.0,"state":"open","reason":"needs help",
                "member":{"id":"ops","role":"Ops","agent":"codex","session":{"name":"codex/x/ops","host":""}},
                "operation_request":"44444444-4444-4444-8444-444444444444"}));
            let id = team.id.clone();
            store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
            let error = super::decide_proposal_in(
                &mut store,
                &json!({"request_id":"55555555-5555-4555-8555-555555555555","team":id,"proposal":"p1","accept":true,"reason":"ok"}),
                &Actor::Human,
            )
            .unwrap_err()
            .to_string();
            assert!(error.contains("is not running on this computer"), "{error}");
            let saved = store.load(&id).unwrap().unwrap();
            assert_eq!(saved.proposals[0]["state"], "open", "the proposal was accepted without the check");
            assert!(saved.member("ops").is_none());
        });
        super::live_sessions_override(None);
    }

    /// N7: discarding a draft keeps its reason and stops the lead's session,
    /// only when the owner asked for it (spec 3).
    #[test]
    fn discarding_a_draft_stops_its_lead_and_keeps_the_reason() {
        let dir = tempfile::tempdir().unwrap();
        crate::state::with_test_state_root(dir.path(), || {
            let _worker = hold_worker_lock();
            let mut store = Store::open(true).unwrap();
            let mut team = running_team();
            team.status = TeamStatus::Draft;
            team.draft = Some(Draft { revision: 3, ..Default::default() });
            team.brief = Notice::Pending;
            {
                let lead = team.members.iter_mut().find(|member| member.id == "lead").unwrap();
                lead.session = Some(SessionRef { name: "claude/x/lead".into(), run_id: "r0".into(), ..Default::default() });
                lead.state = MemberState::Active;
            }
            let id = team.id.clone();
            store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
            super::record_lead_stops(true);
            let value = run(
                "draft",
                &json!({"team":id,"by":"human","action":"discard","stop_lead":true,"reason":"not worth it",
                    "request_id":"22222222-2222-4222-8222-222222222222"}),
                false,
            )
            .unwrap();
            super::record_lead_stops(false);
            assert_eq!(value["lead_stopped"], true, "{value}");
            assert_eq!(super::lead_stops(), vec![(String::new(), "claude/x/lead".to_string())]);
            let saved = Store::open(false).unwrap().load(&id).unwrap().unwrap();
            assert_eq!((saved.status, saved.stop_reason.as_str()), (TeamStatus::Stopped, "draft_discarded"));
            assert!(saved.draft.is_none());
            // The durable feed names the reason and the stop request.
            let events = Store::open(false).unwrap().events(&id, None, 20).unwrap();
            let event = events["items"].as_array().unwrap().iter().find(|event| event["kind"] == "draft_discarded").expect("no event");
            assert_eq!(event["detail"]["reason"], "not worth it");
            assert_eq!(event["detail"]["stop_lead"], true);
            // A discard without stop_lead stops nothing.
            let mut second = running_team();
            second.id = "d2".into();
            second.status = TeamStatus::Draft;
            second.draft = Some(Draft { revision: 1, ..Default::default() });
            let other = second.id.clone();
            Store::open(true).unwrap().create("33333333-3333-4333-8333-333333333333", &json!({}), second, vec![]).unwrap();
            super::record_lead_stops(true);
            let value = run(
                "draft",
                &json!({"team":other,"by":"human","action":"discard","stop_lead":false,
                    "request_id":"44444444-4444-4444-8444-444444444444"}),
                false,
            )
            .unwrap();
            super::record_lead_stops(false);
            assert!(value.get("lead_stopped").is_none(), "{value}");
            assert!(super::lead_stops().is_empty());
        });
    }

    /// N10: a member whose only open tasks are drafts leaves when the owner
    /// names the member that takes them (spec 4).
    #[test]
    fn a_removed_member_drafts_move_to_the_named_member() {
        let dir = tempfile::tempdir().unwrap();
        crate::state::with_test_state_root(dir.path(), || {
            let _worker = hold_worker_lock();
            let mut store = Store::open(true).unwrap();
            let mut team = running_team();
            team.members.push(Member { id: "qa".into(), role: "QA".into(), agent: "codex".into(), ..Default::default() });
            team.tasks.push(Task { id: 2, title: "Later work".into(), assignee: "dev".into(), status: TaskStatus::Draft, phase: 2, ..Default::default() });
            team.tasks.push(Task { id: 3, title: "Started".into(), assignee: "dev".into(), status: TaskStatus::Pending, phase: 2, ..Default::default() });
            let id = team.id.clone();
            store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
            // Without a target the removal is refused, and with a target only
            // the drafts may move.
            let error = run("remove-member", &json!({"team":id,"by":"human","member":"dev","request_id":"22222222-2222-4222-8222-222222222222"}), false)
                .unwrap_err()
                .to_string();
            assert!(error.contains("Reassign or cancel its open tasks first"), "{error}");
            let error = run(
                "remove-member",
                &json!({"team":id,"by":"human","member":"dev","reassign_drafts_to":"qa","request_id":"33333333-3333-4333-8333-333333333333"}),
                false,
            )
            .unwrap_err()
            .to_string();
            assert!(error.contains("started tasks first"), "{error}");
            // Its started work is cancelled: the draft moves to qa and dev goes.
            for (task, request) in [(1, "44444444-4444-4444-8444-444444444444"), (3, "55555555-5555-4555-8555-555555555555")] {
                run("cancel", &json!({"team":id,"by":"human","task":task,"request_id":request}), false).unwrap();
            }
            let value = run(
                "remove-member",
                &json!({"team":id,"by":"human","member":"dev","reassign_drafts_to":"qa","request_id":"66666666-6666-4666-8666-666666666666"}),
                false,
            )
            .unwrap();
            assert_eq!(value["status"], "removed", "{value}");
            let saved = Store::open(false).unwrap().load(&id).unwrap().unwrap();
            assert_eq!(saved.task(2).unwrap().assignee, "qa");
            assert_eq!(saved.member("dev").unwrap().state, MemberState::Removed);
        });
    }

    /// The owner, 2026-10-28: the lead or the owner records safe actions, a
    /// member never does, and the team's journal names each change.
    #[test]
    fn only_the_lead_or_the_owner_records_safe_actions() {
        let dir = tempfile::tempdir().unwrap();
        crate::state::with_test_state_root(dir.path(), || {
            let _worker = hold_worker_lock();
            let mut store = Store::open(true).unwrap();
            let mut team = running_team();
            for (id, conversation) in [("lead", "conv-lead"), ("dev", "conv-dev")] {
                let member = team.members.iter_mut().find(|member| member.id == id).unwrap();
                member.session = Some(SessionRef { name: format!("dsh/x/{id}"), run_id: "r1".into(), conversation_id: conversation.into(), ..Default::default() });
            }
            let id = team.id.clone();
            store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
            let add = json!({"team":id,"action":"add","role":"Backend","shape":"cargo test --locked --bin hgs {word}","reason":"its own tests"});
            super::set_test_caller_identities(Some((Some("conv-dev".into()), None, None)));
            assert!(run("safe-action", &add, false).is_err());
            super::set_test_caller_identities(Some((Some("conv-lead".into()), None, None)));
            let listed = run("safe-action", &add, false).unwrap();
            let action = listed["actions"][0].clone();
            assert_eq!((action["shape"].as_str(), action["by"].as_str()), (Some("cargo test --locked --bin hgs {word}"), Some("lead")), "{listed}");
            // A shape that is too short, or starts with a slot, is refused.
            let mut short = add.clone();
            short["shape"] = json!("ls {path}");
            assert!(run("safe-action", &short, false).is_err());
            super::set_test_caller_identities(Some((None, None, None)));
            let removed = run("safe-action", &json!({"team":id,"action":"remove","id":action["id"],"by":"human"}), false).unwrap();
            assert_eq!(removed["actions"], json!([]));
            let events = Store::open(false).unwrap().events(&id, None, 20).unwrap();
            let kinds: Vec<&str> = events["items"].as_array().unwrap().iter().filter_map(|event| event["kind"].as_str()).collect();
            assert!(kinds.contains(&"safe_action_added") && kinds.contains(&"safe_action_removed"), "{kinds:?}");
        });
    }

    /// F5: the owner's edit wins only over a lead edit made before the lead read
    /// it; the lead's read clears the marks, so the lead can change the field.
    #[test]
    fn a_lead_changes_an_owner_edited_field_after_reading_the_draft() {
        let dir = tempfile::tempdir().unwrap();
        crate::state::with_test_state_root(dir.path(), || {
            let _worker = hold_worker_lock();
            let mut store = Store::open(true).unwrap();
            let mut team = running_team();
            team.status = TeamStatus::Draft;
            team.draft = Some(Draft { revision: 5, owner_edited: vec!["members".into()], ..Default::default() });
            let lead_id = team.lead.clone();
            {
                let lead = team.members.iter_mut().find(|member| member.id == lead_id).unwrap();
                lead.session =
                    Some(SessionRef { name: "dsh/x/lead".into(), run_id: "r1".into(), conversation_id: "conv-lead".into(), ..Default::default() });
            }
            let id = team.id.clone();
            store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
            let update = json!({"team":id,"action":"update","revision":5,"by":"human",
                "set":{"members":[{"id":"dev2","role":"Ops","agent":"codex"}]},
                "request_id":"22222222-2222-4222-8222-222222222222"});
            // As the lead, before it reads, the owner-edited field is refused.
            super::set_test_caller_identities(Some((Some("conv-lead".into()), None, None)));
            let error = run("draft", &update, false).unwrap_err().to_string();
            assert!(error.contains("Owner changed members"), "{error}");
            // The owner's read leaves the marks alone.
            super::set_test_caller_identities(Some((None, None, None)));
            let owner = run("draft", &json!({"team":id,"action":"get","by":"human"}), false).unwrap();
            assert_eq!(owner["draft"]["owner_edited"], json!(["members"]), "{owner}");
            // The lead's read clears them and names them in the answer (N22).
            super::set_test_caller_identities(Some((Some("conv-lead".into()), None, None)));
            let get = run("draft", &json!({"team":id,"action":"get","by":"human"}), false).unwrap();
            assert!(get["draft"]["owner_edited"].is_null() || get["draft"]["owner_edited"] == json!([]), "{get}");
            assert_eq!(get["owner_edited"], json!(["members"]), "{get}");
            // A second read has nothing left to clear.
            let again_get = run("draft", &json!({"team":id,"action":"get","by":"human"}), false).unwrap();
            assert_eq!(again_get["owner_edited"], json!([]), "{again_get}");
            let mut again = update;
            again["request_id"] = json!("33333333-3333-4333-8333-333333333333");
            let value = run("draft", &again, false).unwrap();
            assert_eq!(value["status"], "updated", "{value}");
            let saved = Store::open(false).unwrap().load(&id).unwrap().unwrap();
            assert!(saved.member("dev2").is_some(), "{:?}", saved.members.iter().map(|member| member.id.clone()).collect::<Vec<_>>());
            assert!(saved.draft.as_ref().unwrap().owner_edited.is_empty());
        });
    }

    /// F5/N6: a draft started with a new lead launches it in the team folder and
    /// gives it the draft brief, like a desktop team's lead.
    #[test]
    fn a_started_draft_launches_and_briefs_its_new_lead() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        crate::state::with_test_state_root(dir.path(), || {
            let _worker = hold_worker_lock();
            let value = run(
                "draft",
                &json!({"action":"start","by":"human","request_id":"11111111-1111-4111-8111-111111111111",
                    "folder":project.display().to_string(),"name":"New team",
                    "lead":{"agent":"claude","model":"opus","effort":"high","account":"work"}}),
                false,
            )
            .unwrap();
            let id = value["team"].as_str().unwrap().to_owned();
            let team = Store::open(false).unwrap().load(&id).unwrap().unwrap();
            assert_eq!((team.status, team.brief), (TeamStatus::Draft, Notice::Pending));
            let lead = team.member("lead").unwrap();
            assert_eq!((lead.state, lead.session.is_some(), lead.folder), (MemberState::NotCreated, false, MemberFolder::Shared));
            // The lead starts in the team's folder, never a worktree.
            assert_eq!(super::super::dispatcher::member_folder_path(&team, lead).unwrap(), project.display().to_string());
            // The dispatcher's next tick starts it, then briefs it once.
            let steps = super::super::dispatcher::steps(&team, &[], 100.0);
            assert!(steps.contains(&crate::state::teams::dispatcher::Step::Launch { member: "lead".into() }), "{steps:?}");
            let mut launched = team.clone();
            {
                let lead = launched.members.iter_mut().find(|member| member.id == "lead").unwrap();
                lead.launch_id = "L".into();
                lead.session = Some(SessionRef { name: "claude/x/newlead".into(), run_id: "r1".into(), ..Default::default() });
            }
            let free = crate::state::teams::dispatcher::Seen {
                name: "claude/x/newlead".into(),
                run_id: "r1".into(),
                free: true,
                idle: true,
                live: true,
                ..Default::default()
            };
            assert!(super::super::dispatcher::steps(&launched, &[free], 100.0).contains(&crate::state::teams::dispatcher::Step::Brief));
            let brief = super::super::dispatcher::brief_text(&launched);
            assert!(brief.contains("hgs guide designer") && brief.contains(&id), "{brief}");
        });
    }

    // ---- #480: the phase 35 review's findings through the real entry point
    // (commands::run on a private store), as the owner, the lead and members.

    /// A running team: the lead (a dsh session, so a test can act as it),
    /// dev and qa; #1 handed to dev, #2 to qa; phase 2 is a draft.
    fn review_team() -> (Team, String, String) {
        use crate::state::teams::model::{Attempt, AttemptState, TaskStatus};
        let (mut team, _) = ops::create(
            "rv",
            &json!({"name":"Review","lead":{"id":"lead","agent":"dsh",
                "session":{"name":"dsh/x/lead","run_id":"r0","conversation_id":"lead-conversation"}}}),
        )
        .unwrap();
        let mut events = Vec::new();
        ops::apply(
            &mut team,
            "plan",
            &json!({"members":[{"id":"dev","role":"Backend","agent":"codex"},{"id":"qa","role":"Tester","agent":"kimi"}],
                "phases":[{"n":2,"title":"Polish"}],
                "tasks":[{"id":1,"title":"API","assignee":"dev"},{"id":2,"title":"Tests","assignee":"qa"},
                    {"id":3,"title":"Docs","assignee":"dev","phase":2}]}),
            &Actor::Human,
            &BTreeSet::new(),
            &mut events,
        )
        .unwrap();
        ops::apply(&mut team, "approve", &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
        let mut attempts = Vec::new();
        for id in [1u32, 2] {
            let task = team.task_mut(id).unwrap();
            let attempt = format!("00000000-0000-4000-8000-00000000010{id}");
            task.attempts.push(Attempt { n: 1, id: attempt.clone(), member: task.assignee.clone(), state: AttemptState::Delivered, ..Default::default() });
            task.status = TaskStatus::InProgress;
            attempts.push(attempt);
        }
        (team, attempts[0].clone(), attempts[1].clone())
    }

    fn with_review_team<T>(team: &Team, body: impl FnOnce() -> T) -> T {
        let dir = tempfile::tempdir().unwrap();
        crate::state::with_test_state_root(dir.path(), || {
            Store::open(true).unwrap().create(&uuid::Uuid::new_v4().to_string(), &json!({}), team.clone(), vec![]).unwrap();
            body()
        })
    }

    /// One command, with a fresh request id, as the owner unless the input
    /// names an attempt (a member) or the lead acts.
    fn call(op: &str, mut input: Value) -> anyhow::Result<Value> {
        input["request_id"] = json!(uuid::Uuid::new_v4().to_string());
        input["team"] = json!("rv");
        run(op, &input, false)
    }

    fn as_lead<T>(body: impl FnOnce() -> T) -> T {
        set_test_caller_identities(Some((Some("lead-conversation".into()), None, None)));
        let result = body();
        set_test_caller_identities(Some((None, None, None)));
        result
    }

    fn stored() -> Team {
        Store::open(false).unwrap().load("rv").unwrap().unwrap()
    }

    fn finding(severity: &str, title: &str) -> Value {
        json!({"severity":severity,"title":title,"scenario":"two saves at once","task":1})
    }

    /// Owner 2026-10-31: every task carries its estimate in status and in its
    /// timeline, and a running one the time spent against it.
    #[test]
    fn status_and_spans_carry_each_tasks_estimate() {
        let (mut team, _, _) = review_team();
        if let Some(task) = team.task_mut(1) {
            task.estimate_minutes = Some(40.0);
            task.estimate_source = "lead".into();
        }
        if let Some(task) = team.task_mut(2) {
            task.status = crate::state::teams::model::TaskStatus::InProgress;
            task.attempts = vec![crate::state::teams::model::Attempt {
                n: 1,
                member: "qa".into(),
                sent_at: crate::state::now() - 3600.0,
                delivered_at: Some(crate::state::now() - 3600.0),
                ..Default::default()
            }];
        }
        with_review_team(&team, || {
            let status = call("status", json!({})).unwrap();
            let one = status["tasks"].as_array().unwrap().iter().find(|task| task["id"] == 1).unwrap();
            assert_eq!(one["estimate_minutes"], json!(40.0));
            assert_eq!(one["estimate_source"], json!("lead"));
            assert_eq!(one["estimate_text"], json!("40 min"));
            let running = status["tasks"].as_array().unwrap().iter().find(|task| task["id"] == 2).unwrap();
            assert!(running["estimate_spent_minutes"].as_f64().unwrap() >= 59.0, "{running}");
            assert!(running["estimate_minutes"].is_null(), "no history yet: {running}");
            assert_eq!(running["estimate_text"], json!("no estimate yet"));
            let spans = call("spans", json!({"task":1})).unwrap();
            assert_eq!(spans["tasks"][0]["estimate_minutes"], json!(40.0));
            assert_eq!(spans["tasks"][0]["estimate_text"], json!("40 min"));
        });
    }

    /// U36 through `hgs team`: the implementer's report enters review, status
    /// shows the stage, its candidates and its span, the reviewer's verdict is
    /// proven by its stage id alone, and the owner's overrides are mutations.
    /// U38: `spans` gives one task's timeline, or a phase's or the plan's in
    /// pages by task id, with one bounded read of the journal.
    #[test]
    fn spans_give_a_task_or_a_page_of_timelines() {
        let (team, _, _) = review_team();
        assert!(USAGE.contains("|spans|") && !MUTATIONS.contains(&"spans"));
        with_review_team(&team, || {
            let one = run("spans", &json!({"team":"rv","task":1}), false).unwrap();
            assert_eq!(one["tasks"].as_array().unwrap().len(), 1, "{one}");
            assert_eq!(one["tasks"][0]["task"], 1);
            assert!(one["tasks"][0]["spans"].is_array() && one["tasks"][0]["totals"].is_object(), "{one}");
            assert_eq!((one["events"]["truncated"].clone(), one["next_cursor"].clone()), (json!(false), Value::Null));
            let page = run("spans", &json!({"team":"rv","limit":2}), false).unwrap();
            let ids: Vec<Value> = page["tasks"].as_array().unwrap().iter().map(|task| task["task"].clone()).collect();
            assert_eq!((ids, page["next_cursor"].clone()), (vec![json!(1), json!(2)], json!(2)));
            let rest = run("spans", &json!({"team":"rv","limit":2,"after":2}), false).unwrap();
            assert_eq!((rest["tasks"][0]["task"].clone(), rest["next_cursor"].clone()), (json!(3), Value::Null));
            let phase = run("spans", &json!({"team":"rv","phase":2}), false).unwrap();
            assert_eq!(phase["tasks"].as_array().unwrap().len(), 1);
            assert!(run("spans", &json!({"team":"rv","task":99}), false).is_err());
        });
    }

    /// U38: `profile` ranks a scope's losses read only; status carries it only
    /// when asked; two phases compare side by side.
    #[test]
    fn profile_ranks_a_scope_and_status_carries_it_on_request() {
        let (team, _, _) = review_team();
        assert!(USAGE.contains("|profile|") && !MUTATIONS.contains(&"profile"));
        with_review_team(&team, || {
            let plan = run("profile", &json!({"team":"rv","tokens":false}), false).unwrap();
            assert_eq!(plan["scope"], json!({"plan":true}));
            assert!(plan["losses"]["by_time"]["items"].is_array() && plan["critical_path"]["planned"]["tasks"].is_array(), "{plan}");
            assert_eq!(plan["token_reads"]["read"], 0);
            let one = run("profile", &json!({"team":"rv","task":1,"tokens":false}), false).unwrap();
            assert_eq!(one["summary"]["tasks"], 1);
            let compared = run("profile", &json!({"team":"rv","compare":[1,2],"tokens":false}), false).unwrap();
            assert_eq!((compared["compare"]["a"]["phase"].clone(), compared["compare"]["b"]["phase"].clone()), (json!(1), json!(2)));
            assert!(run("profile", &json!({"team":"rv","compare":[1]}), false).is_err());
            let status = run("status", &json!({"team":"rv"}), false).unwrap();
            assert!(status.get("profile").is_none());
            let status = run("status", &json!({"team":"rv","profile":{"phase":2,"tokens":false}}), false).unwrap();
            assert_eq!(status["profile"]["scope"], json!({"phase":2}));
        });
    }

    #[test]
    fn a_pipeline_runs_through_hgs_team() {
        use crate::state::teams::model::{Member, MemberState, TaskStatus};
        let (mut team, attempt, _) = review_team();
        team.members.push(Member { id: "rev".into(), role: "Reviewer".into(), agent: "claude".into(), state: MemberState::Active, ..Default::default() });
        team.task_mut(1).unwrap().pipeline_run = super::super::pipeline::resolve(
            &super::super::catalog::builtin(),
            &team,
            &crate::state::teams::model::Task { pipeline: "code".into(), ..team.task(1).unwrap().clone() },
        );
        with_review_team(&team, || {
            for op in ["skip-stage", "send-back"] {
                assert!(MUTATIONS.contains(&op) && USAGE.contains(op), "{op}");
            }
            let value = call("report", json!({"task":1,"attempt":attempt,"text":"Done"})).unwrap();
            assert_eq!(value["status"], "in_review", "{value}");
            let status = run("status", &json!({"team":"rv"}), false).unwrap();
            let task = status["tasks"].as_array().unwrap().iter().find(|task| task["id"] == 1).unwrap().clone();
            assert_eq!(task["status"], "in_review");
            assert_eq!(task["stage_candidates"], json!(["rev"]), "never the implementer dev");
            assert_eq!(task["waits"]["for"], "stage", "{task}");
            assert_eq!(task["stage_spans"][0]["kind"], "review");
            // The polled team list (without `list`, which may start the worker).
            let list = list_items(&[stored()], true);
            let summary = list["items"][0].clone();
            assert_eq!(summary["in_review"], 1, "{summary}");
            // Stage delivery (#532) gives the review to rev.
            let stage = Store::open(true)
                .unwrap()
                .mutate(None, "rv", None, |team, _| Ok(json!(super::super::pipeline::give_stage(team, 1, "rev")?)))
                .unwrap();
            let stage = stage.as_str().unwrap().to_owned();
            let value = call("report", json!({"task":1,"attempt":stage,"verdict":"pass","text":"Reads well"})).unwrap();
            assert_eq!(value["status"], "in_test", "{value}");
            // The owner skips the test, then sends the task back with a reason.
            let value = call("skip-stage", json!({"task":1,"reason":"No suites changed","by":"human"})).unwrap();
            assert_eq!(value["status"], "submitted", "{value}");
            let value = call("send-back", json!({"task":1,"reason":"Rename the endpoint","by":"human"})).unwrap();
            assert_eq!(value["status"], "pending", "{value}");
            let saved = stored();
            assert_eq!(saved.task(1).unwrap().status, TaskStatus::Pending);
            let events = run("events", &json!({"team":"rv"}), false).unwrap();
            let kinds: Vec<String> = events["items"].as_array().unwrap().iter().map(|event| event["kind"].as_str().unwrap_or_default().to_owned()).collect();
            for kind in ["stage_started", "stage_passed", "stage_skipped", "sent_back"] {
                assert!(kinds.iter().any(|found| found == kind), "{kind}: {kinds:?}");
            }
        });
    }

    /// U19 (2): the lead answers a member's subagent request with hgs team
    /// answer; the answer goes to that member's session for that exact card
    /// (its tool use), as `hgs answer` with the card's id and hash.
    #[test]
    fn the_leads_answer_reaches_the_members_subagent_card() {
        use crate::state::teams::model::SessionRef;
        let (mut team, _, _) = review_team();
        team.members.iter_mut().find(|member| member.id == "dev").unwrap().session =
            Some(SessionRef { name: "claude/x/dev".into(), run_id: "r1".into(), conversation_id: "c".into(), ..Default::default() });
        let card = super::super::dispatcher::subagent_card();
        with_review_team(&team, || {
            let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::<(Vec<String>, Value)>::new()));
            let record = calls.clone();
            let inspected = json!({"pending_questions":[card.clone()]}).to_string();
            super::super::dispatcher::set_fake_hgs(Some(Box::new(move |_, args: &[&str], input: Option<&Value>| {
                record.borrow_mut().push((args.iter().map(|arg| arg.to_string()).collect(), input.cloned().unwrap_or_default()));
                Ok(match args.first().copied() {
                    Some("inspect") => super::super::dispatcher::fake_output(0, &inspected, ""),
                    _ => super::super::dispatcher::fake_output(0, r#"{"status":"answered"}"#, ""),
                })
            })));
            let result = as_lead(|| {
                call(
                    "answer",
                    json!({"member":"dev","question_id":card["question_id"],"expected_question_hash":card["question_hash"],
                        "answers":[{"question_id":"approval","selected_option_ids":["1"]}]}),
                )
            });
            super::super::dispatcher::set_fake_hgs(None);
            result.unwrap();
            let calls = calls.borrow();
            let answer = calls.iter().find(|(args, _)| args.first().map(String::as_str) == Some("answer")).expect("hgs answer was called");
            assert_eq!(answer.0, vec!["answer".to_owned(), "claude/x/dev".to_owned(), "--json".to_owned()]);
            assert_eq!(answer.1["question_id"], card["question_id"]);
            assert_eq!(answer.1["expected_question_hash"], card["question_hash"]);
            assert_eq!(answer.1["answers"][0]["selected_option_ids"], json!(["1"]));
            assert_eq!(answer.1["expected_run_id"], "r1");
        });
    }

    /// N20: the lead's own permission card is the owner's to answer. The lead
    /// answering it would decide the whole subagent group it was asked to
    /// confirm, so `hgs team answer` refuses it before reading the card.
    #[test]
    fn the_lead_never_answers_its_own_permission_card() {
        use crate::state::teams::model::SessionRef;
        let (mut team, _, _) = review_team();
        team.members.iter_mut().find(|member| member.id == "dev").unwrap().session =
            Some(SessionRef { name: "claude/x/dev".into(), run_id: "r1".into(), conversation_id: "c".into(), ..Default::default() });
        with_review_team(&team, || {
            let error = as_lead(|| {
                call("answer", json!({"member":"lead","question_id":"q1","expected_question_hash":"h1","answers":[]}))
            })
            .unwrap_err()
            .to_string();
            assert!(error.contains("owner"), "{error}");
            // A member's card is still the lead's to answer.
            super::super::dispatcher::set_fake_hgs(Some(Box::new(|_, args: &[&str], _input: Option<&Value>| {
                Ok(match args.first().copied() {
                    Some("inspect") => super::super::dispatcher::fake_output(0, r#"{"pending_questions":[]}"#, ""),
                    _ => super::super::dispatcher::fake_output(0, r#"{"status":"answered"}"#, ""),
                })
            })));
            let error = as_lead(|| {
                call("answer", json!({"member":"dev","question_id":"q1","expected_question_hash":"h1","answers":[]}))
            })
            .unwrap_err()
            .to_string();
            super::super::dispatcher::set_fake_hgs(None);
            assert!(error.contains("no longer pending"), "{error}");
        });
    }

    /// U31: the lead's inbox names a report made ahead of its draft phase as
    /// "worked ahead of phase N", through `hgs team inbox`.
    #[test]
    fn a_worked_ahead_report_says_so_in_the_leads_inbox() {
        use crate::state::teams::model::{Attempt, AttemptState, Report, TaskStatus};
        let (mut team, _, _) = review_team();
        let task = team.task_mut(3).unwrap();
        task.status = TaskStatus::Submitted;
        task.attempts.push(Attempt {
            n: 1,
            id: "00000000-0000-4000-8000-000000000301".into(),
            member: "dev".into(),
            state: AttemptState::Reported,
            worked_ahead: true,
            report: Some(Report { text: "Docs written".into(), at: 1.0, ..Default::default() }),
            ..Default::default()
        });
        with_review_team(&team, || {
            let inbox = run("inbox", &json!({"team":"rv"}), false).unwrap();
            let items = inbox["items"].as_array().cloned().unwrap_or_default();
            let item = items.iter().find(|item| item["task"] == 3).cloned().unwrap_or_else(|| panic!("{inbox}"));
            assert_eq!(item["summary"], "worked ahead of phase 2: Docs written", "{item}");
            // An ordinary report keeps its text alone.
            let mut team = stored();
            team.task_mut(3).unwrap().attempts[0].worked_ahead = false;
            let ordinary = super::inbox_view_for(&team, &team.lead);
            assert!(!ordinary.to_string().contains("worked ahead"), "{ordinary}");
        });
    }

    #[test]
    fn a_bad_finding_is_refused_with_the_report_not_at_accept() {
        // N13: "high" is no severity: the member hears it at once, and the
        // corrected report is accepted.
        let (team, attempt, _) = review_team();
        with_review_team(&team, || {
            let error = call("report", json!({"task":1,"attempt":attempt,"text":"Done",
                "report":{"summary":"Done","findings":[finding("high","Lost write")]}}))
            .unwrap_err()
            .to_string();
            assert!(error.contains("A finding is critical, normal or later"), "{error}");
            assert_eq!(stored().task(1).unwrap().attempts[0].report, None);
            let error = call("report", json!({"task":1,"attempt":attempt,"text":"Done",
                "report":{"summary":"Done","findings":[{"severity":"normal","title":"No scenario"}]}}))
            .unwrap_err()
            .to_string();
            assert!(error.contains("A finding needs its scenario"), "{error}");
            call("report", json!({"task":1,"attempt":attempt,"text":"Done",
                "report":{"summary":"Done","findings":[finding("critical","Lost write")]}}))
            .unwrap();
            call("accept", json!({"task":1,"by":"human"})).unwrap();
            let team = stored();
            assert_eq!(team.findings.len(), 1);
            assert_eq!(team.findings[0].severity, "critical");
        });
    }

    #[test]
    fn fixed_and_dropped_findings_leave_room_under_the_cap() {
        // N14: 1000 closed findings no longer block the next accept; the
        // oldest closed ones make room and the store stays at 1000.
        use crate::state::teams::model::Finding;
        let (mut team, attempt, _) = review_team();
        for n in 1..=1000 {
            team.findings.push(Finding { id: format!("f{n}"), severity: "normal".into(), title: format!("F{n}"), scenario: "s".into(),
                state: if n % 2 == 0 { "fixed".into() } else { "dropped".into() }, ..Default::default() });
        }
        team.findings_seq = 1000;
        with_review_team(&team, || {
            call("report", json!({"task":1,"attempt":attempt,"text":"Done",
                "report":{"summary":"Done","findings":[finding("normal","New"),finding("later","Later")]}}))
            .unwrap();
            call("accept", json!({"task":1,"by":"human"})).unwrap();
            let team = stored();
            assert_eq!(team.findings.len(), 1000);
            assert_eq!(team.findings.last().unwrap().id, "f1002");
            assert!(!team.findings.iter().any(|finding| finding.id == "f1" || finding.id == "f2"));
        });
        // Live findings still count: 1000 open ones leave no room.
        let (mut team, attempt, _) = review_team();
        for n in 1..=1000 {
            team.findings.push(Finding { id: format!("f{n}"), severity: "normal".into(), title: format!("F{n}"), scenario: "s".into(),
                state: "open".into(), ..Default::default() });
        }
        with_review_team(&team, || {
            call("report", json!({"task":1,"attempt":attempt,"text":"Done","report":{"summary":"Done","findings":[finding("normal","New")]}}))
                .unwrap();
            let error = call("accept", json!({"task":1,"by":"human"})).unwrap_err().to_string();
            assert!(error.contains("at most 1000 findings that are not fixed or dropped"), "{error}");
        });
    }

    #[test]
    fn the_owners_accept_after_the_leads_files_the_findings_once() {
        // N15
        let (team, attempt, _) = review_team();
        with_review_team(&team, || {
            call("report", json!({"task":1,"attempt":attempt,"text":"Done",
                "report":{"summary":"Done","findings":[finding("normal","Slow"),finding("critical","Lost write")]}}))
            .unwrap();
            as_lead(|| call("accept", json!({"task":1}))).unwrap();
            assert_eq!(stored().findings.len(), 2);
            call("accept", json!({"task":1,"by":"human"})).unwrap();
            let team = stored();
            assert_eq!(team.findings.len(), 2, "{:?}", team.findings);
            assert_eq!(team.task(1).unwrap().decided_by, Some(crate::state::teams::model::Decider::Human));
        });
    }

    #[test]
    fn a_replan_keeps_its_proposal_and_fix_phase_its_phase() {
        // N17, N18
        let (team, attempt, _) = review_team();
        with_review_team(&team, || {
            call("report", json!({"task":1,"attempt":attempt,"text":"Done",
                "report":{"summary":"Done","findings":[finding("critical","Lost write"),finding("normal","Slow")]}}))
            .unwrap();
            call("accept", json!({"task":1,"by":"human"})).unwrap();
            // A critical finding is not parked in a draft phase.
            let error = call("finding", json!({"finding":"f1","action":"fix_phase","phase":2,"by":"human"})).unwrap_err().to_string();
            assert!(error.contains("Only a normal finding"), "{error}");
            call("finding", json!({"finding":"f2","action":"fix_phase","phase":2,"by":"human"})).unwrap();
            call("finding", json!({"finding":"f1","action":"replan","proposal":"Split the save into two steps","by":"human"})).unwrap();
            let team = stored();
            assert_eq!(team.findings[1].phase, Some(2));
            assert_eq!(team.findings[0].proposal, "Split the save into two steps");
            // Status shows both, and a replanned critical finding is still open.
            let status = run("status", &json!({"team":"rv"}), false).unwrap();
            let items = status["findings"]["items"].as_array().unwrap();
            let f1 = items.iter().find(|item| item["id"] == "f1").unwrap();
            assert_eq!(f1["proposal"], "Split the save into two steps");
            assert_eq!(items.iter().find(|item| item["id"] == "f2").unwrap()["phase"], 2);
            assert_eq!(status["findings_summary"]["open"]["critical"], 1);
            assert_eq!(status["findings_summary"]["open"]["normal"], 1);
            // The team waits paused, and the owner's attention names the proposal.
            assert_eq!(status["status"], "paused");
            let replan = status["attention"].as_array().unwrap().iter().find(|item| item["kind"] == "replan").cloned();
            assert_eq!(replan.as_ref().map(|item| item["proposal"].clone()), Some(json!("Split the save into two steps")), "{}", status["attention"]);
        });
    }

    #[test]
    fn a_finding_in_a_replan_or_a_draft_phase_still_blocks_the_finish() {
        // N18: replan and fix_phase are not resolved.
        use crate::state::teams::model::{Finding, TaskStatus};
        for state in ["replan", "fix_phase"] {
            let (mut team, _, _) = review_team();
            for task in &mut team.tasks {
                task.status = if task.phase == 1 { TaskStatus::Accepted } else { TaskStatus::Cancelled };
            }
            team.findings.push(Finding { id: "f1".into(), severity: "critical".into(), title: "Lost write".into(), scenario: "s".into(),
                state: state.into(), ..Default::default() });
            with_review_team(&team, || {
                let error = call("finish", json!({"summary":"Done","by":"human"})).unwrap_err().to_string();
                assert!(error.contains("A critical finding is open"), "{state}: {error}");
            });
        }
    }

    #[test]
    fn evidence_folders_are_a_setting_status_carries() {
        // N19
        let (team, _, _) = review_team();
        with_review_team(&team, || {
            call("settings", json!({"settings":{"evidence_folders":["shots","D:/work/team-phases/shots-v2"]},"by":"human"})).unwrap();
            assert_eq!(stored().settings.evidence_folders, vec!["shots".to_owned(), "D:/work/team-phases/shots-v2".to_owned()]);
            let status = run("status", &json!({"team":"rv"}), false).unwrap();
            assert_eq!(status["settings"]["evidence_folders"][1], "D:/work/team-phases/shots-v2");
            let error = call("settings", json!({"settings":{"evidence_folders":["  "]},"by":"human"})).unwrap_err().to_string();
            assert!(error.contains("An evidence folder"), "{error}");
        });
    }

    #[test]
    fn a_finished_team_takes_no_reopen() {
        // N20: reopen, urgent, replan and fix_phase need a running team;
        // closing a finding still works.
        use crate::state::teams::model::{Finding, TaskStatus, TeamStatus};
        let (mut team, _, _) = review_team();
        for task in &mut team.tasks {
            task.status = TaskStatus::Accepted;
        }
        team.status = TeamStatus::Done;
        team.findings.push(Finding { id: "f1".into(), severity: "normal".into(), title: "Slow".into(), scenario: "s".into(), task: Some(1),
            state: "open".into(), ..Default::default() });
        with_review_team(&team, || {
            for action in [json!({"action":"reopen"}), json!({"action":"replan","proposal":"x"}), json!({"action":"fix_phase","phase":2})] {
                let mut input = action.clone();
                input["finding"] = json!("f1");
                input["by"] = json!("human");
                let error = call("finding", input).unwrap_err().to_string();
                assert!(error.contains("need a running or paused team"), "{action}: {error}");
            }
            assert_eq!(stored().task(1).unwrap().status, TaskStatus::Accepted);
            call("finding", json!({"finding":"f1","action":"fixed","by":"human"})).unwrap();
            assert_eq!(stored().findings[0].state, "fixed");
        });
    }

    #[test]
    fn a_removed_or_paused_member_is_not_swapped() {
        // N31
        use crate::state::teams::model::MemberState;
        let (mut team, _, _) = review_team();
        team.members.iter_mut().find(|member| member.id == "qa").unwrap().state = MemberState::Removed;
        team.members.iter_mut().find(|member| member.id == "dev").unwrap().state = MemberState::Paused;
        with_review_team(&team, || {
            let error = call("swap-member", json!({"member":"qa","agent":"claude","by":"human"})).unwrap_err().to_string();
            assert!(error.contains("This member was removed"), "{error}");
            let error = call("swap-member", json!({"member":"dev","agent":"claude","by":"human"})).unwrap_err().to_string();
            assert!(error.contains("Resume dev before swapping it"), "{error}");
            let team = stored();
            assert_eq!(team.member("qa").unwrap().state, MemberState::Removed);
            assert!(!team.member("qa").unwrap().launch_after_swap);
            assert_eq!(team.member("dev").unwrap().state, MemberState::Paused);
        });
    }

    #[test]
    fn a_swap_to_an_existing_session_takes_the_listings_folder_not_the_requests() {
        // N32: the adopted session's own folder comes from the live listing; the
        // request may not name a different folder or folder kind for it.
        use crate::state::teams::dispatcher::Seen;
        use crate::state::teams::model::MemberFolder;
        let (team, _, _) = review_team();
        with_review_team(&team, || {
            super::live_sessions_override(Some(vec![Seen {
                name: "codex/x/other".into(),
                run_id: "r9".into(),
                conversation_id: "c9".into(),
                live: true,
                cwd: "/work/other-checkout".into(),
                ..Default::default()
            }]));
            let value = call(
                "swap-member",
                json!({"member":"dev","session":{"name":"codex/x/other","run_id":"r9"},
                    "directory":"/work/forged","folder":"worktree","by":"human"}),
            )
            .unwrap();
            assert_eq!(value["status"], "swapped", "{value}");
            let saved = stored();
            let dev = saved.member("dev").unwrap();
            assert_eq!(dev.directory, "/work/other-checkout", "the listing's folder, never the request's");
            assert_eq!(dev.folder, MemberFolder::Shared, "the adopted session's own kind");
        });
        super::live_sessions_override(None);
    }

    #[test]
    fn a_swap_to_an_existing_session_takes_its_folder() {
        // N32: the adopted session's folder replaces the member's worktree.
        use crate::state::teams::model::MemberFolder;
        let (mut team, _, _) = review_team();
        let dev = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
        dev.folder = MemberFolder::Worktree;
        dev.directory = "/work/repo-team-rv-dev".into();
        let mut events = Vec::new();
        ops::apply(
            &mut team,
            "swap-member",
            &json!({"member":"dev","session":{"name":"codex/x/other","run_id":"r9","conversation_id":"c9"},
                "directory":"/work/other-checkout","folder":"shared"}),
            &Actor::Human,
            &BTreeSet::new(),
            &mut events,
        )
        .unwrap();
        let dev = team.member("dev").unwrap();
        assert_eq!(dev.directory, "/work/other-checkout");
        assert_eq!(dev.folder, MemberFolder::Shared);
        assert_eq!(dev.session.as_ref().unwrap().name, "codex/x/other");
    }

    /// #432 N9: the swap and the owner's word together, through the command
    /// entry: the lead swaps on the exact sentence naming the member by its
    /// role, and that sentence pays for one swap only.
    #[test]
    fn a_lead_swaps_a_member_on_the_owners_word() {
        let dir = tempfile::tempdir().unwrap();
        let now = crate::state::now();
        let said = "замени бэкендера на кими";
        owner_word_events(dir.path(), &[(now - 60.0, said)]);
        crate::state::with_test_state_root(dir.path(), || {
            set_test_caller_identities(Some((Some("c1".into()), None, None)));
            let mut store = Store::open(true).unwrap();
            let team = full_team();
            let id = team.id.clone();
            store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
            let swap = |request: &str, member: &str, agent: &str| {
                run(
                    "swap-member",
                    &json!({"request_id":request,"team":id,"member":member,"agent":agent,"owner_word":{"text":said}}),
                    false,
                )
            };
            // Without the owner's sentence the lead does not swap.
            let error = run(
                "swap-member",
                &json!({"request_id":"22222222-2222-4222-8222-222222222222","team":id,"member":"m1","agent":"kimi"}),
                false,
            )
            .unwrap_err()
            .to_string();
            assert!(error.contains("your word"), "{error}");
            let swapped = swap("33333333-3333-4333-8333-333333333333", "m1", "kimi").unwrap();
            assert_eq!((swapped["status"].as_str(), swapped["agent"].as_str()), (Some("swapped"), Some("kimi")));
            // The same sentence pays for one swap only.
            let error = swap("44444444-4444-4444-8444-444444444444", "m2", "kimi").unwrap_err().to_string();
            assert!(error.contains("already used"), "{error}");
            assert!(Store::open(false).unwrap().load(&id).unwrap().unwrap().member("m2").unwrap().agent == "dsh");
        });
    }

    #[test]
    fn a_draft_behind_an_approved_running_phase_does_not_wait() {
        // N40 (spec 2.6): phase 1 still runs, phase 2 can be approved now.
        let (team, _, _) = review_team();
        with_review_team(&team, || {
            let status = run("status", &json!({"team":"rv"}), false).unwrap();
            let phase = status["phases"].as_array().unwrap().iter().find(|phase| phase["n"] == 2).unwrap().clone();
            // #515: it says it can run alongside, not that it waits.
            assert_eq!(phase["waiting"]["alongside"], true, "{phase}");
            assert_eq!(phase["waiting"]["text"], "Can run alongside phase 1", "{phase}");
            call("approve-phase", json!({"phase":2,"by":"human"})).unwrap();
        });
    }

    #[test]
    fn a_confirmed_alongside_approval_hands_the_phase_out_now() {
        // N6: "Can run alongside phase 1" was false - approving phase 2 put it
        // in Queued and every task waited for phase 1. The owner's confirmed
        // approval starts it; without the confirmation it still queues (U25).
        let (team, _, _) = review_team();
        with_review_team(&team, || {
            let value = call("approve-phase", json!({"phase":2,"by":"human"})).unwrap();
            assert_eq!(value["phase_state"], "queued", "{value}");
            assert!(!crate::state::teams::plan::ready(&stored()).contains(&3), "a queued phase hands nothing out");
        });
        let (team, _, _) = review_team();
        with_review_team(&team, || {
            let value = call("approve-phase", json!({"phase":2,"alongside":true,"by":"human"})).unwrap();
            assert_eq!(
                (value["phase_state"].as_str(), value["alongside"].as_bool()),
                (Some("running"), Some(true)),
                "{value}"
            );
            let saved = stored();
            assert_eq!(saved.phases.iter().find(|phase| phase.n == 2).unwrap().state, PhaseState::Approved);
            assert!(crate::state::teams::plan::ready(&saved).contains(&3), "the phase really runs alongside phase 1");
        });
        // A barrier phase keeps its tasks for the end of phase 1 either way, so
        // its waiting text names that, not an alongside that cannot happen.
        let (mut team, _, _) = review_team();
        team.phases.iter_mut().find(|phase| phase.n == 2).unwrap().barrier = true;
        with_review_team(&team, || {
            let status = run("status", &json!({"team":"rv"}), false).unwrap();
            let phase = status["phases"].as_array().unwrap().iter().find(|phase| phase["n"] == 2).unwrap().clone();
            assert_eq!(phase["waiting"]["text"], "Starts when phase 1 is finished", "{phase}");
            call("approve-phase", json!({"phase":2,"alongside":true,"by":"human"})).unwrap();
            assert!(!crate::state::teams::plan::ready(&stored()).contains(&3), "a barrier holds the phase's tasks");
        });
    }

    #[test]
    fn a_planning_team_has_no_phase_in_work() {
        // N41 (spec 10.2): before Approve & Run every phase waits for you.
        let (mut team, _) = ops::create("rv", &json!({"name":"Review","lead":{"id":"lead","agent":"claude"}})).unwrap();
        ops::apply(&mut team, "plan", &json!({"tasks":[{"id":1,"title":"A","assignee":"lead"}]}), &Actor::Human, &BTreeSet::new(), &mut Vec::new())
            .unwrap();
        with_review_team(&team, || {
            let status = run("status", &json!({"team":"rv"}), false).unwrap();
            let first = status["phase_summary"].as_array().unwrap().iter().find(|phase| phase["n"] == 1).unwrap().clone();
            assert_eq!(first["status"], "draft", "{first}");
            assert_eq!(first["state"], "planned");
        });
    }

    #[test]
    fn findings_pages_are_stable_with_a_before_cursor() {
        use crate::state::teams::model::{Finding, Team};
        let mut team = Team { ..Default::default() };
        for n in 1..=120 {
            team.findings.push(Finding { id: format!("f{n}"), severity: "normal".into(), title: format!("F{n}"), scenario: "s".into(), state: "open".into(), ..Default::default() });
        }
        let first = super::findings_page(&team, None);
        assert_eq!(first["items"].as_array().unwrap().len(), 50);
        assert_eq!(first["items"][0]["id"], "f120");
        assert_eq!(first["next_cursor"], 71);
        let second = super::findings_page(&team, Some(71));
        assert_eq!(second["items"].as_array().unwrap().len(), 50);
        assert_eq!(second["items"][0]["id"], "f70");
        assert_eq!(second["next_cursor"], 21);
        let third = super::findings_page(&team, Some(21));
        assert_eq!(third["items"].as_array().unwrap().len(), 20);
        assert_eq!(third["next_cursor"], serde_json::Value::Null);
        // A new finding does not shift the second page.
        team.findings.push(Finding { id: "f121".into(), severity: "normal".into(), title: "New".into(), scenario: "s".into(), state: "open".into(), ..Default::default() });
        assert_eq!(super::findings_page(&team, Some(71))["items"][0]["id"], "f70");
    }

    #[test]
    fn a_draft_starts_with_its_lead_and_nothing_before_it() {
        use crate::state::teams::model::TeamStatus;
        use crate::state::teams::store::Store;
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open_at(&dir.path().join("teams.db"), true).unwrap();
        let request = "11111111-1111-4111-8111-111111111111";
        let input = json!({"request_id":request,"folder":dir.path().display().to_string(),"name":"Checkout","lead":{"agent":"claude"}});
        let value = super::draft_start_in(&mut store, request, &input, input["lead"].clone()).unwrap();
        let id = value["team"].as_str().unwrap().to_owned();
        assert_eq!(value["revision"], 1);
        let team = store.load(&id).unwrap().unwrap();
        assert_eq!(team.status, TeamStatus::Draft);
        assert_eq!(team.draft.as_ref().unwrap().revision, 1);
        assert_eq!(team.member("lead").map(|member| member.role.as_str()), Some("Lead"));
        assert_eq!(team.members.len(), 1);
        // A repeated request returns the first result and creates nothing twice.
        super::draft_start_in(&mut store, request, &input, input["lead"].clone()).unwrap();
        assert_eq!(store.teams(true, 10).unwrap().len(), 1);
    }

    #[test]
    fn draft_get_returns_the_revision_and_preflight() {
        use crate::state::teams::store::Store;
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open_at(&dir.path().join("teams.db"), true).unwrap();
        let request = "22222222-2222-4222-8222-222222222222";
        let input = json!({"request_id":request,"folder":dir.path().display().to_string(),"lead":{"agent":"claude"}});
        let value = super::draft_start_in(&mut store, request, &input, input["lead"].clone()).unwrap();
        let id = value["team"].as_str().unwrap().to_owned();
        let get = super::draft_get_in(&mut store, &json!({"team":id})).unwrap();
        assert_eq!(get["revision"], 1);
        assert_eq!(get["draft"]["revision"], 1);
        assert!(get["preflight"]["errors"].is_array());
    }

    #[test]
    fn a_done_team_starts_the_next_goal_with_the_same_sessions() {
        use crate::state::teams::model::{Member, SessionRef, Team, TeamStatus};
        use crate::state::teams::store::Store;
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open_at(&dir.path().join("teams.db"), true).unwrap();
        let old_id = "33333333-3333-4333-8333-333333333333";
        let mut done = Team {
            id: old_id.into(),
            name: "T".into(),
            lead: "lead".into(),
            status: TeamStatus::Done,
            folder: dir.path().display().to_string(),
            goal: "one".into(),
            ..Default::default()
        };
        done.members.push(Member { id: "lead".into(), role: "Lead".into(), agent: "claude".into(), ..Default::default() });
        done.members.push(Member {
            id: "dev".into(),
            role: "Backend".into(),
            agent: "claude".into(),
            session: Some(SessionRef { name: "claude/x/dev".into(), ..Default::default() }),
            ..Default::default()
        });
        store.create("44444444-4444-4444-8444-444444444444", &json!({"id":old_id}), done, Vec::new()).unwrap();
        // N8: the live sessions are read once; this test's listing has dev's.
        super::live_sessions_override(Some(vec![crate::state::teams::dispatcher::Seen {
            name: "claude/x/dev".into(),
            run_id: "r9".into(),
            conversation_id: "c9".into(),
            live: true,
            ..Default::default()
        }]));
        let value = super::next_goal_as(
            &mut store,
            "55555555-5555-4555-8555-555555555555",
            &json!({"request_id":"55555555-5555-4555-8555-555555555555","team":old_id,"goal":"two"}),
            &Actor::Human,
        )
        .unwrap();
        super::live_sessions_override(None);
        let next_id = value["team"].as_str().unwrap().to_owned();
        let next = store.load(&next_id).unwrap().unwrap();
        assert_eq!(next.status, TeamStatus::Planning);
        assert_eq!((next.previous.as_str(), next.goal.as_str()), (old_id, "two"));
        let dev = next.member("dev").unwrap();
        assert_eq!(dev.session.as_ref().map(|session| (session.name.as_str(), session.run_id.as_str())), Some(("claude/x/dev", "r9")));
        assert_eq!(dev.state, crate::state::teams::model::MemberState::Active);
        assert_eq!(store.load(old_id).unwrap().unwrap().next, next_id);
    }

    #[test]
    fn every_roster_change_needs_the_owner_word_naming_it() {
        use rusqlite::Connection;
        let now = crate::state::now();
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE events (seq INTEGER PRIMARY KEY, name TEXT, conversation TEXT, payload TEXT);").unwrap();
        let add = |text: &str| {
            db.execute(
                "INSERT INTO events(name, conversation, payload) VALUES ('claude/x/lead','c1',?1)",
                rusqlite::params![json!({"type":"UserPromptSubmit","at":now - 60.0,"agent_id":"","detail":text}).to_string()],
            ).unwrap();
        };
        add("Please add a backend to the roster");
        add("Please add a frontend to the roster");
        add("Please swap dev for another member");
        add("Please remove dev from the roster");
        add("Please change max members to 5");
        let mut team = Team { id: "t".into(), name: "T".into(), lead: "lead".into(), status: TeamStatus::Running, ..Default::default() };
        team.members.push(Member { id: "lead".into(), role: "Lead".into(), agent: "claude".into(),
            session: Some(SessionRef { name: "claude/x/lead".into(), conversation_id: "c1".into(), ..Default::default() }), ..Default::default() });
        team.members.push(Member { id: "dev".into(), role: "Backend".into(), agent: "codex".into(), ..Default::default() });
        let caller = Actor::Member("lead".into());
        let verify = |_: &Team, word: &Value, rule: &super::super::owner_word::WordRule, now: f64| {
            super::super::owner_word::verify_roster_in(&db, "claude/x/lead", "c1", word["text"].as_str().unwrap_or_default(), rule, now)
        };
        let cases = [
            ("add-member", json!({"member":{"id":"bob","role":"Backend","agent":"codex"}}), "Please add a backend to the roster", "Please add a frontend to the roster"),
            ("swap-member", json!({"member":"dev"}), "Please swap dev for another member", "Please add a frontend to the roster"),
            ("remove-member", json!({"member":"dev"}), "Please remove dev from the roster", "Please add a frontend to the roster"),
            ("settings", json!({"settings":{"max_members":5}}), "Please change max members to 5", "Please add a frontend to the roster"),
        ];
        for (op, base, sentence, other) in cases {
            let mut without = base.clone();
            let input = base.clone();
            assert!(super::roster_word_rule(&team, op, &input).needed(), "{op}");
            let error = super::check_roster_word(&team, op, &input, &mut without, &caller, verify).unwrap_err().to_string();
            assert!(error.contains("your word"), "{op}: {error}");
            // A caller-supplied roster_word is not a word.
            let mut claimed = base.clone();
            claimed["roster_word"] = Value::Bool(true);
            let input = claimed.clone();
            let error = super::check_roster_word(&team, op, &input, &mut claimed, &caller, verify).unwrap_err().to_string();
            assert!(error.contains("your word"), "{op}: {error}");
            // A word naming something else is refused.
            let mut wrong = base.clone();
            wrong["owner_word"] = json!({"text": other});
            let input = wrong.clone();
            let error = super::check_roster_word(&team, op, &input, &mut wrong, &caller, verify).unwrap_err().to_string();
            assert!(error.contains("your word") || error.contains("not in the lead session") || error.contains("does not name"), "{op}: {error}");
            // The fresh word naming the change fills the verified flag.
            let mut good = base.clone();
            good["owner_word"] = json!({"text": sentence});
            let input = good.clone();
            super::check_roster_word(&team, op, &input, &mut good, &caller, verify).unwrap();
            assert_eq!(good["roster_word"], true, "{op}");
            assert!(good["owner_word_checked"].is_object(), "{op}");
        }
    }

    #[test]
    fn scale_and_drain_need_the_owner_word_naming_them() {
        // U49: the lead scales on the count the sentence names, and drains on
        // the word that removes the member.
        use rusqlite::Connection;
        let now = crate::state::now();
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE events (seq INTEGER PRIMARY KEY, name TEXT, conversation TEXT, payload TEXT);").unwrap();
        let say = |text: &str| {
            db.execute(
                "INSERT INTO events(name, conversation, payload) VALUES ('claude/x/lead','c1',?1)",
                rusqlite::params![json!({"type":"UserPromptSubmit","at":now - 60.0,"agent_id":"","detail":text}).to_string()],
            )
            .unwrap();
        };
        say("Please add two backend members to the roster");
        say("Please drain frontend2 from the roster");
        say("Please remove frontend2 from the roster");
        say("Please add three backend members to the roster");
        let mut team = Team { id: "t".into(), name: "T".into(), lead: "lead".into(), status: TeamStatus::Running, ..Default::default() };
        team.members.push(Member {
            id: "lead".into(),
            role: "Lead".into(),
            agent: "claude".into(),
            session: Some(SessionRef { name: "claude/x/lead".into(), conversation_id: "c1".into(), ..Default::default() }),
            ..Default::default()
        });
        let caller = Actor::Member("lead".into());
        let verify = |_: &Team, word: &Value, rule: &super::super::owner_word::WordRule, now: f64| {
            super::super::owner_word::verify_roster_in(&db, "claude/x/lead", "c1", word["text"].as_str().unwrap_or_default(), rule, now)
        };
        // scale: the sentence says to add, names the role and the count.
        let scale = json!({"role":"Backend","agent":"codex","count":2});
        assert!(super::roster_word_rule(&team, "scale", &scale).needed());
        let mut without = scale.clone();
        let error = super::check_roster_word(&team, "scale", &scale, &mut without, &caller, verify).unwrap_err().to_string();
        assert!(error.contains("your word"), "{error}");
        let mut worded = scale.clone();
        worded["owner_word"] = json!({"text":"Please add two backend members to the roster"});
        let mut filled = worded.clone();
        super::check_roster_word(&team, "scale", &worded, &mut filled, &caller, verify).unwrap();
        assert_eq!((filled["roster_word"].clone(), filled["owner_word_change"].as_str()), (json!(true), Some("scaling Backend by 2")));
        // Another count or another role is not this scale.
        for text in ["Please add three backend members to the roster", "Please add two frontend members to the roster"] {
            let mut wrong = json!({"role":"Backend","agent":"codex","count":2,"owner_word":{"text":text}});
            let input = wrong.clone();
            let error = super::check_roster_word(&team, "scale", &input, &mut wrong, &caller, verify).unwrap_err().to_string();
            assert!(error.contains("your word") || error.contains("does not name"), "{text}: {error}");
        }
        // drain: the owner's drain or remove sentence names the member.
        let drain = json!({"member":"frontend2"});
        assert!(super::roster_word_rule(&team, "drain", &drain).needed());
        let mut without = drain.clone();
        let error = super::check_roster_word(&team, "drain", &drain, &mut without, &caller, verify).unwrap_err().to_string();
        assert!(error.contains("your word"), "{error}");
        let mut worded = json!({"member":"frontend2","owner_word":{"text":"Please drain frontend2 from the roster"}});
        let input = worded.clone();
        super::check_roster_word(&team, "drain", &input, &mut worded, &caller, verify).unwrap();
        assert_eq!(worded["roster_word"], true);
        let mut removal = json!({"member":"frontend2","owner_word":{"text":"Please remove frontend2 from the roster"}});
        let input = removal.clone();
        super::check_roster_word(&team, "drain", &input, &mut removal, &caller, verify).unwrap();
        let mut other = json!({"member":"frontend2","owner_word":{"text":"Please drain backend from the roster"}});
        let input = other.clone();
        assert!(super::check_roster_word(&team, "drain", &input, &mut other, &caller, verify).is_err());
        // Cancelling a drain is no roster change.
        assert!(!super::roster_word_rule(&team, "drain", &json!({"member":"frontend2","draining":false})).needed());
    }

    /// U49 through `run`: the lead's scale and drain ask for the owner's word
    /// before anything changes.
    #[test]
    fn a_lead_scales_and_drains_only_through_the_word() {
        let _env = crate::state::ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        crate::state::with_test_state_root(dir.path(), || {
            let _worker = hold_worker_lock();
            let mut store = Store::open(true).unwrap();
            let mut team = running_team();
            {
                let lead = team.members.iter_mut().find(|member| member.id == "lead").unwrap();
                lead.agent = "dsh".into();
                lead.session = Some(SessionRef { name: "dsh/x/lead".into(), run_id: "r1".into(), conversation_id: "conv-lead".into(), ..Default::default() });
            }
            let id = team.id.clone();
            store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
            super::set_test_caller_identities(Some((Some("conv-lead".into()), None, None)));
            let mut n = 0u8;
            let mut request = |op: &str, input: Value| {
                n += 1;
                let mut input = input;
                input["team"] = json!(id);
                input["request_id"] = json!(format!("00000000-0000-4000-8000-0000000006{n:02}"));
                run(op, &input, false)
            };
            let error = request("scale", json!({"role":"Backend","agent":"codex","count":1})).unwrap_err().to_string();
            assert!(error.contains("your word"), "{error}");
            let error = request("drain", json!({"member":"dev"})).unwrap_err().to_string();
            assert!(error.contains("your word"), "{error}");
            // Cancelling a drain changes no roster: the lead may do it alone.
            let cancelled = request("drain", json!({"member":"dev","draining":false})).unwrap();
            assert_eq!(cancelled["status"], "draining_cancelled");
            super::set_test_caller_identities(None);
        });
    }

    #[test]
    fn one_owner_word_is_spent_on_one_roster_change() {
        use rusqlite::Connection;
        let now = crate::state::now();
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE events (seq INTEGER PRIMARY KEY, name TEXT, conversation TEXT, payload TEXT);").unwrap();
        for (seq, text) in [(1, "Please remove dev from the roster"), (2, "Please remove dev from the roster again")] {
            db.execute("INSERT INTO events(seq,name,conversation,payload) VALUES (?1,'claude/x/lead','c1',?2)",
                rusqlite::params![seq, json!({"type":"UserPromptSubmit","at":now - 60.0,"agent_id":"","detail":text}).to_string()]).unwrap();
        }
        let mut team = Team { id: "t".into(), name: "T".into(), lead: "lead".into(), status: TeamStatus::Running, ..Default::default() };
        team.members.push(Member { id: "lead".into(), role: "Lead".into(), agent: "claude".into(), ..Default::default() });
        let caller = Actor::Member("lead".into());
        let verify = |_: &Team, word: &Value, rule: &super::super::owner_word::WordRule, now: f64| {
            super::super::owner_word::verify_roster_in(&db, "claude/x/lead", "c1", word["text"].as_str().unwrap_or_default(), rule, now)
        };
        let base = json!({"member":"dev"});
        let mut first = base.clone();
        first["owner_word"] = json!({"text":"Please remove dev from the roster"});
        let input = first.clone();
        super::check_roster_word(&team, "remove-member", &input, &mut first, &caller, verify).unwrap();
        // The operation spends the event on the team.
        let event = first["owner_word_event"].as_i64().unwrap();
        team.record_owner_word(event, "the roster change");
        let mut second = base.clone();
        second["owner_word"] = json!({"text":"Please remove dev from the roster"});
        let input = second.clone();
        let error = super::check_roster_word(&team, "remove-member", &input, &mut second, &caller, verify).unwrap_err().to_string();
        assert!(error.contains("already used"), "{error}");
        // A new sentence works.
        let mut third = base.clone();
        third["owner_word"] = json!({"text":"Please remove dev from the roster again"});
        let input = third.clone();
        super::check_roster_word(&team, "remove-member", &input, &mut third, &caller, verify).unwrap();
    }

    /// U29 through `run`: a task joins a running phase only on the owner's
    /// word twice, with the warning between the two words.
    #[test]
    fn a_task_joins_a_running_phase_on_the_owners_word_twice() {
        let _env = crate::state::ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        crate::state::with_test_state_root(dir.path(), || {
            let _worker = hold_worker_lock();
            let mut store = Store::open(true).unwrap();
            let mut team = running_team();
            {
                let lead = team.members.iter_mut().find(|member| member.id == "lead").unwrap();
                lead.agent = "dsh".into();
                lead.session = Some(SessionRef { name: "dsh/x/lead".into(), run_id: "r1".into(), conversation_id: "conv-lead".into(), ..Default::default() });
            }
            let id = team.id.clone();
            store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
            let db = super::super::super::journal::event_db().unwrap();
            let say = |at: f64, text: &str| {
                db.execute(
                    "INSERT INTO events(name, conversation, payload) VALUES ('dsh/x/lead','conv-lead',?1)",
                    rusqlite::params![json!({"type":"UserPromptSubmit","at":at,"agent_id":"","detail":text}).to_string()],
                ).unwrap();
            };
            let mut n = 0u8;
            let mut request = |input: Value| {
                n += 1;
                let mut input = input;
                input["team"] = json!(id);
                input["request_id"] = json!(format!("00000000-0000-4000-8000-0000000005{n:02}"));
                run("add-to-phase", &input, false)
            };
            let task = json!([{"id":2,"title":"Fix the login crash","assignee":"dev"}]);
            let stored = || Store::open(false).unwrap().load("t").unwrap().unwrap();
            let lead = || super::set_test_caller_identities(Some((Some("conv-lead".into()), None, None)));
            let now = crate::state::now();
            say(now - 300.0, "Yes, add it");   // older than any warning
            say(now - 200.0, "please continue with the plan");
            say(now - 100.0, "Please add a task to the current phase: fix the login crash");
            lead();
            // 1. No word: refused, nothing changes.
            let error = request(json!({"phase":1,"tasks":task})).unwrap_err().to_string();
            assert!(error.contains("your word"), "{error}");
            // A sentence that does not ask to add a task is not the word.
            let error = request(json!({"phase":1,"tasks":task,"owner_word":{"text":"please continue with the plan"}})).unwrap_err().to_string();
            assert!(error.contains("has not asked to add a task to phase 1"), "{error}");
            // 2. The request word alone changes nothing and returns the warning.
            let warned = request(json!({"phase":1,"tasks":task,"owner_word":{"text":"Please add a task to the current phase"}})).unwrap();
            assert_eq!(warned["status"], "warning", "{warned}");
            assert!(warned["warning"].as_str().unwrap().contains("load balance of the lanes"), "{warned}");
            // #511 C1: the warning names the queued task titles, so the owner
            // confirms what joins.
            assert!(warned["warning"].as_str().unwrap().contains("\"Fix the login crash\""), "{warned}");
            let pending = warned["pending"].as_str().unwrap().to_owned();
            let team = stored();
            assert_eq!(team.tasks.len(), 1);
            assert_eq!(team.pending_additions.len(), 1);
            // 3. A confirmation older than the warning is refused.
            let confirm = |text: &str, phase: u32| json!({"phase":phase,"confirm":pending,"owner_word":{"text":text}});
            let error = request(confirm("Yes, add it", 1)).unwrap_err().to_string();
            assert!(error.contains("has not confirmed the addition to phase 1"), "{error}");
            // 4. A confirmation for another phase is refused: by its words and by its number.
            say(crate::state::now() + 0.2, "Yes, add it to phase 2");
            let error = request(confirm("Yes, add it to phase 2", 1)).unwrap_err().to_string();
            assert!(error.contains("has not confirmed"), "{error}");
            say(crate::state::now() + 0.2, "Confirmed, add it now");
            let error = request(confirm("Confirmed, add it now", 2)).unwrap_err().to_string();
            assert!(error.contains("for phase 1, not phase 2") || error.contains("not running"), "{error}");
            assert_eq!(stored().tasks.len(), 1);
            // C1: a refusal never confirms, an unrelated sentence with "ok" is
            // not the answer, and a fragment is not the owner's whole sentence.
            for said in ["No, don't add it", "не добавляй", "ok, what's the status"] {
                say(crate::state::now() + 0.2, said);
                let error = request(confirm(said, 1)).unwrap_err().to_string();
                assert!(error.contains("has not confirmed"), "{said}: {error}");
            }
            let error = request(confirm("add it", 1)).unwrap_err().to_string();
            assert!(error.contains("has not confirmed"), "{error}");
            assert_eq!(stored().tasks.len(), 1, "no refusal added a task");
            // 5. The confirmed addition adds the task with added_late.
            let added = request(confirm("Confirmed, add it now", 1)).unwrap();
            assert_eq!(added["status"], "added", "{added}");
            let team = stored();
            let task = team.task(2).unwrap();
            assert_eq!((task.phase, task.status), (1, TaskStatus::Pending));
            let late = task.added_late.as_ref().unwrap();
            assert_eq!(late.by, "Lead");
            assert_eq!(late.request, "Please add a task to the current phase: fix the login crash");
            assert_eq!(late.confirm, "Confirmed, add it now");
            assert!(team.pending_additions.is_empty());
            let feed = run("events", &json!({"team":"t"}), false).unwrap().to_string();
            assert!(feed.contains("phase_tasks_added") && feed.contains("phase_addition_warned"), "{feed}");
            // The same words are spent.
            let error = request(json!({"phase":1,"tasks":[{"id":3,"title":"Again","assignee":"dev"}],
                "owner_word":{"text":"Please add a task to the current phase"}})).unwrap_err().to_string();
            assert!(error.contains("already used"), "{error}");
            // 6. The owner on the desktop: the warning, then "add" or the phase number.
            super::set_test_caller_identities(Some((None, None, None)));
            let human = |value: Value| { let mut value = value; value["by"] = json!("human"); value };
            let warned = request(human(json!({"phase":1,"tasks":[{"id":3,"title":"Docs","assignee":"dev"}]}))).unwrap();
            assert!(warned["warning"].as_str().unwrap().contains("\"Docs\""), "{warned}");
            let pending = warned["pending"].as_str().unwrap().to_owned();
            let error = request(human(json!({"phase":1,"confirm":pending,"confirmation":"sure"}))).unwrap_err().to_string();
            assert!(error.contains("Type \"add\" or \"1\""), "{error}");
            let added = request(human(json!({"phase":1,"confirm":pending,"confirmation":"1"}))).unwrap();
            assert_eq!(added["status"], "added", "{added}");
            assert_eq!(stored().task(3).unwrap().added_late.as_ref().unwrap().by, "You");
            // A draft phase keeps edit-draft.
            let mut draft = stored();
            let mut events = Vec::new();
            ops::apply(&mut draft, "extend", &json!({"phase_title":"Next","tasks":[{"id":4,"title":"Later","assignee":"dev"}]}),
                &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
            let error = ops::apply(&mut draft, "add-to-phase", &json!({"phase":2,"tasks":[{"id":5,"title":"Late","assignee":"dev"}]}),
                &Actor::Human, &BTreeSet::new(), &mut events).unwrap_err().to_string();
            assert!(error.contains("edit-draft") || error.contains("not running"), "{error}");
            // 7. A member never adds.
            super::set_test_caller_identities(Some((None, Some("codex/x/dev".into()), Some("r1".into()))));
            let error = request(json!({"phase":1,"tasks":[{"id":4,"title":"Mine","assignee":"dev"}],"by":"human"})).unwrap_err().to_string();
            assert!(!error.is_empty());
            assert!(stored().task(4).is_none());
            super::set_test_caller_identities(None);
        });
    }

    /// U32: the events fixture the lead's own session reads owner words from.
    fn owner_word_events(dir: &std::path::Path, prompts: &[(f64, &str)]) {
        use rusqlite::Connection;
        let db = Connection::open(dir.join("events.sqlite3")).unwrap();
        db.execute_batch("CREATE TABLE IF NOT EXISTS events (seq INTEGER PRIMARY KEY, name TEXT, conversation TEXT, payload TEXT);").unwrap();
        for (at, detail) in prompts {
            db.execute(
                "INSERT INTO events(name,conversation,payload) VALUES ('dsh/x/lead','c1',?1)",
                rusqlite::params![json!({"type":"UserPromptSubmit","at":at,"agent_id":"","detail":detail}).to_string()],
            ).unwrap();
        }
    }

    /// U32: a running team whose member cap is already full (six members).
    fn full_team() -> Team {
        use crate::state::teams::model::{FolderPolicy, Settings};
        let mut team = Team {
            id: "abcdef12-0000-4000-8000-000000000000".into(),
            name: "T".into(),
            lead: "lead".into(),
            status: TeamStatus::Running,
            settings: Settings { max_members: 6, folder_policy: FolderPolicy::Shared, ..Default::default() },
            ..Default::default()
        };
        team.members.push(Member {
            id: "lead".into(),
            role: "Lead".into(),
            agent: "dsh".into(),
            session: Some(SessionRef { name: "dsh/x/lead".into(), conversation_id: "c1".into(), ..Default::default() }),
            ..Default::default()
        });
        for index in 1..=5 {
            team.members.push(Member { id: format!("m{index}"), role: "Backend".into(), agent: "dsh".into(), ..Default::default() });
        }
        team
    }

    /// U32: the owner's plain request adds a member by role or agent, raises a
    /// full cap by the count the sentence names, and one word covers one add.
    #[test]
    fn the_owners_word_adds_a_member_by_role_and_raises_a_full_cap() {
        let dir = tempfile::tempdir().unwrap();
        let now = crate::state::now();
        let sentence = "может еще одного дипсик бэкендера добавим?";
        let frontend = "может еще одного фронтендера добавим?";
        let stale = "еще одного дипсик бэкендера добавим";
        let relayed = "[Zerus team · from: Zerus] should we add a backend?";
        owner_word_events(dir.path(), &[
            (now - 60.0, sentence),
            (now - 60.0, frontend),
            (now - super::super::owner_word::FRESH_SECONDS - 60.0, stale),
            (now - 60.0, relayed),
            (now - 60.0, "Please remove the backend from the roster"),
            (now - 60.0, "Please remove the tester from the roster"),
        ]);
        crate::state::with_test_state_root(dir.path(), || {
            set_test_caller_identities(Some((Some("c1".into()), None, None)));
            let mut store = Store::open(true).unwrap();
            let team = full_team();
            let id = team.id.clone();
            store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
            let add = |request: &str, member: &str, text: &str| {
                run(
                    "add-member",
                    &json!({"request_id":request,"team":id,"member":{"id":member,"role":"Backend","agent":"dsh"},
                        "owner_word":{"text":text}}),
                    false,
                )
            };
            // A sentence naming another role does not add a backend.
            let error = add("22222222-2222-4222-8222-222222222222", "backend4", frontend).unwrap_err();
            let error = format!("{error:#}");
            assert!(error.contains("does not name the member to add"), "{error}");
            // A stale sentence and a relayed one are not the owner's word.
            let error = add("33333333-3333-4333-8333-333333333333", "backend4", stale).unwrap_err().to_string();
            assert!(error.contains("your word"), "{error}");
            let error = add("44444444-4444-4444-8444-444444444444", "backend4", relayed).unwrap_err().to_string();
            assert!(error.contains("your word"), "{error}");
            // The owner's plain request adds a dsh Backend and raises the full cap by one.
            let value = add("55555555-5555-4555-8555-555555555555", "backend4", sentence).unwrap();
            assert_eq!(value["status"], "added");
            assert_eq!(value["by"], "Lead on your word");
            let saved = Store::open(false).unwrap().load(&id).unwrap().unwrap();
            assert_eq!(saved.settings.max_members, 7);
            let added = saved.member("backend4").unwrap().added.clone().unwrap();
            assert_eq!(added["by"], "Lead on your word");
            assert_eq!(added["owner_word"], sentence);
            // Status shows how the member joined.
            let status = run("status", &json!({"team":id}), false).unwrap();
            assert_eq!(status["settings"]["max_members"], 7);
            let member = status["members"].as_array().unwrap().iter().find(|member| member["id"] == "backend4").cloned().unwrap();
            assert_eq!(member["added"]["by"], "Lead on your word");
            assert_eq!(member["added"]["owner_word"], sentence);
            // The feed names both caps and carries the quote.
            let events = run("events", &json!({"team":id}), false).unwrap();
            let items = events["items"].as_array().unwrap();
            let raised = items.iter().find(|item| item["kind"] == "max_members_raised").unwrap();
            assert_eq!((raised["detail"]["old"].as_u64(), raised["detail"]["new"].as_u64()), (Some(6), Some(7)));
            let joined = items.iter().find(|item| item["kind"] == "member_added").unwrap();
            assert_eq!(
                (joined["detail"]["by"].as_str(), joined["detail"]["owner_word"].as_str()),
                (Some("Lead on your word"), Some(sentence))
            );
            // One word covers one add: the same sentence adds no second member.
            let error = add("66666666-6666-4666-8666-666666666666", "backend5", sentence).unwrap_err().to_string();
            assert!(error.contains("already used"), "{error}");
            assert!(Store::open(false).unwrap().load(&id).unwrap().unwrap().member("backend5").is_none());
            // N15: a removal names the member by id or by its role: a sentence
            // naming another role is refused, the role's own one is the word.
            let error = run(
                "remove-member",
                &json!({"request_id":"77777777-7777-4777-8777-777777777777","team":id,"member":"backend4",
                    "owner_word":{"text":"Please remove the tester from the roster"}}),
                false,
            )
            .unwrap_err()
            .to_string();
            assert!(error.contains("your word"), "{error}");
            let removed = run(
                "remove-member",
                &json!({"request_id":"88888888-8888-4888-8888-888888888888","team":id,"member":"backend4",
                    "owner_word":{"text":"Please remove the backend from the roster"}}),
                false,
            )
            .unwrap();
            assert_eq!((removed["status"].as_str(), removed["member"].as_str()), (Some("removed"), Some("backend4")));
        });
    }

    /// U42 (#587): the lead discards an accepted phase, or returns one to a
    /// draft, on the owner's sentence naming the phase and the change; the CLI
    /// verifies it as it does for an approval.
    #[test]
    fn the_lead_discards_or_redrafts_an_accepted_phase_on_the_owners_word() {
        use crate::state::teams::model::{Phase, PhaseState, Task, TaskStatus};
        let dir = tempfile::tempdir().unwrap();
        let now = crate::state::now();
        owner_word_events(dir.path(), &[
            (now - 60.0, "discard phase 3, it is stale"),
            (now - 60.0, "верни фазу 2 в черновик"),
            (now - 60.0, "phase 2 looks fine"),
            (now - 60.0, "Don't discard phase 3, rework it"),
            (now - 60.0, "не отменяй фазу 2"),
            (now - 60.0, "phase 2 is not a draft any more"),
        ]);
        crate::state::with_test_state_root(dir.path(), || {
            set_test_caller_identities(Some((Some("c1".into()), None, None)));
            let mut store = Store::open(true).unwrap();
            let mut team = full_team();
            team.tasks = vec![
                Task { id: 1, title: "A".into(), assignee: "m1".into(), status: TaskStatus::InProgress, ..Default::default() },
                Task { id: 2, phase: 2, title: "B".into(), assignee: "m1".into(), status: TaskStatus::Pending, ..Default::default() },
                Task { id: 3, phase: 3, title: "C".into(), assignee: "m1".into(), status: TaskStatus::Pending, ..Default::default() },
            ];
            team.phases = vec![
                Phase { n: 2, order: 2, title: "Two".into(), state: PhaseState::Queued, tasks: vec![2], ..Default::default() },
                Phase { n: 3, order: 3, title: "Three".into(), state: PhaseState::Queued, tasks: vec![3], ..Default::default() },
            ];
            let id = team.id.clone();
            store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
            let call = |op: &str, request: &str, phase: u32, text: Option<&str>| {
                let mut input = json!({"request_id":request,"team":id,"phase":phase});
                if let Some(text) = text {
                    input["owner_word"] = json!({"text":text});
                }
                run(op, &input, false)
            };
            // Without a word, or on a sentence that does not name the change,
            // the lead cannot.
            let error = call("discard-phase", "22222222-2222-4222-8222-222222222222", 3, None).unwrap_err().to_string();
            assert!(error.contains("Only you discard an accepted phase"), "{error}");
            let error = call("discard-phase", "33333333-3333-4333-8333-333333333333", 2, Some("phase 2 looks fine")).unwrap_err().to_string();
            assert!(error.contains("has not asked to discard phase 2"), "{error}");
            // A discard sentence names its own phase only.
            let error = call("discard-phase", "44444444-4444-4444-8444-444444444444", 2, Some("discard phase 3")).unwrap_err().to_string();
            assert!(error.contains("has not asked to discard phase 2"), "{error}");
            // #595 C1: a refusal never authorizes the change, in either language,
            // and the quote is the owner's whole sentence, never a fragment.
            let error = call("discard-phase", "4a444444-4444-4444-8444-444444444444", 3, Some("Don't discard phase 3")).unwrap_err().to_string();
            assert!(error.contains("has not asked to discard phase 3"), "{error}");
            let error = call("discard-phase", "4b444444-4444-4444-8444-444444444444", 2, Some("не отменяй фазу 2")).unwrap_err().to_string();
            assert!(error.contains("has not asked to discard phase 2"), "{error}");
            let error = call("return-to-draft", "4c444444-4444-4444-8444-444444444444", 2, Some("phase 2 is not a draft any more")).unwrap_err().to_string();
            assert!(error.contains("has not asked to return phase 2 to a draft"), "{error}");
            let error = call("discard-phase", "4d444444-4444-4444-8444-444444444444", 3, Some("discard phase 3")).unwrap_err().to_string();
            assert!(error.contains("has not asked to discard phase 3"), "a fragment is not the sentence: {error}");
            // The owner's discard of phase 3 is the lead's word for it: the whole
            // sentence, as the owner wrote it.
            let discarded = call("discard-phase", "55555555-5555-4555-8555-555555555555", 3, Some("discard phase 3, it is stale")).unwrap();
            assert_eq!(discarded["status"], "phase_discarded");
            // The Russian sentence returns phase 2 to a draft.
            let returned = call("return-to-draft", "66666666-6666-4666-8666-666666666666", 2, Some("верни фазу 2 в черновик")).unwrap();
            assert_eq!(returned["status"], "phase_returned_to_draft");
            let saved = Store::open(false).unwrap().load(&id).unwrap().unwrap();
            assert_eq!(saved.phases.iter().map(|phase| (phase.n, phase.state)).collect::<Vec<_>>(), vec![(2, PhaseState::Draft)]);
            assert_eq!(saved.task(2).unwrap().status, TaskStatus::Draft);
            let events = run("events", &json!({"team":id}), false).unwrap();
            let item = events["items"].as_array().unwrap().iter().find(|item| item["kind"] == "phase_returned_to_draft").cloned().unwrap();
            assert_eq!((item["detail"]["by"].as_str(), item["detail"]["owner_word"]["text"].as_str()), (Some("Lead"), Some("верни фазу 2 в черновик")));
        });
    }

    /// N3/C6: the owner paths that create a team or change the catalog have a
    /// test through the command entry: an agent session is refused, and you
    /// (outside every session, saying `"by":"human"`) may still act.
    #[test]
    fn the_create_and_catalog_owner_paths_refuse_an_agent_session() {
        let dir = tempfile::tempdir().unwrap();
        crate::state::with_test_state_root(dir.path(), || {
            let _ = Store::open(true).unwrap();
            let request = "11111111-1111-4111-8111-111111111111";
            let input = json!({"request_id":request,"name":"T","folder":dir.path().join("project").display().to_string(),
                "lead":{"id":"lead","agent":"claude"},"by":"human"});
            set_test_caller_identities(Some((Some("conversation".into()), None, None)));
            let error = format!("{:#}", run("create", &input, false).unwrap_err());
            assert!(error.contains("An agent session cannot act as the owner"), "{error}");
            // The catalog's owner path takes the same rule, for every caller.
            let error = super::catalog_owner(&json!({"by":"human"})).map_err(|error| error.to_string()).unwrap_err();
            assert!(error.contains("An agent session cannot act as the owner"), "{error}");
            // An hgs session and a run marker are as much an agent as a dsh one.
            for identities in [(None, Some("hgs/x/dev".to_owned()), None), (None, None, Some("run-1".to_owned()))] {
                set_test_caller_identities(Some(identities));
                let error = format!("{:#}", run("create", &input, false).unwrap_err());
                assert!(error.contains("An agent session cannot act as the owner"), "{error}");
            }
            // Without "by":"human" the catalog command is not the owner's at all.
            set_test_caller_identities(Some((None, None, None)));
            assert!(!super::catalog_owner(&json!({})).unwrap());
            // Outside every session you create the team and reach the owner path.
            assert!(super::catalog_owner(&json!({"by":"human"})).unwrap());
            let created = run("create", &input, false).unwrap();
            assert_eq!(created["team"].as_str().map(str::len), Some(36), "{created}");
        });
    }

    /// N7: "add two backends" authorizes two adds, and the full cap grows by
    /// one for each of them; a third add on the same sentence is refused.
    #[test]
    fn a_count_word_authorizes_that_many_adds_and_the_cap_grows_by_one_each() {
        let dir = tempfile::tempdir().unwrap();
        let now = crate::state::now();
        let sentence = "add two deepseek backends to the roster";
        owner_word_events(dir.path(), &[(now - 60.0, sentence)]);
        crate::state::with_test_state_root(dir.path(), || {
            set_test_caller_identities(Some((Some("c1".into()), None, None)));
            let mut store = Store::open(true).unwrap();
            let team = full_team();
            let id = team.id.clone();
            let cap = team.settings.max_members;
            store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
            let add = |request: &str, member: &str| {
                run(
                    "add-member",
                    &json!({"request_id":request,"team":id,"member":{"id":member,"role":"Backend","agent":"dsh"},
                        "owner_word":{"text":sentence}}),
                    false,
                )
            };
            let members = || Store::open(false).unwrap().load(&id).unwrap().unwrap().settings.max_members;
            assert_eq!(add("22222222-2222-4222-8222-222222222222", "backend4").unwrap()["status"], "added");
            assert_eq!(members(), cap + 1);
            assert_eq!(add("33333333-3333-4333-8333-333333333333", "backend5").unwrap()["status"], "added");
            assert_eq!(members(), cap + 2);
            let error = add("44444444-4444-4444-8444-444444444444", "backend6").unwrap_err().to_string();
            assert!(error.contains("already used"), "{error}");
        });
    }

    /// #432 N8: a sub-team's create input cannot set an owner-only setting,
    /// through the command entry itself - the stored child keeps its defaults.
    #[test]
    fn a_subteam_create_input_cannot_set_owner_settings() {
        use crate::state::teams::model::Settings;
        let dir = tempfile::tempdir().unwrap();
        crate::state::with_test_state_root(dir.path(), || {
            let mut store = Store::open(true).unwrap();
            let parent = running_team();
            let parent_id = parent.id.clone();
            let subteam_approval = parent.settings.subteam_approval;
            store.create("11111111-1111-4111-8111-111111111111", &json!({}), parent, vec![]).unwrap();
            let created = run(
                "create",
                &json!({"request_id":"22222222-2222-4222-8222-222222222222","name":"Sub","by":"human",
                    "parent":{"team":parent_id,"task":1},
                    "settings":{"auto_accept":true,"max_members":99,"tree_depth":8,"subteam_approval":"owner",
                        "report_timeout_seconds":90}}),
                false,
            )
            .unwrap();
            let child = Store::open(false).unwrap().load(created["team"].as_str().unwrap()).unwrap().unwrap();
            let defaults = Settings::default();
            assert_eq!(child.settings.max_members, defaults.max_members);
            assert_eq!(child.settings.tree_depth, defaults.tree_depth);
            assert!(!child.settings.auto_accept);
            // The parent's approval rule is inherited and the plan stays review.
            assert_eq!(child.settings.subteam_approval, subteam_approval);
            assert_eq!(child.settings.approval, Approval::Review);
            assert_eq!(child.settings.phase_approval, Some(Approval::Review));
            // The rest of the input is the owner's still.
            assert_eq!(child.settings.report_timeout_seconds, 90);
        });
    }

    #[test]
    fn a_lead_starts_the_next_goal_only_after_an_accepted_proposal() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open_at(&dir.path().join("teams.sqlite3"), true).unwrap();
        let mut done = Team { id: "done".into(), name: "Done".into(), lead: "lead".into(), status: TeamStatus::Done, goal: "one".into(), ..Default::default() };
        done.members.push(Member { id: "lead".into(), role: "Lead".into(), agent: "claude".into(), ..Default::default() });
        store.create("66666666-6666-4666-8666-666666666666", &json!({}), done.clone(), Vec::new()).unwrap();
        // N1: the lead alone cannot start the next goal.
        let error = super::next_goal_as(
            &mut store,
            "77777777-7777-4777-8777-777777777777",
            &json!({"request_id":"77777777-7777-4777-8777-777777777777","team":"done","goal":"two"}),
            &Actor::Member("lead".into()),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("accepted next-goal proposal"), "{error}");
        // With the owner's accepted proposal it can.
        let mut with_proposal = done.clone();
        with_proposal.id = "done2".into();
        with_proposal.proposals.push(json!({"id":"p1","kind":"next-goal","state":"accepted","goal":"two"}));
        store.create("88888888-8888-4888-8888-888888888888", &json!({}), with_proposal, Vec::new()).unwrap();
        let value = super::next_goal_as(
            &mut store,
            "99999999-9999-4999-8999-999999999999",
            &json!({"request_id":"99999999-9999-4999-8999-999999999999","team":"done2","goal":"other"}),
            &Actor::Member("lead".into()),
        )
        .unwrap();
        assert_eq!(value["status"], "planned");
        // N12: the accepted proposal's goal is used, and the done team cannot
        // start a second next goal.
        let next = store.load(value["team"].as_str().unwrap()).unwrap().unwrap();
        assert_eq!(next.goal, "two");
        let error = super::next_goal_as(
            &mut store,
            "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
            &json!({"request_id":"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa","team":"done2","goal":"three"}),
            &Actor::Member("lead".into()),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("already started"), "{error}");
    }

    #[test]
    fn leftovers_lists_running_sessions_of_finished_teams() {
        use crate::state::teams::dispatcher::Seen;
        use crate::state::teams::model::{Member, MemberSeen, SessionRef, Team, TeamStatus};
        let mut done = Team { id: "t1".into(), name: "T".into(), lead: "lead".into(), status: TeamStatus::Done, ..Default::default() };
        done.members.push(Member { id: "lead".into(), role: "Lead".into(), agent: "claude".into(), ..Default::default() });
        let mut dev = Member { id: "dev".into(), role: "Backend".into(), agent: "codex".into(), ..Default::default() };
        dev.session = Some(SessionRef { name: "claude/x/dev".into(), ..Default::default() });
        dev.seen = Some(MemberSeen { at: 30.0, live: true });
        done.members.push(dev);
        let sessions = vec![Seen { name: "claude/x/dev".into(), live: true, ..Default::default() }];
        let items = super::leftovers(std::slice::from_ref(&done), &sessions);
        assert_eq!(items.len(), 1);
        assert_eq!(
            (items[0]["team"].as_str(), items[0]["member"].as_str(), items[0]["since"].as_f64()),
            (Some("t1"), Some("dev"), Some(30.0))
        );
        // A running team's session is not a leftover; an archived team's is.
        let mut running = done.clone();
        running.status = TeamStatus::Running;
        assert!(super::leftovers(&[running], &sessions).is_empty());
        let mut archived = done.clone();
        archived.status = TeamStatus::Archived;
        assert_eq!(super::leftovers(&[archived], &sessions).len(), 1);
    }

    #[test]
    fn a_shared_dsh_host_lists_its_members_and_marks_them() {
        let members = vec![
            ("a".to_owned(), Some(json!({"pid":7,"started":10.0}))),
            ("b".to_owned(), Some(json!({"pid":7,"started":10.0}))),
            ("c".to_owned(), Some(json!({"pid":9,"started":20.0}))),
            ("d".to_owned(), None),
        ];
        let (hosts, shared) = hosts_value(&members);
        assert_eq!(
            hosts,
            json!([
                {"agent":"dsh","pid":7,"started":10.0,"members":["a","b"]},
                {"agent":"dsh","pid":9,"started":20.0,"members":["c"]}
            ])
        );
        assert_eq!(shared, BTreeSet::from(["a".to_owned(), "b".to_owned()]));
    }

    #[test]
    fn a_member_prompt_snapshot_survives_a_catalog_edit_through_run() {
        let _env = crate::state::ENV_LOCK.lock().unwrap();
        let old = ["HGS_SESSION", "HGS_RUN_ID", "DSH_SESSION_ID"].map(|key| (key, std::env::var(key).ok()));
        for key in ["HGS_SESSION", "HGS_RUN_ID", "DSH_SESSION_ID"] { std::env::remove_var(key); }
        let dir = tempfile::tempdir().unwrap();
        crate::state::with_test_state_root(dir.path(), || {
            super::set_test_caller_identities(Some((None, None, None)));
            let mut catalog = crate::state::teams::catalog::builtin();
            let role = catalog.roles.iter_mut().find(|role| role.id == "backend").unwrap();
            role.prompt_order = vec!["protocol".into()];
            role.protocol_extra = "OLD BLOCK".into();
            crate::state::teams::catalog::run("set", &json!({"version":1,"catalog":catalog}), true).unwrap();
            let _ = Store::open(true).unwrap();
            let created = super::run("create", &json!({"request_id":"10000000-0000-4000-8000-000000000001","name":"T","by":"human",
                "lead":{"id":"lead","role":"Lead","agent":"codex"},
                "members":[{"id":"dev","role":"Backend","agent":"codex","prompt":"Backend prompt"}],
                "tasks":[{"id":1,"title":"Work","assignee":"dev"}]}), false).unwrap();
            let store = Store::open(false).unwrap();
            let team_id = created["team"].as_str().unwrap_or("t").to_owned();
            let mut next = crate::state::teams::catalog::load().unwrap();
            next.roles.iter_mut().find(|role| role.id == "backend").unwrap().protocol_extra = "NEW BLOCK".into();
            crate::state::teams::catalog::run("set", &json!({"version":next.version,"catalog":next}), true).unwrap();
            assert_eq!(crate::state::teams::catalog::load().unwrap().roles.iter().find(|role| role.id == "backend").unwrap().protocol_extra, "NEW BLOCK");
            let team = store.load(&team_id).unwrap().unwrap();
            let task = team.task(1).unwrap().clone();
            let attempt = Attempt { n: 1, id: "a1".into(), member: "dev".into(), ..Default::default() };
            let text = crate::state::teams::dispatcher::hand_off_text(&team, &task, &attempt, "", "");
            assert!(text.contains("OLD BLOCK") && !text.contains("NEW BLOCK"), "{text}");
            let mut old_team = team.clone();
            old_team.members.iter_mut().find(|member| member.id == "dev").unwrap().prompt_snapshot = None;
            let text = crate::state::teams::dispatcher::hand_off_text(&old_team, &task, &attempt, "", "");
            assert!(text.contains("NEW BLOCK"), "{text}");
            super::set_test_caller_identities(None);
        });
        for (key, value) in old {
            match value { Some(value) => std::env::set_var(key, value), None => std::env::remove_var(key) }
        }
    }

    #[test]
    fn phase35_findings_work_through_run() {
        let _env = crate::state::ENV_LOCK.lock().unwrap();
        let old = ["HGS_SESSION", "HGS_RUN_ID", "DSH_SESSION_ID"].map(|key| (key, std::env::var(key).ok()));
        for key in ["HGS_SESSION", "HGS_RUN_ID", "DSH_SESSION_ID"] { std::env::remove_var(key); }
        let dir = tempfile::tempdir().unwrap();
        crate::state::with_test_state_root(dir.path(), || {
            super::set_test_caller_identities(Some((None, None, None)));
            let mut store = Store::open(true).unwrap();
            let req = |n: u8| format!("00000000-0000-4000-8000-0000000000{n:02}");
            let mut parent = Team { id: "p".into(), name: "Parent".into(), lead: "lead".into(), status: TeamStatus::Running,
                settings: Settings { tree_sessions: 16, ..Default::default() }, ..Default::default() };
            parent.tasks.push(Task { id: 1, title: "Delegated".into(), assignee: "dev".into(), status: TaskStatus::InProgress,
                subteam: Some("c".into()), ..Default::default() });
            parent.tasks[0].attempts.push(Attempt { n: 1, id: "pa".into(), member: "dev".into(), state: AttemptState::Delivered,
                questions: vec![Question { id: "pq".into(), text: "Which DB?".into(), at: 1.0, forwarded: "q1".into(), ..Default::default() }], ..Default::default() });
            let mut child = Team { id: "c".into(), name: "Child".into(), lead: "dev".into(), status: TeamStatus::Running,
                parent: Some(Parent { team: "p".into(), task: 1, lead: "lead".into(), member: "dev".into() }), ..Default::default() };
            child.tasks.push(Task { id: 1, title: "Work".into(), assignee: "dev".into(), status: TaskStatus::Accepted, ..Default::default() });
            child.tasks[0].attempts.push(Attempt { n: 1, id: "ca".into(), member: "dev".into(), state: AttemptState::Reported,
                report: Some(Report { text: "Done".into(), at: 1.0, ..Default::default() }),
                questions: vec![Question { id: "q1".into(), text: "Which DB?".into(), at: 1.0, forwarded: "pq".into(), ..Default::default() }], ..Default::default() });
            store.create(&req(1), &json!({}), parent.clone(), Vec::new()).unwrap();
            store.create(&req(2), &json!({}), child.clone(), Vec::new()).unwrap();

            // N21: the parent's answer leaves the child question Pending.
            super::run("reply", &json!({"request_id":req(3),"team":"p","task":1,"question":"pq","text":"Postgres","by":"human"}), false).unwrap();
            let saved = store.load("c").unwrap().unwrap();
            let question = &saved.tasks[0].attempts[0].questions[0];
            assert_eq!((question.answer.as_str(), question.notice), ("Postgres", Notice::Pending));

            // N22/N23: finish files a Reported attempt the parent inbox lists,
            // and a replay adds no second one.
            let finish = json!({"request_id":req(4),"team":"c","summary":"Sub done","by":"human"});
            super::run("finish", &finish, false).unwrap();
            let parent_now = store.load("p").unwrap().unwrap();
            let attempt = parent_now.tasks[0].attempts.last().unwrap();
            assert_eq!(attempt.state, AttemptState::Reported);
            let inbox = super::run("inbox", &json!({"team":"p","by":"human"}), false).unwrap();
            assert!(inbox["items"].as_array().unwrap().iter().any(|item| item["task"] == 1), "{inbox}");
            let count = parent_now.tasks[0].attempts.len();
            super::run("finish", &finish, false).unwrap();
            assert_eq!(store.load("p").unwrap().unwrap().tasks[0].attempts.len(), count);

            // N24: a reject does not reopen a Stopped sub-team, and the stored
            // reject is not turned into an error by the tree hook (N28).
            let mut stopped = store.load("c").unwrap().unwrap();
            stopped.status = TeamStatus::Stopped;
            store.mutate(None, "c", None, |team, _| { *team = stopped; Ok(Value::Null) }).unwrap();
            super::run("reject", &json!({"request_id":req(5),"team":"p","task":1,"reason":"Rework","by":"human"}), false).unwrap();
            assert_eq!(store.load("c").unwrap().unwrap().status, TeamStatus::Stopped);

            // N25: a second create on the same parent task is refused; a
            // replay after the parent link was lost repairs it.
            let mut parent2 = Team { id: "p2".into(), name: "Parent2".into(), lead: "dev".into(), status: TeamStatus::Running,
                settings: Settings { tree_sessions: 16, ..Default::default() }, ..Default::default() };
            parent2.members.push(Member { id: "dev".into(), role: "Lead".into(), agent: "codex".into(),
                session: Some(SessionRef { name: "codex/x/dev".into(), run_id: "r0".into(), ..Default::default() }), ..Default::default() });
            parent2.tasks.push(Task { id: 2, title: "Delegated".into(), assignee: "dev".into(), status: TaskStatus::InProgress, ..Default::default() });
            store.create(&req(6), &json!({}), parent2.clone(), Vec::new()).unwrap();
            let create = json!({"request_id":req(7),"team":"p2","name":"Sub","by":"human",
                "parent":{"team":"p2","task":2},"lead":{"id":"dev","agent":"codex","role":"Lead"}});
            super::run("create", &create, false).unwrap();
            let second = super::run("create", &json!({"request_id":req(8),"team":"p2","name":"Other","by":"human",
                "parent":{"team":"p2","task":2},"lead":{"id":"dev","agent":"codex","role":"Lead"}}), false);
            assert!(second.unwrap_err().to_string().contains("already has a sub-team"));
            let child_id = store.load("p2").unwrap().unwrap().tasks[0].subteam.clone().unwrap();
            store.mutate(None, "p2", None, |team, _| { team.tasks[0].subteam = None; Ok(Value::Null) }).unwrap();
            super::run("create", &create, false).unwrap();
            assert_eq!(store.load("p2").unwrap().unwrap().tasks[0].subteam.as_deref(), Some(child_id.as_str()));

            // N26: a 20-member plan under a 16-session budget is refused.
            let mut sub = Team { id: "c3".into(), name: "Sub3".into(), lead: "lead".into(), status: TeamStatus::Planning,
                settings: Settings { tree_sessions: 16, ..Default::default() },
                parent: Some(Parent { team: "p2".into(), task: 2, lead: "dev".into(), member: "dev".into() }), ..Default::default() };
            sub.members.push(Member { id: "lead".into(), role: "Lead".into(), agent: "codex".into(), ..Default::default() });
            store.create(&req(9), &json!({}), sub, Vec::new()).unwrap();
            let members: Vec<Value> = (1..=20).map(|n| json!({"id":format!("m{n}"),"role":"Dev","agent":"codex"})).collect();
            let error = super::run("plan", &json!({"request_id":req(10),"team":"c3","by":"human","members":members}), false)
                .unwrap_err().to_string();
            assert!(error.contains("at most 16 sessions"), "{error}");

            // N28: a replay returns the first receipt; a failed upstream
            // forward clears the child's forwarded mark.
            let mut p3 = Team { id: "p3".into(), name: "Parent3".into(), lead: "dev".into(), status: TeamStatus::Running, ..Default::default() };
            p3.members.push(Member { id: "dev".into(), role: "Lead".into(), agent: "codex".into(),
                session: Some(SessionRef { name: "codex/x/dev".into(), run_id: "r0".into(), ..Default::default() }), ..Default::default() });
            p3.tasks.push(Task { id: 3, title: "Delegated".into(), assignee: "dev".into(), status: TaskStatus::InProgress,
                subteam: Some("c4".into()), ..Default::default() });
            p3.tasks[0].attempts.push(Attempt { n: 1, id: "p3a".into(), member: "dev".into(), state: AttemptState::Delivered,
                questions: vec![Question { id: "pq4".into(), text: "Q".into(), at: 1.0, forwarded: "q9".into(), ..Default::default() }], ..Default::default() });
            let mut c4 = Team { id: "c4".into(), name: "Child4".into(), lead: "dev".into(), status: TeamStatus::Running,
                parent: Some(Parent { team: "p3".into(), task: 3, lead: "dev".into(), member: "dev".into() }), ..Default::default() };
            c4.tasks.push(Task { id: 1, title: "W".into(), assignee: "dev".into(), status: TaskStatus::InProgress, ..Default::default() });
            c4.tasks[0].attempts.push(Attempt { n: 1, id: "c4a".into(), member: "dev".into(), state: AttemptState::Delivered,
                questions: vec![Question { id: "q9".into(), text: "Q".into(), at: 1.0, ..Default::default() }], ..Default::default() });
            store.create(&req(11), &json!({}), p3, Vec::new()).unwrap();
            store.create(&req(12), &json!({}), c4, Vec::new()).unwrap();
            let escalate = json!({"request_id":req(13),"team":"c4","question":"q9","by":"human"});
            super::run("escalate", &escalate, false).unwrap();
            let count = store.load("p3").unwrap().unwrap().tasks[0].attempts[0].questions.len();
            super::run("escalate", &escalate, false).unwrap();
            assert_eq!(store.load("p3").unwrap().unwrap().tasks[0].attempts[0].questions.len(), count, "a replay does not duplicate");
            // N24: a question goes up one level at a time: c6 -> c4 -> p3, and
            // the parent's answer travels back down to the member that asked.
            let mut c6 = Team { id: "c6".into(), name: "Child6".into(), lead: "dev".into(), status: TeamStatus::Running,
                parent: Some(Parent { team: "c4".into(), task: 1, lead: "dev".into(), member: "dev".into() }), ..Default::default() };
            c6.tasks.push(Task { id: 1, title: "Deep work".into(), assignee: "dev".into(), status: TaskStatus::InProgress, ..Default::default() });
            c6.tasks[0].attempts.push(Attempt { n: 1, id: "c6a".into(), member: "dev".into(), state: AttemptState::Delivered,
                questions: vec![Question { id: "q11".into(), text: "Which cache?".into(), at: 1.0, ..Default::default() }], ..Default::default() });
            store.create(&req(16), &json!({}), c6, Vec::new()).unwrap();
            let mut c4_deep = store.load("c4").unwrap().unwrap();
            c4_deep.children.push("c6".into());
            c4_deep.tasks[0].subteam = Some("c6".into());
            store.mutate(None, "c4", None, |team, _| { *team = c4_deep; Ok(Value::Null) }).unwrap();
            super::run("escalate", &json!({"request_id":req(17),"team":"c6","question":"q11","by":"human"}), false).unwrap();
            let c4_level = store.load("c4").unwrap().unwrap();
            let carrier = c4_level.tasks[0].attempts[0].questions.iter().find(|q| q.source == "q11").cloned();
            let carrier = carrier.unwrap_or_else(|| panic!("no carrier in c4: {:?}", c4_level.tasks[0].attempts[0].questions));
            // The middle level passes the same question one level up.
            super::run("escalate", &json!({"request_id":req(18),"team":"c4","question":carrier.id,"by":"human"}), false).unwrap();
            let top = store.load("p3").unwrap().unwrap();
            let top_question = top.tasks[0].attempts[0].questions.iter().find(|q| q.source == carrier.id).cloned();
            let top_question = top_question.unwrap_or_else(|| panic!("no question in p3: {:?}", top.tasks[0].attempts[0].questions));
            // The top lead answers by the question only (N23), and the answer
            // reaches q11 two levels down.
            super::run("reply", &json!({"request_id":req(19),"team":"p3","question":top_question.id,"text":"Redis","by":"human"}), false).unwrap();
            let deep = store.load("c6").unwrap().unwrap();
            assert_eq!(deep.tasks[0].attempts[0].questions[0].answer, "Redis", "{:?}", deep.tasks[0].attempts[0].questions);
            let middle = store.load("c4").unwrap().unwrap();
            assert!(middle.tasks[0].attempts[0].questions.iter().any(|q| q.id == carrier.id && q.answer == "Redis"));
            // N25: a create refused before the child exists releases the parent
            // task's reservation; a corrected create for the same task works.
            let mut p5 = Team { id: "p5".into(), name: "Parent5".into(), lead: "dev".into(), status: TeamStatus::Running, ..Default::default() };
            p5.members.push(Member { id: "dev".into(), role: "Lead".into(), agent: "codex".into(),
                session: Some(SessionRef { name: "codex/x/dev".into(), run_id: "r0".into(), ..Default::default() }), ..Default::default() });
            p5.tasks.push(Task { id: 1, title: "Delegated".into(), assignee: "dev".into(), status: TaskStatus::InProgress, ..Default::default() });
            store.create(&req(20), &json!({}), p5, Vec::new()).unwrap();
            let refused = super::run("create", &json!({"request_id":req(21),"team":"p5","name":"Sub","by":"human",
                "parent":{"team":"p5","task":1},"lead":{"id":"dev","agent":"codex","role":"Lead"},
                "members":[{"id":"dev","role":"Helper","agent":"codex"}]}), false).unwrap_err().to_string();
            assert!(refused.contains("used twice"), "{refused}");
            assert!(store.load("p5").unwrap().unwrap().tasks[0].subteam.is_none(), "a refused create leaves the task undelegated");
            super::run("create", &json!({"request_id":req(22),"team":"p5","name":"Sub","by":"human",
                "parent":{"team":"p5","task":1},"lead":{"id":"dev","agent":"codex","role":"Lead"}}), false).unwrap();
            assert_eq!(store.load("p5").unwrap().unwrap().tasks[0].subteam.as_deref(), Some(req(22).as_str()));
            // N26: an added member without a session still takes a tree session:
            // at 16/16 the 17th is refused (it used to count 0 and launch).
            let mut full = Team { id: "m1".into(), name: "Full".into(), lead: "lead".into(), status: TeamStatus::Planning,
                settings: Settings { tree_sessions: 16, ..Default::default() }, ..Default::default() };
            for n in 0..16 {
                full.members.push(Member { id: if n == 0 { "lead".into() } else { format!("m{n}") }, role: "Dev".into(), agent: "codex".into(), ..Default::default() });
            }
            store.create(&req(23), &json!({}), full, Vec::new()).unwrap();
            let error = super::run("add-member", &json!({"request_id":req(24),"team":"m1","by":"human",
                "member":{"id":"extra","role":"Ops","agent":"codex"}}), false).unwrap_err().to_string();
            assert!(error.contains("at most 16 sessions"), "{error}");
            // N26: a same-size re-plan counts the roster once, not twice.
            let members: Vec<Value> = (1..=15).map(|n| json!({"id":format!("m{n}"),"role":"Dev","agent":"codex"})).collect();
            super::run("plan", &json!({"request_id":req(25),"team":"m1","by":"human",
                "members":members,"tasks":[{"id":1,"title":"A","assignee":"lead"}]}), false)
                .unwrap_or_else(|error| panic!("a same-size re-plan is allowed: {error:#}"));
            // N27: a forward that landed but lost its receipt answers with the
            // receipt instead of refusing the question as already escalated.
            let again = super::run("escalate", &json!({"request_id":req(26),"team":"c4","question":"q9","by":"human"}), false);
            assert_eq!(again.unwrap()["status"], "escalated");
            let mut stopped_parent = store.load("p3").unwrap().unwrap();
            stopped_parent.tasks[0].status = TaskStatus::Accepted;
            store.mutate(None, "p3", None, |team, _| { *team = stopped_parent; Ok(Value::Null) }).unwrap();
            let mut c5 = store.load("c4").unwrap().unwrap();
            c5.tasks[0].attempts[0].questions.push(Question { id: "q10".into(), text: "Q2".into(), at: 2.0, ..Default::default() });
            store.mutate(None, "c4", None, |team, _| { *team = c5; Ok(Value::Null) }).unwrap();
            let _ = super::run("escalate", &json!({"request_id":req(14),"team":"c4","question":"q10","by":"human"}), false);
            let child = store.load("c4").unwrap().unwrap();
            assert!(child.tasks[0].attempts[0].questions.iter().find(|q| q.id == "q10").unwrap().forwarded.is_empty());
            super::set_test_caller_identities(None);
            for (key, value) in old {
                match value { Some(value) => std::env::set_var(key, value), None => std::env::remove_var(key) }
            }
        });
    }

    #[test]
    fn the_tree_changes_work_through_the_command_helpers() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open_at(&dir.path().join("teams.sqlite3"), true).unwrap();
        let request = |n: u8| format!("00000000-0000-4000-8000-00000000000{n}");
        let mut parent = Team { id: "p".into(), name: "Parent".into(), lead: "lead".into(), status: TeamStatus::Running, children: vec!["c".into(), "c2".into()], ..Default::default() };
        parent.tasks.push(Task { id: 1, title: "Delegated".into(), assignee: "dev".into(), subteam: Some("c".into()), ..Default::default() });
        parent.tasks.push(Task { id: 2, title: "Other".into(), assignee: "dev".into(), ..Default::default() });
        let mut child = Team { id: "c".into(), name: "Child".into(), lead: "dev".into(), status: TeamStatus::Done, parent: Some(Parent { team: "p".into(), task: 1, lead: "lead".into(), member: "dev".into() }), ..Default::default() };
        child.finish = Some(Finish { by: "Lead".into(), summary: "Sub done".into(), at: 1.0 });
        let mut sibling = Team { id: "c2".into(), name: "Sibling".into(), lead: "dev".into(), status: TeamStatus::Running, children: vec!["g".into()], parent: Some(Parent { team: "p".into(), task: 2, lead: "lead".into(), member: "dev".into() }), ..Default::default() };
        sibling.tasks.push(Task { id: 1, title: "Work".into(), assignee: "dev".into(), ..Default::default() });
        let grand = Team { id: "g".into(), name: "Grand".into(), lead: "dev".into(), status: TeamStatus::Running, parent: Some(Parent { team: "c2".into(), task: 1, lead: "dev".into(), member: "dev".into() }), ..Default::default() };
        for (i, team) in [parent.clone(), child.clone(), sibling.clone(), grand.clone()].into_iter().enumerate() {
            store.create(&request(i as u8 + 1), &json!({}), team, Vec::new()).unwrap();
        }
        // 9.5: finish files the report, reject reopens with Rework.
        file_subteam_report(&mut store, "c").unwrap();
        let parent = store.load("p").unwrap().unwrap();
        let task = parent.task(1).unwrap();
        assert_eq!(task.status, TaskStatus::Submitted);
        assert!(task.attempts.last().unwrap().report.as_ref().unwrap().text.contains("Sub-team c Child"));
        reopen_rejected_subteam(&mut store, "p", &json!({"task":1,"reason":"Fix the migration"})).unwrap();
        let child = store.load("c").unwrap().unwrap();
        assert_eq!(child.status, TeamStatus::Running);
        assert!(child.phases.iter().any(|phase| phase.title == "Rework" && phase.reason == "Fix the migration"));
        // 9.6: escalate forwards and the parent reply returns to the child.
        let mut child = child;
        child.tasks.push(Task { id: 1, title: "Child work".into(), assignee: "dev".into(), ..Default::default() });
        child.tasks[0].attempts.push(Attempt { n: 1, id: "a1".into(), member: "dev".into(), state: AttemptState::Delivered, questions: vec![Question { id: "q1".into(), text: "Which DB?".into(), at: 1.0, ..Default::default() }], ..Default::default() });
        store.mutate(None, "c", None, |team, _| { *team = child.clone(); Ok(Value::Null) }).unwrap();
        let link = child.parent.clone().unwrap();
        let forwarded = ops::forward_question(&child, &child.tasks[0].attempts[0].questions[0]);
        store.mutate(None, "c", None, |team, _| { team.tasks[0].attempts[0].questions[0].forwarded = forwarded.clone(); Ok(Value::Null) }).unwrap();
        forward_to_parent(&mut store, &child, &forwarded, "q1", "Which DB?", &link).unwrap();
        let parent = store.load("p").unwrap().unwrap();
        assert!(parent.task(1).unwrap().attempts.iter().flat_map(|a| &a.questions).any(|q| q.id == forwarded));
        // N23: the inbox's reply names only the question; its task is found.
        route_reply_down(&mut store, "p", &json!({"question":forwarded,"text":"Postgres"})).unwrap();
        let child = store.load("c").unwrap().unwrap();
        assert_eq!(child.tasks[0].attempts[0].questions[0].answer, "Postgres");
        // 9.7: archive needs closed children, cancel asks, stop cascades two levels.
        let parent = store.load("p").unwrap().unwrap();
        assert!(ensure_children_closed(&store, &parent).is_err());
        let cancel = json!({"task":1});
        assert!(choose_subteam(&mut store, &parent, &cancel).is_err());
        let stop = json!({"task":1,"subteam":"stop"});
        choose_subteam(&mut store, &parent, &stop).unwrap();
        assert_eq!(store.load("c").unwrap().unwrap().status, TeamStatus::Stopped);
        cascade_status(&mut store, "p", "stop", 0).unwrap();
        for id in ["c", "c2", "g"] {
            assert_eq!(store.load(id).unwrap().unwrap().status, TeamStatus::Stopped, "{id}");
        }
        let parent = store.load("p").unwrap().unwrap();
        ensure_children_closed(&store, &parent).unwrap();
    }

    #[test]
    fn a_rejected_subteam_report_reopens_it_but_a_final_reject_does_not() {
        // N28: the tree hooks run after the reject is stored; they never turn
        // it into an error, reopen only a non-final reject of a finished
        // sub-team, and leave the parent task with the sub-team.
        let dir = tempfile::tempdir().unwrap();
        crate::state::with_test_state_root(dir.path(), || {
            super::set_test_caller_identities(Some((None, None, None)));
            let mut store = Store::open(true).unwrap();
            let req = |n: u8| format!("00000000-0000-4000-8000-0000000000{n:02}");
            let pair = |parent: &str, child: &str, child_status: TeamStatus| -> (Team, Team) {
                let mut parent_team = Team { id: parent.into(), name: parent.into(), lead: "lead".into(), status: TeamStatus::Running, ..Default::default() };
                parent_team.members.push(Member { id: "lead".into(), role: "Lead".into(), agent: "codex".into(),
                    session: Some(SessionRef { name: "codex/x/lead".into(), run_id: "r0".into(), ..Default::default() }), ..Default::default() });
                parent_team.members.push(Member { id: "dev".into(), role: "Backend".into(), agent: "codex".into(), ..Default::default() });
                parent_team.tasks.push(Task { id: 1, title: "Delegated".into(), assignee: "dev".into(), status: TaskStatus::Submitted,
                    subteam: Some(child.into()), ..Default::default() });
                parent_team.tasks[0].attempts.push(Attempt { n: 1, id: "pa".into(), member: "dev".into(), state: AttemptState::Reported,
                    report: Some(Report { text: format!("[Sub-team {child} Child] Done"), at: 1.0, ..Default::default() }), ..Default::default() });
                parent_team.children.push(child.into());
                let child_team = Team { id: child.into(), name: "Child".into(), lead: "dev".into(), status: child_status,
                    parent: Some(Parent { team: parent.into(), task: 1, lead: "lead".into(), member: "dev".into() }),
                    finish: Some(Finish { by: "Lead".into(), summary: "Done".into(), at: 1.0 }), ..Default::default() };
                (parent_team, child_team)
            };
            let (p1, c1) = pair("p1", "c1", TeamStatus::Done);
            store.create(&req(1), &json!({}), p1, Vec::new()).unwrap();
            store.create(&req(2), &json!({}), c1, Vec::new()).unwrap();
            super::run("reject", &json!({"request_id":req(3),"team":"p1","task":1,"reason":"Fix the migration","by":"human"}), false)
                .unwrap();
            assert_eq!(store.load("c1").unwrap().unwrap().status, TeamStatus::Running);
            let p1_now = store.load("p1").unwrap().unwrap();
            assert_eq!(p1_now.tasks[0].status, TaskStatus::InProgress);
            assert_eq!(p1_now.tasks[0].attempts.last().unwrap().state, AttemptState::Rejected);
            assert!(!crate::state::teams::plan::ready(&p1_now).contains(&1), "the reworking sub-team keeps its task");
            // C5: the reworked sub-team finishes again and its report is filed.
            super::run("finish", &json!({"request_id":req(4),"team":"c1","summary":"The migration is fixed","by":"human"}), false)
                .unwrap();
            let p1_now = store.load("p1").unwrap().unwrap();
            assert_eq!((p1_now.tasks[0].status, p1_now.tasks[0].attempts.len()), (TaskStatus::Submitted, 2));
            assert!(p1_now.tasks[0].attempts.last().unwrap().report.as_ref().unwrap().text.contains("The migration is fixed"));
            // A final reject ends the task and does not reopen a finished child.
            let (p2, c2) = pair("p2", "c2", TeamStatus::Done);
            store.create(&req(5), &json!({}), p2, Vec::new()).unwrap();
            store.create(&req(6), &json!({}), c2, Vec::new()).unwrap();
            super::run("reject", &json!({"request_id":req(7),"team":"p2","task":1,"reason":"Wrong approach","final":true,"by":"human"}), false)
                .unwrap();
            assert_eq!(store.load("c2").unwrap().unwrap().status, TeamStatus::Done, "a final reject leaves the sub-team");
            assert_eq!(store.load("p2").unwrap().unwrap().tasks[0].status, TaskStatus::Rejected);
            // A stopped child does not turn the stored reject into an error.
            let (p3, c3) = pair("p3", "c3", TeamStatus::Stopped);
            store.create(&req(8), &json!({}), p3, Vec::new()).unwrap();
            store.create(&req(9), &json!({}), c3, Vec::new()).unwrap();
            let rejected = super::run("reject", &json!({"request_id":req(10),"team":"p3","task":1,"reason":"Stop it","by":"human"}), false)
                .unwrap();
            assert!(rejected.get("subteam_reopen_error").is_none(), "{rejected}");
            assert_eq!(store.load("c3").unwrap().unwrap().status, TeamStatus::Stopped);
            super::set_test_caller_identities(None);
        });
    }

    #[test]
    fn status_carries_each_phases_health() {
        let mut team = running_team();
        team.tasks.push(Task { id: 2, title: "Next".into(), assignee: "dev".into(), phase: 2, status: TaskStatus::Draft, ..Default::default() });
        team.phases.push(Phase { n: 2, title: "Next".into(), state: PhaseState::Draft, tasks: vec![2], ..Default::default() });
        let value = view_with(&team, &[], None);
        // C7: the plan block, the phase map and each phase's own health.
        assert_eq!(value["phases_health"]["1"]["tasks"], 1);
        assert_eq!(value["phases_health"]["2"]["tasks"], 1);
        let phase = value["phases"].as_array().unwrap().iter().find(|phase| phase["n"] == 2).unwrap();
        assert_eq!(phase["health"]["tasks"], 1);
        // Old fields stay: the plan health still names tasks and a path.
        assert_eq!(value["health"]["tasks"], 2);
        assert!(value["health"].get("critical_path").is_some());
        // Section 10.2: the workspace header rows.
        let summary = value["phase_summary"].as_array().unwrap();
        assert_eq!(summary.len(), 2);
        assert_eq!(summary[0]["n"], 1);
        assert_eq!(summary[0]["status"], "in_work");
        assert_eq!(summary[1]["status"], "draft");
    }

    #[test]
    fn a_permission_escalation_carries_command_source_reason_and_ids() {
        let child = Team { id: "child".into(), name: "Child".into(), lead: "dev".into(), ..Default::default() };
        let pending = PendingQuestion {
            question_id: "claude-approval:x".into(),
            question_hash: "h".into(),
            approval: true,
            at: 1.0,
            card: json!({"tool_input":{"command":"cargo test --locked"},"questions":[{"id":"approval","header":"Bash","body":"Run the tests","options":[]}]}),
        };
        let question = permission_question(&child, "dev", &pending);
        for part in ["Permission request from dev", "Bash", "cargo test --locked", "from the tool input", "Reason: Run the tests", "claude-approval:x", "\"h\"", "hgs team answer --json", "\"team\":\"child\""] {
            assert!(question.text.contains(part), "{part} missing in {}", question.text);
        }
    }

    /// N34 (spec 6.2): PowerShell 5.1 pipes JSON with a UTF-8 BOM. Every
    /// reader that parses JSON from stdin drops it: here through the shared
    /// helper, and below by reading each reader's source, so a new reader
    /// that forgets it fails this test.
    #[test]
    fn every_stdin_json_reader_drops_a_bom() {
        let bom = |text: &str| [&[0xEF, 0xBB, 0xBF][..], text.as_bytes()].concat();
        assert_eq!(crate::state::without_bom(&bom("{\"a\":1}")), b"{\"a\":1}");
        assert_eq!(crate::state::without_bom(b"{}"), b"{}");
        let parsed: Value = serde_json::from_slice(crate::state::without_bom(&bom("{\"text\":\"hi\"}"))).unwrap();
        assert_eq!(parsed["text"], "hi");
        assert!(serde_json::from_slice::<Value>(&bom("{}")).is_err(), "serde refuses a BOM, which is the bug");
        // Streams and framed protocols that never parse JSON from stdin.
        let streams = ["peer_channel.rs", "screen/mod.rs", "platform/windows.rs", "cli.rs"];
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = vec![root.clone()];
        let mut missing = Vec::new();
        let mut readers = 0;
        while let Some(path) = files.pop() {
            if path.is_dir() {
                files.extend(std::fs::read_dir(&path).unwrap().flatten().map(|entry| entry.path()));
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                continue;
            }
            let relative = path.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
            if streams.contains(&relative.as_str()) {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let lines: Vec<&str> = text.lines().collect();
            for (at, line) in lines.iter().enumerate() {
                if !line.contains("stdin()") || line.contains("Stdio::") || line.contains("is_terminal") || line.trim_start().starts_with("//") {
                    continue;
                }
                let window = lines[at..(at + 12).min(lines.len())].join("\n");
                if !(window.contains("from_slice") || window.contains("parse(") || window.contains("submit(") || window.contains("import(")
                    || window.contains("enveloped("))
                {
                    continue;
                }
                readers += 1;
                if !(window.contains("without_bom") || window.contains("json_request") || window.contains("submit(")) {
                    missing.push(format!("{relative}:{}", at + 1));
                }
            }
        }
        assert!(readers >= 25, "only {readers} readers found");
        assert!(missing.is_empty(), "stdin JSON readers that keep a BOM: {missing:?}");
        // `send` strips it inside submit, before its digest.
        let input = std::fs::read_to_string(root.join("state/input.rs")).unwrap();
        let submit = input.find("pub(super) fn submit(").unwrap();
        assert!(input[submit..submit + 400].contains("without_bom(input)"));
    }

    #[test]
    fn a_json_request_drops_a_bom_and_refuses_non_utf8() {
        let mut with_bom = vec![0xEF, 0xBB, 0xBF];
        with_bom.extend_from_slice(br#"{"a":1}"#);
        assert_eq!(crate::state::json_request(&with_bom).unwrap()["a"], 1);
        let error = crate::state::json_request(&[0xFF, 0xFE, b'{']).unwrap_err().to_string();
        assert_eq!(error, "Input is not UTF-8 (PowerShell 5.1: use --json-file)");
        // A `--json-file` request is read and goes through the same reader.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("request.json");
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(br#"{"task":342}"#);
        std::fs::write(&path, bytes).unwrap();
        let read = std::fs::read(&path).unwrap();
        assert_eq!(crate::state::json_request(&read).unwrap()["task"], 342);
    }

    #[test]
    fn the_list_summary_carries_task_phases_and_phase_records() {
        let mut team = crate::state::teams::model::Team::default();
        team.tasks.push(crate::state::teams::model::Task { id: 1, phase: 1, ..Default::default() });
        team.tasks.push(crate::state::teams::model::Task { id: 2, phase: 2, ..Default::default() });
        team.phases.push(crate::state::teams::model::Phase { n: 2, title: "Fixes".into(), tasks: vec![2], ..Default::default() });
        let value = super::summary(&team, true);
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
    fn a_phase_whose_tasks_are_all_cancelled_reads_cancelled_and_is_hidden() {
        let mut team = running_team();
        let mut events = Vec::new();
        let none = BTreeSet::new();
        ops::apply(
            &mut team,
            "extend",
            &json!({"phase_title":"Next","tasks":[{"id":2,"title":"D","assignee":"dev"},{"id":3,"title":"E","assignee":"dev"}]}),
            &Actor::Human,
            &none,
            &mut events,
        )
        .unwrap();
        ops::apply(&mut team, "approve-phase", &json!({"phase":2}), &Actor::Human, &none, &mut events).unwrap();
        for id in [2, 3] {
            ops::apply(&mut team, "cancel", &json!({"task":id}), &Actor::Human, &none, &mut events).unwrap();
        }
        // The status hides the cancelled phase and its tasks by default.
        let hidden = view_with_cancelled(&team, &[], None, false);
        let phases: Vec<u64> = hidden["phases"].as_array().unwrap().iter().map(|phase| phase["n"].as_u64().unwrap()).collect();
        assert!(!phases.contains(&2), "{hidden}");
        assert!(hidden["tasks"].as_array().unwrap().iter().all(|task| task["phase"] != 2), "{hidden}");
        assert!(hidden["phase_summary"].as_array().unwrap().iter().all(|phase| phase["n"] != 2), "{hidden}");
        // Asked for, the phase reads cancelled and its tasks are listed.
        let shown = view_with_cancelled(&team, &[], None, true);
        let phase = shown["phases"].as_array().unwrap().iter().find(|phase| phase["n"] == 2).unwrap();
        assert_eq!(phase["state"], "cancelled", "{shown}");
        assert_eq!(shown["tasks"].as_array().unwrap().iter().filter(|task| task["phase"] == 2).count(), 2);
        let header = shown["phase_summary"].as_array().unwrap().iter().find(|phase| phase["n"] == 2).unwrap();
        assert_eq!((header["status"].clone(), header["state"].clone()), (json!("cancelled"), json!("cancelled")), "{shown}");
        // A phase with one task still open is not cancelled and stays visible.
        ops::apply(&mut team, "extend", &json!({"phase":2,"tasks":[{"id":4,"title":"Back","assignee":"dev"}]}), &Actor::Human, &none, &mut events).unwrap();
        let partial = view_with_cancelled(&team, &[], None, false);
        assert!(partial["phases"].as_array().unwrap().iter().any(|phase| phase["n"] == 2), "{partial}");
        // The plain view (used inside the dispatcher) keeps the whole team.
        assert!(view_with(&team, &[], None)["phases"].as_array().unwrap().iter().any(|phase| phase["n"] == 2));
    }

    #[test]
    fn status_shows_the_plan_mode_and_the_free_phases_state() {
        use super::super::model::{PlanMode, Task, TaskStatus};
        let team = Team {
            id: "t".into(),
            name: "T".into(),
            lead: "lead".into(),
            plan_mode: PlanMode::Free,
            tasks: vec![
                Task { id: 1, title: "A".into(), assignee: "dev".into(), phase: 1, status: TaskStatus::InProgress, ..Default::default() },
                Task { id: 2, title: "B".into(), assignee: "dev".into(), phase: 1, status: TaskStatus::Accepted, ..Default::default() },
            ],
            ..Default::default()
        };
        let value = view_with(&team, &[], None);
        assert_eq!(value["plan_mode"], "free");
        assert_eq!(value["free_phase"]["phase"], 1);
        assert_eq!(value["free_phase"]["state"], "open");
        assert_eq!(value["free_phase"]["running"], json!([1]));
        assert_eq!(value["free_phase"]["accepted"], 1);
        // U59: the header's count since the last build and a pending close.
        let mut with_build = team.clone();
        with_build.free_accepted_since_build = 2;
        with_build.free_close_pending = Some(1);
        let value = view_with(&with_build, &[], None);
        assert_eq!(value["free_phase"]["accepted_since"], 2);
        assert_eq!(value["free_phase"]["close_pending"], 1);
        // Standard teams carry plan_mode and no free phase.
        let standard = Team { plan_mode: PlanMode::Standard, ..team.clone() };
        let value = view_with(&standard, &[], None);
        assert_eq!(value["plan_mode"], "standard");
        assert!(value["free_phase"].is_null());
    }

    #[test]
    fn status_shows_the_owners_phase_states_and_the_stuck_scope() {
        // U25: Build runs, Polish was sent back, Ship waits for the owner.
        let task = |id: u32, phase: u32, status: TaskStatus| Task { id, phase, status, title: format!("T{id}"), assignee: "lead".into(), ..Default::default() };
        let mut team = Team {
            id: "t".into(),
            name: "T".into(),
            lead: "lead".into(),
            status: TeamStatus::Running,
            tasks: vec![task(1, 1, TaskStatus::Accepted), task(2, 2, TaskStatus::Pending), task(3, 3, TaskStatus::Draft), task(4, 4, TaskStatus::Pending)],
            phases: vec![
                Phase { n: 2, title: "Build".into(), state: PhaseState::Approved, tasks: vec![2], ..Default::default() },
                Phase {
                    n: 3,
                    title: "Polish".into(),
                    state: PhaseState::Rejected,
                    tasks: vec![3],
                    rejection: Some(PhaseRejection { comment: "Smaller".into(), by: "You".into(), at: 5.0, reworked_at: None }),
                    ..Default::default()
                },
                Phase { n: 4, title: "Ship".into(), state: PhaseState::Queued, tasks: vec![4], ..Default::default() },
            ],
            ..Default::default()
        };
        let value = view_with(&team, &[], None);
        let states: Vec<&str> = value["phase_summary"].as_array().unwrap().iter().map(|phase| phase["state"].as_str().unwrap()).collect();
        assert_eq!(states, vec!["done", "running", "rejected", "queued"]);
        assert_eq!(value["phases"][1]["state"], "rejected");
        assert_eq!(value["phases"][1]["rejection"]["comment"], "Smaller");
        assert_eq!(value["phases"][2]["waiting"]["text"], "Starts after phase 2");
        // Two approved phases ahead: it names the nearer one, the one it
        // queues behind (phase 43 said "after phase 40" with 41 and 42 ahead).
        {
            let mut ahead = team.clone();
            ahead.tasks.push(task(5, 5, TaskStatus::Pending));
            ahead.phases.push(Phase { n: 5, title: "Docs".into(), state: PhaseState::Queued, tasks: vec![5], ..Default::default() });
            let value = view_with(&ahead, &[], None);
            assert_eq!(value["phases"][3]["waiting"]["text"], "Starts after phase 4");
        }
        // Only the running phase can make the team Stuck.
        assert_eq!(value["stuck_scope"], json!([2]));
        // A sent-back draft waits for the lead, not the owner.
        assert!(value["pending_phase"].is_null());
        assert_eq!(value["attention"], json!([]));
        // Reworked, the draft waits for the owner's Accept.
        team.phases[1].state = PhaseState::Draft;
        let value = view_with(&team, &[], None);
        assert_eq!(value["attention"][0]["kind"], "phase_draft");
        assert_eq!(value["attention"][0]["summary"], "Phase 3 \u{b7} Polish \u{2014} draft waits for your Accept");
        // Even with the report reminder off, and the team list carries it too.
        team.settings.remind_owner_minutes = 0;
        team.settings.auto_accept = true;
        let listed = summary(&team, true);
        assert_eq!(listed["attention"][0]["phase"], 3);
        assert_eq!((listed["stuck_scope"].clone(), listed["auto_accept"].clone()), (json!([2]), json!(true)));
        assert_eq!(listed["phases"][1]["state"], "draft");
        // While planning every phase is in the plan.
        team.status = TeamStatus::Planning;
        let planned = plan::phase_summary(&team);
        assert!(planned.iter().all(|phase| phase["state"] == "planned"));
    }

    #[test]
    fn the_send_back_commands_are_mutations_that_may_start_work() {
        for op in ["reject-phase", "undo-reject-phase", "rework-phase"] {
            assert!(MUTATIONS.contains(&op), "{op} is not a mutation");
            assert!(USAGE.contains(op), "{op} is not in the usage");
        }
        // Auto-accept may take a draft on enabling or after a rework.
        assert!(starts_worker("settings") && starts_worker("rework-phase"));
        assert!(!starts_worker("reject-phase") && !starts_worker("undo-reject-phase"));
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
        let listed = summary(&paused, true);
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

    #[test]
    fn a_queued_report_keeps_its_whole_text_and_a_budget_queues_too() {
        // U42 (#512): the queued route stored the placeholder "poll" over the
        // whole report. One request id keeps the first whole text; a later
        // request under it is refused, never queued in its place. A budget
        // from the sandbox queues like a report.
        let _env = crate::state::ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        crate::state::with_test_state_root(dir.path(), || {
            let _worker = hold_worker_lock();
            let mut store = Store::open(true).unwrap();
            let mut team = running_team();
            let member_folder = dir.path().join("dev");
            std::fs::create_dir_all(&member_folder).unwrap();
            {
                let dev = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
                dev.agent = "dsh".into();
                dev.session = Some(SessionRef { name: "dsh/x/dev".into(), run_id: "r1".into(), conversation_id: "conv-dev".into(), ..Default::default() });
                dev.folder = crate::state::teams::model::MemberFolder::Shared;
                dev.directory = member_folder.display().to_string();
            }
            let id = team.id.clone();
            store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
            super::set_test_caller_identities(Some((Some("conv-dev".into()), None, None)));
            let request = "00000000-0000-4000-8000-000000000901";
            let attempt = "00000000-0000-4000-8000-000000000001";
            let whole = "The whole report: 512 checks and their totals";
            let first = super::queue_write(
                "report",
                &json!({"request_id":request,"team":id,"attempt":attempt,"task":1,"text":whole}),
                false,
            )
            .unwrap();
            assert_eq!(first["status"], "queued");
            // A poll that carries a placeholder neither replaces it nor queues again.
            let error = super::queue_write(
                "report",
                &json!({"request_id":request,"team":id,"attempt":attempt,"task":1,"text":"poll"}),
                false,
            )
            .unwrap_err()
            .to_string();
            assert!(error.contains("already queues another request"), "{error}");
            let queued = crate::state::teams::outbox::pending(&member_folder, 10).unwrap();
            assert_eq!(queued.len(), 1);
            assert_eq!(queued[0].1["input"]["text"], whole);
            // The same request again is a retry: it answers its state, one file.
            let again = super::queue_write(
                "report",
                &json!({"request_id":request,"team":id,"attempt":attempt,"task":1,"text":whole}),
                false,
            )
            .unwrap();
            assert_eq!(again["status"], "queued");
            assert_eq!(crate::state::teams::outbox::pending(&member_folder, 10).unwrap().len(), 1);
            // U42: a budget from the member's sandbox queues in its folder too.
            let budget = super::queue_write(
                "expect",
                &json!({"request_id":"00000000-0000-4000-8000-000000000902","team":id,"label":"the suites","minutes":15}),
                false,
            )
            .unwrap();
            assert_eq!(budget["status"], "queued");
            let queued = crate::state::teams::outbox::pending(&member_folder, 10).unwrap();
            assert_eq!(queued.len(), 2);
            assert_eq!(
                (queued[1].1["op"].as_str(), queued[1].1["input"]["minutes"].as_u64()),
                (Some("expect"), Some(15))
            );
            // The gate that sends a write to the queue holds for both kinds.
            for op in ["report", "ask", "commit", "expect"] {
                assert!(super::queues_instead(op), "{op}");
            }
            for op in ["accept", "cancel", "scale", "drain"] {
                assert!(!super::queues_instead(op), "{op}");
            }
            super::set_test_caller_identities(None);
        });
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
    fn current_names_a_stage_members_run_and_its_verdict_route() {
        // U36: a stage member's current attempt is the open stage run given to
        // it; the answer it fills in is a verdict, not a report.
        let mut team = running_team();
        {
            let task = team.task_mut(1).unwrap();
            task.attempts[0].state = AttemptState::Reported;
            task.attempts[0].report = Some(Report { text: "Done".into(), at: 12.0, ..Default::default() });
            task.status = TaskStatus::InReview;
            task.pipeline_run = Some(PipelineRun {
                id: "code".into(),
                name: "Code: review, test, lead".into(),
                stages: vec![PipelineStage { kind: "review".into(), role: "Tester".into(), ..Default::default() }],
                max_rounds: 3,
                folder: "hold".into(),
                accept: "lead".into(),
                round: 1,
                hold_from_report: false,
                trimmed_returns: 0,
                runs: vec![StageRun {
                    id: "stage-run-1".into(),
                    stage: 0,
                    kind: "review".into(),
                    round: 1,
                    attempt: 1,
                    member: "qa".into(),
                    state: StageState::Delivered,
                    ..Default::default()
                }],
            });
        }
        let value = current_view(&team, "qa").unwrap();
        assert_eq!(
            (value["team"].as_str(), value["task"].as_u64(), value["attempt"].as_str()),
            (Some("t"), Some(1), Some("stage-run-1"))
        );
        assert_eq!(
            (value["stage"].as_str(), value["round"].as_u64(), value["phase"].as_str()),
            (Some("review"), Some(1), Some("in_review"))
        );
        let report = &value["report"];
        assert_eq!(
            (report["attempt"].as_str(), report["verdict"].as_str()),
            (Some("stage-run-1"), Some("pass|return"))
        );
        assert_eq!(report["findings"], json!([]));
        assert!(canonical_uuid(report["request_id"].as_str().unwrap()));
        // The implementer's own attempt is not the stage member's current one.
        assert_eq!(current_view(&team, "dev").unwrap_err().to_string(), "#1 has a report in its review stage");
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
        // U36: a stage run id names its member the same way.
        let mut staged = team.clone();
        staged.task_mut(1).unwrap().pipeline_run = Some(PipelineRun {
            runs: vec![StageRun { id: "stage-run-1".into(), member: "qa".into(), ..Default::default() }],
            ..Default::default()
        });
        assert_eq!(attempt_member(&staged, "stage-run-1").as_deref(), Some("qa"));
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
        assert_eq!(resolve_actor(&team, "ask", &json!({}), Some(&own), None, foreign, None).unwrap(), Actor::Member("dev".into()));
        // A dsh member with a leaked env is proven by its conversation.
        team.members.iter_mut().find(|member| member.id == "dev").unwrap().session.as_mut().unwrap().conversation_id = "c1".into();
        assert_eq!(resolve_actor(&team, "ask", &json!({}), None, Some("c1"), foreign, None).unwrap(), Actor::Member("dev".into()));
        // A report by attempt id is proven too.
        assert_eq!(resolve_actor(&team, "report", &json!({"attempt":attempt}), None, None, foreign, None).unwrap(), Actor::Member("dev".into()));
        // The leaked env alone proves nothing: it cannot act as the owner.
        let error = resolve_actor(&team, "stop", &human, None, None, foreign, None).unwrap_err().to_string();
        assert_eq!(error, "Zerus could not verify the calling session");
        // Outside every session, the owner may act.
        assert_eq!(resolve_actor(&team, "stop", &human, None, None, None, None).unwrap(), Actor::Human);
        // U12: a dsh session outside this team, and any session carrying the
        // hgs run marker, never acts as the owner - whatever "by" says.
        for op in ["stop", "approve-phase", "swap-member"] {
            let error = resolve_actor(&team, op, &human, None, Some("session-other"), None, None).unwrap_err().to_string();
            assert_eq!(error, "An agent session cannot act as the owner", "{op}");
            let error = resolve_actor(&team, op, &human, None, None, None, Some("run-other")).unwrap_err().to_string();
            assert_eq!(error, "An agent session cannot act as the owner", "{op}");
        }
    }

    #[test]
    fn a_members_permission_and_answers_show_in_status() {
        let mut team = running_team();
        let member = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
        member.permission = Some(json!({"question_id":"toolu_1","tool":"Bash","command":"rm -rf build",
            "command_source":"tool_input","reason":"clean the build","asked_at":30.0}));
        // U17: an answer keeps the tool and command it was asked about; the one
        // from this attempt's window belongs to the attempt.
        member.permission_answers = vec![
            json!({"question_id":"q0","decision":"deny","by":"lead","at":25.0,"reason":"not now",
                "tool":"Bash","command":"rm -rf build"}),
            json!({"question_id":"q1","decision":"allow","by":"owner","at":5.0,"reason":"","tool":"Bash","command":"ls"}),
        ];
        let value = view_with(&team, &[], None);
        let question = &value["members"][1]["question"];
        assert_eq!(
            (question["tool"].as_str(), question["command"].as_str(), question["command_source"].as_str()),
            (Some("Bash"), Some("rm -rf build"), Some("tool_input"))
        );
        assert_eq!(value["members"][1]["permission_answers"][0]["by"], "lead");
        assert_eq!(value["members"][1]["permission_answers"][0]["decision"], "deny");
        assert_eq!(value["members"][1]["permission_answers"][0]["command"], "rm -rf build");
        // The attempt (sent at 10.0) carries the answer at 25.0, not the one at 5.0.
        let attempt = &value["tasks"][0]["attempts"][0];
        assert_eq!(attempt["permissions"].as_array().unwrap().len(), 1, "{attempt}");
        assert_eq!(attempt["permissions"][0]["command"], "rm -rf build");
        assert_eq!(attempt["permissions"][0]["at"], 25.0);
        // U37: every task carries its timing, computed by status.
        let timing = &value["tasks"][0]["timing"];
        assert!(timing["worked_s"].is_i64() && timing["waits"]["member_s"].is_i64(), "{timing}");
        // N31: the answers belong to the member the attempt ran for, not to the
        // task's current assignee after a reassignment.
        team.task_mut(1).unwrap().assignee = "qa".into();
        let value = view_with(&team, &[], None);
        assert_eq!(value["tasks"][0]["attempts"][0]["permissions"].as_array().unwrap().len(), 1, "{value}");
        // N31: a closed attempt's window ends with its report; an answer recorded
        // after it is not that attempt's any more.
        {
            let attempt = team.task_mut(1).unwrap().attempts.last_mut().unwrap();
            attempt.state = AttemptState::Reported;
            attempt.report = Some(crate::state::teams::model::Report { text: "Done".into(), at: 30.0, ..Default::default() });
        }
        team.members.iter_mut().find(|member| member.id == "dev").unwrap().permission_answers.push(json!(
            {"question_id":"q2","decision":"allow","by":"lead","at":60.0,"reason":"late","tool":"Bash","command":"npm test"}
        ));
        let value = view_with(&team, &[], None);
        let permissions = value["tasks"][0]["attempts"][0]["permissions"].as_array().unwrap();
        assert_eq!(permissions.len(), 1, "{permissions:?}");
        assert_eq!(permissions[0]["at"], 25.0);
        assert!(denies("No") && denies("Deny") && !denies("Allow"));
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
        let listed = summary(&team, true);
        assert_eq!(listed["members"].as_array().unwrap().iter().find(|member| member["id"] == "dev").unwrap()["queue"], expected);
    }

    /// so clients need not derive the hold from inbox text.
    #[test]
    fn status_gives_each_member_the_task_and_stage_that_hold_its_folder() {
        use crate::state::teams::model::{PipelineRun, PipelineStage, StageRun, StageState};
        let mut team = running_team();
        let member = |value: &Value, id: &str| value["members"].as_array().unwrap().iter().find(|member| member["id"] == id).unwrap().clone();
        assert!(member(&view_with(&team, &[], None), "dev").get("held_by").is_none(), "nothing holds dev yet");
        let task = team.task_mut(1).unwrap();
        task.status = TaskStatus::InTest;
        task.attempts = vec![Attempt { n: 1, id: "A1".into(), member: "dev".into(), state: AttemptState::Reported, ..Default::default() }];
        task.pipeline_run = Some(PipelineRun {
            id: "code".into(),
            name: "Code".into(),
            stages: vec![PipelineStage { kind: "test".into(), role: "tester".into(), ..Default::default() }],
            max_rounds: 3,
            folder: "hold".into(),
            accept: "lead".into(),
            round: 1,
            runs: vec![StageRun {
                id: "s".into(),
                stage: 0,
                kind: "test".into(),
                round: 1,
                attempt: 1,
                member: "qa".into(),
                state: StageState::Delivered,
                holds: true,
                at: 1.0,
                delivered_at: Some(2.0),
                ..Default::default()
            }],
            ..Default::default()
        });
        let value = view_with(&team, &[], None);
        assert_eq!(member(&value, "dev")["held_by"], json!({"task":1,"stage":"test"}));
        assert!(member(&value, "lead").get("held_by").is_none());
    }

    /// #717: submit queues, force runs within the resource rule, results list it.
    #[test]
    fn test_submit_force_and_results_use_the_projects_suite() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".zerus")).unwrap();
        std::fs::write(
            dir.path().join(".zerus/tests.toml"),
            "version = 1\n[budgets]\nsuite_minutes = 1\n[[provider]]\nname = \"echo\"\nkind = \"command\"\ncommand = \"echo hello\"\n",
        )
        .unwrap();
        crate::state::with_test_state_root(dir.path(), || {
            let id = "11111111-1111-4111-8111-111111111111";
            let team = Team { id: id.into(), lead: "lead".into(), folder: dir.path().display().to_string(), ..Default::default() };
            let mut store = Store::open_at(&super::super::store::path(), true).unwrap();
            store.create(id, &json!({}), team, vec![]).unwrap();
            let queued = run("test", &json!({"team":id,"action":"submit","suite":"echo","targets":[]}), false).unwrap();
            assert_eq!(queued["status"], "queued", "{queued}");
            let ran = run("test", &json!({"team":id,"action":"force","suite":"echo","targets":[]}), false).unwrap();
            assert_eq!(ran["results"][0]["status"], "passed", "{ran}");
            let listed = run("test", &json!({"team":id,"action":"results"}), false).unwrap();
            assert!(!listed["items"].as_array().unwrap().is_empty(), "{listed}");
        });
    }

    #[test]
    fn status_shows_the_captured_report_that_holds_a_member() {
        let mut team = running_team();
        let dev = |value: &Value| value["members"].as_array().unwrap().iter().find(|member| member["id"] == "dev").unwrap().clone();
        // Nothing holds dev: the field is absent, so the JSON is unchanged.
        assert!(dev(&view_with(&team, &[], None)).get("captured_hold").is_none());
        assert!(dev(&summary(&team, true)).get("captured_hold").is_none());
        // A7: its report on #1 was captured from its last reply.
        let task = team.task_mut(1).unwrap();
        task.status = TaskStatus::Submitted;
        let attempt = task.attempts.last_mut().unwrap();
        attempt.state = AttemptState::Reported;
        attempt.report = Some(Report { text: "Queued, after FU-trees".into(), at: 20.0, captured: true, ..Default::default() });
        let expected = json!({"task":1,"text":"No new work: the captured report on #1 awaits a decision"});
        assert_eq!(dev(&view_with(&team, &[], None))["captured_hold"], expected);
        assert_eq!(dev(&summary(&team, true))["captured_hold"], expected);
        // An explicit report holds nothing.
        team.task_mut(1).unwrap().attempts.last_mut().unwrap().report.as_mut().unwrap().captured = false;
        assert!(dev(&view_with(&team, &[], None)).get("captured_hold").is_none());
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
        assert!(summary(&team, true).get("left_running").is_none());
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
        let items = list_items(&[done], true);
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
        let inbox = inbox_view_for(&team, &team.lead);
        assert_eq!(inbox["items"][0]["kind"], "report");
        assert_eq!((inbox["items"][0]["task"].as_u64(), inbox["items"][0]["member"].as_str()), (Some(1), Some("dev (Backend)")));
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
        assert_eq!(summary(&old, true)["attention"][0]["task"], 1);
        old.settings.remind_owner_minutes = 0;
        assert_eq!(view_with(&old, &[], None)["attention"], json!([]));
        // The settings are optional with the spec's defaults (C2, C4, U18).
        assert_eq!(
            (
                team.settings.inbox_interval_seconds,
                team.settings.remind_lead_minutes,
                team.settings.remind_owner_minutes,
                team.settings.remind_owner_permission_minutes
            ),
            (120, 15, 60, 10)
        );
        // The inbox is a read, named in the usage.
        assert!(USAGE.contains("inbox"));
        assert!(!MUTATIONS.contains(&"inbox"));
    }

    #[test]
    fn the_status_lists_every_check_command() {
        let mut team = running_team();
        team.tasks.push(Task {
            id: 7,
            title: "Build".into(),
            assignee: "dev".into(),
            kind: TaskKind::Check,
            check: Some(Check { command: "cargo test".into(), expect_exit: 0, timeout_minutes: 15 }),
            ..Default::default()
        });
        let value = view_with(&team, &[], None);
        assert_eq!(
            value["checks"],
            json!([{"task":7,"title":"Build","command":"cargo test","expect_exit":0,"timeout_minutes":15}])
        );
        // A plan without checks says so with an empty list.
        assert_eq!(view_with(&running_team(), &[], None)["checks"], json!([]));
    }

    #[test]
    fn a_parent_task_shows_its_subteam_and_the_team_lists_its_children() {
        let parent = running_team();
        let mut child = Team { id: "child".into(), name: "Child".into(), lead: "dev".into(), status: TeamStatus::Running, ..Default::default() };
        child.parent = Some(Parent { team: parent.id.clone(), task: 1, lead: "lead".into(), member: "dev".into() });
        child.members.push(Member { id: "dev".into(), role: "Backend".into(), agent: "codex".into(), ..Default::default() });
        child.tasks.push(Task { id: 1, title: "Sub work".into(), assignee: "dev".into(), status: TaskStatus::Accepted, ..Default::default() });
        child.tasks.push(Task { id: 2, title: "More".into(), assignee: "dev".into(), ..Default::default() });
        let view = view_with(&parent, &[parent.clone(), child.clone()], None);
        // Section 9.4: the delegated task shows its sub-team and counts.
        let task = view["tasks"].as_array().unwrap().iter().find(|task| task["id"] == 1).unwrap();
        assert_eq!(task["subteam"], json!({"team":"child","status":"running","done":1,"total":2}));
        assert_eq!(view["children"], json!([{"team":"child","name":"Child","status":"running","done":1,"total":2}]));
        // The parent's plan health counts only its own tasks.
        assert_eq!(view["health"]["tasks"], 1);
    }

    #[test]
    fn a_subteam_caller_is_the_assignee_parent_lead_or_owner() {
        let mut parent = Team {
            id: "t".into(),
            name: "T".into(),
            lead: "lead".into(),
            status: TeamStatus::Running,
            ..Default::default()
        };
        let session = |name: &str, run: &str, conversation: &str| SessionRef {
            name: name.into(),
            run_id: run.into(),
            conversation_id: conversation.into(),
            host: String::new(),
        };
        parent.members = vec![
            Member { id: "lead".into(), role: "Lead".into(), agent: "claude".into(), session: Some(session("claude/x/lead", "r0", "")), ..Default::default() },
            Member { id: "dev".into(), role: "Backend".into(), agent: "codex".into(), session: Some(session("codex/x/dev", "r1", "")), ..Default::default() },
            Member {
                id: "dshdev".into(),
                role: "Backend".into(),
                agent: "dsh".into(),
                session: Some(session("dsh/x/dshdev", "r2", "conv-dev")),
                ..Default::default()
            },
            Member {
                id: "dshqa".into(),
                role: "Tester".into(),
                agent: "dsh".into(),
                session: Some(session("dsh/x/dshqa", "r3", "conv-qa")),
                ..Default::default()
            },
        ];
        // A DeepSeek assignee is proven by its conversation id, not by an hgs
        // session record.
        assert_eq!(parent_caller(&parent, "dshdev", None, Some("conv-dev"), None, None, "").unwrap(), ParentCaller::Assignee);
        // A dsh non-assignee cannot pass by:human as the owner.
        let error = parent_caller(&parent, "dshdev", None, Some("conv-qa"), None, None, "human")
            .unwrap_err()
            .to_string();
        assert!(error.contains("assignee"), "{error}");
        // The owner path still needs no session env and says it is you.
        assert_eq!(parent_caller(&parent, "dev", None, None, None, None, "human").unwrap(), ParentCaller::Owner);
        // U12: a dsh session outside the parent, or a run marker, may not claim
        // the owner path either.
        let error = parent_caller(&parent, "dev", None, Some("session-other"), None, None, "human").unwrap_err().to_string();
        assert_eq!(error, "An agent session cannot act as the owner");
        let error = parent_caller(&parent, "dev", None, None, None, Some("run-other"), "human").unwrap_err().to_string();
        assert_eq!(error, "An agent session cannot act as the owner");
        let error = parent_caller(&parent, "dev", None, None, Some("HGS"), None, "human")
            .unwrap_err()
            .to_string();
        assert!(error.contains("could not verify"), "{error}");
        // A session record is checked against the parent team: the assignee
        // and the parent lead are accepted, a stranger is refused.
        let dev = json!({"name":"codex/x/dev","run_id":"r1"});
        assert_eq!(parent_caller(&parent, "dev", Some(&dev), None, Some(""), None, "").unwrap(), ParentCaller::Assignee);
        let lead = json!({"name":"claude/x/lead","run_id":"r0"});
        assert_eq!(parent_caller(&parent, "dev", Some(&lead), None, None, None, "").unwrap(), ParentCaller::ParentLead);
        let stranger = json!({"name":"codex/x/other","run_id":"r9"});
        assert!(parent_caller(&parent, "dev", Some(&stranger), None, None, None, "").is_err());
    }

    #[test]
    fn a_flat_team_tree_check_reads_no_teams_and_a_child_reads_by_id() {
        let flat = running_team();
        assert!(flat.parent.is_none() && flat.children.is_empty());
        let mut reads = 0;
        let loaded = tree_around(&flat, &mut |_| {
            reads += 1;
            Ok(None)
        })
        .unwrap();
        assert_eq!(reads, 0, "a flat team reads nothing extra");
        assert_eq!(loaded.len(), 1);
        // A parent with one child reads that child by id, never the full list.
        let mut parent = flat.clone();
        parent.children = vec!["child".into()];
        let mut child = Team { id: "child".into(), name: "Child".into(), lead: "dev".into(), ..Default::default() };
        child.parent = Some(Parent { team: parent.id.clone(), task: 1, lead: "lead".into(), member: "dev".into() });
        let mut reads = 0;
        let loaded = tree_around(&parent, &mut |id| {
            reads += 1;
            Ok((id == "child").then(|| child.clone()))
        })
        .unwrap();
        assert_eq!(reads, 1);
        assert_eq!(loaded.len(), 2);
    }

    #[test]
    fn status_shows_the_budget_state_on_the_member() {
        let mut team = running_team();
        let now = super::super::super::now();
        let since = now - 40.0 * 60.0;
        team.members.iter_mut().find(|member| member.id == "dev").unwrap().budget = Some(Budget {
            label: "install".into(),
            minutes: 15,
            red_minutes: 30,
            declared_at: since,
            job: Some(JobRef { command: "npm install".into(), since }),
            ..Default::default()
        });
        let value = view_with(&team, &[], None);
        let dev = value["members"].as_array().unwrap().iter().find(|member| member["id"] == "dev").unwrap();
        assert_eq!(dev["budget"]["state"], "red");
        assert_eq!((dev["budget"]["label"].as_str(), dev["budget"]["elapsed_minutes"].as_u64()), (Some("install"), Some(40)));
        assert_eq!(dev["budget"]["command"], "npm install");
        // Amber between the estimate and the red time.
        team.members.iter_mut().find(|member| member.id == "dev").unwrap().budget.as_mut().unwrap().job.as_mut().unwrap().since =
            now - 20.0 * 60.0;
        let value = view_with(&team, &[], None);
        let dev = value["members"].as_array().unwrap().iter().find(|member| member["id"] == "dev").unwrap();
        assert_eq!(dev["budget"]["state"], "amber");
    }

    #[test]
    fn a_draft_phase_can_run_alongside_a_running_phase() {
        let mut team = running_team();
        // F12/N40: phase 1 still runs, and the draft after it says it can run
        // alongside - not that it waits; the owner is warned by the desktop.
        team.tasks.push(Task { id: 3, title: "Fix".into(), phase: 2, status: TaskStatus::Draft, assignee: "dev".into(), ..Default::default() });
        team.phases.push(Phase {
            n: 2,
            order: 2,
            title: "Fixes".into(),
            approval: Approval::Review,
            state: PhaseState::Draft,
            tasks: vec![3],
            ..Default::default()
        });
        let value = view_with(&team, &[], None);
        let phase = &value["phases"][0];
        assert_eq!(phase["waiting"]["text"], "Can run alongside phase 1");
        assert_eq!(phase["waiting"]["alongside"], true);
        assert_eq!((phase["waiting"]["phase"].as_u64(), phase["waiting"]["open"].clone()), (Some(1), json!([1])));
        // The server's rule is unchanged: a draft opens when no earlier phase is
        // a draft, so this one can be approved while phase 1 still runs (it is
        // queued until the dispatcher hands it out).
        let mut approved = team.clone();
        let mut events = Vec::new();
        ops::apply(&mut approved, "approve-phase", &json!({"phase":2}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
        assert_eq!(approved.phases[0].state, PhaseState::Queued);
        // Once the previous phase is decided, the draft is simply a draft.
        team.task_mut(1).unwrap().status = TaskStatus::Accepted;
        let value = view_with(&team, &[], None);
        assert!(value["phases"][0].get("waiting").is_none(), "{value}");
        // A second draft waits for the first draft by its number.
        team.phases.push(Phase {
            n: 3,
            order: 3,
            title: "Later".into(),
            approval: Approval::Review,
            state: PhaseState::Draft,
            tasks: vec![],
            ..Default::default()
        });
        // F12: a draft behind a draft says it waits for the approval, not for
        // readiness.
        let value = view_with(&team, &[], None);
        assert_eq!(value["phases"][1]["waiting"]["phase"], 2);
        assert_eq!(value["phases"][1]["waiting"]["open"], json!([3]));
        assert_eq!(value["phases"][1]["waiting"]["text"], "Waiting for the previous phase to be approved");
        assert!(value["phases"][1]["waiting"].get("alongside").is_none(), "{value}");
        // The approval refusal stays specific about which phase is in the way.
        let mut events = Vec::new();
        let error = ops::apply(&mut team, "approve-phase", &json!({"phase":3}), &Actor::Human, &BTreeSet::new(), &mut events)
            .unwrap_err()
            .to_string();
        assert_eq!(error, "Phase 3 opens after the previous phase is approved");
    }

    #[test]
    fn the_commands_that_create_work_start_the_worker() {
        for op in ["approve", "resume", "extend", "approve-phase", "ask", "reply", "release", "expect"] {
            assert!(starts_worker(op), "{op}");
        }
        for op in ["report", "cancel", "finish", "discard-phase", "hold"] {
            assert!(!starts_worker(op), "{op}");
        }
    }

    #[test]
    fn a_draft_and_phase_edits_keep_their_own_revision() {
        // C1: "revision" of these operations is not the team's revision, so a
        // change elsewhere in the team never locks the draft or the phases.
        for op in ["draft", "edit-draft", "move-phase", "discard-phase", "extend"] {
            assert_eq!(expected_revision(op, &json!({"revision":3})), None, "{op}");
        }
        assert_eq!(expected_revision("plan", &json!({"revision":3})), Some(3));
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let (mut team, _) = ops::create("d", &json!({"name":"D","lead":{"id":"lead","agent":"claude"}})).unwrap();
        team.status = TeamStatus::Draft;
        team.draft = Some(Draft { revision: 1, ..Default::default() });
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
        // The owner marks a field seen: the team revision moves, the draft's not.
        let seen = json!({"action":"seen","fields":["goal"]});
        store
            .mutate(None, "d", expected_revision("draft", &seen), |team, events| ops::apply(team, "draft", &seen, &Actor::Human, &BTreeSet::new(), events))
            .unwrap();
        let update = json!({"action":"update","revision":1,"set":{"goal":"Ship"}});
        let receipt = store
            .mutate(Some(("22222222-2222-4222-8222-222222222222", &update)), "d", expected_revision("draft", &update), |team, events| {
                ops::apply(team, "draft", &update, &Actor::Member("lead".into()), &BTreeSet::new(), events)
            })
            .unwrap();
        // The reply names the draft's revision apart from the team's.
        assert_eq!(receipt["draft_revision"], 2);
        assert_ne!(receipt["revision"], receipt["draft_revision"]);
        let again = json!({"action":"update","revision":2,"set":{"goal":"Ship it"}});
        store
            .mutate(None, "d", expected_revision("draft", &again), |team, events| {
                ops::apply(team, "draft", &again, &Actor::Member("lead".into()), &BTreeSet::new(), events)
            })
            .unwrap();
        assert_eq!(store.load("d").unwrap().unwrap().goal, "Ship it");
    }

    #[test]
    fn only_the_owner_outside_every_session_starts_a_draft() {
        // C2: no team exists yet to prove a member by.
        let owner = json!({"by":"human"});
        owner_without_team(&owner, false, false, false, false).unwrap();
        for (record, dsh, env, run) in [(true, false, false, false), (false, true, false, false), (false, false, true, false), (false, false, false, true)] {
            assert!(owner_without_team(&owner, record, dsh, env, run).is_err(), "{record} {dsh} {env} {run}");
        }
        assert!(owner_without_team(&json!({}), false, false, false, false).is_err());
    }

    /// U57: the polled list carries the open proposals with their change and
    /// hash for the lead's cards, never a decided one or the replay request id.
    #[test]
    fn the_team_list_carries_the_open_proposals_for_their_cards() {
        let mut team = running_team();
        let lead = Actor::Member("lead".into());
        for input in [
            json!({"kind":"cancel-tasks","reason":"drop","tasks":[2]}),
            json!({"kind":"discard-phase","reason":"nothing started","phase":2}),
        ] {
            ops::apply(&mut team, "propose", &input, &lead, &BTreeSet::new(), &mut Vec::new()).unwrap();
        }
        team.proposals[1]["state"] = json!("rejected");
        let listed = summary(&team, false);
        let proposals = listed["proposals"].as_array().unwrap();
        assert_eq!(proposals.len(), 1, "{listed}");
        assert_eq!(proposals[0]["id"], "p1");
        assert_eq!(proposals[0]["change"]["tasks"], json!([2]));
        assert_eq!(proposals[0]["hash"], team.proposals[0]["hash"]);
        assert!(proposals[0].get("operation_request").is_none(), "{}", proposals[0]);
    }

    /// U57: Allow applies exactly the proposed change once, bound to the id and
    /// hash; a changed card is refused; Deny records the reason for the lead.
    #[test]
    fn an_allow_applies_the_proposed_change_once_and_a_changed_card_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let mut team = running_team();
        ops::apply(
            &mut team,
            "propose",
            &json!({"kind":"add-member","reason":"needs help",
                "members":[{"id":"x","role":"Ops","agent":"claude"},{"id":"y","role":"Ops","agent":"claude"}]}),
            &Actor::Member("lead".into()),
            &BTreeSet::new(),
            &mut Vec::new(),
        )
        .unwrap();
        let hash = team.proposals[0]["hash"].as_str().unwrap().to_owned();
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
        let error = decide_proposal_in(
            &mut store,
            &json!({"team":"t","proposal":"p1","accept":true,"by":"human","expected_hash":"bad"}),
            &Actor::Human,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("changed"), "{error}");
        let receipt = decide_proposal_in(&mut store, &json!({"team":"t","proposal":"p1","accept":true,"by":"human","expected_hash":hash}), &Actor::Human).unwrap();
        assert_eq!(receipt["status"], "accepted");
        let team = store.load("t").unwrap().unwrap();
        assert!(team.member("x").is_some() && team.member("y").is_some());
        // A replay returns the stored state; no member is added twice.
        let replay = decide_proposal_in(&mut store, &json!({"team":"t","proposal":"p1","accept":true,"by":"human","expected_hash":hash}), &Actor::Human).unwrap();
        assert_eq!(replay["status"], "accepted");
        assert_eq!(store.load("t").unwrap().unwrap().members.iter().filter(|member| member.id == "x").count(), 1);
        // Deny records its reason for the lead and runs nothing.
        store
            .mutate(None, "t", None, |team, events| {
                ops::apply(team, "propose", &json!({"kind":"cancel-tasks","reason":"drop","tasks":[2]}), &Actor::Member("lead".into()), &BTreeSet::new(), events)
            })
            .unwrap();
        let team = store.load("t").unwrap().unwrap();
        let hash = team.proposals[1]["hash"].as_str().unwrap().to_owned();
        let before = team.task(1).unwrap().status;
        let denied = decide_proposal_in(
            &mut store,
            &json!({"team":"t","proposal":"p2","accept":false,"reason":"not now","expected_hash":hash}),
            &Actor::Human,
        )
        .unwrap();
        assert_eq!(denied["status"], "rejected");
        let team = store.load("t").unwrap().unwrap();
        assert_eq!(team.proposals[1]["note"], "not now");
        assert_eq!(team.task(1).unwrap().status, before, "a Deny runs nothing");
    }

    /// U57: Allow on an add-to-phase card for the running phase adds the tasks
    /// at once: the Allow is the confirmation the CLI route asks for after its
    /// warning (owner, 2026-10-30: the card was accepted and nothing was added).
    #[test]
    fn an_allowed_addition_to_the_running_phase_adds_its_tasks() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let mut team = running_team();
        let input = json!({"kind":"add-to-phase","reason":"a crash to hunt","phase":1,
            "tasks":[{"id":2,"title":"Find the crash","assignee":"dev","criteria":["Fixed"]}]});
        ops::apply(&mut team, "propose", &input, &Actor::Member("lead".into()), &BTreeSet::new(), &mut Vec::new()).unwrap();
        let hash = team.proposals[0]["hash"].as_str().unwrap().to_owned();
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
        let receipt = decide_proposal_in(
            &mut store,
            &json!({"team":"t","proposal":"p1","accept":true,"by":"human","expected_hash":hash}),
            &Actor::Human,
        )
        .unwrap();
        assert_eq!(receipt["status"], "accepted", "{receipt}");
        let team = store.load("t").unwrap().unwrap();
        let added = team.task(2).expect("the allowed task is in the plan");
        assert_eq!((added.phase, added.title.as_str()), (1, "Find the crash"));
    }

    /// U59: the desktop's switch dialog decides through decide-proposal; its
    /// Allow switches the plan mode once, and a Deny keeps it and tells the lead.
    #[test]
    fn the_switch_dialog_switches_the_plan_mode_through_decide_proposal() {
        use super::super::model::PlanMode;
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let mut team = running_team();
        let lead = Actor::Member("lead".into());
        for reason in ["short requests", "a stream"] {
            let input = json!({"kind":"switch-plan-mode","mode":"free","reason":reason});
            ops::apply(&mut team, "propose", &input, &lead, &BTreeSet::new(), &mut Vec::new()).unwrap();
        }
        let first = team.proposals[0]["hash"].as_str().unwrap().to_owned();
        let second = team.proposals[1]["hash"].as_str().unwrap().to_owned();
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
        // A guessed field never decides: no "accept" is an error, not a rejection.
        let error = decide_proposal_in(&mut store, &json!({"team":"t","proposal":"p1","decision":"allow","hash":first}), &Actor::Human)
            .unwrap_err()
            .to_string();
        assert!(error.contains("accept") && error.contains("nothing was decided"), "{error}");
        assert_eq!(store.load("t").unwrap().unwrap().proposals[0]["state"], "open");
        let denied = decide_proposal_in(
            &mut store,
            &json!({"team":"t","proposal":"p1","accept":false,"reason":"not yet","expected_hash":first}),
            &Actor::Human,
        )
        .unwrap();
        assert_eq!(denied["status"], "rejected");
        assert_eq!(store.load("t").unwrap().unwrap().plan_mode, PlanMode::Standard);
        let feed = store.events("t", None, 50).unwrap().to_string();
        assert!(feed.contains("plan_mode_denied") && feed.contains("not yet"), "{feed}");
        let receipt = decide_proposal_in(
            &mut store,
            &json!({"team":"t","proposal":"p2","accept":true,"by":"human","hash":second}),
            &Actor::Human,
        )
        .unwrap();
        assert_eq!(receipt["status"], "accepted", "{receipt}");
        assert_eq!(receipt["result"]["plan_mode"], "free");
        assert_eq!(store.load("t").unwrap().unwrap().plan_mode, PlanMode::Free);
        assert!(store.events("t", None, 50).unwrap().to_string().contains("plan_mode_switched"));
    }

    /// U56: status shows each member's phase, activity and minutes without
    /// progress, and whether it is stuck, from the dispatcher's last view.
    #[test]
    fn status_shows_each_members_phase_activity_and_minutes_without_progress() {
        use super::super::model::MemberStuck;
        let mut team = running_team();
        let dev = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
        dev.seen_phase = "tool".into();
        dev.seen_activity = "busy".into();
        dev.seen_event_at = Some(crate::state::now() - 25.0 * 60.0);
        dev.stuck = Some(MemberStuck { at: crate::state::now() - 15.0 * 60.0, reason: "no_progress".into(), phase: "tool".into() });
        let value = view_with(&team, &[], None);
        let dev = value["members"].as_array().unwrap().iter().find(|member| member["id"] == "dev").unwrap();
        assert_eq!((dev["phase"].as_str(), dev["activity"].as_str()), (Some("tool"), Some("busy")));
        assert_eq!(dev["minutes_without_progress"], 25);
        assert_eq!(dev["stuck"]["reason"], "no_progress");
    }

    #[test]
    fn an_accepted_swap_proposal_runs_with_its_fields() {
        // C8: the spec's shape (member id plus section 5 fields) is kept by
        // propose and run by the owner's decision without a panic.
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let mut team = running_team();
        let proposal = json!({"kind":"swap-member","member":"dev","agent":"claude","model":"opus","reason":"Stuck three times"});
        ops::apply(&mut team, "propose", &proposal, &Actor::Member("lead".into()), &BTreeSet::new(), &mut Vec::new()).unwrap();
        assert_eq!(team.proposals[0]["member"], json!({"id":"dev","agent":"claude","model":"opus"}));
        let hash = team.proposals[0]["hash"].as_str().unwrap().to_owned();
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
        let decide = json!({"team":"t","proposal":"p1","accept":true,"by":"human","expected_hash":hash});
        let error = decide_proposal_in(&mut store, &decide, &Actor::Member("lead".into())).unwrap_err().to_string();
        assert!(error.contains("Only you decide"), "{error}");
        let receipt = decide_proposal_in(&mut store, &decide, &Actor::Human).unwrap();
        assert_eq!((receipt["status"].as_str(), receipt["kind"].as_str()), (Some("accepted"), Some("swap-member")));
        let team = store.load("t").unwrap().unwrap();
        let dev = team.member("dev").unwrap();
        assert_eq!((dev.agent.as_str(), dev.model.as_str(), dev.session.is_none()), ("claude", "opus", true));
        assert_eq!(team.proposals[0]["state"], "accepted");
        assert!(team.proposals[0]["decided_at"].as_f64().is_some());
        // An old proposal stored with a bare member id runs as well.
        assert_eq!(proposal_input("t", "swap-member", &json!("dev"), "11111111-1111-4111-8111-111111111111").unwrap()["member"], "dev");
    }

    #[test]
    fn the_parent_lead_is_proven_in_the_parent_team_by_record_or_dsh() {
        // C6: never by a sub-team member that carries the parent lead's id.
        let mut parent = running_team();
        parent.id = "parent".into();
        parent.members[0].session.as_mut().unwrap().conversation_id = "dsh-lead".into();
        let mut child = running_team();
        child.id = "child".into();
        child.parent = Some(Parent { team: "parent".into(), task: 1, lead: "lead".into(), member: "dev".into() });
        let lead = json!({"name":"claude/x/lead","run_id":"r0"});
        let stranger = json!({"name":"claude/x/other","run_id":"r9"});
        // The sub-team's own lead runs in the same session as the parent's in
        // running_team(); a distinct sub-lead shows the proof.
        child.members[0].session = Some(SessionRef { name: "claude/x/sublead".into(), run_id: "r5".into(), ..Default::default() });
        assert!(parent_lead_proven(&child, Some(&parent), Some(&lead), None));
        assert!(!parent_lead_proven(&child, Some(&parent), Some(&stranger), None));
        assert!(parent_lead_proven(&child, Some(&parent), None, Some("dsh-lead")));
        assert!(!parent_lead_proven(&child, Some(&parent), None, Some("dsh-other")));
        assert!(!parent_lead_proven(&child, None, Some(&lead), None));
        let mut unlinked = child.clone();
        unlinked.parent = None;
        assert!(!parent_lead_proven(&unlinked, Some(&parent), Some(&lead), None));
    }

    #[test]
    fn an_accepted_next_goal_runs_as_the_owner() {
        // N2: the decision does not re-derive its caller from the input.
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let mut team = running_team();
        ops::apply(&mut team, "propose", &json!({"kind":"next-goal","goal":"Ship v2","reason":"Done here"}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut Vec::new()).unwrap();
        let hash = team.proposals[0]["hash"].as_str().unwrap().to_owned();
        team.status = TeamStatus::Done;
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
        let receipt = decide_proposal_in(&mut store, &json!({"team":"t","proposal":"p1","accept":true,"by":"human","expected_hash":hash}), &Actor::Human).unwrap();
        let next = receipt["result"]["team"].as_str().unwrap().to_owned();
        assert_eq!(store.load(&next).unwrap().unwrap().goal, "Ship v2");
        assert_eq!(store.load("t").unwrap().unwrap().next, next);
    }

    /// U51: the owner's key adds one bounded urgent phase and nothing else.
    #[test]
    fn an_intake_key_adds_one_urgent_phase_and_no_other_operation() {
        let (team, _, _) = review_team();
        with_review_team(&team, || {
            // A team with none of the new fields behaves as today.
            let before = stored();
            assert!(before.intakes.is_empty() && before.intakes_seq == 0, "{before:?}");
            assert!(!before.settings.restart_crashed);
            assert!(before.members.iter().all(|member| member.restarts.is_empty() && member.crash.is_none() && !member.crash_told));
            let request = "11111111-2222-4333-8444-555555555551";
            let created = run("intake-key", &json!({"request_id":request,"team":"rv","label":"health watch","by":"human"}), false).unwrap();
            assert_eq!((created["status"].clone(), created["id"].clone()), (json!("created"), json!("i1")));
            let key = created["key"].as_str().unwrap().to_owned();
            assert!(!key.is_empty());
            let saved = stored();
            assert_eq!(saved.intakes.len(), 1);
            assert_eq!(saved.intakes[0].label, "health watch");
            assert_eq!(saved.intakes[0].hash, ops::intake_hash(&key));
            assert_ne!(saved.intakes[0].hash, key);
            // A replay returns its first receipt, and the key is not shown again.
            let again = run("intake-key", &json!({"request_id":request,"team":"rv","label":"health watch","by":"human"}), false).unwrap();
            assert_eq!(again["status"], json!("created"));
            assert!(again.get("key").is_none(), "{again}");
            // The key adds one urgent phase: barrier off, approved at once.
            let added = call("intake", json!({"key":key,"tasks":[{"title":"Hotfix","assignee":"dev"},{"title":"Probe","assignee":"qa"}]})).unwrap();
            assert_eq!(added["status"], json!("intake"));
            let saved = stored();
            let phase = saved.phases.last().unwrap();
            assert!(phase.urgent && !phase.barrier, "{phase:?}");
            assert_eq!(phase.approval, Approval::Auto);
            assert_eq!(phase.title, "Urgent: health watch");
            assert_eq!(phase.state, PhaseState::Approved);
            assert_eq!(phase.tasks, vec![4, 5]);
            // The feed names the key's label, never the key.
            let feed = Store::open(false).unwrap().events("rv", None, 50).unwrap().to_string();
            assert!(feed.contains("Intake health watch"), "{feed}");
            assert!(!feed.contains(&key), "{feed}");
            // Bounded: one to five tasks, and never a named phase.
            for tasks in [vec![json!({"title":"A","assignee":"dev"}); 6], Vec::new()] {
                let error = call("intake", json!({"key":key,"tasks":tasks})).unwrap_err();
                assert!(error.to_string().contains("1 to 5"), "{error}");
            }
            let error = call("intake", json!({"key":key,"phase":2,"tasks":[{"title":"X","assignee":"dev"}]})).unwrap_err();
            assert!(error.to_string().contains("cannot name"), "{error}");
            // The key opens no other operation.
            for op in ["status", "inbox", "extend", "plan", "report", "accept", "settings", "intake-key", "relaunch", "stop"] {
                let error = call(op, json!({"key":key})).unwrap_err();
                assert!(error.to_string().contains("intake key adds urgent tasks only"), "{op}: {error}");
            }
            // A member cannot create a key; the owner revokes it by id.
            let denied = as_lead(|| {
                run("intake-key", &json!({"request_id":uuid::Uuid::new_v4().to_string(),"team":"rv","label":"x","by":"human"}), false)
            })
            .unwrap_err();
            assert!(denied.to_string().contains("Only you"), "{denied}");
            let revoked = call("intake-key", json!({"revoke":"i1","by":"human"})).unwrap();
            assert_eq!(revoked["status"], json!("revoked"));
            assert!(stored().intakes.is_empty());
            let after = call("intake", json!({"key":key,"tasks":[{"title":"Late","assignee":"dev"}]})).unwrap_err();
            assert!(after.to_string().contains("Unknown or revoked"), "{after}");
            // Without a key nothing changes.
            let error = call("intake", json!({"tasks":[{"title":"Late","assignee":"dev"}]})).unwrap_err();
            assert!(error.to_string().contains("carries the owner's key"), "{error}");
        });
    }

    /// #581 N4/L3: an intake task keeps only the plan fields the key is given,
    /// and a key has at most one open urgent phase.
    #[test]
    fn an_intake_task_keeps_only_its_plan_fields_and_one_phase_is_open_per_key() {
        use crate::state::teams::model::TaskKind;
        let (team, _, _) = review_team();
        with_review_team(&team, || {
            let created = run(
                "intake-key",
                &json!({"request_id":"11111111-2222-4333-8444-555555555552","team":"rv","label":"watch","by":"human"}),
                false,
            )
            .unwrap();
            let key = created["key"].as_str().unwrap().to_owned();
            let added = call(
                "intake",
                json!({"key":key,"tasks":[{
                    "title":"Fix","description":"d","criteria":["c"],"assignee":"dev",
                    "pipeline":"none","pinned":true,"phase":9,"models":["m"],"kind":"check",
                    "check":{"command":"true"},"deps":[{"task":1}],"lane":{"id":"x"},"files":["a"]}]}),
            )
            .unwrap();
            let saved = stored();
            let task = saved.task(added["tasks"][0].as_u64().unwrap() as u32).unwrap();
            assert_eq!((task.title.as_str(), task.description.as_str()), ("Fix", "d"));
            assert_eq!(task.criteria, vec!["c"]);
            assert_eq!(task.assignee, "dev");
            // The owner's plan fields are not the key's; the pipeline resolves
            // as for a plan task with no choice.
            assert!(task.pipeline.is_empty(), "pipeline: {}", task.pipeline);
            assert!(!task.pinned && task.models.is_empty() && task.lane.is_none() && task.check.is_none());
            assert!(task.deps.is_empty() && task.kind == TaskKind::Task);
            assert_eq!(task.phase, saved.phases.last().unwrap().n);
            // L3: the key's phase is still open, so it adds no second one.
            let error = call("intake", json!({"key":key,"tasks":[{"title":"More","assignee":"dev"}]})).unwrap_err();
            assert!(error.to_string().contains("still open"), "{error}");
            // Once its work is decided, the key may add another urgent phase.
            for id in added["tasks"].as_array().unwrap() {
                let id = id.as_u64().unwrap() as u32;
                call("cancel", json!({"task":id,"by":"human"})).unwrap();
            }
            let again = call("intake", json!({"key":key,"tasks":[{"title":"Later","assignee":"dev"}]})).unwrap();
            assert_eq!(again["status"], json!("intake"));
        });
    }

    /// U61 #709 N2: start-ahead is a CLI mutation for the lead and the owner.
    #[test]
    fn the_lead_or_the_owner_starts_an_auto_draft_ahead_through_the_cli() {
        use crate::state::teams::model::{Approval, Phase, PhaseState, Task, TaskStatus};
        let (mut team, _, _) = review_team();
        team.settings.auto_accept = true;
        team.phases.push(Phase {
            n: 9,
            order: 9,
            title: "Next".into(),
            approval: Approval::Auto,
            state: PhaseState::Draft,
            tasks: vec![99],
            ..Default::default()
        });
        team.tasks.push(Task { id: 99, title: "Next".into(), phase: 9, status: TaskStatus::Draft, assignee: "dev".into(), ..Default::default() });
        with_review_team(&team, || {
            // The lead, through the CLI's mutation list.
            let started = as_lead(|| call("start-ahead", json!({"phase":9}))).unwrap();
            assert_eq!(started["start_ahead"], json!(true));
            assert!(stored().phases.iter().find(|phase| phase.n == 9).unwrap().start_ahead);
            // The owner undoes it before any hand-out.
            let undone = call("start-ahead", json!({"phase":9,"undo":true,"by":"human"})).unwrap();
            assert_eq!(undone["status"], json!("undone"));
            // The owner starts it too.
            let again = call("start-ahead", json!({"phase":9,"by":"human"})).unwrap();
            assert_eq!(again["status"], json!("started"));
        });
    }

    /// #728: the lead relaunches a stuck member on the owner's setting without a
    /// word; the owner has the same route; without the setting the word is still
    /// asked.
    #[test]
    fn the_lead_relaunches_a_stuck_member_with_lead_restarts_stuck() {
        use crate::state::teams::model::MemberStuck;
        let (mut team, _, _) = review_team();
        team.settings.lead_restarts_stuck = true;
        team.members.iter_mut().find(|member| member.id == "dev").unwrap().stuck =
            Some(MemberStuck { at: 1.0, reason: "no_progress".into(), phase: "tool".into() });
        with_review_team(&team, || {
            let receipt = as_lead(|| call("relaunch", json!({"member":"dev"}))).unwrap();
            assert_eq!(receipt["status"], json!("relaunching"));
            assert!(stored().member("dev").unwrap().restart_pending);
            // The owner takes the same route.
            let owner = call("relaunch", json!({"member":"dev","by":"human"})).unwrap();
            assert_eq!(owner["status"], json!("relaunching"));
        });
        // Without the setting the lead still needs the owner's word.
        let (mut strict, _, _) = review_team();
        strict.members.iter_mut().find(|member| member.id == "dev").unwrap().stuck =
            Some(MemberStuck { at: 1.0, reason: "no_progress".into(), phase: "tool".into() });
        with_review_team(&strict, || {
            let error = as_lead(|| call("relaunch", json!({"member":"dev"}))).unwrap_err().to_string();
            assert!(error.contains("owner_word"), "{error}");
        });
    }

    /// U61/U52: a team stored before this phase carries none of the new fields,
    /// loads with today's defaults and writes none of them back.
    #[test]
    fn a_stored_team_without_the_new_fields_loads_and_behaves_as_today() {
        let (team, _, _) = review_team();
        let mut value = serde_json::to_value(&team).unwrap();
        for key in ["intakes", "intakes_seq"] {
            value.as_object_mut().unwrap().remove(key);
        }
        value["settings"].as_object_mut().unwrap().remove("restart_crashed");
        for member in value["members"].as_array_mut().unwrap() {
            for key in ["restarts", "crash", "crash_told"] {
                member.as_object_mut().unwrap().remove(key);
            }
        }
        let mut legacy: Team = serde_json::from_value(value).unwrap();
        assert!(legacy.intakes.is_empty() && legacy.intakes_seq == 0);
        assert!(!legacy.settings.restart_crashed);
        assert!(legacy.members.iter().all(|member| member.restarts.is_empty() && member.crash.is_none() && !member.crash_told));
        // The same plan, hand-out and settings work; none of the new fields is
        // written for a team that does not set them.
        let mut events = Vec::new();
        ops::apply(&mut legacy, "settings", &json!({"settings":{"restart_crashed":true}}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
        assert!(legacy.settings.restart_crashed);
        let refused = ops::apply(&mut legacy, "settings", &json!({"settings":{"restart_crashed":false}}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut events).unwrap_err();
        assert!(refused.to_string().contains("restart_crashed"), "{refused}");
        legacy.settings.restart_crashed = false;
        let written = serde_json::to_value(&legacy).unwrap();
        assert!(written.get("intakes").is_none() && written.get("intakes_seq").is_none(), "{written}");
        assert!(written["settings"].get("restart_crashed").is_none(), "{written}");
        assert!(written["members"].as_array().unwrap().iter().all(|member| member.get("restarts").is_none()
            && member.get("crash").is_none() && member.get("crash_told").is_none()), "{written}");
    }
}
