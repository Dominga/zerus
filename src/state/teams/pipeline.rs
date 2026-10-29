//! WhiteWh fork: a task's pipeline (U36). After the implementer's report a
//! task passes its preset's stages (review, test) and then the final
//! acceptance (the lead, the sub-lead or auto). A stage member's verdict moves
//! the task on (`pass`) or back to the implementer with its findings
//! (`return`) without the lead; after the preset's rounds the lead decides
//! with every round's findings. A team or task without a pipeline keeps
//! today's flow: Submitted, then the lead.
use super::super::now;
use super::catalog::{Catalog, DEFAULT_ROUNDS};
use super::model::*;
use super::ops::Actor;
use super::store::Event;
use anyhow::{bail, ensure, Context};
use serde_json::{json, Value};

/// At most this many stage runs stay on a task; older ones leave first.
const MAX_RUNS: usize = 64;

/// A task's pipeline choice as the plan, extend or edit-draft names it: empty
/// (the role's or the template's default), "none", or a preset of `catalog`.
pub(crate) fn check_choice(choice: &str, catalog: &Catalog) -> anyhow::Result<()> {
    ensure!(
        choice.is_empty() || choice == "none" || catalog.pipelines.iter().any(|pipeline| pipeline.id == choice),
        "Unknown pipeline {choice}: name a preset of the team catalog, or none"
    );
    Ok(())
}

/// Whether a task can have a pipeline: a check decides by its exit code, the
/// lead accepts its own task on its report, and a sub-team reports through
/// its own lead.
fn eligible(team: &Team, task: &Task) -> bool {
    !task.is_check() && task.assignee != team.lead && task.subteam.is_none()
}

/// The preset a task takes: its own choice, else its role's, else the team
/// template's. None for "none", an unknown preset, or no default anywhere.
pub(crate) fn resolve(catalog: &Catalog, team: &Team, task: &Task) -> Option<PipelineRun> {
    if !eligible(team, task) {
        return None;
    }
    let role_name = team.member(&task.assignee).map(|member| member.role.clone()).unwrap_or_default();
    let role = catalog.roles.iter().find(|role| role.id.eq_ignore_ascii_case(&role_name) || role.name.eq_ignore_ascii_case(&role_name));
    let template = catalog.templates.iter().find(|template| template.id == team.template);
    let choice = [Some(task.pipeline.as_str()), role.map(|role| role.pipeline.as_str()), template.map(|template| template.pipeline.as_str())]
        .into_iter()
        .flatten()
        .find(|choice| !choice.is_empty())?;
    let preset = catalog.pipelines.iter().find(|pipeline| pipeline.id == choice)?;
    let stages = preset
        .stages
        .iter()
        .map(|stage| PipelineStage { kind: stage.kind.clone(), role: stage.role.clone(), member: stage.member.clone() })
        .collect();
    Some(PipelineRun {
        id: preset.id.clone(),
        name: preset.name.clone(),
        stages,
        max_rounds: if preset.max_rounds == 0 { DEFAULT_ROUNDS } else { preset.max_rounds },
        folder: if preset.folder.is_empty() { "hold".into() } else { preset.folder.clone() },
        accept: if preset.accept.is_empty() { "lead".into() } else { preset.accept.clone() },
        round: 1,
        runs: Vec::new(),
    })
}

/// Snapshots the task's pipeline when it is first assigned (before its first
/// attempt): a later catalog edit does not change a running task, and a task
/// handed out before pipelines (an old team) keeps today's flow.
pub(crate) fn snapshot_with(team: &mut Team, task_id: u32, catalog: &Catalog) {
    let Some(task) = team.task(task_id) else { return };
    if task.pipeline_run.is_some() || !task.attempts.is_empty() {
        return;
    }
    let run = resolve(catalog, team, task);
    if let Some(task) = team.task_mut(task_id) {
        task.pipeline_run = run;
    }
}

/// The dispatcher's snapshot at a hand-out, from the catalog on disk.
pub(crate) fn snapshot(team: &mut Team, task_id: u32) {
    if team.task(task_id).is_none_or(|task| task.pipeline_run.is_some() || !task.attempts.is_empty()) {
        return;
    }
    let catalog = super::catalog::load().unwrap_or_else(|_| super::catalog::builtin());
    snapshot_with(team, task_id, &catalog);
}

fn status_of(kind: &str) -> TaskStatus {
    if kind == "test" {
        TaskStatus::InTest
    } else {
        TaskStatus::InReview
    }
}

/// The implementer's attempt the pipeline checks: the newest reported one.
fn reported_attempt(task: &Task) -> Option<u32> {
    task.attempts.iter().rev().find(|attempt| attempt.state == AttemptState::Reported).map(|attempt| attempt.n)
}

/// The members of the implementer's attempts: none of them takes a stage of
/// the task.
fn implementers(task: &Task) -> Vec<String> {
    let mut found: Vec<String> = task.attempts.iter().map(|attempt| attempt.member.clone()).collect();
    found.push(task.assignee.clone());
    found
}

/// The implementer reported: the task enters its first stage, or goes straight
/// to the final acceptance when the preset has none. None without a pipeline
/// (the caller keeps today's Submitted).
pub(crate) fn on_report(team: &mut Team, id: u32, events: &mut Vec<Event>) -> anyhow::Result<Option<Value>> {
    let Some(task) = team.task_mut(id) else { return Ok(None) };
    let Some(run) = task.pipeline_run.as_mut() else { return Ok(None) };
    // Each report the pipeline reads is one round.
    run.round = run.runs.iter().map(|stage| stage.round).max().map_or(1, |round| round + 1);
    if run.stages.is_empty() {
        return final_acceptance(team, id, events).map(Some);
    }
    // The lead hears of this report only after the last stage (or the limit).
    if let Some(attempt) = task.attempts.iter_mut().rev().find(|attempt| attempt.state == AttemptState::Reported) {
        attempt.notice = Notice::None;
    }
    enter(team, id, 0, events).map(Some)
}

/// The task waits for stage `stage` of the current round.
fn enter(team: &mut Team, id: u32, stage: usize, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    let task = team.task_mut(id).context("Unknown task")?;
    let implementers = implementers(task);
    let attempt = reported_attempt(task).unwrap_or_default();
    let title = task.title.clone();
    let run = task.pipeline_run.as_mut().context("The task has no pipeline")?;
    let spec = run.stages.get(stage).cloned().context("No such stage")?;
    // A pinned member takes its stage, never one of the task's implementers.
    let member = if spec.member.is_empty() || implementers.contains(&spec.member) { String::new() } else { spec.member.clone() };
    let round = run.round;
    let max_rounds = run.max_rounds;
    run.runs.push(StageRun {
        id: uuid::Uuid::new_v4().to_string(),
        stage,
        kind: spec.kind.clone(),
        round,
        attempt,
        member: member.clone(),
        at: now(),
        ..Default::default()
    });
    let excess = run.runs.len().saturating_sub(MAX_RUNS);
    run.runs.drain(..excess);
    task.status = status_of(&spec.kind);
    let status = serde_json::to_value(task.status)?;
    let who = if member.is_empty() { spec.role.clone() } else { team.role_of(&member) };
    events.push(Event::new(
        "stage_started",
        json!({"task":id,"title":title,"stage":spec.kind,"round":round,"max_rounds":max_rounds,"who":who,"attempt":attempt}),
    ));
    Ok(json!({"status":status,"task":id,"attempt":attempt,"stage":spec.kind,"round":round}))
}

/// After the last stage: the lead (or sub-lead) decides, or `auto` accepts.
fn final_acceptance(team: &mut Team, id: u32, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    let lead = team.lead.clone();
    let task = team.task_mut(id).context("Unknown task")?;
    let run = task.pipeline_run.as_ref().context("The task has no pipeline")?;
    let (accept, name, round) = (run.accept.clone(), run.name.clone(), run.round);
    let title = task.title.clone();
    let attempt = reported_attempt(task);
    if accept == "auto" {
        task.status = TaskStatus::Accepted;
        task.decided_by = None;
        task.decision_reason = format!("Accepted by the pipeline {name}");
        let reason = task.decision_reason.clone();
        let structured = task
            .attempts
            .iter()
            .rev()
            .find(|found| found.state == AttemptState::Reported)
            .and_then(|found| found.report.as_ref())
            .and_then(|report| report.structured.clone());
        // D2: an accepted report files its findings, as the lead's accept does.
        if let Some(report) = structured {
            let (filed, deferred) = super::ops::file_findings(team, id, &report, "Pipeline")?;
            if filed > 0 {
                events.push(Event::new("findings_filed", json!({"task":id,"count":filed,"deferred":deferred})));
            }
        }
        events.push(Event::new(
            "accepted",
            json!({"by":"Pipeline","task":id,"title":title,"attempt":attempt,"reason":reason,"manual":false,"round":round}),
        ));
        return Ok(json!({"status":"accepted","task":id,"attempt":attempt}));
    }
    task.status = TaskStatus::Submitted;
    let member = task.attempts.iter().rev().find(|found| found.state == AttemptState::Reported).map(|found| found.member.clone());
    if let Some(found) = task.attempts.iter_mut().rev().find(|found| found.state == AttemptState::Reported) {
        found.notice = if Some(&lead) == member.as_ref() { Notice::None } else { Notice::Pending };
    }
    let to = if accept == "sub_lead" { team.role_of(&team.reviewer_of(&member.unwrap_or_default())) } else { "Lead".into() };
    events.push(Event::new("stages_passed", json!({"task":id,"title":title,"attempt":attempt,"round":round,"to":to})));
    Ok(json!({"status":"submitted","task":id,"attempt":attempt}))
}

/// Ends the open stage run (if any) with `state`, by whom and why.
fn close_run(task: &mut Task, state: StageState, by: &str, text: &str, findings: Vec<String>) -> Option<StageRun> {
    let run = task.pipeline_run.as_mut()?.open_run_mut()?;
    run.state = state;
    run.ended_at = Some(now());
    run.by = by.to_owned();
    run.text = text.to_owned();
    run.findings = findings;
    Some(run.clone())
}

/// Any change that ends the task's current report (accept, reject, retry,
/// reassign, cancel, stop) also ends its open stage without a verdict.
pub(crate) fn revoke_runs(task: &mut Task, why: &str) {
    if task.pipeline_run.as_ref().and_then(PipelineRun::open_run).is_some() {
        close_run(task, StageState::Revoked, "Zerus", why, Vec::new());
    }
}

/// Moves on after the open stage passed or was skipped.
fn advance(team: &mut Team, id: u32, from: usize, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    let stages = team.task(id).and_then(|task| task.pipeline_run.as_ref()).map(|run| run.stages.len()).unwrap_or_default();
    if from + 1 < stages {
        enter(team, id, from + 1, events)
    } else {
        final_acceptance(team, id, events)
    }
}

/// Every round's findings, oldest first, as the lead and the implementer read
/// them.
pub(crate) fn rounds_text(task: &Task) -> String {
    let Some(run) = task.pipeline_run.as_ref() else { return String::new() };
    let mut text = String::new();
    for stage in run.runs.iter().filter(|stage| stage.state == StageState::Returned) {
        text += &format!("Round {} of {}, {} by {}:\n", stage.round, run.max_rounds, stage.kind, stage.by);
        if stage.findings.is_empty() {
            text += &format!("{}\n", stage.text.trim());
        }
        for finding in &stage.findings {
            text += &format!("- {finding}\n");
        }
    }
    text
}

/// The stage run a request names, as its attempt id.
pub(crate) fn run_of<'a>(team: &'a Team, task: u32, id: &str) -> Option<&'a StageRun> {
    if id.is_empty() {
        return None;
    }
    team.task(task)?.pipeline_run.as_ref()?.runs.iter().find(|run| run.id == id)
}

/// The member a stage run id belongs to, for a caller with no session
/// identity of its own.
pub(crate) fn run_member(team: &Team, id: &str) -> Option<String> {
    if id.is_empty() {
        return None;
    }
    team.tasks
        .iter()
        .filter_map(|task| task.pipeline_run.as_ref())
        .flat_map(|run| &run.runs)
        .find(|run| run.id == id && !run.member.is_empty())
        .map(|run| run.member.clone())
}

/// The task whose open stage run `id` is given to `member`: a stage member's
/// queued verdict or template run names it as its attempt.
pub(crate) fn open_run_task(team: &Team, id: &str, member: &str) -> Option<u32> {
    if id.is_empty() || member.is_empty() {
        return None;
    }
    team.tasks
        .iter()
        .find(|task| task.pipeline_run.as_ref().and_then(PipelineRun::open_run).is_some_and(|run| run.id == id && run.member == member))
        .map(|task| task.id)
}

/// The members who may take the open stage: active members with the stage's
/// role (or the pinned member), never one of the task's implementers.
pub(crate) fn candidates(team: &Team, id: u32) -> Vec<String> {
    let Some(task) = team.task(id) else { return Vec::new() };
    let Some(run) = task.pipeline_run.as_ref() else { return Vec::new() };
    let Some(open) = run.open_run() else { return Vec::new() };
    let Some(spec) = run.stages.get(open.stage) else { return Vec::new() };
    let implementers = implementers(task);
    team.members
        .iter()
        .filter(|member| member.state != MemberState::Removed && member.state != MemberState::Paused)
        .filter(|member| !implementers.contains(&member.id))
        .filter(|member| if spec.member.is_empty() { member.role.eq_ignore_ascii_case(&spec.role) || role_matches(&member.role, &spec.role) } else { member.id == spec.member })
        .map(|member| member.id.clone())
        .collect()
}

/// A member's role name against a catalog role id ("QA reviewer" against
/// "qa-reviewer").
fn role_matches(name: &str, id: &str) -> bool {
    let norm = |value: &str| value.chars().filter(|ch| ch.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
    norm(name) == norm(id)
}

/// Stage delivery gives the open stage to `member` (U36, #532). The task's
/// implementer never takes its own stage.
#[cfg_attr(not(test), allow(dead_code))] // the dispatcher's stage delivery (#532) calls it
pub(crate) fn give_stage(team: &mut Team, id: u32, member: &str) -> anyhow::Result<String> {
    ensure!(candidates(team, id).iter().any(|found| found == member), "{} cannot take this stage of task #{id}", team.role_of(member));
    let task = team.task_mut(id).context("Unknown task")?;
    let run = task.pipeline_run.as_mut().and_then(PipelineRun::open_run_mut).context("The task is not in a stage")?;
    ensure!(run.state == StageState::Waiting || run.member == member, "This stage already has its member");
    run.member = member.to_owned();
    run.state = StageState::Delivered;
    run.delivered_at = Some(now());
    Ok(run.id.clone())
}

/// A stage member's `hgs team report` with a verdict. None when the request's
/// attempt is not a stage run of the task (an ordinary report).
pub(crate) fn stage_report(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Option<Value>> {
    let id = input["task"].as_u64().and_then(|id| u32::try_from(id).ok()).unwrap_or_default();
    let run_id = input["attempt"].as_str().unwrap_or_default();
    let Some(run) = run_of(team, id, run_id).cloned() else {
        ensure!(input.get("verdict").is_none(), "A verdict reports a stage: name the stage's attempt id");
        return Ok(None);
    };
    let caller = match actor {
        Actor::Member(member) if !member.is_empty() => member.clone(),
        _ => bail!("A stage's verdict comes from its member; the lead or you skip a stage or send the task back"),
    };
    if !run.state.open() || team.task(id).and_then(|task| task.pipeline_run.as_ref()).and_then(PipelineRun::open_run).map(|open| open.id.as_str()) != Some(run_id) {
        events.push(Event::new("stale_report", json!({"by":team.role_of(&caller),"task":id,"stage":run.kind,"round":run.round})));
        return Ok(Some(json!({"status":"stale","task":id,"stage":run.kind,"round":run.round})));
    }
    ensure!(run.member == caller, "This stage of task #{id} was given to {}, not to you", if run.member.is_empty() { "nobody yet".to_owned() } else { team.role_of(&run.member) });
    ensure!(!team.task(id).map(implementers).unwrap_or_default().contains(&caller), "The implementer never takes its own task's stage");
    let text = input["text"].as_str().unwrap_or_default().trim().to_owned();
    ensure!(text.chars().count() <= 64 * 1024, "A report is at most 64 KiB");
    let findings: Vec<String> = input["findings"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|finding| finding.as_str().unwrap_or_default().trim().to_owned())
        .filter(|finding| !finding.is_empty())
        .collect();
    ensure!(findings.len() <= 50 && findings.iter().all(|finding| finding.chars().count() <= 2000), "At most 50 findings of 2000 characters each");
    let by = team.role_of(&caller);
    let title = team.task(id).map(|task| task.title.clone()).unwrap_or_default();
    match input["verdict"].as_str().unwrap_or_default() {
        "pass" => {
            let task = team.task_mut(id).context("Unknown task")?;
            close_run(task, StageState::Passed, &by, &text, findings);
            events.push(Event::new("stage_passed", json!({"by":by,"task":id,"title":title,"stage":run.kind,"round":run.round})));
            advance(team, id, run.stage, events).map(Some)
        }
        "return" => {
            ensure!(!findings.is_empty() || !text.is_empty(), "A return names its findings");
            let task = team.task_mut(id).context("Unknown task")?;
            close_run(task, StageState::Returned, &by, &text, findings.clone());
            let pipeline = task.pipeline_run.as_ref().context("The task has no pipeline")?;
            let (returned, max_rounds) = (pipeline.returned(), pipeline.max_rounds);
            events.push(Event::new(
                "stage_returned",
                json!({"by":by,"task":id,"title":title,"stage":run.kind,"round":run.round,"max_rounds":max_rounds,"findings":findings}),
            ));
            if returned >= max_rounds {
                // The round limit: the lead decides, with every round's findings.
                let rounds = rounds_text(task);
                let result = final_to_lead(team, id)?;
                events.push(Event::new("round_limit", json!({"task":id,"title":title,"rounds":returned,"findings":rounds})));
                return Ok(Some(result));
            }
            // Back to the implementer as a new attempt, without the lead.
            if let Some(attempt) = task.attempts.iter_mut().rev().find(|attempt| attempt.state == AttemptState::Reported) {
                attempt.state = AttemptState::Rejected;
            }
            task.status = TaskStatus::Pending;
            task.decided_by = None;
            Ok(Some(json!({"status":"pending","task":id,"stage":run.kind,"round":run.round,"returned":true})))
        }
        _ => bail!("A stage's verdict is pass or return"),
    }
}

/// The round limit, or a skip past the last stage with a lead acceptance:
/// Submitted, and the lead hears of it.
fn final_to_lead(team: &mut Team, id: u32) -> anyhow::Result<Value> {
    let lead = team.lead.clone();
    let task = team.task_mut(id).context("Unknown task")?;
    task.status = TaskStatus::Submitted;
    let attempt = reported_attempt(task);
    if let Some(found) = task.attempts.iter_mut().rev().find(|found| found.state == AttemptState::Reported) {
        found.notice = if found.member == lead { Notice::None } else { Notice::Pending };
    }
    Ok(json!({"status":"submitted","task":id,"attempt":attempt,"round_limit":true}))
}

fn reason(input: &Value) -> anyhow::Result<String> {
    let reason = input["reason"].as_str().unwrap_or_default().trim().to_owned();
    ensure!(!reason.is_empty(), "Give a reason");
    ensure!(reason.chars().count() <= 4000, "A reason is at most 4000 characters");
    Ok(reason)
}

/// `hgs team skip-stage`: the lead or you pass the open stage over, with a
/// reason; the feed names who.
pub(crate) fn skip_stage(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    let id = input["task"].as_u64().and_then(|id| u32::try_from(id).ok()).context("A task number is required")?;
    let assignee = team.task(id).map(|task| task.assignee.clone()).context("Unknown task")?;
    ensure!(actor.reviews(team, &assignee), "Only the lead or you skip a stage");
    ensure!(matches!(team.status, TeamStatus::Running | TeamStatus::Paused), "The team is not running");
    let reason = reason(input)?;
    let by = actor.label(team);
    let task = team.task_mut(id).context("Unknown task")?;
    ensure!(task.status.in_stage(), "Task #{id} is not in a stage");
    let title = task.title.clone();
    let run = close_run(task, StageState::Skipped, &by, &reason, Vec::new()).context("Task #{id} is not in a stage")?;
    events.push(Event::new("stage_skipped", json!({"by":by,"task":id,"title":title,"stage":run.kind,"round":run.round,"reason":reason})));
    advance(team, id, run.stage, events)
}

/// `hgs team send-back`: the lead or you return the task to its implementer
/// from any stage (or from the final acceptance) with a reason, as a new
/// attempt; the round limit counts only stage returns.
pub(crate) fn send_back(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    let id = input["task"].as_u64().and_then(|id| u32::try_from(id).ok()).context("A task number is required")?;
    let assignee = team.task(id).map(|task| task.assignee.clone()).context("Unknown task")?;
    ensure!(actor.reviews(team, &assignee), "Only the lead or you send a task back");
    ensure!(matches!(team.status, TeamStatus::Running | TeamStatus::Paused), "The team is not running");
    let reason = reason(input)?;
    let by = actor.label(team);
    let human = matches!(actor, Actor::Human);
    let task = team.task_mut(id).context("Unknown task")?;
    ensure!(task.pipeline_run.is_some() && task.status.reported(), "Task #{id} has no report in its pipeline to send back");
    let stage = task.pipeline_run.as_ref().and_then(PipelineRun::open_run).map(|run| run.kind.clone());
    close_run(task, StageState::Revoked, &by, &reason, Vec::new());
    let attempt = reported_attempt(task);
    if let Some(found) = task.attempts.iter_mut().rev().find(|found| found.state == AttemptState::Reported) {
        found.state = AttemptState::Rejected;
    }
    task.status = TaskStatus::Pending;
    task.decided_by = Some(if human { Decider::Human } else { Decider::Lead });
    task.decision_reason = reason.clone();
    let title = task.title.clone();
    events.push(Event::new("sent_back", json!({"by":by,"task":id,"title":title,"attempt":attempt,"stage":stage,"reason":reason})));
    Ok(json!({"status":"pending","task":id,"attempt":attempt}))
}

/// The final acceptance a sub-lead may not give: a preset that names the lead.
pub(crate) fn lead_accepts_only(task: &Task) -> bool {
    task.pipeline_run.as_ref().is_some_and(|run| run.accept == "lead")
}

/// U37: each stage run's span, oldest first: waiting from `at`, working from
/// delivery to its end (or `now` while it runs).
pub(crate) fn spans(task: &Task, at: f64) -> Vec<Value> {
    let Some(run) = task.pipeline_run.as_ref() else { return Vec::new() };
    run.runs
        .iter()
        .map(|stage| {
            let end = stage.ended_at.unwrap_or(at);
            let start = stage.delivered_at.unwrap_or(end).min(end);
            json!({"kind":stage.kind,"round":stage.round,"member":stage.member,"state":stage.state,
                "waited_s":(start - stage.at).max(0.0).round(),"worked_s":(end - start).max(0.0).round(),
                "running":stage.state.open()})
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::teams::catalog::{builtin, Pipeline, Stage};
    use crate::state::teams::ops::{apply, create};
    use std::collections::BTreeSet;

    fn run(team: &mut Team, op: &str, input: Value, actor: &Actor) -> anyhow::Result<Value> {
        let mut events = Vec::new();
        apply(team, op, &input, actor, &BTreeSet::new(), &mut events)
    }

    fn feed(team: &mut Team, op: &str, input: Value, actor: &Actor) -> (Value, Vec<Event>) {
        let mut events = Vec::new();
        let value = apply(team, op, &input, actor, &BTreeSet::new(), &mut events).unwrap();
        (value, events)
    }

    fn member(id: &str) -> Actor {
        Actor::Member(id.into())
    }

    /// A running team: dev (Backend) implements #1; rev and rev2 review, qa tests.
    fn team_with(pipeline: &str, catalog: &Catalog) -> Team {
        let (mut team, _) = create("t", &json!({"name":"T","lead":{"id":"lead","agent":"claude"}})).unwrap();
        run(
            &mut team,
            "plan",
            json!({"members":[{"id":"dev","role":"Backend","agent":"codex"},{"id":"rev","role":"Reviewer","agent":"claude"},
                    {"id":"qa","role":"Tester","agent":"kimi"},{"id":"rev2","role":"Reviewer","agent":"claude"}],
                "tasks":[{"id":1,"title":"API","assignee":"dev","criteria":["Paged"]}]}),
            &Actor::Human,
        )
        .unwrap();
        run(&mut team, "approve", json!({}), &Actor::Human).unwrap();
        team.task_mut(1).unwrap().pipeline = pipeline.into();
        snapshot_with(&mut team, 1, catalog);
        team
    }

    /// The dispatcher's hand-out: a delivered attempt of the assignee.
    fn hand(team: &mut Team) -> String {
        let task = team.task_mut(1).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let n = task.attempts.len() as u32 + 1;
        task.attempts.push(Attempt { n, id: id.clone(), member: task.assignee.clone(), state: AttemptState::Delivered, ..Default::default() });
        task.status = TaskStatus::InProgress;
        id
    }

    fn report(team: &mut Team, attempt: &str) -> Value {
        run(team, "report", json!({"task":1,"attempt":attempt,"text":"Done"}), &member("dev")).unwrap()
    }

    fn verdict(team: &mut Team, by: &str, verdict: &str, findings: &[&str]) -> anyhow::Result<Value> {
        let stage = give_stage(team, 1, by)?;
        run(team, "report", json!({"task":1,"attempt":stage,"verdict":verdict,"text":"Read it","findings":findings}), &member(by))
    }

    fn pipeline(team: &Team) -> &PipelineRun {
        team.task(1).unwrap().pipeline_run.as_ref().unwrap()
    }

    fn kinds(events: &[Event]) -> Vec<&str> {
        events.iter().map(|event| event.kind).collect()
    }

    #[test]
    fn a_report_passes_review_and_test_then_the_lead_accepts() {
        let mut team = team_with("code", &builtin());
        assert_eq!(pipeline(&team).id, "code");
        let attempt = hand(&mut team);
        let value = report(&mut team, &attempt);
        assert_eq!((value["status"].clone(), value["stage"].clone(), value["round"].clone()), (json!("in_review"), json!("review"), json!(1)));
        let task = team.task(1).unwrap();
        assert_eq!(task.status, TaskStatus::InReview);
        assert_eq!(task.attempts[0].notice, Notice::None, "the lead is not told while a stage reads the report");
        assert_eq!(pipeline(&team).open_run().unwrap().state, StageState::Waiting);
        // A captured report replaced while the review reads it tells nobody yet.
        team.task_mut(1).unwrap().attempts[0].report.as_mut().unwrap().captured = true;
        run(&mut team, "report", json!({"task":1,"attempt":attempt,"text":"Done, really"}), &member("dev")).unwrap();
        let task = team.task(1).unwrap();
        assert_eq!((task.status, task.attempts[0].notice), (TaskStatus::InReview, Notice::None));
        // A sandboxed stage member's queued verdict names its open run.
        let stage = give_stage(&mut team, 1, "rev").unwrap();
        assert_eq!((open_run_task(&team, &stage, "rev"), open_run_task(&team, &stage, "dev")), (Some(1), None));
        // The reviewer passes it: the test stage is next.
        let value = verdict(&mut team, "rev", "pass", &[]).unwrap();
        assert_eq!(open_run_task(&team, &stage, "rev"), None, "a closed run proves nothing");
        assert_eq!(value["status"], "in_test");
        assert_eq!(team.task(1).unwrap().status, TaskStatus::InTest);
        // The tester passes it: the lead decides, and hears of it now.
        let mut events = Vec::new();
        let stage = give_stage(&mut team, 1, "qa").unwrap();
        let value = apply(&mut team, "report", &json!({"task":1,"attempt":stage,"verdict":"pass","text":"All green"}), &member("qa"), &BTreeSet::new(), &mut events).unwrap();
        assert_eq!(value["status"], "submitted");
        assert!(kinds(&events).contains(&"stages_passed"), "{:?}", kinds(&events));
        let task = team.task(1).unwrap();
        assert_eq!((task.status, task.attempts[0].notice), (TaskStatus::Submitted, Notice::Pending));
        let states: Vec<StageState> = pipeline(&team).runs.iter().map(|run| run.state).collect();
        assert_eq!(states, [StageState::Passed, StageState::Passed]);
        run(&mut team, "accept", json!({"task":1}), &member("lead")).unwrap();
        assert_eq!(team.task(1).unwrap().status, TaskStatus::Accepted);
    }

    #[test]
    fn a_return_makes_a_new_attempt_with_the_findings_and_no_lead_step() {
        let mut team = team_with("code", &builtin());
        let first = hand(&mut team);
        report(&mut team, &first);
        let value = verdict(&mut team, "rev", "return", &["Page size is unbounded", "No test for an empty page"]).unwrap();
        assert_eq!((value["status"].clone(), value["returned"].clone()), (json!("pending"), json!(true)));
        let task = team.task(1).unwrap();
        assert_eq!((task.status, task.attempts[0].state, task.decided_by), (TaskStatus::Pending, AttemptState::Rejected, None));
        assert!(crate::state::teams::plan::ready(&team).contains(&1), "the dispatcher hands it out again");
        // The next hand-out carries the findings, not a lead's decision.
        let second = hand(&mut team);
        let task = team.task(1).unwrap().clone();
        let text = crate::state::teams::dispatcher::hand_off_text(&team, &task, task.attempts.last().unwrap(), "", "");
        assert!(text.contains("Attempt 1 was returned by the review of Reviewer (round 1 of 3). Fix these findings:"), "{text}");
        assert!(text.contains("- Page size is unbounded\n- No test for an empty page\n"), "{text}");
        // Its report is round 2, in review again.
        let value = report(&mut team, &second);
        assert_eq!((value["status"].clone(), value["round"].clone()), (json!("in_review"), json!(2)));
    }

    #[test]
    fn the_round_limit_sends_the_task_to_the_lead_with_every_rounds_findings() {
        let mut catalog = builtin();
        catalog.pipelines[0].max_rounds = 2;
        let mut team = team_with("code", &catalog);
        let first = hand(&mut team);
        report(&mut team, &first);
        verdict(&mut team, "rev", "return", &["Round one finding"]).unwrap();
        let second = hand(&mut team);
        report(&mut team, &second);
        let mut events = Vec::new();
        let stage = give_stage(&mut team, 1, "rev2").unwrap();
        let value = apply(&mut team, "report", &json!({"task":1,"attempt":stage,"verdict":"return","findings":["Round two finding"]}),
            &member("rev2"), &BTreeSet::new(), &mut events).unwrap();
        assert_eq!((value["status"].clone(), value["round_limit"].clone()), (json!("submitted"), json!(true)));
        let task = team.task(1).unwrap();
        assert_eq!((task.status, task.attempts[1].state, task.attempts[1].notice), (TaskStatus::Submitted, AttemptState::Reported, Notice::Pending));
        let limit = events.iter().find(|event| event.kind == "round_limit").expect("round_limit");
        let findings = limit.payload["findings"].as_str().unwrap();
        assert!(findings.contains("Round 1 of 2, review by Reviewer:\n- Round one finding"), "{findings}");
        assert!(findings.contains("Round 2 of 2, review by Reviewer:\n- Round two finding"), "{findings}");
        assert_eq!(rounds_text(task), findings);
        // The lead's inbox lists the report with every round's findings.
        let items = crate::state::teams::inbox::items(&team);
        let item = items.iter().find(|item| item.kind == "report").expect("the report reaches the lead");
        assert_eq!(item.detail, findings);
        // While a stage reads a report, the lead's inbox has nothing of it.
        let mut team = team_with("code", &builtin());
        let attempt = hand(&mut team);
        report(&mut team, &attempt);
        assert!(!crate::state::teams::inbox::items(&team).iter().any(|item| item.kind == "report"));
    }

    #[test]
    fn an_auto_pipeline_accepts_after_its_last_stage() {
        let mut team = team_with("docs", &builtin());
        let attempt = hand(&mut team);
        report(&mut team, &attempt);
        let mut events = Vec::new();
        let stage = give_stage(&mut team, 1, "rev").unwrap();
        let value = apply(&mut team, "report", &json!({"task":1,"attempt":stage,"verdict":"pass"}), &member("rev"), &BTreeSet::new(), &mut events).unwrap();
        assert_eq!(value["status"], "accepted");
        let task = team.task(1).unwrap();
        assert_eq!((task.status, task.decided_by), (TaskStatus::Accepted, None));
        assert_eq!(task.decision_reason, "Accepted by the pipeline Docs: review, auto");
        let accepted = events.iter().find(|event| event.kind == "accepted").unwrap();
        assert_eq!(accepted.payload["by"], "Pipeline");
        // A preset without stages decides on the report itself.
        let mut catalog = builtin();
        catalog.pipelines.push(Pipeline { id: "trust".into(), name: "Trust".into(), accept: "auto".into(), ..Default::default() });
        let mut team = team_with("trust", &catalog);
        let attempt = hand(&mut team);
        assert_eq!(report(&mut team, &attempt)["status"], "accepted");
        // "Check: lead" has no stages: today's Submitted.
        let mut team = team_with("check", &builtin());
        let attempt = hand(&mut team);
        assert_eq!(report(&mut team, &attempt)["status"], "submitted");
    }

    #[test]
    fn the_lead_and_the_owner_skip_a_stage_and_send_a_task_back_with_a_reason() {
        let mut team = team_with("code", &builtin());
        let attempt = hand(&mut team);
        report(&mut team, &attempt);
        // A reason is required, and a member cannot skip.
        assert!(run(&mut team, "skip-stage", json!({"task":1}), &member("lead")).unwrap_err().to_string().contains("Give a reason"));
        assert!(run(&mut team, "skip-stage", json!({"task":1,"reason":"x"}), &member("rev")).is_err());
        let (value, events) = feed(&mut team, "skip-stage", json!({"task":1,"reason":"Reviewed in the pair session"}), &member("lead"));
        assert_eq!(value["status"], "in_test");
        let skipped = events.iter().find(|event| event.kind == "stage_skipped").unwrap();
        assert_eq!((skipped.payload["by"].clone(), skipped.payload["reason"].clone()), (json!("Lead"), json!("Reviewed in the pair session")));
        assert_eq!(pipeline(&team).runs[0].state, StageState::Skipped);
        // The owner sends it back from the test stage: a new attempt, and the
        // open stage ends without a verdict.
        let (value, events) = feed(&mut team, "send-back", json!({"task":1,"reason":"Wrong endpoint"}), &Actor::Human);
        assert_eq!(value["status"], "pending");
        let sent = events.iter().find(|event| event.kind == "sent_back").unwrap();
        assert_eq!((sent.payload["by"].clone(), sent.payload["stage"].clone()), (json!("You"), json!("test")));
        let task = team.task(1).unwrap();
        assert_eq!((task.status, task.attempts[0].state, task.decided_by), (TaskStatus::Pending, AttemptState::Rejected, Some(Decider::Human)));
        assert_eq!(pipeline(&team).runs[1].state, StageState::Revoked);
        assert_eq!(pipeline(&team).returned(), 0, "a send-back is not a stage's round");
        let second = hand(&mut team);
        let task = team.task(1).unwrap().clone();
        let text = crate::state::teams::dispatcher::hand_off_text(&team, &task, task.attempts.last().unwrap(), "", "");
        assert!(text.contains("Attempt 1 was rejected: Wrong endpoint"), "{text}");
        // The lead accepts in the middle of review: the stage ends.
        report(&mut team, &second);
        assert_eq!(team.task(1).unwrap().status, TaskStatus::InReview);
        let (value, events) = feed(&mut team, "accept", json!({"task":1,"reason":"Urgent"}), &member("lead"));
        assert_eq!(value["status"], "accepted");
        assert!(events.iter().any(|event| event.kind == "accepted" && event.payload["by"] == "Lead"));
        assert_eq!(pipeline(&team).runs.last().unwrap().state, StageState::Revoked);
        assert!(run(&mut team, "skip-stage", json!({"task":1,"reason":"x"}), &member("lead")).is_err(), "a decided task has no stage");
        // The owner skips the last stage too: the lead's acceptance is next.
        let mut team = team_with("code", &builtin());
        let attempt = hand(&mut team);
        report(&mut team, &attempt);
        verdict(&mut team, "rev", "pass", &[]).unwrap();
        let (value, _) = feed(&mut team, "skip-stage", json!({"task":1,"reason":"No suites for docs"}), &Actor::Human);
        assert_eq!(value["status"], "submitted");
        assert_eq!(pipeline(&team).runs[1].by, "You");
    }

    #[test]
    fn the_lead_rejects_in_a_stage_and_a_stale_verdict_is_refused() {
        let mut team = team_with("code", &builtin());
        let attempt = hand(&mut team);
        report(&mut team, &attempt);
        let stage = give_stage(&mut team, 1, "rev").unwrap();
        run(&mut team, "reject", json!({"task":1,"reason":"Start over"}), &member("lead")).unwrap();
        let task = team.task(1).unwrap();
        assert_eq!((task.status, task.attempts[0].state), (TaskStatus::Pending, AttemptState::Rejected));
        assert_eq!(pipeline(&team).runs[0].state, StageState::Revoked);
        // The reviewer's verdict on the ended stage is stale, not a decision.
        let value = run(&mut team, "report", json!({"task":1,"attempt":stage,"verdict":"pass"}), &member("rev")).unwrap();
        assert_eq!(value["status"], "stale");
        assert_eq!(team.task(1).unwrap().status, TaskStatus::Pending);
        // A verdict without a stage attempt is refused.
        let error = run(&mut team, "report", json!({"task":1,"attempt":attempt,"verdict":"pass","text":"x"}), &member("dev")).unwrap_err();
        assert!(error.to_string().contains("A verdict reports a stage"), "{error}");
        // An unknown verdict is refused; a return needs its findings.
        let attempt = hand(&mut team);
        report(&mut team, &attempt);
        let stage = give_stage(&mut team, 1, "rev").unwrap();
        assert!(run(&mut team, "report", json!({"task":1,"attempt":stage,"verdict":"maybe"}), &member("rev")).is_err());
        assert!(run(&mut team, "report", json!({"task":1,"attempt":stage,"verdict":"return"}), &member("rev")).unwrap_err().to_string().contains("names its findings"));
    }

    #[test]
    fn a_lead_pipeline_is_not_accepted_by_a_sub_lead() {
        let mut team = team_with("code", &builtin());
        // rev2 becomes the sub-lead dev reports to.
        team.members.iter_mut().find(|member| member.id == "rev2").unwrap().sub = true;
        team.members.iter_mut().find(|member| member.id == "dev").unwrap().parent = "rev2".into();
        let attempt = hand(&mut team);
        report(&mut team, &attempt);
        let error = run(&mut team, "accept", json!({"task":1}), &member("rev2")).unwrap_err();
        assert!(error.to_string().contains("accepted by the lead"), "{error}");
        // A sub_lead pipeline is the sub-lead's to accept.
        team.task_mut(1).unwrap().pipeline_run.as_mut().unwrap().accept = "sub_lead".into();
        run(&mut team, "accept", json!({"task":1}), &member("rev2")).unwrap();
        assert_eq!(team.task(1).unwrap().status, TaskStatus::Accepted);
    }

    #[test]
    fn the_implementer_never_takes_its_own_stage() {
        let mut catalog = builtin();
        // Even a stage pinned to the implementer waits for someone else.
        catalog.pipelines.push(Pipeline {
            id: "self".into(),
            name: "Self".into(),
            stages: vec![Stage { kind: "review".into(), role: String::new(), member: "dev".into() }],
            ..Default::default()
        });
        let mut team = team_with("code", &catalog);
        let attempt = hand(&mut team);
        report(&mut team, &attempt);
        assert_eq!(candidates(&team, 1), ["rev", "rev2"]);
        assert!(give_stage(&mut team, 1, "dev").is_err());
        assert!(give_stage(&mut team, 1, "qa").is_err(), "a tester does not take a review");
        // The stage's member reports its verdict; nobody else does.
        let stage = give_stage(&mut team, 1, "rev").unwrap();
        for who in ["rev2", "dev"] {
            let error = run(&mut team, "report", json!({"task":1,"attempt":stage,"verdict":"pass"}), &member(who)).unwrap_err();
            assert!(error.to_string().contains("was given to Reviewer"), "{who}: {error}");
        }
        assert!(run(&mut team, "report", json!({"task":1,"attempt":stage,"verdict":"pass"}), &Actor::Human).is_err());
        let mut pinned = team_with("self", &catalog);
        let attempt = hand(&mut pinned);
        report(&mut pinned, &attempt);
        assert_eq!(pipeline(&pinned).open_run().unwrap().member, "");
        assert!(candidates(&pinned, 1).is_empty());
    }

    #[test]
    fn the_task_keeps_the_pipeline_it_was_assigned_with() {
        let mut catalog = builtin();
        let mut team = team_with("code", &catalog);
        // A later catalog edit changes no running task.
        catalog.pipelines[0].stages.clear();
        catalog.pipelines[0].max_rounds = 9;
        snapshot_with(&mut team, 1, &catalog);
        assert_eq!((pipeline(&team).stages.len(), pipeline(&team).max_rounds), (2, 3));
        // A task handed out before it had a pipeline keeps today's flow.
        let mut old = team_with("", &catalog);
        hand(&mut old);
        old.task_mut(1).unwrap().pipeline = "code".into();
        snapshot_with(&mut old, 1, &builtin());
        assert!(old.task(1).unwrap().pipeline_run.is_none());
        // The task's choice, else its role's default, else its template's;
        // "none" overrides both.
        let mut catalog = builtin();
        catalog.roles.iter_mut().find(|role| role.id == "backend").unwrap().pipeline = "docs".into();
        catalog.templates[0].pipeline = "code".into();
        let team = team_with("", &catalog);
        assert_eq!(pipeline(&team).id, "docs");
        catalog.roles.iter_mut().find(|role| role.id == "backend").unwrap().pipeline.clear();
        let mut team = team_with("", &catalog);
        assert!(team.task(1).unwrap().pipeline_run.is_none(), "the team's template is not feature");
        team.template = catalog.templates[0].id.clone();
        snapshot_with(&mut team, 1, &catalog);
        assert_eq!(pipeline(&team).id, "code");
        let mut none = team_with("none", &catalog);
        none.template = catalog.templates[0].id.clone();
        snapshot_with(&mut none, 1, &catalog);
        assert!(none.task(1).unwrap().pipeline_run.is_none());
        // The lead's own task and a check never take one.
        let mut lead_task = team_with("code", &builtin());
        lead_task.task_mut(1).unwrap().assignee = "lead".into();
        assert!(resolve(&builtin(), &lead_task, lead_task.task(1).unwrap()).is_none());
        lead_task.task_mut(1).unwrap().assignee = "dev".into();
        lead_task.task_mut(1).unwrap().kind = TaskKind::Check;
        assert!(resolve(&builtin(), &lead_task, lead_task.task(1).unwrap()).is_none());
    }

    #[test]
    fn an_old_team_file_keeps_todays_flow_and_shape() {
        // A team stored before pipelines: no pipeline fields anywhere.
        let mut team = team_with("", &Catalog::default());
        let stored = serde_json::to_string(&team).unwrap();
        assert!(!stored.contains("pipeline") && !stored.contains("in_review"), "{stored}");
        team = serde_json::from_str(&stored).unwrap();
        let attempt = hand(&mut team);
        assert_eq!(report(&mut team, &attempt)["status"], "submitted");
        let task = team.task(1).unwrap();
        assert_eq!((task.status, task.attempts[0].notice), (TaskStatus::Submitted, Notice::Pending));
        assert!(run(&mut team, "send-back", json!({"task":1,"reason":"x"}), &member("lead")).is_err(), "no pipeline to send back in");
        assert!(run(&mut team, "skip-stage", json!({"task":1,"reason":"x"}), &member("lead")).is_err());
        run(&mut team, "accept", json!({"task":1}), &member("lead")).unwrap();
        assert!(!serde_json::to_string(&team).unwrap().contains("pipeline"));
    }

    #[test]
    fn the_plan_and_edit_draft_name_a_pipeline_but_never_a_run() {
        let (mut team, _) = create("t", &json!({"name":"T","lead":{"id":"lead","agent":"claude"}})).unwrap();
        let error = run(&mut team, "plan", json!({"tasks":[{"id":1,"title":"A","pipeline":"nope-x"}]}), &Actor::Human).unwrap_err();
        assert!(error.to_string().contains("Unknown pipeline nope-x"), "{error}");
        run(
            &mut team,
            "plan",
            json!({"members":[{"id":"dev","role":"Backend","agent":"codex"}],
                "tasks":[{"id":1,"title":"A","assignee":"dev","pipeline":"none",
                    "pipeline_run":{"id":"x","name":"x","max_rounds":1,"folder":"hold","accept":"auto"}}]}),
            &Actor::Human,
        )
        .unwrap();
        let task = team.task(1).unwrap();
        assert_eq!((task.pipeline.as_str(), task.pipeline_run.is_none()), ("none", true));
        run(&mut team, "approve", json!({}), &Actor::Human).unwrap();
        let lead = member("lead");
        run(&mut team, "extend", json!({"tasks":[{"id":2,"title":"B","assignee":"dev","pipeline":"none"}]}), &lead).unwrap();
        let phase = team.task(2).unwrap().phase;
        assert_eq!(team.task(2).unwrap().pipeline, "none");
        let (value, _) = feed(&mut team, "edit-draft", json!({"phase":phase,"tasks_update":[{"id":2,"pipeline":""}]}), &lead);
        assert_eq!(value["diff"]["updated"][0]["fields"], json!(["pipeline"]));
        assert_eq!(team.task(2).unwrap().pipeline, "");
        assert!(run(&mut team, "edit-draft", json!({"phase":phase,"tasks_update":[{"id":2,"pipeline":"nope-x"}]}), &lead).is_err());
    }

    #[test]
    fn stage_spans_wait_until_delivery_and_work_until_the_verdict() {
        let mut team = team_with("code", &builtin());
        let attempt = hand(&mut team);
        report(&mut team, &attempt);
        team.task_mut(1).unwrap().pipeline_run.as_mut().unwrap().open_run_mut().unwrap().at = 100.0;
        give_stage(&mut team, 1, "rev").unwrap();
        team.task_mut(1).unwrap().pipeline_run.as_mut().unwrap().open_run_mut().unwrap().delivered_at = Some(160.0);
        let open = spans(team.task(1).unwrap(), 400.0);
        assert_eq!((open[0]["waited_s"].clone(), open[0]["worked_s"].clone(), open[0]["running"].clone()), (json!(60.0), json!(240.0), json!(true)));
        let task = team.task_mut(1).unwrap();
        close_run(task, StageState::Passed, "Reviewer", "", Vec::new());
        task.pipeline_run.as_mut().unwrap().runs[0].ended_at = Some(460.0);
        let done = spans(team.task(1).unwrap(), 9999.0);
        assert_eq!((done[0]["worked_s"].clone(), done[0]["running"].clone()), (json!(300.0), json!(false)));
    }

    /// Four tasks of one team, as status lists them: in review, in test,
    /// returned to the implementer, and at the lead after the round limit.
    fn fixture_tasks() -> Vec<Task> {
        let mut team = team_with("code", &builtin());
        team.tasks[0].description = "Paged list of saved cards".into();
        let at_stage = |team: &mut Team, passes: usize, returns: usize| {
            let attempt = hand(team);
            report(team, &attempt);
            for _ in 0..passes {
                verdict(team, "rev", "pass", &[]).unwrap();
            }
            for round in 0..returns {
                verdict(team, "rev", "return", &["Page size is unbounded"]).unwrap();
                if round + 1 < returns || team.task(1).unwrap().status == TaskStatus::Pending {
                    let attempt = hand(team);
                    if round + 1 < returns {
                        report(team, &attempt);
                    }
                }
            }
            team.task(1).unwrap().clone()
        };
        let mut review = at_stage(&mut team.clone(), 0, 0);
        let mut test = at_stage(&mut team.clone(), 1, 0);
        test.id = 2;
        let mut returned = at_stage(&mut team.clone(), 0, 1);
        returned.id = 3;
        returned.status = TaskStatus::Pending;
        returned.attempts.truncate(1);
        let mut limit = at_stage(&mut team, 0, 3);
        limit.id = 4;
        // Fixed ids and times, so the file is stable.
        for (n, task) in [&mut review, &mut test, &mut returned, &mut limit].into_iter().enumerate() {
            for (i, attempt) in task.attempts.iter_mut().enumerate() {
                attempt.id = format!("00000000-0000-4000-8000-0000000{:02}{:03}", n + 1, i + 1);
                if let Some(report) = attempt.report.as_mut() {
                    report.at = 1_800_000_100.0 + i as f64 * 1000.0;
                }
            }
            let run = task.pipeline_run.as_mut().unwrap();
            for (i, stage) in run.runs.iter_mut().enumerate() {
                stage.id = format!("00000000-0000-4000-9000-0000000{:02}{:03}", n + 1, i + 1);
                stage.at = 1_800_000_200.0 + i as f64 * 1000.0;
                stage.delivered_at = stage.delivered_at.map(|_| stage.at + 60.0);
                stage.ended_at = stage.ended_at.map(|_| stage.at + 600.0);
            }
        }
        vec![review, test, returned, limit]
    }

    /// The shared fixture the desktop and Android read for the stage statuses
    /// and a task's pipeline is exactly what the model writes; with
    /// ZERUS_WRITE_FIXTURES=1 the test writes it.
    #[test]
    fn the_shared_pipeline_fixture_matches_the_model() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/team-pipeline-tasks.json");
        if std::env::var("ZERUS_WRITE_FIXTURES").as_deref() == Ok("1") {
            let text = serde_json::to_string_pretty(&json!({"tasks":fixture_tasks()})).unwrap() + "
";
            std::fs::write(&path, text).unwrap();
        }
        assert_eq!(serde_json::to_value(fixture_tasks()).unwrap(), serde_json::from_str::<Value>(&std::fs::read_to_string(&path).unwrap()).unwrap()["tasks"]);
        let fixture: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let tasks: Vec<Task> = serde_json::from_value(fixture["tasks"].clone()).unwrap();
        let statuses: Vec<TaskStatus> = tasks.iter().map(|task| task.status).collect();
        assert_eq!(statuses, [TaskStatus::InReview, TaskStatus::InTest, TaskStatus::Pending, TaskStatus::Submitted]);
        assert_eq!(serde_json::to_value(&tasks).unwrap(), fixture["tasks"], "the fixture is what the server writes");
        let first = tasks[0].pipeline_run.as_ref().unwrap();
        assert_eq!((first.open_run().unwrap().kind.as_str(), first.round), ("review", 1));
        assert_eq!(tasks[2].pipeline_run.as_ref().unwrap().returned(), 1);
        assert_eq!(tasks[3].pipeline_run.as_ref().unwrap().returned(), 3);
    }

    #[test]
    fn the_catalog_checks_its_pipelines() {
        crate::state::teams::catalog::validate(&builtin()).unwrap();
        let broken = |change: &dyn Fn(&mut Catalog)| {
            let mut catalog = builtin();
            change(&mut catalog);
            crate::state::teams::catalog::validate(&catalog).unwrap_err().to_string()
        };
        assert!(broken(&|c| c.pipelines[0].stages[0].kind = "lint".into()).contains("kind is review or test"));
        assert!(broken(&|c| c.pipelines[0].stages[0].role = "ghost".into()).contains("unknown role ghost"));
        assert!(broken(&|c| c.pipelines[0].stages[0].member = "rev".into()).contains("name a role or a member"));
        assert!(broken(&|c| c.pipelines[0].accept = "owner".into()).contains("accept is lead, sub_lead or auto"));
        assert!(broken(&|c| c.pipelines[0].folder = "copy".into()).contains("folder is hold or checkout"));
        assert!(broken(&|c| c.pipelines[1].id = "code".into()).contains("listed twice"));
        assert!(broken(&|c| c.pipelines[0].id = "none".into()).contains("reserved"));
        assert!(broken(&|c| c.templates[0].pipeline = "ghost".into()).contains("unknown pipeline ghost"));
        assert!(broken(&|c| c.roles[0].pipeline = "ghost".into()).contains("unknown pipeline ghost"));
        // An old catalog without pipelines still loads, with none.
        let old: Catalog = serde_json::from_value(json!({"version":2,"roles":[],"templates":[],"routing":[]})).unwrap();
        assert!(old.pipelines.is_empty());
        crate::state::teams::catalog::validate(&old).unwrap();
        assert!(check_choice("code", &old).is_err() && check_choice("none", &old).is_ok() && check_choice("", &old).is_ok());
        assert!(!serde_json::to_string(&old).unwrap().contains("pipelines"), "an old catalog saves as it was");
    }
}
