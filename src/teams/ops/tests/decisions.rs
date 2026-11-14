use super::*;
#[test]
fn full_return_reopen_progress_report_accept_integration_cycle() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(
        &lead,
        Operation::Return {
            node,
            reason: "Fix the edge case".into(),
        },
    )
    .unwrap();
    let id = h.reopen(node, "m1");
    assert!(h.node(node).implementer.is_none());
    assert_eq!(h.store.generation(TEAM, node).unwrap(), Some(2));
    h.reopen_outcome(
        &id,
        HandoutOutcome::Submitted {
            receipt: "Reopen receipt".into(),
        },
    );
    h.observe(node, TaskStage::InProgress);
    h.report(node, "m1");
    h.run(&lead, Operation::Accept { node }).unwrap();
    assert!(!h.node(node).integrated);
    h.run(
        &service(),
        Operation::RecordIntegration {
            decision: h.accept_decision(node),
            node,
            commit: COMMIT.into(),
        },
    )
    .unwrap();
    h.observe(node, TaskStage::Done);
    assert_eq!(h.node(node).state, NodeState::Accepted);
    assert!(h.node(node).integrated);
}
fn competing(node: u32) -> Vec<Operation> {
    vec![
        Operation::Accept { node },
        Operation::Return {
            node,
            reason: "Another decision".into(),
        },
        Operation::Reassign {
            node,
            member: "m1".into(),
            reason: "Another decision".into(),
        },
        Operation::Cancel {
            node,
            reason: "Another decision".into(),
        },
        Operation::Release { node },
    ]
}
/// An acceptance and a cancel block every competing decision and a release.
/// A Return or Reassign that has not started blocks an acceptance and a
/// release; a new Return, Reassign or Cancel replaces it (tests/replace.rs).
#[test]
fn each_pending_decision_blocks_competing_decisions_it_cannot_be_replaced_by() {
    let ops = competing(1);
    for first in [ops[0].clone(), ops[3].clone()] {
        let mut h = Harness::new();
        let (_, lead) = h.reported();
        h.run(&lead, first).unwrap();
        for next in competing(1) {
            refused(h.run(&lead, next), code::INVALID_STATE);
        }
    }
    for first in [ops[1].clone(), ops[2].clone()] {
        for (index, next) in competing(1).into_iter().enumerate() {
            let mut h = Harness::new();
            let (_, lead) = h.reported();
            h.run(&lead, first.clone()).unwrap();
            if (1..=3).contains(&index) {
                h.run(&lead, next).unwrap();
            } else {
                refused(h.run(&lead, next), code::INVALID_STATE);
            }
        }
    }
}
#[test]
fn integration_requires_a_pending_accept_and_service_receipt_and_is_recorded_once() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    let op = Operation::RecordIntegration {
        decision: h.accept_decision(node),
        node,
        commit: COMMIT.into(),
    };
    refused(h.run(&service(), op.clone()), code::INVALID_STATE);
    h.run(&lead, Operation::Accept { node }).unwrap();
    let op = Operation::RecordIntegration {
        node,
        decision: h.accept_decision(node),
        commit: COMMIT.into(),
    };
    h.observe(node, TaskStage::Done);
    assert!(!h.node(node).integrated);
    h.run(&service(), op.clone()).unwrap();
    assert!(h.node(node).integrated);
    refused(h.run(&service(), op), code::INVALID_STATE);
}
#[test]
fn merge_delegation_off_opens_an_owner_card_and_approval_authorizes_service_integration() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.settings(|s| s.delegations.merge_accepted = false);
    let card = h
        .run(&lead, Operation::Accept { node })
        .unwrap()
        .item
        .unwrap();
    let op = Operation::RecordIntegration {
        decision: h.accept_decision(node),
        node,
        commit: COMMIT.into(),
    };
    refused(
        h.run(
            &service(),
            Operation::RecordIntegrationConflict {
                node,
                decision: h.accept_decision(node),
                conflicts: vec!["src/example.rs".into()],
            },
        ),
        code::INVALID_STATE,
    );
    h.run(
        &person(),
        Operation::DecideCard {
            card,
            decision: "approve".into(),
        },
    )
    .unwrap();
    h.run(&service(), op).unwrap();
    h.observe(node, TaskStage::Done);
    assert!(h.node(node).integrated);
}
#[test]
fn owner_rejection_settles_acceptance_without_an_integration_receipt() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.settings(|s| s.delegations.merge_accepted = false);
    let card = h
        .run(&lead, Operation::Accept { node })
        .unwrap()
        .item
        .unwrap();
    h.run(
        &person(),
        Operation::DecideCard {
            card,
            decision: "reject".into(),
        },
    )
    .unwrap();
    assert!(!h.node(node).integrated);
    h.run(
        &lead,
        Operation::Return {
            node,
            reason: "Use another approach".into(),
        },
    )
    .unwrap();
}
#[test]
fn integration_none_acceptance_only_records_receipt_provenance() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.settings(|s| s.integration = Integration::None);
    h.run(&lead, Operation::Accept { node }).unwrap();
    // A service fact is provenance; neither acceptance nor this node needs a merge.
    h.run(
        &service(),
        Operation::RecordIntegration {
            decision: h.accept_decision(node),
            node,
            commit: COMMIT.into(),
        },
    )
    .unwrap();
    assert!(h.store.integration_fact(TEAM, node).unwrap().is_some());
    h.observe(node, TaskStage::Done);
    assert!(!h.node(node).integrated);
}
#[test]
fn stage_acceptance_only_records_receipt_provenance() {
    let mut h = Harness::new();
    let (node, _) = h.reported();
    let mut n = h.node(node);
    n.kind = NodeKind::Stage;
    n.stage_of = Some(2);
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute(
            "UPDATE nodes SET data=?3 WHERE team=?1 AND id=?2",
            params![TEAM, node, rows::node_payload(&n).unwrap()],
        )
        .unwrap();
    h.run(&person(), Operation::Accept { node }).unwrap();
    // A service fact is provenance; neither acceptance nor this node needs a merge.
    h.run(
        &service(),
        Operation::RecordIntegration {
            decision: h.accept_decision(node),
            node,
            commit: COMMIT.into(),
        },
    )
    .unwrap();
    assert!(h.store.integration_fact(TEAM, node).unwrap().is_some());
    h.observe(node, TaskStage::Done);
    assert!(!h.node(node).integrated);
}
#[test]
fn conflict_is_a_service_recorded_return_with_its_findings() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    h.run(
        &service(),
        Operation::RecordIntegrationConflict {
            decision: h.accept_decision(node),
            node,
            conflicts: vec!["src/example.rs".into()],
        },
    )
    .unwrap();
    let decisions = h.store.decisions(TEAM, 200, None).unwrap().rows;
    assert_eq!(decisions[0].state, DecisionState::Settled);
    assert_eq!(decisions[1].kind, "return");
    assert!(decisions[1].text.contains("src/example.rs"));
    h.reopen(node, "m1");
}
#[test]
fn unknown_reopen_is_not_retried_and_explicit_reassignment_opens_a_new_generation() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.bind("m2");
    h.run(
        &lead,
        Operation::Return {
            node,
            reason: "Fix".into(),
        },
    )
    .unwrap();
    let id = h.reopen(node, "m1");
    h.reopen_outcome(
        &id,
        HandoutOutcome::Uncertain {
            reason: "No delivery receipt".into(),
        },
    );
    refused(
        h.run(
            &service(),
            Operation::ReopenIntent {
                node,
                member: "m1".into(),
                effect_request_id: uuid::Uuid::new_v4().to_string(),
            },
        ),
        code::INVALID_STATE,
    );
    h.run(
        &lead,
        Operation::Reassign {
            node,
            member: "m2".into(),
            reason: "Checked; use a new attempt".into(),
        },
    )
    .unwrap();
    h.reopen(node, "m2");
    assert_eq!(h.store.generation(TEAM, node).unwrap(), Some(3));
}
#[test]
fn report_can_arrive_after_reopen_intent_but_before_its_receipt() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(
        &lead,
        Operation::Return {
            node,
            reason: "Fix".into(),
        },
    )
    .unwrap();
    let id = h.reopen(node, "m1");
    h.observe(node, TaskStage::InProgress);
    h.report(node, "m1");
    h.reopen_outcome(
        &id,
        HandoutOutcome::Submitted {
            receipt: "Late receipt".into(),
        },
    );
    assert_eq!(
        h.store
            .effects(TEAM, 200, None)
            .unwrap()
            .rows
            .last()
            .unwrap()
            .state,
        "reported"
    );
    h.run(&lead, Operation::Accept { node }).unwrap();
}
#[test]
fn cancel_is_a_decision_until_the_service_observes_cancellation() {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    let node = h.add(&person(), 1);
    h.run(
        &lead,
        Operation::Cancel {
            node,
            reason: "Superseded".into(),
        },
    )
    .unwrap();
    assert_eq!(h.node(node).state, NodeState::Pending);
    assert!(h.node(node).hold);
    h.cancel_fact(node);
    assert_eq!(h.node(node).state, NodeState::Cancelled);
}

#[test]
fn an_old_report_does_not_settle_a_reassignment_before_its_reopen_intent() {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    h.bind("m1");
    h.bind("m2");
    let node = h.add(&person(), 1);
    h.submit(node, "m1");
    h.run(
        &lead,
        Operation::Reassign {
            node,
            member: "m2".into(),
            reason: "Use a replacement".into(),
        },
    )
    .unwrap();
    h.report(node, "m1");
    assert_eq!(
        h.store
            .decisions(TEAM, 200, None)
            .unwrap()
            .rows
            .last()
            .unwrap()
            .state,
        DecisionState::Pending
    );
    h.reopen(node, "m2");
    assert_eq!(h.store.generation(TEAM, node).unwrap(), Some(2));
}
#[test]
fn a_late_reopen_receipt_cannot_settle_a_later_team_stop() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(
        &lead,
        Operation::Return {
            node,
            reason: "Fix".into(),
        },
    )
    .unwrap();
    let id = h.reopen(node, "m1");
    h.run(&person(), Operation::Stop).unwrap();
    h.reopen_outcome(
        &id,
        HandoutOutcome::Submitted {
            receipt: "Late receipt".into(),
        },
    );
    let last = h
        .store
        .decisions(TEAM, 200, None)
        .unwrap()
        .rows
        .pop()
        .unwrap();
    assert_eq!(last.kind, "cancel");
    assert_eq!(last.state, DecisionState::Pending);
    assert!(h.node(node).hold);
}

#[test]
fn a_ready_accept_can_record_its_late_receipt_after_stop_or_archive() {
    for archive in [false, true] {
        let mut h = Harness::new();
        let (node, lead) = h.reported();
        h.run(&lead, Operation::Accept { node }).unwrap();
        h.run(&person(), Operation::Stop).unwrap();
        if archive {
            h.run(&person(), Operation::Archive).unwrap();
        }
        h.run(
            &service(),
            Operation::RecordIntegration {
                decision: h.accept_decision(node),
                node,
                commit: COMMIT.into(),
            },
        )
        .unwrap();
        assert!(h.node(node).integrated && h.node(node).hold);
        let fact = h.store.integration_fact(TEAM, node).unwrap().unwrap();
        assert_eq!(fact.commit.as_deref(), Some(COMMIT));
        assert_eq!(fact.actor["kind"], "service");
        h.store = Store::open(h.temp.path()).unwrap();
        assert_eq!(h.store.integration_fact(TEAM, node).unwrap(), Some(fact));
        let last = h
            .store
            .decisions(TEAM, 200, None)
            .unwrap()
            .rows
            .pop()
            .unwrap();
        assert_eq!(last.kind, "cancel");
        assert_eq!(last.state, DecisionState::Pending);
        refused(
            h.run(
                &service(),
                Operation::RecordIntegration {
                    decision: h.accept_decision(node),
                    node,
                    commit: COMMIT.into(),
                },
            ),
            code::INVALID_STATE,
        );
    }
}
#[test]
fn a_ready_accept_can_record_its_late_conflict_without_a_new_return_after_stop() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    h.run(&person(), Operation::Stop).unwrap();
    h.cancel_fact(node);
    h.run(&person(), Operation::Archive).unwrap();
    h.run(
        &service(),
        Operation::RecordIntegrationConflict {
            decision: h.accept_decision(node),
            node,
            conflicts: vec!["src/example.rs".into()],
        },
    )
    .unwrap();
    assert_eq!(h.node(node).state, NodeState::Cancelled);
    let fact = h.store.integration_fact(TEAM, node).unwrap().unwrap();
    assert_eq!(fact.conflicts, vec!["src/example.rs"]);
    assert!(fact.commit.is_none());
    assert_eq!(fact.actor["kind"], "service");
    refused(
        h.run(
            &service(),
            Operation::RecordIntegrationConflict {
                decision: h.accept_decision(node),
                node,
                conflicts: vec!["src/other.rs".into()],
            },
        ),
        code::INVALID_STATE,
    );
    let decisions = h.store.decisions(TEAM, 200, None).unwrap().rows;
    assert_eq!(decisions.len(), 2);
    assert_eq!(decisions.last().unwrap().kind, "cancel");
    assert!(h
        .store
        .effects(TEAM, 200, None)
        .unwrap()
        .rows
        .iter()
        .all(|e| e.kind == "handout"));
}
#[test]
fn an_unapproved_accept_never_authorizes_late_integration_after_stop() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.settings(|s| s.delegations.merge_accepted = false);
    h.run(&lead, Operation::Accept { node }).unwrap();
    h.run(&person(), Operation::Stop).unwrap();
    refused(
        h.run(
            &service(),
            Operation::RecordIntegration {
                decision: h.accept_decision(node),
                node,
                commit: COMMIT.into(),
            },
        ),
        code::INVALID_STATE,
    );
    refused(
        h.run(
            &service(),
            Operation::RecordIntegrationConflict {
                decision: h.accept_decision(node),
                node,
                conflicts: vec!["src/example.rs".into()],
            },
        ),
        code::INVALID_STATE,
    );
}

#[test]
fn a_ready_accept_can_record_a_late_receipt_or_conflict_after_cancellation() {
    for conflict in [false, true] {
        let mut h = Harness::new();
        let (node, lead) = h.reported();
        h.run(&lead, Operation::Accept { node }).unwrap();
        h.cancel_fact(node);
        let before = h.node(node);
        record_late_fact(&mut h, node, conflict);
        assert_eq!(h.node(node).state, NodeState::Cancelled);
        assert_eq!(h.node(node).hold, before.hold);
        assert_eq!(h.store.decisions(TEAM, 200, None).unwrap().rows.len(), 1);
        assert_eq!(
            h.store.decisions(TEAM, 200, None).unwrap().rows[0].state,
            DecisionState::Settled
        );
    }
}

#[test]
fn a_ready_accept_can_record_a_late_fact_after_board_restore_without_integrating_the_new_round() {
    for conflict in [false, true] {
        let mut h = Harness::new();
        let (node, lead) = h.reported();
        h.run(&lead, Operation::Accept { node }).unwrap();
        h.observe(node, TaskStage::Done);
        h.observe(node, TaskStage::Todo);
        record_late_fact(&mut h, node, conflict);
        let n = h.node(node);
        assert_eq!(n.state, NodeState::Pending);
        assert!(!n.hold && !n.integrated && n.decided_by.is_none());
        assert_eq!(h.store.generation(TEAM, node).unwrap(), Some(2));
        assert_eq!(
            h.store.decisions(TEAM, 200, None).unwrap().rows[0].state,
            DecisionState::Settled
        );
        h.intent(node, "m1");
    }
}

#[test]
fn cancellation_then_restore_retains_the_old_accepts_late_fact_authority() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    h.cancel_fact(node);
    h.observe(node, TaskStage::Todo);
    record_late_fact(&mut h, node, false);
    assert!(!h.node(node).integrated && !h.node(node).hold);
    assert_eq!(h.store.generation(TEAM, node).unwrap(), Some(2));
}

#[test]
fn cancellation_does_not_authorize_an_unapproved_accepts_late_fact() {
    for conflict in [false, true] {
        let mut h = Harness::new();
        let (node, lead) = h.reported();
        h.settings(|s| s.delegations.merge_accepted = false);
        h.run(&lead, Operation::Accept { node }).unwrap();
        h.cancel_fact(node);
        refused(
            h.run(
                &service(),
                integration_outcome(node, h.accept_decision(node), conflict),
            ),
            code::INVALID_STATE,
        );
        assert!(h.store.integration_fact(TEAM, node).unwrap().is_none());
    }
}

fn integration_outcome(node: u32, decision: u64, conflict: bool) -> Operation {
    if conflict {
        Operation::RecordIntegrationConflict {
            decision,
            node,
            conflicts: vec!["src/example.rs".into()],
        }
    } else {
        Operation::RecordIntegration {
            decision,
            node,
            commit: COMMIT.into(),
        }
    }
}
fn record_late_fact(h: &mut Harness, node: u32, conflict: bool) {
    let op = integration_outcome(node, h.accept_decision(node), conflict);
    h.run(&service(), op.clone()).unwrap();
    let fact = h.store.integration_fact(TEAM, node).unwrap().unwrap();
    assert_eq!(fact.actor["kind"], "service");
    if conflict {
        assert_eq!(fact.conflicts, vec!["src/example.rs"]);
        assert!(fact.commit.is_none());
    } else {
        assert_eq!(fact.commit.as_deref(), Some(COMMIT));
    }
    h.store = Store::open(h.temp.path()).unwrap();
    assert_eq!(h.store.integration_fact(TEAM, node).unwrap(), Some(fact));
    refused(h.run(&service(), op), code::INVALID_STATE);
}
