use super::*;

fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf, Store) {
    let temp = tempfile::tempdir().unwrap();
    let state = temp.path().join("state");
    fs::create_dir_all(state.join("archives")).unwrap();
    let conversation = uuid::Uuid::new_v4().to_string();
    let transcript = temp.path().join(format!("{conversation}.jsonl"));
    let rows = (1..=6)
        .map(|i| {
            json!({"type":"event_msg","timestamp":i,
        "payload":{"type":"agent_message","id":format!("m{i}"),"message":format!("needle {i}")}})
            .to_string()
                + "\n"
        })
        .collect::<String>();
    fs::write(&transcript, rows).unwrap();
    let archive = uuid::Uuid::new_v4().to_string();
    let path = state.join("archives").join(format!("{archive}.json"));
    fs::write(
        &path,
        json!({"version":1,"name":"codex/example/archive","agent":"codex",
        "run_id":"fixture-run","conversation_id":conversation,"archive_id":archive,
        "archived_at":42,"transcript":transcript,"cwd":"/work/example"})
        .to_string(),
    )
    .unwrap();
    let mut store = Store::open(&state.join("content-search-v1")).unwrap();
    store
        .discover(
            &state,
            &DiscoveryBudget::interactive(),
            &AtomicBool::new(false),
        )
        .unwrap();
    store
        .consume(&state, &budget(), &AtomicBool::new(false))
        .unwrap();
    (temp, state, path, transcript, store)
}
fn budget() -> Budget {
    Budget {
        files: 128,
        records: 512,
        bytes: 8 << 20,
        deadline: Instant::now() + Duration::from_secs(1),
    }
}
fn page(store: &mut Store, state: &Path, cursor: Option<&str>, budget: &Budget) -> Value {
    store
        .publish(state, "needle", 2, cursor, budget, &AtomicBool::new(false))
        .unwrap()
}

#[test]
fn archive_pages_keep_exact_navigation_and_cursor_snapshot_without_fallback() {
    let (_temp, state, _path, _transcript, mut store) = fixture();
    let first = page(&mut store, &state, None, &budget());
    assert_eq!(first["abi"], QUERY_ABI);
    assert_eq!(first["index_coverage"]["state"], "ready");
    assert!(first["fallback"].is_null());
    assert_eq!(first["results"][0]["message_id"], "m6");
    assert_eq!(first["results"][0]["session"]["state"], "archived");
    assert_eq!(
        first["results"][0]["session"]["archive_id"],
        first["results"][0]["archive_id"]
    );
    assert_eq!(first["proof"]["files"], 2);
    let second = page(&mut store, &state, first["next_cursor"].as_str(), &budget());
    assert_eq!(second["results"][0]["message_id"], "m4");
    let third = page(
        &mut store,
        &state,
        second["next_cursor"].as_str(),
        &budget(),
    );
    assert_eq!(third["results"][0]["message_id"], "m2");
    assert!(third["next_cursor"].is_null());
}

#[test]
fn an_unverified_hit_is_never_consumed_when_the_proof_budget_runs_out() {
    let (_temp, state, _path, _transcript, mut store) = fixture();
    let mut tiny = budget();
    tiny.files = 1;
    let first = page(&mut store, &state, None, &tiny);
    assert!(first["results"].as_array().unwrap().is_empty());
    assert_eq!(first["proof"]["files"], 0);
    assert!(first["next_cursor"].is_string());
    let resumed = page(&mut store, &state, first["next_cursor"].as_str(), &budget());
    assert_eq!(resumed["results"][0]["message_id"], "m6");
}

#[test]
fn a_forgotten_archive_is_rejected_before_publication_without_discovery_or_sweep() {
    let (_temp, state, path, _transcript, mut store) = fixture();
    fs::remove_file(path).unwrap();
    let reply = page(&mut store, &state, None, &budget());
    assert!(reply["results"].as_array().unwrap().is_empty());
    assert_eq!(reply["unavailable_sessions"], 1);
    assert_ne!(reply["index_coverage"]["state"], "ready");
    assert!(store
        .index
        .query("needle", 100, None)
        .unwrap()
        .hits
        .is_empty());
}

#[test]
fn changed_transcript_identity_cannot_publish_old_copied_text() {
    let (_temp, state, _path, transcript, mut store) = fixture();
    fs::remove_file(&transcript).unwrap();
    fs::write(&transcript, "replacement").unwrap();
    let reply = page(&mut store, &state, None, &budget());
    assert!(reply["results"].as_array().unwrap().is_empty());
    assert_eq!(reply["unavailable_sessions"], 1);
}

#[test]
fn cancelled_publication_retains_a_resumable_position_and_reads_no_source_files() {
    let (_temp, state, _path, _transcript, mut store) = fixture();
    let reply = store
        .publish(&state, "needle", 2, None, &budget(), &AtomicBool::new(true))
        .unwrap();
    assert!(reply["results"].as_array().unwrap().is_empty());
    assert_eq!(reply["proof"]["files"], 0);
    let next = page(&mut store, &state, reply["next_cursor"].as_str(), &budget());
    assert_eq!(next["results"][0]["message_id"], "m6");
    assert!(store
        .publish(
            &state,
            "other",
            2,
            reply["next_cursor"].as_str(),
            &budget(),
            &AtomicBool::new(false)
        )
        .is_err());
}

#[test]
fn the_output_budget_preserves_the_first_unreturned_hit() {
    let (_temp, state, _path, _transcript, mut store) = fixture();
    let key: String = store
        .index
        .connection()
        .query_row("SELECT id FROM sources LIMIT 1", [], |r| r.get(0))
        .unwrap();
    for i in 0..80 {
        store
            .index
            .ingest(
                &key,
                &[Message {
                    id: format!("budget-{i}"),
                    at: 10.0 + i as f64,
                    role: Role::Assistant,
                    kind: "AgentMessage".into(),
                    source: "codex_transcript".into(),
                    seq: 0,
                    text: format!("needle {i} {}", "x".repeat(40_000)),
                }],
            )
            .unwrap();
    }
    let mut cursor = None;
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..10 {
        let reply = store
            .publish(
                &state,
                "needle",
                100,
                cursor.as_deref(),
                &budget(),
                &AtomicBool::new(false),
            )
            .unwrap();
        let hits = reply["results"].as_array().unwrap();
        assert!(serde_json::to_vec(hits).unwrap().len() <= INDEX_REPLY_BYTES);
        for hit in hits {
            assert!(seen.insert(hit["message_id"].as_str().unwrap().to_owned()));
        }
        cursor = reply["next_cursor"].as_str().map(str::to_owned);
        if cursor.is_none() {
            break;
        }
    }
    assert!(cursor.is_none());
    assert_eq!(seen.len(), 86);
}
