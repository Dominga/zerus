//! External chat workstreams are cooperative inboxes, not a cloud-chat API.
use crate::{
    cli::{Error, Result},
    config::Config,
    platform,
};
use anyhow::{bail, ensure, Context};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read},
    path::Path,
    time::Duration,
};
const INPUT_BYTES: usize = 128 * 1024;
const TEXT_BYTES: usize = 16 * 1024;
const LIST_LIMIT: i64 = 256;
const INBOX_LIMIT: i64 = 128;
const MODES: &[&str] = &["chatgpt_chat", "chatgpt_work", "codex_chat"];
const SCHEMA: &str = "PRAGMA journal_mode=DELETE; PRAGMA max_page_count=131072;
 CREATE TABLE IF NOT EXISTS workstreams(id TEXT PRIMARY KEY,name TEXT NOT NULL UNIQUE,payload TEXT NOT NULL);
 CREATE TABLE IF NOT EXISTS events(seq INTEGER PRIMARY KEY AUTOINCREMENT,stream TEXT NOT NULL,run TEXT NOT NULL,payload TEXT NOT NULL);
 CREATE INDEX IF NOT EXISTS events_stream_run ON events(stream,run,seq);
 CREATE TABLE IF NOT EXISTS inbox(seq INTEGER PRIMARY KEY AUTOINCREMENT,id TEXT NOT NULL UNIQUE,stream TEXT NOT NULL,run TEXT NOT NULL,conversation TEXT NOT NULL,status TEXT NOT NULL,payload TEXT NOT NULL);
 CREATE INDEX IF NOT EXISTS inbox_stream_run ON inbox(stream,run,status,seq);
 CREATE TABLE IF NOT EXISTS operations(id TEXT PRIMARY KEY,digest TEXT NOT NULL,response TEXT NOT NULL);
 PRAGMA user_version=1;";
fn now() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}
fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}
fn fields(v: &Value, allowed: &[&str]) -> anyhow::Result<()> {
    let object = v.as_object().context("Expected a JSON object")?;
    ensure!(
        object.keys().all(|k| allowed.contains(&k.as_str())),
        "Unknown request field"
    );
    Ok(())
}
fn bounded(value: &str, max: usize, multiline: bool) -> bool {
    value.len() <= max
        && !value
            .chars()
            .any(|c| c.is_control() && !(multiline && matches!(c, '\n' | '\t')))
}
fn uuid(value: &str) -> anyhow::Result<()> {
    let id = uuid::Uuid::parse_str(value).context("Expected a UUID")?;
    ensure!(
        !id.is_nil() && id.to_string() == value,
        "Expected a canonical nonzero UUID"
    );
    Ok(())
}
fn name(value: &str) -> anyhow::Result<()> {
    let parts: Vec<_> = value.split('/').collect();
    ensure!(
        value.len() <= 240
            && parts.len() == 3
            && parts[0] == "external"
            && parts[1..].iter().all(|p| !p.is_empty()
                && !matches!(*p, "." | "..")
                && p.trim() == *p
                && bounded(p, 100, false)
                && !p.contains(['\\', ':'])),
        "Use an explicit external/project/session name"
    );
    Ok(())
}
fn conversation(url: &str) -> anyhow::Result<String> {
    let id = url
        .strip_prefix("https://chatgpt.com/c/")
        .context("Use the exact https://chatgpt.com/c/<UUID> conversation URL")?;
    uuid(id)?;
    Ok(id.to_owned())
}
fn mode(value: &str) -> anyhow::Result<()> {
    ensure!(MODES.contains(&value), "Choose chatgpt_chat, chatgpt_work or codex_chat explicitly; do not infer a mode from the model");
    Ok(())
}
fn checked(record: &Value, input: &Value) -> anyhow::Result<()> {
    ensure!(
        input["expected_run_id"] == record["run_id"]
            && input["expected_conversation_id"] == record["conversation_id"],
        "External conversation identity changed; refresh before acting"
    );
    Ok(())
}
fn input() -> anyhow::Result<Value> {
    let mut bytes = Vec::new();
    io::stdin()
        .take((INPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= INPUT_BYTES,
        "External request exceeds 128 KiB"
    );
    if bytes.is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_slice(&bytes).context("Invalid external JSON")
}
fn db(config: &Config, write: bool) -> anyhow::Result<Option<Connection>> {
    let directory = config.state.join("external-workstreams");
    let path = directory.join("workstreams.sqlite3");
    if !write && !path.try_exists()? {
        return Ok(None);
    }
    if write {
        platform::private_dir(&directory)?;
    }
    ensure!(
        platform::verify_private(&directory, platform::Private::Directory)?,
        "External directory must be private, owned and not redirected"
    );
    if write && !path.try_exists()? {
        match platform::private_open(fs::OpenOptions::new().write(true).create_new(true))
            .open(&path)
        {
            Ok(_) => (),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(e.into()),
        }
    }
    ensure!(
        platform::verify_private(&path, platform::Private::File)?,
        "External store must be a private regular file with one owner/link"
    );
    let db = Connection::open_with_flags(
        &path,
        (if write {
            OpenFlags::SQLITE_OPEN_READ_WRITE
        } else {
            OpenFlags::SQLITE_OPEN_READ_ONLY
        }) | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    db.busy_timeout(Duration::from_secs(2))?;
    let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    ensure!(
        version == 1 || (write && version == 0),
        "Unknown external store schema; do not replace it"
    );
    if write {
        db.execute_batch(SCHEMA)?;
    }
    Ok(Some(db))
}
fn load(db: &Connection, session: &str) -> anyhow::Result<Value> {
    name(session)?;
    let data: String = db
        .query_row(
            "SELECT payload FROM workstreams WHERE name=?",
            [session],
            |r| r.get(0),
        )
        .optional()?
        .context("External workstream is not registered on this computer")?;
    ensure!(data.len() <= 64 * 1024, "External record exceeds bound");
    let value: Value = serde_json::from_str(&data)?;
    ensure!(
        value["schema_version"] == 1 && value["name"] == session,
        "Invalid external record identity"
    );
    uuid(text(&value, "id"))?;
    uuid(text(&value, "run_id"))?;
    ensure!(
        conversation(text(&value, "url"))? == text(&value, "conversation_id"),
        "Invalid external conversation reference"
    );
    mode(text(&value, "kind"))?;
    Ok(value)
}
fn save(db: &Connection, record: &Value) -> anyhow::Result<()> {
    let encoded = record.to_string();
    ensure!(encoded.len() <= 64 * 1024, "External record exceeds bound");
    db.execute("INSERT INTO workstreams(id,name,payload) VALUES(?,?,?) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload",params![record["id"].as_str(),record["name"].as_str(),encoded])?;
    Ok(())
}
fn event(
    db: &Connection,
    record: &Value,
    kind: &str,
    detail: &str,
    request: &str,
) -> anyhow::Result<()> {
    ensure!(
        bounded(detail, TEXT_BYTES + 256, true),
        "External event exceeds text bound"
    );
    let row = json!({"at":now(),"type":kind,"detail":detail,"source":"external_checkpoint","message_id":request,
        "reply_id":if kind=="AgentMessage"{json!(request)}else{Value::Null},"run_id":record["run_id"],"conversation_id":record["conversation_id"],"external_report":true});
    db.execute(
        "INSERT INTO events(stream,run,payload) VALUES(?,?,?)",
        params![
            record["id"].as_str(),
            record["run_id"].as_str(),
            row.to_string()
        ],
    )?;
    Ok(())
}
fn cursor(record: &Value, seq: i64) -> String {
    format!("{}:{seq}", text(record, "run_id"))
}
fn decode_cursor(record: &Value, value: &str) -> anyhow::Result<i64> {
    let (run, number) = value
        .split_once(':')
        .context("Invalid external history cursor")?;
    let seq = number
        .parse::<i64>()
        .context("Invalid external history cursor")?;
    ensure!(
        run == text(record, "run_id") && seq > 0 && seq.to_string() == number,
        "History cursor belongs to another generation"
    );
    Ok(seq)
}
fn history(db: &Connection, record: &Value, v: &Value) -> anyhow::Result<Value> {
    fields(
        v,
        &[
            "request_id",
            "expected_run_id",
            "expected_conversation_id",
            "limit",
            "before",
            "after",
            "archive_id",
            "agent_id",
        ],
    )?;
    checked(record, v)?;
    if let Some(id) = v.get("request_id") {
        uuid(id.as_str().context("Invalid request identity")?)?;
    }
    ensure!(
        ["archive_id", "agent_id"]
            .iter()
            .all(|k| v[*k].is_null() || v[*k] == ""),
        "External history does not substitute archive or child scope"
    );
    let limit = match v.get("limit") {
        None => 50,
        Some(v) => v.as_i64().context("Expected integer limit")?,
    };
    ensure!(
        (1..=100).contains(&limit),
        "History page must contain 1 to 100 rows"
    );
    for key in ["before", "after"] {
        ensure!(
            v[key].is_null() || v[key].is_string(),
            "History cursor must be text"
        );
    }
    let before = text(v, "before");
    let after = text(v, "after");
    ensure!(
        before.is_empty() || after.is_empty(),
        "Choose one history cursor"
    );
    let ascending = !after.is_empty();
    let bound = if ascending {
        decode_cursor(record, after)?
    } else if before.is_empty() {
        i64::MAX
    } else {
        decode_cursor(record, before)?
    };
    let sql = if ascending {
        "SELECT seq,payload FROM events WHERE stream=? AND run=? AND seq>? ORDER BY seq LIMIT ?"
    } else {
        "SELECT seq,payload FROM events WHERE stream=? AND run=? AND seq<? ORDER BY seq DESC LIMIT ?"
    };
    let mut q = db.prepare(sql)?;
    let raw = q
        .query_map(
            params![
                record["id"].as_str(),
                record["run_id"].as_str(),
                bound,
                limit + 1
            ],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut rows = Vec::new();
    let mut bytes = 0;
    for (seq, payload) in raw.into_iter().take(limit as usize) {
        ensure!(
            payload.len() <= TEXT_BYTES + 4096,
            "Invalid external event size"
        );
        bytes += payload.len();
        ensure!(
            bytes <= 2 * 1024 * 1024,
            "External history response exceeds bound"
        );
        let mut row: Value = serde_json::from_str(&payload)?;
        row["seq"] = json!(seq);
        row["incoming_seq"] = json!(seq);
        row["history_id"] = json!(cursor(record, seq));
        rows.push(row);
    }
    if !ascending {
        rows.reverse();
    }
    let first = rows
        .first()
        .map(|r| cursor(record, r["seq"].as_i64().unwrap_or(0)));
    let last = rows
        .last()
        .map(|r| cursor(record, r["seq"].as_i64().unwrap_or(0)));
    let first_seq = rows.first().and_then(|r| r["seq"].as_i64());
    let last_seq = rows.last().and_then(|r| r["seq"].as_i64());
    let exists = |operator: &str, seq: Option<i64>| -> anyhow::Result<bool> {
        let Some(seq) = seq else { return Ok(false) };
        let sql = format!(
            "SELECT EXISTS(SELECT 1 FROM events WHERE stream=? AND run=? AND seq{operator}?)"
        );
        Ok(db.query_row(
            &sql,
            params![record["id"].as_str(), record["run_id"].as_str(), seq],
            |r| r.get(0),
        )?)
    };
    let has_before = exists("<", first_seq)?;
    let has_after = exists(">", last_seq)?;
    let mut output = json!({"schema_version":1,"name":record["name"],"run_id":record["run_id"],"conversation_id":record["conversation_id"],"request_id":v["request_id"],"history_epoch":record["run_id"],
        "events":rows,"complete":true,"partial":false,"has_older":has_before,"has_newer":has_after,"older_cursor":first,"newer_cursor":last,"before":first,"after":last,
        "source":"external_checkpoints_only","cloud_transcript_available":false});
    output.as_object_mut().unwrap().extend(json!({"has_more_before":has_before,"has_more_after":has_after,
        "next_before":if has_before{first}else{None},"next_after":if has_after{last}else{None},
        "head":{"incoming_seq":null,"total_incoming":null,"complete":true},"indexing":false,"truncated":false,
        "source_status":{"provider":"external_checkpoints_only"}}).as_object().unwrap().clone());
    Ok(output)
}
fn summary(db: &Connection, r: &Value) -> anyhow::Result<Value> {
    let pending: i64 = db.query_row(
        "SELECT count(*) FROM inbox WHERE stream=? AND run=? AND status IN ('queued','accepted')",
        params![r["id"].as_str(), r["run_id"].as_str()],
        |r| r.get(0),
    )?;
    let parts: Vec<_> = text(r, "name").split('/').collect();
    let fresh = r["reported_at"]
        .as_f64()
        .is_some_and(|v| now() - v <= 300.0 && v <= now() + 5.0);
    let activity = if !fresh {
        "unknown"
    } else if r["reported_state"] == "working" {
        "busy"
    } else {
        "idle"
    };
    let mut row = json!({"name":r["name"],"cmd":"external","agent":"external","backend":"external-workstream","external_protocol":"cooperative-inbox-v1","project":parts[1],"tag":parts[2],"created":r["created"],"state":"running","tracked":true,
        "run_id":r["run_id"],"conversation_id":r["conversation_id"],"external_workstream_id":r["id"],"client_context":{"schema_version":1,"kind":r["kind"],"evidence":"explicit_registration"},
        "external_url":r["url"],"cwd":r["cwd"],"activity":activity,"phase":"idle","process_state":"external","runtime_state":"external","reported_at":r["reported_at"],"last_event_at":r["updated"],
        "external_status_fresh":fresh,"activity_summary":if fresh{"External agent checkpoint"}else{"External chat - last checkpoint only"},"activity_detail":text(r,"summary").chars().take(512).collect::<String>(),"external_next_steps":text(r,"next_steps").chars().take(512).collect::<String>(),"external_inbox_pending":pending});
    row.as_object_mut().unwrap().extend(json!({"terminal_supported":false,"terminal_attach_supported":false,"terminal_reason":"External chats have no owned terminal","bulk_lifecycle_supported":false,"allowed_actions":[],"action_reasons":{},"native_ui_available":false,
        "inspection_available":true,"send_supported":true,"send_available":true,"send_delivery":"cooperative_inbox","send_warning":"Queue only: the agent must poll through Desktop Commander. This does not send to or wake ChatGPT.",
        "attachments_supported":false,"interrupt_supported":false,"settings_change_supported":false,"history_supported":true,"external_cloud_control":false,"unread_reply":false,"pending_questions":[],"cloud_transcript_available":false}).as_object().unwrap().clone());
    Ok(row)
}
fn inspect(db: &Connection, r: &Value) -> anyhow::Result<Value> {
    let mut value = summary(db, r)?;
    let page = history(
        db,
        r,
        &json!({"expected_run_id":r["run_id"],"expected_conversation_id":r["conversation_id"],"limit":50}),
    )?;
    value["events"] = page["events"].clone();
    value["history_truncated"] = page["has_older"].clone();
    value["history_epoch"] = r["run_id"].clone();
    value["replace_events"] = json!(true);
    value["cursor"] = json!(0);
    value["checkpoint"] =
        json!({"summary":r["summary"],"next_steps":r["next_steps"],"reported_at":r["reported_at"]});
    Ok(value)
}
fn register(db: &Connection, v: &Value) -> anyhow::Result<Value> {
    fields(
        v,
        &[
            "request_id",
            "name",
            "kind",
            "url",
            "cwd",
            "summary",
            "next_steps",
        ],
    )?;
    let session = text(v, "name");
    name(session)?;
    mode(text(v, "kind"))?;
    let conversation_id = conversation(text(v, "url"))?;
    let cwd = text(v, "cwd");
    ensure!(
        Path::new(cwd).is_absolute() && bounded(cwd, 4096, false),
        "Use an explicit absolute workspace on this computer"
    );
    let count: i64 = db.query_row(
        "SELECT count(*) FROM workstreams WHERE name=?",
        [session],
        |r| r.get(0),
    )?;
    ensure!(
        count == 0,
        "Workstream name already registered; use its exact identity for checkpoints or handoff"
    );
    for key in ["summary", "next_steps"] {
        ensure!(
            v[key].is_null() || v[key].is_string(),
            "Invalid checkpoint field"
        );
        ensure!(
            bounded(text(v, key), TEXT_BYTES, true),
            "Checkpoint exceeds 16 KiB"
        );
    }
    let at = now();
    let r = json!({"schema_version":1,"id":uuid::Uuid::new_v4().to_string(),"name":session,"kind":v["kind"],"url":v["url"],"conversation_id":conversation_id,
        "run_id":uuid::Uuid::new_v4().to_string(),"cwd":cwd,"created":at,"updated":at,"summary":text(v,"summary"),"next_steps":text(v,"next_steps"),"reported_at":at,"reported_state":"ready"});
    save(db, &r)?;
    event(
        db,
        &r,
        "AgentMessage",
        "External workstream registered; checkpoints are not the cloud transcript.",
        text(v, "request_id"),
    )?;
    summary(db, &r)
}
fn mutate(db: &Connection, op: &str, v: &Value) -> anyhow::Result<Value> {
    if op == "register" {
        return register(db, v);
    }
    let mut r = load(db, text(v, "name"))?;
    checked(&r, v)?;
    match op {
        "checkpoint" => {
            fields(
                v,
                &[
                    "request_id",
                    "name",
                    "expected_run_id",
                    "expected_conversation_id",
                    "summary",
                    "next_steps",
                    "state",
                ],
            )?;
            ensure!(
                matches!(text(v, "state"), "working" | "ready" | "blocked"),
                "Choose an explicitly reported working, ready or blocked state"
            );
            for key in ["summary", "next_steps"] {
                ensure!(
                    v[key].is_string() && bounded(text(v, key), TEXT_BYTES, true),
                    "Expected bounded checkpoint text"
                );
            }
            r["summary"] = v["summary"].clone();
            r["next_steps"] = v["next_steps"].clone();
            r["reported_at"] = json!(now());
            r["reported_state"] = v["state"].clone();
            r["updated"] = json!(now());
            save(db, &r)?;
            event(
                db,
                &r,
                "AgentMessage",
                text(v, "summary"),
                text(v, "request_id"),
            )?;
            summary(db, &r)
        }
        "handoff" => {
            fields(
                v,
                &[
                    "request_id",
                    "name",
                    "expected_run_id",
                    "expected_conversation_id",
                    "url",
                    "kind",
                ],
            )?;
            mode(text(v, "kind"))?;
            let next = conversation(text(v, "url"))?;
            ensure!(
                next != text(&r, "conversation_id"),
                "Handoff requires a different explicitly selected conversation"
            );
            let pending:i64=db.query_row("SELECT count(*) FROM inbox WHERE stream=? AND run=? AND status IN ('queued','accepted')",params![r["id"].as_str(),r["run_id"].as_str()],|r|r.get(0))?;
            ensure!(pending==0,"Resolve outstanding inbox messages before handoff; requests are never moved to another chat");
            r["previous_conversation"] = json!({"run_id":r["run_id"],"conversation_id":r["conversation_id"],"url":r["url"],"kind":r["kind"]});
            event(
                db,
                &r,
                "AgentMessage",
                "External handoff explicitly recorded; the old chat was not stopped or changed.",
                text(v, "request_id"),
            )?;
            r["conversation_id"] = json!(next);
            r["url"] = v["url"].clone();
            r["kind"] = v["kind"].clone();
            r["run_id"] = json!(uuid::Uuid::new_v4().to_string());
            r["updated"] = json!(now());
            r["reported_at"] = Value::Null;
            save(db, &r)?;
            event(
                db,
                &r,
                "AgentMessage",
                &format!(
                    "Checkpoint carried from previous chat (not a new provider reply):\n{}",
                    text(&r, "summary")
                ),
                text(v, "request_id"),
            )?;
            summary(db, &r)
        }
        "ack" => {
            fields(
                v,
                &[
                    "request_id",
                    "name",
                    "expected_run_id",
                    "expected_conversation_id",
                    "message_id",
                    "status",
                ],
            )?;
            uuid(text(v, "message_id"))?;
            let previous: Option<String> = db
                .query_row(
                    "SELECT status FROM inbox WHERE id=? AND stream=? AND run=? AND conversation=?",
                    params![
                        v["message_id"].as_str(),
                        r["id"].as_str(),
                        r["run_id"].as_str(),
                        r["conversation_id"].as_str()
                    ],
                    |r| r.get(0),
                )
                .optional()?;
            let previous =
                previous.context("Message does not belong to this exact workstream generation")?;
            let next = text(v, "status");
            ensure!(
                matches!(
                    (previous.as_str(), next),
                    ("queued", "accepted" | "rejected") | ("accepted", "completed" | "failed")
                ) || previous == next,
                "Invalid acknowledgement transition"
            );
            db.execute(
                "UPDATE inbox SET status=? WHERE id=?",
                params![next, v["message_id"].as_str()],
            )?;
            event(
                db,
                &r,
                "AgentMessage",
                &format!(
                    "External inbox {}: {} (reported by cooperative agent)",
                    text(v, "message_id"),
                    next
                ),
                text(v, "request_id"),
            )?;
            Ok(
                json!({"request_id":v["request_id"],"message_id":v["message_id"],"status":next,"run_id":r["run_id"],"conversation_id":r["conversation_id"],"provider_delivery":"not_attempted"}),
            )
        }
        "send" => {
            fields(
                v,
                &[
                    "request_id",
                    "name",
                    "expected_run_id",
                    "expected_conversation_id",
                    "text",
                    "attachments",
                    "expected_compaction_id",
                ],
            )?;
            ensure!(
                v["text"].is_string()
                    && !text(v, "text").trim().is_empty()
                    && bounded(text(v, "text"), TEXT_BYTES, true),
                "Message must be nonempty text at most 16 KiB"
            );
            ensure!(
                v["attachments"].is_null()
                    || v["attachments"].as_array().is_some_and(Vec::is_empty),
                "External inbox does not support attachments"
            );
            ensure!(
                v["expected_compaction_id"].is_null() || v["expected_compaction_id"] == "",
                "External chats have no native compaction identity"
            );
            let count:i64=db.query_row("SELECT count(*) FROM inbox WHERE stream=? AND run=? AND status IN ('queued','accepted')",params![r["id"].as_str(),r["run_id"].as_str()],|r|r.get(0))?;
            ensure!(
                count < INBOX_LIMIT,
                "External inbox is full; resolve messages before submitting more"
            );
            let at = now();
            let row = json!({"request_id":v["request_id"],"text":v["text"],"created_at":at,"expected_run_id":r["run_id"],"expected_conversation_id":r["conversation_id"]});
            db.execute(
                "INSERT INTO inbox(id,stream,run,conversation,status,payload) VALUES(?,?,?,?,?,?)",
                params![
                    v["request_id"].as_str(),
                    r["id"].as_str(),
                    r["run_id"].as_str(),
                    r["conversation_id"].as_str(),
                    "queued",
                    row.to_string()
                ],
            )?;
            event(
                db,
                &r,
                "UserPromptSubmit",
                &format!(
                    "[Queued in external inbox; not sent to ChatGPT]\n{}",
                    text(v, "text")
                ),
                text(v, "request_id"),
            )?;
            Ok(
                json!({"request_id":v["request_id"],"name":r["name"],"run_id":r["run_id"],"conversation_id":r["conversation_id"],"status":"submitted","transport":"external_inbox","delivery_state":"queued","provider_delivery":"not_attempted",
                "submitted_at":at,"text":v["text"],"submitted_text":v["text"],"attachments":[],"warning":"Saved locally; the external agent must explicitly poll and acknowledge it. ChatGPT has not been prompted."}),
            )
        }
        _ => bail!("Unknown external workstream operation"),
    }
}
fn operation(db: &mut Connection, op: &str, v: &Value) -> anyhow::Result<Value> {
    uuid(text(v, "request_id"))?;
    let digest = format!(
        "{:x}",
        Sha256::digest(json!({"operation":op,"payload":v}).to_string())
    );
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let prior: Option<(String, String)> = tx
        .query_row(
            "SELECT digest,response FROM operations WHERE id=?",
            [text(v, "request_id")],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((old, response)) = prior {
        ensure!(
            old == digest,
            "Request UUID already reserved for different content or target"
        );
        return Ok(serde_json::from_str(&response)?);
    }
    let result = mutate(&tx, op, v)?;
    tx.execute(
        "INSERT INTO operations(id,digest,response) VALUES(?,?,?)",
        params![v["request_id"].as_str(), digest, result.to_string()],
    )?;
    tx.commit()?;
    Ok(result)
}
fn inbox(db: &Connection, v: &Value) -> anyhow::Result<Value> {
    fields(
        v,
        &[
            "name",
            "expected_run_id",
            "expected_conversation_id",
            "after",
            "limit",
        ],
    )?;
    let r = load(db, text(v, "name"))?;
    checked(&r, v)?;
    let limit = match v.get("limit") {
        None => 50,
        Some(x) => x.as_i64().context("Expected integer limit")?,
    };
    ensure!(
        (1..=100).contains(&limit),
        "Inbox page must contain 1 to 100 entries"
    );
    let after = if text(v, "after").is_empty() {
        0
    } else {
        decode_cursor(&r, text(v, "after"))?
    };
    let mut q=db.prepare("SELECT seq,status,payload FROM inbox WHERE stream=? AND run=? AND status IN ('queued','accepted') AND seq>? ORDER BY seq LIMIT ?")?;
    let mut rows = q
        .query_map(
            params![r["id"].as_str(), r["run_id"].as_str(), after, limit + 1],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            },
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let more = rows.len() > limit as usize;
    rows.truncate(limit as usize);
    let next = rows.last().map(|row| cursor(&r, row.0));
    let data: Vec<Value> = rows
        .into_iter()
        .map(|(seq, status, data)| {
            ensure!(data.len() <= TEXT_BYTES + 4096, "Inbox row exceeds bound");
            let mut v: Value = serde_json::from_str(&data)?;
            v["status"] = json!(status);
            v["sequence"] = json!(seq);
            Ok(v)
        })
        .collect::<anyhow::Result<_>>()?;
    Ok(
        json!({"name":v["name"],"run_id":r["run_id"],"conversation_id":r["conversation_id"],"messages":data,"has_more":more,"next":if more{next}else{None},"read_does_not_acknowledge":true}),
    )
}
fn listing(db: &Connection, after: &str, limit: i64) -> anyhow::Result<Value> {
    ensure!(
        (1..=LIST_LIMIT).contains(&limit) && bounded(after, 240, false),
        "Invalid bounded external catalog page"
    );
    let mut q = db.prepare("SELECT name FROM workstreams WHERE name>? ORDER BY name LIMIT ?")?;
    let mut names = q
        .query_map(params![after, limit + 1], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let more = names.len() > limit as usize;
    names.truncate(limit as usize);
    let next = names.last().cloned();
    let rows = names
        .into_iter()
        .map(|name| summary(db, &load(db, &name)?))
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(json!({"sessions":rows,"has_more":more,"next":if more{next}else{None}}))
}
pub(crate) fn append_listing(config: &Config, raw: &mut Value) -> Result<()> {
    (||->anyhow::Result<()>{let Some(db)=db(config,false)?else{return Ok(());};let page=listing(&db,"",LIST_LIMIT)?;
        raw["sessions"].as_array_mut().context("Invalid native session list")?.extend(page["sessions"].as_array().context("Invalid external page")?.iter().cloned());
        raw["external_catalog"]=json!({"schema_version":1,"has_more":page["has_more"],"next":page["next"],"limit":LIST_LIMIT});Ok(())})().map_err(|e|Error::new(1,e.to_string()))
}
pub(crate) fn reserved(session: &str) -> bool {
    session.starts_with("external/")
}
pub(crate) fn dispatch(config: &Config, args: &[String], dry: bool) -> Result<i32> {
    let result = (|| -> anyhow::Result<Value> {
        ensure!(
            !dry && args.len() == 2 && args[1] == "--json",
            "usage: hgs external register|checkpoint|handoff|inbox|ack|list --json (JSON stdin)"
        );
        let v = input()?;
        match args[0].as_str() {
            "list" => {
                fields(&v, &["after", "limit"])?;
                let limit = match v.get("limit") {
                    None => 100,
                    Some(x) => x.as_i64().context("Expected integer limit")?,
                };
                ensure!((1..=LIST_LIMIT).contains(&limit), "Invalid catalog limit");
                if let Some(db) = db(config, false)? {
                    listing(&db, text(&v, "after"), limit)
                } else {
                    Ok(json!({"sessions":[],"has_more":false,"next":null}))
                }
            }
            "inbox" => inbox(
                &db(config, false)?.context("No external workstreams registered")?,
                &v,
            ),
            op @ ("register" | "checkpoint" | "handoff" | "ack") => {
                operation(&mut db(config, true)?.context("Missing store")?, op, &v)
            }
            _ => bail!("Unknown external workstream operation"),
        }
    })()
    .map_err(|e| Error::new(2, e.to_string()))?;
    println!("{result}");
    Ok(0)
}
pub(crate) fn session_command(
    config: &Config,
    command: &str,
    args: &[String],
    dry: bool,
) -> Result<i32> {
    let result=(||->anyhow::Result<Value>{ensure!(!dry&&!args.is_empty(),"External workstream requires an explicit request");let session=&args[0];name(session)?;
        match command{
            "inspect"=>{let mut i=1;while i<args.len(){match args[i].as_str(){"--skip-processes"=>i+=1,"--after"=>{ensure!(i+1<args.len(),"Missing inspection cursor");i+=2},_=>bail!("External inspection does not substitute child/archive scope")}}
                let db=db(config,false)?.context("No external workstreams registered")?;inspect(&db,&load(&db,session)?)},
            "send"|"history"=>{ensure!(args.len()==2&&args[1]=="--json","Expected exact JSON session request");let mut v=input()?;ensure!(v.is_object()&&!v.as_object().unwrap().contains_key("name"),"Session name comes from the command target");
                if command=="send"{v["name"]=json!(session);operation(&mut db(config,true)?.context("Missing store")?,"send",&v)}else{let db=db(config,false)?.context("Missing store")?;history(&db,&load(&db,session)?,&v)}},
            _=>bail!("External chats support inspection, checkpoint history and a cooperative inbox only; no cloud send, terminal, stop, approval or automatic continuation is available"),
        }})().map_err(|e|Error::new(2,e.to_string()))?;
    println!("{result}");
    Ok(0)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn store() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(SCHEMA).unwrap();
        db
    }
    fn request() -> Value {
        json!({"request_id":uuid::Uuid::new_v4().to_string(),"name":"external/project/chat","kind":"chatgpt_chat","url":"https://chatgpt.com/c/11111111-2222-4333-8444-555555555555","cwd":if cfg!(windows){"C:/workspace"}else{"/workspace"},"summary":"checkpoint","next_steps":"continue carefully"})
    }
    fn scope(r: &Value) -> Value {
        json!({"request_id":uuid::Uuid::new_v4().to_string(),"name":r["name"],"expected_run_id":r["run_id"],"expected_conversation_id":r["conversation_id"]})
    }
    #[test]
    fn strict_modes_urls_names() {
        for bad in [
            "https://evil.test/c/11111111-2222-4333-8444-555555555555",
            "https://chatgpt.com/c/11111111-2222-4333-8444-555555555555?token=x",
            "file:///tmp/file",
            "https://chatgpt.com@evil.test/c/x",
        ] {
            assert!(conversation(bad).is_err());
        }
        for bad in [
            "codex/project/chat",
            "external/a/../b",
            "external/../chat",
            "external/a/b\\c",
        ] {
            assert!(name(bad).is_err());
        }
        assert!(mode("gpt-6").is_err());
        assert!(mode("chatgpt_work").is_ok());
    }
    #[test]
    fn exact_retries_are_idempotent() {
        let mut db = store();
        let v = request();
        let r = operation(&mut db, "register", &v).unwrap();
        assert_eq!(r, operation(&mut db, "register", &v).unwrap());
        let mut changed = v;
        changed["kind"] = json!("chatgpt_work");
        assert!(operation(&mut db, "register", &changed).is_err());
        let mut send = scope(&r);
        send["text"] = json!("Inspect only");
        let result = operation(&mut db, "send", &send).unwrap();
        assert_eq!(result["provider_delivery"], "not_attempted");
        assert_eq!(result, operation(&mut db, "send", &send).unwrap());
        assert_eq!(
            db.query_row("SELECT count(*) FROM inbox", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
    #[test]
    fn reads_never_ack_and_handoff_never_retargets() {
        let mut db = store();
        let r = operation(&mut db, "register", &request()).unwrap();
        let mut send = scope(&r);
        send["text"] = json!("instruction");
        operation(&mut db, "send", &send).unwrap();
        let mut read = scope(&r);
        read.as_object_mut().unwrap().remove("request_id");
        assert_eq!(
            inbox(&db, &read).unwrap()["messages"][0]["status"],
            "queued"
        );
        let mut next = scope(&r);
        next["url"] = json!("https://chatgpt.com/c/22222222-2222-4333-8444-555555555555");
        next["kind"] = json!("chatgpt_work");
        assert!(operation(&mut db, "handoff", &next).is_err());
        let mut ack = scope(&r);
        ack["message_id"] = send["request_id"].clone();
        ack["status"] = json!("rejected");
        operation(&mut db, "ack", &ack).unwrap();
        let newer = operation(&mut db, "handoff", &next).unwrap();
        assert_ne!(newer["run_id"], r["run_id"]);
        assert_eq!(newer["client_context"]["kind"], "chatgpt_work");
        assert!(inbox(&db, &read).is_err());
        send["request_id"] = json!(uuid::Uuid::new_v4().to_string());
        assert!(operation(&mut db, "send", &send).is_err());
    }
    #[test]
    fn history_is_bounded_and_generation_scoped() {
        let mut db = store();
        let r = operation(&mut db, "register", &request()).unwrap();
        for n in 0..5 {
            let mut v = scope(&r);
            v["summary"] = json!(format!("step {n}"));
            v["next_steps"] = json!("later");
            v["state"] = json!("working");
            operation(&mut db, "checkpoint", &v).unwrap();
        }
        let record = load(&db, "external/project/chat").unwrap();
        let q = json!({"expected_run_id":r["run_id"],"expected_conversation_id":r["conversation_id"],"limit":2});
        let first = history(&db, &record, &q).unwrap();
        assert_eq!(first["events"].as_array().unwrap().len(), 2);
        assert_eq!(first["has_older"], true);
        let mut older = q;
        older["before"] = first["older_cursor"].clone();
        let second = history(&db, &record, &older).unwrap();
        assert_ne!(
            first["events"][0]["history_id"],
            second["events"][0]["history_id"]
        );
        older["before"] = json!("00000000-0000-4000-8000-000000000001:1");
        assert!(history(&db, &record, &older).is_err());
    }
    #[test]
    fn invalid_writes_leave_no_inbox_record() {
        let mut db = store();
        let r = operation(&mut db, "register", &request()).unwrap();
        let mut v = scope(&r);
        v["text"] = json!("hello");
        v["attachments"] = json!([{"path":"private"}]);
        assert!(operation(&mut db, "send", &v).is_err());
        v.as_object_mut().unwrap().remove("attachments");
        v["command"] = json!("echo bad");
        assert!(operation(&mut db, "send", &v).is_err());
        assert_eq!(
            db.query_row("SELECT count(*) FROM inbox", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
}
