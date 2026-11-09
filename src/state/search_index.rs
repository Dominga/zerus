//! On-demand, bounded public-history ingestion. No worker, service, discovery
//! scan or provider RPC is started here. Exact targets come from the caller's
//! current catalog; every target is re-read from its authoritative private file.
use super::*;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
use zerus_content_search::{
    Index, Message, Part, Role, Target, DEFAULT_FILE_BYTES, DOCUMENT_BYTES,
};
mod catalog;
mod metadata;
mod query;
mod wire;
pub(crate) use catalog::{DiscoveryBudget, DiscoveryProgress};
pub(super) use query::read as indexed;

const ABI: &str = "content-search-index-v1";
const INPUT_BYTES: usize = 64 << 10;
const RECORD_BYTES: usize = 1 << 20;
const READ_BYTES: usize = 2 << 20;
const PART_BYTES: usize = 16 << 10;
const PART_OVERLAP: usize = 255;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Identity {
    name: String,
    run_id: String,
    conversation_id: String,
    #[serde(default)]
    archive_id: String,
}
impl Identity {
    fn target(&self) -> Target {
        Target {
            name: self.name.clone(),
            run_id: self.run_id.clone(),
            conversation_id: self.conversation_id.clone(),
            archive_id: self.archive_id.clone(),
            agent: "unknown".into(),
            cwd: String::new(),
            archived_at: 0.0,
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    abi: String,
    targets: Vec<Identity>,
}

pub(crate) struct Budget {
    pub files: usize,
    pub directory_entries: usize,
    pub records: usize,
    pub bytes: usize,
    pub deadline: Instant,
}
impl Budget {
    pub fn interactive() -> Self {
        Self {
            files: 32,
            directory_entries: 2000,
            records: 128,
            bytes: 4 << 20,
            deadline: Instant::now() + Duration::from_millis(250),
        }
    }
}
#[derive(Clone, Copy, PartialEq)]
enum Flow {
    Next,
    Stop,
}
#[derive(Default, Serialize)]
pub(crate) struct Progress {
    files: usize,
    directory_entries: usize,
    locator_probes: usize,
    records: usize,
    bytes: usize,
    targets: usize,
    stopped: bool,
    errors: Vec<String>,
}
impl Progress {
    fn can_read(&self, budget: &Budget, cancel: &AtomicBool) -> bool {
        !cancel.load(Ordering::Relaxed)
            && Instant::now() < budget.deadline
            && self.records < budget.records
            && self.bytes < budget.bytes
    }
    fn available(&self, budget: &Budget, cancel: &AtomicBool) -> bool {
        self.can_read(budget, cancel) && self.files < budget.files
    }
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct Cursor {
    stamp: Option<Stamp>,
    wire: Option<wire::Locator>,
    offset: u64,
    seq: i64,
    skipping: bool,
    incomplete: bool,
    chunk_version: u8,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Stamp {
    path: PathBuf,
    device: u64,
    inode: u64,
    head_bytes: usize,
    head: String,
    tail: String,
    len: u64,
    modified: i64,
    nanos: i64,
}

pub(crate) struct Store {
    directory: PathBuf,
    index: Index,
    wire_roots: std::collections::BTreeMap<PathBuf, std::sync::Arc<wire::Snapshot>>,
    /// An owned source-step transaction is open (see `begin_step`).
    step: bool,
}
impl Store {
    pub fn open(directory: &Path) -> Result<Self> {
        // A sandbox installer can leave an inherited read ACE on the profile
        // (Codex adds its group to ~/.local), so a folder made by inheritance
        // is not private. Create it owner-only, and make an existing owned one
        // private in place, as the other private stores do (#569); the check
        // below stays the stricter verify_private, never a looser one.
        crate::platform::owner_only_dir(directory).map_err(|e| e.to_string())?;
        if !crate::private_stores::ensure(directory, crate::platform::Private::Directory)
            .map_err(|e| e.to_string())?
        {
            return Err("Search index directory must be private and owned by this user".into());
        }
        // SQLite NOFOLLOW rejects symlinks in every path component, including
        // macOS's ordinary /var -> /private/var parent. Resolve the verified
        // directory only; keep refusing a symlink at the database itself.
        let canonical = crate::platform::canonical(directory).map_err(|e| e.to_string())?;
        // The canonical folder is the one used from here: check it as well, so
        // an ancestor changed after the first check never selects another one.
        if !crate::platform::verify_private(&canonical, crate::platform::Private::Directory)
            .map_err(|e| e.to_string())?
        {
            return Err("Search index directory must be private and owned by this user".into());
        }
        let path = canonical.join("index.sqlite3");
        // A copied/restored database or auxiliary file must not become a new
        // public copy of message text, even if its parent is currently private.
        for name in [
            "index.sqlite3",
            "index.sqlite3-journal",
            "index.sqlite3-wal",
            "index.sqlite3-shm",
        ] {
            match crate::platform::verify_private(
                &directory.join(name),
                crate::platform::Private::File,
            ) {
                Ok(true) => (),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                _ => {
                    return Err(
                        "Search index files must be private single-link regular files".into(),
                    )
                }
            }
        }
        // Never close a raw descriptor of the live index on Unix: it would drop
        // the SQLite locks of every connection this process has open on it
        // (zerus-iiee). Create it owner-only if missing, check it by path, and
        // bind the connection to the checked file.
        #[cfg(unix)]
        let identity = {
            crate::sqlite_file::create(&path).map_err(|e| e.to_string())?;
            crate::sqlite_file::private(&path)
                .map_err(|_| "Search index file is not private".to_string())?
        };
        #[cfg(not(unix))]
        {
            let file = crate::platform::no_follow(crate::platform::private_open(
                fs::OpenOptions::new()
                    .create(true)
                    .truncate(false)
                    .read(true)
                    .write(true),
            ))
            .open(&path)
            .map_err(|e| e.to_string())?;
            if !crate::platform::verify_private_file(&file).map_err(|e| e.to_string())? {
                return Err("Search index file is not private".into());
            }
        }
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW;
        let db = Connection::open_with_flags(&path, flags).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        crate::sqlite_file::bind_private(&db, &path, identity).map_err(|e| e.to_string())?;
        let index = Index::new(db, DEFAULT_FILE_BYTES).map_err(|e| e.to_string())?;
        let parent = crate::platform::canonical(directory.parent().ok_or("Invalid index path")?)
            .map_err(|e| e.to_string())?;
        let configuration = format!("{:x}", Sha256::digest(parent.to_string_lossy().as_bytes()));
        index
            .connection()
            .execute(
                "INSERT OR IGNORE INTO meta VALUES('configuration',?)",
                [&configuration],
            )
            .map_err(|e| e.to_string())?;
        let saved: String = index
            .connection()
            .query_row(
                "SELECT value FROM meta WHERE key='configuration'",
                [],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if saved != configuration {
            return Err(
                "Search index belongs to another state directory; dispose and rebuild it".into(),
            );
        }
        index.connection().execute_batch("CREATE TABLE IF NOT EXISTS input_cursors(source TEXT PRIMARY KEY,cursor TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS purge_queue(source TEXT PRIMARY KEY)").map_err(|e|e.to_string())?;
        Ok(Self {
            directory: directory.to_owned(),
            index,
            wire_roots: Default::default(),
            step: false,
        })
    }

    /// Pure bounded-work entry point for a later G1 owner. No work is scheduled
    /// after return; cancelled jobs leave their committed input cursors intact.
    pub fn step(
        &mut self,
        state: &Path,
        targets: &[Identity],
        budget: &Budget,
        cancel: &AtomicBool,
    ) -> Result<Progress> {
        self.step_with_reader(
            state,
            targets,
            budget,
            cancel,
            &mut metadata::Reader::default(),
        )
    }

    fn step_with_reader(
        &mut self,
        state: &Path,
        targets: &[Identity],
        budget: &Budget,
        cancel: &AtomicBool,
        reader: &mut metadata::Reader,
    ) -> Result<Progress> {
        if targets.len() > 16
            || budget.files > 256
            || budget.directory_entries > 2000
            || budget.records > 512
            || budget.bytes > 8 << 20
            || budget.deadline > Instant::now() + Duration::from_secs(2)
        {
            return Err("Invalid search-index step budget".into());
        }
        if crate::platform::canonical(state).map_err(|e| e.to_string())?
            != crate::platform::canonical(self.directory.parent().ok_or("Invalid index path")?)
                .map_err(|e| e.to_string())?
        {
            return Err("Search-index step configuration changed".into());
        }
        self.wire_roots.clear();
        let mut progress = Progress::default();
        if !progress.available(budget, cancel) {
            progress.stopped = true;
            return Ok(progress);
        }
        self.cleanup()?;
        self.sweep(state, budget, &mut progress, cancel, reader)?;
        for identity in targets {
            let target = identity.target();
            let key = target.key().map_err(|e| e.to_string())?;
            if !progress.available(budget, cancel) {
                progress.stopped = true;
                break;
            }
            let owned = self.begin_step()?;
            let flow =
                self.step_identity(state, identity, &key, budget, cancel, reader, &mut progress);
            let ended = self.end_step(owned);
            let flow = flow?;
            ended?;
            if flow == Flow::Stop {
                break;
            }
        }
        progress.stopped |= !progress.available(budget, cancel);
        Ok(progress)
    }

    /// One identity's source step; `Stop` ends this bounded call.
    #[allow(clippy::too_many_arguments)]
    fn step_identity(
        &mut self,
        state: &Path,
        identity: &Identity,
        key: &str,
        budget: &Budget,
        cancel: &AtomicBool,
        reader: &mut metadata::Reader,
        progress: &mut Progress,
    ) -> Result<Flow> {
        let key = key.to_owned();
        let record = match binding(state, identity, progress, budget, reader, 0) {
            Ok(metadata::Record::Ready(record)) => record,
            Ok(metadata::Record::Deferred | metadata::Record::OverBudget) => {
                progress.stopped = true;
                return Ok(Flow::Stop);
            }
            Err(_) => {
                self.retire(&key)?;
                progress
                    .errors
                    .push("Source missing, replaced or unreadable; index entry retired".into());
                return Ok(Flow::Next);
            }
        };
        let target = record_target(&record);
        if let Err(error) = self.index.register(&target) {
            progress.errors.push(error.to_string());
            return Ok(Flow::Next);
        }
        let encoded: Option<String> = self
            .index
            .connection()
            .query_row(
                "SELECT cursor FROM input_cursors WHERE source=?",
                [&key],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let mut cursor: Cursor = match encoded {
            Some(encoded) => serde_json::from_str(&encoded)
                .map_err(|_| "Invalid source cursor; dispose and rebuild the search index")?,
            None => Cursor::default(),
        };
        // Old capped writers advanced past oversized public messages. Rewind
        // only after transcript identity verification (or when none was saved).
        if cursor.chunk_version == 0 && !cursor.incomplete {
            cursor.chunk_version = 1;
        }
        let before = cursor.clone();
        let mut unavailable = false;
        let result = self.transcript(&record, &key, &mut cursor, budget, progress, cancel);
        let complete = match result {
            Ok(complete) => complete,
            Err(error) => {
                cursor = before;
                unavailable = true;
                progress.errors.push(error.chars().take(160).collect());
                false
            }
        };
        let active: bool = self
            .index
            .connection()
            .query_row("SELECT active FROM sources WHERE id=?", [&key], |r| {
                r.get(0)
            })
            .map_err(|e| e.to_string())?;
        if !active {
            progress.targets += 1;
            return Ok(Flow::Next);
        }
        if cursor.chunk_version == 0 && cursor.stamp.is_none() {
            cursor.seq = 0;
            cursor.incomplete = false;
            cursor.chunk_version = 1;
        }
        // A missing or unsupported provider never hides its recorded journal.
        let journal_complete =
            match self.journal(&record, &key, &mut cursor, budget, progress, cancel) {
                Ok(complete) => complete,
                Err(_) => {
                    unavailable = true;
                    false
                }
            };
        let mut snapshots_complete = true;
        for (field, kind, role, time) in [
            ("prompt", "UserPromptSubmit", "user", "turn_started"),
            ("last_message", "AgentMessage", "assistant", "last_event_at"),
        ] {
            if !progress.can_read(budget, cancel) {
                snapshots_complete = false;
                break;
            }
            let text = if field == "prompt" {
                journal::user_prompt(&record).as_str().unwrap_or("")
            } else {
                journal::last_message(&record)
            };
            if !text.is_empty() {
                let event =
                    json!({"type":kind,"source":"snapshot","at":record[time],"detail":text});
                match self.put(&key, event, role, budget, progress, cancel) {
                    Ok(Flow::Next) => (),
                    Ok(Flow::Stop) => snapshots_complete = false,
                    Err(_) => {
                        unavailable = true;
                        snapshots_complete = false;
                    }
                }
            }
        }
        // Never record a cursor past rows that SQLite rolled back.
        self.step_open()?;
        self.index
            .connection()
            .execute(
                "INSERT OR REPLACE INTO input_cursors VALUES(?,?)",
                params![
                    key,
                    serde_json::to_string(&cursor).map_err(|e| e.to_string())?
                ],
            )
            .map_err(|e| e.to_string())?;
        let _ = self.index.source_progress(
            &key,
            complete && journal_complete && snapshots_complete && !cursor.incomplete,
            unavailable || cursor.incomplete,
        );
        progress.targets += 1;
        Ok(Flow::Next)
    }

    /// A source step commits once: its index rows, input cursor, progress and
    /// queue completion become durable together. Each journaled commit flushes
    /// the disk several times, and a small session used to take about fourteen,
    /// more than a slow Windows disk fits into an interactive budget. A step
    /// begun inside an open one joins it. IMMEDIATE takes the write lock first,
    /// so a concurrent writer waits for the busy timeout instead of deadlocking.
    fn begin_step(&mut self) -> Result<bool> {
        if self.step {
            self.step_open()?;
            return Ok(false);
        }
        self.index
            .connection()
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| e.to_string())?;
        self.step = true;
        Ok(true)
    }

    /// SQLite can cancel a whole transaction after SQLITE_FULL, IOERR or NOMEM;
    /// later statements would then commit one by one. A step must not record
    /// its cursor or completion after that: the next step redoes it instead.
    fn step_open(&self) -> Result<()> {
        if self.step && self.index.connection().is_autocommit() {
            return Err("Search-index step was rolled back; a later step retries it".into());
        }
        Ok(())
    }

    /// Commit the step even when it returned an error, as the former
    /// per-statement commits kept everything written before the error. A
    /// failed commit rolls the whole step back.
    fn end_step(&mut self, owned: bool) -> Result<()> {
        if !owned {
            return Ok(());
        }
        let cancelled = self.step_open();
        self.step = false;
        cancelled?;
        let connection = self.index.connection();
        if let Err(error) = connection.execute_batch("COMMIT") {
            let _ = connection.execute_batch("ROLLBACK");
            return Err(error.to_string());
        }
        Ok(())
    }

    fn put(
        &mut self,
        key: &str,
        event: Value,
        role: &str,
        budget: &Budget,
        progress: &mut Progress,
        cancel: &AtomicBool,
    ) -> Result<Flow> {
        let text = string(&event, "detail");
        if text.is_empty() {
            progress.records += 1;
            return Ok(Flow::Next);
        }
        let id = if string(&event, "message_id").is_empty() {
            format!("indexed-{:x}", Sha256::digest(format!("{key}:{event}")))
        } else {
            string(&event, "message_id").to_owned()
        };
        // Only these typed public fields reach the persistent copy. IDs and
        // fingerprints describe the original, never an individual window.
        let mut message = Message {
            id,
            at: event["at"]
                .as_f64()
                .filter(|n| n.is_finite() && *n >= 0.0)
                .unwrap_or(0.0),
            role: match role {
                "user" => Role::User,
                "assistant" => Role::Assistant,
                "tool" => Role::Tool,
                _ => Role::Event,
            },
            kind: string(&event, "type").to_owned(),
            source: string(&event, "source").to_owned(),
            seq: event["seq"].as_u64().unwrap_or(0),
            text: text.to_owned(),
        };
        let result = if text.len() <= DOCUMENT_BYTES {
            self.index.ingest(key, &[message]).map(|inserted| {
                // Snapshots are revisited every slice. An unchanged prompt
                // must not starve the following reply under a one-record cap.
                if inserted > 0 || event["source"] != "snapshot" {
                    progress.records += 1;
                }
                Flow::Next
            })
        } else {
            let fingerprint = format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&(1, key, &message)).map_err(|e| e.to_string())?)
            );
            let mut offset = self
                .index
                .part_offset(key, &message.id, &fingerprint, text.len())
                .map_err(|e| e.to_string())?
                .unwrap_or(0);
            if offset == text.len() && event["source"] != "snapshot" {
                progress.records += 1;
            }
            let mut result = Ok(Flow::Next);
            while offset < text.len() {
                if cancel.load(Ordering::Relaxed)
                    || Instant::now() >= budget.deadline
                    || progress.records >= budget.records
                {
                    result = Ok(Flow::Stop);
                    break;
                }
                let (end, next) = part_window(text, offset)?;
                message.text = text[offset..end].to_owned();
                if let Err(error) = self.index.ingest_part(
                    key,
                    &message,
                    Part {
                        fingerprint: &fingerprint,
                        start: offset,
                        next,
                        total: text.len(),
                    },
                ) {
                    result = Err(error);
                    break;
                }
                progress.records += 1;
                offset = next;
            }
            result
        };
        match result {
            Ok(flow) => Ok(flow),
            Err(error) => {
                // A failed part keeps its source/input cursor. Eviction is
                // separately bounded and invalidates affected part receipts.
                if matches!(error.downcast_ref::<rusqlite::Error>(),Some(rusqlite::Error::SqliteFailure(code,_)) if code.code==rusqlite::ErrorCode::DiskFull)
                {
                    let _ = self.index.evict();
                }
                Err(error.to_string())
            }
        }
    }

    fn transcript(
        &mut self,
        record: &Value,
        key: &str,
        cursor: &mut Cursor,
        budget: &Budget,
        progress: &mut Progress,
        cancel: &AtomicBool,
    ) -> Result<bool> {
        if !progress.available(budget, cancel) {
            return Ok(false);
        }
        if record["backend"] == "dsh" {
            return Err(
                "Native history needs legacy search until its indexed adapter is available".into(),
            );
        }
        let path = if record["agent"] == "kimi" {
            let Some((path, locator)) = self.locate_wire(record, budget, progress, cancel)? else {
                return Ok(false);
            };
            cursor.wire = Some(locator);
            path
        } else {
            PathBuf::from(string(record, "transcript"))
        };
        if !path.is_absolute()
            || (record["agent"] != "kimi"
                && !path.file_name().is_some_and(|name| {
                    name.to_string_lossy()
                        .contains(string(record, "conversation_id"))
                }))
        {
            return Err("Exact conversation transcript unavailable".into());
        }
        progress.files += 1;
        let mut file = crate::platform::no_follow(fs::OpenOptions::new().read(true))
            .open(&path)
            .map_err(|_| "Transcript unavailable")?;
        let (identity, metadata) =
            crate::platform::FileIdentity::with_metadata(&file).map_err(|e| e.to_string())?;
        if !metadata.is_file() {
            return Err("Transcript is not a regular file".into());
        }
        let head_bytes = cursor
            .stamp
            .as_ref()
            .map(|s| s.head_bytes)
            .unwrap_or(metadata.len().min(256) as usize);
        let head = sample(&mut file, 0, head_bytes, progress, budget)?;
        let tail = sample(
            &mut file,
            cursor.offset.saturating_sub(256),
            cursor.offset.min(256) as usize,
            progress,
            budget,
        )?;
        if cursor.stamp.as_ref().is_some_and(|s| {
            s.path != path
                || s.device != identity.device
                || s.inode != identity.inode
                || cursor.offset > metadata.len()
                || s.head != head
                || s.tail != tail
                || (s.len == metadata.len()
                    && (s.modified != identity.modified || s.nanos != identity.modified_nanos))
        }) {
            self.retire(key)?;
            return Err(
                "Transcript changed identity; index source retired, use legacy search".into(),
            );
        }
        if cursor.chunk_version == 0 {
            // The old cursor's file/head/tail were checked above before any
            // rewind. Replaced input never acquires a fresh identity here.
            cursor.offset = 0;
            cursor.seq = 0;
            cursor.skipping = false;
            cursor.incomplete = false;
            cursor.chunk_version = 1;
        }
        file.seek(SeekFrom::Start(cursor.offset))
            .map_err(|e| e.to_string())?;
        // Reserve the final fingerprint read before committing an input cursor.
        let count = READ_BYTES.min(budget.bytes.saturating_sub(progress.bytes + 256));
        if count == 0 {
            return Ok(false);
        }
        let mut bytes = Vec::new();
        (&mut file)
            .take(count as u64)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        progress.bytes += bytes.len();
        for line in bytes.split_inclusive(|c| *c == b'\n') {
            if cancel.load(Ordering::Relaxed)
                || Instant::now() >= budget.deadline
                || progress.records >= budget.records
            {
                break;
            }
            if !line.ends_with(b"\n") {
                if line.len() == READ_BYTES {
                    cursor.skipping = true;
                    cursor.incomplete = true;
                    cursor.offset += line.len() as u64;
                }
                break;
            }
            if cursor.skipping {
                cursor.skipping = false;
                cursor.offset += line.len() as u64;
                continue;
            }
            if line.iter().all(u8::is_ascii_whitespace) {
                progress.records += 1;
                cursor.offset += line.len() as u64;
                continue;
            }
            match serde_json::from_slice::<Value>(line) {
                Ok(value) => {
                    if (record["agent"] == "claude"
                        && value["sessionId"].is_string()
                        && value["sessionId"] != record["conversation_id"])
                        || (record["agent"] == "codex"
                            && value["type"] == "session_meta"
                            && value["payload"]["id"] != record["conversation_id"])
                    {
                        self.retire(key)?;
                        return Err("Transcript conversation changed; source retired".into());
                    }
                    if let Some((event, role)) =
                        search::provider_event(string(record, "agent"), &value)
                    {
                        if self.put(key, event, role, budget, progress, cancel)? == Flow::Stop {
                            break;
                        }
                    } else {
                        progress.records += 1;
                    }
                }
                Err(_) => {
                    cursor.incomplete = true;
                    progress.records += 1;
                }
            }
            cursor.offset += line.len() as u64;
        }
        let tail = sample(
            &mut file,
            cursor.offset.saturating_sub(256),
            cursor.offset.min(256) as usize,
            progress,
            budget,
        )?;
        cursor.stamp = Some(Stamp {
            path,
            device: identity.device,
            inode: identity.inode,
            head_bytes,
            head,
            tail,
            len: metadata.len(),
            modified: identity.modified,
            nanos: identity.modified_nanos,
        });
        Ok(cursor.offset == metadata.len())
    }

    fn journal(
        &mut self,
        record: &Value,
        key: &str,
        cursor: &mut Cursor,
        budget: &Budget,
        progress: &mut Progress,
        cancel: &AtomicBool,
    ) -> Result<bool> {
        // NOFOLLOW below: resolve the state directory, never the database name.
        let state = self
            .directory
            .parent()
            .ok_or("Invalid search state directory")?;
        let path = match crate::platform::canonical(state) {
            Ok(state) => state.join("events.sqlite3"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(true),
            Err(e) => return Err(e.to_string()),
        };
        if !path.exists() {
            return Ok(true);
        }
        if !progress.available(budget, cancel) {
            return Ok(false);
        }
        progress.files += 1;
        let db = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .map_err(|e| e.to_string())?;
        db.busy_timeout(Duration::from_millis(25))
            .map_err(|e| e.to_string())?;
        let deadline = budget.deadline;
        db.progress_handler(1000, Some(move || Instant::now() >= deadline));
        let name = if string(record, "journal_name").is_empty() {
            string(record, "name")
        } else {
            string(record, "journal_name")
        };
        let mut statement=db.prepare(&format!("SELECT seq,substr(CAST(payload AS BLOB),1,?6),length(CAST(payload AS BLOB)) FROM events WHERE {} AND seq>?4 ORDER BY seq LIMIT ?5",journal::TIMELINE)).map_err(|e|e.to_string())?;
        let mut rows = statement
            .query(params![
                name,
                record["conversation_id"].as_str(),
                clear_context::earlier_conversations(record).to_string(),
                cursor.seq,
                budget.records - progress.records + 1,
                RECORD_BYTES.min(budget.bytes - progress.bytes)
            ])
            .map_err(|e| e.to_string())?;
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            if !progress.can_read(budget, cancel) {
                return Ok(false);
            }
            let bytes: Vec<u8> = row.get(1).map_err(|e| e.to_string())?;
            let size: usize = row.get(2).map_err(|e| e.to_string())?;
            if bytes.len() > budget.bytes - progress.bytes {
                return Ok(false);
            }
            progress.bytes += bytes.len();
            let seq: i64 = row.get(0).map_err(|e| e.to_string())?;
            if size > bytes.len() {
                if size <= RECORD_BYTES {
                    return Ok(false);
                }
                cursor.incomplete = true;
                progress.records += 1;
                cursor.seq = seq;
                continue;
            }
            let Ok(mut event) = serde_json::from_slice::<Value>(&bytes) else {
                cursor.incomplete = true;
                cursor.seq = seq;
                progress.records += 1;
                continue;
            };
            if event.is_object() {
                journal::mark_system_prompt(record, &mut event, "");
                let role = search::journal_role(&event);
                event["seq"] = json!(seq);
                event["source"] = json!("journal");
                if self.put(key, event, role, budget, progress, cancel)? == Flow::Stop {
                    return Ok(false);
                }
            } else {
                cursor.incomplete = true;
                progress.records += 1;
            }
            cursor.seq = seq;
        }
        Ok(true)
    }

    pub fn status(&self) -> Result<Value> {
        let coverage = self.index.status().map_err(|e| e.to_string())?;
        Ok(
            json!({"abi":ABI,"coverage":coverage,"disposable":true,"worker":"on_demand","catalog":self.catalog_status()?}),
        )
    }

    fn retire(&self, key: &str) -> Result<()> {
        self.index.retire(key).map_err(|e| e.to_string())?;
        self.mark_retired_references(key)?;
        self.index
            .connection()
            .execute("DELETE FROM input_cursors WHERE source=?", [key])
            .map_err(|e| e.to_string())?;
        self.index
            .connection()
            .execute("INSERT OR IGNORE INTO purge_queue VALUES(?)", [key])
            .map_err(|e| e.to_string())?;
        self.cleanup()
    }

    fn cleanup(&self) -> Result<()> {
        let key: Option<String> = self
            .index
            .connection()
            .query_row(
                "SELECT source FROM purge_queue ORDER BY source LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if let Some(key) = key {
            // A missing source has no payload; otherwise the index enforces a
            // <=32-document secure purge. Keep the queue until its last row.
            let active: Option<bool> = self
                .index
                .connection()
                .query_row("SELECT active FROM sources WHERE id=?", [&key], |r| {
                    r.get(0)
                })
                .optional()
                .map_err(|e| e.to_string())?;
            if active == Some(false) {
                self.index.purge(&key).map_err(|e| e.to_string())?;
            }
            let remains: bool = self
                .index
                .connection()
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM messages WHERE source=? LIMIT 1)",
                    [&key],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            let parts: bool = self
                .index
                .connection()
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='message_parts')",
                    [],
                    |row| row.get(0),
                )
                .map_err(|e| e.to_string())?;
            let remains = remains
                || (parts
                    && self
                        .index
                        .connection()
                        .query_row(
                            "SELECT EXISTS(SELECT 1 FROM message_parts WHERE source=?1 LIMIT 1) OR EXISTS(SELECT 1 FROM message_groups WHERE source=?1 LIMIT 1)",
                            [&key],
                            |row| row.get::<_, bool>(0),
                        )
                        .map_err(|e| e.to_string())?);
            if !remains {
                self.index
                    .connection()
                    .execute("DELETE FROM purge_queue WHERE source=?", [&key])
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    fn sweep(
        &self,
        state: &Path,
        budget: &Budget,
        progress: &mut Progress,
        cancel: &AtomicBool,
        reader: &mut metadata::Reader,
    ) -> Result<()> {
        let last: String = self
            .index
            .connection()
            .query_row(
                "SELECT COALESCE((SELECT value FROM meta WHERE key='source_sweep'),'')",
                [],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        let entries: Vec<(String, String)> = self
            .index
            .connection()
            .prepare("SELECT id,target FROM sources WHERE active=1 AND id>? ORDER BY id LIMIT 4")
            .map_err(|e| e.to_string())?
            .query_map([&last], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|e| e.to_string())?
            .collect::<rusqlite::Result<_>>()
            .map_err(|e| e.to_string())?;
        let mut next = if entries.is_empty() {
            String::new()
        } else {
            last
        };
        for (key, target) in entries {
            if !progress.available(budget, cancel) {
                break;
            }
            let target: Target = serde_json::from_str(&target)
                .map_err(|_| "Invalid indexed target; dispose and rebuild the index")?;
            let identity = Identity {
                name: target.name,
                run_id: target.run_id,
                conversation_id: target.conversation_id,
                archive_id: target.archive_id,
            };
            match binding(state, &identity, progress, budget, reader, 0) {
                Ok(metadata::Record::Ready(_)) => (),
                // Keep this source active and revisit it on a later sweep;
                // an expensive record must not fence every following source.
                Ok(metadata::Record::Deferred | metadata::Record::OverBudget) => (),
                Err(_) => self.retire(&key)?,
            }
            next = key;
        }
        self.index
            .connection()
            .execute(
                "INSERT OR REPLACE INTO meta VALUES('source_sweep',?)",
                [next],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

fn part_window(text: &str, start: usize) -> Result<(usize, usize)> {
    if start >= text.len() || !text.is_char_boundary(start) {
        return Err("Invalid public-message continuation".into());
    }
    let mut end = (start + PART_BYTES).min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let next = if end == text.len() {
        end
    } else {
        text[start..end]
            .char_indices()
            .rev()
            .nth(PART_OVERLAP - 1)
            .map(|(offset, _)| start + offset)
            .ok_or("Public-message window is too small")?
    };
    Ok((end, next))
}

fn record_target(record: &Value) -> Target {
    Target {
        name: string(record, "name").into(),
        run_id: string(record, "run_id").into(),
        conversation_id: string(record, "conversation_id").into(),
        archive_id: string(record, "archive_id").into(),
        agent: string(record, "agent").into(),
        cwd: string(record, "cwd").into(),
        archived_at: record["archived_at"]
            .as_f64()
            .filter(|n| n.is_finite() && *n >= 0.0)
            .unwrap_or(0.0),
    }
}
/// The record of an ID-layout session (zerus-ucck P3a) whose label has no
/// first holder: the file in `sessions/` with this identity's label and run.
/// The scan reads at most 2,000 files.
pub(super) fn id_layout_path(state: &Path, identity: &Identity, label: &Path) -> Option<PathBuf> {
    // The label index names the candidates when its manifest proves it; a
    // label it proves has no session in the ID layout is never scanned, and
    // only a label path with no record falls back to a bounded scan.
    let sessions = state.join("sessions");
    let candidates: Vec<PathBuf> = match super::labels::listed_in(state, &identity.name) {
        Some(ids) => ids.iter().map(|id| sessions.join(format!("{id}.json"))).collect(),
        None if !label.exists() => std::fs::read_dir(&sessions)
            .ok()?
            .take(2_000)
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_stem()
                    .and_then(|stem| stem.to_str())
                    .is_some_and(super::session_id::canonical)
            })
            .collect(),
        None => Vec::new(),
    };
    // Each read is bounded, never follows a final link and never blocks on a
    // FIFO (#1437 review); the whole search stops at its deadline.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    candidates.into_iter().find(|path| {
        std::time::Instant::now() < deadline
            && super::labels::read_file(path, super::storage::ID_RECORD_BYTES)
                .ok()
                .flatten()
                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                .is_some_and(|record| {
                    string(&record, "name") == identity.name
                        && string(&record, "run_id") == identity.run_id
                        && record["layout"] == "id"
                })
    })
}

fn binding(
    state: &Path,
    identity: &Identity,
    progress: &mut Progress,
    budget: &Budget,
    reader: &mut metadata::Reader,
    reserve: usize,
) -> Result<metadata::Record> {
    let path = if identity.archive_id.is_empty() {
        let basename = format!("{:x}.json", Sha256::digest(identity.name.as_bytes()));
        let label = if state.join("name_routes").join(&basename).exists() {
            state.join("bindings").join(basename)
        } else {
            state.join(basename)
        };
        // A session in the ID layout (zerus-ucck P3a): found by its run, also
        // when the label's own path holds its first holder.
        id_layout_path(state, identity, &label).unwrap_or(label)
    } else {
        uuid::Uuid::parse_str(&identity.archive_id).map_err(|_| "Invalid archive identity")?;
        state
            .join("archives")
            .join(format!("{}.json", identity.archive_id))
    };
    let mut record = match reader.read(&path, progress, budget, reserve)? {
        metadata::Record::Ready(record) => record,
        other => return Ok(other),
    };
    if !identity.archive_id.is_empty() {
        rename::normalize_archive(&mut record)?;
    }
    if record["version"] != 1
        || record["rename_shadow"] == true
        || record["name"] != identity.name
        || record["run_id"] != identity.run_id
        || record["conversation_id"] != identity.conversation_id
        || string(&record, "archive_id") != identity.archive_id
    {
        return Err("Binding identity changed".into());
    }
    Ok(metadata::Record::Ready(record))
}
fn sample(
    file: &mut fs::File,
    start: u64,
    len: usize,
    progress: &mut Progress,
    budget: &Budget,
) -> Result<String> {
    if len > budget.bytes.saturating_sub(progress.bytes) {
        return Err("Search-index byte budget reached".into());
    }
    file.seek(SeekFrom::Start(start))
        .map_err(|e| e.to_string())?;
    let mut bytes = vec![0; len];
    file.read_exact(&mut bytes).map_err(|e| e.to_string())?;
    progress.bytes += len;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

pub(super) fn dispatch(args: &[String]) -> Option<Result<i32>> {
    let mode = match args {
        [mode]
            if [
                "--index-status",
                "--index-step",
                "--index-discover",
                "--index-catchup",
            ]
            .contains(&mode.as_str()) =>
        {
            mode
        }
        _ => return None,
    };
    Some((|| {
        let directory = root().join("content-search-v1");
        if mode == "--index-status" && !directory.exists() {
            println!(
                "{}",
                json!({"abi":ABI,"coverage":{"state":"cold","file_bytes":0},"worker":"on_demand","disposable":true})
            );
            return Ok(0);
        }
        let mut store = Store::open(&directory)?;
        if mode == "--index-discover" || mode == "--index-catchup" {
            lower_own_priority();
            let progress: DiscoveryProgress = store.discover(
                &root(),
                &DiscoveryBudget::interactive(),
                &AtomicBool::new(false),
            )?;
            let ingestion = if mode == "--index-catchup" {
                Some(store.consume(&root(), &Budget::interactive(), &AtomicBool::new(false))?)
            } else {
                None
            };
            println!(
                "{}",
                json!({"abi":ABI,"discovery":progress,"catchup":ingestion,"status":store.status()?})
            );
            return Ok(0);
        }
        if mode == "--index-step" {
            let mut bytes = Vec::new();
            std::io::stdin()
                .take((INPUT_BYTES + 1) as u64)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() > INPUT_BYTES {
                return Err("Search-index request exceeds its input budget".into());
            }
            let request: Request =
                serde_json::from_slice(crate::state::without_bom(&bytes)).map_err(|_| "Invalid search-index request")?;
            if request.abi != ABI {
                return Err("Unsupported search-index ABI".into());
            }
            // Only this short CLI process is reprioritized. An eventual G1
            // owner sets priority on its owned worker, not the service actor.
            lower_own_priority();
            let progress = store.step(
                &root(),
                &request.targets,
                &Budget::interactive(),
                &AtomicBool::new(false),
            )?;
            println!(
                "{}",
                json!({"abi":ABI,"progress":progress,"status":store.status()?})
            );
        } else {
            println!("{}", store.status()?);
        }
        Ok(0)
    })())
}

fn lower_own_priority() {
    #[cfg(unix)]
    unsafe {
        libc::setpriority(libc::PRIO_PROCESS, 0, 19);
    }
    #[cfg(target_os = "linux")]
    unsafe {
        libc::syscall(libc::SYS_ioprio_set, 1, 0, 3 << 13);
    }
}

#[cfg(test)]
mod tests;

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    #[test]
    fn indexed_search_opens_and_discovers_a_windows_fixture() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path().join("state");
        fs::create_dir(&state).unwrap();
        fs::write(state.join(format!("{:064x}.json", 1)), "{}").unwrap();
        let store = Store::open(&state.join("content-search-v1")).unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut progress = DiscoveryProgress::default();
        for _ in 0..128 {
            let budget = DiscoveryBudget::interactive();
            progress = store
                .discover(&state, &budget, &AtomicBool::new(false))
                .unwrap();
            assert!(Instant::now() < deadline, "fixture discovery timed out");
            assert!(progress.entries <= budget.entries);
            if progress.complete {
                break;
            }
        }
        assert!(progress.complete);
        assert_eq!(progress.staged, 1);
        assert_eq!(store.status().unwrap()["catalog"]["queued_paths"], 1);
    }
}

#[cfg(all(test, unix))]
mod lock_regressions {
    use super::*;
    use crate::sqlite_file::foreign_lock_refused;

    /// zerus-iiee: opening the search index again keeps the SQLite locks of a
    /// connection already open, also after a restart.
    #[test]
    fn opening_the_index_again_keeps_a_live_connection_s_locks() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("state").join("content-search-v1");
        let path = directory.join("index.sqlite3");
        // The second round is a restart: every earlier connection is gone.
        for round in 0..2 {
            let live = Store::open(&directory).unwrap();
            // A writer's transaction: opening again during it is refused as
            // busy and must never drop the writer's lock. (A reader's shared
            // lock is not used here: the refused writer's own downgrade takes
            // a shared lock again and would hide its loss.)
            live.index.connection().execute_batch("BEGIN IMMEDIATE").unwrap();
            assert!(foreign_lock_refused(&path), "round {round}: the writer holds its lock");
            drop(Store::open(&directory));
            assert!(foreign_lock_refused(&path), "round {round}: another open kept the lock");
            live.index.connection().execute_batch("COMMIT").unwrap();
            drop(live);
            assert!(!foreign_lock_refused(&path), "round {round}: the lock went with its connection");
        }
    }
}
