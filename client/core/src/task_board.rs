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
    Reorder {
        anchor: String,
        before: bool,
    },
    /// Sends the task into a team's queue (`task target REF team TEAM`).
    SendToTeam {
        team: String,
    },
}

const CHANGED: &str = "The task changed. Nothing was changed. Refresh the task.";

/// The team's reason a queue refused the task, in its own words.
fn team_reason(error: &Value) -> String {
    if let Some(text) = error.as_str() {
        return text.to_owned();
    }
    error
        .get("message")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .unwrap_or("The team refused the task.")
        .to_owned()
}
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
    if let Action::SendToTeam { team } = action {
        // A card in Backlog or To do goes into the team's queue; the reply is
        // the queue's own, and a refusal is the team's reason in its words.
        if !matches!(expected_stage, "todo" | "backlog") || team.is_empty() {
            return Err(CHANGED.into());
        }
        let receipt = run(&TaskCall::target_team(project, task, team))?;
        if receipt.get("task_exists").and_then(Value::as_bool) != Some(true) {
            return Err(UNCONFIRMED.into());
        }
        let queue = receipt.get("team").ok_or(UNCONFIRMED)?;
        if queue["id"].as_str() != Some(team.as_str()) {
            return Err(UNCONFIRMED.into());
        }
        if let Some(error) = queue.get("error").filter(|error| !error.is_null()) {
            return Err(team_reason(error));
        }
        return Ok(());
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

#[cfg(test)]
mod team_tests {
    use super::*;
    use serde_json::json;
    const TASK: &str = "11111111-1111-4111-8111-111111111111";
    const TEAM: &str = "33333333-3333-4333-8333-333333333333";
    fn card(stage: &str) -> Map<String, Value> {
        json!({"project":{"id":"example"},"task":{"uuid":TASK,"project":"example","stage":stage,"writable":true,"history_schema":1,"log":[],"history_next":null}})
            .as_object()
            .unwrap()
            .clone()
    }
    fn queue(queued: bool, node: u64, error: Option<Value>) -> Map<String, Value> {
        let mut team = json!({"id": TEAM, "queued": queued, "node": node});
        if let Some(error) = error {
            team["error"] = error;
        }
        json!({"task_exists": true, "team": team})
            .as_object()
            .unwrap()
            .clone()
    }
    #[test]
    fn send_to_team_requires_a_backlog_or_todo_card() {
        for stage in ["backlog", "todo"] {
            let mut calls = vec![];
            apply(
                "example",
                TASK,
                stage,
                &Action::SendToTeam { team: TEAM.into() },
                |call| {
                    calls.push(call.clone());
                    Ok(if calls.len() == 1 {
                        card(stage)
                    } else {
                        queue(true, 7, None)
                    })
                },
            )
            .unwrap();
            assert_eq!(
                calls,
                vec![
                    task_details::document_call("example", TASK).unwrap(),
                    TaskCall::target_team("example", TASK, TEAM)
                ]
            );
        }
        for stage in ["active", "review", "done"] {
            let mut calls = 0;
            assert!(apply(
                "example",
                TASK,
                stage,
                &Action::SendToTeam { team: TEAM.into() },
                |_| {
                    calls += 1;
                    Ok(card(stage))
                }
            )
            .is_err());
            assert_eq!(calls, 1);
        }
    }
    /// zerus-63rru.19: a storage failure of the team store reaches `task
    /// target REF team TEAM` as `ok: true`, `team.queued: false` and the
    /// store's problem in `team.error`. Send to team gives the store's sentence
    /// as it came; it adds no words of its own that the task was not queued.
    #[test]
    fn a_storage_failure_of_the_team_store_gives_the_stores_sentence() {
        let problem: Value = serde_json::from_str(include_str!(
            "../../../crates/team-plan/fixtures/problem-storage-failed.json"
        ))
        .unwrap();
        let mut calls = 0;
        let error = apply(
            "example",
            TASK,
            "backlog",
            &Action::SendToTeam { team: TEAM.into() },
            |_| {
                calls += 1;
                Ok(if calls == 1 {
                    card("backlog")
                } else {
                    queue(false, 0, Some(problem.clone()))
                })
            },
        )
        .unwrap_err();
        assert_eq!(
            error,
            "The result is not known; repeat the same request with the same request ID."
        );
        assert_eq!(calls, 2);
    }
    #[test]
    fn a_team_refusal_keeps_the_teams_reason_and_sends_nothing_more() {
        let mut calls = vec![];
        let error = apply(
            "example",
            TASK,
            "todo",
            &Action::SendToTeam { team: TEAM.into() },
            |call| {
                calls.push(call.clone());
                Ok(if calls.len() == 1 {
                    card("todo")
                } else {
                    queue(
                        false,
                        0,
                        Some(json!({"code": "over_cap", "message": "The queue is full."})),
                    )
                })
            },
        )
        .unwrap_err();
        assert!(error.contains("The queue is full."), "{error}");
        assert_eq!(calls.len(), 2, "the read and the one target call");
        // A different team id in the reply is an unknown outcome, never accepted.
        let mut calls = 0;
        assert!(apply(
            "example",
            TASK,
            "todo",
            &Action::SendToTeam { team: TEAM.into() },
            |_| {
                calls += 1;
                if calls == 1 {
                    return Ok(card("todo"));
                }
                let mut reply = queue(false, 0, None);
                reply.get_mut("team").unwrap()["id"] =
                    json!("44444444-4444-4444-8444-444444444444");
                Ok(reply)
            }
        )
        .is_err());
        assert_eq!(
            calls, 2,
            "the read and the one target call; the wrong queue is never accepted"
        );
    }
    #[test]
    fn a_task_sent_twice_queues_once_and_the_call_carries_no_second_request() {
        // The queue's intake is keyed by the task, so the same command twice
        // names the same node; the second reply says it was already queued.
        let mut calls = vec![];
        for queued in [true, false] {
            apply(
                "example",
                TASK,
                "todo",
                &Action::SendToTeam { team: TEAM.into() },
                |call| {
                    calls.push(call.clone());
                    Ok(if calls.len() % 2 == 1 {
                        card("todo")
                    } else {
                        queue(queued, 7, None)
                    })
                },
            )
            .unwrap();
        }
        let targets: Vec<&TaskCall> = calls.iter().skip(1).step_by(2).collect();
        assert_eq!(
            targets,
            vec![
                &TaskCall::target_team("example", TASK, TEAM),
                &TaskCall::target_team("example", TASK, TEAM)
            ]
        );
    }
}
