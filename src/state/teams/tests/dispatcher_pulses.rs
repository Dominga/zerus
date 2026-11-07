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
        let latest = store.latest_pulses(&t.id, crate::state::now());
        assert_eq!(latest.len(), 1);
        let pulse = &latest[0];
        assert_eq!((pulse.member.as_str(), pulse.task, pulse.source.as_str()), ("dev", 1, "auto"));
        assert_eq!(pulse.done, "#1 API; changed src/api.rs");
        assert_eq!(pulse.next, "last: Bash: hgs team run --json");
        assert!(!format!("{pulse:?}").contains("CHAT-MARKER"), "no chat text is stored");
        assert_eq!(asked.borrow()[0], ["inspect", "codex/x/dev", "--after", "0"]);
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
        let latest = store.latest_pulses(&t.id, crate::state::now() + 2.0);
        assert_eq!((latest[0].source.as_str(), latest[0].done.as_str()), ("note", "wired the API route"));
        assert_eq!(asked.borrow().len(), 1, "a noted turn reads no events");
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
        set_fake_hgs(None);
        assert!(store.latest_pulses(&t.id, crate::state::now()).is_empty());
    });
}
