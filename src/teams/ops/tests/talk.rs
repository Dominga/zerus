use super::*;
#[test]
fn questions_are_bound_to_the_effect_holder_and_capped_per_attempt() {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    let m1 = h.bind("m1");
    let m2 = h.bind("m2");
    let node = h.add(&person(), 1);
    h.submit(node, "m1");
    refused(h.run(&m2, ask(node, "Other attempt")), code::NOT_ALLOWED);
    let mut first = 0;
    for n in 0..3 {
        let item = h
            .run(&m1, ask(node, format!("Question {n}")))
            .unwrap()
            .item
            .unwrap();
        if n == 0 {
            first = item;
        }
    }
    refused(h.run(&m1, ask(node, "Overflow")), code::OVER_CAP);
    h.run(
        &lead,
        Operation::Reply {
            item: first,
            text: "Use the documented behavior".into(),
        },
    )
    .unwrap();
    h.run(&m1, ask(node, "Replacement")).unwrap();
    refused(
        h.run(
            &lead,
            Operation::Reply {
                item: first,
                text: "Again".into(),
            },
        ),
        code::INVALID_STATE,
    );
}
#[test]
fn delegation_revocation_blocks_new_accept_and_reply_decisions() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.settings(|s| {
        s.delegations.accept_results = false;
        s.delegations.answer_members = false;
    });
    refused(h.run(&lead, Operation::Accept { node }), code::NOT_ALLOWED);
    refused(
        h.run(
            &lead,
            Operation::Reply {
                item: 1,
                text: "Answer".into(),
            },
        ),
        code::NOT_ALLOWED,
    );
}
#[test]
fn owner_card_records_proposer_and_person_decision_and_refuses_a_second_decision() {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    let card = h
        .run(
            &lead,
            Operation::OpenCard {
                kind: "proposal".into(),
                text: "Add a member?".into(),
                node: None,
                decisions: None,
                parameters: None,
            },
        )
        .unwrap()
        .item
        .unwrap();
    h.run(
        &person(),
        Operation::DecideCard {
            card,
            decision: "acknowledged".into(),
        },
    )
    .unwrap();
    refused(
        h.run(
            &person(),
            Operation::DecideCard {
                card,
                decision: "Again".into(),
            },
        ),
        code::INVALID_STATE,
    );
    let c = h.store.cards(TEAM, 200, None).unwrap().rows.pop().unwrap();
    assert_eq!(c.actor["person"], "local");
    assert_eq!(c.decided_actor.unwrap()["person"], "local");
}

fn ask(node: u32, body: impl Into<String>) -> Operation {
    Operation::Ask {
        node,
        generation: 1,
        text: body.into(),
    }
}

#[test]
fn settling_a_node_closes_its_open_integration_cards() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.settings(|s| s.delegations.merge_accepted = false);
    let card = h
        .run(&lead, Operation::Accept { node })
        .unwrap()
        .item
        .unwrap();
    h.cancel_fact(node);
    let cards = h.store.cards(TEAM, 200, None).unwrap().rows;
    let c = cards.iter().find(|c| c.seq == card).unwrap();
    assert_eq!(c.state, "closed");
    assert_eq!(c.decision.as_deref(), Some("stale"));
}
#[test]
fn a_stale_integration_card_is_closed_without_reviving_its_accept() {
    for answer in ["approve", "reject"] {
        let mut h = Harness::new();
        let (node, lead) = h.reported();
        h.settings(|s| s.delegations.merge_accepted = false);
        let card = h
            .run(&lead, Operation::Accept { node })
            .unwrap()
            .item
            .unwrap();
        h.cancel_fact(node);
        // Retained stale rows from before automatic closure still need dismissal.
        h.store.db.as_ref().unwrap().execute("UPDATE cards SET state='open',decision=NULL,decided_actor=NULL WHERE team=?1 AND seq=?2",params![TEAM,card]).unwrap();
        h.run(
            &person(),
            Operation::DecideCard {
                card,
                decision: answer.into(),
            },
        )
        .unwrap();
        let c = h.store.cards(TEAM, 200, None).unwrap().rows.pop().unwrap();
        assert_eq!(c.state, "closed");
        assert_eq!(c.decision.as_deref(), Some("stale"));
        assert_eq!(
            h.store.decisions(TEAM, 200, None).unwrap().rows[0].state,
            DecisionState::Settled
        );
    }
}
#[test]
fn a_person_cannot_make_a_hand_made_integration_card() {
    let mut h = Harness::new();
    let node = h.add(&person(), 1);
    refused(
        h.run(
            &person(),
            Operation::OpenCard {
                kind: "integration".into(),
                text: "Approve a fabricated integration".into(),
                node: Some(node),
                decisions: None,
                parameters: None,
            },
        ),
        code::INVALID_STATE,
    );
    assert!(h.store.cards(TEAM, 200, None).unwrap().rows.is_empty());
}

#[test]
fn a_retained_stale_integration_card_can_be_dismissed_after_stop() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.settings(|s| s.delegations.merge_accepted = false);
    let card = h
        .run(&lead, Operation::Accept { node })
        .unwrap()
        .item
        .unwrap();
    h.run(&person(), Operation::Stop).unwrap();
    assert_eq!(
        h.store
            .cards(TEAM, 200, None)
            .unwrap()
            .rows
            .last()
            .unwrap()
            .state,
        "closed"
    );
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute(
            "UPDATE cards SET state='open',decision=NULL WHERE team=?1 AND seq=?2",
            params![TEAM, card],
        )
        .unwrap();
    h.run(
        &person(),
        Operation::DecideCard {
            card,
            decision: "approve".into(),
        },
    )
    .unwrap();
    let c = h.store.cards(TEAM, 200, None).unwrap().rows.pop().unwrap();
    assert_eq!(c.state, "closed");
    assert_eq!(c.decision.as_deref(), Some("stale"));
    let last = h
        .store
        .decisions(TEAM, 200, None)
        .unwrap()
        .rows
        .pop()
        .unwrap();
    assert_eq!(last.kind, "cancel");
    assert_eq!(last.state, DecisionState::Pending);
}

#[test]
fn stop_closes_every_open_card_as_stale_and_preserves_decided_cards() {
    let mut h = Harness::new();
    let node = h.add(&person(), 1);
    let decided = ordinary_cards(&mut h, node);
    h.run(&person(), Operation::Stop).unwrap();
    assert_stale_cards(&h, decided);
}

#[test]
fn archive_closes_every_open_card_even_without_a_prior_stop() {
    let mut h = Harness::new();
    let node = h.add(&person(), 1);
    let decided = ordinary_cards(&mut h, node);
    // A finished team can be archived without Stop having closed its cards.
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute(
            "UPDATE teams SET status=?2 WHERE id=?1",
            params![TEAM, encode(&TeamStatus::Finished).unwrap()],
        )
        .unwrap();
    h.run(&person(), Operation::Archive).unwrap();
    assert_stale_cards(&h, decided);
}
fn ordinary_cards(h: &mut Harness, node: u32) -> u64 {
    let mut decided = 0;
    for target in [None, Some(node), None] {
        let card = h
            .run(
                &person(),
                Operation::OpenCard {
                    kind: "proposal".into(),
                    text: "Proposed team change".into(),
                    node: target,
                    decisions: None,
                parameters: None,
                },
            )
            .unwrap()
            .item
            .unwrap();
        decided = card;
    }
    h.run(
        &person(),
        Operation::DecideCard {
            card: decided,
            decision: "acknowledged".into(),
        },
    )
    .unwrap();
    decided
}
fn assert_stale_cards(h: &Harness, decided: u64) {
    let cards = h.store.cards(TEAM, 200, None).unwrap().rows;
    assert_eq!(cards.len(), 3);
    for card in cards {
        assert_eq!(card.state, "closed");
        assert_eq!(
            card.decision.as_deref(),
            Some(if card.seq == decided {
                "acknowledged"
            } else {
                "stale"
            })
        );
        assert_eq!(card.decided_actor.unwrap()["kind"], "person");
    }
}
