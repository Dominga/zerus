//! Merge controls use product-written teams/cards/receipts. Named rewinds and
//! corruptions change only metadata, never invent a canonical Team/Task row.
use super::*;

fn rewind_without_native_leaf(h: &Harness, version: u32) {
    let db = h.store.db.as_ref().unwrap();
    let count: u32 = db
        .query_row("SELECT count(*) FROM person_gestures", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0, "legacy rewind never drops real native evidence");
    db.execute_batch("DROP TRIGGER person_gesture_store_kept; DROP TRIGGER person_gesture_store_immutable; DROP TRIGGER person_gestures_immutable; DROP TABLE person_gestures; DROP TABLE person_gesture_store;").unwrap();
    db.pragma_update(None, "user_version", version).unwrap();
}

#[test]
fn planning_layout_three_gets_a_native_leaf_without_changing_its_team() {
    let (mut h, request) = sample();
    let mut input = serde_json::to_value(producers::create()).unwrap();
    input["op"] = json!("start_planning");
    input["goal"] = json!("Plan the native migration control");
    input["name"] = json!("Planning migration control");
    input["integration_branch"] = json!("teams/native-migration-control");
    h.run_for(
        producers::TEAM_B,
        &producers::person(),
        serde_json::from_value(input).unwrap(),
    )
    .unwrap();
    let team = h.store.team(producers::TEAM_B).unwrap();
    let planning = h.store.planning(producers::TEAM_B).unwrap();
    rewind_without_native_leaf(&h, 3);
    assert!(Store::person_evidence(h.temp.path()).is_err());
    h.store = Store::open(h.temp.path()).unwrap();
    assert_eq!(h.store.team(producers::TEAM_B).unwrap(), team);
    assert_eq!(h.store.planning(producers::TEAM_B).unwrap(), planning);
    let first = identity(h.store.db.as_ref().unwrap(), h.store.state()).unwrap();
    let admitted = Admission::fixture(&h.store, &request);
    apply(&mut h, &request, admitted).unwrap();
    h.store = Store::open(h.temp.path()).unwrap();
    assert_eq!(
        identity(h.store.db.as_ref().unwrap(), h.store.state()).unwrap(),
        first
    );
}

#[test]
fn native_layout_three_keeps_its_nonce_and_original_receipt() {
    let (mut h, request) = sample();
    let admission = Admission::fixture(&h.store, &request);
    let original_intent = intent(&admission);
    let result = apply(&mut h, &request, admission).unwrap();
    let before = identity(h.store.db.as_ref().unwrap(), h.store.state()).unwrap();
    // The original S1 candidate's exact format: complete native proof at 3.
    h.store
        .db
        .as_ref()
        .unwrap()
        .pragma_update(None, "user_version", 3)
        .unwrap();
    h.store = Store::open(h.temp.path()).unwrap();
    let db = h.store.db.as_ref().unwrap();
    assert_eq!(identity(db, h.store.state()).unwrap(), before);
    original(db, &original_intent).unwrap();
    assert_eq!(
        ops::replay(db, &actor(), &request)
            .unwrap()
            .unwrap()
            .unwrap(),
        result
    );
    let version: u32 = db
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert!(
        version > 3,
        "the combined format must fence both earlier engines"
    );
}

#[test]
fn partial_native_layout_three_is_refused_without_repair() {
    let (mut h, _) = sample();
    let db = h.store.db.as_ref().unwrap();
    db.execute_batch("DROP TRIGGER person_gestures_immutable; PRAGMA user_version=3;")
        .unwrap();
    assert!(Store::open(h.temp.path()).is_err());
    let count: u32 = db
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='person_gestures_immutable'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        3
    );
    // Keep the original holder alive until all readback assertions finish.
    h.store.db.take();
}

#[test]
fn corrupt_native_nonce_at_layout_three_is_refused_without_replacement() {
    let (h, _) = sample();
    let db = h.store.db.as_ref().unwrap();
    // Named corruption: preserve the complete object set, corrupt only the
    // nonce through a temporary removal/replacement of its immutable trigger.
    db.execute_batch("DROP TRIGGER person_gesture_store_immutable; UPDATE person_gesture_store SET nonce='zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz'; CREATE TRIGGER person_gesture_store_immutable BEFORE UPDATE ON person_gesture_store BEGIN SELECT RAISE(ABORT,'Person gesture store identity is immutable'); END; PRAGMA user_version=3;").unwrap();
    assert!(Store::open(h.temp.path()).is_err());
    let nonce: String = db
        .query_row(
            "SELECT nonce FROM person_gesture_store WHERE id=1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(nonce, "z".repeat(36));
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        3
    );
}

#[test]
fn complete_native_leaf_with_a_corrupt_trigger_is_refused_as_is() {
    let (h, _) = sample();
    let db = h.store.db.as_ref().unwrap();
    db.execute_batch("DROP TRIGGER person_gestures_immutable; CREATE TRIGGER person_gestures_immutable BEFORE UPDATE ON person_gestures BEGIN SELECT 1; END; PRAGMA user_version=3;").unwrap();
    assert!(Store::open(h.temp.path()).is_err());
    assert!(db
        .query_row(
            "SELECT sql FROM sqlite_master WHERE name='person_gestures_immutable'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap()
        .contains("SELECT 1"));
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        3
    );
}

type WriterGate = (
    std::sync::mpsc::SyncSender<()>,
    std::sync::mpsc::Receiver<()>,
);
thread_local! {
    static WRITER_GATE: std::cell::RefCell<Option<WriterGate>> = const { std::cell::RefCell::new(None) };
}

fn wait_for_current_writer(_: i32) -> bool {
    WRITER_GATE.with(|slot| {
        let Some((blocked, resume)) = slot.borrow_mut().take() else {
            return false;
        };
        blocked.send(()).is_ok() && resume.recv_timeout(Duration::from_secs(15)).is_ok()
    })
}

#[test]
fn writer_fence_precedes_original_native_receipt_replay_after_lock_wait() {
    let (mut h, request) = sample();
    let admitted = Admission::fixture(&h.store, &request);
    let original_intent = intent(&admitted);
    let first = apply(&mut h, &request, admitted).unwrap();
    let mut duplicate = Admission::fixture(&h.store, &request);
    rewrite(&mut duplicate, &original_intent);
    let writer = Connection::open(h.temp.path().join("teams/teams-v1.sqlite3")).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    writer
        .pragma_update(None, "user_version", store::SCHEMA_VERSION + 1)
        .unwrap();
    let (blocked_tx, blocked_rx) = std::sync::mpsc::sync_channel(1);
    let (resume_tx, resume_rx) = std::sync::mpsc::sync_channel(1);
    let worker_request = request.clone();
    let worker = std::thread::spawn(move || {
        WRITER_GATE.with(|slot| *slot.borrow_mut() = Some((blocked_tx, resume_rx)));
        h.store
            .db
            .as_ref()
            .unwrap()
            .busy_handler(Some(wait_for_current_writer))
            .unwrap();
        let result = apply(&mut h, &worker_request, duplicate);
        (h, result)
    });
    // The busy callback proves the unlocked active check saw the previous
    // committed mark and the native original is waiting for this writer.
    let blocked = blocked_rx.recv_timeout(Duration::from_secs(15));
    writer.execute_batch("COMMIT").unwrap();
    let _ = resume_tx.send(());
    let (h, result) = worker.join().unwrap();
    blocked.expect("native original did not reach the blocked writer");
    assert_eq!(result.unwrap_err().code, code::INVALID_STATE);
    let db = h.store.db.as_ref().unwrap();
    original(db, &original_intent).unwrap();
    assert_eq!(
        ops::replay(db, &actor(), &request)
            .unwrap()
            .unwrap()
            .unwrap(),
        first
    );
    assert_eq!(
        db.query_row("SELECT revision FROM teams WHERE id=?1", [TEAM], |r| r
            .get::<_, u64>(0))
            .unwrap(),
        first.revision
    );
}

#[test]
fn current_native_leaf_is_never_rebuilt_even_when_wholly_missing() {
    let (h, _) = sample();
    rewind_without_native_leaf(&h, store::SCHEMA_VERSION);
    assert!(Store::open(h.temp.path()).is_err());
    assert!(Store::person_evidence(h.temp.path()).is_err());
    assert_eq!(
        h.store
            .db
            .as_ref()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='person_gesture_store'",
                [],
                |r| r.get::<_, u32>(0)
            )
            .unwrap(),
        0
    );
}
