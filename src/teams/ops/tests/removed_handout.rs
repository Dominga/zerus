//! zerus-63rru.77: a member's removal settles its open hand-out of a task
//! that has not started through the existing hand-back (the effect
//! superseded, the node held, one inbox item); a reopen already reserved is
//! handed back with its decision; and a reopen that ends refused tells the
//! lead, whoever its member is.
use super::*;
use crate::teams::store::{EffectIntent, InboxItem};

fn removed(member: &str) -> Operation {
    Operation::ArchiveMember {
        member: member.into(),
    }
}
fn items(h: &Harness, node: u32) -> Vec<InboxItem> {
    let rows = h.store.inbox(TEAM, 200, None).unwrap().rows;
    rows.into_iter().filter(|i| i.node == node).collect()
}
fn effect(h: &Harness, id: &str) -> EffectIntent {
    let rows = h.store.effects(TEAM, 200, None).unwrap().rows;
    rows.into_iter().find(|e| e.request_id == id).unwrap()
}
fn decision(h: &Harness, node: u32) -> crate::teams::store::Decision {
    let rows = h.store.decisions(TEAM, 200, None).unwrap().rows;
    rows.into_iter().rev().find(|d| d.node == node).unwrap()
}
fn handed(member: &str, node: u32) -> String {
    format!(
        "Member {member} was removed before task {node} started; the delivery's result is not known: the prompt may have reached a session that is now gone. The task waits for the lead."
    )
}
/// The team with m1 holding task 1's hand-out in `state`, and m2 free.
fn holding(state: &str) -> (Harness, u32, String) {
    let mut h = Harness::new();
    h.bind("lead");
    h.bind("m1");
    h.bind("m2");
    let node = h.add(&person(), 1);
    let id = h.intent(node, "m1");
    let reason = "fixture reason".to_owned();
    match state {
        "reserved" => {}
        "submitted" => h.outcome(
            &id,
            HandoutOutcome::Submitted {
                receipt: "receipt-example".into(),
            },
        ),
        "uncertain" => h.outcome(&id, HandoutOutcome::Uncertain { reason }),
        "cutting_over" => h.outcome(&id, HandoutOutcome::CuttingOver { reason }),
        _ => unreachable!(),
    }
    assert_eq!(effect(&h, &id).state, state);
    (h, node, id)
}

#[test]
fn removal_settles_an_unstarted_hand_out_in_each_open_state_and_tells_the_lead_once() {
    for state in ["reserved", "submitted", "uncertain", "cutting_over"] {
        let (mut h, node, id) = holding(state);
        let before = items(&h, node);
        // An uncertain end already told the lead once.
        let told = usize::from(matches!(state, "uncertain" | "cutting_over"));
        assert_eq!(before.len(), told, "{state}");
        h.run(&person(), removed("m1")).unwrap();

        let e = effect(&h, &id);
        assert_eq!(e.state, "superseded", "{state}");
        // The outcome stays a fact: an uncertainty is not lost.
        assert_eq!(e.outcome.is_some(), state != "reserved", "{state}");
        let n = h.node(node);
        assert!(n.hold, "{state}: the node is with the lead");
        assert_eq!(n.state, NodeState::Pending, "{state}");
        let after = items(&h, node);
        assert_eq!(after.len(), before.len() + 1, "{state}: one new item");
        for (old, now) in before.iter().zip(&after) {
            assert_eq!((&old.kind, &old.state), (&now.kind, &now.state), "{state}");
        }
        let item = after.last().unwrap();
        assert_eq!(item.kind, format!("member_removed:{id}"), "{state}");
        assert_eq!(item.state, "open", "{state}");
        assert_eq!(item.text, handed("m1", node), "{state}");
        assert!(
            !item.text.contains("uncertain"),
            "{state}: not a second fact"
        );
        assert!(
            h.store.unstarted(TEAM, 0, 200).unwrap().is_empty(),
            "{state}"
        );

        // A repeated removal is refused and writes nothing.
        let again = h.run(&person(), removed("m1")).unwrap_err();
        assert_eq!(again.message, "The member cannot be removed.", "{state}");
        assert_eq!(items(&h, node).len(), after.len(), "{state}");
        // No pass hands the held task out again, to anybody.
        let other = uuid::Uuid::new_v4().to_string();
        let refused = h
            .run(
                &service(),
                Operation::HandoutIntent {
                    node,
                    member: "m2".into(),
                    effect_request_id: other,
                },
            )
            .unwrap_err();
        assert_eq!(
            refused.message, "The plan kernel does not consider this node ready.",
            "{state}"
        );
    }
}

#[test]
fn a_late_outcome_of_the_settled_hand_out_is_a_fact_and_reopens_nothing() {
    for outcome in [
        HandoutOutcome::Submitted {
            receipt: "receipt-example".into(),
        },
        HandoutOutcome::Refused {
            reason: "Effect is closed or belongs to another generation.".into(),
        },
    ] {
        let (mut h, node, id) = holding("reserved");
        h.run(&person(), removed("m1")).unwrap();
        let before = items(&h, node).len();
        h.outcome(&id, outcome.clone());
        let e = effect(&h, &id);
        assert_eq!(e.state, "superseded", "{outcome:?}");
        assert!(e.outcome.is_some(), "{outcome:?}");
        assert!(h.node(node).hold, "{outcome:?}");
        assert_eq!(h.node(node).implementer, None, "{outcome:?}");
        assert_eq!(items(&h, node).len(), before, "{outcome:?}");
    }
}

#[test]
fn removal_leaves_a_paused_members_tasks_and_a_pending_cancel_as_they_were() {
    // A paused member keeps its hand-out and its started task; that wait is
    // intended.
    for started in [false, true] {
        let (mut h, node, id) = holding("submitted");
        if started {
            h.observe(node, TaskStage::InProgress);
        }
        h.run(
            &person(),
            Operation::PauseMember {
                member: "m1".into(),
            },
        )
        .unwrap();
        assert_eq!(effect(&h, &id).state, "submitted", "{started}");
        assert!(!h.node(node).hold, "{started}");
        assert!(items(&h, node).is_empty(), "{started}");
    }

    // A Cancel still ends its task: the removal leaves the node to it.
    let (mut h, node, id) = holding("submitted");
    h.run(
        &person(),
        Operation::Cancel {
            node,
            reason: "Not needed".into(),
        },
    )
    .unwrap();
    h.run(&person(), removed("m1")).unwrap();
    assert_eq!(effect(&h, &id).state, "submitted");
    let d = decision(&h, node);
    assert_eq!(
        (d.kind.as_str(), d.state),
        ("cancel", DecisionState::Pending)
    );
    assert!(items(&h, node).is_empty());
}

#[test]
fn removal_hands_back_a_reopen_already_reserved_with_its_decision() {
    for kind in ["return", "reassign"] {
        let mut h = Harness::new();
        let (node, lead) = h.reported();
        h.bind("m2");
        let target = if kind == "return" { "m1" } else { "m2" };
        let op = if kind == "return" {
            Operation::Return {
                node,
                reason: "Revise the result".into(),
            }
        } else {
            Operation::Reassign {
                node,
                member: "m2".into(),
                reason: "Another member takes it".into(),
            }
        };
        h.run(&lead, op).unwrap();
        let id = h.reopen(node, target);
        let seq = decision(&h, node).seq;
        assert_eq!(decision(&h, node).state, DecisionState::EffectReserved);
        let before = items(&h, node).len();
        h.run(&person(), removed(target)).unwrap();

        assert_eq!(decision(&h, node).state, DecisionState::Settled, "{kind}");
        assert_eq!(decision(&h, node).seq, seq, "{kind}: no new decision");
        assert_eq!(effect(&h, &id).state, "superseded", "{kind}");
        assert!(h.node(node).hold, "{kind}");
        let after = items(&h, node);
        assert_eq!(after.len(), before + 1, "{kind}");
        let item = after.last().unwrap();
        assert_eq!(item.kind, format!("member_removed:{seq}"), "{kind}");
        assert_eq!(
            item.text,
            format!(
                "Member {target} was removed while the {kind} of task {node} was being delivered; the delivery's result is not known: the prompt may have reached a session that is now gone. The decision is settled and the task waits for a new one."
            ),
            "{kind}"
        );
        // Its delivery cannot be admitted any more.
        assert!(!h.store.effect_reserved(TEAM, &id).unwrap(), "{kind}");
    }
}

#[test]
fn a_reopen_that_ends_refused_tells_the_lead_whoever_its_member_is() {
    for (outcome, told) in [
        (
            HandoutOutcome::Refused {
                reason: "Delivery did not start".into(),
            },
            Some("reopen_refused"),
        ),
        (
            HandoutOutcome::Uncertain {
                reason: "fixture unknown".into(),
            },
            Some("delivery_uncertain"),
        ),
        (
            HandoutOutcome::Submitted {
                receipt: "receipt-example".into(),
            },
            None,
        ),
    ] {
        let mut h = Harness::new();
        let (node, lead) = h.reported();
        h.run(
            &lead,
            Operation::Return {
                node,
                reason: "Revise the result".into(),
            },
        )
        .unwrap();
        let id = h.reopen(node, "m1");
        let before: Vec<String> = items(&h, node)
            .into_iter()
            .filter(|i| i.state == "open")
            .map(|i| i.kind)
            .collect();
        h.reopen_outcome(&id, outcome.clone());
        assert_eq!(
            decision(&h, node).state,
            DecisionState::Settled,
            "{outcome:?}"
        );
        let new: Vec<InboxItem> = items(&h, node)
            .into_iter()
            .filter(|i| i.state == "open" && !before.contains(&i.kind))
            .collect();
        match told {
            Some("reopen_refused") => {
                assert!(h.node(node).hold);
                assert_eq!(new.len(), 1, "{new:?}");
                assert_eq!(new[0].kind, format!("reopen_refused:{id}"));
                assert_eq!(
                    new[0].text,
                    format!("The reopen of task {node} for member m1 was not delivered: Delivery did not start. The decision is settled and the task waits for the lead.")
                );
            }
            Some(kind) => {
                assert!(h.node(node).hold);
                assert_eq!(
                    new.iter().map(|i| i.kind.as_str()).collect::<Vec<_>>(),
                    [kind],
                    "one item, not two"
                );
            }
            None => {
                assert!(!h.node(node).hold);
                assert!(new.is_empty(), "{new:?}");
            }
        }
    }
}

fn worktree(h: &Harness, member: &str) -> String {
    let m = h.store.member(TEAM, member).unwrap().unwrap();
    assert!(!m.worktree.is_empty());
    m.worktree
}
fn late(node: u32, generation: u32) -> Operation {
    Operation::RecordLateReport {
        node,
        generation,
        member: "m1".into(),
    }
}

#[test]
fn removal_takes_back_a_task_in_progress_and_says_what_is_not_done() {
    let (mut h, node, id) = holding("submitted");
    h.observe(node, TaskStage::InProgress);
    let path = worktree(&h, "m1");
    h.run(&person(), removed("m1")).unwrap();

    assert_eq!(effect(&h, &id).state, "superseded");
    let n = h.node(node);
    assert!(n.hold);
    assert_eq!(n.state, NodeState::InProgress);
    let all = items(&h, node);
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].kind, format!("member_removed:{id}"));
    assert_eq!(
        all[0].text,
        format!("Member m1 was removed while task {node} was in progress; the task is with the lead again. Its session was not told and may still be working on it, in the worktree at {path}. Reassign gives the task to another member, and a later report from that session will not move it; Cancel stops that session and ends the task.")
    );
    // The lead decides from the held state: a Reassign waits for its reopen,
    // and a Cancel (on another such team) waits for the task's cancellation.
    let lead = lead_actor();
    h.run(
        &lead,
        Operation::Reassign {
            node,
            member: "m2".into(),
            reason: "Another member takes it".into(),
        },
    )
    .unwrap();
    let d = decision(&h, node);
    assert_eq!(
        (d.kind.as_str(), d.state),
        ("reassign", DecisionState::Pending)
    );
    let (mut h, node, _) = holding("submitted");
    h.observe(node, TaskStage::InProgress);
    h.run(&person(), removed("m1")).unwrap();
    let lead = lead_actor();
    h.run(
        &lead,
        Operation::Cancel {
            node,
            reason: "Not needed".into(),
        },
    )
    .unwrap();
    let d = decision(&h, node);
    assert_eq!(
        (d.kind.as_str(), d.state),
        ("cancel", DecisionState::Pending)
    );
}

#[test]
fn a_report_written_before_the_removal_stays_the_teams() {
    // Recorded before the removal: untouched, and the lead can accept it.
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.bind("m2");
    h.run(&person(), removed("m1")).unwrap();
    assert!(!h.node(node).hold);
    assert!(items(&h, node)
        .iter()
        .all(|i| !i.kind.starts_with("member_removed:")));
    h.run(&lead, Operation::Accept { node }).unwrap();
    assert_eq!(decision(&h, node).kind, "accept");

    // Written at the task level (In review) but not yet recorded by the
    // team when the removal lands: the removal leaves it, and the team
    // records it as today.
    let (mut h, node, id) = holding("submitted");
    h.observe(node, TaskStage::InReview);
    h.run(&person(), removed("m1")).unwrap();
    assert_eq!(effect(&h, &id).state, "submitted");
    assert!(!h.node(node).hold);
    let generation = h.store.generation(TEAM, node).unwrap().unwrap();
    // It is no late report: neither the actor's read nor the store's own
    // operation takes it for one.
    assert_eq!(
        h.store.removed_reporter(TEAM, node, generation).unwrap(),
        None
    );
    let refused = h.run(&service(), late(node, generation)).unwrap_err();
    assert_eq!(
        refused.message,
        "No report of a removed member waits here after its removal."
    );
    h.run(
        &service(),
        Operation::RecordReport {
            node,
            generation,
            member: "m1".into(),
            text: "Implemented and checked".into(),
            commit: COMMIT.into(),
            notes: Vec::new(),
        },
    )
    .unwrap();
    assert!(h.store.report(TEAM, node, generation).unwrap().is_some());
}

#[test]
fn a_report_written_after_the_removal_is_told_once_and_never_recorded() {
    let (mut h, node, _) = holding("submitted");
    h.observe(node, TaskStage::InProgress);
    let path = worktree(&h, "m1");
    // Not yet: the task is still in progress.
    h.run(&person(), removed("m1")).unwrap();
    let generation = h.store.generation(TEAM, node).unwrap().unwrap();
    assert_eq!(
        h.store.removed_reporter(TEAM, node, generation).unwrap(),
        Some("m1".into())
    );
    let early = h.run(&service(), late(node, generation)).unwrap_err();
    assert_eq!(
        early.message,
        "No report of a removed member waits here after its removal."
    );
    // The old session reports: the task is In review at the task level.
    h.observe(node, TaskStage::InReview);
    let refused = h
        .run(
            &service(),
            Operation::RecordReport {
                node,
                generation,
                member: "m1".into(),
                text: "Implemented and checked".into(),
                commit: COMMIT.into(),
                notes: Vec::new(),
            },
        )
        .unwrap_err();
    assert_eq!(
        refused.message,
        "Report does not belong to the current reviewed attempt's holder."
    );
    assert!(!h.store.late_report_told(TEAM, node, generation).unwrap());
    h.run(&service(), late(node, generation)).unwrap();
    assert!(h.store.late_report_told(TEAM, node, generation).unwrap());
    let told: Vec<InboxItem> = items(&h, node)
        .into_iter()
        .filter(|i| i.kind == "removed_report")
        .collect();
    assert_eq!(told.len(), 1);
    assert_eq!(
        told[0].text,
        format!("Member m1 reported task {node} after it was removed; the team does not record that report. Its work is in the worktree at {path}.")
    );
    let again = h.run(&service(), late(node, generation)).unwrap_err();
    assert_eq!(again.message, "This late report was already told.");
    assert!(h.store.report(TEAM, node, generation).unwrap().is_none());
    assert!(h.node(node).hold);
}

#[test]
fn a_late_report_is_only_for_a_removed_members_settled_hand_out() {
    // The member is not removed, or another member's hand-out is newest.
    let (mut h, node, _) = holding("submitted");
    h.observe(node, TaskStage::InProgress);
    h.observe(node, TaskStage::InReview);
    let generation = h.store.generation(TEAM, node).unwrap().unwrap();
    assert_eq!(
        h.store.removed_reporter(TEAM, node, generation).unwrap(),
        None
    );
    let refused = h.run(&service(), late(node, generation)).unwrap_err();
    assert_eq!(
        refused.message,
        "No report of a removed member waits here after its removal."
    );
    // A removed member's other late report: not this generation.
    h.run(&person(), removed("m1")).unwrap();
    let refused = h.run(&service(), late(node, generation + 1)).unwrap_err();
    assert_eq!(
        refused.message,
        "No report of a removed member waits here after its removal."
    );
}

/// A Reassign aimed at the member that holds the task's open hand-out, then
/// the member's removal: the decision's hand-back is the one item, and the
/// hand-out is settled all the same, so the start timer never speaks of it.
#[test]
fn a_node_handed_back_for_its_decision_has_its_hand_out_settled_too() {
    let (mut h, node, id) = holding("submitted");
    h.run(
        &person(),
        Operation::Reassign {
            node,
            member: "m1".into(),
            reason: "The same member keeps it".into(),
        },
    )
    .unwrap();
    let seq = decision(&h, node).seq;
    assert_eq!(decision(&h, node).state, DecisionState::Pending);
    h.run(&person(), removed("m1")).unwrap();
    assert_eq!(decision(&h, node).state, DecisionState::Settled);
    assert_eq!(effect(&h, &id).state, "superseded");
    assert!(h.node(node).hold);
    let kinds: Vec<String> = items(&h, node).into_iter().map(|i| i.kind).collect();
    assert_eq!(kinds, [format!("member_removed:{seq}")]);
    assert!(h.store.unstarted(TEAM, 0, 200).unwrap().is_empty());
}

/// After a handover to m2, m1's hand-out of the earlier generation was
/// superseded by the reopen, not by a removal; m1 is removed later. A late
/// report is only ever the current generation's.
#[test]
fn a_late_report_is_never_an_earlier_generations() {
    let (mut h, node, _) = holding("submitted");
    h.observe(node, TaskStage::InProgress);
    let earlier = h.store.generation(TEAM, node).unwrap().unwrap();
    h.run(
        &person(),
        Operation::Reassign {
            node,
            member: "m2".into(),
            reason: "Another member takes it".into(),
        },
    )
    .unwrap();
    let id = h.reopen(node, "m2");
    h.reopen_outcome(
        &id,
        HandoutOutcome::Submitted {
            receipt: "receipt-example".into(),
        },
    );
    let current = h.store.generation(TEAM, node).unwrap().unwrap();
    assert!(current > earlier);
    h.observe(node, TaskStage::InReview);
    h.run(&person(), removed("m1")).unwrap();
    assert_eq!(h.node(node).state, NodeState::Reported);
    assert_eq!(h.store.removed_reporter(TEAM, node, current).unwrap(), None);
    let refused = h.run(&service(), late(node, earlier)).unwrap_err();
    assert_eq!(
        refused.message,
        "No report of a removed member waits here after its removal."
    );
    assert!(items(&h, node).iter().all(|i| i.kind != "removed_report"));
}

/// The lead's own session actor, as `Harness::bind` made it when `holding`
/// bound the lead: the item's reader, who makes the decisions it names.
fn lead_actor() -> Actor {
    let m = fixture()
        .members
        .into_iter()
        .find(|m| m.id == "lead")
        .unwrap();
    Actor::Session {
        person: "local".into(),
        team: TEAM.into(),
        member: "lead".into(),
        session: m.session.unwrap(),
    }
}
