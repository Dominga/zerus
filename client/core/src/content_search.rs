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
const INDEX_MARKER: &str = "content-search-query-v1";

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
    #[serde(default)]
    pub contents_unavailable: bool,
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
    let Some(abi) = value.get("abi") else {
        return decode_channel(query, value);
    };
    if abi != INDEX_MARKER
        || !matches!(
            value["index_coverage"]["state"].as_str(),
            Some("ready" | "cold" | "indexing" | "partial" | "unavailable")
        )
    {
        return Err(invalid());
    }
    let has_cursor = match &value["next_cursor"] {
        Value::Null => false,
        Value::String(cursor) if !cursor.is_empty() && printable(cursor, 1024) => true,
        _ => return Err(invalid()),
    };
    let value_coverage_incomplete = value["index_coverage"]["state"] != "ready";
    let incomplete = value_coverage_incomplete || has_cursor;
    let fallback = value.get("fallback").cloned().ok_or_else(invalid)?;
    let mut reply = decode_channel(query, value)?;
    reply.truncated |= incomplete;
    reply.contents_unavailable |= value_coverage_incomplete;
    if !fallback.is_null() {
        let fallback = decode_channel(query, fallback)?;
        reply.truncated |= fallback.truncated;
        reply.unavailable_sessions = reply
            .unavailable_sessions
            .saturating_add(fallback.unavailable_sessions);
        reply.contents_unavailable |= fallback.contents_unavailable;
        let primary = std::mem::replace(&mut reply.results, fallback.results);
        for hit in primary {
            if !reply
                .results
                .iter()
                .any(|existing| same_hit(existing, &hit))
            {
                reply.results.push(hit);
            }
        }
        reply.results.sort_by(|a, b| {
            b.at.total_cmp(&a.at)
                .then_with(|| a.name.cmp(&b.name))
                .then_with(|| a.message_id.cmp(&b.message_id))
        });
        if reply.results.len() > MAX_HITS {
            reply.results.truncate(MAX_HITS);
            reply.truncated = true;
        }
    }
    Ok(reply)
}
fn same_hit(a: &Hit, b: &Hit) -> bool {
    a.run_id == b.run_id
        && a.conversation_id == b.conversation_id
        && a.archive_id == b.archive_id
        && a.message_id == b.message_id
}

fn decode_channel(query: &str, value: Value) -> Result<Reply, ReadError> {
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
            || !["user", "assistant", "tool", "event"].contains(&hit.role.as_str())
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
            // A scope is an identity: oversized values become unknown, never
            // another profile or folder obtained by truncating a common prefix.
            let scope = |key: &str, max| {
                session[key]
                    .as_str()
                    .filter(|v| v.chars().count() <= max)
                    .unwrap_or("")
            };
            let bad_binding = [("account_id", 4096), ("account_home", 16 << 10)]
                .iter()
                .any(|(key, max)| {
                    session.get(key).is_some_and(|v| {
                        !v.is_null() && v.as_str().is_none_or(|s| s.chars().count() > *max)
                    })
                });
            hit.session = Some(serde_json::json!({ "name": hit.name, "run_id": hit.run_id,
                "conversation_id": hit.conversation_id, "archive_id": hit.archive_id,
                "title": if title.is_empty() { hit.name.clone() } else { title },
                "agent": agent.chars().take(128).collect::<String>(), "state":"archived", "status":"ended", "caption":"Archived",
                "tracked": true, "resumable": session["resumable"] == true, "cwd": field("cwd"),
                "cwd_canonical":scope("cwd_canonical", 16 << 10), "git_root":scope("git_root", 16 << 10),
                "account_provider":agent.chars().take(128).collect::<String>(),
                "account_id":if bad_binding { "" } else { scope("account_id",4096) }, "account_home":if bad_binding { "" } else { scope("account_home",16 << 10) },
                "account_binding_reported":!bad_binding && (session.get("account_id").is_some_and(Value::is_string) || session.get("account_home").is_some_and(Value::is_string)),
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
    let mut request = invocation(computer, query)?;
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
    let indexed = match run(&help, 128 << 10)? {
        Exit::Code {
            code: 0, stdout, ..
        } if String::from_utf8_lossy(&stdout).contains(MARKER) => {
            String::from_utf8_lossy(&stdout).contains(INDEX_MARKER)
        }
        Exit::Code { code: 0, .. } => return Err(ReadError::Unsupported),
        exit => {
            crate::reads::decode(&Read::Recovery, exit)?;
            return Err(ReadError::Unsupported);
        }
    };
    if indexed {
        request
            .args
            .insert(usize::from(!computer.is_local()) + 1, "--indexed".into());
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
    fn indexed(results: Vec<Value>, fallback: Value) -> Value {
        json!({"abi":INDEX_MARKER,"query":"needle","results":results,
            "next_cursor":null,"index_coverage":{"state":"indexing"},"fallback":fallback})
    }
    #[test]
    fn indexed_channels_are_validated_independently_and_keep_one_bounded_sorted_page() {
        let mut primary = Vec::new();
        let mut legacy = Vec::new();
        for i in 0..100 {
            let mut item = hit();
            item["message_id"] = json!(format!("m{i}"));
            item["at"] = json!(i);
            item["event"]["detail"] = json!(format!("needle {i}"));
            primary.push(item.clone());
            item["message_id"] = json!(format!("m{}", i + 100));
            item["at"] = json!(i + 100);
            item["event"]["detail"] = json!(format!("needle {}", i + 100));
            legacy.push(item);
        }
        let reply = decode(
            "needle",
            indexed(primary, json!({"query":"needle","results":legacy})),
        )
        .unwrap();
        assert_eq!(reply.results.len(), 100);
        assert_eq!(reply.results[0].at, 199.0);
        assert_eq!(reply.results[99].at, 100.0);
        assert!(reply.truncated);
        assert!(reply.contents_unavailable);
        let old = hit();
        let mut fresh = hit();
        fresh["event"]["detail"] = json!("needle fresh public message");
        let preferred = decode(
            "needle",
            indexed(vec![old], json!({"query":"needle","results":[fresh]})),
        )
        .unwrap();
        assert_eq!(preferred.results.len(), 1);
        assert_eq!(
            preferred.results[0].event["detail"],
            "needle fresh public message"
        );
        assert!(decode("needle", indexed(vec![hit(); 101], Value::Null)).is_err());
        assert!(decode(
            "needle",
            indexed(vec![], json!({"query":"needle","results":vec![hit();101]}))
        )
        .is_err());
    }
    #[test]
    fn indexed_abi_coverage_cursor_and_fallback_query_are_checked() {
        let good = indexed(vec![hit()], Value::Null);
        for (key, bad) in [
            ("abi", json!("unknown")),
            ("index_coverage", json!({"state":"invented"})),
            ("next_cursor", json!("x".repeat(1025))),
            ("fallback", json!({"query":"different","results":[]})),
        ] {
            let mut value = good.clone();
            value[key] = bad;
            assert!(decode("needle", value).is_err(), "{key}");
        }
        let mut event = hit();
        event["role"] = json!("event");
        assert_eq!(
            decode("needle", indexed(vec![event], Value::Null))
                .unwrap()
                .results[0]
                .role,
            "event"
        );
    }
    #[test]
    fn a_ready_indexed_archive_keeps_the_exact_read_only_navigation_row() {
        let mut item = hit();
        item["archive_id"] = json!("archive-one");
        item["session"] = json!({"name":item["name"],"run_id":item["run_id"],
            "conversation_id":item["conversation_id"],"archive_id":"archive-one",
            "state":"archived","project":"example","tag":"search","cmd":"codex","archived_at":42});
        let mut value = indexed(vec![item], Value::Null);
        value["index_coverage"]["state"] = json!("ready");
        let reply = decode("needle", value).unwrap();
        assert!(!reply.truncated);
        let row = reply.results[0].session.as_ref().unwrap();
        assert_eq!(row["archive_id"], "archive-one");
        assert_eq!(row["run_id"], "run-one");
        assert_eq!(row["title"], "example / search");
        assert_eq!(row["account_binding_reported"], false);
    }

    #[test]
    fn legacy_and_indexed_archive_hits_keep_bounded_public_scope_metadata_only() {
        let mut item = hit();
        item["archive_id"] = json!("archive-one");
        item["session"] = json!({"name":item["name"],"run_id":item["run_id"],
            "conversation_id":item["conversation_id"],"archive_id":"archive-one","state":"archived","cmd":"dsh",
            "account_id":"recorded","account_home":"/srv/example/profile","cwd_canonical":"/srv/example/app/sub","git_root":"/srv/example/app",
            "credential_body":"must never be forwarded"});
        for value in [
            json!({"query":"needle","results":[item.clone()],"truncated":false,"unavailable_sessions":0}),
            indexed(vec![item], Value::Null),
        ] {
            let reply = decode("needle", value).unwrap();
            let row = reply.results[0].session.as_ref().unwrap();
            assert_eq!(row["account_provider"], "dsh");
            assert_eq!(row["account_id"], "recorded");
            assert_eq!(row["account_home"], "/srv/example/profile");
            assert_eq!(row["account_binding_reported"], true);
            assert_eq!(row["cwd_canonical"], "/srv/example/app/sub");
            assert_eq!(row["git_root"], "/srv/example/app");
            assert!(row.get("credential_body").is_none());
        }
    }

    #[test]
    fn oversized_scope_identities_are_unknown_instead_of_matching_a_shorter_prefix() {
        let mut item = hit();
        item["archive_id"] = json!("archive-one");
        item["session"] = json!({"name":item["name"],"run_id":item["run_id"],"conversation_id":item["conversation_id"],
            "archive_id":"archive-one","state":"archived","cmd":"claude","account_id":"x".repeat(4097),
            "account_home":"/srv/example/other-account","cwd_canonical":"p".repeat((16 << 10) + 1)});
        let reply = decode("needle", indexed(vec![item], Value::Null)).unwrap();
        let row = reply.results[0].session.as_ref().unwrap();
        assert_eq!(row["account_binding_reported"], false);
        assert_eq!(row["account_id"], "");
        assert_eq!(row["account_home"], "");
        assert_eq!(row["cwd_canonical"], "");
    }
}
