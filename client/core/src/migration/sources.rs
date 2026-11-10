//! Adapters over existing decoders, with no source filesystem or effect stack.
//! The caller supplies bounded bytes read after G1's fresh drain and attaches
//! SourceStream::recheck using that same reader before publication.
use super::{bounded_json, SourceError, SourceResult, SourceStream};
use crate::creation;
pub use crate::next_source::next_pending_reads;
pub(crate) use crate::next_source::pages;
use crate::next_source::{valid_mark, READ_BYTES, READ_ENTRIES};
use crate::readmarks::ReadMark;
use crate::settings::{qt::QtValue, QtSnapshot};
use serde_json::{json, Value};
use zerus_desktop_migration::{sha256, SourceRecord, MAX_SNAPSHOT_BYTES};

mod tasks;
pub use tasks::{qt_task_forms, task_delivery, task_start_delivery};
mod approvals;
pub use approvals::approval_policies;
mod launches;
pub use launches::{qt_executor_launches, qt_pending_launches};
mod clones;
pub use clones::{next_repository_clones, qt_repository_clones};
mod downloads;
pub use downloads::download_index;
mod organization;
pub use organization::{next_organization_draft, qt_organization_checkpoints};
mod provider;
pub use provider::provider_operations;
mod worktrees;
pub use worktrees::{next_worktree_snapshots, qt_worktree_snapshots};
mod drafts;
pub use drafts::draft_checkpoint;
mod preferences;
pub use preferences::qt_retained_view_preference;
mod startup;
pub use startup::tray_config;
mod attention;
pub use attention::{next_attention_observed, qt_attention_observed};
mod local_reads;
pub use local_reads::{next_local_read_marks, qt_local_read_marks};
mod linux_notifications;
pub use linux_notifications::{
    next_linux_notifications, next_linux_owned_notifications, qt_linux_notifications,
};
mod recovery;
pub use recovery::recovery_policy_caches;

#[derive(Clone, Copy)]
pub enum Origin {
    Qt,
    Next,
}

impl Origin {
    fn family(self, suffix: &str) -> String {
        format!(
            "{}-{suffix}",
            match self {
                Self::Qt => "qt",
                Self::Next => "next",
            }
        )
    }
}

/// Exact original creation states/IDs, including dispatched/unknown requests.
/// This never calls Creations::load, changes a state or resumes an operation.
pub fn creations(origin: Origin, bytes: &[u8]) -> SourceResult<SourceStream> {
    let mut root = creation::decode_journal(bytes).map_err(|reason| {
        if reason == creation::READ_FAILED {
            SourceError::TooLarge
        } else {
            SourceError::UnsupportedSource
        }
    })?;
    let Value::Object(operations) = root.remove("operations").unwrap() else {
        unreachable!()
    };
    let metadata = json!({"kind":"journal-header", "content":root});
    bounded_json(&metadata, MAX_SNAPSHOT_BYTES as usize)?;
    // The journal is already bounded at 128 operations/8 MiB. Each item is
    // checked before becoming a snapshot, never truncated to fit a page.
    let chunks = std::iter::once(Ok(metadata)).chain(operations.into_iter().map(|(id, value)| {
        let item = json!({"kind":"creation", "id":id, "content":value});
        bounded_json(&item, MAX_SNAPSHOT_BYTES as usize)?;
        Ok(item)
    }));
    Ok(SourceStream::new(
        SourceRecord {
            family: origin.family("creations"),
            schema: 1,
            format: "creation-json-v1".into(),
            sha256: sha256(bytes),
            build: None,
        },
        chunks,
    ))
}

/// Qt's exact queued-read key and fields. The existing ReadMark serde decoder
/// owns the normalized shape; raw unknown fields remain in the private body.
fn qt_mark(key: &str, value: Value) -> SourceResult<Value> {
    let mut fields = value
        .as_object()
        .cloned()
        .ok_or(SourceError::UnsupportedSource)?;
    let host = fields
        .remove("host")
        .ok_or(SourceError::UnsupportedSource)?;
    // A newer source may not silently replace an explicit normalized identity.
    if fields.contains_key("computer") {
        return Err(SourceError::UnsupportedSource);
    }
    fields.insert("computer".into(), host);
    let mark: ReadMark = serde_json::from_value(Value::Object(fields))
        .map_err(|_| SourceError::UnsupportedSource)?;
    valid_mark(&mark)?;
    let identity: Vec<String> =
        serde_json::from_str(key).map_err(|_| SourceError::UnsupportedSource)?;
    if identity
        != [
            mark.computer.clone(),
            mark.name.clone(),
            mark.conversation_id.clone(),
        ]
    {
        return Err(SourceError::UnsupportedSource);
    }
    Ok(json!({"key":key,"original":value,"read_mark":mark}))
}

fn qt_json(value: &QtValue) -> SourceResult<Value> {
    qt_json_bounded(value, READ_BYTES)
}

fn qt_json_bounded(value: &QtValue, maximum: usize) -> SourceResult<Value> {
    let bytes = match value {
        QtValue::Bytes(bytes) => bytes.as_slice(),
        QtValue::Text(text) => text.as_bytes(),
        _ => return Err(SourceError::UnsupportedSource),
    };
    if bytes.len() > maximum {
        return Err(SourceError::TooLarge);
    }
    serde_json::from_slice(bytes).map_err(|_| SourceError::UnsupportedSource)
}

/// Pending read deliveries are evidence only: no ReadMarks::add/load, retry,
/// settlement or hgs read invocation. A host remains authoritative.
pub fn qt_pending_reads(snapshot: &QtSnapshot) -> SourceResult<SourceStream> {
    if !["qt-ini", "qt-plist", "qt-defaults"].contains(&snapshot.source) {
        return Err(SourceError::UnsupportedSource);
    }
    let raw = snapshot
        .values
        .get("attention/pendingReads")
        .map(qt_json)
        .transpose()?;
    let object = match raw {
        None => serde_json::Map::new(),
        Some(Value::Object(object)) if object.len() <= READ_ENTRIES => object,
        Some(Value::Object(_)) => return Err(SourceError::TooLarge),
        _ => return Err(SourceError::UnsupportedSource),
    };
    let identity = SourceRecord {
        family: "qt-pending-reads".into(),
        schema: 1,
        format: snapshot.source.into(),
        sha256: snapshot.sha256.clone(),
        build: None,
    };
    identity
        .validate()
        .map_err(|_| SourceError::InvalidMetadata)?;
    Ok(SourceStream::new(
        identity,
        pages(object.into_iter().map(|(key, value)| qt_mark(&key, value))),
    ))
}

/// The former per-host conversion marker suppresses duplicate conversion of
/// older local reads. Preserve the exact host strings; this never queues reads.
pub fn qt_shared_read_migrated(snapshot: &QtSnapshot) -> SourceResult<SourceStream> {
    let values = match snapshot
        .values
        .get("attention/sharedReadMigrated")
        .map(qt_json)
        .transpose()?
    {
        None => Vec::new(),
        Some(Value::Array(values)) if values.len() <= READ_ENTRIES => values,
        Some(Value::Array(_)) => return Err(SourceError::TooLarge),
        _ => return Err(SourceError::UnsupportedSource),
    };
    let identity = SourceRecord {
        family: "qt-shared-read-migrated".into(),
        schema: 1,
        format: snapshot.source.into(),
        sha256: snapshot.sha256.clone(),
        build: None,
    };
    identity
        .validate()
        .map_err(|_| SourceError::InvalidMetadata)?;
    if !["qt-ini", "qt-plist", "qt-defaults"].contains(&snapshot.source) {
        return Err(SourceError::UnsupportedSource);
    }
    Ok(SourceStream::new(
        identity,
        pages(values.into_iter().map(|value| {
            if !value
                .as_str()
                .is_some_and(|s| s.len() <= 8192 && !s.chars().any(char::is_control))
            {
                return Err(SourceError::UnsupportedSource);
            }
            Ok(json!({"host":value}))
        })),
    ))
}

#[cfg(test)]
mod tests;
