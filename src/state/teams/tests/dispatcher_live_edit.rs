use super::super::ops::{self, Actor};
use super::support::*;
use super::*;

/// Task #6: one member change as the owner sends it, through the real settings
/// operation; each call is its own request, so a second change is a new one.
fn update_member(store: &mut Store, team_id: &str, update: Value) {
    let request = uuid::Uuid::new_v4().to_string();
    let empty = json!({});
    store
        .mutate(Some((request.as_str(), &empty)), team_id, None, |team, events| {
            ops::apply(team, "settings", &json!({"members_update":[update]}), &Actor::Human, &BTreeSet::new(), events)?;
            Ok(Value::Null)
        })
        .unwrap();
}

/// Task #6: what one dispatcher tick plans.
fn tick_steps(store: &Store, team_id: &str) -> Vec<Step> {
    let team = store.load(team_id).unwrap().unwrap();
    steps(&team, &free_sessions(), crate::state::now())
}

/// Task #6: runs the tick's `ApplyModel` or `ReplacePrompt`, as the worker does.
fn run_member_step(store: &mut Store, team_id: &str, what: &str) -> Vec<String> {
    let team = store.load(team_id).unwrap().unwrap();
    let mut ran = Vec::new();
    for step in tick_steps(store, team_id) {
        let name = match &step {
            Step::ApplyModel { .. } => "ApplyModel",
            Step::ReplacePrompt { .. } => "ReplacePrompt",
            _ => continue,
        };
        run_step(store, &team, &step, "").unwrap();
        ran.push(name.to_owned());
    }
    assert_eq!(ran, [what], "one member step runs, in its own tick");
    ran
}

/// Task #6: a start prompt changed while the member ran reaches it as the one
/// message that replaces its earlier instructions, and the next hand-out
/// carries the new prompt.
#[test]
fn a_changed_start_prompt_replaces_the_running_members_instructions() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let t = team();
        stored(&mut store, &t);
        update_member(&mut store, &t.id, json!({"id":"dev","prompt":"Ship the cards API"}));
        // The member takes no task in the tick that sends the message: the new
        // prompt reaches it first.
        let first = tick_steps(&store, &t.id);
        assert!(first.iter().any(|step| matches!(step, Step::ReplacePrompt { member } if member == "dev")), "{first:?}");
        assert!(
            !first.iter().any(|step| matches!(step, Step::HandOut { member, .. } if member == "dev")),
            "the hand-out waits for the message: {first:?}"
        );
        let sent = std::rc::Rc::new(std::cell::RefCell::new(Vec::<Value>::new()));
        let seen_sent = sent.clone();
        set_fake_hgs(Some(Box::new(move |_, args: &[&str], input: Option<&Value>| {
            assert_eq!(args.first().copied(), Some("send"), "only the replacing message goes to the session");
            seen_sent.borrow_mut().push(input.cloned().unwrap_or_default());
            Ok(fake_output(0, r#"{"status":"submitted"}"#, ""))
        })));
        run_member_step(&mut store, &t.id, "ReplacePrompt");
        set_fake_hgs(None);
        let saved = store.load(&t.id).unwrap().unwrap();
        assert!(saved.member("dev").unwrap().prompt_change.is_none(), "sent once");
        assert_eq!(event_kinds(&store, &t.id).last().map(String::as_str), Some("prompt_replaced"));
        let text = crate::state::teams::doorbell::posted_text(&sent.borrow()[0]);
        assert!(text.contains("replaces previous instructions"), "{text}");
        assert!(text.contains("Ship the cards API"), "{text}");
        // The next hand-out gives the member its next task with the new prompt.
        let handout = std::rc::Rc::new(std::cell::RefCell::new(String::new()));
        let seen_handout = handout.clone();
        set_fake_hgs(Some(Box::new(move |_, _: &[&str], input: Option<&Value>| {
            *seen_handout.borrow_mut() = input.map(crate::state::teams::doorbell::posted_text).unwrap_or_default();
            Ok(fake_output(0, r#"{"status":"submitted"}"#, ""))
        })));
        hand_out(&mut store, &t.id, 1, "dev", None, "").unwrap();
        set_fake_hgs(None);
        assert!(handout.borrow().contains("Ship the cards API"), "{}", handout.borrow());
    });
}

/// Task #6: "Clear context first" clears the member's context before the
/// replacing message, and the message names the conversation the clear made.
#[test]
fn a_changed_start_prompt_clears_the_context_first() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let mut t = team();
        // A session that names the conversation the clear replaces.
        with_context(&mut t, "dev", "none");
        stored(&mut store, &t);
        update_member(
            &mut store,
            &t.id,
            json!({"id":"dev","prompt":"Ship the cards API","clear_context_first":true}),
        );
        let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let inspects = std::rc::Rc::new(std::cell::Cell::new(0));
        let sent = std::rc::Rc::new(std::cell::RefCell::new(Value::Null));
        let (seen, seen_inspects, seen_sent) = (calls.clone(), inspects.clone(), sent.clone());
        set_fake_hgs(Some(Box::new(move |_, args: &[&str], input: Option<&Value>| {
            let call = args.first().copied().unwrap_or_default().to_owned();
            seen.borrow_mut().push(call.clone());
            match call.as_str() {
                "clear-context" => Ok(fake_output(
                    0,
                    &json!({"request_id":input.unwrap()["request_id"],"run_id":"r1","conversation_id":"c1","status":"submitted"}).to_string(),
                    "",
                )),
                // The old conversation first: the message waits for the clear.
                "inspect" => {
                    seen_inspects.set(seen_inspects.get() + 1);
                    let conversation = if seen_inspects.get() == 1 { "c1" } else { "c2" };
                    Ok(fake_output(0, &json!({"run_id":"r1","conversation_id":conversation,"activity":"idle","phase":"idle"}).to_string(), ""))
                }
                "send" => {
                    *seen_sent.borrow_mut() = input.cloned().unwrap_or_default();
                    Ok(fake_output(0, r#"{"status":"submitted"}"#, ""))
                }
                other => panic!("unexpected hgs {other}"),
            }
        })));
        // The clear starts; nothing is sent yet.
        run_member_step(&mut store, &t.id, "ReplacePrompt");
        assert_eq!(*calls.borrow(), ["clear-context"]);
        assert!(store.load(&t.id).unwrap().unwrap().member("dev").unwrap().prompt_change.is_some());
        // The old conversation is still there: the message waits.
        run_member_step(&mut store, &t.id, "ReplacePrompt");
        assert_eq!(*calls.borrow(), ["clear-context", "inspect"]);
        // The new conversation appears: the next tick sends the replacing message.
        run_member_step(&mut store, &t.id, "ReplacePrompt");
        set_fake_hgs(None);
        assert_eq!(*calls.borrow(), ["clear-context", "inspect", "inspect", "send"]);
        assert_eq!(sent.borrow()["expected_conversation_id"], "c2");
        assert!(
            sent.borrow()["text"].as_str().unwrap_or_default().contains("replaces previous instructions"),
            "{}",
            sent.borrow()
        );
        let saved = store.load(&t.id).unwrap().unwrap();
        assert!(saved.member("dev").unwrap().prompt_change.is_none());
        assert_eq!(saved.member("dev").unwrap().session.as_ref().unwrap().conversation_id, "c2", "the member follows its cleared context");
        assert_eq!(event_kinds(&store, &t.id).last().map(String::as_str), Some("prompt_replaced"));
    });
}

/// Task #6: a model change reaches the member's session before its next task.
#[test]
fn a_changed_model_switches_the_running_member_before_its_next_task() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let t = team();
        stored(&mut store, &t);
        update_member(&mut store, &t.id, json!({"id":"dev","model":"gpt-5.5","effort":"high"}));
        // The task waits for the switch: the member takes no hand-out first.
        let first = tick_steps(&store, &t.id);
        assert!(first.iter().any(|step| matches!(step, Step::ApplyModel { member } if member == "dev")), "{first:?}");
        assert!(
            !first.iter().any(|step| matches!(step, Step::HandOut { member, .. } if member == "dev")),
            "the hand-out waits for the switch: {first:?}"
        );
        let settings = std::rc::Rc::new(std::cell::RefCell::new(Vec::<Value>::new()));
        let seen_settings = settings.clone();
        set_fake_hgs(Some(Box::new(move |_, args: &[&str], input: Option<&Value>| {
            assert_eq!(args.first().copied(), Some("settings"), "only the switch goes to the session");
            seen_settings.borrow_mut().push(input.cloned().unwrap_or_default());
            Ok(fake_output(0, r#"{"status":"applied"}"#, ""))
        })));
        run_member_step(&mut store, &t.id, "ApplyModel");
        set_fake_hgs(None);
        let request = settings.borrow()[0].clone();
        assert_eq!(
            (request["model"].as_str(), request["effort"].as_str(), request["expected_run_id"].as_str()),
            (Some("gpt-5.5"), Some("high"), Some("r1"))
        );
        let saved = store.load(&t.id).unwrap().unwrap();
        assert!(saved.member("dev").unwrap().model_switch.is_none(), "switched once");
        assert_eq!(event_kinds(&store, &t.id).last().map(String::as_str), Some("model_switched"));
        // The next tick hands the member its next task.
        let next = tick_steps(&store, &t.id);
        assert!(
            next.iter().any(|step| matches!(step, Step::HandOut { task: 1, member } if member == "dev")),
            "{next:?}"
        );
    });
}

/// Task #6: a refused switch is recorded once and does not hold the member's
/// task back.
#[test]
fn a_refused_switch_is_recorded_and_not_repeated() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let t = team();
        stored(&mut store, &t);
        update_member(&mut store, &t.id, json!({"id":"dev","model":"gpt-5.5"}));
        set_fake_hgs(Some(Box::new(|_, _: &[&str], _: Option<&Value>| Ok(fake_output(1, "", "hgs: the session is busy")))));
        run_member_step(&mut store, &t.id, "ApplyModel");
        set_fake_hgs(None);
        let saved = store.load(&t.id).unwrap().unwrap();
        assert!(saved.member("dev").unwrap().model_switch.is_none(), "a refusal does not repeat");
        // A refused switch leaves the session on the model it ran, so the
        // member's model follows it back.
        assert_eq!((saved.member("dev").unwrap().model.as_str(), saved.member("dev").unwrap().effort.as_str()), ("", ""));
        assert_eq!(event_kinds(&store, &t.id).last().map(String::as_str), Some("model_switch_failed"));
        let next = tick_steps(&store, &t.id);
        assert!(
            next.iter().any(|step| matches!(step, Step::HandOut { task: 1, member } if member == "dev")),
            "the task goes out on the model the session runs: {next:?}"
        );
    });
}

/// Task #6: a model change the owner makes while the hgs call runs is newer and
/// stays for the next tick; the older switch does not clear it.
#[test]
fn a_newer_model_change_survives_the_switch_in_flight() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let t = team();
        stored(&mut store, &t);
        update_member(&mut store, &t.id, json!({"id":"dev","model":"gpt-5.5"}));
        // The tick plans the first switch from the team as it is now.
        let planned = store.load(&t.id).unwrap().unwrap();
        let step = steps(&planned, &free_sessions(), now()).into_iter().find(|step| matches!(step, Step::ApplyModel { .. })).unwrap();
        // The owner changes the model again before the call returns.
        update_member(&mut store, &t.id, json!({"id":"dev","model":"gpt-6"}));
        let settings = std::rc::Rc::new(std::cell::RefCell::new(Vec::<Value>::new()));
        let seen = settings.clone();
        set_fake_hgs(Some(Box::new(move |_, args: &[&str], input: Option<&Value>| {
            assert_eq!(args.first().copied(), Some("settings"));
            seen.borrow_mut().push(input.cloned().unwrap_or_default());
            Ok(fake_output(0, r#"{"status":"applied"}"#, ""))
        })));
        run_step(&mut store, &planned, &step, "").unwrap();
        set_fake_hgs(None);
        assert_eq!(settings.borrow()[0]["model"], "gpt-5.5", "the older call went out first");
        let saved = store.load(&t.id).unwrap().unwrap();
        let dev = saved.member("dev").unwrap();
        assert_eq!(dev.model, "gpt-6", "the newer change stands");
        assert_eq!(dev.model_switch.as_ref().map(|change| change.model.as_str()), Some("gpt-6"), "the newer switch still waits");
        assert!(!event_kinds(&store, &t.id).iter().any(|kind| kind == "model_switched"), "the older change is not recorded as done");
        // The next tick switches to the newer model and clears it.
        set_fake_hgs(Some(Box::new(|_, _: &[&str], _: Option<&Value>| Ok(fake_output(0, r#"{"status":"applied"}"#, "")))));
        run_member_step(&mut store, &t.id, "ApplyModel");
        set_fake_hgs(None);
        let saved = store.load(&t.id).unwrap().unwrap();
        assert!(saved.member("dev").unwrap().model_switch.is_none());
        assert_eq!(event_kinds(&store, &t.id).last().map(String::as_str), Some("model_switched"));
    });
}

/// Task #6: a prompt change the owner makes while the message is on its way is
/// newer and stays; the older send does not clear it.
#[test]
fn a_newer_prompt_change_survives_the_message_in_flight() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let t = team();
        stored(&mut store, &t);
        update_member(&mut store, &t.id, json!({"id":"dev","prompt":"Ship the cards API"}));
        let planned = store.load(&t.id).unwrap().unwrap();
        let step = steps(&planned, &free_sessions(), now()).into_iter().find(|step| matches!(step, Step::ReplacePrompt { .. })).unwrap();
        // The owner writes the next prompt before the first message returns.
        update_member(&mut store, &t.id, json!({"id":"dev","prompt":"Ship the cards API and the audit log"}));
        let sent = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let seen = sent.clone();
        set_fake_hgs(Some(Box::new(move |_, _: &[&str], input: Option<&Value>| {
            seen.borrow_mut().push(input.and_then(|input| input["text"].as_str()).unwrap_or_default().to_owned());
            Ok(fake_output(0, r#"{"status":"submitted"}"#, ""))
        })));
        run_step(&mut store, &planned, &step, "").unwrap();
        set_fake_hgs(None);
        assert!(sent.borrow().is_empty(), "the stale message does not go out: {sent:?}");
        let saved = store.load(&t.id).unwrap().unwrap();
        let dev = saved.member("dev").unwrap();
        assert_eq!(dev.prompt, "Ship the cards API and the audit log");
        assert!(dev.prompt_change.is_some(), "the newer prompt still waits: {dev:?}");
        // The next tick sends the newer prompt and clears it.
        let sent = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let seen = sent.clone();
        set_fake_hgs(Some(Box::new(move |_, _: &[&str], input: Option<&Value>| {
            seen.borrow_mut().push(input.and_then(|input| input["text"].as_str()).unwrap_or_default().to_owned());
            Ok(fake_output(0, r#"{"status":"submitted"}"#, ""))
        })));
        run_member_step(&mut store, &t.id, "ReplacePrompt");
        set_fake_hgs(None);
        assert!(sent.borrow()[0].contains("and the audit log"), "{}", sent.borrow()[0]);
        assert!(store.load(&t.id).unwrap().unwrap().member("dev").unwrap().prompt_change.is_none());
    });
}

/// Task #6: a second prompt edit while a clear waits keeps the clear record and
/// the clear, and the newer prompt is sent from the cleared conversation.
#[test]
fn a_second_prompt_edit_keeps_the_clear_in_flight() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let mut t = team();
        with_context(&mut t, "dev", "none");
        stored(&mut store, &t);
        update_member(&mut store, &t.id, json!({"id":"dev","prompt":"First instructions","clear_context_first":true}));
        let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let inspects = std::rc::Rc::new(std::cell::Cell::new(0));
        let sent = std::rc::Rc::new(std::cell::RefCell::new(Value::Null));
        let (seen, seen_inspects, seen_sent) = (calls.clone(), inspects.clone(), sent.clone());
        set_fake_hgs(Some(Box::new(move |_, args: &[&str], input: Option<&Value>| {
            let call = args.first().copied().unwrap_or_default().to_owned();
            seen.borrow_mut().push(call.clone());
            match call.as_str() {
                "clear-context" => Ok(fake_output(
                    0,
                    &json!({"request_id":input.unwrap()["request_id"],"run_id":"r1","conversation_id":"c1","status":"submitted"}).to_string(),
                    "",
                )),
                "inspect" => {
                    seen_inspects.set(seen_inspects.get() + 1);
                    let conversation = if seen_inspects.get() == 1 { "c1" } else { "c2" };
                    Ok(fake_output(0, &json!({"run_id":"r1","conversation_id":conversation,"activity":"idle","phase":"idle"}).to_string(), ""))
                }
                "send" => {
                    *seen_sent.borrow_mut() = input.cloned().unwrap_or_default();
                    Ok(fake_output(0, r#"{"status":"submitted"}"#, ""))
                }
                other => panic!("unexpected hgs {other}"),
            }
        })));
        // The first tick starts the clear and records it.
        run_member_step(&mut store, &t.id, "ReplacePrompt");
        assert_eq!(*calls.borrow(), ["clear-context"]);
        assert!(store.load(&t.id).unwrap().unwrap().member("dev").unwrap().prompt_change.as_ref().unwrap().clear.is_some());
        // The owner edits the prompt again without the checkbox.
        update_member(&mut store, &t.id, json!({"id":"dev","prompt":"Second instructions"}));
        let waiting = store.load(&t.id).unwrap().unwrap();
        let change = waiting.member("dev").unwrap().prompt_change.as_ref().expect("the clear is still in flight");
        assert!(change.clear_context, "the first edit's clear stays on");
        assert!(change.clear.is_some(), "the clear request is kept");
        // The clear confirms, and the newer prompt is sent in its conversation.
        run_member_step(&mut store, &t.id, "ReplacePrompt");
        run_member_step(&mut store, &t.id, "ReplacePrompt");
        set_fake_hgs(None);
        assert_eq!(*calls.borrow(), ["clear-context", "inspect", "inspect", "send"]);
        assert_eq!(sent.borrow()["expected_conversation_id"], "c2");
        assert!(sent.borrow()["text"].as_str().unwrap_or_default().contains("Second instructions"), "{}", sent.borrow());
        assert_eq!(store.load(&t.id).unwrap().unwrap().member("dev").unwrap().session.as_ref().unwrap().conversation_id, "c2");
    });
}

/// Task #6: a send interrupted by a crash is recorded uncertain and never sent
/// again under a new id (U45's rule).
#[test]
fn an_interrupted_prompt_send_is_uncertain_and_not_replayed() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let t = team();
        stored(&mut store, &t);
        update_member(&mut store, &t.id, json!({"id":"dev","prompt":"Ship the cards API"}));
        // A previous tick recorded the send and crashed before it returned.
        store
            .mutate(None, &t.id, None, |team, _| {
                let change = team.members.iter_mut().find(|member| member.id == "dev").unwrap().prompt_change.as_mut().unwrap();
                change.sent_at = Some(10.0);
                Ok(Value::Null)
            })
            .unwrap();
        let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let seen = calls.clone();
        set_fake_hgs(Some(Box::new(move |_, args: &[&str], _: Option<&Value>| {
            seen.borrow_mut().push(args.first().copied().unwrap_or_default().to_owned());
            Ok(fake_output(0, r#"{"status":"submitted"}"#, ""))
        })));
        run_member_step(&mut store, &t.id, "ReplacePrompt");
        set_fake_hgs(None);
        assert!(calls.borrow().is_empty(), "nothing goes out twice: {calls:?}");
        let saved = store.load(&t.id).unwrap().unwrap();
        assert!(saved.member("dev").unwrap().prompt_change.is_none(), "the uncertain change leaves");
        assert_eq!(event_kinds(&store, &t.id).last().map(String::as_str), Some("prompt_change_uncertain"));
    });
}

/// Task #6: a member Zerus is already switching for a task (U45) keeps that
/// claim; the prompt message waits for it instead of taking the member.
#[test]
fn a_task_switch_in_flight_keeps_its_member() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let t = team();
        stored(&mut store, &t);
        update_member(&mut store, &t.id, json!({"id":"dev","prompt":"Ship the cards API"}));
        store
            .mutate(None, &t.id, None, |team, _| {
                // A task that names a model is the U45 path a switch waits on.
                let task = team.task_mut(1).unwrap();
                task.models = vec!["opus".into()];
                super::super::models::record(
                    task,
                    PrepareStep { step: "model".into(), member: "dev".into(), to: "opus".into(), result: "pending".into(), ..Default::default() },
                );
                Ok(Value::Null)
            })
            .unwrap();
        let saved = store.load(&t.id).unwrap().unwrap();
        let planned = steps(&saved, &free_sessions(), now());
        assert!(!planned.iter().any(|step| matches!(step, Step::ReplacePrompt { .. })), "{planned:?}");
        // Once the task's switch is done, the message goes out.
        store
            .mutate(None, &t.id, None, |team, _| {
                team.task_mut(1).unwrap().prepare.clear();
                Ok(Value::Null)
            })
            .unwrap();
        let saved = store.load(&t.id).unwrap().unwrap();
        assert!(steps(&saved, &free_sessions(), now()).iter().any(|step| matches!(step, Step::ReplacePrompt { .. })));
    });
}

/// Task #6: a model step left on a cancelled task is no claim on its member: the
/// member's own edit still goes out, and cancel drops the step.
#[test]
fn a_cancelled_tasks_pending_switch_does_not_block_a_live_edit() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let t = team();
        stored(&mut store, &t);
        update_member(&mut store, &t.id, json!({"id":"dev","prompt":"Ship the cards API"}));
        // A U45 switch left pending on a task that was cancelled later.
        store
            .mutate(None, &t.id, None, |team, _| {
                let task = team.task_mut(1).unwrap();
                task.status = TaskStatus::Cancelled;
                super::super::models::record(
                    task,
                    PrepareStep { step: "model".into(), member: "dev".into(), to: "opus".into(), result: "pending".into(), ..Default::default() },
                );
                Ok(Value::Null)
            })
            .unwrap();
        let saved = store.load(&t.id).unwrap().unwrap();
        let planned = steps(&saved, &free_sessions(), now());
        assert!(
            planned.iter().any(|step| matches!(step, Step::ReplacePrompt { member } if member == "dev")),
            "a finished task's step claims nothing: {planned:?}"
        );
        // The cancel op drops a model step still pending on its task.
        store
            .mutate(None, &t.id, None, |team, events| {
                super::super::models::record(
                    team.task_mut(2).unwrap(),
                    PrepareStep { step: "model".into(), member: "dev".into(), to: "opus".into(), result: "pending".into(), ..Default::default() },
                );
                ops::apply(team, "cancel", &json!({"task":2}), &Actor::Human, &BTreeSet::new(), events)?;
                Ok(Value::Null)
            })
            .unwrap();
        let saved = store.load(&t.id).unwrap().unwrap();
        assert_eq!(saved.task(2).unwrap().status, TaskStatus::Cancelled);
        assert!(saved.task(2).unwrap().prepare.iter().all(|step| step.step != "model"), "cancel drops the pending switch");
    });
}

/// Task #6: an edit made while a message is in flight does not inherit the clear
/// that message already finished (which would time out or clear twice).
#[test]
fn a_prompt_edit_during_the_send_does_not_reuse_a_finished_clear() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let mut t = team();
        with_context(&mut t, "dev", "none");
        stored(&mut store, &t);
        update_member(&mut store, &t.id, json!({"id":"dev","prompt":"First instructions","clear_context_first":true}));
        // The clear finished and the first message is on its way.
        store
            .mutate(None, &t.id, None, |team, _| {
                let change = team.members.iter_mut().find(|member| member.id == "dev").unwrap().prompt_change.as_mut().unwrap();
                change.clear = Some(PromptClear { request_id: "clear-1".into(), run_id: "r1".into(), conversation_id: "c1".into(), started_at: 1.0 });
                change.sent_at = Some(2.0);
                Ok(Value::Null)
            })
            .unwrap();
        // The owner edits the prompt again while that message is on its way.
        update_member(&mut store, &t.id, json!({"id":"dev","prompt":"Second instructions"}));
        let saved = store.load(&t.id).unwrap().unwrap();
        let change = saved.member("dev").unwrap().prompt_change.as_ref().expect("the newer prompt waits");
        assert!(!change.clear_context && change.clear.is_none(), "the finished clear is not reused: {change:?}");
        assert!(change.sent_at.is_none(), "the newer prompt is a send of its own");
        // The next tick sends it without touching the old clear.
        let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let seen = calls.clone();
        set_fake_hgs(Some(Box::new(move |_, args: &[&str], input: Option<&Value>| {
            seen.borrow_mut().push(args.first().copied().unwrap_or_default().to_owned());
            assert!(input.and_then(|input| input["text"].as_str()).is_some_and(|text| text.contains("Second instructions")));
            Ok(fake_output(0, r#"{"status":"submitted"}"#, ""))
        })));
        run_member_step(&mut store, &t.id, "ReplacePrompt");
        set_fake_hgs(None);
        assert_eq!(*calls.borrow(), ["send"]);
        assert!(store.load(&t.id).unwrap().unwrap().member("dev").unwrap().prompt_change.is_none());
    });
}

/// Task #6: an uncertain switch keeps the new model (the session may have it),
/// unlike a definite refusal that puts the old one back.
#[test]
fn an_uncertain_switch_keeps_the_new_model() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let t = team();
        stored(&mut store, &t);
        update_member(&mut store, &t.id, json!({"id":"dev","model":"gpt-5.5"}));
        set_fake_hgs(Some(Box::new(|_, _: &[&str], _: Option<&Value>| Err(anyhow::anyhow!("hgs: the pipe closed")))));
        run_member_step(&mut store, &t.id, "ApplyModel");
        set_fake_hgs(None);
        let saved = store.load(&t.id).unwrap().unwrap();
        assert!(saved.member("dev").unwrap().model_switch.is_none());
        assert_eq!(saved.member("dev").unwrap().model, "gpt-5.5", "an uncertain call may have reached the session");
        assert_eq!(event_kinds(&store, &t.id).last().map(String::as_str), Some("model_switch_uncertain"));
    });
}
