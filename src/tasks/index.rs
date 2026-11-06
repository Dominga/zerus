//! A disposable local index of one project's task headers (zerus-cg5h.2): what
//! paged task lists and per-task lookups read instead of decoding every task
//! document. The Loro files stay authoritative and every write still goes to
//! them. The index is never replicated; a missing, damaged or foreign one is
//! rebuilt silently from the documents, and a read never fails because of it:
//! callers read the documents instead.
//!
//! It is opened and written only while the project's store lock is held, its
//! connection never outlives that store, and callers never hold two at once,
//! so nothing else has it open and a damaged one can be removed and rebuilt;
//! WAL and a busy timeout stay as a second guard. It learns what changed from the store's change
//! journal, checks every task file's stamp (length, modification time, inode)
//! at least every 30 seconds for writers that predate the journal, and checks
//! the files of a page's rows before it serves them.
use super::*;
use crate::platform;
use rusqlite::{params, Connection, ErrorCode, OpenFlags, OptionalExtension};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::Path;
use std::time::Duration;
use zerus_replica::ids;

const FILE: &str = "task-index-v1.sqlite3";
/// 2: the histories of recently read tasks as well (slice B). 3: rows and
/// history headers carry the task's execution (zerus-cg5h.4), so rows cached
/// without it are built again rather than mixed with rows that have it.
const ROWS: i64 = 3;
/// The index's version: its rows' shape and the replica's mark format, since
/// a row remembers whether its task loaded, which the mark decides. An index
/// made under another mark rule (before the seal, for one) is built again.
const SCHEMA: i64 = ROWS * 1000 + zerus_replica::MARK_FORMAT;
/// The histories kept: the most recently read tasks.
const HISTORIES: i64 = 16;
/// The one-task history cache this index replaces.
const OLD_HISTORY: &str = "task-history-v1.sqlite3";
/// How often every task file's stamp is checked, for writers that do not
/// append to the change journal.
const FULL_CHECK_MS: i64 = 30_000;
/// Tasks decoded per transaction, so no transaction is long.
const CHUNK: usize = 256;
/// A row whose document was read longer ago than this is suspect.
const MAX_AGE_MS: i64 = 10 * 60_000;
/// A file modified more recently than this is not settled: a later write in
/// the same clock tick could keep its stamp, so the row is read again.
const SETTLE_MS: i64 = 2_000;
/// Suspect rows read again at each read of the index, oldest first.
const BATCH: i64 = 256;
/// The longest damage text kept for a task that does not load.
const ERROR_BYTES: usize = 512;
/// The longest prompt text a paged row carries; `get` returns all of it.
pub(super) const TEXT_BYTES: usize = 2048;
/// The largest page a list request may ask for.
pub(super) const MAX_LIMIT: usize = 200;
/// The damaged tasks a page names; `damaged_total` counts them all.
pub(super) const DAMAGED_SHOWN: usize = 20;
/// The most the index may hold, whatever its page size: a full or larger
/// one is disposed of and built again, never grown.
const BUDGET_BYTES: i64 = 256 * 1024 * 1024;

/// The pages `BUDGET_BYTES` allows at `page_size`, and whether `pages` of
/// them exceed it.
fn budget(page_size: i64, pages: i64) -> Option<i64> {
    let allowed = BUDGET_BYTES.checked_div(page_size).filter(|n| *n > 0)?;
    (pages <= allowed).then_some(allowed)
}

/// Why the index cannot answer: a damaged index is removed and rebuilt once;
/// anything else leaves the caller to read the documents.
#[derive(Debug)]
pub(super) enum Failure {
    Damaged,
    Other,
}

impl From<rusqlite::Error> for Failure {
    fn from(error: rusqlite::Error) -> Self {
        match error.sqlite_error_code() {
            Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => Self::Other,
            // A cache: any other SQLite failure, including a missing table
            // or a full file, is answered by building it again.
            _ => Self::Damaged,
        }
    }
}

impl From<std::io::Error> for Failure {
    fn from(_: std::io::Error) -> Self {
        Self::Other
    }
}

impl From<zerus_replica::Error> for Failure {
    fn from(_: zerus_replica::Error) -> Self {
        Self::Other
    }
}

type Outcome<T> = std::result::Result<T, Failure>;

#[cfg(test)]
thread_local! {
    /// Task documents decoded and task files stamped, for the bounded-work tests.
    pub(super) static DECODED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(super) static STAMPED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// Task histories read from their documents into the index.
    pub(super) static MATERIALIZED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A file's length, modification time and inode, or `-` when it is missing,
/// and whether it is settled: modified at least `SETTLE_MS` before `now`.
fn stamp_at(path: &Path, now: i64) -> (String, bool) {
    entry_stamp(fs::metadata(path), now)
}

/// A stamp of the entry itself, not what it links to: a dangling mark
/// changes it too.
fn link_stamp_at(path: &Path, now: i64) -> (String, bool) {
    entry_stamp(fs::symlink_metadata(path), now)
}

/// A file's stamp when it is not there.
const MISSING: &str = "-";

fn entry_stamp(meta: std::io::Result<fs::Metadata>, now: i64) -> (String, bool) {
    #[cfg(test)]
    STAMPED.with(|n| n.set(n.get() + 1));
    let Ok(meta) = meta else {
        return (MISSING.into(), true);
    };
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    #[cfg(unix)]
    let inode = std::os::unix::fs::MetadataExt::ino(&meta);
    #[cfg(not(unix))]
    let inode = 0;
    let millis = (modified / 1_000_000) as i64;
    (format!("{}:{modified}:{inode}", meta.len()), millis <= now - SETTLE_MS)
}

fn stamp(path: &Path) -> String {
    stamp_at(path, 0).0
}

/// The stamps of a task's snapshot, appended updates and mark, and whether
/// all are settled: a change to the mark alone is a change of the task.
fn stamped(dir: &Path, uuid: &str, now: i64) -> (String, bool) {
    let tasks = dir.join("tasks");
    let (snapshot, a) = stamp_at(&tasks.join(format!("{uuid}.loro")), now);
    let (updates, b) = stamp_at(&tasks.join(format!("{uuid}.updates")), now);
    let (mark, c) = link_stamp_at(&tasks.join(format!("{uuid}.managed")), now);
    (format!("{snapshot}|{updates}|{mark}"), a && b && c)
}

fn files(dir: &Path, uuid: &str) -> String {
    stamped(dir, uuid, 0).0
}

/// Whether stamps from `stamped` say every file of the task is gone.
fn gone(stamps: &str) -> bool {
    stamps.split('|').all(|part| part == MISSING)
}

/// `text` cut to at most `bytes` at a character boundary.
fn cut(text: &str, bytes: usize) -> &str {
    let mut end = text.len().min(bytes);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// A damaged task's error text as the index keeps it.
pub(super) fn capped_error(error: &str) -> String {
    cut(error, ERROR_BYTES).to_owned()
}

/// A damaged task as a page names it: its error capped, and what is known
/// here of its recovery (`kept` with `kept_reason`, or `recovery`).
pub(super) fn damaged_row(damaged: &zerus_replica::Damaged) -> Value {
    let mut row = serde_json::to_value(damaged).unwrap_or_else(|_| json!({"uuid": damaged.uuid}));
    row["error"] = json!(capped_error(&damaged.error));
    row
}

/// A row's prompt text capped for a page, marked when it was cut.
pub(super) fn capped(mut row: Value) -> Value {
    if let Some(text) = row["text"].as_str() {
        if text.len() > TEXT_BYTES {
            row["text"] = json!(cut(text, TEXT_BYTES));
            row["text_truncated"] = json!(true);
        }
    }
    row
}

/// A cached row, only if it is the object the index wrote for this task:
/// its uuid, stage, short ID and rank equal the indexed ones the page was
/// selected and ordered by. Anything else is a damaged index, never served.
fn cached_row(raw: &str, listed: &Listed, stage: Stage) -> Outcome<Value> {
    let row: Value = serde_json::from_str(raw).map_err(|_| Failure::Damaged)?;
    let fits = row.is_object()
        && row["uuid"].as_str() == Some(listed.0.as_str())
        && row["stage"].as_str() == Some(stage.as_str())
        && row["id"].as_str() == Some(listed.6.as_str())
        && row["rank"].as_str() == Some(listed.1.as_str());
    if fits {
        Ok(row)
    } else {
        Err(Failure::Damaged)
    }
}

/// What the index keeps of one task.
enum Derived {
    Row {
        stage: &'static str,
        rank: String,
        id: String,
        row: String,
        sends: Vec<i64>,
    },
    Damaged(String),
}

fn derive(store: &ProjectStore, uuid: &str) -> Derived {
    #[cfg(test)]
    DECODED.with(|n| n.set(n.get() + 1));
    let damaged = |error: String| Derived::Damaged(cut(&error, ERROR_BYTES).to_owned());
    let task = match store.load(uuid) {
        Ok(task) => task,
        Err(error) => return damaged(error.to_string()),
    };
    let view = match task.view() {
        Ok(view) => view,
        Err(error) => return damaged(error.to_string()),
    };
    let (stage, rank, id) = (view.stage.as_str(), view.rank.clone(), view.id.clone());
    // What `ProjectStore::agent_starts_since` counts.
    let sends = view
        .log
        .iter()
        .filter(|e| e.kind == "sent" && matches!(e.actor, Actor::Agent(_)))
        .map(|e| e.at)
        .collect();
    match row(view, task.revision()) {
        Ok(row) => Derived::Row {
            stage,
            rank,
            id,
            row: capped(row).to_string(),
            sends,
        },
        Err(error) => damaged(error.message),
    }
}

/// Where a page is: the first rows, after or before a row's key, or the last rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Anchor {
    Start,
    After(String, String),
    Before(String, String),
    /// The last page.
    End,
}

/// One page of a stage, in the stage's order.
pub(super) struct Page {
    pub rows: Vec<Value>,
    /// The zero-based place of the first row in the stage.
    pub position: usize,
    pub next: Option<String>,
    pub previous: Option<String>,
}

fn scope(project: &str, stage: Stage) -> String {
    let hash = Sha256::digest(format!("task-list\n{project}\n{}", stage.as_str()));
    format!("{hash:x}")[..16].to_owned()
}

/// An opaque list cursor: bound to the project and stage, naming a row's
/// rank and UUID, so rows added or moved elsewhere never shift it.
pub(super) fn cursor(project: &str, stage: Stage, rank: &str, uuid: &str) -> String {
    let rank: String = rank.bytes().map(|b| format!("{b:02x}")).collect();
    format!("t1.{}.{rank}.{uuid}", scope(project, stage))
}

pub(super) fn parse_cursor(project: &str, stage: Stage, cursor: &str) -> Result<(String, String)> {
    let refused = || fail("task list cursor does not match this list");
    let parts: Vec<&str> = cursor.split('.').collect();
    let [version, found, rank, uuid] = parts[..] else {
        return Err(refused());
    };
    if version != "t1"
        || found != scope(project, stage)
        || rank.len() > 256
        || rank.len() % 2 != 0
        || !rank.bytes().all(|b| b.is_ascii_hexdigit())
        || !ids::is_uuid(uuid)
    {
        return Err(refused());
    }
    let bytes = (0..rank.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&rank[i..i + 2], 16))
        .collect::<std::result::Result<Vec<u8>, _>>()
        .map_err(|_| refused())?;
    let rank = String::from_utf8(bytes).map_err(|_| refused())?;
    Ok((rank, uuid.to_owned()))
}

/// A listed row: UUID, rank, file stamps, row, when it was read, whether its
/// files were settled then, and its short ID.
type Listed = (String, String, String, String, i64, bool, String);

pub(super) struct Index<'a> {
    store: &'a ProjectStore,
    db: Connection,
}

/// Removes the index and its SQLite files. Only while the project lock is
/// held and this process has no connection to it open.
fn dispose(dir: &Path) {
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let _ = fs::remove_file(dir.join(format!("{FILE}{suffix}")));
    }
}

fn connect(dir: &Path) -> Outcome<Connection> {
    // The project folder is private; check its resolved form too, so an
    // ancestor changed between the two never selects another folder.
    let canonical = platform::canonical(dir)?;
    for folder in [dir, canonical.as_path()] {
        if !platform::verify_private(folder, platform::Private::Directory)? {
            return Err(Failure::Other);
        }
    }
    for suffix in ["", "-wal", "-shm", "-journal"] {
        match platform::verify_private(&dir.join(format!("{FILE}{suffix}")), platform::Private::File) {
            Ok(true) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            _ => return Err(Failure::Damaged),
        }
    }
    let path = canonical.join(FILE);
    // Never close a raw descriptor of a live database (zerus-iiee): create
    // it owner-only, check it by path and bind the connection to that file.
    #[cfg(unix)]
    let identity = {
        crate::sqlite_file::create(&path)?;
        crate::sqlite_file::private(&path).map_err(|_| Failure::Damaged)?
    };
    // Windows locks belong to the handle that took them.
    #[cfg(not(unix))]
    {
        let file = platform::no_follow(platform::private_open(
            fs::OpenOptions::new().create(true).truncate(false).read(true).write(true),
        ))
        .open(&path)?;
        if !platform::verify_private_file(&file)? {
            return Err(Failure::Damaged);
        }
    }
    let db = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    #[cfg(unix)]
    crate::sqlite_file::bind_private(&db, &path, identity).map_err(|_| Failure::Damaged)?;
    db.busy_timeout(Duration::from_secs(2))?;
    let mode: String = db.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(Failure::Damaged);
    }
    db.execute_batch("PRAGMA synchronous=NORMAL;")?;
    let page_size: i64 = db.query_row("PRAGMA page_size", [], |r| r.get(0))?;
    let pages: i64 = db.query_row("PRAGMA page_count", [], |r| r.get(0))?;
    let allowed = budget(page_size, pages).ok_or(Failure::Damaged)?;
    db.execute_batch(&format!("PRAGMA max_page_count={allowed};"))?;
    let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let objects: i64 = db.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE name IN
            ('meta', 'tasks', 'sends', 'tasks_order', 'tasks_short', 'tasks_checked', 'tasks_untrusted',
             'sends_at', 'sends_uuid', 'histories', 'entries', 'entries_place', 'entries_id')",
        [],
        |r| r.get(0),
    )?;
    if version != SCHEMA || objects != 13 {
        // The one-task history cache before this index; nothing has it open
        // while the project lock is held.
        for suffix in ["", "-journal", "-wal", "-shm"] {
            let _ = fs::remove_file(dir.join(format!("{OLD_HISTORY}{suffix}")));
        }
        // New, or not this schema: start again from the documents.
        db.execute_batch(&format!(
            "BEGIN IMMEDIATE;
            DROP TABLE IF EXISTS meta; DROP TABLE IF EXISTS tasks; DROP TABLE IF EXISTS sends;
            DROP TABLE IF EXISTS histories; DROP TABLE IF EXISTS entries;
            CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE tasks(uuid TEXT NOT NULL UNIQUE, files TEXT NOT NULL, stage TEXT, rank TEXT,
                short_id TEXT, row TEXT, error TEXT, checked INTEGER NOT NULL, trusted INTEGER NOT NULL);
            CREATE INDEX tasks_order ON tasks(stage, rank, uuid);
            CREATE INDEX tasks_short ON tasks(short_id);
            CREATE INDEX tasks_checked ON tasks(checked);
            CREATE INDEX tasks_untrusted ON tasks(checked) WHERE trusted = 0;
            CREATE TABLE sends(uuid TEXT NOT NULL, at INTEGER NOT NULL);
            CREATE INDEX sends_at ON sends(at);
            CREATE INDEX sends_uuid ON sends(uuid);
            CREATE TABLE histories(uuid TEXT PRIMARY KEY, files TEXT NOT NULL, header TEXT NOT NULL,
                summary TEXT NOT NULL, total INTEGER NOT NULL, used INTEGER NOT NULL);
            CREATE TABLE entries(uuid TEXT NOT NULL, id TEXT NOT NULL, at INTEGER NOT NULL,
                ordinal INTEGER NOT NULL, payload TEXT NOT NULL, summary TEXT NOT NULL);
            CREATE UNIQUE INDEX entries_place ON entries(uuid, at, ordinal);
            CREATE UNIQUE INDEX entries_id ON entries(uuid, id);
            PRAGMA user_version={SCHEMA};
            COMMIT;"
        ))?;
    }
    Ok(db)
}

impl<'a> Index<'a> {
    /// The project's index, brought up to date with its documents; `None`
    /// when it cannot be used now, and the caller reads the documents.
    pub(super) fn open(store: &'a ProjectStore) -> Option<Self> {
        let dir = store.dir();
        for attempt in 0..2 {
            let opened = connect(dir).and_then(|db| {
                let mut index = Self { store, db };
                index.sync()?;
                Ok(index)
            });
            match opened {
                Ok(index) => return Some(index),
                // The connection is closed by now; nothing else has it open.
                Err(Failure::Damaged) if attempt == 0 => dispose(dir),
                Err(_) => return None,
            }
        }
        None
    }

    /// Closes the index; a damaged one is removed, to be built again.
    pub(super) fn discard(self, failure: &Failure) {
        let dir = self.store.dir().to_owned();
        drop(self.db);
        if matches!(failure, Failure::Damaged) {
            dispose(&dir);
        }
    }

    fn meta(&self, key: &str) -> Outcome<Option<String>> {
        Ok(self
            .db
            .query_row("SELECT value FROM meta WHERE key=?1", [key], |r| r.get(0))
            .optional()?)
    }

    /// The index's place in the journal, the task folder's stamp and the
    /// time of the last full check.
    fn sync(&mut self) -> Outcome<()> {
        let dir = self.store.dir().to_owned();
        let generation = self.meta("generation")?.unwrap_or_default();
        let offset = self.meta("offset")?.and_then(|v| v.parse().ok()).unwrap_or(0);
        let checked: i64 = self.meta("checked")?.and_then(|v| v.parse().ok()).unwrap_or(0);
        let built = self.meta("revision")?.is_some();
        let folder = stamp(&dir.join("tasks"));
        let changes = self.store.changes_since(&generation, offset)?;
        let now = now();
        let quiet = changes.documents.as_ref().is_some_and(BTreeSet::is_empty);
        let periodic = now < checked || now - checked >= FULL_CHECK_MS;
        let full = !built
            || changes.documents.is_none()
            // The folder changed while the journal says nothing did: a
            // writer that does not keep the journal.
            || (quiet && self.meta("folder")?.as_deref() != Some(folder.as_str()))
            || periodic;
        // A clock set back makes earlier reads look recent: any read dated
        // after now is suspect, and so is everything before a suspect mark
        // dated after now.
        let past = now - MAX_AGE_MS - 1;
        self.db.execute("UPDATE tasks SET checked=?1 WHERE checked > ?2", params![past, now])?;
        if self.meta("suspect")?.and_then(|v| v.parse::<i64>().ok()).is_some_and(|mark| mark > now) {
            self.db.execute("INSERT OR REPLACE INTO meta VALUES('suspect', ?1)", [now.to_string()])?;
        }
        let moved = changes.generation != generation || changes.offset != offset;
        if built && changes.documents.is_none() && moved {
            // The journal cannot say what changed (another generation, left
            // behind, damaged or missing; it is started again): every row
            // read before now is suspect, whatever its stamps.
            self.db.execute("INSERT OR REPLACE INTO meta VALUES('suspect', ?1)", [now.to_string()])?;
        }
        if full {
            let mut known: HashMap<String, String> = HashMap::new();
            {
                let mut query = self.db.prepare("SELECT uuid, files FROM tasks")?;
                let rows = query.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
                for row in rows {
                    let (uuid, files) = row?;
                    known.insert(uuid, files);
                }
            }
            let mut uuids: BTreeSet<String> = self.store.uuids()?.into_iter().collect();
            uuids.extend(known.keys().cloned());
            let uuids: Vec<String> = uuids.into_iter().collect();
            self.refresh(&uuids, Some(&known), changes.documents.as_ref())?;
        } else if let Some(documents) = &changes.documents {
            let uuids: Vec<String> = documents.iter().filter(|d| ids::is_uuid(d)).cloned().collect();
            self.refresh(&uuids, None, Some(documents))?;
        }
        // Suspect rows are read again, a bounded batch at a time, those read
        // or tried longest ago first: a row read again goes to the back, so
        // every suspect row is reached within ceil(suspect / BATCH) reads.
        let floor = self.floor(now)?;
        let suspect: BTreeSet<String> = {
            let mut oldest: Vec<(i64, String)> = Vec::new();
            for sql in [
                "SELECT checked, uuid FROM tasks WHERE checked < ?1 ORDER BY checked LIMIT ?2",
                "SELECT checked, uuid FROM tasks WHERE trusted = 0 AND ?1 = ?1 ORDER BY checked LIMIT ?2",
            ] {
                let mut query = self.db.prepare_cached(sql)?;
                let rows = query.query_map(params![floor, BATCH], |r| Ok((r.get(0)?, r.get(1)?)))?;
                for row in rows {
                    oldest.push(row?);
                }
            }
            oldest.sort();
            oldest.dedup_by(|a, b| a.1 == b.1);
            oldest.into_iter().take(BATCH as usize).map(|(_, uuid)| uuid).collect()
        };
        if !suspect.is_empty() {
            let list: Vec<String> = suspect.iter().cloned().collect();
            self.refresh(&list, None, Some(&suspect))?;
        }
        let tx = self.db.transaction()?;
        {
            let mut set = tx.prepare("INSERT OR REPLACE INTO meta VALUES(?1, ?2)")?;
            set.execute(params!["generation", changes.generation])?;
            set.execute(params!["offset", changes.offset.to_string()])?;
            set.execute(params!["folder", folder])?;
            if full {
                set.execute(params!["checked", now.to_string()])?;
            }
            tx.execute("INSERT OR IGNORE INTO meta VALUES('revision', ?1)", [ids::new_uuid()])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Rows checked before this are suspect: read before the journal last
    /// became unknown, or longer than `MAX_AGE_MS` ago. So are rows read from
    /// unsettled files (`trusted = 0`) and rows dated after now.
    fn floor(&self, now: i64) -> Outcome<i64> {
        let suspect = self.meta("suspect")?.and_then(|v| v.parse().ok()).unwrap_or(0);
        Ok(suspect.max(now - MAX_AGE_MS))
    }

    /// Indexes again each of `uuids` whose files changed (`known` holds the
    /// stamps already read, for a full check), and every one the journal
    /// names (`noted`) whatever its stamps: Windows has no inode and coarse
    /// file times, so a rewrite of the same length can keep its stamp. Short
    /// transactions; any change gives the index a new revision.
    fn refresh(
        &mut self,
        uuids: &[String],
        known: Option<&HashMap<String, String>>,
        noted: Option<&BTreeSet<String>>,
    ) -> Outcome<()> {
        let dir = self.store.dir().to_owned();
        let read_at = now();
        for chunk in uuids.chunks(CHUNK) {
            let mut work = Vec::new();
            for uuid in chunk {
                let (now, settled) = stamped(&dir, uuid, read_at);
                // When it was read orders the background queue; whether its
                // files were settled decides if a page may trust it.
                let (checked, trusted) = (read_at, settled);
                let stored: Option<(String, Option<String>, Option<String>)> = self
                    .db
                    .query_row("SELECT files, row, error FROM tasks WHERE uuid=?1", [uuid], |r| {
                        Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                    })
                    .optional()?;
                let stamps = match known {
                    Some(known) => known.get(uuid).cloned(),
                    None => stored.as_ref().map(|s| s.0.clone()),
                };
                let unchanged = stamps.as_deref() == Some(now.as_str()) || (stamps.is_none() && gone(&now));
                if unchanged && !noted.is_some_and(|noted| noted.contains(uuid)) {
                    continue;
                }
                let derived = (!gone(&now)).then(|| derive(self.store, uuid));
                // Read again to the same row: only its stamps and time change.
                let same = match (&derived, &stored) {
                    (Some(Derived::Row { row, .. }), Some((_, Some(old), None))) => row == old,
                    (Some(Derived::Damaged(error)), Some((_, None, Some(old)))) => error == old,
                    _ => false,
                };
                work.push((uuid, now, derived, same, checked, trusted));
            }
            if work.is_empty() {
                continue;
            }
            let tx = self.db.transaction()?;
            let mut changed = false;
            for (uuid, files, derived, same, checked, trusted) in work {
                if same {
                    tx.execute(
                        "UPDATE tasks SET files=?1, checked=?2, trusted=?3 WHERE uuid=?4",
                        params![files, checked, trusted, uuid],
                    )?;
                    continue;
                }
                changed = true;
                tx.execute("DELETE FROM tasks WHERE uuid=?1", [uuid])?;
                tx.execute("DELETE FROM sends WHERE uuid=?1", [uuid])?;
                // A changed task's history is read again when next asked for.
                tx.execute("DELETE FROM histories WHERE uuid=?1", [uuid])?;
                tx.execute("DELETE FROM entries WHERE uuid=?1", [uuid])?;
                match derived {
                    None => (),
                    Some(Derived::Damaged(error)) => {
                        tx.execute(
                            "INSERT INTO tasks(uuid, files, error, checked, trusted) VALUES(?1, ?2, ?3, ?4, ?5)",
                            params![uuid, files, error, checked, trusted],
                        )?;
                    }
                    Some(Derived::Row { stage, rank, id, row, sends }) => {
                        tx.execute(
                            "INSERT INTO tasks(uuid, files, stage, rank, short_id, row, checked, trusted)
                             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                            params![uuid, files, stage, rank, id, row, checked, trusted],
                        )?;
                        for at in sends {
                            tx.execute("INSERT INTO sends VALUES(?1, ?2)", params![uuid, at])?;
                        }
                    }
                }
            }
            if changed {
                tx.execute("INSERT OR REPLACE INTO meta VALUES('revision', ?1)", [ids::new_uuid()])?;
            }
            tx.commit()?;
        }
        Ok(())
    }

    /// Changes whenever any indexed row changes, including after a rebuild.
    pub(super) fn revision(&self) -> Outcome<String> {
        self.meta("revision")?.ok_or(Failure::Damaged)
    }

    /// One page of `stage`. A page past either end is the nearest whole
    /// page, so every row stays reachable both ways; only an empty stage has
    /// none. Each row's files are checked first; a changed task is indexed
    /// again and the page read again.
    pub(super) fn page(&mut self, stage: Stage, limit: usize, from: &Anchor) -> Outcome<Page> {
        let mut from = from.clone();
        // Rows read from their documents in this call are current even when
        // their files are not yet settled.
        let mut read = BTreeSet::new();
        for _ in 0..4 {
            let rows = self.rows(stage, limit, &from)?;
            if rows.is_empty() {
                match from {
                    Anchor::After(..) => {
                        from = Anchor::End;
                        continue;
                    }
                    Anchor::Before(..) => {
                        from = Anchor::Start;
                        continue;
                    }
                    Anchor::Start | Anchor::End => (),
                }
            }
            // A row is served only when its files are as read and it is not
            // suspect; any other is read again from its document first.
            let dir = self.store.dir().to_owned();
            let at = now();
            let floor = self.floor(at)?;
            let stale: BTreeSet<String> = rows
                .iter()
                .filter(|(uuid, _, stored, _, checked, trusted, _)| {
                    ((!*trusted || *checked < floor || *checked > at) && !read.contains(uuid))
                        || files(&dir, uuid) != *stored
                })
                .map(|(uuid, ..)| uuid.clone())
                .collect();
            if !stale.is_empty() {
                let list: Vec<String> = stale.iter().cloned().collect();
                self.refresh(&list, None, Some(&stale))?;
                read.extend(stale);
                continue;
            }
            return self.finish(stage, rows);
        }
        Err(Failure::Other)
    }

    fn rows(&self, stage: Stage, limit: usize, from: &Anchor) -> Outcome<Vec<Listed>> {
        const ASCENDING: &str = "SELECT uuid, rank, files, row, checked, trusted, short_id FROM tasks WHERE stage=?1 AND (rank, uuid) > (?2, ?3) ORDER BY rank, uuid LIMIT ?4";
        const DESCENDING: &str = "SELECT uuid, rank, files, row, checked, trusted, short_id FROM tasks WHERE stage=?1 AND (rank, uuid) < (?2, ?3) ORDER BY rank DESC, uuid DESC LIMIT ?4";
        const LAST: &str = "SELECT uuid, rank, files, row, checked, trusted, short_id FROM tasks WHERE stage=?1 AND ?2 = ?3 ORDER BY rank DESC, uuid DESC LIMIT ?4";
        let (sql, rank, uuid) = match from {
            Anchor::Start => (ASCENDING, "", ""),
            Anchor::After(rank, uuid) => (ASCENDING, rank.as_str(), uuid.as_str()),
            Anchor::Before(rank, uuid) => (DESCENDING, rank.as_str(), uuid.as_str()),
            Anchor::End => (LAST, "", ""),
        };
        let mut query = self.db.prepare_cached(sql)?;
        let mut rows = query
            .query_map(params![stage.as_str(), rank, uuid, limit as i64], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?))
            })?
            .collect::<rusqlite::Result<Vec<Listed>>>()?;
        if matches!(from, Anchor::Before(..) | Anchor::End) {
            rows.reverse();
        }
        Ok(rows)
    }

    fn exists(&self, stage: Stage, rank: &str, uuid: &str, after: bool) -> Outcome<bool> {
        let sql = if after {
            "SELECT EXISTS(SELECT 1 FROM tasks WHERE stage=?1 AND (rank, uuid) > (?2, ?3))"
        } else {
            "SELECT EXISTS(SELECT 1 FROM tasks WHERE stage=?1 AND (rank, uuid) < (?2, ?3))"
        };
        Ok(self.db.query_row(sql, params![stage.as_str(), rank, uuid], |r| r.get(0))?)
    }

    fn before(&self, stage: Stage, rank: &str, uuid: &str) -> Outcome<usize> {
        let count: i64 = self.db.query_row(
            "SELECT count(*) FROM tasks WHERE stage=?1 AND (rank, uuid) < (?2, ?3)",
            params![stage.as_str(), rank, uuid],
            |r| r.get(0),
        )?;
        Ok(count as usize)
    }

    fn finish(&self, stage: Stage, rows: Vec<Listed>) -> Outcome<Page> {
        let project = self.store.project_id();
        let (mut next, mut previous, mut position) = (None, None, 0);
        if let (Some(first), Some(last)) = (rows.first(), rows.last()) {
            if self.exists(stage, &last.1, &last.0, true)? {
                next = Some(cursor(project, stage, &last.1, &last.0));
            }
            if self.exists(stage, &first.1, &first.0, false)? {
                previous = Some(cursor(project, stage, &first.1, &first.0));
            }
            position = self.before(stage, &first.1, &first.0)?;
        }
        let rows = rows
            .into_iter()
            .map(|listed| cached_row(&listed.3, &listed, stage))
            .collect::<Outcome<Vec<Value>>>()?;
        Ok(Page { rows, position, next, previous })
    }

    /// The number of loadable tasks in each stage, by stage name.
    pub(super) fn counts(&self) -> Outcome<BTreeMap<String, usize>> {
        let mut query = self
            .db
            .prepare_cached("SELECT stage, count(*) FROM tasks WHERE stage IS NOT NULL GROUP BY stage")?;
        let rows = query.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        let mut counts = BTreeMap::new();
        for row in rows {
            let (stage, count) = row?;
            counts.insert(stage, count as usize);
        }
        Ok(counts)
    }

    /// The first damaged tasks by UUID, and how many there are. What is known
    /// of a shown one's recovery is read from its files now.
    pub(super) fn damaged(&self) -> Outcome<(Vec<Value>, usize)> {
        let mut query = self
            .db
            .prepare_cached("SELECT uuid, error FROM tasks WHERE error IS NOT NULL ORDER BY uuid LIMIT ?1")?;
        let shown = query
            .query_map([DAMAGED_SHOWN as i64], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .map(|(uuid, error)| damaged_row(&self.store.damaged(&uuid, error)))
            .collect();
        let total: i64 = self
            .db
            .query_row("SELECT count(*) FROM tasks WHERE error IS NOT NULL", [], |r| r.get(0))?;
        Ok((shown, total as usize))
    }

    /// The UUIDs of the loadable tasks with short ID `id`, and whether any
    /// task does not load (its short ID cannot be read).
    pub(super) fn short_id(&self, id: &str) -> Outcome<(Vec<String>, bool)> {
        let mut query = self.db.prepare_cached("SELECT uuid FROM tasks WHERE short_id=?1")?;
        let uuids = query
            .query_map([id], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?;
        let damaged: bool = self
            .db
            .query_row("SELECT EXISTS(SELECT 1 FROM tasks WHERE error IS NOT NULL)", [], |r| r.get(0))?;
        Ok((uuids, damaged))
    }

    /// The place of task `uuid` in its stage, counting from 1.
    pub(super) fn place(&self, uuid: &str) -> Outcome<Option<usize>> {
        let found: Option<(String, String)> = self
            .db
            .query_row(
                "SELECT stage, rank FROM tasks WHERE uuid=?1 AND stage IS NOT NULL",
                [uuid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((stage, rank)) = found else {
            return Ok(None);
        };
        let count: i64 = self.db.query_row(
            "SELECT count(*) FROM tasks WHERE stage=?1 AND (rank, uuid) < (?2, ?3)",
            params![stage, rank, uuid],
            |r| r.get(0),
        )?;
        Ok(Some(count as usize + 1))
    }

    /// One chronological page of task `uuid`'s history, `None` when the
    /// entry `before` names is gone. The histories of the most recently read
    /// tasks are kept; another is read from its document once and replaces
    /// the least recently read. `summary` returns attachment metadata
    /// instead of the files and names each entry.
    pub(super) fn history(
        &mut self,
        uuid: &str,
        limit: usize,
        before: Option<&str>,
        summary: bool,
        prefix: &str,
    ) -> Outcome<Option<Value>> {
        // The task's row is held to the rules a page holds its rows to: one
        // that is untrusted, read before the floor or dated after now is read
        // again first. A row read again with other content drops the cached
        // history, so the stamps check below never trusts a stale history.
        let at = now();
        let floor = self.floor(at)?;
        let row: Option<(i64, bool)> = self
            .db
            .query_row("SELECT checked, trusted FROM tasks WHERE uuid=?1", [uuid], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        if !row.is_some_and(|(checked, trusted)| trusted && checked >= floor && checked <= at) {
            let only = BTreeSet::from([uuid.to_owned()]);
            self.refresh(&[uuid.to_owned()], None, Some(&only))?;
        }
        let dir = self.store.dir().to_owned();
        let now = files(&dir, uuid);
        let cached: Option<String> = self
            .db
            .query_row("SELECT files FROM histories WHERE uuid=?1", [uuid], |r| r.get(0))
            .optional()?;
        let used: i64 = self
            .db
            .query_row("SELECT coalesce(max(used), 0) + 1 FROM histories", [], |r| r.get(0))?;
        if cached.as_deref() == Some(now.as_str()) {
            self.db.execute("UPDATE histories SET used=?1 WHERE uuid=?2", params![used, uuid])?;
        } else {
            #[cfg(test)]
            MATERIALIZED.with(|n| n.set(n.get() + 1));
            // A task that cannot be read is answered from its document,
            // which gives the reason.
            let task = self.store.load(uuid).map_err(|_| Failure::Other)?;
            let read = super::history::materialize(&task).map_err(|_| Failure::Other)?;
            let tx = self.db.transaction()?;
            tx.execute("DELETE FROM histories WHERE uuid=?1", [uuid])?;
            tx.execute("DELETE FROM entries WHERE uuid=?1", [uuid])?;
            tx.execute(
                "INSERT INTO histories VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
                params![uuid, now, read.header.to_string(), read.summary.to_string(), read.entries.len() as i64, used],
            )?;
            {
                let mut insert = tx.prepare("INSERT INTO entries VALUES(?1, ?2, ?3, ?4, ?5, ?6)")?;
                for (ordinal, entry) in read.entries.iter().enumerate() {
                    insert.execute(params![
                        uuid,
                        entry.id,
                        entry.at,
                        ordinal as i64,
                        entry.payload.to_string(),
                        entry.summary.to_string()
                    ])?;
                }
            }
            tx.execute(
                "DELETE FROM entries WHERE uuid IN (SELECT uuid FROM histories ORDER BY used DESC LIMIT -1 OFFSET ?1)",
                [HISTORIES],
            )?;
            tx.execute(
                "DELETE FROM histories WHERE uuid IN (SELECT uuid FROM histories ORDER BY used DESC LIMIT -1 OFFSET ?1)",
                [HISTORIES],
            )?;
            tx.commit()?;
        }
        let column = if summary { "summary" } else { "header" };
        let (header, total): (String, i64) = self.db.query_row(
            &format!("SELECT {column}, total FROM histories WHERE uuid=?1"),
            [uuid],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let boundary = match before {
            Some(id) => match self
                .db
                .query_row(
                    "SELECT at, ordinal FROM entries WHERE uuid=?1 AND id=?2",
                    params![uuid, id],
                    |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
                )
                .optional()?
            {
                Some(boundary) => boundary,
                None => return Ok(None),
            },
            None => (i64::MAX, i64::MAX),
        };
        let column = if summary { "summary" } else { "payload" };
        let mut query = self.db.prepare_cached(&format!(
            "SELECT id, {column}, at FROM entries WHERE uuid=?1 AND (at, ordinal) < (?2, ?3)
             ORDER BY at DESC, ordinal DESC LIMIT ?4"
        ))?;
        let mut rows = query
            .query_map(params![uuid, boundary.0, boundary.1, (limit + 1) as i64], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let more = rows.len() > limit;
        rows.truncate(limit);
        rows.reverse();
        // Cached JSON is served only as the index wrote it: objects, the
        // header for this task (its uuid and indexed short ID) and each entry
        // at its indexed time. Anything else is a damaged index.
        let object = |raw: &str| {
            serde_json::from_str::<Value>(raw)
                .ok()
                .filter(Value::is_object)
                .ok_or(Failure::Damaged)
        };
        let mut entries = Vec::with_capacity(rows.len());
        for (id, raw, at) in &rows {
            let mut entry = object(raw)?;
            if entry["at"].as_i64() != Some(*at) {
                return Err(Failure::Damaged);
            }
            if summary {
                entry["id"] = json!(format!("{prefix}{id}"));
            }
            entries.push(entry);
        }
        let header = object(&header)?;
        let short_id: Option<String> = self
            .db
            .query_row("SELECT short_id FROM tasks WHERE uuid=?1", [uuid], |r| r.get(0))
            .optional()?
            .flatten();
        if header["uuid"].as_str() != Some(uuid) || short_id.is_none() || header["id"].as_str() != short_id.as_deref() {
            return Err(Failure::Damaged);
        }
        let next = more.then(|| rows.first().map(|(id, ..)| format!("{prefix}{id}"))).flatten();
        Ok(Some(super::history::finish(header, entries, total as usize, next)))
    }
}

#[cfg(test)]
mod tests;
