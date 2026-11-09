//! Observe a clear successor only from a new real main hook and the
//! original supervised Codex's actually open native rollout. Never replay input
//! or fabricate a missing SessionStart event.
use super::*;
use crate::platform;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader};
use std::time::{Duration, Instant};

pub(super) const MISMATCH: &str = "agent resumed a different conversation";

pub(super) struct Successor {
    pub transcript: PathBuf,
    _header: File,
}

pub(super) fn observe(record: &Value, event: &Value) -> Option<Successor> {
    // Stop/PostToolUse are completion observations admitted during Terminate,
    // but must not turn into a successor's main-start binding transition.
    if terminating::pending(record)
        || !clear_context::recovering_resume(record)
        || !string(event, "agent_id").is_empty()
        || ![
            "SessionStart",
            "UserPromptSubmit",
            "PreToolUse",
            "PostToolUse",
            "Stop",
        ]
        .contains(&string(event, "hook_event_name"))
        || (event["hook_event_name"] == "SessionStart" && event["source"] != "clear")
        || !external_agents::main_codex_hook(record)
    {
        return None;
    }
    prove(
        record,
        event,
        record["clear_context_request"]["at"].as_f64()?,
    )
}

/// A fresh, unrequested Terminal clear is separate from recovery of an old
/// refusal. It needs the exact native end binding, the original open rollout,
/// and a genuine clear SessionStart. Never synthesize a request for this path.
pub(super) fn terminal_start(record: &Value, event: &Value) -> Option<Successor> {
    if terminating::pending(record)
        || !clear_context::ended_terminal_resume(record)
        || event["hook_event_name"] != "SessionStart"
        || event["source"] != "clear"
        || !string(event, "agent_id").is_empty()
        || !external_agents::main_codex_hook(record)
        || resume_input::owned(record).is_err()
    {
        return None;
    }
    // Native creation can precede the main end, so bind it to the original run
    // rather than the later receipt time of that end.
    let created = record["created"].as_f64().filter(|at| *at > 0.0)?;
    prove(record, event, created)
}

fn prove(record: &Value, event: &Value, since: f64) -> Option<Successor> {
    let id = string(event, "session_id");
    if uuid::Uuid::parse_str(id).is_err() || id == string(record, "conversation_id") {
        return None;
    }
    let until = Instant::now() + Duration::from_secs(2);
    let pid = resume_input::checked_process(record).ok()?;
    let original = platform::process_identity(u32::try_from(pid).ok()?)?;
    let account = if string(record, "agent_home").is_empty() {
        home().join(".codex")
    } else {
        PathBuf::from(string(record, "agent_home"))
    };
    let sessions = platform::canonical(account.join("sessions")).ok()?;
    let path = platform::canonical(string(event, "transcript_path")).ok()?;
    if !path.starts_with(sessions)
        || !path
            .file_name()?
            .to_str()?
            .ends_with(&format!("{id}.jsonl"))
    {
        return None;
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    platform::no_follow(&mut options);
    #[cfg(windows)]
    platform::refuse_link(&mut options);
    let mut file = options.open(&path).ok()?;
    let (identity, metadata) = platform::FileIdentity::with_metadata(&file).ok()?;
    if !metadata.is_file() {
        return None;
    }
    let mut first = Vec::new();
    BufReader::new((&mut file).take(256 * 1024 + 1))
        .read_until(b'\n', &mut first)
        .ok()?;
    if first.len() > 256 * 1024 || !first.ends_with(b"\n") {
        return None;
    }
    let header: Value = serde_json::from_slice(&first).ok()?;
    let meta = &header["payload"];
    let created = chrono::DateTime::parse_from_rfc3339(string(meta, "timestamp"))
        .ok()?
        .timestamp_millis() as f64
        / 1000.0;
    let cwd = platform::canonical(string(record, "cwd")).ok()?;
    if header["type"] != "session_meta"
        || meta["id"] != id
        || meta["source"] != "cli"
        || meta.get("forked_from_id").is_some_and(|v| !v.is_null())
        || meta.get("parent_thread_id").is_some_and(|v| !v.is_null())
        || platform::canonical(string(meta, "cwd")).ok()? != cwd
        || (!string(event, "cwd").is_empty()
            && platform::canonical(string(event, "cwd")).ok()? != cwd)
        || created < since
        || created > now() + 1.0
        || platform::holds_open(u32::try_from(pid).ok()?, &path).ok()? != platform::OpenFiles::Held
    {
        return None;
    }
    let current = platform::file_identity(&path)?;
    if (identity.device, identity.inode) != (current.device, current.inode)
        || resume_input::checked_process(record).ok()? != pid
        || platform::process_identity(u32::try_from(pid).ok()?)? != original
        || Instant::now() >= until
    {
        return None;
    }
    Some(Successor {
        transcript: path,
        _header: file,
    })
}
