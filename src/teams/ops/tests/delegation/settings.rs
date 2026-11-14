use super::*;

#[test]
fn r1_none_then_branch_cannot_carry_a_delegated_accept_past_revocation() {
    for paused in [false, true] {
        let mut h = Harness::new();
        let (node, lead) = h.reported();
        h.run(&lead, Operation::Accept { node }).unwrap();
        let seq = latest(&h, node);
        if paused {
            h.run(&person(), Operation::Pause).unwrap();
        }
        h.settings(|s| {
            s.integration = Integration::None;
            s.delegations.merge_accepted = false;
        });
        assert_eq!(state(&h, seq), DecisionState::Ready);
        assert!(open(&h).is_empty());
        h.settings(|s| s.integration = Integration::Branch);
        if paused {
            h.run(&person(), Operation::Resume).unwrap();
        }
        assert_eq!(state(&h, seq), DecisionState::AwaitingOwner);
        assert!(h
            .store
            .integration_accepts(TEAM, 200, None)
            .unwrap()
            .rows
            .is_empty());
        let cards = open(&h);
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].node, Some(node));
    }
}

#[test]
fn r2_a_none_off_accept_needs_the_owner_when_branch_integration_is_enabled() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.settings(|s| {
        s.integration = Integration::None;
        s.delegations.merge_accepted = false;
    });
    h.run(&lead, Operation::Accept { node }).unwrap();
    let seq = latest(&h, node);
    assert_eq!(state(&h, seq), DecisionState::Ready);
    h.settings(|s| s.integration = Integration::Branch);
    assert_eq!(state(&h, seq), DecisionState::AwaitingOwner);
    assert_eq!(open(&h).len(), 1);
    assert!(h
        .store
        .integration_accepts(TEAM, 200, None)
        .unwrap()
        .rows
        .is_empty());
}

#[test]
fn an_unrelated_setting_preserves_a_person_approved_accept_and_its_one_card() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.settings(|s| s.delegations.merge_accepted = false);
    let card = h
        .run(&lead, Operation::Accept { node })
        .unwrap()
        .item
        .unwrap();
    let seq = latest(&h, node);
    h.run(
        &person(),
        Operation::DecideCard {
            card,
            decision: "approve".into(),
        },
    )
    .unwrap();
    let before = h.store.card(TEAM, card).unwrap().unwrap();
    h.settings(|s| s.caps.max_open_questions += 1);
    assert_eq!(state(&h, seq), DecisionState::Ready);
    assert_eq!(h.store.card(TEAM, card).unwrap().unwrap(), before);
    assert_eq!(h.store.cards(TEAM, 200, None).unwrap().rows.len(), 1);
    assert!(open(&h).is_empty());
}

#[test]
fn an_unrelated_setting_keeps_a_delegated_accept_ready_while_delegation_is_on() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    let seq = latest(&h, node);
    h.settings(|s| s.caps.max_open_questions += 1);
    assert_eq!(state(&h, seq), DecisionState::Ready);
    assert!(open(&h).is_empty());
}
