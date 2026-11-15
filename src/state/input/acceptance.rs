//! Repair a missed acceptance hook from the exact native main-user input.
//! No receipt is replayed, and completion/absence alone never proves delivery.
use super::*;
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom};

const BYTES: u64 = 16 << 20;
const HEADER: u64 = 64 << 10;
const ROWS: usize = 16_384;

struct Source {
    rows: Vec<Value>,
    path: PathBuf,
    held: fs::File,
    stamp: (u64, u64, u64, std::time::SystemTime),
}

impl Source {
    fn current(&self) -> bool {
        stamp(&self.held) == Some(self.stamp)
            && crate::platform::no_follow(fs::OpenOptions::new().read(true))
                .open(&self.path)
                .ok()
                .and_then(|file| stamp(&file))
                == Some(self.stamp)
    }
}

fn stamp(file: &fs::File) -> Option<(u64, u64, u64, std::time::SystemTime)> {
    let (id, meta) = crate::platform::FileIdentity::with_metadata(file).ok()?;
    (meta.is_file() && id.inode != 0).then_some((
        id.device,
        id.inode,
        meta.len(),
        meta.modified().ok()?,
    ))
}

// A held regular file, bounded header + tail. A clipped row, replacement,
// truncation, concurrent append, gap or malformed row yields unknown.
fn source(path: &Path, conversation: Option<&str>) -> Option<Source> {
    if !path.is_absolute() {
        return None;
    }
    let open = || {
        crate::platform::no_follow(fs::OpenOptions::new().read(true))
            .open(path)
            .ok()
    };
    let mut file = open()?;
    let before = stamp(&file)?;
    if let Some(id) = conversation {
        let mut header = Vec::new();
        (&mut file).take(HEADER).read_to_end(&mut header).ok()?;
        let end = header.iter().position(|byte| *byte == b'\n')?;
        let row: Value = serde_json::from_slice(&header[..end]).ok()?;
        if row["type"] != "session_meta" || row["payload"]["id"] != id {
            return None;
        }
    }
    let start = before.2.saturating_sub(BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    (&mut file).take(BYTES + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > BYTES || stamp(&file)? != before || stamp(&open()?)? != before {
        return None;
    }
    let mut rows = Vec::new();
    for (index, line) in bytes.split_inclusive(|byte| *byte == b'\n').enumerate() {
        if start > 0 && index == 0 {
            continue;
        }
        if !line.ends_with(b"\n") || line.len() > 1024 * 1024 || rows.len() == ROWS {
            return None;
        }
        let row: Value = serde_json::from_slice(line).ok()?;
        if matches!(row["type"].as_str(), Some("gap" | "wire.gap")) {
            return None;
        }
        rows.push(row);
    }
    Some(Source {
        rows,
        path: path.into(),
        held: file,
        stamp: before,
    })
}

fn kimi(record: &Value) -> Option<Source> {
    // Presentation discovery canonicalizes paths and truncates its roster.
    // Acceptance needs the complete bounded roster and the original final
    // component, so a link or an unseen second match remains unknown.
    let conversation = string(record, "conversation_id");
    if !questions::token(conversation) {
        return None;
    }
    let root = crate::platform::canonical(questions::kimi_home(record).join("sessions")).ok()?;
    let mut selected = None;
    for (index, entry) in fs::read_dir(&root).ok()?.enumerate() {
        if index == 2000 {
            return None;
        }
        let entry = entry.ok()?;
        let path = entry
            .path()
            .join(conversation)
            .join("agents/main/wire.jsonl");
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return None,
        };
        if !metadata.is_file()
            || !crate::platform::canonical(&path).ok()?.starts_with(&root)
            || selected.replace(path).is_some()
        {
            return None;
        }
    }
    let path = selected?;
    let state = path.parent()?.parent()?.parent()?.join("state.json");
    let bytes = labels::read_file(&state, 64 << 10).ok()??;
    let state: Value = serde_json::from_slice(&bytes).ok()?;
    if state["id"] != record["conversation_id"] || state["version"] != 2 {
        return None;
    }
    source(&path, None)
}

fn text<'a>(parts: &'a Value, kind: &str) -> Option<&'a str> {
    let parts = parts.as_array()?;
    let [part] = parts.as_slice() else {
        return None;
    };
    (part["type"] == kind)
        .then(|| part["text"].as_str())
        .flatten()
}

fn consumed(rows: &[Value], agent: &str, pending: f64, submitted: &str) -> Option<f64> {
    rows.iter().find_map(|row| {
        let (at, input) = if agent == "codex" {
            let payload = &row["payload"];
            if row["type"] != "response_item"
                || payload["type"] != "message"
                || payload["role"] != "user"
            {
                return None;
            }
            let at = chrono::DateTime::parse_from_rfc3339(row["timestamp"].as_str()?)
                .ok()?
                .timestamp_micros() as f64
                / 1_000_000.0;
            (at, text(&payload["content"], "input_text")?)
        } else {
            if row["type"] != "turn.prompt"
                || row["agentId"] != "main"
                || row["origin"]["kind"] != "user"
                || row["turnId"].as_u64().is_none()
            {
                return None;
            }
            (row["time"].as_f64()? / 1000.0, text(&row["input"], "text")?)
        };
        (at.is_finite() && at > pending && at <= now() && input == submitted).then_some(at)
    })
}

fn receipt(record: &Value) -> Option<Value> {
    let id = string(record, "input_pending_request");
    if !zerus_client_core::input_acknowledgment::canonical(id) {
        return None;
    }
    let bytes = labels::read_file(
        &root().join("input_receipts").join(format!("{id}.json")),
        256 << 10,
    )
    .ok()??;
    let receipt: Value = serde_json::from_slice(&bytes).ok()?;
    let digest = string(&receipt, "digest");
    if receipt["request_id"] != id
        || receipt["name"] != record["name"]
        || receipt["run_id"] != record["run_id"]
        || receipt["conversation_id"] != record["conversation_id"]
        || receipt["submitted_at"] != record["input_pending_at"]
        || !matches!(receipt["status"].as_str(), Some("submitted" | "uncertain"))
        || receipt["transport"] != "terminal"
        || string(&receipt, "submitted_text").is_empty()
        || !receipt["attachments"].as_array().is_some_and(Vec::is_empty)
        || digest.len() != 64
        || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        || record
            .get("input_pending_digest")
            .is_some_and(|value| value != digest)
        || record
            .get("input_pending_text_sha256")
            .is_some_and(|value| {
                value
                    != &json!(format!(
                        "{:x}",
                        Sha256::digest(string(&receipt, "submitted_text").as_bytes())
                    ))
            })
    {
        return None;
    }
    Some(receipt)
}

/// The caller holds Root and the selected record. Only acceptance changes;
/// original delivery/unknown receipts, clearing and person audits stay intact.
pub(in crate::state) fn reconcile_locked(record: &mut Value) -> Result<()> {
    let agent = string(record, "agent");
    if !matches!(agent, "codex" | "kimi")
        || !input_unconfirmed(record)
        || terminating::pending(record)
        || string(record, "run_id").is_empty()
        || string(record, "conversation_id").is_empty()
        || !string(record, "expected_id").is_empty()
        || record["conversation_state"] == "unknown"
        || input_typing(record).is_some()
        || send_in_flight(record)
    {
        return Ok(());
    }
    let Some(receipt) = receipt(record) else {
        return Ok(());
    };
    let source = if agent == "codex" {
        source(
            Path::new(string(record, "transcript")),
            Some(string(record, "conversation_id")),
        )
    } else {
        kimi(record)
    };
    let Some(source) = source else {
        return Ok(());
    };
    let Some(at) = consumed(
        &source.rows,
        agent,
        record["input_pending_at"].as_f64().unwrap_or(f64::INFINITY),
        string(&receipt, "submitted_text"),
    ) else {
        return Ok(());
    };
    if !source.current() {
        return Ok(());
    }
    record["input_seen_at"] = json!(at);
    record["input_acceptance"] = json!({"source":"native_user_input", "request_id":receipt["request_id"],
        "run_id":record["run_id"],"conversation_id":record["conversation_id"],"pending_at":record["input_pending_at"],
        "digest":receipt["digest"],"text_sha256":format!("{:x}",Sha256::digest(string(&receipt,"submitted_text").as_bytes())),"at":at});
    write(record)
}

/// Positive native completion is only eligibility for the person's unknown
/// decision. An old completed turn never settles a newer pending input itself.
pub(in crate::state) fn native_idle(record: &Value) -> bool {
    let (source, agent) = match string(record, "agent") {
        "kimi" => (kimi(record), "kimi"),
        "codex" => (
            source(
                Path::new(string(record, "transcript")),
                Some(string(record, "conversation_id")),
            ),
            "codex",
        ),
        _ => return false,
    };
    let Some(source) = source else {
        return false;
    };
    let mut turn = None;
    let mut started_at = 0.0;
    let mut ended = false;
    for row in &source.rows {
        let at = if agent == "kimi" {
            row["time"].as_f64().map(|at| at / 1000.0)
        } else {
            row["timestamp"]
                .as_str()
                .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
                .map(|at| at.timestamp_micros() as f64 / 1_000_000.0)
        };
        let payload = if agent == "kimi" {
            if row["agentId"] != "main" {
                if matches!(string(row, "type"), "turn.prompt" | "turn.ended")
                    && string(row, "agentId").is_empty()
                {
                    return false;
                }
                continue;
            }
            row.clone()
        } else {
            if row["type"] != "event_msg" {
                continue;
            }
            row["payload"].clone()
        };
        match string(&payload, "type") {
            "turn.prompt" | "task_started" | "turn_started" => {
                let Some(at) = at.filter(|at| at.is_finite() && *at > 0.0 && *at <= now()) else {
                    return false;
                };
                started_at = at;
                turn = payload
                    .get(if agent == "kimi" { "turnId" } else { "turn_id" })
                    .cloned()
                    .filter(|id| !id.is_null());
                ended = false;
            }
            "turn.ended" | "task_complete" | "turn_complete" => {
                let id = payload.get(if agent == "kimi" { "turnId" } else { "turn_id" });
                ended = at.is_some_and(|at| at.is_finite() && at >= started_at && at <= now())
                    && turn.as_ref().is_some_and(|turn| Some(turn) == id)
                    && if agent == "kimi" {
                        payload["reason"] == "completed"
                    } else {
                        payload["error"].is_null()
                    };
            }
            _ => {}
        }
    }
    ended && source.current()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_user_input_only_never_timing_completion_substring_or_peer() {
        let pending = 10.0;
        let good = json!({"timestamp":"1970-01-01T00:00:11Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"exact\ninput"}]}});
        assert_eq!(
            consumed(
                std::slice::from_ref(&good),
                "codex",
                pending,
                "exact\ninput"
            ),
            Some(11.0)
        );
        assert!(consumed(std::slice::from_ref(&good), "codex", 11.0, "exact\ninput").is_none());
        for key in ["assistant", "tool", "system"] {
            let mut row = good.clone();
            row["payload"]["role"] = json!(key);
            assert!(consumed(&[row], "codex", pending, "exact\ninput").is_none());
        }
        for text in ["exact", " exact\ninput", "exact\ninput\n"] {
            assert!(consumed(std::slice::from_ref(&good), "codex", pending, text).is_none());
        }
        let kimi = json!({"type":"turn.prompt","agentId":"main","origin":{"kind":"user"},"turnId":1,"time":11000,"input":[{"type":"text","text":"exact"}]});
        assert_eq!(
            consumed(std::slice::from_ref(&kimi), "kimi", pending, "exact"),
            Some(11.0)
        );
        for (key, value) in [
            ("agentId", json!("child")),
            ("origin", json!({"kind":"task"})),
            (
                "input",
                json!([{"type":"text","text":"prefix"},{"type":"text","text":"exact"}]),
            ),
        ] {
            let mut row = kimi.clone();
            row[key] = value;
            assert!(consumed(&[row], "kimi", pending, "exact").is_none());
        }
    }
}
