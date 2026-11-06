//! The download folder's committed original index as a Staged source of
//! the historical report (zerus-5wmh.66.5.2.1.3.1): evidence only, never a
//! download, a removal, a send, cleanup or current-coverage consent.
//!
//! The source is only the folder's evidence receipt and its exact original
//! (`download_cache::evidence`, boundary
//! `next-download-index-before-first-write-v1`), read through the bound
//! snapshot and streamed by the unchanged `sources::download_index`
//! decoder (`download-index/<folder hash>`). A folder without a receipt, an
//! unsupported location and an unreadable record publish nothing here.
use super::{sources, SourceError, SourceResult, SourceStream};
use crate::download_cache::{
    self,
    evidence::{self, Evidence, Original},
};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use zerus_desktop_migration::{canonical_uuid, sha256};

/// What a report record binds: the exact receipt and original it publishes.
/// A later receipt, another folder object or another original is another
/// source, never adopted under this record's ID.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub boundary: String,
    pub capture_id: String,
    /// The folder object (digest) the evidence seed binds.
    pub directory: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub birth: Option<String>,
    pub index_bytes: u64,
    pub index_sha256: String,
    /// The receipt file's exact bytes.
    pub receipt_sha256: String,
}

fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl Binding {
    pub fn capture_id(&self) -> &str {
        &self.capture_id
    }
    pub fn valid(&self) -> bool {
        self.boundary == evidence::BOUNDARY
            && canonical_uuid(&self.capture_id)
            && digest(&self.directory)
            && self.index_bytes > 0
            && self.index_bytes <= download_cache::MAX_INDEX_BYTES
            && digest(&self.index_sha256)
            && digest(&self.receipt_sha256)
    }
}

/// One folder's committed original, held for one report attempt.
pub struct Snapshot {
    /// The configured folder as given: its evidence is checked again here.
    root: PathBuf,
    /// Its canonical path when read, as the computer names it
    /// ([`computer_folder`]): the decoder's folder identity.
    folder: String,
    receipt: Original,
    binding: Binding,
    index: Vec<u8>,
}

impl Snapshot {
    /// The binding, after the folder's evidence is checked again by bounded
    /// stats and the receipt's own read: the same receipt, byte for byte
    /// (and so the same stamped parts), is still there for the same folder
    /// object. The original's parts are not read again.
    pub fn reference(&self) -> SourceResult<Binding> {
        match evidence::fingerprint(&self.root) {
            Ok(Some((receipt, sha256)))
                if *receipt == self.receipt && sha256 == self.binding.receipt_sha256 =>
            {
                Ok(self.binding.clone())
            }
            _ => Err(SourceError::SourceChanged),
        }
    }

    /// The original index through the unchanged download-index decoder,
    /// checked again just before publication: the same receipt is still
    /// there, so the same original (its hash) is what is published.
    pub fn report_sources(self: Arc<Self>) -> SourceResult<Vec<SourceStream>> {
        let stream = sources::download_index(&self.folder, &self.index)?;
        let again = self.clone();
        Ok(vec![stream.recheck(move || {
            again.reference().map(|binding| binding.index_sha256)
        })])
    }
}

/// A canonical folder as its computer names it, the form the download-index
/// decoder takes. Windows canonicalizes to verbatim paths: a drive's
/// `\\?\C:\x` is `C:\x` and a share's `\\?\UNC\server\share\x` is
/// `\\server\share\x`. Any other verbatim or device path (a volume GUID,
/// `\\.\`) names no folder of a computer: `None`. Other paths are unchanged.
fn computer_folder(path: &str) -> Option<String> {
    if let Some(share) = path.strip_prefix(r"\\?\UNC\") {
        return Some(format!(r"\\{share}"));
    }
    if let Some(rest) = path.strip_prefix(r"\\?\") {
        let drive = rest.as_bytes();
        let local = drive.len() >= 3
            && drive[0].is_ascii_alphabetic()
            && drive[1] == b':'
            && drive[2] == b'\\';
        return local.then(|| rest.to_owned());
    }
    if path.starts_with(r"\\.\") {
        return None;
    }
    Some(path.to_owned())
}

/// The configured folder's committed original. `Ok(None)`: nothing to
/// publish (no receipt, or Next's first change found no index). An
/// unsupported location or an unreadable record is an error, never a
/// complete source.
pub fn retained(root: &Path) -> SourceResult<Option<Arc<Snapshot>>> {
    let captured = match evidence::captured(root) {
        Ok(Some(captured)) => captured,
        Ok(None) => return Ok(None),
        Err(Evidence::Unsupported) => return Err(SourceError::UnsupportedSource),
        Err(_) => return Err(SourceError::SourceUnreadable),
    };
    let receipt = captured.snapshot.receipt;
    let Some(index) = captured.snapshot.index else {
        return Ok(None);
    };
    let folder = captured
        .folder
        .to_str()
        .and_then(computer_folder)
        .ok_or(SourceError::UnsupportedSource)?;
    let binding = Binding {
        boundary: receipt.boundary.clone(),
        capture_id: receipt.capture_id.clone(),
        directory: captured.directory,
        birth: captured.birth,
        index_bytes: receipt.bytes,
        index_sha256: receipt.sha256.clone().ok_or(SourceError::InvalidMetadata)?,
        receipt_sha256: sha256(&captured.receipt),
    };
    if !binding.valid() || binding.index_sha256 != sha256(&index) {
        return Err(SourceError::InvalidMetadata);
    }
    Ok(Some(Arc::new(Snapshot {
        root: root.into(),
        folder,
        receipt,
        binding,
        index,
    })))
}

#[cfg(test)]
mod tests;
