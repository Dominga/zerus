//! The shared supervised-child attention fixture (zerus-sa9qn.3): each
//! listing row's `quiet` as the desktop core projects it.
use serde_json::Value;
use zerus_client_core::view;

#[test]
fn every_fixture_row_projects_its_expected_quiet_attention() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/supervised-attention.json");
    let fixture: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(fixture["schema"], 1);
    let cases = fixture["cases"].as_array().unwrap();
    assert!(cases.len() >= 10);
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let row = view::row(&case["row"]);
        assert_eq!(row.quiet, case["quiet"].as_bool().unwrap(), "{name}");
        assert_eq!(
            serde_json::to_value(&row).unwrap().get("quiet").is_some(),
            row.quiet,
            "{name}: serialized only when set"
        );
    }
}
