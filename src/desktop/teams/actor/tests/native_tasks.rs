use super::super::ports::{Ports, TaskReply};
use super::*;
use crate::teams::ops::{Operation, StageTaskOutcome};

fn reserved() -> (Harness, crate::teams::store::StageTaskEffect) {
    let (mut h, _, node) = super::tasks::wanted();
    let id = uuid::Uuid::new_v4().to_string();
    h.run(
        &service(),
        Operation::StageTaskIntent {
            node,
            effect_request_id: id.clone(),
        },
    )
    .unwrap();
    let effect = h.store.stage_task_effect(TEAM, node, &id).unwrap().unwrap();
    (h, effect)
}
fn created_reply(e: &crate::teams::store::StageTaskEffect) -> Vec<u8> {
    json!({"schema":1,"team":e.team,"node":e.node,"id":e.request_id,"state":"created","error":""})
        .to_string()
        .into_bytes()
}
#[test]
fn native_stage_uses_its_exact_reserved_effect_inside_the_spawn_admission() {
    let (h, e) = reserved();
    crate::desktop::teams::child::FIXTURE_EXIT.with(|s| *s.borrow_mut() = Some(created_reply(&e)));
    assert_eq!(
        super::production::native(&h)
            .stage(
                &h.store.team(TEAM).unwrap().unwrap(),
                &e,
                &uuid::Uuid::new_v4().to_string()
            )
            .unwrap(),
        TaskReply::Created
    );
}
#[test]
fn native_stage_refuses_pause_and_stop_committed_after_the_effect_was_read() {
    for op in [Operation::Pause, Operation::Stop] {
        let (mut h, e) = reserved();
        let team = h.store.team(TEAM).unwrap().unwrap();
        h.run(&person(), op).unwrap();
        crate::desktop::teams::child::FIXTURE_EXIT
            .with(|s| *s.borrow_mut() = Some(created_reply(&e)));
        assert!(super::production::native(&h)
            .stage(&team, &e, &uuid::Uuid::new_v4().to_string())
            .is_err());
        crate::desktop::teams::child::FIXTURE_EXIT.with(|s| s.borrow_mut().take());
    }
}
fn released() -> (Harness, crate::teams::store::timers::KeyedTask) {
    let (mut h, e) = reserved();
    h.run(
        &service(),
        Operation::StageTaskOutcome {
            effect_request_id: e.request_id.clone(),
            outcome: StageTaskOutcome::Created {
                task: TaskRef {
                    project: e.project.clone(),
                    uuid: e.request_id.clone(),
                },
            },
        },
    )
    .unwrap();
    let target = h.node(e.node).stage_of.unwrap();
    h.run(
        &person(),
        Operation::Return {
            node: target,
            reason: "Please revise".into(),
        },
    )
    .unwrap();
    // The retired stage's task runs nothing: its cancel is answered released.
    h.release_retired(e.node);
    let keyed = h.store.released_stages(TEAM, 1, 0).unwrap().remove(0);
    (h, keyed)
}
#[test]
fn native_clear_uses_only_an_exact_released_stage() {
    let (h, keyed) = released();
    let team = h.store.team(TEAM).unwrap().unwrap();
    crate::desktop::teams::child::FIXTURE_EXIT.with(|s| {
        *s.borrow_mut() = Some(
            json!({"schema":1,"team":TEAM,"node":keyed.node,"state":"cleared","error":""})
                .to_string()
                .into_bytes(),
        )
    });
    assert_eq!(
        super::production::native(&h)
            .clear_stage(&team, &keyed, &uuid::Uuid::new_v4().to_string())
            .unwrap(),
        TaskReply::Cleared
    );
}
#[test]
fn native_clear_refuses_pause_and_stop_after_the_release_was_read() {
    for op in [Operation::Pause, Operation::Stop] {
        let (mut h, keyed) = released();
        let team = h.store.team(TEAM).unwrap().unwrap();
        h.run(&person(), op).unwrap();
        crate::desktop::teams::child::FIXTURE_EXIT.with(|s| {
            *s.borrow_mut() = Some(
                json!({"schema":1,"team":TEAM,"node":keyed.node,"state":"cleared","error":""})
                    .to_string()
                    .into_bytes(),
            )
        });
        assert!(super::production::native(&h)
            .clear_stage(&team, &keyed, &uuid::Uuid::new_v4().to_string())
            .is_err());
        crate::desktop::teams::child::FIXTURE_EXIT.with(|s| s.borrow_mut().take());
    }
}

#[test]
fn literal_stage_and_clear_replies_require_the_exact_echo_and_a_successful_child() {
    use super::super::native::task_reply;
    use crate::desktop::teams::child::Ended;
    let request = json!({"schema":1,"team":"team","node":2,"id":"effect"});
    let created =
        br#"{"schema":1,"team":"team","node":2,"id":"effect","state":"created","error":""}"#;
    assert_eq!(
        task_reply(
            Ended::Exited {
                code: 0,
                output: created.to_vec()
            },
            &request,
            "created"
        )
        .unwrap(),
        TaskReply::Created
    );
    for (field, value) in [
        ("schema", json!(2)),
        ("team", json!("other")),
        ("node", json!(3)),
        ("id", json!("other")),
        ("state", json!("other")),
        ("error", json!(null)),
    ] {
        let mut reply: serde_json::Value = serde_json::from_slice(created).unwrap();
        reply[field] = value;
        assert!(
            task_reply(
                Ended::Exited {
                    code: 0,
                    output: reply.to_string().into_bytes()
                },
                &request,
                "created"
            )
            .is_err(),
            "{field}"
        );
    }
    assert!(task_reply(
        Ended::Exited {
            code: 1,
            output: created.to_vec()
        },
        &request,
        "created"
    )
    .is_err());
    assert!(task_reply(Ended::NotStarted, &request, "created").is_err());
    assert!(task_reply(Ended::Unknown("fixture child unknown"), &request, "created").is_err());
    let clear = json!({"schema":1,"team":"team","node":2});
    for state in ["cleared", "refused", "cutting_over"] {
        let reply = json!({"schema":1,"team":"team","node":2,"state":state,"error":if state=="cleared"{""}else{"Reason"}});
        let value = task_reply(
            Ended::Exited {
                code: 0,
                output: reply.to_string().into_bytes(),
            },
            &clear,
            "cleared",
        )
        .unwrap();
        assert!(matches!(
            (state, value),
            ("cleared", TaskReply::Cleared)
                | ("refused", TaskReply::Refused { .. })
                | ("cutting_over", TaskReply::CuttingOver { .. })
        ));
    }
}

#[test]
fn native_stage_and_clear_hold_the_store_transaction_until_the_spawn_returns() {
    use crate::desktop::original_child;
    for creation in [true, false] {
        let (h, e) = reserved();
        let (h, keyed) = if creation {
            (h, None)
        } else {
            let (h, k) = released();
            (h, Some(k))
        };
        let path = h.temp.path().join("teams/teams-v1.sqlite3");
        let before = path.clone();
        let checked = std::rc::Rc::new(std::cell::Cell::new(0));
        let c = checked.clone();
        original_child::AFTER_FINAL_CHECK.with(|s| {
            *s.borrow_mut() = Some(Box::new(move || {
                let db = rusqlite::Connection::open(before).unwrap();
                db.busy_timeout(std::time::Duration::ZERO).unwrap();
                assert!(
                    db.execute_batch("BEGIN IMMEDIATE").is_err(),
                    "task admission released before spawn"
                );
                c.set(1);
            }))
        });
        let c = checked.clone();
        original_child::AFTER_SPAWN.with(|s| {
            *s.borrow_mut() = Some(Box::new(move || {
                let db = rusqlite::Connection::open(path).unwrap();
                db.execute_batch("BEGIN IMMEDIATE;ROLLBACK").unwrap();
                c.set(2);
            }))
        });
        let t = h.store.team(TEAM).unwrap().unwrap();
        let reply = if let Some(k) = &keyed {
            json!({"schema":1,"team":TEAM,"node":k.node,"state":"cleared","error":""})
                .to_string()
                .into_bytes()
        } else {
            created_reply(&e)
        };
        crate::desktop::teams::child::FIXTURE_EXIT.with(|s| *s.borrow_mut() = Some(reply));
        let mut n = super::production::native(&h);
        let answer = if let Some(k) = keyed {
            n.clear_stage(&t, &k, &uuid::Uuid::new_v4().to_string())
        } else {
            n.stage(&t, &e, &uuid::Uuid::new_v4().to_string())
        };
        assert!(answer.is_ok());
        assert_eq!(checked.get(), 2);
    }
}

#[test]
fn native_task_admission_refuses_changed_creation_and_a_live_or_other_task_clear() {
    let (h, mut e) = reserved();
    let t = h.store.team(TEAM).unwrap().unwrap();
    e.text.push_str("changed");
    crate::desktop::teams::child::FIXTURE_EXIT.with(|s| *s.borrow_mut() = Some(created_reply(&e)));
    assert!(super::production::native(&h)
        .stage(&t, &e, &uuid::Uuid::new_v4().to_string())
        .is_err());
    let (h, k) = released();
    let t = h.store.team(TEAM).unwrap().unwrap();
    for target in [
        crate::teams::store::timers::KeyedTask {
            node: k.node,
            task: task(999),
        },
        crate::teams::store::timers::KeyedTask {
            node: 1,
            task: task(1),
        },
    ] {
        crate::desktop::teams::child::FIXTURE_EXIT.with(|s| {
            *s.borrow_mut() = Some(
                json!({"schema":1,"team":TEAM,"node":target.node,"state":"cleared","error":""})
                    .to_string()
                    .into_bytes(),
            )
        });
        assert!(super::production::native(&h)
            .clear_stage(&t, &target, &uuid::Uuid::new_v4().to_string())
            .is_err());
    }
}

#[test]
fn literal_cancel_replies_keep_release_distinct_until_the_store_fact() {
    use crate::desktop::teams::child::Ended;
    let request = json!({"schema":1,"team":"team","node":2});
    for state in ["cancelled", "released", "refused", "cutting_over"] {
        let error = if matches!(state, "cancelled" | "released") {
            ""
        } else {
            "Reason"
        };
        let raw = json!({"schema":1,"team":"team","node":2,"state":state,"error":error,"attempt":1,"attempt_id":"observed-attempt"})
            .to_string()
            .into_bytes();
        let reply = super::super::native::cancel_answer(
            Ended::Exited {
                code: 0,
                output: raw,
            },
            &request,
        )
        .unwrap()
        .reply;
        assert!(matches!(
            (state, reply),
            ("cancelled", TaskReply::Cancelled)
                | ("released", TaskReply::Released)
                | ("refused", TaskReply::Refused { .. })
                | ("cutting_over", TaskReply::CuttingOver { .. })
        ));
    }
    for state in ["cleared", "done", "uncertain", "unknown"] {
        let raw = json!({"schema":1,"team":"team","node":2,"state":state,"error":""})
            .to_string()
            .into_bytes();
        assert!(super::super::native::cancel_answer(
            Ended::Exited {
                code: 0,
                output: raw
            },
            &request
        )
        .is_err());
    }
}

/// A stage or clear command's refusal is recorded cleaned: decoded by the
/// production command, recorded through the operation the actor applies,
/// read back from the store and from the lead's item.
#[test]
fn a_stage_or_clear_refusal_is_recorded_without_control_characters() {
    let refusal = |team: &str, node: u32, id: Option<&str>| {
        let mut reply = json!({"schema":1,"team":team,"node":node,"state":"refused","error":super::hostile_sentence()});
        if let Some(id) = id {
            reply["id"] = json!(id);
        }
        crate::desktop::teams::child::FIXTURE_EXIT
            .with(|s| *s.borrow_mut() = Some(reply.to_string().into_bytes()));
    };
    // Creation refused: the stage's outcome and the lead's item.
    let (mut h, e) = reserved();
    let team = h.store.team(TEAM).unwrap().unwrap();
    refusal(&e.team, e.node, Some(&e.request_id));
    let TaskReply::Refused { reason } = super::production::native(&h)
        .stage(&team, &e, &uuid::Uuid::new_v4().to_string())
        .unwrap()
    else {
        panic!("not a refusal")
    };
    h.run(
        &service(),
        Operation::StageTaskOutcome {
            effect_request_id: e.request_id.clone(),
            outcome: StageTaskOutcome::Refused { reason },
        },
    )
    .unwrap();
    let outcome: String = h
        .store
        .db
        .as_ref()
        .unwrap()
        .query_row(
            "SELECT outcome FROM stage_tasks WHERE team=?1 AND node=?2",
            rusqlite::params![TEAM, e.node],
            |r| r.get(0),
        )
        .unwrap();
    let outcome: serde_json::Value = serde_json::from_str(&outcome).unwrap();
    let stored = outcome["refused"]["reason"]
        .as_str()
        .or_else(|| outcome["Refused"]["reason"].as_str())
        .or_else(|| outcome["reason"].as_str())
        .unwrap_or_else(|| panic!("{outcome}"))
        .to_owned();
    super::assert_clean(&[stored], "stage");
    let items: Vec<String> = h
        .store
        .inbox(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .map(|i| i.text)
        .collect();
    // The lead's item carries the command's words cleaned; the item's own
    // line breaks are its template's.
    let told = items
        .iter()
        .find(|t| t.contains("busy busy busy"))
        .unwrap_or_else(|| panic!("{items:?}"));
    assert!(
        !told.chars().any(|c| c.is_control() && c != '\n'),
        "{told:?}"
    );
    // Clearing refused: the recorded key-clear reason.
    let (mut h, keyed) = released();
    let team = h.store.team(TEAM).unwrap().unwrap();
    refusal(TEAM, keyed.node, None);
    let TaskReply::Refused { reason } = super::production::native(&h)
        .clear_stage(&team, &keyed, &uuid::Uuid::new_v4().to_string())
        .unwrap()
    else {
        panic!("not a refusal")
    };
    h.run(
        &service(),
        Operation::RecordKeyClear {
            node: keyed.node,
            state: crate::teams::ops::KeyClearState::Refused,
            reason,
        },
    )
    .unwrap();
    let stored: String = h
        .store
        .db
        .as_ref()
        .unwrap()
        .query_row(
            "SELECT reason FROM key_clears WHERE team=?1 AND node=?2",
            rusqlite::params![TEAM, keyed.node],
            |r| r.get(0),
        )
        .unwrap();
    super::assert_clean(&[stored], "clear");
}
