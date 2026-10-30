//! Screen view models of the Overview, Projects, Machines and Accounts
//! screens: pure projections of `hgs` read replies (`crate::reads`) and of
//! the fleet's stream state into compact serde structs the webview renders
//! as they are. Texts that depend on the clock (ages, resets, pace) stay in
//! the frontend, which owns the clock; every other text Qt words is worded here.
use crate::ComputerId;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod accounts;
pub mod machines;
pub mod projects;
pub mod schedule;
pub mod session_usage;
pub mod usage;

/// One computer of the fleet as the screens see it: the Tauri pump's mirror
/// of the stream statuses and machine cards. An ended computer is not listed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FleetEntry {
    pub computer: ComputerId,
    /// `probing`, `unsupported`, `live`, `polled` or `reconnecting`.
    pub status: String,
    pub attempt: u32,
    /// False only on a control console (`catalog.meta.local_sessions`).
    pub local_sessions: bool,
    /// The meta names a Windows session host (`catalog.meta.local_host`).
    pub local_host: bool,
    /// `catalog.meta.host`, once its card arrived.
    pub host: Option<String>,
    /// `catalog.meta.metrics.platform`.
    pub platform: Option<String>,
}

impl FleetEntry {
    /// The name people know the computer by: a peer's alias, this computer's host name.
    pub fn name(&self) -> String {
        match self.computer.alias() {
            Some(alias) => alias.to_owned(),
            None => self
                .host
                .clone()
                .filter(|host| !host.is_empty())
                .unwrap_or_else(|| "this computer".to_owned()),
        }
    }
}

pub(crate) fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned()
}

pub(crate) fn flag(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

pub(crate) fn number(value: &Value, key: &str) -> Option<f64> {
    value
        .get(key)
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite())
}

/// A whole non-negative number, also when sent as a float such as `300.0`.
pub(crate) fn whole(value: &Value, key: &str) -> Option<u64> {
    let found = value.get(key)?;
    found.as_u64().or_else(|| {
        found
            .as_f64()
            .filter(|n| n.is_finite() && *n >= 0.0 && n.fract() == 0.0)
            .map(|n| n as u64)
    })
}

pub(crate) fn items<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

#[cfg(test)]
pub(crate) fn fixture(name: &str) -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/views")
        .join(name);
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}
