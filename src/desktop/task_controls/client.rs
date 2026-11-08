//! Bounded local service client. It has no database or native-effect fallback.
//! A lost/malformed result after submission remains unknown, with its original
//! request ID. Neither probe nor status retries execute.
use super::wire::{Envelope, OP, VERSION};
use crate::{
    config::Config,
    desktop::{protocol, store::Location},
    tasks::controls::wire::Command,
};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::{
    io::BufReader,
    time::{Duration, Instant},
};
use zerus_desktop_ledger::task_controls::TASK_CONTROLS;

const REPLY_BYTES: usize = 80 << 10;
const TOTAL: Duration = Duration::from_secs(10);

/// No filesystem/transport access while off, even from an internal caller.
pub(super) fn request(config: &Config, command: Command) -> Result<Value> {
    if !TASK_CONTROLS {
        anyhow::bail!("Task controls are inactive. Nothing was attempted.");
    }
    let deadline = Instant::now() + TOTAL;
    let envelope = Envelope::new(command).map_err(anyhow::Error::msg)?;
    let location = Location::new(config)?;
    let computer = config
        .selves
        .first()
        .map(String::as_str)
        .unwrap_or("this computer");
    Ok(invoke(&envelope, computer, |op, body| {
        exchange(&location, op, body, deadline)
    }))
}

fn exchange(
    location: &Location,
    operation: &str,
    body: Option<Value>,
    deadline: Instant,
) -> Result<Option<Value>> {
    anyhow::ensure!(Instant::now() < deadline, "task-control request deadline");
    let Some(stream) = super::super::connection_until(location, operation, body, deadline)? else {
        return Ok(None);
    };
    let remaining = deadline.saturating_duration_since(Instant::now());
    anyhow::ensure!(!remaining.is_zero(), "task-control request deadline");
    stream.set_read_timeout(Some(remaining))?;
    let bytes = super::read_until(&mut BufReader::new(stream), REPLY_BYTES, deadline)?
        .context("task-control service disconnected")?;
    reply(operation, &bytes).map(Some)
}

fn reply(operation: &str, bytes: &[u8]) -> Result<Value> {
    let frame: Value = serde_json::from_slice(bytes)?;
    let kind = if operation == "status" {
        "status"
    } else {
        "response"
    };
    anyhow::ensure!(
        frame["abi"] == protocol::ABI && frame["t"] == kind,
        "task-control service refused the request"
    );
    Ok(frame["value"].clone())
}

fn update(computer: &str, id: &str) -> Value {
    json!({"schema":1,"request_id":id,"ok":false,"attempted":false,
        "code":"task_control_service_update_required","computer":computer,
        "error":format!("Update Zerus on {computer}: a compatible task-control service is not available. Nothing was attempted by this call.")})
}
fn unknown(computer: &str, envelope: &Envelope) -> Value {
    let id = &envelope.request_id;
    let read_only = matches!(envelope.command, Command::Plan(_) | Command::Status(_));
    let error = if read_only {
        format!("The task-control read on {computer} could not be confirmed. No task input was attempted by this read.")
    } else {
        format!("The task-control outcome on {computer} is unknown. Check status for the original request {id}; do not repeat its input.")
    };
    json!({"schema":1,"request_id":id,"ok":false,"attempted":!read_only,"read_only":read_only,
        "code":"task_control_outcome_unknown","computer":computer,"error":error})
}

fn invoke(
    envelope: &Envelope,
    computer: &str,
    mut exchange: impl FnMut(&str, Option<Value>) -> Result<Option<Value>>,
) -> Value {
    let ready = match exchange("status", None) {
        Ok(Some(status)) => {
            status["task_controls"]["schema"] == VERSION
                && status["task_controls"]["capability"] == "task-control-v1"
        }
        _ => false,
    };
    if !ready {
        return update(computer, &envelope.request_id);
    }
    let body = match envelope
        .encoded()
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
    {
        Some(body) => body,
        None => {
            return json!({"schema":1,"request_id":envelope.request_id,"ok":false,"attempted":false,
            "code":"invalid_task_control","error":"Invalid original task-control service request"})
        }
    };
    // One offer only. Any failure from here is unknown; no retry or direct
    // writer is reachable, including after an apparently missing service.
    let reply = match exchange(OP, Some(body)) {
        Ok(Some(reply)) => reply,
        _ => return unknown(computer, envelope),
    };
    if reply["schema"] != VERSION
        || reply["request_id"] != envelope.request_id
        || !reply["ok"].is_boolean()
        || !reply["attempted"].is_boolean()
        || serde_json::to_vec(&reply).map_or(true, |b| b.len() > REPLY_BYTES)
    {
        return unknown(computer, envelope);
    }
    reply
}

#[cfg(test)]
mod tests {
    use super::*;
    const ID: &str = "00000000-0000-4000-8000-000000000001";
    #[test]
    fn the_probe_uses_the_existing_status_frame_not_an_effect_response() {
        let frame = |kind: &str| {
            serde_json::to_vec(&json!({"abi":protocol::ABI,"epoch":ID,"seq":1,
            "t":kind,"value":{"task_controls":{"schema":1,"capability":"task-control-v1"}}}))
            .unwrap()
        };
        assert_eq!(
            reply("status", &frame("status")).unwrap()["task_controls"]["schema"],
            1
        );
        assert!(reply("status", &frame("response")).is_err());
        assert!(reply(OP, &frame("status")).is_err());
        assert!(reply(OP, &frame("response")).is_ok());
        assert!(reply("status", br#"{"abi":"desktop-v0","t":"status","value":{}}"#).is_err());
    }
    fn envelope() -> Envelope {
        Envelope::read(&serde_json::to_vec(&json!({"schema":1,"request_id":ID,
            "command":{"operation":"begin","input":{"reference":"orchard/T-234567","attempt":"00000000-0000-4000-8000-000000000002"}}})).unwrap()).unwrap()
    }
    fn ready() -> Value {
        json!({"task_controls":{"schema":1,"capability":"task-control-v1"}})
    }
    #[test]
    fn old_missing_future_or_unreadable_services_name_the_computer_without_an_offer() {
        for status in [
            None,
            Some(json!({})),
            Some(json!({"task_controls":{"schema":0}})),
            Some(json!({"task_controls":{"schema":2,"capability":"task-control-v1"}})),
        ] {
            let mut calls = 0;
            let reply = invoke(&envelope(), "example-computer", |op, body| {
                calls += 1;
                assert_eq!(op, "status");
                assert!(body.is_none());
                Ok(status.clone())
            });
            assert_eq!(calls, 1);
            assert_eq!(reply["code"], "task_control_service_update_required");
            assert_eq!(reply["attempted"], false);
            assert!(reply["error"]
                .as_str()
                .unwrap()
                .contains("example-computer"));
        }
        let reply = invoke(&envelope(), "example-computer", |_, _| {
            anyhow::bail!("private unavailable probe")
        });
        assert_eq!(reply["code"], "task_control_service_update_required");
    }
    #[test]
    fn a_lost_offer_result_never_replays_or_claims_the_original_was_not_delivered() {
        for failure in ["missing", "error", "wrong-id", "malformed", "oversize"] {
            let mut calls = 0;
            let reply = invoke(&envelope(), "example-computer", |op, body| {
                calls += 1;
                if calls == 1 {
                    assert_eq!(op, "status");
                    return Ok(Some(ready()));
                }
                assert_eq!(op, OP);
                assert_eq!(body.unwrap()["request_id"], ID);
                match failure {
                    "missing" => Ok(None),
                    "error" => anyhow::bail!("private result unavailable"),
                    "wrong-id" => Ok(Some(
                        json!({"schema":1,"request_id":uuid::Uuid::new_v4().to_string(),"ok":true,"attempted":false}),
                    )),
                    "oversize" => Ok(Some(
                        json!({"schema":1,"request_id":ID,"ok":true,"attempted":false,"data":"x".repeat(REPLY_BYTES)}),
                    )),
                    _ => Ok(Some(json!({"schema":1,"request_id":ID}))),
                }
            });
            assert_eq!(calls, 2);
            assert_eq!(reply["code"], "task_control_outcome_unknown");
            assert_eq!(reply["request_id"], ID);
            assert_eq!(reply["attempted"], true);
        }
    }
    #[test]
    fn an_exact_original_result_is_returned_without_another_request() {
        let expected = json!({"schema":1,"request_id":ID,"ok":false,"attempted":false,
            "code":"task_control_person_requires_gui"});
        let mut calls = 0;
        let reply = invoke(&envelope(), "example-computer", |_, _| {
            calls += 1;
            Ok(Some(if calls == 1 {
                ready()
            } else {
                expected.clone()
            }))
        });
        assert_eq!(calls, 2);
        assert_eq!(reply, expected);
    }

    #[test]
    fn a_lost_read_result_never_claims_that_status_sent_task_input() {
        let status = Envelope::read(&serde_json::to_vec(&json!({"schema":1,"request_id":ID,
            "command":{"operation":"status","input":{"schema":1,"target":{"scope":"request","request_id":ID}}}})).unwrap()).unwrap();
        let mut calls = 0;
        let reply = invoke(&status, "example-computer", |_, _| {
            calls += 1;
            Ok(if calls == 1 { Some(ready()) } else { None })
        });
        assert_eq!(calls, 2);
        assert_eq!(reply["read_only"], true);
        assert_eq!(reply["attempted"], false);
    }
}
