//! Task #29 (team ede724c4, owner 2026-11-10): "а мы не хотим как то чистить
//! этот служебный шум?" - the lead's inbox held 44 items with nothing to
//! decide. Every item carries its key; every informational one (test results,
//! playbook items, lore_missing, waiting_to_land, refused queued calls...)
//! closes with `hgs team seen` and comes back only when its content changes,
//! and clears by itself once it no longer holds. A decision is answered, never
//! closed (task #289, the lead's answer q11).
use super::*;
use super::support::*;
use crate::state::teams::inbox::{closable, items, signature, Item};

const HAND_COMMIT: &str = "19a2020bff37449ae9baaf430f0d7ddb41d40261";

fn lead() -> Actor {
    Actor::Member("lead".into())
}

fn run(team: &mut Team, op: &str, input: Value, actor: &Actor) -> anyhow::Result<Value> {
    ops::apply(team, op, &input, actor, &BTreeSet::new(), &mut Vec::new())
}

fn of_kind(team: &Team, kind: &str) -> Vec<Item> {
    items(team).into_iter().filter(|item| item.kind == kind).collect()
}

fn notice(record: &str, task: Option<u32>, status: &str, at: f64) -> TestNotice {
    TestNotice { record: record.into(), suite: "rust".into(), status: status.into(), duration_ms: 4000, asked_by: "merge queue".into(), at, task, ..Default::default() }
}

/// The lead's desk of the owner's screenshot, one item of each informational
/// kind and one decision: #1 accepted by hand (the member committed it
/// itself) with its test results and waiting to land; a test result for no
/// task; a broken playbook rule; a fault lore cannot write yet; a refused
/// queued call; and #2's report.
fn desk() -> Team {
    let now = crate::state::now();
    let mut team = running_team();
    {
        let task = team.task_mut(1).unwrap();
        task.status = TaskStatus::Accepted;
        let attempt = &mut task.attempts[0];
        attempt.state = AttemptState::Reported;
        attempt.folder_head = HAND_COMMIT.into();
        attempt.report = Some(Report { text: "done: commit 19a2020bf".into(), at: now - 900.0, ..Default::default() });
        attempt.decided_at = Some(now - 600.0);
    }
    team.tasks.push(Task {
        id: 2,
        title: "UI".into(),
        assignee: "dev".into(),
        status: TaskStatus::Submitted,
        attempts: vec![Attempt {
            n: 1,
            id: "00000000-0000-4000-8000-000000000002".into(),
            member: "dev".into(),
            state: AttemptState::Reported,
            sent_at: now - 3000.0,
            delivered_at: Some(now - 3000.0),
            report: Some(Report { text: "UI done".into(), at: now - 300.0, ..Default::default() }),
            ..Default::default()
        }],
        ..Default::default()
    });
    team.test_notices = vec![
        notice("rec-a", Some(1), "failed", now - 1200.0),
        notice("rec-b", Some(1), "passed", now - 1100.0),
        notice("rec-free", None, "passed", now - 1000.0),
    ];
    team.working_mode = String::new();
    team.mode_proposal = Some(json!({"preset":"branch-pipeline","title":"the branch pipeline"}));
    team.playbook = Some(crate::state::teams::playbook::playbook(&team));
    team.settings.memory = Some(TeamMemory { mode: "team".into(), workspace: "D:/synthetic/lore".into(), ..Default::default() });
    team.lore.pending = vec![LorePending {
        id: "tt-fault-1".into(),
        kind: "fault".into(),
        missing: vec!["fix".into()],
        text: "write the fault for finding f1 (slow status): fix".into(),
    }];
    team.outbox_failures = vec![json!({"member":"dev","op":"ask","request_id":"11111111-1111-4111-8111-111111111111",
        "error":"attempt does not run for this member","at":now - 120.0})];
    team
}

const KINDS: [&str; 5] = ["test_result", "playbook", "lore_missing", "waiting_to_land", "refused_queued_call"];

#[test]
fn every_item_carries_its_key_and_each_informational_kind_closes_with_seen() {
    let team = desk();
    let all = items(&team);
    for item in &all {
        assert_eq!(item.value()["key"], json!(item.key), "every item carries its key: {item:?}");
    }
    for kind in KINDS {
        let found = of_kind(&team, kind);
        assert!(!found.is_empty(), "no {kind} item: {all:?}");
        for item in &found {
            let value = item.value();
            assert_eq!(value["notice"], json!(true), "{kind} is information: {value}");
            assert!(value["seen"].as_str().unwrap_or_default().contains(&format!("\"items\":[\"{}\"]}} | hgs team seen --json", item.key)), "{value}");
        }
        // The lead closes this kind alone: it leaves, the other kinds stay.
        let mut closed = team.clone();
        let keys: Vec<String> = found.iter().map(|item| item.key.clone()).collect();
        let answer = run(&mut closed, "seen", json!({"items":keys}), &lead()).unwrap();
        assert_eq!((answer["seen"].clone(), answer["unknown"].clone()), (json!(keys), json!([])), "{kind}: {answer}");
        assert!(of_kind(&closed, kind).is_empty(), "{kind} stays closed: {:?}", of_kind(&closed, kind));
        for other in KINDS.iter().filter(|other| **other != kind) {
            assert!(!of_kind(&closed, other).is_empty(), "closing {kind} closed {other} too");
        }
        // The owner closes it as well.
        let mut by_owner = team.clone();
        run(&mut by_owner, "seen", json!({"items":keys}), &Actor::Human).unwrap();
        assert!(of_kind(&by_owner, kind).is_empty(), "the owner closes {kind}");
    }
    // A decision carries its key but no close command, and seen refuses it
    // by name with the command that settles it.
    let report = of_kind(&team, "report").pop().expect("#2's report");
    let value = report.value();
    assert_eq!((value["key"].clone(), value["notice"].clone(), value.get("seen").cloned()), (json!(report.key), json!(false), None), "{value}");
    assert!(!closable(&report));
    let mut decided = team.clone();
    let answer = run(&mut decided, "seen", json!({"items":[report.key]}), &lead()).unwrap();
    assert_eq!(answer["seen"], json!([]), "{answer}");
    assert_eq!(answer["unknown"], json!([report.key]), "{answer}");
    assert_eq!(answer["decisions"][0]["kind"], "report", "{answer}");
    assert!(answer["decisions"][0]["why"].as_str().unwrap_or_default().contains("never closed"), "{answer}");
    assert!(answer["decisions"][0]["command"].as_str().unwrap_or_default().contains("hgs team"), "{answer}");
    assert_eq!(of_kind(&decided, "report").len(), 1, "the report still waits for its decision");
}

#[test]
fn informational_items_clear_by_themselves() {
    let now = crate::state::now();
    // Test results: one item per task; they clear once the task is accepted
    // and landed, and any result after 12 hours.
    let mut team = desk();
    assert!(of_kind(&team, "test_result").iter().any(|item| item.task == Some(1)), "{:?}", of_kind(&team, "test_result"));
    team.task_mut(1).unwrap().landed = Some(Landed { commit: HAND_COMMIT.into(), by: "Zerus".into(), at: now });
    let left = of_kind(&team, "test_result");
    assert!(left.iter().all(|item| item.task != Some(1)), "an accepted, landed task's results clear: {left:?}");
    assert_eq!(left.len(), 1, "the result for no task stays: {left:?}");
    let mut old = desk();
    let hours = crate::state::teams::inbox::TEST_RESULT_HOURS;
    for notice in &mut old.test_notices {
        notice.at = now - (hours + 1.0) * 3600.0;
    }
    old.test_notices.push(notice("rec-new", Some(2), "passed", now - 60.0));
    let left = of_kind(&old, "test_result");
    assert_eq!(left.iter().map(|item| item.task).collect::<Vec<_>>(), vec![Some(2)], "results older than {hours} h clear: {left:?}");

    // waiting_to_land clears once the work landed (the same landing above).
    assert!(!of_kind(&desk(), "waiting_to_land").is_empty());
    assert!(of_kind(&team, "waiting_to_land").is_empty(), "landed work no longer waits");

    // A playbook item clears as soon as its rule no longer breaks.
    let mut coached = desk();
    assert!(of_kind(&coached, "playbook").iter().any(|item| item.key == "playbook:a"), "{:?}", of_kind(&coached, "playbook"));
    crate::state::teams::working_mode::apply(&mut coached.settings, "branch-pipeline", "main").unwrap();
    coached.working_mode = "branch-pipeline".into();
    assert!(of_kind(&coached, "playbook").iter().all(|item| item.key != "playbook:a"), "{:?}", of_kind(&coached, "playbook"));

    // lore_missing for an accepted task clears once the task has its commit,
    // and the member's own commit counts (the reported folder head the
    // merge queue lands), even before the next lore pass rewrites the list.
    let mut lore = desk();
    let plan = crate::state::teams::lore_sync::plan_id(&lore, 1);
    lore.lore.pending.push(LorePending {
        id: format!("{plan}#1"),
        kind: "item".into(),
        missing: vec!["commits".into()],
        text: "#1 API is accepted with no commit Zerus made: name its commit".into(),
    });
    lore.task_mut(1).unwrap().attempts[0].folder_head.clear();
    assert!(of_kind(&lore, "lore_missing").iter().any(|item| item.summary.starts_with("#1 API")), "no commit yet: {:?}", of_kind(&lore, "lore_missing"));
    lore.task_mut(1).unwrap().attempts[0].folder_head = HAND_COMMIT.into();
    let left = of_kind(&lore, "lore_missing");
    assert!(left.iter().all(|item| !item.summary.starts_with("#1 API")), "the hand commit settles it: {left:?}");
    assert_eq!(left.len(), 1, "the fault still waits for its fix: {left:?}");
    // The next lore pass writes #1 as done with that commit and leaves it out.
    let (records, pending) = crate::state::teams::lore_sync::desired(&lore);
    assert!(pending.iter().all(|found| found.id != format!("{plan}#1")), "{pending:?}");
    let item = records.iter().find(|record| record["id"] == format!("{plan}#1")).expect("#1's record");
    assert_eq!((item["status"].clone(), item["commits"].clone()), (json!("done"), json!([HAND_COMMIT])), "{item}");
}

#[test]
fn a_closed_item_comes_back_only_when_its_content_changes() {
    let now = crate::state::now();
    let mut team = desk();
    let keys: Vec<String> = items(&team).into_iter().filter(closable).map(|item| item.key).collect();
    run(&mut team, "seen", json!({"items":keys}), &lead()).unwrap();
    for kind in KINDS {
        assert!(of_kind(&team, kind).is_empty(), "{kind} closed");
    }
    // Only time passes: the playbook item is dated now and waiting_to_land
    // counts minutes, and neither comes back.
    let minutes = regex::Regex::new(r"\b\d+ min\b").unwrap();
    let older = |item: &Item| Item { since: item.since - 7200.0, summary: minutes.replace_all(&item.summary, "130 min").into_owned(), ..item.clone() };
    for item in crate::state::teams::inbox::all_items(&team).iter().filter(|item| closable(item)) {
        assert_eq!(signature(item), signature(&older(item)), "{} changed with the clock alone", item.key);
    }
    let landing = crate::state::teams::inbox::all_items(&team).into_iter().find(|item| item.kind == "waiting_to_land").unwrap();
    assert!(landing.summary.contains("#1 (") && landing.summary.contains(" min)"), "{}", landing.summary);
    assert_ne!(older(&landing).summary, landing.summary, "the clock moved the words");
    for _ in 0..2 {
        assert!(of_kind(&team, "playbook").is_empty() && of_kind(&team, "waiting_to_land").is_empty(), "a read later they stay closed");
    }
    // Content changes bring each back.
    team.tasks.push(Task {
        id: 3,
        title: "Docs".into(),
        assignee: "dev".into(),
        status: TaskStatus::Accepted,
        attempts: vec![Attempt { n: 1, id: "A3".into(), member: "dev".into(), state: AttemptState::Reported, folder_head: "0123456789abcdef0123456789abcdef01234567".into(),
            report: Some(Report { text: "docs".into(), at: now - 60.0, ..Default::default() }), decided_at: Some(now - 30.0), ..Default::default() }],
        ..Default::default()
    });
    let back = of_kind(&team, "waiting_to_land");
    assert!(back.len() == 1 && back[0].summary.contains("#3"), "another task waits: {back:?}");
    team.test_notices.push(notice("rec-c", Some(1), "passed", now - 10.0));
    assert!(of_kind(&team, "test_result").iter().any(|item| item.task == Some(1)), "a new result for #1 comes back");
    team.lore.pending[0].text = "write the fault for finding f1 (slow status): fix, check".into();
    team.lore.pending[0].missing.push("check".into());
    assert_eq!(of_kind(&team, "lore_missing").len(), 1, "the fault's record changed");
    // The unchanged ones stay closed.
    assert!(of_kind(&team, "playbook").is_empty() && of_kind(&team, "refused_queued_call").is_empty());
}
