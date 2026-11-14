//! A startup observation is independent of the renderer we requested. The public
//! GTK API exposes policy, but neither the composited layer tree nor the selected
//! backing buffer. Keep those unknown, including when the policy is ALWAYS.

use serde::Serialize;
use std::sync::OnceLock;

#[derive(Debug, Serialize)]
pub struct Observation {
    pub hardware_acceleration_policy: &'static str,
    pub composited_layers: &'static str,
    pub buffer_transport: &'static str,
    pub unavailable_reason: &'static str,
}

#[derive(Debug, Serialize)]
pub struct Override {
    pub name: &'static str,
    pub value: Option<&'static str>,
}

#[derive(Debug, Serialize)]
pub struct Snapshot {
    pub requested: serde_json::Value,
    pub observed: Observation,
    pub overrides: Vec<Override>,
}

impl Snapshot {
    pub fn capture(
        requested: serde_json::Value,
        policy: Option<&'static str>,
        mut env: impl FnMut(&str) -> Option<String>,
    ) -> Self {
        let overrides = [
            "ZERUS_CLIENT_RENDERER",
            "WEBKIT_DISABLE_DMABUF_RENDERER",
            "WEBKIT_DISABLE_COMPOSITING_MODE",
            "WEBKIT_DMABUF_RENDERER_FORCE_SHM",
            "__NV_DISABLE_EXPLICIT_SYNC",
        ]
        .into_iter()
        .map(|name| Override {
            name,
            // Arbitrary environment text is neither useful diagnostic evidence
            // nor safe log content. Preserve only recognized bounded values.
            value: env(name).map(|value| match (name, value.as_str()) {
                ("ZERUS_CLIENT_RENDERER", "auto") => "auto",
                ("ZERUS_CLIENT_RENDERER", "dmabuf") => "dmabuf",
                ("ZERUS_CLIENT_RENDERER", "shm") => "shm",
                (_, "0") => "0",
                (_, "1") => "1",
                _ => "other",
            }),
        })
        .collect();
        Self {
            requested,
            observed: Observation {
                hardware_acceleration_policy: policy.unwrap_or("unavailable"),
                composited_layers: "unavailable",
                buffer_transport: "unavailable",
                unavailable_reason: "not-exposed-by-public-webkitgtk-api",
            },
            overrides,
        }
    }
}

pub fn record_once(
    slot: &OnceLock<Snapshot>,
    read: impl FnOnce() -> Snapshot,
    log: impl FnOnce(&Snapshot),
) -> &Snapshot {
    slot.get_or_init(|| {
        let snapshot = read();
        log(&snapshot);
        snapshot
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn native_policy_is_not_the_request_and_never_proves_layers_or_transport() {
        for policy in [Some("always"), Some("never"), None] {
            let snapshot =
                Snapshot::capture(serde_json::json!({"renderer":"dmabuf"}), policy, |_| None);
            let value = serde_json::to_value(snapshot).unwrap();
            assert_eq!(value["requested"]["renderer"], "dmabuf");
            assert_eq!(
                value["observed"]["hardware_acceleration_policy"],
                policy.unwrap_or("unavailable")
            );
            assert_eq!(value["observed"]["composited_layers"], "unavailable");
            assert_eq!(value["observed"]["buffer_transport"], "unavailable");
        }
    }

    #[test]
    fn reloads_and_concurrent_readers_reuse_one_native_read_and_log() {
        let slot = OnceLock::new();
        let reads = AtomicUsize::new(0);
        let logs = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    let snapshot = record_once(
                        &slot,
                        || {
                            reads.fetch_add(1, Ordering::SeqCst);
                            Snapshot::capture(serde_json::Value::Null, Some("never"), |_| None)
                        },
                        |_| {
                            logs.fetch_add(1, Ordering::SeqCst);
                        },
                    );
                    assert_eq!(snapshot.observed.hardware_acceleration_policy, "never");
                });
            }
        });
        record_once(
            &slot,
            || panic!("reload reread the native state"),
            |_| panic!("reload logged twice"),
        );
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        assert_eq!(logs.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn overrides_are_bounded_and_missing_is_distinct_from_disabled() {
        let snapshot = Snapshot::capture(serde_json::Value::Null, None, |name| match name {
            "WEBKIT_DISABLE_DMABUF_RENDERER" => Some("0".into()),
            "WEBKIT_DISABLE_COMPOSITING_MODE" => Some("private\n".repeat(1000)),
            _ => None,
        });
        assert_eq!(snapshot.overrides.len(), 5);
        assert_eq!(snapshot.overrides[0].value, None);
        assert_eq!(snapshot.overrides[1].value, Some("0"));
        assert_eq!(snapshot.overrides[2].value, Some("other"));
    }
}
