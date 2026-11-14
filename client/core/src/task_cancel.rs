//! One explicit cancellation of the execution the person confirmed. Reads are
//! bounded; the CLI repeats the identity guard under its project lock, including
//! on the executor's computer. Unknown outcomes are never retried here.
use crate::{task_details, tasks_write::TaskCall};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

const CHANGED: &str = "The task execution changed. Nothing was cancelled. Refresh the task.";
const UNCONFIRMED: &str = "Cancellation unconfirmed. Refresh the task before changing it again.";

/// Disposition only: the native command admits the exact unknown attempt (or
/// observes its original disposition) under its project and Team writers.
pub fn abandon(
    project: &str,
    task: &str,
    attempt: &str,
    reason: &str,
    mut run: impl FnMut(&TaskCall) -> Result<Map<String, Value>, String>,
) -> Result<(), String> {
    if !crate::creation::canonical_uuid(attempt)
        || reason.len() > 64 * 1024
        || reason.contains('\0')
    {
        return Err(CHANGED.into());
    }
    let read = task_details::document_call(project, task)?;
    let current = task_details::checked(&run(&read)?, project, task)?;
    if current["writable"] != true {
        return Err(CHANGED.into());
    }
    // Do not infer idempotence from the latest displayed attempt. The native
    // command owns both the exact original disposition and stale-ID refusal.
    let call = TaskCall {
        args: vec![
            "cancel-attempt".into(),
            task.into(),
            "--project".into(),
            project.into(),
            "--attempt".into(),
            attempt.into(),
            "--reason".into(),
            reason.trim().into(),
            "--json".into(),
        ],
        input: String::new(),
    };
    let reply = run(&call)?;
    if reply["task"]["uuid"].as_str() != Some(task)
        || reply["task"]["project"].as_str() != Some(project)
        || reply["attempt"]["id"].as_str() != Some(attempt)
        || reply["attempt"]["state"] != "cancelled"
        || reply["attempt"]["delivery"] != "unknown"
    {
        return Err(UNCONFIRMED.into());
    }
    Ok(())
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Execution {
    pub dispatch_id: String,
    pub stage_at: i64,
    pub machine: String,
    pub session_id: String,
}

pub fn apply(
    project: &str,
    task: &str,
    execution: &Execution,
    reason: &str,
    mut run: impl FnMut(&TaskCall) -> Result<Map<String, Value>, String>,
) -> Result<Option<String>, String> {
    let read = task_details::document_call(project, task)?;
    if execution.stage_at <= 0
        || [
            &execution.dispatch_id,
            &execution.machine,
            &execution.session_id,
        ]
        .iter()
        .any(|s| s.is_empty() || s.len() > 256 || s.chars().any(char::is_control))
        || reason.len() > 64 * 1024
        || reason.contains('\0')
    {
        return Err(CHANGED.into());
    }
    let reply = run(&read)?;
    let current = task_details::checked(&reply, project, task)?;
    if current["stage_at"].as_i64() != Some(execution.stage_at)
        || current["writable"] != true
        || current["stage"] != "active"
        || current["dispatch_id"].as_str() != Some(execution.dispatch_id.as_str())
        || current["session"]["machine"].as_str() != Some(execution.machine.as_str())
        || current["session"]["session_id"].as_str() != Some(execution.session_id.as_str())
    {
        return Err(CHANGED.into());
    }
    let call = TaskCall {
        args: vec!["cancel-execution".into(), task.into(), "--project".into(), project.into(), "--json".into()],
        input: json!({"schema":1,"dispatch_id":execution.dispatch_id,"stage_at":execution.stage_at,"machine":execution.machine,"session_id":execution.session_id,"reason":reason.trim()}).to_string(),
    };
    let reply = run(&call)?;
    let returned = reply.get("task").ok_or(UNCONFIRMED)?;
    if returned["uuid"].as_str() != Some(task)
        || returned["project"].as_str() != Some(project)
        || returned["stage"] != "review"
    {
        return Err(UNCONFIRMED.into());
    }
    match reply
        .get("cancel_message")
        .and_then(|v| v["sent"].as_bool())
    {
        Some(true) => Ok(None),
        Some(false) => Ok(Some(
            reply
                .get("cancel_message")
                .and_then(|v| v["error"].as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("The cancel message was not confirmed. Check the executor Activity.")
                .into(),
        )),
        None => Err(UNCONFIRMED.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const TASK: &str = "11111111-1111-4111-8111-111111111111";
    fn execution() -> Execution {
        Execution {
            dispatch_id: "dispatch-1".into(),
            stage_at: 123,
            machine: "node-example".into(),
            session_id: "run_id:example".into(),
        }
    }
    fn reply() -> Map<String, Value> {
        json!({"project":{"id":"example"},"task":{"uuid":TASK,"project":"example","stage":"active","stage_at":123,"writable":true,"dispatch_id":"dispatch-1","session":{"machine":"node-example","session_id":"run_id:example"},"history_schema":1,"log":[],"history_next":null}}).as_object().unwrap().clone()
    }
    const ATTEMPT: &str = "22222222-2222-4222-8222-222222222222";
    fn abandoned() -> Map<String, Value> {
        json!({"task":{"uuid":TASK,"project":"example","stage":"todo"},
            "attempt":{"id":ATTEMPT,"state":"cancelled","delivery":"unknown","already_cancelled":true}})
            .as_object().unwrap().clone()
    }
    #[test]
    fn abandonment_reads_exact_task_then_sends_only_the_original_uuid_once() {
        let mut calls = vec![];
        abandon("example", TASK, ATTEMPT, " --agent ", |call| {
            calls.push(call.clone());
            Ok(if calls.len() == 1 {
                reply()
            } else {
                abandoned()
            })
        })
        .unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].args[0], "get");
        assert_eq!(
            calls[1].args,
            [
                "cancel-attempt",
                TASK,
                "--project",
                "example",
                "--attempt",
                ATTEMPT,
                "--reason",
                "--agent",
                "--json"
            ]
        );
        assert!(calls[1].input.is_empty());
    }
    #[test]
    fn abandonment_refuses_bad_identity_readonly_or_an_unconfirmed_receipt() {
        let mut calls = 0;
        assert!(abandon("example", TASK, "latest", "", |_| {
            calls += 1;
            Ok(reply())
        })
        .is_err());
        assert_eq!(calls, 0);
        for key in ["uuid", "project", "writable"] {
            calls = 0;
            assert!(abandon("example", TASK, ATTEMPT, "", |_| {
                calls += 1;
                let mut answer = reply();
                answer["task"][key] = json!(false);
                Ok(answer)
            })
            .is_err());
            assert_eq!(calls, 1);
        }
        for key in ["id", "state", "delivery"] {
            calls = 0;
            assert!(abandon("example", TASK, ATTEMPT, "", |_| {
                calls += 1;
                if calls == 1 {
                    return Ok(reply());
                }
                let mut answer = abandoned();
                answer["attempt"][key] = json!("changed");
                Ok(answer)
            })
            .is_err());
            assert_eq!(calls, 2);
        }
    }
    #[test]
    fn abandonment_never_falls_back_or_retries_an_unknown_native_result() {
        let mut calls = 0;
        assert!(abandon("example", TASK, ATTEMPT, "", |_| {
            calls += 1;
            if calls == 1 {
                Ok(reply())
            } else {
                Err("unknown outcome".into())
            }
        })
        .is_err());
        assert_eq!(calls, 2);
    }
    #[test]
    fn exact_execution_and_reason_are_sent_once_with_undelivered_result() {
        for sent in [true, false] {
            let mut calls = vec![];
            let result = apply("example", TASK, &execution(), " --agent ", |call| {
                calls.push(call.clone());
                let mut r = reply();
                if calls.len() == 2 {
                    r.get_mut("task").unwrap()["stage"] = json!("review");
                    r.insert(
                        "cancel_message".into(),
                        json!({"sent":sent,"error":"offline"}),
                    );
                }
                Ok(r)
            })
            .unwrap();
            assert_eq!(calls.len(), 2);
            assert_eq!(
                calls[1].args,
                vec!["cancel-execution", TASK, "--project", "example", "--json"]
            );
            assert_eq!(
                serde_json::from_str::<Value>(&calls[1].input).unwrap(),
                json!({"schema":1,"dispatch_id":"dispatch-1","stage_at":123,"machine":"node-example","session_id":"run_id:example","reason":"--agent"})
            );
            assert_eq!(result, if sent { None } else { Some("offline".into()) });
            assert_eq!(
                calls[1].invocation().timeout,
                std::time::Duration::from_secs(90)
            );
        }
    }
    #[test]
    fn stale_dispatch_executor_stage_and_readonly_never_cancel() {
        for (key, value) in [
            ("dispatch_id", json!("other")),
            ("stage_at", json!(124)),
            (
                "session",
                json!({"machine":"other","session_id":"run_id:example"}),
            ),
            (
                "session",
                json!({"machine":"node-example","session_id":"run_id:other"}),
            ),
            ("stage", json!("review")),
            ("writable", json!(false)),
            ("uuid", json!("22222222-2222-4222-8222-222222222222")),
        ] {
            let mut r = reply();
            r.get_mut("task").unwrap()[key] = value;
            let mut n = 0;
            assert!(apply("example", TASK, &execution(), "", |_| {
                n += 1;
                Ok(r.clone())
            })
            .is_err());
            assert_eq!(n, 1);
        }
    }
    #[test]
    fn uncertain_or_foreign_receipt_is_not_replayed() {
        for unknown in [true, false] {
            let mut n = 0;
            assert!(apply("example", TASK, &execution(), "", |_| {
                n += 1;
                if n == 2 && unknown {
                    Err("uncertain".into())
                } else {
                    Ok(reply())
                }
            })
            .is_err());
            assert_eq!(n, 2);
        }
    }
}
