use super::*;
use serde_json::json;

fn raw() -> Value {
    json!({"log":[],"attachments":[],"execution":{
        "requirement":3,"attempts":1,"unknown":["11111111-1111-4111-8111-111111111111"],
        "unexplained":["private-old-entry"],"conflict":true,
        "current":{"id":"11111111-1111-4111-8111-111111111111","generation":2,
            "origin":"send","executor":{"machine":"example-machine","session":"codex/orchard/worker",
                "session_id":"run_id:example-run","host":"","resolved":true},
            "request_id":"example-request","requirement":3,"phase":"submitted","at":1234,
            "request":{"prompt":"PRIVATE FROZEN INPUT"},"events":["PRIVATE HISTORY"]}}})
}
fn view(raw: Value) -> Value {
    present(raw, |_, _| Ok(json!([]))).unwrap()
}

#[test]
fn inspector_keeps_only_the_bounded_typed_saved_execution() {
    let result = view(raw());
    assert_eq!(result["execution"]["current"]["generation"], 2);
    assert_eq!(result["execution"]["current"]["phase"], "submitted");
    assert_eq!(result["execution"]["has_unknown"], true);
    assert_eq!(result["execution"]["has_unexplained"], true);
    assert_eq!(result["execution"]["conflict"], true);
    assert!(result["execution"].get("unknown").is_none());
    assert!(result["execution"].get("unexplained").is_none());
    assert!(!result.to_string().contains("PRIVATE"));
    assert!(!result.to_string().contains("private-old-entry"));
}

#[test]
fn an_unavailable_or_future_execution_never_becomes_a_legacy_or_running_claim() {
    let mut future = raw();
    future["execution"]["current"]["phase"] = json!("future-phase");
    assert!(view(future).get("execution").is_none());
    assert!(view(json!({"log":[],"attachments":[]}))
        .get("execution")
        .is_none());
    let mut inconsistent = raw();
    inconsistent["execution"]["current"]["phase"] = json!("running");
    assert!(view(inconsistent).get("execution").is_none());
}
