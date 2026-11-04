//! Real Store transactions and owner exclusion with synthetic native permits.
//! These controls are not a frontend-authentication or Terminate execution proof.
use super::*;
use crate::desktop::{store::Location, ui_authority};
use std::sync::Arc;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::time::{Duration, Instant};
use zerus_client_core::{desktop::ui_requests::Request, termination_cleanup::Capture, Target};

struct Fixture {
    _temp: tempfile::TempDir,
    _owner: Arc<crate::platform::FileLock>,
    location: Location,
    store: Store,
    epoch: String,
}
fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let location = Location {
            root: temp.path().canonicalize().unwrap().join("state/desktop"),
            configuration: "a".repeat(64),
        };
        location.prepare().unwrap();
        let owner = Arc::new(location.lock().unwrap());
        let epoch = id();
        let store = Store::open(&location, &epoch).unwrap();
        Self {
            _temp: temp,
            _owner: owner,
            location,
            store,
            epoch,
        }
    }
    fn intent(&self) -> Intent {
        let capture = Capture {
            target: Target {
                computer: String::new(),
                name: "codex/example/original".into(),
                run_id: "original-run".into(),
                conversation_id: "original-conversation".into(),
            },
            machine: "22222222-2222-4222-8222-222222222222".into(),
            root: "/srv/example/work".into(),
            common_dir: "/srv/example/main/.git".into(),
            owner_id: "original-owner".into(),
        };
        let request_id = id();
        let original =
            String::from_utf8(capture.invocation(&request_id).unwrap().stdin.unwrap()).unwrap();
        Intent {
            scope: wire::Scope {
                configuration: self.location.configuration.clone(),
                service_epoch: self.epoch.clone(),
            },
            confirmation_id: id(),
            choice: wire::Choice::RemoveAfterTerminate,
            request_id,
            capture,
            original,
        }
    }
    fn rows(&self) -> i64 {
        self.store
            .db
            .query_row(
                "SELECT count(*) FROM desktop_termination_outcomes",
                [],
                |r| r.get(0),
            )
            .unwrap()
    }
    fn query(&self, original: &str) -> Value {
        query(
            &self.store,
            &self.location.configuration,
            &json!({"op":"termination_outcome_status","request_id":id(),"original_request_id":original,"configuration":self.location.configuration}),
        )
    }
}
fn value(intent: &Intent) -> Value {
    serde_json::to_value(Request::TerminationOutcome {
        intent: Box::new(intent.clone()),
    })
    .unwrap()
}
fn permit(intent: &Intent) -> Validated {
    ui_authority::Validated::fixture(&value(intent))
}

#[test]
fn typed_retention_keeps_original_bytes_without_claiming_execution_or_drain() {
    let mut f = Fixture::new();
    let mut intent = f.intent();
    let data: Value = serde_json::from_str(&intent.original).unwrap();
    intent.original = serde_json::to_string_pretty(&data).unwrap();
    assert!(
        permit(&intent).into_request().is_err(),
        "generic mutation lane must refuse"
    );
    let response = frontend(&mut f.store, permit(&intent));
    assert_eq!(response["accepted"], true, "{response}");
    assert_eq!(response["durable"], true);
    assert_eq!(response["phase"], "retained");
    assert_eq!(response["cleanup_authorized"], false);
    for field in ["drained", "completed", "terminated"] {
        assert!(response.get(field).is_none());
    }
    let recorded: Status = serde_json::from_value(response["status"].clone()).unwrap();
    assert!(recorded.matches(&intent));
    let raw: String = f
        .store
        .db
        .query_row(
            "SELECT original FROM desktop_termination_outcomes WHERE request_id=?1",
            [&intent.request_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(serde_json::from_str::<Intent>(&raw).unwrap(), intent);
    assert_eq!(
        f.store.operation_undrained().unwrap(),
        0,
        "metadata is not a fictitious live effect"
    );
    assert_eq!(frontend(&mut f.store, permit(&intent)), response);
    assert_eq!(f.rows(), 1);
    let mut changed = intent.clone();
    changed.capture.owner_id = "replacement".into();
    assert_eq!(frontend(&mut f.store, permit(&changed))["accepted"], false);
    assert_eq!(f.rows(), 1);
    assert!(!f
        .query(&intent.request_id)
        .to_string()
        .contains("/srv/example"));
}

#[test]
fn a_new_uuid_cannot_rebind_an_original_confirmation() {
    let mut f = Fixture::new();
    let original = f.intent();
    assert_eq!(frontend(&mut f.store, permit(&original))["accepted"], true);
    let mut changed = original.clone();
    changed.request_id = id();
    changed.original = String::from_utf8(
        changed
            .capture
            .invocation(&changed.request_id)
            .unwrap()
            .stdin
            .unwrap(),
    )
    .unwrap();
    assert!(changed.valid());
    assert_eq!(frontend(&mut f.store, permit(&changed))["accepted"], false);
    assert_eq!(f.rows(), 1);
    assert_eq!(f.query(&changed.request_id)["found"], false);
    assert_eq!(f.query(&original.request_id)["found"], true);
}

#[test]
fn unreadable_or_replaced_stored_data_is_not_an_empty_success_or_repaired() {
    let mut f = Fixture::new();
    let original = f.intent();
    assert_eq!(frontend(&mut f.store, permit(&original))["accepted"], true);
    f.store
        .db
        .execute_batch("DROP TRIGGER desktop_termination_outcomes_immutable;")
        .unwrap();
    f.store
        .db
        .execute(
            "UPDATE desktop_termination_outcomes SET fingerprint=?1",
            ["0".repeat(64)],
        )
        .unwrap();
    assert_eq!(f.query(&original.request_id)["accepted"], false);
    assert_eq!(frontend(&mut f.store, permit(&original))["accepted"], false);
    assert_eq!(f.rows(), 1);
    assert_eq!(
        f.store
            .db
            .query_row::<String, _, _>(
                "SELECT fingerprint FROM desktop_termination_outcomes",
                [],
                |r| r.get(0)
            )
            .unwrap(),
        "0".repeat(64)
    );
}

#[test]
fn reply_loss_and_restart_use_original_id_without_rewriting_the_original_scope() {
    let mut f = Fixture::new();
    let intent = f.intent();
    assert_eq!(frontend(&mut f.store, permit(&intent))["accepted"], true);
    let original: String = f
        .store
        .db
        .query_row(
            "SELECT original FROM desktop_termination_outcomes",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let before = f.query(&intent.request_id);
    assert_eq!(before["found"], true);
    assert_eq!(before["durable"], false);
    assert_eq!(f.query(&id())["found"], false);
    drop(f.store);
    f.epoch = id();
    f.store = Store::open(&f.location, &f.epoch).unwrap();
    let after = f.query(&intent.request_id);
    assert_eq!(after["status"], before["status"]);
    let mut current = intent.clone();
    current.scope.service_epoch = f.epoch.clone();
    assert_eq!(
        frontend(&mut f.store, permit(&current))["status"],
        before["status"]
    );
    let retained: String = f
        .store
        .db
        .query_row(
            "SELECT original FROM desktop_termination_outcomes",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(retained, original);
    assert_eq!(f.rows(), 1);
    assert_eq!(
        frontend(&mut f.store, permit(&intent))["accepted"],
        false,
        "old service scope is not renewed"
    );
}

#[test]
fn foreign_scope_or_replaced_capture_never_creates_a_retained_row() {
    let mut f = Fixture::new();
    let original = f.intent();
    for part in ["epoch", "configuration", "run", "conversation", "action"] {
        let mut intent = original.clone();
        match part {
            "configuration" => intent.scope.configuration = "b".repeat(64),
            "epoch" => intent.scope.service_epoch = id(),
            "run" => intent.capture.target.run_id = "replacement".into(),
            "conversation" => intent.capture.target.conversation_id = "replacement".into(),
            _ => intent.original = intent.original.replace("terminate", "archive"),
        }
        assert_eq!(
            frontend(&mut f.store, permit(&intent))["accepted"],
            false,
            "{part}"
        );
        assert_eq!(f.rows(), 0);
    }
}

#[test]
fn failed_commit_is_not_reported_as_durable_retention() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let mut f = Fixture::new();
    let intent = f.intent();
    let inserts = Arc::new(AtomicUsize::new(0));
    let observed = inserts.clone();
    f.store.db.update_hook(Some(
        move |action: rusqlite::hooks::Action, _database: &str, table: &str, _row: i64| {
            if action == rusqlite::hooks::Action::SQLITE_INSERT
                && table == "desktop_termination_outcomes"
            {
                observed.fetch_add(1, Ordering::AcqRel);
            }
        },
    ));
    f.store.db.commit_hook(Some(|| true));
    let result = frontend(&mut f.store, permit(&intent));
    f.store.db.commit_hook(None::<fn() -> bool>);
    f.store
        .db
        .update_hook(None::<fn(rusqlite::hooks::Action, &str, &str, i64)>);
    assert_eq!(inserts.load(Ordering::Acquire), 1);
    assert_eq!(result["accepted"], false);
    assert_eq!(result["durable"], false);
    assert_eq!(f.rows(), 0);
    assert_eq!(f.query(&intent.request_id)["found"], false);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn proof_expiring_while_begin_waits_is_refused_after_the_lock_is_acquired() {
    let mut f = Fixture::new();
    let intent = f.intent();
    let path = f.location.root.join("state.sqlite3");
    let (held, ready) = std::sync::mpsc::sync_channel(1);
    let (release, start) = std::sync::mpsc::sync_channel(1);
    let holder = std::thread::spawn(move || {
        let db = Connection::open(path).unwrap();
        db.execute_batch("BEGIN IMMEDIATE;").unwrap();
        held.send(()).unwrap();
        start.recv_timeout(Duration::from_secs(3)).unwrap();
        std::thread::sleep(Duration::from_millis(400));
        db.execute_batch("ROLLBACK;").unwrap();
    });
    ready.recv_timeout(Duration::from_secs(3)).unwrap();
    f.store.db.busy_timeout(Duration::from_secs(2)).unwrap();
    let authority = ui_authority::Validated::fixture_expiring(
        &value(&intent),
        Instant::now() + Duration::from_millis(150),
    );
    release.send(()).unwrap();
    let result = frontend(&mut f.store, authority);
    holder.join().unwrap();
    assert_eq!(result["accepted"], false);
    assert_eq!(f.rows(), 0);
}

#[test]
fn capacity_and_immutability_do_not_expire_or_recycle_retained_originals() {
    let mut f = Fixture::new();
    let original = f.intent();
    assert_eq!(frontend(&mut f.store, permit(&original))["accepted"], true);
    for _ in 1..wire::RECORDS {
        let intent = f.intent();
        assert_eq!(frontend(&mut f.store, permit(&intent))["accepted"], true);
    }
    assert_eq!(f.rows(), wire::RECORDS as i64);
    let overflow = f.intent();
    assert_eq!(frontend(&mut f.store, permit(&overflow))["accepted"], false);
    assert_eq!(frontend(&mut f.store, permit(&original))["accepted"], true);
    assert!(f
        .store
        .db
        .execute("UPDATE desktop_termination_outcomes SET original='{}'", [])
        .is_err());
    assert!(f
        .store
        .db
        .execute("DELETE FROM desktop_termination_outcomes", [])
        .is_err());
    assert_eq!(f.rows(), wire::RECORDS as i64);
    let mut wrong = json!({"op":"termination_outcome_status","request_id":id(),"original_request_id":original.request_id,"configuration":"b".repeat(64)});
    assert_eq!(
        query(&f.store, &f.location.configuration, &wrong)["accepted"],
        false
    );
    wrong["configuration"] = json!(f.location.configuration);
    wrong["allow_retry"] = json!(true);
    assert_eq!(
        query(&f.store, &f.location.configuration, &wrong)["accepted"],
        false
    );
}

#[test]
fn exact_twenty_four_upgrade_is_atomic_and_preserves_existing_leaves() {
    assert_eq!(SCHEMA - 1, crate::desktop::creation_launch::SCHEMA);
    let f = Fixture::new();
    remove_fixture(&f.store.db);
    f.store
        .db
        .pragma_update(None, "user_version", SCHEMA - 1)
        .unwrap();
    f.store.db.execute_batch("CREATE TABLE desktop_termination_outcomes(sentinel TEXT); INSERT INTO desktop_termination_outcomes VALUES('keep');").unwrap();
    let Fixture {
        _temp,
        _owner,
        location,
        store,
        epoch: _,
    } = f;
    drop(store);
    assert!(Store::open(&location, &id()).is_err());
    let db = Connection::open(location.root.join("state.sqlite3")).unwrap();
    assert_eq!(
        db.query_row::<i64, _, _>("PRAGMA user_version", [], |r| r.get(0))
            .unwrap(),
        SCHEMA - 1
    );
    assert_eq!(
        db.query_row::<String, _, _>(
            "SELECT sentinel FROM desktop_termination_outcomes",
            [],
            |r| r.get(0)
        )
        .unwrap(),
        "keep"
    );
    db.execute_batch("DROP TABLE desktop_termination_outcomes;")
        .unwrap();
    drop(db);
    let store = Store::open(&location, &id()).unwrap();
    assert_eq!(
        store
            .db
            .query_row::<i64, _, _>("PRAGMA user_version", [], |r| r.get(0))
            .unwrap(),
        SCHEMA
    );
    for table in [
        "desktop_creation_clones",
        "desktop_cache_roots",
        "desktop_creation_worktrees",
        "desktop_creation_launches",
    ] {
        assert_eq!(
            store
                .db
                .query_row::<i64, _, _>(
                    "SELECT count(*) FROM sqlite_schema WHERE type='table' AND name=?1",
                    [table],
                    |r| r.get(0)
                )
                .unwrap(),
            1
        );
    }
}
