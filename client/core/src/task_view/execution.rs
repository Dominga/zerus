//! A bounded read of the CLI's existing replica projection, not a reducer or
//! authority proof. Native runtime freshness and executor queues are absent.
//! Only selected scalar fields are copied; frozen inputs/history never cross
//! this boundary. New phases/origins and malformed fields stay unavailable.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Execution {
    pub requirement: u64,
    pub attempts: u64,
    pub current: Option<Attempt>,
    pub conflict: bool,
    /// Any generation may still have uncertain delivery. Retain that fact
    /// without retaining its potentially history-sized list of IDs.
    pub has_unknown: bool,
    pub has_unexplained: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attempt {
    /// Empty with generation zero for the explicitly marked legacy projection.
    pub id: String,
    pub generation: u64,
    pub origin: Origin,
    pub executor: Option<Executor>,
    pub request_id: String,
    pub requirement: u64,
    pub phase: Phase,
    /// Recorded Unix milliseconds; these are not a current native observation.
    pub at: i64,
    pub began_at: Option<i64>,
    pub progress_at: Option<i64>,
    pub legacy: bool,
}

/// The exact saved session reference, including older name-only references.
/// Neither a label nor `resolved` establishes a native PID/birth or child.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Executor {
    pub machine: String,
    pub session: String,
    pub session_id: String,
    pub host: Option<String>,
    pub resolved: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    Send,
    Start,
    Handoff,
    Reopen,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Submitted,
    Unknown,
    Running,
    Paused,
    Ready,
    Cancelled,
    #[serde(rename = "not_started")]
    NotStarted,
    Unconfirmed,
}

// Refuse overlong identities rather than truncating them into another identity.
fn text(value: &Value) -> Option<String> {
    let text = value.as_str()?;
    (text.len() <= 512 && !text.chars().any(char::is_control)).then(|| text.to_owned())
}
fn default_text(value: &Value, key: &str) -> Option<String> {
    value.get(key).map_or_else(|| Some(String::new()), text)
}
fn flag(value: &Value, key: &str, default: bool) -> Option<bool> {
    value.get(key).map_or(Some(default), Value::as_bool)
}
fn time(value: &Value, key: &str) -> Option<Option<i64>> {
    match value.get(key) {
        None | Some(Value::Null) => Some(None),
        Some(value) => value.as_i64().map(Some),
    }
}
fn nonempty(value: &Value, key: &str) -> Option<bool> {
    match value.get(key) {
        None => Some(false),
        Some(value) => Some(!value.as_array()?.is_empty()),
    }
}

impl Execution {
    pub(crate) fn from_value(value: &Value) -> Option<Self> {
        value.as_object()?;
        let current = match value.get("current") {
            None | Some(Value::Null) => None,
            Some(value) => Some(Attempt::from_value(value)?),
        };
        Some(Self {
            requirement: value["requirement"].as_u64()?,
            // The existing replica protocol admits at most 256 attempts.
            attempts: value["attempts"].as_u64().filter(|n| *n <= 256)?,
            current,
            conflict: flag(value, "conflict", false)?,
            has_unknown: nonempty(value, "unknown")?,
            has_unexplained: nonempty(value, "unexplained")?,
        })
    }
}

impl Attempt {
    fn from_value(value: &Value) -> Option<Self> {
        value.as_object()?;
        let origin = match value["origin"].as_str()? {
            "send" => Origin::Send,
            "start" => Origin::Start,
            "handoff" => Origin::Handoff,
            "reopen" => Origin::Reopen,
            _ => return None,
        };
        let phase = match value["phase"].as_str()? {
            "submitted" => Phase::Submitted,
            "unknown" => Phase::Unknown,
            "running" => Phase::Running,
            "paused" => Phase::Paused,
            "ready" => Phase::Ready,
            "cancelled" => Phase::Cancelled,
            "not_started" => Phase::NotStarted,
            "unconfirmed" => Phase::Unconfirmed,
            _ => return None,
        };
        let id = default_text(value, "id")?;
        let generation = value["generation"].as_u64()?;
        let legacy = flag(value, "legacy", false)?;
        if if legacy {
            !id.is_empty() || generation != 0
        } else {
            !crate::creation::canonical_uuid(&id) || generation == 0
        } {
            return None;
        }
        let executor = match value.get("executor") {
            None | Some(Value::Null) if legacy => None,
            Some(value) => Some(Executor::from_value(value)?),
            _ => return None,
        };
        let began_at = time(value, "began_at")?;
        if phase == Phase::Running && began_at.is_none() {
            return None;
        }
        Some(Self {
            id,
            generation,
            origin,
            executor,
            request_id: default_text(value, "request_id")?,
            requirement: value["requirement"].as_u64()?,
            phase,
            at: value["at"].as_i64()?,
            began_at,
            progress_at: time(value, "progress_at")?,
            legacy,
        })
    }
}

impl Executor {
    fn from_value(value: &Value) -> Option<Self> {
        let host = match value.get("host") {
            None | Some(Value::Null) => None,
            Some(value) => Some(text(value)?),
        };
        Some(Self {
            machine: text(&value["machine"])?,
            session: text(&value["session"])?,
            session_id: default_text(value, "session_id")?,
            host,
            resolved: flag(value, "resolved", true)?
                && !value.get("host").is_some_and(Value::is_null),
        })
    }
}
