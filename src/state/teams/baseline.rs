//! Task 81: lateness against the plan, overrun against the estimate.
//!
//! A task that by the plan starts only after its dependencies is not losing
//! time while those dependencies work on schedule. So the profile measures two
//! things apart from the causes of lost time:
//!
//! - LATENESS: how late a task started (and ended) against a baseline schedule
//!   fixed when its plan was approved. A task's planned start is the latest
//!   planned end of its hard dependencies and of its member's earlier task (and
//!   never before the baseline's moment); its planned end is that plus its
//!   estimate. The lateness the task inherits from a predecessor that ran late
//!   is told apart from its own.
//! - OVERRUN: the task's own work (every attempt, rework included) minus its
//!   estimate, and the ratio. Unknown without an estimate.
//!
//! A moment that needs an estimate nobody has is unknown, never a guess.
use super::estimates::{self, History};
use super::model::*;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

/// Whether a task waits for its baseline: in a running plan, in an approved
/// phase, not a draft, not started or finished (a task already running when
/// the field arrived has no planned start to be late against).
fn wants(team: &Team, task: &Task, historical: bool) -> bool {
    task.baseline.is_none()
        && task.status != TaskStatus::Draft
        && (historical || (task.attempts.is_empty() && !task.status.finished()))
        && team.phases.iter().find(|phase| phase.n == task.phase).is_none_or(|phase| phase.state == PhaseState::Approved)
}

/// Task 94: when this task's plan was fixed: its phase's decision, the task's
/// late addition, else the team's approval. Old records have no phase moment.
fn moment(team: &Team, task: &Task) -> f64 {
    team.phases
        .iter()
        .find(|phase| phase.n == task.phase)
        .and_then(|phase| phase.decided_at)
        .or_else(|| task.added_late.as_ref().map(|late| late.at).filter(|at| *at > 0.0))
        .or(team.approved_at)
        .unwrap_or(0.0)
}

/// Fixes the baseline of every task of an approved, running plan that has none:
/// the moment its phase is approved, or the task is added later. A baseline
/// once fixed is kept through later plan edits.
pub(crate) fn backfill(team: &mut Team, now: f64) { stamp_with(team, now, true); }

pub(crate) fn stamp(team: &mut Team, now: f64) { stamp_with(team, now, false); }

fn stamp_with(team: &mut Team, now: f64, historical: bool) {
    if team.approved_at.is_none() || !matches!(team.status, TeamStatus::Running | TeamStatus::Paused) {
        return;
    }
    let wanted: Vec<u32> = team.tasks.iter().filter(|task| wants(team, task, historical)).map(|task| task.id).collect();
    if wanted.is_empty() {
        return;
    }
    let history = estimates::load();
    let mut planned: BTreeMap<u32, Baseline> = BTreeMap::new();
    for id in &wanted {
        plan_of(team, &history, &wanted, *id, now, historical, &mut planned, &mut BTreeSet::new());
    }
    for (id, baseline) in planned {
        if let Some(task) = team.task_mut(id) {
            task.baseline = Some(baseline);
        }
    }
}

/// The tasks `task` is planned to follow and are still ahead: its hard
/// dependencies, its member's previous task, and (in a barrier phase) every
/// task of the lower phases. A finished one is no longer ahead.
fn ahead(team: &Team, task: &Task, historical: bool) -> Vec<u32> {
    let mut out: BTreeSet<u32> = BTreeSet::new();
    for dep in task.deps.iter().filter(|dep| dep.kind == DependencyKind::Hard) {
        out.insert(dep.task);
    }
    if !task.assignee.is_empty() {
        if let Some(previous) = team
            .tasks
            .iter()
            .filter(|other| other.id < task.id && other.assignee == task.assignee && (historical || !other.status.finished()) && other.status != TaskStatus::Draft)
            .map(|other| other.id)
            .max()
        {
            out.insert(previous);
        }
    }
    if team.phases.iter().any(|phase| phase.n == task.phase && phase.barrier) {
        out.extend(team.tasks.iter().filter(|other| other.phase < task.phase && other.status != TaskStatus::Draft).map(|other| other.id));
    }
    out.retain(|id| *id != task.id && team.task(*id).is_some_and(|other| historical || !other.status.finished()));
    out.into_iter().collect()
}

/// The baseline of `id`, planned (and memoized) after those it follows.
#[expect(clippy::too_many_arguments, reason = "Keep the recursive schedule inputs and traversal state explicit")]
fn plan_of(
    team: &Team,
    history: &History,
    wanted: &[u32],
    id: u32,
    now: f64,
    historical: bool,
    planned: &mut BTreeMap<u32, Baseline>,
    visiting: &mut BTreeSet<u32>,
) -> Option<Baseline> {
    if let Some(found) = planned.get(&id) {
        return Some(found.clone());
    }
    let task = team.task(id)?;
    if !wanted.contains(&id) {
        return task.baseline.clone();
    }
    let after = ahead(team, task, historical);
    let estimate_s = estimates::estimate_from(history, team, task).minutes.map(|minutes| minutes * 60.0);
    // A cycle in the dependencies has no schedule: its tasks start unknown.
    let at = if historical { moment(team, task) } else { now };
    let mut start = if visiting.insert(id) { Some(at) } else { None };
    if start.is_some() {
        for before in &after {
            let end = plan_of(team, history, wanted, *before, now, historical, planned, visiting).and_then(|found| found.end);
            start = match (start, end) {
                (Some(start), Some(end)) => Some(start.max(end)),
                _ => None,
            };
        }
        visiting.remove(&id);
    }
    let baseline = Baseline { at, estimate_s, start, end: start.zip(estimate_s).map(|(start, estimate)| start + estimate), after, backfilled: historical && !task.attempts.is_empty() };
    planned.insert(id, baseline.clone());
    Some(baseline)
}

fn seconds(value: f64) -> i64 {
    value.max(0.0).round() as i64
}

/// When a task began: its first attempt's hand-out.
fn actual_start(task: &Task) -> Option<f64> {
    task.attempts.first().map(|attempt| attempt.delivered_at.or(attempt.started_at).unwrap_or(attempt.sent_at))
}

/// When an accepted task became available to the tasks behind it.
fn available_at(task: &Task) -> Option<f64> {
    if task.status != TaskStatus::Accepted {
        return None;
    }
    task.attempts
        .iter()
        .filter_map(|attempt| attempt.decided_at.or(attempt.report.as_ref().map(|report| report.at)))
        .reduce(f64::max)
}

/// How late the tasks behind `task` have become because of it: it ended (was
/// accepted) after its planned end, or is still running past it.
fn delay(task: &Task, now: f64) -> f64 {
    let Some(end) = task.baseline.as_ref().and_then(|baseline| baseline.end) else { return 0.0 };
    match task.status {
        TaskStatus::Accepted => available_at(task).map_or(0.0, |at| (at - end).max(0.0)),
        status if !status.finished() && status != TaskStatus::Draft => (now - end).max(0.0),
        _ => 0.0,
    }
}

/// The task's lateness at `now` against its baseline, or null when it has none
/// (an old task, one already running when baselines came, a draft).
pub(crate) fn lateness(team: &Team, task: &Task, now: f64) -> Value {
    let Some(baseline) = &task.baseline else { return Value::Null };
    let unknown = |reason: &str| {
        json!({"planned_start":baseline.start,"planned_end":baseline.end,"start_late_s":Value::Null,"inherited_s":Value::Null,
            "own_s":Value::Null,"end_late_s":Value::Null,"unknown":reason})
    };
    let Some(planned_start) = baseline.start else { return unknown("An estimate on the way is missing") };
    let live = !task.status.finished() && task.status != TaskStatus::Draft;
    let late = match actual_start(task) {
        Some(start) => (start - planned_start).max(0.0),
        // Not started: late only from the planned start on, and only while it can still start.
        None if live => (now - planned_start).max(0.0),
        None => 0.0,
    };
    // The most any predecessor the task was planned to follow is behind: that
    // much of its lateness is inherited, the rest is its own. Task 82: `from`
    // names that predecessor (the lower id on a tie), for "late 20m from #3".
    let (from, behind) = baseline
        .after
        .iter()
        .filter_map(|id| team.task(*id))
        .map(|before| (before.id, delay(before, now)))
        .fold((None, 0.0), |(id, most), (next, by)| if by > most { (Some(next), by) } else { (id, most) });
    let inherited = late.min(behind);
    let end_late = baseline.end.and_then(|end| {
        let reported = task.attempts.iter().rev().find_map(|attempt| attempt.report.as_ref().map(|report| report.at));
        match (reported, actual_start(task)) {
            (Some(at), _) if task.status != TaskStatus::InProgress && task.status != TaskStatus::Assigned => Some((at - end).max(0.0)),
            (_, Some(_)) if live => Some((now - end).max(0.0)),
            _ => None,
        }
    });
    // Task 82: the actual start and end for the heat's tooltip (the end: the
    // last report of a task that is no longer working).
    let actual_end = task.attempts.iter().rev().find_map(|attempt| attempt.report.as_ref().map(|report| report.at))
        .filter(|_| task.status != TaskStatus::InProgress && task.status != TaskStatus::Assigned);
    json!({"planned_start":planned_start,"planned_end":baseline.end,"start_late_s":seconds(late),"inherited_s":seconds(inherited),
        "own_s":seconds(late) - seconds(inherited),"end_late_s":end_late.map(seconds),
        "from":if inherited > 0.0 { from } else { None },"actual_start":actual_start(task),"actual_end":actual_end})
}

/// The estimate a task's overrun is measured against: the baseline's, else the
/// current one (read once from the history).
fn estimate_s(team: &Team, task: &Task, history: &mut Option<History>) -> Option<f64> {
    if let Some(estimate) = task.baseline.as_ref().and_then(|baseline| baseline.estimate_s) {
        return Some(estimate);
    }
    let history = history.get_or_insert_with(estimates::load);
    estimates::estimate_from(history, team, task).minutes.map(|minutes| minutes * 60.0)
}

/// The task's own work (`work_s`: every attempt, rework included) against its
/// estimate: `overrun_s` is signed (negative: under), `ratio` is work over
/// estimate. Both are null without an estimate, never zero.
pub(crate) fn overrun(team: &Team, task: &Task, work_s: i64, history: &mut Option<History>) -> Value {
    if task.attempts.is_empty() {
        return json!({"not_started":true});
    }
    let running = !task.status.finished();
    match estimate_s(team, task, history).filter(|estimate| *estimate > 0.0) {
        Some(estimate) => json!({"estimate_s":estimate.round() as i64,"work_s":work_s,"overrun_s":work_s - estimate.round() as i64,
            "ratio":((work_s as f64 / estimate) * 1000.0).round() / 1000.0,"running":running}),
        None => json!({"estimate_s":Value::Null,"work_s":work_s,"overrun_s":Value::Null,"ratio":Value::Null,"running":running,
            "unknown":"No estimate"}),
    }
}

#[cfg(test)]
#[path = "baseline_tests.rs"]
mod tests;






