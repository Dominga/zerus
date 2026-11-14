//! The frontend fixture is serialized from the core's own view types.
use super::*;

fn payload() -> serde_json::Value {
    let raw: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../crates/team-plan/fixtures/cli-session-groups.json"
    ))
    .unwrap();
    serde_json::json!({"list":list(&raw["list"]),"team":team(&raw["status"], &raw["queue"])})
}

#[test]
fn session_groups_are_the_core_view_types() {
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../src/ui/sessions/fixtures/agent-team-views.json"
    ))
    .unwrap();
    assert_eq!(payload(), expected);
}

#[test]
#[ignore = "regenerates the frontend fixture from the core's view writers"]
fn regenerate_session_group_views() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../src/ui/sessions/fixtures/agent-team-views.json");
    std::fs::write(
        path,
        format!("{}\n", serde_json::to_string_pretty(&payload()).unwrap()),
    )
    .unwrap();
}
