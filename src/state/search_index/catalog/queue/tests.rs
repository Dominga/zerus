use super::*;

fn fixture(archive: bool) -> (tempfile::TempDir, PathBuf, Store) {
    let temp = tempfile::tempdir().unwrap();
    let state = temp.path().join("state");
    fs::create_dir(&state).unwrap();
    let conversation = uuid::Uuid::new_v4().to_string();
    let transcript = temp.path().join(format!("{conversation}.jsonl"));
    let rows=(1..=6).map(|id|json!({"type":"event_msg","timestamp":id,"payload":{"type":"agent_message","id":format!("m{id}"),"message":format!("needle {id}"),"phase":"final"}}).to_string()+"\n").collect::<String>();
    fs::write(&transcript, rows).unwrap();
    let mut record = json!({"version":1,"name":"codex/example/search","run_id":"fixture-run","conversation_id":conversation,"agent":"codex","cwd":state,"transcript":transcript,"updated":42});
    let path = if archive {
        let id = uuid::Uuid::new_v4().to_string();
        record["archive_id"] = json!(id);
        record["archived_at"] = json!(42);
        fs::create_dir(state.join("archives")).unwrap();
        state.join("archives").join(format!("{id}.json"))
    } else {
        state.join(format!(
            "{:x}.json",
            Sha256::digest(b"codex/example/search")
        ))
    };
    fs::write(path, record.to_string()).unwrap();
    let store = Store::open(&state.join("content-search-v1")).unwrap();
    (temp, state, store)
}
fn budget(records: usize) -> Budget {
    Budget {
        files: 32,
        directory_entries: 2000,
        records,
        bytes: 4 << 20,
        deadline: Instant::now() + Duration::from_secs(1),
    }
}
fn discover(store: &Store, state: &Path) {
    for _ in 0..32 {
        let progress = store
            .discover(
                state,
                &DiscoveryBudget {
                    entries: 100,
                    path_bytes: 8 << 20,
                    deadline: Instant::now() + Duration::from_secs(1),
                },
                &AtomicBool::new(false),
            )
            .unwrap();
        if progress.complete {
            return;
        }
    }
    panic!("fixture discovery did not finish within 32 bounded passes");
}
fn consume_until(store: &mut Store, state: &Path, ready: impl Fn(&Store) -> bool) {
    for _ in 0..32 {
        store
            .consume(state, &budget(128), &AtomicBool::new(false))
            .unwrap();
        if ready(store) {
            return;
        }
    }
    panic!("fixture consumption did not finish within 32 bounded passes");
}
fn totals(store: &Store) -> (i64, i64) {
    store
        .index
        .connection()
        .query_row(
            "SELECT pending,fallback FROM catalog_totals WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
}

#[test]
fn queue_consumption_resumes_public_archive_content_and_updates_fallback_counters() {
    let (_temp, state, mut store) = fixture(true);
    discover(&store, &state);
    let mut records = 0;
    let mut visited = false;
    for _ in 0..32 {
        let first = store
            .consume(&state, &budget(2 - records), &AtomicBool::new(false))
            .unwrap();
        assert!(first.visited_paths <= 1);
        visited |= first.visited_paths == 1;
        records += first.ingestion.records;
        assert!(records <= 2);
        assert!(first.ingestion.files <= 32);
        assert!(first.ingestion.bytes <= 4 << 20);
        if records == 2 {
            break;
        }
    }
    assert!(visited);
    assert_eq!(records, 2);
    assert_eq!(
        store.index.query("needle", 100, None).unwrap().hits.len(),
        2
    );
    consume_until(&mut store, &state, |store| totals(store) == (0, 0));
    assert_eq!(
        store.index.query("needle", 100, None).unwrap().hits.len(),
        6
    );
    let (pending, fallback) = totals(&store);
    assert_eq!((pending, fallback), (0, 0));
}

#[test]
fn live_history_keeps_fresh_legacy_fallback_after_cache_ingestion() {
    let (_temp, state, mut store) = fixture(false);
    discover(&store, &state);
    consume_until(&mut store, &state, |store| {
        totals(store) == (0, 1) && store.index.status().unwrap().messages == 6
    });
    assert_eq!(
        store.index.query("needle", 100, None).unwrap().hits.len(),
        6
    );
    let fallback: i64 = store
        .index
        .connection()
        .query_row("SELECT fallback FROM catalog_totals WHERE id=1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(fallback, 1);
}

#[test]
fn large_job_records_progress_through_discovery_and_catchup() {
    for archive in [false, true] {
        let (_temp, state, mut store) = fixture(archive);
        let path = if archive {
            fs::read_dir(state.join("archives"))
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path()
        } else {
            state.join(format!(
                "{:x}.json",
                Sha256::digest(b"codex/example/search")
            ))
        };
        let mut record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        record["shell_jobs"] = json!({"command":"private-shell-command ".repeat(70_000)});
        fs::write(&path, record.to_string()).unwrap();
        assert!(fs::metadata(&path).unwrap().len() > RECORD_BYTES as u64);
        discover(&store, &state);
        let mut small = budget(2);
        small.bytes = RECORD_BYTES;
        let deferred = store
            .consume(&state, &small, &AtomicBool::new(false))
            .unwrap();
        assert!(deferred.ingestion.stopped && deferred.ingestion.errors.is_empty());
        assert!(deferred.ingestion.bytes <= small.bytes);
        assert_eq!(totals(&store).0, 1);
        for _ in 0..32 {
            let progress = store
                .consume(&state, &budget(2), &AtomicBool::new(false))
                .unwrap();
            assert!(progress.ingestion.bytes <= 4 << 20);
            assert!(progress.ingestion.files <= 32 && progress.ingestion.records <= 2);
            if totals(&store).0 == 0 {
                break;
            }
        }
        assert_eq!(totals(&store), (0, i64::from(!archive)));
        assert_eq!(store.index.query("needle", 50, None).unwrap().hits.len(), 6);
    }
}

#[test]
fn a_record_beyond_this_slice_stays_pending_without_blocking_other_sessions() {
    let (_temp, state, mut store) = fixture(false);
    let path = state.join(format!(
        "{:x}.json",
        Sha256::digest(b"codex/example/search")
    ));
    let mut record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    record["shell_jobs"] = json!({"command":"private command ".repeat(300_000)});
    fs::write(&path, record.to_string()).unwrap();
    assert!(fs::metadata(&path).unwrap().len() > (4 << 20));
    discover(&store, &state);
    // Ensure the expensive path is staged before a healthy session.
    record["name"] = json!("codex/example/other");
    record.as_object_mut().unwrap().remove("shell_jobs");
    fs::write(
        state.join(format!("{:x}.json", Sha256::digest(b"codex/example/other"))),
        record.to_string(),
    )
    .unwrap();
    discover(&store, &state);
    for _ in 0..32 {
        let progress = store
            .consume(&state, &budget(2), &AtomicBool::new(false))
            .unwrap();
        assert!(progress.ingestion.bytes <= 4 << 20);
        assert!(progress.ingestion.files <= 32 && progress.ingestion.records <= 2);
        if totals(&store).0 == 1 && store.index.query("needle", 50, None).unwrap().hits.len() == 6 {
            break;
        }
    }
    assert_eq!(store.index.query("needle", 50, None).unwrap().hits.len(), 6);
    assert_eq!(totals(&store).0, 1);
    let skipped: i64 = store
        .index
        .connection()
        .query_row("SELECT sum(skipped) FROM catalog_queue", [], |r| r.get(0))
        .unwrap();
    assert_eq!(skipped, 0);
}

#[test]
fn invalid_metadata_is_marked_unavailable_without_a_fake_source_or_body() {
    invalid_metadata_preparation(false);
}

#[test]
fn invalid_metadata_preparation_resumes_after_an_expired_slice() {
    invalid_metadata_preparation(true);
}

fn invalid_metadata_preparation(expire_first: bool) {
    let (_temp, state, mut store) = fixture(false);
    let path = state.join(format!(
        "{:x}.json",
        Sha256::digest(b"codex/example/search")
    ));
    fs::write(&path, "PRIVATE_INVALID_BODY").unwrap();
    discover(&store, &state);
    let done = |store: &Store| -> bool {
        store
            .index
            .connection()
            .query_row(
                "SELECT done FROM catalog_queue WHERE path=?",
                [path.to_str().unwrap()],
                |row| row.get(0),
            )
            .unwrap()
    };
    let mut first = budget(128);
    if expire_first {
        first.deadline = Instant::now();
    }
    let progress = store
        .consume(&state, &first, &AtomicBool::new(false))
        .unwrap();
    if expire_first {
        assert!(progress.ingestion.stopped);
        assert!(
            !done(&store),
            "an expired slice must leave preparation pending"
        );
    }
    // A valid bounded consume may yield before this row. Preparation waits
    // for the exact queue outcome, retaining every slice's original limits.
    if !done(&store) {
        consume_until(&mut store, &state, done);
    }
    let (done, skipped): (bool, i64) = store
        .index
        .connection()
        .query_row("SELECT done,skipped FROM catalog_queue LIMIT 1", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert!(done);
    assert_eq!(skipped, 1);
    assert_eq!(store.index.status().unwrap().sources, 0);
}

#[test]
fn a_retired_source_marks_current_queue_references_for_fallback() {
    let (_temp, state, mut store) = fixture(true);
    discover(&store, &state);
    store
        .consume(&state, &budget(128), &AtomicBool::new(false))
        .unwrap();
    let key: String = store
        .index
        .connection()
        .query_row("SELECT source FROM catalog_queue LIMIT 1", [], |r| r.get(0))
        .unwrap();
    store.retire(&key).unwrap();
    let fallback: i64 = store
        .index
        .connection()
        .query_row("SELECT fallback FROM catalog_totals WHERE id=1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(fallback, 1);
    assert!(store
        .index
        .query("needle", 100, None)
        .unwrap()
        .hits
        .is_empty());
}

#[test]
fn deleting_an_archive_prunes_its_queue_and_public_text_on_the_next_catchup() {
    let (_temp, state, mut store) = fixture(true);
    discover(&store, &state);
    store
        .consume(&state, &budget(128), &AtomicBool::new(false))
        .unwrap();
    let archive = fs::read_dir(state.join("archives"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    fs::remove_file(archive).unwrap();
    discover(&store, &state);
    let progress = store
        .consume(&state, &budget(128), &AtomicBool::new(false))
        .unwrap();
    assert_eq!(progress.pruned_paths, 1);
    assert_eq!(store.catalog_status().unwrap()["queued_paths"], 0);
    assert_eq!(store.index.status().unwrap().messages, 0);
}

#[test]
fn cancellation_does_not_consume_or_mark_a_source_complete() {
    let (_temp, state, mut store) = fixture(true);
    discover(&store, &state);
    let progress = store
        .consume(&state, &budget(128), &AtomicBool::new(true))
        .unwrap();
    assert_eq!(progress.visited_paths, 0);
    assert!(progress.ingestion.stopped);
    assert_eq!(store.catalog_status().unwrap()["pending_paths"], 1);
    assert_eq!(store.index.status().unwrap().messages, 0);
}

#[test]
fn queue_ranges_use_bounded_index_searches_including_retired_references() {
    let (_temp, state, store) = fixture(true);
    discover(&store, &state);
    for (sql, index) in [
        ("SELECT id FROM catalog_queue WHERE done=0 AND id>4 ORDER BY id LIMIT 16", "catalog_pending"),
        ("SELECT id FROM catalog_queue WHERE seen<'generation' ORDER BY seen,id LIMIT 16", "catalog_seen"),
        ("SELECT id FROM catalog_queue WHERE seen>'generation' ORDER BY seen,id LIMIT 16", "catalog_seen"),
        ("SELECT id FROM catalog_queue WHERE source='source' AND fallback=0 ORDER BY id LIMIT 16", "catalog_source_fallback"),
        ("SELECT id FROM catalog_queue WHERE identity IS NULL LIMIT 1", "catalog_known"),
    ] {
        let plan = store.index.connection().prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap()
            .query_map([], |r|r.get::<_,String>(3)).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap().join("\n");
        assert!(plan.contains(index) && plan.contains("SEARCH"), "{plan}");
        assert!(!plan.contains("SCAN catalog_queue"), "{plan}");
    }
}

// A saved binding can grow beyond every allowed publication slice.
#[test]
fn grown_binding_beyond_the_publication_budget_does_not_fence_later_hits() {
    let (_temp, state, mut store) = fixture(false);
    let path = state.join(format!(
        "{:x}.json",
        Sha256::digest(b"codex/example/search")
    ));
    discover(&store, &state);
    for _ in 0..32 {
        store
            .consume(&state, &budget(128), &AtomicBool::new(false))
            .unwrap();
        if totals(&store).0 == 0 {
            break;
        }
    }
    let publication = || Budget {
        files: 128,
        directory_entries: 2000,
        records: 512,
        bytes: 8 << 20,
        deadline: Instant::now() + Duration::from_millis(500),
    };
    let show = |label: &str, page: &Value| {
        eprintln!(
            "{label}: results={} next={} truncated={} coverage={} unavailable={}",
            page["results"].as_array().map_or(0, Vec::len),
            page["next_cursor"],
            page["truncated"],
            page["index_coverage"]["state"],
            page["unavailable_sessions"]
        )
    };
    let page = store
        .publish(
            &state,
            "needle",
            2,
            None,
            &publication(),
            &AtomicBool::new(false),
        )
        .unwrap();
    show("indexed, first page", &page);
    let mut record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    record["shell_jobs"] = json!({"big":{"command":"x".repeat(9 << 20)}});
    fs::write(&path, record.to_string()).unwrap();
    record["name"] = json!("codex/example/other");
    record["run_id"] = json!("fixture-other-run");
    record.as_object_mut().unwrap().remove("shell_jobs");
    fs::write(
        state.join(format!("{:x}.json", Sha256::digest(b"codex/example/other"))),
        record.to_string(),
    )
    .unwrap();
    eprintln!("grown binding bytes={}", fs::metadata(&path).unwrap().len());
    // As read() does for a first page: discovery, one interactive consume, publish.
    discover(&store, &state);
    let consumed = store.consume(&state, &Budget::interactive(), &AtomicBool::new(false));
    eprintln!(
        "interactive consume ok={} progress={} pending={:?}",
        consumed.is_ok(),
        consumed
            .as_ref()
            .map_or(String::new(), |p| serde_json::to_string(p).unwrap()),
        totals(&store)
    );
    consumed.unwrap();
    // That pass has a 250 ms wall-clock deadline. Ingesting the later session
    // takes about 14 journaled index commits, so slow durable writes (Windows
    // under a full suite) can stop it mid-source with valid pending work.
    // Resume the fixture with bounded passes, as consume_until does, until the
    // later healthy session is fully indexed; the grown binding stays pending.
    let other = state.join(format!("{:x}.json", Sha256::digest(b"codex/example/other")));
    let other_done = |store: &Store| -> bool {
        store
            .index
            .connection()
            .query_row(
                "SELECT done FROM catalog_queue WHERE path=?",
                [other.to_str().unwrap()],
                |r| r.get(0),
            )
            .unwrap()
    };
    let mut passes = 0;
    while !other_done(&store) {
        assert!(
            passes < 32,
            "the later session was not indexed within 32 bounded passes"
        );
        store
            .consume(&state, &budget(128), &AtomicBool::new(false))
            .unwrap();
        passes += 1;
    }
    eprintln!("resumed passes={passes} pending={:?}", totals(&store));
    let mut cursor: Option<String> = None;
    let mut stalled = 0;
    let mut returned = 0;
    let mut unavailable = 0;
    for round in 0..4 {
        let page = store
            .publish(
                &state,
                "needle",
                2,
                cursor.as_deref(),
                &publication(),
                &AtomicBool::new(false),
            )
            .unwrap();
        show(&format!("after growth, page {round}"), &page);
        returned += page["results"].as_array().unwrap().len();
        unavailable += page["unavailable_sessions"].as_u64().unwrap();
        assert_ne!(page["index_coverage"]["state"], "ready");
        let next = page["next_cursor"].as_str().map(str::to_owned);
        if round > 0
            && next.is_some()
            && next == cursor
            && page["results"].as_array().unwrap().is_empty()
        {
            stalled += 1;
        }
        cursor = next;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(
        stalled, 0,
        "pagination returned its own cursor with no results"
    );
    assert!(cursor.is_none());
    assert!(totals(&store).0 > 0);
    assert_eq!(
        returned, 6,
        "the later healthy session must remain reachable"
    );
    assert!(unavailable > 0);
    assert_eq!(
        store.index.query("needle", 50, None).unwrap().hits.len(),
        12,
        "unavailable proof must not retire the source"
    );
}

#[test]
fn a_fresh_slice_resumes_a_proof_that_only_exhausted_the_previous_slice() {
    let (_temp, state, mut store) = fixture(false);
    let path = state.join(format!(
        "{:x}.json",
        Sha256::digest(b"codex/example/search")
    ));
    let mut record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    record["shell_jobs"] = json!({"command":"private command ".repeat(90_000)});
    fs::write(&path, record.to_string()).unwrap();
    record["name"] = json!("codex/example/other");
    record["run_id"] = json!("fixture-other-run");
    let other = state.join(format!("{:x}.json", Sha256::digest(b"codex/example/other")));
    fs::write(&other, record.to_string()).unwrap();
    let bytes = fs::metadata(&path)
        .unwrap()
        .len()
        .max(fs::metadata(&other).unwrap().len()) as usize
        + 1024;
    discover(&store, &state);
    for _ in 0..32 {
        store
            .consume(&state, &budget(128), &AtomicBool::new(false))
            .unwrap();
        if totals(&store).0 == 0 {
            break;
        }
    }
    assert_eq!(totals(&store).0, 0);
    let mut cursor: Option<String> = None;
    let mut returned = std::collections::BTreeSet::new();
    for _ in 0..16 {
        let mut slice = budget(128);
        slice.bytes = bytes;
        let page = store
            .publish(
                &state,
                "needle",
                50,
                cursor.as_deref(),
                &slice,
                &AtomicBool::new(false),
            )
            .unwrap();
        assert_eq!(page["unavailable_sessions"], 0);
        assert!(page["proof"]["bytes"].as_u64().unwrap() <= bytes as u64);
        let hits = page["results"].as_array().unwrap();
        assert!(!hits.is_empty());
        for hit in hits {
            assert!(returned.insert((hit["name"].to_string(), hit["message_id"].to_string())));
        }
        let next = page["next_cursor"].as_str().map(str::to_owned);
        if next.is_none() {
            cursor = None;
            break;
        }
        assert_ne!(next, cursor);
        cursor = next;
    }
    assert!(cursor.is_none());
    assert_eq!(returned.len(), 12);
}

// Every journaled commit flushes the disk several times. A queued source step
// is one commit: its index rows never become durable without its input cursor,
// source row and queue completion, however many bounded passes it takes.
#[test]
fn a_queued_source_step_commits_rows_cursor_and_completion_together() {
    use std::collections::BTreeSet;
    use std::sync::{Arc, Mutex};
    let (_temp, state, mut store) = fixture(false);
    discover(&store, &state);
    let open = Arc::new(Mutex::new(BTreeSet::<String>::new()));
    let commits = Arc::new(Mutex::new(Vec::<BTreeSet<String>>::new()));
    let (tables, committed) = (open.clone(), commits.clone());
    store.index.connection().update_hook(Some(
        move |_: rusqlite::hooks::Action, _: &str, table: &str, _: i64| {
            tables.lock().unwrap().insert(table.to_owned());
        },
    ));
    store.index.connection().commit_hook(Some(move || {
        committed
            .lock()
            .unwrap()
            .push(std::mem::take(&mut *open.lock().unwrap()));
        false
    }));
    consume_until(&mut store, &state, |store| totals(store).0 == 0);
    store.index.connection().commit_hook(None::<fn() -> bool>);
    store
        .index
        .connection()
        .update_hook(None::<fn(rusqlite::hooks::Action, &str, &str, i64)>);
    let commits = commits.lock().unwrap();
    let steps: Vec<_> = commits.iter().filter(|t| t.contains("messages")).collect();
    assert!(!steps.is_empty(), "{commits:?}");
    for step in steps {
        for table in ["input_cursors", "sources", "catalog_queue"] {
            assert!(
                step.contains(table),
                "{table} outside its step: {commits:?}"
            );
        }
    }
    assert_eq!(
        store.index.query("needle", 100, None).unwrap().hits.len(),
        6
    );
}

#[test]
fn an_interrupted_source_step_persists_nothing_and_a_new_step_commits_it_whole() {
    let (_temp, state, mut store) = fixture(false);
    let path = state.join(format!(
        "{:x}.json",
        Sha256::digest(b"codex/example/search")
    ));
    let record: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    let identity = Identity {
        name: "codex/example/search".into(),
        run_id: "fixture-run".into(),
        conversation_id: record["conversation_id"].as_str().unwrap().into(),
        archive_id: String::new(),
    };
    let budget = || Budget {
        deadline: Instant::now() + Duration::from_secs(2),
        ..budget(128)
    };
    let database = state.join("content-search-v1/index.sqlite3");
    let durable = || {
        let other = rusqlite::Connection::open_with_flags(
            &database,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let count = |table: &str| -> i64 {
            other
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
                .unwrap()
        };
        (count("messages"), count("input_cursors"))
    };
    // A nested step joins the open one: nothing is durable before it commits.
    assert!(store.begin_step().unwrap());
    store
        .step(
            &state,
            std::slice::from_ref(&identity),
            &budget(),
            &AtomicBool::new(false),
        )
        .unwrap();
    assert!(!store
        .index
        .query("needle", 100, None)
        .unwrap()
        .hits
        .is_empty());
    assert_eq!(durable(), (0, 0));
    // The process ends before its commit; SQLite rolls the whole step back.
    drop(store);
    assert_eq!(durable(), (0, 0));
    let mut store = Store::open(&state.join("content-search-v1")).unwrap();
    for _ in 0..32 {
        store
            .step(
                &state,
                std::slice::from_ref(&identity),
                &budget(),
                &AtomicBool::new(false),
            )
            .unwrap();
        let (messages, cursors) = durable();
        // Rows and their cursor only ever become durable together.
        assert_eq!(cursors, 1, "{messages} rows without a cursor");
        if messages == 6 {
            return;
        }
    }
    panic!("the source was not indexed within 32 bounded steps");
}

#[test]
fn a_step_that_sqlite_cancelled_refuses_further_step_writes() {
    let (_temp, _state, mut store) = fixture(false);
    assert!(store.begin_step().unwrap());
    // SQLite can end the transaction itself after SQLITE_FULL, IOERR or NOMEM.
    store.index.connection().execute_batch("ROLLBACK").unwrap();
    assert!(store.step_open().is_err());
    // A nested step must not silently open a fresh transaction.
    assert!(store.begin_step().is_err());
    assert!(store.end_step(true).is_err());
    assert!(!store.step);
    assert!(store.begin_step().unwrap());
    store.end_step(true).unwrap();
    assert!(store.index.connection().is_autocommit());
}

#[test]
fn a_cancelled_snapshot_put_never_persists_a_cursor_past_rolled_back_rows() {
    use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
    use std::sync::Arc;
    let (_temp, state, mut store) = fixture(false);
    let path = state.join(format!(
        "{:x}.json",
        Sha256::digest(b"codex/example/search")
    ));
    let mut record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    record["last_message"] = json!("needle snapshot");
    record["last_event_at"] = json!(43);
    fs::write(&path, record.to_string()).unwrap();
    let identity = Identity {
        name: "codex/example/search".into(),
        run_id: "fixture-run".into(),
        conversation_id: record["conversation_id"].as_str().unwrap().into(),
        archive_id: String::new(),
    };
    let budget = || Budget {
        deadline: Instant::now() + Duration::from_secs(2),
        ..budget(128)
    };
    let database = state.join("content-search-v1/index.sqlite3");
    let durable = || {
        let other = rusqlite::Connection::open_with_flags(
            &database,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let count = |table: &str| -> i64 {
            other
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
                .unwrap()
        };
        (count("messages"), count("input_cursors"))
    };
    let inserted = Arc::new(AtomicUsize::new(0));
    let armed = Arc::new(AtomicBool::new(false));
    let (count, arm, fire) = (inserted.clone(), armed.clone(), armed.clone());
    store.index.connection().update_hook(Some(
        move |action: rusqlite::hooks::Action, _: &str, table: &str, _: i64| {
            if action == rusqlite::hooks::Action::SQLITE_INSERT
                && table == "messages"
                && count.fetch_add(1, SeqCst) == 6
            {
                arm.store(true, SeqCst);
            }
        },
    ));
    store
        .index
        .connection()
        .progress_handler(1, Some(move || fire.swap(false, SeqCst)));
    let cancelled = store.step(
        &state,
        std::slice::from_ref(&identity),
        &budget(),
        &AtomicBool::new(false),
    );
    store
        .index
        .connection()
        .progress_handler(1, None::<fn() -> bool>);
    store
        .index
        .connection()
        .update_hook(None::<fn(rusqlite::hooks::Action, &str, &str, i64)>);
    assert!(inserted.load(SeqCst) >= 7, "the snapshot put never ran");
    assert!(store.index.connection().is_autocommit());
    assert!(cancelled.is_err(), "the cancelled step reported success");
    let (messages, cursors) = durable();
    assert_eq!(
        cursors, 0,
        "cursor persisted past {} rolled-back rows (durable rows: {messages})",
        6
    );
    for _ in 0..32 {
        store
            .step(
                &state,
                std::slice::from_ref(&identity),
                &budget(),
                &AtomicBool::new(false),
            )
            .unwrap();
        if durable().0 == 7 {
            break;
        }
    }
    assert_eq!(
        store.index.query("needle", 100, None).unwrap().hits.len(),
        7
    );
}
