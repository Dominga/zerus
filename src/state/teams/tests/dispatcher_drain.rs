use super::super::ops::{self, Actor};
use super::*;
use super::support::*;

#[test]
fn a_draining_member_finishes_its_attempt_then_its_session_is_paused_and_it_leaves() {
    // U49: the member owes its open attempt; then its session is paused
    // (never killed) and the member leaves the roster.
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let mut t = team();
        // dev owes only task 1.
        let mut events = Vec::new();
        ops::apply(&mut t, "cancel", &json!({"task":2}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
        let task = t.task_mut(1).unwrap();
        task.attempts.push(Attempt {
            n: 1,
            id: "A1".into(),
            member: "dev".into(),
            state: AttemptState::Delivered,
            sent_at: 10.0,
            delivered_at: Some(10.0),
            ..Default::default()
        });
        task.status = TaskStatus::InProgress;
        t.members.iter_mut().find(|member| member.id == "dev").unwrap().draining = true;
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
        let seen = [session("claude/x/lead", "r0", true), session("codex/x/dev", "r1", true)];
        assert!(
            !steps(&t, &seen, 100.0).iter().any(|step| matches!(step, Step::Drain { .. })),
            "the open attempt is finished first"
        );
        // The report ends the attempt; the task waits at the lead, which is
        // no work of the member any more.
        store
            .mutate(None, &t.id, None, |team, events| {
                ops::apply(team, "report", &json!({"task":1,"attempt":"A1","text":"Done"}), &Actor::Member("dev".into()), &BTreeSet::new(), events)
            })
            .unwrap();
        let after = store.load(&t.id).unwrap().unwrap();
        assert!(steps(&after, &seen, 200.0).iter().any(|step| matches!(step, Step::Drain { member, .. } if member == "dev")));
        // The session is paused, never killed, and the member leaves.
        let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::<Vec<String>>::new()));
        let record = calls.clone();
        set_fake_hgs(Some(Box::new(move |_, args: &[&str], _| {
            record.borrow_mut().push(args.iter().map(|arg| (*arg).to_owned()).collect());
            Ok(fake_output(0, "", ""))
        })));
        run_step(&mut store, &after, &Step::Drain { member: "dev".into(), pause: true }, "").unwrap();
        set_fake_hgs(None);
        assert_eq!(calls.borrow()[0], vec!["pause".to_owned(), "codex/x/dev".to_owned()]);
        assert_eq!(calls.borrow().len(), 1, "one pause, no kill and no terminate");
        let saved = store.load(&t.id).unwrap().unwrap();
        let dev = saved.member("dev").unwrap();
        assert_eq!((dev.state, dev.draining), (MemberState::Removed, false));
        let events = store.events(&t.id, None, 100).unwrap();
        let removed = events["items"].as_array().unwrap().iter().find(|event| event["kind"] == "member_removed").unwrap();
        assert_eq!((removed["detail"]["by"].clone(), removed["detail"]["member"].clone()), (json!("Zerus"), json!("Backend")));
    });
}

#[test]
fn a_draining_member_finishes_its_stage_run_first() {
    // U49: an open stage run it holds keeps the drain waiting; a stage never
    // goes to a draining member.
    let mut t = team();
    let mut events = Vec::new();
    let none = BTreeSet::new();
    // qa owes only its stage run.
    ops::apply(&mut t, "cancel", &json!({"task":3}), &Actor::Human, &none, &mut events).unwrap();
    {
        let task = t.task_mut(1).unwrap();
        task.attempts.push(Attempt { n: 1, id: "A1".into(), member: "dev".into(), state: AttemptState::Reported, ..Default::default() });
        task.status = TaskStatus::InReview;
        task.pipeline_run = Some(PipelineRun {
            id: "code".into(),
            name: "Code".into(),
            stages: vec![PipelineStage { kind: "review".into(), role: "Reviewer".into(), member: "qa".into(), ..Default::default() }],
            max_rounds: 3,
            folder: "hold".into(),
            accept: "lead".into(),
            round: 1,
            hold_from_report: false,
            trimmed_returns: 0,
            runs: vec![StageRun {
                id: "S1".into(),
                stage: 0,
                kind: "review".into(),
                round: 1,
                attempt: 1,
                member: "qa".into(),
                state: StageState::Delivered,
                at: 10.0,
                delivered_at: Some(10.0),
                ..Default::default()
            }],
        });
    }
    t.members.iter_mut().find(|member| member.id == "qa").unwrap().draining = true;
    let seen = [session("claude/x/lead", "r0", true), session("codex/x/dev", "r1", true)];
    assert!(
        !steps(&t, &seen, 100.0).iter().any(|step| matches!(step, Step::Drain { .. })),
        "its stage run is finished first"
    );
    assert!(super::super::pipeline::candidates(&t, 1).is_empty(), "a stage never goes to a draining member");
    // The verdict ends the run and the task waits at the lead.
    let task = t.task_mut(1).unwrap();
    task.pipeline_run.as_mut().unwrap().runs[0].state = StageState::Passed;
    task.status = TaskStatus::Submitted;
    assert!(steps(&t, &seen, 200.0).iter().any(|step| matches!(step, Step::Drain { member, .. } if member == "qa")));
}

#[test]
fn a_drain_with_nothing_to_finish_ends_at_once_and_a_cancelled_one_never_does() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let mut t = team();
        let mut events = Vec::new();
        let none = BTreeSet::new();
        for id in [1, 2] {
            ops::apply(&mut t, "cancel", &json!({"task":id}), &Actor::Human, &none, &mut events).unwrap();
        }
        t.members.iter_mut().find(|member| member.id == "dev").unwrap().draining = true;
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
        let seen = [session("claude/x/lead", "r0", true), session("codex/x/dev", "r1", true)];
        assert!(steps(&t, &seen, 100.0).iter().any(|step| matches!(step, Step::Drain { member, .. } if member == "dev")));
        // Undo before it ends: nothing is paused and nothing is removed.
        store
            .mutate(None, &t.id, None, |team, events| {
                ops::apply(team, "drain", &json!({"member":"dev","draining":false}), &Actor::Human, &BTreeSet::new(), events)
            })
            .unwrap();
        let undone = store.load(&t.id).unwrap().unwrap();
        assert!(!steps(&undone, &seen, 200.0).iter().any(|step| matches!(step, Step::Drain { .. })));
        let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::<Vec<String>>::new()));
        let record = calls.clone();
        set_fake_hgs(Some(Box::new(move |_, args: &[&str], _| {
            record.borrow_mut().push(args.iter().map(|arg| (*arg).to_owned()).collect());
            Ok(fake_output(0, "", ""))
        })));
        run_step(&mut store, &undone, &Step::Drain { member: "dev".into(), pause: true }, "").unwrap();
        set_fake_hgs(None);
        assert!(calls.borrow().is_empty(), "a step decided before the undo does nothing");
        let saved = store.load(&t.id).unwrap().unwrap();
        assert_eq!(saved.member("dev").unwrap().state, MemberState::Active);
    });
}

#[test]
fn a_session_with_no_process_to_pause_is_not_paused() {
    // U49: OpenCode has no separate process to pause; the member still leaves.
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let mut t = team();
        let mut events = Vec::new();
        for id in [1, 2] {
            ops::apply(&mut t, "cancel", &json!({"task":id}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
        }
        {
            let dev = t.members.iter_mut().find(|member| member.id == "dev").unwrap();
            dev.agent = "opencode".into();
            dev.draining = true;
        }
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
        let seen = [session("claude/x/lead", "r0", true), session("codex/x/dev", "r1", true)];
        let found = steps(&t, &seen, 100.0);
        assert!(found.iter().any(|step| matches!(step, Step::Drain { pause: false, .. })), "{found:?}");
        let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::<Vec<String>>::new()));
        let record = calls.clone();
        set_fake_hgs(Some(Box::new(move |_, args: &[&str], _| {
            record.borrow_mut().push(args.iter().map(|arg| (*arg).to_owned()).collect());
            Ok(fake_output(0, "", ""))
        })));
        run_step(&mut store, &t, &Step::Drain { member: "dev".into(), pause: false }, "").unwrap();
        set_fake_hgs(None);
        assert!(calls.borrow().is_empty(), "nothing to pause and nothing to kill");
        assert_eq!(store.load(&t.id).unwrap().unwrap().member("dev").unwrap().state, MemberState::Removed);
    });
}

#[test]
fn a_draining_member_gets_no_new_task() {
    // U49: a draining member takes nothing new, and its ready task is a
    // rebalancing source for a member of the same role.
    let mut t = team();
    t.members.iter_mut().find(|member| member.id == "dev").unwrap().draining = true;
    let seen = [session("claude/x/lead", "r0", true), session("codex/x/dev", "r1", true)];
    let found = steps(&t, &seen, 100.0);
    assert!(!found.iter().any(|step| matches!(step, Step::HandOut { member, .. } if member == "dev")), "{found:?}");
    // It still owes its two ready tasks, so the drain waits while they are
    // assigned to it and no peer can take them.
    assert!(!found.iter().any(|step| matches!(step, Step::Drain { .. })), "{found:?}");
    let mut with_peer = t.clone();
    with_peer.members.push(Member {
        id: "dev2".into(),
        role: "Backend".into(),
        agent: "codex".into(),
        state: MemberState::Active,
        session: Some(SessionRef { name: "codex/x/dev2".into(), run_id: "r2".into(), conversation_id: "c".into(), ..Default::default() }),
        ..Default::default()
    });
    let seen = [
        session("claude/x/lead", "r0", true),
        session("codex/x/dev", "r1", true),
        session("codex/x/dev2", "r2", true),
    ];
    let found = steps(&with_peer, &seen, 100.0);
    assert!(
        found.iter().any(|step| matches!(step, Step::Rebalance { task: 1, from, to } if from == "dev" && to == "dev2")),
        "{found:?}"
    );
    assert!(!found.iter().any(|step| matches!(step, Step::HandOut { task: 1, member } if member == "dev")), "{found:?}");
}
