//! Legacy Qt placeholders and executor reservations, separate from the
//! creation journal. Reading these never places, expires or launches a session
//! and never dispatches, releases or reconciles a task delivery.
use super::{
    pages, qt_json_bounded, QtSnapshot, SourceError, SourceResult, SourceStream, READ_BYTES,
    READ_ENTRIES,
};
use crate::creation;
use serde_json::{json, Map, Value};
use zerus_desktop_migration::SourceRecord;

const PENDING: &str = "workspace/pendingLaunches";
const EXECUTORS: &str = "tasks/pendingExecutorLaunches";

fn text(value: &Value, key: &str, required: bool) -> bool {
    value[key].as_str().is_some_and(|s| {
        (!required || !s.is_empty()) && s.len() <= 8192 && !s.chars().any(char::is_control)
    })
}

fn entry(id: String, record: Value, executor: bool) -> SourceResult<Value> {
    if !creation::uuid(&id)
        || !record.is_object()
        // Current creation operations belong only to their existing journal.
        // Qt's pending-placeholder writer deliberately excludes them.
        || record.get("creation_schema").is_some()
        || !text(&record, "host", false)
        || !text(&record, "cmd", true)
        || !text(&record, "tag", true)
        || !record["created"].is_i64()
    {
        return Err(SourceError::UnsupportedSource);
    }
    let command = record["cmd"].as_str().unwrap();
    if command.encode_utf16().count() > 64 || command.starts_with('-') || command.contains(' ') {
        return Err(SourceError::UnsupportedSource);
    }
    if executor {
        // These are the original Qt launch/task/parent identities, never a
        // newly generated request or a match against a display name.
        if !record["task"].as_str().is_some_and(creation::uuid)
            || !record["project"].as_str().is_some_and(|project| {
                crate::task_details::document_call(project, record["task"].as_str().unwrap())
                    .is_ok()
            })
            || !text(&record, "parent", true)
            || !record["phase"]
                .as_str()
                .is_some_and(|phase| ["waiting", "reserved", "review"].contains(&phase))
            || (record["phase"] == "reserved" && !text(&record, "session", true))
            || record
                .get("session")
                .is_some_and(|_| !text(&record, "session", true))
        {
            return Err(SourceError::UnsupportedSource);
        }
    } else if !text(&record, "group", true)
        || ["target", "folder"]
            .iter()
            .any(|key| record.get(key).is_some_and(|_| !text(&record, key, false)))
        || ["implicit_inbox", "add_folder", "show_pending"]
            .iter()
            .any(|key| record.get(key).is_some_and(|v| !v.is_boolean()))
    {
        return Err(SourceError::UnsupportedSource);
    }
    if record
        .get("launch_id")
        .is_some_and(|value| value.as_str() != Some(id.as_str()))
    {
        return Err(SourceError::UnsupportedSource);
    }
    Ok(json!({"launch_id":id,"record":record}))
}

fn pending(snapshot: &QtSnapshot, executor: bool) -> SourceResult<SourceStream> {
    if !["qt-ini", "qt-plist", "qt-defaults"].contains(&snapshot.source) {
        return Err(SourceError::UnsupportedSource);
    }
    let (key, family, maximum, entries) = if executor {
        (EXECUTORS, "qt-executor-launches", 65536, 16)
    } else {
        (PENDING, "qt-pending-launches", READ_BYTES, READ_ENTRIES)
    };
    let raw = snapshot.values.get(key);
    let records = match raw.map(|v| qt_json_bounded(v, maximum)).transpose()? {
        None => Map::new(),
        Some(Value::Object(records)) if records.len() <= entries => records,
        Some(Value::Object(_)) => return Err(SourceError::TooLarge),
        _ => return Err(SourceError::UnsupportedSource),
    };
    let identity = SourceRecord {
        family: family.into(),
        schema: 1,
        format: snapshot.source.into(),
        sha256: snapshot.sha256.clone(),
        build: None,
    };
    identity
        .validate()
        .map_err(|_| SourceError::InvalidMetadata)?;
    // Missing and explicitly empty stores remain distinct private evidence.
    // Every original item must fit the existing 64-entry/1 MiB page budget;
    // oversized items fail rather than truncating fields or producing a record.
    let header = json!({"kind":"pending-header","key":key,"present":raw.is_some()});
    let chunks = std::iter::once(Ok(header)).chain(pages(
        records
            .into_iter()
            .map(move |(id, record)| entry(id, record, executor)),
    ));
    Ok(SourceStream::new(identity, chunks))
}

/// Legacy local Qt launch placeholders, without expiry, placement or retry.
pub fn qt_pending_launches(snapshot: &QtSnapshot) -> SourceResult<SourceStream> {
    pending(snapshot, false)
}

/// Qt's <=16 task executor reservations. Saved waiting/reserved/review phases
/// and original scopes are evidence only, never task-delivery authority.
pub fn qt_executor_launches(snapshot: &QtSnapshot) -> SourceResult<SourceStream> {
    pending(snapshot, true)
}

#[cfg(test)]
mod tests;
