//! Keep cache warm, ported from Qt's `CacheKeepalive` (WhiteWh fork,
//! `tray/src/CacheKeepalive.{h,cpp}`): shortly before the provider's prompt
//! cache of an idle conversation would expire, Zerus sends the session a
//! one-word service message. hgs does not count its reply; a reply it does
//! count while a keepalive is pending (agents without a journaled prompt) is
//! handed back to be marked read. Warming stops after the chosen hours
//! without a turn and never revives a cold cache.
//!
//! It costs the account's limit, so it is strictly opt-in: a session is
//! warmed only while the person set its hours (device-local, default off).
//! A build Zerus runs warms every [`BUILD_INTERVAL`] whatever those hours
//! (U30, #508), but only for a session whose keep-warm is on.
//!
//! The hours live in `workspace/cacheKeepWarm`, Qt's own JSON object of
//! `hex(SessionRowPreferences::key) -> hours`, so a migrated Qt choice keeps
//! working. [`Keepalive`] is the pure state machine; the app runs it on every
//! fleet poll and carries out its [`Effect`]s.
use crate::actions::Target;
use crate::readmarks::ReadMark;
use crate::view::ViewRow;
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};

/// Qt's settings key (`CacheKeepalive.cpp` SettingsKey).
pub const KEY: &str = "workspace/cacheKeepWarm";
/// On from the session menu is one hour (`CacheKeepalive::DefaultHours`).
pub const DEFAULT_HOURS: i64 = 1;
/// What storage keeps (`MaxHours`); the Details slider offers 1 to
/// [`SLIDER_HOURS`].
pub const MAX_HOURS: i64 = 24;
pub const SLIDER_HOURS: i64 = 8;
/// The provider cache's lifetime until Claude's native usage names it.
pub const DEFAULT_TTL: f64 = 300.0;
/// U30 (#508): a building session is warmed every 30 minutes.
pub const BUILD_INTERVAL: f64 = 1800.0;
/// Bounded like the other device-local session preferences.
pub const MAX_SESSIONS: usize = 10_000;
/// hgs recognizes the envelope (CACHE_KEEPALIVE in src/state/journal.rs).
pub const MESSAGE: &str = "<zerus-cache-keepalive>Zerus keeps this conversation's prompt cache warm while the person is away. Do not use tools or continue any task. Reply with exactly: ok</zerus-cache-keepalive>";
/// An unanswered keepalive counts as done after this long idle again...
const ANSWERED: f64 = 20.0;
/// ...and as lost, the cache cold, after this long.
const LOST: f64 = 600.0;
/// Polls of other computers come about once a minute.
const MARGIN: f64 = 90.0;

/// `QJsonValue::toInt()`: a whole number within 32 bits, otherwise 0.
fn qt_int(value: &Value) -> i64 {
    match value.as_i64() {
        Some(n) if i32::try_from(n).is_ok() => n,
        Some(_) => 0,
        None => value
            .as_f64()
            .filter(|n| n.fract() == 0.0 && (f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(n))
            .map_or(0, |n| n as i64),
    }
}

/// The stored object (`SettingsJson::object`): empty when it is missing or
/// not a JSON object.
pub fn parse(text: &str) -> Map<String, Value> {
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(object)) => object,
        _ => Map::new(),
    }
}

/// The compact JSON Qt stores.
pub fn stored(saved: &Map<String, Value>) -> String {
    Value::Object(saved.clone()).to_string()
}

/// The session's identity: the exact `SessionRowPreferences::key`, or its
/// conversation-less fallback, as lowercase hex. `host` is the configured
/// peer alias, "" for this computer.
pub fn id(host: &str, row: &ViewRow) -> Option<String> {
    crate::row_preferences::keys(host, row).map(|[exact, _]| exact)
}

/// `CacheKeepalive::hours`: the saved hours of `id`, 0 when off.
pub fn hours(saved: &Map<String, Value>, id: &str) -> i64 {
    saved.get(id).map_or(0, qt_int).clamp(0, MAX_HOURS)
}

/// `CacheKeepalive::setHours`: 0 or less removes the key; a new key is
/// added only below [`MAX_SESSIONS`]. Whether `saved` changed.
pub fn set_hours(saved: &mut Map<String, Value>, id: &str, hours: i64) -> bool {
    if hours <= 0 {
        return saved.remove(id).is_some();
    }
    if !saved.contains_key(id) && saved.len() >= MAX_SESSIONS {
        return false;
    }
    let value = Value::from(hours.min(MAX_HOURS));
    saved.insert(id.to_owned(), value.clone()) != Some(value)
}

/// What the session menu's toggle stores: off, or one hour (the hours stay
/// as set in Details only while it is on).
pub fn toggled(current: i64) -> i64 {
    if current > 0 {
        0
    } else {
        DEFAULT_HOURS
    }
}

/// `CacheKeepalive::ttl`: Claude's cache lifetime from its native usage.
pub fn ttl(details: &Value) -> f64 {
    let seconds = qt_int(&details["session_usage"]["prompt_cache"]["ttl_seconds"]);
    if seconds > 0 {
        seconds as f64
    } else {
        DEFAULT_TTL
    }
}

fn running(s: &ViewRow) -> bool {
    s.state == "running" && s.process_state != "exited"
}

/// `CacheKeepalive::idle`: nothing at all runs or waits.
pub fn idle(s: &ViewRow) -> bool {
    running(s)
        && s.activity == "idle"
        && s.phase.as_deref() == Some("idle")
        && s.background <= 0
        && s.notify.subagents_active == 0
        && !s.notify.coordination_wait
        && !s.needs_action
}

/// `CacheKeepalive::building`: a build Zerus runs; the agent itself is idle,
/// the build is its work.
pub fn building(s: &ViewRow) -> bool {
    s.session_state == "build"
        && running(s)
        && s.activity == "idle"
        && s.phase.as_deref() == Some("idle")
        && !s.needs_action
}

/// What the app does for the state machine.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// Send [`MESSAGE`] through the ordinary send path, guarded by the
    /// target's run and conversation, then report [`Keepalive::sent`] or
    /// [`Keepalive::failed`] with `request`. Never retried.
    Send { request: u64, target: Target },
    /// One inspection of the session, answered with [`Keepalive::inspected`]
    /// or [`Keepalive::inspection_failed`].
    Inspect { target: Target },
    /// A reply hgs counted while a keepalive was pending: mark it read.
    Read(ReadMark),
}

#[derive(Clone, Debug, PartialEq)]
struct State {
    host: String,
    reply_id: String,
    run: String,
    conversation: String,
    /// The person's or the agent's last turn.
    real: f64,
    /// The cache's last use.
    warmed: f64,
    /// A keepalive in flight or unanswered.
    sent: f64,
    request: u64,
    ttl: f64,
    /// Until the next real turn.
    cold: bool,
    inspected: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            host: String::new(),
            reply_id: String::new(),
            run: String::new(),
            conversation: String::new(),
            real: 0.0,
            warmed: 0.0,
            sent: 0.0,
            request: 0,
            ttl: DEFAULT_TTL,
            cold: false,
            inspected: false,
        }
    }
}

/// The keepalives of every computer, keyed by the session identity ([`id`]).
#[derive(Default)]
pub struct Keepalive {
    states: HashMap<String, State>,
    /// Sends not yet answered, by request.
    requests: HashMap<u64, String>,
    /// Inspections under way, by `host\nname`.
    inspections: HashMap<String, String>,
    next_request: u64,
}

impl Keepalive {
    pub fn new() -> Self {
        Self::default()
    }

    /// Nothing is tracked: with no hours saved, a poll has nothing to do.
    pub fn is_empty(&self) -> bool {
        self.states.is_empty() && self.requests.is_empty() && self.inspections.is_empty()
    }

    /// A keepalive of this session is in flight or unanswered.
    pub fn pending(&self, id: &str) -> bool {
        self.states.get(id).is_some_and(|state| state.sent > 0.0)
    }

    /// Keepalives sent and not yet answered.
    pub fn in_flight(&self) -> usize {
        self.requests.len()
    }

    /// `CacheKeepalive::observe`: one computer's fresh listing (`host` is ""
    /// for this computer, else its alias), the saved hours and the time in
    /// seconds since the epoch. Call it only for a listing that succeeded.
    pub fn observe(
        &mut self,
        host: &str,
        rows: &[ViewRow],
        saved: &Map<String, Value>,
        now: f64,
    ) -> Vec<Effect> {
        let mut effects = Vec::new();
        let mut listed = HashSet::new();
        for s in rows {
            let conversation = s.conversation_id.as_deref().unwrap_or("");
            let run = s.run_id.as_deref().unwrap_or("");
            // Opt-in only: a session the person did not turn on is never warmed,
            // not even while it builds.
            if s.state != "running" || conversation.is_empty() || saved.is_empty() {
                continue;
            }
            let Some(key) = id(host, s) else {
                continue;
            };
            let limit = hours(saved, &key);
            if limit == 0 {
                continue;
            }
            let build = building(s);
            listed.insert(key.clone());
            let reply_id = s.reply_id.as_deref().unwrap_or("");
            let reply_at = s.reply_at.unwrap_or(0.0);
            let target = || Target {
                computer: host.to_owned(),
                name: s.name.clone(),
                run_id: run.to_owned(),
                conversation_id: conversation.to_owned(),
            };
            let st = self.states.entry(key.clone()).or_default();
            if st.run != run || st.conversation != conversation {
                // Enabled, resumed or replaced: the cache was last used by the last reply.
                *st = State {
                    host: host.to_owned(),
                    run: run.to_owned(),
                    conversation: conversation.to_owned(),
                    reply_id: reply_id.to_owned(),
                    real: reply_at,
                    warmed: reply_at,
                    ..State::default()
                };
            }
            if reply_id != st.reply_id {
                if st.sent > 0.0 && reply_at + 1.0 >= st.sent {
                    effects.push(Effect::Read(ReadMark {
                        computer: host.to_owned(),
                        name: s.name.clone(),
                        run_id: run.to_owned(),
                        conversation_id: conversation.to_owned(),
                        reply_id: Some(reply_id.to_owned()).filter(|id| !id.is_empty()),
                        attention_signature: None,
                    }));
                    st.warmed = reply_at;
                } else {
                    st.real = if reply_at > 0.0 { reply_at } else { now };
                    st.warmed = st.real;
                    st.cold = false;
                    st.inspected = false;
                }
                st.sent = 0.0;
                st.request = 0;
                st.reply_id = reply_id.to_owned();
            }
            if st.sent > 0.0 {
                // hgs does not count a keepalive's reply: idle again later means it ran.
                if (idle(s) || build) && now - st.sent >= ANSWERED {
                    st.warmed = st.sent;
                    st.sent = 0.0;
                    st.request = 0;
                } else if now - st.sent > LOST {
                    st.sent = 0.0;
                    st.request = 0;
                    st.cold = true;
                }
                continue;
            }
            // A turn in progress uses the cache.
            if s.activity == "busy" {
                st.real = now;
                st.warmed = now;
                st.cold = false;
                continue;
            }
            // A build keeps the context warm every 30 minutes, with the same
            // message and reply handling; the hours and the cold mark wait for it.
            if build {
                // Counted from the build's start or the last warming, whichever is later.
                let since = st.warmed.max(s.build.as_ref().map_or(0.0, |b| b.since));
                if run.is_empty() {
                    continue;
                }
                if since <= 0.0 {
                    st.warmed = now;
                    continue;
                }
                if now - since < BUILD_INTERVAL {
                    continue;
                }
                self.next_request += 1;
                st.sent = now;
                st.request = self.next_request;
                self.requests.insert(st.request, key.clone());
                effects.push(Effect::Send {
                    request: st.request,
                    target: target(),
                });
                continue;
            }
            if !idle(s) || st.cold || run.is_empty() || st.warmed <= 0.0 {
                continue;
            }
            if s.notify.cmd == "claude" && !st.inspected {
                st.inspected = true;
                let inspection = format!("{host}\n{}", s.name);
                if let std::collections::hash_map::Entry::Vacant(e) = self.inspections.entry(inspection) {
                    e.insert(key.clone());
                    effects.push(Effect::Inspect { target: target() });
                }
            }
            if now - st.real > limit as f64 * 3600.0 {
                continue;
            }
            let margin = MARGIN.min(st.ttl / 3.0);
            if now >= st.warmed + st.ttl {
                st.cold = true;
                continue;
            }
            if now < st.warmed + st.ttl - margin {
                continue;
            }
            self.next_request += 1;
            st.sent = now;
            st.request = self.next_request;
            self.requests.insert(st.request, key.clone());
            effects.push(Effect::Send {
                request: st.request,
                target: target(),
            });
        }
        self.states
            .retain(|key, state| state.host != host || listed.contains(key));
        effects
    }

    /// The send of `request` was delivered.
    pub fn sent(&mut self, request: u64) {
        self.requests.remove(&request);
    }

    /// An undelivered or uncertain keepalive is never resent: the cache counts
    /// as cold until the next real turn.
    pub fn failed(&mut self, request: u64) {
        let Some(key) = self.requests.remove(&request) else {
            return;
        };
        if let Some(state) = self.states.get_mut(&key).filter(|s| s.request == request) {
            state.sent = 0.0;
            state.request = 0;
            state.cold = true;
        }
    }

    /// The inspection's TTL and the cache's expiry, for the same conversation.
    pub fn inspected(&mut self, host: &str, name: &str, details: &Value) {
        let Some(key) = self.inspections.remove(&format!("{host}\n{name}")) else {
            return;
        };
        let Some(state) = self.states.get_mut(&key) else {
            return;
        };
        if details["conversation_id"].as_str().unwrap_or("") != state.conversation {
            return;
        }
        state.ttl = ttl(details);
        let expires = details["session_usage"]["prompt_cache"]["expires_at"]
            .as_f64()
            .unwrap_or(0.0);
        if expires > 0.0 {
            state.warmed = state.warmed.max(expires - state.ttl);
        }
    }

    pub fn inspection_failed(&mut self, host: &str, name: &str) {
        self.inspections.remove(&format!("{host}\n{name}"));
    }
}

#[cfg(test)]
mod tests;
