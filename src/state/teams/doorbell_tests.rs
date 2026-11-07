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
    assert!(unread_stalled(&member(Some(twice.clone())), 200.0 + working::RING_AGAIN_SECONDS - 1.0).is_none());
    let why = unread_stalled(&member(Some(twice)), 200.0 + working::RING_AGAIN_SECONDS).unwrap();
    assert!(why.contains("rang twice"), "{why}");
    // An urgent message the member's turn kept from ringing past the bound.
    let held = Unread { urgent: 1, ..fresh };
    let why = unread_stalled(&member(Some(held)), 100.0 + working::URGENT_BOUND_SECONDS).unwrap();
    assert!(why.contains("urgent") && why.contains("stop it natively"), "{why}");
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
        let text = paste(agent, "", &list);
        assert!(!text.contains('\n') && !text.contains("Rebase"), "{agent}: {text}");
    }
    for agent in ["kimi", "dsh", "opencode"] {
        let text = paste(agent, "", &list);
        assert!(text.lines().next().unwrap().contains("hgs team inbox") && text.contains("Rebase on main first"), "{agent}: {text}");
    }
}
