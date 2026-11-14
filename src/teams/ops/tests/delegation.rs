use super::*;
mod bounds;
mod settings;

fn latest(h: &Harness, node: u32) -> u64 {
    h.store
        .db
        .as_ref()
        .unwrap()
        .query_row(
            "SELECT seq FROM decisions WHERE team=?1 AND node=?2 ORDER BY seq DESC LIMIT 1",
            params![TEAM, node],
            |r| r.get(0),
        )
        .unwrap()
}
fn state(h: &Harness, decision: u64) -> DecisionState {
    h.store
        .db
        .as_ref()
        .unwrap()
        .query_row(
            "SELECT state FROM decisions WHERE seq=?1",
            [decision],
            |r| r.get(0),
        )
        .unwrap()
}
fn open(h: &Harness) -> Vec<store::Card> {
    h.store
        .waiting(200, None)
        .unwrap()
        .rows
        .into_iter()
        .map(|r| r.card)
        .collect()
}

#[test]
fn a_leads_ready_accept_waits_for_the_owner_when_merge_delegation_is_revoked() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    let seq = latest(&h, node);
    assert_eq!(state(&h, seq), DecisionState::Ready);
    assert!(open(&h).is_empty());
    h.settings(|s| s.delegations.merge_accepted = false);
    assert_eq!(state(&h, seq), DecisionState::AwaitingOwner);
    let cards = open(&h);
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].kind, "integration");
    assert_eq!(cards[0].node, Some(node));
    assert_eq!(cards[0].text, "Approve integration of the accepted result.");
    let linked: u64 = h
        .store
        .db
        .as_ref()
        .unwrap()
        .query_row(
            "SELECT decision_seq FROM cards WHERE seq=?1",
            [cards[0].seq],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(linked, seq);
    assert!(h
        .store
        .integration_accepts(TEAM, 200, None)
        .unwrap()
        .rows
        .is_empty());
    refused(
        h.run(
            &service(),
            Operation::RecordIntegrationConflict {
                node,
                decision: seq,
                conflicts: vec!["src/example.rs".into()],
            },
        ),
        code::INVALID_STATE,
    );
    h.run(
        &person(),
        Operation::DecideCard {
            card: cards[0].seq,
            decision: "reject".into(),
        },
    )
    .unwrap();
    assert_eq!(state(&h, seq), DecisionState::Settled);
    assert!(!h.node(node).hold);
}

#[test]
fn the_persons_accept_keeps_its_authority_after_delegation_is_revoked() {
    let mut h = Harness::new();
    let (node, _) = h.reported();
    h.run(&person(), Operation::Accept { node }).unwrap();
    let seq = latest(&h, node);
    h.settings(|s| s.delegations.merge_accepted = false);
    assert_eq!(state(&h, seq), DecisionState::Ready);
    assert!(open(&h).is_empty());
    h.run(
        &service(),
        Operation::RecordIntegration {
            node,
            decision: seq,
            commit: COMMIT.into(),
        },
    )
    .unwrap();
    assert!(h.node(node).integrated);
}

#[test]
fn a_person_approved_accept_keeps_its_authority_after_delegation_is_revoked() {
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
    h.settings(|s| s.delegations.merge_accepted = true);
    h.settings(|s| s.delegations.merge_accepted = false);
    assert_eq!(state(&h, seq), DecisionState::Ready);
    assert!(open(&h).is_empty());
    let decision = h
        .store
        .decisions(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .find(|d| d.seq == seq)
        .unwrap();
    assert_eq!(decision.actor, serde_json::to_value(lead).unwrap());
}

#[test]
fn an_already_merged_accept_is_not_reopened_by_delegation_revocation() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    let seq = latest(&h, node);
    h.run(
        &service(),
        Operation::RecordIntegration {
            node,
            decision: seq,
            commit: COMMIT.into(),
        },
    )
    .unwrap();
    let before = h.node(node);
    h.settings(|s| s.delegations.merge_accepted = false);
    assert_eq!(state(&h, seq), DecisionState::Ready);
    assert!(open(&h).is_empty());
    assert_eq!(h.node(node), before);
}

#[test]
fn reenabling_delegation_never_approves_a_waiting_card() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    let seq = latest(&h, node);
    h.settings(|s| s.delegations.merge_accepted = false);
    let card = open(&h).into_iter().next().unwrap();
    h.settings(|s| s.delegations.merge_accepted = true);
    assert_eq!(state(&h, seq), DecisionState::AwaitingOwner);
    assert_eq!(open(&h), vec![card.clone()]);
    h.settings(|s| s.delegations.merge_accepted = false);
    assert_eq!(open(&h), vec![card.clone()]);
    h.run(
        &person(),
        Operation::DecideCard {
            card: card.seq,
            decision: "approve".into(),
        },
    )
    .unwrap();
    assert_eq!(state(&h, seq), DecisionState::Ready);
}

#[test]
fn no_merge_tasks_and_non_accept_decisions_get_no_integration_cards() {
    for no_merge in [false, true] {
        let mut h = Harness::new();
        let (node, lead) = h.reported();
        if no_merge {
            h.settings(|s| s.integration = Integration::None);
            h.run(&lead, Operation::Accept { node }).unwrap();
        } else {
            h.run(
                &lead,
                Operation::Return {
                    node,
                    reason: "Check again".into(),
                },
            )
            .unwrap();
        }
        let seq = latest(&h, node);
        let before = state(&h, seq);
        h.settings(|s| s.delegations.merge_accepted = false);
        assert_eq!(state(&h, seq), before);
        assert!(open(&h).is_empty());
    }
}

#[test]
fn revocation_and_its_owner_card_are_atomic_with_the_settings() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    let seq = latest(&h, node);
    let before = h.store.team(TEAM).unwrap().unwrap();
    h.store.db.as_ref().unwrap().execute_batch(
        "CREATE TRIGGER refuse_integration_card BEFORE INSERT ON cards WHEN NEW.kind='integration' BEGIN SELECT RAISE(ABORT,'fixture card refusal'); END;"
    ).unwrap();
    let mut settings = before.settings.clone();
    settings.delegations.merge_accepted = false;
    refused(
        h.run(&person(), Operation::ChangeSettings { settings }),
        code::STORAGE_FAILED,
    );
    assert_eq!(h.store.team(TEAM).unwrap().unwrap(), before);
    assert_eq!(state(&h, seq), DecisionState::Ready);
    assert!(open(&h).is_empty());
}

#[test]
fn settled_decisions_and_old_approvals_do_not_protect_a_new_accept() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    let db = h.store.db.as_ref().unwrap();
    db.execute("WITH RECURSIVE history(n) AS (VALUES(1) UNION ALL SELECT n+1 FROM history WHERE n<5000) INSERT INTO decisions(team,node,generation,actor,kind,text,state) SELECT ?1,?2,0,?3,'accept','Earlier attempt',?4 FROM history",params![TEAM,node,encode(&lead).unwrap(),DecisionState::Settled]).unwrap();
    db.execute("INSERT INTO cards(team,kind,node,actor,text,state,decision,decided_actor,created_ms,decision_seq) SELECT team,'integration',node,actor,'Earlier approval','closed','approve',?2,0,seq FROM decisions WHERE team=?1",params![TEAM,encode(&person()).unwrap()]).unwrap();
    h.run(&lead, Operation::Accept { node }).unwrap();
    let seq = latest(&h, node);
    h.settings(|s| s.delegations.merge_accepted = false);
    assert_eq!(state(&h, seq), DecisionState::AwaitingOwner);
    assert_eq!(open(&h).len(), 1);
    let old_ready: u32 = h
        .store
        .db
        .as_ref()
        .unwrap()
        .query_row(
            "SELECT count(*) FROM decisions WHERE team=?1 AND seq<?2 AND state<>?3",
            params![TEAM, seq, DecisionState::Settled],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(old_ready, 0);
}

#[test]
fn revocation_uses_pending_and_approval_indexes_and_an_older_v2_gains_the_index() {
    let mut h = Harness::new();
    {
        let db = h.store.db.as_ref().unwrap();
        let mut statement = db
            .prepare(&format!(
                "EXPLAIN QUERY PLAN {}",
                super::super::delegation::revocation_sql(),
            ))
            .unwrap();
        let plan = statement
            .query_map(params![TEAM, DecisionState::Ready, 601], |r| {
                r.get::<_, String>(3)
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
            .join("\n");
        assert!(plan.contains("decision_pending"), "{plan}");
        assert!(plan.contains("integration_approved"), "{plan}");
        assert!(
            !plan.contains("SCAN d") && !plan.contains("SCAN c") && !plan.contains("SCAN f"),
            "{plan}"
        );
        drop(statement);
        db.execute_batch("DROP INDEX integration_approved;")
            .unwrap();
    }
    h.store = Store::open(h.temp.path()).unwrap();
    let db = h.store.db.as_ref().unwrap();
    let installed: u32 = db
        .query_row(
            "SELECT count(*) FROM sqlite_schema WHERE type='index' AND name='integration_approved'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(installed, 1);
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        2
    );
}
