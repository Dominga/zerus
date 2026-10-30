//! Private Qt form and task-delivery evidence. These adapters invoke no task
//! command and never turn a saved request or message into a new submission.
use super::{
    bounded_json, pages, qt_json, Origin, QtSnapshot, QtValue, SourceError, SourceResult,
    SourceStream, READ_BYTES, READ_ENTRIES,
};
use crate::{creation, tasks_write::outbox};
use serde_json::{json, Value};
use zerus_desktop_migration::{sha256, SourceRecord, MAX_SNAPSHOT_BYTES};

fn part(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 256
        && !text.contains(['/', '\\'])
        && !text.chars().any(char::is_control)
}
fn text(value: &QtValue) -> SourceResult<String> {
    let value = match value {
        QtValue::Text(text) => text.as_str(),
        QtValue::Bytes(bytes) => {
            std::str::from_utf8(bytes).map_err(|_| SourceError::UnsupportedSource)?
        }
        _ => return Err(SourceError::UnsupportedSource),
    };
    if value.len() as u64 > MAX_SNAPSHOT_BYTES {
        return Err(SourceError::TooLarge);
    }
    Ok(value.into())
}
fn form(key: String, value: QtValue) -> SourceResult<Value> {
    let parts = key.split('/').collect::<Vec<_>>();
    let Some(project) = parts.get(2).filter(|s| part(s)) else {
        return Err(SourceError::UnsupportedSource);
    };
    let kind = *parts.get(1).ok_or(SourceError::UnsupportedSource)?;
    match (kind, parts.get(3).copied(), parts.len()) {
        ("newCard", Some("title"), 4) => {
            Ok(json!({"key":key,"kind":"new-card-title","project":project,"value":text(&value)?}))
        }
        ("newCard", Some("request"), 4) => {
            let request = qt_json(&value)?;
            let title = request["title"]
                .as_str()
                .ok_or(SourceError::UnsupportedSource)?;
            let target = request["session"]
                .as_str()
                .ok_or(SourceError::UnsupportedSource)?;
            let host = request["host"]
                .as_str()
                .ok_or(SourceError::UnsupportedSource)?;
            let session = request["session_id"]
                .as_str()
                .ok_or(SourceError::UnsupportedSource)?;
            if request["project"].as_str() != Some(project)
                || !request["request_id"].as_str().is_some_and(creation::uuid)
                || !request["stage"]
                    .as_str()
                    .is_some_and(|s| ["backlog", "todo"].contains(&s))
                || title.trim().is_empty()
                || title.len() > 512
                || title.chars().any(char::is_control)
                || target.is_empty()
                || [target, host, session]
                    .iter()
                    .any(|s| s.len() > 8192 || s.chars().any(char::is_control))
                || (target == "any" && (!host.is_empty() || !session.is_empty()))
                || (target != "any" && session.is_empty())
            {
                return Err(SourceError::UnsupportedSource);
            }
            Ok(json!({"key":key,"kind":"new-card-request","project":project,"value":request}))
        }
        ("titleDraft" | "noteDraft" | "messageRequest", Some(task), 4) if creation::uuid(task) => {
            let value = text(&value)?;
            if kind == "messageRequest" && !creation::uuid(&value) {
                return Err(SourceError::UnsupportedSource);
            }
            Ok(json!({"key":key,"kind":kind,"project":project,"task":task,"value":value}))
        }
        _ => Err(SourceError::UnsupportedSource),
    }
}

/// Preserve forms/markers under their exact original scopes. Description and
/// comment bodies/attachments remain in the existing DraftStore importer.
/// A saved new-card request is distinct from a task UUID and is never replayed.
pub fn qt_task_forms(snapshot: &QtSnapshot) -> SourceResult<SourceStream> {
    if !["qt-ini", "qt-plist", "qt-defaults"].contains(&snapshot.source) {
        return Err(SourceError::UnsupportedSource);
    }
    let candidates = snapshot
        .values
        .iter()
        .filter(|(key, _)| {
            [
                "tasks/newCard",
                "tasks/titleDraft",
                "tasks/noteDraft",
                "tasks/messageRequest",
            ]
            .iter()
            .any(|prefix| {
                key.as_str() == *prefix
                    || key
                        .strip_prefix(prefix)
                        .is_some_and(|rest| rest.starts_with('/'))
            })
        })
        .take(READ_ENTRIES + 1);
    let mut entries = Vec::new();
    let mut total = 0;
    for (key, value) in candidates {
        let length = match value {
            QtValue::Text(text) => text.len(),
            QtValue::Bytes(bytes) => bytes.len(),
            _ => return Err(SourceError::UnsupportedSource),
        };
        total += key.len() + length;
        if length as u64 > MAX_SNAPSHOT_BYTES || total > READ_BYTES || entries.len() >= READ_ENTRIES
        {
            return Err(SourceError::TooLarge);
        }
        entries.push((key.clone(), value.clone()));
    }
    let source = SourceRecord {
        family: "qt-task-forms".into(),
        schema: 1,
        format: snapshot.source.into(),
        sha256: snapshot.sha256.clone(),
        build: None,
    };
    source
        .validate()
        .map_err(|_| SourceError::InvalidMetadata)?;
    Ok(SourceStream::new(
        source,
        pages(entries.into_iter().map(|(key, value)| form(key, value))),
    ))
}

/// `project`/`task` come from the exact marker scope. The caller reads only
/// outbox::path(root,project,task), then freshly rechecks that file and marker
/// after the drain. There is no caller-selected final evidence path.
pub fn task_delivery(
    origin: Origin,
    project: &str,
    task: &str,
    marker_id: &str,
    bytes: &[u8],
) -> SourceResult<SourceStream> {
    if !part(project) || !creation::uuid(task) || !creation::uuid(marker_id) {
        return Err(SourceError::UnsupportedSource);
    }
    // A delivery is one indivisible original intent. Refuse an oversized
    // source item before JSON allocation rather than buffering/truncating it.
    if bytes.len() as u64 > MAX_SNAPSHOT_BYTES {
        return Err(SourceError::TooLarge);
    }
    let record = outbox::decode(bytes).map_err(|_| SourceError::UnsupportedSource)?;
    if record["id"].as_str() != Some(marker_id)
        || !record["intent"]["content"].is_object()
        || !record["intent"]["mode"].is_string()
        || !record["intent"]["dispatch"].is_string()
    {
        return Err(SourceError::UnsupportedSource);
    }
    let item = json!({"project":project,"task":task,"marker_id":marker_id,"record":record});
    bounded_json(&item, MAX_SNAPSHOT_BYTES as usize)?;
    let family = origin.family(&format!(
        "task-delivery/{}",
        sha256(format!("{project}\n{task}").as_bytes())
    ));
    Ok(SourceStream::new(
        SourceRecord {
            family,
            schema: 1,
            format: "task-outbox-json-v1".into(),
            sha256: sha256(bytes),
            build: None,
        },
        std::iter::once(Ok(item)),
    ))
}

/// Next's existing-executor Start checkpoint, separate from discussion
/// delivery. The caller reads only outbox::path(root.join("task-start"),
/// project, task) and verifies/rechecks that exact file and reservation marker
/// under G1's fresh drain. This never opens a task, recovers or releases it.
pub fn task_start_delivery(
    project: &str,
    task: &str,
    marker_id: &str,
    bytes: &[u8],
) -> SourceResult<SourceStream> {
    // Use the live writer's scope validator, without running its TaskCall.
    crate::task_details::document_call(project, task)
        .map_err(|_| SourceError::UnsupportedSource)?;
    if !creation::canonical_uuid(marker_id) {
        return Err(SourceError::UnsupportedSource);
    }
    // The original guarded plan is one indivisible item. Refuse before JSON
    // allocation rather than dropping its prompt, target or unknown fields.
    if bytes.len() as u64 > MAX_SNAPSHOT_BYTES {
        return Err(SourceError::TooLarge);
    }
    let record = outbox::decode(bytes).map_err(|_| SourceError::UnsupportedSource)?;
    crate::task_start::checked(&record, project, task)
        .map_err(|_| SourceError::UnsupportedSource)?;
    if record["id"].as_str() != Some(marker_id) {
        return Err(SourceError::UnsupportedSource);
    }
    let item = json!({"project":project,"task":task,"marker_id":marker_id,"record":record});
    bounded_json(&item, MAX_SNAPSHOT_BYTES as usize)?;
    Ok(SourceStream::new(
        SourceRecord {
            family: format!(
                "next-task-start/{}",
                sha256(format!("{project}\n{task}").as_bytes())
            ),
            schema: 1,
            format: "task-start-outbox-json-v1".into(),
            sha256: sha256(bytes),
            build: None,
        },
        std::iter::once(Ok(item)),
    ))
}
