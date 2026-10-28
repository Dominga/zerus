//! A computer's machine card data, projected from its `catalog.meta`: the
//! host, hgs version, platform, the latest metrics sample and desktop
//! presence. Every field is optional, as an older host may lack it; a field of
//! the wrong type reads as missing. Projects, peers, preferences and archive
//! paging stay out.
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One computer's card data.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MachineMeta {
    pub host: Option<String>,
    pub hgs_version: Option<String>,
    /// `metrics.platform`, such as `linux`, `macos` or `windows`.
    pub platform: Option<String>,
    /// Whether the listing behind the meta succeeded.
    pub ok: Option<bool>,
    pub metrics: Metrics,
    pub presence: Presence,
}

/// The latest metrics sample.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Metrics {
    pub cpu_percent: Option<f64>,
    pub cpu_count: Option<u64>,
    /// The sampler's state, which sets the card's load colour and its "no
    /// sample yet" state.
    pub cpu_state: Option<String>,
    pub cpu_interval_seconds: Option<f64>,
    pub load_1: Option<f64>,
    pub load_5: Option<f64>,
    pub load_15: Option<f64>,
    pub memory_used_bytes: Option<u64>,
    pub memory_total_bytes: Option<u64>,
    pub memory_available_bytes: Option<u64>,
    pub uptime_seconds: Option<f64>,
    /// When the sample was taken, in Unix seconds.
    pub sampled_at: Option<f64>,
}

/// Whether someone is at the computer's desktop.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Presence {
    pub idle_seconds: Option<f64>,
    pub locked: Option<bool>,
    pub locked_seconds: Option<f64>,
}

fn text(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn number(value: &Value, key: &str) -> Option<f64> {
    value
        .get(key)
        .and_then(Value::as_f64)
        .filter(|number| number.is_finite())
}

fn count(value: &Value, key: &str) -> Option<u64> {
    let found = value.get(key)?;
    found.as_u64().or_else(|| {
        // A count sent as a float, such as 8.0, is still a count.
        found
            .as_f64()
            .filter(|number| number.is_finite() && *number >= 0.0 && number.fract() == 0.0)
            .map(|number| number as u64)
    })
}

fn flag(value: &Value, key: &str) -> Option<bool> {
    value.get(key).and_then(Value::as_bool)
}

impl MachineMeta {
    /// The card data of one `catalog.meta` value.
    pub fn project(meta: &Value) -> Self {
        let metrics = meta.get("metrics").unwrap_or(&Value::Null);
        let presence = meta.get("desktop_presence").unwrap_or(&Value::Null);
        Self {
            host: text(meta, "host"),
            hgs_version: text(meta, "hgs_version"),
            platform: text(metrics, "platform"),
            ok: flag(meta, "ok"),
            metrics: Metrics {
                cpu_percent: number(metrics, "cpu_percent"),
                cpu_count: count(metrics, "cpu_count"),
                cpu_state: text(metrics, "cpu_state"),
                cpu_interval_seconds: number(metrics, "cpu_interval_seconds"),
                load_1: number(metrics, "load_1"),
                load_5: number(metrics, "load_5"),
                load_15: number(metrics, "load_15"),
                memory_used_bytes: count(metrics, "memory_used_bytes"),
                memory_total_bytes: count(metrics, "memory_total_bytes"),
                memory_available_bytes: count(metrics, "memory_available_bytes"),
                uptime_seconds: number(metrics, "uptime_seconds"),
                sampled_at: number(metrics, "sampled_at"),
            },
            presence: Presence {
                idle_seconds: number(presence, "idle_seconds"),
                locked: flag(presence, "locked"),
                locked_seconds: number(presence, "locked_seconds"),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_meta_projects_to_its_card_and_missing_or_mistyped_fields_are_none() {
        let meta = json!({
            "host": "workstation", "hgs_version": "1.56.0", "ok": true,
            "peers": ["studio"], "projects": {}, "preferences": {}, "archive_paging": true,
            "metrics": {"cpu_percent": 12.5, "cpu_count": 16, "cpu_state": "ok", "cpu_interval_seconds": 5.0,
                        "load_1": 1.25, "load_5": 1.5, "load_15": 1.75, "memory_used_bytes": 8_000_000_000_u64,
                        "memory_total_bytes": 32_000_000_000_u64, "memory_available_bytes": 24_000_000_000_u64,
                        "uptime_seconds": 3600.0, "platform": "linux", "sampled_at": 1787840000.5},
            "desktop_presence": {"idle_seconds": 12.0, "locked": false, "locked_seconds": 0.0}
        });
        let machine = MachineMeta::project(&meta);
        assert_eq!(machine.host.as_deref(), Some("workstation"));
        assert_eq!(machine.platform.as_deref(), Some("linux"));
        assert_eq!(machine.ok, Some(true));
        assert_eq!(machine.metrics.cpu_count, Some(16));
        assert_eq!(machine.metrics.cpu_state.as_deref(), Some("ok"));
        assert_eq!(machine.metrics.sampled_at, Some(1787840000.5));
        assert_eq!(machine.metrics.memory_total_bytes, Some(32_000_000_000));
        assert_eq!(machine.presence.locked, Some(false));
        let value = serde_json::to_value(&machine).unwrap();
        assert!(value.get("peers").is_none() && value.get("projects").is_none());
        assert_eq!(value["metrics"]["load_15"], 1.75);
        // An older host without metrics or presence, and mistyped fields.
        let older = MachineMeta::project(&json!({"host": "studio", "ok": "yes",
            "metrics": {"cpu_count": 8.0, "cpu_percent": "12", "memory_used_bytes": -1}}));
        assert_eq!(older.host.as_deref(), Some("studio"));
        assert_eq!(older.ok, None);
        assert_eq!(older.metrics.cpu_count, Some(8));
        assert_eq!(older.metrics.cpu_percent, None);
        assert_eq!(older.metrics.memory_used_bytes, None);
        assert_eq!(older.presence, Presence::default());
        assert_eq!(MachineMeta::project(&Value::Null), MachineMeta::default());
    }
}
