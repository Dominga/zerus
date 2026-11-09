//! One bounded UUID-index hit, without name resolution, reconciliation or a
//! fallback scan. This is record evidence only: the service separately proves
//! native process ancestry and the actually loaded adapter before any effect.
use super::stop_facts::verify_writer;
use crate::state::{repositories::directory::Directory, storage};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};
use zerus_desktop_ledger::task_controls::{Executor, TASK_CONTROLS};

type Result<T> = std::result::Result<T, String>;
const INDEX_BYTES: usize = 4096;

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Process {
    pub pid: u32,
    pub recorded_start: String,
}

// Deliberately not serializable or Debug: native records can contain private
// launch arguments. Only these bounded, independently revalidated pins escape.
pub(crate) struct Observed {
    pub executor: Executor,
    pub agent: String,
    pub process: Process,
    pub supervisor: Process,
}

#[derive(Clone, Copy)]
pub(crate) enum Conversation {
    Exact,
    /// Only the original request's scoped startup begin may select this mode.
    FirstBinding,
}

pub(crate) fn read(
    root: &Path,
    writer: &storage::Guard,
    computer: &str,
    expected: &Executor,
    conversation: Conversation,
) -> Result<Option<Observed>> {
    if !TASK_CONTROLS {
        return Ok(None);
    }
    read_enabled(root, writer, computer, expected, conversation).map(Some)
}

fn read_enabled(
    path: &Path,
    writer: &storage::Guard,
    computer: &str,
    expected: &Executor,
    conversation: Conversation,
) -> Result<Observed> {
    expected.validate().map_err(err)?;
    if expected.computer != computer || expected.child.is_some() {
        return Err("The local executor or independently addressable child is unverified".into());
    }
    let root = Directory::open(path)?;
    private(&root)?;
    verify_writer(&root, writer)?;
    let ids = root.child("ids")?;
    let entry = json(&ids, &format!("{}.json", expected.session_id), INDEX_BYTES)?;
    let name = text(&entry, "name", 2048)?;
    if entry["version"] != 1 {
        return Err("Unsupported native session index; refresh or review required".into());
    }
    let (directory, filename, id_layout) = match entry["kind"].as_str() {
        Some("id") => (
            root.child("sessions")?,
            format!("{}.json", expected.session_id),
            true,
        ),
        Some("record") => {
            // This is the existing label-holder route from the UUID index,
            // never a name selector or search for a newer run. A present route
            // is read no-follow; damaged or unexpected objects refuse.
            let filename = format!("{:x}.json", Sha256::digest(name.as_bytes()));
            let routed = match std::fs::symlink_metadata(root.path.join("name_routes")) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
                Err(e) => return Err(err(e)),
                Ok(_) => {
                    let routes = root.child("name_routes")?;
                    private(&routes)?;
                    match std::fs::symlink_metadata(routes.path.join(&filename)) {
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
                        Err(e) => return Err(err(e)),
                        Ok(_) => {
                            let _route = json(&routes, &filename, INDEX_BYTES)?;
                            true
                        }
                    }
                }
            };
            let directory = if routed {
                root.child("bindings")?
            } else {
                Directory::open(&root.path)?
            };
            (directory, filename, false)
        }
        // Shared provider hosts do not prove a particular native agent's
        // ancestry. They need their own loaded, scoped adapter qualification.
        _ => return Err("The selected native adapter has no verified task-control path".into()),
    };
    let record = json(&directory, &filename, storage::ID_RECORD_BYTES as usize)?;
    if record["version"] != 1
        || record["session_id"] != expected.session_id
        || record["name"] != name
        || (record["layout"] == "id") != id_layout
        || record["run_id"] != expected.run
        || ["archive_id", "archived_at"]
            .iter()
            .any(|key| !record[*key].is_null())
        || ["paused", "pausing", "terminating", "rename_shadow"]
            .iter()
            .any(|key| !record[*key].is_null() && record[*key] != false)
        || match record.get("state") {
            None | Some(Value::Null) => false, // Compatible legacy absence only.
            Some(Value::String(s)) => !matches!(s.as_str(), "live" | "active"),
            Some(_) => true, // Present malformed liveness is unknown, never live.
        }
    {
        return Err("The indexed native session, original run or live binding changed".into());
    }
    let current_conversation = match record.get("conversation_id") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if s.is_empty() => None,
        Some(Value::String(s)) => Some(s.clone()),
        _ => return Err("The native conversation binding is unverified".into()),
    };
    let first = matches!(conversation, Conversation::FirstBinding)
        && expected.conversation.is_none()
        && current_conversation.is_some();
    if current_conversation != expected.conversation && !first {
        return Err("The original native conversation changed".into());
    }
    if matches!(conversation, Conversation::FirstBinding) && !first {
        return Err("The first native conversation is not confirmed".into());
    }
    let mut executor = expected.clone();
    executor.conversation = current_conversation;
    executor.validate().map_err(err)?;
    let agent_process = process(&record, "pid", "process_start")?;
    let supervisor = process(&record["supervisor"], "pid", "start")?;
    if agent_process.pid == supervisor.pid {
        return Err("The native agent has not been independently published".into());
    }
    let agent = text(&record, "agent", 64)?.to_owned();
    if !matches!(agent.as_str(), "claude" | "codex" | "kimi") {
        return Err("The selected native adapter is not qualified for task controls".into());
    }
    root.verify()?;
    ids.verify()?;
    directory.verify()?;
    verify_writer(&root, writer)?;
    Ok(Observed {
        executor,
        agent,
        process: agent_process,
        supervisor,
    })
}

fn private(directory: &Directory) -> Result<()> {
    directory.verify()?;
    if !directory.private()? {
        return Err("The native session directory is not private".into());
    }
    Ok(())
}
fn json(directory: &Directory, name: &str, limit: usize) -> Result<Value> {
    private(directory)?;
    let file = directory.read_private(name)?;
    let identity = crate::platform::FileIdentity::of(&file).map_err(err)?;
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(err)?;
    if bytes.len() > limit {
        return Err("The native session evidence exceeds its bound".into());
    }
    let value = serde_json::from_slice(&bytes)
        .map_err(|_| "The native session evidence is unreadable".to_owned())?;
    let current = directory.read_private(name)?;
    if crate::platform::FileIdentity::of(&current).map_err(err)? != identity {
        return Err("The original native session evidence changed".into());
    }
    private(directory)?;
    Ok(value)
}
fn text<'a>(record: &'a Value, key: &str, bound: usize) -> Result<&'a str> {
    record[key]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= bound && !s.chars().any(char::is_control))
        .ok_or_else(|| "The native session identity is unverified".into())
}
fn process(record: &Value, pid: &str, birth: &str) -> Result<Process> {
    let pid = record[pid]
        .as_u64()
        .filter(|pid| *pid > 0 && *pid <= u32::MAX as u64)
        .ok_or("The native process identity is unverified")? as u32;
    Ok(Process {
        pid,
        recorded_start: text(record, birth, 512)?.to_owned(),
    })
}
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[cfg(test)]
mod tests;
