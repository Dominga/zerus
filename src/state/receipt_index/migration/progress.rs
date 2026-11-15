//! Bounded, disposable continuation for the single receipt worker. A checkpoint
//! resumes work, never establishes delivery or absence. Canonical question
//! reconciliation has its own connection and never consumes this cursor.
use super::*;

const KEY: &str = "receipt_worker_walk_v1";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Checkpoint {
    source: Source,
    started: String,
    covered: bool,
    after: Option<String>,
    indexed: i64,
    epoch: String,
    count_at_start: i64,
    schema: i64,
    witness_error: bool,
    at: f64,
    walk_errors: bool,
    reparse: bool,
    rows: bool,
    guard_count: i64,
}

pub(super) fn restore(migration: &mut Migration) -> Result<()> {
    let value: Option<String> = sql(migration
        .catalog
        .db
        .query_row(
            "SELECT substr(value,1,8193) FROM receipt_meta WHERE key=?",
            [KEY],
            |row| row.get(0),
        )
        .optional())?;
    let Some(saved) = value
        .filter(|v| v.len() <= 8192)
        .and_then(|v| serde_json::from_str::<Checkpoint>(&v).ok())
    else {
        return Ok(());
    };
    let Some(position) = migration.sources.iter().position(|s| *s == saved.source) else {
        return Ok(());
    };
    if saved.indexed < 0
        || saved.indexed > saved.guard_count
        || saved.count_at_start < 0
        || saved.guard_count < 0
        || !saved.at.is_finite()
        || saved.at > now()
        || saved.started.len() > 512
        || saved.epoch.len() > 128
        || saved
            .after
            .as_ref()
            .is_some_and(|id| !bounded_identity(id, 128) || id.contains(['/', '\\']))
    {
        return Ok(());
    }
    // Full invalidation repairs must re-read originals if discovery was interrupted.
    if saved.reparse && !saved.rows {
        return Ok(());
    }
    let directory = migration.catalog.state.join(saved.source.directory());
    let (epoch, count, schema) = source_witness(&migration.catalog, saved.source)?;
    if epoch != saved.epoch
        || count != saved.guard_count
        || schema != saved.schema
        || stamp(&directory).ok().as_ref() != Some(&saved.started)
    {
        return Ok(());
    }
    let entries = if saved.rows || saved.started == "absent" {
        None
    } else {
        Some(fs::read_dir(directory).map_err(|_| "Receipt source is unavailable")?)
    };
    migration.next_source = position + 1;
    migration.walk = Some(Walk {
        source: saved.source,
        entries,
        started: saved.started,
        covered: saved.covered,
        after: saved.after,
        indexed: saved.indexed,
        epoch: saved.epoch,
        count_at_start: saved.count_at_start,
        schema: saved.schema,
        witness_error: saved.witness_error,
        at: saved.at,
        walk_errors: saved.walk_errors,
        attempts: 0,
        catchup: None,
        reparse: saved.reparse,
    });
    Ok(())
}

pub(super) fn save(migration: &Migration) -> Result<()> {
    let Some(walk) = migration.walk.as_ref().filter(|w| w.catchup.is_none()) else {
        sql(migration
            .catalog
            .db
            .execute("DELETE FROM receipt_meta WHERE key=?", [KEY]))?;
        return Ok(());
    };
    let (epoch, count, schema) = source_witness(&migration.catalog, walk.source)?;
    // Never resume across an invalidated epoch/schema. The ordinary next walk
    // repairs it instead. Row counting retains its original, separate fence.
    if epoch != walk.epoch || schema != walk.schema {
        return Ok(());
    }
    let saved = Checkpoint {
        source: walk.source,
        started: walk.started.clone(),
        covered: walk.covered,
        after: walk.after.clone(),
        indexed: walk.indexed,
        epoch,
        count_at_start: walk.count_at_start,
        schema,
        witness_error: walk.witness_error,
        at: walk.at,
        walk_errors: walk.walk_errors,
        reparse: walk.reparse,
        rows: walk.entries.is_none(),
        guard_count: count,
    };
    let value =
        serde_json::to_string(&saved).map_err(|_| "Receipt worker progress is unavailable")?;
    sql(migration.catalog.db.execute(
        "INSERT INTO receipt_meta VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![KEY,value],
    ))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn partial() -> (tempfile::TempDir, Migration) {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("input_receipts");
        fs::create_dir(&directory).unwrap();
        for n in 1..=160_u128 {
            let id = uuid::Uuid::from_u128(n).to_string();
            let path = directory.join(format!("{id}.json"));
            fs::write(&path,json!({"request_id":id,"name":"codex/fixture/cursor","run_id":"run",
                "conversation_id":"conversation","digest":"fixture","status":"submitted","submitted_at":n as f64}).to_string()).unwrap();
            fs::File::options()
                .write(true)
                .open(path)
                .unwrap()
                .set_times(
                    fs::FileTimes::new()
                        .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(n as u64)),
                )
                .unwrap();
        }
        let mut migration = Migration::background(temp.path()).unwrap();
        migration.light = 1;
        loop {
            assert!(!migration.step().unwrap());
            if migration.walk.as_ref().is_some_and(|walk| {
                walk.source == Source::Terminal && walk.entries.is_none() && walk.after.is_some()
            }) {
                break;
            }
        }
        (temp, migration)
    }
    fn reopened(state: &Path) -> Migration {
        let mut migration = Migration::with_sources(state, Source::ALL.to_vec(), false).unwrap();
        migration.background = true;
        restore(&mut migration).unwrap();
        migration
    }
    #[test]
    fn row_count_continues_from_the_committed_cursor_after_reopen() {
        let (temp, migration) = partial();
        let after = migration.walk.as_ref().unwrap().after.clone();
        let counted = migration.walk.as_ref().unwrap().indexed;
        drop(migration);
        let mut next = reopened(temp.path());
        assert_eq!(next.walk.as_ref().unwrap().after, after);
        assert_eq!(next.walk.as_ref().unwrap().indexed, counted);
        while !next.step().unwrap() {}
        let result: (i64, bool) = next
            .catalog
            .db
            .query_row(
                "SELECT indexed,complete FROM receipt_sources WHERE source='terminal'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(result, (160, true));
    }
    #[test]
    fn epoch_schema_counter_and_directory_changes_each_discard_the_cursor() {
        for changed in ["epoch", "schema", "counter", "directory"] {
            let (temp, migration) = partial();
            match changed {
                "epoch" => {
                    migration
                        .catalog
                        .db
                        .execute(
                            "UPDATE receipt_meta SET value=? WHERE key='epoch'",
                            [uuid::Uuid::new_v4().to_string()],
                        )
                        .unwrap();
                }
                "schema" => {
                    migration
                        .catalog
                        .db
                        .execute_batch("CREATE TABLE private_schema_change(value TEXT)")
                        .unwrap();
                }
                "counter" => {
                    migration
                        .catalog
                        .db
                        .execute(
                            "UPDATE receipt_sources SET indexed=indexed+1 WHERE source='terminal'",
                            [],
                        )
                        .unwrap();
                }
                "directory" => {
                    fs::write(
                        temp.path().join("input_receipts/new-entry"),
                        b"private fixture",
                    )
                    .unwrap();
                }
                _ => unreachable!(),
            }
            drop(migration);
            assert!(
                reopened(temp.path()).walk.is_none(),
                "{changed} must discard the checkpoint"
            );
        }
    }
    #[test]
    fn malformed_oversized_and_foreign_progress_remain_disposable() {
        for value in ["not json".to_owned(), "x".repeat(8193)] {
            let (temp, migration) = partial();
            migration
                .catalog
                .db
                .execute(
                    "UPDATE receipt_meta SET value=? WHERE key=?",
                    params![value, KEY],
                )
                .unwrap();
            drop(migration);
            assert!(reopened(temp.path()).walk.is_none());
        }
        let (temp, migration) = partial();
        let bytes: String = migration
            .catalog
            .db
            .query_row("SELECT value FROM receipt_meta WHERE key=?", [KEY], |row| {
                row.get(0)
            })
            .unwrap();
        let mut value: Value = serde_json::from_str(&bytes).unwrap();
        value["unreviewed_extension"] = json!(true);
        migration
            .catalog
            .db
            .execute(
                "UPDATE receipt_meta SET value=? WHERE key=?",
                params![value.to_string(), KEY],
            )
            .unwrap();
        drop(migration);
        assert!(reopened(temp.path()).walk.is_none());
    }
}
