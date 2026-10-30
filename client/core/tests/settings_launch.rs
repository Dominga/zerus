//! The single settings seed reads New session choices from Qt, without
//! normalizing or rewriting Qt's compact launchAccounts JSON text.
use serde_json::json;
use zerus_client_core::settings::qt::{read_ini, read_plist};
use zerus_client_core::settings::{effective, find, seed_values};

const KEYS: &[&str] = &[
    "workspace/launchAgent",
    "workspace/launchOpenTerminal",
    "workspace/launchAccounts",
];

#[test]
fn launch_choices_have_qts_defaults() {
    for (name, value) in KEYS.iter().zip([json!(""), json!(false), json!("")]) {
        let key = find(name).unwrap();
        assert!(key.seeded);
        assert_eq!(effective(key.kind, None), value);
    }
}

#[test]
fn the_seed_reads_qsettings_quoted_bytearray_with_two_computers() {
    let data = br#"[workspace]
launchAgent=claude
launchOpenTerminal=true
launchAccounts="@ByteArray({\"\":{\"claude\":\"local\"},\"studio\":{\"codex\":\"remote\"}})"
"#;
    let seeded = seed_values(&read_ini(data), KEYS);
    assert_eq!(seeded[KEYS[0]], "claude");
    assert_eq!(seeded[KEYS[1]], true);
    assert_eq!(
        seeded[KEYS[2]],
        r#"{"":{"claude":"local"},"studio":{"codex":"remote"}}"#
    );
}

#[test]
fn the_seed_reads_plist_data_and_preserves_unparsable_account_text() {
    let accounts = r#"{"":{"claude":"local"},"studio":{"codex":"remote"}}"#;
    for text in [accounts, "unparsable accounts"] {
        let mut dict = plist::Dictionary::new();
        dict.insert(
            "workspace.launchAgent".into(),
            plist::Value::String("codex".into()),
        );
        dict.insert(
            "workspace.launchOpenTerminal".into(),
            plist::Value::Boolean(true),
        );
        dict.insert(
            "workspace.launchAccounts".into(),
            plist::Value::Data(text.as_bytes().to_vec()),
        );
        let mut bytes = Vec::new();
        plist::Value::Dictionary(dict)
            .to_writer_binary(&mut bytes)
            .unwrap();
        let seeded = seed_values(&read_plist(&bytes).unwrap(), KEYS);
        assert_eq!(seeded[KEYS[0]], "codex");
        assert_eq!(seeded[KEYS[1]], true);
        assert_eq!(seeded[KEYS[2]], text);
    }
}
