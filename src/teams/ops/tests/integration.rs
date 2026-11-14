use super::*;
mod owner_window;
const NEW_COMMIT: &str = "1123456789abcdef0123456789abcdef01234567";

#[test]
fn exact_accept_receipts_keep_their_generation_in_both_arrival_orders() {
    for old_first in [true, false] {
        let mut h = Harness::new();
        let (node, old, new) = restored_with_two_accepts(&mut h);
        let old_op = Operation::RecordIntegration {
            node,
            decision: old,
            commit: COMMIT.into(),
        };
        let new_op = Operation::RecordIntegration {
            node,
            decision: new,
            commit: NEW_COMMIT.into(),
        };
        let ordered = if old_first {
            [old_op, new_op]
        } else {
            [new_op, old_op]
        };
        h.run(&service(), ordered[0].clone()).unwrap();
        assert_eq!(h.node(node).integrated, !old_first);
        h.run(&service(), ordered[1].clone()).unwrap();
        assert_current_integration(&h, node, new);
        assert_fact(&h, old, Some(COMMIT), &[]);
        assert_fact(&h, new, Some(NEW_COMMIT), &[]);
        for op in ordered {
            refused(h.run(&service(), op), code::INVALID_STATE);
        }
    }
}

#[test]
fn an_old_accepts_late_conflict_never_returns_the_new_round_in_either_order() {
    for old_first in [true, false] {
        let mut h = Harness::new();
        let (node, old, new) = restored_with_two_accepts(&mut h);
        let old_op = Operation::RecordIntegrationConflict {
            node,
            decision: old,
            conflicts: vec!["src/old.rs".into()],
        };
        let new_op = Operation::RecordIntegration {
            node,
            decision: new,
            commit: NEW_COMMIT.into(),
        };
        let ordered = if old_first {
            [old_op, new_op]
        } else {
            [new_op, old_op]
        };
        h.run(&service(), ordered[0].clone()).unwrap();
        assert_eq!(h.node(node).integrated, !old_first);
        h.run(&service(), ordered[1].clone()).unwrap();
        assert_current_integration(&h, node, new);
        assert_fact(&h, old, None, &["src/old.rs".into()]);
        assert_fact(&h, new, Some(NEW_COMMIT), &[]);
        assert_eq!(h.store.decisions(TEAM, 200, None).unwrap().rows.len(), 2);
        for op in ordered {
            refused(h.run(&service(), op), code::INVALID_STATE);
        }
    }
}

#[test]
fn unrelated_unknown_and_non_accept_decisions_cannot_authorize_a_fact() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    h.bind("m2");
    let other = h.add(&person(), 2);
    h.submit(other, "m2");
    h.report(other, "m2");
    h.run(&lead, Operation::Accept { node: other }).unwrap();
    let other_accept = h.accept_decision(other);
    let cancelled = h.add(&person(), 3);
    h.run(
        &lead,
        Operation::Cancel {
            node: cancelled,
            reason: "Cancel decision".into(),
        },
    )
    .unwrap();
    let not_accept = h
        .store
        .decisions(TEAM, 200, None)
        .unwrap()
        .rows
        .last()
        .unwrap()
        .seq;
    for (target, decision) in [
        (node, other_accept),
        (cancelled, not_accept),
        (node, 9_999_999),
    ] {
        for op in [
            Operation::RecordIntegration {
                node: target,
                decision,
                commit: COMMIT.into(),
            },
            Operation::RecordIntegrationConflict {
                node: target,
                decision,
                conflicts: vec!["src/example.rs".into()],
            },
        ] {
            refused(h.run(&service(), op), code::INVALID_STATE);
        }
    }
    let count: u64 = h
        .store
        .db
        .as_ref()
        .unwrap()
        .query_row("SELECT count(*) FROM integration_facts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

fn restored_with_two_accepts(h: &mut Harness) -> (u32, u64, u64) {
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    let old = h.accept_decision(node);
    h.observe(node, TaskStage::Done);
    h.observe(node, TaskStage::Todo);
    h.submit(node, "m1");
    h.report(node, "m1");
    h.run(&lead, Operation::Accept { node }).unwrap();
    let new = h.accept_decision(node);
    assert_ne!(old, new);
    (node, old, new)
}
fn assert_current_integration(h: &Harness, node: u32, decision: u64) {
    assert!(h.node(node).integrated);
    let commit: String = h
        .store
        .db
        .as_ref()
        .unwrap()
        .query_row(
            "SELECT integration_commit FROM nodes WHERE team=?1 AND id=?2",
            params![TEAM, node],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(commit, NEW_COMMIT);
    let current = h
        .store
        .decisions(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .find(|d| d.seq == decision)
        .unwrap();
    assert_eq!(current.kind, "accept");
    assert_eq!(current.state, DecisionState::Ready);
    assert_eq!(current.generation, 2);
}
fn assert_fact(h: &Harness, decision: u64, commit: Option<&str>, conflicts: &[String]) {
    let fact: (Option<String>, Vec<String>) = h
        .store
        .db
        .as_ref()
        .unwrap()
        .query_row(
            "SELECT commit_id,conflicts FROM integration_facts WHERE decision=?1",
            [decision],
            |r| Ok((r.get(0)?, store::sql_json(r, 1)?)),
        )
        .unwrap();
    assert_eq!(fact.0.as_deref(), commit);
    assert_eq!(fact.1, conflicts);
}

#[test]
fn switching_integration_off_keeps_receipts_and_conflicts_as_provenance_only() {
    for (conflict, done) in [(false, false), (true, false), (false, true), (true, true)] {
        let mut h = Harness::new();
        let (node, lead) = h.reported();
        h.run(&lead, Operation::Accept { node }).unwrap();
        let decision = h.accept_decision(node);
        if done {
            h.observe(node, TaskStage::Done);
        }
        h.settings(|s| s.integration = Integration::None);
        let before = h.node(node);
        let decisions = h.store.decisions(TEAM, 200, None).unwrap().rows;
        let op = if conflict {
            Operation::RecordIntegrationConflict {
                node,
                decision,
                conflicts: vec!["src/example.rs".into()],
            }
        } else {
            Operation::RecordIntegration {
                node,
                decision,
                commit: COMMIT.into(),
            }
        };
        h.run(&service(), op.clone()).unwrap();
        assert_eq!(h.node(node), before);
        assert_eq!(h.store.decisions(TEAM, 200, None).unwrap().rows, decisions);
        let paths = if conflict {
            vec!["src/example.rs".into()]
        } else {
            vec![]
        };
        assert_fact(&h, decision, (!conflict).then_some(COMMIT), &paths);
        refused(h.run(&service(), op), code::INVALID_STATE);
    }
}

#[test]
fn done_after_disabling_integration_preserves_the_late_receipt_or_conflict() {
    for conflict in [false, true] {
        let mut h = Harness::new();
        let (node, lead) = h.reported();
        h.run(&lead, Operation::Accept { node }).unwrap();
        let decision = h.accept_decision(node);
        h.settings(|s| s.integration = Integration::None);
        h.observe(node, TaskStage::Done);
        let before = h.node(node);
        let decisions = h.store.decisions(TEAM, 200, None).unwrap().rows;
        assert_eq!(before.state, NodeState::Accepted);
        assert!(!before.integrated);
        assert_eq!(decisions.last().unwrap().state, DecisionState::Settled);
        let op = if conflict {
            Operation::RecordIntegrationConflict {
                node,
                decision,
                conflicts: vec!["src/example.rs".into()],
            }
        } else {
            Operation::RecordIntegration {
                node,
                decision,
                commit: COMMIT.into(),
            }
        };
        h.run(&service(), op.clone()).unwrap();
        let paths = if conflict {
            vec!["src/example.rs".into()]
        } else {
            vec![]
        };
        assert_fact(&h, decision, (!conflict).then_some(COMMIT), &paths);
        assert_eq!(h.node(node), before);
        assert_eq!(h.store.decisions(TEAM, 200, None).unwrap().rows, decisions);
        refused(h.run(&service(), op), code::INVALID_STATE);
    }
}
