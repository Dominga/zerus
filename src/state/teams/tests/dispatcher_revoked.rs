// Task 24: a task reassigned (taken over, cancelled) while its member works on
// it reaches that member at once, and the member's next task goes out without
// a message from the lead.
use super::*;

/// Team 33fbafeb, 2026-11-05: task 4 was handed to core and core started it;
/// task 18 (its next task) was given to core too; then the lead reassigned
/// task 4 while core built it (npm ci, tauri build). Here: dev works on #1,
/// #2 waits for dev, and #1 is reassigned to qa.
fn reassigned_while_working(interrupt: bool) -> Team {
    let mut t = team();
    t.id = "33fbafeb-0000-4000-8000-000000000024".into();
    t.settings.interrupt_revoked = interrupt;
    let task = t.task_mut(1).unwrap();
    task.status = TaskStatus::InProgress;
    task.attempts.push(Attempt { n: 1, id: "a1".into(), member: "dev".into(), state: AttemptState::Delivered, ..Default::default() });
    let mut events = Vec::new();
    ops::apply(&mut t, "reassign", &json!({"task":1,"member":"qa"}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut events).unwrap();
    assert_eq!(t.task(1).unwrap().attempts[0].state, AttemptState::Revoked);
    t
}

fn busy_dev() -> Seen {
    let mut busy = session("codex/x/dev", "r1", false);
    busy.idle = false;
    busy
}

#[test]
fn a_task_reassigned_while_its_member_works_tells_it_to_stop_and_its_next_task_goes_out() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let t = reassigned_while_working(false);
        // The member that was working is told; nothing else is.
        assert_eq!(t.stops.len(), 1, "{:?}", t.stops);
        let stop = t.stops[0].clone();
        assert_eq!((stop.task, stop.member.as_str(), stop.attempt, stop.phase, stop.notice), (1, "dev", 1, 0, Notice::Pending));
        assert!(!stop.interrupt);
        let text = stop_text(&t, &stop);
        assert!(text.contains("Stop working on task #1 API: it was reassigned to Tester, so Zerus revoked your attempt 1."), "{text}");
        assert!(text.contains("Stop the builds, tests and commands you started for it") && text.contains("Your next task comes in its own message"), "{text}");
        // While dev is still busy with the old work, the notice goes at once
        // (no interrupt without the owner's setting), and #2 waits for its turn.
        let found = steps(&t, &[busy_dev()], 100.0);
        assert!(found.contains(&Step::TellStop { stop: stop.id.clone() }), "{found:?}");
        assert!(!found.iter().any(|step| matches!(step, Step::InterruptStop { .. } | Step::HandOut { task: 2, .. })), "{found:?}");
        let mut store = Store::open_at(&super::super::super::store::path(), true).unwrap();
        store.create(&uuid::Uuid::new_v4().to_string(), &json!({}), t.clone(), vec![]).unwrap();
        let sent = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let record = sent.clone();
        set_fake_hgs(Some(Box::new(move |_, args: &[&str], input: Option<&Value>| {
            assert_eq!(args.first().copied(), Some("send"));
            record.borrow_mut().push(input.and_then(|input| input["text"].as_str()).unwrap_or_default().to_owned());
            Ok(fake_output(0, r#"{"status":"submitted"}"#, ""))
        })));
        run_step(&mut store, &t, &Step::TellStop { stop: stop.id.clone() }, "").unwrap();
        set_fake_hgs(None);
        assert_eq!(sent.borrow().len(), 1);
        assert!(sent.borrow()[0].contains("Stop working on task #1"));
        let saved = store.load(&t.id).unwrap().unwrap();
        assert_eq!(saved.stops[0].notice, Notice::Sent);
        assert!(event_kinds(&store, &t.id).contains(&"stop_told".to_owned()));
        // The member stops and its turn ends: its next task goes out with no
        // message from the lead, and the notice is not sent again.
        let found = steps(&saved, &[session("codex/x/dev", "r1", true)], 200.0);
        assert!(found.contains(&Step::HandOut { task: 2, member: "dev".into() }), "{found:?}");
        assert!(!found.iter().any(|step| matches!(step, Step::TellStop { .. })), "{found:?}");
    });
}

#[test]
fn the_owner_lets_zerus_interrupt_revoked_work_once() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let t = reassigned_while_working(true);
        let stop = t.stops[0].clone();
        assert!(stop.interrupt && !stop.interrupted);
        // A busy turn is interrupted before the notice; an ended turn is not.
        let found = steps(&t, &[busy_dev()], 100.0);
        let interrupt = found.iter().position(|step| *step == Step::InterruptStop { stop: stop.id.clone() });
        let tell = found.iter().position(|step| *step == Step::TellStop { stop: stop.id.clone() });
        assert!(interrupt.is_some() && interrupt < tell, "{found:?}");
        assert!(!steps(&t, &[session("codex/x/dev", "r1", true)], 100.0).iter().any(|step| matches!(step, Step::InterruptStop { .. })));
        let mut store = Store::open_at(&super::super::super::store::path(), true).unwrap();
        store.create(&uuid::Uuid::new_v4().to_string(), &json!({}), t.clone(), vec![]).unwrap();
        let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let record = calls.clone();
        set_fake_hgs(Some(Box::new(move |_, args: &[&str], _| {
            record.borrow_mut().push(args.iter().take(2).copied().collect::<Vec<_>>().join(" "));
            Ok(fake_output(0, r#"{"status":"submitted"}"#, ""))
        })));
        run_step(&mut store, &t, &Step::InterruptStop { stop: stop.id.clone() }, "").unwrap();
        let saved = store.load(&t.id).unwrap().unwrap();
        // Once: a later tick with the turn still busy only sends the notice.
        assert!(saved.stops[0].interrupted);
        assert!(!steps(&saved, &[busy_dev()], 150.0).iter().any(|step| matches!(step, Step::InterruptStop { .. })));
        run_step(&mut store, &saved, &Step::InterruptStop { stop: stop.id.clone() }, "").unwrap();
        set_fake_hgs(None);
        assert_eq!(*calls.borrow(), ["interrupt codex/x/dev"]);
        assert!(event_kinds(&store, &t.id).contains(&"revoked_interrupted".to_owned()));
        // Only the owner allows it.
        let mut lead_try = t.clone();
        lead_try.settings.interrupt_revoked = false;
        let mut events = Vec::new();
        let refused = ops::apply(
            &mut lead_try,
            "settings",
            &json!({"settings":{"interrupt_revoked":true}}),
            &Actor::Member("lead".into()),
            &BTreeSet::new(),
            &mut events,
        );
        assert!(refused.unwrap_err().to_string().contains("interrupt_revoked"));
    });
}

#[test]
fn only_a_member_still_working_is_told() {
    // A take-over and a cancel tell the working member too; a reported
    // attempt, or a reassign to the same member, tells nobody.
    for op in ["take-over", "cancel"] {
        let mut t = team();
        let task = t.task_mut(1).unwrap();
        task.status = TaskStatus::InProgress;
        task.attempts.push(Attempt { n: 1, id: "a1".into(), member: "dev".into(), state: AttemptState::Delivered, ..Default::default() });
        let mut events = Vec::new();
        ops::apply(&mut t, op, &json!({"task":1}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
        assert_eq!(t.stops.len(), 1, "{op}");
        let reason = &t.stops[0].reason;
        assert!(reason == "the owner took it over" || reason == "it was cancelled", "{op}: {reason}");
    }
    for (state, to) in [(AttemptState::Reported, "qa"), (AttemptState::Delivered, "dev")] {
        let mut t = team();
        let task = t.task_mut(1).unwrap();
        task.status = TaskStatus::InProgress;
        task.attempts.push(Attempt { n: 1, id: "a1".into(), member: "dev".into(), state, ..Default::default() });
        let mut events = Vec::new();
        ops::apply(&mut t, "reassign", &json!({"task":1,"member":to}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
        assert!(t.stops.is_empty(), "{state:?} to {to}");
    }
}
