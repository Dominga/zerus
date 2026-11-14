use super::*;
#[test]
fn create_and_add_replay_the_first_reply_and_one_journal_entry() {
    let mut h = Harness::empty();
    let r = request(0, create());
    let first = super::super::apply(&mut h.store, &person(), &r, 1).unwrap();
    assert_eq!(
        super::super::apply(&mut h.store, &person(), &r, 2).unwrap(),
        first
    );
    assert_eq!(h.store.members(TEAM, 200, None).unwrap().rows.len(), 4);
    let r = request(
        1,
        Operation::AddTask {
            task: task(1),
            role: None,
            pipeline: None,
        },
    );
    let first = super::super::apply(&mut h.store, &person(), &r, 1).unwrap();
    assert_eq!(
        super::super::apply(&mut h.store, &person(), &r, 2).unwrap(),
        first
    );
    assert_eq!(h.store.journal(TEAM, 200, None).unwrap().rows.len(), 2);
}
#[test]
fn replay_compares_actor_and_full_content() {
    let mut h = Harness::new();
    let r = request(1, Operation::Pause);
    super::super::apply(&mut h.store, &person(), &r, 1).unwrap();
    let other = Actor::Person {
        person: "local".into(),
        origin: PersonOrigin::CliInterim,
    };
    refused(
        super::super::apply(&mut h.store, &other, &r, 2),
        code::DUPLICATE,
    );
    let mut changed = r;
    changed.operation = Operation::Resume;
    refused(
        super::super::apply(&mut h.store, &person(), &changed, 2),
        code::DUPLICATE,
    );
}
#[test]
fn stale_revision_refusal_is_durable_and_rolls_back_the_operation() {
    let mut h = Harness::new();
    let r = request(0, Operation::Pause);
    refused(
        super::super::apply(&mut h.store, &person(), &r, 1),
        code::STALE_REVISION,
    );
    h.run(&person(), Operation::Pause).unwrap();
    refused(
        super::super::apply(&mut h.store, &person(), &r, 2),
        code::STALE_REVISION,
    );
    assert_eq!(h.store.journal(TEAM, 200, None).unwrap().rows.len(), 2);
}
#[test]
fn exact_receipt_replay_is_read_only_after_delegation_revocation() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    let r = request(
        h.store.team(TEAM).unwrap().unwrap().revision,
        Operation::Accept { node },
    );
    let first = super::super::apply(&mut h.store, &lead, &r, 1).unwrap();
    h.settings(|s| s.delegations.accept_results = false);
    assert_eq!(
        super::super::apply(&mut h.store, &lead, &r, 2).unwrap(),
        first
    );
    refused(h.run(&lead, Operation::Accept { node }), code::NOT_ALLOWED);
}
#[test]
fn request_wire_refuses_unknown_fields_and_sender_observations() {
    for op in [
        Operation::Pause,
        Operation::AddTask {
            task: task(1),
            role: None,
            pipeline: None,
        },
    ] {
        let mut v = serde_json::to_value(request(0, op)).unwrap();
        v["observation"] = serde_json::json!({"stage":"done"});
        assert!(serde_json::from_value::<Request>(v).is_err());
    }
    let mut v = serde_json::to_value(request(0, Operation::Accept { node: 1 })).unwrap();
    v["integration_commit"] = COMMIT.into();
    assert!(serde_json::from_value::<Request>(v).is_err());
}
#[test]
fn rejected_creation_does_not_create_a_store() {
    let mut h = Harness::empty();
    refused(h.run(&other(), create()), code::NOT_ALLOWED);
    assert!(!h.temp.path().join("teams/teams-v1.sqlite3").exists());
}
#[test]
fn failed_operation_has_no_partial_rows_or_journal() {
    let mut h = Harness::new();
    let node = h.add(&person(), 1);
    let old = h.store.team(TEAM).unwrap();
    let journal = h.store.journal(TEAM, 200, None).unwrap();
    let mut r = route(node);
    if let Operation::Route { role, .. } = &mut r {
        *role = "missing".into();
    }
    refused(h.run(&person(), r), code::UNKNOWN_ROLE);
    assert_eq!(h.store.team(TEAM).unwrap(), old);
    assert_eq!(h.store.journal(TEAM, 200, None).unwrap(), journal);
}
/// The saved content of one request, as the product's apply writer wrote it.
fn content(h: &Harness, id: &str) -> String {
    h.store
        .db
        .as_ref()
        .unwrap()
        .query_row("SELECT content FROM requests WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .unwrap()
}
fn content_rows(h: &Harness, id: &str) -> u32 {
    h.store
        .db
        .as_ref()
        .unwrap()
        .query_row("SELECT count(*) FROM requests WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .unwrap()
}
/// Round 2's product-written `SettleLeftHandout` requests carry the reason
/// field on the wire: `null` when the actor read no departure and `""` for a
/// store-state departure. The strict `Request` decoder learns an operation's
/// known fields by serializing the decoded operation, so the field must
/// serialize even when it is `None`; otherwise those saved bytes are refused
/// as an unknown field and their exact receipt cannot be replayed
/// (zerus-63rru.85 round 4). The rows are written by the product's apply
/// writer in this test, not copied from a store.
#[test]
fn round2_settle_requests_with_null_and_empty_reasons_decode_and_replay_exactly() {
    for reason in [None, Some(String::new())] {
        let mut h = Harness::new();
        h.bind("m1");
        h.add(&person(), 1);
        h.intent(1, "m1");
        h.run(
            &person(),
            Operation::PauseMember {
                member: "m1".into(),
            },
        )
        .unwrap();
        let before = h.store.team(TEAM).unwrap().unwrap();
        let settle = request(
            before.revision,
            Operation::SettleLeftHandout {
                node: 1,
                reason: reason.clone(),
            },
        );
        let first = crate::teams::ops::apply(&mut h.store, &service(), &settle, 1234).unwrap();
        let saved = content(&h, &settle.request_id);
        match &reason {
            None => assert!(saved.contains("\"reason\":null"), "{reason:?}: {saved}"),
            Some(_) => assert!(saved.contains("\"reason\":\"\""), "{reason:?}: {saved}"),
        }
        let decoded: Request =
            serde_json::from_str(&saved).expect("the round-2 content decodes");
        assert_eq!(decoded, settle, "{reason:?}: the decoded request is exact");
        let journal = h.store.journal(TEAM, 200, None).unwrap();
        let replay = crate::teams::ops::apply(&mut h.store, &service(), &decoded, 1235).unwrap();
        assert_eq!(replay, first, "{reason:?}: exact receipt replay");
        assert_eq!(content_rows(&h, &settle.request_id), 1, "{reason:?}");
        assert_eq!(
            h.store.team(TEAM).unwrap().unwrap().revision,
            first.revision,
            "{reason:?}: the replay writes nothing"
        );
        assert_eq!(
            h.store.journal(TEAM, 200, None).unwrap(),
            journal,
            "{reason:?}: the replay journals nothing"
        );
    }
}
/// A newly written settle request may omit the reason key, as round 1's shape
/// did: `serde(default)` accepts it, the operation decodes with `None`, and
/// the product writer keeps the key on the wire for the next reader.
#[test]
fn a_new_settle_request_without_the_reason_key_decodes_and_applies() {
    let mut h = Harness::new();
    h.bind("m1");
    h.add(&person(), 1);
    h.intent(1, "m1");
    h.run(
        &person(),
        Operation::PauseMember {
            member: "m1".into(),
        },
    )
    .unwrap();
    let rev = h.store.team(TEAM).unwrap().unwrap().revision;
    let id = uuid::Uuid::new_v4().to_string();
    let wire = serde_json::json!({
        "request_id": id,
        "team": TEAM,
        "expected_revision": rev,
        "op": "settle_left_handout",
        "node": 1,
    });
    let request: Request =
        serde_json::from_value(wire).expect("a request without the reason key decodes");
    assert_eq!(
        request.operation,
        Operation::SettleLeftHandout {
            node: 1,
            reason: None,
        }
    );
    let reply = crate::teams::ops::apply(&mut h.store, &service(), &request, 1234).unwrap();
    assert_eq!(reply.revision, rev + 1);
    assert!(
        content(&h, &id).contains("\"reason\":null"),
        "the new writer keeps the key on the wire"
    );
    let effects = h.store.effects(TEAM, 200, None).unwrap().rows;
    assert_eq!(effects.len(), 1);
    assert_eq!(effects[0].state, "superseded");
    assert_eq!(h.store.inbox(TEAM, 200, None).unwrap().rows.len(), 1);
}
