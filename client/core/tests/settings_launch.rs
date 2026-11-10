//! New session's launch choices and their defaults.
use serde_json::json;
use zerus_client_core::settings::{effective, find};

const KEYS: &[&str] = &[
    "workspace/launchAgent",
    "workspace/launchOpenTerminal",
    "workspace/launchAccounts",
];

#[test]
fn launch_choices_have_qts_defaults() {
    for (name, value) in KEYS.iter().zip([json!(""), json!(false), json!("")]) {
        let key = find(name).unwrap();
        assert_eq!(effective(key.kind, None), value);
    }
}
