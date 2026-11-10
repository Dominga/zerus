// Task 134: team messages by pull. The text lives in the working tier; the
// terminal gets a one-line doorbell under the message's own request id.
use super::*;
use crate::state::teams::working;

type Sends = std::rc::Rc<std::cell::RefCell<Vec<(Vec<String>, Value)>>>;

/// A fake `hgs send` that records each send and answers with `code`/`stderr`.
fn record_sends(code: i32, stderr: &'static str) -> Sends {
    let sends: Sends = Default::default();
    let seen = sends.clone();
    set_fake_hgs(Some(Box::new(move |_, args: &[&str], input| {
        assert_eq!(args.first().copied(), Some("send"), "{args:?}");
        seen.borrow_mut().push((args.iter().map(|arg| arg.to_string()).collect(), input.cloned().unwrap_or(Value::Null)));
        Ok(fake_output(code, if code == 0 { r#"{"status":"submitted"}"# } else { "" }, stderr))
    })));
    sends
}

fn stored_store(root: &std::path::Path, team: Team) -> Store {
    let _ = root;
    let mut store = Store::open_at(&super::super::super::store::path(), true).unwrap();
    store.create(&uuid::Uuid::new_v4().to_string(), &json!({}), team, vec![]).unwrap();
    store
}

#[test]
fn a_hand_out_rings_a_doorbell_and_its_text_lives_in_the_working_tier() {
    let root = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(root.path(), || {
        let team = team();
        let team_id = team.id.clone();
        let mut store = stored_store(root.path(), team);
        let sends = record_sends(0, "");
        hand_out(&mut store, &team_id, 1, "dev", None, "").unwrap();
        set_fake_hgs(None);
        let saved = store.load(&team_id).unwrap().unwrap();
        let attempt = saved.task(1).unwrap().attempts.last().unwrap().clone();
        assert_eq!(attempt.state, AttemptState::Delivered);
        // One send, under the attempt's own request id, one line, no task text.
        let sends = sends.borrow();
        assert_eq!(sends.len(), 1);
        let pasted = sends[0].1["text"].as_str().unwrap();
        assert_eq!(sends[0].1["request_id"], attempt.id.as_str(), "the receipt keeps the hand-out's request identity");
        assert!(!pasted.contains('\n') && pasted.chars().count() <= working::DOORBELL_CHARS, "{pasted}");
        // Task 186: the header's fields, normal, the message's id, the command.
        assert!(pasted.starts_with("[Zerus team \u{b7} from: Lead \u{b7} task: #1 \u{b7} attempt: 1 \u{b7} normal \u{b7} w-message-"), "{pasted}");
        assert!(pasted.ends_with("] read it with hgs team inbox"), "{pasted}");
        assert!(!pasted.contains("Task #1: API"), "the task's text is never pasted to a hook agent: {pasted}");
        // The whole hand-out is stored, rung, unread.
        let unread = working::unread(store.connection(), &team_id, "dev", crate::state::now()).unwrap();
        assert_eq!(unread.len(), 1);
        let message = &unread[0];
        assert_eq!((message.request_id.as_str(), message.kind.as_str(), message.state.as_str(), message.rings), (attempt.id.as_str(), "hand_out", "rung", 1));
        assert!(message.text.contains("Task #1: API") && message.text.contains("hgs team report"), "{}", message.text);
        let member = saved.member("dev").unwrap();
        assert_eq!(member.unread.as_ref().map(|unread| unread.count), Some(1), "status shows unread: 1");
        assert!(serde_json::to_value(member).unwrap()["unread"]["count"] == 1);
    });
}

#[test]
fn a_doorbell_that_never_submits_loses_nothing_and_a_refused_one_withdraws_its_message() {
    let root = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(root.path(), || {
        let team = team();
        let team_id = team.id.clone();
        let mut store = stored_store(root.path(), team);
        // The paste may or may not have arrived (exit 2): the text is stored.
        let _sends = record_sends(2, "uncertain: the input did not submit");
        hand_out(&mut store, &team_id, 1, "dev", None, "").unwrap();
        set_fake_hgs(None);
        let saved = store.load(&team_id).unwrap().unwrap();
        let attempt = saved.task(1).unwrap().attempts.last().unwrap().clone();
        assert_eq!(attempt.state, AttemptState::Unconfirmed);
        let stored = working::by_request(store.connection(), &attempt.id).unwrap().expect("the text is stored though the paste is stuck");
        assert!(stored.text.contains("Task #1: API") && stored.state == "rung");
        // Zerus puts it into the member's next turn (the hook), once, unread.
        let context = working::turn_context_for("codex/x/dev", "r1").expect("the next turn receives it");
        assert!(context.contains("Task #1: API") && context.contains(&stored.id), "{context}");
        assert!(working::turn_context_for("codex/x/dev", "r1").is_none(), "each message goes into one turn");
        let after = working::by_request(store.connection(), &attempt.id).unwrap().unwrap();
        assert_eq!((after.state.as_str(), after.safe_point.as_str()), ("rung", "idle"), "a turn's context is no read receipt");
        assert!(after.injected_at.is_some());
        // Another session is nobody's member: nothing.
        assert!(working::turn_context_for("codex/x/other", "r1").is_none());

        // A refused doorbell (nothing reached the member) withdraws its message:
        // the retry's own message is the only copy.
        let team = team_with_two_backends().0;
        let other = Team { id: "22222222-2222-4222-8222-222222222222".into(), ..team };
        let other_id = other.id.clone();
        store.create(&other_id, &json!({}), other, vec![]).unwrap();
        let _sends = record_sends(1, "session is busy");
        hand_out(&mut store, &other_id, 1, "dev", None, "").unwrap();
        set_fake_hgs(None);
        let saved = store.load(&other_id).unwrap().unwrap();
        assert_eq!(saved.task(1).unwrap().attempts.last().unwrap().state, AttemptState::Refused);
        assert!(working::unread(store.connection(), &other_id, "dev", crate::state::now()).unwrap().is_empty());
        assert!(saved.member("dev").unwrap().unread.is_none());
    });
}

#[test]
fn an_agent_without_a_turn_hook_gets_the_text_after_the_doorbell() {
    let root = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(root.path(), || {
        let mut team = team();
        team.members.iter_mut().find(|member| member.id == "dev").unwrap().agent = "kimi".into();
        let team_id = team.id.clone();
        let mut store = stored_store(root.path(), team);
        let sends = record_sends(0, "");
        hand_out(&mut store, &team_id, 1, "dev", None, "").unwrap();
        set_fake_hgs(None);
        let pasted = sends.borrow()[0].1["text"].as_str().unwrap().to_owned();
        let (line, rest) = pasted.split_once('\n').unwrap();
        assert!(line.contains("hgs team inbox"), "{line}");
        assert!(rest.contains("Task #1: API"), "the hand-out text goes with it: {rest}");
        let stored = working::unread(store.connection(), &team_id, "dev", crate::state::now()).unwrap();
        assert_eq!(stored.len(), 1, "and it is stored all the same");
        assert!(stored[0].injected_at.is_some(), "it went into the turn");
        assert!(working::turn_context_for("codex/x/dev", "r1").is_none());
    });
}

#[test]
fn unread_messages_ring_only_at_an_idle_moment_twice_then_the_lead_hears() {
    let root = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(root.path(), || {
        let mut team = team();
        delivered(&mut team);
        let team_id = team.id.clone();
        let mut store = stored_store(root.path(), team);
        // A message posted while dev works: it never rang.
        let at = crate::state::now();
        store.mutate_working(&team_id, |tx, team, _| {
            let new = working::NewMessage { request_id: "33333333-3333-4333-8333-333333333333", by: "lead", to: "dev", kind: "message",
                urgency: working::Urgency::Urgent, text: "Stop touching src/api.rs" };
            working::post(tx, &team_id, &new, at)?;
            working::refresh(tx, team, "dev", at)
        }).unwrap();
        let team = store.load(&team_id).unwrap().unwrap();
        let busy = vec![session("claude/x/lead", "r0", false), Seen { activity: "busy".into(), ..session("codex/x/dev", "r1", false) }];
        assert!(!steps(&team, &busy, at).iter().any(|step| matches!(step, Step::RingMessages { .. })), "never into a busy composer");
        let idle = vec![session("claude/x/lead", "r0", false), session("codex/x/dev", "r1", true)];
        assert!(steps(&team, &idle, at).contains(&Step::RingMessages { member: "dev".into() }));
        // The ring: one line, URGENT, under a new request id.
        let sends = record_sends(0, "");
        run_step(&mut store, &team, &Step::RingMessages { member: "dev".into() }, "").unwrap();
        set_fake_hgs(None);
        let pasted = sends.borrow()[0].1["text"].as_str().unwrap().to_owned();
        assert!(pasted.contains("URGENT") && pasted.contains("w-message-") && pasted.ends_with("read it with hgs team inbox") && !pasted.contains("src/api.rs"), "{pasted}");
        assert!(pasted.chars().count() <= working::DOORBELL_CHARS, "{pasted}");
        let team = store.load(&team_id).unwrap().unwrap();
        let unread = team.member("dev").unwrap().unread.clone().unwrap();
        assert_eq!((unread.count, unread.urgent, unread.rings), (1, 1, 1));
        // Rung once: not again before the bound, once more after it.
        assert!(!steps(&team, &idle, at + 60.0).iter().any(|step| matches!(step, Step::RingMessages { .. })));
        let later = unread.rung_at.unwrap() + working::URGENT_BOUND_SECONDS + 1.0;
        assert!(steps(&team, &idle, later).contains(&Step::RingMessages { member: "dev".into() }));
        // After two rings and the bound, the lead hears with the member's state.
        let mut twice = team.clone();
        let bound = working::urgent_bound(&twice);
        let found = twice.members.iter_mut().find(|member| member.id == "dev").unwrap();
        found.unread = Some(Unread { rings: 2, rung_at: Some(at), ring_at: None, stall_at: Some(at + bound), ..unread.clone() });
        assert!(!steps(&twice, &idle, at + working::URGENT_BOUND_SECONDS + 1.0).iter().any(|step| matches!(step, Step::RingMessages { .. })), "at most two rings");
        assert!(super::super::super::doorbell::unread_stalled(twice.member("dev").unwrap(), at + working::URGENT_BOUND_SECONDS + 1.0, bound).is_some());
        let found = twice.members.iter_mut().find(|member| member.id == "dev").unwrap();
        found.unread.as_mut().unwrap().oldest_at = at - 2.0 * working::URGENT_BOUND_SECONDS;
        found.unread.as_mut().unwrap().stall_at = Some(at - working::URGENT_BOUND_SECONDS);
        let item = inbox::items(&twice).into_iter().find(|item| item.kind == "unread_messages").expect("an inbox item for the lead");
        assert!(item.summary.contains("does not read") && item.detail.contains("Member state") && item.command.contains("hgs team look"), "{item:?}");
    });
}

/// Task #42 (team ede724c4): 2026-11-10, a lead's message rang dsh2 20 s into
/// the cold-context compaction before task 26; the DeepSeek host took the
/// message as consuming the compaction ("cancelled") and the task went out on
/// the uncompacted context. While Zerus compacts or clears a member's context
/// for a hand-out, its doorbell waits; once the step settles it rings.
#[test]
fn a_doorbell_waits_while_the_members_context_is_compacted_for_a_hand_out() {
    use crate::state::teams::model::MemberCompact;
    let root = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(root.path(), || {
        let mut team = team();
        delivered(&mut team);
        let team_id = team.id.clone();
        let mut store = stored_store(root.path(), team);
        let at = crate::state::now();
        store.mutate_working(&team_id, |tx, team, _| {
            let new = working::NewMessage { request_id: "44444444-4444-4444-8444-444444444444", by: "lead", to: "dev", kind: "message",
                urgency: working::Urgency::Normal, text: "This task is yours now" };
            working::post(tx, &team_id, &new, at)?;
            working::refresh(tx, team, "dev", at)
        }).unwrap();
        let mut team = store.load(&team_id).unwrap().unwrap();
        let idle = vec![session("claude/x/lead", "r0", false), session("codex/x/dev", "r1", true)];
        assert!(steps(&team, &idle, at).contains(&Step::RingMessages { member: "dev".into() }));
        // The compaction before its next task runs: no ring, no context step lost.
        team.members.iter_mut().find(|member| member.id == "dev").unwrap().context_compact =
            Some(MemberCompact { action: "compact".into(), request_id: "55555555-5555-4555-8555-555555555555".into(), started_at: at, ..Default::default() });
        let dev = team.member("dev").unwrap().clone();
        assert!(super::super::context_pending(&team, &dev));
        assert!(!steps(&team, &idle, at).iter().any(|step| matches!(step, Step::RingMessages { .. })), "{:?}", steps(&team, &idle, at));
        // A fresh conversation (no native request) has nothing to cancel.
        team.members.iter_mut().find(|member| member.id == "dev").unwrap().context_compact =
            Some(MemberCompact { action: "fresh".into(), started_at: at, ..Default::default() });
        assert!(!super::super::context_pending(&team, team.member("dev").unwrap()));
        // Settled: it rings.
        team.members.iter_mut().find(|member| member.id == "dev").unwrap().context_compact = None;
        assert!(steps(&team, &idle, at).contains(&Step::RingMessages { member: "dev".into() }));
    });
}

#[test]
fn the_leads_inbox_rings_urgent_for_a_question_and_its_text_is_stored() {
    let root = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(root.path(), || {
        let mut team = team();
        delivered(&mut team);
        team.task_mut(1).unwrap().attempts[0].questions.push(Question { id: "q1".into(), text: "Which port?".into(), at: 11.0, ..Default::default() });
        let team_id = team.id.clone();
        let mut store = stored_store(root.path(), team);
        let team = store.load(&team_id).unwrap().unwrap();
        let sends = record_sends(0, "");
        run_step(&mut store, &team, &Step::Inbox, "").unwrap();
        set_fake_hgs(None);
        let pasted = sends.borrow()[0].1["text"].as_str().unwrap().to_owned();
        assert!(pasted.contains("URGENT") && !pasted.contains("Which port?"), "{pasted}");
        let stored = working::unread(store.connection(), &team_id, "lead", crate::state::now()).unwrap();
        assert_eq!((stored.len(), stored[0].kind.as_str(), stored[0].urgent()), (1, "inbox", true));
        assert!(stored[0].text.contains("Which port?"), "{}", stored[0].text);
    });
}

#[test]
fn a_working_lead_gets_its_inbox_stored_and_never_typed() {
    // Task 186 (HIGH 3, the lead's decision): hard rule 3 holds for the lead
    // too. A Claude lead takes the urgent list at its next tool boundary.
    let root = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(root.path(), || {
        let mut team = team();
        delivered(&mut team);
        team.task_mut(1).unwrap().attempts[0].questions.push(Question { id: "q1".into(), text: "Which port?".into(), at: 11.0, ..Default::default() });
        let team_id = team.id.clone();
        let mut store = stored_store(root.path(), team);
        let team = store.load(&team_id).unwrap().unwrap();
        let working = Seen { activity: "busy".into(), phase: "working".into(), ..session("claude/x/lead", "r0", false) };
        let found = steps(&team, &[working.clone(), session("codex/x/dev", "r1", false)], 20.0);
        assert!(found.contains(&Step::InboxWaits) && !found.contains(&Step::Inbox), "{found:?}");
        let sends = record_sends(0, "");
        run_step(&mut store, &team, &Step::InboxWaits, "").unwrap();
        set_fake_hgs(None);
        assert!(sends.borrow().is_empty(), "nothing is typed into the working lead");
        let stored = working::unread(store.connection(), &team_id, "lead", crate::state::now()).unwrap();
        assert_eq!((stored.len(), stored[0].rings, stored[0].urgent()), (1, 0, true));
        let saved = store.load(&team_id).unwrap().unwrap();
        assert!(saved.inbox.told_at > 0.0, "the items count as told: the list waits stored");
        assert!(saved.member("lead").unwrap().unread.as_ref().unwrap().held_urgent_at.is_some());
        // The tool boundary takes it once, as added context; nothing for a
        // session without an urgent message waiting.
        assert!(working::urgent_waiting("claude/x/lead", "r0") && !working::urgent_waiting("codex/x/dev", "r1"));
        let context = working::tool_context_for("claude/x/lead", "r0").expect("the next tool boundary receives it");
        assert!(context.contains("Which port?") && context.contains("URGENT"), "{context}");
        assert!(working::tool_context_for("claude/x/lead", "r0").is_none() && !working::urgent_waiting("claude/x/lead", "r0"));
        let after = working::unread(store.connection(), &team_id, "lead", crate::state::now()).unwrap();
        assert_eq!((after[0].safe_point.as_str(), after[0].state.as_str()), ("hook", "rung"), "delivered at a safe point, still unread");
        assert!(store.load(&team_id).unwrap().unwrap().member("lead").unwrap().unread.as_ref().unwrap().held_urgent_at.is_none());
        // A Kimi lead hears only when it is free, from a fresh list.
        let mut kimi = team.clone();
        kimi.members.iter_mut().find(|member| member.id == "lead").unwrap().agent = "kimi".into();
        let found = steps(&kimi, &[working, session("codex/x/dev", "r1", false)], 20.0);
        assert!(!found.iter().any(|step| matches!(step, Step::Inbox | Step::InboxWaits)), "{found:?}");
    });
}

#[test]
fn a_message_that_waited_for_the_turn_rings_at_its_end() {
    // Task 186: a Codex lead has no tool-boundary hook: the stored list rings
    // when the turn ends, recorded as the turn_end safe point.
    let root = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(root.path(), || {
        let mut team = team();
        team.members.iter_mut().find(|member| member.id == "lead").unwrap().agent = "codex".into();
        let team_id = team.id.clone();
        let mut store = stored_store(root.path(), team);
        super::super::super::doorbell::post_waiting(&mut store, &team_id, "lead", "44444444-4444-4444-8444-444444444444", "inbox", "zerus",
            working::Urgency::Normal, "[Zerus team \u{b7} from: Zerus \u{b7} inbox]\nA report waits").unwrap();
        let team = store.load(&team_id).unwrap().unwrap();
        assert!(!working::urgent_waiting("claude/x/lead", "r0"), "a normal message waits for no tool boundary");
        let busy = Seen { activity: "busy".into(), phase: "working".into(), ..session("claude/x/lead", "r0", false) };
        let now = crate::state::now();
        assert!(!steps(&team, &[busy], now).iter().any(|step| matches!(step, Step::RingMessages { .. })));
        let free = session("claude/x/lead", "r0", true);
        assert!(steps(&team, &[free], now).contains(&Step::RingMessages { member: "lead".into() }));
        let sends = record_sends(0, "");
        run_step(&mut store, &team, &Step::RingMessages { member: "lead".into() }, "").unwrap();
        set_fake_hgs(None);
        assert_eq!(sends.borrow().len(), 1);
        let rung = working::unread(store.connection(), &team_id, "lead", crate::state::now()).unwrap();
        assert_eq!((rung[0].rings, rung[0].safe_point.as_str()), (1, "turn_end"));
    });
}

#[test]
fn an_urgent_message_the_lead_never_received_is_the_owners_waiting_item() {
    // Task 186 (11.11): past urgent_bound the owner hears, never the lead
    // about itself; the bound is the team's setting.
    let mut team = team();
    team.status = TeamStatus::Running;
    let now = 10_000.0;
    team.members.iter_mut().find(|member| member.id == "lead").unwrap().unread =
        Some(Unread { count: 1, urgent: 1, oldest: "w-message-1".into(), oldest_at: now - 200.0, planned: true,
            ring_at: Some(now - 200.0), held_urgent_at: Some(now - 200.0), ..Default::default() });
    let waiting = |team: &Team| super::super::super::owner_waiting::items(std::slice::from_ref(team), now)
        .into_iter().filter(|item| item["kind"] == "lead_message_undelivered").collect::<Vec<_>>();
    assert!(waiting(&team).is_empty(), "within the default 10 minutes");
    team.settings.urgent_bound_minutes = 3;
    let items = waiting(&team);
    assert_eq!(items.len(), 1);
    assert_eq!((items[0]["action"].as_str(), items[0]["session"].as_str()), (Some("open_terminal"), Some("claude/x/lead")));
    assert!(items[0]["title"].as_str().unwrap().starts_with("URGENT"), "{}", items[0]["title"]);
    assert!(!inbox::items(&team).iter().any(|item| item.kind == "unread_messages"), "the lead is not told about itself");
    let mut bad = team.settings.clone();
    bad.urgent_bound_minutes = 121;
    assert!(ops::apply(&mut team, "settings", &json!({"settings":bad}), &Actor::Human, &BTreeSet::new(), &mut Vec::new()).is_err());
}
