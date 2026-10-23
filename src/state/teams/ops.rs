//! The team state machine. Every change is a pure function of
//! the team, the request and who asks, so the CLI and the dispatcher share it
//! and tests need no processes.
use super::super::now;
use super::model::*;
use super::plan;
use super::store::Event;
use anyhow::{bail, ensure, Context};
use serde_json::{json, Value};
use std::collections::BTreeSet;

/// Who asks for a change.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Actor {
    /// The desktop or a person at the CLI; their decision is final.
    Human,
    /// A team member's session, by member id (the lead included).
    Member(String),
}

impl Actor {
    fn label(&self, team: &Team) -> String {
        match self {
            Actor::Human => "You".into(),
            Actor::Member(id) => team.role_of(id),
        }
    }
    fn is_lead(&self, team: &Team) -> bool {
        matches!(self, Actor::Member(id) if *id == team.lead)
    }
}

fn text<'a>(input: &'a Value, key: &str) -> &'a str {
    input[key].as_str().unwrap_or_default()
}

fn task_id(input: &Value) -> anyhow::Result<u32> {
    input["task"].as_u64().and_then(|id| u32::try_from(id).ok()).filter(|id| *id > 0).context("A task number is required")
}

fn clean(value: &str, max: usize, what: &str) -> anyhow::Result<String> {
    ensure!(value.chars().count() <= max, "{what} is too long");
    ensure!(!value.chars().any(|c| c.is_control() && c != '\n' && c != '\t'), "{what} contains control characters");
    Ok(value.to_owned())
}

fn member_id(value: &str) -> anyhow::Result<String> {
    ensure!(
        !value.is_empty() && value.len() <= 40 && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
        "Member ids use letters, digits, - and _"
    );
    Ok(value.to_owned())
}

fn parse_member(value: &Value) -> anyhow::Result<Member> {
    let mut member: Member = serde_json::from_value(value.clone()).context("Invalid member")?;
    member.id = member_id(&member.id)?;
    member.role = clean(&member.role, 60, "A role")?;
    ensure!(!member.agent.is_empty(), "Member {} needs an agent", member.id);
    member.launch_id.clear();
    member.error.clear();
    member.state = match member.session {
        Some(_) => MemberState::Active,
        None => MemberState::NotCreated,
    };
    Ok(member)
}

fn parse_task(value: &Value) -> anyhow::Result<Task> {
    let mut task: Task = serde_json::from_value(value.clone()).context("Invalid task")?;
    ensure!(task.id > 0, "Task numbers start at 1");
    task.title = clean(&task.title, 200, "A task title")?;
    ensure!(!task.title.trim().is_empty(), "Task #{} needs a title", task.id);
    task.description = clean(&task.description, 20_000, "A task description")?;
    for criterion in &mut task.criteria {
        *criterion = clean(criterion, 500, "An acceptance criterion")?;
    }
    let mut seen = BTreeSet::new();
    task.deps.retain(|dep| seen.insert(dep.task));
    ensure!(task.deps.iter().all(|dep| dep.task != task.id), "Task #{} depends on itself", task.id);
    task.status = TaskStatus::Draft;
    task.attempts.clear();
    task.decided_by = None;
    Ok(task)
}

/// A new team in Planning with its lead.
pub(crate) fn create(id: &str, input: &Value) -> anyhow::Result<(Team, Vec<Event>)> {
    let name = clean(text(input, "name").trim(), 120, "A team name")?;
    ensure!(!name.is_empty(), "A team name is required");
    let mut lead = parse_member(&input["lead"]).context("Invalid lead")?;
    if lead.role.is_empty() {
        lead.role = "Lead".into();
    }
    let settings: Settings = if input["settings"].is_null() {
        Settings::default()
    } else {
        serde_json::from_value(input["settings"].clone()).context("Invalid team settings")?
    };
    ensure!((1..=32).contains(&settings.max_members), "A team has 1 to 32 members");
    let team = Team {
        id: id.to_owned(),
        name: name.clone(),
        goal: clean(text(input, "goal"), 20_000, "The goal")?,
        project: clean(text(input, "project"), 200, "The project")?,
        folder: clean(text(input, "folder"), 4096, "The folder")?,
        template: clean(text(input, "template"), 120, "The template")?,
        status: TeamStatus::Planning,
        settings,
        lead: lead.id.clone(),
        members: vec![lead],
        tasks: Vec::new(),
        ..Default::default()
    };
    let events = vec![Event::new("created", json!({"by":"You","name":name}))];
    Ok((team, events))
}

/// Replaces the plan: members besides the lead, tasks and dependencies, and
/// optionally settings. Only while Planning.
fn plan(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    ensure!(team.status == TeamStatus::Planning, "Only a team in Planning can change its plan");
    ensure!(matches!(actor, Actor::Human) || actor.is_lead(team), "Only the lead or you can change the plan");
    let lead = team.member(&team.lead).cloned().context("The team has no lead")?;
    let mut members = vec![lead];
    for value in input["members"].as_array().into_iter().flatten() {
        let member = parse_member(value)?;
        ensure!(members.iter().all(|m| m.id != member.id), "Member id {} is used twice", member.id);
        members.push(member);
    }
    let mut tasks = Vec::new();
    for value in input["tasks"].as_array().into_iter().flatten() {
        let task = parse_task(value)?;
        ensure!(tasks.iter().all(|t: &Task| t.id != task.id), "Task #{} is used twice", task.id);
        tasks.push(task);
    }
    ensure!(tasks.len() <= 200, "A plan has at most 200 tasks");
    if !input["settings"].is_null() {
        let settings: Settings = serde_json::from_value(input["settings"].clone()).context("Invalid team settings")?;
        ensure!((1..=32).contains(&settings.max_members), "A team has 1 to 32 members");
        team.settings = settings;
    }
    team.members = members;
    team.tasks = tasks;
    let validation = plan::validate(team, &BTreeSet::new());
    events.push(Event::new(
        "planned",
        json!({"by":actor.label(team),"members":team.members.len(),"tasks":team.tasks.len(),"errors":validation.errors.len()}),
    ));
    Ok(json!({"status":"planned","validation":validation}))
}

fn set_status(team: &mut Team, status: TeamStatus, kind: &'static str, actor: &Actor, events: &mut Vec<Event>) -> Value {
    team.status = status;
    events.push(Event::new(kind, json!({"by":actor.label(team)})));
    json!({"status":kind})
}

fn approve(team: &mut Team, actor: &Actor, offline: &BTreeSet<String>, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    ensure!(team.status == TeamStatus::Planning, "Only a plan in Planning can be approved");
    ensure!(matches!(actor, Actor::Human) || (team.settings.approval == Approval::Auto && actor.is_lead(team)),
        "You approve this plan in Zerus");
    ensure!(!team.tasks.is_empty(), "The plan has no tasks");
    let validation = plan::validate(team, offline);
    ensure!(validation.errors.is_empty(), "Fix {} plan error(s) to run: {}", validation.errors.len(),
        validation.errors.iter().map(|f| f.text.as_str()).collect::<Vec<_>>().join("; "));
    for task in &mut team.tasks {
        if task.status == TaskStatus::Draft {
            task.status = TaskStatus::Pending;
        }
    }
    Ok(set_status(team, TeamStatus::Running, "approved", actor, events))
}

/// The task's open attempt must be revoked before another starts. The
/// dispatcher interrupts the old worker separately.
fn revoke(task: &mut Task, why: &str) -> Option<u32> {
    let attempt = task.open_attempt_mut()?;
    attempt.state = AttemptState::Revoked;
    attempt.error = why.to_owned();
    Some(attempt.n)
}

fn finish_if_done(team: &mut Team, events: &mut Vec<Event>) {
    if team.status == TeamStatus::Running && !team.tasks.is_empty() && team.tasks.iter().all(|task| task.status.finished()) {
        team.status = TeamStatus::Done;
        let accepted = team.tasks.iter().filter(|task| task.status == TaskStatus::Accepted).count();
        events.push(Event::new("done", json!({"accepted":accepted,"tasks":team.tasks.len()})));
    }
}

fn report(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    let id = task_id(input)?;
    let attempt_id = text(input, "attempt");
    let body = clean(text(input, "text").trim(), 64 * 1024, "A report")?;
    ensure!(!body.is_empty(), "A report needs text");
    let by_member = match actor {
        Actor::Member(member) => member.clone(),
        Actor::Human => String::new(),
    };
    let label = actor.label(team);
    let team_lead = team.lead.clone();
    let task = team.task_mut(id).context("Unknown task")?;
    ensure!(task.attempts.iter().any(|a| a.id == attempt_id), "Task #{id} has no attempt {attempt_id}");
    let current = task.open_attempt().map(|a| (a.id.clone(), a.member.clone(), a.n));
    let Some((open_id, member, n)) = current.filter(|(open, member, _)| open == attempt_id && (by_member.is_empty() || *member == by_member))
    else {
        // A late report of a revoked or replaced attempt is refused and shown.
        let n = task.attempts.iter().find(|a| a.id == attempt_id).map(|a| a.n);
        // The refusal is a result, not an error, so its feed entry is kept.
        events.push(Event::new("stale_report", json!({"by":label,"task":id,"attempt":n})));
        return Ok(json!({"status":"stale","task":id,"attempt":n}));
    };
    let attempt = task.attempts.iter_mut().find(|a| a.id == open_id).unwrap();
    attempt.state = AttemptState::Reported;
    attempt.report = Some(Report { text: body.clone(), at: now() });
    attempt.no_report = false;
    // The lead hears of a report unless it wrote it.
    attempt.notice = if member == team_lead { Notice::None } else { Notice::Pending };
    task.status = TaskStatus::Submitted;
    task.decided_by = None;
    let title = task.title.clone();
    let role = team.role_of(&member);
    events.push(Event::new("reported", json!({"by":role,"task":id,"title":title,"attempt":n,"human":by_member.is_empty()})));
    Ok(json!({"status":"submitted","task":id,"attempt":n}))
}

fn decide(team: &mut Team, input: &Value, actor: &Actor, accept: bool, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    let id = task_id(input)?;
    let human = matches!(actor, Actor::Human);
    ensure!(human || actor.is_lead(team), "Only the lead or you decide on a report");
    let reason = clean(text(input, "reason").trim(), 4000, "A reason")?;
    ensure!(accept || !reason.is_empty(), "A rejection needs a reason");
    let label = actor.label(team);
    let task = team.task_mut(id).context("Unknown task")?;
    ensure!(!(task.decided_by == Some(Decider::Human) && task.status.finished()), "You already decided task #{id}");
    if human {
        ensure!(!matches!(task.status, TaskStatus::Draft | TaskStatus::Cancelled), "Task #{id} cannot be decided now");
    } else {
        ensure!(task.status == TaskStatus::Submitted, "Task #{id} has no report to decide");
    }
    let n = task.attempts.iter().rev().find(|a| a.state == AttemptState::Reported).map(|a| a.n);
    if accept {
        revoke(task, "accepted without this attempt");
        task.status = TaskStatus::Accepted;
    } else {
        if let Some(attempt) = task.attempts.iter_mut().rev().find(|a| a.state == AttemptState::Reported) {
            attempt.state = AttemptState::Rejected;
        }
        revoke(task, "rejected");
        task.status = if input["final"].as_bool() == Some(true) { TaskStatus::Rejected } else { TaskStatus::Pending };
    }
    task.decided_by = Some(if human { Decider::Human } else { Decider::Lead });
    task.decision_reason = reason.clone();
    let title = task.title.clone();
    events.push(Event::new(
        if accept { "accepted" } else { "rejected" },
        json!({"by":label,"task":id,"title":title,"attempt":n,"reason":reason}),
    ));
    let status = serde_json::to_value(team.task(id).unwrap().status)?;
    finish_if_done(team, events);
    Ok(json!({"status":status,"task":id}))
}

fn reassign(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    let id = task_id(input)?;
    ensure!(matches!(actor, Actor::Human) || actor.is_lead(team), "Only the lead or you reassign tasks");
    let target = text(input, "member");
    ensure!(team.member(target).is_some_and(|m| m.state != MemberState::Removed), "Unknown member {target}");
    let label = actor.label(team);
    let role = team.role_of(target);
    let task = team.task_mut(id).context("Unknown task")?;
    ensure!(!task.status.finished(), "Task #{id} is already decided");
    let revoked = revoke(task, "reassigned");
    task.assignee = target.to_owned();
    if task.status != TaskStatus::Draft {
        task.status = TaskStatus::Pending;
    }
    events.push(Event::new("reassigned", json!({"by":label,"task":id,"to":role,"revoked_attempt":revoked})));
    Ok(json!({"status":"reassigned","task":id,"revoked_attempt":revoked}))
}

fn retry(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    let id = task_id(input)?;
    ensure!(matches!(actor, Actor::Human) || actor.is_lead(team), "Only the lead or you retry tasks");
    let label = actor.label(team);
    let task = team.task_mut(id).context("Unknown task")?;
    ensure!(!task.status.finished() && task.status != TaskStatus::Draft, "Task #{id} cannot be retried now");
    let revoked = revoke(task, "retried");
    task.status = TaskStatus::Pending;
    events.push(Event::new("retried", json!({"by":label,"task":id,"revoked_attempt":revoked})));
    Ok(json!({"status":"pending","task":id,"revoked_attempt":revoked}))
}

fn take_over(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    let id = task_id(input)?;
    ensure!(matches!(actor, Actor::Human), "Only you can take over a task");
    let task = team.task_mut(id).context("Unknown task")?;
    ensure!(!task.status.finished() && task.status != TaskStatus::Draft, "Task #{id} cannot be taken over now");
    let revoked = revoke(task, "taken over");
    task.assignee.clear();
    task.status = TaskStatus::InProgress;
    events.push(Event::new("taken_over", json!({"by":"You","task":id,"revoked_attempt":revoked})));
    Ok(json!({"status":"in_progress","task":id,"revoked_attempt":revoked}))
}

fn cancel(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    let id = task_id(input)?;
    ensure!(matches!(actor, Actor::Human) || actor.is_lead(team), "Only the lead or you cancel tasks");
    let label = actor.label(team);
    let task = team.task_mut(id).context("Unknown task")?;
    ensure!(!task.status.finished(), "Task #{id} is already decided");
    let revoked = revoke(task, "cancelled");
    task.status = TaskStatus::Cancelled;
    events.push(Event::new("cancelled", json!({"by":label,"task":id,"revoked_attempt":revoked})));
    finish_if_done(team, events);
    Ok(json!({"status":"cancelled","task":id,"revoked_attempt":revoked}))
}

/// A human settles an unconfirmed hand-off after checking the session: it
/// arrived, or it did not (the task returns to Pending; nothing is re-sent).
fn check(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    let id = task_id(input)?;
    ensure!(matches!(actor, Actor::Human), "Only you settle an unconfirmed hand-off");
    let arrived = match text(input, "outcome") {
        "delivered" => true,
        "not_delivered" => false,
        _ => bail!("The outcome is delivered or not_delivered"),
    };
    let task = team.task_mut(id).context("Unknown task")?;
    let attempt = task.open_attempt_mut().filter(|a| a.state == AttemptState::Unconfirmed).context("Task has no unconfirmed hand-off")?;
    let n = attempt.n;
    if arrived {
        attempt.state = AttemptState::Delivered;
        attempt.delivered_at = Some(now());
        task.status = TaskStatus::InProgress;
    } else {
        attempt.state = AttemptState::Refused;
        attempt.error = "not delivered (checked)".into();
        task.status = TaskStatus::Pending;
    }
    events.push(Event::new("checked", json!({"by":"You","task":id,"attempt":n,"delivered":arrived})));
    Ok(json!({"status":if arrived {"in_progress"} else {"pending"},"task":id}))
}

/// Applies one human, lead or member request. The dispatcher records its own
/// changes directly.
pub(crate) fn apply(team: &mut Team, op: &str, input: &Value, actor: &Actor, offline: &BTreeSet<String>, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    let human_or_lead = matches!(actor, Actor::Human) || actor.is_lead(team);
    let result = match op {
        "plan" => plan(team, input, actor, events),
        "approve" => approve(team, actor, offline, events),
        "discard" => {
            ensure!(human_or_lead, "Only the lead or you discard a plan");
            ensure!(team.status == TeamStatus::Planning, "Only a plan in Planning can be discarded");
            Ok(set_status(team, TeamStatus::Stopped, "discarded", actor, events))
        }
        "pause" => {
            ensure!(human_or_lead, "Only the lead or you pause the team");
            ensure!(team.status == TeamStatus::Running, "Only a running team can pause");
            Ok(set_status(team, TeamStatus::Paused, "paused", actor, events))
        }
        "resume" => {
            ensure!(human_or_lead, "Only the lead or you resume the team");
            ensure!(team.status == TeamStatus::Paused, "Only a paused team can resume");
            Ok(set_status(team, TeamStatus::Running, "resumed", actor, events))
        }
        "stop" => {
            ensure!(matches!(actor, Actor::Human), "Only you stop the team");
            ensure!(matches!(team.status, TeamStatus::Running | TeamStatus::Paused | TeamStatus::Planning), "The team is not running");
            for task in &mut team.tasks {
                if !task.status.finished() {
                    revoke(task, "team stopped");
                    task.status = TaskStatus::Cancelled;
                }
            }
            Ok(set_status(team, TeamStatus::Stopped, "stopped", actor, events))
        }
        "archive" => {
            ensure!(matches!(actor, Actor::Human), "Only you archive the team");
            ensure!(matches!(team.status, TeamStatus::Done | TeamStatus::Stopped), "Stop the team before archiving it");
            Ok(set_status(team, TeamStatus::Archived, "archived", actor, events))
        }
        "report" | "use-last-reply" => {
            ensure!(op == "report" || matches!(actor, Actor::Human), "Only you use a last reply as the report");
            ensure!(matches!(team.status, TeamStatus::Running | TeamStatus::Paused), "The team is not running");
            report(team, input, actor, events)
        }
        "accept" => decide(team, input, actor, true, events),
        "reject" => decide(team, input, actor, false, events),
        "reassign" => reassign(team, input, actor, events),
        "retry" => retry(team, input, actor, events),
        "take-over" => take_over(team, input, actor, events),
        "cancel" => cancel(team, input, actor, events),
        "check" => check(team, input, actor, events),
        "relaunch" => {
            ensure!(matches!(actor, Actor::Human), "Only you relaunch a member");
            let id = text(input, "member").to_owned();
            let member = team.members.iter_mut().find(|m| m.id == id).context("Unknown member")?;
            ensure!(member.session.is_none() && !member.error.is_empty(), "This member's session did not fail to start");
            member.error.clear();
            member.launch_id.clear();
            events.push(Event::new("relaunch", json!({"by":"You","member":id})));
            Ok(json!({"status":"relaunch","member":id}))
        }
        _ => bail!("Unknown team operation {op}"),
    }?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running_team() -> Team {
        let (mut team, _) = create(
            "t",
            &json!({"name":"Checkout","lead":{"id":"lead","agent":"claude","session":{"name":"claude/x/lead"}}}),
        )
        .unwrap();
        let mut events = Vec::new();
        plan(
            &mut team,
            &json!({"members":[{"id":"dev","role":"Backend","agent":"codex"},{"id":"qa","role":"Tester","agent":"kimi"}],
                "tasks":[{"id":1,"title":"API","assignee":"dev"},{"id":2,"title":"Tests","assignee":"qa","deps":[{"task":1,"kind":"soft"}]}]}),
            &Actor::Member("lead".into()),
            &mut events,
        )
        .unwrap();
        approve(&mut team, &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
        team
    }
    /// What the dispatcher records for a delivered hand-off.
    fn hand_out(team: &mut Team, id: u32) -> String {
        let task = team.task_mut(id).unwrap();
        let n = task.attempts.len() as u32 + 1;
        let attempt = format!("00000000-0000-4000-8000-00000000000{n}");
        task.attempts.push(Attempt { n, id: attempt.clone(), member: task.assignee.clone(), state: AttemptState::Delivered, ..Default::default() });
        task.status = TaskStatus::InProgress;
        attempt
    }

    #[test]
    fn only_you_approve_and_errors_block_it() {
        let (mut team, _) = create("t", &json!({"name":"T","lead":{"id":"lead","agent":"claude"}})).unwrap();
        let mut events = Vec::new();
        plan(&mut team, &json!({"tasks":[{"id":1,"title":"A","assignee":"ghost"}]}), &Actor::Human, &mut events).unwrap();
        assert!(approve(&mut team, &Actor::Member("lead".into()), &BTreeSet::new(), &mut events).is_err());
        let error = approve(&mut team, &Actor::Human, &BTreeSet::new(), &mut events).unwrap_err().to_string();
        assert!(error.contains("no assignee"), "{error}");
        assert!(plan(&mut team, &json!({"tasks":[{"id":1,"title":"A","assignee":"lead","deps":[{"task":1}]}]}), &Actor::Human, &mut events).is_err());
    }

    #[test]
    fn a_report_needs_the_current_attempt_and_a_stale_one_is_recorded() {
        let mut team = running_team();
        let first = hand_out(&mut team, 1);
        let mut events = Vec::new();
        reassign(&mut team, &json!({"task":1,"member":"dev"}), &Actor::Human, &mut events).unwrap();
        let second = hand_out(&mut team, 1);
        let late = report(&mut team, &json!({"task":1,"attempt":first,"text":"done"}), &Actor::Member("dev".into()), &mut events).unwrap();
        assert_eq!((late["status"].as_str(), events.last().unwrap().kind), (Some("stale"), "stale_report"));
        // Another member cannot report this attempt either.
        let foreign = report(&mut team, &json!({"task":1,"attempt":second,"text":"done"}), &Actor::Member("qa".into()), &mut events).unwrap();
        assert_eq!(foreign["status"], "stale");
        report(&mut team, &json!({"task":1,"attempt":second,"text":"done"}), &Actor::Member("dev".into()), &mut events).unwrap();
        assert_eq!(team.task(1).unwrap().status, TaskStatus::Submitted);
    }

    #[test]
    fn a_human_decision_is_final_and_overrides_the_lead() {
        let mut team = running_team();
        let attempt = hand_out(&mut team, 1);
        let mut events = Vec::new();
        report(&mut team, &json!({"task":1,"attempt":attempt,"text":"done"}), &Actor::Member("dev".into()), &mut events).unwrap();
        assert!(decide(&mut team, &json!({"task":1}), &Actor::Member("qa".into()), true, &mut events).is_err());
        decide(&mut team, &json!({"task":1}), &Actor::Member("lead".into()), true, &mut events).unwrap();
        // You override the lead's acceptance; the lead cannot undo yours.
        decide(&mut team, &json!({"task":1,"reason":"404 for foreign cards","final":true}), &Actor::Human, false, &mut events).unwrap();
        assert_eq!(team.task(1).unwrap().status, TaskStatus::Rejected);
        assert!(decide(&mut team, &json!({"task":1}), &Actor::Member("lead".into()), true, &mut events).is_err());
    }

    #[test]
    fn rejecting_returns_the_task_for_another_attempt() {
        let mut team = running_team();
        let attempt = hand_out(&mut team, 1);
        let mut events = Vec::new();
        report(&mut team, &json!({"task":1,"attempt":attempt,"text":"done"}), &Actor::Member("dev".into()), &mut events).unwrap();
        assert!(decide(&mut team, &json!({"task":1}), &Actor::Member("lead".into()), false, &mut events).is_err());
        decide(&mut team, &json!({"task":1,"reason":"paginate"}), &Actor::Member("lead".into()), false, &mut events).unwrap();
        let task = team.task(1).unwrap();
        assert_eq!((task.status, task.attempts[0].state), (TaskStatus::Pending, AttemptState::Rejected));
    }

    #[test]
    fn the_team_is_done_when_every_task_is_decided() {
        let mut team = running_team();
        let mut events = Vec::new();
        for id in [1, 2] {
            let attempt = hand_out(&mut team, id);
            let member = team.task(id).unwrap().assignee.clone();
            report(&mut team, &json!({"task":id,"attempt":attempt,"text":"ok"}), &Actor::Member(member), &mut events).unwrap();
        }
        decide(&mut team, &json!({"task":1}), &Actor::Human, true, &mut events).unwrap();
        cancel(&mut team, &json!({"task":2}), &Actor::Human, &mut events).unwrap();
        assert_eq!(team.status, TeamStatus::Done);
    }

    #[test]
    fn an_unconfirmed_hand_off_is_settled_only_by_you() {
        let mut team = running_team();
        hand_out(&mut team, 1);
        team.task_mut(1).unwrap().attempts[0].state = AttemptState::Unconfirmed;
        team.task_mut(1).unwrap().status = TaskStatus::Assigned;
        let mut events = Vec::new();
        assert!(check(&mut team, &json!({"task":1,"outcome":"delivered"}), &Actor::Member("lead".into()), &mut events).is_err());
        check(&mut team, &json!({"task":1,"outcome":"not_delivered"}), &Actor::Human, &mut events).unwrap();
        let task = team.task(1).unwrap();
        assert_eq!((task.status, task.attempts[0].state), (TaskStatus::Pending, AttemptState::Refused));
    }

    #[test]
    fn stop_cancels_open_tasks_and_archive_needs_a_stopped_team() {
        let mut team = running_team();
        let mut events = Vec::new();
        assert!(apply(&mut team, "archive", &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).is_err());
        hand_out(&mut team, 1);
        apply(&mut team, "stop", &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
        assert!(team.tasks.iter().all(|t| t.status == TaskStatus::Cancelled));
        assert_eq!(team.task(1).unwrap().attempts[0].state, AttemptState::Revoked);
        apply(&mut team, "archive", &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
        assert_eq!(team.status, TeamStatus::Archived);
    }
}
