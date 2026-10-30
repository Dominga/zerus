//! Fixed digest references to private snapshot bodies. Metadata pages contain
//! these references, never the bodies, and no input selects an output path.
use super::{import_directory, lower_hex, name, sha256, Invalid, Result, VERSION};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const MAX_SNAPSHOT_BYTES: u64 = 1 << 20;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotRef {
    pub sha256: String,
    pub bytes: u64,
}

impl SnapshotRef {
    pub fn validate(&self) -> Result<()> {
        if !lower_hex(&self.sha256, 64) || self.bytes == 0 || self.bytes > MAX_SNAPSHOT_BYTES {
            return Err(Invalid("invalid snapshot reference"));
        }
        Ok(())
    }

    pub fn relative_path(&self, import_id: &str) -> Result<PathBuf> {
        self.validate()?;
        Ok(import_directory(import_id)?
            .join("snapshots")
            .join(format!("{}.json", self.sha256)))
    }

    pub fn verify(&self, bytes: &[u8]) -> Result<()> {
        self.validate()?;
        if bytes.len() as u64 != self.bytes || sha256(bytes) != self.sha256 {
            return Err(Invalid("snapshot length or digest mismatch"));
        }
        Ok(())
    }
}

/// Body-free page in an import manifest. Snapshot contents remain private.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotPage {
    pub version: u32,
    pub family: String,
    pub schema: u32,
    pub source_sha256: String,
    pub index: u64,
    pub snapshot: SnapshotRef,
}

impl SnapshotPage {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() as u64 > MAX_SNAPSHOT_BYTES {
            return Err(Invalid("snapshot metadata exceeds its byte limit"));
        }
        let page: Self =
            serde_json::from_slice(bytes).map_err(|_| Invalid("invalid snapshot metadata"))?;
        page.validate()?;
        Ok(page)
    }

    pub fn validate(&self) -> Result<()> {
        if self.version != VERSION
            || !name(&self.family)
            || self.schema == 0
            || !lower_hex(&self.source_sha256, 64)
        {
            return Err(Invalid("invalid snapshot metadata identity"));
        }
        self.snapshot.validate()
    }
}
