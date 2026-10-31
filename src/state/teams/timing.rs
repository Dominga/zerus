//! WhiteWh fork: how long a task has worked (U37). Status gives every task a
//! `timing` object computed here from its attempts, so the desktop, Android
//! and the profile of U38 read one number: the working spans (started, else
//! delivered, to the report or the turn end), the attempt running now, and
//! the waits for dependencies, a free member and the lead, counted apart.
//! What was never recorded is named in `unknown`, never guessed.
use super::model::{AttemptState, DependencyKind, PhaseState, Task, TaskStatus, Team};
use serde_json::{json, Value};
use std::collections::BTreeSet;

/// A moment the timeline needs: recorded, not reached yet, or never recorded.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Moment {
    At(f64),
    NotYet,
    Unknown,
}

/// When the task's phase opened: the plan's approval for phase 1, the
/// phase's own approval after it.
pub(super) fn opened(team: &Team, task: &Task) -> Moment {
    if task.phase <= 1 {
        return if team.approved_by.is_empty() && team.approved_at.is_none() {
            Moment::NotYet
        } else {
            team.approved_at.map_or(Moment::Unknown, Moment::At)
        };
    }
    match team.phases.iter().find(|phase| phase.n == task.phase) {
        Some(phase) if phase.state == PhaseState::Approved => phase.decided_at.map_or(Moment::Unknown, Moment::At),
        _ => Moment::NotYet,
    }
}

/// When a dependency let the task start: a hard one at its acceptance, a soft
/// one once it was first handed out.
fn satisfied(team: &Team, id: u32, kind: DependencyKind) -> Moment {
    let Some(dep) = team.task(id) else { return Moment::NotYet };
    if !super::plan::satisfied(team, id, kind) {
        return Moment::NotYet;
    }
    // #759: a cancelled dependency released it; when is not recorded.
    if dep.status == super::model::TaskStatus::Cancelled {
        return Moment::Unknown;
    }
    let at = if team.settings.all_hard || kind == DependencyKind::Hard {
        dep.attempts.iter().rev().find_map(|attempt| attempt.decided_at)
    } else {
        dep.attempts.first().map(|attempt| attempt.sent_at)
    };
    at.map_or(Moment::Unknown, Moment::At)
}

/// When every dependency and the phase let the task start.
pub(super) fn ready(team: &Team, task: &Task, opened: Moment) -> Moment {
    let mut at = opened;
    for dep in &task.deps {
        at = match (at, satisfied(team, dep.task, dep.kind)) {
            (Moment::NotYet, _) | (_, Moment::NotYet) => Moment::NotYet,
            (Moment::Unknown, _) | (_, Moment::Unknown) => Moment::Unknown,
            (Moment::At(a), Moment::At(b)) => Moment::At(a.max(b)),
        };
    }
    at
}

/// The length of [from, to] inside [low, high].
fn overlap(from: f64, to: f64, low: f64, high: f64) -> f64 {
    (to.min(high) - from.max(low)).max(0.0)
}

fn seconds(value: f64) -> i64 {
    value.max(0.0).round() as i64
}

/// The task's `timing` at `now`: {worked_s, running_since, waits: {deps_s,
/// member_s, lead_s}, attempts: [{n, member, state, from, to, worked_s,
/// member_s, lead_s}], unknown}. `worked_s` includes the running attempt up
/// to `now`; `to` is null while it runs.
pub(crate) fn timing(team: &Team, task: &Task, now: f64) -> Value {
    let live = !task.status.finished() && task.status != TaskStatus::Draft;
    let mut unknown = BTreeSet::new();
    let (mut worked, mut member, mut lead, mut deps) = (0.0, 0.0, 0.0, 0.0);
    let opened = opened(team, task);
    let ready = ready(team, task, opened);
    if !task.deps.is_empty() {
        match (opened, ready) {
            (Moment::At(open), Moment::At(ready)) => deps += ready - open,
            (Moment::At(open), Moment::NotYet) if live => deps += now - open,
            (Moment::Unknown, _) | (_, Moment::Unknown) => {
                unknown.insert("deps");
            }
            _ => {}
        }
    }
    // Waiting for a free member: from ready to the first hand-out.
    match (ready, task.attempts.first()) {
        (Moment::At(ready), Some(first)) => member += (first.sent_at - ready).max(0.0),
        (Moment::Unknown, Some(_)) => {
            unknown.insert("member");
        }
        _ => {}
    }
    let mut running_since = None;
    let mut rows = Vec::new();
    // Where the wait after the last attempt starts: its decision, else ready.
    let mut since = match ready {
        Moment::At(at) if task.attempts.is_empty() => Some(at),
        _ => None,
    };
    for (index, attempt) in task.attempts.iter().enumerate() {
        let next = task.attempts.get(index + 1);
        let open = attempt.state.open();
        let (mut row_worked, mut row_member, mut row_lead) = (0.0, 0.0, 0.0);
        let begin = attempt.started_at.or(attempt.delivered_at);
        match begin {
            Some(begin) => row_member += (begin - attempt.sent_at).max(0.0),
            // Not delivered yet: the member has not taken it.
            None if open => row_member += (now - attempt.sent_at).max(0.0),
            None => {}
        }
        // A captured report is the last reply: the work ended with the turn.
        let ends = [attempt.report.as_ref().map(|report| report.at), attempt.turn_ended_at];
        let mut end = ends.into_iter().flatten().reduce(f64::min);
        let running = begin.is_some() && end.is_none() && open;
        if begin.is_some() && end.is_none() && !open {
            // A revoked attempt without a report: the next hand-out ended it.
            end = next.map(|next| next.sent_at);
            if end.is_none() {
                unknown.insert("worked");
            }
        }
        if let Some(begin) = begin {
            let stop = if running { now } else { end.unwrap_or(begin) };
            row_worked += (stop - begin).max(0.0);
            if running {
                running_since = Some(begin);
            }
            // A question waits for the lead: not work.
            let reach = if open { now } else { stop };
            for question in &attempt.questions {
                let answered = question.answered_at.unwrap_or(if open { now } else { stop });
                row_worked -= overlap(question.at, answered, begin, stop);
                row_lead += overlap(question.at, answered, begin, reach);
            }
        }
        // The lead's decision on the report.
        let decided = attempt.decided_at;
        if let Some(report) = &attempt.report {
            let undecided = attempt.state == AttemptState::Reported && task.status == TaskStatus::Submitted;
            match decided {
                Some(at) => row_lead += (at - report.at).max(0.0),
                None if undecided && next.is_none() => row_lead += (now - report.at).max(0.0),
                None if matches!(attempt.state, AttemptState::Reported | AttemptState::Rejected) => {
                    unknown.insert("lead");
                }
                None => {}
            }
        }
        // From the decision (or the revoked attempt's end) to the next hand-out.
        let after = decided.or(if attempt.state == AttemptState::Revoked { end } else { None });
        match (after, next) {
            (Some(after), Some(next)) => row_member += (next.sent_at - after).max(0.0),
            (Some(after), None) => since = Some(after),
            _ => {}
        }
        worked += row_worked;
        member += row_member;
        lead += row_lead;
        rows.push(json!({
            "n":attempt.n,"member":attempt.member,"state":attempt.state,
            "from":begin,"to":if running { None } else { end },
            "worked_s":seconds(row_worked),"member_s":seconds(row_member),"lead_s":seconds(row_lead)
        }));
    }
    // A pending task with no open attempt waits for a member, or for the lead
    // while it holds the task (A5).
    if live && task.status == TaskStatus::Pending && task.open_attempt().is_none() {
        if let Some(since) = since {
            match &task.hold {
                Some(hold) => {
                    member += (hold.at - since).max(0.0);
                    lead += (now - hold.at.max(since)).max(0.0);
                }
                None if ready != Moment::NotYet => member += (now - since).max(0.0),
                None => {}
            }
        }
    }
    let mut value = json!({
        "worked_s":seconds(worked),
        "running_since":running_since,
        "waits":{"deps_s":seconds(deps),"member_s":seconds(member),"lead_s":seconds(lead)},
        "attempts":rows,
    });
    if !unknown.is_empty() {
        value["unknown"] = json!(unknown);
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::teams::model::{Attempt, Dependency, Hold, Phase, Question, Report};

    fn attempt(n: u32, state: AttemptState, sent: f64, started: f64) -> Attempt {
        Attempt {
            n,
            id: format!("a{n}"),
            member: "dev".into(),
            state,
            sent_at: sent,
            delivered_at: Some(sent + 1.0),
            started_at: Some(started),
            ..Default::default()
        }
    }

    fn report(at: f64, captured: bool) -> Option<Report> {
        Some(Report { text: "Done".into(), at, captured, ..Default::default() })
    }

    fn task(id: u32, deps: &[u32], status: TaskStatus, attempts: Vec<Attempt>) -> Task {
        Task {
            id,
            phase: 1,
            title: format!("Task {id}"),
            assignee: "dev".into(),
            deps: deps.iter().map(|task| Dependency { task: *task, kind: DependencyKind::Hard }).collect(),
            status,
            attempts,
            ..Default::default()
        }
    }

    /// The plan approved at 100. #1 was rejected once and accepted on its
    /// second attempt; #2 depends on #1 and runs now.
    fn team() -> Team {
        let mut first = attempt(1, AttemptState::Rejected, 110.0, 115.0);
        first.report = report(400.0, false);
        first.turn_ended_at = Some(410.0);
        first.decided_at = Some(460.0);
        let mut second = attempt(2, AttemptState::Reported, 500.0, 505.0);
        second.questions = vec![Question { id: "q1".into(), text: "Which field?".into(), at: 600.0,
            answer: "The new one".into(), answered_at: Some(660.0), ..Default::default() }];
        second.turn_ended_at = Some(860.0);
        second.report = report(900.0, true);
        second.decided_at = Some(1000.0);
        let running = attempt(1, AttemptState::Delivered, 1010.0, 1020.0);
        Team {
            id: "t".into(),
            approved_by: "You".into(),
            approved_at: Some(100.0),
            tasks: vec![
                task(1, &[], TaskStatus::Accepted, vec![first, second]),
                task(2, &[1], TaskStatus::InProgress, vec![running]),
            ],
            ..Default::default()
        }
    }

    #[test]
    fn a_retried_task_sums_its_attempts_and_names_each_wait() {
        let team = team();
        let value = timing(&team, team.task(1).unwrap(), 5000.0);
        // Work: 115-400 is 285; 505-860 (the captured report ends with the
        // turn) is 355, less the question's 60, so 295. 285 + 295 = 580.
        assert_eq!(value["worked_s"], 580);
        assert_eq!(value["running_since"], Value::Null);
        // Member: ready 100 to hand-out 110, start delays 5 and 5, rejected
        // at 460 to the second hand-out at 500: 10 + 5 + 40 + 5.
        // Lead: the first decision 60, the question 60, the second decision
        // 100 (captured at 900, accepted at 1000).
        assert_eq!(value["waits"], json!({"deps_s":0,"member_s":60,"lead_s":220}));
        let rows = value["attempts"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!((rows[0]["from"].clone(), rows[0]["to"].clone()), (json!(115.0), json!(400.0)));
        assert_eq!((rows[0]["worked_s"].clone(), rows[0]["member_s"].clone(), rows[0]["lead_s"].clone()), (json!(285), json!(45), json!(60)));
        assert_eq!((rows[1]["worked_s"].clone(), rows[1]["member_s"].clone(), rows[1]["lead_s"].clone()), (json!(295), json!(5), json!(160)));
        assert_eq!(rows[1]["state"], "reported");
        assert!(value.get("unknown").is_none(), "{value}");
        // A finished task's numbers do not grow with the clock.
        assert_eq!(timing(&team, team.task(1).unwrap(), 9000.0), value);
    }

    #[test]
    fn a_running_task_counts_since_its_attempt_started() {
        let team = team();
        let value = timing(&team, team.task(2).unwrap(), 1620.0);
        // #1 was accepted at 1000: #2 waited 900 for it from the plan's 100,
        // then 10 for the hand-out and 10 for the start; it works since 1020.
        assert_eq!(value["running_since"], json!(1020.0));
        assert_eq!(value["worked_s"], 600);
        assert_eq!(value["waits"], json!({"deps_s":900,"member_s":20,"lead_s":0}));
        assert_eq!(value["attempts"][0]["to"], Value::Null);
        assert_eq!(timing(&team, team.task(2).unwrap(), 1680.0)["worked_s"], 660);
    }

    #[test]
    fn waits_before_the_first_attempt_grow_until_it_is_handed_out() {
        let mut team = team();
        team.tasks.push(task(3, &[2], TaskStatus::Pending, vec![]));
        team.tasks.push(task(4, &[], TaskStatus::Pending, vec![]));
        // #3 still waits for #2: dependencies since the plan's 100.
        let value = timing(&team, team.task(3).unwrap(), 2000.0);
        assert_eq!(value["waits"], json!({"deps_s":1900,"member_s":0,"lead_s":0}));
        assert_eq!((value["worked_s"].clone(), value["running_since"].clone()), (json!(0), Value::Null));
        // #4 was ready at once and waits for a free member.
        assert_eq!(timing(&team, team.task(4).unwrap(), 2000.0)["waits"], json!({"deps_s":0,"member_s":1900,"lead_s":0}));
        // Held by the lead at 1500: the member wait stops there (A5).
        team.task_mut(4).unwrap().hold = Some(Hold { by: "Lead".into(), at: 1500.0, reason: "Later".into() });
        assert_eq!(timing(&team, team.task(4).unwrap(), 2000.0)["waits"], json!({"deps_s":0,"member_s":1400,"lead_s":500}));
    }

    #[test]
    fn a_report_waiting_for_the_lead_and_an_open_question_count_for_the_lead() {
        let mut team = team();
        let mut asking = attempt(1, AttemptState::Delivered, 200.0, 210.0);
        asking.questions = vec![Question { id: "q1".into(), text: "Which DB?".into(), at: 300.0, ..Default::default() }];
        asking.turn_ended_at = Some(305.0);
        team.tasks.push(task(5, &[], TaskStatus::InProgress, vec![asking]));
        let value = timing(&team, team.task(5).unwrap(), 500.0);
        // The turn ended with the question: not running; 90 worked, 5 of them
        // after the question was asked; the lead owes 200 so far.
        assert_eq!(value["running_since"], Value::Null);
        assert_eq!(value["worked_s"], 90);
        assert_eq!(value["waits"], json!({"deps_s":0,"member_s":110,"lead_s":200}));
        let mut reported = attempt(1, AttemptState::Reported, 200.0, 210.0);
        reported.report = report(400.0, false);
        team.tasks.push(task(6, &[], TaskStatus::Submitted, vec![reported]));
        assert_eq!(timing(&team, team.task(6).unwrap(), 700.0)["waits"]["lead_s"], 300);
    }

    #[test]
    fn a_later_phase_opens_at_its_approval_and_a_draft_one_waits_for_nothing() {
        let mut team = team();
        team.phases.push(Phase { n: 2, state: PhaseState::Approved, decided_at: Some(3000.0), ..Default::default() });
        team.phases.push(Phase { n: 3, state: PhaseState::Draft, ..Default::default() });
        let mut later = task(7, &[], TaskStatus::InProgress, vec![attempt(1, AttemptState::Delivered, 3100.0, 3110.0)]);
        later.phase = 2;
        team.tasks.push(later);
        let mut draft = task(8, &[1], TaskStatus::Draft, vec![]);
        draft.phase = 3;
        team.tasks.push(draft);
        assert_eq!(timing(&team, team.task(7).unwrap(), 3200.0)["waits"], json!({"deps_s":0,"member_s":110,"lead_s":0}));
        assert_eq!(timing(&team, team.task(8).unwrap(), 3200.0)["waits"], json!({"deps_s":0,"member_s":0,"lead_s":0}));
    }

    #[test]
    fn a_revoked_attempt_ends_at_the_next_hand_out() {
        let mut team = team();
        let first = attempt(1, AttemptState::Revoked, 200.0, 210.0);
        let second = attempt(2, AttemptState::Delivered, 400.0, 420.0);
        team.tasks.push(task(9, &[], TaskStatus::InProgress, vec![first, second]));
        let value = timing(&team, team.task(9).unwrap(), 500.0);
        // 210-400 and 420-500: 190 + 80; member 100 + 10 + 20.
        assert_eq!(value["worked_s"], 270);
        assert_eq!(value["waits"], json!({"deps_s":0,"member_s":130,"lead_s":0}));
        assert_eq!(value["attempts"][0]["to"], json!(400.0));
    }

    #[test]
    fn an_old_team_file_counts_what_it_recorded_and_names_the_rest() {
        let old = json!({
            "id":"old","name":"Old team","lead":"lead","status":"running","approved_by":"You",
            "members":[{"id":"lead","role":"Lead","agent":"claude"},{"id":"dev","role":"Backend","agent":"codex"}],
            "tasks":[
                {"id":1,"title":"API","assignee":"dev","status":"accepted",
                    "attempts":[{"n":1,"id":"00000000-0000-4000-8000-000000000001","member":"dev",
                        "state":"reported","sent_at":10.0,"delivered_at":11.0,"turn_ended_at":20.0,
                        "report":{"text":"Done","at":30.0}}]},
                {"id":2,"title":"UI","assignee":"dev","status":"pending","deps":[{"task":1}]}]
        });
        let team: Team = serde_json::from_value(old).unwrap();
        // Delivered at 11 (no start was recorded) to the turn end at 20.
        let value = timing(&team, team.task(1).unwrap(), 100.0);
        assert_eq!(value["worked_s"], 9);
        assert_eq!(value["waits"], json!({"deps_s":0,"member_s":1,"lead_s":0}));
        // Neither the plan's approval nor the decision was recorded.
        assert_eq!(value["unknown"], json!(["lead", "member"]));
        let value = timing(&team, team.task(2).unwrap(), 100.0);
        assert_eq!(value["unknown"], json!(["deps"]));
        assert_eq!(value["waits"], json!({"deps_s":0,"member_s":0,"lead_s":0}));
    }
}
