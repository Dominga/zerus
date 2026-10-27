//! What `hgs watch` (src/watch) reads from session state: inspections and the
//! Activity journal exactly as `hgs inspect` reads them, as values instead of
//! printed output. Nothing here writes session state beyond what `hgs inspect`
//! and `hgs ls` already do.
use super::*;
use rusqlite::{Connection, OpenFlags};
use std::time::Duration;

/// The session state folder.
pub(crate) fn state_root() -> PathBuf {
    root()
}

/// Whether `name` is a DeepSeek or OpenCode session. Their Activity comes from
/// the agent's own host and each read replaces the previous window.
pub(crate) fn native(name: &str) -> bool {
    dsh::exists(name) || opencode::exists(name)
}

/// `hgs inspect NAME --after AFTER --skip-processes`.
pub(crate) fn inspect(name: &str, after: i64) -> Result<Value> {
    local_sessions()?;
    if opencode::exists(name) {
        return opencode::inspect(name, None);
    }
    if dsh::exists(name) {
        return dsh::inspect(name, None);
    }
    journal::inspection(name, after, None, false)
}

/// A read-only connection to the Activity journal, once it exists. It never
/// creates the file or its schema, and holds no lock between statements.
pub(crate) fn journal() -> Option<Connection> {
    local_sessions().ok()?;
    let path = root().join("events.sqlite3");
    if !path.is_file() {
        return None;
    }
    let db = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    db.busy_timeout(Duration::from_millis(250)).ok()?;
    Some(db)
}

/// Changes whenever another connection commits to the journal.
pub(crate) fn data_version(db: &Connection) -> Result<i64> {
    db.query_row("PRAGMA data_version", [], |row| row.get(0))
        .map_err(|e| e.to_string())
}

/// The oldest and newest retained `seq` (0 when the journal is empty).
pub(crate) fn bounds(db: &Connection) -> Result<(i64, i64)> {
    db.query_row(
        "SELECT COALESCE(MIN(seq),0), COALESCE(MAX(seq),0) FROM events",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .map_err(|e| e.to_string())
}

/// The journal names with rows after `after`, from at most `limit` rows.
pub(crate) fn changed_names(db: &Connection, after: i64, limit: usize) -> Result<Vec<String>> {
    let mut statement = db
        .prepare("SELECT DISTINCT name FROM (SELECT name FROM events WHERE seq>?1 ORDER BY seq LIMIT ?2)")
        .map_err(|e| e.to_string())?;
    let names = statement
        .query_map(rusqlite::params![after, limit as i64], |row| row.get::<_, Option<String>>(0))
        .map_err(|e| e.to_string())?
        .filter_map(|row| row.ok().flatten())
        .collect();
    Ok(names)
}

/// The rows a tracked session's Activity shows: its journal name and the
/// conversations its timeline spans, taken from its saved binding.
pub(crate) struct Timeline {
    record: Value,
    earlier: String,
}

impl Timeline {
    /// The journal name, which a rename keeps.
    pub(crate) fn journal(&self) -> &str {
        journal::journal_name(&self.record)
    }

    pub(crate) fn run_id(&self) -> &str {
        string(&self.record, "run_id")
    }

    /// Events after `after` as `inspect` shapes them, oldest first: the first
    /// `limit`, or with `newest` the last `limit`.
    pub(crate) fn events(&self, db: &Connection, after: i64, limit: usize, newest: bool) -> Result<Vec<Value>> {
        let rows = journal::timeline_rows(
            db,
            self.journal(),
            self.record["conversation_id"].as_str(),
            &self.earlier,
            after,
            limit,
            newest,
        )?;
        rows.into_iter()
            .map(|(seq, payload)| journal::timeline_event(&self.record, seq, &payload))
            .collect()
    }
}

/// The timeline of a tracked session; None for a session without a saved
/// binding (DeepSeek, OpenCode and untracked sessions).
pub(crate) fn timeline(name: &str) -> Result<Option<Timeline>> {
    local_sessions()?;
    if !record_path(name).exists() {
        return Ok(None);
    }
    let record = read(name)?;
    let earlier = clear_context::earlier_conversations(&record).to_string();
    Ok(Some(Timeline { record, earlier }))
}

/// Whether a run is in Archive. Rare: only for a catalog row that disappeared.
pub(crate) fn archived(run_id: &str) -> bool {
    if run_id.is_empty() || local_sessions().is_err() || !root().join("archives").is_dir() {
        return false;
    }
    let Ok(_guard) = lock(None) else { return false };
    let Ok(db) = archive_catalog::catalog() else { return false };
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM archive_rows WHERE run=?1)",
        [run_id],
        |row| row.get::<_, bool>(0),
    )
    .unwrap_or(false)
}

/// Whether `name` still has a saved binding of any agent.
pub(crate) fn saved(name: &str) -> bool {
    local_sessions().is_ok() && (record_path(name).exists() || native(name))
}
