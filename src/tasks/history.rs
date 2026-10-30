//! Optional bounded reads beside the unchanged legacy `task get`. The index
//! is disposable, local and limited to one task per project. Only a changed
//! source is decoded in full; page reads use SQLite's chronological index.
use super::*;
use crate::platform;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

const MAX_PAGE: usize = 80;
const CACHE: &str = "task-history-v1.sqlite3";

pub(super) fn limit(args: &[String]) -> Result<Option<usize>> {
    let limit = option(args, "--history-limit")?
        .map(|s| {
            s.parse::<usize>()
                .ok()
                .filter(|n| (1..=MAX_PAGE).contains(n))
                .ok_or_else(|| fail("history limit must be between 1 and 80"))
        })
        .transpose()?;
    if limit.is_none() && option(args, "--history-before")?.is_some() {
        return Err(fail("--history-before requires --history-limit"));
    }
    Ok(limit)
}

fn open(dir: &Path) -> Result<Connection> {
    // The project store lock is held throughout. Never follow a replaced cache
    // or a SQLite sidecar; all contain private task text.
    for suffix in ["", "-journal", "-wal", "-shm"] {
        match platform::verify_private(
            &dir.join(format!("{CACHE}{suffix}")),
            platform::Private::File,
        ) {
            Ok(true) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            _ => {
                return Err(fail(
                    "task history cache must contain private regular files",
                ))
            }
        }
    }
    let path = dir.join(CACHE);
    if !path.exists() {
        platform::private_open(fs::OpenOptions::new().write(true).create_new(true))
            .open(&path)
            .map_err(fail)?;
    }
    let db = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .map_err(fail)?;
    db.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA max_page_count=16384;
        CREATE TABLE IF NOT EXISTS current(uuid TEXT NOT NULL, fingerprint TEXT NOT NULL, task TEXT NOT NULL, total INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS entries(id TEXT PRIMARY KEY, at INTEGER NOT NULL, ordinal INTEGER NOT NULL, payload TEXT NOT NULL);
        CREATE UNIQUE INDEX IF NOT EXISTS entries_chronological ON entries(at,ordinal);")
        .map_err(fail)?;
    Ok(db)
}

fn fingerprint(dir: &Path, uuid: &str) -> Result<String> {
    // Hash bytes under the project lock, rather than trusting mtime/size: an
    // append, atomic compaction or same-tick replacement invalidates the index.
    let mut hash = Sha256::new();
    for ext in ["loro", "updates"] {
        let path = dir.join("tasks").join(format!("{uuid}.{ext}"));
        match fs::File::open(path) {
            Ok(mut file) => {
                hash.update([1]);
                hash.update(file.metadata().map_err(fail)?.len().to_le_bytes());
                let mut buf = [0; 65536];
                loop {
                    let n = file.read(&mut buf).map_err(fail)?;
                    if n == 0 {
                        break;
                    }
                    hash.update(&buf[..n]);
                }
            }
            Err(e) if ext == "updates" && e.kind() == std::io::ErrorKind::NotFound => {
                hash.update([0])
            }
            Err(e) => return Err(fail(e)),
        }
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn scope(project: &str, uuid: &str) -> String {
    format!("{:x}:", Sha256::digest(format!("{project}\n{uuid}")))
}

pub(super) fn page(
    store: &ProjectStore,
    dir: &Path,
    uuid: &str,
    limit: usize,
    before: Option<&str>,
) -> Result<Value> {
    if !zerus_replica::ids::is_uuid(uuid) || !(1..=MAX_PAGE).contains(&limit) {
        return Err(fail("invalid task history request"));
    }
    let prefix = scope(store.project_id(), uuid);
    let before = before
        .map(|cursor| {
            cursor
                .strip_prefix(&prefix)
                .filter(|id| !id.is_empty() && id.len() <= 256)
                .ok_or_else(|| fail("task history cursor does not match this task"))
        })
        .transpose()?;
    let fingerprint = fingerprint(dir, uuid)?;
    let mut db = open(dir)?;
    let cached = db
        .query_row("SELECT uuid,fingerprint FROM current", [], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .optional()
        .map_err(fail)?;
    if cached.as_ref() != Some(&(uuid.to_owned(), fingerprint.clone())) {
        let (mut task, ids) = store
            .load(uuid)
            .map_err(fail)?
            .view_with_history_ids()
            .map_err(fail)?;
        let dispatch = task.execution_id();
        let mut started = false;
        for entry in &task.log {
            if entry.kind == "sent" {
                started = false;
            } else if entry.kind == "started" {
                started = true;
            }
        }
        let log = std::mem::take(&mut task.log);
        let mut value = serde_json::to_value(task).map_err(fail)?;
        value["dispatch_id"] = json!(dispatch);
        value["start_confirmed"] = json!(started);
        value.as_object_mut().expect("task").remove("log");
        let tx = db.transaction().map_err(fail)?;
        tx.execute_batch("DELETE FROM current; DELETE FROM entries;")
            .map_err(fail)?;
        {
            let mut insert = tx
                .prepare("INSERT INTO entries(id,at,ordinal,payload) VALUES(?1,?2,?3,?4)")
                .map_err(fail)?;
            for (ordinal, (id, entry)) in ids.into_iter().zip(&log).enumerate() {
                insert
                    .execute(params![
                        id,
                        entry.at,
                        ordinal as i64,
                        serde_json::to_string(entry).map_err(fail)?
                    ])
                    .map_err(fail)?;
            }
        }
        tx.execute(
            "INSERT INTO current VALUES(?1,?2,?3,?4)",
            params![uuid, fingerprint, value.to_string(), log.len() as i64],
        )
        .map_err(fail)?;
        tx.commit().map_err(fail)?;
    }
    let (raw, total): (String, i64) = db
        .query_row("SELECT task,total FROM current", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .map_err(fail)?;
    let boundary = match before {
        Some(id) => db
            .query_row("SELECT at,ordinal FROM entries WHERE id=?1", [id], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            })
            .optional()
            .map_err(fail)?
            .ok_or_else(|| fail("History changed; return to Latest to refresh."))?,
        None => (i64::MAX, i64::MAX),
    };
    let mut query = db.prepare("SELECT id,payload FROM entries WHERE (at,ordinal)<(?1,?2) ORDER BY at DESC,ordinal DESC LIMIT ?3").map_err(fail)?;
    let mut rows = query
        .query_map(params![boundary.0, boundary.1, (limit + 1) as i64], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(fail)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(fail)?;
    let more = rows.len() > limit;
    rows.truncate(limit);
    rows.reverse();
    let next = if more {
        rows.first().map(|(id, _)| format!("{prefix}{id}"))
    } else {
        None
    };
    let entries = rows
        .into_iter()
        .map(|(_, raw)| serde_json::from_str::<Value>(&raw).map_err(fail))
        .collect::<Result<Vec<_>>>()?;
    let mut task: Value = serde_json::from_str(&raw).map_err(fail)?;
    task["log"] = json!(entries);
    task["log_total"] = json!(total);
    task["history_next"] = json!(next);
    task["history_schema"] = json!(1);
    Ok(task)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zerus_replica::NewTask;

    fn task() -> TaskDoc {
        TaskDoc::create(NewTask {
            project: "example",
            id: "T-abcdef",
            text: "Description",
            stage: Stage::Todo,
            target: Target::Any,
            author: Actor::Person,
            rank: "U",
            now: 100,
        })
        .unwrap()
    }

    #[test]
    fn pages_preserve_canonical_order_and_are_indexed() {
        let temp = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(temp.path(), "example").unwrap();
        let dir = temp
            .path()
            .join(format!("{:x}", Sha256::digest(b"example")));
        let mut task = task();
        // Equal times retain the canonical Loro order; merged earlier times
        // need chronological order rather than raw append positions.
        for (at, text) in [(300, "first"), (300, "second"), (200, "earlier")] {
            task.comment(text, &Actor::Person, at).unwrap();
        }
        store.save(&mut task).unwrap();
        let expected = serde_json::to_value(task.view().unwrap().log).unwrap();
        let mut collected = Vec::new();
        let mut before = None;
        loop {
            let p = page(&store, &dir, task.uuid(), 2, before.as_deref()).unwrap();
            assert!(p["log"].as_array().unwrap().len() <= 2);
            let mut older = p["log"].as_array().unwrap().clone();
            older.extend(collected);
            collected = older;
            before = p["history_next"].as_str().map(str::to_owned);
            if before.is_none() {
                break;
            }
        }
        assert_eq!(json!(collected), expected);
        let db = open(&dir).unwrap();
        let plan: String = db.query_row("EXPLAIN QUERY PLAN SELECT id,payload FROM entries WHERE (at,ordinal)<(300,10) ORDER BY at DESC,ordinal DESC LIMIT 2", [], |r| r.get(3)).unwrap();
        assert!(plan.contains("entries_chronological"), "{plan}");
    }

    #[test]
    fn cursor_survives_append_compaction_and_cache_eviction() {
        let temp = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(temp.path(), "example").unwrap();
        let dir = temp
            .path()
            .join(format!("{:x}", Sha256::digest(b"example")));
        let mut a = task();
        for n in 1..=6 {
            a.comment(&format!("note {n}"), &Actor::Person, 100 + n)
                .unwrap();
        }
        store.save(&mut a).unwrap();
        let latest = page(&store, &dir, a.uuid(), 2, None).unwrap();
        let cursor = latest["history_next"].as_str().unwrap();
        let old = page(&store, &dir, a.uuid(), 2, Some(cursor)).unwrap();
        a.comment("new", &Actor::Person, 200).unwrap();
        store.save(&mut a).unwrap();
        assert_eq!(
            page(&store, &dir, a.uuid(), 2, Some(cursor)).unwrap()["log"],
            old["log"]
        );
        let executor = SessionKey {
            machine: "example-computer".into(),
            session: "codex/example/worker".into(),
            session_id: "run_id:example".into(),
        };
        a.record_send(&executor, "dispatch", &Actor::Person, 201)
            .unwrap();
        a.record_report("Result", &Actor::Agent(executor), 202)
            .unwrap();
        a.set_stage(Stage::Done, &Actor::Person, 203).unwrap();
        store.save(&mut a).unwrap(); // Done folds the updates into the snapshot.
        assert_eq!(
            page(&store, &dir, a.uuid(), 2, Some(cursor)).unwrap()["log"],
            old["log"]
        );
        // Another selected task evicts the projection, not the durable cursor.
        let mut b = task();
        store.save(&mut b).unwrap();
        page(&store, &dir, b.uuid(), 1, None).unwrap();
        assert!(page(&store, &dir, b.uuid(), 2, Some(cursor)).is_err());
        assert_eq!(
            page(&store, &dir, a.uuid(), 2, Some(cursor)).unwrap()["log"],
            old["log"]
        );
        let db = open(&dir).unwrap();
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM current", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
    }

    #[test]
    fn invalid_limits_and_cursor_are_refused() {
        for raw in ["0", "81", "-1", "garbage"] {
            assert!(limit(&["--history-limit".into(), raw.into()]).is_err());
        }
        assert_eq!(limit(&[]).unwrap(), None);
        assert_eq!(
            limit(&["--history-limit".into(), "80".into()]).unwrap(),
            Some(80)
        );
        assert!(limit(&["--history-before".into(), "cursor".into()]).is_err());
    }
}
