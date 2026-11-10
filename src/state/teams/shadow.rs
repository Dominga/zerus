//! Task 173 (owner, 2026-11-07): no shadow work. Every piece of a member's work
//! belongs to a handed-out attempt (or the stage run it was given).
//!
//! - The lead's message to a member is attached to the attempt of each task it
//!   names (`#145`, `task 145`, `for 145:`) that is handed to that member.
//!   Task #30 (team ede724c4, 2026-11-10): a task the text only mentions and
//!   the member does not hold is a reference, never refused (it is named back
//!   as one); only a message whose `"task"` field names a task the member does
//!   not hold is refused, naming the hand-out (retry, reassign) to use.
//! - A member's `hgs run` needs an open attempt (or stage run) of its own; a
//!   run that names another attempt is refused.
//! - A member session building, or working past a short grace, with nothing
//!   handed to it is flagged to the lead as unassigned work, with what it
//!   runs, and is never shown as idle.
use super::model::{Member, MemberState, StageState, Task, Team, TeamStatus, Unassigned};
use super::store::{Event, Store};
use serde_json::{json, Value};

/// A member working this long with nothing handed to it is flagged (a reply
/// to the lead's free-form message, or the rest of a turn after its report,
/// stays within it). A build is flagged at once.
pub(crate) const WORK_GRACE: f64 = 120.0;
/// At most this many lead messages are kept on an attempt, each this long.
const MESSAGES_KEPT: usize = 20;
const MESSAGE_CHARS: usize = 500;

/// A lead message bound to the attempt it concerns.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Binding {
    pub team: String,
    pub task: u32,
    pub attempt: String,
    pub member: String,
}

/// The team's task ids a text names: `#N`, `task N`, `task #N`, and the
/// `for N:` the shadow messages used. Other numbers are not tasks.
pub(crate) fn task_ids(team: &Team, text: &str) -> Vec<u32> {
    let lower = text.to_lowercase();
    let chars: Vec<char> = lower.chars().collect();
    let mut found: Vec<u32> = Vec::new();
    let number_at = |start: usize| -> Option<(u32, usize)> {
        let digits: String = chars[start..].iter().take_while(|ch| ch.is_ascii_digit()).collect();
        if digits.is_empty() || digits.len() > 6 {
            return None;
        }
        // Part of a longer word or number ("#12a", "3.5") is no task id.
        let end = start + digits.len();
        if chars.get(end).is_some_and(|ch| ch.is_alphanumeric() || *ch == '.' && chars.get(end + 1).is_some_and(char::is_ascii_digit)) {
            return None;
        }
        digits.parse().ok().map(|n| (n, end))
    };
    let mut index = 0;
    while index < chars.len() {
        let before_ok = index == 0 || !chars[index - 1].is_alphanumeric();
        let rest: String = chars[index..chars.len().min(index + 6)].iter().collect();
        let mut push = |n: u32| {
            if team.task(n).is_some() && !found.contains(&n) {
                found.push(n);
            }
        };
        if chars[index] == '#' {
            if let Some((n, _)) = number_at(index + 1) {
                push(n);
            }
        } else if before_ok && (rest.starts_with("task ") || rest.starts_with("tasks ")) {
            let mut at = index + if rest.starts_with("tasks ") { 6 } else { 5 };
            if chars.get(at) == Some(&'#') {
                at += 1;
            }
            if let Some((n, _)) = number_at(at) {
                push(n);
            }
        } else if before_ok && rest.starts_with("for ") {
            let mut at = index + 4;
            if chars.get(at) == Some(&'#') {
                at += 1;
            }
            if let Some((n, end)) = number_at(at) {
                if chars.get(end) == Some(&':') {
                    push(n);
                }
            }
        }
        index += 1;
    }
    found
}

/// The open work handed to a member: its open attempt, or the stage run it was
/// given, as (task, id).
pub(crate) fn handed(team: &Team, member: &str) -> Vec<(u32, String)> {
    let mut work = Vec::new();
    for task in &team.tasks {
        if let Some(attempt) = task.open_attempt().filter(|attempt| attempt.member == member) {
            work.push((task.id, attempt.id.clone()));
        }
        if let Some(run) = task.pipeline_run.as_ref().and_then(|pipeline| pipeline.open_run()) {
            if run.member == member && run.state == StageState::Delivered {
                work.push((task.id, run.id.clone()));
            }
        }
    }
    work
}

/// What a lead message to a member carries: the attempts it binds to, and the
/// tasks its text mentions for reference only (not held by the member).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct LeadCheck {
    pub bindings: Vec<Binding>,
    pub references: Vec<u32>,
}

/// The lead's message to `member`: bound to the attempt of each task it names
/// that the member holds. Task #30 (team ede724c4): a task the text mentions
/// and the member does not hold is a reference, never refused; only `task`
/// (the message's explicit task) naming one the member does not hold is
/// refused, with the hand-out to use.
pub(crate) fn check_lead_message(team: &Team, member: &Member, text: &str, task: Option<u32>) -> anyhow::Result<LeadCheck> {
    let handed = handed(team, &member.id);
    let mut check = LeadCheck::default();
    let bind = |check: &mut LeadCheck, id: u32, attempt: &str| {
        if !check.bindings.iter().any(|found| found.task == id) {
            check.bindings.push(Binding { team: team.id.clone(), task: id, attempt: attempt.to_owned(), member: member.id.clone() });
        }
    };
    if let Some(id) = task {
        match handed.iter().find(|(found, _)| *found == id) {
            Some((_, attempt)) => bind(&mut check, id, attempt),
            None => refuse(team, member, id)?,
        }
    }
    for id in task_ids(team, text) {
        match handed.iter().find(|(found, _)| *found == id) {
            Some((_, attempt)) => bind(&mut check, id, attempt),
            None => {
                if Some(id) != task && !check.references.contains(&id) {
                    check.references.push(id);
                }
            }
        }
    }
    Ok(check)
}

/// The refusal of a message whose task the member does not hold.
fn refuse(team: &Team, member: &Member, id: u32) -> anyhow::Result<()> {
    let Some(task) = team.task(id) else { anyhow::bail!("Unknown task #{id} in team {}", team.name) };
    {
        let state = task_state(task);
        anyhow::bail!(
            "Task #{id} is not handed to {}: it {state}. A lead message carries no work for a task the member does not hold. Hand it out instead: \
{{\"request_id\":\"UUID\",\"team\":\"{}\",\"task\":{id}}} | hgs team retry --json (its assignee) or hgs team reassign --json with \"to\":\"{}\". \
Answers and decisions go through their own commands (hgs team reply, accept, reject, hold).",
            team.name_of(&member.id),
            team.id,
            member.id
        );
    }
}

/// Task #30: the note a sent message carries for its references.
pub(crate) fn reference_note(references: &[u32]) -> Option<String> {
    if references.is_empty() {
        return None;
    }
    let named: Vec<String> = references.iter().map(|id| format!("#{id}")).collect();
    let (verb, them) = if references.len() == 1 { ("is", "it") } else { ("are", "them") };
    Some(format!("{} {verb} mentioned for reference only: the member does not hold {them}, so the message carries no work for {them}.", named.join(", ")))
}

fn task_state(task: &Task) -> String {
    if task.status.finished() {
        return format!("is {:?}", task.status).to_lowercase();
    }
    match task.open_attempt() {
        Some(attempt) => format!("is handed to {}", attempt.member),
        None => "has no open attempt".into(),
    }
}

/// `hgs send TARGET`: when this process is a team lead's session and TARGET is
/// a member of its team, the text (and its `"task"`, if any) is checked.
/// Nothing else is touched.
pub(crate) fn lead_send(target: &str, text: &str, task: Option<u32>) -> anyhow::Result<LeadCheck> {
    let Some(record) = super::super::session_graph::caller_record() else { return Ok(LeadCheck::default()) };
    let Ok(store) = Store::open(false) else { return Ok(LeadCheck::default()) };
    let teams = store.active_teams(1000).unwrap_or_default();
    let target = target.trim();
    for team in &teams {
        if super::commands::calling_member(team, &record).as_deref() != Some(team.lead.as_str()) {
            continue;
        }
        let Some(member) = team.members.iter().find(|member| {
            member.id != team.lead && member.state != MemberState::Removed && member.session.as_ref().is_some_and(|session| session.name == target)
        }) else {
            continue;
        };
        return check_lead_message(team, member, text, task);
    }
    Ok(LeadCheck::default())
}

/// Records each binding on its attempt (bounded) with a `lead_message` event.
pub(crate) fn record(bindings: &[Binding], text: &str, via: &str) {
    let Some(first) = bindings.first() else { return };
    let Ok(mut store) = Store::open(true) else { return };
    let at = super::super::now();
    let kept: String = text.trim().chars().take(MESSAGE_CHARS).collect();
    let _ = store.mutate(None, &first.team, None, |team, events| {
        attach(team, bindings, &kept, via, at, events);
        Ok(Value::Null)
    });
}

/// Attaches a lead message to the attempts it is bound to.
pub(crate) fn attach(team: &mut Team, bindings: &[Binding], text: &str, via: &str, at: f64, events: &mut Vec<Event>) {
    let team_id = team.id.clone();
    for binding in bindings.iter().filter(|binding| binding.team == team_id) {
        let Some(task) = team.task_mut(binding.task) else { continue };
        if let Some(attempt) = task.attempts.iter_mut().find(|attempt| attempt.id == binding.attempt) {
            attempt.lead_messages.push(json!({"at":at,"via":via,"text":text}));
            let excess = attempt.lead_messages.len().saturating_sub(MESSAGES_KEPT);
            attempt.lead_messages.drain(..excess);
        }
        events.push(Event::new("lead_message", json!({"task":binding.task,"attempt":binding.attempt,"member":binding.member,"via":via})));
    }
}

/// `hgs run` from a session: a team member's job belongs to its handed work.
/// None for a session in no running team, or the lead's; the member's
/// binding; or the refusal.
pub(crate) fn member_job(caller: &Value, named: &str) -> anyhow::Result<Option<Binding>> {
    let Ok(store) = Store::open(false) else { return Ok(None) };
    let teams = store.active_teams(1000).unwrap_or_default();
    for team in teams.iter().filter(|team| matches!(team.status, TeamStatus::Running | TeamStatus::Paused)) {
        let Some(member) = super::commands::calling_member(team, caller) else { continue };
        if member == team.lead {
            return Ok(None);
        }
        return job_of(team, &member, named).map(Some);
    }
    Ok(None)
}

/// The member's job binding: its handed work, or the one `named` among it.
pub(crate) fn job_of(team: &Team, member: &str, named: &str) -> anyhow::Result<Binding> {
    let handed = handed(team, member);
    anyhow::ensure!(
        !handed.is_empty(),
        "No task is handed to you in team {}: ask the lead (hgs team ask) instead of running work; hgs team current shows what is handed to you",
        team.name
    );
    let chosen = if named.is_empty() {
        handed.first()
    } else {
        handed.iter().find(|(_, id)| id == named)
    };
    let Some((task, attempt)) = chosen else {
        let yours: Vec<String> = handed.iter().map(|(task, id)| format!("#{task} (attempt {id})")).collect();
        anyhow::bail!("Attempt {named} is not handed to you; your work is {}. A job for another task is refused: ask the lead", yours.join(", "));
    };
    Ok(Binding { team: team.id.clone(), task: *task, attempt: attempt.clone(), member: member.to_owned() })
}

/// What a member works on with nothing handed to it, or None. A build is
/// unassigned at once; a turn only after `WORK_GRACE` and when it started
/// after the member's last attempt ended (the rest of a reported turn is not).
pub(crate) fn unassigned(team: &Team, member: &Member, found: &super::dispatcher::Seen, now: f64) -> Option<String> {
    if member.id == team.lead || member.state != MemberState::Active || !found.live || !handed(team, &member.id).is_empty() {
        return None;
    }
    // A hand-out being prepared or a model switch owns the member.
    if team.tasks.iter().any(|task| task.assignee == member.id && task.waits.as_ref().is_some_and(super::model::TaskWait::running)) {
        return None;
    }
    if found.building {
        let label = if found.build_label.is_empty() { "a build".to_owned() } else { found.build_label.clone() };
        return Some(format!("builds {label}"));
    }
    if found.free {
        return None;
    }
    let started = found.turn_started?;
    let last_end = team
        .tasks
        .iter()
        .flat_map(|task| task.attempts.iter())
        .filter(|attempt| attempt.member == member.id)
        .map(|attempt| attempt.report.as_ref().map_or(0.0, |report| report.at).max(attempt.decided_at.unwrap_or(0.0)).max(attempt.sent_at))
        .fold(0.0_f64, f64::max);
    if started <= last_end || now - started < WORK_GRACE {
        return None;
    }
    let doing = if found.activity.is_empty() { found.phase.clone() } else { found.activity.clone() };
    Some(format!("works ({}) since {} min", if doing.is_empty() { "busy".to_owned() } else { doing }, ((now - started) / 60.0).round() as i64))
}

/// The lead's line about a member's unassigned work.
pub(crate) fn lead_line(team: &Team, member: &Member, flag: &Unassigned) -> String {
    format!(
        "{} {} with no task handed to it (unassigned work). Hand the task out (retry or reassign) or tell it to stop; never steer work by message.",
        team.name_of(&member.id),
        flag.what
    )
}

#[cfg(test)]
#[path = "tests/shadow.rs"]
mod tests;
