//! Pending provider changes are evidence, never calls or migrated consent.
use super::{bounded_json, QtSnapshot, QtValue, SourceError, SourceResult, SourceStream};
use crate::{settings, speech_settings};
use serde_json::{json, Value};
use zerus_desktop_migration::{sha256, SourceRecord, MAX_SNAPSHOT_BYTES};

fn body(raw: Option<&str>) -> SourceResult<Value> {
    let record = raw
        .filter(|text| !text.is_empty())
        .map(|text| {
            if text.len() as u64 > MAX_SNAPSHOT_BYTES {
                return Err(SourceError::TooLarge);
            }
            let record =
                speech_settings::decode_pending(text).ok_or(SourceError::UnsupportedSource)?;
            if !record
                .get("action")
                .and_then(Value::as_str)
                .is_some_and(|action| {
                    ["profile_upsert", "profile_remove", "profile_test"].contains(&action)
                })
                || !record
                    .get("host")
                    .and_then(Value::as_str)
                    .is_some_and(|host| host.len() <= 8192 && !host.chars().any(char::is_control))
                || !record
                    .get("computer")
                    .and_then(Value::as_str)
                    .is_some_and(|name| {
                        !name.is_empty()
                            && name.len() <= 8192
                            && !name.chars().any(char::is_control)
                    })
            {
                return Err(SourceError::UnsupportedSource);
            }
            Ok(record)
        })
        .transpose()?;
    let body = json!({"kind":"provider-operation","key":speech_settings::PENDING_KEY,
        "present":raw.is_some(),"cleared":raw==Some(""),"record":record});
    bounded_json(&body, MAX_SNAPSHOT_BYTES as usize)?;
    Ok(body)
}

/// Read the two existing settings snapshots before returning either stream.
/// These operation keys were already switch-seeded; this never re-seeds them.
/// Attach each existing reader's fresh identity/hash recheck before publication.
pub fn provider_operations(
    qt: &QtSnapshot,
    next_settings: &[u8],
) -> SourceResult<[SourceStream; 2]> {
    if !["qt-ini", "qt-plist", "qt-defaults"].contains(&qt.source) {
        return Err(SourceError::UnsupportedSource);
    }
    let raw = qt
        .values
        .get(speech_settings::PENDING_KEY)
        .map(|value| match value {
            QtValue::Text(text) => Ok(text.as_str()),
            QtValue::Bytes(bytes) => {
                std::str::from_utf8(bytes).map_err(|_| SourceError::UnsupportedSource)
            }
            _ => Err(SourceError::UnsupportedSource),
        })
        .transpose()?;
    let qt_body = body(raw)?;
    let next = settings::snapshot_values(next_settings).map_err(|error| match error {
        settings::SnapshotError::TooLarge => SourceError::TooLarge,
        settings::SnapshotError::Unsupported => SourceError::UnsupportedSource,
    })?;
    let raw = next
        .get(speech_settings::PENDING_KEY)
        .map(|value| value.as_str().ok_or(SourceError::UnsupportedSource))
        .transpose()?;
    let next_body = body(raw)?;
    let identities = [
        SourceRecord {
            family: "qt-provider-operation".into(),
            schema: 1,
            format: qt.source.into(),
            sha256: qt.sha256.clone(),
            build: None,
        },
        SourceRecord {
            family: "next-provider-operation".into(),
            schema: 1,
            format: "settings-json-v1".into(),
            sha256: sha256(next_settings),
            build: None,
        },
    ];
    for source in &identities {
        source
            .validate()
            .map_err(|_| SourceError::InvalidMetadata)?;
    }
    let [qt_identity, next_identity] = identities;
    Ok([
        SourceStream::new(qt_identity, std::iter::once(Ok(qt_body))),
        SourceStream::new(next_identity, std::iter::once(Ok(next_body))),
    ])
}

#[cfg(test)]
mod tests;
