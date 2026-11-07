//! Parent links between independent sessions (zerus-sa9qn.2, spec
//! `docs/superpowers/specs/2026-11-07-account-labels-child-sessions-design.md`).
//!
//! A child's record carries `parent`: the machine and durable session ID of
//! the live tracked agent that launched or adopted it, proven by native process
//! ancestry exactly as `hgs self` proves its caller. The child's computer is
//! the link's only writer. The link is presentation and attention routing
//! only: it never authorizes a send, answer, approval, stop or removal, and it
//! never changes project membership.
use super::*;
use std::io::Read;

/// The launching hgs's proven caller for a new session, as compact JSON. The
/// launcher always sets it, empty when no live tracked agent owns it, so an
/// inherited value never reaches the record.
pub(crate) const ENV: &str = "HGS_PARENT";
const USAGE: &str = "usage: hgs parent <session> --json   (JSON stdin: expected_run_id, expected_conversation_id, action read|adopt|clear, optional replace)";
/// The longest parent chain walked when refusing a cycle.
const DEPTH: usize = 8;

fn text(value: &Value, limit: usize, required: bool) -> bool {
    value.as_str().is_some_and(|s| {
        (!required || !s.is_empty()) && s.len() <= limit && !s.chars().any(char::is_control)
    })
}

/// A well-formed link: schema 1, a canonical machine UUID, a session ID or a
/// conversation or run to match, a session label and a known source.
pub(crate) fn valid(link: &Value) -> bool {
    let Some(object) = link.as_object() else {
        return false;
    };
    let canonical_machine = object
        .get("machine")
        .and_then(Value::as_str)
        .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok_and(|u| !u.is_nil() && u.hyphenated().to_string() == id));
    let session = object.get("session_id").and_then(Value::as_str).unwrap_or("");
    object.len() <= 12
        && object.get("schema") == Some(&json!(1))
        && canonical_machine
        && (session.is_empty() || session_id::canonical(session))
        && text(&object.get("host").cloned().unwrap_or(Value::Null), 80, true)
        && text(&object.get("name").cloned().unwrap_or(Value::Null), 512, true)
        && ["conversation_id", "run_id"]
            .iter()
            .all(|key| text(object.get(*key).unwrap_or(&json!("")), 512, false))
        && (!session.is_empty()
            || !string(link, "conversation_id").is_empty()
            || !string(link, "run_id").is_empty())
        && matches!(string(link, "source"), "launch" | "adopt")
        && object.get("at").is_some_and(Value::is_i64)
}

/// The link naming `record` (a live tracked session on this computer).
fn link_of(record: &Value, machine: &str, host: &str, source: &str) -> Option<Value> {
    let link = json!({"schema": 1, "machine": machine, "host": host,
        "session_id": string(record, "session_id"), "name": string(record, "name"),
        "conversation_id": string(record, "conversation_id"), "run_id": string(record, "run_id"),
        "source": source, "at": now() as i64});
    valid(&link).then_some(link)
}

/// The link a launch run by a live tracked agent records for its new session.
pub(crate) fn caller_link(machine: &str, host: &str) -> Option<String> {
    let record = session_graph::caller_record()?;
    link_of(&record, machine, host, "launch").map(|link| link.to_string())
}

/// `HGS_PARENT`'s value for a session this process launches: the proven
/// caller's link, or empty, which keeps an inherited value from the record.
pub(crate) fn launcher_value() -> String {
    crate::config::Config::load()
        .ok()
        .and_then(|config| {
            let machine = crate::swarm::local_node_id(&config).ok()?;
            caller_link(&machine, config.host())
        })
        .unwrap_or_default()
}

/// The link the launching hgs passed, for a new session's first record.
pub(super) fn from_env() -> Option<Value> {
    nonempty_env(ENV)
        .filter(|value| value.len() <= 4096)
        .and_then(|value| serde_json::from_str::<Value>(&value).ok())
        .filter(valid)
}

/// The record's link for listings, when well formed.
pub(super) fn listed(record: &Value) -> Option<Value> {
    record.get("parent").filter(|link| valid(link)).cloned()
}

/// The live state of each listed row's parent, from the same listing:
/// `live` (running, its agent not exited), `saved` (paused, stopped or
/// exited), `archived`, `missing`, or `remote` for a parent on another
/// computer, which this computer cannot see.
pub(super) fn decorate(sessions: &mut [Value]) {
    if !sessions.iter().any(|row| row.get("parent").is_some_and(Value::is_object)) {
        return;
    }
    let machine = crate::config::Config::load()
        .ok()
        .and_then(|config| crate::swarm::local_node_id(&config).ok())
        .unwrap_or_default();
    decorate_with(sessions, &machine);
}

fn decorate_with(sessions: &mut [Value], machine: &str) {
    let state = |row: &Value| -> &'static str {
        match row["state"].as_str().unwrap_or("running") {
            "archived" => "archived",
            "running" if row["process_state"] != "exited" => "live",
            _ => "saved",
        }
    };
    // A live or saved row wins over an archived copy of the same session.
    let mut by_session: BTreeMap<String, &'static str> = BTreeMap::new();
    let mut by_conversation: BTreeMap<String, &'static str> = BTreeMap::new();
    for row in sessions.iter() {
        let found = state(row);
        for (map, key) in [(&mut by_session, "session_id"), (&mut by_conversation, "conversation_id")] {
            let id = string(row, key);
            if key == "conversation_id" && !string(row, "session_id").is_empty() {
                continue;
            }
            if !id.is_empty() && map.get(id).is_none_or(|old| *old == "archived") {
                map.insert(id.to_owned(), found);
            }
        }
    }
    for row in sessions.iter_mut() {
        let Some(link) = row.get("parent").filter(|link| link.is_object()) else {
            continue;
        };
        let found = if machine.is_empty() || string(link, "machine") != machine {
            "remote"
        } else {
            by_session
                .get(string(link, "session_id"))
                .or_else(|| by_conversation.get(string(link, "conversation_id")))
                .copied()
                .unwrap_or("missing")
        };
        row["parent"]["state"] = json!(found);
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    expected_run_id: String,
    expected_conversation_id: String,
    action: String,
    #[serde(default)]
    replace: bool,
}

/// `hgs parent <session> --json`: read, adopt (set the link to the proven
/// caller) or clear one tracked session's parent link.
pub(super) fn dispatch(args: &[String]) -> Result<i32> {
    let [name, flag] = args else {
        return Err(USAGE.into());
    };
    if flag != "--json" || name.starts_with(['-', '@']) {
        return Err(USAGE.into());
    }
    let mut bytes = Vec::new();
    io::stdin()
        .take(8193)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 8192 {
        return Err("parent request exceeds limits".into());
    }
    let request: Request = serde_json::from_slice(&bytes).map_err(|_| USAGE.to_owned())?;
    if !matches!(request.action.as_str(), "read" | "adopt" | "clear") {
        return Err(USAGE.into());
    }
    if opencode::exists(name) || dsh::exists(name) {
        return Err("Parent links are recorded for Claude, Codex and Kimi sessions".into());
    }
    expect(name, &request.expected_run_id, &request.expected_conversation_id)?;
    // Prove the caller before taking the writer lock: its own record is read.
    let caller = if request.action == "adopt" {
        Some(session_graph::caller_record().ok_or("hgs parent adopt: no live Zerus session owns this process")?)
    } else {
        None
    };
    let config = crate::config::Config::load().map_err(|e| e.message)?;
    let machine = crate::swarm::local_node_id(&config).map_err(|e| e.to_string())?;
    let _writer = lock(None)?;
    let mut record = read(name)?;
    if string(&record, "run_id") != request.expected_run_id
        || string(&record, "conversation_id") != request.expected_conversation_id
    {
        return Err(format!("{name} changed; refresh and retry"));
    }
    if !string(&record, "archive_id").is_empty() || record["state"] == "archived" {
        return Err("Archived sessions keep their parent link; restore the session first".into());
    }
    let previous = listed(&record);
    match request.action.as_str() {
        "read" => {}
        "clear" => {
            if record.as_object_mut().and_then(|o| o.remove("parent")).is_some() {
                write(&mut record)?;
            }
        }
        _ => {
            let caller = caller.unwrap();
            let target = string(&record, "session_id");
            if string(&caller, "session_id") == target && !target.is_empty()
                || string(&caller, "run_id") == string(&record, "run_id")
            {
                return Err("A session cannot adopt itself".into());
            }
            if ancestors(&caller).contains(&target.to_owned()) {
                return Err("This session already leads the caller; adopting it would make a cycle".into());
            }
            if let Some(old) = previous.as_ref() {
                let same = string(old, "machine") == machine
                    && string(old, "session_id") == string(&caller, "session_id");
                if !same && !request.replace {
                    return Err(format!(
                        "{name} already has the parent {}; pass \"replace\": true to change it",
                        string(old, "name")
                    ));
                }
            }
            let link = link_of(&caller, &machine, config.host(), "adopt")
                .ok_or("The calling session has no complete identity yet; retry after it starts")?;
            record["parent"] = link;
            write(&mut record)?;
        }
    }
    println!("{}", json!({"ok": true, "name": name, "parent": listed(&record), "previous": previous}));
    Ok(0)
}

/// The session IDs of `record`'s parents on this computer, nearest first,
/// through the verified ID index only (no record scan).
fn ancestors(record: &Value) -> Vec<String> {
    let mut found = Vec::new();
    let mut current = record.clone();
    for _ in 0..DEPTH {
        let Some(link) = listed(&current) else { break };
        let id = string(&link, "session_id").to_owned();
        if id.is_empty() || found.contains(&id) {
            break;
        }
        found.push(id.clone());
        let Some(next) = session_index::find(&id).and_then(|hit| read(&hit.name).ok()) else {
            break;
        };
        if string(&next, "session_id") != id {
            break;
        }
        current = next;
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link() -> Value {
        json!({"schema":1,"machine":"11111111-1111-4111-8111-111111111111","host":"arch",
            "session_id":"22222222-2222-4222-8222-222222222222","name":"claude/p/lead",
            "conversation_id":"c","run_id":"r","source":"launch","at":1})
    }

    #[test]
    fn a_link_needs_a_canonical_machine_an_identity_a_label_and_a_known_source() {
        assert!(valid(&link()));
        for (key, value) in [
            ("schema", json!(2)), ("machine", json!("not-a-uuid")),
            ("machine", json!("00000000-0000-0000-0000-000000000000")),
            ("machine", json!("11111111-1111-4111-8111-11111111111A")),
            ("session_id", json!("X")), ("name", json!("")), ("name", json!("a\nb")),
            ("host", json!("")), ("source", json!("guess")), ("at", json!("now")),
        ] {
            let mut bad = link();
            bad[key] = value;
            assert!(!valid(&bad), "{key}");
        }
        let mut anonymous = link();
        for key in ["session_id", "conversation_id", "run_id"] {
            anonymous[key] = json!("");
        }
        assert!(!valid(&anonymous));
        let mut older = link();
        older["session_id"] = json!("");
        assert!(valid(&older), "an older parent record matches by conversation");
    }

    #[test]
    fn listings_report_the_parent_state_from_the_same_listing() {
        let machine = "11111111-1111-4111-8111-111111111111";
        let child = |session: &str| json!({"name":"codex/p/kid","parent":{"machine":machine,"session_id":session,"conversation_id":"conv"}});
        let mut rows = vec![
            json!({"name":"claude/p/lead","session_id":"22222222-2222-4222-8222-222222222222","state":"running"}),
            json!({"name":"claude/p/old","session_id":"33333333-3333-4333-8333-333333333333","state":"archived"}),
            json!({"name":"claude/p/old","session_id":"33333333-3333-4333-8333-333333333333","state":"paused"}),
            json!({"name":"claude/p/gone","session_id":"44444444-4444-4444-8444-444444444444","state":"running","process_state":"exited"}),
            child("22222222-2222-4222-8222-222222222222"),
            child("33333333-3333-4333-8333-333333333333"),
            child("44444444-4444-4444-8444-444444444444"),
            child("55555555-5555-4555-8555-555555555555"),
        ];
        let states = |rows: &[Value]| rows[4..].iter().map(|row| row["parent"]["state"].clone()).collect::<Vec<_>>();
        decorate_with(&mut rows, machine);
        assert_eq!(states(&rows), [json!("live"), json!("saved"), json!("saved"), json!("missing")]);
        decorate_with(&mut rows, "99999999-9999-4999-8999-999999999999");
        assert!(states(&rows).iter().all(|state| state == "remote"));
        decorate_with(&mut rows, "");
        assert!(states(&rows).iter().all(|state| state == "remote"), "an unknown machine never claims a local parent");
    }
}
