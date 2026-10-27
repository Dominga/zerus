//! Native assistant messages and Claude's recorded thinking from the exact
//! conversation. Hook Stop events alone miss progress between tools.
use super::*;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};

// A transcript line longer than this is never a public message.
const LINE_BYTES: usize = 1024 * 1024;

/// The Codex or Claude transcript of the record's exact conversation; None for
/// other agents or a path that does not name the conversation.
fn transcript(record: &Value) -> Option<(&str, &str, PathBuf)> {
    let agent = string(record, "agent");
    let id = string(record, "conversation_id");
    if !["codex", "claude"].contains(&agent) || uuid::Uuid::parse_str(id).is_err() {
        return None;
    }
    let path = PathBuf::from(string(record, "transcript"));
    (path.is_absolute() && path.file_name().is_some_and(|v| v.to_string_lossy().contains(id))).then_some((agent, id, path))
}

/// The first line of a transcript, at most 256 KiB; a Codex transcript must
/// name the conversation in its `session_meta`.
fn first_line(file: &mut File, agent: &str, id: &str) -> Result<Vec<u8>> {
    let mut first = Vec::new();
    BufReader::new(file.take(256 * 1024)).read_until(b'\n', &mut first).map_err(|e| e.to_string())?;
    if agent == "codex" {
        let meta: Value = serde_json::from_slice(&first).map_err(|e| e.to_string())?;
        if meta["type"] != "session_meta" || meta["payload"]["id"] != id {
            return Err("Conversation transcript identity mismatch".into());
        }
    }
    Ok(first)
}

/// The public messages one transcript line contributes: Claude's thinking and
/// assistant replies of the exact conversation, never private reasoning.
fn line_messages(agent: &str, id: &str, line: &[u8]) -> Vec<Value> {
    let Ok(event) = serde_json::from_slice::<Value>(line) else {
        return vec![];
    };
    if agent == "claude" && !string(&event, "sessionId").is_empty() && event["sessionId"] != id {
        return vec![];
    }
    let mut messages = Vec::new();
    if agent == "claude" {
        messages.extend(claude_thinking(&event));
    }
    if let Some((message, "assistant")) = search::provider_event(agent, &event) {
        messages.push(message);
    }
    messages
}

/// Codex can emit both response_item and event_msg for the same public
/// message. Genuinely repeated later messages stay in the conversation, and
/// Claude's thinking is never a repeat.
fn repeated(recent: &[Value], message: &Value) -> bool {
    message["type"] != "AgentThinking" && recent.iter().rev().take(4).any(|m| {
        m["type"] == message["type"]
            && m["detail"] == message["detail"]
            && (m["at"].as_f64().unwrap_or(0.0) - message["at"].as_f64().unwrap_or(0.0)).abs() < 3.0
    })
}

pub(super) fn read(record: &Value) -> Result<Vec<Value>> {
    if string(record, "agent") == "kimi" {
        return kimi_messages::read(record);
    }
    let Some((agent, id, path)) = transcript(record) else {
        return Ok(vec![]);
    };
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    first_line(&mut file, agent, id)?;
    let offset = file
        .metadata()
        .map_err(|e| e.to_string())?
        .len()
        .saturating_sub(8 * 1024 * 1024);
    file.seek(SeekFrom::Start(offset))
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(8 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    let mut messages: Vec<Value> = Vec::new();
    for (index, line) in bytes.split_inclusive(|b| *b == b'\n').enumerate() {
        if (offset > 0 && index == 0) || line.len() > LINE_BYTES || !line.ends_with(b"\n") {
            continue;
        }
        for message in line_messages(agent, id, line) {
            if !repeated(&messages, &message) {
                messages.push(message);
            }
        }
    }
    messages.sort_by(|a,b|a["at"].as_f64().unwrap_or(0.).total_cmp(&b["at"].as_f64().unwrap_or(0.)));
    if messages.len() > 100 {
        messages.drain(..messages.len() - 100);
    }
    Ok(messages)
}

/// What a [`Tail`] read found.
#[derive(Debug, PartialEq)]
pub(crate) enum TailRead {
    /// Public messages written since the previous read, oldest first.
    Messages(Vec<Value>),
    /// The transcript was replaced, rewritten, shortened or fell too far
    /// behind; a reader re-reads the messages whole.
    Ended,
}

/// Follows one Codex or Claude transcript from where it ended when opened and
/// yields the public messages written since, as `read` projects them. The
/// file is identified by device, inode and a hash of its first line; reads
/// continue from an offset and finish a line split across reads.
pub(crate) struct Tail {
    agent: String,
    conversation: String,
    path: PathBuf,
    identity: (u64, u64),
    head: [u8; 32],
    offset: u64,
    partial: Vec<u8>,
    // The bytes up to the next newline belong to a line read only in part.
    skipping: bool,
    recent: Vec<Value>,
}

impl Tail {
    /// A transcript yields what each read finds after this many bytes at most.
    pub(crate) const READ_BYTES: usize = 2 * 1024 * 1024;
    /// Further behind than this, a tail ends rather than catch up.
    const BEHIND_BYTES: u64 = 16 * 1024 * 1024;

    /// A tail at the transcript's current end; None for other agents, a path
    /// that does not name the conversation, or a transcript that is not there.
    pub(crate) fn open(record: &Value) -> Option<Tail> {
        let (agent, id, path) = transcript(record)?;
        let identity = crate::platform::file_identity(&path)?;
        let mut file = File::open(&path).ok()?;
        let head: [u8; 32] = Sha256::digest(first_line(&mut file, agent, id).ok()?).into();
        let offset = file.metadata().ok()?.len();
        // Opened during a write, the rest of that line is skipped.
        let mut last = [0u8; 1];
        let skipping = offset > 0
            && file.seek(SeekFrom::Start(offset - 1)).is_ok()
            && file.read_exact(&mut last).is_ok()
            && last[0] != b'\n';
        Some(Tail {
            agent: agent.to_owned(),
            conversation: id.to_owned(),
            path,
            identity: (identity.device, identity.inode),
            head,
            offset,
            partial: Vec::new(),
            skipping,
            recent: Vec::new(),
        })
    }

    pub(crate) fn conversation(&self) -> &str {
        &self.conversation
    }

    /// Messages written since the previous read, from at most `READ_BYTES`.
    pub(crate) fn read(&mut self) -> TailRead {
        let Some(identity) = crate::platform::file_identity(&self.path) else {
            return TailRead::Ended;
        };
        let Ok(mut file) = File::open(&self.path) else {
            return TailRead::Ended;
        };
        let Ok(len) = file.metadata().map(|meta| meta.len()) else {
            return TailRead::Ended;
        };
        if (identity.device, identity.inode) != self.identity || len < self.offset || len - self.offset > Self::BEHIND_BYTES {
            return TailRead::Ended;
        }
        if len == self.offset {
            return TailRead::Messages(vec![]);
        }
        let same = first_line(&mut file, &self.agent, &self.conversation)
            .is_ok_and(|first| <[u8; 32]>::from(Sha256::digest(first)) == self.head);
        if !same || file.seek(SeekFrom::Start(self.offset)).is_err() {
            return TailRead::Ended;
        }
        let mut bytes = Vec::new();
        let wanted = (len - self.offset).min(Self::READ_BYTES as u64);
        if (&mut file).take(wanted).read_to_end(&mut bytes).is_err() {
            return TailRead::Ended;
        }
        self.offset += bytes.len() as u64;
        let mut messages = Vec::new();
        let mut rest = bytes.as_slice();
        while let Some(end) = rest.iter().position(|b| *b == b'\n') {
            let (piece, after) = rest.split_at(end + 1);
            rest = after;
            if std::mem::take(&mut self.skipping) {
                self.partial.clear();
                continue;
            }
            let line = if self.partial.is_empty() { piece } else {
                self.partial.extend_from_slice(piece);
                self.partial.as_slice()
            };
            if line.len() <= LINE_BYTES {
                for message in line_messages(&self.agent, &self.conversation, line) {
                    if !repeated(&self.recent, &message) {
                        self.recent.push(message.clone());
                        messages.push(message);
                    }
                }
            }
            self.partial.clear();
        }
        if self.recent.len() > 4 {
            self.recent.drain(..self.recent.len() - 4);
        }
        if !self.skipping {
            self.partial.extend_from_slice(rest);
            // A line this long is not a public message: skip the rest of it.
            if self.partial.len() > LINE_BYTES {
                self.partial.clear();
                self.skipping = true;
            }
        }
        TailRead::Messages(messages)
    }
}

// Claude records some terminal-visible progress in thinking blocks. Keep it
// separate from replies; signatures and redacted blocks are never UI content.
fn claude_thinking(event: &Value) -> Option<Value> {
    if event["type"] != "assistant"
        || event["message"]["role"] != "assistant"
        || event["isMeta"] == true
        || event["isSidechain"] == true
    {
        return None;
    }
    let text = event["message"]["content"]
        .as_array()?
        .iter()
        .filter(|part| part["type"] == "thinking")
        .filter_map(|part| part["thinking"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if text.trim().is_empty() {
        return None;
    }
    Some(json!({"type":"AgentThinking","source":"claude_transcript",
        "at":search::timestamp(&event["timestamp"]),
        "message_id":format!("{}:thinking",string(event,"uuid")),"agent_id":"",
        "detail":journal::clipped(&json!(text),32_000)}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[test]
    fn public_progress_and_claude_thinking_keep_separate_roles_without_private_payloads() {
        let temp = tempfile::tempdir().unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let path = temp.path().join(format!("rollout-{id}.jsonl"));
        let at = "2026-10-04T12:00:00Z";
        let rows = [
            json!({"type":"session_meta","payload":{"id":id}}),
            json!({"type":"response_item","timestamp":at,"payload":{"type":"message","role":"assistant","channel":"commentary","content":[{"type":"output_text","text":"Checking the deployment."}]}}),
            json!({"type":"event_msg","timestamp":at,"payload":{"type":"agent_message","message":"Checking the deployment."}}),
            json!({"type":"response_item","timestamp":at,"payload":{"type":"message","role":"assistant","channel":"analysis","content":[{"type":"output_text","text":"PRIVATE_REASONING"}]}}),
            json!({"type":"event_msg","timestamp":at,"payload":{"type":"agent_message","phase":"analysis","message":"PRIVATE_REASONING"}}),
        ];
        fs::write(
            &path,
            rows.iter()
                .map(|v| v.to_string() + "\n")
                .collect::<String>(),
        )
        .unwrap();
        let record = json!({"agent":"codex","conversation_id":id,"transcript":path});
        let messages = read(&record).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["detail"], "Checking the deployment.");
        assert_eq!(messages[0]["message_phase"], "commentary");
        let native = [
            json!({"type":"session_meta","payload":{"id":id}}),
            json!({"type":"event_msg","timestamp":at,"payload":{"type":"item_completed","item":{"type":"AgentMessage","id":"progress","phase":"commentary","content":[{"type":"text","text":"Progress from current Codex."}]}}}),
            json!({"type":"response_item","timestamp":at,"payload":{"type":"message","id":"progress","role":"assistant","phase":"commentary","content":[{"type":"output_text","text":"Progress from current Codex."}]}}),
            json!({"type":"response_item","timestamp":at,"payload":{"type":"message","role":"assistant","phase":"analysis","content":[{"type":"output_text","text":"PRIVATE_REASONING"}]}}),
            json!({"type":"response_item","timestamp":"2026-10-04T12:00:10Z","payload":{"type":"message","role":"assistant","phase":"commentary","content":[{"type":"output_text","text":"Progress from current Codex."}]}}),
        ];
        fs::write(&path,native.iter().map(|v|v.to_string()+"\n").collect::<String>()).unwrap();
        let messages=read(&record).unwrap();
        assert_eq!(messages.len(),2);
        assert_eq!(messages[0]["message_id"],"progress");
        assert_eq!(messages[0]["message_phase"],"commentary");
        assert_eq!(messages[1]["message_phase"],"commentary");
        assert_eq!(messages[0]["detail"],messages[1]["detail"]);
        fs::write(
            &path,
            json!({"type":"session_meta","payload":{"id":"wrong"}}).to_string() + "\n",
        )
        .unwrap();
        assert!(read(&record).is_err());
        let claude = json!({"type":"assistant","uuid":"msg-one","timestamp":at,"sessionId":id,"message":{"role":"assistant","content":[{"type":"text","text":"Still working."},{"type":"thinking","thinking":"Read all 24 pages.","signature":"PRIVATE_SIGNATURE"},{"type":"redacted_thinking","data":"REDACTED"}]}});
        let mut other = claude.clone();
        other["sessionId"] = json!("another-conversation");
        fs::write(&path, claude.to_string() + "\n" + &other.to_string() + "\n").unwrap();
        let messages =
            read(&json!({"agent":"claude","conversation_id":id,"transcript":path})).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["type"], "AgentThinking");
        assert_eq!(messages[0]["detail"], "Read all 24 pages.");
        assert_eq!(messages[1]["type"], "AgentMessage");
        assert_eq!(messages[1]["detail"], "Still working.");
        assert_ne!(messages[0]["message_id"], messages[1]["message_id"]);
        assert!(!serde_json::to_string(&messages)
            .unwrap()
            .contains("PRIVATE_SIGNATURE"));
        assert!(!serde_json::to_string(&messages)
            .unwrap()
            .contains("REDACTED"));
        for flag in ["isMeta", "isSidechain"] {
            let mut hidden = claude.clone();
            hidden[flag] = json!(true);
            assert!(claude_thinking(&hidden).is_none());
        }
    }

    fn append(path: &Path, text: &str) {
        use std::io::Write;
        fs::OpenOptions::new().append(true).open(path).unwrap().write_all(text.as_bytes()).unwrap();
    }

    fn codex(text: &str, phase: &str) -> String {
        json!({"type":"response_item","timestamp":"2026-10-04T12:00:00Z","payload":{"type":"message","role":"assistant","phase":phase,
            "content":[{"type":"output_text","text":text}]}}).to_string() + "\n"
    }

    fn details(read: TailRead) -> Vec<String> {
        match read {
            TailRead::Messages(messages) => messages.iter().map(|m| m["detail"].as_str().unwrap().to_owned()).collect(),
            TailRead::Ended => panic!("the tail ended"),
        }
    }

    #[test]
    fn a_tail_yields_public_messages_written_after_it_opened_across_split_lines() {
        let temp = tempfile::tempdir().unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let path = temp.path().join(format!("rollout-{id}.jsonl"));
        fs::write(&path, json!({"type":"session_meta","payload":{"id":id}}).to_string() + "\n" + &codex("Before the tail.", "commentary")).unwrap();
        let record = json!({"agent":"codex","conversation_id":id,"transcript":path});
        let mut tail = Tail::open(&record).unwrap();
        assert_eq!(tail.conversation(), id);
        assert!(details(tail.read()).is_empty());
        // The same message as a response item and an event: one message.
        let event = json!({"type":"event_msg","timestamp":"2026-10-04T12:00:01Z","payload":{"type":"agent_message","message":"Checking the deployment."}});
        append(&path, &(codex("Checking the deployment.", "commentary") + &event.to_string() + "\n" + &codex("PRIVATE_REASONING", "analysis")));
        let messages = match tail.read() { TailRead::Messages(messages) => messages, TailRead::Ended => panic!() };
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["detail"], "Checking the deployment.");
        assert_eq!(messages[0]["message_phase"], "commentary");
        assert_eq!(messages, read(&record).unwrap()[1..].to_vec());
        // A line written in two parts is read once it ends.
        let line = codex("Split across writes.", "commentary");
        let (first, second) = line.split_at(20);
        append(&path, first);
        assert!(details(tail.read()).is_empty());
        append(&path, second);
        assert_eq!(details(tail.read()), ["Split across writes."]);
        // An oversized line is skipped whole, even read in parts.
        append(&path, &(codex(&"x".repeat(LINE_BYTES + 10), "commentary") + &codex("After the large line.", "commentary")));
        let mut found = Vec::new();
        for _ in 0..4 {
            found.extend(details(tail.read()));
        }
        assert_eq!(found, ["After the large line."]);
        assert!(!serde_json::to_string(&read(&record).unwrap()).unwrap().contains("PRIVATE_REASONING"));
    }

    #[test]
    fn a_tail_opened_during_a_write_skips_that_line_and_ends_when_the_file_changes() {
        let temp = tempfile::tempdir().unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let path = temp.path().join(format!("{id}.jsonl"));
        let message = |uuid: &str, text: &str, session: &str| json!({"type":"assistant","uuid":uuid,"timestamp":"2026-10-04T12:00:00Z","sessionId":session,
            "message":{"role":"assistant","content":[{"type":"thinking","thinking":format!("{text} (thinking)")},{"type":"text","text":text}]}}).to_string() + "\n";
        let partial = message("half", "Written in part.", &id);
        fs::write(&path, message("first", "First.", &id) + &partial[..30]).unwrap();
        let record = json!({"agent":"claude","conversation_id":id,"transcript":path});
        let mut tail = Tail::open(&record).unwrap();
        append(&path, &(partial[30..].to_owned() + &message("other", "Another conversation.", "elsewhere") + &message("next", "Next.", &id)));
        let messages = match tail.read() { TailRead::Messages(messages) => messages, TailRead::Ended => panic!() };
        assert_eq!(messages.iter().map(|m| (m["type"].as_str().unwrap(), m["detail"].as_str().unwrap())).collect::<Vec<_>>(),
                   [("AgentThinking", "Next. (thinking)"), ("AgentMessage", "Next.")]);
        assert_eq!(messages[1]["message_id"], "next");
        // A first line rewritten in place is another transcript.
        let text = fs::read_to_string(&path).unwrap();
        fs::write(&path, text.replacen("First.", "Fixed.", 1) + &message("later", "Later.", &id)).unwrap();
        assert_eq!(tail.read(), TailRead::Ended);
        // So is a shortened or replaced file.
        let mut tail = Tail::open(&record).unwrap();
        fs::write(&path, message("first", "First.", &id)).unwrap();
        assert_eq!(tail.read(), TailRead::Ended);
        let mut tail = Tail::open(&record).unwrap();
        let replacement = temp.path().join("replacement");
        fs::write(&replacement, fs::read(&path).unwrap()).unwrap();
        append(&replacement, &message("swapped", "Swapped.", &id));
        fs::rename(&replacement, &path).unwrap();
        assert_eq!(tail.read(), TailRead::Ended);
        // Only Codex and Claude transcripts of the exact conversation are followed.
        assert!(Tail::open(&json!({"agent":"kimi","conversation_id":id,"transcript":path})).is_none());
        assert!(Tail::open(&json!({"agent":"claude","conversation_id":uuid::Uuid::new_v4().to_string(),"transcript":path})).is_none());
        assert!(Tail::open(&json!({"agent":"claude","conversation_id":id,"transcript":temp.path().join(format!("{id}-missing.jsonl"))})).is_none());
    }
}
