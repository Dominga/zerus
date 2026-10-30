//! Metadata from the cache index shared by Qt and Next. No live cache, lock,
//! downloaded content, cleanup or file-system discovery is opened here.
use super::{pages, SourceError, SourceResult, SourceStream, READ_ENTRIES};
use crate::{
    download_cache,
    paths::{self, Style},
};
use serde_json::{json, Value};
use zerus_desktop_migration::{sha256, SourceRecord};

/// The caller supplies the existing cache resolver's canonical UTF-8 folder
/// and bounded index bytes, deduplicating shared/aliased Qt and Next folders.
/// Folder identity stays in private snapshots, never metadata or errors.
///
/// This preserves index evidence, not downloaded content or its immutability.
/// Attach the same reader's fresh source identity/hash recheck before staging;
/// G1's current drain and G2's content/ownership guards remain separate.
pub fn download_index(folder: &str, bytes: &[u8]) -> SourceResult<SourceStream> {
    let style = if paths::windows_shaped(folder) {
        Style::Windows
    } else {
        Style::Posix
    };
    if folder.len() > 8192
        || folder.chars().any(char::is_control)
        || !paths::absolute(folder, style)
    {
        return Err(SourceError::UnsupportedSource);
    }
    if bytes.len() as u64 > download_cache::MAX_INDEX_BYTES {
        return Err(SourceError::TooLarge);
    }
    let mut document =
        download_cache::decode_index(bytes).map_err(|_| SourceError::UnsupportedSource)?;
    let Value::Array(files) = document.remove("files").unwrap() else {
        unreachable!()
    };
    if files.len() > READ_ENTRIES {
        return Err(SourceError::TooLarge);
    }
    let source = SourceRecord {
        family: format!("download-index/{}", sha256(folder.as_bytes())),
        schema: 1,
        format: "download-cache-json-v1".into(),
        sha256: sha256(bytes),
        build: None,
    };
    let header = json!({"kind":"download-index-header","folder":folder,"content":document});
    Ok(SourceStream::new(
        source,
        std::iter::once(Ok(header)).chain(pages(files.into_iter().map(Ok))),
    ))
}

#[cfg(test)]
mod tests;
