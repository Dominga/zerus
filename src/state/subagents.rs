//! Read-only child Activity. The parent roster is the authority for membership;
//! an arbitrary thread ID or filesystem path is never an inspection target.
use super::*;
use rusqlite::{params, OptionalExtension};
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};

// One SQLite read snapshot for both the replacement page and its overflow.
// The probe visits at most 201 child index entries and fetches no payload;
// events_subagent_activity keeps unrelated sessions/children out of both reads.
const RECORDED_EVENTS: &str = "SELECT seq,payload,EXISTS(
    SELECT 1 FROM events WHERE name=?1 AND conversation IS ?2
      AND json_extract(payload,'$.agent_id')=?3 ORDER BY seq DESC LIMIT 1 OFFSET 200)
    FROM events WHERE name=?1 AND conversation IS ?2
      AND json_extract(payload,'$.agent_id')=?3 ORDER BY seq DESC LIMIT 200";

pub(super) fn inspection(name: &str, id: &str, archive_id: Option<&str>) -> Result<Value> {
    if id.is_empty()
        || id.len() > 160
        || id.contains(['/', '\\'])
        || id.chars().any(char::is_control)
    {
        return Err("Invalid subagent identity".into());
    }
    let record = {
        let _guard = lock(None)?;
        if let Some(archive) = archive_id {
            archive::read_archive(name, archive)?
        } else {
            read(name)?
        }
    };
    let child = record["subagents"]
        .get(id)
        .filter(|v| v.is_object())
        .ok_or("This subagent is not in the selected conversation")?;
    let external = child["external"] == true;
    let mut native_record = record.clone();
    if external {
        native_record["agent"] = child["provider"].clone();
        native_record["conversation_id"] = child["conversation_id"].clone();
        for key in ["transcript", "cwd", "agent_home"] {
            native_record[key] = child[key].clone();
        }
    }
    let mut output = json!({"name":name,"agent_id":id,"parent_conversation_id":record["conversation_id"],
        "conversation_id":format!("{}/{}",string(&record,"conversation_id"),id),"run_id":record["run_id"],
        "provider":native_record["agent"],"model":child["model"],"label":child["name"],"tracked":true,"read_only":true,"cwd":native_record["cwd"],
        "history_scope":"Recorded subagent activity","state":child["state"],"cursor":0,
        "completion_source":child["completion_source"],"tracking_evidence":child["tracking_evidence"]});
    if let Some(archive) = archive_id {
        output["archive_id"] = json!(archive);
        output["state"] = json!("archived");
    }
    let db = journal::event_db()?;
    let mut query = db.prepare(RECORDED_EVENTS).map_err(|e| e.to_string())?;
    let rows = query
        .query_map(
            params![
                journal::journal_name(&record),
                record["conversation_id"].as_str(),
                id
            ],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, bool>(2)?,
                ))
            },
        )
        .map_err(|e| e.to_string())?;
    let mut events = Vec::new();
    let mut hook_truncated = false;
    for row in rows {
        let (seq, payload, truncated) = row.map_err(|e| e.to_string())?;
        hook_truncated |= truncated;
        let mut event: Value = serde_json::from_str(&payload).map_err(|e| e.to_string())?;
        event["seq"] = json!(seq);
        event["agent_id"] = json!("");
        if event["type"] == "SubagentStop" {
            event["type"] = json!("Stop");
        }
        events.push(event);
    }
    events.reverse();
    let messages = if external {
        provider_messages::read(&native_record)
            .map(|messages| Some((messages, string(&native_record, "cwd").to_owned(), false)))
    } else {
        native_messages(&record, id)
    };
    let mut source_truncated = false;
    match messages {
        Ok(Some((messages, cwd, truncated))) if !messages.is_empty() => {
            // Native messages replace hook excerpts; tool events retain their
            // provenance and are merged chronologically by ActivityView.
            events.retain(|e| {
                !["Stop", "AgentMessage"].contains(&string(e, "type"))
                    && (external
                        || !["UserPromptSubmit", "TurnStarted"].contains(&string(e, "type")))
            });
            events.extend(messages);
            if !cwd.is_empty() {
                output["cwd"] = json!(cwd);
            }
            source_truncated = truncated;
            output["history_scope"] = json!("Native subagent history");
        }
        Err(_) => {
            output["history_scope"] =
                json!("Recorded events; native subagent history is unavailable");
        }
        _ => {}
    }
    if merge_activity_events(&mut events, source_truncated || hook_truncated) {
        output["history_truncated"] = json!(true);
    }
    if events.is_empty() && !string(child, "detail").is_empty() {
        events.push(json!({"type":"AgentMessage","agent_id":"","at":child["updated"],"detail":child["detail"],"source":"subagent_summary"}));
        output["history_scope"] = json!("Only the last reported subagent result is available");
    }
    output["events"] = json!(events);
    if external {
        output["session_usage"] = usage::read(&native_record);
    }
    Ok(output)
}

fn merge_activity_events(events: &mut Vec<Value>, mut truncated: bool) -> bool {
    fn at(value: &Value) -> f64 {
        value["at"].as_f64().unwrap_or_default()
    }
    events.sort_by(|a, b| at(a).total_cmp(&at(b)));
    if events.len() > 200 {
        events.drain(..events.len() - 200);
        truncated = true;
    }
    truncated
}

fn native_messages(record: &Value, id: &str) -> Result<Option<(Vec<Value>, String, bool)>> {
    let agent = string(record, "agent");
    let native_home = if string(record, "agent_home").is_empty() {
        home().join(match agent {
            "codex" => ".codex",
            "claude" => ".claude",
            "kimi" => ".kimi-code",
            _ => return Ok(None),
        })
    } else {
        PathBuf::from(string(record, "agent_home"))
    };
    let mut cwd = String::new();
    let path = match agent {
        "codex" => {
            if uuid::Uuid::parse_str(id).is_err() {
                return Ok(None);
            }
            let db = codex_sqlite::NativeConnection::open(native_home.join("state_5.sqlite"))
                .map_err(|e| e.to_string())?;
            db.busy_timeout(std::time::Duration::from_millis(20))
                .map_err(|e| e.to_string())?;
            let row = db
                .query_row(
                    "SELECT rollout_path,cwd FROM threads WHERE id=?1",
                    [id],
                    |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
                )
                .optional()
                .map_err(|e| e.to_string())?;
            let Some((path, dir)) = row else {
                return Ok(None);
            };
            cwd = dir;
            PathBuf::from(path)
        }
        "claude" => {
            let Some(path) = claude_subagents::paths(record, &[id]).remove(id) else {
                return Ok(None);
            };
            path
        }
        "kimi" => {
            let Some(main) = questions::wire_paths(record)?.into_iter().next() else {
                return Ok(None);
            };
            main.parent()
                .and_then(Path::parent)
                .ok_or("Invalid wire location")?
                .join(id)
                .join("wire.jsonl")
        }
        _ => return Ok(None),
    };
    let path = crate::platform::canonical(&path).map_err(|e| e.to_string())?;
    if !path.starts_with(crate::platform::canonical(&native_home).map_err(|e| e.to_string())?) {
        return Err("Native history is outside the selected account".into());
    }
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    if agent == "codex" {
        let mut header = String::new();
        BufReader::new((&mut file).take(256 * 1024))
            .read_line(&mut header)
            .map_err(|e| e.to_string())?;
        let header: Value = serde_json::from_str(&header).map_err(|e| e.to_string())?;
        if header["type"] != "session_meta" || header["payload"]["id"] != id {
            return Err("Subagent transcript identity mismatch".into());
        }
    }
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
    let mut messages = Vec::new();
    for (index, line) in bytes.split_inclusive(|b| *b == b'\n').enumerate() {
        if (offset > 0 && index == 0) || line.len() > 1024 * 1024 || !line.ends_with(b"\n") {
            continue;
        }
        let Ok(mut event) = serde_json::from_slice::<Value>(line) else {
            continue;
        };
        if agent == "kimi" {
            if !["", id].contains(&string(&event, "agentId")) {
                continue;
            }
            event["agentId"] = json!("main");
        } else if agent == "claude" {
            if event["agentId"] != id || event["sessionId"] != record["conversation_id"] {
                continue;
            }
            // Sidechain messages are the selected child's own conversation.
            event["isSidechain"] = json!(false);
        }
        if let Some((message, _)) = search::provider_event(agent, &event) {
            if messages.last().is_none_or(|last: &Value| {
                last["detail"] != message["detail"] || last["type"] != message["type"]
            }) {
                messages.push(message);
            }
        }
    }
    let truncated = offset > 0 || messages.len() > 200;
    if messages.len() > 200 {
        messages.drain(..messages.len() - 200);
    }
    Ok(Some((messages, cwd, truncated)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn hook_db(db: &Connection) {
        db.execute_batch("CREATE TABLE events(seq INTEGER PRIMARY KEY,name TEXT,conversation TEXT,payload TEXT);
            CREATE INDEX events_subagent_activity ON events(name,conversation,json_extract(payload,'$.agent_id'),seq);")
            .unwrap();
    }

    fn hook(db: &Connection, name: &str, conversation: Option<&str>, child: &str) {
        db.execute(
            "INSERT INTO events(name,conversation,payload) VALUES(?1,?2,?3)",
            params![name, conversation, json!({"agent_id":child}).to_string()],
        )
        .unwrap();
    }

    #[test]
    fn hook_overflow_is_bound_to_the_exact_indexed_child_range() {
        let db = Connection::open_in_memory().unwrap();
        hook_db(&db);
        for _ in 0..200 {
            hook(&db, "selected", Some("conversation"), "child");
        }
        for _ in 0..201 {
            hook(&db, "other", Some("conversation"), "child");
            hook(&db, "selected", Some("other"), "child");
            hook(&db, "selected", None, "child");
            hook(&db, "selected", Some("conversation"), "other");
        }
        let mut query = db.prepare(RECORDED_EVENTS).unwrap();
        let mut read = |conversation: Option<&str>| {
            query
                .query_map(params!["selected", conversation, "child"], |r| {
                    Ok((r.get::<_, i64>(0)?, r.get::<_, bool>(2)?))
                })
                .unwrap()
                .collect::<std::result::Result<Vec<_>, _>>()
                .unwrap()
        };
        let page = read(Some("conversation"));
        assert_eq!(page.len(), 200);
        assert_eq!((page.first().unwrap().0, page.last().unwrap().0), (200, 1));
        assert!(
            page.iter().all(|row| !row.1),
            "other identities never imply overflow"
        );
        hook(&db, "selected", Some("conversation"), "child");
        assert!(read(Some("conversation")).iter().all(|row| row.1));
        assert!(
            read(None).iter().all(|row| row.1),
            "NULL conversation keeps its own range"
        );

        let plan = db
            .prepare(&format!("EXPLAIN QUERY PLAN {RECORDED_EVENTS}"))
            .unwrap()
            .query_map(params!["selected", "conversation", "child"], |r| {
                r.get::<_, String>(3)
            })
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        let searches = plan
            .iter()
            .filter(|step| step.starts_with("SEARCH events "))
            .collect::<Vec<_>>();
        assert_eq!(searches.len(), 2, "{plan:?}");
        assert!(
            searches
                .iter()
                .all(|step| step.contains("events_subagent_activity")
                    && step.contains("name=? AND conversation=? AND <expr>=?")),
            "{plan:?}"
        );
        assert!(
            !plan
                .iter()
                .any(|step| step.contains("SCAN events") || step.contains("TEMP B-TREE")),
            "{plan:?}"
        );
        // SQLite 3.46 does not label a covering expression-index probe in
        // QUERY PLAN. Its actual EXISTS subroutine must never read the table.
        let program = db
            .prepare(&format!("EXPLAIN {RECORDED_EVENTS}"))
            .unwrap()
            .query_map(params!["selected", "conversation", "child"], |r| {
                Ok((
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        let start = program.iter().position(|op| op.0 == "BeginSubrtn").unwrap();
        let end = start
            + program[start..]
                .iter()
                .position(|op| op.0 == "Return")
                .unwrap();
        let probe = &program[start..end];
        let table: i64 = db
            .query_row(
                "SELECT rootpage FROM sqlite_master WHERE name='events'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let table_cursors = probe
            .iter()
            .filter(|op| op.0 == "OpenRead" && op.2 == table)
            .map(|op| op.1)
            .collect::<Vec<_>>();
        assert!(
            !probe
                .iter()
                .any(|op| op.0 == "Column" && table_cursors.contains(&op.1)),
            "{program:?}"
        );
    }

    #[test]
    fn hook_page_and_overflow_share_a_snapshot_while_new_events_arrive() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.sqlite3");
        let db = Connection::open(&path).unwrap();
        db.pragma_update(None, "journal_mode", "WAL").unwrap();
        hook_db(&db);
        for _ in 0..200 {
            hook(&db, "selected", Some("conversation"), "child");
        }
        let writer = Connection::open(path).unwrap();
        let mut query = db.prepare(RECORDED_EVENTS).unwrap();
        let mut rows = query
            .query(params!["selected", "conversation", "child"])
            .unwrap();
        let first = rows.next().unwrap().unwrap();
        assert_eq!(first.get::<_, i64>(0).unwrap(), 200);
        assert!(!first.get::<_, bool>(2).unwrap());
        hook(&writer, "selected", Some("conversation"), "child");
        let mut expected = 199;
        while let Some(row) = rows.next().unwrap() {
            assert_eq!(row.get::<_, i64>(0).unwrap(), expected);
            assert!(
                !row.get::<_, bool>(2).unwrap(),
                "new events cannot tear the page's overflow flag"
            );
            expected -= 1;
        }
        assert_eq!(expected, 0);
        drop(rows);
        let mut rows = query
            .query(params!["selected", "conversation", "child"])
            .unwrap();
        let first = rows.next().unwrap().unwrap();
        assert_eq!(first.get::<_, i64>(0).unwrap(), 201);
        assert!(first.get::<_, bool>(2).unwrap());
    }

    fn event(at: i64, kind: &str) -> Value {
        json!({"at":at,"type":kind,"detail":format!("{kind} {at}")})
    }

    #[test]
    fn merged_page_keeps_the_latest_two_hundred_in_stable_order() {
        let mut events = (1..=150)
            .map(|at| event(at, "hook"))
            .chain((101..=250).map(|at| event(at, "native")))
            .collect::<Vec<_>>();
        assert!(merge_activity_events(&mut events, false));
        assert_eq!(events.len(), 200);
        assert_eq!(events.first().unwrap()["at"], 101);
        assert_eq!(events.last().unwrap()["at"], 250);
        assert!(events.iter().any(|event| event["type"] == "hook"));
        assert!(events.iter().any(|event| event["type"] == "native"));
        let hook = events
            .iter()
            .position(|event| event["at"] == 101 && event["type"] == "hook")
            .unwrap();
        let native = events
            .iter()
            .position(|event| event["at"] == 101 && event["type"] == "native")
            .unwrap();
        assert!(hook < native, "equal timestamps retain source order");
    }

    #[test]
    fn exactly_two_hundred_does_not_infer_truncation() {
        let mut events = (1..=100)
            .map(|at| event(at, "hook"))
            .chain((101..=200).map(|at| event(at, "native")))
            .collect::<Vec<_>>();
        assert!(!merge_activity_events(&mut events, false));
        assert_eq!(events.len(), 200);
        assert_eq!(events.first().unwrap()["at"], 1);
        assert_eq!(events.last().unwrap()["at"], 200);
    }

    #[test]
    fn native_truncation_is_retained_and_ties_keep_source_order() {
        let mut events = vec![event(1, "first"), event(1, "second"), event(1, "third")];
        assert!(merge_activity_events(&mut events, true));
        assert_eq!(
            events
                .iter()
                .map(|event| event["type"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["first", "second", "third"]
        );
    }
}
