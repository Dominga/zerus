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

/// Caches shorter than this are not kept warm: pings every few minutes cost
/// about as much as rebuilding the cache and fill the conversation.
const MIN_TTL: u64 = 15 * 60;
/// Pings land at 80% of the lifetime, ahead of Claude Code's idle compaction
/// at 90% and of worker scheduling delay.
const INTERVAL_PERCENT: u64 = 80;
/// Kimi Code's own `estimated_cache_duration` values, fetched 2026-11-07.
const KIMI_TABLE: &[(&str, u64)] = &[
    ("kimi-for-coding", 3600), ("kimi-for-coding-highspeed", 3600), ("k3", 1800),
    ("k3-256k", 1800), ("k3.1-pro", 1800), ("k3.1-pro-highspeed", 1800),
];

/// Whether and how this session's cache can be kept warm, from what the agent
/// reported. `expires_at` is the estimated end of the current cache lifetime.
#[derive(Debug, PartialEq)]
pub(super) struct Eligibility {
    pub supported: bool,
    pub reason_code: &'static str,
    pub reason: &'static str,
    pub ttl: Option<u64>,
    pub source: &'static str,
    pub expires_at: Option<f64>,
}

impl Eligibility {
    fn refused(reason_code: &'static str, reason: &'static str) -> Self {
        Self { supported: false, reason_code, reason, ttl: None, source: "", expires_at: None }
    }

    fn waiting(reason: &'static str) -> Self {
        Self { supported: true, reason_code: "no_cache_report", reason, ttl: None, source: "", expires_at: None }
    }

    fn known(ttl: u64, source: &'static str, expires_at: Option<f64>) -> Self {
        Self { supported: true, reason_code: "", reason: "", ttl: Some(ttl), source, expires_at }
    }

    pub fn interval(&self) -> Option<u64> {
        self.ttl.map(|ttl| ttl * INTERVAL_PERCENT / 100)
    }

    /// When the next ping is due: the start of the current lifetime plus the interval.
    pub fn due_at(&self) -> Option<f64> {
        let (ttl, interval) = (self.ttl? as f64, self.interval()? as f64);
        self.expires_at.map(|expires| expires - ttl + interval)
    }
}

/// A Codex model with OpenAI's documented 30-minute default: GPT-5.6 and later.
fn codex_thirty_minutes(model: &str) -> bool {
    let Some(version) = model.strip_prefix("gpt-") else { return false };
    let version: String = version.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    let mut parts = version.split('.').map(|part| part.parse::<u32>().ok());
    match (parts.next().flatten(), parts.next().flatten()) {
        (Some(major), _) if major > 5 => true,
        (Some(5), Some(minor)) => minor >= 6,
        _ => false,
    }
}

/// Kimi Code's newest fetched `estimated_cache_duration` for `model`, read
/// from the session's Kimi home: bounded, read only, never required.
fn kimi_config(home: &Path, model: &str) -> Option<u64> {
    let directory = home.join("cache/client-configs");
    let mut newest: Option<(f64, Option<u64>)> = None;
    for entry in fs::read_dir(directory).ok()?.flatten().take(64) {
        let name = entry.file_name();
        if !name.to_string_lossy().ends_with("estimated_cache_duration.json") {
            continue;
        }
        let mut bytes = Vec::new();
        let Ok(file) = fs::File::open(entry.path()) else { continue };
        if file.take(64 * 1024 + 1).read_to_end(&mut bytes).is_err() || bytes.len() > 64 * 1024 {
            continue;
        }
        let Ok(value) = serde_json::from_slice::<Value>(&bytes) else { continue };
        let fetched = value["fetchedAt"].as_f64().unwrap_or(0.0);
        let rules = &value["config"]["config"];
        if !rules.is_object() || newest.is_some_and(|(at, _)| at >= fetched) {
            continue;
        }
        newest = Some((fetched, rules[model]["cache_duration"].as_u64().filter(|ttl| (1..=86_400).contains(ttl))));
    }
    newest.and_then(|(_, ttl)| ttl)
}

/// `last_turn_at` is when the last main-agent turn (or ping) ended, the start
/// of the current cache lifetime for agents whose TTL is not observed.
pub(super) fn eligibility(record: &Value, usage: &Value, last_turn_at: Option<f64>) -> Eligibility {
    let from_turn = |ttl: u64, source| Eligibility::known(ttl, source, last_turn_at.map(|at| at + ttl as f64));
    match string(record, "agent") {
        "claude" => {
            let cache = &usage["prompt_cache"];
            match cache["ttl_seconds"].as_u64() {
                None => Eligibility::waiting("Waiting for Claude to report its cache lifetime with its next reply."),
                Some(ttl) if ttl < MIN_TTL => Eligibility::refused("claude_short_ttl",
                    "Claude uses a 5-minute cache on this account. Keeping it warm would cost about as much as rebuilding it."),
                Some(ttl) => Eligibility::known(ttl, "native_usage", cache["expires_at"].as_f64()),
            }
        }
        "codex" => match input::codex_footer_model(record) {
            None => Eligibility::waiting("Waiting for Codex to report its model."),
            Some(model) if codex_thirty_minutes(&model.to_ascii_lowercase()) => from_turn(1800, "provider_default"),
            Some(_) => Eligibility::refused("codex_model",
                "OpenAI keeps a 30-minute cache only for GPT-5.6 and later. This model's cache lasts about 5 to 10 minutes."),
        },
        "kimi" => {
            let model = string(record, "model");
            let model = model.rsplit('/').next().unwrap_or(model).to_ascii_lowercase();
            if model.is_empty() {
                return Eligibility::waiting("Waiting for Kimi to report its model.");
            }
            let configured = (!string(record, "agent_home").is_empty())
                .then(|| kimi_config(Path::new(string(record, "agent_home")), &model)).flatten()
                .or_else(|| kimi_config(&home().join(".kimi-code"), &model).filter(|_| string(record, "agent_home").is_empty()));
            match (configured, KIMI_TABLE.iter().find(|(name, _)| *name == model)) {
                (Some(ttl), _) if ttl >= MIN_TTL => from_turn(ttl, "kimi_config"),
                (None, Some((_, ttl))) => from_turn(*ttl, "kimi_table"),
                _ => Eligibility::refused("kimi_model", "Kimi reports no cache lifetime of at least 15 minutes for this model."),
            }
        }
        "dsh" => Eligibility::refused("deepseek", "DeepSeek keeps its context cache for hours without pings."),
        _ => Eligibility::refused("agent", "This agent reports no prompt cache that pings can keep warm."),
    }
}

/// Work around the agent that its record alone does not show.
#[derive(Debug)]
pub(super) struct Facts {
    /// Live owned shell or native jobs (desktop Background); None is unknown.
    pub background: Option<u64>,
    /// Jobs the agent scheduled for itself.
    pub schedules: u64,
    /// An automatic recovery episode owns this session's next input.
    pub recovery_active: bool,
}

impl Facts {
    pub fn observe(record: &Value) -> Self {
        let live = process_alive(record);
        let background = processes::WorkSample::default().summary(record, live)["count"].as_u64();
        let schedules = if string(record, "agent") == "kimi" {
            // Unknown schedules count as present: never ping into a job's turn.
            goals::summary(record)["schedules"]["count"].as_u64().unwrap_or(1)
        } else {
            0
        };
        Self { background, schedules, recovery_active: recovery::job_active(record) }
    }
}

/// Why ping `id` must not be typed now, if it must not. The guarded submit
/// asks this under the input lock, once before staging and again immediately
/// before the first input byte.
fn submission_refusal(record: &Value, entry: Option<&Entry>, runtime: &Value, facts: &Facts, id: &str, now: f64) -> Option<&'static str> {
    let Some(entry) = entry.filter(|entry| entry.enabled) else {
        return Some("Keep cache warm was turned off; nothing was submitted");
    };
    if entry.until.is_some_and(|until| until <= now) {
        return Some("Keep cache warm has expired; nothing was submitted");
    }
    if runtime["state"] != "dispatching" || runtime["request_id"] != id {
        return Some("This keep-warm ping is no longer current; nothing was submitted");
    }
    if !telemetry::settled(record) || record["keep_warm_turn"].is_object() || record["paused"] == true
        || ["pending", "attempted"].contains(&string(&record["launch_prompt"], "status"))
    {
        return Some("The agent is not idle; nothing was submitted");
    }
    if facts.background != Some(0) {
        return Some("Background work is running or unknown; nothing was submitted");
    }
    if facts.schedules > 0 {
        return Some("The agent has scheduled jobs; nothing was submitted");
    }
    if facts.recovery_active {
        return Some("Automatic recovery owns the next message; nothing was submitted");
    }
    None
}

/// The guarded submit's check for ping `id` (`input::submit_keep_warm`).
pub(super) fn check_submission(record: &Value, id: &str) -> Result<()> {
    let owner = storage::owner_key(record);
    let store = load().ok_or("Keep cache warm settings are unreadable; nothing was submitted")?;
    let facts = Facts::observe(record);
    match submission_refusal(record, store.sessions.get(&owner), &read_runtime(&owner), &facts, id, now()) {
        Some(reason) => Err(reason.into()),
        None => Ok(()),
    }
}

/// When the last main-agent turn or ping ended, the start of the current
/// cache lifetime for agents whose TTL is not observed per request.
pub(super) fn last_turn_at(record: &Value) -> Option<f64> {
    ["cache_touched_at", "recovery_success_at"].iter()
        .find_map(|key| record[*key].as_f64().filter(|at| at.is_finite() && *at > 0.0))
}

/// Add the derived fields of the Details read to `output["keep_warm"]`.
pub(super) fn describe(output: &mut Value, record: &Value, usage: &Value) {
    if !output["keep_warm"].is_object() {
        return;
    }
    let rule = eligibility(record, usage, last_turn_at(record));
    let warm = &mut output["keep_warm"];
    warm["supported"] = json!(rule.supported);
    warm["reason_code"] = json!(rule.reason_code);
    warm["reason"] = json!(if rule.reason.is_empty() { string(warm, "reason") } else { rule.reason });
    warm["ttl_seconds"] = json!(rule.ttl);
    warm["ttl_source"] = json!(rule.source);
    warm["interval_seconds"] = json!(rule.interval());
    warm["cache_expires_at"] = json!(rule.expires_at);
}

/// Why this session cannot keep its cache warm now, if it cannot.
fn eligible(record: &Value) -> std::result::Result<(), String> {
    let usage = if string(record, "agent") == "claude" { usage::read(record) } else { Value::Null };
    let rule = eligibility(record, &usage, None);
    if rule.supported { Ok(()) } else { Err(rule.reason.into()) }
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
        let usage = if string(&record, "agent") == "claude" { usage::read(&record) } else { Value::Null };
        describe(&mut output, &record, &usage);
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

    fn claude(ttl: Value, expires: Value) -> (Value, Value) {
        (json!({"agent":"claude","model":"claude-opus-5-5"}),
         json!({"prompt_cache":{"status":"warm","ttl_seconds":ttl,"expires_at":expires}}))
    }

    #[test]
    fn claude_uses_the_ttl_its_last_request_wrote() {
        let (record, usage) = claude(json!(3600), json!(NOW + 3000.0));
        let rule = eligibility(&record, &usage, None);
        assert_eq!((rule.supported, rule.ttl, rule.source), (true, Some(3600), "native_usage"));
        assert_eq!(rule.interval(), Some(2880));
        // The last request was at NOW - 600; the ping is due 48 minutes after it.
        assert_eq!(rule.due_at(), Some(NOW - 600.0 + 2880.0));
        let (record, usage) = claude(json!(300), json!(NOW + 200.0));
        let short = eligibility(&record, &usage, None);
        assert_eq!((short.supported, short.reason_code), (false, "claude_short_ttl"));
        assert!(short.reason.contains("5-minute"), "{}", short.reason);
        // Before the first reply reports a cache, keep-warm can be on but waits.
        let waiting = eligibility(&json!({"agent":"claude"}), &json!({}), None);
        assert_eq!((waiting.supported, waiting.reason_code, waiting.due_at()), (true, "no_cache_report", None));
    }

    #[test]
    fn codex_keeps_thirty_minutes_only_on_models_with_that_documented_cache() {
        for model in ["gpt-5.6", "gpt-5.6-codex", "gpt-6.1-sol", "gpt-7"] {
            let rule = eligibility(&json!({"agent":"codex","model":model}), &json!({}), Some(NOW));
            assert_eq!((rule.supported, rule.ttl, rule.source), (true, Some(1800), "provider_default"), "{model}");
            assert_eq!(rule.due_at(), Some(NOW + 1440.0));
        }
        for model in ["gpt-5.5", "gpt-5", "o3", "codex-mini-latest", "gpt-4.1"] {
            let rule = eligibility(&json!({"agent":"codex","model":model}), &json!({}), Some(NOW));
            assert_eq!((rule.supported, rule.reason_code), (false, "codex_model"), "{model}");
        }
        // The launch argument names the model before Codex reports it.
        let launched = json!({"agent":"codex","base":["codex","-m","gpt-6.1-sol"]});
        assert!(eligibility(&launched, &json!({}), Some(NOW)).supported);
        let unknown = eligibility(&json!({"agent":"codex"}), &json!({}), Some(NOW));
        assert_eq!((unknown.supported, unknown.reason_code), (true, "no_cache_report"));
        // No finished turn yet: nothing to keep warm.
        assert_eq!(eligibility(&json!({"agent":"codex","model":"gpt-6.1-sol"}), &json!({}), None).due_at(), None);
    }

    #[test]
    fn kimi_follows_its_own_cache_duration_estimates() {
        // An empty Kimi home: the built-in table decides.
        let empty = tempfile::tempdir().unwrap();
        let bare = empty.path().to_str().unwrap().to_owned();
        let rule = |model: &str| eligibility(&json!({"agent":"kimi","model":model,"agent_home":bare}), &json!({}), Some(NOW));
        assert_eq!(rule("kimi-for-coding").ttl, Some(3600));
        assert_eq!(rule("kimi-for-coding-highspeed").ttl, Some(3600));
        assert_eq!(rule("k3").ttl, Some(1800));
        assert_eq!(rule("k3.1-pro").ttl, Some(1800));
        assert_eq!(rule("k3").source, "kimi_table");
        assert_eq!((rule("moonshot-v1").supported, rule("moonshot-v1").reason_code), (false, "kimi_model"));
        let dir = tempfile::tempdir().unwrap();
        let configs = dir.path().join("cache/client-configs");
        std::fs::create_dir_all(&configs).unwrap();
        std::fs::write(configs.join("estimated_cache_duration.json"), json!({"version":1,"fetchedAt":1,
            "config":{"version":1,"config":{"k3":{"min_tokens_to_hint":1,"cache_duration":1200}}}}).to_string()).unwrap();
        std::fs::write(configs.join("mainland-cn%3A%2Fclient_configs%3Aestimated_cache_duration.json"), json!({"version":1,"fetchedAt":2,
            "config":{"version":1,"config":{"k3":{"min_tokens_to_hint":1,"cache_duration":2400},"k4":{"min_tokens_to_hint":1,"cache_duration":3600}}}}).to_string()).unwrap();
        let home = dir.path().to_str().unwrap();
        let configured = |model: &str| eligibility(&json!({"agent":"kimi","model":model,"agent_home":home}), &json!({}), Some(NOW));
        // The newest fetched configuration decides; it can add models.
        assert_eq!((configured("k3").ttl, configured("k3").source), (Some(2400), "kimi_config"));
        assert_eq!(configured("k4").ttl, Some(3600));
    }

    #[test]
    fn deepseek_opencode_and_unknown_agents_are_not_offered() {
        for agent in ["dsh", "opencode", "sh"] {
            let rule = eligibility(&json!({"agent":agent}), &json!({}), Some(NOW));
            assert_eq!((rule.supported, rule.reason_code), (false, if agent == "dsh" { "deepseek" } else { "agent" }));
            assert!(!rule.reason.is_empty());
        }
    }

    fn idle() -> Value {
        json!({"name":"claude/project/warm","agent":"claude","run_id":"run-1","conversation_id":"conversation-1",
            "activity":"idle","phase":"idle","active_tools":{},"subagents":{}})
    }

    fn quiet() -> Facts {
        Facts { background: Some(0), schedules: 0, recovery_active: false }
    }

    #[test]
    fn a_ping_goes_only_to_a_settled_agent_with_nothing_else_pending() {
        let mut store = Store::default();
        update(&mut store, "owner", &record(), &request(json!({})), NOW, ok);
        let entry = store.sessions.get("owner");
        let runtime = json!({"state":"dispatching","request_id":"ping-1"});
        let refusal = |record: &Value, entry: Option<&Entry>, runtime: &Value, facts: &Facts, now: f64| {
            submission_refusal(record, entry, runtime, facts, "ping-1", now)
        };
        assert_eq!(refusal(&idle(), entry, &runtime, &quiet(), NOW), None);
        // The setting changed or expired, or another ping is current.
        let off = Entry { enabled: false, ..entry.unwrap().clone() };
        assert!(refusal(&idle(), Some(&off), &runtime, &quiet(), NOW).is_some());
        assert!(refusal(&idle(), None, &runtime, &quiet(), NOW).is_some());
        assert!(refusal(&idle(), entry, &runtime, &quiet(), NOW + 3.0 * 3600.0).is_some());
        assert!(refusal(&idle(), entry, &json!({"state":"dispatching","request_id":"ping-0"}), &quiet(), NOW).is_some());
        assert!(refusal(&idle(), entry, &json!({"state":"waiting","request_id":"ping-1"}), &quiet(), NOW).is_some());
        // The agent works, or work or input waits.
        for (key, value) in [("activity", json!("busy")), ("phase", json!("tool")),
            ("active_tools", json!({"t":{"name":"Bash"}})), ("keep_warm_turn", json!({"request_id":"ping-0"})),
            ("launch_prompt", json!({"status":"pending"})), ("paused", json!(true))] {
            let mut busy = idle();
            busy[key] = value;
            assert!(refusal(&busy, entry, &runtime, &quiet(), NOW).is_some(), "{key}");
        }
        for facts in [Facts { background: Some(1), ..quiet() }, Facts { background: None, ..quiet() },
            Facts { schedules: 1, ..quiet() }, Facts { recovery_active: true, ..quiet() }] {
            assert!(refusal(&idle(), entry, &runtime, &facts, NOW).is_some(), "{facts:?}");
        }
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
