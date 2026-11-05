use super::super::retention as gc;
use super::*;

fn counts(f: &Fixture) -> (i64, i64, i64) {
    f.store
        .db
        .query_row(
            "SELECT edges,edge_bytes,(SELECT count(*) FROM desktop_attention_tombstones)
        FROM desktop_attention_totals WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap()
}
fn baseline(f: &mut Fixture, reply: &str) -> Value {
    let mut raw = row();
    raw["reply_id"] = json!(reply);
    f.save(&raw, true);
    raw
}
fn attempted(f: &mut Fixture, reply: &str, outcome: Option<ledger::State>) -> String {
    let mut raw = row();
    raw["reply_id"] = json!(reply);
    f.save(&raw, false);
    let intent = project(&raw).candidates[0].key.id.clone();
    let writer: String = f
        .store
        .db
        .query_row("SELECT epoch FROM desktop_writer WHERE id=1", [], |r| {
            r.get(0)
        })
        .unwrap();
    let request = ledger::Reserve {
        request_id: id(),
        intent_id: intent.clone(),
        revision: 1,
        epoch: writer,
        service_epoch: f.runtime.epoch.clone(),
        adapter: id(),
        lease: id(),
        retry_of: None,
    };
    assert!(ledger::reserve(&mut f.store.db, &request).unwrap().fresh);
    if let Some(outcome) = outcome {
        ledger::finish(&mut f.store.db, &request, outcome, &json!({})).unwrap();
    }
    f.store
        .db
        .execute(
            "UPDATE desktop_attention_edges SET live=0,blocked=1 WHERE intent=?1",
            [&intent],
        )
        .unwrap();
    f.store
        .db
        .execute(
            "UPDATE desktop_intents SET blocked=1 WHERE id=?1",
            [&intent],
        )
        .unwrap();
    intent
}

#[test]
fn compacted_baselines_do_not_reinflate_or_replay_after_resume_and_restart() {
    let mut f = Fixture::new();
    let old = baseline(&mut f, "old");
    let logical = project(&old).candidates[0].logical.clone();
    let (_, before, _) = counts(&f);
    let compacted = gc::compact(&mut f.store.db).unwrap();
    assert_eq!(compacted.removed, 1);
    assert_eq!(counts(&f), (0, 0, 1));
    assert!(before > 32);
    assert_eq!(f.intents(), 0);
    assert!(gc::suppressed(&f.store.db, &logical).unwrap());
    let temp = f._temp;
    drop((f.store, f.outbox, f.host, f.runtime));
    let mut f = Fixture::from_temp(temp);
    let mut resumed = old.clone();
    resumed["run_id"] = json!("resumed-run");
    f.save(&resumed, false);
    assert_eq!(
        counts(&f),
        (0, 0, 1),
        "suppression lookup must not rebuild quota-consuming payloads"
    );
    assert_eq!(f.intents(), 0);
    f.runtime.set_policy(enabled());
    f.snapshot(vec![resumed.clone()]);
    f.ticks(4);
    assert!(
        f.host.try_next().is_err(),
        "the compacted baseline cannot post after restart"
    );
    resumed["reply_id"] = json!("fresh-after-restart");
    f.delta(resumed);
    assert_single_reply(&mut f, "fresh-after-restart", 8);
    assert_eq!(counts(&f).0, 1);
}

#[test]
fn live_and_every_attempt_state_keep_exact_original_mappings() {
    let mut f = Fixture::new();
    let mut protected = Vec::new();
    for (reply, state) in [
        ("reserved", None),
        ("unknown", Some(ledger::State::Unknown)),
        ("delivered", Some(ledger::State::Delivered)),
        ("refused", Some(ledger::State::Refused)),
    ] {
        protected.push(attempted(&mut f, reply, state));
    }
    let live = baseline(&mut f, "live-blocked");
    let live_intent = project(&live).candidates[0].key.id.clone();
    f.store
        .db
        .execute(
            "UPDATE desktop_attention_edges SET live=1 WHERE intent=?1",
            [&live_intent],
        )
        .unwrap();
    protected.push(live_intent.clone());
    let before: Vec<(String, String, String)> = f
        .store
        .db
        .prepare("SELECT intent,identity,payload FROM desktop_attention_edges ORDER BY id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    baseline(&mut f, "safe");
    f.store
        .db
        .execute(
            "UPDATE desktop_attention_edges SET live=1 WHERE intent=?1",
            [&live_intent],
        )
        .unwrap();
    let removed = gc::compact(&mut f.store.db).unwrap();
    assert_eq!(removed.removed, 1);
    let after: Vec<(String, String, String)> = f
        .store
        .db
        .prepare("SELECT intent,identity,payload FROM desktop_attention_edges ORDER BY id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(before, after);
    for intent in protected {
        assert!(f
            .store
            .db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM desktop_intents WHERE id=?1)",
                [intent],
                |r| r.get::<_, bool>(0)
            )
            .unwrap());
    }
    assert_eq!(
        f.store
            .db
            .query_row("SELECT count(*) FROM desktop_attempts", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        4
    );
}

#[test]
fn interrupted_compaction_rolls_back_hash_payload_totals_and_cursor_together() {
    let mut f = Fixture::new();
    let raw = baseline(&mut f, "atomic");
    let original = counts(&f);
    f.store
        .db
        .execute_batch(
            "CREATE TRIGGER fixture_interrupt_compaction BEFORE DELETE ON desktop_attention_edges
        BEGIN SELECT RAISE(ABORT,'interrupted compaction'); END;",
        )
        .unwrap();
    assert!(gc::compact(&mut f.store.db).is_err());
    assert_eq!(counts(&f), original);
    assert_eq!(f.intents(), 1);
    assert_eq!(
        f.store
            .db
            .query_row("SELECT after_id FROM desktop_attention_gc", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    f.store
        .db
        .execute_batch("DROP TRIGGER fixture_interrupt_compaction;")
        .unwrap();
    assert_eq!(gc::compact(&mut f.store.db).unwrap().removed, 1);
    f.save(&raw, false);
    assert_eq!(f.intents(), 0);
}

#[test]
fn a_shared_baseline_intent_stays_until_its_last_mapping_compacts() {
    let mut f = Fixture::new();
    let raw = baseline(&mut f, "shared");
    let intent = project(&raw).candidates[0].key.id.clone();
    let alias = projection::digest(&"another retained mapping").unwrap();
    f.store.db.execute("INSERT INTO desktop_attention_edges(logical,session,intent,identity,payload,blocked,live,dirty,bytes)
        SELECT ?1,session,intent,identity,payload,1,1,0,bytes FROM desktop_attention_edges WHERE intent=?2",params![alias,intent]).unwrap();
    assert_eq!(gc::compact(&mut f.store.db).unwrap().removed, 1);
    assert_eq!(
        f.intents(),
        1,
        "a live mapping still retains the original baseline intent"
    );
    assert_eq!(counts(&f).0, 1);
    f.store
        .db
        .execute(
            "UPDATE desktop_attention_edges SET live=0 WHERE logical=?1",
            [alias],
        )
        .unwrap();
    assert_eq!(gc::compact(&mut f.store.db).unwrap().removed, 1);
    assert_eq!(f.intents(), 0);
    assert_eq!(counts(&f), (0, 0, 2));
}

#[test]
fn compaction_pages_and_full_payload_bytes_are_bounded() {
    let mut f = Fixture::new();
    for i in 0..gc::ROWS + 4 {
        baseline(&mut f, &format!("large-{i}"));
    }
    let payload = serde_json::to_string(&json!({"body":"x".repeat(60_000)})).unwrap();
    f.store.db.execute("UPDATE desktop_attention_edges SET payload=?1,bytes=length(CAST(identity AS BLOB))+length(CAST(?1 AS BLOB))+length(session)+length(logical)+length(intent)",[&payload]).unwrap();
    f.store
        .db
        .execute("UPDATE desktop_intents SET payload=?1", [&payload])
        .unwrap();
    f.store.db.execute("UPDATE desktop_attention_totals SET edge_bytes=(SELECT sum(bytes) FROM desktop_attention_edges)",[]).unwrap();
    let before = counts(&f);
    let result = gc::compact(&mut f.store.db).unwrap();
    assert!(result.scanned <= gc::ROWS);
    assert!(result.bytes <= gc::BYTES);
    assert!(result.removed > 0 && result.removed < gc::ROWS);
    let after = counts(&f);
    assert_eq!(before.0 - after.0, result.removed as i64);
    assert_eq!(after.2, result.removed as i64);
    let actual: i64 = f
        .store
        .db
        .query_row("SELECT sum(bytes) FROM desktop_attention_edges", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(after.1, actual);
    let temp = f._temp;
    drop((f.store, f.outbox, f.host, f.runtime));
    let mut reopened = Fixture::from_temp(temp);
    let next = gc::compact(&mut reopened.store.db).unwrap();
    assert!(next.removed > 0);
    assert!(next.bytes <= gc::BYTES);
    assert!(
        counts(&reopened).0 < after.0,
        "the durable GC cursor resumes after restart"
    );
}

#[test]
fn protected_prefix_is_indexed_and_quota_wait_resumes_without_losing_the_row() {
    let mut f = Fixture::new();
    let intent = attempted(&mut f, "protected", Some(ledger::State::Unknown));
    f.store.db.execute("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<4000)
        INSERT INTO desktop_attention_edges(logical,session,intent,identity,payload,blocked,live,dirty,bytes)
        SELECT printf('%064x',x),session,intent,identity,payload,1,0,0,bytes FROM n,desktop_attention_edges WHERE intent=?1 AND id=1",[&intent]).unwrap();
    // Put a safe row beyond the first protected page, but before the much
    // larger tail. GC must advance a bounded page rather than scan for it.
    let safe = baseline(&mut f, "safe-after-prefix");
    let safe_id: i64 = f
        .store
        .db
        .query_row(
            "SELECT id FROM desktop_attention_edges WHERE intent=?1",
            [project(&safe).candidates[0].key.id.clone()],
            |r| r.get(0),
        )
        .unwrap();
    f.runtime.set_policy(enabled());
    let mut current = row();
    current["reply_id"] = Value::Null;
    f.snapshot(vec![current.clone()]);
    f.ticks(2);
    current["reply_id"] = json!("fresh-under-quota");
    f.delta(current);
    f.store
        .db
        .execute(
            "UPDATE desktop_attention_totals SET edges=?1",
            [state::MAX_EDGES],
        )
        .unwrap();
    // Restart the cursor at a protected page so this turn cannot compact yet.
    f.store
        .db
        .execute(
            "UPDATE desktop_attention_gc SET after_id=?1,through_id=?2",
            params![safe_id - gc::ROWS as i64 - 1, safe_id],
        )
        .unwrap();
    let steps = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let measured = steps.clone();
    f.store.db.progress_handler(
        1,
        Some(move || measured.fetch_add(1, std::sync::atomic::Ordering::Relaxed) > 15_000),
    );
    f.tick();
    f.store.db.progress_handler(0, None::<fn() -> bool>);
    assert!(steps.load(std::sync::atomic::Ordering::Relaxed) < 15_000);
    assert!(f.runtime.capacity_waiting);
    assert!(!f.runtime.failed());
    assert!(!f.runtime.work_pending());
    assert!(f
        .runtime
        .pending
        .as_ref()
        .is_some_and(|p| !p.rows.is_empty()));
    f.tick();
    assert!(!f.runtime.capacity_waiting);
    assert_single_reply(&mut f, "fresh-under-quota", 8);
}

#[test]
fn count_and_byte_pressure_recover_on_restart_before_new_discovery() {
    for column in ["edges", "edge_bytes"] {
        let mut f = Fixture::new();
        let old = baseline(&mut f, &"h".repeat(900));
        let quota = if column == "edges" {
            state::MAX_EDGES
        } else {
            state::MAX_EDGE_BYTES
        };
        f.store
            .db
            .execute(
                &format!("UPDATE desktop_attention_totals SET {column}=?1"),
                [quota],
            )
            .unwrap();
        let temp = f._temp;
        drop((f.store, f.outbox, f.host, f.runtime));
        let mut f = Fixture::from_temp(temp);
        f.runtime.set_policy(enabled());
        let mut current = row();
        current["reply_id"] = Value::Null;
        f.snapshot(vec![current.clone()]);
        f.ticks(3);
        assert!(gc::suppressed(&f.store.db, &project(&old).candidates[0].logical).unwrap());
        assert!(!f.runtime.failed());
        current["reply_id"] = json!("new-after-pressure-restart");
        f.delta(current);
        assert_single_reply(&mut f, "new-after-pressure-restart", 8);
    }
}

#[test]
fn a_compacted_optional_question_stays_blocked_after_tracker_forgets_it() {
    let mut f = Fixture::new();
    let mut raw = row();
    raw["reply_id"] = Value::Null;
    raw["optional_questions"] = json!({"count":1,"ids":["old-question"]});
    f.save(&raw, true);
    assert_eq!(gc::compact(&mut f.store.db).unwrap().removed, 1);
    for i in 0..70 {
        raw["optional_questions"] = json!({"count":1,"ids":[format!("question-{i}")]});
        f.save(&raw, true);
    }
    raw["run_id"] = json!("resumed-run");
    raw["optional_questions"] = json!({"count":1,"ids":["old-question"]});
    let previous = state::previous(&f.store.db, &state::session("", &raw).unwrap()).unwrap();
    let projection = projection::project(
        "",
        "fixture",
        &raw,
        previous,
        Policy {
            enabled: true,
            optional_questions: true,
        },
        false,
        false,
    )
    .unwrap();
    let candidate = projection.candidates.iter().find(|c| !c.consumed).unwrap();
    let intent = candidate.key.id.clone();
    state::save(
        &mut f.store.db,
        "",
        "key",
        state::Position {
            scope: "scope",
            row_hash: "hash",
            policy_revision: 0,
        },
        projection,
        false,
    )
    .unwrap();
    assert!(!f
        .store
        .db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM desktop_intents WHERE id=?1)",
            [&intent],
            |r| r.get::<_, bool>(0)
        )
        .unwrap());
    assert!(f
        .outbox
        .offer_intent(&mut f.store, &intent, 1, Instant::now())
        .is_err());
    assert!(f.host.try_next().is_err());
}

#[test]
fn schema_twenty_migration_preserves_prior_effects_and_rolls_back_partial_ddl() {
    let mut f = Fixture::new();
    let intent = attempted(&mut f, "original-unknown", Some(ledger::State::Unknown));
    let original: (String, String) = f
        .store
        .db
        .query_row(
            "SELECT identity,payload FROM desktop_intents WHERE id=?1",
            [&intent],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    let location = Location {
        root: std::path::Path::new(f.store.db.path().unwrap())
            .parent()
            .unwrap()
            .to_owned(),
        configuration: "a".repeat(64),
    };
    super::super::remove_retention_fixture(&f.store.db);
    f.store
        .db
        .execute_batch("PRAGMA user_version=19; CREATE TABLE desktop_attention_gc(fixture TEXT);")
        .unwrap();
    assert!(Store::open(&location, &id()).is_err());
    assert_eq!(
        f.store
            .db
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        19
    );
    assert!(!f
        .store
        .db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='desktop_attention_tombstones')",
            [],
            |r| r.get::<_, bool>(0)
        )
        .unwrap());
    f.store
        .db
        .execute_batch("DROP TABLE desktop_attention_gc")
        .unwrap();
    drop(f.store);
    let reopened = Store::open(&location, &id()).unwrap();
    assert_eq!(
        reopened
            .db
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        crate::desktop::store::SCHEMA
    );
    assert_eq!(
        reopened
            .db
            .query_row(
                "SELECT identity,payload FROM desktop_intents WHERE id=?1",
                [&intent],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            )
            .unwrap(),
        original
    );
    assert_eq!(
        reopened
            .db
            .query_row(
                "SELECT state FROM desktop_attempts WHERE intent_id=?1",
                [&intent],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "unknown"
    );
}
