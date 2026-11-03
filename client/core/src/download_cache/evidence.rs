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
    stamp: Option<Stamp>,
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

fn slot(canonical: &Path, object: &str, birth: Option<&str>) -> PathBuf {
    let key = format!("{object}\0{}", birth.unwrap_or("-"));
    canonical
        .join(DIR)
        .join(hex(&Sha256::digest(key.as_bytes())))
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
        if !slot.exists() {
            let count = fs::read_dir(&slots)?
                .take(MAX_SLOTS + 1)
                .filter(|entry| entry.as_ref().is_ok_and(|e| e.path().is_dir()))
                .count();
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
        if seed.version != 1
            || seed.boundary != BOUNDARY
            || !zerus_desktop_migration::canonical_uuid(&seed.capture_id)
            || !digest(&seed.directory)
            || seed.directory != object
            || seed.birth != birth
        {
            return Err(refused());
        }
        if let Some(receipt) = read_json::<Original>(&slot.join("receipt.json"))? {
            // An earlier first change already recorded this folder: final.
            return if receipt.capture_id == seed.capture_id {
                Ok(())
            } else {
                Err(refused())
            };
        }
        // No receipt, so no change followed any earlier attempt: its leftover
        // copy is replaced by this load's version.
        remove(&slot.join("original.bin"))?;
        let (kind, bytes, sha256, stamp) = match &self.index {
            None => (Index::Absent, 0, None, None),
            Some(index) => {
                if index.len() > ORIGINAL {
                    return Err(refused());
                }
                let stamp = disk::create(&slot.join("original.bin"), index)?;
                (
                    Index::Present,
                    index.len() as u64,
                    Some(hex(&Sha256::digest(index))),
                    Some(stamp),
                )
            }
        };
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
            },
        )
    }
}

/// The locked load's phase, read-only: the folder and the index exactly as
/// the ledger loaded it (`None`: no index, verified under the lock).
pub(crate) fn prepare(root: &Path, index: Option<&[u8]>) -> Result<Pending, String> {
    let (canonical, object, birth) = folder(root).map_err(|_| NOT_RECORDED.to_owned())?;
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
    Unreadable,
}

/// What the folder at `root` holds for its current directory object.
pub fn evidence(root: &Path) -> Evidence {
    let Ok((canonical, object, birth)) = folder(root) else {
        return Evidence::Unreadable;
    };
    let slot = slot(&canonical, &object, birth.as_deref());
    if !slot.exists() {
        return Evidence::None;
    }
    match Snapshot::open(&slot) {
        Ok(snapshot) => Evidence::Captured(Box::new(snapshot.receipt)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Evidence::None,
        Err(_) => Evidence::Unreadable,
    }
}

impl Snapshot {
    pub fn open(slot: &Path) -> io::Result<Self> {
        let seed: Seed =
            read_json(&slot.join("identity.json"))?.ok_or(io::ErrorKind::InvalidData)?;
        let receipt: Original =
            read_json(&slot.join("receipt.json"))?.ok_or(io::ErrorKind::NotFound)?;
        if receipt.capture_id != seed.capture_id
            || receipt.format != FORMAT
            || receipt.boundary != BOUNDARY
            || receipt.directory != seed.directory
        {
            return Err(refused());
        }
        let index = match (
            receipt.index,
            disk::read(&slot.join("original.bin"), ORIGINAL)?,
        ) {
            (Index::Absent, None) => None,
            (Index::Present, Some((bytes, stamp)))
                if receipt.stamp.as_ref() == Some(&stamp)
                    && bytes.len() as u64 == receipt.bytes
                    && receipt.sha256.as_deref() == Some(hex(&Sha256::digest(&bytes)).as_str()) =>
            {
                Some(bytes)
            }
            _ => return Err(refused()),
        };
        Ok(Self { receipt, index })
    }
}

#[cfg(test)]
mod tests;
