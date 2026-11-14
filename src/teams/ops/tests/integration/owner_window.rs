use super::*;
mod revoked;

fn accepted(h: &mut Harness) -> (u32, Actor, u64) {
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    (node, lead, h.accept_decision(node))
}
fn revoke(h: &mut Harness, node: u32) -> u64 {
    h.settings(|s| s.delegations.merge_accepted = false);
    h.store
        .waiting(200, None)
        .unwrap()
        .rows
        .into_iter()
        .find(|r| r.card.node == Some(node))
        .unwrap()
        .card
        .seq
}
fn receipt(node: u32, decision: u64) -> Operation {
    Operation::RecordIntegration {
        node,
        decision,
        commit: COMMIT.into(),
    }
}
fn conflict(node: u32, decision: u64) -> Operation {
    Operation::RecordIntegrationConflict {
        node,
        decision,
        conflicts: vec!["src/example.rs".into()],
    }
}
fn decision(h: &Harness, seq: u64) -> store::Decision {
    h.store
        .decisions(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .find(|d| d.seq == seq)
        .unwrap()
}
fn journal(h: &Harness) -> Vec<store::JournalEntry> {
    h.store.journal(TEAM, 200, None).unwrap().rows
}
fn no_writes(h: &Harness) {
    assert!(h
        .store
        .integration_accepts(TEAM, 200, None)
        .unwrap()
        .rows
        .is_empty());
}

#[test]
fn a_merge_fact_overtakes_revocation_and_finishes_like_a_ready_accept() {
    for done_first in [false, true] {
        let mut ready = Harness::new();
        let (expected_node, _, ready_seq) = accepted(&mut ready);
        if done_first {
            ready.observe(expected_node, TaskStage::Done);
        }
        ready
            .run(&service(), receipt(expected_node, ready_seq))
            .unwrap();
        if !done_first {
            ready.observe(expected_node, TaskStage::Done);
        }

        let mut h = Harness::new();
        let (node, _, seq) = accepted(&mut h);
        let before = journal(&h).len();
        let card = revoke(&mut h, node);
        assert_eq!(journal(&h).len(), before + 1);
        assert_eq!(journal(&h).last().unwrap().kind, "change_settings");
        no_writes(&h);
        if done_first {
            h.observe(node, TaskStage::Done);
        }
        // The actor may have restarted after updating the integration ref.
        h.store = Store::open(h.temp.path()).unwrap();
        let before = journal(&h).len();
        h.run(&service(), receipt(node, seq)).unwrap();
        assert_eq!(journal(&h).len(), before + 1);
        assert_eq!(journal(&h).last().unwrap().kind, "record_integration");
        assert_eq!(journal(&h).last().unwrap().actor["kind"], "service");
        let overtaken = h.store.card(TEAM, card).unwrap().unwrap();
        assert_eq!(overtaken.state, "closed");
        assert_eq!(overtaken.decision.as_deref(), Some("stale"));
        assert_eq!(overtaken.decided_actor.unwrap()["kind"], "service");
        assert!(h.store.waiting(200, None).unwrap().rows.is_empty());
        if !done_first {
            assert_eq!(decision(&h, seq).state, DecisionState::Ready);
            h.observe(node, TaskStage::Done);
        }
        assert_eq!(h.node(node), ready.node(expected_node));
        assert_eq!(h.node(node).state, NodeState::Accepted);
        assert_eq!(decision(&h, seq).state, DecisionState::Settled);
        assert_fact(&h, seq, Some(COMMIT), &[]);
        let before = journal(&h).len();
        refused(h.run(&service(), receipt(node, seq)), code::INVALID_STATE);
        assert_eq!(journal(&h).len(), before);
    }
}

#[test]
fn owner_approval_before_the_receipt_keeps_the_one_approved_card_and_one_fact() {
    let mut h = Harness::new();
    let (node, _, seq) = accepted(&mut h);
    let card = revoke(&mut h, node);
    assert!(preserved(&h, seq));
    h.run(
        &person(),
        Operation::DecideCard {
            card,
            decision: "approve".into(),
        },
    )
    .unwrap();
    assert!(preserved(&h, seq));
    let approved = h.store.card(TEAM, card).unwrap().unwrap();
    let before = journal(&h).len();
    h.run(&service(), receipt(node, seq)).unwrap();
    assert_eq!(journal(&h).len(), before + 1);
    assert_eq!(h.store.card(TEAM, card).unwrap().unwrap(), approved);
    assert_eq!(approved.decision.as_deref(), Some("approve"));
    assert_eq!(h.store.cards(TEAM, 200, None).unwrap().rows.len(), 1);
    assert_fact(&h, seq, Some(COMMIT), &[]);
    refused(h.run(&service(), receipt(node, seq)), code::INVALID_STATE);
    assert_eq!(journal(&h).len(), before + 1);
    h.observe(node, TaskStage::Done);
    assert_eq!(h.node(node).state, NodeState::Accepted);
}

#[test]
fn a_receipt_after_owner_rejection_is_only_late_provenance() {
    for done_first in [false, true] {
        let mut h = Harness::new();
        let (node, _, seq) = accepted(&mut h);
        let card = revoke(&mut h, node);
        if done_first {
            h.observe(node, TaskStage::Done);
        }
        h.run(
            &person(),
            Operation::DecideCard {
                card,
                decision: "reject".into(),
            },
        )
        .unwrap();
        let before_node = h.node(node);
        let before_decision = decision(&h, seq);
        let rejected = h.store.card(TEAM, card).unwrap().unwrap();
        let before = journal(&h).len();
        h.run(&service(), receipt(node, seq)).unwrap();
        assert_fact(&h, seq, Some(COMMIT), &[]);
        assert_eq!(h.node(node), before_node);
        assert_eq!(decision(&h, seq), before_decision);
        assert_eq!(h.store.card(TEAM, card).unwrap().unwrap(), rejected);
        assert_eq!(rejected.decision.as_deref(), Some("reject"));
        assert_eq!(journal(&h).len(), before + 1);
        // The late fact is provenance, but the person is now told once: the
        // only waiting card is the acknowledgement the merge deserves.
        let waiting = h.store.waiting(200, None).unwrap().rows;
        assert_eq!(waiting.len(), 1);
        assert_eq!(waiting[0].card.kind, "rejected_merge");
        no_writes(&h);
        refused(h.run(&service(), receipt(node, seq)), code::INVALID_STATE);
    }
}

#[test]
fn a_rejected_receipts_provenance_cannot_integrate_or_close_a_new_accept() {
    let mut h = Harness::new();
    let (node, lead, old) = accepted(&mut h);
    let rejected = revoke(&mut h, node);
    h.run(
        &person(),
        Operation::DecideCard {
            card: rejected,
            decision: "reject".into(),
        },
    )
    .unwrap();
    let new_card = h
        .run(&lead, Operation::Accept { node })
        .unwrap()
        .item
        .unwrap();
    let new = h.accept_decision(node);
    assert_ne!(old, new);
    let before = h.node(node);
    h.run(&service(), receipt(node, old)).unwrap();
    assert_fact(&h, old, Some(COMMIT), &[]);
    assert_eq!(h.node(node), before);
    assert_eq!(decision(&h, new).state, DecisionState::AwaitingOwner);
    assert_eq!(h.store.card(TEAM, new_card).unwrap().unwrap().state, "open");
    assert_eq!(
        h.store
            .card(TEAM, rejected)
            .unwrap()
            .unwrap()
            .decision
            .as_deref(),
        Some("reject")
    );
    no_writes(&h);
}

#[test]
fn awaiting_and_rejected_accepts_refuse_conflicts_without_changing_anything() {
    for reject in [false, true] {
        let mut h = Harness::new();
        let (node, _, seq) = accepted(&mut h);
        let card = revoke(&mut h, node);
        if reject {
            h.run(
                &person(),
                Operation::DecideCard {
                    card,
                    decision: "reject".into(),
                },
            )
            .unwrap();
        }
        let before_node = h.node(node);
        let before_decision = decision(&h, seq);
        let before_card = h.store.card(TEAM, card).unwrap().unwrap();
        let before_team = h.store.team(TEAM).unwrap().unwrap();
        let before = journal(&h);
        refused(h.run(&service(), conflict(node, seq)), code::INVALID_STATE);
        assert_eq!(h.node(node), before_node);
        assert_eq!(decision(&h, seq), before_decision);
        assert_eq!(h.store.card(TEAM, card).unwrap().unwrap(), before_card);
        assert_eq!(h.store.team(TEAM).unwrap().unwrap(), before_team);
        assert_eq!(journal(&h), before);
        assert!(h.store.integration_fact(TEAM, node).unwrap().is_none());
        if !reject {
            assert_eq!(before_card.state, "open");
            h.run(
                &person(),
                Operation::DecideCard {
                    card,
                    decision: "approve".into(),
                },
            )
            .unwrap();
            h.run(&service(), conflict(node, seq)).unwrap();
            assert_fact(&h, seq, None, &["src/example.rs".into()]);
        }
    }
}

#[test]
fn an_accept_made_with_delegation_off_never_ready_refuses_a_merge_fact() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.settings(|s| s.delegations.merge_accepted = false);
    let card = h
        .run(&lead, Operation::Accept { node })
        .unwrap()
        .item
        .unwrap();
    let seq = h.accept_decision(node);
    assert_eq!(decision(&h, seq).state, DecisionState::AwaitingOwner);
    no_writes(&h);
    let before_node = h.node(node);
    let before_decision = decision(&h, seq);
    let before_card = h.store.card(TEAM, card).unwrap();
    let before_team = h.store.team(TEAM).unwrap();
    let before_journal = journal(&h);
    refused(h.run(&service(), receipt(node, seq)), code::INVALID_STATE);
    assert!(h.store.integration_fact(TEAM, node).unwrap().is_none());
    assert_eq!(h.node(node), before_node);
    assert_eq!(decision(&h, seq), before_decision);
    assert_eq!(h.store.card(TEAM, card).unwrap(), before_card);
    assert_eq!(h.store.team(TEAM).unwrap(), before_team);
    assert_eq!(journal(&h), before_journal);
}

fn preserved(h: &Harness, seq: u64) -> bool {
    h.store
        .db
        .as_ref()
        .unwrap()
        .query_row(
            "SELECT stopped_ready FROM decisions WHERE seq=?1",
            [seq],
            |r| r.get(0),
        )
        .unwrap()
}

#[test]
fn a_refused_conflict_does_not_consume_the_merge_after_owner_approval() {
    let mut h = Harness::new();
    let (node, _, seq) = accepted(&mut h);
    let card = revoke(&mut h, node);
    refused(h.run(&service(), conflict(node, seq)), code::INVALID_STATE);
    assert!(h.store.integration_fact(TEAM, node).unwrap().is_none());
    h.run(
        &person(),
        Operation::DecideCard {
            card,
            decision: "approve".into(),
        },
    )
    .unwrap();
    assert_eq!(
        h.store.integration_accepts(TEAM, 200, None).unwrap().rows[0].seq,
        seq
    );
    h.run(&service(), receipt(node, seq)).unwrap();
    assert_fact(&h, seq, Some(COMMIT), &[]);
    h.observe(node, TaskStage::Done);
    assert_eq!(h.node(node).state, NodeState::Accepted);
}

#[test]
fn an_awaiting_receipt_is_still_a_service_only_fact() {
    let mut h = Harness::new();
    let (node, lead, seq) = accepted(&mut h);
    let card = revoke(&mut h, node);
    let member = h.bind("m2");
    for actor in [person(), lead, member, other()] {
        refused(h.run(&actor, receipt(node, seq)), code::NOT_ALLOWED);
        assert!(h.store.integration_fact(TEAM, node).unwrap().is_none());
        assert_eq!(decision(&h, seq).state, DecisionState::AwaitingOwner);
        assert_eq!(h.store.card(TEAM, card).unwrap().unwrap().state, "open");
    }
}

#[test]
fn a_failed_card_close_rolls_back_the_receipt_and_all_its_transitions() {
    let mut h = Harness::new();
    let (node, _, seq) = accepted(&mut h);
    let card = revoke(&mut h, node);
    let before_card = h.store.card(TEAM, card).unwrap().unwrap();
    let before_node = h.node(node);
    let before_team = h.store.team(TEAM).unwrap().unwrap();
    let before_journal = journal(&h);
    h.store.db.as_ref().unwrap().execute_batch(
        "CREATE TRIGGER refuse_card_close BEFORE UPDATE OF state ON cards WHEN OLD.kind='integration' AND NEW.state='closed' BEGIN SELECT RAISE(ABORT,'fixture card close refusal'); END;"
    ).unwrap();
    refused(h.run(&service(), receipt(node, seq)), code::STORAGE_FAILED);
    assert!(h.store.integration_fact(TEAM, node).unwrap().is_none());
    assert_eq!(h.store.card(TEAM, card).unwrap().unwrap(), before_card);
    assert_eq!(h.node(node), before_node);
    assert_eq!(h.store.team(TEAM).unwrap().unwrap(), before_team);
    assert_eq!(journal(&h), before_journal);
    assert_eq!(decision(&h, seq).state, DecisionState::AwaitingOwner);
}

#[test]
fn the_exact_card_lookup_is_indexed_and_an_older_v2_file_gains_the_index() {
    let mut h = Harness::new();
    let (node, _, seq) = accepted(&mut h);
    let card = revoke(&mut h, node);
    h.run(
        &person(),
        Operation::DecideCard {
            card,
            decision: "reject".into(),
        },
    )
    .unwrap();
    {
        let db = h.store.db.as_ref().unwrap();
        let mut statement = db
            .prepare(&format!(
                "EXPLAIN QUERY PLAN {}",
                crate::teams::ops::integration::INTEGRATION_CARD_SQL
            ))
            .unwrap();
        let plan = statement
            .query_map(params![seq, TEAM], |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
            .join("\n");
        assert!(
            plan.contains("SEARCH cards USING INDEX integration_card (decision_seq=?)"),
            "{plan}"
        );
        assert!(!plan.contains("SCAN "), "{plan}");
        drop(statement);
        db.execute_batch("DROP INDEX integration_card;").unwrap();
    }
    h.store = Store::open(h.temp.path()).unwrap();
    let db = h.store.db.as_ref().unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE type='index' AND name='integration_card'",
            [],
            |r| r.get::<_, u32>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        2
    );
    h.run(&service(), receipt(node, seq)).unwrap();
    assert_fact(&h, seq, Some(COMMIT), &[]);
}

/// The owner cards opened for a merge that survived a rejection.
fn rejected_merge_cards(h: &Harness) -> Vec<store::Card> {
    h.store
        .cards(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .filter(|card| card.kind == "rejected_merge")
        .collect()
}

fn rejected_merge_items(h: &Harness) -> u32 {
    h.store
        .db
        .as_ref()
        .unwrap()
        .query_row(
            "SELECT count(*) FROM inbox WHERE team=?1 AND kind LIKE 'rejected_merge%'",
            [TEAM],
            |r| r.get(0),
        )
        .unwrap()
}

/// The integration card a rejected decision has: the merge fact keeps it open.
fn open_integration_card(h: &Harness, node: u32, decision: u64) -> u64 {
    let db = h.store.db.as_ref().unwrap();
    db.execute(
        "UPDATE decisions SET state='awaiting_owner',stopped_ready=1 WHERE seq=?1",
        [decision],
    )
    .unwrap();
    db.execute(
        "INSERT INTO cards(team,kind,node,actor,text,state,created_ms,decision_seq,decisions)
         VALUES(?1,'integration',?2,?3,'Approve integration of the accepted result.','open',1,?4,?5)",
        params![TEAM, node, encode(&person()).unwrap(), decision, encode(&["approve", "reject"]).unwrap()],
    )
    .unwrap();
    db.last_insert_rowid() as u64
}

#[test]
fn a_merge_fact_then_a_rejection_opens_one_card_and_one_lead_item() {
    let mut h = Harness::new();
    let (node, _, seq) = accepted(&mut h);
    // The merge was written while the delegation was active; the person's card
    // then waits for an answer that keeps the merge fact.
    h.run(&service(), receipt(node, seq)).unwrap();
    let card = open_integration_card(&h, node, seq);
    let integrated = h.node(node).integrated;
    h.run(
        &person(),
        Operation::DecideCard {
            card,
            decision: "reject".into(),
        },
    )
    .unwrap();
    let branch = h.store.team(TEAM).unwrap().unwrap().integration_branch;
    let cards = rejected_merge_cards(&h);
    assert_eq!(cards.len(), 1);
    let told = &cards[0];
    assert_eq!(told.node, Some(node));
    assert_eq!(told.decisions, ["acknowledged"]);
    for fact in [COMMIT, &branch, &format!("node {node}")] {
        assert!(told.text.contains(fact), "{}", told.text);
    }
    assert!(told.text.contains("did not undo the merge"));
    assert!(told.text.contains(&format!("only a revert on {branch}")));
    assert_eq!(rejected_merge_items(&h), 1);
    assert_eq!(
        h.store
            .inbox(TEAM, 200, None)
            .unwrap()
            .rows
            .into_iter()
            .find(|item| item.kind == format!("rejected_merge:{seq}"))
            .unwrap()
            .text,
        told.text
    );
    // Nothing else moved: the decision settles and the merge stays.
    assert_eq!(decision(&h, seq).state, DecisionState::Settled);
    assert_eq!(h.node(node).integrated, integrated);
    // Acknowledge closes the card and changes nothing else.
    let before = h.node(node);
    h.run(
        &person(),
        Operation::DecideCard {
            card: told.seq,
            decision: "acknowledged".into(),
        },
    )
    .unwrap();
    assert_eq!(h.store.card(TEAM, told.seq).unwrap().unwrap().state, "closed");
    assert_eq!(h.node(node), before);
    assert_eq!(rejected_merge_cards(&h).len(), 1);
    assert_eq!(rejected_merge_items(&h), 1);
    assert_eq!(decision(&h, seq).state, DecisionState::Settled);
    // A replay of the same fact opens nothing.
    let replayed = rows::rejected_merge(
        h.store.db.as_ref().unwrap(),
        TEAM,
        node,
        seq,
        &branch,
        &encode(&person()).unwrap(),
        1,
    )
    .unwrap();
    assert!(!replayed);
    assert_eq!(rejected_merge_cards(&h).len(), 1);
    assert_eq!(rejected_merge_items(&h), 1);
}

#[test]
fn a_rejection_then_the_late_merge_fact_opens_one_card_and_one_lead_item() {
    let mut h = Harness::new();
    let (node, _, seq) = accepted(&mut h);
    let card = revoke(&mut h, node);
    h.run(
        &person(),
        Operation::DecideCard {
            card,
            decision: "reject".into(),
        },
    )
    .unwrap();
    assert!(rejected_merge_cards(&h).is_empty());
    let settled = decision(&h, seq);
    let integrated = h.node(node).integrated;
    h.run(&service(), receipt(node, seq)).unwrap();
    let branch = h.store.team(TEAM).unwrap().unwrap().integration_branch;
    let cards = rejected_merge_cards(&h);
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].node, Some(node));
    assert!(cards[0].text.contains(COMMIT));
    assert!(cards[0].text.contains(&branch));
    assert_eq!(rejected_merge_items(&h), 1);
    // The fact stays provenance: the decision and the node do not move.
    assert_eq!(decision(&h, seq), settled);
    assert_eq!(h.node(node).integrated, integrated);
    // A replay of the receipt is refused and opens nothing.
    refused(h.run(&service(), receipt(node, seq)), code::INVALID_STATE);
    assert_eq!(rejected_merge_cards(&h).len(), 1);
    assert_eq!(rejected_merge_items(&h), 1);
}

#[test]
fn a_rejection_without_a_merge_opens_nothing() {
    let mut h = Harness::new();
    let (node, _, seq) = accepted(&mut h);
    let card = revoke(&mut h, node);
    h.run(
        &person(),
        Operation::DecideCard {
            card,
            decision: "reject".into(),
        },
    )
    .unwrap();
    assert!(rejected_merge_cards(&h).is_empty());
    assert_eq!(rejected_merge_items(&h), 0);
    assert_eq!(decision(&h, seq).state, DecisionState::Settled);
}

#[test]
fn two_rejected_decisions_of_one_node_and_generation_each_get_a_card_and_item() {
    let mut h = Harness::new();
    let (node, _, first) = accepted(&mut h);
    h.run(&service(), receipt(node, first)).unwrap();
    let db = h.store.db.as_ref().unwrap();
    let generation: u32 = db
        .query_row("SELECT generation FROM decisions WHERE seq=?1", [first], |r| r.get(0))
        .unwrap();
    // The first decision is settled, so the node can take another one in the
    // same generation; the two must not collide on the inbox key.
    db.execute(
        "INSERT INTO decisions(team,node,generation,actor,kind,text,state,stopped_ready)
         VALUES(?1,?2,?3,?4,'accept','A second acceptance',?5,1)",
        params![TEAM, node, generation, encode(&person()).unwrap(), DecisionState::Settled],
    )
    .unwrap();
    let second = db.last_insert_rowid() as u64;
    db.execute(
        "INSERT INTO integration_facts(decision,team,node,actor,commit_id,conflicts,created_ms)
         VALUES(?1,?2,?3,?4,?5,?6,0)",
        params![second, TEAM, node, encode(&service()).unwrap(), NEW_COMMIT, encode(&Vec::<String>::new()).unwrap()],
    )
    .unwrap();
    let branch = h.store.team(TEAM).unwrap().unwrap().integration_branch;
    let actor = encode(&person()).unwrap();
    assert!(rows::rejected_merge(db, TEAM, node, first, &branch, &actor, 1).unwrap());
    assert!(rows::rejected_merge(db, TEAM, node, second, &branch, &actor, 2).unwrap());
    assert_eq!(rejected_merge_cards(&h).len(), 2);
    assert_eq!(rejected_merge_items(&h), 2);
}
