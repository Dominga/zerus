//! Settings > Automatic recovery (tray/src/RecoverySettings.h): the recovery
//! policy every connected machine shares.
//!
//! [`RecoverySync`] keeps the newest policy by its Lamport order (revision,
//! then writer), reads every machine every [`REFRESH`], copies the newest to
//! the machines behind it and saves a change to one reachable machine. As in
//! Qt it runs whether or not the page is open: each machine's worker keeps
//! executing its own durable copy. [`form`] and [`policy`] are the page's
//! load and save.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::actions::{Exit, Invocation};
use crate::machine::MachineMeta;
use crate::ComputerId;

/// Where the newest policy known is kept between runs (Qt's key).
pub const SHARED_POLICY_KEY: &str = "recovery/sharedPolicy";

/// How often every machine is read again.
pub const REFRESH: Duration = Duration::from_secs(15);

/// A reply larger than this is refused unread.
const MAX_REPLY_BYTES: usize = 1024 * 1024;

/// The error classes, in the page's order, with Qt's labels.
pub const RULES: [(&str, &str); 4] = [
    ("service", "Overload / server unavailable"),
    ("rate_limit", "Temporary rate limit"),
    ("session_limit", "Session usage limit"),
    ("network", "Connection / timeout"),
];

/// What a call does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Get,
    /// A change made here, checked against the base it was made on.
    Set,
    /// A newer policy copied to a machine behind.
    Sync,
}

impl Op {
    pub fn as_str(self) -> &'static str {
        match self {
            Op::Get => "get",
            Op::Set => "set",
            Op::Sync => "sync",
        }
    }
}

/// `hgs [@host] recovery <op>`, the policy on standard input for set and sync.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub id: u64,
    pub host: String,
    pub op: Op,
    pub data: Map<String, Value>,
}

/// The call's invocation (HgsClient::requestRecovery): 10 s on this
/// computer, 25 s on another.
pub fn invocation(call: &Call) -> Invocation {
    let computer = ComputerId::from(call.host.as_str());
    let mut args: Vec<String> = computer
        .alias()
        .map(|alias| format!("@{alias}"))
        .into_iter()
        .collect();
    args.extend(["recovery".to_owned(), call.op.as_str().to_owned()]);
    Invocation {
        args,
        stdin: (call.op != Op::Get)
            .then(|| Value::Object(call.data.clone()).to_string().into_bytes()),
        timeout: Duration::from_secs(if call.host.is_empty() { 10 } else { 25 }),
        request_id: None,
    }
}

/// The call's answer as Qt words it: the reply object, or why it failed.
pub fn answer(exit: Exit) -> Result<Map<String, Value>, String> {
    let (code, stdout, stderr) = match exit {
        Exit::TimedOut => {
            return Err("Recovery request timed out. Reload its state before trying again.".into())
        }
        Exit::StartFailed(error) => return Err(error),
        Exit::Code {
            code,
            stdout,
            stderr,
        } => (code, stdout, stderr),
        Exit::Abnormal { stderr } => (-1, Vec::new(), stderr),
        Exit::WriteFailed => (-1, Vec::new(), String::new()),
    };
    if code != 0 {
        let error: String = stderr.trim().chars().take(2000).collect();
        return Err(if error.is_empty() {
            "Could not update recovery. Reload its state.".into()
        } else {
            error
        });
    }
    if stdout.len() > MAX_REPLY_BYTES {
        return Err("Invalid recovery response".into());
    }
    match serde_json::from_slice::<Value>(&stdout) {
        Ok(Value::Object(reply)) => Ok(reply),
        _ => Err("Invalid recovery response".into()),
    }
}

/// Runs the call with its reply bounded: a peer that sends more than the
/// limit is cut off rather than read to the end.
pub fn call(hgs: &std::path::Path, call: &Call) -> Result<Map<String, Value>, String> {
    match crate::run::run_limited(hgs, &invocation(call), &[], MAX_REPLY_BYTES) {
        Ok(exit) => answer(exit),
        Err(_) => Err("Invalid recovery response".into()),
    }
}

/// What the caller does.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// Run the call ([`invocation`], [`answer`]) and answer
    /// [`RecoverySync::answered`].
    Call(Call),
    /// Keep the newest policy under [`SHARED_POLICY_KEY`].
    Persist(String),
}

/// An envelope (`{order, policy}`) a machine answered with: a policy object
/// and a two-part order.
fn envelope_valid(envelope: &Map<String, Value>) -> bool {
    envelope.get("policy").is_some_and(Value::is_object)
        && envelope
            .get("order")
            .and_then(Value::as_array)
            .is_some_and(|order| order.len() == 2)
}

/// RecoverySync::compare: by revision, then by writer; an envelope without
/// an order is older than one with.
pub fn compare(a: &Map<String, Value>, b: &Map<String, Value>) -> Ordering {
    let order = |envelope: &Map<String, Value>| {
        envelope
            .get("order")
            .and_then(Value::as_array)
            .filter(|order| order.len() == 2)
            .cloned()
    };
    match (order(a), order(b)) {
        (Some(x), Some(y)) => {
            let (rx, ry) = (x[0].as_f64().unwrap_or(0.0), y[0].as_f64().unwrap_or(0.0));
            if rx != ry {
                return rx.partial_cmp(&ry).unwrap_or(Ordering::Equal);
            }
            x[1].as_str().unwrap_or("").cmp(y[1].as_str().unwrap_or(""))
        }
        (Some(_), None) => Ordering::Greater,
        (None, Some(_)) => Ordering::Less,
        (None, None) => Ordering::Equal,
    }
}

/// The shared policy across this computer and its peers.
pub struct RecoverySync {
    best: Map<String, Value>,
    machines: Vec<String>,
    local: bool,
    enable_offered: bool,
    copies: BTreeMap<String, Map<String, Value>>,
    errors: BTreeMap<String, String>,
    pending: BTreeMap<u64, (String, Op)>,
    save_error: String,
    saves: u64,
    next: u64,
    /// This computer's machine card arrived: until then nothing is read or
    /// written anywhere, so a control console never touches itself.
    known: bool,
}

impl RecoverySync {
    /// Starts from the newest policy kept from before, if it is valid.
    pub fn new(stored: &str) -> RecoverySync {
        let best = serde_json::from_str::<Value>(stored)
            .ok()
            .and_then(|value| value.as_object().cloned())
            .filter(envelope_valid)
            .unwrap_or_default();
        RecoverySync {
            best,
            machines: Vec::new(),
            local: true,
            enable_offered: false,
            copies: BTreeMap::new(),
            errors: BTreeMap::new(),
            pending: BTreeMap::new(),
            save_error: String::new(),
            saves: 0,
            next: 0,
            known: false,
        }
    }

    /// The machines that keep a policy: this one (when it runs agents),
    /// then the peers.
    fn hosts(&self) -> Vec<String> {
        let mut hosts = Vec::new();
        if self.local {
            hosts.push(String::new());
        }
        hosts.extend(self.machines.iter().cloned());
        hosts
    }

    fn label(host: &str) -> &str {
        if host.is_empty() {
            "this machine"
        } else {
            host
        }
    }

    fn send(&mut self, host: &str, op: Op, data: Map<String, Value>) -> Effect {
        self.next += 1;
        self.pending.insert(self.next, (host.to_owned(), op));
        Effect::Call(Call {
            id: self.next,
            host: host.to_owned(),
            op,
            data,
        })
    }

    /// The peers this computer lists (empty names and repeats dropped); a
    /// change reads them all again.
    pub fn set_peers(&mut self, peers: Vec<String>) -> Vec<Effect> {
        let mut machines: Vec<String> = Vec::new();
        for peer in peers {
            if !peer.is_empty() && !machines.contains(&peer) {
                machines.push(peer);
            }
        }
        if machines == self.machines {
            return Vec::new();
        }
        self.machines = machines;
        self.refresh()
    }

    /// Whether this computer runs agents and so keeps a policy of its own;
    /// a control-only computer reads only the remote machines.
    pub fn set_local_sessions(&mut self, enabled: bool) -> Vec<Effect> {
        if enabled == self.local {
            return Vec::new();
        }
        self.local = enabled;
        self.copies.remove("");
        self.errors.remove("");
        self.refresh()
    }

    /// Whether Machines offers to enable local sessions here, which the
    /// status then names.
    pub fn set_local_sessions_offered(&mut self, offered: bool) {
        self.enable_offered = offered;
    }

    /// The newest policy known (`{order, policy}`), empty before any.
    pub fn envelope(&self) -> &Map<String, Value> {
        &self.best
    }

    pub fn has_machines(&self) -> bool {
        !self.hosts().is_empty()
    }

    pub fn busy(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Why the last save failed; empty when it did not.
    pub fn error(&self) -> &str {
        &self.save_error
    }

    /// How many saves have been answered, so a page can tell its own
    /// save's answer from a read's.
    pub fn saves(&self) -> u64 {
        self.saves
    }

    fn save_target(&self) -> Option<String> {
        self.machines
            .iter()
            .find(|host| !self.errors.contains_key(*host) && self.copies.contains_key(*host))
            .cloned()
    }

    /// Whether a save has a machine to go to: this one, or a reachable peer.
    pub fn can_save(&self) -> bool {
        !self.busy() && !self.best.is_empty() && (self.local || self.save_target().is_some())
    }

    /// The synchronization line under the page.
    pub fn status(&self) -> String {
        if !self.has_machines() {
            return if self.enable_offered {
                "To configure automatic recovery, enable local sessions for this computer in Machines or add a remote machine."
            } else {
                "Add a remote machine in Machines to configure automatic recovery; this computer runs no agents."
            }
            .into();
        }
        let hosts = self.hosts();
        let missing: Vec<&str> = hosts
            .iter()
            .filter(|host| self.errors.contains_key(*host) || !self.copies.contains_key(*host))
            .map(|host| Self::label(host))
            .collect();
        if self.busy() {
            return "Synchronizing recovery settings…".into();
        }
        if !missing.is_empty() {
            return format!(
                "Waiting for {}. Changes will sync when connected; those machines keep their last settings.",
                missing.join(", ")
            );
        }
        if hosts.len() == 1 {
            "Synchronized with 1 machine. Changes from any Zerus are shared.".into()
        } else {
            format!(
                "Synchronized across {} machines. Changes from any Zerus are shared.",
                hosts.len()
            )
        }
    }

    /// Reads every machine, unless a read or save is under way or this
    /// computer is not known yet.
    pub fn refresh(&mut self) -> Vec<Effect> {
        if self.busy() || !self.known {
            return Vec::new();
        }
        self.hosts()
            .iter()
            .map(|host| self.send(host, Op::Get, Map::new()))
            .collect()
    }

    /// Saves `policy`, edited from `base`, to one reachable machine; the
    /// next reads copy it to the rest.
    pub fn save(&mut self, policy: Map<String, Value>, base: &Map<String, Value>) -> Vec<Effect> {
        if self.busy() || !self.known {
            self.save_error = "Wait for synchronization to finish before saving.".into();
            return Vec::new();
        }
        if self.best.is_empty() || compare(base, &self.best) != Ordering::Equal {
            self.save_error = "Settings changed in another Zerus. Reload before saving.".into();
            return Vec::new();
        }
        let target = if self.local {
            Some(String::new())
        } else {
            self.save_target()
        };
        let Some(target) = target else {
            self.save_error = "Connect a remote machine to save recovery settings.".into();
            return Vec::new();
        };
        self.save_error.clear();
        vec![self.send(&target, Op::Set, policy)]
    }

    /// A call's answer.
    pub fn answered(&mut self, id: u64, reply: Result<Map<String, Value>, String>) -> Vec<Effect> {
        let Some((host, op)) = self.pending.remove(&id) else {
            return Vec::new();
        };
        if op == Op::Set {
            self.saves += 1;
        }
        let mut effects = Vec::new();
        match reply {
            Err(error) => {
                if op == Op::Set {
                    self.save_error = error.clone();
                }
                self.errors.insert(host, error);
            }
            Ok(data) if !envelope_valid(&data) => {
                let error =
                    "Update HGS on this machine to synchronize recovery settings.".to_owned();
                if op == Op::Set {
                    self.save_error = error.clone();
                }
                self.errors.insert(host, error);
            }
            Ok(data) => {
                self.errors.remove(&host);
                if self.best.is_empty() || compare(&data, &self.best) == Ordering::Greater {
                    self.best = data.clone();
                    effects.push(Effect::Persist(
                        Value::Object(self.best.clone()).to_string(),
                    ));
                }
                self.copies.insert(host, data);
                if op == Op::Set {
                    self.save_error.clear();
                }
            }
        }
        if self.pending.is_empty() {
            effects.extend(self.reconcile());
        }
        effects
    }

    /// Copies the newest policy to every reachable machine behind it.
    fn reconcile(&mut self) -> Vec<Effect> {
        if self.best.is_empty() {
            return Vec::new();
        }
        let policy = self
            .best
            .get("policy")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let behind: Vec<String> = self
            .hosts()
            .into_iter()
            .filter(|host| {
                !self.errors.contains_key(host)
                    && self
                        .copies
                        .get(host)
                        .is_some_and(|copy| compare(copy, &self.best) == Ordering::Less)
            })
            .collect();
        behind
            .iter()
            .map(|host| self.send(host, Op::Sync, policy.clone()))
            .collect()
    }
}

/// One period of the service, as SessionsWindow drives Qt's RecoverySync:
/// nothing until this computer's machine card is known; then whether it runs
/// agents (setLocalSessions, before the peers), whether Machines offers to
/// enable them, the peers (setPeers), and, when `due` (Qt's 15 s timer), a
/// read of every machine.
pub fn tick(
    sync: &mut RecoverySync,
    card: Option<&MachineMeta>,
    peers: Option<Vec<String>>,
    due: bool,
) -> Vec<Effect> {
    let Some(card) = card else {
        return Vec::new();
    };
    sync.known = true;
    let mut effects = sync.set_local_sessions(card.local_sessions.unwrap_or(true));
    sync.set_local_sessions_offered(crate::launch::can_enable_local_sessions(card));
    if let Some(peers) = peers {
        effects.extend(sync.set_peers(peers));
    }
    if due {
        effects.extend(sync.refresh());
    }
    effects
}

/// The page's form: the switch, each class's choice, the session limit's
/// mode and each class's delays as typed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Form {
    pub enabled: bool,
    pub rules: BTreeMap<String, bool>,
    /// `reset` or `interval`.
    pub session_limit_mode: String,
    pub delays: BTreeMap<String, String>,
}

fn whole(value: &Value) -> i64 {
    value
        .as_i64()
        .or_else(|| {
            value
                .as_f64()
                .filter(|n| n.fract() == 0.0)
                .map(|n| n as i64)
        })
        .unwrap_or(0)
}

/// RecoverySettings::load: the form for the newest policy, None before any.
pub fn form(envelope: &Map<String, Value>) -> Option<Form> {
    if envelope.is_empty() {
        return None;
    }
    let policy = envelope
        .get("policy")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let schedules = policy
        .get("schedules")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mode = match policy.get("session_limit_mode").and_then(Value::as_str) {
        Some("interval") => "interval",
        _ => "reset",
    };
    let mut form = Form {
        enabled: policy
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        session_limit_mode: mode.into(),
        ..Form::default()
    };
    for (rule, _) in RULES {
        let session = rule == "session_limit";
        form.rules.insert(
            rule.into(),
            policy.get(rule).and_then(Value::as_bool).unwrap_or(session),
        );
        let mut sequence: Vec<i64> = match schedules.get(rule) {
            Some(list) => list
                .as_array()
                .map(|items| items.iter().map(whole).collect())
                .unwrap_or_default(),
            None if session => vec![300, 0],
            None => policy
                .get("delays")
                .and_then(Value::as_array)
                .map(|items| items.iter().map(whole).collect())
                .unwrap_or_default(),
        };
        if sequence.last().is_some_and(|last| *last > 0) {
            sequence.push(-1);
        }
        let text: Vec<String> = sequence.iter().map(i64::to_string).collect();
        form.delays.insert(rule.into(), text.join(", "));
    }
    Some(form)
}

/// One class's delays as typed: 1-16 values from 1 to 86400 seconds, and
/// only the last may be -1 (stop) or 0 (repeat the delay before it).
fn delays(text: &str) -> Option<Vec<i64>> {
    let parts: Vec<&str> = text.split(',').collect();
    if parts.is_empty() || parts.len() > 16 {
        return None;
    }
    let last = parts.len() - 1;
    parts
        .iter()
        .enumerate()
        .map(|(at, part)| {
            let value: i32 = part.trim().parse().ok()?;
            let value = i64::from(value);
            ((1..=86400).contains(&value)
                || (at == last && (value == -1 || (value == 0 && at > 0))))
                .then_some(value)
        })
        .collect()
}

/// RecoverySettings' Save: the policy to send, built on the policy the form
/// was loaded from (its revision and writer included), or Qt's text when a
/// class's delays are invalid.
pub fn policy(base: &Map<String, Value>, form: &Form) -> Result<Map<String, Value>, String> {
    let mut policy = base
        .get("policy")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut schedules = Map::new();
    for (rule, _) in RULES {
        let text = form.delays.get(rule).map(String::as_str).unwrap_or("");
        let Some(list) = delays(text) else {
            return Err("Use 1–16 values: delays from 1 to 86400 seconds; −1 or 0 may only be last, and 0 needs a preceding delay.".into());
        };
        schedules.insert(rule.into(), json!(list));
        policy.insert(
            rule.into(),
            json!(form.rules.get(rule).copied().unwrap_or(false)),
        );
    }
    let mode = if form.session_limit_mode == "interval" {
        "interval"
    } else {
        "reset"
    };
    policy.insert("version".into(), json!(3));
    policy.insert("enabled".into(), json!(form.enabled));
    policy.insert("schedules".into(), Value::Object(schedules));
    policy.insert("session_limit_mode".into(), json!(mode));
    Ok(policy)
}

#[cfg(test)]
mod tests;
