//! Original draft manifests captured for G3 (zerus-5wmh.66.5.2.1.2), never a
//! live draft, a blob copy, a send or an import.
//!
//! The Next draft store's sole writer runs this as its first job, before the
//! one-time Qt/dev import and before any load, save or recovery: Next's own
//! manifests are the version before the store's first job. Qt's and the dev
//! app's stores have their own writers; their manifests are the versions
//! observed at that job, read with the store's retrying reader, and never
//! called drained or pre-first-write.
//!
//! One attempt only. `identity.json` (the lineage seed) is durable before any
//! read; the attempt ends in `receipt.json` (captured) or `failed.json` (not
//! captured, permanently: a later run never backfills an original that typing,
//! the import or a recovery may have changed since). A process that died inside
//! the capture job ran no later job, so its next start captures again. When the
//! seed or the outcome cannot be persisted, the store refuses writes for the
//! run, so nothing is overwritten without lineage.
//!
//! A capture never reports Next's store as captured when it shows an earlier
//! run that wrote without capturing first: the one-time import marker, or
//! changes after a seed that has no outcome. Its Qt and dev originals are
//! still recorded; Next's family stays incomplete
//! (`draft_capture_next_earlier_run`).
//!
//! Drafts differ from the attention-marks capture, whose not-captured outcome
//! pauses file writes: once the failure is durable it is permanent, so
//! refusing saves would protect nothing and would hold back what the user
//! types. Saves continue; only the one-time import waits for an explicit
//! Retry. A not-captured outcome stays incomplete in the report and never
//! becomes supported or complete.
//!
//! The container holds the exact manifest bytes, in the order of `index.json`,
//! whose entries bind each original's offset, size and SHA-256 and the
//! container's own SHA-256. Attachment references are stat-only observations
//! under the checked roots: no blob content, no send or deletion claim.
use super::{sources, SourceError, SourceResult};
use crate::fsx::preload::{self as disk, Stamp};
use crate::{attachments, drafts};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::{SystemTime, UNIX_EPOCH},
};
use zerus_desktop_migration::{canonical_uuid, sha256};

pub const BOUNDARY: &str = "next-drafts-before-first-job-v1";
pub const DEV_BOUNDARY: &str = "next-dev-drafts-observed-v1";
pub const QT_BOUNDARY: &str = "qt-drafts-observed-v1";
pub const FORMAT: &str = "draft-preload-container-v1";
const SLOT: &str = "drafts-preload-v1";
/// Directory entries one root may hold, drafts or not.
pub const MAX_SCAN: usize = 4096;
/// Manifests across every root.
pub const MAX_MANIFESTS: usize = 4096;
/// Manifest bytes across every root (the Bundle cap).
pub const MAX_BYTES: usize = 64 << 20;
const MANIFEST: usize = drafts::MAX_MANIFEST as usize;
const META: usize = 64 << 10;
const INDEX: usize = 16 << 20;

/// Which store a manifest came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Store {
    /// This client's own store, before its first job.
    Next,
    /// A dev app store (`dev.zerus.next`), as observed then.
    Dev,
    /// Qt's store, as observed then.
    Qt,
}

/// What one entry of a store holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Class {
    /// A manifest the existing draft checkpoint decoder accepts.
    Supported,
    /// Original bytes the decoder refuses: kept as raw evidence only.
    Unsupported,
    /// A draft folder with no manifest.
    NoManifest,
    /// A manifest that could not be read, even on a second try.
    Unreadable,
}

/// A stat of one referenced attachment snapshot, never its content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Blob {
    Present,
    SizeMismatch,
    Missing,
    Unreadable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlobRef {
    pub sha256: String,
    pub bytes: u64,
    pub class: Blob,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub store: Store,
    /// Which root of that store (dev stores may be several).
    pub root: u32,
    /// The draft folder's name: the SHA-256 of its key.
    pub folder: String,
    pub class: Class,
    /// Where the original bytes start in the container, and their size.
    pub offset: u64,
    pub size: u64,
    pub sha256: String,
    pub blobs: Vec<BlobRef>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Root {
    pub store: Store,
    pub root: u32,
    pub present: bool,
    /// Next's store already shows an uncaptured earlier run (its one-time
    /// import marker, or changes after a seed that has no outcome): none of
    /// its entries is recorded under the before-first-job boundary.
    pub earlier_run: bool,
}

/// The ordered container index: what the source hashes bind.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Index {
    pub version: u32,
    pub format: String,
    pub capture_id: String,
    pub roots: Vec<Root>,
    pub entries: Vec<Entry>,
    pub container_bytes: u64,
    pub container_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Seed {
    version: u32,
    boundary: String,
    capture_id: String,
}
impl Seed {
    fn valid(&self) -> bool {
        self.version == 1 && self.boundary == BOUNDARY && canonical_uuid(&self.capture_id)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    capture_id: String,
    index_sha256: String,
    index: Stamp,
    container: Option<Stamp>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Failed {
    version: u32,
    capture_id: String,
    reason: String,
}

/// What the draft store may do after the capture job.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gate {
    /// Saves may proceed: the lineage is durable.
    pub writes: bool,
    /// The one-time bulk import may run: the originals are captured.
    pub import: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Status {
    pub code: &'static str,
    pub capture_id: Option<String>,
    pub manifests: usize,
    /// Draft saves proceed; false refuses them for the run.
    pub writes: bool,
    /// The one-time import waits for an explicit Retry.
    pub paused: bool,
}

type Hook = Arc<dyn Fn() + Send + Sync>;

/// Managed native setup supplies the roots; no IPC or webview chooses them.
pub struct Capture {
    data: PathBuf,
    next: PathBuf,
    qt: Option<PathBuf>,
    dev: Vec<PathBuf>,
    state: Mutex<Status>,
    /// Republishes the status after it changes.
    notify: Mutex<Option<Hook>>,
    /// Queues the draft store's Retry job.
    retry: Mutex<Option<Hook>>,
}

impl Capture {
    pub fn new(data: PathBuf, next: PathBuf, qt: Option<PathBuf>, dev: Vec<PathBuf>) -> Self {
        Self {
            data,
            next,
            qt,
            dev,
            state: Mutex::new(Status {
                code: "draft_capture_waiting",
                capture_id: None,
                manifests: 0,
                writes: true,
                paused: false,
            }),
            notify: Mutex::new(None),
            retry: Mutex::new(None),
        }
    }

    /// The capture for the Next store at `next`, over the same sources as its
    /// one-time import ([`drafts::import_sources`]): Qt's store and the dev
    /// app's stores, never one that overlaps `next`.
    pub fn for_store(data: PathBuf, next: PathBuf, sources: &[drafts::ImportSource]) -> Self {
        let qt = sources
            .iter()
            .find(|source| source.name == "qt")
            .map(|source| source.root.clone());
        let dev = sources
            .iter()
            .filter(|source| source.name == "dev")
            .map(|source| source.root.clone())
            .collect();
        Self::new(data, next, qt, dev)
    }

    /// The capture of the switch build's own store in `data` and of this
    /// computer's Qt and dev stores, as its one-time import reads them.
    pub fn native(data: &Path) -> Self {
        let next = data.join(drafts::dir(false));
        let sources = drafts::import_sources(&next, drafts::qt_root(), drafts::dev_roots());
        Self::for_store(data.into(), next, &sources)
    }

    /// The capture a run's store opens with: every run of the switch build,
    /// in service mode or not (the service's capture also reports status), so
    /// no run of this build writes its store without capturing first. Other
    /// identities and replay never capture.
    pub fn for_run(switch: bool, service: Option<Arc<Self>>, data: &Path) -> Option<Arc<Self>> {
        switch.then(|| service.unwrap_or_else(|| Arc::new(Self::native(data))))
    }

    pub fn status(&self) -> Status {
        lock(&self.state).clone()
    }

    pub fn hook(&self, notify: Hook) {
        *lock(&self.notify) = Some(notify);
    }

    /// The draft store's Retry job; set once the store is open.
    pub fn hook_retry(&self, retry: Hook) {
        *lock(&self.retry) = Some(retry);
    }

    /// Retry from the migration status: queues the store's Retry job, which
    /// never changes a recorded outcome.
    pub fn request_retry(&self) {
        let retry = lock(&self.retry).clone();
        if let Some(retry) = retry {
            retry();
        }
    }

    fn changed(&self) {
        let notify = lock(&self.notify).clone();
        if let Some(notify) = notify {
            notify();
        }
    }

    fn set(&self, code: &'static str, capture_id: Option<String>, manifests: usize) {
        let mut state = lock(&self.state);
        state.code = code;
        state.capture_id = capture_id;
        state.manifests = manifests;
    }

    /// Set by the draft store when its import starts or stops waiting.
    pub(crate) fn set_paused(&self, paused: bool) {
        lock(&self.state).paused = paused;
        self.changed();
    }

    /// The draft store's first job: capture once, or report the recorded
    /// outcome of the one earlier attempt.
    pub fn prepare(&self) -> Gate {
        let gate = self.attempt();
        lock(&self.state).writes = gate.writes;
        self.changed();
        gate
    }

    fn attempt(&self) -> Gate {
        let refuse = Gate {
            writes: false,
            import: false,
        };
        if !self.data.is_absolute() || !self.next.is_absolute() {
            self.set("draft_capture_private_slot", None, 0);
            return refuse;
        }
        let slot = self.data.join(SLOT);
        let Ok(_parents) = disk::Parents::open(&slot, true) else {
            self.set("draft_capture_private_slot", None, 0);
            return refuse;
        };
        let Ok(_lock) = disk::Lock::take(&slot.join("capture.lock")) else {
            self.set("draft_capture_private_slot", None, 0);
            return refuse;
        };
        match read_json::<Failed>(&slot.join("failed.json")) {
            Ok(Some(failed)) => {
                self.set(
                    failed_code(&failed.reason),
                    Some(failed.capture_id).filter(|id| canonical_uuid(id)),
                    0,
                );
                return Gate {
                    writes: true,
                    import: false,
                };
            }
            Ok(None) => {}
            Err(_) => {
                self.set("draft_capture_private_slot", None, 0);
                return refuse;
            }
        }
        let mut resumed = None;
        let seed = match read_json::<Seed>(&slot.join("identity.json")) {
            Ok(Some(seed)) if seed.valid() => {
                // A seed with no outcome: the attempt died inside its job.
                // Anything in Next's store newer than the seed was written
                // by a run that did not capture first.
                resumed = Some(
                    fs::metadata(slot.join("identity.json"))
                        .and_then(|meta| meta.modified())
                        .unwrap_or(UNIX_EPOCH),
                );
                seed
            }
            Ok(Some(_)) | Err(_) => {
                self.set("draft_capture_private_slot", None, 0);
                return refuse;
            }
            Ok(None) => {
                let seed = Seed {
                    version: 1,
                    boundary: BOUNDARY.into(),
                    capture_id: uuid::Uuid::new_v4().to_string(),
                };
                if create_json(&slot.join("identity.json"), &seed).is_err() {
                    self.set("draft_capture_private_slot", None, 0);
                    return refuse;
                }
                seed
            }
        };
        let id = Some(seed.capture_id.clone());
        if slot.join("receipt.json").exists() {
            return match Snapshot::open(&slot, &seed) {
                Ok(snapshot) => {
                    self.set(
                        captured_code(&snapshot.index),
                        id,
                        snapshot.index.entries.len(),
                    );
                    Gate {
                        writes: true,
                        import: true,
                    }
                }
                Err(_) => {
                    self.set("draft_capture_changed", id, 0);
                    Gate {
                        writes: true,
                        import: false,
                    }
                }
            };
        }
        match self.capture(&slot, &seed, resumed) {
            Ok(index) => {
                self.set(captured_code(&index), id, index.entries.len());
                Gate {
                    writes: true,
                    import: true,
                }
            }
            Err(error) => {
                let reason = reason(error);
                let failed = Failed {
                    version: 1,
                    capture_id: seed.capture_id.clone(),
                    reason: reason.into(),
                };
                if create_json(&slot.join("failed.json"), &failed).is_err() {
                    self.set("draft_capture_private_slot", id, 0);
                    return refuse;
                }
                self.set(failed_code(reason), id, 0);
                Gate {
                    writes: true,
                    import: false,
                }
            }
        }
    }

    fn roots(&self) -> Vec<(Store, u32, &Path)> {
        let mut roots = vec![(Store::Next, 0, self.next.as_path())];
        roots.extend(
            self.dev
                .iter()
                .enumerate()
                .map(|(n, root)| (Store::Dev, n as u32, root.as_path())),
        );
        roots.extend(self.qt.iter().map(|root| (Store::Qt, 0, root.as_path())));
        roots
    }

    fn capture(
        &self,
        slot: &Path,
        seed: &Seed,
        resumed: Option<SystemTime>,
    ) -> SourceResult<Index> {
        let (index, container) = scan(&self.roots(), &seed.capture_id, resumed)?;
        let index_bytes = serde_json::to_vec(&index).map_err(|_| SourceError::InvalidMetadata)?;
        if index_bytes.len() > INDEX {
            return Err(SourceError::TooLarge);
        }
        // A crashed attempt's leaves are kept, never replaced: an attempt
        // that sees other originals than the crashed one fails instead.
        let index_stamp = keep_or_create(&slot.join("index.json"), &index_bytes, INDEX)?;
        let container_stamp = if container.is_empty() {
            None
        } else {
            Some(keep_or_create(
                &slot.join("original.bin"),
                &container,
                MAX_BYTES,
            )?)
        };
        // The roots again, after the copy: the same originals, or none.
        let (again, _) = scan(&self.roots(), &seed.capture_id, resumed)?;
        if again != index {
            return Err(SourceError::SourceChanged);
        }
        for (leaf, limit) in [
            ("identity.json", META),
            ("index.json", INDEX),
            ("original.bin", MAX_BYTES),
        ] {
            let path = slot.join(leaf);
            let current = disk::read(&path, limit).map_err(unreadable)?;
            let synced = disk::durable(&path, limit).map_err(unreadable)?;
            if current != synced {
                return Err(SourceError::SourceChanged);
            }
        }
        let receipt = Receipt {
            capture_id: seed.capture_id.clone(),
            index_sha256: sha256(&index_bytes),
            index: index_stamp,
            container: container_stamp,
        };
        create_json(&slot.join("receipt.json"), &receipt)?;
        Ok(index)
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn captured_code(index: &Index) -> &'static str {
    if index.roots.iter().any(|root| root.earlier_run) {
        "draft_capture_next_earlier_run"
    } else if index.entries.is_empty() {
        "draft_capture_not_applicable"
    } else {
        "draft_capture_captured"
    }
}

fn reason(error: SourceError) -> &'static str {
    match error {
        SourceError::TooLarge => "too_large",
        SourceError::SourceChanged => "changed",
        _ => "unreadable",
    }
}

fn failed_code(reason: &str) -> &'static str {
    match reason {
        "too_large" => "draft_capture_too_large",
        "changed" => "draft_capture_changed",
        _ => "draft_capture_unreadable",
    }
}

fn unreadable(error: std::io::Error) -> SourceError {
    if error.kind() == std::io::ErrorKind::FileTooLarge {
        SourceError::TooLarge
    } else {
        SourceError::SourceUnreadable
    }
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> SourceResult<Option<T>> {
    match disk::read(path, META).map_err(unreadable)? {
        None => Ok(None),
        Some((bytes, _)) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| SourceError::InvalidMetadata),
    }
}

fn create_json(path: &Path, value: &impl Serialize) -> SourceResult<Stamp> {
    let bytes = serde_json::to_vec(value).map_err(|_| SourceError::InvalidMetadata)?;
    disk::create(path, &bytes).map_err(unreadable)
}

fn keep_or_create(path: &Path, bytes: &[u8], limit: usize) -> SourceResult<Stamp> {
    match disk::read(path, limit).map_err(unreadable)? {
        Some((saved, stamp)) if saved == bytes => Ok(stamp),
        Some(_) => Err(SourceError::SourceChanged),
        None => disk::create(path, bytes).map_err(unreadable),
    }
}

fn is_folder_name(name: &str) -> bool {
    name.len() == 64
        && name
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// One read of a manifest, a second when the first fails: the store's own
/// retrying reader rule (an external writer replaces files atomically). A
/// manifest is a regular file of at most `MANIFEST` bytes; a link is not one.
fn manifest(path: &Path) -> SourceResult<Option<Vec<u8>>> {
    fn once(path: &Path) -> SourceResult<Option<Vec<u8>>> {
        use std::io::Read;
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(SourceError::SourceUnreadable),
        };
        if !metadata.is_file() {
            return Err(SourceError::SourceUnreadable);
        }
        if metadata.len() > MANIFEST as u64 {
            return Err(SourceError::TooLarge);
        }
        let mut bytes = Vec::new();
        fs::File::open(path)
            .and_then(|file| file.take(MANIFEST as u64 + 1).read_to_end(&mut bytes))
            .map_err(|_| SourceError::SourceUnreadable)?;
        if bytes.len() > MANIFEST {
            return Err(SourceError::TooLarge);
        }
        Ok(Some(bytes))
    }
    match once(path) {
        Err(SourceError::SourceUnreadable) => once(path),
        result => result,
    }
}

/// The entries of every root in a fixed order (root, then folder name), with
/// the container of their exact bytes. Over any cap the whole capture is
/// too large: never a complete prefix.
/// Whether Next's store shows a run that wrote without capturing first: its
/// one-time import marker, or (`resumed`: a seed with no outcome) anything in
/// it modified after that seed. Bounded like the scan.
fn earlier_run(root: &Path, resumed: Option<SystemTime>) -> SourceResult<bool> {
    let marker = root.join(drafts::IMPORT_MARKER);
    if fs::symlink_metadata(&marker).is_ok() {
        return Ok(true);
    }
    let Some(seed) = resumed else {
        return Ok(false);
    };
    let newer = |path: &Path| {
        fs::symlink_metadata(path)
            .ok()
            .and_then(|meta| meta.modified().ok())
            .is_some_and(|modified| modified > seed)
    };
    if newer(root) {
        return Ok(true);
    }
    for (scanned, entry) in fs::read_dir(root)
        .map_err(|_| SourceError::SourceUnreadable)?
        .enumerate()
    {
        if scanned >= MAX_SCAN {
            return Err(SourceError::TooLarge);
        }
        let path = entry.map_err(|_| SourceError::SourceUnreadable)?.path();
        if newer(&path) || newer(&path.join("draft.json")) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn scan(
    roots: &[(Store, u32, &Path)],
    capture_id: &str,
    resumed: Option<SystemTime>,
) -> SourceResult<(Index, Vec<u8>)> {
    let mut index = Index {
        version: 1,
        format: FORMAT.into(),
        capture_id: capture_id.into(),
        roots: Vec::new(),
        entries: Vec::new(),
        container_bytes: 0,
        container_sha256: String::new(),
    };
    let mut container = Vec::new();
    for &(store, number, root) in roots {
        let listing = match fs::read_dir(root) {
            Ok(listing) => listing,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                index.roots.push(Root {
                    store,
                    root: number,
                    present: false,
                    earlier_run: false,
                });
                continue;
            }
            Err(_) => return Err(SourceError::SourceUnreadable),
        };
        let earlier = store == Store::Next && earlier_run(root, resumed)?;
        index.roots.push(Root {
            store,
            root: number,
            present: true,
            earlier_run: earlier,
        });
        if earlier {
            continue;
        }
        let mut folders = Vec::new();
        for (scanned, entry) in listing.enumerate() {
            if scanned >= MAX_SCAN {
                return Err(SourceError::TooLarge);
            }
            let entry = entry.map_err(|_| SourceError::SourceUnreadable)?;
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            let kind = entry
                .file_type()
                .map_err(|_| SourceError::SourceUnreadable)?;
            if kind.is_dir() && is_folder_name(&name) {
                folders.push(name);
            }
        }
        folders.sort();
        for folder in folders {
            if index.entries.len() >= MAX_MANIFESTS {
                return Err(SourceError::TooLarge);
            }
            let path = root.join(&folder);
            let bytes = match manifest(&path.join("draft.json")) {
                Ok(bytes) => bytes,
                Err(SourceError::TooLarge) => return Err(SourceError::TooLarge),
                Err(_) => {
                    index
                        .entries
                        .push(entry(store, number, folder, Class::Unreadable, &[], 0));
                    continue;
                }
            };
            let Some(bytes) = bytes else {
                index
                    .entries
                    .push(entry(store, number, folder, Class::NoManifest, &[], 0));
                continue;
            };
            if container.len() + bytes.len() > MAX_BYTES {
                return Err(SourceError::TooLarge);
            }
            let (class, blobs) = classify(store, &folder, &path, &bytes);
            let offset = container.len() as u64;
            container.extend_from_slice(&bytes);
            let mut item = entry(store, number, folder, class, &bytes, offset);
            item.blobs = blobs;
            index.entries.push(item);
        }
    }
    index.container_bytes = container.len() as u64;
    index.container_sha256 = sha256(&container);
    Ok((index, container))
}

fn entry(
    store: Store,
    root: u32,
    folder: String,
    class: Class,
    bytes: &[u8],
    offset: u64,
) -> Entry {
    Entry {
        store,
        root,
        folder,
        class,
        offset,
        size: bytes.len() as u64,
        sha256: sha256(bytes),
        blobs: Vec::new(),
    }
}

/// Supported only when the existing per-key checkpoint decoder accepts the
/// manifest under the key its folder is named after.
fn classify(store: Store, folder: &str, path: &Path, bytes: &[u8]) -> (Class, Vec<BlobRef>) {
    let key = serde_json::from_slice::<Value>(bytes)
        .ok()
        .and_then(|value| value.get("key")?.as_str().map(str::to_owned));
    let Some(key) = key.filter(|key| sha256(key.as_bytes()) == folder) else {
        return (Class::Unsupported, Vec::new());
    };
    let origin = match store {
        Store::Qt => sources::Origin::Qt,
        Store::Next | Store::Dev => sources::Origin::Next,
    };
    if sources::draft_checkpoint(origin, &key, bytes).is_err() {
        return (Class::Unsupported, Vec::new());
    }
    let Some(record) = drafts::decode_manifest(&key, bytes) else {
        return (Class::Unsupported, Vec::new());
    };
    let files = record
        .get("attachments")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let blobs = files
        .iter()
        .take(attachments::MAX_ATTACHMENTS)
        .filter_map(|file| {
            let sha256 = file.get("sha256")?.as_str()?.to_owned();
            let bytes = file.get("bytes")?.as_u64()?;
            let class = match fs::symlink_metadata(path.join(format!("{sha256}.bin"))) {
                Ok(m) if m.is_file() && m.len() == bytes => Blob::Present,
                Ok(m) if m.is_file() => Blob::SizeMismatch,
                Ok(_) => Blob::Unreadable,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Blob::Missing,
                Err(_) => Blob::Unreadable,
            };
            Some(BlobRef {
                sha256,
                bytes,
                class,
            })
        })
        .collect();
    (Class::Supported, blobs)
}

/// The captured version, reopened from its slot: the receipt binds the index
/// leaf, and the index binds the container.
pub struct Snapshot {
    pub index: Index,
    container: Vec<u8>,
}

impl Snapshot {
    fn open(slot: &Path, seed: &Seed) -> SourceResult<Self> {
        let receipt = read_json::<Receipt>(&slot.join("receipt.json"))?
            .ok_or(SourceError::InvalidMetadata)?;
        if receipt.capture_id != seed.capture_id {
            return Err(SourceError::InvalidMetadata);
        }
        let (bytes, stamp) = disk::read(&slot.join("index.json"), INDEX)
            .map_err(unreadable)?
            .ok_or(SourceError::SourceChanged)?;
        if stamp != receipt.index || sha256(&bytes) != receipt.index_sha256 {
            return Err(SourceError::SourceChanged);
        }
        let index: Index =
            serde_json::from_slice(&bytes).map_err(|_| SourceError::InvalidMetadata)?;
        let container = match &receipt.container {
            None => Vec::new(),
            Some(expected) => {
                let (bytes, stamp) = disk::read(&slot.join("original.bin"), MAX_BYTES)
                    .map_err(unreadable)?
                    .ok_or(SourceError::SourceChanged)?;
                if &stamp != expected {
                    return Err(SourceError::SourceChanged);
                }
                bytes
            }
        };
        if index.capture_id != seed.capture_id
            || index.format != FORMAT
            || index.container_bytes != container.len() as u64
            || index.container_sha256 != sha256(&container)
        {
            return Err(SourceError::SourceChanged);
        }
        Ok(Self { index, container })
    }

    /// The exact original bytes of one entry.
    pub fn bytes(&self, entry: &Entry) -> &[u8] {
        let start = entry.offset as usize;
        &self.container[start..start + entry.size as usize]
    }
}

#[cfg(test)]
mod tests;
