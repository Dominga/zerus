use super::{Error, Support};
use crate::{creation, migration::sources};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use zerus_desktop_migration::{SnapshotPage, SourceRecord};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Creation,
    TaskMessage,
    TaskStart,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordedState {
    /// Original writer recorded a terminal phase; no current effect proof.
    Terminal,
    /// Interrupted, unknown or pending evidence always requires review.
    NeedsReview,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Scope {
    Creation {
        computer: String,
        expected_machine: Option<String>,
    },
    Task {
        project: String,
        task: String,
    },
}

/// Only bounded identifiers and recorded state. Prompts, paths, commands,
/// errors, attachments, plan bodies and unknown fields stay in private evidence.
/// This type has no conversion to a live request or a settled operation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Operation {
    pub family: String,
    pub manifest_page: usize,
    pub source_index: u64,
    pub snapshot_sha256: String,
    pub kind: Kind,
    pub id: String,
    pub requests: BTreeMap<String, String>,
    pub scope: Scope,
    pub phase: String,
    pub recorded_state: RecordedState,
}

fn family(source: &SourceRecord) -> Option<(Kind, sources::Origin)> {
    let (name, origin) = if let Some(name) = source.family.strip_prefix("next-") {
        (name, sources::Origin::Next)
    } else {
        (source.family.strip_prefix("qt-")?, sources::Origin::Qt)
    };
    if source.schema != 1 {
        return None;
    }
    if name == "creations" && source.format == "creation-json-v1" {
        return Some((Kind::Creation, origin));
    }
    let scoped = |prefix: &str| {
        name.strip_prefix(prefix).is_some_and(|digest| {
            digest.len() == 64
                && digest
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
    };
    if scoped("task-delivery/") && source.format == "task-outbox-json-v1" {
        return Some((Kind::TaskMessage, origin));
    }
    if matches!(origin, sources::Origin::Next)
        && scoped("task-start/")
        && source.format == "task-start-outbox-json-v1"
    {
        return Some((Kind::TaskStart, origin));
    }
    None
}

pub(super) fn support(source: &SourceRecord) -> Support {
    if family(source).is_some() {
        Support::Decodable
    } else if matches!(
        source.family.as_str(),
        "qt-pending-reads"
            | "next-pending-reads"
            | "qt-shared-read-migrated"
            | "qt-approval-policy"
            | "next-approval-policy"
    ) && source.schema == 1
    {
        Support::OtherResponsibility
    } else {
        Support::Unsupported
    }
}

pub(super) fn kind(source: &SourceRecord) -> Option<Kind> {
    family(source).map(|(kind, _)| kind)
}

fn text(value: &Value, key: &str) -> Result<String, Error> {
    value[key]
        .as_str()
        .filter(|text| text.len() <= 8192 && !text.chars().any(char::is_control))
        .map(str::to_owned)
        .ok_or(Error::InvalidEvidence)
}

pub(super) fn decode(
    source: &SourceRecord,
    page: &SnapshotPage,
    manifest_page: usize,
    bytes: &[u8],
) -> Result<Option<Operation>, Error> {
    let (kind, origin) = family(source).ok_or(Error::InvalidEvidence)?;
    let value: Value = serde_json::from_slice(bytes).map_err(|_| Error::InvalidEvidence)?;
    let mut requests = BTreeMap::new();
    let (id, scope, phase, terminal) = match kind {
        Kind::Creation => {
            if value["kind"] == "journal-header" && page.index == 0 {
                // Share the existing journal schema decoder, without loading
                // the state machine or treating its header as an operation.
                let mut header = value["content"]
                    .as_object()
                    .cloned()
                    .ok_or(Error::InvalidEvidence)?;
                if header.contains_key("operations") {
                    return Err(Error::InvalidEvidence);
                }
                header.insert("operations".into(), serde_json::json!({}));
                creation::decode_journal(
                    &serde_json::to_vec(&header).map_err(|_| Error::InvalidEvidence)?,
                )
                .map_err(|_| Error::InvalidEvidence)?;
                return Ok(None);
            }
            let id = text(&value, "id")?;
            let operation = &value["content"];
            if value["kind"] != "creation"
                || !(1..=128).contains(&page.index)
                || !creation::valid_operation(&id, operation)
            {
                return Err(Error::InvalidEvidence);
            }
            for (key, field) in [
                ("clone", "clone_id"),
                ("worktree", "worktree_id"),
                ("launch", "launch_id"),
            ] {
                if operation.get(field).is_some() {
                    requests.insert(key.into(), text(operation, field)?);
                }
            }
            let computer = if operation.get("host").is_some() {
                text(operation, "host")?
            } else {
                String::new()
            };
            let scope = Scope::Creation {
                computer,
                expected_machine: operation
                    .get("expected_machine")
                    .map(|_| text(operation, "expected_machine"))
                    .transpose()?,
            };
            let phase = text(operation, "state")?;
            let terminal = matches!(phase.as_str(), "bound" | "stopped" | "failed");
            (id, scope, phase, terminal)
        }
        Kind::TaskMessage | Kind::TaskStart => {
            if page.index != 0 {
                return Err(Error::InvalidEvidence);
            }
            let project = text(&value, "project")?;
            let task = text(&value, "task")?;
            let marker_id = text(&value, "marker_id")?;
            let record = &value["record"];
            let bytes = serde_json::to_vec(record).map_err(|_| Error::InvalidEvidence)?;
            // These G3 adapters reuse the live outbox and original guarded
            // plan checks. They are pure: no read, reservation or task call.
            let checked = if kind == Kind::TaskStart {
                sources::task_start_delivery(&project, &task, &marker_id, &bytes)
            } else {
                sources::task_delivery(origin, &project, &task, &marker_id, &bytes)
            }
            .map_err(|_| Error::InvalidEvidence)?;
            if checked.identity().family != source.family {
                return Err(Error::InvalidEvidence);
            }
            let id = text(record, "id")?;
            requests.insert("delivery".into(), id.clone());
            let phase = if kind == Kind::TaskStart {
                text(record, "phase")?
            } else {
                // A message outbox is an unresolved intent, even if an opaque
                // future field claims success. Only an exact live receipt can
                // settle it under its existing owner's command semantics.
                "pending".into()
            };
            let terminal =
                kind == Kind::TaskStart && matches!(phase.as_str(), "started" | "released");
            (id, Scope::Task { project, task }, phase, terminal)
        }
    };
    Ok(Some(Operation {
        family: source.family.clone(),
        manifest_page,
        source_index: page.index,
        snapshot_sha256: page.snapshot.sha256.clone(),
        kind,
        id,
        requests,
        scope,
        phase,
        recorded_state: if terminal {
            RecordedState::Terminal
        } else {
            RecordedState::NeedsReview
        },
    }))
}
