//! A separate creation-group publication record. Historical snapshot binding
//! only; reuses desktop-import-v1 and never carries Qt/Settings hashes, worker
//! authority, consent or a mutable creation operation.
use super::{
    journal::MAX_ATTEMPTS, preload::Reference, transport::StagedReceipt, SourceError, SourceResult,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use zerus_desktop_migration::{sha256, Evidence};
pub const SCOPE: &str = "next-creations-preload-v1";
const BYTES: usize = 64 << 10;
const FILE: &str = "creations-retained-import-v1.json";
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
        };
        if !record.valid() {
            return Err(SourceError::InvalidMetadata);
        }
        Ok(record)
    }
    pub fn valid(&self) -> bool {
        self.version == 1
            && self.scope == SCOPE
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
        if self.configuration != old.configuration
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
    data: PathBuf,
    configuration: String,
}
impl Journal {
    pub fn new(data: &Path, configuration: &str) -> SourceResult<Self> {
        if !data.is_absolute() || !digest(configuration) {
            return Err(SourceError::InvalidMetadata);
        }
        Ok(Self {
            data: data.into(),
            configuration: configuration.into(),
        })
    }
    pub fn load(&self) -> SourceResult<Option<Stored>> {
        let Some(bytes) = crate::fsx::read_private_bounded(&self.data.join(FILE), BYTES)
            .map_err(|_| SourceError::SourceUnreadable)?
        else {
            return Ok(None);
        };
        let record: Record =
            serde_json::from_slice(&bytes).map_err(|_| SourceError::UnsupportedSource)?;
        if !record.valid() || record.configuration != self.configuration {
            return Err(SourceError::InvalidMetadata);
        }
        Ok(Some(Stored {
            record,
            sha256: sha256(&bytes),
        }))
    }
    pub fn save(&self, record: Record, previous: Option<&str>) -> SourceResult<Stored> {
        if !record.valid() || record.configuration != self.configuration {
            return Err(SourceError::InvalidMetadata);
        }
        // Serialize only this bounded metadata compare/write. No original
        // creation-worker lease or SDK work runs under this native lock.
        let _lock = crate::fsx::preload::Lock::take_managed(
            &self.data.join("creations-retained-import-v1.lock"),
        )
        .map_err(|_| SourceError::SourceUnreadable)?;
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
        crate::fsx::write_private_durable(&self.data.join(FILE), &bytes)
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
