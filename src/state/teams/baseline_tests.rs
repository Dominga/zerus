use super::*;
use crate::state::teams::model::{Attempt, Dependency, Member, Report};

const T0: f64 = 10_000.0;

fn task(id: u32, assignee: &str, deps: &[u32], minutes: Option<f64>) -> Task {
    Task {
        id,
        title: format!("Task {id}"),
        assignee: assignee.into(),
        phase: 1,
        status: TaskStatus::Pending,
        estimate_minutes: minutes,
        deps: deps.iter().map(|task| Dependency { task: *task, kind: DependencyKind::Hard }).collect(),
        ..Default::default()
    }
}

fn team(tasks: Vec<Task>) -> Team {
    Team {
        id: "t".into(),
        lead: "lead".into(),
        status: TeamStatus::Running,
        approved_at: Some(T0),
        members: ["lead", "dev", "qa"].iter().map(|id| Member { id: (*id).into(), role: id.to_string(), ..Default::default() }).collect(),
        tasks,
        ..Default::default()
    }
}

fn stamped(tasks: Vec<Task>) -> Team {
    let mut team = team(tasks);
    let temp = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(temp.path(), || stamp(&mut team, T0));
    team
}

/// The task worked from `from` to `reported` and was accepted at `decided`.
fn accept(team: &mut Team, id: u32, from: f64, reported: f64, decided: f64) {
    let task = team.task_mut(id).unwrap();
    task.status = TaskStatus::Accepted;
    task.attempts = vec![Attempt {
        n: 1,
        member: task.assignee.clone(),
        sent_at: from,
        delivered_at: Some(from),
        report: Some(Report { text: "done".into(), at: reported, ..Default::default() }),
        decided_at: Some(decided),
        ..Default::default()
    }];
}

fn start(team: &mut Team, id: u32, at: f64) {
    let task = team.task_mut(id).unwrap();
    task.status = TaskStatus::InProgress;
    task.attempts = vec![Attempt { n: 1, member: task.assignee.clone(), sent_at: at, delivered_at: Some(at), ..Default::default() }];
}

fn numbers(value: &Value) -> (Option<i64>, Option<i64>, Option<i64>, Option<i64>) {
    (value["start_late_s"].as_i64(), value["inherited_s"].as_i64(), value["own_s"].as_i64(), value["end_late_s"].as_i64())
}

#[test]
fn the_baseline_plans_a_chain_after_its_dependencies_and_its_members_earlier_task() {
    // 1 (dev, 10 min) <- 2 (qa, 20 min); 3 (dev, 5 min) follows dev's task 1.
    let team = stamped(vec![task(1, "dev", &[], Some(10.0)), task(2, "qa", &[1], Some(20.0)), task(3, "dev", &[], Some(5.0))]);
    let at = |id: u32| {
        let baseline = team.task(id).unwrap().baseline.clone().unwrap();
        (baseline.start, baseline.end)
    };
    assert_eq!(at(1), (Some(T0), Some(T0 + 600.0)));
    assert_eq!(at(2), (Some(T0 + 600.0), Some(T0 + 1800.0)));
    assert_eq!(at(3), (Some(T0 + 600.0), Some(T0 + 900.0)));
    assert_eq!(team.task(2).unwrap().baseline.as_ref().unwrap().after, [1]);
    assert_eq!(team.task(3).unwrap().baseline.as_ref().unwrap().after, [1]);
}

#[test]
fn a_task_waiting_for_dependencies_that_finish_on_time_is_not_late() {
    let mut team = stamped(vec![task(1, "dev", &[], Some(10.0)), task(2, "qa", &[1], Some(10.0))]);
    start(&mut team, 1, T0);
    // While 1 works on schedule, 2 waits: no lateness at all.
    let waiting = lateness(&team, team.task(2).unwrap(), T0 + 500.0);
    assert_eq!(numbers(&waiting), (Some(0), Some(0), Some(0), None), "{waiting}");
    // 1 is accepted on time; 2 starts exactly at its planned start.
    accept(&mut team, 1, T0, T0 + 590.0, T0 + 600.0);
    start(&mut team, 2, T0 + 600.0);
    assert_eq!(numbers(&lateness(&team, team.task(2).unwrap(), T0 + 700.0)), (Some(0), Some(0), Some(0), Some(0)));
    // The finished one was on time too, at its end.
    assert_eq!(numbers(&lateness(&team, team.task(1).unwrap(), T0 + 700.0)), (Some(0), Some(0), Some(0), Some(0)));
}

#[test]
fn lateness_counts_from_the_planned_start_while_not_started() {
    let mut team = stamped(vec![task(1, "dev", &[], Some(10.0)), task(2, "qa", &[1], Some(10.0))]);
    accept(&mut team, 1, T0, T0 + 590.0, T0 + 600.0);
    // 2 was ready at its planned start and nobody took it: late by the time since, its own.
    let late = lateness(&team, team.task(2).unwrap(), T0 + 900.0);
    assert_eq!(numbers(&late), (Some(300), Some(0), Some(300), None), "{late}");
    assert_eq!(late["planned_start"], T0 + 600.0);
    // Started late, it stays late by the start.
    start(&mut team, 2, T0 + 1000.0);
    assert_eq!(numbers(&lateness(&team, team.task(2).unwrap(), T0 + 1100.0)), (Some(400), Some(0), Some(400), Some(0)));
}

#[test]
fn a_late_dependencys_lateness_is_inherited_and_the_tasks_own_is_apart() {
    let mut team = stamped(vec![task(1, "dev", &[], Some(10.0)), task(2, "qa", &[1], Some(10.0))]);
    start(&mut team, 1, T0);
    // 1 runs 400 s past its planned end (600): 2 waits, all of it inherited.
    let waiting = lateness(&team, team.task(2).unwrap(), T0 + 1000.0);
    assert_eq!(numbers(&waiting), (Some(400), Some(400), Some(0), None), "{waiting}");
    // 1 is accepted 500 s late; 2 starts 700 s late: 500 inherited, 200 its own.
    accept(&mut team, 1, T0, T0 + 1050.0, T0 + 1100.0);
    start(&mut team, 2, T0 + 1300.0);
    let late = lateness(&team, team.task(2).unwrap(), T0 + 1400.0);
    assert_eq!(numbers(&late), (Some(700), Some(500), Some(200), Some(200)), "{late}");
    // 1 itself ended late by its report (450 s) and was not late to start.
    assert_eq!(numbers(&lateness(&team, team.task(1).unwrap(), T0 + 1400.0)), (Some(0), Some(0), Some(0), Some(450)));
}

#[test]
fn a_member_busy_with_its_earlier_task_is_a_predecessor() {
    // dev works 1 then 3; neither depends on the other.
    let mut team = stamped(vec![task(1, "dev", &[], Some(10.0)), task(3, "dev", &[], Some(10.0))]);
    assert_eq!(team.task(3).unwrap().baseline.as_ref().unwrap().start, Some(T0 + 600.0));
    // 1 is accepted on time; 3 starts 300 s late with nobody to blame: its own.
    accept(&mut team, 1, T0, T0 + 590.0, T0 + 600.0);
    start(&mut team, 3, T0 + 900.0);
    assert_eq!(numbers(&lateness(&team, team.task(3).unwrap(), T0 + 950.0)), (Some(300), Some(0), Some(300), Some(0)));
    // Had 1 been accepted 400 s late, 3 starting then is wholly inherited.
    let mut team = stamped(vec![task(1, "dev", &[], Some(10.0)), task(3, "dev", &[], Some(10.0))]);
    accept(&mut team, 1, T0, T0 + 990.0, T0 + 1000.0);
    start(&mut team, 3, T0 + 1000.0);
    assert_eq!(numbers(&lateness(&team, team.task(3).unwrap(), T0 + 1100.0)), (Some(400), Some(400), Some(0), Some(0)));
}

#[test]
fn overrun_is_the_own_work_with_rework_minus_the_estimate() {
    let mut team = stamped(vec![task(1, "dev", &[], Some(10.0))]);
    // Never started: no overrun to speak of.
    assert_eq!(overrun(&team, team.task(1).unwrap(), 0, &mut None), json!({"not_started":true}));
    // Two attempts (the first rejected): 1500 s of work against 600 s.
    let attempt = |n: u32, state: AttemptState| Attempt { n, member: "dev".into(), state, sent_at: T0, delivered_at: Some(T0), ..Default::default() };
    let task = team.task_mut(1).unwrap();
    task.status = TaskStatus::Accepted;
    task.attempts = vec![attempt(1, AttemptState::Rejected), attempt(2, AttemptState::Reported)];
    let value = overrun(&team, team.task(1).unwrap(), 1500, &mut None);
    assert_eq!(
        (value["estimate_s"].clone(), value["work_s"].clone(), value["overrun_s"].clone(), value["ratio"].clone(), value["running"].clone()),
        (json!(600), json!(1500), json!(900), json!(2.5), json!(false)),
        "{value}"
    );
    // Under the estimate the overrun is negative, not clamped.
    assert_eq!(overrun(&team, team.task(1).unwrap(), 300, &mut None)["overrun_s"], -300);
}

#[test]
fn without_an_estimate_the_schedule_and_the_overrun_are_unknown_never_zero() {
    // 1 has no estimate (and the history is empty): its end, and so 2's start, are unknown.
    let mut team = stamped(vec![task(1, "dev", &[], None), task(2, "qa", &[1], Some(10.0))]);
    let one = team.task(1).unwrap().baseline.clone().unwrap();
    assert_eq!((one.start, one.end, one.estimate_s), (Some(T0), None, None));
    assert_eq!(team.task(2).unwrap().baseline.as_ref().unwrap().start, None);
    let late = lateness(&team, team.task(2).unwrap(), T0 + 5000.0);
    assert_eq!(numbers(&late), (None, None, None, None), "{late}");
    assert!(late["unknown"].is_string(), "{late}");
    start(&mut team, 1, T0);
    let value = overrun(&team, team.task(1).unwrap(), 900, &mut None);
    assert_eq!((value["estimate_s"].clone(), value["overrun_s"].clone(), value["ratio"].clone()), (Value::Null, Value::Null, Value::Null), "{value}");
    assert_eq!(value["work_s"], 900);
}

#[test]
fn a_baseline_is_fixed_once_kept_on_plan_edits_and_a_later_task_gets_its_own() {
    let temp = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(temp.path(), || {
        let mut team = team(vec![task(1, "dev", &[], Some(10.0))]);
        stamp(&mut team, T0);
        let first = team.task(1).unwrap().baseline.clone().unwrap();
        // The lead edits the estimate and adds a task later: 1 keeps its baseline.
        team.task_mut(1).unwrap().estimate_minutes = Some(60.0);
        team.tasks.push(task(2, "qa", &[1], Some(10.0)));
        stamp(&mut team, T0 + 5000.0);
        assert_eq!(team.task(1).unwrap().baseline, Some(first));
        // The new one starts after 1's planned end, on the old plan.
        let second = team.task(2).unwrap().baseline.clone().unwrap();
        assert_eq!((second.at, second.start, second.end), (T0 + 5000.0, Some(T0 + 5000.0), Some(T0 + 5600.0)));
        // A draft, a task of a draft phase and a team still planning get none.
        let mut planning = team.clone();
        planning.tasks = vec![task(1, "dev", &[], Some(10.0))];
        planning.status = TeamStatus::Planning;
        stamp(&mut planning, T0);
        assert!(planning.task(1).unwrap().baseline.is_none());
        let mut drafted = team.clone();
        drafted.tasks = vec![Task { status: TaskStatus::Draft, ..task(1, "dev", &[], Some(10.0)) }];
        stamp(&mut drafted, T0);
        assert!(drafted.task(1).unwrap().baseline.is_none());
        let mut started = team.clone();
        started.tasks = vec![task(1, "dev", &[], Some(10.0))];
        start(&mut started, 1, T0 - 50.0);
        stamp(&mut started, T0);
        assert!(started.task(1).unwrap().baseline.is_none(), "fresh stamping skips a task already running");
    });
}

#[test]
fn old_records_load_without_a_baseline_and_show_no_lateness() {
    let old = json!({"id":3,"title":"Old","phase":1,"status":"in_progress","assignee":"dev"});
    let task: Task = serde_json::from_value(old).unwrap();
    assert!(task.baseline.is_none());
    let team = team(vec![task.clone()]);
    assert_eq!(lateness(&team, &task, T0 + 100.0), Value::Null);
    // The stored form leaves the field out, and a baseline survives a round trip.
    assert!(serde_json::to_value(&task).unwrap().get("baseline").is_none());
    let stamped = stamped(vec![self::task(1, "dev", &[], Some(10.0))]);
    let round: Task = serde_json::from_value(serde_json::to_value(stamped.task(1).unwrap()).unwrap()).unwrap();
    assert_eq!(round.baseline, stamped.task(1).unwrap().baseline);
}

/// The store stamps in the change that approves or adds: no caller has to remember.
#[test]
fn the_store_fixes_a_baseline_in_the_change_that_approves_the_plan() {
    use crate::state::teams::store::Store;
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let mut planning = team(vec![task(1, "dev", &[], Some(10.0)), task(2, "qa", &[1], Some(10.0))]);
        planning.status = TeamStatus::Planning;
        planning.approved_at = None;
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), planning, vec![]).unwrap();
        // A change that does not approve leaves the plan without a baseline.
        store.mutate(None, "t", None, |team, _| { team.name = "Renamed".into(); Ok(json!({})) }).unwrap();
        assert!(store.load("t").unwrap().unwrap().tasks.iter().all(|task| task.baseline.is_none()));
        // The change that approves it fixes both, the second after the first.
        store.mutate(None, "t", None, |team, _| {
            team.status = TeamStatus::Running;
            team.approved_at = Some(T0);
            Ok(json!({}))
        }).unwrap();
        let team = store.load("t").unwrap().unwrap();
        let (one, two) = (team.task(1).unwrap().baseline.clone().unwrap(), team.task(2).unwrap().baseline.clone().unwrap());
        assert_eq!(two.start, one.end);
        assert_eq!(one.end.unwrap() - one.start.unwrap(), 600.0);
        // Later changes keep them.
        store.mutate(None, "t", None, |team, _| { team.name = "Again".into(); Ok(json!({})) }).unwrap();
        assert_eq!(store.load("t").unwrap().unwrap().task(1).unwrap().baseline, Some(one));
    });
}

#[test]
fn a_phase_running_at_upgrade_is_backfilled_from_its_approval() {
    let mut old = team(vec![task(1, "dev", &[], Some(10.0)), task(2, "qa", &[1], Some(20.0))]);
    start(&mut old, 1, T0 + 100.0);
    let temp = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(temp.path(), || backfill(&mut old, T0 + 300.0));
    let one = old.task(1).unwrap().baseline.as_ref().unwrap();
    assert!(one.backfilled);
    assert_eq!(one.start, Some(T0));
    assert_eq!(one.end, Some(T0 + 600.0));
    let late = lateness(&old, old.task(1).unwrap(), T0 + 300.0);
    assert_eq!(late["start_late_s"], 100);
    let mut fresh = team(vec![task(1, "dev", &[], Some(10.0)), task(2, "qa", &[1], Some(20.0))]);
    crate::state::with_test_state_root(temp.path(), || stamp(&mut fresh, T0));
    for id in [1, 2] {
        let a = old.task(id).unwrap().baseline.as_ref().unwrap();
        let b = fresh.task(id).unwrap().baseline.as_ref().unwrap();
        assert_eq!((a.start, a.end, a.after.clone()), (b.start, b.end, b.after.clone()));
    }
}



