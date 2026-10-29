#![cfg(unix)]
//! Windows admission policy exercised through the real durable connector journal.
mod common;
use common::{Scratch, EPOCH};
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use zerus_mobile_connector::{
    capabilities::{self, Flags},
    execute::Outcome,
    journal::Journal,
    native::{Call, Native, NativeResult, Output},
    runtime::{Connector, Settings},
};
struct External {
    detail: RefCell<Value>,
    writes: Cell<usize>,
}
impl Native for External {
    async fn call(&self, call: Call) -> NativeResult {
        match call.argv.first().map(String::as_str) {
            Some("inspect") => Ok(Output::Json(self.detail.borrow().clone())),
            Some("send") => {
                self.writes.set(self.writes.get() + 1);
                Ok(Output::Json(
                    json!({"status":"submitted","transport":"external_inbox","delivery_state":"queued","provider_delivery":"not_attempted"}),
                ))
            }
            Some("history") => Ok(Output::Json(
                json!({"events":[],"cloud_transcript_available":false}),
            )),
            other => panic!("external native control reached dispatch: {other:?}"),
        }
    }
}
fn detail() -> Value {
    json!({"name":"external/project/chat","run_id":"run","conversation_id":"conversation","agent":"external","cmd":"external","backend":"external-workstream","external_protocol":"cooperative-inbox-v1",
    "send_delivery":"cooperative_inbox","external_cloud_control":false,"client_context":{"schema_version":1,"kind":"chatgpt_chat","evidence":"explicit_registration"}})
}
fn connector(scratch: &Scratch, value: Value) -> Connector<External> {
    let c = Connector::new(
        External {
            detail: RefCell::new(value),
            writes: Cell::new(0),
        },
        Journal::open(&scratch.path("state"), "fixture", EPOCH).unwrap(),
        Settings {
            interval: 5.0,
            binding: "fixture".into(),
            default_timeout: 45.0,
            clock_scale: 1.0,
        },
        Box::new(|| EPOCH),
    );
    c.flags.set(
        Flags {
            history: true,
            lifecycle: true,
            terminal: true,
            ..Flags::default()
        }
        .windows_local(),
    );
    c.state
        .borrow_mut()
        .sessions
        .insert("external/project/chat".into());
    c
}
fn request(operation: &str, id: u32) -> Value {
    json!({"request_id":format!("00000000-0000-4000-8000-{id:012}"),"operation":operation,"session":"external/project/chat",
    "payload":{"request_id":format!("00000000-0000-4000-8000-{id:012}"),"expected_run_id":"run","expected_conversation_id":"conversation","text":"Cooperative instruction"}})
}
fn finished(value: Outcome) -> Value {
    match value {
        Outcome::Finished(v) => v,
        other => panic!("{other:?}"),
    }
}
#[test]
fn exact_projection_does_not_add_a_launch_provider() {
    assert!(capabilities::windows_session(&detail()));
    assert!(!capabilities::windows_provider(&json!("external")));
    for (field, value) in [
        ("external_protocol", json!("unknown")),
        ("external_cloud_control", json!(true)),
        ("backend", json!("terminal")),
    ] {
        let mut v = detail();
        v[field] = value;
        assert!(!capabilities::windows_session(&v));
    }
    let mut v = detail();
    v["client_context"]["schema_version"] = json!(true);
    assert!(!capabilities::windows_session(&v));
    for op in [
        "terminate",
        "answer",
        "terminal_input",
        "pause",
        "launch",
        "settings",
    ] {
        assert!(!capabilities::external_operation_allowed(op));
    }
}
#[tokio::test(flavor = "current_thread")]
async fn send_is_a_single_durable_cooperative_submission() {
    let scratch = Scratch::new("external-send");
    let c = connector(&scratch, detail());
    let req = request("send", 1);
    let result = finished(c.execute(req.clone()).await.unwrap());
    assert_eq!(result["state"], "completed", "{result}");
    assert_eq!(result["result"]["provider_delivery"], "not_attempted");
    assert_eq!(result["result"]["delivery_state"], "queued");
    assert_eq!(c.native.writes.get(), 1);
    assert_eq!(c.execute(req).await.unwrap(), Outcome::Replayed);
    assert_eq!(c.native.writes.get(), 1);
}
#[tokio::test(flavor = "current_thread")]
async fn stale_identity_and_native_controls_never_reach_a_write() {
    for (index, op) in ["send", "terminate", "interrupt", "answer", "terminal_input"]
        .iter()
        .enumerate()
    {
        let scratch = Scratch::new("external-denial");
        let mut d = detail();
        if *op == "send" {
            d["run_id"] = json!("replacement");
        }
        let c = connector(&scratch, d);
        let result = finished(c.execute(request(op, 10 + index as u32)).await.unwrap());
        assert_eq!(result["state"], "failed", "{result}");
        assert_eq!(c.native.writes.get(), 0);
    }
}
#[tokio::test(flavor = "current_thread")]
async fn unknown_external_schema_cannot_enter_as_native_codex() {
    let scratch = Scratch::new("external-schema");
    let mut d = detail();
    d["external_protocol"] = json!("v99");
    let c = connector(&scratch, d);
    let result = finished(c.execute(request("send", 50)).await.unwrap());
    assert_eq!(result["state"], "failed");
    assert_eq!(c.native.writes.get(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn linux_external_native_controls_are_refused_before_uncertain_delivery() {
    let scratch = Scratch::new("external-linux-denial");
    let c = connector(&scratch, detail());
    c.flags.set(Flags {
        history: true,
        lifecycle: true,
        terminal: true,
        ..Flags::default()
    });
    let mut req = request("interrupt", 80);
    req["payload"]["expected_turn_started"] = json!(0);
    let result = finished(c.execute(req).await.unwrap());
    assert_eq!(result["state"], "failed", "{result}");
    assert_eq!(c.native.writes.get(), 0);
}
