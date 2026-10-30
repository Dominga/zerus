use super::super::ops::{self, Actor};
use super::*;
use super::support::*;

#[test]
fn a_members_context_setting_overrides_its_role() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let t = team();
        let dev = t.member("dev").unwrap().clone();
        assert_eq!(context_before_task(&t, &dev), "", "no fields keeps today's none");
        let mut role = dev.clone();
        role.prompt_snapshot = Some(PromptSnapshot { context_before_task: "clear".into(), ..Default::default() });
        assert_eq!(context_before_task(&t, &role), "clear");
        // A member's own setting wins, and "none" overrides the role.
        role.context_before_task = "compact".into();
        assert_eq!(context_before_task(&t, &role), "compact");
        role.context_before_task = "none".into();
        assert_eq!(context_before_task(&t, &role), "");
    });
}

#[test]
fn a_team_without_a_context_action_hands_out_and_decides_as_today() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let t = team();
        // A team with none of U46's fields loads and serializes as today.
        let value = serde_json::to_value(&t).unwrap();
        assert!(value["members"][1].get("context_before_task").is_none(), "{value}");
        assert!(value["tasks"][0].get("waits").is_none(), "{value}");
        stored(&mut store, &t);
        let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let record = calls.clone();
        set_fake_hgs(Some(Box::new(move |_, args: &[&str], _| {
            record.borrow_mut().push(args.first().copied().unwrap_or_default().to_owned());
            Ok(fake_output(0, r#"{"status":"submitted"}"#, ""))
        })));
        hand_out(&mut store, &t.id, 1, "dev", None, "").unwrap();
        let saved = store.load(&t.id).unwrap().unwrap();
        let attempt = saved.task(1).unwrap().attempts.last().unwrap();
        assert!(attempt.prepare.is_empty(), "{attempt:?}");
        assert_eq!(attempt.state, AttemptState::Delivered);
        assert_eq!(*calls.borrow(), ["send"], "a team that sets none gets today's hand-out, with no context call");
        // A report and an accept keep today's events too.
        let attempt_id = attempt.id.clone();
        store
            .mutate(Some(("22222222-2222-4222-8222-222222222222", &json!({}))), &t.id, None, |team, events| {
                ops::apply(
                    team,
                    "report",
                    &json!({"task":1,"attempt":attempt_id,"text":"Done"}),
                    &Actor::Member("dev".into()),
                    &BTreeSet::new(),
                    events,
                )?;
                Ok(Value::Null)
            })
            .unwrap();
        store
            .mutate(Some(("33333333-3333-4333-8333-333333333333", &json!({}))), &t.id, None, |team, events| {
                ops::apply(team, "accept", &json!({"task":1}), &Actor::Human, &BTreeSet::new(), events)?;
                Ok(Value::Null)
            })
            .unwrap();
        assert_eq!(event_kinds(&store, &t.id), ["handed_out", "delivered", "reported", "accepted"]);
        set_fake_hgs(None);
    });
}

#[test]
fn a_compact_waits_for_its_confirmation_before_the_hand_out() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let mut t = team();
        with_context(&mut t, "dev", "compact");
        // The member overrides the role snapshot, which asks for clear.
        t.members.iter_mut().find(|member| member.id == "dev").unwrap().prompt_snapshot = Some(PromptSnapshot {
            context_before_task: "clear".into(),
            ..Default::default()
        });
        stored(&mut store, &t);
        let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let request = std::rc::Rc::new(std::cell::RefCell::new(String::new()));
        let inspects = std::rc::Rc::new(std::cell::Cell::new(0));
        let (seen, seen_request, seen_inspects) = (calls.clone(), request.clone(), inspects.clone());
        set_fake_hgs(Some(Box::new(move |_, args: &[&str], input: Option<&Value>| {
            let call = args.first().copied().unwrap_or_default().to_owned();
            seen.borrow_mut().push(call.clone());
            match call.as_str() {
                "compact-context" => {
                    let input = input.unwrap();
                    let id = input["request_id"].as_str().unwrap().to_owned();
                    assert_eq!(input["expected_run_id"], "r1");
                    assert_eq!(input["expected_conversation_id"], "c1");
                    *seen_request.borrow_mut() = id.clone();
                    Ok(fake_output(0, &json!({"request_id":id,"status":"submitted"}).to_string(), ""))
                }
                // The first look shows it still compacting: the hand-out waits.
                "inspect" => {
                    seen_inspects.set(seen_inspects.get() + 1);
                    let status = if seen_inspects.get() == 1 { "compacting" } else { "completed" };
                    Ok(fake_output(0, &json!({"run_id":"r1","conversation_id":"c1","activity":"idle","phase":"idle",
                        "compact_context_request":{"request_id":seen_request.borrow().clone(),"run_id":"r1",
                            "conversation_id":"c1","status":status}}).to_string(), ""))
                }
                "send" => Ok(fake_output(0, r#"{"status":"submitted"}"#, "")),
                other => panic!("unexpected hgs {other}"),
            }
        })));
        hand_out(&mut store, &t.id, 1, "dev", None, "").unwrap();
        // The action is started and recorded; nothing is sent yet.
        assert_eq!(*calls.borrow(), ["compact-context"]);
        let saved = store.load(&t.id).unwrap().unwrap();
        let wait = saved.task(1).unwrap().waits.as_ref().expect("the wait is recorded");
        assert_eq!((wait.state.as_str(), wait.action.as_str(), wait.conversation.as_str()), ("waiting", "compact", "c1"));
        assert!(!wait.request_id.is_empty());
        assert!(saved.task(1).unwrap().attempts.is_empty());
        // A tick while it waits reads the record once and runs no action again.
        assert!(prepare_tick(&mut store, &t.id));
        assert_eq!(*calls.borrow(), ["compact-context", "inspect"]);
        let saved = store.load(&t.id).unwrap().unwrap();
        assert_eq!(saved.task(1).unwrap().waits.as_ref().unwrap().state, "waiting");
        assert!(saved.task(1).unwrap().attempts.is_empty());
        // The confirmation arrives: the next tick hands out.
        assert!(prepare_tick(&mut store, &t.id));
        set_fake_hgs(None);
        let saved = store.load(&t.id).unwrap().unwrap();
        let task = saved.task(1).unwrap();
        assert!(task.waits.is_none(), "the confirmed wait is gone");
        let attempt = task.attempts.last().unwrap();
        assert_eq!(
            (attempt.prepare[0].step.as_str(), attempt.prepare[0].to.as_str(), attempt.prepare[0].result.as_str()),
            ("context", "compact", "done")
        );
        assert_eq!(attempt.state, AttemptState::Delivered);
        assert_eq!(*calls.borrow(), ["compact-context", "inspect", "inspect", "send"], "the send waits for the confirmation");
        assert_eq!(inspects.get(), 2);
        assert_eq!(event_kinds(&store, &t.id), ["context_prepared", "handed_out", "delivered"]);
    });
}

#[test]
fn a_clear_waits_for_the_new_conversation_before_the_hand_out() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let mut t = team();
        t.task_mut(1).unwrap().description = "Build the cards API".into();
        t.task_mut(1).unwrap().criteria = vec!["Paged list".into()];
        // The role snapshot asks for a clear; the member adds no override.
        let dev = t.members.iter_mut().find(|member| member.id == "dev").unwrap();
        dev.session = Some(SessionRef { name: "codex/x/dev".into(), run_id: "r1".into(), conversation_id: "c1".into(), ..Default::default() });
        dev.state = MemberState::Active;
        dev.prompt_snapshot = Some(PromptSnapshot { context_before_task: "clear".into(), ..Default::default() });
        stored(&mut store, &t);
        let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let inspects = std::rc::Rc::new(std::cell::Cell::new(0));
        let sent = std::rc::Rc::new(std::cell::RefCell::new(Value::Null));
        let (seen, seen_inspects, seen_sent) = (calls.clone(), inspects.clone(), sent.clone());
        set_fake_hgs(Some(Box::new(move |_, args: &[&str], input: Option<&Value>| {
            let call = args.first().copied().unwrap_or_default().to_owned();
            seen.borrow_mut().push(call.clone());
            match call.as_str() {
                "clear-context" => Ok(fake_output(0, &json!({"request_id":input.unwrap()["request_id"],
                    "name":"codex/x/dev","run_id":"r1","conversation_id":"c1","status":"submitted"}).to_string(), "")),
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
        hand_out(&mut store, &t.id, 1, "dev", None, "").unwrap();
        assert_eq!(*calls.borrow(), ["clear-context"]);
        let saved = store.load(&t.id).unwrap().unwrap();
        let wait = saved.task(1).unwrap().waits.as_ref().expect("the wait is recorded");
        assert_eq!((wait.state.as_str(), wait.action.as_str(), wait.conversation.as_str()), ("waiting", "clear", "c1"));
        assert!(saved.task(1).unwrap().attempts.is_empty());
        // The old conversation is still there: the hand-out waits.
        assert!(prepare_tick(&mut store, &t.id));
        assert_eq!(*calls.borrow(), ["clear-context", "inspect"]);
        // The new conversation appears: the next tick hands out naming it.
        assert!(prepare_tick(&mut store, &t.id));
        set_fake_hgs(None);
        let saved = store.load(&t.id).unwrap().unwrap();
        assert!(saved.task(1).unwrap().waits.is_none());
        let attempt = saved.task(1).unwrap().attempts.last().unwrap();
        assert_eq!((attempt.prepare[0].to.as_str(), attempt.prepare[0].result.as_str()), ("clear", "done"));
        assert_eq!(attempt.state, AttemptState::Delivered);
        assert_eq!(*calls.borrow(), ["clear-context", "inspect", "inspect", "send"]);
        // The member's session follows the new conversation, and the send names it.
        assert_eq!(saved.member("dev").unwrap().session.as_ref().unwrap().conversation_id, "c2");
        assert_eq!(sent.borrow()["expected_conversation_id"], "c2");
        // A cleared context gets the task's own input, never "as before".
        let text = sent.borrow()["text"].as_str().unwrap_or_default().to_owned();
        assert!(text.contains("Build the cards API"), "{text}");
        assert!(text.contains("Paged list"), "{text}");
        assert!(!text.to_lowercase().contains("as before"), "{text}");
    });
}

#[test]
fn a_tick_while_the_context_waits_does_not_run_the_action_again() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let mut t = team();
        with_context(&mut t, "dev", "compact");
        stored(&mut store, &t);
        let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let request = std::rc::Rc::new(std::cell::RefCell::new(String::new()));
        let (seen, seen_request) = (calls.clone(), request.clone());
        set_fake_hgs(Some(Box::new(move |_, args: &[&str], input: Option<&Value>| {
            let call = args.first().copied().unwrap_or_default().to_owned();
            seen.borrow_mut().push(call.clone());
            match call.as_str() {
                "compact-context" => {
                    let id = input.unwrap()["request_id"].as_str().unwrap().to_owned();
                    *seen_request.borrow_mut() = id.clone();
                    Ok(fake_output(0, &json!({"request_id":id,"status":"submitted"}).to_string(), ""))
                }
                "inspect" => Ok(fake_output(0, &json!({"run_id":"r1","conversation_id":"c1","activity":"idle","phase":"idle",
                    "compact_context_request":{"request_id":seen_request.borrow().clone(),"run_id":"r1",
                        "conversation_id":"c1","status":"compacting"}}).to_string(), "")),
                "send" => panic!("a waiting context never hands out"),
                other => panic!("unexpected hgs {other}"),
            }
        })));
        hand_out(&mut store, &t.id, 1, "dev", None, "").unwrap();
        let recorded = store.load(&t.id).unwrap().unwrap().task(1).unwrap().waits.clone().unwrap();
        assert!(prepare_tick(&mut store, &t.id));
        assert!(prepare_tick(&mut store, &t.id));
        set_fake_hgs(None);
        assert_eq!(*calls.borrow(), ["compact-context", "inspect", "inspect"], "only looks, never the action again");
        let saved = store.load(&t.id).unwrap().unwrap();
        let wait = saved.task(1).unwrap().waits.as_ref().unwrap();
        assert_eq!((wait.state.as_str(), wait.request_id.as_str()), ("waiting", recorded.request_id.as_str()));
        assert!(saved.task(1).unwrap().attempts.is_empty());
    });
}

#[test]
fn a_restart_resumes_the_context_request_recorded_on_the_task() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let path = dir.path().join("teams/teams.sqlite3");
        let mut store = Store::open_at(&path, true).unwrap();
        let mut t = team();
        with_context(&mut t, "dev", "compact");
        stored(&mut store, &t);
        let started = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let calls = started.clone();
        set_fake_hgs(Some(Box::new(move |_, args: &[&str], input: Option<&Value>| {
            calls.borrow_mut().push(args[0].to_owned());
            match args[0] {
                "compact-context" => Ok(fake_output(0, &json!({"request_id":input.unwrap()["request_id"],
                    "status":"submitted"}).to_string(), "")),
                other => panic!("the start runs the action only: {other}"),
            }
        })));
        hand_out(&mut store, &t.id, 1, "dev", None, "").unwrap();
        set_fake_hgs(None);
        let recorded = store.load(&t.id).unwrap().unwrap().task(1).unwrap().waits.clone().unwrap();
        assert_eq!((recorded.state.as_str(), recorded.request_id.is_empty()), ("waiting", false));
        assert_eq!(*started.borrow(), ["compact-context"]);
        // A restarted worker opens the same store and resumes from the record.
        drop(store);
        let mut store = Store::open_at(&path, true).unwrap();
        let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let resumed = calls.clone();
        let request_id = recorded.request_id.clone();
        set_fake_hgs(Some(Box::new(move |_, args: &[&str], _| {
            resumed.borrow_mut().push(args[0].to_owned());
            match args[0] {
                "inspect" => Ok(fake_output(0, &json!({"run_id":"r1","conversation_id":"c1","activity":"idle","phase":"idle",
                    "compact_context_request":{"request_id":request_id,"run_id":"r1","conversation_id":"c1","status":"completed"}}).to_string(), "")),
                "send" => Ok(fake_output(0, r#"{"status":"submitted"}"#, "")),
                other => panic!("the restart runs no action again: {other}"),
            }
        })));
        assert!(prepare_tick(&mut store, &t.id));
        set_fake_hgs(None);
        assert_eq!(*calls.borrow(), ["inspect", "send"], "no compact again after the restart");
        let saved = store.load(&t.id).unwrap().unwrap();
        assert!(saved.task(1).unwrap().waits.is_none());
        assert_eq!(saved.task(1).unwrap().attempts.last().unwrap().state, AttemptState::Delivered);
    });
}

#[test]
fn an_unconfirmed_context_leaves_the_task_waiting_and_never_retries_by_itself() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let mut t = team();
        with_context(&mut t, "dev", "compact");
        stored(&mut store, &t);
        let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let seen = calls.clone();
        set_fake_hgs(Some(Box::new(move |_, args: &[&str], input: Option<&Value>| {
            seen.borrow_mut().push(args[0].to_owned());
            match args[0] {
                "compact-context" => Ok(fake_output(0, &json!({"request_id":input.unwrap()["request_id"],
                    "status":"submitted"}).to_string(), "")),
                other => panic!("a timed-out wait only looks at the clock: {other}"),
            }
        })));
        hand_out(&mut store, &t.id, 1, "dev", None, "").unwrap();
        // The action is still running; a later tick finds it past the bound.
        store
            .mutate(None, &t.id, None, |team, _| {
                team.task_mut(1).unwrap().waits.as_mut().unwrap().started_at = crate::state::now() - 61.0;
                Ok(Value::Null)
            })
            .unwrap();
        assert!(prepare_tick(&mut store, &t.id));
        set_fake_hgs(None);
        assert_eq!(*calls.borrow(), ["compact-context"], "the bound is checked before any read");
        let saved = store.load(&t.id).unwrap().unwrap();
        let task = saved.task(1).unwrap();
        assert_eq!(task.status, TaskStatus::Pending);
        let attempt = task.attempts.last().unwrap();
        assert_eq!((attempt.prepare[0].result.as_str(), attempt.state), ("uncertain", AttemptState::Refused));
        let wait = task.waits.as_ref().expect("the task waits for Zerus");
        assert_eq!(wait.state, "uncertain");
        assert!(wait.reason.contains("not confirmed within 60 s"), "{wait:?}");
        // It is not ready again, and status names Zerus, not the member.
        assert!(!plan::ready(&saved).contains(&1));
        assert_eq!(super::inbox::waits(&saved, task, false).unwrap()["for"], "zerus");
        let items = super::inbox::items(&saved);
        assert!(items.iter().any(|item| item.kind == "task_waits" && item.task == Some(1)), "{items:?}");
        // A retry (the lead or the owner) starts the preparation again.
        let mut cleared = saved.clone();
        let mut events = Vec::new();
        ops::apply(&mut cleared, "retry", &json!({"task":1}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
        assert!(cleared.task(1).unwrap().waits.is_none());
    });
}

#[test]
fn a_refused_context_leaves_the_task_waiting_with_its_reason() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let mut t = team();
        with_context(&mut t, "dev", "clear");
        stored(&mut store, &t);
        set_fake_hgs(Some(Box::new(|_, args: &[&str], _| match args.first().copied() {
            Some("clear-context") => Ok(fake_output(1, "", "hgs: Wait for the agent to become idle before clearing context")),
            Some("send") => panic!("a refused clear never hands out"),
            other => panic!("unexpected hgs {other:?}"),
        })));
        hand_out(&mut store, &t.id, 1, "dev", None, "").unwrap();
        set_fake_hgs(None);
        let saved = store.load(&t.id).unwrap().unwrap();
        let task = saved.task(1).unwrap();
        let attempt = task.attempts.last().unwrap();
        assert_eq!((attempt.prepare[0].result.as_str(), attempt.state), ("refused", AttemptState::Refused));
        let wait = task.waits.as_ref().expect("the task waits for Zerus");
        assert!(wait.reason.contains("Wait for the agent to become idle"), "{wait:?}");
        assert_eq!(event_kinds(&store, &t.id), ["prepare_failed"]);
    });
}

#[test]
fn an_agent_without_context_actions_gets_none_with_one_feed_note() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let mut t = team();
        // OpenCode has no native clear: the task continues the conversation.
        t.members.iter_mut().find(|member| member.id == "dev").unwrap().agent = "opencode".into();
        with_context(&mut t, "dev", "clear");
        stored(&mut store, &t);
        set_fake_hgs(Some(Box::new(|_, args: &[&str], _| match args.first().copied() {
            Some("send") => Ok(fake_output(0, r#"{"status":"submitted"}"#, "")),
            other => panic!("a skipped context calls no native action: {other:?}"),
        })));
        hand_out(&mut store, &t.id, 1, "dev", None, "").unwrap();
        hand_out(&mut store, &t.id, 2, "dev", None, "").unwrap();
        set_fake_hgs(None);
        let saved = store.load(&t.id).unwrap().unwrap();
        for id in [1, 2] {
            let attempt = saved.task(id).unwrap().attempts.last().unwrap();
            assert_eq!(attempt.prepare[0].result, "skipped", "{attempt:?}");
            assert_eq!(attempt.prepare[0].to, "none");
            assert_eq!(attempt.prepare[0].reason, "OpenCode has no clear; the task continues the conversation");
        }
        assert!(saved.member("dev").unwrap().context_noted);
        let noted = event_kinds(&store, &t.id).into_iter().filter(|kind| kind == "context_skipped").count();
        assert_eq!(noted, 1, "one feed note per member");
    });
}
