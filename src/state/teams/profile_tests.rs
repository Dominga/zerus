use super::*;
use crate::state::teams::model::{Attempt, Dependency, DependencyKind, Member, Question, Report, RunRecord, SessionRef};
use crate::state::teams::spans;

fn attempt(n: u32, member: &str, state: AttemptState, sent: f64, started: f64, reported: f64, decided: f64) -> Attempt {
    Attempt {
        n,
        id: format!("{member}-a{n}"),
        member: member.into(),
        state,
        sent_at: sent,
        delivered_at: Some(sent + 1.0),
        started_at: Some(started),
        turn_ended_at: Some(reported),
        report: Some(Report { text: "Done".into(), at: reported, ..Default::default() }),
        decided_at: Some(decided),
        ..Default::default()
    }
}

/// Two tasks whose losses are known by hand (phase 1 opens at 100):
/// #1 by dev is rejected once (rework 115-400) and accepted at 1000, with the
/// lead deciding 400-460 and 900-1000 and dev waited for 100-110 and 460-500.
/// #2 by qa waits for #1 (100-1000), for qa (1000-1100), asks the lead
/// (1200-1500), runs a 400 s test build and a merge that conflicts, and is
/// accepted at 2100. The scope is open 100-2100: dev works 680 s of it and qa
/// 895 s, the rest each is idle.
fn known() -> Team {
    let first = attempt(1, "dev", AttemptState::Rejected, 110.0, 115.0, 400.0, 460.0);
    let second = attempt(2, "dev", AttemptState::Reported, 500.0, 505.0, 900.0, 1000.0);
    let mut tested = attempt(1, "qa", AttemptState::Reported, 1100.0, 1105.0, 2000.0, 2100.0);
    tested.questions = vec![Question { id: "q1".into(), text: "Which port?".into(), at: 1200.0, answer: "8080".into(),
        answered_at: Some(1500.0), by: "Lead".into(), ..Default::default() }];
    tested.runs = vec![
        RunRecord { template: "cargo-test".into(), exit: Some(0), at: 1900.0, ms: 400_000, ..Default::default() },
        RunRecord { template: "git-merge-lead".into(), exit: Some(1), at: 1950.0, ms: 5_000, ..Default::default() },
    ];
    let task = |id: u32, assignee: &str, deps: &[u32], attempts: Vec<Attempt>| Task {
        id,
        phase: 1,
        title: format!("Task {id}"),
        assignee: assignee.into(),
        deps: deps.iter().map(|task| Dependency { task: *task, kind: DependencyKind::Hard }).collect(),
        status: TaskStatus::Accepted,
        attempts,
        ..Default::default()
    };
    Team {
        id: "t".into(),
        lead: "lead".into(),
        folder: "/f".into(),
        approved_by: "You".into(),
        approved_at: Some(100.0),
        members: vec![
            Member { id: "lead".into(), role: "Lead".into(), agent: "claude".into(), ..Default::default() },
            Member { id: "dev".into(), role: "Backend".into(), agent: "codex".into(), model: "gpt-5".into(), ..Default::default() },
            Member { id: "qa".into(), role: "Tester".into(), agent: "kimi".into(), ..Default::default() },
        ],
        tasks: vec![task(1, "dev", &[], vec![first, second]), task(2, "qa", &[1], vec![tested])],
        ..Default::default()
    }
}

fn timelines(team: &Team, now: f64) -> Vec<Value> {
    let holders = spans::holders(team, now);
    team.tasks.iter().map(|task| spans::spans(team, task, &[], &holders, now)).collect()
}

fn tokens(qa: Option<Tokens>) -> TokenMap {
    let mut map = TokenMap::new();
    map.insert((1, "a1".into()), Some(Tokens { total: 3000, uncached_input: 3000, cost_usd: 0.3, ..Default::default() }));
    map.insert((1, "a2".into()), Some(Tokens { total: 6000, uncached_input: 6000, cost_usd: 0.6, ..Default::default() }));
    map.insert((2, "a1".into()), qa);
    map
}

fn run(team: &Team, map: &TokenMap, page: &Page) -> Value {
    let tasks: Vec<&Task> = team.tasks.iter().collect();
    let times = timelines(team, 5000.0);
    profile(team, &tasks, &times, &times, map, page, 5000.0)
}

fn ids(list: &Value) -> Vec<(String, i64)> {
    list["items"].as_array().unwrap().iter().map(|item| (item["id"].as_str().unwrap().to_owned(), item["s"].as_i64().unwrap())).collect()
}

#[test]
fn the_losses_of_a_hand_built_team_rank_by_time_and_by_tokens() {
    let team = known();
    let value = run(&team, &tokens(Some(Tokens { total: 4000, cost_usd: 0.4, ..Default::default() })), &Page { limit: 100, ..Default::default() });
    let expected: Vec<(String, i64)> = [
        ("idle_member:dev", 1320),
        ("idle_member:qa", 1105),
        ("blocked_chain:2", 900),
        // A tie on seconds goes by id.
        ("lead_wait:2", 400),
        ("long_test:2:b0:1900", 400),
        ("rework:1:a1", 285),
        ("lead_wait:1", 160),
        ("member_wait:2", 100),
        ("member_wait:1", 50),
        ("conflict:2:b1:1950", 5),
    ]
    .iter()
    .map(|(id, s)| ((*id).to_owned(), *s))
    .collect();
    assert_eq!(ids(&value["losses"]["by_time"]), expected, "{}", value["losses"]);
    let items = value["losses"]["by_time"]["items"].as_array().unwrap();
    let find = |id: &str| items.iter().find(|item| item["id"] == id).unwrap();
    assert_eq!((find("lead_wait:2")["lead_s"].clone(), find("lead_wait:2")["question_s"].clone()), (json!(100), json!(300)));
    assert_eq!(find("blocked_chain:2")["on"], json!([1]));
    assert_eq!((find("rework:1:a1")["cause"].clone(), find("rework:1:a1")["share"].clone()), (json!("rejected"), json!(0.333)));
    assert_eq!(find("conflict:2:b1:1950")["template"], "git-merge-lead");
    // Only the rework spent tokens a loss can name.
    let by_tokens = &value["losses"]["by_tokens"]["items"];
    assert_eq!((by_tokens.as_array().unwrap().len(), by_tokens[0]["id"].clone(), by_tokens[0]["tokens"].clone()), (1, json!("rework:1:a1"), json!(3000)));
    // The plan said #1 then #2, and so it went: 100 to 2100.
    assert_eq!(value["critical_path"], json!({"planned":{"tasks":[1,2],"length":2},"real":{"tasks":[1,2],"s":2000},"off_plan":[],"matches":true}));
    let summary = &value["summary"];
    assert_eq!((summary["tasks"].clone(), summary["accepted"].clone(), summary["elapsed_s"].clone(), summary["worked_s"].clone()),
        (json!(2), json!(2), json!(2000), json!(1275)));
    assert_eq!((summary["tokens"]["total"].clone(), summary["tokens"]["partial"].clone()), (json!(13000), json!(false)));
    // Per member: dev worked 680 of 2000 s and spent 9000 tokens on its one accepted task.
    let dev = value["members"].as_array().unwrap().iter().find(|member| member["member"] == "dev").unwrap();
    assert_eq!((dev["busy_s"].clone(), dev["idle_s"].clone(), dev["utilization"].clone(), dev["accepted"].clone(), dev["tokens_per_accepted"].clone()),
        (json!(680), json!(1320), json!(0.34), json!(1), json!(9000)));
    assert_eq!(dev["model"], "gpt-5");
    let qa = value["members"].as_array().unwrap().iter().find(|member| member["member"] == "qa").unwrap();
    assert_eq!((qa["model"].clone(), qa["tokens_per_accepted"].clone()), (json!("kimi default"), json!(4000)));
    assert!(value["members"].as_array().unwrap().iter().all(|member| member["member"] != "lead"));
    let roles: Vec<Value> = value["roles"].as_array().unwrap().iter().map(|role| role["role"].clone()).collect();
    assert_eq!(roles, [json!("Backend"), json!("Tester")]);
    // A task row carries its rework tokens and their share.
    let row = &value["tasks"]["items"][0];
    assert_eq!((row["task"].clone(), row["rework_s"].clone(), row["rework_tokens"].clone(), row["rework_share"].clone()),
        (json!(1), json!(285), json!(3000), json!(0.333)));
}

#[test]
fn an_unknown_token_count_stays_unknown_never_zero() {
    let team = known();
    let value = run(&team, &tokens(None), &Page { limit: 100, ..Default::default() });
    assert_eq!((value["summary"]["tokens"]["total"].clone(), value["summary"]["tokens"]["partial"].clone()), (json!(9000), json!(true)));
    let qa = value["members"].as_array().unwrap().iter().find(|member| member["member"] == "qa").unwrap();
    assert_eq!(qa["tokens"], json!({"unknown":true}));
    assert!(qa.get("tokens_per_accepted").is_none(), "{qa}");
    assert_eq!(value["tasks"]["items"][1]["tokens"], json!({"unknown":true}));
    // A member on another computer or with no record reads as unknown.
    let mut remote = known();
    remote.members[1].session = Some(SessionRef { host: "box".into(), name: "codex/x/dev".into(), ..Default::default() });
    let asks = asks(&timelines(&remote, 5000.0), 5000.0);
    assert_eq!(
        asks.iter().map(|ask| (ask.task, ask.part.as_str(), ask.member.as_str(), ask.windows.clone())).collect::<Vec<_>>(),
        [(1, "a1", "dev", vec![(115.0, 400.0)]), (1, "a2", "dev", vec![(505.0, 900.0)]), (2, "a1", "qa", vec![(1105.0, 2000.0)])]
    );
    let (read, capped) = read_tokens(&remote, &asks);
    assert!(!capped && read.len() == 3 && read.values().all(Option::is_none), "{read:?}");
}

#[test]
fn loss_pages_are_bounded_and_stable() {
    let team = known();
    let map = tokens(Some(Tokens { total: 4000, cost_usd: 0.4, ..Default::default() }));
    let whole = ids(&run(&team, &map, &Page { limit: 100, ..Default::default() })["losses"]["by_time"]);
    // Pages of three walk the same order with no repeat and no gap.
    let mut walked = Vec::new();
    let mut cursor: Option<Value> = None;
    loop {
        let value = run(&team, &map, &Page { limit: 3, after_time: cursor.clone(), ..Default::default() });
        let page = &value["losses"]["by_time"];
        assert!(page["items"].as_array().unwrap().len() <= 3);
        assert_eq!(page["total"], whole.len());
        walked.extend(ids(page));
        cursor = Some(page["next_cursor"].clone()).filter(|next| !next.is_null());
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(walked, whole);
    // A loss that arrives above the cursor never shifts the next page.
    let made = |id: &str, s: i64| loss("idle_member", id, s, None, json!({}));
    let list = [made("a", 50), made("b", 40), made("c", 30), made("d", 20)];
    let order: Vec<&Loss> = list.iter().collect();
    let first = page_of(&order, |loss| loss.s, "s", None, 2);
    let cursor = first["next_cursor"].clone();
    assert_eq!(cursor, json!({"s":40,"id":"idle_member:b"}));
    let grown = [made("z", 90), made("a", 50), made("b", 40), made("c", 30), made("d", 20)];
    let order: Vec<&Loss> = grown.iter().collect();
    let next = page_of(&order, |loss| loss.s, "s", Some(&cursor), 2);
    let named: Vec<&str> = next["items"].as_array().unwrap().iter().map(|item| item["id"].as_str().unwrap()).collect();
    assert_eq!((named, next["next_cursor"].clone()), (vec!["idle_member:c", "idle_member:d"], Value::Null));
    // The limit is bounded and task rows page by id.
    let value = run(&team, &map, &Page { limit: 10_000, ..Default::default() });
    assert!(value["losses"]["by_time"]["items"].as_array().unwrap().len() <= LOSS_PAGE_MAX);
    let value = run(&team, &map, &Page { limit: 1, ..Default::default() });
    assert_eq!((value["tasks"]["items"][0]["task"].clone(), value["tasks"]["next_cursor"].clone()), (json!(1), json!(1)));
    let value = run(&team, &map, &Page { limit: 1, after_task: 1, ..Default::default() });
    assert_eq!((value["tasks"]["items"][0]["task"].clone(), value["tasks"]["next_cursor"].clone()), (json!(2), Value::Null));
}

#[test]
fn the_real_critical_path_follows_the_dependency_that_ended_last() {
    // #3 waits for #1 and #2; #2 ended later, so the real path runs through it,
    // while the plan, by count, names the lower id.
    let mut team = known();
    let mut late = team.tasks[0].clone();
    late.id = 3;
    late.deps = vec![Dependency { task: 1, kind: DependencyKind::Hard }, Dependency { task: 2, kind: DependencyKind::Hard }];
    for (index, attempt) in late.attempts.iter_mut().enumerate() {
        let shift = 2200.0;
        attempt.n = index as u32 + 1;
        attempt.sent_at += shift;
        attempt.delivered_at = attempt.delivered_at.map(|at| at + shift);
        attempt.started_at = attempt.started_at.map(|at| at + shift);
        attempt.turn_ended_at = attempt.turn_ended_at.map(|at| at + shift);
        attempt.decided_at = attempt.decided_at.map(|at| at + shift);
        if let Some(report) = attempt.report.as_mut() {
            report.at += shift;
        }
    }
    team.tasks.push(late);
    let value = run(&team, &tokens(None), &Page { limit: 5, ..Default::default() });
    assert_eq!(value["critical_path"]["planned"]["tasks"], json!([1, 2, 3]));
    assert_eq!(value["critical_path"]["real"]["tasks"], json!([1, 2, 3]));
    team.tasks[1].deps.clear();
    let value = run(&team, &tokens(None), &Page { limit: 5, ..Default::default() });
    assert_eq!(value["critical_path"]["planned"]["tasks"], json!([1, 3]));
    assert_eq!((value["critical_path"]["real"]["tasks"].clone(), value["critical_path"]["off_plan"].clone()), (json!([2, 3]), json!([2])));
    assert_eq!(value["critical_path"]["matches"], false);
}

#[test]
fn two_phases_compare_side_by_side() {
    let team = known();
    let map = tokens(Some(Tokens { total: 4000, cost_usd: 0.4, ..Default::default() }));
    let first = run(&team, &map, &Page { limit: 3, ..Default::default() });
    let one: Vec<&Task> = team.tasks.iter().take(1).collect();
    let times = timelines(&team, 5000.0);
    let second = profile(&team, &one, &times[..1], &times, &map, &Page { limit: 3, ..Default::default() }, 5000.0);
    let compared = compare((1, &first), (2, &second));
    assert_eq!(compared["a"]["top_losses"].as_array().unwrap().len(), 3);
    assert_eq!(compared["delta"]["tasks"], -1);
    assert_eq!(compared["delta"]["elapsed_s"], 900 - 2000);
    assert_eq!(compared["delta"]["tokens"], 9000 - 13000);
    assert_eq!(compared["delta"]["totals"]["deps"], -900);
}

/// A large team stays within the time bound: 500 tasks of 20 members, two
/// attempts each, half of them in one chain.
#[test]
fn a_large_team_profiles_within_its_time_bound() {
    let mut team = known();
    team.members = (0..20).map(|n| Member { id: format!("m{n}"), role: format!("R{}", n % 4), agent: "codex".into(), ..Default::default() }).collect();
    team.members.push(Member { id: "lead".into(), role: "Lead".into(), ..Default::default() });
    team.tasks = (1..=500u32)
        .map(|id| {
            let member = format!("m{}", id % 20);
            let at = 100.0 + f64::from(id) * 50.0;
            let mut task = Task {
                id,
                phase: 1,
                title: format!("T{id}"),
                assignee: member.clone(),
                status: TaskStatus::Accepted,
                attempts: vec![
                    attempt(1, &member, AttemptState::Rejected, at, at + 2.0, at + 10.0, at + 12.0),
                    attempt(2, &member, AttemptState::Reported, at + 15.0, at + 16.0, at + 40.0, at + 45.0),
                ],
                ..Default::default()
            };
            if id % 2 == 0 && id > 2 {
                task.deps = vec![Dependency { task: id - 2, kind: DependencyKind::Hard }];
            }
            task
        })
        .collect();
    let started = std::time::Instant::now();
    let lines = timelines(&team, 100_000.0);
    let asks = asks(&lines, 100_000.0);
    let map: TokenMap = asks.iter().map(|ask| ((ask.task, ask.part.clone()), Some(Tokens { total: 100, cost_usd: 0.01, ..Default::default() }))).collect();
    let tasks: Vec<&Task> = team.tasks.iter().collect();
    let value = profile(&team, &tasks, &lines, &lines, &map, &Page { limit: 20, ..Default::default() }, 100_000.0);
    let took = started.elapsed();
    assert!(took < std::time::Duration::from_secs(10), "{took:?}");
    assert_eq!(value["losses"]["by_time"]["items"].as_array().unwrap().len(), 20);
    assert_eq!(value["summary"]["tasks"], 500);
    assert_eq!(value["critical_path"]["planned"]["length"], 250);
    assert_eq!(value["tasks"]["items"].as_array().unwrap().len(), 20);
    assert_eq!(value["members"].as_array().unwrap().len(), 20);
}

fn bare_team(tasks: Vec<Task>, members: Vec<Member>) -> Team {
    let mut members = members;
    members.insert(0, Member { id: "lead".into(), role: "Lead".into(), agent: "claude".into(), ..Default::default() });
    Team { id: "t".into(), lead: "lead".into(), folder: "/f".into(), approved_at: Some(100.0), members, tasks, ..Default::default() }
}

/// #736: never started is its own state, not unknown; a started task whose
/// provider has no timed counters stays unknown.
#[test]
fn a_task_that_never_started_is_not_unknown_and_a_provider_without_counters_stays_unknown() {
    let team = bare_team(
        vec![
            Task { id: 1, phase: 1, title: "Waiting".into(), assignee: "dev".into(), status: TaskStatus::Pending, ..Default::default() },
            Task {
                id: 2,
                phase: 1,
                title: "DeepSeek".into(),
                assignee: "ds".into(),
                status: TaskStatus::InProgress,
                attempts: vec![attempt(1, "ds", AttemptState::Reported, 100.0, 105.0, 200.0, 300.0)],
                ..Default::default()
            },
        ],
        vec![
            Member { id: "dev".into(), role: "Backend".into(), agent: "codex".into(), ..Default::default() },
            Member { id: "ds".into(), role: "Backend".into(), agent: "dsh".into(), ..Default::default() },
        ],
    );
    let lines = vec![
        json!({"task":1,"spans":[{"kind":"member","member":"dev","from":100.0,"to":200.0,"s":100}],"totals":{"member":100},"overlay":[]}),
        json!({"task":2,"spans":[{"kind":"work","member":"ds","from":100.0,"to":200.0,"s":100,"attempt":1}],"totals":{"work":100},"overlay":[]}),
    ];
    let mut map = TokenMap::new();
    map.insert((2, "a1".into()), None);
    let tasks: Vec<&Task> = team.tasks.iter().collect();
    let value = profile(&team, &tasks, &lines, &lines, &map, &Page { limit: 100, ..Default::default() }, 5000.0);
    let rows = value["tasks"]["items"].as_array().unwrap();
    let started = rows.iter().find(|row| row["task"] == 1).unwrap();
    assert_eq!((started["started"].clone(), started["tokens"].clone()), (json!(false), json!({"not_started":true})));
    let unknown = rows.iter().find(|row| row["task"] == 2).unwrap();
    assert_eq!((unknown["started"].clone(), unknown["tokens"].clone()), (json!(true), json!({"unknown":true})));
}

/// #736: the member's other task in the same hours is not this task's tokens,
/// and a part's gap between its own spending spans is never read.
#[test]
fn a_members_other_work_in_the_same_hours_is_not_the_tasks_tokens() {
    let team = bare_team(
        vec![
            Task { id: 1, phase: 1, title: "A".into(), assignee: "dev".into(), status: TaskStatus::InProgress,
                attempts: vec![attempt(1, "dev", AttemptState::Reported, 90.0, 95.0, 600.0, 700.0)], ..Default::default() },
            Task { id: 2, phase: 2, title: "B".into(), assignee: "dev".into(), status: TaskStatus::Pending,
                attempts: vec![Attempt { n: 1, id: "dev-b".into(), member: "dev".into(), state: AttemptState::Delivered, sent_at: 100.0, ..Default::default() }], ..Default::default() },
        ],
        vec![Member { id: "dev".into(), role: "Backend".into(), agent: "codex".into(), ..Default::default() }],
    );
    let lines = vec![
        json!({"task":1,"spans":[
            {"kind":"work","member":"dev","from":100.0,"to":200.0,"s":100,"attempt":1},
            {"kind":"work","member":"dev","from":400.0,"to":500.0,"s":100,"attempt":1}],
            "totals":{"work":200},"overlay":[]}),
        json!({"task":2,"spans":[{"kind":"delivery","member":"dev","from":100.0,"to":500.0,"s":400}],"totals":{"delivery":400},"overlay":[]}),
    ];
    let asks = asks(&lines, 5000.0);
    assert_eq!(asks.iter().map(|ask| (ask.task, ask.windows.clone())).collect::<Vec<_>>(), [(1, vec![(100.0, 200.0), (400.0, 500.0)])], "{asks:?}");
    let mut map = TokenMap::new();
    map.insert((1, "a1".into()), Some(Tokens { total: 5000, uncached_input: 5000, ..Default::default() }));
    let tasks: Vec<&Task> = team.tasks.iter().collect();
    let value = profile(&team, &tasks, &lines, &lines, &map, &Page { limit: 100, ..Default::default() }, 5000.0);
    let rows = value["tasks"]["items"].as_array().unwrap();
    assert_eq!(rows.iter().find(|row| row["task"] == 1).unwrap()["tokens"]["total"], 5000);
    assert_eq!(rows.iter().find(|row| row["task"] == 2).unwrap()["tokens"], json!({"not_started":true}));
}

/// #736: cache reads and writes are apart; the totals and the ranking count
/// uncached input plus output, never the cache reads.
#[test]
fn tokens_show_the_cache_buckets_apart_and_rank_the_fresh_input_and_output() {
    let team = known();
    let mut map = tokens(Some(Tokens { total: 4000, uncached_input: 4000, ..Default::default() }));
    map.insert((1, "a1".into()), Some(Tokens { total: 1000, uncached_input: 600, output: 400, cache_read: 9000, cache_write: 1000, cost_usd: 0.1 }));
    let value = run(&team, &map, &Page { limit: 100, ..Default::default() });
    let first = value["tasks"]["items"].as_array().unwrap().iter().find(|row| row["task"] == 1).unwrap();
    assert_eq!(first["tokens"], json!({"total":7000,"uncached_input":6600,"output":400,"cache_read":9000,"cache_write":1000,"cost_usd":0.7,"partial":false}));
    assert_eq!((value["summary"]["tokens"]["total"].clone(), value["summary"]["tokens"]["cache_read"].clone()), (json!(11000), json!(9000)));
    // The rework loss ranks by its fresh input and output, not its cache reads.
    assert_eq!(value["losses"]["by_tokens"]["items"][0]["tokens"], 1000);
}

/// #736: idle time and a member wait use the member's busy time in every
/// phase; another phase's work (or a task moved between phases) is not idle.
#[test]
fn idle_and_member_waits_use_the_members_busy_time_in_every_phase() {
    let team = bare_team(
        vec![
            Task { id: 1, phase: 1, title: "A".into(), assignee: "dev".into(), status: TaskStatus::InProgress,
                attempts: vec![attempt(1, "dev", AttemptState::Reported, 100.0, 105.0, 200.0, 300.0)], ..Default::default() },
            Task { id: 2, phase: 2, title: "B".into(), assignee: "dev".into(), status: TaskStatus::InProgress,
                attempts: vec![attempt(1, "dev", AttemptState::Reported, 300.0, 305.0, 800.0, 900.0)], ..Default::default() },
        ],
        vec![Member { id: "dev".into(), role: "Backend".into(), agent: "codex".into(), ..Default::default() }],
    );
    let scope = vec![json!({"task":1,"spans":[
            {"kind":"work","member":"dev","from":100.0,"to":200.0,"s":100,"attempt":1},
            {"kind":"member","member":"dev","from":300.0,"to":800.0,"s":500},
            {"kind":"lead","from":800.0,"to":1000.0,"s":200}],
            "totals":{"work":100,"member":500,"lead":200},"overlay":[]})];
    let all = vec![scope[0].clone(), json!({"task":2,"spans":[
            {"kind":"work","member":"dev","from":300.0,"to":800.0,"s":500,"attempt":1}],
            "totals":{"work":500},"overlay":[]})];
    let tasks: Vec<&Task> = team.tasks.iter().take(1).collect();
    let value = profile(&team, &tasks, &scope, &all, &TokenMap::new(), &Page { limit: 100, ..Default::default() }, 5000.0);
    let dev = value["members"].as_array().unwrap().iter().find(|member| member["member"] == "dev").unwrap();
    assert_eq!((dev["busy_s"].clone(), dev["idle_s"].clone()), (json!(600), json!(300)), "{dev}");
    // The member worked #2 through the whole wait for #1: no member_wait loss.
    assert!(!value["losses"]["by_time"]["items"].as_array().unwrap().iter().any(|loss| loss["kind"] == "member_wait"), "{value}");
    assert_eq!(value["summary"]["totals"]["member"], 0);
}
