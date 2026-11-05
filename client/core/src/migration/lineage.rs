//! Lineage leaves shared by the G3 captures (zerus-0ht0): the bounded private
//! JSON leaf I/O and the three-field seed. `preload` and `draft_preload` keep
//! their own receipts, failure records, kinds, locks, decoders and callbacks;
//! the download evidence seed binds a folder object and stays its own.
//!
//! Every leaf goes through `fsx::preload` (`disk::read`, `disk::create`):
//! private, never following links, with its native identity and durability.
use super::{SourceError, SourceResult};
use crate::fsx::preload::{self as disk, Stamp};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::path::Path;
use zerus_desktop_migration::canonical_uuid;

/// The bound of every lineage leaf: a seed, a receipt, a failure record.
pub(crate) const META: usize = 64 << 10;

/// A leaf's I/O error as a source error: over its bound is too large,
/// anything else unreadable.
pub(crate) fn unreadable(error: std::io::Error) -> SourceError {
    if error.kind() == std::io::ErrorKind::FileTooLarge {
        SourceError::TooLarge
    } else {
        SourceError::SourceUnreadable
    }
}

/// A lineage leaf and its stamp, `None` when there is none. A leaf that does
/// not parse is invalid metadata, never absent.
pub(crate) fn read_json<T: DeserializeOwned>(path: &Path) -> SourceResult<Option<(T, Stamp)>> {
    disk::read(path, META)
        .map_err(unreadable)?
        .map(|(bytes, stamp)| {
            serde_json::from_slice(&bytes)
                .map(|value| (value, stamp))
                .map_err(|_| SourceError::InvalidMetadata)
        })
        .transpose()
}

/// Creates a new lineage leaf, never over an existing one. Over [`META`] is
/// too large and refused before anything is created.
pub(crate) fn create_json(path: &Path, value: &impl Serialize) -> SourceResult<Stamp> {
    let bytes = serde_json::to_vec(value).map_err(|_| SourceError::InvalidMetadata)?;
    if bytes.len() > META {
        return Err(SourceError::TooLarge);
    }
    disk::create(path, &bytes).map_err(unreadable)
}

/// A capture's lineage seed (`identity.json`): durable before any read, and
/// the identity every later leaf of the attempt names.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Seed {
    pub(crate) version: u32,
    pub(crate) boundary: String,
    pub(crate) capture_id: String,
}

impl Seed {
    /// A new seed for the native family `boundary`, with a new capture ID.
    pub(crate) fn new(boundary: &str) -> Self {
        Self {
            version: 1,
            boundary: boundary.into(),
            capture_id: uuid::Uuid::new_v4().to_string(),
        }
    }

    /// A version 1 seed of the native family `boundary`, which the caller
    /// passes (never the seed's own), with a canonical capture ID.
    pub(crate) fn valid(&self, boundary: &str) -> bool {
        self.version == 1 && self.boundary == boundary && canonical_uuid(&self.capture_id)
    }
}

#[cfg(test)]
mod tests;
