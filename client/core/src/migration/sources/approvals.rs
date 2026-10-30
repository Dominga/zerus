//! Approval choices are historical evidence only. Both stores must decode
//! successfully before either stream is returned. No setting, reservation or
//! service policy is written, and no former choice carries consent across G3.
use super::{bounded_json, QtSnapshot, QtValue, SourceError, SourceResult, SourceStream};
use crate::settings::{self, QT_AUTOMATIC_APPROVALS};
use serde_json::{json, Map, Value};
use zerus_desktop_migration::{sha256, SourceRecord, MAX_SNAPSHOT_BYTES};

const ENABLED: &str = "automaticApprovals/enabled";

/// Inspect the Qt choice and interim Next policy together. Callers use their
/// existing readers, then attach a fresh recheck to each returned stream.
/// An absent enabled key is reported as absent, not as an affirmative choice.
/// The old qtEnabledAtSwitch notice is distinct from Next's own enabled policy.
pub fn approval_policies(qt: &QtSnapshot, next_settings: &[u8]) -> SourceResult<[SourceStream; 2]> {
    if !["qt-ini", "qt-plist", "qt-defaults"].contains(&qt.source) {
        return Err(SourceError::UnsupportedSource);
    }
    let qt_choice = qt
        .values
        .get(ENABLED)
        .map(|value| {
            let enabled = settings::checked_qt_flag(value).ok_or(SourceError::UnsupportedSource)?;
            let raw = match value {
                QtValue::Bool(v) => json!(v),
                QtValue::Int(v) => json!(v),
                QtValue::Double(v) => json!(v),
                QtValue::Text(v) => json!(v),
                _ => return Err(SourceError::UnsupportedSource),
            };
            Ok((enabled, raw))
        })
        .transpose()?;
    let next = settings::snapshot_values(next_settings).map_err(|reason| match reason {
        settings::SnapshotError::TooLarge => SourceError::TooLarge,
        settings::SnapshotError::Unsupported => SourceError::UnsupportedSource,
    })?;
    let flag = |key: &str| -> SourceResult<Option<bool>> {
        next.get(key)
            .map(|value| value.as_bool().ok_or(SourceError::UnsupportedSource))
            .transpose()
    };
    let next_choice = flag(ENABLED)?;
    let qt_at_switch = flag(QT_AUTOMATIC_APPROVALS)?;
    // Preserve future policy fields privately without treating them as known
    // opt-ins. Other settings remain in their own explicitly scoped families.
    let originals: Map<String, Value> = next
        .into_iter()
        .filter(|(key, _)| key.starts_with("automaticApprovals/"))
        .collect();
    let qt_body = json!({"kind":"approval-policy","previous_enabled":qt_choice.as_ref().map(|v|v.0),
        "original":qt_choice.map(|v|v.1),"automatic_approvals":false});
    let next_body = json!({"kind":"approval-policy","previous_enabled":next_choice,
        "qt_enabled_at_switch":qt_at_switch,"original":originals,"automatic_approvals":false});
    bounded_json(&qt_body, MAX_SNAPSHOT_BYTES as usize)?;
    bounded_json(&next_body, MAX_SNAPSHOT_BYTES as usize)?;
    let identities = [
        SourceRecord {
            family: "qt-approval-policy".into(),
            schema: 1,
            format: qt.source.into(),
            sha256: qt.sha256.clone(),
            build: None,
        },
        SourceRecord {
            family: "next-approval-policy".into(),
            schema: 1,
            format: "settings-json-v1".into(),
            sha256: sha256(next_settings),
            build: None,
        },
    ];
    for identity in &identities {
        identity
            .validate()
            .map_err(|_| SourceError::InvalidMetadata)?;
    }
    let [qt_identity, next_identity] = identities;
    Ok([
        SourceStream::new(qt_identity, std::iter::once(Ok(qt_body))),
        SourceStream::new(next_identity, std::iter::once(Ok(next_body))),
    ])
}
