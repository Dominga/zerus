//! WhiteWh fork (owner, 2026-11-18): Hermes Agent (Nous Research) as a tracked
//! agent, verified with Hermes 0.23.2 on Windows.
//!
//! Hermes runs shell hooks from the `hooks:` block of its configuration and
//! deep-merges a managed layer (`HERMES_MANAGED_DIR/config.yaml`) over the
//! user's file. hgs never edits the user's `config.yaml`: a session it starts
//! gets `HERMES_MANAGED_DIR` naming hgs's own layer, which holds only hgs's
//! hooks, and hgs's exact `(event, command)` pairs are added to Hermes's
//! consent list (`shell-hooks-allowlist.json` in the Hermes home), so no
//! consent prompt stops the start. The layer is JSON, which YAML reads as is.
//!
//! Each Hermes payload (`hook_event_name`, `session_id`, `tool_name`,
//! `tool_input`, `extra`) becomes the Claude-shaped event hgs tracks. Hermes
//! names a session `local-<hex>`; hgs's conversation is a UUID derived from it
//! (`conversation_id`), and the session's turns are kept in a Claude-format
//! transcript under hgs's state (`transcript`) whose first line names the
//! Hermes session a resume opens (`native_id`).
use super::*;
use std::fs;
use std::io::{BufRead, BufReader, Read as _, Write as _};

pub(super) const AGENT: &str = "hermes";

/// Hermes events hgs registers, as Hermes names them.
pub(super) const HOOK_EVENTS: &[&str] = &[
    "on_session_open",
    "pre_llm_call",
    "post_llm_call",
    "pre_tool_call",
    "post_tool_call",
    "pre_approval_request",
    "post_approval_response",
    "on_human_input_request",
    "on_human_input_resolved",
    "on_session_end",
    "on_session_finalize",
];

/// The Claude-shaped events a Hermes session produces.
pub(super) const TRACKED: &[&str] = &[
    "SessionStart",
    "SessionEnd",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PermissionRequest",
    "PermissionResult",
    "Stop",
    "StopFailure",
];

/// The hgs conversation of a Hermes session: a UUID from the SHA-256 of a
/// fixed prefix and the Hermes id, so the same session is always the same
/// conversation.
pub(super) fn conversation_id(native: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(format!("zerus-hermes-conversation
{native}").as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    uuid::Builder::from_random_bytes(bytes).into_uuid().to_string()
}

fn transcripts() -> PathBuf {
    root().join("hermes-transcripts")
}

/// The Claude-format transcript hgs keeps for a Hermes conversation.
pub(super) fn transcript(conversation: &str) -> PathBuf {
    transcripts().join(format!("{conversation}.jsonl"))
}

/// The Hermes session a conversation came from, from its transcript's first line.
pub(super) fn native_id(conversation: &str) -> Option<String> {
    let file = fs::File::open(transcript(conversation)).ok()?;
    let mut first = String::new();
    BufReader::new(file.take(64 * 1024)).read_line(&mut first).ok()?;
    let header: Value = serde_json::from_str(first.trim()).ok()?;
    (header["type"] == "hermes-session" && header["sessionId"] == conversation)
        .then(|| string(&header, "hermes_session_id").to_owned())
        .filter(|id| valid_native(id))
}

fn valid_native(id: &str) -> bool {
    !id.is_empty() && id.len() <= 256 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
}

/// Append one Claude-format line, writing the transcript's header first.
fn append(conversation: &str, native: &str, line: &Value) -> Result<()> {
    let path = transcript(conversation);
    fs::create_dir_all(transcripts()).map_err(|e| e.to_string())?;
    let mut file = fs::OpenOptions::new().create(true).append(true).open(&path).map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() == 0 {
        let header = json!({"type":"hermes-session","sessionId":conversation,"hermes_session_id":native,"timestamp":now()});
        writeln!(file, "{header}").map_err(|e| e.to_string())?;
    }
    writeln!(file, "{line}").map_err(|e| e.to_string())
}

/// The newest reply in a conversation's transcript, read from its last 256 KiB.
fn last_reply(conversation: &str) -> Option<String> {
    use std::io::{Seek, SeekFrom};
    let mut file = fs::File::open(transcript(conversation)).ok()?;
    let length = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(length.saturating_sub(256 * 1024))).ok()?;
    let mut tail = String::new();
    file.read_to_string(&mut tail).ok()?;
    tail.lines().rev().find_map(|line| {
        let line: Value = serde_json::from_str(line).ok()?;
        (line["type"] == "assistant").then(|| line["message"]["content"][0]["text"].as_str().map(str::to_owned)).flatten()
    })
}

/// The Claude-shaped event of one Hermes hook payload, or None when hgs takes
/// nothing from it. A turn's prompt and reply are added to the transcript.
pub(super) fn translate(payload: &Value) -> Result<Option<Value>> {
    let native = string(payload, "session_id");
    if !valid_native(native) {
        return Ok(None);
    }
    let conversation = conversation_id(native);
    let extra = &payload["extra"];
    let base = |name: &str| {
        json!({
            "hook_event_name": name,
            "session_id": conversation,
            "transcript_path": transcript(&conversation).to_string_lossy(),
            "hermes_session_id": native,
        })
    };
    let tool_id = string(extra, "tool_call_id");
    let event = match string(payload, "hook_event_name") {
        "on_session_open" => {
            let mut event = base("SessionStart");
            event["source"] = json!(if string(extra, "reason") == "create" { "startup" } else { "resume" });
            // The transcript exists from the start, so a resume finds its id.
            append(&conversation, native, &json!({"type":"hermes-open","sessionId":conversation,"timestamp":now()}))?;
            event
        }
        "pre_llm_call" => {
            let prompt = string(extra, "user_message");
            append(&conversation, native, &json!({
                "type":"user","sessionId":conversation,"uuid":uuid::Uuid::new_v4().to_string(),"timestamp":now(),
                "message":{"role":"user","content":prompt}}))?;
            let mut event = base("UserPromptSubmit");
            event["prompt"] = json!(prompt);
            event
        }
        "post_llm_call" => {
            let reply = string(extra, "assistant_response");
            if !reply.trim().is_empty() {
                append(&conversation, native, &json!({
                    "type":"assistant","sessionId":conversation,"uuid":uuid::Uuid::new_v4().to_string(),"timestamp":now(),
                    "message":{"role":"assistant","content":[{"type":"text","text":reply}]}}))?;
            }
            return Ok(None);
        }
        "pre_tool_call" | "post_tool_call" => {
            let mut event = base(if string(payload, "hook_event_name") == "pre_tool_call" { "PreToolUse" } else { "PostToolUse" });
            event["tool_name"] = json!(string(payload, "tool_name"));
            event["tool_input"] = payload["tool_input"].clone();
            event["tool_use_id"] = json!(tool_id);
            event
        }
        "pre_approval_request" => {
            let mut event = base("PermissionRequest");
            event["tool_name"] = json!("terminal");
            event["tool_input"] = json!({"command": string(extra, "command"), "description": string(extra, "description")});
            event["tool_use_id"] = json!(tool_id);
            event
        }
        // A clarifying question waits for the person like Claude's
        // AskUserQuestion; an approval's request is pre_approval_request's.
        "on_human_input_request" if string(extra, "kind") == "clarify" => {
            let mut event = base("PermissionRequest");
            event["tool_name"] = json!("AskUserQuestion");
            event["tool_input"] = json!({"question": string(extra, "prompt")});
            event["tool_use_id"] = json!(string(extra, "request_id"));
            event
        }
        "post_approval_response" | "on_human_input_resolved" => base("PermissionResult"),
        "on_session_end" => {
            let failed = extra["failed"] == true;
            let mut event = base(if failed { "StopFailure" } else { "Stop" });
            if failed {
                event["error"] = json!(string(extra, "turn_exit_reason"));
            } else if let Some(reply) = last_reply(&conversation) {
                // The turn's reply came with post_llm_call, before its end.
                event["last_assistant_message"] = json!(reply);
            }
            event
        }
        "on_session_finalize" => base("SessionEnd"),
        _ => return Ok(None),
    };
    Ok(Some(event))
}

/// The title a fresh hgs start gives its Hermes session (`chat -c TITLE
/// --create-if-missing`): hgs's durable session ID and the run, so a hook can
/// name the session whatever process runs it. Hermes 0.23 runs a turn in a
/// runtime shared by every client of its home, started with the first
/// client's environment, so a hook's `HGS_SESSION` may be another session's.
pub(super) fn session_title(session: &str, run: &str) -> String {
    format!("zerus-{session}-{}", run.chars().take(8).collect::<String>())
}

/// A fresh start names its new Hermes session; a resume keeps its own.
pub(super) fn name_session(argv: &mut Vec<String>, session: &str, run: &str) {
    let chosen = argv.iter().any(|arg| {
        let key = arg.split('=').next().unwrap_or(arg);
        ["-c", "--continue", "-r", "--resume", "chat", "-q", "--query", "-z"].contains(&key)
    });
    if !chosen && uuid::Uuid::parse_str(session).is_ok() {
        argv.extend(["chat".into(), "-c".into(), session_title(session, run), "--create-if-missing".into()]);
    }
}

/// The hgs session a Hermes session belongs to, from the title hgs gave it:
/// `(name, run_id)` of the live record, or None for a session hgs did not
/// start. Hermes commits the title as it creates the session; a hook that
/// comes first waits for it briefly.
pub(super) fn owner(native: &str) -> Option<(String, String)> {
    let database = hermes_home().join("state.db");
    let title = (0..10).find_map(|attempt| {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let connection = rusqlite::Connection::open_with_flags(&database, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
        let _ = connection.busy_timeout(std::time::Duration::from_secs(2));
        connection
            .query_row("SELECT title FROM sessions WHERE id = ?1", [native], |row| row.get::<_, Option<String>>(0))
            .ok()
            .flatten()
    })?;
    let session = owner_session(&title)?;
    let record = super::session_index::find(&session)?.record;
    let (name, run) = (string(&record, "name"), string(&record, "run_id"));
    if name.is_empty() || run.is_empty() {
        return None;
    }
    // This hook acts for the owner alone, whatever session started the runtime.
    std::env::set_var("HGS_SESSION", name);
    std::env::set_var("HGS_RUN_ID", run);
    std::env::set_var("HGS_SESSION_ID", &session);
    std::env::set_var("HGS_AGENT", AGENT);
    Some((name.to_owned(), run.to_owned()))
}

/// The hgs session ID a title names, if hgs gave it.
fn owner_session(title: &str) -> Option<String> {
    let rest = title.strip_prefix("zerus-")?;
    let id = rest.get(..36)?;
    uuid::Uuid::parse_str(id).ok().map(|id| id.to_string())
}

/// The Hermes home of this launch: `HERMES_HOME`, else Hermes's default.
fn hermes_home() -> PathBuf {
    if let Some(home) = nonempty_env("HERMES_HOME") {
        return PathBuf::from(home);
    }
    if cfg!(windows) {
        if let Some(local) = nonempty_env("LOCALAPPDATA") {
            return PathBuf::from(local).join("hermes");
        }
    }
    home().join(".hermes")
}

/// The command a Hermes hook runs: hgs's own `__state hook`.
fn hook_command(executable: &str) -> String {
    if cfg!(windows) {
        format!("\"{executable}\" __state hook")
    } else {
        format!("'{}' __state hook", executable.replace('\'', r"'\''"))
    }
}

/// hgs's managed layer: only its hooks.
pub(super) fn managed_config(command: &str) -> Value {
    let hooks: serde_json::Map<String, Value> = HOOK_EVENTS
        .iter()
        .map(|event| ((*event).to_owned(), json!([{"command": command, "timeout": 30}])))
        .collect();
    json!({"hooks": hooks})
}

/// Add hgs's pairs to Hermes's consent list, keeping every other entry.
pub(super) fn allow(list: &mut Value, command: &str, at: &str) -> bool {
    if !list.is_object() {
        *list = json!({});
    }
    if !list["approvals"].is_array() {
        list["approvals"] = json!([]);
    }
    let approvals = list["approvals"].as_array_mut().expect("approvals is an array");
    let mut changed = false;
    for event in HOOK_EVENTS {
        if !approvals.iter().any(|entry| entry["event"] == *event && entry["command"] == command) {
            approvals.push(json!({"event": event, "command": command, "approved_at": at, "script_mtime_at_approval": null}));
            changed = true;
        }
    }
    changed
}

/// Before a Hermes start: write hgs's managed layer, consent to its hooks and
/// point this launch at the layer. An administrator's managed scope
/// (`HERMES_MANAGED_DIR`, or `/etc/hermes` on Unix) stays in force: hgs adds
/// nothing then, and the session is not tracked by hooks.
pub(super) fn prepare(executable: &str) -> Result<()> {
    if nonempty_env("HERMES_MANAGED_DIR").is_some() || (cfg!(unix) && Path::new("/etc/hermes").is_dir()) {
        eprintln!("hgs: Hermes has a managed configuration of its own; its session is not tracked by hooks");
        return Ok(());
    }
    let command = hook_command(executable);
    let dir = root().join("hermes-managed");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let wanted = format!("{}\n", serde_json::to_string_pretty(&managed_config(&command)).map_err(|e| e.to_string())?);
    let path = dir.join("config.yaml");
    if fs::read_to_string(&path).ok().as_deref() != Some(wanted.as_str()) {
        atomic(&path, &wanted)?;
    }
    let home = hermes_home();
    let allowlist = home.join("shell-hooks-allowlist.json");
    let mut list = match fs::read(&allowlist) {
        Ok(bytes) => serde_json::from_slice::<Value>(super::without_bom(&bytes))
            .map_err(|_| format!("{} is not readable JSON; hgs leaves it as it is", allowlist.display()))?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => json!({"approvals": []}),
        Err(error) => return Err(error.to_string()),
    };
    if allow(&mut list, &command, &iso_now()) {
        fs::create_dir_all(&home).map_err(|e| e.to_string())?;
        atomic(&allowlist, &format!("{}\n", serde_json::to_string_pretty(&list).map_err(|e| e.to_string())?))?;
    }
    std::env::set_var("HERMES_MANAGED_DIR", &dir);
    Ok(())
}

fn iso_now() -> String {
    let seconds = now() as i64;
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", rest / 3600, rest % 3600 / 60, rest % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NATIVE: &str = "local-0a18fd4b950342dc23670e318f562207594daeb1c4d5ad3d85f36cca4dbd8230";

    fn payload(event: &str, extra: Value) -> Value {
        json!({"hook_event_name":event,"tool_name":null,"tool_input":null,"session_id":NATIVE,"cwd":"C:\\x","profile":"default","extra":extra})
    }

    #[test]
    fn a_hermes_session_is_one_stable_uuid_conversation() {
        let id = conversation_id(NATIVE);
        assert!(uuid::Uuid::parse_str(&id).is_ok());
        assert_eq!(id, conversation_id(NATIVE));
        assert_ne!(id, conversation_id("local-other"));
        assert!(valid_native(NATIVE) && !valid_native("") && !valid_native("a b"));
    }

    #[test]
    fn hermes_events_become_the_events_hgs_tracks() {
        let _guard = crate::state::ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let old = std::env::var("HGS_STATE_DIR").ok();
        std::env::set_var("HGS_STATE_DIR", dir.path());
        let conversation = conversation_id(NATIVE);
        let open = translate(&payload("on_session_open", json!({"reason":"create"}))).unwrap().unwrap();
        assert_eq!((open["hook_event_name"].as_str(), open["source"].as_str()), (Some("SessionStart"), Some("startup")));
        assert_eq!(open["session_id"], conversation);
        let resumed = translate(&payload("on_session_open", json!({"reason":"resume"}))).unwrap().unwrap();
        assert_eq!(resumed["source"], "resume");
        let prompt = translate(&payload("pre_llm_call", json!({"user_message":"Say hi"}))).unwrap().unwrap();
        assert_eq!((prompt["hook_event_name"].as_str(), prompt["prompt"].as_str()), (Some("UserPromptSubmit"), Some("Say hi")));
        assert!(translate(&payload("post_llm_call", json!({"assistant_response":"Hi there"}))).unwrap().is_none());
        let mut tool = payload("pre_tool_call", json!({"tool_call_id":"call_1"}));
        tool["tool_name"] = json!("terminal");
        tool["tool_input"] = json!({"command":"echo hi"});
        let tool = translate(&tool).unwrap().unwrap();
        assert_eq!((tool["hook_event_name"].as_str(), tool["tool_use_id"].as_str()), (Some("PreToolUse"), Some("call_1")));
        assert_eq!(tool["tool_input"]["command"], "echo hi");
        let ask = translate(&payload("pre_approval_request", json!({"command":"rm -rf x","tool_call_id":"call_1"}))).unwrap().unwrap();
        assert_eq!((ask["hook_event_name"].as_str(), ask["tool_input"]["command"].as_str()), (Some("PermissionRequest"), Some("rm -rf x")));
        assert!(translate(&payload("on_human_input_request", json!({"kind":"approval"}))).unwrap().is_none());
        let clarify = translate(&payload("on_human_input_request", json!({"kind":"clarify","prompt":"Which?","request_id":"r1"}))).unwrap().unwrap();
        assert_eq!(clarify["tool_name"], "AskUserQuestion");
        assert_eq!(translate(&payload("post_approval_response", json!({}))).unwrap().unwrap()["hook_event_name"], "PermissionResult");
        let stop = translate(&payload("on_session_end", json!({"completed":true,"failed":false}))).unwrap().unwrap();
        assert_eq!((stop["hook_event_name"].as_str(), stop["last_assistant_message"].as_str()), (Some("Stop"), Some("Hi there")));
        let failed = translate(&payload("on_session_end", json!({"failed":true,"turn_exit_reason":"api_error"}))).unwrap().unwrap();
        assert_eq!((failed["hook_event_name"].as_str(), failed["error"].as_str()), (Some("StopFailure"), Some("api_error")));
        assert_eq!(translate(&payload("on_session_finalize", json!({}))).unwrap().unwrap()["hook_event_name"], "SessionEnd");
        assert!(translate(&payload("on_session_start", json!({}))).unwrap().is_none());
        // The transcript holds the turn in Claude's format and names the session.
        assert_eq!(native_id(&conversation).as_deref(), Some(NATIVE));
        let text = fs::read_to_string(transcript(&conversation)).unwrap();
        let lines: Vec<Value> = text.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
        let user = lines.iter().find(|line| line["type"] == "user").unwrap();
        assert_eq!(user["message"]["content"], "Say hi");
        let reply = lines.iter().find(|line| line["type"] == "assistant").unwrap();
        assert_eq!(reply["message"]["content"][0]["text"], "Hi there");
        assert!(lines.iter().all(|line| line["sessionId"] == conversation));
        match old {
            Some(value) => std::env::set_var("HGS_STATE_DIR", value),
            None => std::env::remove_var("HGS_STATE_DIR"),
        }
    }

    #[test]
    fn a_fresh_start_names_its_hermes_session_and_a_resume_keeps_its_own() {
        let session = "f2e7d6e8-2a34-46a3-92ae-e633cd46e63c";
        let title = session_title(session, "df392d1c-c521-4d60-92e2-cd69af50d3a3");
        assert_eq!(title, "zerus-f2e7d6e8-2a34-46a3-92ae-e633cd46e63c-df392d1c");
        assert_eq!(owner_session(&title).as_deref(), Some(session));
        assert_eq!(owner_session("OK from fake #7"), None);
        assert_eq!(owner_session("zerus-not-a-uuid"), None);
        let mut fresh = vec!["hermes".to_owned(), "--yolo".to_owned()];
        name_session(&mut fresh, session, "df392d1c");
        assert_eq!(fresh[2..], ["chat", "-c", "zerus-f2e7d6e8-2a34-46a3-92ae-e633cd46e63c-df392d1c", "--create-if-missing"]);
        let mut resumed = vec!["hermes".to_owned(), "--resume".to_owned(), NATIVE.to_owned()];
        name_session(&mut resumed, session, "df392d1c");
        assert_eq!(resumed.len(), 3, "a resume opens its own session");
    }

    #[test]
    fn the_managed_layer_holds_only_hgs_hooks_and_consent_keeps_other_entries() {
        let command = hook_command(r"C:\Program Files\Zerus\hgs.exe");
        let layer = managed_config(&command);
        assert_eq!(layer.as_object().unwrap().len(), 1);
        assert_eq!(layer["hooks"].as_object().unwrap().len(), HOOK_EVENTS.len());
        assert_eq!(layer["hooks"]["pre_llm_call"][0]["command"], command);
        let mut list = json!({"approvals":[{"event":"pre_tool_call","command":"mine.sh"}],"other":1});
        assert!(allow(&mut list, &command, "2026-11-18T00:00:00Z"));
        assert!(!allow(&mut list, &command, "2026-11-18T00:00:00Z"), "a second start adds nothing");
        let approvals = list["approvals"].as_array().unwrap();
        assert_eq!(approvals.len(), HOOK_EVENTS.len() + 1);
        assert_eq!(approvals[0]["command"], "mine.sh");
        assert_eq!(list["other"], 1);
        if cfg!(windows) {
            assert_eq!(command, r#""C:\Program Files\Zerus\hgs.exe" __state hook"#);
        }
        assert_eq!(iso_now().len(), 20);
    }
}
