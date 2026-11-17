//! Pin the list projection from the root CLI's product-writer fixture.
use super::*;

fn payload() -> Value {
    let raw: Value = serde_json::from_str(include_str!(
        "../../../../../crates/team-plan/fixtures/cli-progress.json"
    ))
    .unwrap();
    serde_json::json!({ "running": list(&raw["running"]), "stuck": list(&raw["stuck"]) })
}

#[test]
fn list_progress_survives_the_real_core_projection() {
    let p = payload();
    assert_eq!(p["running"]["teams"][0]["progress"]["accepted"], 1);
    assert_eq!(p["running"]["teams"][0]["progress"]["tasks"], 3);
    assert_eq!(p["running"]["teams"][0]["progress"]["questions"], 1);
    assert_eq!(p["stuck"]["teams"][0]["progress"]["stuck"], true);
    let expected: Value = serde_json::from_str(include_str!(
        "../../../../src/ui/sessions/fixtures/team-progress.json"
    ))
    .unwrap();
    assert_eq!(p, expected);
}

#[test]
fn an_older_cli_without_progress_does_not_invent_a_strip() {
    assert!(list(&serde_json::json!({"rows":[{"id":"older"}]})).teams[0]
        .progress
        .is_none());
}

#[test]
#[ignore = "writes the frontend progress fixture from the real core view"]
fn regenerate_progress_views() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../src/ui/sessions/fixtures/team-progress.json");
    std::fs::write(
        path,
        format!("{}\n", serde_json::to_string_pretty(&payload()).unwrap()),
    )
    .unwrap();
}
