use super::*;

fn index() -> Index {
    Index::new(Connection::open_in_memory().unwrap(), DEFAULT_FILE_BYTES).unwrap()
}
fn target(run: &str, archive: bool) -> Target {
    Target {
        name: "codex/example/search".into(),
        run_id: run.into(),
        conversation_id: format!("conversation-{run}"),
        archive_id: if archive {
            "10000000-0000-4000-8000-000000000001".into()
        } else {
            String::new()
        },
        agent: "codex".into(),
        cwd: "/work/example".into(),
        archived_at: if archive { 42.0 } else { 0.0 },
    }
}
fn message(id: usize, text: &str) -> Message {
    Message {
        id: format!("message-{id}"),
        at: id as f64,
        role: Role::Assistant,
        kind: "AgentMessage".into(),
        source: "codex_transcript".into(),
        seq: 0,
        text: text.into(),
    }
}
fn fill(index: &mut Index, key: &str, start: usize, count: usize, text: &str) {
    for chunk in (start..start + count)
        .collect::<Vec<_>>()
        .chunks(BATCH_DOCUMENTS)
    {
        index
            .ingest(
                key,
                &chunk.iter().map(|i| message(*i, text)).collect::<Vec<_>>(),
            )
            .unwrap();
    }
}

#[test]
fn unicode_literal_substrings_match_regex_simple_case_folding() {
    let mut index = index();
    let key = index.register(&target("one", false)).unwrap();
    index
        .ingest(
            &key,
            &[message(
                1,
                "МИГРАЦИЯ / ſource / Kelvin / ςigma / emoji 🦀🦀 / literal [.*]",
            )],
        )
        .unwrap();
    for query in ["миграция", "SOURCE", "kelvin", "ΣIGMA", "🦀🦀", "[.*]"] {
        let result = index.query(query, 100, None).unwrap();
        assert_eq!(result.hits.len(), 1, "{query}");
        assert!(
            !result.hits[0].message.text[result.hits[0].match_start..result.hits[0].match_end]
                .is_empty()
        );
    }
    assert!(index.query(".*absent", 100, None).unwrap().hits.is_empty());
    assert!(index.query("a", 100, None).is_err());
    assert!(index.query("a\nb", 100, None).is_err());
}

#[test]
fn cursor_is_query_limit_database_and_snapshot_bound() {
    let mut index = index();
    let key = index.register(&target("one", false)).unwrap();
    fill(&mut index, &key, 1, 12, "needle");
    let first = index.query("needle", 4, None).unwrap();
    assert_eq!(
        first
            .hits
            .iter()
            .map(|h| h.message.id.as_str())
            .collect::<Vec<_>>(),
        ["message-12", "message-11", "message-10", "message-9"]
    );
    let cursor = first.next_cursor.as_deref().unwrap();
    // Both newer and backdated insertions are excluded from this snapshot.
    index
        .ingest(&key, &[message(1000, "needle"), message(0, "needle")])
        .unwrap();
    let second = index.query("needle", 4, Some(cursor)).unwrap();
    assert_eq!(
        second
            .hits
            .iter()
            .map(|h| h.message.id.as_str())
            .collect::<Vec<_>>(),
        ["message-8", "message-7", "message-6", "message-5"]
    );
    let third = index
        .query("needle", 4, second.next_cursor.as_deref())
        .unwrap();
    assert_eq!(third.hits.len(), 4);
    // A later backdated candidate may require an empty final bounded page.
    let last = index
        .query("needle", 4, third.next_cursor.as_deref())
        .unwrap();
    assert!(last.hits.is_empty());
    assert!(last.next_cursor.is_none());
    assert!(index.query("NEEDLE", 4, Some(cursor)).is_err());
    assert!(index.query("needle", 5, Some(cursor)).is_err());
    assert!(super::tests::index()
        .query("needle", 4, Some(cursor))
        .is_err());
    assert!(index.query("needle", 4, Some("malformed")).is_err());
}

#[test]
fn a_partial_page_keeps_one_cursor_identity_and_does_not_skip_unconsumed_hits() {
    let mut index = index();
    let key = index.register(&target("one", false)).unwrap();
    fill(&mut index, &key, 1, 6, "needle");
    let start = index.start_cursor("needle", 4).unwrap();
    let first = index.query_part("needle", 4, 1, Some(&start)).unwrap();
    assert_eq!(first.hits[0].message.id, "message-6");
    let second = index
        .query_part("needle", 4, 2, first.next_cursor.as_deref())
        .unwrap();
    assert_eq!(
        second
            .hits
            .iter()
            .map(|h| h.message.id.as_str())
            .collect::<Vec<_>>(),
        ["message-5", "message-4"]
    );
    let third = index
        .query_part("needle", 4, 3, second.next_cursor.as_deref())
        .unwrap();
    assert_eq!(
        third
            .hits
            .iter()
            .map(|h| h.message.id.as_str())
            .collect::<Vec<_>>(),
        ["message-3", "message-2", "message-1"]
    );
    assert!(third.next_cursor.is_none());
    assert!(index.query_part("needle", 4, 0, Some(&start)).is_err());
}

#[test]
fn common_grams_and_hidden_rows_cannot_scan_beyond_a_page() {
    let mut index = index();
    let key = index.register(&target("one", false)).unwrap();
    fill(&mut index, &key, 1, 300, "ab unrelated");
    let first = index.query("ab missing", 100, None).unwrap();
    assert_eq!(first.candidates, CANDIDATES);
    assert!(first.hits.is_empty());
    let second = index
        .query("ab missing", 100, first.next_cursor.as_deref())
        .unwrap();
    assert_eq!(second.candidates, CANDIDATES);
    index.retire(&key).unwrap();
    let last = index
        .query("ab missing", 100, second.next_cursor.as_deref())
        .unwrap();
    assert!(last.hits.is_empty());
    assert_eq!(last.candidates, 44);
    assert!(last.next_cursor.is_none());
}

#[test]
fn retirement_hides_content_immediately_purges_boundedly_and_fences_old_producers() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("index.sqlite3");
    let mut index = Index::new(Connection::open(&path).unwrap(), DEFAULT_FILE_BYTES).unwrap();
    let archived = target("archive", true);
    let live = target("live", false);
    let archived_key = index.register(&archived).unwrap();
    let live_key = index.register(&live).unwrap();
    fill(
        &mut index,
        &archived_key,
        1,
        70,
        "needle disposable public text",
    );
    fill(&mut index, &live_key, 1, 1, "needle live");
    index.retire(&archived_key).unwrap();
    let page = index.query("needle", 100, None).unwrap();
    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].target, live);
    assert_eq!(index.status().unwrap().sources, 1);
    assert!(index
        .ingest(&archived_key, &[message(100, "delayed needle")])
        .is_err());
    assert!(index.register(&archived).is_err());
    assert!(index.purge(&live_key).is_err());
    assert_eq!(index.purge(&archived_key).unwrap(), 32);
    assert_eq!(index.purge(&archived_key).unwrap(), 32);
    assert_eq!(index.purge(&archived_key).unwrap(), 6);
    assert_eq!(index.purge(&archived_key).unwrap(), 0);
    assert_eq!(index.status().unwrap().messages, 1);
    drop(index);
    // secure_delete removes payload bytes, including from reusable free pages.
    assert!(!std::fs::read(&path)
        .unwrap()
        .windows(b"disposable public text".len())
        .any(|w| w == b"disposable public text"));
    let reopened = Index::new(Connection::open(path).unwrap(), DEFAULT_FILE_BYTES).unwrap();
    assert!(reopened.register(&archived).is_err());
    assert_eq!(reopened.query("needle", 100, None).unwrap().hits.len(), 1);
}

#[test]
fn coverage_and_constant_size_counters_do_not_claim_completeness() {
    let mut index = index();
    assert_eq!(index.status().unwrap().state, "cold");
    let key = index.register(&target("one", false)).unwrap();
    assert_eq!(index.status().unwrap().state, "indexing");
    index.catalog_ready(true).unwrap();
    index.source_progress(&key, false, true).unwrap();
    assert_eq!(index.status().unwrap().state, "partial");
    index.source_progress(&key, true, false).unwrap();
    assert_eq!(index.status().unwrap().state, "ready");
    let value = message(1, "needle");
    assert_eq!(index.ingest(&key, std::slice::from_ref(&value)).unwrap(), 1);
    assert_eq!(index.ingest(&key, &[value]).unwrap(), 0);
    assert_eq!(index.status().unwrap().messages, 1);
    assert_eq!(index.evict().unwrap(), 1);
    assert_eq!(index.status().unwrap().messages, 0);
    assert_eq!(index.status().unwrap().state, "partial");
    index.catalog_ready(false).unwrap();
    assert!(index.status().unwrap().evicted);
}

#[test]
fn failed_batches_do_not_commit_partial_text_and_replacement_keeps_one_message() {
    let mut index = index();
    let key = index.register(&target("one", false)).unwrap();
    let oversized = message(2, &"x".repeat(DOCUMENT_BYTES + 1));
    assert!(index
        .ingest(&key, &[message(1, "needle"), oversized])
        .is_err());
    assert_eq!(index.status().unwrap().messages, 0);
    assert!(index
        .ingest(&key, &vec![message(1, "needle"); BATCH_DOCUMENTS + 1])
        .is_err());
    index.ingest(&key, &[message(1, "old needle")]).unwrap();
    index.ingest(&key, &[message(1, "new needle")]).unwrap();
    let page = index.query("needle", 100, None).unwrap();
    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].message.text, "new needle");
    assert!(index
        .query("old needle", 100, None)
        .unwrap()
        .hits
        .is_empty());
}

#[test]
fn file_size_is_hard_capped_and_eviction_reuses_pages() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("index.sqlite3");
    let mut index = Index::new(Connection::open(&path).unwrap(), MIN_FILE_BYTES).unwrap();
    let key = index.register(&target("one", false)).unwrap();
    let large = "needle public text ".repeat(6000);
    let mut full = false;
    for id in 0..100 {
        if index.ingest(&key, &[message(id, &large)]).is_err() {
            full = true;
            break;
        }
    }
    assert!(full);
    let status = index.status().unwrap();
    assert!(status.file_bytes <= status.file_cap_bytes);
    assert!(std::fs::metadata(&path).unwrap().len() <= MIN_FILE_BYTES);
    assert!(index.evict().unwrap() > 0);
    index.ingest(&key, &[message(1000, &large)]).unwrap();
    assert!(index.status().unwrap().file_bytes <= MIN_FILE_BYTES);
    assert_eq!(index.status().unwrap().state, "partial");
}

#[test]
fn large_catalog_plans_bound_candidate_source_and_purge_lookups() {
    let mut index = index();
    let key = index.register(&target("one", false)).unwrap();
    // 30,000 indexed rows: the query plan must not depend on catalog size.
    fill(&mut index, &key, 1, 30_000, "common public message");
    // Source lookup must remain indexed with tens of thousands of sessions too.
    let tx = index.db.transaction().unwrap();
    for i in 0..30_000 {
        let target = target(&format!("catalog-{i}"), false);
        tx.execute(
            "INSERT INTO sources VALUES(?,?,1,1,0)",
            params![
                target.key().unwrap(),
                serde_json::to_string(&target).unwrap()
            ],
        )
        .unwrap();
    }
    tx.commit().unwrap();
    assert_eq!(index.status().unwrap().sources, 30_001);
    let plan: Vec<String> = index
        .db
        .prepare(&format!("EXPLAIN QUERY PLAN {CANDIDATE_SQL}"))
        .unwrap()
        .query_map(params!["CO", f64::MAX, i64::MAX, CANDIDATES + 1], |r| {
            r.get(3)
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    println!("candidate query plan: {plan:?}");
    assert!(plan
        .iter()
        .any(|line| line.contains("SEARCH grams USING PRIMARY KEY") && line.contains("gram=?")));
    assert!(plan
        .iter()
        .any(|line| line.contains("SEARCH m USING INTEGER PRIMARY KEY")));
    assert!(plan
        .iter()
        .any(|line| line.contains("SEARCH s USING INDEX")));
    assert!(!plan
        .iter()
        .any(|line| line == "SCAN grams" || line == "SCAN messages" || line == "SCAN sources"));
    let purge_plan: Vec<String> = index
        .db
        .prepare("EXPLAIN QUERY PLAN SELECT id FROM messages WHERE source=? ORDER BY id LIMIT 32")
        .unwrap()
        .query_map([key], |r| r.get(3))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    println!("purge query plan: {purge_plan:?}");
    assert!(purge_plan
        .iter()
        .any(|line| line.contains("COVERING INDEX messages_source (source=?)")));
    let page = index.query("common absent", 100, None).unwrap();
    assert_eq!(page.candidates, CANDIDATES);
    assert!(page.next_cursor.is_some());
}
