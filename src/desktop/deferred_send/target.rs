//! The selected session's actual backend and run, checked last before the
//! original child (zerus-5wmh.66.4.4, slice 3). The session name's agent
//! prefix only routes a send to this kind; it never authorizes executing it.
//!
//! From the held original state root ([`super::receipt::Roots`]), without
//! following a link: the CLI's own session record for the name (its
//! `name_routes` route or the legacy file) must name this session, a terminal
//! agent, and the plan's run and conversation; and neither an OpenCode nor a
//! DeepSeek binding may exist for the name, since `hgs send` routes those
//! names to their own native backends. Anything else, an unreadable record
//! included, refuses before the child: nothing ran.
use super::receipt::Roots;
use sha2::{Digest, Sha256};
use std::io::{self, Read};
use zerus_client_core::desktop::{creation_launch::AGENTS, deferred_send::Plan};

/// The largest session record read.
const RECORD_BYTES: u64 = 1 << 20;

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
    if json_at(roots, path).is_some_and(|record| names_plan(&record)) {
        return true;
    }
    // A session in the ID layout (zerus-ucck P3a), whose label's own record
    // is another session's or none: the label index names its candidates, and
    // each is opened at its fixed `sessions/<id>.json`, never a path from
    // elsewhere. Exactly one may carry the plan's run and conversation; an
    // absent or unreadable index refuses, as an unreadable record does.
    let Some(index) = json_at(roots, &["labels", &file]) else {
        return false;
    };
    if index["version"] != 1 || string(&index, "label") != plan.session {
        return false;
    }
    let ids = index["ids"].as_array().into_iter().flatten();
    let found = ids
        .filter_map(|id| id.as_str())
        .filter(|id| {
            uuid::Uuid::parse_str(id).is_ok_and(|uuid| uuid.hyphenated().to_string() == *id)
        })
        .filter(|id| {
            json_at(roots, &["sessions", &format!("{id}.json")]).is_some_and(|record| {
                record["layout"] == "id"
                    && string(&record, "session_id") == *id
                    && names_plan(&record)
            })
        })
        .count();
    found == 1
}

/// The JSON object at `path` under the held root, bounded; None when it is
/// absent or cannot be read.
fn json_at(roots: &Roots, path: &[&str]) -> Option<serde_json::Value> {
    let record = (|| -> io::Result<serde_json::Value> {
        let file = roots.open_file(path)?;
        let mut bytes = Vec::new();
        (&file).take(RECORD_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > RECORD_BYTES {
            return Err(io::ErrorKind::InvalidData.into());
        }
        serde_json::from_slice(&bytes).map_err(io::Error::other)
    })();
    record.ok()
}
