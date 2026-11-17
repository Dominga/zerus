//! Disposable, bounded projections. Every lookup first opens the canonical
//! source and compares its native version; cached data grants no authority.
use super::*;

pub(super) const VERSION: i64 = 2;
const CACHE_BYTES: i64 = 32 << 20;
const ENTRY_BYTES: usize = 1 << 20;

pub(super) fn schema(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS search_metadata(path TEXT PRIMARY KEY,stamp TEXT NOT NULL,body TEXT NOT NULL,bytes INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS search_metadata_total(id INTEGER PRIMARY KEY CHECK(id=1),bytes INTEGER NOT NULL,rows INTEGER NOT NULL);
        INSERT OR IGNORE INTO search_metadata_total VALUES(1,0,0);
        CREATE TRIGGER IF NOT EXISTS search_metadata_insert AFTER INSERT ON search_metadata BEGIN UPDATE search_metadata_total SET bytes=bytes+NEW.bytes,rows=rows+1 WHERE id=1; END;
        CREATE TRIGGER IF NOT EXISTS search_metadata_delete AFTER DELETE ON search_metadata BEGIN UPDATE search_metadata_total SET bytes=bytes-OLD.bytes,rows=rows-1 WHERE id=1; END;
        CREATE TRIGGER IF NOT EXISTS search_metadata_update AFTER UPDATE ON search_metadata BEGIN UPDATE search_metadata_total SET bytes=bytes+NEW.bytes-OLD.bytes WHERE id=1; END;")
        .map_err(|e| e.to_string())
}

pub(super) fn open(state: &Path) -> Option<Connection> {
    let directory = crate::platform::canonical(state.join("content-search-v1")).ok()?;
    if !crate::platform::verify_private(&directory, crate::platform::Private::Directory).ok()? {
        return None;
    }
    let path = directory.join("index.sqlite3");
    #[cfg(unix)]
    let identity = crate::sqlite_file::private(&path).ok()?;
    #[cfg(not(unix))]
    if !crate::platform::verify_private(&path, crate::platform::Private::File).ok()? {
        return None;
    }
    let db = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .ok()?;
    #[cfg(unix)]
    crate::sqlite_file::bind_private(&db, &path, identity).ok()?;
    db.busy_timeout(Duration::from_millis(25)).ok()?;
    db.execute_batch("PRAGMA cache_size=-512").ok()?;
    let configuration: String = db
        .query_row(
            "SELECT value FROM meta WHERE key='configuration'",
            [],
            |r| r.get(0),
        )
        .ok()?;
    let canonical = crate::platform::canonical(state).ok()?;
    if configuration
        != format!(
            "{:x}",
            Sha256::digest(canonical.to_string_lossy().as_bytes())
        )
    {
        return None;
    }
    Some(db)
}

// Resolve an ordinary /tmp or /var parent alias, never the final source link.
// Foreground callers and the canonical-root worker must use the same key.
pub(super) fn path(path: &Path) -> Option<PathBuf> {
    Some(
        crate::platform::canonical(path.parent()?)
            .ok()?
            .join(path.file_name()?),
    )
}

pub(super) fn get(db: &Connection, path: &Path, stamp: &str) -> Option<Value> {
    let path = self::path(path)?;
    let body: String = db
        .query_row(
            "SELECT substr(body,1,?) FROM search_metadata WHERE path=? AND stamp=? AND bytes<=?",
            params![
                ENTRY_BYTES as i64 + 1,
                path.to_str()?,
                stamp,
                ENTRY_BYTES as i64
            ],
            |r| r.get(0),
        )
        .ok()?;
    if body.len() > ENTRY_BYTES {
        return None;
    }
    let value: Value = serde_json::from_str(&body).ok()?;
    value.as_object()?;
    Some(value)
}

pub(super) fn put(db: &Connection, path: &Path, stamp: &str, record: &Value) -> Result<()> {
    let path = self::path(path).ok_or("Search metadata parent is unavailable")?;
    let body = serde_json::to_string(record).map_err(|e| e.to_string())?;
    if body.len() > ENTRY_BYTES {
        return Ok(());
    }
    let (bytes, rows): (i64, i64) = db
        .query_row(
            "SELECT bytes,rows FROM search_metadata_total WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())?;
    if bytes + body.len() as i64 > CACHE_BYTES || rows >= 50_000 {
        db.execute("DELETE FROM search_metadata WHERE path IN(SELECT path FROM search_metadata ORDER BY path LIMIT 32)", []).map_err(|e| e.to_string())?;
        // A batch of small rows may not make room for a large projection. Skip
        // it rather than exceeding the cap or doing an unbounded eviction.
        let bytes: i64 = db
            .query_row(
                "SELECT bytes FROM search_metadata_total WHERE id=1",
                [],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if bytes + body.len() as i64 > CACHE_BYTES {
            return Ok(());
        }
    }
    db.execute("INSERT INTO search_metadata VALUES(?,?,?,?) ON CONFLICT(path) DO UPDATE SET stamp=excluded.stamp,body=excluded.body,bytes=excluded.bytes", params![path.to_string_lossy(),stamp,body,body.len() as i64]).map_err(|e| e.to_string())?;
    Ok(())
}

pub(super) fn stamp(file: &fs::File) -> Result<String> {
    let (id, meta) =
        crate::platform::FileIdentity::with_metadata(file).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    let changed = {
        use std::os::unix::fs::MetadataExt;
        (meta.ctime(), meta.ctime_nsec())
    };
    #[cfg(not(unix))]
    let changed = meta.created().ok();
    Ok(format!("{VERSION}:{id:?}:{}:{changed:?}", meta.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prepared_metadata_avoids_private_body_reads_but_never_survives_source_replacement() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path();
        let store = Store::open(&state.join("content-search-v1")).unwrap();
        schema(store.index.connection()).unwrap();
        let path = state.join("source.json");
        let source = json!({"version":1,"name":"codex/example/session","run_id":"first",
            "shell_jobs":{"job":{"output":"x".repeat(9 << 20)}}});
        fs::write(&path, source.to_string()).unwrap();
        let mut preparation = metadata::Reader::default();
        let budget = Budget {
            deadline: Instant::now() + Duration::from_secs(2),
            ..Budget::interactive()
        };
        assert!(matches!(
            preparation
                .read(&path, &mut Progress::default(), &budget, 0)
                .unwrap(),
            metadata::Record::Ready(_)
        ));
        preparation
            .cache_into(store.index.connection(), &path)
            .unwrap();
        store
            .index
            .connection()
            .execute_batch("BEGIN EXCLUSIVE")
            .unwrap();
        let mut blinded = metadata::Reader::foreground(state);
        store.index.connection().execute_batch("ROLLBACK").unwrap();
        let mut progress = Progress::default();
        let record = blinded.read(&path, &mut progress, &budget, 0).unwrap();
        assert!(matches!(record, metadata::Record::Ready(_)));
        assert_eq!(
            progress.metadata_io, 0,
            "warm lookup reread the private job body"
        );
        assert!(progress.bytes < 1024);
        // A different canonical version must not borrow the old seed. This
        // record exceeds the foreground raw limit, so no uncached proof fits.
        #[cfg(unix)]
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        let changed = source.to_string().replace("first", "other");
        fs::write(&path, changed).unwrap();
        #[cfg(unix)]
        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        assert_eq!(
            metadata::Reader::foreground(state)
                .read(&path, &mut Progress::default(), &budget, 0)
                .unwrap(),
            metadata::Record::OverBudget
        );
        fs::remove_file(&path).unwrap();
        assert!(metadata::Reader::foreground(state)
            .read(&path, &mut Progress::default(), &budget, 0)
            .is_err());
    }
    #[test]
    fn compact_cache_has_a_constant_byte_and_row_bound() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&temp.path().join("content-search-v1")).unwrap();
        schema(store.index.connection()).unwrap();
        for n in 0..800 {
            put(
                store.index.connection(),
                &temp.path().join(format!("record-{n:04}")),
                "version",
                &json!({"prompt":"x".repeat(60 << 10)}),
            )
            .unwrap();
        }
        let (bytes, rows): (i64, i64) = store
            .index
            .connection()
            .query_row(
                "SELECT bytes,rows FROM search_metadata_total WHERE id=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert!(bytes <= CACHE_BYTES && rows <= 50_000);
    }
    #[cfg(unix)]
    #[test]
    fn ordinary_parent_aliases_share_the_workers_canonical_projection_key() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path().join("state");
        fs::create_dir(&state).unwrap();
        let alias = temp.path().join("alias");
        std::os::unix::fs::symlink(&state, &alias).unwrap();
        let store = Store::open(&state.join("content-search-v1")).unwrap();
        schema(store.index.connection()).unwrap();
        let source = state.join("source.json");
        fs::write(
            &source,
            json!({"version":1,"shell_jobs":{"job":{"output":"x".repeat(9 << 20)}}}).to_string(),
        )
        .unwrap();
        let mut reader = metadata::Reader::default();
        let budget = Budget {
            deadline: Instant::now() + Duration::from_secs(2),
            ..Budget::interactive()
        };
        assert!(matches!(
            reader
                .read(&source, &mut Progress::default(), &budget, 0)
                .unwrap(),
            metadata::Record::Ready(_)
        ));
        reader
            .cache_into(store.index.connection(), &source)
            .unwrap();
        let mut progress = Progress::default();
        assert!(matches!(
            metadata::Reader::foreground(&alias)
                .read(&alias.join("source.json"), &mut progress, &budget, 0)
                .unwrap(),
            metadata::Record::Ready(_)
        ));
        assert_eq!(progress.metadata_io, 0);
    }
}
