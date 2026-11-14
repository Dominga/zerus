use super::*;

fn end(h: &mut Harness, node: u32, how: &str, card: u64) {
    match how {
        "stop" => {
            h.run(&person(), Operation::Stop).unwrap();
        }
        "cancel" => h.cancel_fact(node),
        "reopen" => h.observe(node, TaskStage::InProgress),
        "reject" => {
            h.run(
                &person(),
                Operation::DecideCard {
                    card,
                    decision: "reject".into(),
                },
            )
            .unwrap();
        }
        _ => {}
    }
}

#[test]
fn a_revoked_accept_keeps_its_merge_fact_through_stop_cancel_and_reopen() {
    for how in ["stop", "cancel", "reopen"] {
        let mut h = Harness::new();
        let (node, _, seq) = accepted(&mut h);
        let card = revoke(&mut h, node);
        assert!(preserved(&h, seq), "revocation: {how}");
        no_writes(&h);
        end(&mut h, node, how, card);
        assert!(preserved(&h, seq), "settlement: {how}");
        assert_eq!(decision(&h, seq).state, DecisionState::Settled);
        no_writes(&h);
        h.store = Store::open(h.temp.path()).unwrap();
        let before_decision = decision(&h, seq);
        let before_card = h.store.card(TEAM, card).unwrap();
        let before_state = h.node(node).state;
        let before_generation = h.store.generation(TEAM, node).unwrap();
        let before = journal(&h).len();
        h.run(&service(), receipt(node, seq)).unwrap();
        assert_fact(&h, seq, Some(COMMIT), &[]);
        assert_eq!(decision(&h, seq), before_decision);
        assert_eq!(h.store.card(TEAM, card).unwrap(), before_card);
        assert_eq!(h.node(node).state, before_state);
        assert_eq!(h.store.generation(TEAM, node).unwrap(), before_generation);
        assert_eq!(journal(&h).len(), before + 1);
        no_writes(&h);
        refused(h.run(&service(), receipt(node, seq)), code::INVALID_STATE);
    }
}

#[test]
fn an_accept_that_never_had_merge_authority_refuses_a_fact_in_every_state() {
    for how in ["awaiting", "stop", "cancel", "reopen", "reject"] {
        let mut h = Harness::new();
        let (node, lead) = h.reported();
        h.settings(|s| s.delegations.merge_accepted = false);
        let card = h
            .run(&lead, Operation::Accept { node })
            .unwrap()
            .item
            .unwrap();
        let seq = h.accept_decision(node);
        assert!(!preserved(&h, seq));
        end(&mut h, node, how, card);
        assert!(!preserved(&h, seq));
        let before_node = h.node(node);
        let before_decision = decision(&h, seq);
        let before_card = h.store.card(TEAM, card).unwrap();
        let before_team = h.store.team(TEAM).unwrap();
        let before_journal = journal(&h);
        refused(h.run(&service(), receipt(node, seq)), code::INVALID_STATE);
        refused(h.run(&service(), conflict(node, seq)), code::INVALID_STATE);
        assert!(h.store.integration_fact(TEAM, node).unwrap().is_none());
        assert_eq!(h.node(node), before_node);
        assert_eq!(decision(&h, seq), before_decision);
        assert_eq!(h.store.card(TEAM, card).unwrap(), before_card);
        assert_eq!(h.store.team(TEAM).unwrap(), before_team);
        assert_eq!(journal(&h), before_journal);
        no_writes(&h);
    }
}

#[test]
fn marking_an_awaiting_accept_never_makes_it_discoverable() {
    let mut h = Harness::new();
    let (node, _, seq) = accepted(&mut h);
    let card = revoke(&mut h, node);
    assert!(preserved(&h, seq));
    no_writes(&h);
    h.settings(|s| s.delegations.merge_accepted = true);
    assert!(preserved(&h, seq));
    no_writes(&h);
    h.run(
        &person(),
        Operation::DecideCard {
            card,
            decision: "approve".into(),
        },
    )
    .unwrap();
    assert!(preserved(&h, seq));
    assert_eq!(
        h.store.integration_accepts(TEAM, 200, None).unwrap().rows[0].seq,
        seq
    );
    h.run(&service(), receipt(node, seq)).unwrap();
    h.observe(node, TaskStage::Done);
    assert_eq!(h.node(node).state, NodeState::Accepted);
    no_writes(&h);
}

#[test]
fn a_store_with_duplicate_legacy_integration_cards_opens_with_a_non_unique_index() {
    let mut h = Harness::new();
    let (node, _, seq) = accepted(&mut h);
    revoke(&mut h, node);
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute_batch("DROP INDEX integration_card")
        .unwrap();
    h.store.db.as_ref().unwrap().execute("INSERT INTO cards(team,kind,node,actor,text,created_ms,decision_seq) SELECT team,kind,node,actor,'Legacy duplicate',created_ms,decision_seq FROM cards WHERE decision_seq=?1",[seq]).unwrap();
    h.store = Store::open(h.temp.path()).unwrap();
    h.store = Store::open(h.temp.path()).unwrap();
    assert_eq!(h.store.cards(TEAM, 200, None).unwrap().rows.len(), 2);
    assert_non_unique(&h);
}

#[test]
fn an_existing_unique_integration_card_index_is_replaced_on_open() {
    let mut h = Harness::new();
    h.store.db.as_ref().unwrap().execute_batch("DROP INDEX integration_card; CREATE UNIQUE INDEX integration_card ON cards(decision_seq) WHERE kind='integration'").unwrap();
    h.store = Store::open(h.temp.path()).unwrap();
    assert_non_unique(&h);
}

fn assert_non_unique(h: &Harness) {
    let unique: bool = h
        .store
        .db
        .as_ref()
        .unwrap()
        .query_row(
            "SELECT \"unique\" FROM pragma_index_list('cards') WHERE name='integration_card'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!unique);
}
