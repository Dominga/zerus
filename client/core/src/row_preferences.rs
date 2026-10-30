//! Qt-compatible identities for device-local row density. Names are used only
//! in Qt's explicit conversation-less fallback, never to identify another run.
use crate::settings::qt::QtValue;
use crate::ViewRow;
use serde_json::{json, Value};
use zerus_desktop_migration::sha256;

pub const KEY: &str = "workspace/collapsedSessionRows";
pub const MAX_ROWS: usize = 50_000;

/// Qt's sorted packed 32-byte hashes, preserved without decoding or guessing
/// which unloaded conversation they belong to.
pub fn from_qt(value: &QtValue) -> Option<Value> {
    let QtValue::Bytes(bytes) = value else {
        return None;
    };
    if bytes.len() > MAX_ROWS * 32 || bytes.len() % 32 != 0 {
        return None;
    }
    let keys = bytes
        .chunks_exact(32)
        .map(|key| key.iter().map(|b| format!("{b:02x}")).collect::<String>())
        .collect::<std::collections::BTreeSet<_>>();
    Some(json!(keys))
}

pub fn valid(value: &Value) -> bool {
    value.as_array().is_some_and(|keys| {
        keys.len() <= MAX_ROWS
            && keys.iter().all(|key| {
                key.as_str().is_some_and(|s| {
                    s.len() == 64
                        && s.bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                })
            })
    })
}

/// Exact and fallback hashes as SessionRowPreferences::key computes them.
/// `host` is the configured peer alias or the empty local identity, never the
/// displayed machine name. Raw `dsh` must remain `dsh`.
pub fn keys(host: &str, row: &ViewRow) -> Option<[String; 2]> {
    let cmd = &row.notify.cmd;
    if cmd.is_empty() {
        return None;
    }
    let archive = row.archive_id.as_deref().unwrap_or_default();
    let fallback = sha256(
        json!([host, cmd, archive, row.name, row.created])
            .to_string()
            .as_bytes(),
    );
    let exact = row
        .conversation_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|conversation| {
            sha256(
                json!([host, cmd, archive, conversation])
                    .to_string()
                    .as_bytes(),
            )
        })
        .unwrap_or_else(|| fallback.clone());
    Some([exact, fallback])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identities_follow_qt_exact_fallback_raw_command_and_archive_boundaries() {
        let mut row = crate::view::row(&json!({"name":"codex/sample/名","cmd":"dsh","created":7,
            "conversation_id":"conversation","archive_id":"archive"}));
        let hashes = keys("", &row).unwrap();
        assert_eq!(hashes[0], sha256(br#"["","dsh","archive","conversation"]"#));
        assert_eq!(
            hashes[1],
            sha256("[\"\",\"dsh\",\"archive\",\"codex/sample/名\",7]".as_bytes())
        );
        assert_ne!(hashes, keys("peer", &row).unwrap());
        row.name = "renamed".into();
        assert_eq!(hashes[0], keys("", &row).unwrap()[0]);
        assert_ne!(hashes[1], keys("", &row).unwrap()[1]);
        row.archive_id = None;
        assert_ne!(hashes[0], keys("", &row).unwrap()[0]);
        row.conversation_id = None;
        let fallback = keys("", &row).unwrap();
        assert_eq!(fallback[0], fallback[1]);
        assert!(keys("", &ViewRow::default()).is_none());
    }
    #[test]
    fn packed_keys_are_bounded_and_malformed_families_are_not_partially_decoded() {
        assert_eq!(
            from_qt(&QtValue::Bytes(vec![0xab; 32])).unwrap(),
            json!(["ab".repeat(32)])
        );
        assert!(from_qt(&QtValue::Bytes(vec![1; 33])).is_none());
        assert!(from_qt(&QtValue::Bytes(vec![1; MAX_ROWS * 32 + 32])).is_none());
        assert!(from_qt(&QtValue::Text("unrecognized".into())).is_none());
        assert!(!valid(&json!(["../wrong"])));
        assert!(!valid(&json!(["A".repeat(64)])));
        assert!(valid(&json!(["a".repeat(64)])));
    }
}
