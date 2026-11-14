//! The UI's membership input is emitted by the real CLI and ops writers.
use super::harness::*;
use crate::teams::cli::actor::Caller;
use serde_json::{json, Value};

fn payload() -> Value {
    let h = seeded();
    h.bind("m2", "launch_id:00000000-0000-4000-8000-000000000032");
    h.bind("m3", "launch_id:00000000-0000-4000-8000-000000000033");
    let read = |command: &str| {
        let (reply, exit) = h.cli(&Caller::Person, command, "");
        assert_eq!(exit, 0, "{reply}");
        reply
    };
    json!({"list":read("list"),"status":read(&format!("status --team {TEAM}")),"queue":read(&format!("queue --team {TEAM}"))})
}

#[test]
fn session_group_fixture_is_the_cli_writers_payload() {
    let expected: Value = serde_json::from_str(include_str!(
        "../../../../crates/team-plan/fixtures/cli-session-groups.json"
    ))
    .unwrap();
    assert_eq!(payload(), expected);
}

#[test]
#[ignore = "regenerates the UI's real CLI fixture when its shape changes"]
fn regenerate_session_group_fixture() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("crates/team-plan/fixtures/cli-session-groups.json");
    std::fs::write(
        path,
        format!("{}\n", serde_json::to_string_pretty(&payload()).unwrap()),
    )
    .unwrap();
}
