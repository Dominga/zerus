use super::super::native::*;
use crate::teams::ops::HandoutOutcome;
#[test]
fn literal_delivery_replies_preserve_each_authoritative_state() {
    assert!(matches!(
        decode_delivery(
            br#"{"schema":1,"id":"effect","state":"submitted","error":"","task":{"id":"a"}}"#,
            "effect"
        ),
        HandoutOutcome::Submitted { .. }
    ));
    assert!(matches!(
        decode_delivery(
            br#"{"schema":1,"id":"effect","state":"refused","error":"Busy"}"#,
            "effect"
        ),
        HandoutOutcome::Refused { .. }
    ));
    assert!(matches!(
        decode_delivery(
            br#"{"schema":1,"id":"effect","state":"uncertain","error":"Lost"}"#,
            "effect"
        ),
        HandoutOutcome::Uncertain { .. }
    ));
    assert!(matches!(
        decode_delivery(
            br#"{"schema":1,"id":"effect","state":"cutting_over","error":"Cutover"}"#,
            "effect"
        ),
        HandoutOutcome::CuttingOver { .. }
    ));
    for raw in [
        b"{}".as_slice(),
        br#"{"state":"submitted","schema":2}"#,
        br#"{"status":"submitted"}"#,
        b"not json",
    ] {
        assert!(matches!(
            decode_delivery(raw, "effect"),
            HandoutOutcome::Uncertain { .. }
        ));
    }
}
#[test]
fn reply_of_another_effect_is_never_a_delivery_receipt() {
    assert!(matches!(
        decode_delivery(
            br#"{"schema":1,"id":"other","state":"submitted","error":""}"#,
            "effect"
        ),
        HandoutOutcome::Uncertain { .. }
    ));
}
#[test]
fn literal_finishing_replies_echo_the_team_and_node_and_have_no_uncertain_state() {
    use super::super::ports::Finish;
    assert_eq!(
        decode_finish(
            br#"{"schema":1,"team":"team","node":7,"state":"done","error":""}"#,
            "team",
            7
        )
        .unwrap(),
        Finish::Done
    );
    assert_eq!(
        decode_finish(
            br#"{"schema":1,"team":"team","node":7,"state":"refused","error":"No accept"}"#,
            "team",
            7
        )
        .unwrap(),
        Finish::Refused("No accept".into())
    );
    assert_eq!(
        decode_finish(
            br#"{"schema":1,"team":"team","node":7,"state":"cutting_over","error":"Held"}"#,
            "team",
            7
        )
        .unwrap(),
        Finish::CuttingOver("Held".into())
    );
    for (team, node, state) in [
        ("other", 7, "done"),
        ("team", 8, "done"),
        ("team", 7, "uncertain"),
        ("team", 7, "new"),
    ] {
        let raw = serde_json::json!({"schema":1,"team":team,"node":node,"state":state,"error":""})
            .to_string();
        assert!(decode_finish(raw.as_bytes(), "team", 7).is_err());
    }
    assert!(decode_finish(b"{}", "team", 7).is_err());
    assert!(decode_finish(b"bad", "team", 7).is_err());
}
#[test]
fn delivery_requires_the_echoed_id_and_version_and_never_accepts_a_failed_child() {
    use crate::desktop::teams::child::Ended;
    for raw in [
        br#"{"schema":1,"state":"submitted","error":""}"#.as_slice(),
        br#"{"id":"effect","state":"submitted","error":""}"#,
    ] {
        assert!(matches!(
            decode_delivery(raw, "effect"),
            HandoutOutcome::Uncertain { .. }
        ));
    }
    for state in ["submitted", "refused", "uncertain", "cutting_over", "stale"] {
        let output = serde_json::json!({"schema":1,"id":"effect","state":state,"error":"Reason"})
            .to_string()
            .into_bytes();
        assert!(
            matches!(
                delivery_reply(Ended::Exited { code: 1, output }, "effect"),
                HandoutOutcome::Uncertain { .. }
            ),
            "{state}"
        );
    }
}

#[test]
fn adapter_argument_vectors_use_the_shared_builders() {
    let task = zerus_team_plan::model::TaskRef {
        project: "project-1".into(),
        uuid: "00000000-0000-4000-8000-000000000001".into(),
    };
    let occupancy = occupancy_arguments("project-1", "run_id:review-run");
    assert!(occupancy.windows(2).any(|a| a == ["--stage", "active"]));
    let deliver = task_arguments(&task, "team-deliver");
    let done = task_arguments(&task, "team-done");
    assert_eq!(deliver[1], "team-deliver");
    assert_eq!(done[1], "team-done");
    assert_eq!(deliver[2], "project-1/00000000-0000-4000-8000-000000000001");
    println!(
        "TEAM_ACTOR_ARGUMENTS={}",
        serde_json::json!({"occupancy":occupancy,"deliver":deliver,"done":done})
    );
}

#[test]
fn production_occupancy_excludes_busy_tasks_and_unknown_indexes() {
    use super::*;
    let mut h = Harness::new();
    h.bind("m1");
    let n = super::production::native(&h);
    let member = h.store.member(TEAM, "m1").unwrap().unwrap();
    TEST_INVENTORY.with(|s|*s.borrow_mut()=Some(serde_json::json!({"node_id":n.machine,"organization":{"projects":[{"id":"project-1"}]}})));
    for (reply, expected) in [
        (
            serde_json::json!({"damaged_total":0,"tasks":[{"id":"busy"}]}),
            Some(false),
        ),
        (
            serde_json::json!({"damaged_total":0,"tasks":[]}),
            Some(true),
        ),
        (serde_json::json!({"damaged_total":1,"tasks":[]}), None),
    ] {
        crate::desktop::teams::child::FIXTURE_EXIT
            .with(|s| *s.borrow_mut() = Some(reply.to_string().into_bytes()));
        assert_eq!(n.unoccupied(&member).ok(), expected);
    }
    TEST_INVENTORY.with(|s| *s.borrow_mut() = None);
}
#[test]
fn production_delivery_and_done_refuse_a_pause_before_the_spawn() {
    use super::*;
    for done in [false, true] {
        let mut h = Harness::new();
        let (node, id, decision) = if done {
            h.settings(|s| s.integration = Integration::None);
            let (node, lead) = h.reported();
            h.run(&lead, crate::teams::ops::Operation::Accept { node })
                .unwrap();
            (
                node,
                uuid::Uuid::new_v4().to_string(),
                Some(
                    h.store
                        .pending_accepts(TEAM, 200, None)
                        .unwrap()
                        .rows
                        .remove(0),
                ),
            )
        } else {
            h.bind("m1");
            let node = h.add(&person(), 1);
            (node, h.intent(node, "m1"), None)
        };
        let team = h.store.team(TEAM).unwrap().unwrap();
        let row = h.store.node(TEAM, node).unwrap().unwrap();
        let member = h.store.member(TEAM, "m1").unwrap().unwrap();
        h.run(&person(), crate::teams::ops::Operation::Pause)
            .unwrap();
        let mut native = super::production::native(&h);
        let reply = if done {
            serde_json::json!({"schema":1,"team":TEAM,"node":node,"state":"done","error":""})
        } else {
            serde_json::json!({"schema":1,"id":id,"state":"submitted","error":""})
        };
        crate::desktop::teams::child::FIXTURE_EXIT
            .with(|s| *s.borrow_mut() = Some(reply.to_string().into_bytes()));
        if let Some(d) = decision {
            assert!(super::super::ports::Ports::finish(&mut native, &team, &row, &d, &id).is_err());
        } else {
            assert!(matches!(
                super::super::ports::Ports::deliver(&mut native, &team, &row, &member, &id),
                super::super::ports::DeliveryResult::Outcome(HandoutOutcome::Refused { .. })
            ));
        }
    }
}
#[test]
fn production_command_keeps_its_transaction_until_spawn_returns() {
    use super::*;
    use crate::desktop::original_child;
    for done in [false, true] {
        let mut h = Harness::new();
        let (node, id, decision) = if done {
            h.settings(|s| s.integration = Integration::None);
            let (node, lead) = h.reported();
            h.run(&lead, crate::teams::ops::Operation::Accept { node })
                .unwrap();
            (
                node,
                uuid::Uuid::new_v4().to_string(),
                Some(
                    h.store
                        .pending_accepts(TEAM, 200, None)
                        .unwrap()
                        .rows
                        .remove(0),
                ),
            )
        } else {
            h.bind("m1");
            let node = h.add(&person(), 1);
            (node, h.intent(node, "m1"), None)
        };
        let team = h.store.team(TEAM).unwrap().unwrap();
        let row = h.store.node(TEAM, node).unwrap().unwrap();
        let member = h.store.member(TEAM, "m1").unwrap().unwrap();
        let mut native = super::production::native(&h);
        let path = h.temp.path().join("teams/teams-v1.sqlite3");
        let before = path.clone();
        let checked = std::rc::Rc::new(std::cell::Cell::new(0));
        let seen = checked.clone();
        original_child::AFTER_FINAL_CHECK.with(|s| {
            *s.borrow_mut() = Some(Box::new(move || {
                let db = rusqlite::Connection::open(before).unwrap();
                db.busy_timeout(std::time::Duration::ZERO).unwrap();
                assert!(
                    db.execute_batch("BEGIN IMMEDIATE").is_err(),
                    "admission released before spawn"
                );
                seen.set(1);
            }))
        });
        let seen = checked.clone();
        original_child::AFTER_SPAWN.with(|s| {
            *s.borrow_mut() = Some(Box::new(move || {
                let db = rusqlite::Connection::open(path).unwrap();
                db.execute_batch("BEGIN IMMEDIATE;ROLLBACK").unwrap();
                seen.set(2);
            }))
        });
        let reply = if done {
            serde_json::json!({"schema":1,"team":TEAM,"node":node,"state":"done","error":""})
        } else {
            serde_json::json!({"schema":1,"id":id,"state":"submitted","error":""})
        };
        crate::desktop::teams::child::FIXTURE_EXIT
            .with(|s| *s.borrow_mut() = Some(reply.to_string().into_bytes()));
        if let Some(d) = decision {
            assert!(super::super::ports::Ports::finish(&mut native, &team, &row, &d, &id).is_ok());
        } else {
            assert!(matches!(
                super::super::ports::Ports::deliver(&mut native, &team, &row, &member, &id),
                super::super::ports::DeliveryResult::Outcome(HandoutOutcome::Submitted { .. })
            ));
        }
        assert_eq!(checked.get(), 2);
    }
}

#[test]
fn the_real_delivery_success_envelope_may_omit_the_error_field() {
    assert!(matches!(decode_delivery(
        br#"{"schema":1,"id":"effect","team":"team","node":1,"member":"m1","state":"submitted"}"#,
        "effect"), HandoutOutcome::Submitted { .. }));
    for state in ["refused", "uncertain", "cutting_over"] {
        let raw = serde_json::json!({"schema":1,"id":"effect","state":state}).to_string();
        assert!(matches!(
            decode_delivery(raw.as_bytes(), "effect"),
            HandoutOutcome::Uncertain { .. }
        ));
    }
}

#[test]
fn literal_stale_replies_are_positive_nonwrites_only_for_the_exact_successful_child() {
    use super::super::ports::{DeliveryResult, Finish};
    use crate::desktop::teams::child::Ended;
    let raw = br#"{"schema":1,"id":"effect","state":"stale","error":"Observe first"}"#.to_vec();
    assert!(matches!(
        delivery_result(
            Ended::Exited {
                code: 0,
                output: raw.clone()
            },
            "effect"
        ),
        DeliveryResult::Stale
    ));
    for (code, id) in [(1, "effect"), (0, "other")] {
        assert!(matches!(
            delivery_result(
                Ended::Exited {
                    code,
                    output: raw.clone()
                },
                id
            ),
            DeliveryResult::Outcome(HandoutOutcome::Uncertain { .. })
        ));
    }
    assert_eq!(
        decode_finish(
            br#"{"schema":1,"team":"team","node":1,"state":"stale","error":"Observe first"}"#,
            "team",
            1
        )
        .unwrap(),
        Finish::Stale
    );
}

#[test]
fn project_limit_refusals_keep_the_task_commands_stable_reason_unchanged() {
    for reason in [
        "project_agents_other: This project allows agents to start tasks only in their own session.",
        "project_agent_limit: The project's hourly agent start limit is reached.",
    ] {
        let raw = serde_json::json!({"schema":1,"id":"effect","state":"refused","error":reason}).to_string();
        match decode_delivery(raw.as_bytes(), "effect") {
            HandoutOutcome::Refused {reason: actual} => assert_eq!(actual, reason),
            outcome => panic!("rule refusal was lost: {outcome:?}"),
        }
    }
}

/// The typed cause beside the refusal: the actor reads it, never the
/// sentence (zerus-63rru.23); the store outcome stays a plain refusal.
#[test]
fn a_project_limits_answer_carries_the_typed_cause() {
    use crate::desktop::teams::child::Ended;
    let raw = serde_json::json!({"schema":1,"id":"effect","state":"refused","error":"project_agent_limit: The project's hourly agent start limit is reached.","cause":"project_limit"}).to_string().into_bytes();
    match decode_delivery(&raw, "effect") {
        HandoutOutcome::Refused { reason } => {
            assert!(reason.starts_with("project_agent_limit:"), "{reason:?}")
        }
        outcome => panic!("the cause is beside the refusal: {outcome:?}"),
    }
    match delivery_result(Ended::Exited { code: 0, output: raw }, "effect") {
        super::super::ports::DeliveryResult::ProjectLimit { reason } => {
            assert!(reason.starts_with("project_agent_limit:"), "{reason:?}")
        }
        result => panic!("{result:?}"),
    }
}

/// The command's task-store-busy answer is the actor's non-start, by its
/// state, never a kill read as uncertain (zerus-63rru.72).
#[test]
fn a_task_store_busy_answer_is_a_non_start_the_actor_retries() {
    use crate::desktop::teams::child::Ended;
    let raw = serde_json::json!({"schema":1,"id":"effect","state":"not_started","error":"The task store was busy; nothing was reserved or sent.","cause":"task_store_busy"}).to_string().into_bytes();
    match delivery_result(Ended::Exited { code: 0, output: raw }, "effect") {
        super::super::ports::DeliveryResult::NotStarted { reason } => {
            assert!(reason.contains("task store was busy"), "{reason:?}")
        }
        result => panic!("{result:?}"),
    }
}

/// zerus-63rru.58: the real task command's answer for a hand-out kept from
/// its reservation, read by the actor's own reader. Returns what the command
/// did and what the actor read: its delivery result, and what it records.
fn blocked(
    blocked: crate::tasks::Blocked,
) -> (
    crate::tasks::RealHandout,
    super::super::ports::DeliveryResult,
) {
    use crate::desktop::teams::child::Ended;
    let mut read = None;
    let seen = crate::tasks::blocked_handout(blocked, |code, output, id| {
        let ended = Ended::Exited {
            code,
            output: output.to_vec(),
        };
        let result = delivery_result(ended, id);
        read = Some(result.clone());
        match result {
            super::super::ports::DeliveryResult::Outcome(outcome) => Some(outcome),
            _ => None,
        }
    });
    eprintln!("the real command and what the actor records: {seen:?}, read as {read:?}");
    assert!(
        !seen.task_changed && !seen.reserved && !seen.team_changed && !seen.offered,
        "nothing written, reserved or offered: {seen:?}"
    );
    (seen, read.expect("the actor read the command"))
}

/// When the person archived a member's stopped session, a hand-out to it is
/// refused before any reservation: the task command resolves the member's
/// binding by its exact identity and finds none. The actor records that
/// refusal: never uncertain, no decision for anyone.
#[test]
fn a_hand_out_whose_member_session_is_gone_is_refused_with_its_reason() {
    let (seen, _) = blocked(crate::tasks::Blocked::ArchivedMember);
    assert_eq!((seen.exit, &seen.error), (0, &None), "{seen:?}");
    assert!(seen.stdout.contains(r#""state":"refused""#), "{seen:?}");
    // Another refusal's answer carries no typed cause (zerus-63rru.23).
    assert!(!seen.stdout.contains("project_limit"), "{seen:?}");
    assert_eq!(
        seen.outcome,
        Some(HandoutOutcome::Refused {
            reason: "Bound session identity is unavailable.".into()
        })
    );
    assert_eq!(seen.effect, "refused", "{seen:?}");
    assert!(seen.inbox.is_empty(), "{seen:?}");
}

/// A team store that another writer holds keeps a hand-out from beginning its
/// reservation. Nothing was reserved or sent and the cause passes: the
/// command answers a non-start, which the actor retries; the effect stays
/// reserved and no one is asked to decide.
#[test]
fn a_busy_team_store_before_the_reservation_is_a_non_start_the_actor_retries() {
    let (seen, read) = blocked(crate::tasks::Blocked::BusyTeamStore);
    assert_eq!((seen.exit, &seen.error), (0, &None), "{seen:?}");
    assert!(seen.stdout.contains(r#""state":"not_started""#), "{seen:?}");
    assert!(
        matches!(&read, super::super::ports::DeliveryResult::NotStarted { reason } if reason.contains("locked")),
        "{read:?}"
    );
    assert_eq!(seen.outcome, None, "{seen:?}");
    assert_eq!(seen.effect, "reserved", "{seen:?}");
    assert!(seen.inbox.is_empty(), "{seen:?}");
}

/// A read that fails before the admission sends nothing either: here this
/// computer's session bindings, one of whose directories cannot be listed.
/// The command answers a non-start, which the actor retries.
#[test]
fn a_failed_read_before_the_admission_is_a_non_start_the_actor_retries() {
    let (seen, read) = blocked(crate::tasks::Blocked::UnlistableBindings);
    assert_eq!((seen.exit, &seen.error), (0, &None), "{seen:?}");
    assert!(seen.stdout.contains(r#""state":"not_started""#), "{seen:?}");
    assert!(
        matches!(&read, super::super::ports::DeliveryResult::NotStarted { reason } if reason.contains("denied")),
        "{read:?}"
    );
    assert_eq!(seen.outcome, None, "{seen:?}");
    assert_eq!(seen.effect, "reserved", "{seen:?}");
    assert!(seen.inbox.is_empty(), "{seen:?}");
}

/// Witness b: a reopen after the member cleared its conversation. The real
/// SessionStart hook of a `/clear` gave the member's session a new
/// conversation since the hand-out, so the reopen's "continue this task"
/// would land in a conversation that never had the task. The task command
/// refuses it for good before any reservation (zerus-63rru.64), the actor
/// reads that as a hand-back, and the store hands the task to the lead at
/// once: the effect refused and one inbox item that says why. The actor,
/// settling that outcome, opens no failure gate. The real clear keeps the run,
/// so only the conversation tells the general route that the dispatch's
/// conversation is gone.
#[test]
fn a_reopen_after_the_member_cleared_its_conversation_is_handed_back_to_the_lead() {
    let (seen, _) = blocked(crate::tasks::Blocked::ClearedMember);
    let (before, after) = seen.conversation.clone().unwrap();
    assert_ne!(before, after, "the real hook changed the conversation");
    assert_eq!((seen.exit, &seen.error), (0, &None), "{seen:?}");
    assert!(
        seen.stdout.contains(r#""state":"refused""#) && seen.stdout.contains(r#""final":true"#),
        "{seen:?}"
    );
    assert!(
        matches!(&seen.outcome, Some(HandoutOutcome::HandedBack { reason })
            if reason.contains("no longer has this task's conversation")),
        "{seen:?}"
    );
    assert_eq!(seen.effect, "refused", "{seen:?}");
    assert!(
        seen.inbox
            .iter()
            .any(|kind| kind.starts_with("reopen_handed_back:")),
        "{seen:?}"
    );
    // The general route, as for a session outside a team, keeps the
    // dispatch's conversation as well as its run: it prepares the reopen
    // before the clear and refuses it after, although the real clear kept
    // the run.
    let (before_clear, after_clear) = seen.general.clone().unwrap();
    assert!(before_clear["prepared"].is_object(), "{before_clear}");
    assert_eq!(
        after_clear,
        serde_json::json!({"refused": "the task's dispatched run or conversation changed; refresh or hand off the task explicitly"})
    );
    // The actor settles that very outcome: it opens no failure gate, and no
    // later pass sends anything again.
    let settled = super::core::settle_reopen(seen.outcome.clone().unwrap());
    assert_eq!(settled.gate, None, "{settled:?}");
    assert_eq!(settled.sends, 2, "the hand-out and the reopen: {settled:?}");
    assert_eq!(settled.effect, "refused", "{settled:?}");
    assert!(
        settled
            .inbox
            .iter()
            .any(|kind| kind.starts_with("reopen_handed_back:")),
        "{settled:?}"
    );
}

/// A hand-out to a member that waits for a person, through the real task
/// command (zerus-63rru.79). The command refuses before any key, by the
/// terminal's composer when the record says the agent is ready, and by the
/// record when it already shows the question. The answer is a refusal with
/// the typed cause, and the actor reads that cause, not the sentence. Nothing
/// is typed into the pane; the attempt was reserved and settled as not
/// admitted.
#[test]
fn a_hand_out_to_a_member_that_waits_for_a_person_is_refused_with_its_typed_cause() {
    use crate::desktop::teams::child::Ended;
    const STATE: &str =
        "agent is waiting for input, approval, or has an unknown state; open Terminal";
    const BUSY: &str = "agent busy: terminal queue prompt is unrecognized or contains a draft; wait for the agent or open Terminal; nothing was submitted";
    for (activity, phase, sentence) in [
        ("idle", "idle", crate::state::TERMINAL_DRAFT),
        ("busy", "working", BUSY),
        ("idle", "input", STATE),
        ("idle", "approval", STATE),
    ] {
        let seen = crate::tasks::waiting_handout(activity, phase);
        eprintln!("the real command, record activity {activity} phase {phase}: {seen:?}");
        assert_eq!((seen.exit, &seen.error), (0, &None), "{seen:?}");
        let reply: serde_json::Value = serde_json::from_str(&seen.stdout).unwrap();
        assert_eq!(reply["state"], "refused", "{seen:?}");
        assert_eq!(reply["cause"], crate::tasks::TERMINAL_WAITING, "{seen:?}");
        assert_eq!(reply["error"], sentence, "{seen:?}");
        assert_eq!(seen.before, seen.after, "nothing was typed: {seen:?}");
        assert!(seen.reserved, "{seen:?}");
        let read = delivery_result(
            Ended::Exited {
                code: seen.exit,
                output: seen.stdout.clone().into_bytes(),
            },
            &seen.id,
        );
        assert!(
            matches!(&read, super::super::ports::DeliveryResult::TerminalWaiting { reason, .. }
                if reason == sentence),
            "{phase}: {read:?}"
        );
        // The same answer without its cause, as an older command gives it, is
        // a plain refusal, as before.
        let mut older = reply.clone();
        older.as_object_mut().unwrap().remove("cause");
        let read = delivery_result(
            Ended::Exited {
                code: 0,
                output: older.to_string().into_bytes(),
            },
            &seen.id,
        );
        assert!(
            matches!(
                &read,
                super::super::ports::DeliveryResult::Outcome(HandoutOutcome::Refused { .. })
            ),
            "{phase}: {read:?}"
        );
    }
}

/// A record whose state is unknown says nothing of a person it waits for:
/// the same sentence, refused without the cause, a failed hand-out as before.
#[test]
fn a_record_in_an_unknown_state_is_refused_without_the_cause() {
    use crate::desktop::teams::child::Ended;
    let seen = crate::tasks::waiting_handout("idle", "");
    let reply: serde_json::Value = serde_json::from_str(&seen.stdout).unwrap();
    assert_eq!(reply["state"], "refused", "{seen:?}");
    assert_eq!(
        reply["error"],
        "agent is waiting for input, approval, or has an unknown state; open Terminal",
        "{seen:?}"
    );
    assert!(reply.get("cause").is_none(), "{seen:?}");
    assert_eq!(seen.before, seen.after, "nothing was typed: {seen:?}");
    let read = delivery_result(
        Ended::Exited {
            code: seen.exit,
            output: seen.stdout.clone().into_bytes(),
        },
        &seen.id,
    );
    assert!(
        matches!(
            &read,
            super::super::ports::DeliveryResult::Outcome(HandoutOutcome::Refused { .. })
        ),
        "{read:?}"
    );
}

/// A hand-out command's own sentence is recorded cleaned: decoded by the
/// production deliver, recorded through the outcome operation the actor
/// applies, read back from the store.
#[test]
fn a_delivery_commands_sentence_is_recorded_without_control_characters() {
    use super::*;
    for state in ["refused", "uncertain", "cutting_over"] {
        let mut h = Harness::new();
        h.bind("m1");
        let node = h.add(&person(), 1);
        let id = h.intent(node, "m1");
        let team = h.store.team(TEAM).unwrap().unwrap();
        let row = h.store.node(TEAM, node).unwrap().unwrap();
        let member = h.store.member(TEAM, "m1").unwrap().unwrap();
        let mut native = super::production::native(&h);
        let reply =
            serde_json::json!({"schema":1,"id":id,"state":state,"error":super::hostile_sentence()});
        crate::desktop::teams::child::FIXTURE_EXIT
            .with(|s| *s.borrow_mut() = Some(reply.to_string().into_bytes()));
        let super::super::ports::DeliveryResult::Outcome(outcome) =
            super::super::ports::Ports::deliver(&mut native, &team, &row, &member, &id)
        else {
            panic!("{state}: no outcome")
        };
        h.outcome(&id, outcome);
        let recorded = h
            .store
            .effects(TEAM, 200, None)
            .unwrap()
            .rows
            .into_iter()
            .find(|e| e.request_id == id)
            .and_then(|e| e.outcome)
            .map(|v| serde_json::from_value::<HandoutOutcome>(v).unwrap())
            .expect("the recorded outcome");
        let reason = match recorded {
            HandoutOutcome::Refused { reason }
            | HandoutOutcome::Uncertain { reason }
            | HandoutOutcome::CuttingOver { reason } => reason,
            other => panic!("{state}: {other:?}"),
        };
        super::assert_clean(&[reason], state);
        let items = h.store.inbox(TEAM, 200, None).unwrap().rows;
        // Items keep their own line breaks; no other control character.
        assert!(
            items
                .iter()
                .all(|i| !i.text.chars().any(|c| c.is_control() && c != '\n')),
            "{state}: an inbox item carries a control character"
        );
    }
}

/// A non-start's sentence is cleaned as the other delivery sentences are:
/// decoded by the production deliver, recorded as the actor records it after
/// five non-starts (a refusal), read back from the store (zerus-63rru.58).
#[test]
fn a_non_starts_sentence_is_recorded_without_control_characters() {
    use super::*;
    let mut h = Harness::new();
    h.bind("m1");
    let node = h.add(&person(), 1);
    let id = h.intent(node, "m1");
    let team = h.store.team(TEAM).unwrap().unwrap();
    let row = h.store.node(TEAM, node).unwrap().unwrap();
    let member = h.store.member(TEAM, "m1").unwrap().unwrap();
    let mut native = super::production::native(&h);
    let reply = serde_json::json!({"schema":1,"id":id,"state":"not_started","error":super::hostile_sentence()});
    crate::desktop::teams::child::FIXTURE_EXIT
        .with(|s| *s.borrow_mut() = Some(reply.to_string().into_bytes()));
    let super::super::ports::DeliveryResult::NotStarted { reason } =
        super::super::ports::Ports::deliver(&mut native, &team, &row, &member, &id)
    else {
        panic!("no non-start")
    };
    super::assert_clean(std::slice::from_ref(&reason), "not_started");
    h.outcome(&id, HandoutOutcome::Refused { reason });
    let recorded = h
        .store
        .effects(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .find(|e| e.request_id == id)
        .and_then(|e| e.outcome)
        .map(|v| serde_json::from_value::<HandoutOutcome>(v).unwrap())
        .expect("the recorded outcome");
    let HandoutOutcome::Refused { reason } = recorded else {
        panic!("{recorded:?}")
    };
    super::assert_clean(&[reason], "not_started recorded");
}
