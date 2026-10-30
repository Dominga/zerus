//! Short-lived directory discovery. Only bounded filename/stamp batches enter
//! the disk queue; provider text and full binding JSON are never retained here.
//! An interrupted iterator is not serialized or treated as a complete catalog.
use super::*;
use std::collections::VecDeque;

const BATCH: usize = 256;
pub(crate) struct DiscoveryBudget {
    pub entries: usize,
    pub path_bytes: usize,
    pub deadline: Instant,
}
impl DiscoveryBudget {
    pub fn interactive() -> Self {
        Self {
            entries: 65_536,
            path_bytes: 8 << 20,
            deadline: Instant::now() + Duration::from_millis(500),
        }
    }
}
#[derive(Default, Serialize)]
pub(crate) struct DiscoveryProgress {
    pub entries: usize,
    pub path_bytes: usize,
    pub staged: usize,
    pub complete: bool,
    pub changed: bool,
}

impl Store {
    pub(super) fn catalog_status(&self) -> Result<Value> {
        let exists: bool = self
            .index
            .connection()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='catalog_totals')",
                [],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if !exists {
            return Ok(json!({"discovery":"pending","queued_paths":0,"pending_paths":0}));
        }
        let (paths, pending): (u64, u64) = self
            .index
            .connection()
            .query_row(
                "SELECT paths,pending FROM catalog_totals WHERE id=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        let complete: bool = self
            .index
            .connection()
            .query_row(
                "SELECT COALESCE((SELECT value='1' FROM meta WHERE key='catalog_complete'),0)",
                [],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        let fresh = if complete {
            let saved: String = self
                .index
                .connection()
                .query_row(
                    "SELECT value FROM meta WHERE key='catalog_stamp'",
                    [],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            saved
                == directory_stamp(&roots(
                    self.directory.parent().ok_or("Invalid index directory")?,
                ))?
        } else {
            false
        };
        let current: bool = self
            .index
            .connection()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='catalog_source_fallback')",
                [],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        let (fallback, unknown, invalid) = if current {
            let fallback: u64 = self
                .index
                .connection()
                .query_row("SELECT fallback FROM catalog_totals WHERE id=1", [], |r| {
                    r.get(0)
                })
                .map_err(|e| e.to_string())?;
            let unknown: bool = self
                .index
                .connection()
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM catalog_queue WHERE identity IS NULL LIMIT 1)",
                    [],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            let invalid: bool = self
                .index
                .connection()
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM catalog_queue WHERE skipped=1 LIMIT 1)",
                    [],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            (fallback, unknown, invalid)
        } else {
            (paths, paths > 0, false)
        };
        let overflow: bool = self
            .index
            .connection()
            .query_row(
                "SELECT COALESCE((SELECT value='1' FROM meta WHERE key='invalidation_overflow'),0)",
                [],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        Ok(
            json!({"invalidation_overflow":overflow,"discovery":if fresh{"complete"}else if complete{"stale"}else{"incomplete"},"queued_paths":paths,"pending_paths":pending,
            "fallback_sources":fallback,"unknown_metadata":unknown,"invalid_metadata":invalid}),
        )
    }

    fn catalog_schema(&self) -> Result<()> {
        // Schema/migrations are one atomic disk commit. A sequence of implicit
        // DDL commits can spend the whole discovery deadline on Windows before
        // visiting even the first directory entry.
        let tx = self
            .index
            .connection()
            .unchecked_transaction()
            .map_err(|e| e.to_string())?;
        self.index.connection().execute_batch("CREATE TABLE IF NOT EXISTS catalog_queue(
            id INTEGER PRIMARY KEY AUTOINCREMENT,path TEXT NOT NULL UNIQUE,
            kind TEXT NOT NULL,stamp TEXT NOT NULL,seen TEXT NOT NULL,
            done INTEGER NOT NULL CHECK(done IN(0,1)),identity TEXT);
            CREATE INDEX IF NOT EXISTS catalog_pending ON catalog_queue(done,id);
            CREATE INDEX IF NOT EXISTS catalog_seen ON catalog_queue(seen,id);
            CREATE TABLE IF NOT EXISTS catalog_totals(id INTEGER PRIMARY KEY CHECK(id=1),paths INTEGER NOT NULL,pending INTEGER NOT NULL);
            INSERT OR IGNORE INTO catalog_totals(id,paths,pending) VALUES(1,0,0);
            CREATE TRIGGER IF NOT EXISTS catalog_insert AFTER INSERT ON catalog_queue BEGIN
                UPDATE catalog_totals SET paths=paths+1,pending=pending+1-NEW.done WHERE id=1; END;
            CREATE TRIGGER IF NOT EXISTS catalog_update AFTER UPDATE OF done ON catalog_queue BEGIN
                UPDATE catalog_totals SET pending=pending+OLD.done-NEW.done WHERE id=1; END;
            CREATE TRIGGER IF NOT EXISTS catalog_delete AFTER DELETE ON catalog_queue BEGIN
                UPDATE catalog_totals SET paths=paths-1,pending=pending-1+OLD.done WHERE id=1; END;")
            .map_err(|e|e.to_string())?;
        let columns: Vec<String> = self
            .index
            .connection()
            .prepare("PRAGMA table_info(catalog_queue)")
            .map_err(|e| e.to_string())?
            .query_map([], |r| r.get(1))
            .map_err(|e| e.to_string())?
            .collect::<rusqlite::Result<_>>()
            .map_err(|e| e.to_string())?;
        for (name, definition) in [
            ("source", "TEXT"),
            ("fallback", "INTEGER NOT NULL DEFAULT 1"),
            ("updated", "REAL NOT NULL DEFAULT 0"),
            ("skipped", "INTEGER NOT NULL DEFAULT 0"),
        ] {
            if !columns.iter().any(|column| column == name) {
                self.index
                    .connection()
                    .execute(
                        &format!("ALTER TABLE catalog_queue ADD COLUMN {name} {definition}"),
                        [],
                    )
                    .map_err(|e| e.to_string())?;
            }
        }
        let counter: bool = self
            .index
            .connection()
            .prepare("PRAGMA table_info(catalog_totals)")
            .map_err(|e| e.to_string())?
            .query_map([], |r| r.get::<_, String>(1))
            .map_err(|e| e.to_string())?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| e.to_string())?
            .iter()
            .any(|column| column == "fallback");
        if !counter {
            tx.execute_batch(
                "ALTER TABLE catalog_totals ADD COLUMN fallback INTEGER NOT NULL DEFAULT 0;
                UPDATE catalog_totals SET fallback=paths WHERE id=1",
            )
            .map_err(|e| e.to_string())?;
        }
        self.index.connection().execute_batch("CREATE INDEX IF NOT EXISTS catalog_known ON catalog_queue(identity,id);
            CREATE INDEX IF NOT EXISTS catalog_source ON catalog_queue(source,id);
            CREATE INDEX IF NOT EXISTS catalog_source_fallback ON catalog_queue(source,fallback,id);
            CREATE INDEX IF NOT EXISTS catalog_fallback ON catalog_queue(fallback,updated DESC,id DESC);
            CREATE INDEX IF NOT EXISTS catalog_skipped ON catalog_queue(skipped,id);
            CREATE TRIGGER IF NOT EXISTS fallback_insert AFTER INSERT ON catalog_queue BEGIN
                UPDATE catalog_totals SET fallback=fallback+NEW.fallback WHERE id=1; END;
            CREATE TRIGGER IF NOT EXISTS fallback_update AFTER UPDATE OF fallback ON catalog_queue BEGIN
                UPDATE catalog_totals SET fallback=fallback+NEW.fallback-OLD.fallback WHERE id=1; END;
            CREATE TRIGGER IF NOT EXISTS fallback_delete AFTER DELETE ON catalog_queue BEGIN
                UPDATE catalog_totals SET fallback=fallback-OLD.fallback WHERE id=1; END;").map_err(|e|e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }

    /// Called only for explicit search demand. Handles live within this call,
    /// and filename batches are bounded independently of transcript ingestion.
    /// G1 can call the same bounded operation on its future owned worker.
    pub fn discover(
        &self,
        state: &Path,
        budget: &DiscoveryBudget,
        cancel: &AtomicBool,
    ) -> Result<DiscoveryProgress> {
        if budget.entries > 65_536
            || budget.path_bytes > 8 << 20
            || budget.deadline > Instant::now() + Duration::from_secs(2)
        {
            return Err("Invalid catalog discovery budget".into());
        }
        if crate::platform::canonical(state).map_err(|e| e.to_string())?
            != crate::platform::canonical(self.directory.parent().ok_or("Invalid index directory")?)
                .map_err(|e| e.to_string())?
        {
            return Err("Catalog discovery configuration changed".into());
        }
        let mut progress = DiscoveryProgress::default();
        if cancel.load(Ordering::Relaxed)
            || Instant::now() >= budget.deadline
            || budget.entries == 0
        {
            return Ok(progress);
        }
        self.catalog_schema()?;
        // Setup is already committed if this pass expires here. The next pass
        // reuses it instead of repeating the initial schema/migration writes.
        if cancel.load(Ordering::Relaxed) || Instant::now() >= budget.deadline {
            return Ok(progress);
        }
        let roots = roots(state);
        let before = directory_stamp(&roots)?;
        let saved: Option<String> = self
            .index
            .connection()
            .query_row(
                "SELECT value FROM meta WHERE key='catalog_stamp'",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if saved.as_deref() == Some(before.as_str()) {
            progress.complete = true;
            return Ok(progress);
        }
        self.index.catalog_ready(false).map_err(|e| e.to_string())?;
        self.index
            .connection()
            .execute(
                "INSERT OR REPLACE INTO meta VALUES('catalog_complete','0')",
                [],
            )
            .map_err(|e| e.to_string())?;
        let generation = uuid::Uuid::new_v4().to_string();
        let mut streams = VecDeque::new();
        for (kind, path) in &roots {
            match fs::read_dir(path) {
                Ok(stream) => streams.push_back((*kind, stream)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(_) => return Err("Search catalog directory is unreadable".into()),
            }
        }
        let mut batch = Vec::with_capacity(BATCH);
        let mut exhausted = true;
        'walk: while let Some((kind, stream)) = streams.front_mut() {
            for entry in stream.by_ref() {
                if cancel.load(Ordering::Relaxed)
                    || Instant::now() >= budget.deadline
                    || progress.entries >= budget.entries
                {
                    exhausted = false;
                    break 'walk;
                }
                progress.entries += 1;
                let entry = entry.map_err(|_| "Search catalog entry is unreadable")?;
                let path = entry.path();
                if !candidate(&path, kind) {
                    continue;
                }
                if !entry.file_type().map_err(|e| e.to_string())?.is_file() {
                    continue;
                }
                let Some(path_text) = path.to_str() else {
                    return Err("Search catalog path is not UTF-8".into());
                };
                if path_text.len() > 16 << 10
                    || progress.path_bytes + path_text.len() > budget.path_bytes
                {
                    exhausted = false;
                    break 'walk;
                }
                let (identity, metadata) =
                    crate::platform::FileIdentity::at(&path).map_err(|e| e.to_string())?;
                if !metadata.is_file() {
                    return Err("Search catalog entry changed kind".into());
                }
                let stamp = format!(
                    "{}:{}:{}:{}:{}",
                    identity.device,
                    identity.inode,
                    metadata.len(),
                    identity.modified,
                    identity.modified_nanos
                );
                progress.path_bytes += path_text.len();
                batch.push((path_text.to_owned(), *kind, stamp));
                if batch.len() == BATCH {
                    self.stage(&batch, &generation)?;
                    progress.staged += batch.len();
                    batch.clear();
                }
            }
            streams.pop_front();
        }
        if !batch.is_empty() {
            self.stage(&batch, &generation)?;
            progress.staged += batch.len();
        }
        let after = directory_stamp(&roots)?;
        progress.changed = before != after;
        progress.complete = exhausted && !progress.changed;
        if progress.complete {
            let tx = self
                .index
                .connection()
                .unchecked_transaction()
                .map_err(|e| e.to_string())?;
            tx.execute(
                "INSERT OR REPLACE INTO meta VALUES('catalog_stamp',?)",
                [after],
            )
            .map_err(|e| e.to_string())?;
            tx.execute(
                "INSERT OR REPLACE INTO meta VALUES('catalog_complete','1')",
                [],
            )
            .map_err(|e| e.to_string())?;
            tx.execute(
                "INSERT OR REPLACE INTO meta VALUES('catalog_generation',?)",
                [generation],
            )
            .map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
        }
        // Even complete discovery leaves coverage incomplete until the queued
        // sources and their public histories have actually been consumed.
        Ok(progress)
    }

    fn stage(&self, batch: &[(String, &str, String)], generation: &str) -> Result<()> {
        let tx = self
            .index
            .connection()
            .unchecked_transaction()
            .map_err(|e| e.to_string())?;
        for (path, kind, stamp) in batch {
            tx.execute("INSERT INTO catalog_queue(path,kind,stamp,seen,done) VALUES(?,?,?,?,0)
                ON CONFLICT(path) DO UPDATE SET
                done=CASE WHEN catalog_queue.stamp=excluded.stamp THEN catalog_queue.done ELSE 0 END,
                identity=CASE WHEN catalog_queue.stamp=excluded.stamp THEN catalog_queue.identity ELSE NULL END,
                source=CASE WHEN catalog_queue.stamp=excluded.stamp THEN catalog_queue.source ELSE NULL END,
                fallback=CASE WHEN catalog_queue.stamp=excluded.stamp THEN catalog_queue.fallback ELSE 1 END,
                skipped=CASE WHEN catalog_queue.stamp=excluded.stamp THEN catalog_queue.skipped ELSE 0 END,
                stamp=excluded.stamp,seen=excluded.seen",params![path,kind,stamp,generation]).map_err(|e|e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())
    }
}

fn roots(state: &Path) -> [(&'static str, PathBuf); 4] {
    [
        ("binding", state.to_owned()),
        ("binding", state.join("bindings")),
        ("archive", state.join("archives")),
        ("native", state.join("dsh/bindings")),
    ]
}
fn candidate(path: &Path, kind: &str) -> bool {
    if path.extension().is_none_or(|v| v != "json") {
        return false;
    }
    let Some(stem) = path.file_stem().and_then(|v| v.to_str()) else {
        return false;
    };
    if kind == "archive" {
        uuid::Uuid::parse_str(stem).is_ok()
    } else {
        stem.len() == 64
            && stem
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    }
}
fn directory_stamp(roots: &[(&str, PathBuf)]) -> Result<String> {
    let mut hash = Sha256::new();
    for (kind, path) in roots {
        hash.update(kind);
        hash.update(path.to_string_lossy().as_bytes());
        match crate::platform::FileIdentity::at(path) {
            Ok((id, meta)) if meta.is_dir() => hash.update(format!(
                "{}:{}:{}:{}",
                id.device, id.inode, id.modified, id.modified_nanos
            )),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => hash.update(b"absent"),
            _ => return Err("Search catalog root is unreadable or changed kind".into()),
        }
    }
    Ok(format!("{:x}", hash.finalize()))
}

mod queue;
#[cfg(test)]
mod tests;
