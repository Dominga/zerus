//! The action version is independent of the replicated task document's schema.
use serde_json::{json, Value};
use zerus_relay::protocol;

fn send() -> Value {
    json!({"schema":2,"project":"project-1","task":"0b9d6c55-2f5e-4c39-9a51-0d6f3c2a7e11",
        "session":"codex/old-label","session_id":"launch_id:original"})
}

#[test]
fn task_identity_requests_require_the_complete_exact_identity() {
    protocol::task_payload("task_send", &send()).unwrap();
    for value in [
        Value::Null,
        json!(""),
        json!("  "),
        json!(7),
        json!("run_id:a\nb"),
        json!("x".repeat(1025)),
    ] {
        let mut bad = send();
        bad["session_id"] = value;
        assert!(protocol::task_payload("task_send", &bad).is_err(), "{bad}");
    }
    let mut missing = send();
    missing.as_object_mut().unwrap().remove("session_id");
    assert!(protocol::task_payload("task_send", &missing).is_err());
    let mut legacy = send();
    legacy["schema"] = json!(1);
    assert!(protocol::task_payload("task_send", &legacy).is_err());
    legacy.as_object_mut().unwrap().remove("session_id");
    protocol::task_payload("task_send", &legacy).unwrap();
    legacy["schema"] = json!(3);
    assert!(protocol::task_payload("task_send", &legacy).is_err());
    for schema in [0, 3] {
        let mut bad = send();
        bad["schema"] = json!(schema);
        assert!(protocol::task_payload("task_send", &bad).is_err());
    }
    let mut extra = send();
    extra["extra"] = json!(true);
    assert!(protocol::task_payload("task_send", &extra).is_err());
}

#[test]
fn task_identity_done_filter_is_bound_and_legacy_stays_unchanged() {
    let mut request = json!({"schema":2,"project":"project-1","limit":50,
        "session":{"machine":"node-1","session":"codex/old-label","session_id":"run_id:original"}});
    protocol::task_payload("task_done", &request).unwrap();
    for value in [Value::Null, json!(""), json!("  "), json!(7)] {
        let mut bad = request.clone();
        bad["session"]["session_id"] = value;
        assert!(protocol::task_payload("task_done", &bad).is_err());
    }
    let mut missing = request.clone();
    missing.as_object_mut().unwrap().remove("session");
    assert!(protocol::task_payload("task_done", &missing).is_err());
    request["schema"] = json!(1);
    assert!(protocol::task_payload("task_done", &request).is_err());
    request["session"]
        .as_object_mut()
        .unwrap()
        .remove("session_id");
    protocol::task_payload("task_done", &request).unwrap();
    request.as_object_mut().unwrap().remove("session");
    protocol::task_payload("task_done", &request).unwrap();
    assert!(protocol::task_payload(
        "task_cancel",
        &json!({"schema":2,"project":"project-1",
        "task":"0b9d6c55-2f5e-4c39-9a51-0d6f3c2a7e11"})
    )
    .is_err());
}
