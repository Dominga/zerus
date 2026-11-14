//! Reconstructed, anonymized startup facts from the affected live member.
//! The result fixture is emitted by the same CatalogState projection as reads.
use serde_json::{json, Value};
use zerus_client_core::{CatalogState, ComputerId};

fn payload() -> Value {
    let state = CatalogState::new();
    let raw = json!({
        "name":"codex/example/member", "agent":"codex", "tracked":true,
        "runtime_state":"live", "process_state":"running",
        "activity":"unknown", "activity_summary":"Status unavailable", "phase":"unknown",
        "conversation_id":null, "conversation_state":"unknown",
        "launch_id":"00000000-0000-4000-8000-000000000031",
        "run_id":"00000000-0000-4000-8000-000000000032",
        "session_id":"00000000-0000-4000-8000-000000000033",
        "process_work":{"count":null,"started_at":null,"command":""}
    });
    let mut ready = raw.clone();
    ready["activity"] = json!("idle");
    ready["phase"] = json!("idle");
    ready["process_work"]["count"] = json!(0);
    let mut background = raw.clone();
    background["process_work"]["count"] = json!(1);
    json!({
        "unknown":state.project_row(&ComputerId::local(), &raw),
        "ready":state.project_row(&ComputerId::local(), &ready),
        "background":state.project_row(&ComputerId::local(), &background)
    })
}

#[test]
fn startup_facts_are_serialized_by_the_real_core_projection() {
    let projected = payload();
    assert_eq!(projected["unknown"]["status"], "ready");
    assert_eq!(projected["unknown"]["caption"], "Status unknown");
    assert_eq!(projected["unknown"]["background"], -1);
    assert_eq!(projected["unknown"]["conversation_id"], Value::Null);
    assert_eq!(projected["unknown"]["process_state"], "running");
    assert_eq!(projected["ready"]["background"], 0);
    assert_eq!(projected["background"]["background"], 1);
    let pinned: Value = serde_json::from_str(include_str!(
        "../../src/ui/sessions/fixtures/team-session-presentation.json"
    ))
    .unwrap();
    assert_eq!(projected, pinned);
}

#[test]
#[ignore = "writes the frontend fixture from the real core projection"]
fn regenerate_team_session_presentation() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../src/ui/sessions/fixtures/team-session-presentation.json");
    std::fs::write(
        path,
        format!("{}\n", serde_json::to_string_pretty(&payload()).unwrap()),
    )
    .unwrap();
}
