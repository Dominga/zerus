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
    store
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
}

#[test]
fn queue_consumption_resumes_public_archive_content_and_updates_fallback_counters() {
    let (_temp, state, mut store) = fixture(true);
    discover(&store, &state);
    let first = store
        .consume(&state, &budget(2), &AtomicBool::new(false))
        .unwrap();
    assert_eq!(first.visited_paths, 1);
    assert_eq!(first.ingestion.records, 2);
    assert!(first.ingestion.files <= 32);
    assert!(first.ingestion.bytes <= 4 << 20);
    assert_eq!(
        store.index.query("needle", 100, None).unwrap().hits.len(),
        2
    );
    store
        .consume(&state, &budget(128), &AtomicBool::new(false))
        .unwrap();
    assert_eq!(
        store.index.query("needle", 100, None).unwrap().hits.len(),
        6
    );
    let (pending, fallback): (i64, i64) = store
        .index
        .connection()
        .query_row(
            "SELECT pending,fallback FROM catalog_totals WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((pending, fallback), (0, 0));
}

#[test]
fn live_history_keeps_fresh_legacy_fallback_after_cache_ingestion() {
    let (_temp, state, mut store) = fixture(false);
    discover(&store, &state);
    store
        .consume(&state, &budget(128), &AtomicBool::new(false))
        .unwrap();
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
