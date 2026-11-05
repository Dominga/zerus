//! The selected session's actual backend and run, checked last before the
//! original child (zerus-5wmh.66.4.4, slice 3). The session name's agent
//! prefix only routes a send to this kind; it never authorizes executing it.
//!
//! From the held original state root ([`super::receipt::Roots`]), without
//! following a link: exactly one of the name's session records must name
//! this session, a terminal agent, and the plan's run and conversation. Those
//! records are the CLI's own record for the name (its `name_routes` route or
//! the legacy file) and, where the ID layout exists (zerus-ucck P3a), every
//! session its label index proves for the name. Neither an OpenCode nor a
//! DeepSeek binding may exist for the name, since `hgs send` routes those
//! names to their own native backends. Anything else, an unreadable record
//! or an index that cannot be proven included, refuses before the child:
//! nothing ran.
use super::receipt::Roots;
use sha2::{Digest, Sha256};
use std::io::{self, Read};
use zerus_client_core::desktop::{creation_launch::AGENTS, deferred_send::Plan};

/// The largest session record read.
const RECORD_BYTES: u64 = 1 << 20;
/// The largest label-index manifest read, as the CLI bounds it.
const MANIFEST_BYTES: u64 = 16 << 20;

fn string<'a>(value: &'a serde_json::Value, key: &str) -> &'a str {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
}

/// Whether the plan's session is a terminal agent's, running the plan's run
/// and conversation, with no native binding.
pub(super) fn current(roots: &Roots, plan: &Plan) -> bool {
    let file = format!("{:x}.json", Sha256::digest(plan.session.as_bytes()));
    // `hgs send` sends an OpenCode or DeepSeek name natively.
    for backend in ["opencode", "dsh"] {
        match roots.absent(&[backend, "bindings", &file]) {
            Ok(true) => (),
            _ => return false,
        }
    }
    // The CLI's record path: a routed record lives in `bindings`.
    let routed = match roots.absent(&["name_routes", &file]) {
        Ok(absent) => !absent,
        Err(_) => return false,
    };
    let path: &[&str] = if routed {
        &["bindings", &file]
    } else {
        &[&file]
    };
    let names_plan = |record: &serde_json::Value| {
        string(record, "name") == plan.session
            && AGENTS.contains(&string(record, "agent"))
            && plan
                .session
                .split('/')
                .next()
                .is_some_and(|agent| agent == string(record, "agent"))
            && string(record, "run_id") == plan.run_id
            && string(record, "conversation_id") == plan.conversation_id
    };
    // The label's own record, read whole: one that is there but cannot be
    // read refuses, as an unreadable candidate does.
    let legacy = match bytes_at(roots, path, RECORD_BYTES) {
        Ok(None) => false,
        Ok(Some(bytes)) => match serde_json::from_slice::<serde_json::Value>(&bytes) {
            Ok(record) => names_plan(&record),
            Err(_) => return false,
        },
        Err(_) => return false,
    };
    // With no `sessions/` there is no ID layout (zerus-ucck P3a): the label's
    // own record decides, as before.
    match roots.absent(&["sessions"]) {
        Ok(true) => return legacy,
        Ok(false) => {}
        Err(_) => return false,
    }
    // Otherwise the label index must prove the label's sessions in the ID
    // layout before any answer (#1437 review): the manifest lists the label
    // with its file's digest, or proves it has none by not listing it. Each
    // candidate is opened at its fixed `sessions/<id>.json`, never a path
    // from elsewhere; one that is there but cannot be read refuses. The label's
    // own record and the candidates count together: exactly one may carry the
    // plan's run and conversation.
    let Some(ids) = proven(roots, plan, &file) else {
        return false;
    };
    let mut found = usize::from(legacy);
    for id in ids {
        match bytes_at(roots, &["sessions", &format!("{id}.json")], RECORD_BYTES) {
            // A stale entry: the session left the label.
            Ok(None) => {}
            Ok(Some(bytes)) => {
                let Ok(record) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                    return false;
                };
                if record["layout"] == "id"
                    && string(&record, "session_id") == id
                    && names_plan(&record)
                {
                    found += 1;
                }
            }
            Err(_) => return false,
        }
    }
    found == 1
}

/// The IDs the label index proves for the plan's label: none when the
/// manifest does not list the label, else exactly what the label's file
/// lists, which must hash to the manifest's entry. None when that cannot be
/// proven.
fn proven(roots: &Roots, plan: &Plan, file: &str) -> Option<Vec<String>> {
    let key = format!("{:x}", Sha256::digest(plan.session.as_bytes()));
    let manifest = bytes_at(roots, &["labels", ".complete"], MANIFEST_BYTES).ok()??;
    let manifest = serde_json::from_slice::<serde_json::Value>(&manifest).ok()?;
    if manifest["version"] != 1 || !manifest["labels"].is_object() {
        return None;
    }
    let Some(expected) = manifest["labels"].get(&key) else {
        return Some(Vec::new());
    };
    let index = bytes_at(roots, &["labels", file], RECORD_BYTES).ok()??;
    if expected.as_str() != Some(format!("{:x}", Sha256::digest(&index)).as_str()) {
        return None;
    }
    let index = serde_json::from_slice::<serde_json::Value>(&index).ok()?;
    if index["version"] != 1 || string(&index, "label") != plan.session {
        return None;
    }
    let canonical = |id: &str| uuid::Uuid::parse_str(id).is_ok_and(|uuid| uuid.hyphenated().to_string() == id);
    index["ids"]
        .as_array()?
        .iter()
        .map(|id| id.as_str().filter(|id| canonical(id)).map(str::to_owned))
        .collect()
}

/// The bytes at `path` under the held root, bounded: `Ok(None)` only when it
/// verifiably does not exist; anything there that cannot be read is an error.
fn bytes_at(roots: &Roots, path: &[&str], limit: u64) -> io::Result<Option<Vec<u8>>> {
    let file = match roots.open_file(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    (&file).take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(Some(bytes))
}

