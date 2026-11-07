//! Keep cache warm: a per-session setting kept on the agent's computer, and the
//! recovery worker's round that keeps the provider prompt cache warm by sending
//! the idle agent one short marked message before the cache would expire.
//! See docs/superpowers/specs/2026-11-07-keep-cache-warm-design.md.
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;

/// The exact ping. Its journal marks and silent presentation depend on it.
pub(super) const TEXT: &str = "[Zerus keep-warm] Reply with exactly OK and do nothing else.";
/// Enabled sessions per computer: bounds every read of the store and the round.
const MAX_ENABLED: usize = 64;
/// Disabled entries kept for revision checks before the oldest are dropped.
const MAX_ENTRIES: usize = 512;
const MAX_FILE: u64 = 1024 * 1024;
const MAX_REVISION: u64 = i64::MAX as u64;
const USAGE: &str = "usage: hgs keep-warm <session> --json (JSON stdin: expected_run_id, expected_conversation_id, expected_revision, enabled, hours 1-7 or always)";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(super) struct Entry {
    name: String,
    enabled: bool,
    /// Seconds since the epoch on this computer's clock; None is Always.
    until: Option<f64>,
    revision: u64,
    set_at: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Store {
    version: u8,
    sessions: BTreeMap<String, Entry>,
}

impl Default for Store {
    fn default() -> Self {
        Self { version: 1, sessions: BTreeMap::new() }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    expected_run_id: String,
    expected_conversation_id: String,
    expected_revision: u64,
    enabled: bool,
    #[serde(default)]
    hours: Option<u8>,
    #[serde(default)]
    always: bool,
}

fn identity(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}

fn parse(bytes: &[u8]) -> Result<Request> {
    let request: Request = serde_json::from_slice(bytes).map_err(|_| "invalid keep-warm request")?;
    let duration = match (request.hours, request.always) {
        (None, false) => !request.enabled,
        (Some(hours), false) => request.enabled && (1..=7).contains(&hours),
        (None, true) => request.enabled,
        (Some(_), true) => false,
    };
    if !identity(&request.expected_run_id)
        || !identity(&request.expected_conversation_id)
        || request.expected_revision > MAX_REVISION
        || !duration
    {
        return Err("invalid keep-warm request".into());
    }
    Ok(request)
}

fn path() -> PathBuf {
    root().join("keep-warm.json")
}

fn lock_path() -> PathBuf {
    root().join("keep-warm.lock")
}

/// The worker's state of one session's keep-warm, written only by the worker.
fn runtime_path(owner: &str) -> PathBuf {
    root().join("recovery/keep-warm").join(format!("{owner}.json"))
}

fn read_store(path: &Path) -> Result<Store> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Store::default()),
        Err(error) => return Err(error.to_string()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_FILE + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_FILE {
        return Err("keep-warm settings exceed limits".into());
    }
    let store: Store = serde_json::from_slice(&bytes).map_err(|e| format!("invalid keep-warm settings: {e}"))?;
    if store.version != 1
        || store.sessions.len() > MAX_ENTRIES + 1
        || store.sessions.values().filter(|entry| entry.enabled).count() > MAX_ENABLED
        || store.sessions.iter().any(|(owner, entry)| {
            !identity(owner) || entry.name.len() > 2048 || entry.revision > MAX_REVISION
                || entry.until.is_some_and(|until| !until.is_finite())
        })
    {
        return Err("invalid keep-warm settings".into());
    }
    Ok(store)
}

fn persist(path: &Path, store: &Store) -> Result<()> {
    let serialized = serde_json::to_string(store).map_err(|e| e.to_string())?;
    if serialized.len() as u64 >= MAX_FILE {
        return Err("keep-warm settings exceed limits".into());
    }
    atomic(path, &format!("{serialized}\n"))
}

/// The store, or None when it is unreadable: unknown, never a default "off".
pub(super) fn load() -> Option<Store> {
    read_store(&path()).ok()
}

/// The `keep_warm` object of a listing row.
fn view(entry: Option<&Entry>, runtime: &Value, record: &Value) -> Value {
    let enabled = entry.is_some_and(|entry| entry.enabled);
    let state = if !enabled {
        "off"
    } else {
        runtime["state"].as_str().filter(|state| !state.is_empty()).unwrap_or("waiting")
    };
    json!({"enabled":enabled,"until":entry.and_then(|entry| entry.until),
        "revision":entry.map_or(0, |entry| entry.revision),"state":state,
        "next_at":if enabled { runtime["next_at"].clone() } else { Value::Null },
        "reason":if enabled { runtime["reason"].as_str().unwrap_or("") } else { "" },
        "turn_active":record["keep_warm_turn"].is_object()})
}

fn read_runtime(owner: &str) -> Value {
    fs::File::open(runtime_path(owner)).ok().and_then(|file| {
        let mut bytes = Vec::new();
        file.take(64 * 1024).read_to_end(&mut bytes).ok()?;
        serde_json::from_slice(&bytes).ok()
    }).unwrap_or(Value::Null)
}

/// Add `keep_warm` to one listing or inspect row. Only enabled sessions read
/// the worker's state, so a listing does at most 64 extra small reads.
pub(super) fn enrich(session: &mut Value, store: Option<&Store>, record: &Value) {
    let Some(store) = store else {
        session["keep_warm"] = Value::Null;
        return;
    };
    let owner = storage::owner_key(record);
    let entry = store.sessions.get(&owner);
    let runtime = if entry.is_some_and(|entry| entry.enabled) { read_runtime(&owner) } else { Value::Null };
    session["keep_warm"] = view(entry, &runtime, record);
}

fn entry(store: &Store, owner: &str) -> Option<Entry> {
    store.sessions.get(owner).cloned()
}

/// Pure compare-and-set reducer. `eligible` is asked only when enabling.
fn update(
    store: &mut Store,
    owner: &str,
    record: &Value,
    request: &Request,
    now: f64,
    eligible: impl FnOnce() -> std::result::Result<(), String>,
) -> (i32, Value, bool) {
    let run = string(record, "run_id");
    let conversation = string(record, "conversation_id");
    let previous = entry(store, owner);
    let revision = previous.as_ref().map_or(0, |entry| entry.revision);
    let refusal = |error: &str, reason: &str| {
        let mut output = json!({"ok":false,"error":error,"run_id":run,"conversation_id":conversation,
            "keep_warm":previous});
        if !reason.is_empty() {
            output["reason"] = json!(reason);
        }
        (1, output, false)
    };
    if request.expected_run_id != run || request.expected_conversation_id != conversation {
        return refusal("stale_identity", "");
    }
    if request.expected_revision != revision {
        return refusal("conflict", "");
    }
    let was_enabled = previous.as_ref().is_some_and(|entry| entry.enabled);
    if !request.enabled && !was_enabled {
        return (0, json!({"ok":true,"applied":false,"run_id":run,"conversation_id":conversation,
            "keep_warm":previous}), false);
    }
    if revision == MAX_REVISION {
        return refusal("revision_exhausted", "");
    }
    if request.enabled {
        if !was_enabled && store.sessions.values().filter(|entry| entry.enabled).count() >= MAX_ENABLED {
            return refusal("capacity", "");
        }
        if let Err(reason) = eligible() {
            return refusal("unsupported", &reason);
        }
    }
    let next = Entry {
        name: string(record, "name").into(),
        enabled: request.enabled,
        until: match request.hours {
            Some(hours) if request.enabled => Some(now + f64::from(hours) * 3600.0),
            _ => None,
        },
        revision: revision + 1,
        set_at: now,
    };
    store.sessions.insert(owner.into(), next.clone());
    prune(store);
    (0, json!({"ok":true,"applied":true,"run_id":run,"conversation_id":conversation,"keep_warm":next}), true)
}

/// Why this session cannot keep its cache warm now, if it cannot.
fn eligible(record: &Value) -> std::result::Result<(), String> {
    match string(record, "agent") {
        "claude" | "codex" | "kimi" => Ok(()),
        _ => Err("This agent does not report a prompt cache that pings can keep warm.".into()),
    }
}

pub(super) fn dispatch(args: &[String]) -> Result<i32> {
    let [name, flag] = args else {
        return Err(USAGE.into());
    };
    if flag != "--json" || name.is_empty() || name.starts_with(['-', '@']) {
        return Err(USAGE.into());
    }
    let mut bytes = Vec::new();
    io::stdin().take(4097).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() > 4096 {
        return Err("keep-warm request exceeds limits".into());
    }
    let request = parse(&bytes)?;
    // zerus-ucck P3c: the request's identities select among sessions that
    // share the label.
    expect(name, &request.expected_run_id, &request.expected_conversation_id)?;
    let Some(record) = notifications::current(name)? else {
        println!("{}", json!({"ok":false,"error":"not_found"}));
        return Ok(1);
    };
    let owner = storage::owner_key(&record);
    let (code, mut output, changed, entry) = {
        let _guard = lock(Some(&lock_path()))?;
        let mut store = read_store(&path())?;
        let (code, output, changed) = update(&mut store, &owner, &record, &request, now(), || eligible(&record));
        if changed {
            persist(&path(), &store)?;
        }
        (code, output, changed, store.sessions.get(&owner).cloned())
    };
    if changed && request.enabled {
        recovery::ensure_worker()?;
    }
    if code == 0 {
        output["keep_warm"] = view(entry.as_ref(), &read_runtime(&owner), &record);
    }
    println!("{output}");
    Ok(code)
}

/// Drop the oldest disabled entries beyond the bound; enabled ones stay.
fn prune(store: &mut Store) {
    let excess = store.sessions.len().saturating_sub(MAX_ENTRIES);
    if excess == 0 {
        return;
    }
    let mut disabled: Vec<(f64, String)> = store.sessions.iter()
        .filter(|(_, entry)| !entry.enabled)
        .map(|(owner, entry)| (entry.set_at, owner.clone()))
        .collect();
    disabled.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, owner) in disabled.into_iter().take(excess) {
        store.sessions.remove(&owner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: f64 = 1_794_000_000.0;

    fn record() -> Value {
        json!({"name":"claude/project/warm","run_id":"run-1","conversation_id":"conversation-1"})
    }

    fn request(body: Value) -> Request {
        let mut base = json!({"expected_run_id":"run-1","expected_conversation_id":"conversation-1",
            "expected_revision":0,"enabled":true,"hours":3});
        for (key, value) in body.as_object().unwrap() {
            if value.is_null() {
                base.as_object_mut().unwrap().remove(key);
            } else {
                base[key] = value.clone();
            }
        }
        parse(base.to_string().as_bytes()).unwrap()
    }

    fn ok() -> std::result::Result<(), String> {
        Ok(())
    }

    #[test]
    fn enabling_sets_a_deadline_on_this_computers_clock() {
        let mut store = Store::default();
        let (code, output, changed) = update(&mut store, "owner", &record(), &request(json!({})), NOW, ok);
        assert_eq!((code, changed), (0, true));
        assert_eq!(output["keep_warm"]["until"], json!(NOW + 3.0 * 3600.0));
        assert_eq!(output["keep_warm"]["revision"], 1);
        assert_eq!(output["keep_warm"]["enabled"], true);
        let always = request(json!({"hours":null,"always":true,"expected_revision":1}));
        let (_, output, _) = update(&mut store, "owner", &record(), &always, NOW + 5.0, ok);
        assert_eq!(output["keep_warm"]["until"], Value::Null);
        assert_eq!(output["keep_warm"]["revision"], 2);
    }

    #[test]
    fn disabling_keeps_the_revision_and_repeating_it_changes_nothing() {
        let mut store = Store::default();
        update(&mut store, "owner", &record(), &request(json!({})), NOW, ok);
        let off = request(json!({"enabled":false,"hours":null,"expected_revision":1}));
        let (code, output, changed) = update(&mut store, "owner", &record(), &off, NOW, ok);
        assert_eq!((code, changed), (0, true));
        assert_eq!((output["keep_warm"]["enabled"].clone(), output["keep_warm"]["revision"].clone()), (json!(false), json!(2)));
        let again = request(json!({"enabled":false,"hours":null,"expected_revision":2}));
        let (code, output, changed) = update(&mut store, "owner", &record(), &again, NOW, ok);
        assert_eq!((code, changed, output["applied"].clone()), (0, false, json!(false)));
    }

    #[test]
    fn stale_identity_conflict_and_ineligible_agents_are_refused() {
        let mut store = Store::default();
        let stale = request(json!({"expected_run_id":"run-0"}));
        assert_eq!(update(&mut store, "owner", &record(), &stale, NOW, ok).1["error"], "stale_identity");
        let conflict = request(json!({"expected_revision":4}));
        assert_eq!(update(&mut store, "owner", &record(), &conflict, NOW, ok).1["error"], "conflict");
        let (code, output, changed) = update(&mut store, "owner", &record(), &request(json!({})), NOW,
            || Err("DeepSeek keeps its context cache for hours without pings.".into()));
        assert_eq!((code, changed, output["error"].clone()), (1, false, json!("unsupported")));
        assert_eq!(output["reason"], "DeepSeek keeps its context cache for hours without pings.");
        assert!(store.sessions.is_empty());
    }

    #[test]
    fn at_most_sixty_four_sessions_keep_their_cache_warm() {
        let mut store = Store::default();
        for index in 0..MAX_ENABLED {
            update(&mut store, &format!("owner-{index}"), &record(), &request(json!({})), NOW, ok);
        }
        let (code, output, _) = update(&mut store, "owner-new", &record(), &request(json!({})), NOW, ok);
        assert_eq!((code, output["error"].clone()), (1, json!("capacity")));
        // Changing the duration of an enabled session is not a new one.
        let longer = request(json!({"hours":7,"expected_revision":1}));
        assert_eq!(update(&mut store, "owner-0", &record(), &longer, NOW, ok).0, 0);
    }

    #[test]
    fn the_listing_shows_the_setting_worker_state_and_a_running_ping() {
        let mut store = Store::default();
        update(&mut store, "owner", &record(), &request(json!({})), NOW, ok);
        let entry = store.sessions.get("owner");
        let runtime = json!({"state":"waiting","next_at":NOW + 2880.0,"reason":""});
        let mut current = record();
        assert_eq!(view(entry, &runtime, &current), json!({"enabled":true,"until":NOW + 10800.0,"revision":1,
            "state":"waiting","next_at":NOW + 2880.0,"reason":"","turn_active":false}));
        current["keep_warm_turn"] = json!({"request_id":"ping"});
        assert_eq!(view(entry, &runtime, &current)["turn_active"], true);
        // No setting, or a disabled one, is simply off; worker state is not shown.
        assert_eq!(view(None, &Value::Null, &record()), json!({"enabled":false,"until":null,"revision":0,
            "state":"off","next_at":null,"reason":"","turn_active":false}));
        let off = Entry { enabled: false, ..entry.unwrap().clone() };
        assert_eq!(view(Some(&off), &runtime, &record())["state"], "off");
        // A new setting has no worker state yet.
        assert_eq!(view(entry, &Value::Null, &record())["state"], "waiting");
    }

    #[test]
    fn corrupt_or_oversized_stores_are_unknown_not_off() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("keep-warm.json");
        assert!(read_store(&path).unwrap().sessions.is_empty());
        std::fs::write(&path, "{\"version\":1,\"sessions\":{}}").unwrap();
        assert!(read_store(&path).is_ok());
        for invalid in ["{", "{\"version\":2,\"sessions\":{}}", "{\"version\":1,\"sessions\":{},\"x\":1}"] {
            std::fs::write(&path, invalid).unwrap();
            assert!(read_store(&path).is_err(), "{invalid}");
        }
        std::fs::write(&path, " ".repeat(MAX_FILE as usize + 1)).unwrap();
        assert!(read_store(&path).is_err());
        let mut store = Store::default();
        update(&mut store, "owner", &record(), &request(json!({})), NOW, ok);
        persist(&path, &store).unwrap();
        assert_eq!(read_store(&path).unwrap().sessions, store.sessions);
    }

    #[test]
    fn requests_need_exactly_one_valid_duration_when_enabling() {
        let parsed = |body: Value| parse(body.to_string().as_bytes());
        let base = |extra: Value| {
            let mut value = json!({"expected_run_id":"run-1","expected_conversation_id":"conversation-1",
                "expected_revision":0,"enabled":true});
            for (key, v) in extra.as_object().unwrap() { value[key] = v.clone(); }
            value
        };
        for invalid in [json!({}), json!({"hours":0}), json!({"hours":8}), json!({"hours":3,"always":true}),
            json!({"enabled":false,"hours":3}), json!({"enabled":false,"always":true}), json!({"hours":3,"extra":1})] {
            assert!(parsed(base(invalid.clone())).is_err(), "{invalid}");
        }
        for valid in [json!({"hours":1}), json!({"hours":7}), json!({"always":true}), json!({"enabled":false})] {
            assert!(parsed(base(valid.clone())).is_ok(), "{valid}");
        }
    }
}
