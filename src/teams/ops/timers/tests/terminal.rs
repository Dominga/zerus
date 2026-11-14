//! Key clears, late sessions and pauses, on the store alone.
use super::*;

fn clear(node: u32, state: KeyClearState, reason: &str) -> Operation {
    Operation::RecordKeyClear {
        node,
        state,
        reason: reason.into(),
    }
}
fn late(member: &str, launch: &str) -> Operation {
    Operation::RecordLateSession {
        member: member.into(),
        launch_id: launch.into(),
        session: SessionKey {
            machine: fixture().team.home,
            session_id: format!("launch_id:{launch}"),
        },
        name: "claude/example/late-member".into(),
    }
}
const LAUNCH: &str = "00000000-0000-4000-8000-00000000a001";

/// A stopped team with tasks in every state a stop can meet.
fn stopped_with_every_state(h: &mut Harness) -> Vec<u32> {
    h.bind("lead");
    h.bind("m1");
    h.bind("m2");
    let waiting = h.enqueue(&person(), 1);
    let pending = h.add(&person(), 2);
    let running = h.add(&person(), 3);
    h.submit(running, "m1");
    let reported = h.add(&person(), 4);
    h.submit(reported, "m2");
    h.report(reported, "m2");
    let left = h.add(&person(), 5);
    let mut o = h.observation(left, TaskStage::Todo);
    o.key = KeyState::Absent;
    h.run(
        &service(),
        Operation::ObserveTask {
            node: left,
            observation: o,
        },
    )
    .unwrap();
    h.run(&person(), Operation::Stop).unwrap();
    assert!(
        h.store.keyed_tasks(TEAM, 0, 200).unwrap().is_empty(),
        "cancels come first"
    );
    settle_cancels(h);
    vec![waiting, pending, running, reported, left]
}
/// Record every pending cancel as the service's cancel step would.
pub(super) fn settle_cancels(h: &mut Harness) {
    for row in h.store.pending_cancels(TEAM, 200, 0).unwrap() {
        let op = Operation::RecordCancel {
            node: row.node,
            decision: row.decision,
            state: CancelState::Cancelled,
            reason: String::new(),
            attempt: 0,
            attempt_id: String::new(),
        };
        h.run(&service(), op).unwrap();
    }
}

#[test]
fn a_stop_leaves_every_keyed_task_for_the_service_and_none_released() {
    let mut h = Harness::new();
    let nodes = stopped_with_every_state(&mut h);
    let keyed: Vec<u32> = h
        .store
        .keyed_tasks(TEAM, 0, 200)
        .unwrap()
        .into_iter()
        .map(|k| k.node)
        .collect();
    assert_eq!(
        keyed,
        nodes[..4].to_vec(),
        "the task that left the team keeps no key of ours"
    );
    assert_eq!(h.store.keyed_teams("", 10).unwrap(), vec![TEAM.to_owned()]);
}

#[test]
fn cleared_and_refused_are_final_and_the_team_leaves_the_pending_set() {
    let mut h = Harness::new();
    let nodes = stopped_with_every_state(&mut h);
    refused(
        h.run(&service(), clear(nodes[0], KeyClearState::Cleared, "why")),
        code::INVALID_STATE,
    );
    refused(
        h.run(&service(), clear(nodes[0], KeyClearState::Refused, " ")),
        code::INVALID_STATE,
    );
    refused(
        h.run(&person(), clear(nodes[0], KeyClearState::Cleared, "")),
        code::NOT_ALLOWED,
    );
    refused(
        h.run(&service(), clear(nodes[4], KeyClearState::Cleared, "")),
        code::INVALID_STATE,
    );
    h.run(&service(), clear(nodes[0], KeyClearState::Cleared, ""))
        .unwrap();
    refused(
        h.run(&service(), clear(nodes[0], KeyClearState::Refused, "Again")),
        code::INVALID_STATE,
    );
    h.run(
        &service(),
        clear(
            nodes[1],
            KeyClearState::Refused,
            "The task moved to another team",
        ),
    )
    .unwrap();
    let refusals = h.store.key_refusals(TEAM, 0, 200).unwrap();
    assert_eq!(
        refusals,
        vec![(nodes[1], "The task moved to another team".to_owned())]
    );
    h.run(&service(), clear(nodes[2], KeyClearState::Cleared, ""))
        .unwrap();
    assert_eq!(h.store.keyed_teams("", 10).unwrap().len(), 1);
    h.run(&service(), clear(nodes[3], KeyClearState::Cleared, ""))
        .unwrap();
    assert!(h.store.keyed_teams("", 10).unwrap().is_empty());
    assert!(h.store.keyed_tasks(TEAM, 0, 200).unwrap().is_empty());
    h.run(&person(), Operation::Archive).unwrap();
    assert!(
        h.store.keyed_teams("", 10).unwrap().is_empty(),
        "nothing is left to clear"
    );
}

#[test]
fn a_running_team_records_no_key_clear() {
    let mut h = Harness::new();
    let node = h.add(&person(), 1);
    refused(
        h.run(&service(), clear(node, KeyClearState::Cleared, "")),
        code::INVALID_STATE,
    );
    assert!(h.store.keyed_teams("", 10).unwrap().is_empty());
}

#[test]
fn a_late_launch_is_provenance_with_one_card_and_never_a_member() {
    let mut h = Harness::new();
    refused(h.run(&service(), late("m1", LAUNCH)), code::INVALID_STATE);
    h.run(&person(), Operation::Stop).unwrap();
    let before = h.store.member(TEAM, "m1").unwrap().unwrap();
    let card = h.run(&service(), late("m1", LAUNCH)).unwrap().item.unwrap();
    refused(h.run(&service(), late("m1", LAUNCH)), code::DUPLICATE);
    refused(
        h.run(
            &person(),
            late("m1", "00000000-0000-4000-8000-00000000a002"),
        ),
        code::NOT_ALLOWED,
    );
    let c = h.store.card(TEAM, card).unwrap().unwrap();
    assert_eq!(
        (c.kind.as_str(), c.state.as_str()),
        ("late_session", "open")
    );
    assert!(c.text.contains("m1") && c.text.contains("claude/example/late-member"));
    assert_eq!(h.store.member(TEAM, "m1").unwrap().unwrap(), before);
    assert_eq!(
        h.store.late_session(TEAM, "m1", LAUNCH).unwrap(),
        Some(card)
    );
    let waiting = h.store.waiting(200, None).unwrap().rows;
    assert!(waiting.iter().any(|w| w.card.seq == card));
}

#[test]
fn a_removed_member_of_a_running_team_records_its_late_launch() {
    let mut h = Harness::new();
    h.run(
        &person(),
        Operation::ArchiveMember {
            member: "m2".into(),
        },
    )
    .unwrap();
    h.run(&service(), late("m2", LAUNCH)).unwrap();
    let mut wrong = late("m2", "00000000-0000-4000-8000-00000000a002");
    if let Operation::RecordLateSession { session, .. } = &mut wrong {
        session.session_id = format!("launch_id:{LAUNCH}");
    }
    refused(h.run(&service(), wrong), code::INVALID_STATE);
    let m = h.store.member(TEAM, "m2").unwrap().unwrap();
    assert!(m.session.is_none() && m.state == MemberState::Removed);
}

#[test]
fn a_pause_before_the_intent_refuses_the_hand_out() {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    h.bind("m1");
    let node = h.add(&person(), 1);
    h.run(&lead, Operation::Pause).unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let op = Operation::HandoutIntent {
        node,
        member: "m1".into(),
        effect_request_id: id,
    };
    refused(h.run(&service(), op), code::INVALID_STATE);
    assert!(h.store.effects(TEAM, 200, None).unwrap().rows.is_empty());
}

#[test]
fn a_paused_member_takes_nothing() {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    h.bind("m1");
    let node = h.add(&person(), 1);
    h.run(
        &lead,
        Operation::PauseMember {
            member: "m1".into(),
        },
    )
    .unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let op = Operation::HandoutIntent {
        node,
        member: "m1".into(),
        effect_request_id: id,
    };
    refused(h.run(&service(), op), code::INVALID_STATE);
}

#[test]
fn the_lead_cannot_resume_a_team_the_person_paused() {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    h.run(&person(), Operation::Pause).unwrap();
    let refusal = h.run(&lead, Operation::Resume).unwrap_err();
    assert_eq!(refusal.code, code::INVALID_STATE);
    assert_eq!(
        refusal.message,
        "Only the person may resume a team they paused."
    );
    assert_eq!(
        h.store.team(TEAM).unwrap().unwrap().status,
        TeamStatus::Paused
    );
    h.run(&person(), Operation::Resume).unwrap();
    h.run(&lead, Operation::Pause).unwrap();
    h.run(&lead, Operation::Resume).unwrap();
    assert_eq!(
        h.store.team(TEAM).unwrap().unwrap().status,
        TeamStatus::Running
    );
}

#[test]
fn a_pause_after_the_delivery_changes_nothing_of_the_hand_out() {
    let mut h = Harness::new();
    h.bind("lead");
    h.bind("m1");
    let node = h.add(&person(), 1);
    let id = h.submit(node, "m1");
    h.run(&person(), Operation::Pause).unwrap();
    let effect = h
        .store
        .effects(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .find(|e| e.request_id == id)
        .unwrap();
    assert_eq!(effect.state, "submitted");
    assert_eq!(h.node(node).implementer.as_deref(), Some("m1"));
    h.report(node, "m1");
    assert_eq!(h.node(node).state, NodeState::Reported);
    assert_eq!(items(&h, "report").len(), 1);
}

#[test]
fn a_late_session_name_is_bounded_by_the_native_contract_and_never_cut() {
    let mut h = Harness::new();
    h.run(&person(), Operation::Stop).unwrap();
    let named = |name: String| {
        let mut op = late("m1", LAUNCH);
        if let Operation::RecordLateSession { name: n, .. } = &mut op {
            *n = name;
        }
        op
    };
    let over = format!("claude/{}", "y".repeat(512 - 6));
    refused(h.run(&service(), named(over)), code::OVER_CAP);
    let whole = format!("claude/{}", "y".repeat(512 - 7));
    assert_eq!(whole.len(), 512);
    let card = h
        .run(&service(), named(whole.clone()))
        .unwrap()
        .item
        .unwrap();
    assert!(h
        .store
        .card(TEAM, card)
        .unwrap()
        .unwrap()
        .text
        .contains(&whole));
}
