//! Historical original Next creation bytes, captured by its sole native queue
//! owner BEFORE load/normalization. This is not a live journal checkpoint, a
//! drain receipt, consent or a source of effects. Qt/Settings keep their leases.
use super::{sources, SourceError, SourceResult, SourceStream};
use crate::fsx::preload::{self as disk, Stamp};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, PoisonError,
    },
};
use zerus_desktop_migration::{canonical_uuid, sha256};
const META: usize = 64 << 10;
const RAW: usize = crate::creation::JOURNAL_LIMIT;
const BOUNDARY: &str = "next-creations-before-first-load-v1";

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
struct Plan {
    seed: Seed,
    source: Option<Stamp>,
    bytes: usize,
    sha256: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    plan: Plan,
    snapshot: Option<Stamp>,
}

/// Managed native setup supplies only the original application data directory.
/// No IPC/JS caller selects a path, capture identity or raw bytes.
pub struct Capture {
    data: PathBuf,
    state: Mutex<Status>,
    retry: AtomicBool,
    preparing: Mutex<()>,
    notify: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Status {
    pub code: &'static str,
    pub paused: bool,
    pub capture_id: Option<String>,
}
impl Default for Status {
    fn default() -> Self {
        Self {
            code: "creation_source_waiting",
            paused: true,
            capture_id: None,
        }
    }
}
impl Capture {
    pub fn new(data: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            data,
            state: Mutex::new(Status::default()),
            retry: AtomicBool::new(true),
            preparing: Mutex::new(()),
            notify: Mutex::new(None),
        })
    }
    pub fn status(&self) -> Status {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
    pub fn hook(&self, notify: Arc<dyn Fn() + Send + Sync>) {
        *self.notify.lock().unwrap_or_else(PoisonError::into_inner) = Some(notify);
    }
    pub fn request_retry(&self) {
        self.retry.store(true, Ordering::Release);
    }
    pub fn take_retry(&self) -> bool {
        self.retry.swap(false, Ordering::AcqRel)
    }
    /// Called only on the ORIGINAL queue worker, before any legacy mutation.
    /// I/O is finite (8 MiB raw, fixed metadata leaves, no listing or helper).
    pub fn prepare(&self) -> SourceResult<Prepared> {
        let _operation = self
            .preparing
            .try_lock()
            .map_err(|_| SourceError::SourceUnreadable)?;
        let result = self.prepare_inner();
        let mut status = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        status.paused = result.is_err();
        status.code = match &result {
            Ok(Prepared::Missing { .. }) => "creation_source_not_applicable",
            Ok(Prepared::Snapshot(_)) => "creation_source_captured",
            Err(SourceError::SourceChanged) => "creation_source_changed",
            Err(SourceError::TooLarge) => "creation_source_too_large",
            Err(SourceError::UnsupportedSource) => "creation_source_unsupported",
            Err(_) => "creation_source_unavailable",
        };
        drop(status);
        let notify = self
            .notify
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(notify) = notify {
            notify();
        }
        result
    }
    fn prepare_inner(&self) -> SourceResult<Prepared> {
        if !self.data.is_absolute() {
            return Err(SourceError::InvalidMetadata);
        }
        let root = self.data.join("creation-preload-v1");
        let _parents = disk::Parents::open(&root, true).map_err(unreadable)?;
        let _lock = disk::Lock::take(&root.join("capture.lock")).map_err(unreadable)?;
        let seed: Seed = match read_json(&root.join("identity.json"))? {
            Some((value, _)) => value,
            None => {
                let value = Seed {
                    version: 1,
                    boundary: BOUNDARY.into(),
                    capture_id: uuid::Uuid::new_v4().to_string(),
                };
                create_json(&root.join("identity.json"), &value)?;
                value
            }
        };
        if !seed.valid() {
            return Err(SourceError::InvalidMetadata);
        }
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .capture_id = Some(seed.capture_id.clone());
        let ready = root.join("receipt.json");
        if let Some((receipt, _)) = read_json::<Receipt>(&ready)? {
            return Snapshot::open(root, receipt, &seed);
        }
        let plan_path = root.join("source.json");
        let previous = read_json::<Plan>(&plan_path)?.map(|(p, _)| p);
        let source_path = self.data.join("creations.json");
        let source = disk::read(&source_path, RAW).map_err(unreadable)?;
        let (bytes, stamp) = match source {
            Some((b, s)) => (b, Some(s)),
            None => (Vec::new(), None),
        };
        let plan = Plan {
            seed: seed.clone(),
            source: stamp,
            bytes: bytes.len(),
            sha256: sha256(&bytes),
        };
        if let Some(previous) = previous {
            if previous != plan {
                return Err(SourceError::SourceChanged);
            }
        } else {
            create_json(&plan_path, &plan)?;
        }
        // A crashed copy never selects a newer version or replaces a partial
        // leaf. A fully written original leaf can be recognized by exact bytes.
        let snapshot = if plan.source.is_some() {
            let path = root.join("original.bin");
            let actual = match disk::read(&path, RAW).map_err(unreadable)? {
                Some((saved, identity)) => {
                    if saved != bytes {
                        return Err(SourceError::SourceChanged);
                    }
                    identity
                }
                None => disk::create(&path, &bytes).map_err(unreadable)?,
            };
            Some(actual)
        } else {
            None
        };
        // Reopen the source before releasing its original worker fence. This
        // verifies the source object/stamp/hash, not a normalized projection.
        let check = disk::read(&source_path, RAW).map_err(unreadable)?;
        if check.as_ref().map(|(_, s)| s) != plan.source.as_ref()
            || check
                .as_ref()
                .map(|(b, _)| sha256(b))
                .unwrap_or_else(|| sha256(&[]))
                != plan.sha256
        {
            return Err(SourceError::SourceChanged);
        }
        // An earlier successful byte write with an uncertain sync is not a
        // durable capture merely because a later read happens to see it.
        for (leaf, limit) in [
            ("identity.json", META),
            ("source.json", META),
            ("original.bin", RAW),
        ] {
            let current = disk::read(&root.join(leaf), limit).map_err(unreadable)?;
            let synced = disk::durable(&root.join(leaf), limit).map_err(unreadable)?;
            if current != synced {
                return Err(SourceError::SourceChanged);
            }
        }
        let receipt = Receipt { plan, snapshot };
        create_json(&ready, &receipt)?;
        Snapshot::open(root, receipt, &seed)
    }
}
fn unreadable(error: std::io::Error) -> SourceError {
    if error.kind() == std::io::ErrorKind::FileTooLarge {
        SourceError::TooLarge
    } else {
        SourceError::SourceUnreadable
    }
}
fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> SourceResult<Option<(T, Stamp)>> {
    disk::read(path, META)
        .map_err(unreadable)?
        .map(|(b, s)| {
            serde_json::from_slice(&b)
                .map(|v| (v, s))
                .map_err(|_| SourceError::InvalidMetadata)
        })
        .transpose()
}
fn create_json(path: &Path, value: &impl Serialize) -> SourceResult<()> {
    let bytes = serde_json::to_vec(value).map_err(|_| SourceError::InvalidMetadata)?;
    if bytes.len() > META {
        return Err(SourceError::TooLarge);
    }
    disk::create(path, &bytes).map_err(unreadable)?;
    Ok(())
}
pub enum Prepared {
    Missing { capture_id: String },
    Snapshot(Arc<Snapshot>),
}
/// An immutable historical version. The actual original source may now change.
/// Every later reader validates THIS retained object and hash afresh; it never
/// treats the live creation writer or an old status digest as a recheck.
pub struct Snapshot {
    root: PathBuf,
    receipt: Receipt,
    receipt_stamp: Stamp,
}
impl Snapshot {
    fn open(root: PathBuf, receipt: Receipt, seed: &Seed) -> SourceResult<Prepared> {
        if receipt.plan.seed != *seed || receipt.plan.bytes > RAW {
            return Err(SourceError::InvalidMetadata);
        }
        let (actual, receipt_stamp) = read_json::<Receipt>(&root.join("receipt.json"))?
            .ok_or(SourceError::SourceUnreadable)?;
        if actual != receipt {
            return Err(SourceError::SourceChanged);
        }
        // Even an earlier complete-looking receipt may have followed an
        // uncertain final sync. Validate and repeat only durability barriers,
        // never rewrite/adopt files or select a new capture identity.
        for (leaf, limit) in [
            ("identity.json", META),
            ("source.json", META),
            ("original.bin", RAW),
            ("receipt.json", META),
        ] {
            let before = disk::read(&root.join(leaf), limit).map_err(unreadable)?;
            let synced = disk::durable(&root.join(leaf), limit).map_err(unreadable)?;
            if before != synced {
                return Err(SourceError::SourceChanged);
            }
        }
        #[cfg(unix)]
        crate::fsx::checkpoint::sync_parent(&root.join("receipt.json")).map_err(unreadable)?;
        let snapshot = Self {
            root,
            receipt,
            receipt_stamp,
        };
        let bytes = snapshot.bytes()?;
        if snapshot.receipt.plan.source.is_none() {
            if !bytes.is_empty() || snapshot.receipt.snapshot.is_some() {
                return Err(SourceError::InvalidMetadata);
            }
            return Ok(Prepared::Missing {
                capture_id: seed.capture_id.clone(),
            });
        }
        // Decode for classification only. No Creations::load or effects.
        for item in sources::creations(sources::Origin::Next, &bytes)?.into_chunks() {
            item?;
        }
        Ok(Prepared::Snapshot(Arc::new(snapshot)))
    }
    pub fn capture_id(&self) -> &str {
        &self.receipt.plan.seed.capture_id
    }
    pub fn bytes(&self) -> SourceResult<Vec<u8>> {
        let (actual, stamp) = read_json::<Receipt>(&self.root.join("receipt.json"))?
            .ok_or(SourceError::SourceUnreadable)?;
        if actual != self.receipt || stamp != self.receipt_stamp {
            return Err(SourceError::SourceChanged);
        }
        let (seed, _) = read_json::<Seed>(&self.root.join("identity.json"))?
            .ok_or(SourceError::SourceUnreadable)?;
        let (plan, _) = read_json::<Plan>(&self.root.join("source.json"))?
            .ok_or(SourceError::SourceUnreadable)?;
        if seed != self.receipt.plan.seed || plan != self.receipt.plan {
            return Err(SourceError::SourceChanged);
        }
        let source = disk::read(&self.root.join("original.bin"), RAW).map_err(unreadable)?;
        let bytes = match (source, &self.receipt.snapshot) {
            (Some((bytes, stamp)), Some(expected)) if stamp == *expected => bytes,
            (None, None) if self.receipt.plan.source.is_none() => Vec::new(),
            _ => return Err(SourceError::SourceChanged),
        };
        if bytes.len() != self.receipt.plan.bytes || sha256(&bytes) != self.receipt.plan.sha256 {
            return Err(SourceError::SourceChanged);
        }
        Ok(bytes)
    }
    pub fn source(self: Arc<Self>) -> SourceResult<SourceStream> {
        let source = sources::creations(sources::Origin::Next, &self.bytes()?)?;
        Ok(source.recheck(move || self.bytes().map(|b| sha256(&b))))
    }
}

#[cfg(all(test, any(unix, windows)))]
mod tests {
    use super::*;
    use std::fs;
    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            let root = std::env::temp_dir()
                .join(format!("zerus-creation-preload-{}", uuid::Uuid::new_v4()));
            drop(disk::Parents::open(&root, true).unwrap());
            Self(root.canonicalize().unwrap())
        }
        fn source(&self, bytes: &[u8]) {
            crate::fsx::write_private_durable(&self.0.join("creations.json"), bytes).unwrap();
        }
        fn capture(&self) -> Arc<Capture> {
            Capture::new(self.0.clone())
        }
        fn snapshot(&self) -> Arc<Snapshot> {
            match self.capture().prepare().unwrap() {
                Prepared::Snapshot(s) => s,
                _ => panic!("missing"),
            }
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn raw() -> Vec<u8> {
        br#"{ "schema":1, "operations":{}, "opaque":"kept byte for byte" }"#.to_vec()
    }
    #[test]
    fn original_bytes_and_stable_identity_survive_later_live_recovery_and_restart() {
        let s = Scratch::new();
        let original = raw();
        s.source(&original);
        let first = s.snapshot();
        let id = first.capture_id().to_owned();
        assert_eq!(first.bytes().unwrap(), original);
        s.source(br#"{"schema":1,"operations":{},"normalized":true}"#);
        let restarted = s.snapshot();
        assert_eq!(restarted.capture_id(), id);
        assert_eq!(restarted.bytes().unwrap(), original);
        let source = first.source().unwrap();
        assert_eq!(source.identity().family, "next-creations");
        assert_eq!(source.identity().format, "creation-json-v1");
        for item in source.into_chunks() {
            item.unwrap();
        }
    }
    #[test]
    fn same_byte_snapshot_replacement_and_changed_receipt_refuse_fresh_recheck() {
        let s = Scratch::new();
        s.source(&raw());
        let first = s.snapshot();
        let path = s.0.join("creation-preload-v1/original.bin");
        fs::rename(&path, path.with_extension("old")).unwrap();
        crate::fsx::write_private_durable(&path, &raw()).unwrap();
        assert_eq!(first.bytes(), Err(SourceError::SourceChanged));
        assert!(s.capture().prepare().is_err());
    }
    #[test]
    fn verified_missing_is_not_an_empty_import_and_remains_that_original_version() {
        let s = Scratch::new();
        let capture = s.capture();
        let Prepared::Missing { capture_id } = capture.prepare().unwrap() else {
            panic!("not missing")
        };
        assert!(!capture.status().paused);
        assert_eq!(capture.status().code, "creation_source_not_applicable");
        s.source(&raw());
        let Prepared::Missing { capture_id: again } = s.capture().prepare().unwrap() else {
            panic!("adopted live file")
        };
        assert_eq!(again, capture_id);
        assert!(!s.0.join("creation-preload-v1/original.bin").exists());
    }
    #[test]
    fn damaged_partial_snapshot_is_kept_and_a_new_source_is_never_adopted() {
        let s = Scratch::new();
        s.source(&raw());
        let snapshot = s.snapshot();
        let root = s.0.join("creation-preload-v1");
        fs::remove_file(root.join("receipt.json")).unwrap();
        crate::fsx::write_private_durable(&root.join("original.bin"), b"partial").unwrap();
        let capture = s.capture();
        assert!(capture.prepare().is_err());
        let id = capture.status().capture_id.unwrap();
        assert_eq!(id, snapshot.capture_id());
        s.source(br#"{"schema":1,"operations":{},"new":1}"#);
        assert!(capture.prepare().is_err());
        assert_eq!(fs::read(root.join("original.bin")).unwrap(), b"partial");
        assert_eq!(capture.status().capture_id.as_deref(), Some(id.as_str()));
    }
    #[test]
    fn unsupported_source_is_durably_reviewable_and_never_replaced_with_an_empty_queue() {
        let s = Scratch::new();
        s.source(br#"{"schema":999,"operations":{}}"#);
        let capture = s.capture();
        assert!(matches!(
            capture.prepare(),
            Err(SourceError::UnsupportedSource)
        ));
        assert!(capture.status().paused);
        assert_eq!(
            fs::read(s.0.join("creation-preload-v1/original.bin")).unwrap(),
            br#"{"schema":999,"operations":{}}"#
        );
        s.source(&raw());
        assert!(capture.prepare().is_err());
    }
    #[test]
    fn oversized_original_remains_whole_and_paused_without_a_truncated_copy() {
        let s = Scratch::new();
        let original = vec![b' '; RAW + 1];
        s.source(&original);
        let capture = s.capture();
        assert!(matches!(capture.prepare(), Err(SourceError::TooLarge)));
        assert_eq!(capture.status().code, "creation_source_too_large");
        assert!(capture.status().paused);
        assert_eq!(
            fs::metadata(s.0.join("creations.json")).unwrap().len(),
            (RAW + 1) as u64
        );
        assert!(!s.0.join("creation-preload-v1/original.bin").exists());
        assert!(!s.0.join("creation-preload-v1/receipt.json").exists());
    }

    #[test]
    fn original_record_and_snapshot_are_fully_validated_before_ready() {
        let s = Scratch::new();
        s.source(&raw());
        let capture = s.capture();
        let root = s.0.join("creation-preload-v1");
        drop(disk::Parents::open(&root, true).unwrap());
        fs::write(root.join("identity.json"), b"bad").unwrap();
        assert!(capture.prepare().is_err());
        assert!(!root.join("receipt.json").exists());
        assert_eq!(fs::read(s.0.join("creations.json")).unwrap(), raw());
    }
    #[test]
    fn a_second_original_owner_cannot_capture_while_the_fixed_slot_is_held() {
        let s = Scratch::new();
        s.source(&raw());
        let root = s.0.join("creation-preload-v1");
        drop(disk::Parents::open(&root, true).unwrap());
        let held = disk::Lock::take(&root.join("capture.lock")).unwrap();
        let capture = s.capture();
        assert!(capture.prepare().is_err());
        assert!(capture.status().paused);
        assert!(!root.join("identity.json").exists());
        drop(held);
        assert!(capture.prepare().is_ok());
        assert!(!capture.status().paused);
    }
    #[cfg(unix)]
    #[test]
    fn source_symlinks_hardlinks_and_public_files_are_not_captured() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        for kind in 0..3 {
            let s = Scratch::new();
            let source = s.0.join("creations.json");
            let other = s.0.join("kept.json");
            crate::fsx::write_private_durable(&other, &raw()).unwrap();
            match kind {
                0 => symlink(&other, &source).unwrap(),
                1 => fs::hard_link(&other, &source).unwrap(),
                _ => {
                    s.source(&raw());
                    fs::set_permissions(&source, fs::Permissions::from_mode(0o644)).unwrap();
                }
            }
            let capture = s.capture();
            assert!(capture.prepare().is_err());
            assert!(capture.status().paused);
            assert!(!s.0.join("creation-preload-v1/receipt.json").exists());
            assert_eq!(fs::read(other).unwrap(), raw());
        }
    }
    #[cfg(windows)]
    #[test]
    fn native_archive_recheck_refuses_hardlinks_and_pins_parent_names_during_io() {
        let s = Scratch::new();
        s.source(&raw());
        let first = s.snapshot();
        let root = s.0.join("creation-preload-v1");
        let pins = disk::Parents::open(&root, false).unwrap();
        assert!(fs::rename(&root, s.0.join("moved")).is_err());
        crate::fsx::write_private_durable(&root.join("ordinary-child"), b"child").unwrap();
        drop(pins);
        fs::hard_link(root.join("original.bin"), root.join("alias.bin")).unwrap();
        assert!(first.bytes().is_err());
    }
}
