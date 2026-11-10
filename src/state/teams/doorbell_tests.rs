// Task 134: when the doorbell rings and when the lead hears.
use super::*;
use crate::state::teams::model::Unread;

fn member(unread: Option<Unread>) -> Member {
    Member { id: "dev".into(), agent: "codex".into(), unread, ..Default::default() }
}

#[test]
fn a_message_rings_first_then_once_more_past_its_bound_and_the_lead_hears_after() {
    let fresh = Unread { count: 1, oldest: "w-message-1".into(), oldest_at: 100.0, ..Default::default() };
    assert!(!due(&member(None), 100.0));
    assert!(due(&member(Some(fresh.clone())), 100.0), "a message that never rang rings at the next idle moment");
    let once = Unread { rings: 1, rung_at: Some(100.0), ..fresh.clone() };
    assert!(!due(&member(Some(once.clone())), 100.0 + working::RING_AGAIN_SECONDS - 1.0));
    assert!(due(&member(Some(once.clone())), 100.0 + working::RING_AGAIN_SECONDS));
    let urgent = Unread { urgent: 1, ..once.clone() };
    assert!(due(&member(Some(urgent)), 100.0 + working::URGENT_BOUND_SECONDS), "an urgent one rings again sooner");
    let twice = Unread { rings: 2, rung_at: Some(200.0), ..once };
    assert!(!due(&member(Some(twice.clone())), 1e9), "at most two rings");
    let bound = working::URGENT_BOUND_SECONDS;
    assert!(unread_stalled(&member(Some(twice.clone())), 200.0 + working::RING_AGAIN_SECONDS - 1.0, bound).is_none());
    let why = unread_stalled(&member(Some(twice)), 200.0 + working::RING_AGAIN_SECONDS, bound).unwrap();
    assert!(why.contains("rang twice"), "{why}");
    // An urgent message the member's turn kept from ringing past the bound.
    let held = Unread { urgent: 1, ..fresh };
    let why = unread_stalled(&member(Some(held)), 100.0 + working::URGENT_BOUND_SECONDS, bound).unwrap();
    assert!(why.contains("urgent") && why.contains("stop it natively"), "{why}");
}

#[test]
fn a_planned_summary_rings_and_stalls_by_each_messages_own_times() {
    // Task 186: the summary's schedule, not the oldest message's rings.
    let planned = Unread { count: 2, oldest: "w-message-1".into(), oldest_at: 50.0, rings: 2, rung_at: Some(60.0), planned: true,
        ring_at: Some(500.0), ..Default::default() };
    assert!(!due(&member(Some(planned.clone())), 499.0));
    assert!(due(&member(Some(planned.clone())), 500.0), "a newer message rings though the oldest rang twice");
    assert!(!due(&member(Some(Unread { ring_at: None, rings: 0, ..planned.clone() })), 1e9), "nothing to ring");
    let stalled = Unread { ring_at: None, stall_at: Some(900.0), ..planned.clone() };
    assert!(unread_stalled(&member(Some(stalled.clone())), 899.0, 600.0).is_none());
    assert!(unread_stalled(&member(Some(stalled)), 900.0, 600.0).unwrap().contains("rang twice"));
    // An urgent one that reached no safe point: past the team's urgent_bound.
    let held = Unread { ring_at: None, held_urgent_at: Some(100.0), ..planned };
    assert!(unread_stalled(&member(Some(held.clone())), 279.0, 180.0).is_none());
    let why = unread_stalled(&member(Some(held)), 280.0, 180.0).unwrap();
    assert!(why.contains("undelivered") && why.contains("3 min"), "{why}");
}

#[test]
fn a_hook_agent_gets_the_line_and_any_other_the_text_after_it() {
    let message = Message {
        id: "w-message-1".into(), request_id: "r".into(), team: "t".into(), by: "lead".into(), to: "dev".into(), kind: "message".into(),
        urgency: "normal".into(), text: "Rebase on main first".into(), at: 1.0, state: "queued".into(), rung_at: None, rings: 0,
        safe_point: String::new(), injected_at: None, read_at: None, read_via: String::new(), read_request: String::new(), expires: 9.0,
    };
    let list = vec![message];
    for agent in ["claude", "codex"] {
        let text = paste(agent, "", &list, false);
        assert!(!text.contains('\n') && !text.contains("Rebase"), "{agent}: {text}");
    }
    for agent in ["kimi", "dsh", "opencode"] {
        let text = paste(agent, "", &list, false);
        assert!(text.lines().next().unwrap().contains("hgs team inbox") && text.contains("Rebase on main first"), "{agent}: {text}");
    }
}

/// Task #24 (team ede724c4): a post rings for its own message and the unread
/// ones whose schedule is due, never every unread one; only those can make it
/// URGENT. A lead's normal inbox list said URGENT every 2 minutes while one
/// urgent question waited unread, and each post counted a ring on every
/// unread message (11 rings in an hour).
#[test]
fn a_post_rings_for_its_message_and_the_due_ones_only() {
    let message = |id: &str, at: f64, urgency: &str, rings: u32, rung_at: Option<f64>| Message {
        id: id.into(), request_id: format!("r-{id}"), team: "t".into(), by: "zerus".into(), to: "lead".into(), kind: "inbox".into(),
        urgency: urgency.into(), text: "list".into(), at, state: "rung".into(), rung_at, rings,
        safe_point: String::new(), injected_at: rung_at, read_at: None, read_via: String::new(), read_request: String::new(), expires: 1e9,
    };
    // An urgent question rang once at 100; a normal list rang at 200; a new
    // normal list posts at 320.
    let list = vec![message("urgent", 100.0, "urgent", 1, Some(100.0)), message("older", 200.0, "normal", 1, Some(200.0)), message("new", 320.0, "normal", 0, None)];
    let rung = ringing(&list, "r-new", 600.0, 320.0);
    assert_eq!(rung.iter().map(|message| message.id.as_str()).collect::<Vec<_>>(), vec!["new"], "only the new message rings");
    assert!(!rung.iter().any(Message::urgent), "a normal post's doorbell is normal");
    assert!(paste("claude", "", &list, rung.iter().any(Message::urgent)).contains("\u{b7} normal \u{b7} new \u{b7} 3 messages]"));
    // Past the urgent one's bound it is due again: then it rings, URGENT.
    let later = ringing(&list, "r-new", 600.0, 701.0);
    assert!(later.iter().any(|message| message.id == "urgent") && later.iter().any(Message::urgent), "{later:?}");
}
