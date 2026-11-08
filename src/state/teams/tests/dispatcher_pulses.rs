//! Task 133: a member's turn end writes its pulse, from its own note or from
//! the turn's tool events; no chat text is read or stored.
use super::*;
use super::support::*;

fn events() -> String {
    json!({"events": [
        {"type": "UserPromptSubmit", "detail": "CHAT-MARKER the prompt", "at": "15.0"},
        {"type": "PostToolUse", "tool": "Edit", "detail": "D:/work/dev/src/api.rs", "at": "20.0"},
        {"type": "PostToolUse", "tool": "Bash", "detail": "hgs team run --json", "at": "21.0"},
    ]})
    .to_string()
}

#[test]
fn a_turn_end_writes_the_members_pulse() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let mut t = team();
        delivered(&mut t);
        t.members.iter_mut().find(|member| member.id == "dev").unwrap().directory = "D:/work/dev".into();
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
        let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::<Vec<String>>::new()));
        let record = asked.clone();
        let body = events();
        set_fake_hgs(Some(Box::new(move |_, args: &[&str], _: Option<&Value>| {
            record.borrow_mut().push(args.iter().map(|arg| arg.to_string()).collect());
            Ok(fake_output(0, &body, ""))
        })));
        run_step(&mut store, &t, &Step::TurnEnded { task: 1, at: 50.0, reply: "r1".into() }, "").unwrap();
        // Task 185: the turn end only queues the build; the session read
        // happens later, on the worker, and without the process list.
        let queued = take_queued_pulses();
        assert_eq!(queued.len(), 1);
        assert_eq!(
            (queued[0].member.as_str(), queued[0].task, queued[0].session.as_str(), queued[0].since),
            ("dev", 1, "codex/x/dev", 10.0)
        );
        let reads = |asked: &[Vec<String>]| asked.iter().filter(|args| args.iter().any(|arg| arg == "--after")).cloned().collect::<Vec<_>>();
        assert!(reads(&asked.borrow()).is_empty(), "the turn end must not read the member's session");
        // The worker builds the queued job: the production builder, called here
        // because the fake hgs lives on this thread.
        let fields = crate::state::teams::pulse_worker::build(&queued[0]);
        if let Some(fields) = fields {
            crate::state::teams::pulse_worker::apply(&store, &queued[0], fields);
        }
        let latest = store.latest_pulses(&t.id, crate::state::now());
        assert_eq!(latest.len(), 1);
        let pulse = &latest[0];
        assert_eq!((pulse.member.as_str(), pulse.task, pulse.source.as_str()), ("dev", 1, "auto"));
        assert_eq!(pulse.done, "#1 API; changed src/api.rs");
        assert_eq!(pulse.next, "last: Bash: hgs team run --json");
        assert!(!format!("{pulse:?}").contains("CHAT-MARKER"), "no chat text is stored");
        // Task 163: the turn end also reads the last reply (an inspect without
        // --after); the pulse build reads the turn's events once.
        assert_eq!(reads(&asked.borrow()), [["inspect", "codex/x/dev", "--after", "0", "--skip-processes"]]);
        let after = store.load(&t.id).unwrap().unwrap();
        assert_eq!(after.task(1).unwrap().attempts.last().unwrap().pulse_at, Some(50.0));
        // The member's own note of the next turn stands instead of a summary.
        let note = crate::state::teams::pulses::Pulse {
            team: t.id.clone(), member: "dev".into(), task: 1, done: "wired the API route".into(), next: "tests".into(),
            at: crate::state::now() + 1.0, source: "note".into(), ..Default::default()
        };
        store.put_pulse(&note).unwrap();
        run_step(&mut store, &after, &Step::TurnEnded { task: 1, at: 60.0, reply: "r2".into() }, "").unwrap();
        set_fake_hgs(None);
        assert!(take_queued_pulses().is_empty(), "a noted turn queues no pulse");
        let latest = store.latest_pulses(&t.id, crate::state::now() + 2.0);
        assert_eq!((latest[0].source.as_str(), latest[0].done.as_str()), ("note", "wired the API route"));
        assert_eq!(reads(&asked.borrow()).len(), 1, "a noted turn reads no events");
    });
}

#[test]
fn a_turn_with_nothing_concrete_writes_no_pulse() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let mut t = team();
        delivered(&mut t);
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
        set_fake_hgs(Some(Box::new(|_, _: &[&str], _: Option<&Value>| {
            Ok(fake_output(0, &json!({"events": [{"type": "Stop", "detail": "", "at": "20.0"}]}).to_string(), ""))
        })));
        run_step(&mut store, &t, &Step::TurnEnded { task: 1, at: 50.0, reply: "r1".into() }, "").unwrap();
        // Task 185: the tick queues the build; the worker's summary of a turn
        // with nothing concrete is still none.
        let queued = take_queued_pulses();
        assert_eq!(queued.len(), 1);
        assert_eq!(crate::state::teams::pulse_worker::build(&queued[0]), None);
        set_fake_hgs(None);
        assert!(store.latest_pulses(&t.id, crate::state::now()).is_empty());
    });
}

/// Task 185: a turn end never retires pulses; the tick does, one team a pass.
#[test]
fn a_turn_end_leaves_pulse_expiry_to_the_tick() {
    use crate::state::teams::pulses::KEEP_SECONDS;
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let mut t = team();
        delivered(&mut t);
        let mut closed = Task { id: 4, status: TaskStatus::Accepted, ..Default::default() };
        closed.attempts.push(Attempt { decided_at: Some(2_500.0), ..Default::default() });
        t.tasks.push(closed);
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
        store
            .put_pulse(&crate::state::teams::pulses::Pulse {
                team: t.id.clone(),
                member: "dev".into(),
                task: 4,
                done: "#4 old".into(),
                at: 2_000.0,
                source: "auto".into(),
                ..Default::default()
            })
            .unwrap();
        set_fake_hgs(Some(Box::new(move |_, _: &[&str], _: Option<&Value>| Ok(fake_output(0, &events(), "")))));
        run_step(&mut store, &t, &Step::TurnEnded { task: 1, at: 50.0, reply: "r1".into() }, "").unwrap();
        set_fake_hgs(None);
        // The turn end's own pulse is the previous test's subject; only the
        // expiry of #4's row matters here.
        take_queued_pulses();
        let expires = 2_500.0 + KEEP_SECONDS;
        // A team's rows, not just each member's latest: the turn end wrote a
        // newer pulse for dev, and #4's row still stands at its own time.
        assert!(
            store.pulses(&t.id, None, expires + 1.0, 50).iter().any(|pulse| pulse.task == 4),
            "a turn end never expires a pulse"
        );
        prune_pulses_step(&store, std::slice::from_ref(&t), expires - 1.0, 0);
        assert!(
            store.pulses(&t.id, None, expires - 0.5, 50).iter().any(|pulse| pulse.task == 4),
            "a row expires before its time"
        );
        assert!(store.pulses(&t.id, None, expires + 1.0, 50).iter().all(|pulse| pulse.task != 4), "the row is hidden once expired, before the delete");
        prune_pulses_step(&store, std::slice::from_ref(&t), expires, 0);
        assert!(
            !store.pulses(&t.id, None, expires + 1.0, 50).iter().any(|pulse| pulse.task == 4),
            "the tick retires the expired row"
        );
    });
}
