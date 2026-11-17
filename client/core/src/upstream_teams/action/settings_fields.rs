use super::types::*;
use super::wire::{classify, prepare};
use crate::actions::Exit;
use serde_json::{json, Value};

fn call() -> TeamCall {
    TeamCall {
        team: "00000000-0000-4000-8000-00000000beef".into(),
        team_name: "Settings fixture".into(),
        action: TeamAction::Settings {
            settings: Default::default(),
        },
        expected_revision: Some(1),
        request_id: "00000000-0000-4000-8000-000000000001".into(),
        confirmed: false,
    }
}

#[test]
fn settings_preflight_keeps_the_shared_validator_field() {
    let mut call = call();
    if let TeamAction::Settings { settings } = &mut call.action {
        settings.caps.max_open_questions = 0;
    }
    let outcome = serde_json::to_value(prepare(&call).unwrap_err()).unwrap();
    assert_eq!(outcome["field"], "settings.caps.max_open_questions");
    assert_eq!(outcome["code"], "over_cap");
}

#[test]
fn synthetic_wire_refusals_keep_bounded_field_paths_and_legacy_codes() {
    for exit in [0, 1] {
        for field in [
            json!("settings.caps.max_members"),
            json!("settings.timers.report_timeout_seconds"),
            Value::Null,
            json!(42),
            json!(["caps"]),
            json!("settings..bad"),
            json!("x".repeat(200)),
        ] {
            let body = json!({"ok": false, "error": {"code": "over_cap", "message": "Cap refusal", "field": field}});
            let outcome = classify(
                &call(),
                Exit::Code {
                    code: exit,
                    stdout: body.to_string().into_bytes(),
                    stderr: String::new(),
                },
            );
            let outcome = serde_json::to_value(outcome).unwrap();
            assert_eq!(outcome["outcome"], "failed");
            assert_eq!(outcome["code"], "over_cap");
            assert_eq!(outcome["detail"], "Cap refusal");
            let valid = field
                .as_str()
                .is_some_and(|path| path.starts_with("settings.") && !path.contains(".."));
            if valid {
                assert_eq!(outcome["field"], field);
            } else {
                assert!(outcome.get("field").is_none());
            }
        }
    }
}

#[test]
fn product_written_refusals_keep_their_field_in_the_desktop_outcome() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/upstream_teams/fixtures/settings-refusals.json");
    let rows: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    for (name, row) in rows["native"].as_object().unwrap() {
        let outcome = classify(
            &call(),
            Exit::Code {
                code: 1,
                stdout: row.to_string().into_bytes(),
                stderr: String::new(),
            },
        );
        let outcome = serde_json::to_value(outcome).unwrap();
        assert_eq!(outcome, rows["outcomes"][name]);
        assert_eq!(outcome["outcome"], "failed");
        assert_eq!(outcome["field"], row["error"]["field"]);
        assert_eq!(outcome["detail"], row["error"]["message"]);
    }
}
