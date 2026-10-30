//! Original draft operation metadata, never a live composer or blob importer.
use super::{bounded_json, Origin, SourceError, SourceResult, SourceStream};
use crate::{attachments, drafts};
use serde_json::{json, Value};
use zerus_desktop_migration::{sha256, SourceRecord, MAX_SNAPSHOT_BYTES};

/// Preserve a single existing manifest in bounded source batches. This does
/// not re-seed drafts or copy/verify blobs: the existing draft store owns those.
/// Exact source/folder/hash rechecks and any future receipt/content checks use
/// the caller's existing readers, separately from this staged metadata.
pub fn draft_checkpoint(origin: Origin, key: &str, bytes: &[u8]) -> SourceResult<SourceStream> {
    if key.is_empty() || key.len() > 8192 || key.chars().any(|c| c.is_control() && c != '\n') {
        return Err(SourceError::UnsupportedSource);
    }
    if bytes.len() as u64 > drafts::MAX_MANIFEST {
        return Err(SourceError::TooLarge);
    }
    let record = drafts::decode_manifest(key, bytes).ok_or(SourceError::UnsupportedSource)?;
    if ["text", "label"]
        .iter()
        .any(|k| record.get(*k).is_some_and(|v| !v.is_string()))
        || ["position", "anchor"]
            .iter()
            .any(|k| record.get(*k).is_some_and(|v| v.as_i64().is_none()))
        || ["sending", "uncertain"]
            .iter()
            .any(|k| record.get(*k).is_some_and(|v| !v.is_boolean()))
        || record.get("form_state").is_some_and(|v| !v.is_object())
        || record
            .get("next_attachment_number")
            .is_some_and(|v| !v.as_u64().is_some_and(|n| n <= u64::from(u32::MAX)))
    {
        return Err(SourceError::UnsupportedSource);
    }
    let files = match record.get("attachments") {
        None => &[][..],
        Some(Value::Array(files)) => files.as_slice(),
        _ => return Err(SourceError::UnsupportedSource),
    };
    if files.len() > attachments::MAX_ATTACHMENTS {
        return Err(SourceError::TooLarge);
    }
    let mut total = 0;
    for file in files {
        if ["name", "mime", "reference"]
            .iter()
            .any(|k| file.get(*k).is_some_and(|v| !v.is_string()))
            || !file.get("sha256").and_then(Value::as_str).is_some_and(|s| {
                s.len() == 64
                    && s.bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
        {
            return Err(SourceError::UnsupportedSource);
        }
        let size = file
            .get("bytes")
            .and_then(Value::as_u64)
            .ok_or(SourceError::UnsupportedSource)?;
        if size == 0 {
            return Err(SourceError::UnsupportedSource);
        }
        if size > attachments::MAX_ATTACHMENT_BYTES as u64 {
            return Err(SourceError::TooLarge);
        }
        total += size;
        if total > attachments::MAX_TOTAL_BYTES as u64 {
            return Err(SourceError::TooLarge);
        }
    }
    let body = json!({"kind":"draft-checkpoint","key":key,"record":record,"attachment_bytes_included":false});
    bounded_json(&body, MAX_SNAPSHOT_BYTES as usize)?;
    Ok(SourceStream::new(
        SourceRecord {
            family: format!(
                "{}/{}",
                origin.family("draft-checkpoint"),
                sha256(key.as_bytes())
            ),
            schema: 1,
            format: "qt-draft-manifest-v1".into(),
            sha256: sha256(bytes),
            build: None,
        },
        std::iter::once(Ok(body)),
    ))
}

#[cfg(test)]
mod tests;
