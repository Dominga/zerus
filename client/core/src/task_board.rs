//! Ordinary board mutations. Read the exact task (and reorder anchor) again
//! before one explicit write; a failed or unknown result is never retried.
use crate::{
    task_details,
    tasks_write::{Stage, TaskCall},
};
use serde::Deserialize;
use serde_json::{Map, Value};

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    ToBacklog,
    ToTodo,
    BackToTodo,
    Accept,
    Top,
    Reorder { anchor: String, before: bool },
}

const CHANGED: &str = "The task changed. Nothing was changed. Refresh the task.";
const UNCONFIRMED: &str = "Task change unconfirmed. Refresh the task before changing it again.";

pub fn apply(
    project: &str,
    task: &str,
    expected_stage: &str,
    action: &Action,
    mut run: impl FnMut(&TaskCall) -> Result<Map<String, Value>, String>,
) -> Result<(), String> {
    let read = task_details::document_call(project, task)?;
    if !matches!(
        expected_stage,
        "todo" | "backlog" | "active" | "review" | "done"
    ) {
        return Err(CHANGED.into());
    }
    let reply = run(&read)?;
    let detail = task_details::checked(&reply, project, task)?;
    if detail["stage"].as_str() != Some(expected_stage) || detail["writable"] == false {
        return Err(CHANGED.into());
    }
    let (call, result_stage) = match (action, expected_stage) {
        (Action::ToBacklog, "todo") => (TaskCall::stage(project, task, Stage::Backlog), "backlog"),
        (Action::ToTodo, "backlog") | (Action::BackToTodo, "review" | "done") => {
            (TaskCall::stage(project, task, Stage::Todo), "todo")
        }
        (Action::Accept, "review") => (TaskCall::stage(project, task, Stage::Done), "done"),
        (Action::Top, "todo") => (TaskCall::move_top(project, task), expected_stage),
        (Action::Reorder { anchor, before }, _) => {
            if anchor == task {
                return Err(CHANGED.into());
            }
            let anchor_call = task_details::document_call(project, anchor)?;
            let anchor_reply = run(&anchor_call)?;
            let anchor_detail = task_details::checked(&anchor_reply, project, anchor)?;
            if anchor_detail["stage"].as_str() != Some(expected_stage)
                || anchor_detail["writable"] == false
            {
                return Err(CHANGED.into());
            }
            (
                TaskCall::move_next_to(project, task, anchor, *before),
                expected_stage,
            )
        }
        _ => return Err(CHANGED.into()),
    };
    let receipt = run(&call)?;
    let returned = receipt.get("task").ok_or(UNCONFIRMED)?;
    if returned["uuid"].as_str() != Some(task)
        || returned["project"].as_str() != Some(project)
        || returned["stage"].as_str() != Some(result_stage)
    {
        return Err(UNCONFIRMED.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    const TASK: &str = "11111111-1111-4111-8111-111111111111";
    const ANCHOR: &str = "22222222-2222-4222-8222-222222222222";
    fn reply(task: &str, stage: &str) -> Map<String, Value> {
        json!({"project":{"id":"example"},"task":{"uuid":task,"project":"example","stage":stage,"writable":true,"history_schema":1,"log":[],"history_next":null}}).as_object().unwrap().clone()
    }
    #[test]
    fn transitions_match_qt_and_carry_exact_identity() {
        for (action, from, to) in [
            (Action::ToBacklog, "todo", "backlog"),
            (Action::ToTodo, "backlog", "todo"),
            (Action::BackToTodo, "review", "todo"),
            (Action::BackToTodo, "done", "todo"),
            (Action::Accept, "review", "done"),
        ] {
            let mut calls = vec![];
            apply("example", TASK, from, &action, |call| {
                calls.push(call.clone());
                Ok(reply(TASK, if calls.len() == 1 { from } else { to }))
            })
            .unwrap();
            assert_eq!(
                calls,
                vec![
                    task_details::document_call("example", TASK).unwrap(),
                    TaskCall::stage(
                        "example",
                        TASK,
                        match to {
                            "todo" => Stage::Todo,
                            "backlog" => Stage::Backlog,
                            _ => Stage::Done,
                        }
                    )
                ]
            );
        }
    }
    #[test]
    fn stale_foreign_readonly_and_illegal_stages_never_write() {
        for detail in [reply(TASK, "done"), reply(ANCHOR, "review"), {
            let mut r = reply(TASK, "review");
            r.get_mut("task").unwrap()["writable"] = json!(false);
            r
        }] {
            let mut count = 0;
            assert!(apply("example", TASK, "review", &Action::Accept, |_| {
                count += 1;
                Ok(detail.clone())
            })
            .is_err());
            assert_eq!(count, 1);
        }
        let mut count = 0;
        assert!(apply("example", TASK, "active", &Action::ToTodo, |_| {
            count += 1;
            Ok(reply(TASK, "active"))
        })
        .is_err());
        assert_eq!(count, 1);
    }
    #[test]
    fn reorder_checks_anchor_stage_and_identity_before_write() {
        for stage in ["todo", "done"] {
            let mut calls = vec![];
            let result = apply(
                "example",
                TASK,
                "todo",
                &Action::Reorder {
                    anchor: ANCHOR.into(),
                    before: false,
                },
                |call| {
                    calls.push(call.clone());
                    Ok(if calls.len() == 2 {
                        reply(ANCHOR, stage)
                    } else {
                        reply(TASK, "todo")
                    })
                },
            );
            assert_eq!(result.is_ok(), stage == "todo");
            assert_eq!(calls.len(), if stage == "todo" { 3 } else { 2 });
            if result.is_ok() {
                assert_eq!(
                    calls[2],
                    TaskCall::move_next_to("example", TASK, ANCHOR, false)
                );
            }
        }
    }
    #[test]
    fn top_and_unknown_ack_are_never_replayed() {
        for uncertain in [false, true] {
            let mut calls = vec![];
            let result = apply("example", TASK, "todo", &Action::Top, |call| {
                calls.push(call.clone());
                if calls.len() == 2 && uncertain {
                    Err("uncertain".into())
                } else {
                    Ok(reply(if calls.len() == 2 { ANCHOR } else { TASK }, "todo"))
                }
            });
            assert!(result.is_err());
            assert_eq!(calls.len(), 2);
            assert_eq!(calls[1], TaskCall::move_top("example", TASK));
        }
    }
}
