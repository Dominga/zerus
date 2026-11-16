use super::*;

fn seeded(count: u128) -> (tempfile::TempDir, Migration) {
    let temp = tempfile::tempdir().unwrap();
    let migration = Migration::new(temp.path()).unwrap();
    let transaction = migration.catalog.db.unchecked_transaction().unwrap();
    for n in 1..=count {
        migration.catalog.db.execute(
            "INSERT INTO receipt_rows(source,request,name,run,conversation,agent,first_message,at,files,binding,stamp,change)
             VALUES('terminal',?1,'codex/fixture/count','run','conversation','',0,?2,0,'fixture','fixture',?2)",
            params![uuid::Uuid::from_u128(n).to_string(), n as i64],
        ).unwrap();
    }
    transaction.commit().unwrap();
    (temp, migration)
}

fn finishing(migration: &mut Migration, count: u128) {
    let directory = migration.catalog.state.join(Source::Terminal.directory());
    fs::create_dir_all(&directory).unwrap();
    migration.next_source = migration.sources.len();
    migration.walk = Some(Walk {
        source: Source::Terminal,
        entries: None,
        started: stamp(&directory).unwrap(),
        covered: false,
        after: Some(uuid::Uuid::from_u128(count).to_string()),
        indexed: count as i64,
        count_at_start: state(migration).0,
        schema: migration
            .catalog
            .db
            .query_row("PRAGMA schema_version", [], |row| row.get(0))
            .unwrap(),
        witness_error: false,
        at: now(),
        walk_errors: false,
        attempts: 0,
        catchup: None,
        reparse: true,
        epoch: migration
            .catalog
            .db
            .query_row(
                "SELECT value FROM receipt_meta WHERE key='epoch'",
                [],
                |row| row.get(0),
            )
            .unwrap(),
    });
}

#[test]
fn a_finishing_step_does_constant_sql_work_at_twelve_thousand_rows() {
    let (_temp, mut migration) = seeded(12_000);
    finishing(&mut migration, 12_000);
    let operations = Arc::new(AtomicUsize::new(0));
    let observed = operations.clone();
    migration.catalog.db.progress_handler(
        1,
        Some(move || {
            observed.fetch_add(1, Ordering::Relaxed);
            false
        }),
    );
    assert!(migration.step().unwrap());
    migration
        .catalog
        .db
        .progress_handler(0, None::<fn() -> bool>);
    let work = operations.load(Ordering::Relaxed);
    eprintln!("FINAL_COUNT rows=12000 sqlite_vm_operations={work}");
    assert!(
        work < 1_000,
        "source finalization exceeded constant SQL work: {work}"
    );
    let state: (i64, bool) = migration
        .catalog
        .db
        .query_row(
            "SELECT indexed,complete FROM receipt_sources WHERE source='terminal'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(state, (12_000, true));
}

fn original(catalog: &Catalog, n: u128) -> PathBuf {
    let directory = catalog.state.join(Source::Terminal.directory());
    fs::create_dir_all(&directory).unwrap();
    let id = uuid::Uuid::from_u128(n).to_string();
    let path = directory.join(format!("{id}.json"));
    let value = json!({"request_id":id,"name":"codex/fixture/count","run_id":"run",
        "conversation_id":"conversation","digest":"fixture","status":"submitted","submitted_at":n as f64});
    fs::write(&path, value.to_string()).unwrap();
    catalog.remember(Source::Terminal, &path, &value).unwrap();
    path
}

fn state(migration: &Migration) -> (i64, bool) {
    migration
        .catalog
        .db
        .query_row(
            "SELECT indexed,complete FROM receipt_sources WHERE source='terminal'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap()
}

/// Step the started walk to its end. Its catch-up listing may take more
/// than one budgeted step on a loaded machine.
fn finish_walk(migration: &mut Migration) {
    for _ in 0..1_000 {
        if migration.step().unwrap() {
            return;
        }
    }
    panic!("the walk did not finish");
}

fn finish(state: &Path) -> Migration {
    let mut migration = Migration::new(state).unwrap();
    while !migration.step().unwrap() {}
    migration
}

#[test]
fn a_backdated_arrival_preserves_the_live_count_until_a_stable_restart() {
    let (temp, mut migration) = seeded(0);
    let kept = original(&migration.catalog, 1);
    original(&migration.catalog, 2);
    let bytes = fs::read(&kept).unwrap();
    finishing(&mut migration, 2);
    // Its request sorts behind the already consumed cursor. The live count
    // stays 3; the walk's catch-up listing then verifies all three originals
    // against their rows and seals the exact count (zerus-kj3mb).
    original(&migration.catalog, 0);
    finish_walk(&mut migration);
    assert_eq!(state(&migration), (3, true));
    assert!(migration.catalog.coverage_verified());
    let restarted = finish(temp.path());
    assert_eq!(state(&restarted), (3, true));
    assert!(restarted.catalog.coverage_verified());
    assert_eq!(fs::read(&kept).unwrap(), bytes);
}

#[test]
fn a_changed_rowset_cannot_publish_the_old_partial_count() {
    let (temp, mut migration) = seeded(0);
    let kept = original(&migration.catalog, 1);
    original(&migration.catalog, 2);
    let bytes = fs::read(&kept).unwrap();
    finishing(&mut migration, 2);
    migration
        .catalog
        .db
        .execute(
            "DELETE FROM receipt_rows WHERE source='terminal' AND request=?",
            [uuid::Uuid::from_u128(1).to_string()],
        )
        .unwrap();
    // The old partial count (2) is never published over the live count (1):
    // the catch-up listing re-reads the original whose row was lost and
    // seals only the re-verified set.
    finish_walk(&mut migration);
    assert_eq!(state(&migration), (2, true));
    assert!(migration.catalog.coverage_verified());
    let restarted = finish(temp.path());
    assert_eq!(state(&restarted), (2, true));
    assert!(restarted.catalog.coverage_verified());
    assert_eq!(fs::read(&kept).unwrap(), bytes);
}

#[test]
fn delayed_sql_cannot_turn_finalization_into_a_source_scan() {
    let (_temp, mut migration) = seeded(12_000);
    finishing(&mut migration, 12_000);
    let callbacks = Arc::new(AtomicUsize::new(0));
    let observed = callbacks.clone();
    migration.catalog.db.progress_handler(
        1_000,
        Some(move || {
            observed.fetch_add(1, Ordering::Relaxed);
            std::thread::sleep(Duration::from_millis(1));
            false
        }),
    );
    let started = Instant::now();
    assert!(migration.step().unwrap());
    let elapsed = started.elapsed();
    migration
        .catalog
        .db
        .progress_handler(0, None::<fn() -> bool>);
    let callbacks = callbacks.load(Ordering::Relaxed);
    eprintln!(
        "FINAL_COUNT_DELAY rows=12000 callbacks={callbacks} elapsed_us={} slice_us={}",
        elapsed.as_micros(),
        SLICE.as_micros()
    );
    assert_eq!(
        callbacks, 0,
        "finishing rescanned the source outside the slice"
    );
    assert_eq!(state(&migration), (12_000, true));
}

#[test]
fn discovery_repair_can_rotate_the_epoch_before_the_count_starts() {
    let (temp, migration) = seeded(0);
    let kept = original(&migration.catalog, 1);
    original(&migration.catalog, 2);
    let bytes = fs::read(&kept).unwrap();
    finish(temp.path());
    // Legacy row loss did not invalidate coverage or decrement its witness.
    let guard: String = migration
        .catalog
        .db
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE type='trigger' AND name='receipt_row_removed'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    migration
        .catalog
        .db
        .execute_batch("DROP TRIGGER receipt_row_removed")
        .unwrap();
    migration
        .catalog
        .db
        .execute(
            "DELETE FROM receipt_rows WHERE source='terminal' AND request=?",
            [uuid::Uuid::from_u128(1).to_string()],
        )
        .unwrap();
    migration.catalog.db.execute_batch(&guard).unwrap();
    let repaired = finish(temp.path());
    assert_eq!(state(&repaired), (2, true));
    assert!(repaired.catalog.coverage_verified());
    assert_eq!(fs::read(&kept).unwrap(), bytes);
}

#[path = "race_tests.rs"]
mod insert_race;

#[path = "catchup_tests.rs"]
mod catchup;

#[test]
fn an_explicit_question_step_waits_before_reading_while_another_writer_is_active() {
    let temp = tempfile::tempdir().unwrap();
    let _wait = admission::Wait::new();
    let mut migration = Migration::questions(temp.path()).unwrap();
    let writer = Connection::open(temp.path().join(DATABASE)).unwrap();
    writer
        .execute_batch("BEGIN IMMEDIATE; UPDATE receipt_meta SET value=value WHERE key='epoch'")
        .unwrap();
    let release = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        writer.execute_batch("COMMIT").unwrap();
    });
    let started = Instant::now();
    let result = migration.step();
    release.join().unwrap();
    assert!(result.is_ok(), "transient writer contention must wait before the snapshot, not fail immediately: {result:?}");
    eprintln!(
        "QUESTION_HANDOFF elapsed_ms={}",
        started.elapsed().as_millis()
    );
    while !migration.step().unwrap() {}
    let complete: bool = migration
        .catalog
        .db
        .query_row(
            "SELECT complete FROM receipt_sources WHERE source='question'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(complete);
}

#[test]
fn bounded_advances_resume_discovery_instead_of_restarting_the_same_prefix() {
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join("input_receipts");
    fs::create_dir(&directory).unwrap();
    for n in 1..=384_u128 {
        let id = uuid::Uuid::from_u128(n).to_string();
        fs::write(
            directory.join(format!("{id}.json")),
            json!({"request_id":id,
            "name":"codex/fixture/progress","run_id":"run","conversation_id":"conversation",
            "digest":"fixture","status":"submitted","submitted_at":n as f64})
            .to_string(),
        )
        .unwrap();
        fs::File::options()
            .write(true)
            .open(directory.join(format!("{id}.json")))
            .unwrap()
            .set_times(
                fs::FileTimes::new()
                    .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(n as u64)),
            )
            .unwrap();
    }
    // Historical receipts are outside the native change-clock ambiguity
    // window; the assertion still bounds every individual advance.
    std::thread::sleep(Duration::from_millis(2100));
    let mut complete = false;
    for _ in 0..40 {
        if advance(temp.path(), Duration::ZERO).unwrap() {
            complete = true;
            break;
        }
    }
    assert!(
        complete,
        "bounded invocations must retain committed discovery and row-count progress"
    );
    let catalog = Catalog::open(temp.path(), false).unwrap();
    let state: (i64, bool) = catalog
        .db
        .query_row(
            "SELECT indexed,complete FROM receipt_sources WHERE source='terminal'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(state, (384, true));
}

#[test]
fn foreground_question_reconciliation_hands_off_the_background_writer() {
    let temp = tempfile::tempdir().unwrap();
    let foreground = Migration::questions(temp.path()).unwrap();
    assert!(priority(temp.path(), true).unwrap().is_none());
    drop(foreground);
    assert!(priority(temp.path(), true).unwrap().is_some());
}

#[test]
fn a_busy_background_step_keeps_its_unadvanced_walk() {
    let temp = tempfile::tempdir().unwrap();
    let mut background = Migration::background(temp.path()).unwrap();
    let writer = Connection::open(temp.path().join(DATABASE)).unwrap();
    writer
        .execute_batch("BEGIN IMMEDIATE; UPDATE receipt_meta SET value=value WHERE key='epoch'")
        .unwrap();
    assert!(!background.step().unwrap());
    assert_eq!(background.next_source, 0);
    assert!(background.walk.is_none());
    writer.execute_batch("COMMIT").unwrap();
    while !background.step().unwrap() {}
    assert!(background.catalog.coverage_verified());
}
