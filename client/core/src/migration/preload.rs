//! Original Next bytes captured by their sole native owner BEFORE first
//! service-mode load/normalization. Each source has a separate fixed historical
//! boundary and private slot. Neither is a live checkpoint or effect authority;
//! Qt/Settings keep their existing current-source leases.
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

pub mod local_reads;
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Creations,
    LocalReads,
}
impl Kind {
    fn boundary(self) -> &'static str {
        match self {
            Self::Creations => BOUNDARY,
            Self::LocalReads => "next-local-read-marks-before-first-load-v1",
        }
    }
    fn from_boundary(boundary: &str) -> Option<Self> {
        [Self::Creations, Self::LocalReads]
            .into_iter()
            .find(|kind| kind.boundary() == boundary)
    }
    fn slot(self) -> &'static str {
        match self {
            Self::Creations => "creation-preload-v1",
            Self::LocalReads => "local-read-preload-v1",
        }
    }
    fn leaf(self) -> &'static str {
        match self {
            Self::Creations => "creations.json",
            Self::LocalReads => "attention-marks.json",
        }
    }
    fn limit(self) -> usize {
        match self {
            Self::Creations => RAW,
            Self::LocalReads => 16 << 20,
        }
    }
    fn source(self, bytes: &[u8]) -> SourceResult<SourceStream> {
        match self {
            Self::Creations => sources::creations(sources::Origin::Next, bytes),
            Self::LocalReads => sources::next_local_read_marks(bytes),
        }
    }
    fn code(self, index: usize) -> &'static str {
        const CREATION: [&str; 11] = [
            "creation_source_waiting",
            "creation_source_not_applicable",
            "creation_source_captured",
            "creation_source_changed",
            "creation_source_too_large",
            "creation_source_unsupported",
            "creation_source_data_directory",
            "creation_source_private_slot",
            "creation_source_unavailable",
            "creation_source_writer_unavailable",
            "creation_source_owner_unavailable",
        ];
        const READS: [&str; 11] = [
            "local_read_source_waiting",
            "local_read_source_not_applicable",
            "local_read_source_captured",
            "local_read_source_changed",
            "local_read_source_too_large",
            "local_read_source_unsupported",
            "local_read_source_data_directory",
            "local_read_source_private_slot",
            "local_read_source_unavailable",
            "local_read_source_writer_unavailable",
            "local_read_source_owner_unavailable",
        ];
        match self {
            Self::Creations => CREATION[index],
            Self::LocalReads => READS[index],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Seed {
    version: u32,
    boundary: String,
    capture_id: String,
}
impl Seed {
    fn valid(&self, kind: Kind) -> bool {
        self.version == 1 && self.boundary == kind.boundary() && canonical_uuid(&self.capture_id)
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

enum CaptureFailure {
    Source(SourceError),
    #[cfg(unix)]
    DataDirectory,
    PrivateSlot,
}
impl From<SourceError> for CaptureFailure {
    fn from(source: SourceError) -> Self {
        Self::Source(source)
    }
}
impl CaptureFailure {
    fn source(self) -> SourceError {
        match self {
            Self::Source(source) => source,
            _ => SourceError::SourceUnreadable,
        }
    }
}
/// Managed native setup supplies only the original application data directory.
/// No IPC/JS caller selects a path, capture identity or raw bytes.
pub struct Capture {
    kind: Kind,
    data: PathBuf,
    #[cfg(unix)]
    resolved: std::sync::OnceLock<Arc<disk::ManagedData>>,
    state: Mutex<Status>,
    retry: AtomicBool,
    preparing: Mutex<()>,
    notify: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    retry_notify: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Status {
    pub code: &'static str,
    pub paused: bool,
    pub capture_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view_ready: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view_revision: Option<String>,
}
impl Default for Status {
    fn default() -> Self {
        Self {
            code: "creation_source_waiting",
            paused: true,
            capture_id: None,
            view_ready: None,
            view_revision: None,
        }
    }
}
impl Capture {
    pub fn new(data: PathBuf) -> Arc<Self> {
        Self::for_kind(data, Kind::Creations)
    }
    /// Only native service-mode setup selects this fixed managed family.
    pub fn local_reads(data: PathBuf) -> Arc<Self> {
        Self::for_kind(data, Kind::LocalReads)
    }
    fn for_kind(data: PathBuf, kind: Kind) -> Arc<Self> {
        Arc::new(Self {
            kind,
            data,
            #[cfg(unix)]
            resolved: std::sync::OnceLock::new(),
            state: Mutex::new(Status {
                code: kind.code(0),
                view_ready: (kind == Kind::LocalReads).then_some(false),
                view_revision: (kind == Kind::LocalReads).then(|| "0".into()),
                ..Status::default()
            }),
            retry: AtomicBool::new(true),
            preparing: Mutex::new(()),
            notify: Mutex::new(None),
            retry_notify: Mutex::new(None),
        })
    }
    pub fn status(&self) -> Status {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
    /// Original native setup's canonical data object, never a caller path.
    /// This accessor does not capture or initialize anything on the reporter.
    pub fn data_path(&self) -> SourceResult<PathBuf> {
        #[cfg(unix)]
        {
            let resolved = self.resolved.get().ok_or(SourceError::SourceUnreadable)?;
            resolved.recheck().map_err(|_| SourceError::SourceChanged)?;
            Ok(resolved.path().to_path_buf())
        }
        #[cfg(not(unix))]
        {
            if !self.data.is_absolute() {
                return Err(SourceError::InvalidMetadata);
            }
            Ok(self.data.clone())
        }
    }
    /// Use the same canonical directory for original legacy load AND writes.
    pub fn live_path(&self) -> SourceResult<PathBuf> {
        self.data_path()
            .map(|path| path.join(self.kind.leaf()))
            .inspect_err(|_| {
                let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
                state.paused = true;
                state.code = self.kind.code(3);
                drop(state);
                let notify = self
                    .notify
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clone();
                if let Some(notify) = notify {
                    notify();
                }
            })
    }
    /// Read ONLY the already committed historical slot. No mutable source read,
    /// capture, recovery, normalization or decoder/effect admission occurs here.
    pub fn retained(&self) -> SourceResult<Prepared> {
        let expected = self
            .status()
            .capture_id
            .ok_or(SourceError::SourceUnreadable)?;
        let root = self.data_path()?.join(self.kind.slot());
        let (seed, _) =
            read_json::<Seed>(&root.join("identity.json"))?.ok_or(SourceError::SourceUnreadable)?;
        if !seed.valid(self.kind) || seed.capture_id != expected {
            return Err(SourceError::SourceChanged);
        }
        let (receipt, _) = read_json::<Receipt>(&root.join("receipt.json"))?
            .ok_or(SourceError::SourceUnreadable)?;
        Snapshot::open(root, receipt, &seed, false)
    }
    pub fn hook(&self, notify: Arc<dyn Fn() + Send + Sync>) {
        *self.notify.lock().unwrap_or_else(PoisonError::into_inner) = Some(notify);
    }
    pub fn request_retry(&self) {
        self.retry.store(true, Ordering::Release);
        let notify = self
            .retry_notify
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(notify) = notify {
            notify();
        }
    }
    /// Wake the existing native source owner; this creates no worker or queue.
    pub fn retry_hook(&self, notify: Arc<dyn Fn() + Send + Sync>) {
        *self
            .retry_notify
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(notify);
    }
    pub fn take_retry(&self) -> bool {
        self.retry.swap(false, Ordering::AcqRel)
    }
    pub fn retry_pending(&self) -> bool {
        self.retry.load(Ordering::Acquire)
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
        if self.kind == Kind::LocalReads {
            status.view_ready = Some(false);
        }
        status.code = match &result {
            Ok(Prepared::Missing { .. }) => self.kind.code(1),
            Ok(Prepared::Snapshot(_)) => self.kind.code(2),
            Err(CaptureFailure::Source(SourceError::SourceChanged)) => self.kind.code(3),
            Err(CaptureFailure::Source(SourceError::TooLarge)) => self.kind.code(4),
            Err(CaptureFailure::Source(SourceError::UnsupportedSource)) => self.kind.code(5),
            #[cfg(unix)]
            Err(CaptureFailure::DataDirectory) => self.kind.code(6),
            Err(CaptureFailure::PrivateSlot) => self.kind.code(7),
            Err(_) => self.kind.code(8),
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
        result.map_err(CaptureFailure::source)
    }
    fn prepare_inner(&self) -> Result<Prepared, CaptureFailure> {
        if !self.data.is_absolute() {
            return Err(SourceError::InvalidMetadata.into());
        }
        #[cfg(unix)]
        let resolved = {
            if self.resolved.get().is_none() {
                let directory = disk::ManagedData::open(&self.data)
                    .map_err(|_| CaptureFailure::DataDirectory)?;
                let _ = self.resolved.set(Arc::new(directory));
            }
            self.resolved.get().unwrap().clone()
        };
        #[cfg(unix)]
        resolved.recheck().map_err(|_| SourceError::SourceChanged)?;
        #[cfg(unix)]
        let data = resolved.path();
        #[cfg(not(unix))]
        let data = self.data.as_path();
        let root = data.join(self.kind.slot());
        let _parents = disk::Parents::open(&root, true).map_err(|_| CaptureFailure::PrivateSlot)?;
        let _lock = disk::Lock::take(&root.join("capture.lock")).map_err(unreadable)?;
        let seed: Seed = match read_json(&root.join("identity.json"))? {
            Some((value, _)) => value,
            None => {
                let value = Seed {
                    version: 1,
                    boundary: self.kind.boundary().into(),
                    capture_id: uuid::Uuid::new_v4().to_string(),
                };
                create_json(&root.join("identity.json"), &value)?;
                value
            }
        };
        if !seed.valid(self.kind) {
            return Err(SourceError::InvalidMetadata.into());
        }
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .capture_id = Some(seed.capture_id.clone());
        let ready = root.join("receipt.json");
        if let Some((receipt, _)) = read_json::<Receipt>(&ready)? {
            let prepared = Snapshot::open(root, receipt, &seed, true)?;
            #[cfg(unix)]
            resolved.recheck().map_err(|_| SourceError::SourceChanged)?;
            return Ok(prepared);
        }
        let plan_path = root.join("source.json");
        let previous = read_json::<Plan>(&plan_path)?.map(|(p, _)| p);
        let source_path = data.join(self.kind.leaf());
        let source = disk::read_source(&source_path, self.kind.limit()).map_err(unreadable)?;
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
                return Err(SourceError::SourceChanged.into());
            }
        } else {
            create_json(&plan_path, &plan)?;
        }
        // A crashed copy never selects a newer version or replaces a partial
        // leaf. A fully written original leaf can be recognized by exact bytes.
        let snapshot = if plan.source.is_some() {
            let path = root.join("original.bin");
            let actual = match disk::read(&path, self.kind.limit()).map_err(unreadable)? {
                Some((saved, identity)) => {
                    if saved != bytes {
                        return Err(SourceError::SourceChanged.into());
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
        let check = disk::read_source(&source_path, self.kind.limit()).map_err(unreadable)?;
        if check.as_ref().map(|(_, s)| s) != plan.source.as_ref()
            || check
                .as_ref()
                .map(|(b, _)| sha256(b))
                .unwrap_or_else(|| sha256(&[]))
                != plan.sha256
        {
            return Err(SourceError::SourceChanged.into());
        }
        // An earlier successful byte write with an uncertain sync is not a
        // durable capture merely because a later read happens to see it.
        for (leaf, limit) in [
            ("identity.json", META),
            ("source.json", META),
            ("original.bin", self.kind.limit()),
        ] {
            let current = disk::read(&root.join(leaf), limit).map_err(unreadable)?;
            let synced = disk::durable(&root.join(leaf), limit).map_err(unreadable)?;
            if current != synced {
                return Err(SourceError::SourceChanged.into());
            }
        }
        let receipt = Receipt { plan, snapshot };
        create_json(&ready, &receipt)?;
        #[cfg(unix)]
        resolved.recheck().map_err(|_| SourceError::SourceChanged)?;
        Snapshot::open(root, receipt, &seed, true).map_err(Into::into)
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    capture_id: String,
    source_sha256: String,
    receipt_sha256: String,
    receipt_identity: Stamp,
}
impl Reference {
    pub fn capture_id(&self) -> &str {
        &self.capture_id
    }
    pub fn valid(&self) -> bool {
        canonical_uuid(&self.capture_id)
            && [&self.source_sha256, &self.receipt_sha256].iter().all(|s| {
                s.len() == 64
                    && s.bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
    }
}
/// An immutable historical version. The actual original source may now change.
/// Every later reader validates THIS retained object and hash afresh; it never
/// treats the live creation writer or an old status digest as a recheck.
pub struct Snapshot {
    kind: Kind,
    root: PathBuf,
    receipt: Receipt,
    receipt_stamp: Stamp,
}
impl Snapshot {
    fn open(root: PathBuf, receipt: Receipt, seed: &Seed, decode: bool) -> SourceResult<Prepared> {
        let kind = Kind::from_boundary(&seed.boundary).ok_or(SourceError::InvalidMetadata)?;
        if !seed.valid(kind) || receipt.plan.seed != *seed || receipt.plan.bytes > kind.limit() {
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
            ("original.bin", kind.limit()),
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
            kind,
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
        if decode {
            for item in kind.source(&bytes)?.into_chunks() {
                item?;
            }
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
        let source =
            disk::read(&self.root.join("original.bin"), self.kind.limit()).map_err(unreadable)?;
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
    pub fn reference(&self) -> SourceResult<Reference> {
        self.bytes()?;
        Ok(Reference {
            capture_id: self.capture_id().into(),
            source_sha256: self.receipt.plan.sha256.clone(),
            receipt_sha256: sha256(
                &serde_json::to_vec(&self.receipt).map_err(|_| SourceError::InvalidMetadata)?,
            ),
            receipt_identity: self.receipt_stamp.clone(),
        })
    }
    fn provenance(&self) -> SourceResult<serde_json::Value> {
        let reference = self.reference()?;
        Ok(
            serde_json::json!({"version":1,"boundary":self.kind.boundary(),"historical":true,
            "current_source_complete":false,"drained":false,"reference":reference,
            "original_source":self.receipt.plan.source,"original_bytes":self.receipt.plan.bytes,
            "snapshot_identity":self.receipt.snapshot}),
        )
    }
    pub fn report_sources(self: Arc<Self>) -> SourceResult<Vec<SourceStream>> {
        let original = self.clone().source()?;
        let value = self.provenance()?;
        let hash = sha256(&serde_json::to_vec(&value).map_err(|_| SourceError::InvalidMetadata)?);
        let source = SourceStream::new(
            zerus_desktop_migration::SourceRecord {
                family: match self.kind {
                    Kind::Creations => "next-creations-preload-provenance",
                    Kind::LocalReads => "next-local-read-marks-preload-provenance",
                }
                .into(),
                schema: 1,
                format: match self.kind {
                    Kind::Creations => "creation-preload-provenance-v1",
                    Kind::LocalReads => "local-read-preload-provenance-v1",
                }
                .into(),
                sha256: hash,
                build: None,
            },
            std::iter::once(Ok(value)),
        )
        .recheck(move || {
            serde_json::to_vec(&self.provenance()?)
                .map(|b| sha256(&b))
                .map_err(|_| SourceError::InvalidMetadata)
        });
        Ok(vec![original, source])
    }
    pub fn source(self: Arc<Self>) -> SourceResult<SourceStream> {
        let source = self.kind.source(&self.bytes()?)?;
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
                .canonicalize()
                .unwrap()
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
    #[cfg(unix)]
    #[test]
    fn ordinary_group_writable_data_folder_keeps_its_mode_and_private_leaf() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let s = Scratch::new();
        s.source(&raw());
        fs::set_permissions(&s.0, fs::Permissions::from_mode(0o775)).unwrap();
        let snapshot = s.snapshot();
        assert_eq!(snapshot.bytes().unwrap(), raw());
        assert_eq!(fs::metadata(&s.0).unwrap().mode() & 0o777, 0o775);
        assert_eq!(
            fs::metadata(s.0.join("creations.json")).unwrap().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(s.0.join("creation-preload-v1"))
                .unwrap()
                .mode()
                & 0o777,
            0o700
        );
    }
    #[cfg(unix)]
    #[test]
    fn symlinks_above_or_at_the_native_data_folder_are_resolved_once() {
        use std::os::unix::fs::symlink;
        for kind in ["home", "xdg-data", "local-share", "data-folder"] {
            let s = Scratch::new();
            let data = s.0.join("real-home/.local/share/com.hgdev.hgs-tray");
            fs::create_dir_all(&data).unwrap();
            crate::fsx::write_private_durable(&data.join("creations.json"), &raw()).unwrap();
            let alias = s.0.join(kind);
            let (target, tail) = match kind {
                "home" => (s.0.join("real-home"), ".local/share/com.hgdev.hgs-tray"),
                "data-folder" => (data.clone(), ""),
                _ => (s.0.join("real-home/.local/share"), "com.hgdev.hgs-tray"),
            };
            symlink(target, &alias).unwrap();
            let capture = Capture::new(alias.join(tail));
            let Prepared::Snapshot(original) = capture.prepare().unwrap() else {
                panic!("missing")
            };
            assert_eq!(original.bytes().unwrap(), raw());
            assert!(!capture.status().paused);
            // A later alias retarget does not adopt another home's source.
            let other = s.0.join("other");
            fs::create_dir_all(other.join(tail)).unwrap();
            crate::fsx::write_private_durable(
                &other.join(tail).join("creations.json"),
                br#"{"schema":1,"operations":{},"different":true}"#,
            )
            .unwrap();
            fs::remove_file(&alias).unwrap();
            symlink(&other, &alias).unwrap();
            let Prepared::Snapshot(again) = capture.prepare().unwrap() else {
                panic!("missing")
            };
            assert_eq!(again.capture_id(), original.capture_id());
            assert_eq!(again.bytes().unwrap(), raw());
            assert!(!other.join(tail).join("creation-preload-v1").exists());
        }
    }
    #[cfg(unix)]
    #[test]
    fn a_replaced_canonical_data_directory_is_not_adopted_on_retry() {
        let s = Scratch::new();
        let data = s.0.join("data");
        fs::create_dir(&data).unwrap();
        crate::fsx::write_private_durable(&data.join("creations.json"), &raw()).unwrap();
        let capture = Capture::new(data.clone());
        assert!(capture.prepare().is_ok());
        fs::rename(&data, s.0.join("old-data")).unwrap();
        fs::create_dir(&data).unwrap();
        crate::fsx::write_private_durable(&data.join("creations.json"), &raw()).unwrap();
        assert!(matches!(
            capture.live_path(),
            Err(SourceError::SourceChanged)
        ));
        assert!(capture.status().paused);
        assert_eq!(capture.status().code, "creation_source_changed");
        assert!(matches!(capture.prepare(), Err(SourceError::SourceChanged)));
        assert!(!data.join("creation-preload-v1").exists());
    }
    #[cfg(unix)]
    #[test]
    fn a_nonprivate_archive_slot_has_a_specific_status_and_same_source_retry() {
        use std::os::unix::fs::PermissionsExt;
        let s = Scratch::new();
        s.source(&raw());
        let slot = s.0.join("creation-preload-v1");
        fs::create_dir(&slot).unwrap();
        fs::set_permissions(&slot, fs::Permissions::from_mode(0o775)).unwrap();
        let capture = s.capture();
        assert!(capture.prepare().is_err());
        assert_eq!(capture.status().code, "creation_source_private_slot");
        assert!(!slot.join("identity.json").exists());
        assert_eq!(fs::read(s.0.join("creations.json")).unwrap(), raw());
        fs::set_permissions(&slot, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(capture.prepare().is_ok());
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
