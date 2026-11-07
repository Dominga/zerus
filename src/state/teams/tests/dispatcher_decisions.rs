//! Task 156: decisions reach the work: the hand-out carries the binding ones,
//! a task in flight is told of a new one once, and a report that contradicts
//! one is flagged and held back from accept.
use super::*;
use super::support::*;
use crate::state::teams::decisions::{self, NewDecision};
use crate::state::teams::ops::{self, Actor};

fn decide(team: &mut Team, tasks: Vec<u32>, forbids: &[&str], at: f64) -> String {
    decisions::record(team, NewDecision {
        question: "Which embedding provider?".into(),
        choice: "built-in only, never Ollama".into(),
        why: "one binary".into(),
        by: "owner".into(),
        tasks,
        forbids: forbids.iter().map(|term| (*term).to_owned()).collect(),
        ..NewDecision::default()
    }, at, &mut Vec::new())
}

#[test]
fn the_hand_out_names_the_decisions_that_bind_its_task() {
    let mut team = team();
    decide(&mut team, vec![1], &[], 5.0);
    let task = team.task(1).unwrap().clone();
    let attempt = Attempt { n: 1, id: "A1".into(), member: "dev".into(), ..Default::default() };
    let text = hand_off_text(&team, &task, &attempt, "", "");
    assert!(text.contains("Decisions that bind this task") && text.contains("- d1 (owner"), "{text}");
    let other = team.task(2).unwrap().clone();
    assert!(!hand_off_text(&team, &other, &attempt, "", "").contains("Decisions that bind"), "#2 is not bound");
}

#[test]
fn a_task_in_flight_is_told_of_a_new_decision_once() {
    let mut team = team();
    delivered(&mut team);
    let seen = vec![session("claude/x/lead", "r0", false), session("codex/x/dev", "r1", false)];
    // A decision older than the hand-out was in it.
    decide(&mut team, vec![1], &[], 5.0);
    assert!(!steps(&team, &seen, 20.0).iter().any(|step| matches!(step, Step::TellDecision { .. })));
    let newer = decide(&mut team, vec![1], &[], 30.0);
    let found = steps(&team, &seen, 40.0);
    assert!(found.contains(&Step::TellDecision { task: 1, decision: newer.clone() }), "{found:?}");
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams.sqlite3"), true).unwrap();
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), team.clone(), vec![]).unwrap();
    let sent = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let keep = sent.clone();
    set_fake_hgs(Some(Box::new(move |_, args, input| {
        assert_eq!(&args[..2], &["send", "codex/x/dev"]);
        *keep.lock().unwrap() = input.map(|value| value["text"].as_str().unwrap_or_default().to_owned()).unwrap_or_default();
        Ok(fake_output(0, r#"{"status":"submitted"}"#, ""))
    })));
    let result = run_step(&mut store, &team, &Step::TellDecision { task: 1, decision: newer.clone() }, "");
    set_fake_hgs(None);
    result.unwrap();
    assert!(sent.lock().unwrap().contains("A decision of the owner binds your task #1"), "{}", sent.lock().unwrap());
    let saved = store.load(&team.id).unwrap().unwrap();
    assert!(!steps(&saved, &seen, 50.0).iter().any(|step| matches!(step, Step::TellDecision { .. })), "told once");
}

#[test]
fn a_report_that_contradicts_a_decision_is_flagged_before_accept() {
    let mut team = team();
    submit(&mut team, 1, "dev", false);
    team.task_mut(1).unwrap().attempts[0].report.as_mut().unwrap().text = "Added an Ollama client for embeddings".into();
    decide(&mut team, vec![1], &["ollama"], 5.0);
    let seen = vec![session("claude/x/lead", "r0", false), session("codex/x/dev", "r1", false)];
    assert!(steps(&team, &seen, 20.0).contains(&Step::ScanReport { task: 1 }));
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams.sqlite3"), true).unwrap();
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), team.clone(), vec![]).unwrap();
    run_step(&mut store, &team, &Step::ScanReport { task: 1 }, "").unwrap();
    let mut saved = store.load(&team.id).unwrap().unwrap();
    let attempt = &saved.task(1).unwrap().attempts[0];
    assert!(attempt.decisions_scanned);
    assert_eq!(attempt.decision_flags[0]["decision"], "d1");
    assert!(!steps(&saved, &seen, 30.0).iter().any(|step| matches!(step, Step::ScanReport { .. })), "scanned once");
    let item = super::super::inbox::items(&saved).into_iter().find(|item| item.kind == "decision_conflict").expect("the lead sees it");
    assert!(item.summary.contains("contradicts decision d1 (adds \"ollama\")"), "{}", item.summary);
    let error = ops::apply(&mut saved.clone(), "accept", &json!({"task":1}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut Vec::new())
        .unwrap_err()
        .to_string();
    assert!(error.contains("contradicts decision d1") && error.contains("against_decisions"), "{error}");
    let accepted = ops::apply(&mut saved, "accept", &json!({"task":1,"against_decisions":true}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut Vec::new());
    assert!(accepted.is_ok(), "{accepted:?}");
}
