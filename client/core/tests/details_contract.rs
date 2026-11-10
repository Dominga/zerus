//! The details the desktop's `inspect_session` returns to the web UI, exactly
//! as Tauri serializes them (serde_json). `client/src/store/forks.test.ts`
//! adopts forks from these values, so a field this projection drops or
//! reshapes fails a test on one side or the other.
use serde_json::{json, Value};
use std::path::PathBuf;
use zerus_client_core::actions::{Exit, Target};
use zerus_client_core::details::parse_details;

/// An `hgs inspect` answer for a candidate row of a pending fork.
fn inspection(conversation: &str, fields: Value) -> Value {
    let mut value = json!({
        "name": "codex/example/review-fork", "run_id": "fork-run", "conversation_id": conversation,
        "phase": "idle", "fork_supported": false, "fork_reason": "Wait for a completed reply before forking",
        "allowed_actions": ["terminate"], "action_reasons": {}, "events": [], "cwd": "/srv/example",
    });
    value
        .as_object_mut()
        .unwrap()
        .extend(fields.as_object().unwrap().clone());
    value
}

fn bridged_as(conversation: &str, fields: Value) -> Value {
    let target = Target {
        computer: "studio".into(),
        name: "codex/example/review-fork".into(),
        run_id: "fork-run".into(),
        conversation_id: conversation.into(),
    };
    let stdout = serde_json::to_vec(&inspection(conversation, fields)).unwrap();
    let exit = Exit::Code {
        code: 0,
        stdout,
        stderr: String::new(),
    };
    // What the `inspect_session` command returns, as its IPC answer.
    serde_json::to_value(parse_details(&target, exit)).unwrap()
}

fn bridged(fields: Value) -> Value {
    bridged_as("fork-conversation", fields)
}

fn fixture() -> Value {
    let parent = json!({"fork_parent_id": "source-conversation", "fork_source_name": "codex/example/review"});
    json!({
        // A Codex fork may have no conversation ID until its first message.
        "fork_before_first_message": bridged_as("", parent.clone()),
        "fork": bridged(parent),
        "not_a_fork": bridged(json!({"fork_parent_id": null, "fork_source_name": null})),
        "older_hgs": bridged(json!({})),
    })
}

fn fixture_file() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/inspect-details.json")
}

#[test]
fn the_bridged_inspection_details_are_current() {
    let current = serde_json::to_string_pretty(&fixture()).unwrap() + "\n";
    if std::env::var_os("ZERUS_UPDATE_FIXTURES").is_some() {
        std::fs::write(fixture_file(), &current).unwrap();
    }
    // Task #20: compared as text: a checkout that converts line ends (Git's
    // autocrlf on Windows) holds the same fixture with CRLF.
    let saved = std::fs::read_to_string(fixture_file()).unwrap().replace("\r\n", "\n");
    assert_eq!(saved, current);
    // The three fork states stay distinct in the bridged value.
    let value = fixture();
    assert_eq!(value["fork"]["fork_parent_id"], "source-conversation");
    assert_eq!(
        value["not_a_fork"].get("fork_parent_id"),
        Some(&Value::Null)
    );
    assert_eq!(value["older_hgs"].get("fork_parent_id"), None);
}
