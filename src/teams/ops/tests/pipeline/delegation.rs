use super::*;

#[test]
fn a_pipeline_accept_loses_delegated_merge_authority_but_its_stage_needs_no_merge() {
    let mut h = pipeline_harness("docs");
    h.bind("m1");
    h.bind("m3");
    let parent = h.add(&person(), 1);
    h.submit(parent, "m1");
    h.report(parent, "m1");
    let stage = stage_attempt(&mut h, parent, "m3");
    let reviewer = bound(&h, "m3");
    h.run(&reviewer, verdict(stage, 1, true)).unwrap();
    assert_eq!(h.node(parent).decided_by, Some(Decider::Pipeline));
    h.settings(|s| s.delegations.merge_accepted = false);
    let decisions = h.store.decisions(TEAM, 200, None).unwrap().rows;
    assert_eq!(
        decisions.iter().find(|d| d.node == parent).unwrap().state,
        DecisionState::AwaitingOwner
    );
    assert_eq!(
        decisions.iter().find(|d| d.node == stage).unwrap().state,
        DecisionState::Ready
    );
    let cards = h.store.waiting(200, None).unwrap().rows;
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].card.node, Some(parent));
}
