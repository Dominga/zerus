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
use std::{fs, io::Read};

/// The launching hgs's proven caller for a new session, as compact JSON. The
/// launcher always sets it, empty when no live tracked agent owns it, so an
/// inherited value never reaches the record.
pub(crate) const ENV: &str = "HGS_PARENT";
const USAGE: &str = "usage: hgs parent <session> --json   (JSON stdin: expected_run_id, expected_conversation_id, action read|adopt|set|clear, optional replace; set names parent {session_id, name, conversation_id, run_id})";
/// The longest parent chain walked when refusing a cycle.
const DEPTH: usize = 8;
/// The source of a link another computer forwarded with a launch: a claim
/// this computer cannot verify, never a live parent (zerus-l4ehe).
const FORWARDED_SOURCE: &str = "forwarded";

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
        && matches!(string(link, "source"), "launch" | "adopt" | "manual" | FORWARDED_SOURCE)
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

/// The largest shared catalog read to observe this computer's identity, as
/// the session action receipts bound it.
const CATALOG_BYTES: u64 = 128 * 1024 * 1024;

/// This computer's machine UUID from its existing shared catalog, read
/// without the catalog lock and never initialized or saved: an optional link
/// must never wait for, or change, Swarm state (zerus-hycp0).
fn observed_machine(config: &crate::config::Config) -> Option<String> {
    let path = config.dir.join("swarm/catalog.json");
    // A regular file only: never a final link, never waiting on a FIFO.
    let file = crate::platform::no_follow(fs::OpenOptions::new().read(true)).open(&path).ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.len() > CATALOG_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(CATALOG_BYTES + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > CATALOG_BYTES {
        return None;
    }
    crate::swarm::existing_node_id(&bytes).ok()
}

/// The proven local caller's link, for a session this process launches.
fn proven_link() -> Option<String> {
    let record = session_graph::caller_record()?;
    let config = crate::config::Config::load().ok()?;
    let machine = observed_machine(&config)?;
    link_of(&record, &machine, config.host(), "launch").map(|link| link.to_string())
}

/// A link another computer's hgs forwarded with this launch (zerus-sa9qn.7).
static FORWARDED: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// `HGS_PARENT`'s value for a session this process launches: the proven
/// local caller's link; else a link forwarded by the computer that ran the
/// launch; else empty, which keeps an inherited value from the record. The
/// caller is proven first; only a proven caller reads the identity.
pub(crate) fn launcher_value() -> String {
    proven_link().or_else(|| FORWARDED.get().cloned()).unwrap_or_default()
}

/// `--parent-link HEX` for a launch on a peer: the proven caller's link, hex
/// encoded so no shell or SSH quoting can change it. None without a caller.
pub(crate) fn forwarded_argument() -> Option<String> {
    proven_link().map(|link| link.bytes().map(|b| format!("{b:02x}")).collect())
}

/// Accepts a link the launching computer forwarded (`--parent-link HEX`): a
/// well-formed `launch` link naming another computer. It is a claim by the
/// SSH-authenticated caller that this computer cannot verify, so it is
/// recorded with the `forwarded` source: the listing never shows such a
/// parent as live, even when its IDs match a session here, and it never
/// quiets the child (zerus-l4ehe).
pub(crate) fn accept_forwarded(hex: &str) -> std::result::Result<(), String> {
    let mut link = forwarded_link(hex)?;
    let local = crate::config::Config::load().ok().and_then(|config| observed_machine(&config));
    if local.as_deref() == Some(string(&link, "machine")) {
        return Err("A link for a parent on this computer comes from this computer's own proof".into());
    }
    link["source"] = json!(FORWARDED_SOURCE);
    FORWARDED.set(link.to_string()).map_err(|_| "--parent-link may be given once".to_owned())
}

/// The well-formed `launch` link `--parent-link` carries, hex-encoded JSON.
fn forwarded_link(hex: &str) -> std::result::Result<Value, String> {
    let refuse = || "--parent-link needs a launch link from another computer".to_owned();
    // ASCII hex digits only, checked before any slicing (zerus-so6hr).
    if hex.is_empty() || hex.len() > 8192 || !hex.len().is_multiple_of(2) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(refuse());
    }
    let bytes = hex
        .as_bytes()
        .chunks(2)
        .map(|pair| std::str::from_utf8(pair).ok().and_then(|pair| u8::from_str_radix(pair, 16).ok()))
        .collect::<Option<Vec<u8>>>()
        .ok_or_else(refuse)?;
    let link: Value = serde_json::from_slice(&bytes).map_err(|_| refuse())?;
    if !valid(&link) || string(&link, "source") != "launch" {
        return Err(refuse());
    }
    Ok(link)
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
/// exited), `archived`, or `missing` when this listing has no such session
/// (gone, or on another computer). Session IDs are UUIDs, so no machine
/// identity is read: a listing never waits on Swarm state (zerus-hycp0). A
/// link another computer forwarded is unverified: always `missing`, whatever
/// sessions here its IDs match (zerus-l4ehe).
pub(crate) fn decorate(sessions: &mut [Value]) {
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
        let found = if string(link, "source") == FORWARDED_SOURCE {
            "missing"
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

/// Native questions only the person may answer: folder and hook trust.
const PERSON_ONLY_QUESTIONS: [&str; 5] = ["claude-trust:", "claude-bypass:", "codex-trust:", "kimi-trust:", "codex-hooks-trust:"];

/// Whether a live row holds a request only the person can answer (spec 2.2):
/// a tool or permission approval, folder or hook trust, or signing in. A
/// supervised child's other requests are its parent's to answer.
pub(super) fn human_request(session: &Value, questions: Option<&[Value]>) -> bool {
    // A native subagent's tool approval is the person's too (zerus-dn4ia).
    let approval = |child: &Value| {
        ["display_state", "state"].iter().any(|key| child[*key] == "approval")
    };
    string(session, "phase") == "approval"
        || session["subagents"].as_object().is_some_and(|children| children.values().any(approval))
        || session["subagent_previews"].as_array().is_some_and(|children| children.iter().any(approval))
        || session["provider_error"]["error_kind"] == "authentication"
        || string(session, "activity_summary") == "Sign in required"
        || questions.is_some_and(|questions| {
            questions.iter().any(|question| {
                let id = string(question, "question_id");
                PERSON_ONLY_QUESTIONS.iter().any(|prefix| id.starts_with(prefix))
            })
        })
}

/// The session `id` without an index entry: its own ID layout record, or the
/// record at the label a link names, each read bounded, regular, no-follow
/// and nonblocking, and only when it carries that ID.
fn unindexed(id: &str, name: &str) -> Option<Value> {
    storage::read_id(id)
        .ok()
        .or_else(|| (!name.is_empty()).then(|| storage::read_label_bounded(name, storage::ID_RECORD_BYTES).ok()).flatten())
        .filter(|record| string(record, "session_id") == id)
}

/// The parent a person chose on the desktop: a session on this computer, by
/// its durable ID, with the name, conversation and run the desktop listed.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Choice {
    session_id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    conversation_id: String,
    #[serde(default)]
    run_id: String,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    expected_run_id: String,
    expected_conversation_id: String,
    action: String,
    #[serde(default)]
    replace: bool,
    #[serde(default)]
    parent: Option<Choice>,
}

/// The session `id` on this computer: through its index entry, else its ID
/// layout record or the label `name` hints, each read bounded, regular,
/// no-follow and nonblocking. None when this computer has no such session;
/// an indexed record that cannot be read safely is an error.
fn read_session(id: &str, name: &str) -> Result<Option<Value>> {
    let unsafe_read = |_| "A session cannot be read safely; retry later".to_owned();
    let found = match session_index::indexed(id) {
        Some((label, session_index::Kind::Record)) => {
            Some(storage::read_label_bounded(&label, storage::ID_RECORD_BYTES).map_err(unsafe_read)?)
        }
        Some((_, session_index::Kind::IdRecord)) => Some(storage::read_id(id).map_err(unsafe_read)?),
        // DeepSeek and OpenCode sessions carry no parent links.
        Some(_) => None,
        None => unindexed(id, name),
    };
    match found {
        Some(record) if string(&record, "session_id") != id => Err("A session changed while it was read; retry".into()),
        found => Ok(found),
    }
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
    let request: Request = serde_json::from_slice(super::without_bom(&bytes)).map_err(|_| USAGE.to_owned())?;
    if !matches!(request.action.as_str(), "read" | "adopt" | "set" | "clear")
        || (request.action == "set") != request.parent.is_some()
    {
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
    let machine = if matches!(request.action.as_str(), "adopt" | "set") {
        observed_machine(&config).ok_or("This computer's identity cannot be read yet; retry later")?
    } else {
        String::new()
    };
    let _writer = lock(None)?;
    let mut record = read_bounded(name, storage::ID_RECORD_BYTES)?;
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
            // adopt: the proven calling agent; set: the session a person chose.
            let (chosen, source) = match (caller, request.parent.as_ref()) {
                (Some(caller), _) => (caller, "adopt"),
                (None, Some(choice)) => {
                    if !session_id::canonical(&choice.session_id) {
                        return Err("Choose the parent by its session ID".into());
                    }
                    let parent = read_session(&choice.session_id, &choice.name)?
                        .ok_or("The chosen parent is not a session on this computer")?;
                    if [(&choice.conversation_id, "conversation_id"), (&choice.run_id, "run_id")]
                        .iter()
                        .any(|(expected, key)| !expected.is_empty() && string(&parent, key) != expected.as_str())
                    {
                        return Err("The chosen parent changed; refresh and retry".into());
                    }
                    if !string(&parent, "archive_id").is_empty() || parent["state"] == "archived" {
                        return Err("An archived session cannot be a parent".into());
                    }
                    (parent, "manual")
                }
                _ => return Err(USAGE.into()),
            };
            let target = string(&record, "session_id");
            if string(&chosen, "session_id") == target && !target.is_empty()
                || string(&chosen, "run_id") == string(&record, "run_id")
            {
                return Err(if source == "adopt" { "A session cannot adopt itself" } else { "A session cannot be its own parent" }.into());
            }
            if ancestors(&chosen)?.contains(&target.to_owned()) {
                return Err("This session already leads the chosen parent; the link would make a cycle".into());
            }
            if let Some(old) = previous.as_ref() {
                let same = string(old, "machine") == machine
                    && string(old, "session_id") == string(&chosen, "session_id");
                if !same && !request.replace {
                    return Err(format!(
                        "{name} already has the parent {}; pass \"replace\": true to change it",
                        string(old, "name")
                    ));
                }
            }
            let link = link_of(&chosen, &machine, config.host(), source)
                .ok_or("The parent session has no complete identity yet; retry after it starts")?;
            record["parent"] = link;
            write(&mut record)?;
        }
    }
    println!("{}", json!({"ok": true, "name": name, "parent": listed(&record), "previous": previous}));
    Ok(0)
}

/// The session IDs of `record`'s parents on this computer, nearest first,
/// through the session ID index. Each record is read bounded, a regular file
/// only, never following a final link or waiting on a FIFO (zerus-mzgfh). A
/// chain that ends at a session this computer no longer lists, or on another
/// computer, is complete. A chain longer than [`DEPTH`], or an indexed record
/// that cannot be read, proves nothing, so adoption refuses (zerus-lwz9t).
fn ancestors(record: &Value) -> Result<Vec<String>> {
    let mut found = Vec::new();
    let mut current = record.clone();
    loop {
        // A forwarded parent is on another computer: the chain here ends.
        let Some(link) = listed(&current).filter(|link| string(link, "source") != FORWARDED_SOURCE) else {
            return Ok(found);
        };
        let id = string(&link, "session_id").to_owned();
        if id.is_empty() || found.contains(&id) {
            return Ok(found);
        }
        if found.len() == DEPTH {
            return Err(format!("The caller's parents reach deeper than {DEPTH} sessions; adoption cannot rule out a cycle"));
        }
        found.push(id.clone());
        let unsafe_read = |_| "A parent of the caller cannot be read safely; adoption cannot rule out a cycle".to_owned();
        let next = match session_index::indexed(&id) {
            Some((name, session_index::Kind::Record)) => {
                storage::read_label_bounded(&name, storage::ID_RECORD_BYTES).map_err(unsafe_read)?
            }
            Some((_, session_index::Kind::IdRecord)) => storage::read_id(&id).map_err(unsafe_read)?,
            // DeepSeek and OpenCode sessions carry no parent links.
            Some(_) => return Ok(found),
            // The index is a cache: without an entry, look where the session
            // would be, its ID layout record or the label the link names, the
            // same bounded way (zerus-lwz9t). Neither there: the chain ends.
            None => match unindexed(&id, string(&link, "name")) {
                Some(next) => next,
                None => return Ok(found),
            },
        };
        if string(&next, "session_id") != id {
            return Err("A parent of the caller changed while it was read; retry".into());
        }
        current = next;
    }
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
    fn only_approvals_trust_and_sign_in_are_the_persons_requests() {
        let row = |phase: &str| json!({"phase": phase});
        assert!(human_request(&row("approval"), None));
        assert!(!human_request(&row("input"), None));
        assert!(!human_request(&row("idle"), Some(&[])));
        let asked = |id: &str| vec![json!({"question_id": id})];
        for id in ["claude-trust:abc", "codex-trust:abc", "kimi-trust:abc", "codex-hooks-trust:abc"] {
            assert!(human_request(&row("input"), Some(&asked(id))), "{id}");
        }
        assert!(!human_request(&row("input"), Some(&asked("toolu_ask_user"))));
        assert!(!human_request(&row("input"), Some(&asked("x-claude-trust:abc"))));
        assert!(human_request(&json!({"phase":"error","provider_error":{"error_kind":"authentication"}}), None));
        assert!(!human_request(&json!({"phase":"error","provider_error":{"error_kind":"quota"}}), None));
        assert!(human_request(&json!({"phase":"input","activity_summary":"Sign in required"}), None));
        let child = |state: &str| json!({"phase":"tool","subagents":{"kid":{"state":"working","display_state":state}}});
        assert!(human_request(&child("approval"), None), "a native subagent's approval");
        assert!(!human_request(&child("input"), None));
        assert!(human_request(&json!({"phase":"tool","subagent_previews":[{"state":"approval"}]}), None));
    }

    #[test]
    fn a_forwarded_link_must_be_a_well_formed_launch_link() {
        let hex = |value: &Value| value.to_string().bytes().map(|b| format!("{b:02x}")).collect::<String>();
        for bad in ["", "zz", "abc", &hex(&json!({"schema": 2}))] {
            assert!(accept_forwarded(bad).is_err(), "{bad}");
        }
        let mut adopted = link();
        adopted["source"] = json!("adopt");
        assert!(accept_forwarded(&hex(&adopted)).is_err(), "only a launch is forwarded");
        let mut forwarded = link();
        forwarded["source"] = json!(FORWARDED_SOURCE);
        assert!(accept_forwarded(&hex(&forwarded)).is_err(), "a forwarded link is not forwarded again");
        // Non-ASCII input is refused, never sliced inside a character (zerus-so6hr).
        for bad in ["\u{20ac}x", "a\u{20ac}", "\u{e9}\u{e9}", "0g"] {
            assert!(forwarded_link(bad).is_err(), "{bad}");
        }
        assert_eq!(forwarded_link(&hex(&link())).unwrap(), link());
    }

    #[test]
    fn a_forwarded_parent_is_never_live_even_when_its_ids_match_here() {
        // zerus-l4ehe: another computer's claim naming a live session here.
        let live = "22222222-2222-4222-8222-222222222222";
        let mut claim = link();
        claim["session_id"] = json!(live);
        claim["conversation_id"] = json!("conv-live");
        claim["source"] = json!(FORWARDED_SOURCE);
        assert!(valid(&claim));
        let mut rows = vec![
            json!({"name":"claude/p/lead","session_id":live,"conversation_id":"conv-live","state":"running"}),
            json!({"name":"codex/p/kid","parent":claim}),
        ];
        decorate(&mut rows);
        assert_eq!(rows[1]["parent"]["state"], "missing");
        // The same IDs from a local proof match.
        rows[1]["parent"]["source"] = json!("launch");
        decorate(&mut rows);
        assert_eq!(rows[1]["parent"]["state"], "live");
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
        decorate(&mut rows);
        assert_eq!(states(&rows), [json!("live"), json!("saved"), json!("saved"), json!("missing")]);
    }
}
