use super::*;
use crate::desktop::store::{Location, Store};
use rusqlite::TransactionBehavior;
use serde_json::json;
use zerus_client_core::desktop::creation_worktree::{self as wire, Intent, Phase, Verified};

fn original(configuration: &str, epoch: &str) -> Verified {
    let creation = uuid::Uuid::new_v4().to_string();
    let request = uuid::Uuid::new_v4().to_string();
    let value = json!({"id":creation,"creation_schema":1,"created":1,"revision":4,
        "cmd":"codex","tag":"example","host":"","state":"worktree","auto":true,
        "parent":"/srv","target":"/srv/repo","branch":"zerus-example",
        "destination":"/srv/repo-zerus-example","worktree_common":"/srv/repo/.git",
        "worktree_id":request,"worktree_receipts":true,
        "expected_machine":"22222222-2222-4222-8222-222222222222"});
    Intent {
        scope: wire::Scope {
            configuration: configuration.into(),
            service_epoch: epoch.into(),
        },
        creation_id: creation,
        worktree_request_id: request,
        revision: 4,
        original: value.to_string(),
    }
    .verify()
    .unwrap()
}
/// The original CLI receipt shape the owned effect records.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn created(plan: &wire::Plan) -> serde_json::Value {
    json!({"schema":1,"request_id":plan.worktree_request_id,
        "fingerprint":plan.request_fingerprint().unwrap(),"status":"created",
        "path":plan.destination,"common_dir":plan.common_dir,"branch":plan.branch,
        "effects_started":true,"started_at":1,"finished_at":2})
}
fn fixture() -> (tempfile::TempDir, Location, Store, String) {
    let temp = tempfile::tempdir().unwrap();
    let location = Location {
        root: temp.path().canonicalize().unwrap().join("state/desktop"),
        configuration: "a".repeat(64),
    };
    location.prepare().unwrap();
    let epoch = uuid::Uuid::new_v4().to_string();
    let store = Store::open(&location, &epoch).unwrap();
    (temp, location, store, epoch)
}

#[test]
fn exact_previous_store_upgrade_adds_only_the_independent_worktree_leaf() {
    assert_eq!(
        SCHEMA - 1,
        crate::desktop::delivery::SCHEMA,
        "rebase this test onto the actual merge-order predecessor"
    );
    let (_temp, location, store, _) = fixture();
    remove_fixture(&store.db);
    store
        .db
        .pragma_update(None, "user_version", SCHEMA - 1)
        .unwrap();
    let before: i64 = store
        .db
        .query_row(
            "SELECT count(*) FROM sqlite_schema WHERE type='table'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let clone_sql: String = store
        .db
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE name='desktop_creation_clones'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    drop(store);
    let store = Store::open(&location, &uuid::Uuid::new_v4().to_string()).unwrap();
    assert_eq!(
        store
            .db
            .query_row::<i64, _, _>("PRAGMA user_version", [], |r| r.get(0))
            .unwrap(),
        crate::desktop::store::SCHEMA
    );
    assert_eq!(
        store
            .db
            .query_row::<i64, _, _>(
                "SELECT count(*) FROM sqlite_schema WHERE type='table'",
                [],
                |r| r.get(0)
            )
            .unwrap(),
        // Its own two tables, then the later launch, termination outcome, task
        // message, task message input, deferred send, deferred answer and
        // compact-to-send leaves.
        before + 2 + 2 + 1 + 2 + 2 + 2 + 2 + 2 + 2
    );
    // The clone leaf's original table is byte-compatible and separate.
    assert_eq!(
        store
            .db
            .query_row::<String, _, _>(
                "SELECT sql FROM sqlite_schema WHERE name='desktop_creation_clones'",
                [],
                |r| r.get(0)
            )
            .unwrap(),
        clone_sql
    );
    assert_eq!(ledger::undrained(&store.db).unwrap(), 0);
}

#[test]
fn shared_resource_total_counts_task_cache_and_worktree_exactly_once() {
    use zerus_desktop_ledger::operations::{self as task, cache};
    let (_temp, location, mut store, epoch) = fixture();
    let writer = uuid::Uuid::new_v4().to_string();
    zerus_desktop_ledger::authorize(&mut store.db, &writer, &epoch).unwrap();
    let source: serde_json::Value = serde_json::from_str(include_str!(
        "../../../client/fixtures/desktop-operation-port-v1.json"
    ))
    .unwrap();
    let binding: task::Binding = serde_json::from_value(source["binding"].clone()).unwrap();
    let admitted = task::admit(
        &mut store.db,
        &task::Admission {
            admission_id: &uuid::Uuid::new_v4().to_string(),
            binding: &binding,
            raw_record: source["raw_record"].as_str().unwrap().as_bytes(),
            retained_input: source["retained_input"].as_str().unwrap().as_bytes(),
            provenance: &json!({"fixture":"original-resource-accounting"}),
            writer_epoch: &writer,
            service_epoch: &epoch,
        },
    )
    .unwrap();
    task::reserve(
        &mut store.db,
        &task::Reserve {
            operation_id: admitted.operation_id,
            attempt_id: uuid::Uuid::new_v4().to_string(),
            revision: admitted.revision,
            writer_epoch: writer.clone(),
            service_epoch: epoch.clone(),
            lease: uuid::Uuid::new_v4().to_string(),
        },
    )
    .unwrap();
    assert_eq!(store.operation_undrained().unwrap(), 1);
    let tx = store.db.transaction().unwrap();
    cache::begin(
        &tx,
        &cache::Original {
            preparation_id: uuid::Uuid::new_v4().to_string(),
            operation_id: uuid::Uuid::new_v4().to_string(),
            configuration: location.configuration.clone(),
            root: "/srv/example/cache".into(),
            limit: 1,
            defer_ms: 0,
            policy_change: true,
            revision: 0,
            service_epoch: epoch.clone(),
            writer_epoch: writer.clone(),
            lease: uuid::Uuid::new_v4().to_string(),
        },
        &"f".repeat(64),
    )
    .unwrap();
    tx.commit().unwrap();
    assert_eq!(store.operation_undrained().unwrap(), 2);
    let tx = store.db.transaction().unwrap();
    ledger::admit(&tx, &original(&location.configuration, &epoch), &writer, 1).unwrap();
    tx.commit().unwrap();
    assert_eq!(task::totals(&store.db).unwrap().undrained, 1);
    assert_eq!(cache::undrained(&store.db).unwrap(), 1);
    assert_eq!(ledger::undrained(&store.db).unwrap(), 1);
    assert_eq!(
        crate::desktop::store::operation_undrained(&store.db).unwrap(),
        3
    );
    assert_eq!(store.operation_undrained().unwrap(), 3);
    assert_eq!(store.operation_status()["undrained"], 3);
    assert_eq!(store.operation_status()["cache_undrained"], 1);
    // Revoked policy alone cannot hide any original per-kind resource.
    store
        .db
        .execute("UPDATE desktop_writer SET revoked=1 WHERE id=1", [])
        .unwrap();
    assert_eq!(
        crate::desktop::store::writer_active(&location).unwrap(),
        Some(true)
    );
}

#[test]
fn admission_is_immutable_and_same_id_returns_only_its_original_receipt() {
    let (_temp, location, mut store, epoch) = fixture();
    let verified = original(&location.configuration, &epoch);
    let writer = uuid::Uuid::new_v4().to_string();
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    let row = ledger::admit(&tx, &verified, &writer, 1).unwrap();
    assert_eq!(row.receipt.phase, Phase::Queued);
    tx.commit().unwrap();
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    let again = ledger::admit(&tx, &verified, &writer, 2).unwrap();
    assert_eq!(again.receipt, row.receipt);
    tx.commit().unwrap();
    assert_eq!(ledger::undrained(&store.db).unwrap(), 1);
    assert_eq!(store.operation_undrained().unwrap(), 1);
    assert_eq!(store.operation_status()["undrained"], 1);
    assert_eq!(
        crate::desktop::store::writer_active(&location).unwrap(),
        Some(true)
    );
    assert!(store
        .db
        .execute("UPDATE desktop_creation_worktrees SET source='{}'", [])
        .is_err());
    assert!(store
        .db
        .execute("DELETE FROM desktop_creation_worktrees", [])
        .is_err());
    let mut intent = verified.intent;
    let mut value: serde_json::Value = serde_json::from_str(&intent.original).unwrap();
    value["opaque_future"] = json!("changed original");
    intent.original = value.to_string();
    let changed = intent.verify().unwrap();
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    assert!(ledger::admit(&tx, &changed, &writer, 3).is_err());
    drop(tx);
    assert_eq!(
        ledger::load(&store.db, &changed.plan.worktree_request_id)
            .unwrap()
            .unwrap()
            .receipt
            .binding,
        row.receipt.binding
    );
}

fn retry(original: &Verified) -> Verified {
    let mut intent = original.intent.clone();
    intent.worktree_request_id = uuid::Uuid::new_v4().to_string();
    intent.revision += 1;
    let mut value: serde_json::Value = serde_json::from_str(&intent.original).unwrap();
    value["worktree_id"] = json!(intent.worktree_request_id);
    value["revision"] = json!(intent.revision);
    value["worktree_previous"] = json!([original.plan.worktree_request_id]);
    intent.original = value.to_string();
    intent.verify().unwrap()
}
fn failed_fixture(
    db: &mut rusqlite::Connection,
    original: &Verified,
    effects: serde_json::Value,
) -> wire::Receipt {
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    let mut row = ledger::admit(&tx, original, &uuid::Uuid::new_v4().to_string(), 1).unwrap();
    // A storage fixture only: the actual-actor control below supplies the
    // independent positive for real custody retirement.
    row.receipt.phase = Phase::Failed;
    row.receipt.drained = true;
    let mut result = json!({"schema":1,"request_id":original.plan.worktree_request_id,
        "fingerprint":original.plan.request_fingerprint().unwrap(),"status":"failed",
        "detail":"fixture refused before checkout"});
    if !effects.is_null() {
        result["effects_started"] = effects;
    }
    row.receipt.worktree = Some(result);
    tx.execute("UPDATE desktop_creation_worktrees SET receipt=?1,phase='failed',drained=1 WHERE worktree_id=?2",
        rusqlite::params![serde_json::to_string(&row.receipt).unwrap(),original.plan.worktree_request_id]).unwrap();
    tx.execute(
        "UPDATE desktop_creation_worktree_budget SET active=active-1 WHERE id=1",
        [],
    )
    .unwrap();
    tx.commit().unwrap();
    row.receipt
}
#[test]
fn retry_retains_the_original_and_reopen_cannot_resurrect_its_slot() {
    let (_temp, location, mut store, epoch) = fixture();
    let original = original(&location.configuration, &epoch);
    let receipt = failed_fixture(&mut store.db, &original, json!(false));
    let next = retry(&original);
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    ledger::admit(&tx, &next, &uuid::Uuid::new_v4().to_string(), 2).unwrap();
    tx.commit().unwrap();
    assert_eq!(
        ledger::load(&store.db, &original.plan.worktree_request_id)
            .unwrap()
            .unwrap()
            .receipt,
        receipt
    );
    assert_eq!(
        store
            .db
            .query_row::<String, _, _>(
                "SELECT creation_slot FROM desktop_creation_worktrees WHERE worktree_id=?1",
                [&original.plan.worktree_request_id],
                |r| r.get(0)
            )
            .unwrap(),
        format!("retired:{}", original.plan.worktree_request_id)
    );
    assert!(store
        .db
        .execute(
            "UPDATE desktop_creation_worktrees SET creation_id='changed'",
            []
        )
        .is_err());
    assert!(store
        .db
        .execute("DELETE FROM desktop_creation_worktrees", [])
        .is_err());
    drop(store);
    let mut store = Store::open(&location, &uuid::Uuid::new_v4().to_string()).unwrap();
    assert_eq!(
        ledger::load(&store.db, &original.plan.worktree_request_id)
            .unwrap()
            .unwrap()
            .receipt,
        receipt
    );
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    assert!(ledger::admit(&tx, &retry(&original), &uuid::Uuid::new_v4().to_string(), 3).is_err());
}
#[test]
fn retry_rolls_back_the_slot_and_all_budget_charges_after_a_late_insert_failure() {
    let (_temp, location, mut store, epoch) = fixture();
    let original = original(&location.configuration, &epoch);
    let receipt = failed_fixture(&mut store.db, &original, json!(false));
    let budget = |db: &rusqlite::Connection| {
        db.query_row::<(u64, u64, u64), _, _>(
            "SELECT rows,bytes,active FROM desktop_creation_worktree_budget",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap()
    };
    let before = budget(&store.db);
    store.db.execute_batch("CREATE TRIGGER fixture_insert_failure BEFORE INSERT ON desktop_creation_worktrees BEGIN SELECT RAISE(ABORT,'late fixture refusal'); END;").unwrap();
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    assert!(ledger::admit(&tx, &retry(&original), &uuid::Uuid::new_v4().to_string(), 2).is_err());
    drop(tx);
    assert_eq!(budget(&store.db), before);
    assert_eq!(
        ledger::load(&store.db, &original.plan.worktree_request_id)
            .unwrap()
            .unwrap()
            .receipt,
        receipt
    );
    assert_eq!(
        store
            .db
            .query_row::<String, _, _>(
                "SELECT creation_slot FROM desktop_creation_worktrees",
                [],
                |r| r.get(0)
            )
            .unwrap(),
        original.binding.creation_id
    );
}
#[test]
fn retry_requires_literal_no_effects_and_the_same_original_geometry() {
    for effects in [
        serde_json::Value::Null,
        json!("false"),
        json!(0),
        json!(true),
    ] {
        let (_temp, location, mut store, epoch) = fixture();
        let original = original(&location.configuration, &epoch);
        failed_fixture(&mut store.db, &original, effects);
        let tx = store
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        assert!(
            ledger::admit(&tx, &retry(&original), &uuid::Uuid::new_v4().to_string(), 2).is_err()
        );
    }
    for key in [
        "expected_machine",
        "target",
        "worktree_common",
        "destination",
        "branch",
    ] {
        let (_temp, location, mut store, epoch) = fixture();
        let original = original(&location.configuration, &epoch);
        failed_fixture(&mut store.db, &original, json!(false));
        let mut next = retry(&original).intent;
        let mut value: serde_json::Value = serde_json::from_str(&next.original).unwrap();
        value[key] = json!(match key {
            "expected_machine" => "33333333-3333-4333-8333-333333333333",
            "branch" => "another-branch",
            _ => "/other/path",
        });
        next.original = value.to_string();
        let tx = store
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        assert!(ledger::admit(
            &tx,
            &next.verify().unwrap(),
            &uuid::Uuid::new_v4().to_string(),
            2
        )
        .is_err());
    }
    for phase in [Phase::Unknown, Phase::Created, Phase::Running] {
        let (_temp, location, mut store, epoch) = fixture();
        let original = original(&location.configuration, &epoch);
        let mut receipt = failed_fixture(&mut store.db, &original, json!(false));
        receipt.phase = phase;
        if phase == Phase::Running {
            receipt.drained = false;
        }
        // A deliberately inconsistent record must refuse too; no migration or
        // load is allowed to turn it into a safe predecessor.
        store
            .db
            .execute(
                "UPDATE desktop_creation_worktrees SET receipt=?1 WHERE worktree_id=?2",
                rusqlite::params![
                    serde_json::to_string(&receipt).unwrap(),
                    original.plan.worktree_request_id
                ],
            )
            .unwrap();
        let tx = store
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        assert!(
            ledger::admit(&tx, &retry(&original), &uuid::Uuid::new_v4().to_string(), 2).is_err()
        );
    }
}
#[test]
fn malformed_failed_results_and_concurrent_attempts_never_share_a_slot() {
    for (field, value) in [
        ("schema", json!(2)),
        ("detail", json!(null)),
        ("resource_error", json!("unknown resource")),
        ("fingerprint", json!("0".repeat(64))),
    ] {
        let (_temp, location, mut store, epoch) = fixture();
        let original = original(&location.configuration, &epoch);
        let mut receipt = failed_fixture(&mut store.db, &original, json!(false));
        receipt.worktree.as_mut().unwrap()[field] = value;
        store
            .db
            .execute(
                "UPDATE desktop_creation_worktrees SET receipt=?1 WHERE worktree_id=?2",
                rusqlite::params![
                    serde_json::to_string(&receipt).unwrap(),
                    original.plan.worktree_request_id
                ],
            )
            .unwrap();
        let tx = store
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        assert!(
            ledger::admit(&tx, &retry(&original), &uuid::Uuid::new_v4().to_string(), 2).is_err(),
            "{field}"
        );
    }
    let (_temp, location, mut store, epoch) = fixture();
    let original = original(&location.configuration, &epoch);
    let old = failed_fixture(&mut store.db, &original, json!(false));
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let mut threads = Vec::new();
    for _ in 0..2 {
        let path = location.root.join("state.sqlite3");
        let intent = retry(&original).intent;
        let barrier = barrier.clone();
        threads.push(std::thread::spawn(move || {
            let mut db = rusqlite::Connection::open_with_flags(
                path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
                    | rusqlite::OpenFlags::SQLITE_OPEN_NOFOLLOW,
            )
            .unwrap();
            db.busy_timeout(std::time::Duration::from_secs(2)).unwrap();
            barrier.wait();
            let tx = db
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .unwrap();
            if ledger::admit(
                &tx,
                &intent.verify().unwrap(),
                &uuid::Uuid::new_v4().to_string(),
                2,
            )
            .is_ok()
            {
                tx.commit().unwrap();
                true
            } else {
                false
            }
        }));
    }
    assert_eq!(
        threads
            .into_iter()
            .map(|t| usize::from(t.join().unwrap()))
            .sum::<usize>(),
        1
    );
    assert_eq!(
        ledger::load(&store.db, &original.plan.worktree_request_id)
            .unwrap()
            .unwrap()
            .receipt,
        old
    );
    assert_eq!(ledger::undrained(&store.db).unwrap(), 1);
}

#[test]
fn slots_upgrade_preserves_the_exact_old_receipt_and_immutable_identity() {
    let (_temp, location, mut store, epoch) = fixture();
    let original = original(&location.configuration, &epoch);
    let receipt = failed_fixture(&mut store.db, &original, json!(false));
    rewind_slots(&store.db);
    store
        .db
        .pragma_update(None, "user_version", SLOTS_SCHEMA - 1)
        .unwrap();
    drop(store);
    let mut store = Store::open(&location, &uuid::Uuid::new_v4().to_string()).unwrap();
    assert_eq!(
        ledger::load(&store.db, &original.plan.worktree_request_id)
            .unwrap()
            .unwrap()
            .receipt,
        receipt
    );
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    ledger::admit(&tx, &retry(&original), &uuid::Uuid::new_v4().to_string(), 2).unwrap();
    tx.commit().unwrap();
    assert!(store
        .db
        .execute("UPDATE desktop_creation_worktrees SET source='{}'", [])
        .is_err());
}

#[test]
fn capacity_refusal_is_atomic_and_retirement_never_rearms_a_queued_original() {
    let (_temp, location, mut store, epoch) = fixture();
    let verified = original(&location.configuration, &epoch);
    let writer = uuid::Uuid::new_v4().to_string();
    store
        .db
        .execute(
            "UPDATE desktop_creation_worktree_budget SET bytes=16777216",
            [],
        )
        .unwrap();
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    assert!(ledger::admit(&tx, &verified, &writer, 1).is_err());
    drop(tx);
    assert!(ledger::load(&store.db, &verified.plan.worktree_request_id)
        .unwrap()
        .is_none());
    assert_eq!(ledger::undrained(&store.db).unwrap(), 0);
    store
        .db
        .execute("UPDATE desktop_creation_worktree_budget SET bytes=0", [])
        .unwrap();
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    let row = ledger::admit(&tx, &verified, &writer, 1).unwrap();
    tx.commit().unwrap();
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    ledger::retire(&tx, &row).unwrap();
    tx.commit().unwrap();
    assert_eq!(ledger::undrained(&store.db).unwrap(), 0);
    assert!(ledger::pending(&store.db).unwrap().is_none());
    assert_eq!(store.operation_undrained().unwrap(), 0);
    assert_eq!(
        crate::desktop::store::writer_active(&location).unwrap(),
        Some(false)
    );
    let retired = ledger::load(&store.db, &verified.plan.worktree_request_id)
        .unwrap()
        .unwrap();
    assert_eq!(retired.receipt.phase, Phase::Unknown);
    assert!(retired.receipt.drained);
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    let replay = ledger::admit(&tx, &verified, &writer, 2).unwrap();
    assert_eq!(replay.receipt, retired.receipt);
    assert_eq!(ledger::undrained(&tx).unwrap(), 0);
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn only_the_original_acquired_custody_lock_settles_a_worktree_result() {
    use crate::desktop::custody;
    use zerus_desktop_ledger::operations::{custody::Record, Ticket};
    let (_temp, location, mut store, epoch) = fixture();
    let verified = original(&location.configuration, &epoch);
    let writer = uuid::Uuid::new_v4().to_string();
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    let row = ledger::admit(&tx, &verified, &writer, 1).unwrap();
    tx.commit().unwrap();
    let ticket = Ticket {
        operation_id: verified.binding.operation_id().unwrap(),
        attempt_id: uuid::Uuid::new_v4().to_string(),
        revision: 2,
        admission_id: verified.plan.worktree_request_id.clone(),
        binding_sha256: verified.binding.fingerprint().unwrap(),
        writer_epoch: writer,
        service_epoch: epoch,
        lease: uuid::Uuid::new_v4().to_string(),
    };
    // This is a storage/kernel custody control, not native execution admission.
    let (prepared, held) = custody::prepare(&location, &ticket).unwrap();
    let record = Record {
        prepared,
        helper: None,
    };
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    ledger::reserve(&tx, &row, &ticket, &record).unwrap();
    tx.commit().unwrap();
    let row = ledger::pending(&store.db).unwrap().unwrap();
    assert_eq!(row.receipt.phase, Phase::Reserved);
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    ledger::running(&tx, &row).unwrap();
    tx.commit().unwrap();
    let row = ledger::pending(&store.db).unwrap().unwrap();
    let worktree = created(&verified.plan);
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    ledger::result(&tx, &row, Some(worktree), "awaiting_resource_drain").unwrap();
    tx.commit().unwrap();
    let row = ledger::pending(&store.db).unwrap().unwrap();
    assert!(row.receipt.result(&verified.plan).is_none());
    let observation = custody::probe(&location, &record);
    assert!(matches!(observation, custody::Observation::Held));
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    assert!(ledger::drained(tx, row, observation).is_err());
    assert_eq!(ledger::undrained(&store.db).unwrap(), 1);
    drop(held);
    let row = ledger::pending(&store.db).unwrap().unwrap();
    let observation = custody::probe(&location, &record);
    assert!(matches!(observation, custody::Observation::Released { .. }));
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    let settled = ledger::drained(tx, row, observation).unwrap();
    assert_eq!(settled.phase, Phase::Created);
    assert_eq!(
        settled.result(&verified.plan).unwrap().unwrap()["status"],
        "created"
    );
    assert_eq!(ledger::undrained(&store.db).unwrap(), 0);
    assert!(ledger::pending(&store.db).unwrap().is_none());
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    let old = ledger::admit(&tx, &verified, &uuid::Uuid::new_v4().to_string(), 2).unwrap();
    assert_eq!(old.receipt, settled);
    assert!(ledger::reserve(&tx, &old, &ticket, &record).is_err());
}

/// Recovery stores the original receipt it reads before recording drain.
/// An original over the wire bound is stored as its bounded projection, or as
/// Unknown when even that does not fit; storage never fails on size, and the
/// released charge always drains.
#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn an_oversized_original_is_stored_bounded_and_its_released_charge_drains() {
    use crate::desktop::custody;
    use zerus_desktop_ledger::operations::{custody::Record, Ticket};
    for unrepresentable in [false, true] {
        let (_temp, location, mut store, epoch) = fixture();
        let mut intent = original(&location.configuration, &epoch).intent;
        let mut value: serde_json::Value = serde_json::from_str(&intent.original).unwrap();
        let long = |base: &str| {
            let mut path = String::from(base);
            while path.len() < 3400 {
                path.push_str("/segment-of-a-long-but-valid-worktree-path");
            }
            path
        };
        let target = long("/srv/repo");
        value["target"] = json!(target);
        value["worktree_common"] = json!(format!("{target}/.git"));
        value["destination"] = json!(long("/srv/repo-zerus-example"));
        intent.original = value.to_string();
        let verified = intent.verify().unwrap();
        let writer = uuid::Uuid::new_v4().to_string();
        let tx = store
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let row = ledger::admit(&tx, &verified, &writer, 1).unwrap();
        tx.commit().unwrap();
        let ticket = Ticket {
            operation_id: verified.binding.operation_id().unwrap(),
            attempt_id: uuid::Uuid::new_v4().to_string(),
            revision: 2,
            admission_id: verified.plan.worktree_request_id.clone(),
            binding_sha256: verified.binding.fingerprint().unwrap(),
            writer_epoch: writer,
            service_epoch: epoch,
            lease: uuid::Uuid::new_v4().to_string(),
        };
        let (prepared, held) = custody::prepare(&location, &ticket).unwrap();
        let record = Record {
            prepared,
            helper: None,
        };
        let tx = store
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        ledger::reserve(&tx, &row, &ticket, &record).unwrap();
        tx.commit().unwrap();
        let row = ledger::pending(&store.db).unwrap().unwrap();
        let tx = store
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        ledger::running(&tx, &row).unwrap();
        tx.commit().unwrap();
        let row = ledger::pending(&store.db).unwrap().unwrap();
        let plan = &verified.plan;
        let mut worktree = created(plan);
        worktree["creation_owner"] = json!({"version":1,
            "id":"44444444-4444-4444-8444-444444444444","common_dir":plan.common_dir,
            "path":plan.destination,"branch":plan.branch,"head":"a".repeat(40),
            "metadata":format!("{}/worktrees/-",plan.common_dir),"created_at":1});
        if unrepresentable {
            worktree["path"] = json!("\"".repeat(9000));
        }
        assert!(serde_json::to_vec(&worktree).unwrap().len() > wire::RESULT_BYTES);
        let tx = store
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        ledger::result(&tx, &row, Some(worktree), "awaiting_resource_drain").unwrap();
        tx.commit().unwrap();
        drop(held);
        let row = ledger::pending(&store.db).unwrap().unwrap();
        let observation = custody::probe(&location, &record);
        assert!(matches!(observation, custody::Observation::Released { .. }));
        let tx = store
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let settled = ledger::drained(tx, row, observation).unwrap();
        if unrepresentable {
            assert_eq!(settled.phase, Phase::Unknown);
            assert_eq!(settled.code, ledger::UNREPRESENTABLE);
        } else {
            assert_eq!(settled.phase, Phase::Created);
            assert!(settled.result(plan).unwrap().is_ok());
        }
        assert_eq!(ledger::undrained(&store.db).unwrap(), 0);
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod actor {
    use super::*;
    use crate::{config::Config, desktop::ui_authority, state::WorktreeStep as Step};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use std::time::{Duration, Instant};

    struct Fixture {
        _temp: tempfile::TempDir,
        location: Location,
        store: Store,
        runtime: runtime::Runtime,
        exclusion: Arc<crate::desktop::store::HandoffLock>,
        epoch: String,
        config: Config,
        owner: Arc<crate::platform::FileLock>,
    }
    impl Fixture {
        fn new(
            execute: impl Fn(
                    &wire::Plan,
                    &crate::state::WorktreeControl<'_>,
                ) -> Result<serde_json::Value, String>
                + Send
                + Sync
                + 'static,
        ) -> Self {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().canonicalize().unwrap();
            let config = Config {
                home: root.to_str().unwrap().into(),
                dir: root.join("config"),
                state: root.join("state"),
                selves: vec![],
                peers: vec![],
                base_peers: vec![],
                peers_from_environment: false,
                machines: Default::default(),
                tab: false,
                tab_colors: String::new(),
            };
            std::fs::create_dir(&config.dir).unwrap();
            let location = Location::new(&config).unwrap();
            location.prepare().unwrap();
            let epoch = uuid::Uuid::new_v4().to_string();
            let mut store = Store::open(&location, &epoch).unwrap();
            zerus_desktop_ledger::authorize(
                &mut store.db,
                &uuid::Uuid::new_v4().to_string(),
                &epoch,
            )
            .unwrap();
            let owner = Arc::new(location.lock().unwrap());
            let exclusion = Arc::new(location.handoff_lock().unwrap());
            let mut runtime = runtime::Runtime::new(&config, &epoch, owner.clone()).unwrap();
            // This synthetic source/Validated seam tests the actor, not native
            // authentication or an installed frontend authority positive.
            runtime.fixture_executor("22222222-2222-4222-8222-222222222222", Arc::new(execute));
            runtime.poll(&mut store, Some(exclusion.clone()));
            Self {
                _temp: temp,
                location,
                store,
                runtime,
                exclusion,
                epoch,
                config,
                owner,
            }
        }
        fn original(&self) -> Verified {
            let mut intent = super::original(&self.location.configuration, &self.epoch).intent;
            let mut value: serde_json::Value = serde_json::from_str(&intent.original).unwrap();
            let parent = self._temp.path().canonicalize().unwrap();
            value["parent"] = json!(parent);
            value["target"] = json!(parent.join("repo"));
            value["worktree_common"] = json!(parent.join("repo/.git"));
            value["destination"] = json!(parent.join("repo-zerus-example"));
            intent.original = value.to_string();
            intent.verify().unwrap()
        }
        /// The same original at other (synthetic) paths.
        fn original_at(&self, target: &str, destination: &str) -> Verified {
            let mut intent = super::original(&self.location.configuration, &self.epoch).intent;
            let mut value: serde_json::Value = serde_json::from_str(&intent.original).unwrap();
            value["parent"] = json!(std::path::Path::new(destination).parent().unwrap());
            value["target"] = json!(target);
            value["worktree_common"] = json!(format!("{target}/.git"));
            value["destination"] = json!(destination);
            intent.original = value.to_string();
            intent.verify().unwrap()
        }
        fn admit(&mut self, original: &Verified) -> wire::Admission {
            let request = zerus_client_core::desktop::ui_requests::Request::CreationWorktree {
                intent: Box::new(original.intent.clone()),
            };
            serde_json::from_value(self.runtime.frontend(
                &mut self.store,
                ui_authority::Validated::fixture(&json!(request)),
                Some(self.exclusion.clone()),
            ))
            .unwrap()
        }
        fn until(&mut self, what: &str, done: impl Fn(&Self) -> bool) {
            let until = Instant::now() + Duration::from_secs(8);
            while !done(self) {
                assert!(
                    Instant::now() < until,
                    "timed out: {what}, {}",
                    self.runtime.status()
                );
                self.runtime
                    .poll(&mut self.store, Some(self.exclusion.clone()));
                std::thread::sleep(Duration::from_millis(2));
            }
        }
        fn row(&self, original: &Verified) -> ledger::Row {
            ledger::load(&self.store.db, &original.plan.worktree_request_id)
                .unwrap()
                .unwrap()
        }
        fn revoke(&mut self) {
            self.store
                .db
                .execute("UPDATE desktop_writer SET revoked=1 WHERE id=1", [])
                .unwrap();
            self.runtime
                .poll(&mut self.store, Some(self.exclusion.clone()));
        }
    }
    #[test]
    fn a_typed_slot_refusal_keeps_the_original_native_charge_and_reason() {
        let mut f = Fixture::new(|plan, control| {
            (control.before)(Step::Reserve)?;
            Ok(json!({"schema":1,"request_id":plan.worktree_request_id,
                "fingerprint":plan.request_fingerprint().unwrap(),"status":"failed",
                "effects_started":false,"detail":"fixture pre-effect failure"}))
        });
        let first = f.original();
        assert!(f.admit(&first).accepted);
        let next = retry(&first);
        let refused = f.admit(&next);
        assert!(!refused.accepted);
        assert_eq!(refused.code, "creation_worktree_predecessor_unsettled");
        assert!(refused
            .refusal(&next.intent)
            .unwrap()
            .contains("previous attempt"));
        assert_eq!(ledger::undrained(&f.store.db).unwrap(), 1);
        assert!(ledger::load(&f.store.db, &next.plan.worktree_request_id)
            .unwrap()
            .is_none());
        f.until("original physical drain", |f| f.row(&first).receipt.drained);
        assert!(f.admit(&next).accepted);
        f.until("new original physical drain", |f| {
            f.row(&next).receipt.drained
        });
    }

    #[test]
    fn a_legacy_unknown_card_stops_through_the_real_reducer_rpc_and_receipt_lane() {
        use zerus_client_core::creation::{
            self, Answer, Call, Creations, Effect, Journal, Placement,
        };
        struct Places;
        impl Placement for Places {
            fn resolve(
                &mut self,
                _: &mut serde_json::Value,
                _: &serde_json::Value,
            ) -> Result<(), String> {
                Ok(())
            }
        }
        // Use the maintained reducer and its actual ordered durable-write
        // barriers, not an extracted transition or a synthesized stopped row.
        fn flush(
            queue: &mut Creations,
            file: &std::path::Path,
            effects: Vec<Effect>,
        ) -> Vec<(u64, Call)> {
            let mut pending: std::collections::VecDeque<_> = effects.into();
            let mut calls = Vec::new();
            while let Some(effect) = pending.pop_front() {
                match effect {
                    Effect::Write { write, bytes, .. } => {
                        let ok =
                            zerus_client_core::fsx::write_private_durable(file, &bytes).is_ok();
                        assert!(ok);
                        pending.extend(queue.written(write, ok));
                    }
                    Effect::Call { flight, call } => calls.push((flight, call)),
                    Effect::Launch { .. } => panic!("Stop must not launch"),
                    _ => (),
                }
            }
            calls
        }
        let mut f = Fixture::new(|plan, control| {
            (control.before)(Step::Reserve)?;
            Ok(json!({"schema":1,"request_id":plan.worktree_request_id,
                "fingerprint":plan.request_fingerprint().unwrap(),"status":"failed",
                "effects_started":false,"detail":"fixture pre-effect failure"}))
        });
        let first = f.original();
        assert!(f.admit(&first).accepted);
        f.until("first original physical drain", |f| {
            f.row(&first).receipt.drained
        });
        let old_receipt = f.row(&first).receipt;
        let requests = f.config.state.join("worktree-requests");
        crate::platform::private_dir(&requests).unwrap();
        let old_path = requests.join(format!("{}.json", first.plan.worktree_request_id));
        let old_bytes = serde_json::to_vec(old_receipt.worktree.as_ref().unwrap()).unwrap();
        zerus_client_core::fsx::write_private_durable(&old_path, &old_bytes).unwrap();
        let current = retry(&first);
        assert!(ledger::load(&f.store.db, &current.plan.worktree_request_id)
            .unwrap()
            .is_none());
        let id = current.binding.creation_id.clone();
        let mut operation: serde_json::Value =
            serde_json::from_str(&current.intent.original).unwrap();
        operation["state"] = json!("unknown");
        operation["stop_requested"] = json!(true);
        let original = json!({"schema":1,"operations":{id.clone():operation}});
        let journal = f.config.state.join("creation-fixture.json");
        let mut queue = Creations::new(|| 1000, || uuid::Uuid::new_v4().to_string());
        let effects = queue.load(Journal::Bytes(serde_json::to_vec(&original).unwrap()));
        assert!(flush(&mut queue, &journal, effects).is_empty());
        let effects = queue.stop(&id);
        let calls = flush(&mut queue, &journal, effects);
        assert_eq!(calls.len(), 1);
        let (flight, call) = &calls[0];
        let Call::WorktreeReceipt {
            action,
            request_id,
            fingerprint,
            ..
        } = call
        else {
            panic!("expected exact original Stop")
        };
        assert_eq!(action, "cancel");
        assert_eq!(request_id, &current.plan.worktree_request_id);
        assert_eq!(
            Some(fingerprint.clone()),
            current.plan.request_fingerprint()
        );
        let before_reply = std::fs::read(&journal).unwrap();
        let request = wire::ControlRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            scope: current.intent.scope.clone(),
            creation_id: id.clone(),
            plan: current.plan.clone(),
            action: wire::ControlAction::Cancel,
        };
        let result = f.runtime.receipt_control(
            &f.store,
            &json!({"op":"creation_worktree_receipt","request":request}),
            Some(f.exclusion.clone()),
        );
        let reply: wire::ControlReply = serde_json::from_value(result).unwrap();
        assert!(reply.matches(&request));
        let fence = reply.receipt.unwrap();
        assert!(wire::fenced(&fence, &current.plan));
        let effects = queue.answered(
            *flight,
            Answer {
                ok: true,
                data: fence.clone(),
                error: String::new(),
            },
            &mut Places,
        );
        assert!(flush(&mut queue, &journal, effects).is_empty());
        let stopped = queue.operation(&id).unwrap();
        assert_eq!(stopped["state"], "stopped");
        assert_eq!(stopped["worktree_id"], current.plan.worktree_request_id);
        assert_eq!(
            stopped["worktree_future_fence"],
            current.plan.worktree_request_id
        );
        assert!(stopped["error"]
            .as_str()
            .unwrap()
            .contains("outcome is unknown"));
        assert!(!creation::can_resume(stopped));
        assert!(queue.can_edit(&id));
        assert!(queue.resume(&id).is_empty());
        assert_eq!(f.row(&first).receipt, old_receipt);
        assert_eq!(std::fs::read(&old_path).unwrap(), old_bytes);
        assert_eq!(ledger::undrained(&f.store.db).unwrap(), 0);
        // Losing the Stop reply cannot lose the fence or cause a second write:
        // reloading the pre-reply journal recovers by STATUS only.
        let saved = before_reply;
        let fence_path = requests.join(format!("{}.json", current.plan.worktree_request_id));
        let bytes = std::fs::read(&fence_path).unwrap();
        let mut recovered = Creations::new(|| 1000, || uuid::Uuid::new_v4().to_string());
        let effects = recovered.load(Journal::Bytes(saved));
        assert!(flush(&mut recovered, &journal, effects).is_empty());
        let effects = recovered.check(&id);
        let calls = flush(&mut recovered, &journal, effects);
        assert_eq!(calls.len(), 1);
        let (recovery_flight, recovery_call) = &calls[0];
        assert!(
            matches!(recovery_call, Call::WorktreeReceipt { action, .. } if action == "status")
        );
        let mut read = request;
        read.request_id = uuid::Uuid::new_v4().to_string();
        read.action = wire::ControlAction::Status;
        let result = f.runtime.receipt_control(
            &f.store,
            &json!({"op":"creation_worktree_receipt","request":read}),
            Some(f.exclusion.clone()),
        );
        let reply: wire::ControlReply = serde_json::from_value(result).unwrap();
        assert!(reply.matches(&read));
        assert_eq!(reply.receipt, Some(fence.clone()));
        let effects = recovered.answered(
            *recovery_flight,
            Answer {
                ok: true,
                data: fence,
                error: String::new(),
            },
            &mut Places,
        );
        assert!(flush(&mut recovered, &journal, effects).is_empty());
        assert_eq!(recovered.operation(&id).unwrap()["state"], "stopped");
        assert_eq!(std::fs::read(&fence_path).unwrap(), bytes);
        assert!(!creation::can_resume(recovered.operation(&id).unwrap()));
        // A separate explicit creation is allowed; its admission still needs
        // its own native proof. The old unknown row and original receipt stay.
        let recipe = json!({"cmd":"codex","tag":"recreated-fixture","host":"","auto":true,
            "target":current.plan.path,"destination":current.plan.destination,"branch":current.plan.branch});
        let (new_id, effects) = recovered.submit(recipe);
        let new_id = new_id.unwrap();
        assert_ne!(new_id, id);
        let calls = flush(&mut recovered, &journal, effects);
        assert_eq!(calls.len(), 1);
        assert!(matches!(calls[0].1, Call::CreationDirectory { .. }));
        let effects = recovered.answered(calls[0].0, Answer { ok:true, data:json!({"path":current.plan.path,"creation_identity_v1":true,"computer_id":current.plan.expected_machine}), error:String::new() }, &mut Places);
        let calls = flush(&mut recovered, &journal, effects);
        assert_eq!(calls.len(), 1);
        assert!(matches!(calls[0].1, Call::Catalog { .. }));
        let effects = recovered.answered(calls[0].0, Answer { ok:true, data:json!({"state":"ok","common_dir":current.plan.common_dir,"selected_root":current.plan.path,"creation_receipts_v1":true}), error:String::new() }, &mut Places);
        let calls = flush(&mut recovered, &journal, effects);
        assert_eq!(calls.len(), 1);
        assert!(matches!(calls[0].1, Call::CreateWorktree { .. }));
        let value = recovered.operation(&new_id).unwrap();
        let fresh = wire::Intent {
            scope: current.intent.scope.clone(),
            creation_id: new_id.clone(),
            worktree_request_id: value["worktree_id"].as_str().unwrap().into(),
            revision: value["revision"].as_u64().unwrap(),
            original: creation::effect_checkpoint(value).unwrap(),
        }
        .verify()
        .unwrap();
        assert_ne!(
            fresh.plan.worktree_request_id,
            current.plan.worktree_request_id
        );
        assert!(f.admit(&fresh).accepted);
        f.until("new original physical drain", |f| {
            f.row(&fresh).receipt.drained
        });
        assert_eq!(
            recovered.operation(&id).unwrap()["worktree_id"],
            current.plan.worktree_request_id
        );
        assert_eq!(f.row(&first).receipt, old_receipt);
    }

    #[test]
    fn receipt_control_refuses_changed_scope_machine_and_the_original_native_root() {
        let mut f = Fixture::new(|_, _| Err("not executed".into()));
        let original = f.original();
        crate::platform::private_dir(&f.config.state.join("worktree-requests")).unwrap();
        let request = wire::ControlRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            scope: original.intent.scope.clone(),
            creation_id: original.binding.creation_id,
            plan: original.plan,
            action: wire::ControlAction::Cancel,
        };
        for changed in ["scope", "machine", "root"] {
            let mut value = json!({"op":"creation_worktree_receipt","request":request});
            match changed {
                "scope" => {
                    value["request"]["scope"]["service_epoch"] =
                        json!(uuid::Uuid::new_v4().to_string())
                }
                "machine" => {
                    value["request"]["plan"]["expected_machine"] =
                        json!(uuid::Uuid::new_v4().to_string())
                }
                _ => {
                    std::fs::rename(
                        &f.config.state,
                        f._temp.path().join("original-state-retained"),
                    )
                    .unwrap();
                    crate::platform::private_dir(&f.config.state).unwrap();
                    crate::platform::private_dir(&f.config.state.join("worktree-requests"))
                        .unwrap();
                }
            }
            let result = f
                .runtime
                .receipt_control(&f.store, &value, Some(f.exclusion.clone()));
            assert_eq!(result["accepted"], false, "{changed}");
        }
        assert!(!f
            .config
            .state
            .join("worktree-requests.identity.json")
            .exists());
    }

    #[test]
    fn a_fresh_attempt_after_a_drained_effect_free_failure_keeps_the_original_receipt() {
        let mut f = Fixture::new(|plan, control| {
            (control.before)(Step::Reserve)?;
            Ok(json!({"schema":1,"request_id":plan.worktree_request_id,
                "fingerprint":plan.request_fingerprint().unwrap(),"status":"failed",
                "effects_started":false,"detail":"synthetic refusal before checkout"}))
        });
        let first = f.original();
        assert!(f.admit(&first).accepted);
        f.until("first failure's actual resource drain", |f| {
            f.row(&first).receipt.drained
        });
        let retained = f.row(&first).receipt;
        assert_eq!(retained.phase, Phase::Failed);
        assert_eq!(
            retained.worktree.as_ref().unwrap()["effects_started"],
            false
        );
        let mut intent = first.intent.clone();
        let request = uuid::Uuid::new_v4().to_string();
        let mut value: serde_json::Value = serde_json::from_str(&intent.original).unwrap();
        value["worktree_previous"] = json!([first.plan.worktree_request_id]);
        value["worktree_id"] = json!(request);
        value["revision"] = json!(intent.revision + 1);
        intent.worktree_request_id = request;
        intent.revision += 1;
        intent.original = value.to_string();
        let next = intent.verify().unwrap();
        let admission = f.admit(&next);
        assert!(
            admission.accepted,
            "fresh attempt refused: {}",
            admission.code
        );
        assert_eq!(f.row(&first).receipt, retained);
        f.until("new attempt's actual resource drain", |f| {
            f.row(&next).receipt.drained
        });
        assert_eq!(f.row(&first).receipt, retained);
        let query = wire::Query::for_plan(
            first.intent.scope.clone(),
            first.binding.creation_id.clone(),
            &first.plan,
        )
        .unwrap();
        let reply: wire::Lookup = serde_json::from_value(super::query(
            &f.store,
            &json!({"op":"creation_worktree_result","query":query}),
        ))
        .unwrap();
        assert!(reply.matches(&query));
        assert_eq!(reply.receipt.unwrap(), retained);
    }
    #[test]
    fn fixed_admitted_plan_finishes_without_gui_and_waits_for_a_real_descendant_lock() {
        use std::process::{Command, Stdio};
        let effects = Arc::new(AtomicUsize::new(0));
        let count = effects.clone();
        let original_children = Arc::new(std::sync::Mutex::new(Vec::new()));
        let children = original_children.clone();
        let mut f = Fixture::new(move |plan, control| {
            for step in [
                Step::Reserve,
                Step::Destination,
                Step::Checkout,
                Step::Verify,
                Step::Track,
                Step::Own,
            ] {
                (control.before)(step)?;
                count.fetch_add(1, Ordering::SeqCst);
            }
            // A real private helper and its descendant retain the exact
            // close-only lock across exec. Keep the original wait owner too;
            // no orphan fixture is delegated to the container PID 1.
            let mut command = Command::new("python3");
            command.args(["-c", "import os,sys,time\npid=os.fork()\nif pid:\n os.waitpid(pid,0)\n os._exit(0)\nopen(sys.argv[1], 'w').close()\nuntil=time.monotonic()+10\nwhile not os.path.exists(sys.argv[2]) and time.monotonic()<until: time.sleep(.005)\nos._exit(0)"])
                .arg(std::path::Path::new(&plan.destination).with_file_name("descendant-held"))
                .arg(std::path::Path::new(&plan.destination).with_file_name("descendant-release"))
                .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
            let mut child = {
                let _spawn = crate::platform::spawn_turn();
                control
                    .custody
                    .inherit(&mut command)
                    .map_err(|e| e.to_string())?;
                command.spawn().map_err(|e| e.to_string())?
            };
            children.lock().unwrap().push(std::thread::spawn(move || {
                assert!(child.wait().unwrap().success())
            }));
            Ok(super::created(plan))
        });
        let original = f.original();
        assert!(f.admit(&original).matches(&original.intent));
        // There is deliberately no subsequent GUI proof/subscription.
        f.until("original result and descendant", |f| {
            f.row(&original).receipt.worktree.is_some()
                && f._temp.path().join("descendant-held").exists()
        });
        assert_eq!(effects.load(Ordering::SeqCst), 6);
        assert_eq!(f.row(&original).next_step, 6);
        assert!(!f.row(&original).receipt.drained);
        assert!(f.row(&original).receipt.result(&original.plan).is_none());
        assert_eq!(f.store.operation_undrained().unwrap(), 1);
        f.runtime.request_stop();
        for _ in 0..8 {
            f.runtime.poll(&mut f.store, Some(f.exclusion.clone()));
        }
        assert!(!f.runtime.drained());
        std::fs::write(f._temp.path().join("descendant-release"), b"").unwrap();
        f.until("original kernel drain", |f| f.runtime.drained());
        assert_eq!(f.row(&original).receipt.phase, Phase::Created);
        assert!(f.row(&original).receipt.result(&original.plan).is_some());
        assert_eq!(f.store.operation_undrained().unwrap(), 0);
        for child in original_children.lock().unwrap().drain(..) {
            child.join().unwrap();
        }
    }
    /// Valid paths of about 3.4 KiB: the full original receipt repeats them in
    /// its ownership record and exceeds the wire's result bound.
    #[test]
    fn a_long_path_original_settles_through_its_bounded_projection_and_drains() {
        for unrepresentable in [false, true] {
            let full = Arc::new(AtomicUsize::new(0));
            let size = full.clone();
            let mut f = Fixture::new(move |plan, control| {
                for step in [
                    Step::Reserve,
                    Step::Destination,
                    Step::Checkout,
                    Step::Verify,
                    Step::Track,
                    Step::Own,
                ] {
                    (control.before)(step)?;
                }
                let mut receipt = super::created(plan);
                receipt["creation_owner"] = json!({"version":1,
                    "id":"44444444-4444-4444-8444-444444444444","common_dir":plan.common_dir,
                    "path":plan.destination,"branch":plan.branch,"head":"a".repeat(40),
                    "metadata":format!("{}/worktrees/-",plan.common_dir),"created_at":1});
                if unrepresentable {
                    // Not this plan's checkout, and too large for any projection.
                    receipt["path"] = json!("\"".repeat(9000));
                }
                size.store(
                    serde_json::to_vec(&receipt).unwrap().len(),
                    Ordering::SeqCst,
                );
                Ok(receipt)
            });
            let root = f._temp.path().canonicalize().unwrap();
            let long = |name: &str| {
                let mut path = root.join(name).to_string_lossy().into_owned();
                while path.len() < 3400 {
                    path.push_str("/segment-of-a-long-but-valid-worktree-path");
                }
                path
            };
            let original = f.original_at(&long("repo"), &long("repo-zerus-example"));
            assert!(f.admit(&original).matches(&original.intent));
            f.until("original drain", |f| f.row(&original).receipt.drained);
            assert!(full.load(Ordering::SeqCst) > wire::RESULT_BYTES);
            let receipt = f.row(&original).receipt;
            if unrepresentable {
                assert_eq!(receipt.phase, Phase::Unknown);
                assert_eq!(receipt.code, ledger::UNREPRESENTABLE);
                assert!(receipt.worktree.is_none());
            } else {
                assert_eq!(receipt.phase, Phase::Created);
                let created = receipt.result(&original.plan).unwrap().unwrap();
                assert_eq!(created["path"], json!(original.plan.destination));
                assert!(receipt.worktree.unwrap().get("creation_owner").is_none());
            }
            f.runtime.request_stop();
            f.until("stopped runtime drain", |f| f.runtime.drained());
            assert_eq!(f.store.operation_undrained().unwrap(), 0);
        }
    }

    /// The real owned executor, through the actor's own step checks, for a
    /// repository without an inherited upstream.
    #[cfg(target_os = "linux")]
    #[test]
    fn real_owned_executor_creates_a_no_upstream_checkout_through_every_actor_step() {
        crate::self_exec::private_probe_with(
            "desktop::creation_worktree::tests::actor::real_git_no_upstream_probe",
            120,
            96,
        );
    }
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "run by real_owned_executor_creates_a_no_upstream_checkout_through_every_actor_step"]
    fn real_git_no_upstream_probe() {
        crate::self_exec::probe_child(
            "desktop::creation_worktree::tests::actor::real_git_no_upstream_probe",
        );
        let mut f = Fixture::new(|plan, control| {
            crate::state::repositories::with_direct_owned_git(|| {
                crate::state::create_worktree_owned(&plan.arguments(), control)
            })
        });
        let root = f._temp.path().canonicalize().unwrap();
        for args in [
            &["init", "-q", "-b", "main", "repo"][..],
            &[
                "-C",
                "repo",
                "-c",
                "user.name=fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "fixture",
            ][..],
        ] {
            let status = std::process::Command::new("git")
                .current_dir(&root)
                .args(args)
                .status()
                .unwrap();
            assert!(status.success(), "{args:?}");
        }
        let original = f.original();
        assert!(f.admit(&original).matches(&original.intent));
        f.until("original drain", |f| f.row(&original).receipt.drained);
        let row = f.row(&original);
        assert_eq!(row.receipt.phase, Phase::Created, "{:?}", row.receipt);
        assert_eq!(row.next_step, 6, "every step, Track included, was checked");
        assert!(row.receipt.result(&original.plan).unwrap().is_ok());
        assert!(root.join("repo-zerus-example/.git").is_file());
        f.runtime.request_stop();
        f.until("stopped runtime drain", |f| f.runtime.drained());
        assert_eq!(f.store.operation_undrained().unwrap(), 0);
    }

    #[test]
    fn revoke_between_steps_refuses_verify_and_new_admission_without_replaying_the_worktree() {
        let effects = Arc::new(AtomicUsize::new(0));
        let count = effects.clone();
        let mut f = Fixture::new(move |_, control| {
            for step in [Step::Reserve, Step::Destination, Step::Checkout] {
                (control.before)(step)?;
                count.fetch_add(1, Ordering::SeqCst);
            }
            let until = Instant::now() + Duration::from_secs(8);
            while !control.cancelled.load(Ordering::Acquire) && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(2));
            }
            assert!(control.cancelled.load(Ordering::Acquire));
            assert!((control.before)(Step::Verify).is_err());
            Err("original cancelled".into())
        });
        let original = f.original();
        assert!(f.admit(&original).accepted);
        f.until("three effects", |_| effects.load(Ordering::SeqCst) == 3);
        f.revoke();
        assert!(!f.admit(&f.original()).accepted);
        f.until("cancelled original custody", |f| {
            f.row(&original).receipt.drained
        });
        assert_eq!(f.row(&original).receipt.phase, Phase::Unknown);
        assert_eq!(f.row(&original).next_step, 3);
        let query = wire::Query::for_plan(
            original.intent.scope.clone(),
            original.intent.creation_id.clone(),
            &original.plan,
        )
        .unwrap();
        let result = super::super::query(
            &f.store,
            &json!({"op":"creation_worktree_result","query":query}),
        );
        let result: wire::Lookup = serde_json::from_value(result).unwrap();
        assert!(result.matches(&query));
        assert_eq!(result.receipt.unwrap().phase, Phase::Unknown);
        assert_eq!(effects.load(Ordering::SeqCst), 3);
    }
    #[test]
    fn restart_retires_a_queued_plan_and_never_reconstructs_execution_consent() {
        let mut f = Fixture::new(|_, _| panic!("recovered plan must not execute"));
        let original = f.original();
        assert!(f.admit(&original).accepted);
        f.runtime = runtime::Runtime::new(&f.config, &f.epoch, f.owner.clone()).unwrap();
        f.runtime.fixture_executor(
            "22222222-2222-4222-8222-222222222222",
            Arc::new(|_, _| panic!("restart must not execute")),
        );
        f.store = Store::open(&f.location, &f.epoch).unwrap();
        zerus_desktop_ledger::authorize(
            &mut f.store.db,
            &uuid::Uuid::new_v4().to_string(),
            &f.epoch,
        )
        .unwrap();
        for _ in 0..5 {
            f.runtime.poll(&mut f.store, Some(f.exclusion.clone()));
        }
        assert_eq!(ledger::undrained(&f.store.db).unwrap(), 0);
        let row = ledger::load(&f.store.db, &original.plan.worktree_request_id)
            .unwrap()
            .unwrap();
        assert_eq!(row.receipt.phase, Phase::Unknown);
        assert!(row.receipt.drained);
        // Even an exact new authenticated duplicate returns its old receipt;
        // it cannot create another live consent or reset the original phase.
        let before = row.receipt;
        let value = f.admit(&original);
        assert_eq!(value.receipt.unwrap(), before);
    }
    #[test]
    fn actor_step_checks_a_revoked_writer_without_worker_cancellation() {
        use std::sync::atomic::AtomicBool;
        let ready = Arc::new(AtomicBool::new(false));
        let resume = Arc::new(AtomicBool::new(false));
        let refused = Arc::new(AtomicBool::new(false));
        let (started, release, stopped) = (ready.clone(), resume.clone(), refused.clone());
        let mut f = Fixture::new(move |_, control| {
            (control.before)(Step::Reserve)?;
            (control.before)(Step::Destination)?;
            started.store(true, Ordering::Release);
            let until = Instant::now() + Duration::from_secs(8);
            while !release.load(Ordering::Acquire) {
                assert!(Instant::now() < until, "actor fixture was not released");
                std::thread::sleep(Duration::from_millis(2));
            }
            assert!(!control.cancelled.load(Ordering::Acquire));
            let result = (control.before)(Step::Checkout);
            assert!(!control.cancelled.load(Ordering::Acquire));
            stopped.store(result.is_err(), Ordering::Release);
            result?;
            Err("revoked actor incorrectly admitted Checkout".into())
        });
        let original = f.original();
        assert!(f.admit(&original).accepted);
        f.until("worker before Checkout", |_| ready.load(Ordering::Acquire));
        assert_eq!(f.row(&original).next_step, 2);
        f.store
            .db
            .execute("UPDATE desktop_writer SET revoked=1 WHERE id=1", [])
            .unwrap();
        resume.store(true, Ordering::Release);
        let until = Instant::now() + Duration::from_secs(8);
        while !refused.load(Ordering::Acquire) {
            assert!(Instant::now() < until, "actor accepted a revoked step");
            // Deliberately omit refresh: its cancellation must not hide a
            // missing check in the actual BEGIN IMMEDIATE step transaction.
            f.runtime
                .fixture_poll_steps_without_refresh(&mut f.store)
                .unwrap();
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(f.row(&original).next_step, 2);
        f.until("revoked step original drain", |f| {
            f.row(&original).receipt.drained
        });
        assert_eq!(f.row(&original).receipt.phase, Phase::Unknown);
    }
    #[test]
    fn effect_steps_are_monotone_and_cannot_skip_or_repeat_a_reservation() {
        let mut f = Fixture::new(|_, control| {
            assert!(
                (control.before)(Step::Checkout).is_err(),
                "cannot skip reservation"
            );
            (control.before)(Step::Reserve)?;
            assert!(
                (control.before)(Step::Reserve).is_err(),
                "cannot repeat reservation"
            );
            Err("fixed plan stopped".into())
        });
        let original = f.original();
        assert!(f.admit(&original).accepted);
        f.until("step protocol drain", |f| f.row(&original).receipt.drained);
        assert_eq!(f.row(&original).next_step, 1);
        assert_eq!(f.row(&original).receipt.phase, Phase::Unknown);
    }
}
