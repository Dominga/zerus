//! Cached shared recovery policy is historical evidence, never consent or a
//! replacement for the service's authoritative policy/current causal proof.
use super::{bounded_json, QtSnapshot, QtValue, SourceError, SourceResult, SourceStream};
use crate::{recovery_settings, settings};
use serde_json::{json, Value};
use zerus_desktop_migration::{sha256, SourceRecord, MAX_SNAPSHOT_BYTES};

const KEY: &str = recovery_settings::SHARED_POLICY_KEY;

fn body(text: Option<&str>) -> SourceResult<Value> {
    let record = match text {
        None | Some("") => Value::Null,
        Some(text) => {
            if text.len() > MAX_SNAPSHOT_BYTES as usize {
                return Err(SourceError::TooLarge);
            }
            if let Some(record) = recovery_settings::decode_cached(text) {
                Value::Object(record)
            } else {
                let value: Value =
                    serde_json::from_str(text).map_err(|_| SourceError::UnsupportedSource)?;
                if !value.as_object().is_some_and(|v| v.is_empty()) {
                    return Err(SourceError::UnsupportedSource);
                }
                value
            }
        }
    };
    let cleared =
        text.is_some() && (record.is_null() || record.as_object().is_some_and(|v| v.is_empty()));
    let value = json!({"kind":"recovery-policy-cache","key":KEY,"present":text.is_some(),"cleared":cleared,"record":record});
    bounded_json(&value, MAX_SNAPSHOT_BYTES as usize)?;
    Ok(value)
}

/// Both caches must decode before either stream is returned. Existing source
/// readers supply bounded bytes and attach independent fresh source rechecks.
pub fn recovery_policy_caches(
    qt: &QtSnapshot,
    next_settings: &[u8],
) -> SourceResult<[SourceStream; 2]> {
    if !["qt-ini", "qt-plist", "qt-defaults"].contains(&qt.source) {
        return Err(SourceError::UnsupportedSource);
    }
    let text = qt
        .values
        .get(KEY)
        .map(|v| match v {
            QtValue::Text(v) => Ok(v.as_str()),
            QtValue::Bytes(v) => std::str::from_utf8(v).map_err(|_| SourceError::UnsupportedSource),
            _ => Err(SourceError::UnsupportedSource),
        })
        .transpose()?;
    let qt_body = body(text)?;
    let next = settings::snapshot_values(next_settings).map_err(|reason| match reason {
        settings::SnapshotError::TooLarge => SourceError::TooLarge,
        settings::SnapshotError::Unsupported => SourceError::UnsupportedSource,
    })?;
    let text = next
        .get(KEY)
        .map(|v| v.as_str().ok_or(SourceError::UnsupportedSource))
        .transpose()?;
    let next_body = body(text)?;
    let sources = [
        SourceRecord {
            family: "qt-recovery-policy-cache".into(),
            schema: 1,
            format: qt.source.into(),
            sha256: qt.sha256.clone(),
            build: None,
        },
        SourceRecord {
            family: "next-recovery-policy-cache".into(),
            schema: 1,
            format: "settings-json-v1".into(),
            sha256: sha256(next_settings),
            build: None,
        },
    ];
    for source in &sources {
        source
            .validate()
            .map_err(|_| SourceError::InvalidMetadata)?;
    }
    let [qt, next] = sources;
    Ok([
        SourceStream::new(qt, std::iter::once(Ok(qt_body))),
        SourceStream::new(next, std::iter::once(Ok(next_body))),
    ])
}

#[cfg(test)]
mod tests;
