//! WhiteWh fork: durable team state. One private SQLite file holds every team
//! (whole JSON record with a revision), the feed (keyset pages) and the
//! idempotent operations, opened as the coordination waits are.
use super::super::*;
use super::model::Team;
use anyhow::{bail, ensure, Context};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};
use std::{
    fs,
    time::{Duration, Instant},
};

/// Feed entries per page.
pub(crate) const PAGE: i64 = 50;

pub(crate) fn path() -> PathBuf {
    root().join("teams/teams.sqlite3")
}

/// Task 8: the heads of the teams at `path` (see [`Store::heads`]); no store
/// yet is no team.
pub(crate) fn heads_at(path: &Path, limit: usize) -> anyhow::Result<(Vec<Value>, bool)> {
    if !path.exists() {
        return Ok((Vec::new(), false));
    }
    Store::open_at(path, false)?.heads(limit)
}

/// Task 8: the files whose stat changes when a team changes: the store, its
/// rollback journal and (task 105) its write-ahead log, where a WAL store's
/// writes land until a checkpoint copies them into the store.
pub(crate) fn files(path: &Path) -> Vec<PathBuf> {
    let beside = |suffix: &str| {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        PathBuf::from(name)
    };
    vec![path.to_path_buf(), beside("-journal"), beside("-wal")]
}

/// Task 105: how the store keeps its transactions. WAL lets every reader
/// (`hgs team list`, the GUI's polls, status) read the last committed state
/// while a writer works; the rollback journal (DELETE) made them wait behind
/// a long write, and a write behind a long read, up to "database is locked".
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Journal {
    Wal,
    /// The mode before task 105, kept for measurements and as the fallback
    /// where SQLite refuses WAL (a network folder).
    Delete,
}

/// Task 105: how long a write open waits to switch the journal mode.
const SWITCH_WAIT: Duration = Duration::from_secs(2);

/// Task 105: a WAL store's log is truncated to this size after a checkpoint.
/// A team's record is megabytes of JSON, so a smaller log would be cut and
/// grown again on almost every write.
const WAL_KEEP_BYTES: i64 = 64 * 1024 * 1024;
/// Task 105: pages written before a commit checkpoints the log into the store
/// (SQLite's default is 1000, about 4 MB: one write of a large team). Measured
/// on a copy of team 33fbafeb's store, fewer checkpoints kept the writes'
/// p95 lower and the reads' steadier.
const WAL_CHECKPOINT_PAGES: i64 = 4000;

/// Task 105: the `-wal` and `-shm` files stay when the last connection closes.
/// A reader whose sandbox may not create files beside the store (a read-only
/// sandbox) can then still open it read-only: SQLite reads a WAL database
/// without write access only when those files exist.
fn persist_wal(db: &Connection) {
    let mut on: std::os::raw::c_int = 1;
    // SAFETY: the handle belongs to this open connection for the call; the
    // argument is the documented int pointer of SQLITE_FCNTL_PERSIST_WAL.
    unsafe {
        rusqlite::ffi::sqlite3_file_control(
            db.handle(),
            c"main".as_ptr(),
            rusqlite::ffi::SQLITE_FCNTL_PERSIST_WAL,
            (&mut on as *mut std::os::raw::c_int).cast(),
        );
    }
}

/// One feed entry to append with a mutation.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Event {
    pub kind: &'static str,
    pub payload: Value,
}

impl Event {
    pub(crate) fn new(kind: &'static str, payload: Value) -> Self {
        Self { kind, payload }
    }
}

pub(crate) struct Store {
    db: Connection,
}

/// The stable identity of a request's content, for idempotent replays.
pub(crate) fn digest(value: &Value) -> String {
    format!("{:x}", Sha256::digest(value.to_string().as_bytes()))
}

pub(crate) fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| !id.is_nil() && id.to_string() == value)
}

/// Task 25: how long one SQLite call waits for the lock another process holds
/// (a reader in rollback-journal mode holds it until its read ends).
const BUSY_WAIT: Duration = Duration::from_secs(30);
/// Task 25: how long a team write as a whole keeps waiting for the database:
/// SQLite answers some contention at once, without its busy wait.
const WRITE_WAIT: Duration = Duration::from_secs(60);

/// Task 25: the database is held by another connection right now.
fn busy(error: &rusqlite::Error) -> bool {
    matches!(error, rusqlite::Error::SqliteFailure(failure, _)
        if matches!(failure.code, rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked))
}

/// Task 25: a write that still found the database busy after `WRITE_WAIT`
/// changed nothing; the same request id applies it once when run again.
fn busy_error(error: rusqlite::Error) -> anyhow::Error {
    if busy(&error) {
        anyhow::anyhow!(
            "The team database stayed busy for {} s (another Zerus process was writing or reading it); nothing was changed. \
             Run the same command again with the same request_id: it is applied once.",
            WRITE_WAIT.as_secs()
        )
    } else {
        error.into()
    }
}

/// Task 25: starts a write transaction, waiting while another connection
/// (the dispatcher, a member's command, a status read) holds the database.
/// Team 33fbafeb: `hgs team reassign` failed with "database is locked" after
/// the 2 s busy wait while the dispatcher wrote.
fn begin(db: &Connection) -> anyhow::Result<rusqlite::Transaction<'_>> {
    let deadline = Instant::now() + WRITE_WAIT;
    loop {
        match rusqlite::Transaction::new_unchecked(db, TransactionBehavior::Immediate) {
            Ok(transaction) => return Ok(transaction),
            Err(error) if busy(&error) && Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            Err(error) => return Err(busy_error(error)),
        }
    }
}

/// Task 25: commits, waiting for readers to finish. A COMMIT that fails with
/// SQLITE_BUSY leaves the transaction open, so it is tried again; any other
/// failure (or the deadline) rolls the whole change back.
fn commit(transaction: rusqlite::Transaction<'_>) -> anyhow::Result<()> {
    let deadline = Instant::now() + WRITE_WAIT;
    loop {
        match transaction.execute_batch("COMMIT") {
            // The connection is back in autocommit: dropping the
            // transaction does nothing more.
            Ok(()) => return Ok(()),
            Err(error) if busy(&error) && Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            Err(error) => return Err(busy_error(error)),
        }
    }
}

/// U20: a member's write the sandbox blocks says what to do, not a bare SQLite
/// error. Everything else is passed through unchanged.
pub(crate) fn write_error(error: impl std::fmt::Display) -> anyhow::Error {
    let text = error.to_string();
    if text.contains("readonly database") || text.contains("read-only database") {
        return anyhow::anyhow!(
            "the sandbox blocks the team store: ask for permission to run hgs team report (or ask/commit), then retry"
        );
    }
    anyhow::anyhow!(text)
}

/// A member in a sandbox runs at Low integrity (DeepSeek's confined shell),
/// which may not write files of the usual Medium label: its `hgs team
/// report` could not record the report. The team folder and what it holds
/// take the Low label once (what is created later inherits it); the folder
/// keeps its owner and permissions. Failing to set it leaves reports from
/// sandboxes to the member's last reply.
#[cfg(windows)]
fn sandbox_writable(dir: &Path) {
    use std::os::windows::process::CommandExt;
    let marker = dir.join(".sandbox-writable");
    if marker.exists() {
        return;
    }
    let done = std::process::Command::new("icacls")
        .arg(dir)
        .args(["/setintegritylevel", "(OI)(CI)L", "/T", "/C", "/Q"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .status()
        .is_ok_and(|status| status.success());
    if done {
        let _ = fs::write(&marker, b"");
    }
}

impl Store {

    pub(crate) fn open(write: bool) -> anyhow::Result<Self> {
        Self::open_at(&path(), write)
    }

    /// Task 117: a read on a hot path (every listing and inspection) waits
    /// at most `wait` for a writer instead of the usual 10 s.
    pub(crate) fn busy_timeout(&self, wait: Duration) -> anyhow::Result<()> {
        Ok(self.db.busy_timeout(wait)?)
    }

    pub(crate) fn open_at(path: &Path, write: bool) -> anyhow::Result<Self> {
        Self::open_journal(path, write, Journal::Wal)
    }

    /// The store's journal mode now: `wal` or `delete`.
    pub(crate) fn journal_mode(&self) -> anyhow::Result<String> {
        Ok(self.db.query_row("PRAGMA journal_mode", [], |row| row.get(0))?)
    }

    /// Opens the store; a write open puts it in `journal` mode. Task 105: an
    /// existing rollback-journal store becomes WAL at its first write open
    /// (SQLite waits for the other connections to finish their transactions);
    /// where SQLite refuses (a busy store, a network folder) it stays as it
    /// was and the next write open tries again. An older hgs reads a WAL store
    /// as it is, and its write open turns it back to DELETE when no other
    /// connection has it open.
    pub(crate) fn open_journal(path: &Path, write: bool, journal: Journal) -> anyhow::Result<Self> {
        if let Ok(meta) = path.symlink_metadata() {
            ensure!(meta.is_file() && !meta.file_type().is_symlink(), "Nonregular team store");
        }
        if write {
            private_dir(path.parent().context("Missing team store parent")?).map_err(anyhow::Error::msg)?;
            if !path.exists() {
                crate::platform::private_open(fs::OpenOptions::new().write(true).create_new(true)).open(path)?;
            }
            #[cfg(windows)]
            sandbox_writable(path.parent().context("Missing team store parent")?);
        } else if !path.exists() {
            bail!("No teams yet");
        }
        let db = Connection::open_with_flags(
            path,
            if write { OpenFlags::SQLITE_OPEN_READ_WRITE } else { OpenFlags::SQLITE_OPEN_READ_ONLY },
        )?;
        // Task 25: a write waits for the lock (it used to give up after 2 s);
        // a read waits less.
        db.busy_timeout(if write { BUSY_WAIT } else { Duration::from_secs(10) })?;
        // Task #28: a sorted read (`team list`, `team current`) spilled to a
        // temporary file in %TEMP%, which a sandboxed member at Low integrity
        // may not create: "unable to open database file". Sorts stay in
        // memory; every such read is bounded by its LIMIT.
        db.pragma_update(None, "temp_store", "MEMORY")?;
        if write {
            let wanted = match journal {
                Journal::Wal => "WAL",
                Journal::Delete => "DELETE",
            };
            // The pragma answers the mode it kept; a refusal keeps the old one.
            // Only a switch takes the store's lock, so it waits briefly: a busy
            // store is switched by a later open instead of delaying this one.
            let current: String = db.query_row("PRAGMA journal_mode", [], |row| row.get(0)).unwrap_or_default();
            let mode = if current.eq_ignore_ascii_case(wanted) {
                current
            } else {
                db.busy_timeout(SWITCH_WAIT)?;
                let mode = db.query_row(&format!("PRAGMA journal_mode={wanted}"), [], |row| row.get(0)).unwrap_or(current);
                db.busy_timeout(BUSY_WAIT)?;
                mode
            };
            if mode.eq_ignore_ascii_case("wal") {
                persist_wal(&db);
                db.pragma_update(None, "journal_size_limit", WAL_KEEP_BYTES)?;
                db.pragma_update(None, "wal_autocheckpoint", WAL_CHECKPOINT_PAGES)?;
            }
            db.execute_batch(
                "PRAGMA max_page_count=262144;
                CREATE TABLE IF NOT EXISTS teams(id TEXT PRIMARY KEY,revision INTEGER NOT NULL,status TEXT NOT NULL,
                    updated REAL NOT NULL,payload TEXT NOT NULL);
                CREATE INDEX IF NOT EXISTS teams_updated ON teams(status,updated);
                CREATE TABLE IF NOT EXISTS events(seq INTEGER PRIMARY KEY AUTOINCREMENT,team TEXT NOT NULL,at REAL NOT NULL,
                    kind TEXT NOT NULL,payload TEXT NOT NULL);
                CREATE INDEX IF NOT EXISTS events_team ON events(team,seq);
                CREATE INDEX IF NOT EXISTS events_team_kind_at ON events(team,kind,at);
                CREATE TABLE IF NOT EXISTS operations(id TEXT PRIMARY KEY,digest TEXT NOT NULL,receipt TEXT NOT NULL,at REAL NOT NULL);",
            )?;
        }
        if write {
            // Build an indexed projection once; triggers keep it in the same
            // transaction as every team write, including dispatcher mutations.
            let mut exists: bool = db.query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='team_rule_members')",
                [],
                |row| row.get(0),
            )?;
            let old_projection = db.query_row("SELECT sql FROM sqlite_master WHERE name='team_rules_insert'", [], |row| row.get::<_,String>(0))
                .optional()?.is_some_and(|sql| !sql.contains("NULLIF"));
            let migration = !exists || old_projection;
            if migration { db.execute_batch("BEGIN IMMEDIATE")?; }
            if old_projection {
                db.execute_batch("DROP TRIGGER IF EXISTS team_rules_insert; DROP TRIGGER IF EXISTS team_rules_update; DROP TRIGGER IF EXISTS team_rules_delete; DELETE FROM team_rule_members;")?;
                exists = false;
            }
            db.execute_batch("CREATE TABLE IF NOT EXISTS team_rule_members(team TEXT NOT NULL,member TEXT NOT NULL,host TEXT NOT NULL,name TEXT NOT NULL,run TEXT NOT NULL,conversation TEXT NOT NULL,launch TEXT NOT NULL,PRIMARY KEY(team,member));
                CREATE INDEX IF NOT EXISTS team_rule_session ON team_rule_members(host,name,run,conversation);
                CREATE INDEX IF NOT EXISTS team_rule_launch ON team_rule_members(launch);
                CREATE TRIGGER IF NOT EXISTS team_rules_delete AFTER DELETE ON teams BEGIN DELETE FROM team_rule_members WHERE team=OLD.id; END;
                CREATE TRIGGER IF NOT EXISTS team_rules_update AFTER UPDATE OF payload ON teams BEGIN
                    DELETE FROM team_rule_members WHERE team=NEW.id;
                    INSERT INTO team_rule_members SELECT NEW.id,json_extract(m.value,'$.id'),COALESCE(NULLIF(json_extract(m.value,'$.session.host'),''),json_extract(m.value,'$.computer'),''),COALESCE(json_extract(m.value,'$.session.name'),''),COALESCE(json_extract(m.value,'$.session.run_id'),''),COALESCE(json_extract(m.value,'$.session.conversation_id'),''),COALESCE(json_extract(m.value,'$.launch_id'),'') FROM json_each(NEW.payload,'$.members') m; END;
                CREATE TRIGGER IF NOT EXISTS team_rules_insert AFTER INSERT ON teams BEGIN
                    INSERT INTO team_rule_members SELECT NEW.id,json_extract(m.value,'$.id'),COALESCE(NULLIF(json_extract(m.value,'$.session.host'),''),json_extract(m.value,'$.computer'),''),COALESCE(json_extract(m.value,'$.session.name'),''),COALESCE(json_extract(m.value,'$.session.run_id'),''),COALESCE(json_extract(m.value,'$.session.conversation_id'),''),COALESCE(json_extract(m.value,'$.launch_id'),'') FROM json_each(NEW.payload,'$.members') m; END;")?;
            if !exists {
                db.execute_batch("INSERT OR IGNORE INTO team_rule_members SELECT t.id,json_extract(m.value,'$.id'),COALESCE(NULLIF(json_extract(m.value,'$.session.host'),''),json_extract(m.value,'$.computer'),''),COALESCE(json_extract(m.value,'$.session.name'),''),COALESCE(json_extract(m.value,'$.session.run_id'),''),COALESCE(json_extract(m.value,'$.session.conversation_id'),''),COALESCE(json_extract(m.value,'$.launch_id'),'') FROM teams t,json_each(t.payload,'$.members') m;")?;
            }
            if migration { db.execute_batch("COMMIT")?; }
        }
        Ok(Self { db })
    }

    pub(crate) fn load(&self, id: &str) -> anyhow::Result<Option<Team>> {
        let payload: Option<String> =
            self.db.query_row("SELECT payload FROM teams WHERE id=?1", params![id], |row| row.get(0)).optional()?;
        payload.map(|text| serde_json::from_str(&text).context("Invalid team record")).transpose()
    }

    /// Startup reads at most two matching records, never the whole roster.
    pub(crate) fn rules_member(&self, record: &Value) -> anyhow::Result<Option<(Team, String)>> {
        self.rules_session(
            "",
            super::super::string(record, "name"),
            super::super::string(record, "run_id"),
            super::super::string(record, "conversation_id"),
            super::super::string(record, "launch_id"),
        )
    }

    pub(crate) fn rules_session(
        &self,
        host: &str,
        name: &str,
        run: &str,
        conversation: &str,
        launch: &str,
    ) -> anyhow::Result<Option<(Team, String)>> {
        let exists: bool = self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='team_rule_members')",
            [],
            |row| row.get(0),
        )?;
        if !exists {
            return Ok(None);
        }
        let mut query = self.db.prepare("SELECT DISTINCT t.payload FROM teams t JOIN
            (SELECT team FROM team_rule_members WHERE name=?1 AND run=?2 AND conversation=?3 AND name<>'' AND host=?5
             UNION SELECT team FROM team_rule_members WHERE launch=?4 AND launch<>'' AND host=?5) m ON t.id=m.team
            WHERE t.status IN ('draft','planning','running','paused') LIMIT 2")?;
        let rows = query
            .query_map(params![name, run, conversation, launch, host], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        anyhow::ensure!(
            rows.len() <= 1,
            "Session belongs to more than one active team"
        );
        let Some(payload) = rows.first() else {
            return Ok(None);
        };
        let team: Team = serde_json::from_str(payload)?;
        let member = team
            .members
            .iter()
            .find(|member| {
                member.session.as_ref().is_some_and(|session| {
                    (if session.host.is_empty() { member.computer.as_str() } else { session.host.as_str() }) == host
                        && session.name == name
                        && session.run_id == run
                        && session.conversation_id == conversation
                }) || (!launch.is_empty() && member.launch_id == launch && member.computer == host)
            })
            .map(|member| member.id.clone());
        Ok(member.map(|member| (team, member)))
    }

    /// Every team, most recently changed first, at most `limit`.
    pub(crate) fn teams(&self, include_archived: bool, limit: i64) -> anyhow::Result<Vec<Team>> {
        let mut statement = self.db.prepare(
            "SELECT payload FROM teams WHERE ?1 OR status!='archived' ORDER BY updated DESC LIMIT ?2",
        )?;
        // Task 105: the rows are read first and parsed after the statement
        // ends, so the read never holds the store while megabytes of JSON parse.
        let rows = statement
            .query_map(params![include_archived, limit], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?;
        drop(statement);
        rows.iter().map(|row| Ok(serde_json::from_str(row)?)).collect()
    }

    /// The teams that still run: Planning, Running or Paused, most recently
    /// changed first, at most `limit`. Uses the (status, updated) index, so a
    /// finished team's status never reads every team (C1).
    pub(crate) fn active_teams(&self, limit: i64) -> anyhow::Result<Vec<Team>> {
        let mut statement = self.db.prepare(
            "SELECT payload FROM teams WHERE status IN ('planning','running','paused') ORDER BY updated DESC LIMIT ?1",
        )?;
        // Task 105: read first, parse after the statement ends (see teams).
        let rows = statement.query_map(params![limit], |row| row.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
        drop(statement);
        rows.iter().map(|row| Ok(serde_json::from_str(row)?)).collect()
    }

    /// The sum of the active teams' revisions, without reading their payloads:
    /// U43's read-time default keys its cache by it, so a member added and one
    /// removed between two equal-length writes still refreshes the set.
    pub(crate) fn active_revision(&self) -> anyhow::Result<i64> {
        Ok(self.db.query_row(
            "SELECT COALESCE(SUM(revision),0) FROM teams WHERE status IN ('planning','running','paused')",
            [],
            |row| row.get(0),
        )?)
    }

    /// The stored receipt of an identical request, read only: a retry returns
    /// it before the request does any outside work (L4).
    pub(crate) fn receipt(&self, id: &str, input: &Value) -> anyhow::Result<Option<Value>> {
        ensure!(canonical_uuid(id), "A canonical request_id is required");
        let stored: Option<(String, String)> = self
            .db
            .query_row("SELECT digest,receipt FROM operations WHERE id=?1", params![id], |row| Ok((row.get(0)?, row.get(1)?)))
            .optional()?;
        match stored {
            None => Ok(None),
            Some((digest_text, receipt)) => {
                ensure!(digest_text == digest(input), "This request_id was already used for another request");
                Ok(Some(serde_json::from_str(&receipt)?))
            }
        }
    }

    /// A page of one team's feed, newest first; `before` is the previous page's
    /// cursor. Entries appended meanwhile never shift an older page.
    pub(crate) fn events(&self, team: &str, before: Option<i64>, limit: i64) -> anyhow::Result<Value> {
        let limit = limit.clamp(1, PAGE);
        let mut statement = self.db.prepare(
            "SELECT seq,at,kind,payload FROM events WHERE team=?1 AND seq<?2 ORDER BY seq DESC LIMIT ?3",
        )?;
        let rows = statement.query_map(params![team, before.unwrap_or(i64::MAX), limit + 1], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?))
        })?;
        let mut items = Vec::new();
        for row in rows {
            let (seq, at, kind, payload) = row?;
            items.push(json!({"seq":seq,"at":at,"kind":kind,"detail":serde_json::from_str::<Value>(&payload)?}));
        }
        let more = items.len() as i64 > limit;
        items.truncate(limit as usize);
        let next = if more { items.last().map(|item| item["seq"].clone()) } else { None };
        Ok(json!({"items":items,"next_cursor":next}))
    }

    /// U38: one team's journal entries a timeline reads (spans::EVENT_KINDS)
    /// between `from` and `to`, oldest first, at most `limit`, through the
    /// (team, kind, at) index; true when more were there.
    pub(crate) fn span_events(&self, team: &str, from: f64, to: f64, limit: i64) -> anyhow::Result<(Vec<super::spans::Journal>, bool)> {
        let kinds = super::spans::EVENT_KINDS.iter().map(|kind| format!("'{kind}'")).collect::<Vec<_>>().join(",");
        let mut statement = self.db.prepare(&format!(
            "SELECT at,kind,payload FROM events WHERE team=?1 AND kind IN ({kinds}) AND at>=?2 AND at<=?3 ORDER BY at,seq LIMIT ?4"
        ))?;
        let rows = statement.query_map(params![team, from, to, limit.max(1) + 1], |row| {
            Ok((row.get::<_, f64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        })?;
        let mut items = Vec::new();
        for row in rows {
            let (at, kind, payload) = row?;
            items.push(super::spans::Journal { at, kind, detail: serde_json::from_str(&payload)? });
        }
        let cut = items.len() as i64 > limit.max(1);
        items.truncate(limit.max(1) as usize);
        Ok((items, cut))
    }

    /// Task 8: every team's head for the watch stream's `team` topic: its id,
    /// name, lead, status, revision and the time of its last change, the most
    /// recently changed first, at most `limit`, and whether more exist. One
    /// indexed query: the payload is read only for the name and the lead.
    pub(crate) fn heads(&self, limit: usize) -> anyhow::Result<(Vec<Value>, bool)> {
        let mut statement = self.db.prepare(
            "SELECT id,revision,status,updated,json_extract(payload,'$.name'),json_extract(payload,'$.lead'),
                    json_extract(payload,'$.plan_mode'),json_extract(payload,'$.phase_modes')
             FROM teams ORDER BY updated DESC, id LIMIT ?1",
        )?;
        let rows = statement.query_map(params![limit.saturating_add(1) as i64], |row| {
            // Task #58: the team's plan mode and each phase's own one (a phase
            // not named plans in the team's), so a watcher sees a switch.
            let phase_modes = row
                .get::<_, Option<String>>(7)?
                .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                .filter(Value::is_object)
                .unwrap_or_else(|| json!({}));
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "revision": row.get::<_, i64>(1)?,
                "status": row.get::<_, String>(2)?,
                "updated": row.get::<_, f64>(3)?,
                "name": row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                "lead": row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                "plan_mode": row.get::<_, Option<String>>(6)?.unwrap_or_else(|| "standard".into()),
                "phase_modes": phase_modes,
            }))
        })?;
        let mut heads = rows.collect::<std::result::Result<Vec<_>, _>>()?;
        let more = heads.len() > limit;
        heads.truncate(limit);
        Ok((heads, more))
    }

    /// Creates a team. A replay of the same request returns its first receipt.
    pub(crate) fn create(&mut self, request_id: &str, input: &Value, mut team: Team, events: Vec<Event>) -> anyhow::Result<Value> {
        let transaction = begin(&self.db)?;
        if let Some(receipt) = replay(&transaction, request_id, input)? {
            return Ok(receipt);
        }
        ensure!(
            transaction.query_row("SELECT 1 FROM teams WHERE id=?1", params![team.id], |_| Ok(())).optional()?.is_none(),
            "A team with this id already exists"
        );
        let at = now();
        team.revision = 1;
        team.created = at;
        team.updated = at;
        write(&transaction, &team)?;
        append(&transaction, &team.id, at, &events)?;
        let receipt = json!({"status":"created","request_id":request_id,"team":team.id,"revision":team.revision});
        record(&transaction, request_id, input, &receipt, at)?;
        commit(transaction)?;
        Ok(receipt)
    }

    /// Applies one change to a team in one transaction: checks the expected
    /// revision, bumps it and appends the change's feed entries. A request id
    /// makes it idempotent; the dispatcher's own changes pass none.
    /// #754: the receipt stored under a request id, whatever its input.
    pub(crate) fn stored_receipt(&self, id: &str) -> anyhow::Result<Option<Value>> {
        let stored: Option<String> = self
            .db
            .query_row("SELECT receipt FROM operations WHERE id=?1", params![id], |row| row.get(0))
            .optional()?;
        Ok(match stored {
            Some(text) => Some(serde_json::from_str(&text)?),
            None => None,
        })
    }

    pub(crate) fn mutate(
        &mut self,
        request: Option<(&str, &Value)>,
        team_id: &str,
        expected_revision: Option<u64>,
        change: impl FnOnce(&mut Team, &mut Vec<Event>) -> anyhow::Result<Value>,
    ) -> anyhow::Result<Value> {
        let transaction = begin(&self.db)?;
        if let Some((id, input)) = request {
            if let Some(receipt) = replay(&transaction, id, input)? {
                return Ok(receipt);
            }
        }
        let payload: String = transaction
            .query_row("SELECT payload FROM teams WHERE id=?1", params![team_id], |row| row.get(0))
            .optional()?
            .context("Unknown team")?;
        let mut team: Team = serde_json::from_str(&payload).context("Invalid team record")?;
        if let Some(expected) = expected_revision {
            ensure!(team.revision == expected, "The team changed; refresh and try again");
        }
        let mut events = Vec::new();
        let mut receipt = change(&mut team, &mut events)?;
        let at = now();
        // Task 81: a task of the approved plan gets its baseline schedule in the
        // change that approves it or adds it.
        super::baseline::stamp(&mut team, at);
        team.revision += 1;
        team.updated = at;
        write(&transaction, &team)?;
        append(&transaction, team_id, at, &events)?;
        if receipt.is_object() {
            receipt["team"] = json!(team_id);
            receipt["revision"] = json!(team.revision);
        }
        if let Some((id, input)) = request {
            if receipt.is_object() {
                receipt["request_id"] = json!(id);
            }
            record(&transaction, id, input, &receipt, at)?;
        }
        commit(transaction)?;
        Ok(receipt)
    }
}

fn write(transaction: &rusqlite::Transaction, team: &Team) -> anyhow::Result<()> {
    let status = serde_json::to_value(team.status)?;
    transaction.execute(
        "INSERT INTO teams(id,revision,status,updated,payload) VALUES(?1,?2,?3,?4,?5)
         ON CONFLICT(id) DO UPDATE SET revision=excluded.revision,status=excluded.status,updated=excluded.updated,payload=excluded.payload",
        params![team.id, team.revision as i64, status.as_str().unwrap_or_default(), team.updated, serde_json::to_string(team)?],
    )?;
    Ok(())
}

fn append(transaction: &rusqlite::Transaction, team: &str, at: f64, events: &[Event]) -> anyhow::Result<()> {
    for event in events {
        transaction.execute(
            "INSERT INTO events(team,at,kind,payload) VALUES(?1,?2,?3,?4)",
            params![team, at, event.kind, event.payload.to_string()],
        )?;
    }
    Ok(())
}

/// The stored receipt of an earlier identical request; another request that
/// reuses the id is refused.
fn replay(transaction: &rusqlite::Transaction, id: &str, input: &Value) -> anyhow::Result<Option<Value>> {
    ensure!(canonical_uuid(id), "A canonical request_id is required");
    let stored: Option<(String, String)> = transaction
        .query_row("SELECT digest,receipt FROM operations WHERE id=?1", params![id], |row| Ok((row.get(0)?, row.get(1)?)))
        .optional()?;
    match stored {
        None => Ok(None),
        Some((digest_text, receipt)) => {
            ensure!(digest_text == digest(input), "This request_id was already used for another request");
            Ok(Some(serde_json::from_str(&receipt)?))
        }
    }
}

fn record(transaction: &rusqlite::Transaction, id: &str, input: &Value, receipt: &Value, at: f64) -> anyhow::Result<()> {
    transaction.execute(
        "INSERT INTO operations(id,digest,receipt,at) VALUES(?1,?2,?3,?4)",
        params![id, digest(input), receipt.to_string(), at],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::model::{TeamStatus};
    use super::*;

    /// Task #28: `hgs team current` and `hgs team list` from a sandboxed
    /// DeepSeek member failed with "unable to open database file": the sorted
    /// read of the active teams spilled to a temporary file, and Low integrity
    /// may not create one in %TEMP%. Here the temporary folder is gone before
    /// the read, and the sort is larger than the cache.
    #[test]
    fn a_sorted_read_needs_no_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("teams.sqlite3");
        let store = Store::open_at(&path, true).unwrap();
        for index in 0..40 {
            let team = Team { id: format!("t{index}"), name: format!("t{index}"), goal: "g".repeat(200_000),
                status: TeamStatus::Running, ..Default::default() };
            store.db.execute("INSERT INTO teams(id,revision,status,updated,payload) VALUES(?1,1,'running',?2,?3)",
                params![team.id, index as f64, serde_json::to_string(&team).unwrap()]).unwrap();
        }
        drop(store);
        let store = Store::open_at(&path, false).unwrap();
        store.db.pragma_update(None, "cache_size", -1).unwrap();
        let gone = dir.path().join("gone");
        std::fs::create_dir(&gone).unwrap();
        store.db.execute_batch(&format!("PRAGMA temp_store_directory='{}'", gone.display())).unwrap();
        std::fs::remove_dir(&gone).unwrap();
        let teams = store.active_teams(1000);
        store.db.execute_batch("PRAGMA temp_store_directory=''").unwrap();
        let teams = teams.unwrap();
        assert_eq!(teams.len(), 40);
        assert_eq!(teams[0].id, "t39");
    }

    /// A store with `count` teams, each a record of about `bytes` bytes.
    fn filled(path: &Path, journal: Journal, count: usize, bytes: usize) -> Store {
        let mut store = Store::open_journal(path, true, journal).unwrap();
        for index in 0..count {
            let team = Team { id: format!("t{index}"), name: format!("Team {index}"), lead: "lead".into(), goal: "g".repeat(bytes),
                status: TeamStatus::Running, ..Default::default() };
            store.create(&format!("11111111-1111-4111-8111-{index:012}"), &json!({"n":index}), team, vec![]).unwrap();
        }
        store
    }

    /// Task 105 (ui1, 2026-11-06): `hgs team list` waited behind a writer and
    /// failed with "database is locked" after ~15 s. A writer that holds the
    /// store exclusively (as a rollback-journal writer does while it commits or
    /// spills a large record) no longer stops a reader: it reads the last
    /// committed state at once. The rollback journal it replaces is shown
    /// failing the same read.
    #[test]
    fn a_reader_answers_while_a_writer_holds_the_store() {
        for journal in [Journal::Wal, Journal::Delete] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("teams.sqlite3");
            let writer = filled(&path, journal, 3, 50_000);
            writer.db.execute_batch("BEGIN EXCLUSIVE; UPDATE teams SET payload=json_set(payload,'$.name','Renamed') WHERE id='t0';").unwrap();
            let reader = Store::open_journal(&path, false, journal).unwrap();
            reader.db.busy_timeout(Duration::from_millis(300)).unwrap();
            let started = Instant::now();
            let read = reader.teams(false, 100).and_then(|teams| Ok((teams, reader.heads(10)?.0)));
            let took = started.elapsed();
            match journal {
                Journal::Wal => {
                    let (teams, heads) = read.unwrap();
                    assert!(took < Duration::from_secs(1), "{took:?}");
                    assert_eq!(teams.len(), 3);
                    // The uncommitted change is not seen.
                    assert!(heads.iter().all(|head| head["name"] != "Renamed"), "{heads:?}");
                }
                Journal::Delete => assert!(read.unwrap_err().to_string().contains("locked"), "the old mode waited and failed"),
            }
            writer.db.execute_batch("COMMIT").unwrap();
            drop(reader);
            let after = Store::open_journal(&path, false, journal).unwrap();
            assert!(after.heads(10).unwrap().0.iter().any(|head| head["name"] == "Renamed"));
        }
    }

    /// Task 117 (owner, 2026-11-06): Activity stuck on "Could not refresh
    /// activity: timed out". Every inspection reads the team reasons; a
    /// rollback-journal writer that holds the store kept that read waiting its
    /// whole busy timeout (10 s, measured here with 2 s), past the desktop's
    /// 5 s limit. The read now waits at most 300 ms and answers with the last
    /// good reasons (at most 2 minutes old); in WAL it reads at once.
    #[test]
    fn the_listing_reads_team_reasons_within_its_bound_or_from_the_last_good_ones() {
        use crate::state::session_state::team_reasons_at;
        use crate::state::teams::model::{Member, MemberState, SessionRef};
        for journal in [Journal::Delete, Journal::Wal] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("teams.sqlite3");
            let cache = dir.path().join("waiting-cache.json");
            let mut writer = Store::open_journal(&path, true, journal).unwrap();
            let lead = Member { id: "lead".into(), role: "Lead".into(), agent: "claude".into(), state: MemberState::Active,
                session: Some(SessionRef { name: "claude/x/lead".into(), run_id: "r0".into(), ..Default::default() }), ..Default::default() };
            let team = Team { id: "t0".into(), name: "Team".into(), lead: "lead".into(), status: TeamStatus::Running,
                members: vec![lead], ..Default::default() };
            writer.create("11111111-1111-4111-8111-000000000117", &json!({}), team, vec![]).unwrap();
            let fresh = team_reasons_at(&path, &cache, Duration::from_millis(300), 1000.0);
            assert_eq!(fresh.len(), 1, "{journal:?}");
            assert!(cache.is_file(), "the last good reasons are kept");
            writer.db.execute_batch("BEGIN EXCLUSIVE; UPDATE teams SET payload=json_set(payload,'$.name','Renamed');").unwrap();
            if journal == Journal::Delete {
                // Before task 117: the reader waits out its whole busy timeout.
                let old = Store::open_journal(&path, false, journal).unwrap();
                old.db.busy_timeout(Duration::from_secs(2)).unwrap();
                let started = Instant::now();
                assert!(old.active_teams(32).is_err());
                let waited = started.elapsed();
                assert!(waited >= Duration::from_millis(1900), "{waited:?}");
                eprintln!("task 117 {journal:?}: the unbounded reader waited {waited:?}");
            }
            let started = Instant::now();
            let bounded = team_reasons_at(&path, &cache, Duration::from_millis(300), 1000.0);
            let took = started.elapsed();
            eprintln!("task 117 {journal:?}: the bounded reader answered in {took:?}");
            assert!(took < Duration::from_millis(1500), "{journal:?} {took:?}");
            assert_eq!(bounded, fresh, "{journal:?}: the read or the last good reasons");
            if journal == Journal::Delete {
                // Older than two minutes, the kept reasons are not used.
                assert!(team_reasons_at(&path, &cache, Duration::from_millis(50), 1121.0).is_empty());
            }
            writer.db.execute_batch("COMMIT").unwrap();
        }
    }

    /// Task 105: an existing rollback-journal store becomes WAL at its first
    /// write open with every team kept; an older hgs (whose write open asks for
    /// DELETE) reads it and turns it back; a read-only open works after the last
    /// writer closed, because the log files stay.
    #[test]
    fn an_old_store_migrates_and_an_older_hgs_still_reads_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("teams.sqlite3");
        drop(filled(&path, Journal::Delete, 2, 1000));
        let old = Store::open_journal(&path, false, Journal::Delete).unwrap();
        assert_eq!(old.journal_mode().unwrap(), "delete");
        drop(old);
        let migrated = Store::open_at(&path, true).unwrap();
        assert_eq!(migrated.journal_mode().unwrap(), "wal");
        assert_eq!(migrated.teams(true, 10).unwrap().len(), 2);
        drop(migrated);
        // The log files stay for a reader that may not create them.
        let wal = files(&path)[2].clone();
        assert!(wal.exists(), "{wal:?}");
        let reader = Store::open_at(&path, false).unwrap();
        assert_eq!(reader.heads(10).unwrap().0.len(), 2);
        drop(reader);
        // An older hgs: its write open asks for DELETE and reads everything.
        let older = Store::open_journal(&path, true, Journal::Delete).unwrap();
        assert_eq!(older.journal_mode().unwrap(), "delete");
        assert_eq!(older.teams(true, 10).unwrap().len(), 2);
        drop(older);
        // This hgs turns it into WAL again.
        assert_eq!(Store::open_at(&path, true).unwrap().journal_mode().unwrap(), "wal");
    }

    /// The read `hgs team list` made before task 105: each record parsed while
    /// the statement still held the store.
    fn old_teams(store: &Store) -> anyhow::Result<Vec<Team>> {
        let mut statement = store.db.prepare("SELECT payload FROM teams WHERE status!='archived' ORDER BY updated DESC LIMIT 1000")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
    }

    /// Task 105's measurement: `ZERUS_STORE_COPY` names a copy of a live
    /// teams.sqlite3. Two writers rewrite its largest active team back to back
    /// (the dispatcher and the owner's decisions) for `ZERUS_STORE_SECONDS`
    /// (default 8) while three readers run `team list`'s read (open, teams,
    /// heads) and a status read of that team without pause. Prints each case's
    /// read and write times: before (rollback journal, the old read), the
    /// rollback journal with the new read, and WAL with the new read.
    #[test]
    #[ignore]
    fn store_contention_on_a_copy() {
        let Some(source) = std::env::var_os("ZERUS_STORE_COPY") else { return };
        let seconds: u64 = std::env::var("ZERUS_STORE_SECONDS").ok().and_then(|text| text.parse().ok()).unwrap_or(8);
        let keep: Option<i64> = std::env::var("ZERUS_WAL_KEEP").ok().and_then(|text| text.parse().ok());
        let checkpoint: Option<i64> = std::env::var("ZERUS_WAL_CHECKPOINT").ok().and_then(|text| text.parse().ok());
        let only = std::env::var("ZERUS_STORE_CASES").unwrap_or_default();
        // ZERUS_STORE_LIST: one reader runs exactly `hgs team list`'s work (the
        // store read and the list items) every 250 ms, as the desktop polls.
        let list = std::env::var_os("ZERUS_STORE_LIST").is_some();
        for (case, journal, old_read) in [("before", Journal::Delete, true), ("delete_new_read", Journal::Delete, false), ("after", Journal::Wal, false)] {
            if !only.is_empty() && !only.split(',').any(|wanted| wanted == case) {
                continue;
            }
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("teams.sqlite3");
            std::fs::copy(&source, &path).unwrap();
            let target: String = {
                let store = Store::open_journal(&path, true, journal).unwrap();
                assert_eq!(store.journal_mode().unwrap(), if journal == Journal::Wal { "wal" } else { "delete" });
                store.db.query_row("SELECT id FROM teams WHERE status IN ('running','paused','planning') ORDER BY length(payload) DESC LIMIT 1", [], |row| row.get(0)).unwrap()
            };
            let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let writers: Vec<_> = (0..2)
                .map(|n| {
                    let (path, target, stop) = (path.clone(), target.clone(), stop.clone());
                    std::thread::spawn(move || {
                        let mut store = Store::open_journal(&path, true, journal).unwrap();
                        if let Some(keep) = keep {
                            store.db.pragma_update(None, "journal_size_limit", keep).unwrap();
                        }
                        if let Some(pages) = checkpoint {
                            store.db.pragma_update(None, "wal_autocheckpoint", pages).unwrap();
                        }
                        let mut times = Vec::new();
                        let mut failed = 0;
                        while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                            let started = Instant::now();
                            let result = store.mutate(None, &target, None, |team, events| {
                                team.stop_reason = format!("writer {n} at {:?}", Instant::now());
                                events.push(Event::new("measured", json!({"writer":n})));
                                Ok(Value::Null)
                            });
                            if result.is_err() { failed += 1; }
                            times.push(started.elapsed().as_secs_f64());
                        }
                        (times, failed)
                    })
                })
                .collect();
            let readers: Vec<_> = (0..if list { 1 } else { 3 })
                .map(|_| {
                    let (path, target, stop) = (path.clone(), target.clone(), stop.clone());
                    std::thread::spawn(move || {
                        let (mut reads, mut errors) = (Vec::new(), Vec::new());
                        while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                            let started = Instant::now();
                            let read = Store::open_at(&path, false).and_then(|store| {
                                let teams = if old_read { old_teams(&store)? } else { store.teams(false, 200)? };
                                if list {
                                    super::super::commands::list_items(&teams, false);
                                } else {
                                    store.heads(200)?;
                                    store.load(&target)?;
                                }
                                Ok(())
                            });
                            reads.push(started.elapsed().as_secs_f64());
                            if let Err(error) = read {
                                errors.push(error.to_string());
                            }
                            if list {
                                std::thread::sleep(Duration::from_millis(250));
                            }
                        }
                        (reads, errors)
                    })
                })
                .collect();
            std::thread::sleep(Duration::from_secs(seconds));
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
            let (mut writes, mut write_failures) = (Vec::new(), 0);
            for writer in writers {
                let (times, failures) = writer.join().unwrap();
                writes.extend(times);
                write_failures += failures;
            }
            let (mut reads, mut errors) = (Vec::new(), Vec::new());
            for reader in readers {
                let (times, failed) = reader.join().unwrap();
                reads.extend(times);
                errors.extend(failed);
            }
            let over = reads.iter().filter(|read| **read > 1.0).count();
            let stats = |values: &mut Vec<f64>| {
                values.sort_by(f64::total_cmp);
                let at = |q: f64| values.get(((values.len() as f64 - 1.0) * q).round() as usize).copied().unwrap_or(0.0);
                json!({"n":values.len(),"p50":at(0.5),"p95":at(0.95),"max":at(1.0)})
            };
            let failures = errors.len();
            errors.dedup();
            println!("{}", json!({"list":list,"case":case,"journal":format!("{journal:?}"),"team":target,"reads":stats(&mut reads),"reads_over_1s":over,
                "read_failures":failures,"read_errors":errors.iter().take(2).collect::<Vec<_>>(),"writes":stats(&mut writes),
                "write_failures":write_failures,"wal_keep":keep,"wal_checkpoint":checkpoint}));
        }
    }

    /// Task 105's write breakdown on `ZERUS_STORE_COPY`: where one write of the
    /// largest active team spends its time (median of 10): reading and parsing
    /// its record, serializing it, the UPDATE with the member projection's
    /// trigger, the same UPDATE without the trigger, and the commit.
    #[test]
    #[ignore]
    fn store_write_breakdown_on_a_copy() {
        let Some(source) = std::env::var_os("ZERUS_STORE_COPY") else { return };
        for journal in [Journal::Delete, Journal::Wal] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("teams.sqlite3");
            std::fs::copy(&source, &path).unwrap();
            let store = Store::open_journal(&path, true, journal).unwrap();
            let target: String = store.db.query_row("SELECT id FROM teams WHERE status IN ('running','paused','planning') ORDER BY length(payload) DESC LIMIT 1", [], |row| row.get(0)).unwrap();
            let median = |mut values: Vec<f64>| { values.sort_by(f64::total_cmp); values[values.len() / 2] };
            let (mut parse, mut serialize, mut update, mut bare, mut commit_ms) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
            for round in 0..10 {
                let started = Instant::now();
                let payload: String = store.db.query_row("SELECT payload FROM teams WHERE id=?1", params![target], |row| row.get(0)).unwrap();
                let mut team: Team = serde_json::from_str(&payload).unwrap();
                parse.push(started.elapsed().as_secs_f64() * 1000.0);
                team.stop_reason = format!("round {round}");
                let started = Instant::now();
                let text = serde_json::to_string(&team).unwrap();
                serialize.push(started.elapsed().as_secs_f64() * 1000.0);
                store.db.execute_batch("BEGIN IMMEDIATE").unwrap();
                let started = Instant::now();
                store.db.execute("UPDATE teams SET payload=?2 WHERE id=?1", params![target, text]).unwrap();
                update.push(started.elapsed().as_secs_f64() * 1000.0);
                let started = Instant::now();
                store.db.execute_batch("COMMIT").unwrap();
                commit_ms.push(started.elapsed().as_secs_f64() * 1000.0);
                store.db.execute_batch("BEGIN IMMEDIATE; DROP TRIGGER team_rules_update;").unwrap();
                let started = Instant::now();
                store.db.execute("UPDATE teams SET payload=?2 WHERE id=?1", params![target, text]).unwrap();
                bare.push(started.elapsed().as_secs_f64() * 1000.0);
                store.db.execute_batch("ROLLBACK").unwrap();
            }
            println!("{}", json!({"journal":format!("{journal:?}"),"team":target,"payload_bytes":store.load(&target).unwrap().map(|team| serde_json::to_string(&team).unwrap().len()),
                "parse_ms":median(parse),"serialize_ms":median(serialize),"update_with_trigger_ms":median(update),
                "update_without_trigger_ms":median(bare),"commit_ms":median(commit_ms)}));
        }
    }

    #[test]
    fn a_sandbox_blocked_write_says_what_to_do() {
        let error = write_error("attempt to write a readonly database").to_string();
        assert!(error.contains("the sandbox blocks the team store"), "{error}");
        assert!(error.contains("hgs team report"), "{error}");
        assert_eq!(write_error("disk I/O error").to_string(), "disk I/O error");
    }

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        (dir, store)
    }
    fn team(id: &str) -> Team {
        Team { id: id.into(), name: "Checkout".into(), lead: "lead".into(), ..Default::default() }
    }
    const REQUEST: &str = "11111111-1111-4111-8111-111111111111";

    /// Task 25: another connection holds the database for `hold`: a writer
    /// (`BEGIN IMMEDIATE`, as the dispatcher's change) or a reader (a read
    /// transaction, as a status). Returns once it holds it.
    fn hold(path: &Path, write: bool, hold: Duration) -> std::thread::JoinHandle<()> {
        let path = path.to_path_buf();
        let (held, ready) = std::sync::mpsc::channel();
        let holder = std::thread::spawn(move || {
            let db = Connection::open(&path).unwrap();
            if write {
                db.execute_batch("BEGIN IMMEDIATE; UPDATE teams SET updated=updated;").unwrap();
            } else {
                db.execute_batch("BEGIN").unwrap();
                let _: i64 = db.query_row("SELECT COUNT(*) FROM teams", [], |row| row.get(0)).unwrap();
            }
            held.send(()).unwrap();
            std::thread::sleep(hold);
            db.execute_batch("COMMIT").unwrap();
        });
        ready.recv().unwrap();
        holder
    }

    /// The lead's `hgs team reassign` as the store applies it.
    fn reassign(store: &mut Store, request: &str) -> anyhow::Result<Value> {
        let input = json!({"task":4,"member":"ui2"});
        store.mutate(Some((request, &input)), "t1", None, |team, events| {
            team.name.push('+');
            events.push(Event::new("reassigned", json!({"task":4})));
            Ok(json!({"status":"reassigned","task":4}))
        })
    }

    #[test]
    fn a_team_write_waits_for_the_dispatchers_write_instead_of_failing() {
        // Task 25, team 33fbafeb: `hgs team reassign` failed with "hgs:
        // database is locked" while the dispatcher wrote, after a 2 s wait.
        let (dir, mut store) = store();
        let path = dir.path().join("teams/teams.sqlite3");
        store.create(REQUEST, &json!({}), team("t1"), vec![]).unwrap();
        let holder = hold(&path, true, Duration::from_millis(3500));
        let started = Instant::now();
        let mut lead = Store::open_at(&path, true).unwrap();
        let receipt = reassign(&mut lead, "22222222-2222-4222-8222-222222222222");
        holder.join().unwrap();
        let receipt = receipt.expect("the write waits for the other writer");
        assert!(started.elapsed() >= Duration::from_secs(3), "it waited");
        assert_eq!((receipt["status"].as_str(), receipt["revision"].as_u64()), (Some("reassigned"), Some(2)));
        // A reader holding the database at the commit is waited for too.
        let holder = hold(&path, false, Duration::from_millis(3500));
        let receipt = reassign(&mut lead, "33333333-3333-4333-8333-333333333333");
        holder.join().unwrap();
        assert_eq!(receipt.unwrap()["revision"].as_u64(), Some(3));
        assert_eq!(store.load("t1").unwrap().unwrap().name, "Checkout++");
    }

    #[test]
    fn concurrent_writes_wait_and_one_request_applies_once() {
        // Task 25: the dispatcher's write and the lead's, and the lead's
        // command run twice by hand, all while the database is held: every
        // write succeeds, the repeated request takes effect once.
        let (dir, mut store) = store();
        let path = dir.path().join("teams/teams.sqlite3");
        store.create(REQUEST, &json!({}), team("t1"), vec![]).unwrap();
        let holder = hold(&path, true, Duration::from_millis(2500));
        let writers: Vec<_> = (0..4)
            .map(|n| {
                let path = path.clone();
                std::thread::spawn(move || {
                    let mut store = Store::open_at(&path, true).unwrap();
                    if n == 0 {
                        // The dispatcher's own change: no request id.
                        store.mutate(None, "t1", None, |team, _| {
                            team.lead.push('!');
                            Ok(Value::Null)
                        })
                    } else {
                        reassign(&mut store, "44444444-4444-4444-8444-444444444444")
                    }
                })
            })
            .collect();
        holder.join().unwrap();
        let results: Vec<Value> = writers.into_iter().map(|writer| writer.join().unwrap().unwrap()).collect();
        let receipts: Vec<&Value> = results[1..].iter().collect();
        assert!(receipts.iter().all(|receipt| *receipt == receipts[0]), "{receipts:?}");
        let saved = store.load("t1").unwrap().unwrap();
        assert_eq!((saved.name.as_str(), saved.lead.as_str(), saved.revision), ("Checkout+", "lead!", 3));
        let kinds: Vec<String> = store.events("t1", None, 50).unwrap()["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|event| event["kind"].as_str().unwrap_or_default().to_owned())
            .collect();
        assert_eq!(kinds.iter().filter(|kind| *kind == "reassigned").count(), 1, "{kinds:?}");
    }

    #[test]
    fn a_database_still_busy_says_nothing_changed_and_how_to_retry() {
        let error = busy_error(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
            Some("database is locked".into()),
        ))
        .to_string();
        assert!(error.contains("nothing was changed") && error.contains("same request_id"), "{error}");
    }

    #[test]
    fn create_is_idempotent_and_a_reused_id_is_refused() {
        let (_dir, mut store) = store();
        let input = json!({"name":"Checkout"});
        let first = store.create(REQUEST, &input, team("t1"), vec![Event::new("created", json!({}))]).unwrap();
        assert_eq!(store.create(REQUEST, &input, team("t1"), vec![]).unwrap(), first);
        assert!(store.create(REQUEST, &json!({"name":"Other"}), team("t2"), vec![]).is_err());
        assert!(store.create("not-a-uuid", &input, team("t3"), vec![]).is_err());
        assert_eq!(store.load("t1").unwrap().unwrap().revision, 1);
    }

    #[test]
    fn the_active_revision_changes_with_a_team_change() {
        // #595 L2: the U43 default's cache key carries this revision, so a
        // change between two equal-length writes still refreshes the set.
        let (_dir, mut store) = store();
        store.create(REQUEST, &json!({}), team("t1"), vec![]).unwrap();
        let first = store.active_revision().unwrap();
        store
            .mutate(None, "t1", None, |team, _| {
                team.name = "Renamed".into();
                Ok(json!({}))
            })
            .unwrap();
        assert!(store.active_revision().unwrap() > first);
    }

    #[test]
    fn the_active_teams_query_is_bounded() {
        let (_dir, mut store) = store();
        for n in 0..12u32 {
            let mut t = team(&format!("t{n}"));
            t.status = if n % 4 == 0 { TeamStatus::Running } else { TeamStatus::Done };
            let request = format!("00000000-0000-4000-8000-{n:012}");
            store.create(&request, &json!({}), t, vec![]).unwrap();
        }
        // Only the three running teams, never the nine done ones, and at most
        // the requested limit (C1).
        let active = store.active_teams(1000).unwrap();
        assert_eq!(active.len(), 3);
        assert!(active.iter().all(|team| team.status == TeamStatus::Running));
        assert_eq!(store.active_teams(2).unwrap().len(), 2);
    }

    #[test]
    fn a_receipt_is_read_before_the_request_runs() {
        let (_dir, mut store) = store();
        store.create(REQUEST, &json!({"name":"Checkout"}), team("t1"), vec![]).unwrap();
        assert_eq!(store.receipt(REQUEST, &json!({"name":"Checkout"})).unwrap().unwrap()["status"], "created");
        assert!(store.receipt("22222222-2222-4222-8222-222222222222", &json!({})).unwrap().is_none());
        // The same id with another request is refused, as a mutation would.
        assert!(store.receipt(REQUEST, &json!({"name":"Other"})).is_err());
    }

    #[test]
    fn an_extend_replay_returns_the_first_receipt() {
        use super::super::model::{Member, TeamStatus};
        use super::super::ops::{self, Actor};
        use std::collections::BTreeSet;
        let (_dir, mut store) = store();
        let mut team = team("t1");
        team.status = TeamStatus::Running;
        team.members = vec![
            Member { id: "lead".into(), agent: "claude".into(), ..Default::default() },
            Member { id: "dev".into(), role: "Backend".into(), agent: "codex".into(), ..Default::default() },
        ];
        store.create(REQUEST, &json!({}), team, vec![]).unwrap();
        const EXTEND: &str = "22222222-2222-4222-8222-222222222222";
        let input = json!({"tasks":[{"id":1,"title":"Fix","assignee":"dev"}]});
        let extend = |store: &mut Store, input: &Value| {
            store.mutate(Some((EXTEND, input)), "t1", None, |team, events| {
                ops::apply(team, "extend", input, &Actor::Member("lead".into()), &BTreeSet::new(), events)
            })
        };
        let first = extend(&mut store, &input).unwrap();
        assert_eq!(first["status"], "added");
        let again = extend(&mut store, &input).unwrap();
        assert_eq!(again, first);
        assert_eq!(again["revision"], json!(2));
        assert_eq!(store.load("t1").unwrap().unwrap().phases.len(), 1);
        assert!(extend(&mut store, &json!({"tasks":[{"id":1,"title":"Other","assignee":"dev"}]})).is_err());
    }

    #[test]
    fn the_new_attempt_fields_survive_the_store_and_a_replay() {
        use super::super::model::{
            Attempt, AttemptState, Hold, Member, ReplacedReport, Report, Settings, Task, TaskStatus, TeamStatus,
        };
        use super::super::ops::{self, Actor};
        use std::collections::BTreeSet;
        let (_dir, mut store) = store();
        let mut stored = team("t1");
        stored.status = TeamStatus::Running;
        stored.settings = Settings { start_timeout_seconds: 300, ..Default::default() };
        stored.members = vec![
            Member { id: "lead".into(), agent: "claude".into(), ..Default::default() },
            Member { id: "dev".into(), role: "Backend".into(), agent: "codex".into(), ..Default::default() },
        ];
        stored.tasks = vec![
            Task {
                id: 1,
                title: "API".into(),
                assignee: "dev".into(),
                status: TaskStatus::InProgress,
                attempts: vec![
                    Attempt {
                        n: 1,
                        id: "00000000-0000-4000-8000-000000000001".into(),
                        member: "dev".into(),
                        state: AttemptState::Reported,
                        sent_at: 1.0,
                        delivered_at: Some(2.0),
                        started_at: Some(3.0),
                        start_warned: true,
                        turn_ended_at: Some(4.0),
                        turn_reply: "reply-7".into(),
                        report: Some(Report {
                            text: "Done".into(),
                            at: 6.0,
                            captured: true,
                            replaced: vec![ReplacedReport { text: "Building".into(), at: 5.5 }],
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                    Attempt {
                        n: 2,
                        id: "00000000-0000-4000-8000-000000000002".into(),
                        member: "dev".into(),
                        state: AttemptState::Delivered,
                        sent_at: 8.0,
                        delivered_at: Some(9.0),
                        started_at: Some(10.0),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
            Task {
                id: 2,
                title: "Tests".into(),
                assignee: "dev".into(),
                status: TaskStatus::Pending,
                hold: Some(Hold { by: "Lead".into(), at: 7.0, reason: "Busy with #1".into() }),
                ..Default::default()
            },
        ];
        store.create(REQUEST, &json!({}), stored.clone(), vec![]).unwrap();
        let loaded = store.load("t1").unwrap().unwrap();
        assert_eq!(loaded.tasks, stored.tasks);
        assert_eq!(loaded.settings.start_timeout_seconds, 300);
        assert_eq!(loaded.tasks[1].hold.as_ref().map(|hold| hold.reason.as_str()), Some("Busy with #1"));

        // A member's question, replayed with the same request: one receipt.
        const ASK: &str = "33333333-3333-4333-8333-333333333333";
        let input = json!({"text":"Must I keep the old JSON field?"});
        let ask = |store: &mut Store, input: &Value| {
            store.mutate(Some((ASK, input)), "t1", None, |team, events| {
                ops::apply(team, "ask", input, &Actor::Member("dev".into()), &BTreeSet::new(), events)
            })
        };
        let first = ask(&mut store, &input).unwrap();
        assert_eq!((first["status"].as_str(), first["question"].as_str()), (Some("asked"), Some("q1")));
        assert_eq!((first["task"].as_u64(), first["attempt"].as_u64()), (Some(1), Some(2)));
        assert_eq!(ask(&mut store, &input).unwrap(), first);
        let loaded = store.load("t1").unwrap().unwrap();
        let questions = &loaded.tasks[0].attempts[1].questions;
        assert_eq!(questions.len(), 1);
        assert_eq!(
            (questions[0].id.as_str(), questions[0].text.as_str(), questions[0].answered_at),
            ("q1", "Must I keep the old JSON field?", None)
        );
        // The feed has the question once, not once per replay.
        let feed = store.events("t1", None, PAGE).unwrap();
        let asked: Vec<&Value> =
            feed["items"].as_array().unwrap().iter().filter(|item| item["kind"] == "question_asked").collect();
        assert_eq!(asked.len(), 1);
        assert!(asked[0]["seq"].as_i64().is_some());
        assert_eq!(asked[0]["detail"]["question"], json!("q1"));
        assert_eq!(loaded.revision, 2);
    }

    #[test]
    fn a_mutation_checks_the_revision_and_appends_its_events() {
        let (_dir, mut store) = store();
        store.create(REQUEST, &json!({}), team("t1"), vec![]).unwrap();
        let changed = store
            .mutate(None, "t1", Some(1), |team, events| {
                team.goal = "Ship".into();
                events.push(Event::new("planned", json!({"tasks":0})));
                Ok(json!({"status":"planned"}))
            })
            .unwrap();
        assert_eq!(changed["revision"], 2);
        assert!(store.mutate(None, "t1", Some(1), |_, _| Ok(json!({}))).is_err());
        // A failed change leaves the team untouched.
        assert!(store.mutate(None, "t1", None, |team, _| { team.goal = "Lost".into(); bail!("refused") }).is_err());
        assert_eq!(store.load("t1").unwrap().unwrap().goal, "Ship");
    }

    #[test]
    fn feed_pages_stay_stable_while_entries_arrive() {
        let (_dir, mut store) = store();
        store.create(REQUEST, &json!({}), team("t1"), vec![]).unwrap();
        for n in 0..60 {
            store.mutate(None, "t1", None, |_, events| { events.push(Event::new("tick", json!({"n":n}))); Ok(json!({})) }).unwrap();
        }
        let first = store.events("t1", None, PAGE).unwrap();
        assert_eq!(first["items"].as_array().unwrap().len(), 50);
        assert_eq!(first["items"][0]["detail"]["n"], 59);
        let cursor = first["next_cursor"].as_i64().unwrap();
        store.mutate(None, "t1", None, |_, events| { events.push(Event::new("tick", json!({"n":60}))); Ok(json!({})) }).unwrap();
        let second = store.events("t1", Some(cursor), PAGE).unwrap();
        assert_eq!(second["items"].as_array().unwrap().len(), 10);
        assert_eq!(second["items"][0]["detail"]["n"], 9);
        assert!(second["next_cursor"].is_null());
    }
}
