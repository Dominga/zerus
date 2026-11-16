//! Teams are made by the operation producers; original rows by reserve and
//! settle below. Literal DTOs are model input, never native admission.
use super::*;
use crate::teams::ops::tests::{self as producers, Harness, TEAM};
use serde_json::json;
use zerus_client_core::desktop::creation_clone::Scope;

fn header(h: &Harness) -> Header {
    let id = uuid::Uuid::new_v4().to_string();
    let team = h.store.team(TEAM).unwrap().unwrap();
    let task = TaskRef {
        project: team.project.clone(),
        uuid: id.clone(),
    };
    let plan = wire::Plan {
        request_id: id,
        team: TEAM.into(),
        expected_revision: team.revision,
        action: wire::Action::NewTask {
            project: task.project.clone(),
            title: "A native intake control".into(),
            backlog: false,
        },
    };
    let request = ops::Request {
        request_id: plan.request_id.clone(),
        team: TEAM.into(),
        expected_revision: team.revision,
        operation: ops::Operation::AddTask {
            task: task.clone(),
            role: None,
            pipeline: None,
        },
    };
    Header {
        intent: wire::Intent {
            scope: Scope {
                configuration: "a".repeat(64),
                service_epoch: uuid::Uuid::new_v4().to_string(),
            },
            machine: team.home,
            store: wire::StoreBinding::Existing {
                fingerprint: "b".repeat(64),
            },
            facts: "c".repeat(64),
            request: wire::digest(&request).unwrap(),
            plan: plan.reference().unwrap(),
        },
        request,
        task,
    }
}
fn migrate_fixture(h: &Harness) {
    let db = h.store.db.as_ref().unwrap();
    // Explicitly exercise the migration's idempotence on current storage.
    let tx = db.unchecked_transaction().unwrap();
    migrate(&tx).unwrap();
    tx.commit().unwrap();
}

#[test]
fn native_intake_reservation_is_durable_unknown_and_a_spent_uuid_never_reenters() {
    let h = Harness::new();
    migrate_fixture(&h);
    let header = header(&h);
    let db = h.store.db.as_ref().unwrap();
    assert!(
        reserve(db, &header).is_err(),
        "a writer transaction is mandatory"
    );
    let tx = db.unchecked_transaction().unwrap();
    reserve(&tx, &header).unwrap();
    tx.commit().unwrap();
    let saved = original(db, &header.intent).unwrap();
    assert!(saved.result.is_none());
    assert_eq!(saved.request, header.request);
    assert_eq!(saved.task, header.task);
    let tx = db.unchecked_transaction().unwrap();
    assert_eq!(reserve(&tx, &header).unwrap_err().code, code::DUPLICATE);
    drop(tx);
    assert!(original(db, &header.intent).unwrap().result.is_none());
    assert_eq!(
        h.store.team(TEAM).unwrap().unwrap().revision,
        header.request.expected_revision
    );
}

#[test]
fn native_intake_original_settlement_is_atomic_and_receipt_only() {
    let h = Harness::new();
    migrate_fixture(&h);
    let header = header(&h);
    let db = h.store.db.as_ref().unwrap();
    let tx = db.unchecked_transaction().unwrap();
    reserve(&tx, &header).unwrap();
    tx.commit().unwrap();
    let result = json!({"task_exists":true,"task":{"uuid":header.task.uuid,"project":header.task.project},"team":{"id":TEAM,"queued":false}});
    assert!(settle(db, &header.intent, &result).is_err());
    {
        let tx = db.unchecked_transaction().unwrap();
        settle(&tx, &header.intent, &result).unwrap();
    }
    assert!(
        original(db, &header.intent).unwrap().result.is_none(),
        "a lost settlement never becomes an empty success"
    );
    let tx = db.unchecked_transaction().unwrap();
    settle(&tx, &header.intent, &result).unwrap();
    tx.commit().unwrap();
    for _ in 0..2 {
        assert_eq!(
            original(db, &header.intent).unwrap().result,
            Some(result.clone())
        );
    }
    let tx = db.unchecked_transaction().unwrap();
    assert!(settle(&tx, &header.intent, &json!({"task_exists":false})).is_err());
    drop(tx);
    assert_eq!(original(db, &header.intent).unwrap().result, Some(result));
    assert!(db
        .execute(
            "DELETE FROM person_action_originals WHERE request_id=?1",
            [&header.request.request_id]
        )
        .is_err());
}

#[test]
fn native_intake_unknown_blocks_another_original_for_the_same_task() {
    let h = Harness::new();
    migrate_fixture(&h);
    let first = header(&h);
    let db = h.store.db.as_ref().unwrap();
    let tx = db.unchecked_transaction().unwrap();
    reserve(&tx, &first).unwrap();
    tx.commit().unwrap();
    let mut next = header(&h);
    next.task = first.task.clone();
    next.request.operation = ops::Operation::AddTask {
        task: next.task.clone(),
        role: None,
        pipeline: None,
    };
    next.intent.request = wire::digest(&next.request).unwrap();
    let plan = wire::Plan {
        request_id: next.request.request_id.clone(),
        team: TEAM.into(),
        expected_revision: next.request.expected_revision,
        action: wire::Action::Intake {
            project: next.task.project.clone(),
            task: next.task.uuid.clone(),
            revision: "opaque reviewed revision".into(),
            role: None,
            pipeline: None,
        },
    };
    next.intent.plan = plan.reference().unwrap();
    let tx = db.unchecked_transaction().unwrap();
    assert!(reserve(&tx, &next).is_err());
    drop(tx);
    let count: u32 = db
        .query_row("SELECT count(*) FROM person_action_originals", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn native_intake_foreign_evidence_and_partial_schema_are_never_repaired() {
    let h = Harness::new();
    migrate_fixture(&h);
    let header = header(&h);
    let db = h.store.db.as_ref().unwrap();
    let tx = db.unchecked_transaction().unwrap();
    reserve(&tx, &header).unwrap();
    tx.commit().unwrap();
    for field in ["facts", "request", "epoch"] {
        let mut intent = header.intent.clone();
        match field {
            "facts" => intent.facts = "d".repeat(64),
            "request" => intent.request = "e".repeat(64),
            _ => intent.scope.service_epoch = uuid::Uuid::new_v4().to_string(),
        }
        assert!(original(db, &intent).is_err());
    }
    db.execute_batch("DROP TRIGGER person_action_original_immutable")
        .unwrap();
    let tx = db.unchecked_transaction().unwrap();
    assert!(migrate(&tx).is_err());
    drop(tx);
    let count: u32 = db
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='person_action_original_immutable'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
    assert!(
        h.store.team(TEAM).is_err(),
        "damaged current evidence is unavailable"
    );
    let revision: u64 = db
        .query_row("SELECT revision FROM teams WHERE id=?1", [TEAM], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(revision, header.request.expected_revision);
}

#[test]
fn native_intake_reserved_uuid_cannot_be_spent_by_another_operation_actor() {
    let mut h = Harness::new();
    migrate_fixture(&h);
    let header = header(&h);
    let db = h.store.db.as_ref().unwrap();
    let tx = db.unchecked_transaction().unwrap();
    reserve(&tx, &header).unwrap();
    tx.commit().unwrap();
    let mut request = header.request.clone();
    request.operation = ops::Operation::OpenCard {
        kind: "launch_refused".into(),
        text: "A different original".into(),
        node: None,
        decisions: None,
        parameters: None,
    };
    let problem = ops::apply(&mut h.store, &producers::service(), &request, 1234).unwrap_err();
    assert_eq!(problem.code, code::DUPLICATE);
    assert_eq!(
        h.store.team(TEAM).unwrap().unwrap().revision,
        header.request.expected_revision
    );
    assert!(original(h.store.db.as_ref().unwrap(), &header.intent)
        .unwrap()
        .result
        .is_none());
}
