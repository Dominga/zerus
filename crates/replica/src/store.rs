//! Documents on disk, one directory per project:
//! `<root>/<sha256(project ID)>/project.loro` and `tasks/<uuid>.loro`, plus
//! `tasks/<uuid>.updates` with the updates appended since that snapshot. Each
//! appended block carries its length and a checksum; a block cut short or
//! damaged by a crash ends the list, and the next save rewrites the snapshot.
//! Updates are folded into a fresh snapshot when they grow, and once when a
//! task is accepted (its Done compaction). One process at a time holds a project.
use crate::project::ProjectDoc;
use crate::schema::{Actor, Stage};
use crate::task::TaskDoc;
use crate::{ids, rank, Error, Result};
use fs2::FileExt;
use loro::VersionVector;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// A rank longer than this renumbers its stage (ranks stay valid up to 64).
const RENUMBER_AT: usize = 32;
/// Appended updates beyond this size are folded into the snapshot.
const FOLD_BYTES: u64 = 256 * 1024;
/// A block: little-endian `u32` length, the first four bytes of the SHA-256
/// of the payload, then the payload (Loro updates).
const HEADER: usize = 8;
/// The change journal: a `gen <UUID> <BASE>` line, then one changed document
/// per line (a task UUID, or `project`). BASE is the place, in the journal's
/// whole history, of its first line here, so dropping its oldest lines keeps
/// every later place valid. A derived index reads it instead of every task
/// file. It is a hint only: never synced and never written in place. Each
/// change replaces it under its name, so a link there carries no write into
/// another file. A reader that finds it missing, oversized, not a regular
/// file, of another generation, past its place or with a line it does not
/// expect reads every task file instead.
const CHANGES: &str = "changes";
/// Past this the journal drops its oldest lines, keeping the newest half.
const CHANGES_LIMIT: usize = 64 * 1024;
/// The most of it ever read.
const CHANGES_READ: u64 = CHANGES_LIMIT as u64 + 4096;

/// The change journal as read: its generation, the place of its first line
/// and its whole lines.
struct Journal {
    generation: String,
    base: u64,
    body: Vec<u8>,
}

enum Hint {
    Missing,
    Unusable,
    Found(Journal),
}

/// Reads the change journal: bounded, never through a link and never
/// waiting on a special file.
fn read_journal(path: &Path) -> Hint {
    use std::io::Read;
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == ErrorKind::NotFound => return Hint::Missing,
        Ok(meta) if meta.file_type().is_file() && meta.len() <= CHANGES_READ => (),
        _ => return Hint::Unusable,
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(
        &mut options,
        libc::O_NOFOLLOW | libc::O_NONBLOCK,
    );
    let Ok(file) = options.open(path) else {
        return Hint::Unusable;
    };
    if !file.metadata().is_ok_and(|meta| meta.is_file()) {
        return Hint::Unusable;
    }
    let mut bytes = Vec::new();
    if file.take(CHANGES_READ + 1).read_to_end(&mut bytes).is_err()
        || bytes.len() as u64 > CHANGES_READ
    {
        return Hint::Unusable;
    }
    // Whole lines only: a line cut short ends the journal.
    let whole = bytes
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |end| end + 1);
    let Some(header) = bytes[..whole].split_inclusive(|&b| b == b'\n').next() else {
        return Hint::Unusable;
    };
    let parsed = std::str::from_utf8(header)
        .ok()
        .and_then(|line| line.strip_suffix('\n')?.strip_prefix("gen "))
        .and_then(|rest| rest.split_once(' '))
        .filter(|(generation, _)| ids::is_uuid(generation))
        .and_then(|(generation, base)| Some((generation.to_owned(), base.parse::<u64>().ok()?)))
        // A base with no room left to grow cannot be a real journal.
        .filter(|(_, base)| base.checked_add(CHANGES_READ * 2).is_some());
    match parsed {
        Some((generation, base)) => Hint::Found(Journal {
            generation,
            base,
            body: bytes[header.len()..whole].to_vec(),
        }),
        None => Hint::Unusable,
    }
}

/// Replaces the change journal under its name, never writing through it.
fn write_journal(path: &Path, generation: &str, base: u64, body: &[u8]) -> bool {
    let mut bytes = format!("gen {generation} {base}\n").into_bytes();
    bytes.extend_from_slice(body);
    let temp = path.with_extension(format!("tmp-{}", ids::new_uuid()));
    let written = private_options()
        .create_new(true)
        .write(true)
        .open(&temp)
        .and_then(|mut file| file.write_all(&bytes));
    if written.is_err() || fs::rename(&temp, path).is_err() {
        let _ = fs::remove_file(&temp);
        return false;
    }
    true
}

/// What changed since a reader's place in the change journal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Changes {
    /// The journal's generation and the end of its last whole line; a
    /// reader passes both back next time.
    pub generation: String,
    pub offset: u64,
    /// The documents changed since that place, or `None` when the journal
    /// cannot tell (another generation, a damaged line): read every task.
    pub documents: Option<BTreeSet<String>>,
}

/// A task file that failed to load. It stays on disk and never blocks the others.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Damaged {
    pub uuid: String,
    pub error: String,
}

/// Where a task goes in the order of its stage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Position {
    Top,
    End,
    /// Zero-based; beyond the end means the end.
    Index(usize),
    /// Before or after the task with this short ID or UUID.
    Before(String),
    After(String),
}

pub struct ProjectStore {
    dir: PathBuf,
    project: String,
    _lock: File,
    /// Serializes this instance's read-modify-write of the change journal:
    /// a store is shared between threads, and `save` takes `&self`.
    journal: std::sync::Mutex<()>,
}

fn private_options() -> OpenOptions {
    #[cfg(unix)]
    let mut options = OpenOptions::new();
    #[cfg(not(unix))]
    let options = OpenOptions::new();
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options
}

fn private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o700))?;
    Ok(())
}

pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let temp = path.with_extension(format!("tmp-{}", ids::new_uuid()));
    let mut file = private_options().create_new(true).write(true).open(&temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&temp, path)?;
    // The rename itself survives a power loss only once the directory is synced.
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        File::open(parent)?.sync_all()?;
    }
    Ok(())
}

fn checksum(payload: &[u8]) -> [u8; 4] {
    Sha256::digest(payload)[..4].try_into().expect("four bytes")
}

/// A block as this store writes it, for fixtures of other writers.
#[cfg(test)]
pub(crate) fn block_for_test(payload: &[u8]) -> Vec<u8> {
    block(payload)
}

fn block(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER + payload.len());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&checksum(payload));
    out.extend_from_slice(payload);
    out
}

/// The managed mark: what this store last saved of a task cut over to the
/// execution schema, written before each save makes it durable. `saved` is
/// the version vector the task's files then hold, by decimal peer; `opening`
/// is the SHA-256 of the first block of the updates file that follows the
/// snapshot, header and payload. Every fold of such a task opens that file
/// with the operations at the document's frontiers: a block every copy
/// already holds, so it imports as nothing in this build and older ones,
/// and one that differs whenever the version does. A writer that folds or
/// rewrites the files without this mark (an older build), or a mark restored
/// from before a later fold, leaves the opening apart from the files; files
/// cut short leave them holding less than `saved`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Mark {
    pub(crate) saved: Option<BTreeMap<String, i32>>,
    #[serde(default)]
    pub(crate) opening: Option<String>,
}

impl Mark {
    /// The mark for files that hold `saved`, whose updates file opens with
    /// `first` (header and payload).
    pub(crate) fn new(saved: &VersionVector, first: &[u8]) -> Self {
        Self {
            saved: Some(
                saved
                    .iter()
                    .map(|(peer, counter)| (peer.to_string(), *counter))
                    .collect(),
            ),
            opening: Some(digest(first)),
        }
    }

    /// The saved version it records, when it records a possible one: the
    /// files hold the task's creation, so an empty version or a peer counted
    /// at zero or below is unreadable.
    pub(crate) fn version(&self) -> Option<VersionVector> {
        let saved = self.saved.as_ref().filter(|s| !s.is_empty())?;
        saved
            .iter()
            .map(|(peer, counter)| Some((peer.parse().ok()?, (*counter > 0).then_some(*counter)?)))
            .collect()
    }

    /// Whether it names `first` as the first block of the updates file.
    fn opens_with(&self, first: &[u8]) -> bool {
        self.opening.as_deref() == Some(digest(first).as_str())
    }
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// The first block of an updates file, header and payload; none when the
/// file is missing or its first block is cut short.
fn first_block(path: &Path) -> Result<Option<Vec<u8>>> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let size = file.metadata()?.len();
    let mut block = vec![0u8; HEADER];
    match std::io::Read::read_exact(&mut file, &mut block) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    let length = u32::from_le_bytes(block[..4].try_into().expect("four bytes")) as u64;
    // A length past the file is a cut block, never an allocation.
    if length == 0 || HEADER as u64 + length > size {
        return Ok(None);
    }
    block.resize(HEADER + length as usize, 0);
    std::io::Read::read_exact(&mut file, &mut block[HEADER..])?;
    Ok(Some(block))
}

/// What a load knows of a task's mark.
enum Marked {
    No,
    Unreadable,
    Saved(VersionVector),
}

/// The intact blocks with their end offsets, and whether the whole input was
/// intact. A short, empty or mismatched block ends the list.
pub(crate) fn blocks(bytes: &[u8]) -> (Vec<(&[u8], usize)>, bool) {
    let mut out = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let rest = &bytes[at..];
        if rest.len() < HEADER {
            return (out, false);
        }
        let length = u32::from_le_bytes(rest[..4].try_into().expect("four bytes")) as usize;
        if length == 0 || rest.len() - HEADER < length {
            return (out, false);
        }
        let payload = &rest[HEADER..HEADER + length];
        if checksum(payload) != rest[4..8] {
            return (out, false);
        }
        at += HEADER + length;
        out.push((payload, at));
    }
    (out, true)
}

impl ProjectStore {
    /// Opens the project's directory below `root` and waits for its lock.
    pub fn open(root: &Path, project: &str) -> Result<Self> {
        Ok(Self::opening(root, project, None)?.expect("open waits for the lock"))
    }

    /// Opens the project like `open`, but gives up after `wait` while another
    /// process holds its lock.
    pub fn open_within(root: &Path, project: &str, wait: Duration) -> Result<Option<Self>> {
        Self::opening(root, project, Some(wait))
    }

    fn opening(root: &Path, project: &str, wait: Option<Duration>) -> Result<Option<Self>> {
        if project.is_empty() || project.len() > 128 || project.chars().any(char::is_control) {
            return Err(Error::new("invalid project ID"));
        }
        private_dir(root)?;
        let dir = root.join(format!("{:x}", Sha256::digest(project.as_bytes())));
        private_dir(&dir)?;
        private_dir(&dir.join("tasks"))?;
        let lock = private_options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join(".lock"))?;
        match wait {
            None => lock.lock_exclusive()?,
            Some(wait) => {
                let deadline = Instant::now() + wait;
                while lock.try_lock_exclusive().is_err() {
                    if Instant::now() >= deadline {
                        return Ok(None);
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        }
        // A crash during an atomic write leaves its temporary file behind. No
        // one else writes while this lock is held, so any left now is garbage;
        // removing it is best effort.
        for folder in [dir.clone(), dir.join("tasks")] {
            for entry in fs::read_dir(&folder)? {
                let path = entry?.path();
                let temporary = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.starts_with("tmp-"));
                if temporary {
                    let _ = fs::remove_file(&path);
                }
            }
        }
        let name = dir.join("project-id");
        if !name.exists() {
            write_atomic(&name, project.as_bytes())?;
        }
        Ok(Some(Self {
            dir,
            project: project.into(),
            _lock: lock,
            journal: std::sync::Mutex::new(()),
        }))
    }

    pub fn project_id(&self) -> &str {
        &self.project
    }

    /// The project's folder: derived local caches (such as hgs's task index)
    /// live beside its documents and are never replicated.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn project(&self) -> Result<ProjectDoc> {
        match fs::read(self.dir.join("project.loro")) {
            Ok(bytes) => ProjectDoc::from_bytes(&bytes),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(ProjectDoc::new()),
            Err(error) => Err(error.into()),
        }
    }

    pub fn save_project(&self, doc: &ProjectDoc) -> Result<()> {
        write_atomic(&self.dir.join("project.loro"), &doc.snapshot()?)?;
        self.note_change("project");
        Ok(())
    }

    /// Notes `document` in the change journal by replacing it, never writing
    /// through its name; an unusable one starts a new generation. Best
    /// effort: a lost line is found by a reader's periodic check of every
    /// task file.
    pub(crate) fn note_change(&self, document: &str) {
        let _serial = self.journal.lock().unwrap_or_else(|e| e.into_inner());
        let path = self.dir.join(CHANGES);
        let (generation, mut base, mut body) = match read_journal(&path) {
            Hint::Found(journal) => (journal.generation, journal.base, journal.body),
            Hint::Missing | Hint::Unusable => (ids::new_uuid(), 0, Vec::new()),
        };
        body.extend_from_slice(document.as_bytes());
        body.push(b'\n');
        if body.len() > CHANGES_LIMIT {
            let from = body.len() - CHANGES_LIMIT / 2;
            let cut = body[from..]
                .iter()
                .position(|&b| b == b'\n')
                .map_or(body.len(), |at| from + at + 1);
            match base.checked_add(cut as u64) {
                Some(moved) => {
                    base = moved;
                    body.drain(..cut);
                }
                // No room left: a new generation, with only this change.
                None => {
                    let fresh = ids::new_uuid();
                    let line = format!("{document}\n");
                    write_journal(&path, &fresh, 0, line.as_bytes());
                    return;
                }
            }
        }
        write_journal(&path, &generation, base, &body);
    }

    /// The documents changed since place `offset` of journal `generation`
    /// (both as returned last time; an empty generation the first time). A
    /// missing or unusable journal cannot say: it is started again, empty, so
    /// later reads can, and this one reports every document as unknown.
    pub fn changes_since(&self, generation: &str, offset: u64) -> Result<Changes> {
        let _serial = self.journal.lock().unwrap_or_else(|e| e.into_inner());
        let path = self.dir.join(CHANGES);
        let journal = match read_journal(&path) {
            Hint::Found(journal) => journal,
            Hint::Missing | Hint::Unusable => {
                let fresh = ids::new_uuid();
                let started = write_journal(&path, &fresh, 0, &[]);
                return Ok(Changes {
                    generation: if started { fresh } else { String::new() },
                    offset: 0,
                    documents: None,
                });
            }
        };
        let Some(end) = journal.base.checked_add(journal.body.len() as u64) else {
            return Ok(Changes {
                generation: String::new(),
                offset: 0,
                documents: None,
            });
        };
        let mut changes = Changes {
            generation: journal.generation,
            offset: end,
            documents: None,
        };
        if changes.generation != generation || offset < journal.base || offset > end {
            return Ok(changes);
        }
        let mut documents = BTreeSet::new();
        let unread = &journal.body[(offset - journal.base) as usize..];
        for line in unread.split_inclusive(|&b| b == b'\n') {
            match std::str::from_utf8(line)
                .ok()
                .and_then(|l| l.strip_suffix('\n'))
            {
                Some(document) if document == "project" || ids::is_uuid(document) => {
                    documents.insert(document.to_owned());
                }
                _ => return Ok(changes),
            }
        }
        changes.documents = Some(documents);
        Ok(changes)
    }

    pub(crate) fn snapshot_path(&self, uuid: &str) -> PathBuf {
        self.dir.join("tasks").join(format!("{uuid}.loro"))
    }

    pub(crate) fn updates_path(&self, uuid: &str) -> PathBuf {
        self.dir.join("tasks").join(format!("{uuid}.updates"))
    }

    /// Beside a task's files: present once this store saved it cut over to the
    /// execution schema (attempts or a floor), recording the version its
    /// snapshot holds. It outlives a damaged copy, so that copy is never
    /// replaced by another replica's whole document that lacks part of it.
    pub(crate) fn managed_path(&self, uuid: &str) -> PathBuf {
        self.dir.join("tasks").join(format!("{uuid}.managed"))
    }

    /// Whether this store ever saved the task cut over to the execution
    /// schema. Any entry at the mark's path counts, even one that cannot be
    /// read or a dangling link: only its verified absence means unmarked.
    pub fn was_managed(&self, uuid: &str) -> bool {
        !matches!(
            fs::symlink_metadata(self.managed_path(uuid)),
            Err(error) if error.kind() == ErrorKind::NotFound
        )
    }

    /// The task's mark; none when it is missing or cannot be read.
    pub(crate) fn mark(&self, uuid: &str) -> Option<Mark> {
        serde_json::from_slice(&fs::read(self.managed_path(uuid)).ok()?).ok()
    }

    /// Whether the task is marked, and the version its mark says this store
    /// saved: a single read of the mark, then a check of its path only
    /// when there is nothing to read.
    fn marked(&self, uuid: &str) -> Marked {
        let path = self.managed_path(uuid);
        match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice::<Mark>(&bytes)
                .ok()
                .and_then(|mark| mark.version())
                .map_or(Marked::Unreadable, Marked::Saved),
            Err(error) if error.kind() == ErrorKind::NotFound => {
                match fs::symlink_metadata(&path) {
                    Err(error) if error.kind() == ErrorKind::NotFound => Marked::No,
                    _ => Marked::Unreadable,
                }
            }
            Err(_) => Marked::Unreadable,
        }
    }

    pub(crate) fn write_mark(&self, uuid: &str, mark: &Mark) -> Result<()> {
        write_atomic(&self.managed_path(uuid), &serde_json::to_vec(mark)?)
    }

    /// The version this store last saved, as the mark records it, only while
    /// the updates file still opens with the exact block the mark names: no
    /// other writer folded or rewrote the files, and no later fold of this
    /// store replaced them, since the mark was written.
    pub(crate) fn fresh_version(&self, uuid: &str) -> Option<VersionVector> {
        let mark = self.mark(uuid)?;
        let first = first_block(&self.updates_path(uuid)).ok()??;
        mark.opens_with(&first).then(|| mark.version()).flatten()
    }

    pub fn uuids(&self) -> Result<Vec<String>> {
        let mut uuids = BTreeSet::new();
        for entry in fs::read_dir(self.dir.join("tasks"))? {
            let path = entry?.path();
            let extension = path.extension().and_then(|e| e.to_str());
            if matches!(extension, Some("loro" | "updates")) {
                if let Some(stem) = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .filter(|s| ids::is_uuid(s))
                {
                    uuids.insert(stem.to_owned());
                }
            }
        }
        Ok(uuids.into_iter().collect())
    }

    pub fn load(&self, uuid: &str) -> Result<TaskDoc> {
        let mut task = TaskDoc::from_bytes(&fs::read(self.snapshot_path(uuid))?)?;
        if task.uuid() != uuid {
            return Err(Error::failed("the task file does not match its name"));
        }
        match fs::read(self.updates_path(uuid)) {
            Ok(bytes) => {
                let (found, intact) = blocks(&bytes);
                task.needs_fold = !intact;
                for (payload, end) in found {
                    if !task.import_block(payload) {
                        task.needs_fold = true;
                        break;
                    }
                    task.saved_len = end as u64;
                }
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        // A copy this store cut over holds at least what its mark says was
        // saved (one small read); less is files cut short, a save that never
        // finished, or an older build that wrote them so, and the copy is
        // damaged rather than read without those changes. A mark that cannot
        // be read proves nothing, so it keeps the copy too.
        match self.marked(uuid) {
            Marked::No => {}
            Marked::Unreadable => {
                return Err(Error::failed(
                    "the copy here was cut over to execution attempts and its mark cannot be read",
                ))
            }
            Marked::Saved(saved) => {
                if !task.current_vv().includes_vv(&saved) {
                    return Err(Error::failed(
                        "the copy here holds less than this computer saved: its files were cut \
                         short or a save did not finish",
                    ));
                }
            }
        }
        // A task that cannot be shown is damaged: listed apart, never blocking the rest.
        task.view()?;
        task.saved = Some(task.current_vv());
        task.saved_stage = Some(task.stage()?);
        Ok(task)
    }

    pub fn save(&self, task: &mut TaskDoc) -> Result<()> {
        let snapshot = self.snapshot_path(task.uuid());
        let updates = self.updates_path(task.uuid());
        let current = task.current_vv();
        let stage = task.stage()?;
        let on_disk = match fs::metadata(&updates) {
            Ok(meta) => meta.len(),
            Err(error) if error.kind() == ErrorKind::NotFound => 0,
            Err(error) => return Err(error.into()),
        };
        let fold = task.saved.is_none()
            || !snapshot.exists()
            || task.needs_fold
            // Bytes past the valid blocks: a cut append, or another writer.
            || on_disk != task.saved_len
            || on_disk >= FOLD_BYTES
            || (stage == Stage::Done && task.saved_stage != Some(Stage::Done));
        if !fold && task.saved.as_ref() == Some(&current) {
            return Ok(());
        }
        let managed = task.execution_managed();
        // A fold of a cut-over task opens its updates file with the
        // operations at the frontiers; any other save appends the changes.
        let next = if fold {
            managed
                .then(|| task.export_frontier().map(|bytes| block(&bytes)))
                .transpose()?
        } else {
            let saved = task.saved.as_ref().expect("checked above");
            Some(block(&task.export_since_vv(saved)?))
        };
        // The mark precedes the files it describes, on every save of a
        // cut-over task (one small durable write each): after a crash between
        // them it names a version or a block they do not hold, which only
        // keeps a later recovery from going ahead.
        if managed {
            let ours = next.as_deref().expect("a cut-over save writes a block");
            let existing = if fold { None } else { first_block(&updates)? };
            let mark = Mark::new(&current, existing.as_deref().unwrap_or(ours));
            if self.mark(task.uuid()).as_ref() != Some(&mark) {
                self.write_mark(task.uuid(), &mark)?;
            }
        }
        if fold {
            // The snapshot first: a crash before the removal leaves updates
            // the snapshot already contains, and importing them again is harmless.
            write_atomic(&snapshot, &task.snapshot()?)?;
            match &next {
                Some(first) => write_atomic(&updates, first)?,
                None => match fs::remove_file(&updates) {
                    Ok(()) => {}
                    Err(error) if error.kind() == ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                },
            }
            task.saved_len = next.as_ref().map_or(0, |first| first.len() as u64);
            task.needs_fold = false;
        } else {
            let bytes = next.expect("an append writes a block");
            let mut file = private_options().create(true).append(true).open(&updates)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            task.saved_len += bytes.len() as u64;
        }
        task.saved = Some(current);
        task.saved_stage = Some(stage);
        self.note_change(task.uuid());
        Ok(())
    }

    /// Every task that loads, and the ones that do not.
    pub fn tasks(&self) -> Result<(Vec<TaskDoc>, Vec<Damaged>)> {
        let mut tasks = Vec::new();
        let mut damaged = Vec::new();
        for uuid in self.uuids()? {
            match self.load(&uuid) {
                Ok(task) => tasks.push(task),
                Err(error) => damaged.push(Damaged {
                    uuid,
                    error: error.to_string(),
                }),
            }
        }
        Ok((tasks, damaged))
    }

    /// A task by its UUID or its short ID.
    pub fn find(&self, reference: &str) -> Result<TaskDoc> {
        if ids::is_uuid(reference) {
            if !self.snapshot_path(reference).exists() && !self.updates_path(reference).exists() {
                return Err(Error::new(format!("no task {reference} in this project")));
            }
            return self
                .load(reference)
                .map_err(|e| Error::failed(format!("task {reference} is damaged: {e}")));
        }
        if !ids::is_short_id(reference) {
            return Err(Error::new(format!("{reference:?} is not a task ID")));
        }
        let (tasks, damaged) = self.tasks()?;
        let mut found: Vec<_> = tasks.into_iter().filter(|t| t.id() == reference).collect();
        match found.len() {
            // A damaged file's short ID cannot be read: it may be the one.
            0 if !damaged.is_empty() => Err(Error::new(format!(
                "no task {reference} in this project; {} could not be read",
                match damaged.len() {
                    1 => "1 task file".to_owned(),
                    n => format!("{n} task files"),
                }
            ))),
            0 => Err(Error::new(format!("no task {reference} in this project"))),
            1 => Ok(found.pop().expect("one")),
            _ => Err(Error::new(format!(
                "{reference} names more than one task; use its UUID"
            ))),
        }
    }

    /// A short ID no local task uses yet.
    pub fn unused_short_id(&self) -> Result<String> {
        let (tasks, _) = self.tasks()?;
        let used: BTreeSet<String> = tasks.iter().map(TaskDoc::id).collect();
        loop {
            let id = ids::short_id();
            if !used.contains(&id) {
                return Ok(id);
            }
        }
    }

    /// The tasks of `stage` in their order: rank, then UUID for equal ranks.
    pub fn ordered(&self, stage: Stage) -> Result<Vec<TaskDoc>> {
        let (tasks, _) = self.listing()?;
        Ok(tasks
            .into_iter()
            .filter(|t| t.stage().ok() == Some(stage))
            .collect())
    }

    /// Every task, loaded once and sorted by rank then UUID (so each stage is
    /// in its order), and the ones that do not load.
    pub fn listing(&self) -> Result<(Vec<TaskDoc>, Vec<Damaged>)> {
        let (mut tasks, damaged) = self.tasks()?;
        tasks.sort_by(|a, b| (a.rank(), a.uuid()).cmp(&(b.rank(), b.uuid())));
        Ok((tasks, damaged))
    }

    /// The rank that puts a task at `position` in `stage`, leaving out the
    /// task being moved (its UUID).
    pub fn rank_for(
        &self,
        stage: Stage,
        position: &Position,
        moving: Option<&str>,
    ) -> Result<String> {
        let list: Vec<TaskDoc> = self
            .ordered(stage)?
            .into_iter()
            .filter(|t| Some(t.uuid()) != moving)
            .collect();
        let at = |reference: &str| {
            list.iter()
                .position(|t| t.id() == reference || t.uuid() == reference)
                .ok_or_else(|| Error::new(format!("{reference} is not in {}", stage.label())))
        };
        let index = match position {
            Position::Top => 0,
            Position::End => list.len(),
            Position::Index(index) => (*index).min(list.len()),
            Position::Before(reference) => at(reference)?,
            Position::After(reference) => at(reference)? + 1,
        };
        let low = index.checked_sub(1).map(|i| list[i].rank());
        // Equal ranks can meet after concurrent moves: aim below the first higher one.
        let high = list[index..]
            .iter()
            .map(TaskDoc::rank)
            .find(|r| low.as_ref().is_none_or(|l| r > l));
        let placed = rank::between(low.as_deref(), high.as_deref());
        if placed.as_ref().is_ok_and(|r| r.len() <= RENUMBER_AT)
            || list.iter().any(|t| t.schema() > crate::TASK_SCHEMA)
        {
            return placed;
        }
        // Inserting at one place again and again lengthens ranks; renumber the
        // stage evenly, in its current order, and place the task among them.
        let fresh = rank::spread(list.len());
        for (mut task, rank) in list.into_iter().zip(&fresh) {
            if task.rank() != *rank {
                task.set_rank(rank)?;
                self.save(&mut task)?;
            }
        }
        let low = index.checked_sub(1).map(|i| fresh[i].as_str());
        rank::between(low, fresh.get(index).map(String::as_str))
    }

    /// How many tasks agents started since `since` (Unix milliseconds).
    pub fn agent_starts_since(&self, since: i64) -> Result<usize> {
        let (tasks, _) = self.tasks()?;
        let mut count = 0;
        for task in tasks {
            count += task
                .view()?
                .log
                .iter()
                .filter(|e| e.kind == "sent" && e.at >= since && matches!(e.actor, Actor::Agent(_)))
                .count();
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{SessionKey, Target};
    use crate::task::NewTask;
    use crate::ErrorKind;

    fn new_task(store: &ProjectStore, rank: &str) -> TaskDoc {
        TaskDoc::create(NewTask {
            project: store.project_id(),
            id: &store.unused_short_id().unwrap(),
            text: "Add rate limiting",
            stage: Stage::Todo,
            target: Target::Any,
            author: Actor::Person,
            rank,
            now: 1,
        })
        .unwrap()
    }

    fn api() -> SessionKey {
        SessionKey {
            machine: "m".into(),
            session: "codex/api".into(),
            ..Default::default()
        }
    }

    fn comments(task: &TaskDoc) -> Vec<String> {
        task.view()
            .unwrap()
            .log
            .into_iter()
            .filter(|e| e.kind == "comment")
            .map(|e| e.text)
            .collect()
    }

    fn append(store: &ProjectStore, task: &TaskDoc, bytes: &[u8]) {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(store.updates_path(task.uuid()))
            .unwrap();
        file.write_all(bytes).unwrap();
    }

    #[test]
    fn saves_append_updates_and_reload() {
        let root = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(root.path(), "p1").unwrap();
        let mut task = new_task(&store, "V");
        store.save(&mut task).unwrap();
        task.comment("first", &Actor::Person, 2).unwrap();
        store.save(&mut task).unwrap();
        task.comment("second", &Actor::Person, 3).unwrap();
        store.save(&mut task).unwrap();
        let updates = fs::read(store.updates_path(task.uuid())).unwrap();
        let (found, intact) = blocks(&updates);
        assert_eq!((found.len(), intact), (2, true));
        let loaded = store.find(&task.id()).unwrap();
        assert_eq!(loaded.view().unwrap(), task.view().unwrap());
        assert_eq!(
            store.find(task.uuid()).unwrap().view().unwrap(),
            task.view().unwrap()
        );
    }

    #[test]
    fn a_cut_tail_is_repaired_and_later_saves_survive() {
        for junk in [&[200, 0, 0, 0, 1, 2][..], &[0; 64][..]] {
            let root = tempfile::tempdir().unwrap();
            let store = ProjectStore::open(root.path(), "p1").unwrap();
            let mut task = new_task(&store, "V");
            store.save(&mut task).unwrap();
            task.comment("first", &Actor::Person, 2).unwrap();
            store.save(&mut task).unwrap();
            append(&store, &task, junk);
            // A later process: the cut tail is skipped, and the next save rewrites the snapshot.
            let mut again = store.load(task.uuid()).unwrap();
            assert!(again.needs_fold);
            again.comment("after the crash", &Actor::Person, 3).unwrap();
            store.save(&mut again).unwrap();
            assert!(!store.updates_path(task.uuid()).exists());
            again.comment("then appended", &Actor::Person, 4).unwrap();
            store.save(&mut again).unwrap();
            let last = store.load(task.uuid()).unwrap();
            assert_eq!(
                comments(&last),
                ["first", "after the crash", "then appended"]
            );
            assert!(!last.needs_fold);
        }
    }

    #[test]
    fn a_damaged_block_ends_the_updates() {
        // A damaged payload byte, and a damaged checksum over an intact payload.
        for damaged in [|len: usize| len - 1, |_| 4] {
            let root = tempfile::tempdir().unwrap();
            let store = ProjectStore::open(root.path(), "p1").unwrap();
            let mut task = new_task(&store, "V");
            store.save(&mut task).unwrap();
            task.comment("first", &Actor::Person, 2).unwrap();
            store.save(&mut task).unwrap();
            let path = store.updates_path(task.uuid());
            let mut bytes = fs::read(&path).unwrap();
            let at = damaged(bytes.len());
            bytes[at] ^= 0xff;
            fs::write(&path, bytes).unwrap();
            let loaded = store.load(task.uuid()).unwrap();
            assert!(loaded.needs_fold);
            assert!(comments(&loaded).is_empty());
        }
    }

    #[test]
    fn done_compacts_once() {
        let root = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(root.path(), "p1").unwrap();
        let mut task = new_task(&store, "V");
        store.save(&mut task).unwrap();
        task.record_send(&api(), "r", &Actor::Person, 2).unwrap();
        store.save(&mut task).unwrap();
        task.record_report("Done.", &Actor::Person, 3).unwrap();
        task.set_stage(Stage::Done, &Actor::Person, 4).unwrap();
        store.save(&mut task).unwrap();
        assert!(!store.updates_path(task.uuid()).exists());
        // Comments on a Done task are appended again.
        task.comment("thanks", &Actor::Person, 5).unwrap();
        store.save(&mut task).unwrap();
        assert!(store.updates_path(task.uuid()).exists());
        assert_eq!(
            store.load(task.uuid()).unwrap().view().unwrap(),
            task.view().unwrap()
        );
    }

    #[test]
    fn a_damaged_file_is_set_aside_and_projects_are_separate() {
        let root = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(root.path(), "p1").unwrap();
        let mut task = new_task(&store, "V");
        store.save(&mut task).unwrap();
        let bad = ids::new_uuid();
        fs::write(store.snapshot_path(&bad), b"not loro").unwrap();
        let (tasks, damaged) = store.tasks().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(damaged.len(), 1);
        assert_eq!(damaged[0].uuid, bad);
        assert!(store.snapshot_path(&bad).exists());
        let other = ProjectStore::open(root.path(), "p2").unwrap();
        assert!(other.tasks().unwrap().0.is_empty());
        assert!(store.find("T-zzzzzz").is_err());
        assert!(store.find("nonsense").is_err());
    }

    #[test]
    fn a_damaged_task_is_named_as_damaged() {
        let root = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(root.path(), "p1").unwrap();
        let mut task = new_task(&store, "V");
        store.save(&mut task).unwrap();
        let bad = ids::new_uuid();
        fs::write(store.snapshot_path(&bad), b"not loro").unwrap();
        let by_uuid = store.find(&bad).err().unwrap();
        assert!(by_uuid.to_string().contains("is damaged"), "{by_uuid}");
        assert_eq!(by_uuid.kind(), ErrorKind::Failed);
        let by_id = store.find("T-zzzzzz").err().unwrap().to_string();
        assert!(by_id.contains("1 task file could not be read"), "{by_id}");
        let missing = store.find(&ids::new_uuid()).err().unwrap().to_string();
        assert!(missing.starts_with("no task"), "{missing}");
        let refused = task.set_stage(Stage::Done, &Actor::Person, 2).unwrap_err();
        assert_eq!(refused.kind(), ErrorKind::Refused);
    }

    #[test]
    fn leftover_temporary_files_are_swept_on_open() {
        let root = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(root.path(), "p1").unwrap();
        let mut task = new_task(&store, "V");
        store.save(&mut task).unwrap();
        let strays = [
            store
                .dir
                .join("tasks")
                .join(format!("{}.tmp-{}", task.uuid(), ids::new_uuid())),
            store.dir.join(format!("project.tmp-{}", ids::new_uuid())),
        ];
        for stray in &strays {
            fs::write(stray, b"half written").unwrap();
        }
        drop(store);
        let store = ProjectStore::open(root.path(), "p1").unwrap();
        assert!(strays.iter().all(|s| !s.exists()));
        assert!(store.load(task.uuid()).is_ok());
    }

    #[test]
    fn a_task_that_cannot_be_shown_is_set_aside() {
        let root = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(root.path(), "p1").unwrap();
        let mut good = new_task(&store, "V");
        store.save(&mut good).unwrap();
        let mut bad = new_task(&store, "W");
        bad.set_raw_field("target", "{\"kind\":\"team\"}");
        store.save(&mut bad).unwrap();
        let (tasks, damaged) = store.tasks().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(damaged.len(), 1);
        assert_eq!(damaged[0].uuid, bad.uuid());
        // Listing a stage and counting agent starts still work.
        assert_eq!(store.ordered(Stage::Todo).unwrap().len(), 1);
        assert_eq!(store.agent_starts_since(0).unwrap(), 0);
    }

    #[test]
    fn ranks_place_tasks_even_between_equal_ranks() {
        let root = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(root.path(), "p1").unwrap();
        assert_eq!(
            store.rank_for(Stage::Todo, &Position::End, None).unwrap(),
            "V"
        );
        for rank in ["V", "V", "X"] {
            let mut task = new_task(&store, rank);
            store.save(&mut task).unwrap();
        }
        let list = store.ordered(Stage::Todo).unwrap();
        let last = list[2].id();
        let between = store
            .rank_for(Stage::Todo, &Position::Index(1), None)
            .unwrap();
        assert!(
            "V" < between.as_str() && between.as_str() < "X",
            "{between}"
        );
        let before = store
            .rank_for(Stage::Todo, &Position::Before(last.clone()), None)
            .unwrap();
        assert!("V" < before.as_str() && before.as_str() < "X", "{before}");
        assert!(
            store
                .rank_for(Stage::Todo, &Position::Top, None)
                .unwrap()
                .as_str()
                < "V"
        );
        assert!(
            store
                .rank_for(Stage::Todo, &Position::End, None)
                .unwrap()
                .as_str()
                > "X"
        );
        let after_last = store
            .rank_for(
                Stage::Todo,
                &Position::After(last.clone()),
                Some(list[2].uuid()),
            )
            .is_err();
        assert!(after_last, "the moving task is not its own anchor");
        assert!(store
            .rank_for(Stage::Backlog, &Position::Before(last), None)
            .is_err());
    }

    #[test]
    fn a_listing_loads_every_task_once_in_rank_order() {
        let root = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(root.path(), "p1").unwrap();
        let mut ids = Vec::new();
        for rank in ["X", "V", "W"] {
            let mut task = new_task(&store, rank);
            store.save(&mut task).unwrap();
            ids.push(task.id());
        }
        fs::write(store.snapshot_path(&ids::new_uuid()), b"not loro").unwrap();
        let (tasks, damaged) = store.listing().unwrap();
        let order: Vec<String> = tasks.iter().map(TaskDoc::id).collect();
        assert_eq!(order, [ids[1].clone(), ids[2].clone(), ids[0].clone()]);
        assert_eq!(damaged.len(), 1);
    }

    #[test]
    fn long_ranks_are_renumbered_in_order() {
        let root = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(root.path(), "p1").unwrap();
        // Ranks that many moves to the top leave behind.
        let long = |last: char| format!("{}{last}", "0".repeat(40));
        for rank in [long('1'), long('2'), "V".to_owned()] {
            let mut task = new_task(&store, &rank);
            store.save(&mut task).unwrap();
        }
        let before: Vec<String> = store
            .ordered(Stage::Todo)
            .unwrap()
            .iter()
            .map(TaskDoc::id)
            .collect();
        let top = store.rank_for(Stage::Todo, &Position::Top, None).unwrap();
        let after = store.ordered(Stage::Todo).unwrap();
        assert_eq!(after.iter().map(TaskDoc::id).collect::<Vec<_>>(), before);
        assert!(after.iter().all(|t| t.rank().len() <= 5), "renumbered");
        assert!(top.len() <= 5 && top < after[0].rank(), "{top}");
    }

    #[test]
    fn agent_starts_are_counted() {
        let root = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(root.path(), "p1").unwrap();
        let mut by_agent = new_task(&store, "V");
        by_agent
            .record_send(&api(), "r1", &Actor::Agent(api()), 1_000)
            .unwrap();
        store.save(&mut by_agent).unwrap();
        let mut by_person = new_task(&store, "W");
        by_person
            .record_send(&api(), "r2", &Actor::Person, 1_000)
            .unwrap();
        store.save(&mut by_person).unwrap();
        assert_eq!(store.agent_starts_since(500).unwrap(), 1);
        assert_eq!(store.agent_starts_since(2_000).unwrap(), 0);
    }

    #[test]
    fn the_project_lock_is_exclusive() {
        let root = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(root.path(), "p1").unwrap();
        let other = File::open(store.dir.join(".lock")).unwrap();
        assert!(other.try_lock_exclusive().is_err());
        drop(store);
        assert!(other.try_lock_exclusive().is_ok());
    }

    #[test]
    fn the_change_journal_names_saved_tasks_and_otherwise_says_read_all() {
        let temp = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(temp.path(), "example").unwrap();
        // No journal: it is started, and this read cannot say.
        let started = store.changes_since("", 0).unwrap();
        assert!(started.documents.is_none() && !started.generation.is_empty());
        let place = |c: &Changes| {
            store
                .changes_since(&c.generation, c.offset)
                .unwrap()
                .documents
        };
        assert_eq!(place(&started), Some(BTreeSet::new()));
        let mut a = new_task(&store, "U");
        store.save(&mut a).unwrap();
        // A reader that does not know this generation cannot tell.
        let first = store.changes_since("", 0).unwrap();
        assert!(first.documents.is_none() && !first.generation.is_empty());
        let place = |c: &Changes| {
            store
                .changes_since(&c.generation, c.offset)
                .unwrap()
                .documents
        };
        assert_eq!(place(&first), Some(BTreeSet::new()));
        let mut b = new_task(&store, "V");
        store.save(&mut b).unwrap();
        a.comment("note", &Actor::Person, 2).unwrap();
        store.save(&mut a).unwrap();
        let next = store
            .changes_since(&first.generation, first.offset)
            .unwrap();
        let both = BTreeSet::from([a.uuid().to_owned(), b.uuid().to_owned()]);
        assert_eq!(next.documents, Some(both));
        // A save that writes nothing notes nothing.
        store.save(&mut a).unwrap();
        assert_eq!(place(&next), Some(BTreeSet::new()));
        // Another generation or a place past the end means read all.
        let other = Changes {
            generation: ids::new_uuid(),
            ..next.clone()
        };
        assert!(place(&other).is_none());
        let beyond = Changes {
            offset: next.offset + 4096,
            ..next.clone()
        };
        assert!(place(&beyond).is_none());
        // An unexpected line means read all, once: from the place returned
        // then, later notes read normally.
        let path = store.dir().join(CHANGES);
        let mut text = fs::read_to_string(&path).unwrap();
        text.push_str("not-a-document\n");
        fs::write(&path, &text).unwrap();
        let damaged = store.changes_since(&next.generation, next.offset).unwrap();
        assert!(damaged.documents.is_none());
        store.save_project(&ProjectDoc::new()).unwrap();
        let fresh = place(&damaged);
        assert_eq!(fresh, Some(BTreeSet::from(["project".to_owned()])));
        // A removed journal means read all.
        fs::remove_file(&path).unwrap();
        assert!(place(&damaged).is_none());
    }

    /// A growing journal keeps its generation and drops its oldest lines: a
    /// reader that keeps up never misses a change; one left behind reads all.
    #[test]
    fn a_full_journal_drops_its_oldest_lines_under_the_same_generation() {
        let temp = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(temp.path(), "example").unwrap();
        store.save_project(&ProjectDoc::new()).unwrap();
        let start = store.changes_since("", 0).unwrap();
        let mut behind = start.clone();
        let mut keeping_up = start.clone();
        let mut task = new_task(&store, "U");
        store.save(&mut task).unwrap();
        for n in 0..3000 {
            task.comment("note", &Actor::Person, 10 + n).unwrap();
            store.save(&mut task).unwrap();
            if n % 500 == 0 {
                let read = store
                    .changes_since(&keeping_up.generation, keeping_up.offset)
                    .unwrap();
                assert_eq!(
                    read.documents,
                    Some(BTreeSet::from([task.uuid().to_owned()])),
                    "{n}"
                );
                keeping_up = read;
            }
        }
        let read = store
            .changes_since(&keeping_up.generation, keeping_up.offset)
            .unwrap();
        assert_eq!(read.generation, start.generation, "the generation is kept");
        assert!(read.documents.is_some());
        assert!(fs::metadata(store.dir().join(CHANGES)).unwrap().len() <= CHANGES_READ);
        behind = store
            .changes_since(&behind.generation, behind.offset)
            .unwrap();
        assert!(behind.documents.is_none(), "its place was dropped");
    }

    /// review-n on #1350: the hint never writes through a link, never reads
    /// more than its budget and never waits on a special file.
    #[test]
    fn an_unsafe_or_oversized_hint_is_unknown_and_never_written_through() {
        let temp = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(temp.path(), "example").unwrap();
        store.save_project(&ProjectDoc::new()).unwrap();
        let path = store.dir().join(CHANGES);
        let project = store.dir().join("project.loro");
        let before = fs::read(&project).unwrap();
        // A hard link to the project document: a save replaces the name.
        fs::remove_file(&path).unwrap();
        fs::hard_link(&project, &path).unwrap();
        assert!(store.changes_since("", 0).unwrap().documents.is_none());
        let mut task = new_task(&store, "U");
        store.save(&mut task).unwrap();
        assert_eq!(
            fs::read(&project).unwrap(),
            before,
            "the project is untouched"
        );
        store.project().unwrap();
        let known = store.changes_since("", 0).unwrap();
        assert!(!known.generation.is_empty());
        // A symbolic link: likewise, and what it names is never touched.
        #[cfg(unix)]
        {
            let outside = temp.path().join("outside");
            fs::write(&outside, b"kept").unwrap();
            fs::remove_file(&path).unwrap();
            std::os::unix::fs::symlink(&outside, &path).unwrap();
            assert!(store
                .changes_since(&known.generation, known.offset)
                .unwrap()
                .documents
                .is_none());
            task.comment("note", &Actor::Person, 2).unwrap();
            store.save(&mut task).unwrap();
            assert_eq!(fs::read(&outside).unwrap(), b"kept");
            assert!(!fs::symlink_metadata(&path)
                .unwrap()
                .file_type()
                .is_symlink());
        }
        // Oversized: unknown, not read and trusted.
        let line = format!("{}\n", task.uuid());
        fs::write(
            &path,
            format!("gen {} 0\n{}", known.generation, line.repeat(30_000)),
        )
        .unwrap();
        let read = store.changes_since(&known.generation, 0).unwrap();
        assert!(read.documents.is_none() && read.generation != known.generation);
        // ... and it was started again, small.
        assert!(fs::metadata(&path).unwrap().len() < 128);
        assert_eq!(
            store
                .changes_since(&read.generation, read.offset)
                .unwrap()
                .documents,
            Some(BTreeSet::new())
        );
        // A special file is never opened to wait on.
        #[cfg(unix)]
        {
            fs::remove_file(&path).unwrap();
            let made = std::process::Command::new("mkfifo")
                .arg(&path)
                .status()
                .unwrap();
            assert!(made.success());
            assert!(store
                .changes_since(&known.generation, known.offset)
                .unwrap()
                .documents
                .is_none());
            task.comment("again", &Actor::Person, 3).unwrap();
            store.save(&mut task).unwrap();
            assert!(fs::symlink_metadata(&path).unwrap().file_type().is_file());
        }
    }

    /// review-n on #1350 R2: a damaged base never panics a read or a save.
    #[test]
    fn a_journal_base_without_room_is_unknown_and_never_panics() {
        let temp = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(temp.path(), "example").unwrap();
        let path = store.dir().join(CHANGES);
        let generation = ids::new_uuid();
        // At the very end: reading it must not overflow.
        fs::write(&path, format!("gen {generation} {}\nproject\n", u64::MAX)).unwrap();
        let read = store.changes_since(&generation, u64::MAX).unwrap();
        assert!(read.documents.is_none());
        // Near the end with a full body: a save stores the task and notes it.
        let near = u64::MAX - 10;
        let full = "project\n".repeat(CHANGES_LIMIT / 8);
        fs::write(&path, format!("gen {generation} {near}\n{full}")).unwrap();
        let mut task = new_task(&store, "U");
        store.save(&mut task).unwrap();
        store.load(task.uuid()).unwrap();
        let known = store.changes_since("", 0).unwrap();
        assert!(!known.generation.is_empty());
        task.comment("note", &Actor::Person, 2).unwrap();
        store.save(&mut task).unwrap();
        assert_eq!(
            store
                .changes_since(&known.generation, known.offset)
                .unwrap()
                .documents,
            Some(BTreeSet::from([task.uuid().to_owned()]))
        );
    }

    /// One store shared by threads: no note is lost to a concurrent
    /// read-modify-write of the journal.
    #[test]
    fn notes_from_threads_sharing_a_store_are_all_kept() {
        let temp = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(temp.path(), "example").unwrap();
        let start = store.changes_since("", 0).unwrap();
        let mut tasks: Vec<TaskDoc> = (0..64).map(|_| new_task(&store, "U")).collect();
        let uuids: BTreeSet<String> = tasks.iter().map(|t| t.uuid().to_owned()).collect();
        std::thread::scope(|scope| {
            for chunk in tasks.chunks_mut(8) {
                let store = &store;
                scope.spawn(move || {
                    for task in chunk {
                        store.save(task).unwrap();
                    }
                });
            }
        });
        let read = store
            .changes_since(&start.generation, start.offset)
            .unwrap();
        assert_eq!(read.documents, Some(uuids));
    }
}
