//! Fresh task observations, independently of the node's retained stage.
use super::*;
pub(super) fn observe(h: &mut Harness, node: u32, stage: TaskStage, id: &str, conflict: bool) {
    let mut o = h.store.task_observation(TEAM, node).unwrap().unwrap();
    o.previous_revision = o.revision.clone();
    o.revision = uuid::Uuid::new_v4().to_string();
    o.stage = stage;
    o.attempt = o.attempt.max(if conflict { 2 } else { 1 });
    o.attempt_id = id.into();
    o.attempt_conflict = conflict;
    h.run(
        &service(),
        Operation::ObserveTask {
            node,
            observation: o,
        },
    )
    .unwrap();
}
pub(super) fn accepted() -> (Harness, u32, String) {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    let id = h
        .store
        .effects(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .find(|e| e.node == node)
        .unwrap()
        .request_id;
    observe(&mut h, node, TaskStage::InReview, &id, false);
    h.run(&lead, Operation::Accept { node }).unwrap();
    (h, node, id)
}
#[test]
fn retained_accepted_nodes_do_not_finish_against_a_conflicting_newest_observation() {
    for receipt in [false, true] {
        let (mut h, node, id) = accepted();
        let decision = h.accept_decision(node);
        observe(&mut h, node, TaskStage::Done, &id, false);
        observe(&mut h, node, TaskStage::InProgress, "", true);
        if receipt {
            h.run(
                &service(),
                Operation::RecordIntegration {
                    node,
                    decision,
                    commit: COMMIT.into(),
                },
            )
            .unwrap();
        } else {
            h.settings(|s| s.integration = Integration::None);
        }
        assert!(h
            .store
            .queue(TEAM, 200, None)
            .unwrap()
            .rows
            .iter()
            .any(|n| n.id == node));
        let mut a = actor(&h);
        let mut p = Work::default();
        let team = h.store.team(TEAM).unwrap().unwrap();
        a.finish(&mut h.store, &mut p, &team, 100).unwrap();
        assert_eq!(
            p.finishes, 0,
            "retained stage hid a newer conflict, receipt={receipt}"
        );
        // Settling on Done needs no fresh finish command: the task ended.
        observe(&mut h, node, TaskStage::Done, &id, false);
        a.finish(&mut h.store, &mut p, &team, 101).unwrap();
        assert_eq!(p.finishes, 0);
    }
}
/// The person's own ended run replaced the accepted result: the node ends
/// without a finish or merge, and the late original attempt does not revive it.
#[test]
fn an_accepted_review_taken_back_by_the_person_is_superseded_not_resumed() {
    let (mut h, node, _) = accepted();
    h.settings(|s| s.integration = Integration::None);
    observe(&mut h, node, TaskStage::InReview, "", true);
    assert!(h.store.queue(TEAM, 200, None).unwrap().rows.is_empty());
    let mut a = actor(&h);
    let mut p = Work::default();
    run(&mut h, &mut a, &mut p, 100);
    assert_eq!((p.finishes, p.merges), (0, 0));
    let items: Vec<_> = h
        .store
        .inbox(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .filter(|i| i.node == node && i.kind == "accepted_superseded")
        .collect();
    assert_eq!(items.len(), 1);
    assert!(items[0].text.contains("the person's own run replaced the team's accepted result"));
}
#[test]
fn another_review_attempt_or_a_stop_never_finishes_the_earlier_acceptance() {
    let (mut h, node, _) = accepted();
    h.settings(|s| s.integration = Integration::None);
    observe(
        &mut h,
        node,
        TaskStage::InReview,
        "another-persons-review",
        false,
    );
    let mut a = actor(&h);
    let mut p = Work::default();
    run(&mut h, &mut a, &mut p, 100);
    assert_eq!(p.finishes, 0);
    h.run(&person(), Operation::Stop).unwrap();
    run(&mut h, &mut a, &mut p, 101);
    assert_eq!((p.finishes, p.merges), (0, 0));
}
#[test]
fn a_conflict_over_an_accepted_result_ends_the_node_without_a_merge() {
    let (mut h, node, _) = accepted();
    observe(&mut h, node, TaskStage::InReview, "", true);
    assert!(h.store.queue(TEAM, 200, None).unwrap().rows.is_empty());
    let mut a = actor(&h);
    let mut p = Work::default();
    run(&mut h, &mut a, &mut p, 100);
    assert_eq!((p.merges, p.finishes), (0, 0));
    assert!(h.store.integration_fact(TEAM, node).unwrap().is_none());
    assert!(h
        .store
        .decisions(TEAM, 200, None)
        .unwrap()
        .rows
        .iter()
        .filter(|d| d.node == node && d.kind == "accept")
        .all(|d| d.state == crate::teams::store::DecisionState::Settled));
}

#[test]
fn done_remains_a_merge_candidate_but_conflicts_cancelled_or_changed_attempts_do_not() {
    for mode in [
        "done",
        "done conflict",
        "cancelled",
        "another review",
        "running",
        "generation",
    ] {
        let (mut h, node, id) = accepted();
        match mode {
            "done" => observe(&mut h, node, TaskStage::Done, &id, false),
            "done conflict" => observe(&mut h, node, TaskStage::Done, "", true),
            "another review" => observe(&mut h, node, TaskStage::InReview, "another-review", false),
            "running" => observe(&mut h, node, TaskStage::InProgress, &id, false),
            "generation" => observe(&mut h, node, TaskStage::InProgress, "person-current", false),
            "cancelled" => {
                let mut o = h.store.task_observation(TEAM, node).unwrap().unwrap();
                o.previous_revision = o.revision.clone();
                o.revision = uuid::Uuid::new_v4().to_string();
                o.cancelled = true;
                h.run(
                    &service(),
                    Operation::ObserveTask {
                        node,
                        observation: o,
                    },
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        let mut a = actor(&h);
        let mut p = Work::default();
        run(&mut h, &mut a, &mut p, 100);
        run(&mut h, &mut a, &mut p, 101);
        assert_eq!(p.merges, u32::from(mode == "done"), "{mode}");
        assert_eq!(p.finishes, 0, "{mode}");
        assert_eq!(
            h.store.integration_fact(TEAM, node).unwrap().is_some(),
            mode == "done",
            "{mode}"
        );
    }
}
