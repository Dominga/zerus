//! Task 160: a parked task frees its member, never blocks its queue, comes
//! back to the same member in the same attempt when its condition holds, and
//! shows as parked (never as a build) in status, Waiting and the inbox.
use super::*;
use super::support::*;
use crate::state::teams::ops::{self, Actor};

const ATTEMPT: &str = "00000000-0000-4000-8000-000000000001";

fn park(team: &mut Team, condition: Value) -> Value {
    ops::apply(team, "park", &json!({"task":1,"attempt":ATTEMPT,"condition":condition,"eta_minutes":30,"note":"tests pass; only the install is left"}),
        &Actor::Member("dev".into()), &BTreeSet::new(), &mut Vec::new()).unwrap()
}

fn seen() -> Vec<Seen> {
    vec![session("claude/x/lead", "r0", false), session("codex/x/dev", "r1", true)]
}

#[test]
fn a_parked_task_frees_its_member_for_its_next_task() {
    let mut team = team();
    delivered(&mut team);
    // While #1 is open, dev is busy: #2 waits.
    assert!(!steps(&team, &seen(), 20.0).iter().any(|step| matches!(step, Step::HandOut { member, .. } if member == "dev")));
    let answer = park(&mut team, json!({"kind":"build","version":"1.58.0"}));
    assert_eq!(answer["status"], "parked");
    assert!(answer["parked"].as_str().unwrap().starts_with("parked until hgs version 1.58.0 installed, ETA "), "{answer}");
    let task = team.task(1).unwrap();
    assert_eq!(task.status, TaskStatus::Pending);
    assert_eq!(task.attempts[0].state, AttemptState::Parked, "the attempt is kept, not revoked");
    assert!(task.open_attempt().is_none(), "a parked attempt is not open");
    let found = steps(&team, &seen(), 20.0);
    assert!(found.contains(&Step::HandOut { task: 2, member: "dev".into() }), "{found:?}");
    assert!(!found.iter().any(|step| matches!(step, Step::HandOut { task: 1, .. } | Step::Resume { .. })), "{found:?}");
    // Only the task's member or its reviewer parks it, once, with an estimate.
    let error = ops::apply(&mut team.clone(), "park", &json!({"task":1,"condition":{"kind":"time","at":99.0},"eta_minutes":5}),
        &Actor::Member("qa".into()), &BTreeSet::new(), &mut Vec::new()).unwrap_err().to_string();
    assert!(error.contains("no open attempt") || error.contains("already parked"), "{error}");
}

#[test]
fn a_park_needs_a_known_condition_and_an_estimate() {
    let mut team = team();
    delivered(&mut team);
    let try_park = |team: &mut Team, input: Value, actor: &str| {
        ops::apply(team, "park", &input, &Actor::Member(actor.into()), &BTreeSet::new(), &mut Vec::new()).map_err(|error| error.to_string())
    };
    let error = try_park(&mut team.clone(), json!({"task":1,"condition":{"kind":"time","at":99.0},"eta_minutes":5}), "qa").unwrap_err();
    assert!(error.contains("Only the task's member"), "{error}");
    let error = try_park(&mut team.clone(), json!({"task":1,"condition":{"kind":"time","at":99.0}}), "dev").unwrap_err();
    assert!(error.contains("eta_minutes"), "{error}");
    let error = try_park(&mut team.clone(), json!({"task":1,"condition":{"kind":"poll","every":2},"eta_minutes":5}), "dev").unwrap_err();
    assert!(error.contains("A park condition is one of"), "{error}");
    let error = try_park(&mut team.clone(), json!({"task":1,"condition":{"kind":"task","task":1},"eta_minutes":5}), "dev").unwrap_err();
    assert!(error.contains("does not park on itself"), "{error}");
    let error = try_park(&mut team.clone(), json!({"task":1,"condition":{"kind":"ci","repo":"no-slash","run":5},"eta_minutes":5}), "dev").unwrap_err();
    assert!(error.contains("OWNER/REPO"), "{error}");
    // The lead parks a member's task too.
    try_park(&mut team, json!({"task":1,"condition":{"kind":"task","task":3},"eta_minutes":45}), "lead").unwrap();
    assert_eq!(team.task(1).unwrap().park.as_ref().unwrap().member, "dev");
}

#[test]
fn a_met_park_goes_back_to_the_same_member_in_the_same_attempt() {
    let mut team = team();
    delivered(&mut team);
    park(&mut team, json!({"kind":"task","task":3}));
    {
        let found = team.task_mut(1).unwrap().park.as_mut().unwrap();
        found.state = "met".into();
        found.detail = "task #3 was accepted".into();
    }
    let found = steps(&team, &seen(), 20.0);
    assert!(found.contains(&Step::Resume { task: 1, member: "dev".into() }), "{found:?}");
    assert!(!found.iter().any(|step| matches!(step, Step::HandOut { member, .. } if member == "dev")), "the return goes first: {found:?}");
    // A busy member gets it at its next free turn, not now.
    let mut busy = seen();
    busy[1].free = false;
    assert!(!steps(&team, &busy, 20.0).iter().any(|step| matches!(step, Step::Resume { .. })));
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams.sqlite3"), true).unwrap();
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), team.clone(), vec![]).unwrap();
    let sent = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let keep = sent.clone();
    let lead_told = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let told = lead_told.clone();
    set_fake_hgs(Some(Box::new(move |_, args, input| {
        // Task 161: the lead is told the outcome too (its doorbell).
        if args[1] == "codex/x/dev" {
            *keep.lock().unwrap() = input.map(|value| value["text"].as_str().unwrap_or_default().to_owned()).unwrap_or_default();
        } else {
            assert_eq!(&args[..2], &["send", "claude/x/lead"]);
            told.lock().unwrap().push(input.map(crate::state::teams::doorbell::posted_text).unwrap_or_default());
        }
        Ok(fake_output(0, r#"{"status":"submitted"}"#, ""))
    })));
    let result = run_step(&mut store, &team, &Step::Resume { task: 1, member: "dev".into() }, "");
    set_fake_hgs(None);
    result.unwrap();
    let saved = store.load(&team.id).unwrap().unwrap();
    let task = saved.task(1).unwrap();
    assert!(task.park.is_none());
    assert_eq!(task.status, TaskStatus::InProgress);
    assert_eq!(task.attempts.len(), 1, "the same attempt, no new one");
    assert_eq!((task.attempts[0].id.as_str(), task.attempts[0].state), (ATTEMPT, AttemptState::Delivered));
    let text = sent.lock().unwrap().clone();
    assert!(text.contains("Your parked task #1") && text.contains("task #3 accepted held (task #3 was accepted)"), "{text}");
    assert!(text.contains("tests pass; only the install is left") && text.contains(ATTEMPT), "{text}");
    // Task 161: the lead gets the outcome without watching.
    let told = lead_told.lock().unwrap().join("\n");
    assert!(told.contains("Task #1") && told.contains("is back with") && told.contains("task #3 was accepted"), "{told}");
}

#[test]
fn a_report_on_a_parked_attempt_gives_it_back_and_reports() {
    let mut team = team();
    delivered(&mut team);
    park(&mut team, json!({"kind":"time","at":4_000_000_000.0}));
    ops::apply(&mut team, "report", &json!({"task":1,"attempt":ATTEMPT,"text":"Done after all"}),
        &Actor::Member("dev".into()), &BTreeSet::new(), &mut Vec::new()).unwrap();
    let task = team.task(1).unwrap();
    assert!(task.park.is_none());
    assert_eq!(task.attempts[0].state, AttemptState::Reported);
    // The lead's unpark gives a parked task back at once.
    let mut other = super::support::team();
    delivered(&mut other);
    park(&mut other, json!({"kind":"time","at":4_000_000_000.0}));
    ops::apply(&mut other, "unpark", &json!({"task":1,"reason":"the owner installed it"}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut Vec::new()).unwrap();
    let found = other.task(1).unwrap().park.clone().unwrap();
    assert_eq!(found.state, "met");
    assert!(found.detail.contains("the owner installed it"), "{}", found.detail);
    // A cancel ends the park and its attempt.
    let mut cancelled = super::support::team();
    delivered(&mut cancelled);
    park(&mut cancelled, json!({"kind":"time","at":4_000_000_000.0}));
    ops::apply(&mut cancelled, "cancel", &json!({"task":1,"reason":"not needed"}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut Vec::new()).unwrap();
    let task = cancelled.task(1).unwrap();
    assert!(task.park.is_none());
    assert_eq!(task.attempts[0].state, AttemptState::Revoked);
}

#[test]
fn a_held_reject_never_blocks_its_members_later_tasks() {
    let mut team = team();
    submit(&mut team, 1, "dev", false);
    ops::apply(&mut team, "reject", &json!({"task":1,"reason":"Wait for #3","hold":true}), &Actor::Member("lead".into()),
        &BTreeSet::new(), &mut Vec::new()).unwrap();
    let task = team.task(1).unwrap();
    assert!(task.rework() && task.hold.is_some());
    assert!(!team.reserved_for_rework("dev", Some(2)), "a held rework reserves nobody");
    let found = steps(&team, &seen(), 20.0);
    assert!(found.contains(&Step::HandOut { task: 2, member: "dev".into() }), "{found:?}");
    // Released, the rework reserves its member again (A5).
    ops::apply(&mut team, "release", &json!({"task":1}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut Vec::new()).unwrap();
    assert!(team.reserved_for_rework("dev", Some(2)));
}

#[test]
fn a_park_shows_in_waiting_status_and_the_inbox_never_as_building() {
    let mut team = team();
    delivered(&mut team);
    park(&mut team, json!({"kind":"ci","repo":"WhiteWh/zerus","run":12345}));
    let reasons = super::super::waiting::reasons(std::slice::from_ref(&team), 20.0);
    let dev = reasons.iter().find(|waiting| waiting.name == "codex/x/dev").unwrap();
    let reason = dev.reason.as_ref().unwrap();
    assert_eq!(reason["kind"], "team_parked");
    assert!(reason["text"].as_str().unwrap().starts_with("Waiting for #1: parked until CI run 12345 of WhiteWh/zerus finishing, ETA "), "{reason}");
    assert!(dev.build.is_none(), "a park is never a build");
    // A failed condition is the lead's to decide, with the reason.
    {
        let found = team.task_mut(1).unwrap().park.as_mut().unwrap();
        found.state = "failed".into();
        found.detail = "CI run 12345 of WhiteWh/zerus finished: failure".into();
        found.checked_at = 30.0;
    }
    let items = super::super::inbox::items(&team);
    let item = items.iter().find(|item| item.kind == "park_failed").expect("the lead's item");
    assert!(item.summary.contains("it failed: CI run 12345 of WhiteWh/zerus finished: failure"), "{}", item.summary);
    assert!(item.command.contains("hgs team unpark --json"), "{}", item.command);
}
