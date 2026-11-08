use super::*;
use crate::{desktop::ui_requests::Request, Target};
use serde_json::json;

fn intent() -> Intent {
    let request_id = "11111111-1111-4111-8111-111111111111".to_owned();
    let capture = Capture {
        target: Target {
            computer: String::new(),
            name: "codex/example/source".into(),
            run_id: "run-1".into(),
            conversation_id: "conversation-1".into(),
        },
        machine: "22222222-2222-4222-8222-222222222222".into(),
        root: "/srv/example/work".into(),
        common_dir: "/srv/example/main/.git".into(),
        owner_id: "original-owner".into(),
    };
    let original =
        String::from_utf8(capture.invocation(&request_id).unwrap().stdin.unwrap()).unwrap();
    Intent {
        scope: Scope {
            configuration: "a".repeat(64),
            service_epoch: "33333333-3333-4333-8333-333333333333".into(),
        },
        confirmation_id: "55555555-5555-4555-8555-555555555555".into(),
        choice: Choice::RemoveAfterTerminate,
        request_id,
        capture,
        original,
    }
}

#[test]
fn retention_binds_the_original_capture_and_planned_terminate_without_authority() {
    let original = intent();
    assert!(original.valid());
    assert_eq!(
        serde_json::from_slice::<Intent>(&serde_json::to_vec(&original).unwrap()).unwrap(),
        original
    );
    for field in [
        "request_id",
        "action",
        "expected_run_id",
        "expected_conversation_id",
    ] {
        let mut altered = original.clone();
        let mut value: serde_json::Value = serde_json::from_str(&altered.original).unwrap();
        value[field] = json!("replacement");
        altered.original = value.to_string();
        assert!(!altered.valid());
    }
    let mut next_epoch = original.clone();
    next_epoch.scope.service_epoch = "44444444-4444-4444-8444-444444444444".into();
    assert_eq!(next_epoch.fingerprint(), original.fingerprint());
    let mut changed = original.clone();
    changed.capture.owner_id = "replacement".into();
    assert_ne!(changed.fingerprint(), original.fingerprint());
    assert!(!format!("{original:?}").contains("/srv/example"));
    assert!(!format!("{original:?}").contains("conversation-1"));
}

#[test]
fn future_nested_fields_duplicate_original_fields_and_limits_refuse() {
    let original = intent();
    for field in ["future", "capture", "target"] {
        let mut value = serde_json::to_value(&original).unwrap();
        if field == "future" {
            value["future"] = json!(true);
        } else if field == "capture" {
            value["capture"]["future"] = json!(true);
        } else {
            value["capture"]["target"]["future"] = json!(true);
        }
        assert!(serde_json::from_value::<Intent>(value).is_err());
    }
    let mut changed = original.clone();
    changed.original = changed
        .original
        .replacen("{", "{\"action\":\"terminate\",", 1);
    assert!(!changed.valid());
    let mut changed = original.clone();
    changed.original.push_str(&" ".repeat(INPUT_BYTES));
    assert!(!changed.valid());
    let mut changed = original.clone();
    changed.scope.service_epoch = "00000000-0000-0000-0000-000000000000".into();
    assert!(!changed.valid());
    let mut changed = original.clone();
    changed.capture.machine.clear();
    assert!(!changed.valid());
    let mut changed = original.clone();
    changed.capture.root = "relative".into();
    assert!(!changed.valid());
    let mut legacy = original;
    legacy.capture.owner_id.clear();
    assert!(
        legacy.valid(),
        "missing ownership stays missing data, never invented"
    );
}

#[test]
fn typed_request_has_its_own_closed_identity_and_status_exposes_only_hashes() {
    let intent = intent();
    let request = Request::TerminationOutcome {
        intent: Box::new(intent.clone()),
    };
    assert_eq!(request.id(), intent.request_id);
    assert_eq!(request.operation(), "termination_outcome");
    assert_eq!(
        serde_json::from_slice::<Request>(&serde_json::to_vec(&request).unwrap()).unwrap(),
        request
    );
    assert_eq!(
        serde_json::from_value::<Request>(serde_json::to_value(&request).unwrap()).unwrap(),
        request
    );
    let status = Status {
        abi: ABI.into(),
        configuration: intent.scope.configuration.clone(),
        confirmation_id: intent.confirmation_id.clone(),
        choice: intent.choice,
        original_request_id: intent.request_id.clone(),
        original_service_epoch: intent.scope.service_epoch.clone(),
        fingerprint: intent.fingerprint().unwrap(),
        capture_sha256: hash(&serde_json::to_vec(&intent.capture).unwrap()),
        request_sha256: hash(intent.original.as_bytes()),
    };
    assert!(status.matches(&intent));
    let serialized = serde_json::to_string(&status).unwrap();
    assert!(!serialized.contains("/srv/example"));
    assert!(!serialized.contains("original-owner"));
    let mut changed = status;
    changed.request_sha256 = "0".repeat(64);
    assert!(!changed.matches(&intent));
}
