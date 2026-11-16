use super::*;

#[test]
fn a_pinned_read_view_does_not_hold_the_publisher_gate() {
    let root = fixture();
    let selected = path(root.path());
    let mut value = record();
    value.as_object_mut().unwrap().remove("shell_jobs");
    storage::atomic(&selected, &value.to_string()).unwrap();
    assert!(repair_to_end(root.path()).complete);
    let reader = pages::Reader::open(root.path()).unwrap();
    assert!(reader.page(None).unwrap().fresh);
    value["phase"] = json!("working");
    write_at(root.path(), &selected, &value, |publication| {
        storage::atomic_observed(&selected, &value.to_string(), publication)
    })
    .unwrap();
    assert!(
        !root
            .path()
            .join("listing-metadata/coverage-lost.json")
            .exists(),
        "an ordinary page reader contended with its publisher"
    );
    let writer = Store::open(root.path()).unwrap();
    let pending: i64 = writer
        .db
        .query_row("SELECT count(*) FROM intents", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        pending, 0,
        "a page forced the canonical publisher to lose coverage"
    );
    assert!(
        !reader.page(None).unwrap().fresh,
        "an old pinned seed borrowed a later native version"
    );
}

#[test]
fn an_in_place_change_cannot_reuse_a_fresh_snapshot() {
    let root = fixture();
    let selected = path(root.path());
    let mut value = record();
    value.as_object_mut().unwrap().remove("shell_jobs");
    storage::atomic(&selected, &value.to_string()).unwrap();
    assert!(repair_to_end(root.path()).complete);
    let store = Store::open(root.path()).unwrap();
    assert!(store.page(None).unwrap().fresh);
    value["phase"] = json!("working");
    // Keep the directory generation intact: only the original file changes.
    fs::write(&selected, value.to_string()).unwrap();
    assert!(
        !store.page(None).unwrap().fresh,
        "a directory witness cannot certify a changed snapshot row"
    );
}

#[test]
#[cfg(target_os = "linux")]
fn ordinary_writes_between_slices_do_not_require_a_globally_quiet_pass() {
    let root = fixture();
    let mut current = record();
    current.as_object_mut().unwrap().remove("shell_jobs");
    for n in 0..384 {
        let mut value = current.clone();
        value["name"] = json!(format!("codex/example/member-{n}"));
        let selected = root.path().join(format!(
            "{}.json",
            storage::label_key(string(&value, "name"))
        ));
        storage::atomic(&selected, &value.to_string()).unwrap();
    }
    let selected = path(root.path());
    let publish = |value: &Value| {
        write_at(root.path(), &selected, value, |publication| {
            storage::atomic_observed(&selected, &value.to_string(), publication)
        })
        .unwrap();
    };
    publish(&current);
    assert!(repair_to_end(root.path()).complete);
    current["last_event_at"] = json!(1);
    publish(&current);
    let mut work = None;
    for tick in 0..128 {
        let stats = background_at(root.path(), &mut work).unwrap();
        assert!(stats.entries <= 256 && stats.rows <= 128 && stats.bytes <= 16 << 20);
        if stats.done && stats.complete {
            return;
        }
        current["last_event_at"] = json!(tick + 2);
        publish(&current);
    }
    panic!("continuous observed writes starved every complete display cut");
}

#[test]
fn a_pinned_reader_cannot_make_advisory_wal_growth_unbounded() {
    let root = fixture();
    let selected = path(root.path());
    let mut value = record();
    value.as_object_mut().unwrap().remove("shell_jobs");
    storage::atomic(&selected, &value.to_string()).unwrap();
    assert!(repair_to_end(root.path()).complete);
    let reader = pages::Reader::open(root.path()).unwrap();
    assert!(reader.page(None).unwrap().fresh);
    let store = Store::open(root.path()).unwrap();
    store
        .db
        .execute_batch("CREATE TABLE private_wal_pressure(body BLOB)")
        .unwrap();
    store
        .db
        .execute(
            "INSERT INTO private_wal_pressure VALUES(zeroblob(?1))",
            [65 << 20],
        )
        .unwrap();
    drop(store);
    assert!(
        fs::metadata(root.path().join("listing-metadata/index.sqlite3-wal"))
            .unwrap()
            .len()
            > 64 << 20
    );
    let outcome = Store::open(root.path());
    assert!(
        outcome.is_err(),
        "a pinned read transaction let advisory WAL grow without a quota"
    );
    assert!(outcome.err().unwrap().to_string().contains("WAL"));
    value["phase"] = json!("working");
    let mut calls = 0;
    let result: Result<()> = write_at(root.path(), &selected, &value, |publication| {
        calls += 1;
        storage::atomic_observed(&selected, &value.to_string(), publication)?;
        Err("original unknown outcome".into())
    });
    assert_eq!(result.unwrap_err(), "original unknown outcome");
    assert_eq!(calls, 1);
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(selected).unwrap()).unwrap()["phase"],
        "working"
    );
    let mut work = None;
    assert!(
        background_at(root.path(), &mut work).is_err(),
        "a pinned reader cannot be forced out by a quota checkpoint"
    );
    drop(reader);
    for _ in 0..128 {
        if background_at(root.path(), &mut work).unwrap().complete {
            assert_eq!(
                pages::Reader::open(root.path())
                    .unwrap()
                    .page(None)
                    .unwrap()
                    .rows[0]
                    .1["record"]["phase"],
                "working"
            );
            assert_eq!(
                calls, 1,
                "display repair replayed an unknown canonical write"
            );
            return;
        }
    }
    panic!("background WAL recovery did not resume after the reader released its pin");
}

#[test]
#[cfg(target_os = "linux")]
fn a_sqlite_failure_after_native_hints_forces_a_new_census() {
    let root = fixture();
    let selected = path(root.path());
    let mut value = record();
    value.as_object_mut().unwrap().remove("shell_jobs");
    storage::atomic(&selected, &value.to_string()).unwrap();
    let mut work = None;
    for _ in 0..128 {
        if background_at(root.path(), &mut work).unwrap().complete {
            break;
        }
    }
    let store = Store::open(root.path()).unwrap();
    assert!(store.page(None).unwrap().fresh);
    store.db.execute_batch("CREATE TRIGGER fail_hint BEFORE INSERT ON dirty BEGIN SELECT RAISE(FAIL,'private consumed-event cut'); END;").unwrap();
    drop(store);
    value["phase"] = json!("working");
    storage::atomic(&selected, &value.to_string()).unwrap();
    assert!(background_at(root.path(), &mut work).is_err());
    assert!(
        work.is_none(),
        "consumed native events survived a rolled-back step"
    );
    let store = Store::open(root.path()).unwrap();
    store.db.execute_batch("DROP TRIGGER fail_hint").unwrap();
    drop(store);
    for _ in 0..128 {
        if background_at(root.path(), &mut work).unwrap().complete {
            let page = pages::Reader::open(root.path())
                .unwrap()
                .page(None)
                .unwrap();
            assert!(page.fresh);
            assert_eq!(page.rows[0].1["record"]["phase"], "working");
            return;
        }
    }
    panic!("a consumed-event SQL failure lost the only changed source");
}

#[test]
fn later_pages_do_not_reuse_a_pinned_ready_bit_after_a_publisher() {
    let root = fixture();
    let mut changed = record();
    changed.as_object_mut().unwrap().remove("shell_jobs");
    for n in 0..256 {
        changed["name"] = json!(format!("codex/example/paged-{n}"));
        let path = root.path().join(format!(
            "{}.json",
            storage::label_key(string(&changed, "name"))
        ));
        storage::atomic(&path, &changed.to_string()).unwrap();
    }
    assert!(repair_to_end(root.path()).complete);
    let reader = pages::Reader::open(root.path()).unwrap();
    let first = reader.page(None).unwrap();
    assert!(first.fresh && first.next.is_some());
    let (source, seed) = &first.rows[0];
    let selected = root.path().join(source);
    changed = seed["record"].clone();
    changed["phase"] = json!("working");
    // Pause a real publisher after its durable intent, before any file change.
    // Every native row/parent stamp and coverage-gap witness stays unchanged.
    let mut store = Store::open(root.path()).unwrap();
    let key = Key::at(root.path(), &selected).unwrap();
    let projected = Seed::encode(&key, &changed).unwrap();
    store.reserve(&key, Some(&projected)).unwrap();
    drop(store);
    assert!(!root
        .path()
        .join("listing-metadata/coverage-lost.json")
        .exists());
    // Current readiness must be checked outside the old pinned transaction.
    assert!(
        !reader.page(first.next.as_ref()).unwrap().fresh,
        "a later page reused ready=true from the pinned SQLite transaction"
    );
}

#[test]
#[cfg(target_os = "linux")]
fn an_incremental_rename_shadow_removes_its_old_snapshot_row() {
    let root = fixture();
    let selected = path(root.path());
    let mut value = record();
    value.as_object_mut().unwrap().remove("shell_jobs");
    storage::atomic(&selected, &value.to_string()).unwrap();
    let mut work = None;
    for _ in 0..128 {
        if background_at(root.path(), &mut work).unwrap().complete {
            break;
        }
    }
    assert_eq!(
        pages::Reader::open(root.path())
            .unwrap()
            .page(None)
            .unwrap()
            .rows
            .len(),
        1
    );
    value["rename_shadow"] = json!(true);
    storage::atomic(&selected, &value.to_string()).unwrap();
    for _ in 0..128 {
        if background_at(root.path(), &mut work).unwrap().complete {
            assert!(
                pages::Reader::open(root.path())
                    .unwrap()
                    .page(None)
                    .unwrap()
                    .rows
                    .is_empty(),
                "incremental clone retained a shadow from its old generation"
            );
            return;
        }
    }
    panic!("rename shadow did not settle");
}
