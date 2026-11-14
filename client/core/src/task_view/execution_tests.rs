use super::*;
use serde_json::json;

const ATTEMPT: &str = "00000000-0000-4000-8000-000000000001";
const REQUEST: &str = "00000000-0000-4000-8000-000000000002";

fn attempt() -> Value {
    json!({"id":ATTEMPT,"generation":3,"origin":"send",
        "executor":{"machine":"computer","session":"codex/project/label",
            "session_id":"run_id:original","host":"studio","resolved":false},
        "request_id":REQUEST,"requirement":5,"phase":"submitted","at":1234})
}
fn task(current: Value) -> Value {
    json!({"uuid":"task","stage":"active","target":{"kind":"any"},
        "execution":{"requirement":7,"attempts":3,"current":current}})
}
fn shown(task: &Value) -> Value {
    serde_json::to_value(TaskItem::from_value(task)).unwrap()["execution"].clone()
}

#[test]
fn submitted_execution_survives_the_existing_task_reader_without_becoming_running() {
    let mut raw = task(attempt());
    // Stage/session status and project order cannot supply begin or queue evidence.
    raw["session"] = json!({"session":"another","status":"working"});
    let board = Board::from_listing(&json!({"tasks":[raw]}));
    let output = serde_json::to_value(&board.tasks[0]).unwrap();
    let execution = &output["execution"];
    assert_eq!(execution["requirement"], 7);
    assert_eq!(execution["attempts"], 3);
    assert_eq!(execution["current"]["id"], ATTEMPT);
    assert_eq!(execution["current"]["generation"], 3);
    assert_eq!(execution["current"]["request_id"], REQUEST);
    assert_eq!(execution["current"]["requirement"], 5);
    assert_eq!(execution["current"]["origin"], "send");
    assert_eq!(execution["current"]["phase"], "submitted");
    assert!(execution["current"]["began_at"].is_null());
    assert_eq!(execution["current"]["at"], 1234);
    assert_eq!(
        execution["current"]["executor"]["session_id"],
        "run_id:original"
    );
    assert_eq!(execution["current"]["executor"]["machine"], "computer");
    assert_eq!(execution["current"]["executor"]["host"], "studio");
    assert_eq!(execution["current"]["executor"]["resolved"], false);
    assert!(execution.get("queue").is_none());
}

#[test]
fn every_phase_and_origin_preserves_saved_evidence_and_exact_integer_times() {
    for phase in [
        "submitted",
        "unknown",
        "running",
        "paused",
        "ready",
        "cancelled",
        "not_started",
        "unconfirmed",
    ] {
        for origin in ["send", "start", "handoff", "reopen"] {
            let mut current = attempt();
            current["phase"] = json!(phase);
            current["origin"] = json!(origin);
            current["began_at"] = json!(9007199254740993_i64);
            current["progress_at"] = json!(9007199254740995_i64);
            let output = shown(&task(current.clone()));
            assert_eq!(output["current"]["phase"], phase);
            assert_eq!(output["current"]["origin"], origin);
            assert_eq!(output["current"]["began_at"], current["began_at"]);
            assert_eq!(output["current"]["progress_at"], current["progress_at"]);
        }
    }
}

#[test]
fn legacy_unconfirmed_and_absent_execution_do_not_invent_a_managed_attempt() {
    assert!(shown(&json!({"stage":"active"})).is_null());
    let legacy = json!({"generation":0,"origin":"send","requirement":0,
        "phase":"unconfirmed","at":0,"legacy":true});
    let mut raw = task(legacy);
    raw["execution"]["attempts"] = json!(0);
    let output = shown(&raw);
    assert_eq!(output["current"]["legacy"], true);
    assert_eq!(output["current"]["id"], "");
    assert_eq!(output["current"]["generation"], 0);
    assert_eq!(output["current"]["phase"], "unconfirmed");
    assert!(output["current"]["executor"].is_null());
    raw["execution"].as_object_mut().unwrap().remove("current");
    assert!(shown(&raw)["current"].is_null());
    assert_eq!(shown(&raw)["attempts"], 0);
}

#[test]
fn conflict_and_older_unknown_evidence_remain_bounded_and_never_leak_native_payloads() {
    let mut raw = task(attempt());
    raw["execution"]["conflict"] = json!(true);
    raw["execution"]["unknown"] = json!([ATTEMPT]);
    raw["execution"]["unexplained"] = json!(["history-entry"]);
    raw["execution"]["current"]["native"] = json!({"prompt":"private frozen input"});
    raw["execution"]["current"]["executor"]["token"] = json!("private token");
    let first = shown(&raw);
    assert_eq!(first["conflict"], true);
    assert_eq!(first["has_unknown"], true);
    assert_eq!(first["has_unexplained"], true);
    assert_eq!(first["current"]["phase"], "submitted");
    assert!(!first.to_string().contains("private"));
    raw["execution"]["unknown"] = json!(vec![ATTEMPT; 10000]);
    raw["execution"]["unexplained"] = json!(vec!["history-entry"; 10000]);
    assert_eq!(
        shown(&raw),
        first,
        "no history-sized arrays in the client DTO"
    );
    assert!(first.to_string().len() < 2048);
}

#[test]
fn future_malformed_and_overlong_execution_is_unavailable_without_changing_task_actions() {
    let mut variants = Vec::new();
    for (field, value) in [
        ("phase", json!("future")),
        ("origin", json!("future")),
        ("generation", json!(-1)),
        ("generation", json!(1.5)),
        ("generation", json!(0)),
        ("id", json!("x".repeat(513))),
        ("id", json!("not-a-uuid")),
        ("request_id", json!("x".repeat(513))),
        ("request_id", json!("receipt\nother")),
        ("at", json!(1.5)),
        ("began_at", json!("123")),
        ("legacy", json!("false")),
        ("executor", json!({"machine":5})),
    ] {
        let mut raw = task(attempt());
        raw["execution"]["current"][field] = value;
        variants.push(raw);
    }
    for (field, value) in [
        ("attempts", json!(-1)),
        ("attempts", json!(257)),
        ("requirement", json!(1.5)),
        ("current", json!([])),
        ("conflict", json!("false")),
        ("unknown", json!({})),
        ("unexplained", json!(false)),
    ] {
        let mut raw = task(attempt());
        raw["execution"][field] = value;
        variants.push(raw);
    }
    let mut missing = task(attempt());
    missing["execution"]["current"]
        .as_object_mut()
        .unwrap()
        .remove("phase");
    variants.push(missing);
    let mut unbegun = task(attempt());
    unbegun["execution"]["current"]["phase"] = json!("running");
    variants.push(unbegun);
    for raw in variants {
        let parsed = TaskItem::from_value(&raw);
        assert!(
            serde_json::to_value(&parsed).unwrap()["execution"].is_null(),
            "{raw}"
        );
        assert!(parsed.writable);
        assert_eq!(parsed.stage, "active");
    }
}

#[test]
fn typed_rows_round_trip_and_old_rows_need_no_execution_field() {
    let old = TaskItem::from_value(&json!({"stage":"todo","title":"Old peer"}));
    let old = serde_json::to_value(old).unwrap();
    assert!(old.get("execution").is_none());
    let restored: TaskItem = serde_json::from_value(old).unwrap();
    assert!(restored.execution.is_none());
    let current = TaskItem::from_value(&task(attempt()));
    let restored: TaskItem =
        serde_json::from_value(serde_json::to_value(&current).unwrap()).unwrap();
    assert_eq!(restored, current);
}
