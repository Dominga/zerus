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
    let (_temp, state, mut store) = fixture(false);
    let path = state.join(format!(
        "{:x}.json",
        Sha256::digest(b"codex/example/search")
    ));
    fs::write(path, "PRIVATE_INVALID_BODY").unwrap();
    discover(&store, &state);
    store
        .consume(&state, &budget(128), &AtomicBool::new(false))
        .unwrap();
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
