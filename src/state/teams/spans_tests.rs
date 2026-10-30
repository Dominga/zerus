use super::*;
use crate::state::teams::model::{
    Attempt, Dependency, DependencyKind, Member, PipelineRun, PipelineStage, Question, Report, RunRecord, StageRun, StageState,
};

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

fn team(tasks: Vec<Task>) -> Team {
    Team {
        id: "t".into(),
        lead: "lead".into(),
        folder: "/f".into(),
        approved_by: "You".into(),
        approved_at: Some(100.0),
        members: vec![
            Member { id: "lead".into(), role: "Lead".into(), agent: "claude".into(), ..Default::default() },
            Member { id: "dev".into(), role: "Backend".into(), agent: "codex".into(), ..Default::default() },
        ],
        tasks,
        ..Default::default()
    }
}

fn entry(at: f64, kind: &str, detail: Value) -> Journal {
    Journal { at, kind: kind.into(), detail }
}

fn timeline(team: &Team, id: u32, journal: &[Journal], now: f64) -> Value {
    spans(team, team.task(id).unwrap(), journal, &holders(team, now), now)
}

/// The lane as (kind, seconds), in order.
fn lane(value: &Value) -> Vec<(String, i64)> {
    value["spans"].as_array().unwrap().iter().map(|span| (span["kind"].as_str().unwrap().to_owned(), span["s"].as_i64().unwrap_or(-1))).collect()
}

fn pairs(list: &[(&str, i64)]) -> Vec<(String, i64)> {
    list.iter().map(|(kind, s)| ((*kind).to_owned(), *s)).collect()
}

/// The lane never overlaps and has no hole: each span starts where the last ended.
fn contiguous(value: &Value) {
    let spans = value["spans"].as_array().unwrap();
    for pair in spans.windows(2) {
        if pair[0]["from"].is_null() {
            continue;
        }
        assert_eq!(pair[0]["to"], pair[1]["from"], "{value}");
    }
}

/// #1 rejected once and accepted on its second attempt (U37's hand count);
/// #2 waits for it, then runs now.
fn retried() -> Team {
    let mut first = attempt(1, AttemptState::Rejected, 110.0, 115.0);
    first.report = report(400.0, false);
    first.turn_ended_at = Some(410.0);
    first.decided_at = Some(460.0);
    let mut second = attempt(2, AttemptState::Reported, 500.0, 505.0);
    second.questions = vec![Question { id: "q1".into(), text: "Which field?".into(), at: 600.0, answer: "The new one".into(),
        answered_at: Some(660.0), by: "Lead".into(), ..Default::default() }];
    second.turn_ended_at = Some(860.0);
    second.report = report(900.0, true);
    second.decided_at = Some(1000.0);
    let running = attempt(1, AttemptState::Delivered, 1010.0, 1020.0);
    team(vec![task(1, &[], TaskStatus::Accepted, vec![first, second]), task(2, &[1], TaskStatus::InProgress, vec![running])])
}

#[test]
fn a_retried_task_splits_into_waits_delivery_work_rework_a_question_and_the_lead() {
    let team = retried();
    let value = timeline(&team, 1, &[], 5000.0);
    // Ready at the plan's 100; the first attempt 110-400 was rejected (rework),
    // decided at 460; the second works 505-860 less its question 600-660, the
    // captured report comes at 900 (no report from the turn's end), accepted at 1000.
    assert_eq!(
        lane(&value),
        pairs(&[("member", 10), ("delivery", 1), ("start_delay", 4), ("rework", 285), ("lead", 60), ("member", 40), ("delivery", 1),
            ("start_delay", 4), ("work", 95), ("question", 60), ("work", 200), ("no_report", 40), ("lead", 100)])
    );
    contiguous(&value);
    let spans = value["spans"].as_array().unwrap();
    assert_eq!((spans[9]["by"].clone(), spans[9]["question"].clone(), spans[9]["attempt"].clone()), (json!("Lead"), json!("q1"), json!(2)));
    assert_eq!((spans[0]["from"].clone(), spans.last().unwrap()["to"].clone()), (json!(100.0), json!(1000.0)));
    // The same seconds U37 counts: worked 580, member 60, lead 220.
    let totals = &value["totals"];
    let sum = |kinds: &[&str]| kinds.iter().map(|kind| totals[*kind].as_i64().unwrap_or(0)).sum::<i64>();
    let timing = crate::state::teams::timing::timing(&team, team.task(1).unwrap(), 5000.0);
    assert_eq!(sum(&["work", "rework"]), timing["worked_s"].as_i64().unwrap());
    assert_eq!(sum(&["member", "delivery", "start_delay"]), timing["waits"]["member_s"].as_i64().unwrap());
    assert_eq!(sum(&["lead", "question"]), timing["waits"]["lead_s"].as_i64().unwrap());
    assert!(value.get("unknown").is_none(), "{value}");
    // A finished task's timeline does not grow with the clock.
    assert_eq!(timeline(&team, 1, &[], 9000.0), value);
}

#[test]
fn a_dependent_task_waits_for_its_dependency_then_works_until_now() {
    let team = retried();
    let value = timeline(&team, 2, &[], 1620.0);
    assert_eq!(lane(&value), pairs(&[("deps", 900), ("member", 10), ("delivery", 1), ("start_delay", 9), ("work", 600)]));
    let spans = value["spans"].as_array().unwrap();
    assert_eq!((spans[0]["from"].clone(), spans[0]["to"].clone()), (json!(100.0), json!(1000.0)));
    // Still working: no end yet.
    assert_eq!((spans[4]["from"].clone(), spans[4]["to"].clone()), (json!(1020.0), Value::Null));
    // Still waiting for #1: the deps span runs to now.
    let mut waiting = retried();
    waiting.task_mut(1).unwrap().status = TaskStatus::InProgress;
    waiting.task_mut(2).unwrap().attempts.clear();
    waiting.task_mut(2).unwrap().status = TaskStatus::Pending;
    let value = timeline(&waiting, 2, &[], 1620.0);
    assert_eq!(lane(&value), pairs(&[("deps", 1520)]));
    assert_eq!(value["spans"][0]["to"], Value::Null);
}

/// #1 runs the Code pipeline: round 1's review returned it, round 2's review
/// and test passed (the test held the folder and ran one build), the lead
/// accepted. #2, the same member's next task, waited for that folder.
fn returned() -> Team {
    let mut first = attempt(1, AttemptState::Rejected, 100.0, 105.0);
    first.pipeline_return = true;
    first.report = report(200.0, false);
    let mut second = attempt(2, AttemptState::Reported, 320.0, 325.0);
    second.report = report(500.0, false);
    second.turn_ended_at = Some(510.0);
    second.decided_at = Some(760.0);
    let run = |stage: usize, kind: &str, round: u32, attempt: u32, state: StageState, times: [f64; 3]| StageRun {
        id: format!("s{stage}-{round}"),
        stage,
        kind: kind.into(),
        round,
        attempt,
        member: if kind == "review" { "rev".into() } else { "qa".into() },
        state,
        at: times[0],
        delivered_at: Some(times[1]),
        ended_at: Some(times[2]),
        ..Default::default()
    };
    let mut test = run(1, "test", 2, 2, StageState::Passed, [560.0, 600.0, 700.0]);
    test.holds = true;
    test.folder = "/f".into();
    test.runs = vec![RunRecord { template: "cargo-test".into(), exit: Some(0), at: 690.0, ms: 60_000, ..Default::default() }];
    let mut one = task(1, &[], TaskStatus::Accepted, vec![first, second]);
    one.pipeline_run = Some(PipelineRun {
        id: "code".into(),
        name: "Code".into(),
        stages: vec![
            PipelineStage { kind: "review".into(), role: "Reviewer".into(), ..Default::default() },
            PipelineStage { kind: "test".into(), role: "Tester".into(), ..Default::default() },
        ],
        max_rounds: 3,
        folder: "hold".into(),
        accept: "lead".into(),
        round: 2,
        runs: vec![
            run(0, "review", 1, 1, StageState::Returned, [200.0, 210.0, 300.0]),
            run(0, "review", 2, 2, StageState::Passed, [500.0, 505.0, 560.0]),
            test,
        ],
        ..Default::default()
    });
    let mut team = team(vec![one, task(2, &[], TaskStatus::Pending, vec![])]);
    team.approved_at = Some(90.0);
    team
}

#[test]
fn a_returned_task_shows_each_stage_and_round_its_rework_and_its_build() {
    let team = returned();
    let value = timeline(&team, 1, &[], 800.0);
    assert_eq!(
        lane(&value),
        pairs(&[("member", 10), ("delivery", 1), ("start_delay", 4), ("rework", 95), ("stage_wait", 10), ("stage", 90), ("member", 20),
            ("delivery", 1), ("start_delay", 4), ("work", 175), ("stage_wait", 5), ("stage", 55), ("stage_wait", 40), ("stage", 100), ("lead", 60)])
    );
    contiguous(&value);
    let spans = value["spans"].as_array().unwrap();
    // Round 1's review returned the first attempt; round 2's test passed.
    assert_eq!((spans[5]["stage_kind"].clone(), spans[5]["round"].clone(), spans[5]["state"].clone(), spans[5]["member"].clone()),
        (json!("review"), json!(1), json!("returned"), json!("rev")));
    assert_eq!((spans[13]["stage_kind"].clone(), spans[13]["round"].clone(), spans[13]["state"].clone(), spans[13]["attempt"].clone()),
        (json!("test"), json!(2), json!("passed"), json!(2)));
    // The build lies on the test's span, not in the lane.
    assert_eq!(value["overlay"], json!([{"kind":"build","from":630.0,"to":690.0,"s":60,"template":"cargo-test","exit":0,"attempt":2,
        "stage":1,"round":2,"member":"qa"}]));
    assert_eq!(value["totals"]["stage"], 245);
    assert_eq!(value["totals"]["rework"], 95);
}

#[test]
fn a_next_task_waits_while_another_tasks_test_holds_its_folder() {
    let team = returned();
    let value = timeline(&team, 2, &[], 800.0);
    assert_eq!(lane(&value), pairs(&[("member", 510), ("folder_held", 100), ("member", 100)]));
    let spans = value["spans"].as_array().unwrap();
    assert_eq!((spans[1]["from"].clone(), spans[1]["to"].clone(), spans[1]["held_by"].clone()), (json!(600.0), json!(700.0), json!(1)));
    assert_eq!(spans[2]["to"], Value::Null);
    // A member in another folder never waited for it.
    let mut elsewhere = returned();
    elsewhere.members[1].directory = "/other".into();
    assert_eq!(lane(&timeline(&elsewhere, 2, &[], 800.0)), pairs(&[("member", 710)]));
}

/// #3: a permission and a native question inside the rejected work, the turn
/// held by Background work until the capture, a hold after the rejection, a
/// stop of the second attempt, and the third attempt working now.
fn blocked() -> (Team, Vec<Journal>) {
    let mut first = attempt(1, AttemptState::Rejected, 100.0, 102.0);
    first.turn_ended_at = Some(300.0);
    first.blocked_since = Some(300.0);
    first.report_ready_at = Some(350.0);
    first.report = report(350.0, true);
    first.decided_at = Some(400.0);
    let second = attempt(2, AttemptState::Revoked, 470.0, 480.0);
    let third = attempt(3, AttemptState::Delivered, 600.0, 605.0);
    let mut team = team(vec![task(3, &[], TaskStatus::InProgress, vec![first, second, third])]);
    team.approved_at = Some(90.0);
    let journal = vec![
        entry(150.0, "question_forwarded", json!({"member":"Backend","approval":true})),
        entry(170.0, "permission_answered", json!({"member":"Backend","by":"You","decision":"allow","question_id":"p1"})),
        entry(200.0, "question_forwarded", json!({"member":"Backend","approval":false})),
        entry(210.0, "permission_answered", json!({"member":"Backend","by":"Lead","decision":"answer","question_id":"p2"})),
        entry(400.0, "held", json!({"task":3,"by":"Lead","reason":"Wait for #4"})),
        entry(450.0, "released", json!({"task":3,"by":"Lead"})),
        entry(500.0, "stop_told", json!({"task":3,"member":"Backend","attempt":2})),
    ];
    (team, journal)
}

#[test]
fn blocks_background_a_hold_and_a_stop_each_take_their_own_span() {
    let (team, journal) = blocked();
    let value = timeline(&team, 3, &journal, 700.0);
    assert_eq!(
        lane(&value),
        pairs(&[("member", 10), ("delivery", 1), ("start_delay", 1), ("rework", 48), ("permission", 20), ("rework", 30), ("question", 10),
            ("rework", 90), ("background", 50), ("lead", 50), ("hold", 50), ("member", 20), ("delivery", 1), ("start_delay", 9), ("work", 20),
            ("stop", 100), ("delivery", 1), ("start_delay", 4), ("work", 95)])
    );
    contiguous(&value);
    let spans = value["spans"].as_array().unwrap();
    // Who answered and how.
    assert_eq!((spans[4]["by"].clone(), spans[4]["decision"].clone()), (json!("You"), json!("allow")));
    assert_eq!(spans[6]["by"], "Lead");
    assert_eq!(spans[10]["by"], "Lead");
    assert_eq!((spans[15]["from"].clone(), spans[15]["to"].clone(), spans[15]["attempt"].clone()), (json!(500.0), json!(600.0), json!(2)));
    assert_eq!(spans.last().unwrap()["to"], Value::Null);
    assert_eq!(value["totals"]["rework"], 168);
    // The stop ended the second attempt's work at once.
    assert_eq!(spans[14]["to"], json!(500.0));
}

#[test]
fn a_shared_role_leaves_its_journal_blocks_unattributed() {
    let (mut team, journal) = blocked();
    team.members.push(Member { id: "dev2".into(), role: "Backend".into(), agent: "kimi".into(), ..Default::default() });
    let value = timeline(&team, 3, &journal, 700.0);
    // Two Backends: the journal cannot say whose permission it was.
    assert_eq!(value["unknown"], json!(["permission"]));
    assert_eq!(lane(&value)[3], ("rework".to_owned(), 198));
    assert!(!lane(&value).iter().any(|(kind, _)| kind == "permission"));
}

#[test]
fn a_stop_without_a_next_attempt_runs_until_the_task_is_handed_out() {
    let mut stopped = attempt(1, AttemptState::Revoked, 100.0, 110.0);
    stopped.started_at = Some(110.0);
    let team = team(vec![task(5, &[], TaskStatus::Pending, vec![stopped])]);
    let journal = [entry(200.0, "stop_told", json!({"task":5,"member":"Backend","attempt":1}))];
    let value = timeline(&team, 5, &journal, 260.0);
    assert_eq!(lane(&value), pairs(&[("delivery", 1), ("start_delay", 9), ("work", 90), ("stop", 60)]));
    assert_eq!(value["spans"][3]["to"], Value::Null);
}

#[test]
fn an_old_team_file_keeps_what_it_never_recorded_unknown() {
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
    let value = timeline(&team, 1, &[], 100.0);
    // The plan's approval, the start, the report's ten seconds after the turn
    // and the decision were never recorded: unknown, never a guess.
    assert_eq!(value["spans"][0], json!({"kind":"unknown","from":null,"to":10.0,"s":null}));
    assert_eq!(lane(&value), pairs(&[("unknown", -1), ("delivery", 1), ("work", 9), ("unknown", 10)]));
    assert_eq!(value["totals"], json!({"delivery":1,"unknown":10,"work":9}));
    // A pending task of that file: nothing recorded since an unknown opening.
    assert_eq!(timeline(&team, 2, &[], 100.0)["spans"], json!([{"kind":"unknown","from":null,"to":null,"s":null}]));
    // An attempt revoked without a report or a next hand-out: its work's end is unknown.
    let mut lost = team.clone();
    lost.tasks[0].attempts[0].state = AttemptState::Revoked;
    lost.tasks[0].attempts[0].turn_ended_at = None;
    lost.tasks[0].attempts[0].report = None;
    lost.tasks[0].status = TaskStatus::Cancelled;
    let value = timeline(&lost, 1, &[], 100.0);
    assert_eq!(value["unknown"], json!(["work"]));
    assert!(!lane(&value).iter().any(|(kind, _)| kind == "work"), "{value}");
}

#[test]
fn thousands_of_tasks_compute_in_bounded_pages_and_time() {
    let mut tasks = Vec::new();
    for id in 1..=3000u32 {
        let mut first = attempt(1, AttemptState::Rejected, f64::from(id) * 10.0, f64::from(id) * 10.0 + 2.0);
        first.report = report(f64::from(id) * 10.0 + 5.0, false);
        first.decided_at = Some(f64::from(id) * 10.0 + 6.0);
        let mut second = attempt(2, AttemptState::Reported, f64::from(id) * 10.0 + 7.0, f64::from(id) * 10.0 + 8.0);
        second.report = report(f64::from(id) * 10.0 + 9.0, false);
        second.decided_at = Some(f64::from(id) * 10.0 + 9.5);
        let deps: Vec<u32> = if id > 1 { vec![id - 1] } else { vec![] };
        tasks.push(task(id, &deps, TaskStatus::Accepted, vec![first, second]));
    }
    let team = team(tasks);
    let journal: Vec<Journal> = (0..EVENT_LIMIT)
        .map(|n| entry(n as f64 * 6.0, if n % 2 == 0 { "question_forwarded" } else { "permission_answered" }, json!({"member":"Other"})))
        .collect();
    let started = std::time::Instant::now();
    let mut seen = 0;
    for page in team.tasks.chunks(TASK_PAGE) {
        let chosen: Vec<&Task> = page.iter().collect();
        assert!(window(&team, &chosen, 40_000.0).is_some());
        let holders = holders(&team, 40_000.0);
        for task in chosen {
            let value = spans(&team, task, &journal, &holders, 40_000.0);
            assert!(value["spans"].as_array().unwrap().len() <= 16, "{value}");
            seen += 1;
        }
    }
    assert_eq!(seen, 3000);
    assert!(started.elapsed() < std::time::Duration::from_secs(20), "{:?}", started.elapsed());
}

#[test]
fn the_journal_is_read_through_its_index_in_one_bounded_page() {
    use crate::state::teams::store::{Event, Store};
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), team(vec![]), vec![]).unwrap();
    store
        .mutate(None, "t", None, |_, events| {
            for n in 0..(EVENT_LIMIT + 1000) {
                events.push(Event::new("question_forwarded", json!({"member":"Backend","n":n})));
                events.push(Event::new("tick", json!({"n":n})));
            }
            Ok(json!({}))
        })
        .unwrap();
    let started = std::time::Instant::now();
    let (items, cut) = store.span_events("t", 0.0, f64::MAX, EVENT_LIMIT).unwrap();
    assert!(started.elapsed() < std::time::Duration::from_secs(5), "{:?}", started.elapsed());
    assert_eq!(items.len() as i64, EVENT_LIMIT);
    assert!(cut);
    assert!(items.iter().all(|item| item.kind == "question_forwarded"), "only the kinds a timeline reads");
    assert_eq!(items[0].detail["n"], 0);
    // Another team's window is empty.
    assert_eq!(store.span_events("other", 0.0, f64::MAX, EVENT_LIMIT).unwrap(), (Vec::new(), false));
}
