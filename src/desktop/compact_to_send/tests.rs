use super::ledger::Child;
use super::*;
use crate::desktop::store::{Location, Store};
use rusqlite::TransactionBehavior;
use serde_json::{json, Value};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use zerus_client_core::desktop::compact_to_send::{Intent, Settled, Step, Verified};
use zerus_client_core::operation_recovery::deferred_input::{self as checkpoint, Checkpoint};

const SESSION: &str = "claude/private-compact";
const RUN: &str = "run-1";
const CONVERSATION: &str = "conversation-1";
const MACHINE: &str = "22222222-2222-4222-8222-222222222222";

/// A saved version 2 compact-to-send checkpoint with both exact stdins the
/// client builds for it.
fn original(configuration: &str, epoch: &str) -> Verified {
    let text = "after compaction";
    let checkpoint = Checkpoint::reserve_compact_to_send(checkpoint::Intent {
        kind: checkpoint::Kind::CompactToSend,
        draft_revision: 3,
        message_sha256: checkpoint::message_sha256(text, Vec::<(&str, &str, &str, &[u8])>::new()),
        run_id: RUN.into(),
        conversation_id: CONVERSATION.into(),
        question: None,
        start: None,
        retry_of: None,
        consent: true,
    })
    .unwrap();
    let compaction = json!({"request_id": checkpoint.request_id, "expected_run_id": RUN,
        "expected_conversation_id": CONVERSATION});
    let stdin = json!({"request_id": checkpoint.send_request_id, "text": text, "attachments": [],
        "expected_run_id": RUN, "expected_conversation_id": CONVERSATION,
        "expected_compaction_id": checkpoint.request_id});
    Intent {
        scope: wire::Scope {
            configuration: configuration.into(),
            service_epoch: epoch.into(),
        },
        session: SESSION.into(),
        request_id: checkpoint.request_id.clone(),
        send_request_id: checkpoint.send_request_id.clone().unwrap(),
        checkpoint: serde_json::to_string(&checkpoint).unwrap(),
        compaction: compaction.to_string(),
        original: stdin.to_string(),
    }
    .verify()
    .unwrap()
}
/// The CLI's own compaction record of `plan`'s request with `status`, as
/// `hgs compact-context` writes it.
fn cli_record(plan: &wire::Plan, status: &str) -> Value {
    json!({"request_id": plan.compaction_request_id, "name": plan.session, "run_id": plan.run_id,
        "conversation_id": plan.conversation_id, "status": status, "at": 7.5})
}
fn mode(path: &Path, mode: u32) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}
/// Writes `body` atomically as the CLI's private compaction receipt of
/// `request` under the CLI state root `state`.
fn write_receipt(state: &Path, request: &str, body: &[u8]) -> PathBuf {
    let folder = state.join("compact_receipts");
    if !folder.exists() {
        std::fs::create_dir(&folder).unwrap();
    }
    mode(&folder, 0o700);
    let path = folder.join(format!("{request}.json"));
    let temp = folder.join(format!(".{request}.tmp"));
    std::fs::write(&temp, body).unwrap();
    mode(&temp, 0o600);
    std::fs::rename(&temp, &path).unwrap();
    path
}
/// The CLI's own session record of `name` under the state root `state`.
fn session_record(state: &Path, name: &str, agent: &str, run: &str, conversation: &str) {
    use sha2::Digest;
    let file = state.join(format!("{:x}.json", sha2::Sha256::digest(name.as_bytes())));
    let temp = state.join(".record.tmp");
    std::fs::write(
        &temp,
        json!({"name": name, "agent": agent, "run_id": run, "conversation_id": conversation})
            .to_string(),
    )
    .unwrap();
    mode(&temp, 0o600);
    std::fs::rename(&temp, file).unwrap();
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
fn tables(db: &rusqlite::Connection) -> i64 {
    db.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE type='table'",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn exact_previous_store_upgrade_adds_only_the_independent_compact_to_send_leaf() {
    assert_eq!(
        SCHEMA - 1,
        crate::desktop::deferred_answer::SCHEMA,
        "rebase this test onto the actual merge-order predecessor"
    );
    let (_temp, location, store, _) = fixture();
    remove_fixture(&store.db);
    store
        .db
        .pragma_update(None, "user_version", SCHEMA - 1)
        .unwrap();
    let before = tables(&store.db);
    drop(store);
    let store = Store::open(&location, &uuid::Uuid::new_v4().to_string()).unwrap();
    assert_eq!(
        store
            .db
            .query_row::<i64, _, _>("PRAGMA user_version", [], |r| r.get(0))
            .unwrap(),
        crate::desktop::store::SCHEMA
    );
    assert_eq!(tables(&store.db), before + 2);
    assert_eq!(ledger::undrained(&store.db).unwrap(), 0);
    assert_eq!(
        store.operation_status()["kinds"]["compact_to_send"]["undrained"],
        0
    );
}

fn admit(store: &mut Store, verified: &Verified, writer: &str) -> ledger::Row {
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    let row = ledger::admit(&tx, verified, writer, 1).unwrap();
    tx.commit().unwrap();
    row
}

#[test]
fn admission_is_immutable_and_the_same_operation_returns_only_its_original_receipt() {
    let (_temp, location, mut store, epoch) = fixture();
    let verified = original(&location.configuration, &epoch);
    let writer = uuid::Uuid::new_v4().to_string();
    let row = admit(&mut store, &verified, &writer);
    assert_eq!(row.receipt.compaction.step, Step::Queued);
    assert_eq!(row.receipt.send.step, Step::Queued);
    assert!(row.compaction.is_none() && row.send.is_none());
    // The same operation again: its original receipt, nothing new.
    let again = admit(&mut store, &verified, &writer);
    assert_eq!(again.receipt, row.receipt);
    assert_eq!(ledger::undrained(&store.db).unwrap(), 1);
    // Another send under the same compaction conflicts.
    let mut other = verified.intent.clone();
    let checkpoint: Checkpoint = serde_json::from_str(&other.checkpoint).unwrap();
    let send = uuid::Uuid::new_v4().to_string();
    let mut changed = checkpoint.clone();
    changed.send_request_id = Some(send.clone());
    other.send_request_id = send.clone();
    other.checkpoint = serde_json::to_string(&changed).unwrap();
    let mut stdin: Value = serde_json::from_str(&other.original).unwrap();
    stdin["request_id"] = json!(send);
    other.original = stdin.to_string();
    let conflicting = other.verify().unwrap();
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    assert!(ledger::admit(&tx, &conflicting, &writer, 2).is_err());
    drop(tx);
    // The original columns never change.
    assert!(store
        .db
        .execute(
            "UPDATE desktop_compact_to_sends SET source='{}' WHERE request_id=?1",
            [&verified.plan.compaction_request_id]
        )
        .is_err());
    assert!(store
        .db
        .execute("DELETE FROM desktop_compact_to_sends", [])
        .is_err());
}

/// desktop-u R1: only this ledger's own proof that a child never crossed its
/// reservation records it as never started; a retired operation never runs.
#[test]
fn retirement_proves_only_unreserved_children_never_started() {
    let (_temp, location, mut store, epoch) = fixture();
    let verified = original(&location.configuration, &epoch);
    let row = admit(&mut store, &verified, &uuid::Uuid::new_v4().to_string());
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    ledger::retire(&tx, &row).unwrap();
    tx.commit().unwrap();
    let retired = ledger::load(&store.db, row.id()).unwrap().unwrap();
    assert_eq!(retired.receipt.compaction.step, Step::NotStarted);
    assert_eq!(retired.receipt.send.step, Step::NotStarted);
    assert!(retired.receipt.drained);
    assert_eq!(retired.receipt.code, ledger::INTERRUPTED);
    assert_eq!(
        retired.receipt.result(&verified.plan),
        Some(Settled::NotStarted)
    );
    assert_eq!(ledger::undrained(&store.db).unwrap(), 0);
    // Nothing follows a drained operation.
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    assert!(ledger::retire(&tx, &retired).is_err());
}

/// desktop-u R1: the ledger writes only a receipt that follows the stored
/// one by one revision; a settled child is never overwritten.
#[test]
fn the_ledger_writes_only_a_following_receipt() {
    let (_temp, location, mut store, epoch) = fixture();
    let verified = original(&location.configuration, &epoch);
    let (_record, _held) = running(&mut store, &location, &verified, &epoch);
    let row = ledger::pending(&store.db).unwrap().unwrap();
    let write = |store: &mut Store, row: &ledger::Row, next: &wire::Receipt| {
        let tx = store
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let result = ledger::state(&tx, row, next);
        if result.is_ok() {
            tx.commit().unwrap();
        }
        result.is_ok()
    };
    let mut same = row.receipt.clone();
    same.revision += 2;
    assert!(!write(&mut store, &row, &same), "a skipped revision");
    let mut back = row.receipt.clone();
    back.revision += 1;
    back.compaction.step = Step::Queued;
    assert!(!write(&mut store, &row, &back), "back to Queued");
    // Settle the compaction, then try to overwrite it.
    let mut recorded = row.receipt.clone();
    recorded.revision += 1;
    recorded.compaction = wire::Component {
        step: Step::Recorded,
        code: String::new(),
        outcome: Some(wire::Compacted {
            request_id: verified.plan.compaction_request_id.clone(),
            status: wire::SUBMITTED.into(),
        }),
    };
    assert!(write(&mut store, &row, &recorded));
    let row = ledger::pending(&store.db).unwrap().unwrap();
    let mut downgraded = row.receipt.clone();
    downgraded.revision += 1;
    downgraded.compaction = wire::Component {
        step: Step::NotStarted,
        code: wire::NOT_STARTED.into(),
        outcome: None,
    };
    downgraded.send = downgraded.compaction.clone().map_outcome();
    assert!(
        !write(&mut store, &row, &downgraded),
        "a recorded compaction overwritten"
    );
}
trait MapOutcome {
    fn map_outcome(self) -> wire::Component<zerus_client_core::desktop::deferred_send::Outcome>;
}
impl MapOutcome for wire::Component<wire::Compacted> {
    fn map_outcome(self) -> wire::Component<zerus_client_core::desktop::deferred_send::Outcome> {
        wire::Component {
            step: self.step,
            code: self.code,
            outcome: None,
        }
    }
}

/// A Queued child was never reserved: its own valid reservation beside it
/// refuses the row.
#[test]
fn a_queued_child_with_a_reservation_never_loads() {
    let (_temp, location, mut store, epoch) = fixture();
    let verified = original(&location.configuration, &epoch);
    let writer = uuid::Uuid::new_v4().to_string();
    zerus_desktop_ledger::authorize(&mut store.db, &writer, &epoch).unwrap();
    let row = admit(&mut store, &verified, &writer);
    let ticket = zerus_desktop_ledger::operations::Ticket {
        operation_id: verified.binding.operation_id().unwrap(),
        attempt_id: uuid::Uuid::new_v4().to_string(),
        revision: row.receipt.revision + 1,
        admission_id: verified.plan.compaction_request_id.clone(),
        binding_sha256: verified.binding.fingerprint().unwrap(),
        writer_epoch: writer,
        service_epoch: epoch.clone(),
        lease: uuid::Uuid::new_v4().to_string(),
    };
    let (prepared, _held) = crate::desktop::custody::prepare(&location, &ticket).unwrap();
    let record = zerus_desktop_ledger::operations::custody::Record {
        prepared,
        helper: None,
    };
    store
        .db
        .execute(
            "UPDATE desktop_compact_to_sends SET compaction_ticket=?1,compaction_custody=?2",
            [
                serde_json::to_string(&ticket).unwrap(),
                serde_json::to_string(&record).unwrap(),
            ],
        )
        .unwrap();
    let error = ledger::load(&store.db, &verified.plan.compaction_request_id)
        .err()
        .unwrap();
    assert!(
        error.to_string().contains("reservation without its child"),
        "{error}"
    );
}

#[test]
fn capacity_refusal_is_atomic() {
    let (_temp, location, mut store, epoch) = fixture();
    store
        .db
        .execute(
            "UPDATE desktop_compact_to_send_budget SET active=32 WHERE id=1",
            [],
        )
        .unwrap();
    let verified = original(&location.configuration, &epoch);
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    assert!(ledger::admit(&tx, &verified, &uuid::Uuid::new_v4().to_string(), 1).is_err());
    drop(tx);
    let rows: i64 = store
        .db
        .query_row("SELECT count(*) FROM desktop_compact_to_sends", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(rows, 0);
}

/// Reserves the compaction under a real custody prepared for the row and
/// returns the exact record and its held lock.
fn running(
    store: &mut Store,
    location: &Location,
    verified: &Verified,
    epoch: &str,
) -> (
    zerus_desktop_ledger::operations::custody::Record,
    std::sync::Arc<zerus_client_core::run::custody::InheritedLock>,
) {
    let writer = uuid::Uuid::new_v4().to_string();
    zerus_desktop_ledger::authorize(&mut store.db, &writer, epoch).unwrap();
    let row = admit(store, verified, &writer);
    let ticket = zerus_desktop_ledger::operations::Ticket {
        operation_id: verified.binding.operation_id().unwrap(),
        attempt_id: uuid::Uuid::new_v4().to_string(),
        revision: row.receipt.revision + 1,
        admission_id: verified.plan.compaction_request_id.clone(),
        binding_sha256: verified.binding.fingerprint().unwrap(),
        writer_epoch: writer,
        service_epoch: epoch.into(),
        lease: uuid::Uuid::new_v4().to_string(),
    };
    let (prepared, held) = crate::desktop::custody::prepare(location, &ticket).unwrap();
    let record = zerus_desktop_ledger::operations::custody::Record {
        prepared,
        helper: None,
    };
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    ledger::reserve(&tx, &row, Child::Compaction, &ticket, &record).unwrap();
    tx.commit().unwrap();
    let row = ledger::load(&store.db, &verified.plan.compaction_request_id)
        .unwrap()
        .unwrap();
    assert_eq!(row.receipt.compaction.step, Step::Running);
    assert_eq!(row.draining(), Some(Child::Compaction));
    // The send can never be reserved while the compaction runs.
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    assert!(ledger::reserve(&tx, &row, Child::Send, &ticket, &record).is_err());
    drop(tx);
    (record, held)
}

/// Observes `record` until its release, within a bound.
fn released(
    location: &Location,
    record: &zerus_desktop_ledger::operations::custody::Record,
) -> crate::desktop::custody::Observation {
    use crate::desktop::custody::{probe, Observation};
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let observation = probe(location, record);
        if matches!(observation, Observation::Released { .. }) || std::time::Instant::now() >= until
        {
            return observation;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn only_the_released_custody_settles_the_compaction_and_only_its_exact_cli_record_decides() {
    use super::receipt::{Observed, MISSING, UNAVAILABLE};
    use crate::desktop::custody::{self, Observation};
    #[derive(Clone, Copy, Debug)]
    enum Ending {
        Exited,
        NotStarted,
        Unknown,
    }
    type Read = Box<dyn Fn(&wire::Plan) -> Observed>;
    let exact: fn(&wire::Plan) -> Observed = |plan| {
        Observed::Compacted(
            wire::Compacted::from_receipt(
                plan,
                cli_record(plan, wire::SUBMITTED).to_string().as_bytes(),
            )
            .unwrap(),
        )
    };
    let other: fn(&wire::Plan) -> Observed = |plan| {
        Observed::Compacted(wire::Compacted {
            request_id: plan.send_request_id.clone(),
            status: wire::SUBMITTED.into(),
        })
    };
    let unknown = |code: &'static str| -> Read { Box::new(move |_| Observed::Unknown(code)) };
    use Step::*;
    let cases: Vec<(Ending, Read, Step, &str, Settled)> = vec![
        // The exit proves nothing; only the CLI's own record decides.
        (
            Ending::Exited,
            Box::new(exact),
            Recorded,
            "",
            Settled::NotSent { compacted: true },
        ),
        (
            Ending::Unknown,
            Box::new(exact),
            Recorded,
            "",
            Settled::NotSent { compacted: true },
        ),
        (
            Ending::NotStarted,
            Box::new(exact),
            Recorded,
            "",
            Settled::NotSent { compacted: true },
        ),
        (
            Ending::Exited,
            unknown(MISSING),
            Unknown,
            MISSING,
            Settled::NotSent { compacted: false },
        ),
        (
            Ending::Exited,
            unknown(wire::RECEIPT_IN_PROGRESS),
            Unknown,
            wire::RECEIPT_IN_PROGRESS,
            Settled::NotSent { compacted: false },
        ),
        (
            Ending::Exited,
            unknown(wire::RECEIPT_UNCERTAIN),
            Unknown,
            wire::RECEIPT_UNCERTAIN,
            Settled::NotSent { compacted: false },
        ),
        (
            Ending::Exited,
            Box::new(other),
            Unknown,
            wire::RECEIPT_MISMATCH,
            Settled::NotSent { compacted: false },
        ),
        (
            Ending::Exited,
            unknown(UNAVAILABLE),
            Unknown,
            UNAVAILABLE,
            Settled::NotSent { compacted: false },
        ),
        // The closed pre-child result: never started only when no record
        // exists; then the send never follows either.
        (
            Ending::NotStarted,
            unknown(MISSING),
            NotStarted,
            wire::NOT_STARTED,
            Settled::NotStarted,
        ),
        (
            Ending::NotStarted,
            unknown(UNAVAILABLE),
            Unknown,
            UNAVAILABLE,
            Settled::NotSent { compacted: false },
        ),
        // The child's own reason stays unless its exact record exists.
        (
            Ending::Unknown,
            unknown(MISSING),
            Unknown,
            "compact_to_send_timed_out",
            Settled::NotSent { compacted: false },
        ),
    ];
    for (index, (ending, read, step, code, settled)) in cases.into_iter().enumerate() {
        let (_temp, location, mut store, epoch) = fixture();
        let verified = original(&location.configuration, &epoch);
        let (record, held) = running(&mut store, &location, &verified, &epoch);
        let row = ledger::pending(&store.db).unwrap().unwrap();
        let tx = store
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        match ending {
            Ending::Exited => ledger::ended(&tx, &row, ledger::AWAITING_DRAIN).unwrap(),
            Ending::NotStarted => ledger::not_started(&tx, &row).unwrap(),
            Ending::Unknown => ledger::ended(&tx, &row, "compact_to_send_timed_out").unwrap(),
        }
        tx.commit().unwrap();
        // Nothing settles before the drain, whatever the receipt says.
        let row = ledger::pending(&store.db).unwrap().unwrap();
        assert_eq!(row.receipt.compaction.step, Running, "{index}");
        assert!(row.receipt.result(&verified.plan).is_none());
        let observation = custody::probe(&location, &record);
        assert!(matches!(observation, Observation::Held));
        let tx = store
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        assert!(ledger::drained(
            tx,
            row,
            observation,
            &read(&verified.plan),
            Default::default(),
            true
        )
        .is_err());
        drop(held);
        let row = ledger::pending(&store.db).unwrap().unwrap();
        let observation = released(&location, &record);
        let tx = store
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let receipt = ledger::drained(
            tx,
            row,
            observation,
            &read(&verified.plan),
            Default::default(),
            true,
        )
        .unwrap();
        assert_eq!(
            (receipt.compaction.step, receipt.compaction.code.as_str()),
            (step, code),
            "{index}: {ending:?}"
        );
        // This slice never reserves the send.
        assert_eq!(receipt.send.step, NotStarted, "{index}");
        assert!(receipt.drained && receipt.valid(), "{index}");
        assert_eq!(receipt.result(&verified.plan), Some(settled), "{index}");
        let stored = ledger::load(&store.db, &verified.plan.compaction_request_id)
            .unwrap()
            .unwrap();
        assert_eq!(stored.receipt, receipt);
        assert!(stored.send.is_none(), "the send was never reserved");
        assert_eq!(ledger::undrained(&store.db).unwrap(), 0);
    }
}

/// A restart retires an operation whose compaction may run: the send is
/// proven never started from the ledger, the compaction keeps its
/// reservation for the read-only drain, and an unheld one drains Unknown
/// without reading any receipt.
#[test]
fn restart_keeps_a_reserved_compaction_for_its_drain_and_never_reads_an_unheld_receipt() {
    use super::receipt::{Observed, UNHELD};
    let (_temp, location, mut store, epoch) = fixture();
    let verified = original(&location.configuration, &epoch);
    let (record, held) = running(&mut store, &location, &verified, &epoch);
    let state = location.root.parent().unwrap().to_owned();
    // Even an exact receipt is never read for an unheld original.
    write_receipt(
        &state,
        &verified.plan.compaction_request_id,
        cli_record(&verified.plan, wire::SUBMITTED)
            .to_string()
            .as_bytes(),
    );
    ledger::startup(&mut store.db).unwrap();
    let row = ledger::pending(&store.db).unwrap().unwrap();
    assert_eq!(row.receipt.compaction.step, Step::Running);
    assert_eq!(row.receipt.send.step, Step::NotStarted);
    assert!(!row.receipt.drained);
    // A second restart changes nothing about either child.
    ledger::startup(&mut store.db).unwrap();
    let again = ledger::pending(&store.db).unwrap().unwrap();
    assert_eq!(
        (again.receipt.compaction.clone(), again.receipt.send.clone()),
        (row.receipt.compaction.clone(), row.receipt.send.clone())
    );
    drop(held);
    let observation = released(&location, &record);
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    let receipt = ledger::drained(
        tx,
        again,
        observation,
        &Observed::Unknown(UNHELD),
        Default::default(),
        true,
    )
    .unwrap();
    assert_eq!(
        (receipt.compaction.step, receipt.compaction.code.as_str()),
        (Step::Unknown, UNHELD)
    );
    assert_eq!(
        receipt.result(&verified.plan),
        Some(Settled::NotSent { compacted: false })
    );
}

/// desktop-u R1 negatives: an unreadable reservation is no proof that a
/// child never started; it stops recovery instead.
#[test]
fn an_unreadable_reservation_never_proves_a_child_never_started() {
    let (_temp, location, mut store, epoch) = fixture();
    let verified = original(&location.configuration, &epoch);
    let (_record, _held) = running(&mut store, &location, &verified, &epoch);
    store
        .db
        .execute_batch("DROP TRIGGER desktop_compact_to_send_compaction_custody;")
        .unwrap();
    for broken in ["{", "null"] {
        store
            .db
            .execute(
                "UPDATE desktop_compact_to_sends SET compaction_custody=?1",
                [broken],
            )
            .unwrap();
        assert!(ledger::load(&store.db, &verified.plan.compaction_request_id).is_err());
        assert!(ledger::startup(&mut store.db).is_err());
        let steps: (String, String) = store
            .db
            .query_row(
                "SELECT compaction_step,send_step FROM desktop_compact_to_sends",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            steps,
            ("running".to_owned(), "queued".to_owned()),
            "{broken}"
        );
    }
}

#[test]
fn the_lookup_reads_only_its_exact_operation() {
    let (_temp, location, mut store, epoch) = fixture();
    let verified = original(&location.configuration, &epoch);
    admit(&mut store, &verified, &uuid::Uuid::new_v4().to_string());
    let scope = wire::Scope {
        configuration: location.configuration.clone(),
        service_epoch: epoch.clone(),
    };
    let query = wire::Query::for_plan(scope.clone(), &verified.plan).unwrap();
    let read = |value: Value| query_value(&store, value);
    let lookup: wire::Lookup =
        serde_json::from_value(read(json!({"op":"compact_to_send_result","query":query}))).unwrap();
    assert!(lookup.matches(&query) && lookup.found);
    assert_eq!(
        lookup.receipt.unwrap().binding,
        verified.binding,
        "its own operation"
    );
    // Another kind's op, another scope or an unknown operation never reads.
    for value in [
        json!({"op":"deferred_send_result","query":query}),
        json!({"op":"compact_to_send_result","query":query,"extra":1}),
    ] {
        assert_eq!(read(value)["code"], "compact_to_send_result_unavailable");
    }
    let mut other = query.clone();
    other.scope.service_epoch = uuid::Uuid::new_v4().to_string();
    assert_eq!(
        read(json!({"op":"compact_to_send_result","query":other}))["code"],
        "compact_to_send_result_unavailable"
    );
    let mut missing = query.clone();
    missing.request_id = verified.plan.send_request_id.clone();
    let none: wire::Lookup =
        serde_json::from_value(read(json!({"op":"compact_to_send_result","query":missing})))
            .unwrap();
    assert!(!none.found && none.receipt.is_none());
}
fn query_value(store: &Store, value: Value) -> Value {
    super::query(store, &value)
}

fn state_root(parent: &Path) -> PathBuf {
    let state = parent.join("state");
    std::fs::create_dir(&state).unwrap();
    mode(&state, 0o700);
    state
}
fn observe(state: &Path, plan: &wire::Plan) -> super::receipt::Observed {
    let roots = super::receipt::Roots::open(state, super::receipt::COMPACTION_FOLDER).unwrap();
    super::receipt::compaction(&roots, plan, &|| ()).0
}

#[test]
fn the_receipt_observation_reads_only_the_exact_private_submitted_record() {
    use super::receipt::{Observed, CHANGED, MISSING};
    let temp = tempfile::tempdir().unwrap();
    let state = state_root(&temp.path().canonicalize().unwrap());
    let verified = original(&"a".repeat(64), &uuid::Uuid::new_v4().to_string());
    let plan = &verified.plan;
    let id = &plan.compaction_request_id;
    write_receipt(&state, id, b"{}");
    std::fs::remove_file(state.join("compact_receipts").join(format!("{id}.json"))).unwrap();
    assert_eq!(observe(&state, plan), Observed::Unknown(MISSING));
    write_receipt(
        &state,
        id,
        cli_record(plan, wire::SUBMITTED).to_string().as_bytes(),
    );
    assert!(matches!(observe(&state, plan), Observed::Compacted(o) if o.matches(plan)));
    // The CLI's placeholder before any input, and its own uncertain end.
    write_receipt(&state, id, br#"{"status":"in_progress"}"#);
    assert_eq!(
        observe(&state, plan),
        Observed::Unknown(wire::RECEIPT_IN_PROGRESS)
    );
    write_receipt(
        &state,
        id,
        cli_record(plan, "uncertain").to_string().as_bytes(),
    );
    assert_eq!(
        observe(&state, plan),
        Observed::Unknown(wire::RECEIPT_UNCERTAIN)
    );
    let mut other = cli_record(plan, wire::SUBMITTED);
    other["conversation_id"] = json!("other");
    write_receipt(&state, id, other.to_string().as_bytes());
    assert_eq!(
        observe(&state, plan),
        Observed::Unknown(wire::RECEIPT_MISMATCH)
    );
    // A receipt that is not private is never read.
    let path = write_receipt(
        &state,
        id,
        cli_record(plan, wire::SUBMITTED).to_string().as_bytes(),
    );
    mode(&path, 0o644);
    assert!(matches!(observe(&state, plan), Observed::Unknown(_)));
    mode(&path, 0o600);
    // A receipt that changes while it is read is Unknown.
    let roots = super::receipt::Roots::open(&state, super::receipt::COMPACTION_FOLDER).unwrap();
    let changed = super::receipt::compaction(&roots, plan, &|| {
        write_receipt(
            &state,
            id,
            cli_record(plan, wire::SUBMITTED).to_string().as_bytes(),
        );
    })
    .0;
    assert_eq!(changed, Observed::Unknown(CHANGED));
}

mod actor {
    use super::super::child::Ended;
    use super::*;
    use crate::{config::Config, desktop::ui_authority};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use std::time::{Duration, Instant};
    use zerus_client_core::run::custody::InheritedLock;

    type Execute = dyn Fn(
            &Path,
            &wire::Plan,
            &[String],
            &[u8],
            &Arc<InheritedLock>,
            &Arc<dyn Send + Sync>,
        ) -> Ended
        + Send
        + Sync;

    struct Fixture {
        _temp: tempfile::TempDir,
        store: Store,
        runtime: runtime::Runtime,
        exclusion: Arc<crate::desktop::store::HandoffLock>,
        location: Location,
        epoch: String,
        config: Config,
        owner: Arc<crate::platform::FileLock>,
    }
    impl Fixture {
        fn new(
            execute: impl Fn(
                    &Path,
                    &wire::Plan,
                    &[String],
                    &[u8],
                    &Arc<InheritedLock>,
                    &Arc<dyn Send + Sync>,
                ) -> Ended
                + Send
                + Sync
                + 'static,
        ) -> Self {
            Self::build(Some(Arc::new(execute)))
        }
        /// No executor: the original path runs through the pinned image.
        fn unexecuted() -> Self {
            Self::build(None)
        }
        fn build(execute: Option<Arc<Execute>>) -> Self {
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
            assert_eq!(location.root.parent().unwrap(), config.state);
            session_record(&config.state, SESSION, "claude", RUN, CONVERSATION);
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
            match execute {
                Some(execute) => {
                    let state = config.state.clone();
                    runtime.fixture_executor(
                        MACHINE,
                        Arc::new(move |plan, args, input, lock, resources| {
                            execute(&state, plan, args, input, lock, resources)
                        }),
                    )
                }
                None => runtime.fixture_machine(MACHINE),
            }
            runtime.fixture_mac_fence(false);
            runtime.poll(&mut store, Some(exclusion.clone()));
            Self {
                _temp: temp,
                store,
                runtime,
                exclusion,
                location,
                epoch,
                config,
                owner,
            }
        }
        fn original(&self) -> Verified {
            super::original(&self.location.configuration, &self.epoch)
        }
        fn admit(&mut self, original: &Verified) -> wire::Admission {
            let request = json!(
                zerus_client_core::desktop::ui_requests::Request::CompactToSend {
                    intent: Box::new(original.intent.clone()),
                }
            );
            serde_json::from_value(self.runtime.frontend(
                &mut self.store,
                ui_authority::Validated::fixture(&request),
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
            ledger::load(&self.store.db, &original.plan.compaction_request_id)
                .unwrap()
                .unwrap()
        }
        fn revoke(&mut self) {
            self.store
                .db
                .execute("UPDATE desktop_writer SET revoked=1 WHERE id=1", [])
                .unwrap();
        }
    }
    fn compacted(state: &Path, plan: &wire::Plan) {
        write_receipt(
            state,
            &plan.compaction_request_id,
            cli_record(plan, wire::SUBMITTED).to_string().as_bytes(),
        );
    }

    #[test]
    fn an_admitted_operation_compacts_once_and_settles_only_from_its_cli_receipt() {
        let ran = Arc::new(AtomicUsize::new(0));
        let count = ran.clone();
        let mut f = Fixture::new(move |state, plan, args, input, _, _| {
            count.fetch_add(1, Ordering::SeqCst);
            // The one `hgs compact-context <session> --json`, exact stdin.
            assert_eq!(args, ["compact-context", SESSION, "--json"]);
            assert_eq!(
                format!("{:x}", <sha2::Sha256 as sha2::Digest>::digest(input)),
                plan.compaction_sha256
            );
            let stdin: Value = serde_json::from_slice(input).unwrap();
            assert_eq!(stdin["request_id"], plan.compaction_request_id.as_str());
            compacted(state, plan);
            Ended::Exited { code: 0 }
        });
        let original = f.original();
        let admission = f.admit(&original);
        assert!(admission.accepted && admission.durable && admission.matches(&original.intent));
        f.until("settled", |f| f.row(&original).receipt.drained);
        let receipt = f.row(&original).receipt;
        assert_eq!(receipt.compaction.step, Step::Recorded);
        assert_eq!(
            (receipt.send.step, receipt.code.as_str()),
            (Step::NotStarted, ledger::SEND_UNAVAILABLE)
        );
        assert_eq!(
            receipt.result(&original.plan),
            Some(Settled::NotSent { compacted: true })
        );
        assert!(f.row(&original).send.is_none(), "never reserved");
        // The same operation admitted again returns its settled receipt.
        assert_eq!(f.admit(&original).receipt.unwrap(), receipt);
        f.runtime.request_stop();
        f.until("stopped", |f| f.runtime.drained());
        assert_eq!(ran.load(Ordering::SeqCst), 1, "never compacted again");
    }

    #[test]
    fn an_exit_alone_never_settles() {
        let mut f = Fixture::new(|_, _, _, _, _, _| Ended::Exited { code: 0 });
        let original = f.original();
        f.admit(&original);
        f.until("settled", |f| f.row(&original).receipt.drained);
        let receipt = f.row(&original).receipt;
        assert_eq!(
            (receipt.compaction.step, receipt.compaction.code.as_str()),
            (Step::Unknown, super::super::receipt::MISSING)
        );
        assert_eq!(receipt.send.step, Step::NotStarted);
    }

    /// The selected session's actual record is checked in the spawn turn: a
    /// session that changed starts no child, and with no record of the
    /// request the compaction never started and neither does the send.
    #[test]
    fn a_changed_session_starts_nothing_and_the_operation_never_started() {
        let ran = Arc::new(AtomicUsize::new(0));
        let count = ran.clone();
        let mut f = Fixture::new(move |_, _, _, _, _, _| {
            count.fetch_add(1, Ordering::SeqCst);
            Ended::Exited { code: 0 }
        });
        session_record(&f.config.state, SESSION, "claude", "run-2", CONVERSATION);
        let original = f.original();
        f.admit(&original);
        f.until("settled", |f| f.row(&original).receipt.drained);
        let receipt = f.row(&original).receipt;
        assert_eq!(
            (receipt.compaction.step, receipt.send.step),
            (Step::NotStarted, Step::NotStarted)
        );
        assert_eq!(receipt.result(&original.plan), Some(Settled::NotStarted));
        assert_eq!(ran.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_revoked_writer_or_stop_after_admission_starts_nothing_and_retires_the_operation() {
        let ran = Arc::new(AtomicUsize::new(0));
        let count = ran.clone();
        let mut f = Fixture::new(move |_, _, _, _, _, _| {
            count.fetch_add(1, Ordering::SeqCst);
            Ended::Exited { code: 0 }
        });
        let original = f.original();
        // Admit without polling the work, then revoke before the child.
        let admission = f.admit(&original);
        assert!(admission.accepted);
        f.store
            .db
            .execute("UPDATE desktop_writer SET revoked=1 WHERE id=1", [])
            .unwrap();
        f.until("retired", |f| f.row(&original).receipt.drained);
        let receipt = f.row(&original).receipt;
        assert_eq!(receipt.compaction.step, Step::NotStarted);
        assert_eq!(receipt.send.step, Step::NotStarted);
        assert_eq!(ran.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn restart_retires_a_queued_operation_and_never_reconstructs_consent() {
        let mut f = Fixture::new(|_, _, _, _, _, _| panic!("recovered operation must not run"));
        let original = f.original();
        assert!(f.admit(&original).accepted);
        // A restart before any child: a new runtime and Store, no consent.
        f.runtime = runtime::Runtime::new(&f.config, &f.epoch, f.owner.clone()).unwrap();
        f.runtime.fixture_executor(
            MACHINE,
            Arc::new(|_, _, _, _, _| panic!("restart must not run a child")),
        );
        f.runtime.fixture_mac_fence(false);
        f.store = Store::open(&f.location, &f.epoch).unwrap();
        zerus_desktop_ledger::authorize(
            &mut f.store.db,
            &uuid::Uuid::new_v4().to_string(),
            &f.epoch,
        )
        .unwrap();
        f.until("retired", |f| f.row(&original).receipt.drained);
        let receipt = f.row(&original).receipt;
        assert_eq!(
            (
                receipt.compaction.step,
                receipt.send.step,
                receipt.code.as_str()
            ),
            (Step::NotStarted, Step::NotStarted, ledger::INTERRUPTED)
        );
        // The same operation again gets only that receipt.
        let again = f.admit(&original);
        assert!(again.accepted);
        assert_eq!(again.receipt.unwrap(), receipt);
    }

    #[test]
    fn another_scope_or_an_invalid_original_admits_nothing() {
        let mut f = Fixture::new(|_, _, _, _, _, _| panic!("no child"));
        let other = super::original(&"b".repeat(64), &f.epoch);
        let admission = f.admit(&other);
        assert!(!admission.accepted && !admission.durable);
        assert_eq!(admission.code, "compact_to_send_unavailable");
        let mut invalid = f.original();
        invalid.intent.compaction = "{}".into();
        assert!(!f.admit(&invalid).accepted);
        let rows: i64 = f
            .store
            .db
            .query_row("SELECT count(*) FROM desktop_compact_to_sends", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(rows, 0);
    }

    #[test]
    fn the_macos_fence_refuses_before_any_row_or_child() {
        let mut f = Fixture::new(|_, _, _, _, _, _| panic!("no child"));
        f.runtime.fixture_mac_fence(true);
        let original = f.original();
        let admission = f.admit(&original);
        assert!(!admission.accepted && !admission.durable);
        assert_eq!(admission.code, runtime::MAC_UNAVAILABLE);
        assert!(
            ledger::load(&f.store.db, &original.plan.compaction_request_id)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn a_receipt_read_past_its_deadline_is_unknown_however_exact() {
        let mut f = Fixture::new(|state, plan, _, _, _, _| {
            compacted(state, plan);
            Ended::Exited { code: 0 }
        });
        f.runtime.fixture_read(
            Some(Arc::new(|| std::thread::sleep(Duration::from_millis(60)))),
            Duration::from_millis(10),
        );
        let original = f.original();
        f.admit(&original);
        f.until("settled", |f| f.row(&original).receipt.drained);
        assert_eq!(
            f.row(&original).receipt.compaction.code,
            super::super::receipt::TIMED_OUT
        );
    }

    #[test]
    fn store_recovery_leaves_the_runtimes_own_original_to_it() {
        let (entered_tx, entered) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel::<()>();
        let released = std::sync::Mutex::new(released);
        let mut f = Fixture::new(move |state, plan, _, _, _, _| {
            entered_tx.send(()).unwrap();
            released
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            compacted(state, plan);
            Ended::Exited { code: 0 }
        });
        let original = f.original();
        f.admit(&original);
        f.until("running", |f| {
            f.row(&original).receipt.compaction.step == Step::Running
        });
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        // Store recovery runs while the child is held, and again after the
        // child ended and released its custody but before the runtime's own
        // drain: it never settles the runtime's own original.
        for _ in 0..5 {
            f.store.recover_operation_custody(&f.location);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!f.row(&original).receipt.drained);
        release.send(()).unwrap();
        f.until("ended", |f| {
            f.row(&original).receipt.code == ledger::AWAITING_DRAIN
        });
        // Past the recovery's own 250 ms probe interval.
        for _ in 0..120 {
            f.store.recover_operation_custody(&f.location);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!f.row(&original).receipt.drained, "recovery settled it");
        f.until("settled", |f| f.row(&original).receipt.drained);
        assert_eq!(f.row(&original).receipt.compaction.step, Step::Recorded);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn the_pinned_image_runs_the_original_compaction_and_its_cli_receipt_settles() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hgs");
        let seen = dir.path().join("seen");
        let mut f = Fixture::unexecuted();
        let receipts = f.config.state.join("compact_receipts");
        std::fs::create_dir(&receipts).unwrap();
        mode(&receipts, 0o700);
        std::fs::write(
            &path,
            format!(
                r#"#!/usr/bin/env python3
import json,os,pathlib,sys
data=sys.stdin.buffer.read()
request=json.loads(data)
pathlib.Path({seen:?}).write_text(json.dumps({{"argv":sys.argv[1:],"stdin":data.decode(),"custody":os.environ.get("HGS_COMPACT_TO_SEND_CUSTODY_FD")}}))
record={{"request_id":request["request_id"],"name":sys.argv[2],"run_id":request["expected_run_id"],"conversation_id":request["expected_conversation_id"],"status":"submitted","at":1.0}}
target=os.path.join({receipts:?},request["request_id"]+".json")
fd=os.open(target+".tmp",os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600)
os.write(fd,json.dumps(record).encode())
os.close(fd)
os.rename(target+".tmp",target)
print(json.dumps(record))
"#,
                seen = seen.to_str().unwrap(),
                receipts = receipts.to_str().unwrap(),
            ),
        )
        .unwrap();
        mode(&path, 0o700);
        f.runtime.fixture_image(&path);
        let original = f.original();
        assert!(f.admit(&original).accepted);
        f.until("settled", |f| f.row(&original).receipt.drained);
        let receipt = f.row(&original).receipt;
        assert_eq!(receipt.compaction.step, Step::Recorded);
        assert_eq!(receipt.send.step, Step::NotStarted);
        let seen: Value = serde_json::from_slice(&std::fs::read(&seen).unwrap()).unwrap();
        assert_eq!(seen["argv"], json!(["compact-context", SESSION, "--json"]));
        assert_eq!(seen["stdin"], original.intent.compaction.as_str());
        assert!(seen["custody"]
            .as_str()
            .is_some_and(|fd| fd.parse::<u32>().is_ok()));
    }

    /// A stop, a revoke or a changed source after the reservation committed
    /// starts nothing: the consent veto runs last in the spawn turn, and with
    /// no record of the request both children never started.
    #[test]
    #[cfg(target_os = "linux")]
    fn a_stop_revoke_or_source_change_after_reservation_starts_nothing() {
        for action in ["stop", "revoke", "source"] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("hgs");
            let ran = dir.path().join("ran");
            std::fs::write(
                &path,
                format!(
                    "#!/bin/sh\ntouch {:?}\ncat >/dev/null\n",
                    ran.to_str().unwrap()
                ),
            )
            .unwrap();
            mode(&path, 0o700);
            let mut f = Fixture::unexecuted();
            f.runtime.fixture_image(&path);
            let (entered_tx, entered) = std::sync::mpsc::channel::<()>();
            let (release, released) = std::sync::mpsc::channel::<()>();
            let entered_tx = std::sync::Mutex::new(entered_tx);
            let released = std::sync::Mutex::new(released);
            f.runtime.fixture_spawn_hook(Arc::new(move || {
                let _ = entered_tx.lock().unwrap().send(());
                let _ = released
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(10));
            }));
            let original = f.original();
            assert!(f.admit(&original).accepted);
            let until = Instant::now() + Duration::from_secs(8);
            while entered.try_recv().is_err() {
                assert!(
                    Instant::now() < until,
                    "{action}: never reached the spawn turn"
                );
                f.runtime.poll(&mut f.store, Some(f.exclusion.clone()));
                std::thread::sleep(Duration::from_millis(2));
            }
            // The reservation committed before the worker existed.
            assert_eq!(
                f.row(&original).receipt.compaction.step,
                Step::Running,
                "{action}"
            );
            match action {
                "stop" => f.runtime.request_stop(),
                "revoke" => f.revoke(),
                _ => f
                    .runtime
                    .fixture_machine("33333333-3333-4333-8333-333333333333"),
            }
            f.runtime.poll(&mut f.store, Some(f.exclusion.clone()));
            release.send(()).unwrap();
            f.until("settled", |f| f.row(&original).receipt.drained);
            let receipt = f.row(&original).receipt;
            assert_eq!(
                (receipt.compaction.step, receipt.send.step),
                (Step::NotStarted, Step::NotStarted),
                "{action}"
            );
            assert!(!ran.exists(), "{action}: no child ever existed");
        }
    }
}
