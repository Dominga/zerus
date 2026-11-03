//! Closed historical preload publication journals. Each group has a separate
//! scope, file and native lock; the original creation format/API stays intact.
//! Reuses desktop-import-v1 and never carries Qt/Settings hashes, worker
//! authority, consent or a mutable operation.
use super::{
    journal::MAX_ATTEMPTS, preload::Reference, transport::StagedReceipt, SourceError, SourceResult,
};
use serde::{Deserialize, Serialize};
use std::{
    io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use zerus_desktop_migration::{sha256, Evidence};
pub const SCOPE: &str = "next-creations-preload-v1";
const BYTES: usize = 64 << 10;
const FILE: &str = "creations-retained-import-v1.json";
/// Native-only selection: no caller-defined scope, filename or source path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Creations,
    LocalReads,
}
impl Group {
    pub fn scope(self) -> &'static str {
        match self {
            Self::Creations => SCOPE,
            Self::LocalReads => "next-local-read-marks-preload-v1",
        }
    }
    fn file(self) -> &'static str {
        match self {
            Self::Creations => FILE,
            Self::LocalReads => "local-read-retained-import-v1.json",
        }
    }
    fn lock(self) -> &'static str {
        match self {
            Self::Creations => "creations-retained-import-v1.lock",
            Self::LocalReads => "local-read-retained-import-v1.lock",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Reserved,
    Attempted,
    /// This attempt's encoding callback failed before any final record write.
    /// Distinct from a child receipt and never inferred from missing output.
    BeforeRecord,
    NotCommitted,
    Published,
    Unknown,
    Quarantined,
}
fn digest(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub version: u32,
    pub scope: String,
    pub configuration: String,
    pub import_id: String,
    pub created_at: u64,
    pub source: Reference,
    pub attempt: u32,
    pub phase: Phase,
    pub expected: Option<StagedReceipt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recorded_attempt: Option<u32>,
}
impl Record {
    pub fn new(configuration: String, source: Reference, created_at: u64) -> SourceResult<Self> {
        let record = Self {
            version: 1,
            scope: SCOPE.into(),
            configuration,
            import_id: source.capture_id().into(),
            created_at,
            source,
            attempt: 0,
            phase: Phase::Reserved,
            expected: None,
            recorded_attempt: None,
        };
        if !record.valid() {
            return Err(SourceError::InvalidMetadata);
        }
        Ok(record)
    }
    pub fn new_v2(configuration: String, source: Reference, created_at: u64) -> SourceResult<Self> {
        let mut record = Self::new(configuration, source, created_at)?;
        record.version = 2;
        record.recorded_attempt = Some(0);
        Ok(record)
    }
    pub fn new_group(
        group: Group,
        configuration: String,
        source: Reference,
        created_at: u64,
    ) -> SourceResult<Self> {
        let mut record = Self::new_v2(configuration, source, created_at)?;
        record.scope = group.scope().into();
        Ok(record)
    }
    /// Durable evidence that this Attempted record has not passed its commit
    /// gate. This is never inferred from the child/status reply or other phases.
    pub fn preparation_recoverable(&self) -> bool {
        self.valid()
            && self.phase == Phase::Attempted
            && match self.version {
                1 => self.expected.is_none(),
                2 => self.recorded_attempt.is_some_and(|n| n < self.attempt),
                _ => false,
            }
    }
    pub fn fence_preparation(&self) -> SourceResult<Self> {
        if !self.preparation_recoverable() {
            return Err(SourceError::InvalidMetadata);
        }
        let mut record = self.clone();
        record.version = 2;
        record.recorded_attempt = Some(self.recorded_attempt.unwrap_or(0));
        record.phase = Phase::BeforeRecord;
        Ok(record)
    }
    /// Called only for the originally admitted attempt, before its final frame.
    pub fn with_expected(&self, expected: &StagedReceipt) -> SourceResult<Self> {
        if self.phase != Phase::Attempted
            || self.expected.as_ref().is_some_and(|old| old != expected)
        {
            return Err(SourceError::SourceChanged);
        }
        let mut record = self.clone();
        record.expected = Some(expected.clone());
        if record.version == 2 {
            record.recorded_attempt = Some(record.attempt);
        }
        if !record.valid() {
            return Err(SourceError::InvalidMetadata);
        }
        Ok(record)
    }
    pub fn valid(&self) -> bool {
        (match self.version {
            1 => self.scope == SCOPE && self.recorded_attempt.is_none(),
            2 => self.recorded_attempt.is_some_and(|n| {
                n <= self.attempt
                    && (n == 0) == self.expected.is_none()
                    && (self.phase != Phase::NotCommitted || n == self.attempt)
            }),
            _ => false,
        }) && [Group::Creations.scope(), Group::LocalReads.scope()].contains(&self.scope.as_str())
            && digest(&self.configuration)
            && self.source.valid()
            && self.import_id == self.source.capture_id()
            && self.created_at > 0
            && self.attempt <= MAX_ATTEMPTS
            && (matches!(self.phase, Phase::Reserved | Phase::Quarantined) || self.attempt > 0)
            && (self.phase != Phase::Reserved || self.attempt == 0 && self.expected.is_none())
            && (!matches!(self.phase, Phase::Published | Phase::NotCommitted)
                || self.expected.is_some())
            && self.expected.as_ref().is_none_or(|r| {
                r.abi == "desktop-import-v1"
                    && r.published
                    && r.evidence == Evidence::Staged
                    && r.configuration == self.configuration
                    && r.import_id == self.import_id
                    && r.import_record.version == 1
                    && digest(&r.import_record.sha256)
                    && r.bytes <= 64 << 20
                    && r.pages <= 1024
                    && r.snapshots <= 1024
            })
    }
    fn follows(&self, old: &Self) -> bool {
        if self.scope != old.scope
            || self.configuration != old.configuration
            || self.import_id != old.import_id
            || self.source != old.source
            || self.created_at != old.created_at
            || old
                .expected
                .as_ref()
                .is_some_and(|e| self.expected.as_ref() != Some(e))
            || old.expected.is_none()
                && self.expected.is_some()
                && (old.phase != Phase::Attempted || self.phase != Phase::Attempted)
        {
            return false;
        }
        if self.version != old.version {
            return old.version == 1 && old.fence_preparation().is_ok_and(|fenced| *self == fenced);
        }
        if self.version == 2 && self.recorded_attempt != old.recorded_attempt {
            let gate = old.phase == Phase::Attempted
                && self.phase == Phase::Attempted
                && self.attempt == old.attempt
                && self.recorded_attempt == Some(self.attempt)
                && old.recorded_attempt.is_some_and(|n| n < self.attempt);
            if !gate {
                return false;
            }
        }
        match (old.phase, self.phase) {
            (Phase::Reserved, Phase::Attempted) => self.attempt == 1,
            (Phase::NotCommitted | Phase::BeforeRecord, Phase::Attempted) => {
                self.attempt == old.attempt + 1
            }
            (Phase::Reserved, Phase::Reserved | Phase::Quarantined)
            | (
                Phase::Attempted,
                Phase::Attempted
                | Phase::BeforeRecord
                | Phase::NotCommitted
                | Phase::Published
                | Phase::Unknown
                | Phase::Quarantined,
            )
            | (Phase::Unknown, Phase::Unknown | Phase::Published)
            | (Phase::NotCommitted, Phase::NotCommitted | Phase::Quarantined)
            | (Phase::BeforeRecord, Phase::BeforeRecord | Phase::Quarantined)
            | (Phase::Published, Phase::Published)
            | (Phase::Quarantined, Phase::Quarantined) => self.attempt == old.attempt,
            _ => false,
        }
    }
}
#[derive(Clone)]
pub struct Stored {
    pub record: Record,
    pub sha256: String,
}
pub struct Journal {
    group: Group,
    data: PathBuf,
    configuration: String,
}
/// How long a save waits for another run's compare/write on the same journal.
/// That write is bounded metadata only, so this is never a long wait.
const LOCK_WAIT: Duration = Duration::from_secs(2);
/// Another run at the journal lock: `flock` refuses on Unix; on Windows see
/// [`contended_on`].
fn contended(error: &io::Error) -> bool {
    contended_on(error, cfg!(windows))
}
/// Windows: a sharing (32) or lock (33) violation while another run holds the
/// lock, or ERROR_FILE_EXISTS (80) when two runs race to create it first.
fn contended_on(error: &io::Error, windows: bool) -> bool {
    error.kind() == io::ErrorKind::WouldBlock
        || (windows && matches!(error.raw_os_error(), Some(32 | 33 | 80)))
}
/// The journal lock, waiting a bounded time for a concurrent run (a recovery
/// fence and the original's final write may overlap). Any other failure, or a
/// holder that outlasts the wait, stays unreadable: never a write without it.
fn lock(path: &Path) -> SourceResult<crate::fsx::preload::Lock> {
    let until = Instant::now() + LOCK_WAIT;
    loop {
        match crate::fsx::preload::Lock::take_managed(path) {
            Ok(lock) => return Ok(lock),
            Err(e) if contended(&e) && Instant::now() < until => {
                std::thread::sleep(Duration::from_millis(5))
            }
            Err(_) => return Err(SourceError::SourceUnreadable),
        }
    }
}
impl Journal {
    pub fn new(data: &Path, configuration: &str) -> SourceResult<Self> {
        Self::for_group(Group::Creations, data, configuration)
    }
    pub fn for_group(group: Group, data: &Path, configuration: &str) -> SourceResult<Self> {
        if !data.is_absolute() || !digest(configuration) {
            return Err(SourceError::InvalidMetadata);
        }
        Ok(Self {
            group,
            data: data.into(),
            configuration: configuration.into(),
        })
    }
    pub fn load(&self) -> SourceResult<Option<Stored>> {
        let Some(bytes) =
            crate::fsx::read_private_bounded(&self.data.join(self.group.file()), BYTES)
                .map_err(|_| SourceError::SourceUnreadable)?
        else {
            return Ok(None);
        };
        let record: Record =
            serde_json::from_slice(&bytes).map_err(|_| SourceError::UnsupportedSource)?;
        if !record.valid()
            || record.scope != self.group.scope()
            || record.configuration != self.configuration
        {
            return Err(SourceError::InvalidMetadata);
        }
        Ok(Some(Stored {
            record,
            sha256: sha256(&bytes),
        }))
    }
    pub fn fence_preparation(&self, original: &Stored) -> SourceResult<Stored> {
        self.save(original.record.fence_preparation()?, Some(&original.sha256))
    }
    pub fn save(&self, record: Record, previous: Option<&str>) -> SourceResult<Stored> {
        if !record.valid()
            || record.scope != self.group.scope()
            || record.configuration != self.configuration
        {
            return Err(SourceError::InvalidMetadata);
        }
        // Serialize only this bounded metadata compare/write. No original
        // creation-worker lease or SDK work runs under this native lock, so a
        // concurrent run's save waits for it instead of failing spuriously.
        let _lock = lock(&self.data.join(self.group.lock()))?;
        let current = self.load()?;
        if current.as_ref().map(|s| s.sha256.as_str()) != previous
            || current.as_ref().is_some_and(|s| !record.follows(&s.record))
            || current.is_none() && record.phase != Phase::Reserved
        {
            return Err(SourceError::SourceChanged);
        }
        let bytes = serde_json::to_vec(&record).map_err(|_| SourceError::InvalidMetadata)?;
        if bytes.len() > BYTES {
            return Err(SourceError::TooLarge);
        }
        crate::fsx::write_private_durable(&self.data.join(self.group.file()), &bytes)
            .map_err(|_| SourceError::SourceUnreadable)?;
        let stored = self.load()?.ok_or(SourceError::SourceUnreadable)?;
        if stored.record != record || stored.sha256 != sha256(&bytes) {
            return Err(SourceError::SourceChanged);
        }
        Ok(stored)
    }
}

#[cfg(all(test, any(unix, windows)))]
mod tests;
