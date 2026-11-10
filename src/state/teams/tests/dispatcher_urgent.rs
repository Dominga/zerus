// Task 277 (owner, 2026-11-08): an urgent message interrupts the member's
// busy turn natively, waits for the stop and opens a new turn that carries the
// message and continues the attempt; an urgent message left unread is an
// alarm, and a hand-out the member took is never counted as unread.
use super::*;
use crate::state::teams::working;
use std::cell::RefCell;
use std::rc::Rc;

fn stored(_root: &std::path::Path, team: Team) -> Store {
    let mut store = Store::open_at(&super::super::super::store::path(), true).unwrap();
    store.create(&uuid::Uuid::new_v4().to_string(), &json!({}), team, vec![]).unwrap();
    store
}

/// Posts a message to `dev` as a store write, as `hgs team message` does.
fn post(store: &mut Store, team_id: &str, request: &str, by: &str, urgency: working::Urgency) {
    let at = crate::state::now();
    store
        .mutate_working(team_id, |tx, team, _| {
            let new = working::NewMessage { request_id: request, by, to: "dev", kind: "message", urgency, text: "Stop: #198 was cancelled" };
            working::post(tx, team_id, &new, at)?;
            working::refresh(tx, team, "dev", at)
        })
        .unwrap();
}

/// A fake hgs that records (args[0], session, input) and answers submitted.
#[allow(clippy::type_complexity)]
fn record(calls: &Rc<RefCell<Vec<(String, String, Value)>>>) {
    let seen = calls.clone();
    set_fake_hgs(Some(Box::new(move |_, args: &[&str], input| {
        seen.borrow_mut().push((args[0].to_owned(), args.get(1).copied().unwrap_or_default().to_owned(), input.cloned().unwrap_or(Value::Null)));
        Ok(fake_output(0, r#"{"status":"submitted"}"#, ""))
    })));
}

fn busy_seen() -> Vec<Seen> {
    vec![
        session("claude/x/lead", "r0", false),
        Seen { activity: "busy".into(), turn_started: Some(100.0), ..session("codex/x/dev", "r1", false) },
    ]
}

fn free_seen() -> Vec<Seen> {
    vec![session("claude/x/lead", "r0", false), session("codex/x/dev", "r1", true)]
}

/// Task 277 (1): every supported agent's busy turn is interrupted natively,
/// and after the stop a new turn carries the message and says to continue.
#[test]
fn an_urgent_message_interrupts_each_agents_turn_and_continues_the_task() {
    for agent in ["claude", "codex", "kimi", "dsh", "opencode"] {
        let root = tempfile::tempdir().unwrap();
        crate::state::with_test_state_root(root.path(), || {
            let mut team = team();
            delivered(&mut team);
            team.members.iter_mut().find(|member| member.id == "dev").unwrap().agent = agent.into();
            let team_id = team.id.clone();
            let mut store = stored(root.path(), team);
            post(&mut store, &team_id, "44444444-4444-4444-8444-444444444444", "lead", working::Urgency::Urgent);
            let at = crate::state::now();
            let team = store.load(&team_id).unwrap().unwrap();
            let step = steps(&team, &busy_seen(), at).into_iter().find_map(|step| match step {
                Step::InterruptMessages { member, turn, run, conversation } if member == "dev" => Some((turn, run, conversation)),
                _ => None,
            });
            assert_eq!(step, Some((100.0, "r1".to_owned(), "c".to_owned())), "{agent}: no interrupt step");
            let calls: Rc<RefCell<Vec<(String, String, Value)>>> = Default::default();
            record(&calls);
            run_step(&mut store, &team, &Step::InterruptMessages { member: "dev".into(), turn: 100.0, run: "r1".into(), conversation: "c".into() }, "").unwrap();
            // The native interrupt is pinned to the displayed turn (each
            // agent's own stop lives in hgs interrupt).
            let interrupt = calls.borrow()[0].clone();
            assert_eq!((interrupt.0.as_str(), interrupt.1.as_str()), ("interrupt", "codex/x/dev"), "{agent}");
            assert_eq!((interrupt.2["expected_run_id"].as_str(), interrupt.2["expected_conversation_id"].as_str()), (Some("r1"), Some("c")), "{agent}");
            assert_eq!(interrupt.2["expected_turn_started"], 100.0, "{agent}");
            set_fake_hgs(None);
            // Recorded, the attempt untouched: it continues.
            let saved = store.load(&team_id).unwrap().unwrap();
            let urgent = saved.member("dev").unwrap().urgent.clone().expect("{agent}: no delivery record");
            assert_eq!(urgent.state, "requested", "{agent}");
            assert_eq!(saved.task(1).unwrap().attempts[0].state, AttemptState::Delivered, "{agent}");
            assert!(!steps(&saved, &busy_seen(), at + 1.0).iter().any(|step| matches!(step, Step::InterruptMessages { .. })), "{agent}: never twice");
            // The stop: the next tick opens the new turn.
            assert!(steps(&saved, &free_seen(), at + 2.0).contains(&Step::ContinueUrgent { member: "dev".into() }), "{agent}");
            let calls: Rc<RefCell<Vec<(String, String, Value)>>> = Default::default();
            record(&calls);
            run_step(&mut store, &saved, &Step::ContinueUrgent { member: "dev".into() }, "").unwrap();
            set_fake_hgs(None);
            let calls = calls.borrow();
            assert_eq!((calls.len(), calls[0].0.as_str(), calls[0].1.as_str()), (1, "send", "codex/x/dev"), "{agent}");
            let text = calls[0].2["text"].as_str().unwrap_or_default();
            assert!(text.contains("Continue your task from where you stopped"), "{agent}: {text}");
            // The text itself: the turn hook carries it for Claude and Codex;
            // an agent without one gets it in the prompt.
            if matches!(agent, "kimi" | "dsh" | "opencode") {
                assert!(text.contains("Stop: #198 was cancelled"), "{agent}: {text}");
            }
            let saved = store.load(&team_id).unwrap().unwrap();
            let urgent = saved.member("dev").unwrap().urgent.clone().unwrap();
            assert_eq!(urgent.state, "sent", "{agent}");
            assert_eq!(saved.task(1).unwrap().attempts[0].state, AttemptState::Delivered, "{agent}: the attempt goes on");
        });
    }
}

/// Task 277 (2): an ordinary message waits for the turn end, as today.
#[test]
fn an_ordinary_message_never_interrupts_a_busy_turn() {
    let root = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(root.path(), || {
        let mut team = team();
        delivered(&mut team);
        let team_id = team.id.clone();
        let mut store = stored(root.path(), team);
        post(&mut store, &team_id, "55555555-5555-4555-8555-555555555555", "lead", working::Urgency::Normal);
        let at = crate::state::now();
        let team = store.load(&team_id).unwrap().unwrap();
        assert!(!steps(&team, &busy_seen(), at).iter().any(|step| matches!(step, Step::InterruptMessages { .. })), "never into a busy turn");
        // At the turn end it rings, as before.
        assert!(steps(&team, &free_seen(), at + 1.0).contains(&Step::RingMessages { member: "dev".into() }));
    });
}

/// Task 277 (3): an urgent message left unread past the alarm is an alarm in
/// the lead's inbox and, for the owner's own message, on the owner's Waiting
/// list; ordinary unread notices stay folded.
#[test]
fn an_urgent_message_unread_past_the_alarm_is_a_lead_and_owner_alarm() {
    let mut team = team();
    let at = crate::state::now();
    let unread = Unread {
        count: 1,
        urgent: 1,
        oldest: "m1".into(),
        oldest_at: at - 600.0,
        urgent_oldest: "m1".into(),
        urgent_at: Some(at - 600.0),
        urgent_by: "owner".into(),
        rings: 1,
        rung_at: Some(at - 600.0),
        lead_told: false,
        ..Default::default()
    };
    let member = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
    member.unread = Some(unread);
    member.urgent = Some(UrgentInterrupt { ids: vec!["m1".into()], oldest: "m1".into(), at: at - 600.0, turn: 100.0, state: "sent".into(), ..Default::default() });
    let items = inbox::items(&team);
    let item = items.iter().find(|item| item.kind == "urgent_unread").expect("the lead's alarm");
    assert!(item.member.contains("dev"), "{item:?}");
    assert!(item.summary.contains("urgent message from you unread 10 min") && item.summary.contains("not acted on"), "{item:?}");
    assert!(item.command.contains("hgs team look"), "{item:?}");
    // The owner's own message is on the owner's Waiting list too.
    let waiting = super::super::super::owner_waiting::items(std::slice::from_ref(&team), at);
    assert!(
        waiting.iter().any(|item| item["kind"] == "urgent_message" && item["member_id"] == "dev" && item["team"] == team.id),
        "{waiting:?}"
    );
    // The lead's message is not the owner's: no Waiting item, still an alarm.
    let mut lead_sent = team.clone();
    let unread = lead_sent.members.iter_mut().find(|member| member.id == "dev").unwrap().unread.as_mut().unwrap();
    unread.urgent_by = "lead".into();
    assert!(inbox::items(&lead_sent).iter().any(|item| item.kind == "urgent_unread"));
    assert!(!super::super::super::owner_waiting::items(std::slice::from_ref(&lead_sent), at).iter().any(|item| item["kind"] == "urgent_message"));
    // Within the bound, no alarm.
    let mut fresh = team.clone();
    let unread = fresh.members.iter_mut().find(|member| member.id == "dev").unwrap().unread.as_mut().unwrap();
    unread.urgent_at = Some(at - 60.0);
    unread.oldest_at = at - 60.0;
    assert!(!inbox::items(&fresh).iter().any(|item| item.kind == "urgent_unread"));
    // 0 turns the alarm off; ordinary unread is never shouted.
    let mut off = team.clone();
    off.settings.urgent_alarm_minutes = 0;
    assert!(!inbox::items(&off).iter().any(|item| item.kind == "urgent_unread"));
    let mut ordinary = team.clone();
    let unread = ordinary.members.iter_mut().find(|member| member.id == "dev").unwrap().unread.as_mut().unwrap();
    unread.urgent = 0;
    unread.urgent_at = None;
    assert!(!inbox::items(&ordinary).iter().any(|item| item.kind == "urgent_unread"));
}

/// Task 277 (4): a hand-out the member took (its text reached the turn) is
/// marked read, so "does not read" counts only real unread.
#[test]
fn a_hand_out_the_member_took_is_marked_read_not_unread() {
    let root = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(root.path(), || {
        let team = team();
        let team_id = team.id.clone();
        let mut store = stored(root.path(), team);
        let calls: Rc<RefCell<Vec<(String, String, Value)>>> = Default::default();
        record(&calls);
        hand_out(&mut store, &team_id, 1, "dev", None, "").unwrap();
        set_fake_hgs(None);
        let saved = store.load(&team_id).unwrap().unwrap();
        let attempt = saved.task(1).unwrap().attempts.last().unwrap().clone();
        assert_eq!(saved.member("dev").unwrap().unread.as_ref().map(|unread| unread.count), Some(1), "stored unread until it reaches a turn");
        // The turn hook puts the hand-out text into the member's turn.
        let context = working::turn_context_for("codex/x/dev", "r1").expect("the hand-out reaches the turn");
        assert!(context.contains("Task #1: API"));
        // A later ordinary message while the member works.
        post(&mut store, &team_id, "66666666-6666-4666-8666-666666666666", "lead", working::Urgency::Normal);
        let saved = store.load(&team_id).unwrap().unwrap();
        super::super::super::doorbell::mark_taken(&mut store, &saved).unwrap();
        let saved = store.load(&team_id).unwrap().unwrap();
        // Only the real unread message is counted; the taken hand-out is read.
        let unread = saved.member("dev").unwrap().unread.clone().unwrap();
        assert_eq!((unread.count, unread.urgent), (1, 0), "the taken hand-out does not count: {unread:?}");
        let taken = working::by_request(store.connection(), &attempt.id).unwrap().expect("the hand-out is stored");
        assert_eq!((taken.state.as_str(), taken.read_via.as_str()), ("read", "taken"));
        // Nothing is marked before the text reaches a turn.
        let mut unturned = team_with_two_backends().0;
        let other_id = "77777777-7777-4777-8777-777777777777".to_owned();
        unturned.id = other_id.clone();
        let mut store2 = stored(root.path(), unturned);
        let calls: Rc<RefCell<Vec<(String, String, Value)>>> = Default::default();
        record(&calls);
        hand_out(&mut store2, &other_id, 1, "dev", None, "").unwrap();
        set_fake_hgs(None);
        let saved = store2.load(&other_id).unwrap().unwrap();
        assert_eq!(saved.member("dev").unwrap().unread.as_ref().map(|unread| unread.count), Some(1));
        super::super::super::doorbell::mark_taken(&mut store2, &saved).unwrap();
        let saved = store2.load(&other_id).unwrap().unwrap();
        assert_eq!(saved.member("dev").unwrap().unread.as_ref().map(|unread| unread.count), Some(1), "not taken until it reaches a turn");
    });
}
