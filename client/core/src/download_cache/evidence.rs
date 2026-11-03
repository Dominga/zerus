//! The folder's original index, captured for G3 (zerus-5wmh.66.5.2.1.3)
//! before Next's first change to it: evidence only, never a download, a
//! removal, a send or cleanup consent.
//!
//! Every Next writer of the index (the client's downloads and trims, and the
//! service's cache worker) reaches the same slot in the folder itself,
//! `.zerus-files-preload-v1/<directory object and birth>/`, under the folder's
//! Qt lock. Two phases share that one lock: the locked load keeps the exact
//! bytes the ledger loaded in memory and writes nothing ([`prepare`]; a
//! service preparation stays read-only), and the first effect after it, in an
//! admitted execution, records them with their receipt ([`Pending::commit`]).
//! So the receipt names the version Next's first change replaced
//! (`next-download-index-before-first-write-v1`). Qt shares the folder and
//! its lock; its own earlier writes belong to that version.
//!
//! A committed receipt is never overwritten or upgraded. A failure refuses
//! that one change before any effect; nothing is recorded as failed, so the
//! next change captures again. A replaced or copied folder (another directory
//! object or birth) has another slot and never inherits an old one. Only
//! index bytes are kept: no downloaded content.
use super::{hex, maintenance, INDEX};
use crate::fsx::preload::{self as disk, Stamp};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs, io,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

pub const BOUNDARY: &str = "next-download-index-before-first-write-v1";
/// The maintained production Next (the switch build): the application whose
/// cache changes record evidence. Development and replay builds never do.
pub const MAINTAINED_APPLICATION: &str = "com.hgdev.hgs-tray";
pub const FORMAT: &str = "download-index-original-v1";
/// The evidence folder inside the cache folder (one of Zerus's own names).
pub const DIR: &str = ".zerus-files-preload-v1";
/// Folder objects one cache folder keeps evidence for; past it, refused.
pub const MAX_SLOTS: usize = 16;
const META: usize = 64 << 10;
const ORIGINAL: usize = super::MAX_INDEX_BYTES as usize;
/// The largest leaf the private reader accepts on every platform (Windows
/// refuses larger read limits): the original is kept in parts this size.
const PART: usize = 16 << 20;

/// The original's leaf for part `n`.
fn part(n: usize) -> String {
    if n == 0 {
        "original.bin".into()
    } else {
        format!("original-{n}.bin")
    }
}

/// The exact size of each part of a `bytes`-long original.
fn part_sizes(bytes: u64) -> Vec<usize> {
    let bytes = bytes as usize;
    (0..bytes.div_ceil(PART))
        .map(|n| PART.min(bytes - n * PART))
        .collect()
}

/// A change refused because the folder's original index could not be
/// recorded first. Nothing in the folder was changed.
pub const NOT_RECORDED: &str = "The downloaded files folder is unchanged: its earlier index could not be recorded first. Check that the folder is available and private to your user, then retry.";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Seed {
    version: u32,
    boundary: String,
    capture_id: String,
    /// The folder's canonical path when first seen; a move keeps the slot.
    root: String,
    directory: String,
    birth: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Index {
    /// No index in the folder, verified under its lock.
    Absent,
    Present,
}

/// The receipt of one folder's original index, committed before the first change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Original {
    pub version: u32,
    pub format: String,
    pub boundary: String,
    pub capture_id: String,
    pub directory: String,
    pub index: Index,
    pub bytes: u64,
    pub sha256: Option<String>,
    /// The index file's object, size and times as loaded.
    pub identity: Option<String>,
    /// The first part's stamp (`original.bin`).
    stamp: Option<Stamp>,
    /// The further parts' stamps (`original-1.bin`, ...).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    more: Vec<Stamp>,
}

impl Seed {
    /// The seed's own invariants: version 1, this boundary, a canonical
    /// capture ID and a directory object digest.
    fn valid(&self) -> bool {
        self.version == 1
            && self.boundary == BOUNDARY
            && zerus_desktop_migration::canonical_uuid(&self.capture_id)
            && digest(&self.directory)
    }

    /// A valid seed of this very folder object: the folder hash only
    /// locates the slot.
    fn binds(&self, object: &str, birth: Option<&str>) -> bool {
        self.valid() && self.directory == object && self.birth.as_deref() == birth
    }
}

impl Original {
    /// The receipt's own invariants against its slot's seed: the version,
    /// format and boundary, the seed's identity and directory object, and a
    /// consistent index kind.
    fn valid(&self, seed: &Seed) -> bool {
        self.version == 1
            && self.format == FORMAT
            && self.boundary == BOUNDARY
            && self.capture_id == seed.capture_id
            && self.directory == seed.directory
            && digest(&self.directory)
            && self.identity.as_deref().is_none_or(digest)
            && match self.index {
                Index::Absent => {
                    self.bytes == 0
                        && self.sha256.is_none()
                        && self.identity.is_none()
                        && self.stamp.is_none()
                        && self.more.is_empty()
                }
                Index::Present => {
                    self.bytes > 0
                        && self.bytes <= ORIGINAL as u64
                        && self.sha256.as_deref().is_some_and(digest)
                        && self.stamp.is_some()
                        // Each recorded part is exactly its share of the
                        // recorded length: the length binds the stamps.
                        && self
                            .stamp
                            .iter()
                            .chain(&self.more)
                            .map(Stamp::size)
                            .eq(part_sizes(self.bytes).into_iter().map(|size| size as u64))
                }
            }
    }

    /// The original's parts as the receipt describes them, by a private
    /// no-follow stat only (no read, no hash): each part's recorded stamp
    /// (object, size and modification time) on a single-link owner-only
    /// file, and no part beyond them; none for an absent index.
    fn leaf_matches(&self, slot: &Path) -> bool {
        let expected: Vec<&Stamp> = self.stamp.iter().chain(&self.more).collect();
        let matches = expected.iter().enumerate().all(|(n, stamp)| {
            disk::stat(&slot.join(part(n))).is_ok_and(|actual| actual.as_ref() == Some(*stamp))
        });
        matches && disk::stat(&slot.join(part(expected.len()))).is_ok_and(|extra| extra.is_none())
    }
}

fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn refused() -> io::Error {
    io::Error::other("refused")
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> io::Result<Option<T>> {
    match disk::read(path, META)? {
        None => Ok(None),
        Some((bytes, _)) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| refused()),
    }
}

fn create_json(path: &Path, value: &impl Serialize) -> io::Result<Stamp> {
    disk::create(path, &serde_json::to_vec(value).map_err(|_| refused())?)
}

/// Publishes a final leaf whole: a fresh `<name>.next`, then a rename, so a
/// crash leaves the leaf absent or complete, never partial.
fn publish(slot: &Path, name: &str, value: &impl Serialize) -> io::Result<()> {
    let next = slot.join(format!("{name}.next"));
    remove(&next)?;
    create_json(&next, value)?;
    fs::rename(&next, slot.join(name))?;
    #[cfg(unix)]
    crate::fsx::checkpoint::sync_parent(&slot.join(name))?;
    Ok(())
}

fn remove(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// The canonical folder, its directory object and its birth time (where the
/// platform keeps one), from the very folder opened.
fn folder(root: &Path) -> io::Result<(PathBuf, String, Option<String>)> {
    let canonical = fs::canonicalize(root)?;
    if canonical
        .to_str()
        .is_none_or(|text| text.len() > 8192 || text.chars().any(char::is_control))
    {
        return Err(refused());
    }
    let directory = maintenance::open(&canonical, true).map_err(|_| refused())?;
    let (object, _) = maintenance::identities(&directory, true).map_err(|_| refused())?;
    let birth = directory
        .metadata()?
        .created()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|since| since.as_nanos().to_string());
    Ok((canonical, object, birth))
}

/// A slot whose seed is verified absent: the writer made it and stopped
/// before publishing anything, a prefix it recovers from. That is "no
/// evidence" only if the receipt is verified absent too; anything else stays
/// fenced as Unreadable.
fn unpublished(slot: &Path) -> Evidence {
    match missing(&slot.join("receipt.json")) {
        Ok(true) => Evidence::None,
        _ => Evidence::Unreadable,
    }
}

/// Whether `path` is verified missing: only NotFound says so. Any other
/// error (a permission, an unreadable parent) is an error, never "missing".
fn missing(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(false),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(error),
    }
}

fn slot(canonical: &Path, object: &str, birth: Option<&str>) -> PathBuf {
    let key = format!("{object}\0{}", birth.unwrap_or("-"));
    canonical
        .join(DIR)
        .join(hex(&Sha256::digest(key.as_bytes())))
}

/// Where a folder is for the private slot, mirroring the gates of
/// `fsx::preload::Parents` on Windows: a local fixed or removable drive, a
/// verified location the slot never uses (a network share, or a network,
/// optical or RAM drive), or a location that could not be classified.
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Place {
    Local,
    /// Verified not capturable: the change proceeds, recording nothing, and
    /// the folder is never reported captured.
    Unsupported,
    /// Not classified: the change is refused like any capture failure.
    Unknown,
}

/// The kind of volume a path starts with.
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Volume {
    /// A drive letter, plain or verbatim.
    Drive,
    /// A UNC share, plain or verbatim.
    Share,
    /// Any other prefix, such as a device namespace.
    Other,
}

#[cfg_attr(not(windows), allow(dead_code))]
/// Pure classification: `drive` is a drive root's `GetDriveTypeW` result.
pub(crate) fn classify(volume: Volume, drive: Option<u32>) -> Place {
    match (volume, drive) {
        // DRIVE_REMOVABLE, DRIVE_FIXED: the private slot's own gate.
        (Volume::Drive, Some(2 | 3)) => Place::Local,
        // DRIVE_REMOTE, DRIVE_CDROM, DRIVE_RAMDISK: verified elsewhere.
        (Volume::Drive, Some(4..=6)) | (Volume::Share, _) => Place::Unsupported,
        // DRIVE_UNKNOWN, DRIVE_NO_ROOT_DIR, no answer, other prefixes.
        _ => Place::Unknown,
    }
}

#[cfg(windows)]
fn place(canonical: &Path) -> Place {
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Component, Prefix};
    let Some(Component::Prefix(prefix)) = canonical.components().next() else {
        return Place::Unknown;
    };
    match prefix.kind() {
        Prefix::Disk(_) | Prefix::VerbatimDisk(_) => {
            let root: PathBuf = canonical.components().take(2).collect();
            let wide: Vec<u16> = root.as_os_str().encode_wide().chain(Some(0)).collect();
            // SAFETY: a NUL-terminated wide path for the duration of the call.
            let kind =
                unsafe { windows_sys::Win32::Storage::FileSystem::GetDriveTypeW(wide.as_ptr()) };
            classify(Volume::Drive, Some(kind))
        }
        Prefix::UNC(..) | Prefix::VerbatimUNC(..) => classify(Volume::Share, None),
        _ => classify(Volume::Other, None),
    }
}

/// Other platforms have no such gate: every folder is local to the slot.
#[cfg(not(windows))]
fn place(_: &Path) -> Place {
    Place::Local
}

/// The original index as one writer's locked load saw it, held in memory:
/// the load writes nothing (a service preparation stays read-only). The
/// first effect under the same lock commits it ([`Pending::commit`]).
pub(crate) struct Pending {
    root: PathBuf,
    object: String,
    birth: Option<String>,
    index: Option<Vec<u8>>,
    identity: Option<String>,
    /// A verified location the slot never uses: nothing is recorded.
    unsupported: bool,
    committed: bool,
    refused: bool,
}

impl Pending {
    /// Before the first effect: records this original in the folder's slot,
    /// unless its receipt is already there. Called again, it does nothing;
    /// after a failure it keeps refusing, so no effect of this load proceeds
    /// without it.
    pub(crate) fn commit(&mut self) -> Result<(), String> {
        if self.refused {
            return Err(NOT_RECORDED.into());
        }
        if self.unsupported {
            return Ok(());
        }
        if !self.committed {
            if self.record().is_err() {
                self.refused = true;
                return Err(NOT_RECORDED.into());
            }
            self.committed = true;
        }
        Ok(())
    }

    fn record(&self) -> io::Result<()> {
        // Still the folder the load saw.
        let (canonical, object, birth) = folder(&self.root)?;
        if canonical != self.root || object != self.object || birth != self.birth {
            return Err(refused());
        }
        let slots = canonical.join(DIR);
        let slot = slot(&canonical, &object, birth.as_deref());
        let _parents = disk::Parents::open(&slots, true)?;
        if missing(&slot)? {
            // Every raw entry counts, whatever it is, and an unreadable one
            // refuses: a bounded listing that may be incomplete never admits
            // another slot.
            let mut count = 0;
            for entry in fs::read_dir(&slots)?.take(MAX_SLOTS) {
                entry?;
                count += 1;
            }
            if count >= MAX_SLOTS {
                return Err(refused());
            }
        }
        drop(disk::Parents::open(&slot, true)?);
        let _lock = disk::Lock::take(&slot.join("capture.lock"))?;
        let seed = match read_json::<Seed>(&slot.join("identity.json"))? {
            Some(seed) => seed,
            None => {
                let seed = Seed {
                    version: 1,
                    boundary: BOUNDARY.into(),
                    capture_id: uuid::Uuid::new_v4().to_string(),
                    root: canonical.to_string_lossy().into_owned(),
                    directory: object.clone(),
                    birth: birth.clone(),
                };
                publish(&slot, "identity.json", &seed)?;
                seed
            }
        };
        if !seed.binds(&object, birth.as_deref()) {
            return Err(refused());
        }
        if let Some(receipt) = read_json::<Original>(&slot.join("receipt.json"))? {
            // An earlier first change already recorded this folder: final,
            // and only if it is exactly a receipt for this slot. Anything
            // else is refused, never overwritten or recreated.
            return if receipt.valid(&seed) && receipt.leaf_matches(&slot) {
                Ok(())
            } else {
                Err(refused())
            };
        }
        #[cfg(test)]
        DURING_COMMIT.with(|hook| {
            if let Some(hook) = hook.borrow_mut().as_mut() {
                hook();
            }
        });
        // No receipt, so no change followed any earlier attempt: its leftover
        // copy is replaced by this load's version.
        for n in 0..ORIGINAL.div_ceil(PART) {
            remove(&slot.join(part(n)))?;
        }
        let (kind, bytes, sha256, stamps) = match &self.index {
            None => (Index::Absent, 0, None, Vec::new()),
            Some(index) => {
                if index.len() > ORIGINAL {
                    return Err(refused());
                }
                // Each part created and read back with its own exact size.
                let mut stamps = Vec::new();
                for (n, bytes) in index.chunks(PART).enumerate() {
                    stamps.push(disk::create(&slot.join(part(n)), bytes)?);
                }
                (
                    Index::Present,
                    index.len() as u64,
                    Some(hex(&Sha256::digest(index))),
                    stamps,
                )
            }
        };
        let mut stamps = stamps.into_iter();
        let (stamp, more) = (stamps.next(), stamps.collect());
        publish(
            &slot,
            "receipt.json",
            &Original {
                version: 1,
                format: FORMAT.into(),
                boundary: BOUNDARY.into(),
                capture_id: seed.capture_id,
                directory: object,
                index: kind,
                bytes,
                sha256,
                identity: self.identity.clone(),
                stamp,
                more,
            },
        )
    }
}

#[cfg(test)]
thread_local! {
    /// Runs inside a commit on this thread: what a slow capture lets happen.
    pub(crate) static DURING_COMMIT: std::cell::RefCell<Option<Box<dyn FnMut()>>> =
        const { std::cell::RefCell::new(None) };
}

/// The locked load's phase, read-only: the folder and the index exactly as
/// the ledger loaded it (`None`: no index, verified under the lock).
pub(crate) fn prepare(root: &Path, index: Option<&[u8]>) -> Result<Pending, String> {
    let canonical = fs::canonicalize(root).map_err(|_| NOT_RECORDED.to_owned())?;
    prepare_at(&canonical, index, place(&canonical))
}

/// [`prepare`] for a folder already classified.
pub(crate) fn prepare_at(
    canonical: &Path,
    index: Option<&[u8]>,
    place: Place,
) -> Result<Pending, String> {
    match place {
        Place::Local => {}
        Place::Unknown => return Err(NOT_RECORDED.into()),
        Place::Unsupported => {
            return Ok(Pending {
                root: canonical.into(),
                object: String::new(),
                birth: None,
                index: None,
                identity: None,
                unsupported: true,
                committed: false,
                refused: false,
            })
        }
    }
    let (canonical, object, birth) = folder(canonical).map_err(|_| NOT_RECORDED.to_owned())?;
    if index.is_some_and(|index| index.len() > ORIGINAL) {
        return Err(NOT_RECORDED.into());
    }
    let identity = index.and_then(|_| {
        maintenance::open(&canonical.join(INDEX), false)
            .and_then(|file| maintenance::identities(&file, false))
            .ok()
            .map(|(_, identity)| identity)
    });
    Ok(Pending {
        root: canonical,
        object,
        birth,
        index: index.map(<[u8]>::to_vec),
        identity,
        unsupported: false,
        committed: false,
        refused: false,
    })
}

/// A folder's committed original, checked against its own receipt only,
/// never against the folder's current index.
pub struct Snapshot {
    pub receipt: Original,
    pub index: Option<Vec<u8>>,
}

/// The current folder's evidence, read without changing anything.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Evidence {
    /// No receipt for this folder object: Next has not changed it.
    None,
    Captured(Box<Original>),
    /// A verified location the private slot never uses: not capturable.
    Unsupported,
    Unreadable,
}

/// A folder's committed original with the folder it binds, for publication:
/// the receipt and exact bytes from [`Snapshot::open`], bound to the current
/// folder object. `Ok(None)`: no receipt (Next has not changed the folder).
pub struct Captured {
    /// The folder's canonical path now.
    pub folder: PathBuf,
    /// The folder object and birth the slot's seed binds.
    pub directory: String,
    pub birth: Option<String>,
    /// The receipt file's exact bytes and the snapshot they describe.
    pub receipt: Vec<u8>,
    pub snapshot: Snapshot,
}

/// The folder at `root`'s committed original, read in full and checked
/// against its own receipt. An unsupported or unreadable folder is an error
/// naming that state: never published as complete.
pub fn captured(root: &Path) -> Result<Option<Captured>, Evidence> {
    if fs::symlink_metadata(root).is_err_and(|error| error.kind() == io::ErrorKind::NotFound) {
        return Ok(None);
    }
    let canonical = fs::canonicalize(root).map_err(|_| Evidence::Unreadable)?;
    match place(&canonical) {
        Place::Local => {}
        Place::Unsupported => return Err(Evidence::Unsupported),
        Place::Unknown => return Err(Evidence::Unreadable),
    }
    let (canonical, object, birth) = folder(&canonical).map_err(|_| Evidence::Unreadable)?;
    let slot = slot(&canonical, &object, birth.as_deref());
    if !slot.exists() {
        return Ok(None);
    }
    let receipt = match disk::read(&slot.join("receipt.json"), META) {
        Ok(None) => return Ok(None),
        Ok(Some((bytes, _))) => bytes,
        Err(_) => return Err(Evidence::Unreadable),
    };
    let snapshot =
        Snapshot::open(&slot, &object, birth.as_deref()).map_err(|_| Evidence::Unreadable)?;
    if serde_json::to_vec(&snapshot.receipt).ok().as_deref()
        != serde_json::from_slice::<Original>(&receipt)
            .ok()
            .and_then(|r| serde_json::to_vec(&r).ok())
            .as_deref()
    {
        return Err(Evidence::Unreadable);
    }
    Ok(Some(Captured {
        folder: canonical,
        directory: object,
        birth,
        receipt,
        snapshot,
    }))
}

/// [`evidence`] by bounded stats only, for a status shown often: the seed
/// and receipt (64 KiB each) and each part's stamp, never the original's
/// bytes. A receipt it reports Captured still passes every check a change
/// makes before relying on it.
pub fn status(root: &Path) -> Evidence {
    if fs::symlink_metadata(root).is_err_and(|error| error.kind() == io::ErrorKind::NotFound) {
        return Evidence::None;
    }
    let Ok(canonical) = fs::canonicalize(root) else {
        return Evidence::Unreadable;
    };
    match place(&canonical) {
        Place::Local => {}
        Place::Unsupported => return Evidence::Unsupported,
        Place::Unknown => return Evidence::Unreadable,
    }
    let Ok((canonical, object, birth)) = folder(&canonical) else {
        return Evidence::Unreadable;
    };
    let slot = slot(&canonical, &object, birth.as_deref());
    match missing(&slot) {
        Ok(true) => return Evidence::None,
        Ok(false) => {}
        Err(_) => return Evidence::Unreadable,
    }
    let seed = match read_json::<Seed>(&slot.join("identity.json")) {
        Ok(Some(seed)) if seed.binds(&object, birth.as_deref()) => seed,
        Ok(None) => return unpublished(&slot),
        _ => return Evidence::Unreadable,
    };
    match read_json::<Original>(&slot.join("receipt.json")) {
        Ok(None) => Evidence::None,
        Ok(Some(receipt)) if receipt.valid(&seed) && receipt.leaf_matches(&slot) => {
            Evidence::Captured(Box::new(receipt))
        }
        _ => Evidence::Unreadable,
    }
}

/// What the folder at `root` holds for its current directory object.
pub fn evidence(root: &Path) -> Evidence {
    if fs::symlink_metadata(root).is_err_and(|error| error.kind() == io::ErrorKind::NotFound) {
        return Evidence::None;
    }
    let Ok(canonical) = fs::canonicalize(root) else {
        return Evidence::Unreadable;
    };
    match place(&canonical) {
        Place::Local => {}
        Place::Unsupported => return Evidence::Unsupported,
        Place::Unknown => return Evidence::Unreadable,
    }
    let Ok((canonical, object, birth)) = folder(&canonical) else {
        return Evidence::Unreadable;
    };
    let slot = slot(&canonical, &object, birth.as_deref());
    match missing(&slot) {
        Ok(true) => return Evidence::None,
        Ok(false) => {}
        Err(_) => return Evidence::Unreadable,
    }
    match read_json::<Seed>(&slot.join("identity.json")) {
        Ok(Some(_)) => {}
        Ok(None) => return unpublished(&slot),
        Err(_) => return Evidence::Unreadable,
    }
    match Snapshot::open(&slot, &object, birth.as_deref()) {
        Ok(snapshot) => Evidence::Captured(Box::new(snapshot.receipt)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Evidence::None,
        Err(_) => Evidence::Unreadable,
    }
}

impl Snapshot {
    /// The slot's receipt and original, for the folder object `directory`
    /// born at `birth`: a valid seed of that very folder, never only a slot
    /// found under its hash.
    pub fn open(slot: &Path, directory: &str, birth: Option<&str>) -> io::Result<Self> {
        let seed: Seed =
            read_json(&slot.join("identity.json"))?.ok_or(io::ErrorKind::InvalidData)?;
        if !seed.binds(directory, birth) {
            return Err(refused());
        }
        let receipt: Original =
            read_json(&slot.join("receipt.json"))?.ok_or(io::ErrorKind::NotFound)?;
        if !receipt.valid(&seed) {
            return Err(refused());
        }
        let index = match receipt.index {
            Index::Absent => {
                if disk::stat(&slot.join(part(0)))?.is_some() {
                    return Err(refused());
                }
                None
            }
            Index::Present => {
                // Each part read with its own exact size, against its stamp.
                let expected = receipt.stamp.iter().chain(&receipt.more);
                let mut bytes = Vec::new();
                for (n, (stamp, size)) in expected.zip(part_sizes(receipt.bytes)).enumerate() {
                    let (data, actual) =
                        disk::read(&slot.join(part(n)), size)?.ok_or_else(refused)?;
                    if &actual != stamp || data.len() != size {
                        return Err(refused());
                    }
                    bytes.extend_from_slice(&data);
                }
                if bytes.len() as u64 != receipt.bytes
                    || receipt.sha256.as_deref() != Some(hex(&Sha256::digest(&bytes)).as_str())
                {
                    return Err(refused());
                }
                Some(bytes)
            }
        };
        Ok(Self { receipt, index })
    }
}

#[cfg(test)]
mod tests;
