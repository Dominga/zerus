//! Saved worktree caches are evidence only, never current verification.
use super::{
    bounded_json, pages, qt_json_bounded, QtSnapshot, SourceError, SourceResult, SourceStream,
};
use crate::worktrees;
use serde_json::{json, Map, Value};
use zerus_desktop_migration::{sha256, SourceRecord};

const KEY: &str = "workspace/worktreeSnapshots";

fn stream(source: SourceRecord, cache: Option<Map<String, Value>>) -> SourceResult<SourceStream> {
    source
        .validate()
        .map_err(|_| SourceError::InvalidMetadata)?;
    if let Some(cache) = &cache {
        if cache.len() > worktrees::SNAPSHOTS {
            return Err(SourceError::TooLarge);
        }
        bounded_json(cache, worktrees::SNAPSHOT_BYTES)?;
        if cache.iter().any(|(key, value)| {
            key.len() != 64
                || !key
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                || !worktrees::catalog_shape(value)
        }) {
            return Err(SourceError::UnsupportedSource);
        }
    }
    let header = json!({"kind":"worktree-cache-header","present":cache.is_some()});
    // The original key hashes executable/host/path. Without the exact original
    // tuple it is opaque: never infer today's CLI/alias or checkout ownership.
    let entries = cache
        .into_iter()
        .flatten()
        .map(|(key, record)| Ok(json!({"cache_key":key,"record":record})));
    Ok(SourceStream::new(
        source,
        std::iter::once(Ok(header)).chain(pages(entries)),
    ))
}

pub fn qt_worktree_snapshots(snapshot: &QtSnapshot) -> SourceResult<SourceStream> {
    if !["qt-ini", "qt-plist", "qt-defaults"].contains(&snapshot.source) {
        return Err(SourceError::UnsupportedSource);
    }
    let cache = snapshot
        .values
        .get(KEY)
        .map(|value| {
            let Value::Object(cache) = qt_json_bounded(value, worktrees::SNAPSHOT_BYTES)? else {
                return Err(SourceError::UnsupportedSource);
            };
            Ok(cache)
        })
        .transpose()?;
    stream(
        SourceRecord {
            family: "qt-worktree-snapshots".into(),
            schema: 1,
            format: snapshot.source.into(),
            sha256: snapshot.sha256.clone(),
            build: None,
        },
        cache,
    )
}

/// Existing Next `worktree-snapshots.json` bytes, never a live Catalogs or its
/// pruning loader. The caller owns bounded reads and fresh identity/hash checks.
pub fn next_worktree_snapshots(bytes: &[u8]) -> SourceResult<SourceStream> {
    if bytes.len() > 2 * worktrees::SNAPSHOT_BYTES {
        return Err(SourceError::TooLarge);
    }
    let Some(Value::Object(cache)) = worktrees::decode_saved(bytes) else {
        return Err(SourceError::UnsupportedSource);
    };
    stream(
        SourceRecord {
            family: "next-worktree-snapshots".into(),
            schema: 1,
            format: "worktree-snapshots-json-v1".into(),
            sha256: sha256(bytes),
            build: None,
        },
        Some(cache),
    )
}

#[cfg(test)]
mod tests;
