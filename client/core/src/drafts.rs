//! Durable composer drafts in the Qt client's format (version 1), so drafts
//! move between the clients. Ported from `tray/src/ComposerDraftStore.cpp`.
//!
//! Each draft has its own private folder, named by the SHA-256 of its key:
//! an atomically replaced `draft.json` manifest and immutable
//! `<sha256>.bin` attachment snapshots, so typing never rewrites attachment
//! bytes or relies on the original file staying around. The manifest is
//! authoritative, including an empty one after a send.
//!
//! [`DraftStore`] writes only its own root, on one writer thread with ordered
//! operations. Qt's store is read-only for this client ([`read_draft`],
//! [`read_index`]): Qt replaces its files atomically while they are read, so a
//! read that fails is tried once more before it counts as invalid.
use crate::attachments::{
    Attachment, Draft, MAX_ATTACHMENTS, MAX_ATTACHMENT_BYTES, MAX_TOTAL_BYTES,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::UNIX_EPOCH;

pub const NOT_RESTORED: &str =
    "Could not restore this draft completely. Saved files have been kept locally.";
pub const INTERRUPTED: &str =
    "Delivery was interrupted. Check Activity or Terminal before sending again.";
pub const ALREADY_EXISTS: &str =
    "Another draft already exists here. Recover the previous one from Saved drafts.";
/// Saved drafts skips larger manifests.
const MAX_MANIFEST: u64 = 1024 * 1024;
const PREVIEW_CHARS: usize = 200;

/// One stored draft: the composer's text and attachments and what Qt keeps
/// with them.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct StoredDraft {
    pub draft: Draft,
    /// The cursor and selection anchor, in UTF-16 code units.
    pub position: i64,
    pub anchor: i64,
    /// A name its owner gave it; Saved drafts shows it.
    pub label: String,
    /// The owner's form fields, such as a task's title.
    pub form_state: Map<String, Value>,
    /// Saved while the message was being sent.
    pub sending: bool,
    pub uncertain: bool,
}

impl StoredDraft {
    fn is_empty(&self) -> bool {
        self.draft.text.is_empty() && self.draft.attachments.is_empty()
    }
}

/// A loaded draft with what the composer shows about it.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Loaded {
    pub draft: StoredDraft,
    /// [`NOT_RESTORED`] when the manifest or an attachment was invalid; the
    /// files stay on disk.
    pub storage_error: Option<String>,
    /// [`INTERRUPTED`] for a draft saved while it was being sent.
    pub notice: Option<String>,
}

/// What Saved drafts lists for one nonempty draft.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub label: String,
    /// The first nonblank line, whitespace collapsed, at most 200 characters.
    pub preview: String,
    /// The manifest's modification time, in seconds since the epoch.
    pub modified: f64,
    pub attachments: usize,
}

/// Whose conversation a draft key names. Message, subagent, question-answer,
/// voice and task drafts share one store and name their session differently.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnerKind {
    #[default]
    Unknown,
    Message,
    Subagent,
    Question,
    Voice,
    Task,
    TaskDescription,
    TaskComment,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Owner {
    pub kind: OwnerKind,
    /// The host alias, empty for this computer; a subagent key holds the
    /// machine name instead.
    pub machine: String,
    pub session: String,
    /// Set for an archived session.
    pub archive: String,
    pub conversation: String,
    pub subagent: String,
    pub project: String,
    pub task: String,
}

/// The owner of a draft key (`ComposerDraftStore::owner`).
pub fn owner(key: &str) -> Owner {
    for (prefix, kind) in [
        ("task-description\n", OwnerKind::TaskDescription),
        ("task-comment\n", OwnerKind::TaskComment),
    ] {
        if let Some(rest) = key.strip_prefix(prefix) {
            let parts: Vec<&str> = rest.split('\n').collect();
            if parts.len() != 2 || parts.iter().any(|part| part.is_empty()) {
                return Owner::default();
            }
            return Owner {
                kind,
                project: parts[0].into(),
                task: parts[1].into(),
                ..Owner::default()
            };
        }
    }
    for (prefix, kind) in [
        ("voice-message-draft\n", OwnerKind::Voice),
        ("task-draft\n", OwnerKind::Task),
    ] {
        if let Some(rest) = key.strip_prefix(prefix) {
            let mut result = owner(rest);
            let task_text = matches!(
                result.kind,
                OwnerKind::TaskDescription | OwnerKind::TaskComment
            );
            if result.kind == OwnerKind::Message || (kind == OwnerKind::Voice && task_text) {
                result.kind = kind;
                return result;
            }
        }
    }
    if let Some(rest) = key.strip_prefix("question\n") {
        // A question identity is a compact JSON array on one line; its first
        // item is the session's draft key.
        let identity = rest.split('\n').next().unwrap_or_default();
        let session = serde_json::from_str::<Value>(identity)
            .ok()
            .and_then(|value| value.get(0)?.as_str().map(str::to_owned))
            .unwrap_or_default();
        let mut result = owner(&session);
        if result.kind != OwnerKind::Unknown {
            result.kind = OwnerKind::Question;
            return result;
        }
    }
    let parts: Vec<&str> = key.split('\n').collect();
    if parts.len() < 2 || parts.len() > 4 || parts[1].is_empty() {
        return Owner::default();
    }
    let mut result = Owner {
        machine: parts[0].into(),
        session: parts[1].into(),
        ..Owner::default()
    };
    if parts.len() == 4 {
        if parts[3].is_empty() {
            return Owner::default();
        }
        result.kind = OwnerKind::Subagent;
        result.conversation = parts[2].into();
        result.subagent = parts[3].into();
    } else {
        result.kind = OwnerKind::Message;
        if parts.len() == 3 {
            result.archive = parts[2].into();
        }
    }
    result
}

/// Qt's draft store: its settings file's path with `.drafts` appended.
pub fn qt_root() -> Option<PathBuf> {
    let home = || {
        std::env::var_os("HOME")
            .filter(|home| !home.is_empty())
            .map(PathBuf::from)
    };
    if cfg!(windows) {
        std::env::var_os("APPDATA")
            .filter(|path| !path.is_empty())
            .map(|path| {
                PathBuf::from(path)
                    .join("hgdev")
                    .join("hgs-tray.ini.drafts")
            })
    } else if cfg!(target_os = "macos") {
        home().map(|home| home.join("Library/Preferences/com.hgdev.hgs-tray.plist.drafts"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .or_else(|| home().map(|home| home.join(".config")))
            .map(|config| config.join("hgdev").join("hgs-tray.conf.drafts"))
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn folder(root: &Path, key: &str) -> PathBuf {
    root.join(sha256_hex(key.as_bytes()))
}

#[cfg(unix)]
fn private(path: &Path, mode: u32) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).is_ok()
}

#[cfg(not(unix))]
fn private(_path: &Path, _mode: u32) -> bool {
    true
}

fn not_saved(error: impl std::fmt::Display) -> String {
    format!("Draft not saved: {error}. Keep this window open until saving succeeds.")
}

static TEMPORARY: AtomicU64 = AtomicU64::new(0);

/// Writes `bytes` to `path` through a private temporary file, synced before
/// it replaces the old one.
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let temporary = path.with_file_name(format!(
        ".{}.{}-{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy(),
        std::process::id(),
        TEMPORARY.fetch_add(1, Ordering::Relaxed)
    ));
    let written = (|| {
        let mut file = File::create(&temporary)?;
        private(&temporary, 0o600);
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    written.map_err(|error| {
        let _ = fs::remove_file(&temporary);
        not_saved(error)
    })
}

/// Reads at most `limit` bytes, failing when the file is larger.
fn read_bounded(path: &Path, limit: u64) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(std::io::Error::other("too large"));
    }
    Ok(bytes)
}

/// Writes one draft (`saveDirect`): blobs first, then the manifest; blobs the
/// manifest no longer names are removed after it.
fn save_direct(root: &Path, key: &str, draft: &StoredDraft) -> Result<(), String> {
    if key.is_empty() {
        return Ok(());
    }
    let path = folder(root, key);
    if fs::create_dir_all(&path).is_err() || !private(root, 0o700) || !private(&path, 0o700) {
        return Err("Draft not saved: cannot create private draft storage. Keep this window open until saving succeeds.".into());
    }
    let mut attachments = Vec::with_capacity(draft.draft.attachments.len());
    let mut retained = HashSet::new();
    for attachment in &draft.draft.attachments {
        let digest = sha256_hex(&attachment.bytes);
        let name = format!("{digest}.bin");
        let blob = path.join(&name);
        if !blob.exists() {
            atomic_write(&blob, &attachment.bytes)?;
        }
        retained.insert(name);
        attachments.push(json!({
            "name": attachment.name, "mime": attachment.mime, "reference": attachment.reference,
            "sha256": digest, "bytes": attachment.bytes.len(),
        }));
    }
    let manifest = json!({
        "version": 1, "key": key, "text": draft.draft.text,
        "position": draft.position, "anchor": draft.anchor, "label": draft.label,
        "form_state": draft.form_state,
        "next_attachment_number": draft.draft.next_attachment_number,
        "sending": draft.sending, "uncertain": draft.uncertain,
        "attachments": attachments,
    });
    atomic_write(&path.join("draft.json"), manifest.to_string().as_bytes())?;
    // The committed manifest is authoritative: a crash during this cleanup
    // cannot bring sent attachments back.
    if let Ok(entries) = fs::read_dir(&path) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".bin") && !retained.contains(&name) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    Ok(())
}

/// One read of a draft (`loadDirect`): what could be restored, and whether
/// the manifest and every attachment were valid.
fn load_once(root: &Path, key: &str) -> (StoredDraft, bool) {
    let path = folder(root, key);
    let manifest = path.join("draft.json");
    if !manifest.exists() {
        return (StoredDraft::default(), true);
    }
    let object = match read_bounded(&manifest, MAX_MANIFEST)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
    {
        Some(Value::Object(object))
            if object.get("version").and_then(Value::as_i64) == Some(1)
                && object.get("key").and_then(Value::as_str) == Some(key) =>
        {
            object
        }
        _ => return (StoredDraft::default(), false),
    };
    let text = |name: &str| {
        object
            .get(name)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned()
    };
    let int = |name: &str| object.get(name).and_then(Value::as_i64).unwrap_or(0);
    let flag = |name: &str| object.get(name).and_then(Value::as_bool).unwrap_or(false);
    let mut draft = StoredDraft {
        draft: Draft {
            text: text("text"),
            attachments: Vec::new(),
            next_attachment_number: int("next_attachment_number").max(1) as u32,
        },
        position: int("position"),
        anchor: int("anchor"),
        label: text("label"),
        form_state: object
            .get("form_state")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default(),
        sending: flag("sending"),
        uncertain: flag("uncertain"),
    };
    let mut valid = true;
    let mut total = 0;
    let listed = object
        .get("attachments")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for attachment in listed {
        let digest = attachment["sha256"].as_str().unwrap_or("");
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
            || draft.draft.attachments.len() >= MAX_ATTACHMENTS
        {
            valid = false;
            break;
        }
        let Ok(bytes) = read_bounded(
            &path.join(format!("{digest}.bin")),
            MAX_ATTACHMENT_BYTES as u64,
        ) else {
            valid = false;
            continue;
        };
        total += bytes.len();
        if bytes.is_empty()
            || total > MAX_TOTAL_BYTES
            || sha256_hex(&bytes) != digest
            || attachment["bytes"].as_u64() != Some(bytes.len() as u64)
        {
            valid = false;
            continue;
        }
        draft.draft.attachments.push(Attachment {
            name: attachment["name"].as_str().unwrap_or("").into(),
            mime: attachment["mime"].as_str().unwrap_or("").into(),
            bytes: bytes.into(),
            reference: attachment["reference"].as_str().unwrap_or("").into(),
        });
    }
    (draft, valid)
}

fn loaded((mut draft, valid): (StoredDraft, bool)) -> Loaded {
    let storage_error = (!valid).then(|| NOT_RESTORED.to_owned());
    // A draft saved during a send may have been delivered.
    draft.uncertain = draft.uncertain || draft.sending;
    let notice = draft.uncertain.then(|| INTERRUPTED.to_owned());
    Loaded {
        draft,
        storage_error,
        notice,
    }
}

/// A position in Saved drafts: a manifest's modification time and its
/// folder, which names its key. Opaque to the view; pass it back as is.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Cursor {
    pub modified_ns: u64,
    pub folder: String,
}

/// Which page of Saved drafts to read: at most `limit` (1-200) drafts older
/// than `before`, or the newest when it is `None`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexPage {
    pub limit: usize,
    #[serde(default)]
    pub before: Option<Cursor>,
}

/// One page of Saved drafts, newest first; `next` reads the following one.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Page {
    pub items: Vec<(String, Summary)>,
    pub next: Option<Cursor>,
}

/// The largest page.
pub const MAX_PAGE: usize = 200;

fn modified_ns(path: &Path) -> Option<u64> {
    let since = fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?;
    u64::try_from(since.as_nanos()).ok()
}

/// The Summary of one folder, `None` for an empty, damaged or foreign draft.
fn summary(root: &Path, folder: &str, modified_ns: u64) -> Option<(String, Summary)> {
    let bytes = read_bounded(&root.join(folder).join("draft.json"), MAX_MANIFEST).ok()?;
    let Ok(Value::Object(object)) = serde_json::from_slice::<Value>(&bytes) else {
        return None;
    };
    let key = object.get("key").and_then(Value::as_str).unwrap_or("");
    let text = object.get("text").and_then(Value::as_str).unwrap_or("");
    let attachments = object
        .get("attachments")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    if object.get("version").and_then(Value::as_i64) != Some(1)
        || folder != sha256_hex(key.as_bytes())
        || (text.trim().is_empty() && attachments == 0)
    {
        return None;
    }
    let preview = text
        .split('\n')
        .find(|line| !line.trim().is_empty())
        .map(|line| {
            line.split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(PREVIEW_CHARS)
                .collect()
        })
        .unwrap_or_default();
    let label = object
        .get("label")
        .and_then(Value::as_str)
        .unwrap_or("")
        .into();
    Some((
        key.to_owned(),
        Summary {
            label,
            preview,
            modified: modified_ns as f64 / 1e9,
            attachments,
        },
    ))
}

/// One page of the nonempty drafts under `root` (`indexDirect`), newest
/// first. Ordering needs only each manifest's modification time; manifests
/// are read for the page alone. A draft saved while someone pages moves to
/// the front, so the drafts that did not change keep their places: no page
/// repeats or skips one of them.
fn index_page(root: &Path, page: &IndexPage) -> Page {
    let limit = page.limit.clamp(1, MAX_PAGE);
    let Ok(entries) = fs::read_dir(root) else {
        return Page::default();
    };
    let mut drafts: Vec<Cursor> = entries
        .flatten()
        .filter_map(|entry| {
            let folder = entry.file_name().into_string().ok()?;
            if folder.len() != 64
                || !folder
                    .bytes()
                    .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
            {
                return None;
            }
            let modified_ns = modified_ns(&entry.path().join("draft.json"))?;
            Some(Cursor {
                modified_ns,
                folder,
            })
        })
        .filter(|cursor| page.before.as_ref().is_none_or(|before| cursor < before))
        .collect();
    drafts.sort_unstable_by(|a, b| b.cmp(a));
    let mut result = Page::default();
    for (index, cursor) in drafts.iter().enumerate() {
        if let Some(item) = summary(root, &cursor.folder, cursor.modified_ns) {
            result.items.push(item);
        }
        if result.items.len() == limit {
            if index + 1 < drafts.len() {
                result.next = Some(cursor.clone());
            }
            break;
        }
    }
    result
}

/// Reads a draft from Qt's store, never writing to it. Qt may replace a file
/// while it is read, so an invalid read is tried once more.
pub fn read_draft(qt_root: &Path, key: &str) -> Loaded {
    let first = load_once(qt_root, key);
    loaded(if first.1 {
        first
    } else {
        load_once(qt_root, key)
    })
}

/// One page of Saved drafts of Qt's store, read-only.
pub fn read_index(qt_root: &Path, page: &IndexPage) -> Page {
    index_page(qt_root, page)
}

type Work = Box<dyn FnOnce() + Send>;

/// A queued operation; a coalescing save may still replace its work.
type Slot = Arc<Mutex<Option<Work>>>;

#[derive(Default)]
struct Queue {
    jobs: VecDeque<(String, Slot)>,
    /// Saves that a later save of the same key may still replace.
    pending: HashMap<String, Slot>,
    closed: bool,
}

struct Writer {
    queue: Mutex<Queue>,
    ready: Condvar,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Writer {
    /// Queues `work`. A coalescing save replaces a queued save of the same
    /// key; anything else seals what is queued, so no later edit jumps ahead
    /// of it.
    fn enqueue(&self, key: &str, coalesce: bool, work: Work) {
        let mut queue = lock(&self.queue);
        if !coalesce {
            queue.pending.clear();
        } else if let Some(slot) = queue.pending.get(key) {
            *lock(slot) = Some(work);
            return;
        }
        let slot: Slot = Arc::new(Mutex::new(Some(work)));
        if coalesce {
            queue.pending.insert(key.to_owned(), Arc::clone(&slot));
        }
        queue.jobs.push_back((key.to_owned(), slot));
        self.ready.notify_one();
    }

    fn run(&self) {
        loop {
            let work = {
                let mut queue = lock(&self.queue);
                loop {
                    if let Some((key, slot)) = queue.jobs.pop_front() {
                        if queue
                            .pending
                            .get(&key)
                            .is_some_and(|queued| Arc::ptr_eq(queued, &slot))
                        {
                            queue.pending.remove(&key);
                        }
                        break lock(&slot).take();
                    }
                    if queue.closed {
                        return;
                    }
                    queue = self
                        .ready
                        .wait(queue)
                        .unwrap_or_else(PoisonError::into_inner);
                }
            };
            if let Some(work) = work {
                work();
            }
        }
    }
}

/// The client's own draft store. Every operation runs in order on its writer
/// thread, and each callback runs there too. Dropping the store flushes it.
pub struct DraftStore {
    root: PathBuf,
    writer: Arc<Writer>,
    thread: Option<JoinHandle<()>>,
}

impl DraftStore {
    pub fn open(root: PathBuf) -> DraftStore {
        let writer = Arc::new(Writer {
            queue: Mutex::new(Queue::default()),
            ready: Condvar::new(),
        });
        let worker = Arc::clone(&writer);
        let thread = std::thread::Builder::new()
            .name("zerus-draft-writer".into())
            .spawn(move || worker.run())
            .expect("the draft writer thread starts");
        DraftStore {
            root,
            writer,
            thread: Some(thread),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Saves as the user types: a later save of the same key replaces this
    /// one while it waits.
    pub fn save(
        &self,
        key: &str,
        draft: StoredDraft,
        done: impl FnOnce(Result<(), String>) + Send + 'static,
    ) {
        if key.is_empty() {
            return;
        }
        let (root, owned) = (self.root.clone(), key.to_owned());
        self.writer.enqueue(
            key,
            true,
            Box::new(move || done(save_direct(&root, &owned, &draft))),
        );
    }

    /// Saves before something depends on it, such as a send: never replaced.
    pub fn save_durable(
        &self,
        key: &str,
        draft: StoredDraft,
        done: impl FnOnce(Result<(), String>) + Send + 'static,
    ) {
        let (root, owned) = (self.root.clone(), key.to_owned());
        self.writer.enqueue(
            key,
            false,
            Box::new(move || done(save_direct(&root, &owned, &draft))),
        );
    }

    /// Loads after every save queued before it.
    pub fn load(&self, key: &str, done: impl FnOnce(Loaded) + Send + 'static) {
        let (root, owned) = (self.root.clone(), key.to_owned());
        self.writer.enqueue(
            key,
            false,
            Box::new(move || done(loaded(load_once(&root, &owned)))),
        );
    }

    /// One page of Saved drafts, after every save queued before it.
    pub fn index(&self, page: IndexPage, done: impl FnOnce(Page) + Send + 'static) {
        let root = self.root.clone();
        self.writer
            .enqueue("", false, Box::new(move || done(index_page(&root, &page))));
    }

    /// Moves a draft to a renamed session's key. An existing draft there is
    /// never overwritten ([`ALREADY_EXISTS`]); the old key is emptied only
    /// after the new one is durable, so a failure leaves both recoverable.
    pub fn rename(
        &self,
        old: &str,
        new: &str,
        draft: StoredDraft,
        done: impl FnOnce(Result<(), String>) + Send + 'static,
    ) {
        let (root, old, new) = (self.root.clone(), old.to_owned(), new.to_owned());
        let key = old.clone();
        self.writer.enqueue(
            &key,
            false,
            Box::new(move || {
                let destination = loaded(load_once(&root, &new));
                if !destination.draft.is_empty() || destination.storage_error.is_some() {
                    return done(Err(ALREADY_EXISTS.into()));
                }
                let result = save_direct(&root, &new, &draft)
                    .and_then(|()| save_direct(&root, &old, &StoredDraft::default()));
                done(result)
            }),
        );
    }

    /// Copies a draft from Qt's store into this one ("Recover"); Qt's copy
    /// stays. A nonempty draft here is never overwritten.
    pub fn recover(
        &self,
        qt_root: &Path,
        key: &str,
        done: impl FnOnce(Result<Loaded, String>) + Send + 'static,
    ) {
        let (root, qt_root, owned) = (self.root.clone(), qt_root.to_path_buf(), key.to_owned());
        self.writer.enqueue(
            key,
            false,
            Box::new(move || {
                let here = loaded(load_once(&root, &owned));
                if !here.draft.is_empty() || here.storage_error.is_some() {
                    return done(Err(ALREADY_EXISTS.into()));
                }
                let copy = read_draft(&qt_root, &owned);
                if copy.storage_error.is_some() {
                    return done(Ok(copy));
                }
                done(save_direct(&root, &owned, &copy.draft).map(|()| copy))
            }),
        );
    }

    /// Blocks until everything queued so far is written: on quit, and on a
    /// termination signal before the process exits.
    pub fn flush(&self) {
        let (sender, receiver) = mpsc::channel();
        self.writer.enqueue(
            "",
            false,
            Box::new(move || {
                let _ = sender.send(());
            }),
        );
        let _ = receiver.recv();
    }
}

impl Drop for DraftStore {
    fn drop(&mut self) {
        self.flush();
        lock(&self.writer.queue).closed = true;
        self.writer.ready.notify_all();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owners_follow_qt_key_kinds() {
        let message = owner("studio\nclaude/sample-project/review");
        assert_eq!(
            (
                message.kind,
                message.machine.as_str(),
                message.session.as_str()
            ),
            (OwnerKind::Message, "studio", "claude/sample-project/review")
        );
        assert_eq!(owner("\nclaude/p/a\narchive-7").archive, "archive-7");
        let subagent = owner("workstation\nclaude/p/a\nconv-1\nagent-2");
        assert_eq!(
            (
                subagent.kind,
                subagent.conversation.as_str(),
                subagent.subagent.as_str()
            ),
            (OwnerKind::Subagent, "conv-1", "agent-2")
        );
        assert_eq!(
            owner("workstation\nclaude/p/a\nconv-1\n").kind,
            OwnerKind::Unknown
        );
        assert_eq!(
            owner("voice-message-draft\n\nclaude/p/a").kind,
            OwnerKind::Voice
        );
        assert_eq!(
            owner("voice-message-draft\ntask-comment\nalpha\nt-1").kind,
            OwnerKind::Voice
        );
        assert_eq!(owner("task-draft\n\nclaude/p/a").kind, OwnerKind::Task);
        let comment = owner("task-comment\nalpha\nt-1");
        assert_eq!(
            (
                comment.kind,
                comment.project.as_str(),
                comment.task.as_str()
            ),
            (OwnerKind::TaskComment, "alpha", "t-1")
        );
        assert_eq!(owner("task-description\nalpha\n").kind, OwnerKind::Unknown);
        let question = owner("question\n[\"studio\\nclaude/p/a\",\"q-1\"]\nanswer");
        assert_eq!(
            (question.kind, question.session.as_str()),
            (OwnerKind::Question, "claude/p/a")
        );
        assert_eq!(owner("lonely").kind, OwnerKind::Unknown);
        assert_eq!(owner("a\n\nb").kind, OwnerKind::Unknown);
        assert_eq!(owner("a\nb\nc\nd\ne").kind, OwnerKind::Unknown);
    }
}
