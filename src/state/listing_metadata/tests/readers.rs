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
    store.db.execute_batch("CREATE TRIGGER fail_hint BEFORE INSERT ON dirty BEGIN SELECT RAISE(FAIL,'Resource temporarily unavailable (os error 11): private consumed-event cut'); END;").unwrap();
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
    assert!(
        !root
            .path()
            .join("listing-metadata/coverage-lost.json")
            .exists()
    );
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

#[test]
fn parent_point_reads_share_one_query_byte_budget() {
    let root = fixture();
    let value = record();
    storage::atomic(&path(root.path()), &value.to_string()).unwrap();
    assert!(repair_to_end(root.path()).complete);
    let reader = pages::Reader::open(root.path()).unwrap();
    for _ in 0..4096 {
        if let Err(error) = reader.parent(&value) {
            assert!(error.to_string().contains("budget"), "{error}");
            return;
        }
    }
    panic!("parent overlays bypassed the shared query byte budget");
}

#[test]
fn pinned_pages_retain_their_own_rows_during_generation_gc_and_epoch_replacement() {
    let root = fixture();
    let mut value = record();
    for n in 0..260 {
        value["name"] = json!(format!("codex/example/retained-{n}"));
        let path = root.path().join(format!(
            "{}.json",
            storage::label_key(string(&value, "name"))
        ));
        storage::atomic(&path, &value.to_string()).unwrap();
    }
    assert!(repair_to_end(root.path()).complete);
    let reader = pages::Reader::open(root.path()).unwrap();
    let first = reader.page(None).unwrap();
    let token = first.next.unwrap();
    let original_second = reader.page(Some(&token)).unwrap().rows;
    for _ in 0..4 {
        assert!(repair_to_end(root.path()).complete);
    }
    let retained = reader.page(Some(&token)).unwrap();
    assert!(!retained.fresh);
    assert_eq!(
        retained.rows, original_second,
        "GC mixed generation bodies in a pinned view"
    );
    assert!(
        pages::Reader::open(root.path())
            .unwrap()
            .page(Some(&token))
            .is_err(),
        "collected cursor returned a partial success"
    );
    assert!(
        Store::recover(
            root.path(),
            &anyhow::anyhow!("Display root/store identity changed")
        )
        .unwrap()
    );
    assert!(repair_to_end(root.path()).complete);
    assert!(
        reader.page(None).is_err(),
        "a replaced native cache object reused the old reader"
    );
    assert!(
        pages::Reader::open(root.path())
            .unwrap()
            .page(Some(&token))
            .is_err(),
        "another epoch adopted an old cursor"
    );
}

#[test]
#[cfg(target_os = "linux")]
fn cold_census_catches_a_real_overlapping_canonical_writer() {
    let root = fixture();
    let mut value = record();
    value.as_object_mut().unwrap().remove("shell_jobs");
    for n in 0..2048 {
        value["name"] = json!(format!("codex/example/cold-{n}"));
        let path = root.path().join(format!(
            "{}.json",
            storage::label_key(string(&value, "name"))
        ));
        storage::atomic(&path, &value.to_string()).unwrap();
    }
    let selected = root.path().join(format!(
        "{}.json",
        storage::label_key(string(&value, "name"))
    ));
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let publications = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    std::thread::scope(|scope| {
        let worker_stop = stop.clone();
        let publications = publications.clone();
        let selected = selected.clone();
        let root_path = root.path();
        scope.spawn(move || {
            while !worker_stop.load(std::sync::atomic::Ordering::SeqCst) {
                value["last_event_at"] = json!(now());
                write_at(root_path, &selected, &value, |publication| {
                    storage::atomic_observed(&selected, &value.to_string(), publication)
                })
                .unwrap();
                publications.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(8));
            }
        });
        let mut work = None;
        let until = Instant::now() + Duration::from_secs(15);
        let mut complete = false;
        while Instant::now() < until {
            if background_at(root.path(), &mut work).is_ok_and(|stats| stats.complete) {
                complete = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(
            complete,
            "live writer starved discovery of stable late records"
        );
    });
    assert!(publications.load(std::sync::atomic::Ordering::SeqCst) > 1);
    assert!(repair_to_end(root.path()).complete);
    let reader = pages::Reader::open(root.path()).unwrap();
    let mut cursor = None;
    let mut rows = 0;
    loop {
        let page = reader.page(cursor.as_ref()).unwrap();
        assert!(page.fresh);
        rows += page.rows.len();
        cursor = page.next;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(rows, 2048);
}

#[test]
fn large_compact_rows_share_a_bounded_background_write_budget() {
    let root = fixture();
    let mut value = record();
    let children: serde_json::Map<String, Value> = (0..100)
        .map(|n| {
            (
                format!("child-{n}"),
                json!({"name":"Worker","state":"working","prompt":"x".repeat(1400)}),
            )
        })
        .collect();
    value["subagents"] = json!(children);
    for n in 0..12 {
        value["name"] = json!(format!("codex/example/large-{n}"));
        let path = root.path().join(format!(
            "{}.json",
            storage::label_key(string(&value, "name"))
        ));
        storage::atomic(&path, &value.to_string()).unwrap();
    }
    let mut work = None;
    let mut largest = 0;
    for _ in 0..128 {
        let stats = background_at(root.path(), &mut work).unwrap();
        largest = largest.max(stats.seed_bytes);
        assert!(
            stats.seed_bytes <= 512 << 10,
            "one background step wrote unbounded seed bytes"
        );
        if stats.done {
            assert!(stats.complete);
            assert!(largest > 256 << 10);
            return;
        }
    }
    panic!("bounded large rows did not finish");
}

#[test]
#[cfg(target_os = "linux")]
fn an_early_advisory_writer_collision_retains_the_census_and_native_feed() {
    let root = fixture();
    let mut value = record();
    for n in 0..2048 {
        value["name"] = json!(format!("codex/example/contention-{n}"));
        let path = root.path().join(format!(
            "{}.json",
            storage::label_key(string(&value, "name"))
        ));
        storage::atomic(&path, &value.to_string()).unwrap();
    }
    let mut work = None;
    assert!(!background_at(root.path(), &mut work).unwrap().done);
    let store = Store::open(root.path()).unwrap();
    let selected = root.path().join(format!(
        "{}.json",
        storage::label_key(string(&value, "name"))
    ));
    let before: (i64, i64, String) = store
        .db
        .query_row(
            "SELECT generation,folder,after_key FROM scan WHERE id=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    for _ in 0..64 {
        value["phase"] = json!("working");
        let mut calls = 0;
        write_at(root.path(), &selected, &value, |publication| {
            calls += 1;
            storage::atomic_observed(&selected, &value.to_string(), publication)
        })
        .unwrap();
        assert_eq!(calls, 1);
        let result = background_at(root.path(), &mut work);
        assert!(
            work.is_some(),
            "early writer collision discarded native feed/census: {result:?}"
        );
        let stats = result.expect("early writer collision was not a bounded retry");
        assert_eq!((stats.entries, stats.rows, stats.bytes), (0, 0, 0));
        assert!(
            !stats.complete && !stats.done,
            "a busy publisher certified coverage"
        );
    }
    let after: (i64, i64, String) = store
        .db
        .query_row(
            "SELECT generation,folder,after_key FROM scan WHERE id=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        before, after,
        "pre-admission contention changed census position"
    );
    drop(store);
    for _ in 0..256 {
        if background_at(root.path(), &mut work).unwrap().complete {
            return;
        }
    }
    panic!("retained census did not resume");
}

#[test]
#[cfg(target_os = "linux")]
fn a_real_commit_busy_after_native_drain_requires_a_new_census() {
    let root = fixture();
    let selected = path(root.path());
    let mut value = record();
    storage::atomic(&selected, &value.to_string()).unwrap();
    let mut work = None;
    for _ in 0..128 {
        if background_at(root.path(), &mut work).unwrap().complete {
            break;
        }
    }
    // A real SQLite commit collision, not a mocked error or permission change.
    let database = root.path().join("listing-metadata/index.sqlite3");
    let reader = Connection::open(&database).unwrap();
    reader
        .execute_batch("PRAGMA journal_mode=DELETE; BEGIN; SELECT * FROM meta;")
        .unwrap();
    value["phase"] = json!("working");
    storage::atomic(&selected, &value.to_string()).unwrap();
    let error = background_at(root.path(), &mut work).unwrap_err();
    assert!(
        error.to_string().contains("COMMIT"),
        "failure did not reach commit: {error:#}"
    );
    assert!(error.downcast_ref::<rusqlite::Error>().is_some());
    assert!(
        work.is_none(),
        "post-drain SQL busy was confused with untouched writer admission"
    );
    reader
        .execute_batch("ROLLBACK; PRAGMA journal_mode=WAL;")
        .unwrap();
    drop(reader);
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
    panic!("commit-failure recovery lost the changed canonical source");
}

#[test]
fn a_generation_row_never_borrows_a_later_sources_stamp() {
    let root = fixture();
    let mut value = record();
    value["phase"] = json!("interrupted");
    for n in 0..2 {
        value["name"] = json!(format!("codex/example/stamp-{n}"));
        storage::atomic(
            &root.path().join(format!(
                "{}.json",
                storage::label_key(string(&value, "name"))
            )),
            &value.to_string(),
        )
        .unwrap();
    }
    assert!(repair_to_end(root.path()).complete);
    let (token, original) = {
        let store = Store::open(root.path()).unwrap();
        let first = store.page_limit(None, 1).unwrap();
        let token = first.next.unwrap();
        let second = store.page_limit(Some(&token), 1).unwrap();
        (token, second.rows[0].clone())
    };
    let selected = root.path().join(&original.0);
    let mut changed: Value = serde_json::from_slice(&fs::read(&selected).unwrap()).unwrap();
    changed["phase"] = json!("working");
    write_at(root.path(), &selected, &changed, |publication| {
        storage::atomic_observed(&selected, &changed.to_string(), publication)
    })
    .unwrap();
    let store = Store::open(root.path()).unwrap();
    let retained = store.page_limit(Some(&token), 1).unwrap();
    assert!(!retained.fresh);
    assert_eq!(retained.rows[0].1["record"]["phase"], "interrupted");
    assert_eq!(
        retained.rows[0].1["native_stamp"], original.1["native_stamp"],
        "immutable seed borrowed a different publisher's source stamp"
    );
}

#[test]
fn generation_collection_retires_a_cursor_before_the_first_partial_delete() {
    let root = fixture();
    let mut value = record();
    for n in 0..260 {
        value["name"] = json!(format!("codex/example/partial-gc-{n}"));
        storage::atomic(
            &root.path().join(format!(
                "{}.json",
                storage::label_key(string(&value, "name"))
            )),
            &value.to_string(),
        )
        .unwrap();
    }
    assert!(repair_to_end(root.path()).complete);
    let token = pages::Reader::open(root.path())
        .unwrap()
        .page_limit(None, 1)
        .unwrap()
        .next
        .unwrap();
    assert!(repair_to_end(root.path()).complete);
    assert!(repair_to_end(root.path()).complete);
    let mut work = None;
    let _ = background_at(root.path(), &mut work).unwrap();
    let store = Store::open(root.path()).unwrap();
    let remaining: i64 = store
        .db
        .query_row(
            "SELECT count(*) FROM snapshots WHERE generation=?1",
            [token.generation()],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        (1..260).contains(&remaining),
        "control did not stop during partial collection: {remaining}"
    );
    drop(store);
    assert!(
        pages::Reader::open(root.path())
            .unwrap()
            .page(Some(&token))
            .is_err(),
        "a half-collected generation returned a partial page"
    );
}
