//! Qt's public-content `hgs search` read. Only a bounded reply crosses the
//! computer boundary; the client never reads provider transcripts itself.
use crate::actions::{Exit, Invocation};
use crate::fleet::valid_alias;
use crate::reads::{Read, ReadError};
use crate::run::{run_cancellable, Stopped};
use crate::ComputerId;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

pub const MAX_HITS: usize = 100;
pub const MAX_REPLY: usize = 8 << 20;
pub const INVALID: &str = "Invalid search response; update hgs on this machine.";
const MARKER: &str = "hgs [@host] search --query";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Hit {
    pub name: String,
    #[serde(default, deserialize_with = "empty_string")]
    pub run_id: String,
    #[serde(default, deserialize_with = "empty_string")]
    pub conversation_id: String,
    #[serde(default, deserialize_with = "empty_string")]
    pub archive_id: String,
    pub message_id: String,
    pub at: f64,
    pub role: String,
    pub snippet: String,
    pub event: Value,
    #[serde(default)]
    pub content_truncated: bool,
    #[serde(default)]
    pub session: Option<Value>,
    /// Shared project placement of an archive, resolved by the app's pump.
    #[serde(default)]
    pub group: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Reply {
    pub query: String,
    pub results: Vec<Hit>,
    #[serde(default)]
    pub truncated: bool,
    #[serde(default)]
    pub unavailable_sessions: u64,
}
fn empty_string<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}
fn invalid() -> ReadError {
    ReadError::Invalid {
        message: INVALID.into(),
    }
}
fn printable(text: &str, max: usize) -> bool {
    text.chars().count() <= max && !text.chars().any(char::is_control)
}
pub fn invocation(computer: &ComputerId, query: &str) -> Result<Invocation, ReadError> {
    if query.trim() != query
        || query.chars().count() < 2
        || !printable(query, 256)
        || computer.alias().is_some_and(|alias| !valid_alias(alias))
    {
        return Err(ReadError::BadRequest {
            message: "Search requires 2..256 printable characters and a valid computer.".into(),
        });
    }
    let mut args: Vec<String> = computer
        .alias()
        .map(|alias| format!("@{alias}"))
        .into_iter()
        .collect();
    args.extend(["search", "--query", query, "--limit", "100"].map(str::to_owned));
    Ok(Invocation {
        args,
        stdin: None,
        timeout: Duration::from_secs(if computer.is_local() { 12 } else { 20 }),
        request_id: None,
    })
}
pub fn decode(query: &str, value: Value) -> Result<Reply, ReadError> {
    let mut reply: Reply = serde_json::from_value(value).map_err(|_| invalid())?;
    if reply.query != query || reply.results.len() > MAX_HITS {
        return Err(invalid());
    }
    for hit in &mut reply.results {
        if hit.name.is_empty()
            || !printable(&hit.name, 1024)
            || !printable(&hit.run_id, 1024)
            || !printable(&hit.conversation_id, 1024)
            || !printable(&hit.archive_id, 256)
            || hit.message_id.is_empty()
            || !printable(&hit.message_id, 1024)
            || !hit.at.is_finite()
            || !["user", "assistant", "tool"].contains(&hit.role.as_str())
            || hit.snippet.chars().count() > 1024
            || !hit.event.is_object()
            || hit.event["detail"]
                .as_str()
                .is_none_or(|text| text.chars().count() > 32_002)
        {
            return Err(invalid());
        }
        if hit.archive_id.is_empty() {
            if hit
                .session
                .as_ref()
                .is_some_and(|row| row["state"] == "archived")
            {
                return Err(invalid());
            }
            hit.session = None;
        } else {
            let Some(session) = hit.session.as_ref() else {
                return Err(invalid());
            };
            if session["name"] != hit.name
                || session["run_id"].as_str().unwrap_or("") != hit.run_id
                || session["conversation_id"].as_str().unwrap_or("") != hit.conversation_id
                || session["archive_id"] != hit.archive_id
                || session["state"] != "archived"
            {
                return Err(invalid());
            }
            let field = |key: &str| {
                session[key]
                    .as_str()
                    .unwrap_or("")
                    .chars()
                    .take(4096)
                    .collect::<String>()
            };
            let project = field("project");
            let tag = field("tag");
            let agent = session["cmd"]
                .as_str()
                .or_else(|| session["agent"].as_str())
                .unwrap_or("");
            let title = if tag.is_empty() {
                project
            } else {
                format!("{project} / {tag}")
            };
            hit.session = Some(serde_json::json!({ "name": hit.name, "run_id": hit.run_id,
                "conversation_id": hit.conversation_id, "archive_id": hit.archive_id,
                "title": if title.is_empty() { hit.name.clone() } else { title },
                "agent": agent.chars().take(128).collect::<String>(), "state":"archived", "status":"ended", "caption":"Archived",
                "tracked": true, "resumable": session["resumable"] == true, "cwd": field("cwd"),
                "archived_at": session["archived_at"].as_f64().filter(|at| at.is_finite()).unwrap_or(0.0) }));
        }
        // A preview is public message text, never a source of attachment,
        // permission or terminal actions from a peer-provided event.
        let event = &hit.event;
        hit.event = serde_json::json!({ "type": if hit.role == "user" { "UserPromptSubmit" } else { "AgentMessage" },
            "detail": event["detail"], "at": hit.at, "message_id": hit.message_id,
            "content_truncated": hit.content_truncated });
    }
    Ok(reply)
}
pub fn read(
    hgs: &Path,
    computer: &ComputerId,
    query: &str,
    cancel: &AtomicBool,
) -> Result<Reply, ReadError> {
    let request = invocation(computer, query)?;
    let run = |inv: &Invocation, cap| {
        run_cancellable(hgs, inv, &[], cap, cancel).map_err(|error| match error {
            Stopped::Cancelled => ReadError::Superseded,
            Stopped::TooLarge(error) => ReadError::TooLarge { limit: error.cap },
        })
    };
    let mut help = request.clone();
    help.args.truncate(usize::from(!computer.is_local()));
    help.args.push("--help".into());
    // Older CLIs must never interpret an unknown read verb as a launch.
    match run(&help, 128 << 10)? {
        Exit::Code {
            code: 0, stdout, ..
        } if String::from_utf8_lossy(&stdout).contains(MARKER) => {}
        Exit::Code { code: 0, .. } => return Err(ReadError::Unsupported),
        exit => {
            crate::reads::decode(&Read::Recovery, exit)?;
            return Err(ReadError::Unsupported);
        }
    }
    let value = crate::reads::decode(&Read::Recovery, run(&request, MAX_REPLY)?)?;
    decode(query, value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn hit() -> Value {
        json!({"name":"claude/demo/sample","run_id":"run-one","conversation_id":"conversation-one","archive_id":"","message_id":"m1","at":10,"role":"assistant","snippet":"needle","event":{"type":"AgentMessage","detail":"needle","attachments":[{"unsafe":"ignored"}]}})
    }
    #[test]
    fn exact_qt_argv_and_timeouts_validate_before_a_child_can_start() {
        assert_eq!(
            invocation(&ComputerId::local(), "--literal query")
                .unwrap()
                .args,
            ["search", "--query", "--literal query", "--limit", "100"]
        );
        let peer = invocation(&ComputerId::from("studio"), "needle").unwrap();
        assert_eq!(peer.args[0], "@studio");
        assert_eq!(peer.timeout, Duration::from_secs(20));
        for query in ["a", " padded", "bad\ntext", &"x".repeat(257)] {
            assert!(invocation(&ComputerId::local(), query).is_err());
        }
        assert!(invocation(&ComputerId::from("-bad"), "needle").is_err());
    }
    #[test]
    fn a_reply_keeps_public_excerpts_and_requires_exact_query_and_archive_identity() {
        let good =
            json!({"query":"needle","results":[hit()],"truncated":true,"unavailable_sessions":2});
        let reply = decode("needle", good.clone()).unwrap();
        assert!(reply.truncated);
        assert_eq!(reply.unavailable_sessions, 2);
        assert!(reply.results[0].event.get("attachments").is_none());
        assert!(decode("other", good.clone()).is_err());
        let mut wrong = good;
        wrong["results"][0]["archive_id"] = json!("archive-one");
        assert!(decode("needle", wrong).is_err());
    }
    #[test]
    fn oversized_pages_and_unbounded_event_text_are_rejected() {
        assert!(decode(
            "needle",
            json!({"query":"needle","results":vec![hit();101]})
        )
        .is_err());
        let mut item = hit();
        item["event"]["detail"] = json!("x".repeat(32_003));
        assert!(decode("needle", json!({"query":"needle","results":[item]})).is_err());
    }
    #[test]
    fn legacy_null_run_and_conversation_ids_follow_qt_without_losing_archive_identity() {
        let mut legacy = hit();
        legacy["run_id"] = Value::Null;
        legacy["conversation_id"] = Value::Null;
        let reply = decode(
            "needle",
            json!({"query":"needle","results":[legacy.clone()]}),
        )
        .unwrap();
        assert!(reply.results[0].run_id.is_empty());
        assert!(reply.results[0].conversation_id.is_empty());
        legacy["session"] = json!({"state":"archived","archive_id":"archive-one"});
        assert!(decode("needle", json!({"query":"needle","results":[legacy]})).is_err());
    }
}
