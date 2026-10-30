//! Existing tray.conf text is evidence, never commands or selected defaults.
use super::{bounded_json, SourceError, SourceResult, SourceStream};
use crate::menubar::tray_conf_value;
use serde_json::{json, Map, Value};
use zerus_desktop_migration::{sha256, SourceRecord, MAX_SNAPSHOT_BYTES};

const MAX_CONFIG: usize = 64 << 10;
const KEYS: [&str; 6] = [
    "hgs",
    "local-poll",
    "peer-poll",
    "terminal",
    "icon-color",
    "open-mode",
];

/// The caller pins the actual configured file with its existing source reader,
/// then freshly rechecks exact file identity/hash after the current drain.
/// Missing files are not invented as an empty file; empty bytes mean a real
/// empty source. No runtime preference or CLI/service is changed here.
pub fn tray_config(bytes: &[u8]) -> SourceResult<SourceStream> {
    if bytes.len() > MAX_CONFIG {
        return Err(SourceError::TooLarge);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| SourceError::UnsupportedSource)?;
    let parsed: Map<String, Value> = KEYS
        .into_iter()
        .filter_map(|key| {
            tray_conf_value(text, key).map(|value| (key.into(), Value::String(value)))
        })
        .collect();
    let body = json!({"kind":"tray-config","text":text,"parsed":parsed});
    bounded_json(&body, MAX_SNAPSHOT_BYTES as usize)?;
    Ok(SourceStream::new(
        SourceRecord {
            family: "tray-config".into(),
            schema: 1,
            format: "tray-conf-v1".into(),
            sha256: sha256(bytes),
            build: None,
        },
        std::iter::once(Ok(body)),
    ))
}

#[cfg(test)]
mod tests;
