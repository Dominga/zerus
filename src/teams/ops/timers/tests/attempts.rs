//! The store follows the task's attempts: an observation carries the
//! highest attempt generation and the current attempt's ID, kept beside it.
use super::*;
use rusqlite::OptionalExtension;

fn stored(h: &Harness, node: u32) -> (u64, String) {
    let db = h.store.db.as_ref().unwrap();
    db.query_row(
        "SELECT attempt,attempt_id FROM task_observations WHERE team=?1 AND node=?2",
        rusqlite::params![TEAM, node],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .unwrap()
}

#[test]
fn an_observation_without_attempts_reads_as_none() {
    let old = r#"{"revision":"token-1","stage":"todo","rank":"a1","title":"Task"}"#;
    let o: TaskObservation = serde_json::from_str(old).unwrap();
    assert_eq!((o.attempt, o.attempt_id.as_str()), (0, ""));
    let cancel: Operation =
        serde_json::from_str(r#"{"op":"record_cancel","node":1,"decision":2,"state":"cancelled"}"#)
            .unwrap();
    assert!(matches!(cancel, Operation::RecordCancel { attempt: 0, .. }));
}

#[test]
fn an_observation_keeps_its_attempt_beside_its_data() {
    let mut h = Harness::new();
    let node = h.enqueue(&person(), 1);
    let mut o = h.observation(node, TaskStage::Todo);
    o.attempt = 3;
    o.attempt_id = "00000000-0000-4000-8000-00000000aaaa".into();
    h.run(
        &service(),
        Operation::ObserveTask {
            node,
            observation: o.clone(),
        },
    )
    .unwrap();
    assert_eq!(stored(&h, node), (3, o.attempt_id.clone()));
    let read = h.store.task_observation(TEAM, node).unwrap().unwrap();
    assert_eq!((read.attempt, read.attempt_id), (3, o.attempt_id));
}

#[test]
fn an_attempt_id_over_256_bytes_is_refused() {
    let mut h = Harness::new();
    let node = h.enqueue(&person(), 1);
    let mut o = h.observation(node, TaskStage::Todo);
    o.attempt_id = "a".repeat(257);
    refused(
        h.run(
            &service(),
            Operation::ObserveTask {
                node,
                observation: o,
            },
        ),
        code::OVER_CAP,
    );
}

#[test]
fn an_older_store_gains_the_attempt_columns_through_the_transition() {
    let mut h = Harness::new();
    let node = h.add(&person(), 1);
    let db = h.store.db.as_ref().unwrap();
    db.execute_batch("ALTER TABLE task_observations DROP COLUMN attempt; ALTER TABLE task_observations DROP COLUMN attempt_id; DROP TABLE timers_extension;").unwrap();
    h.store = Store::open(h.temp.path()).unwrap();
    assert_eq!(stored(&h, node), (0, String::new()));
}

#[test]
fn a_pending_cancel_names_the_attempt_the_store_holds() {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    let node = h.add(&person(), 1);
    let mut o = h.observation(node, TaskStage::Todo);
    o.attempt = 2;
    o.attempt_id = "00000000-0000-4000-8000-00000000aaaa".into();
    h.run(
        &service(),
        Operation::ObserveTask {
            node,
            observation: o.clone(),
        },
    )
    .unwrap();
    h.run(
        &lead,
        Operation::Cancel {
            node,
            reason: "Superseded".into(),
        },
    )
    .unwrap();
    let row = h.store.pending_cancels(TEAM, 1, 0).unwrap().remove(0);
    assert_eq!((row.attempt, row.attempt_id), (2, o.attempt_id));
    let decoded: Operation = serde_json::from_str(
        r#"{"op":"record_cancel","node":1,"decision":2,"state":"cancelled","attempt":2}"#,
    )
    .unwrap();
    assert!(
        matches!(decoded, Operation::RecordCancel { attempt: 2, ref attempt_id, .. } if attempt_id.is_empty())
    );
    let long = Operation::RecordCancel {
        node,
        decision: row.decision,
        state: CancelState::Cancelled,
        reason: String::new(),
        attempt: 2,
        attempt_id: "a".repeat(257),
    };
    refused(h.run(&service(), long), code::OVER_CAP);
}

const PERSON: &str = "00000000-0000-4000-8000-00000000bbbb";

fn seen(h: &mut Harness, node: u32, stage: TaskStage, attempt: u64, id: &str) -> Result<Reply> {
    let mut o = h.observation(node, stage);
    o.attempt = attempt;
    o.attempt_id = id.into();
    h.run(
        &service(),
        Operation::ObserveTask {
            node,
            observation: o,
        },
    )
}
/// m1 holds node 1, handed out by the team as attempt (1, effect ID).
fn handed() -> (Harness, Actor, u32, String) {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    h.bind("m1");
    let node = h.add(&person(), 1);
    let id = h.intent(node, "m1");
    h.outcome(
        &id,
        HandoutOutcome::Submitted {
            receipt: "receipt-example".into(),
        },
    );
    seen(&mut h, node, TaskStage::InProgress, 1, &id).unwrap();
    (h, lead, node, id)
}
/// Two attempts claim generation `attempt`: no current attempt, the flag.
fn conflicted(h: &mut Harness, node: u32, stage: TaskStage, attempt: u64) -> Result<Reply> {
    let mut o = h.observation(node, stage);
    o.attempt = attempt;
    o.attempt_id = String::new();
    o.attempt_conflict = true;
    h.run(
        &service(),
        Operation::ObserveTask {
            node,
            observation: o,
        },
    )
}
fn cancel(node: u32, decision: u64, attempt: u64, id: &str) -> Operation {
    Operation::RecordCancel {
        node,
        decision,
        state: CancelState::Cancelled,
        reason: String::new(),
        attempt,
        attempt_id: id.into(),
    }
}
/// The command's answer that the key was already gone: in a live team it is
/// the answer bound to an attempt that the store still applies.
fn released(node: u32, decision: u64, attempt: u64, id: &str) -> Operation {
    Operation::RecordCancel {
        node,
        decision,
        state: CancelState::Released,
        reason: String::new(),
        attempt,
        attempt_id: id.into(),
    }
}
fn last_event(h: &Harness) -> String {
    h.store
        .journal(TEAM, 200, None)
        .unwrap()
        .rows
        .last()
        .unwrap()
        .kind
        .clone()
}
fn bound(h: &Harness, node: u32) -> bool {
    let db = h.store.db.as_ref().unwrap();
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM live_tasks WHERE team=?1 AND node=?2)",
        rusqlite::params![TEAM, node],
        |r| r.get(0),
    )
    .unwrap()
}
/// What both orders must agree on.
fn state(h: &Harness, node: u32) -> (u32, bool, bool, String, NodeState) {
    let generation = h.store.generation(TEAM, node).unwrap().unwrap();
    let pending = !h.store.pending_cancels(TEAM, 200, 0).unwrap().is_empty();
    (
        generation,
        bound(h, node),
        pending,
        last_event(h),
        h.node(node).state,
    )
}

#[test]
fn the_reviewers_witness_ends_in_the_same_state_in_both_orders() {
    let mut ends = Vec::new();
    for observation_first in [true, false] {
        let (mut h, lead, node, id) = handed();
        h.run(
            &lead,
            Operation::Cancel {
                node,
                reason: "Superseded".into(),
            },
        )
        .unwrap();
        let decision = h
            .store
            .pending_cancels(TEAM, 1, 0)
            .unwrap()
            .remove(0)
            .decision;
        if observation_first {
            seen(&mut h, node, TaskStage::InProgress, 2, PERSON).unwrap();
            refused(
                h.run(&service(), cancel(node, decision, 1, &id)),
                code::INVALID_STATE,
            );
        } else {
            // A live node's task is observed instead: the answer writes nothing.
            refused(
                h.run(&service(), cancel(node, decision, 1, &id)),
                code::INVALID_STATE,
            );
            seen(&mut h, node, TaskStage::InProgress, 2, PERSON).unwrap();
        }
        ends.push(state(&h, node));
    }
    assert_eq!(ends[0], ends[1]);
    assert_eq!(
        ends[0],
        (
            2,
            true,
            false,
            "person_override".into(),
            NodeState::InProgress
        )
    );
}

#[test]
fn the_nodes_own_hand_out_or_reopen_is_never_a_person_attempt() {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    h.bind("m1");
    let node = h.add(&person(), 1);
    let id = h.intent(node, "m1");
    seen(&mut h, node, TaskStage::InProgress, 1, &id).unwrap();
    h.outcome(
        &id,
        HandoutOutcome::Submitted {
            receipt: "receipt-example".into(),
        },
    );
    seen(&mut h, node, TaskStage::InProgress, 1, &id).unwrap();
    assert_eq!(h.store.generation(TEAM, node).unwrap(), Some(1));
    h.report(node, "m1");
    h.run(
        &lead,
        Operation::Return {
            node,
            reason: "Fix it".into(),
        },
    )
    .unwrap();
    let reopen = h.reopen(node, "m1");
    seen(&mut h, node, TaskStage::InProgress, 2, &reopen).unwrap();
    h.reopen_outcome(
        &reopen,
        HandoutOutcome::Submitted {
            receipt: "receipt-example".into(),
        },
    );
    seen(&mut h, node, TaskStage::InProgress, 2, &reopen).unwrap();
    assert_eq!(h.store.generation(TEAM, node).unwrap(), Some(2));
    assert!(h
        .store
        .journal(TEAM, 200, None)
        .unwrap()
        .rows
        .iter()
        .all(|e| e.kind != "person_override"));
}

#[test]
fn a_persons_new_attempt_ends_the_earlier_one_whatever_is_pending() {
    for (case, paused) in [
        ("none", false),
        ("cancel", false),
        ("accept", false),
        ("none", true),
        ("accept", true),
    ] {
        let (mut h, lead, node, _) = handed();
        match case {
            "cancel" => drop(
                h.run(
                    &lead,
                    Operation::Cancel {
                        node,
                        reason: "Superseded".into(),
                    },
                )
                .unwrap(),
            ),
            "accept" => {
                h.report(node, "m1");
                h.run(&lead, Operation::Accept { node }).unwrap();
            }
            _ => (),
        }
        if paused {
            h.run(&person(), Operation::Pause).unwrap();
        }
        seen(&mut h, node, TaskStage::InProgress, 2, PERSON).unwrap();
        assert_eq!(h.store.generation(TEAM, node).unwrap(), Some(2), "{case}");
        assert_eq!(last_event(&h), "person_override", "{case}");
        assert!(
            h.store.pending_cancels(TEAM, 200, 0).unwrap().is_empty(),
            "{case}"
        );
        assert!(
            h.store
                .effects(TEAM, 200, None)
                .unwrap()
                .rows
                .iter()
                .all(|e| !matches!(e.state.as_str(), "reserved" | "submitted")),
            "{case}"
        );
        assert!(
            h.store
                .inbox(TEAM, 200, None)
                .unwrap()
                .rows
                .iter()
                .all(|i| i.state != "open"),
            "{case}"
        );
        assert!(
            bound(&h, node),
            "{case}: the task runs again under the team"
        );
    }
}

#[test]
fn a_stopped_team_restores_nothing() {
    let (mut h, _, node, _) = handed();
    h.run(&person(), Operation::Stop).unwrap();
    seen(&mut h, node, TaskStage::InProgress, 2, PERSON).unwrap();
    assert_eq!(h.store.generation(TEAM, node).unwrap(), Some(1));
    assert_ne!(last_event(&h), "person_override");
}

#[test]
fn a_repeated_attempt_changes_nothing_and_a_lower_one_is_stale() {
    let (mut h, _, node, id) = handed();
    seen(&mut h, node, TaskStage::InProgress, 1, &id).unwrap();
    assert_eq!(h.store.generation(TEAM, node).unwrap(), Some(1));
    refused(
        seen(&mut h, node, TaskStage::InProgress, 0, ""),
        code::STALE_REVISION,
    );
}

#[test]
fn the_current_attempt_can_change_while_the_highest_generation_stays() {
    // Stored (2, A) then observed (2, C), C no effect of the node: an override.
    let (mut h, _, node, a) = handed();
    seen(&mut h, node, TaskStage::InProgress, 2, &a).unwrap();
    assert_eq!(
        h.store.generation(TEAM, node).unwrap(),
        Some(1),
        "(1, A) then (2, A): only the number"
    );
    assert_eq!(
        h.store
            .task_observation(TEAM, node)
            .unwrap()
            .unwrap()
            .attempt,
        2
    );
    seen(&mut h, node, TaskStage::InProgress, 2, &a).unwrap();
    assert_eq!(
        h.store.generation(TEAM, node).unwrap(),
        Some(1),
        "(2, A) then (2, A): nothing"
    );
    seen(&mut h, node, TaskStage::InProgress, 2, PERSON).unwrap();
    assert_eq!(
        h.store.generation(TEAM, node).unwrap(),
        Some(2),
        "(2, A) then (2, C): an override"
    );
    assert_eq!(last_event(&h), "person_override");
}

#[test]
fn a_released_answer_applies_only_to_the_attempt_the_store_holds() {
    for (attempt, id, applies) in [
        (0, "", false),
        (2, "x", false),
        (1, "other", false),
        (1, "", false),
        (1, "held", true),
    ] {
        let (mut h, lead, node, held) = handed();
        h.run(
            &lead,
            Operation::Cancel {
                node,
                reason: "Superseded".into(),
            },
        )
        .unwrap();
        let decision = h
            .store
            .pending_cancels(TEAM, 1, 0)
            .unwrap()
            .remove(0)
            .decision;
        let id = if id == "held" {
            held.clone()
        } else {
            id.to_owned()
        };
        let rev = h.store.team(TEAM).unwrap().unwrap().revision;
        refused(
            h.run(&service(), cancel(node, decision, attempt, &id)),
            code::INVALID_STATE,
        );
        let result = h.run(&service(), released(node, decision, attempt, &id));
        assert_eq!(result.is_ok(), applies, "({attempt}, {id})");
        if !applies {
            assert_eq!(
                h.store.team(TEAM).unwrap().unwrap().revision,
                rev,
                "nothing written"
            );
            let refusal = Operation::RecordCancel {
                node,
                decision,
                state: CancelState::Refused,
                reason: "Delivery unknown".into(),
                attempt: 9,
                attempt_id: "any".into(),
            };
            h.run(&service(), refusal).unwrap();
        }
    }
}

#[test]
fn a_persons_attempt_once_taken_over_is_not_taken_over_again() {
    let (mut h, _, node, _) = handed();
    seen(&mut h, node, TaskStage::InProgress, 2, PERSON).unwrap();
    assert_eq!(h.store.generation(TEAM, node).unwrap(), Some(2));
    seen(&mut h, node, TaskStage::InProgress, 2, PERSON).unwrap();
    seen(&mut h, node, TaskStage::InProgress, 3, PERSON).unwrap();
    assert_eq!(
        h.store.generation(TEAM, node).unwrap(),
        Some(2),
        "the stored pair names it"
    );
}

#[test]
fn a_finished_node_still_in_the_live_plan_takes_todays_restore_once() {
    let (mut h, lead, node, _) = handed();
    h.report(node, "m1");
    h.run(&lead, Operation::Accept { node }).unwrap();
    h.observe(node, TaskStage::Done);
    assert_eq!(
        h.node(node).state,
        NodeState::Reported,
        "done, waiting for its merge"
    );
    seen(&mut h, node, TaskStage::InProgress, 2, PERSON).unwrap();
    assert_eq!(
        h.store.generation(TEAM, node).unwrap(),
        Some(2),
        "one restore, not two"
    );
    assert_eq!(last_event(&h), "person_override");
}

fn pair(h: &Harness, node: u32) -> (u64, String) {
    let o = h.store.task_observation(TEAM, node).unwrap().unwrap();
    (o.attempt, o.attempt_id)
}

#[test]
fn a_conflict_is_recorded_as_it_came_and_decides_nothing() {
    let (mut h, _, node, _) = handed();
    conflicted(&mut h, node, TaskStage::InProgress, 2).unwrap();
    assert_eq!(
        pair(&h, node),
        (2, String::new()),
        "the newest observation's pair"
    );
    assert_eq!(h.store.generation(TEAM, node).unwrap(), Some(1));
    assert_ne!(last_event(&h), "person_override");
}

#[test]
fn the_reviewers_conflict_blocks_a_late_cancel_and_the_next_answer_is_a_refusal() {
    let (mut h, lead, node, a) = handed();
    h.run(
        &lead,
        Operation::Cancel {
            node,
            reason: "Superseded".into(),
        },
    )
    .unwrap();
    let decision = h
        .store
        .pending_cancels(TEAM, 1, 0)
        .unwrap()
        .remove(0)
        .decision;
    conflicted(&mut h, node, TaskStage::InProgress, 2).unwrap();
    let row = h.store.pending_cancels(TEAM, 1, 0).unwrap().remove(0);
    assert_eq!(
        (row.attempt, row.attempt_id.as_str()),
        (2, ""),
        "team-cancel compares the conflict"
    );
    let rev = h.store.team(TEAM).unwrap().unwrap().revision;
    refused(
        h.run(&service(), released(node, decision, 1, &a)),
        code::INVALID_STATE,
    );
    refused(
        h.run(&service(), cancel(node, decision, 2, "")),
        code::INVALID_STATE,
    );
    assert_eq!(
        h.store.team(TEAM).unwrap().unwrap().revision,
        rev,
        "nothing written"
    );
    assert_eq!(
        (h.store.generation(TEAM, node).unwrap(), bound(&h, node)),
        (Some(1), true)
    );
    let refusal = Operation::RecordCancel {
        node,
        decision,
        state: CancelState::Refused,
        reason: "The delivery is in conflict".into(),
        attempt: 2,
        attempt_id: String::new(),
    };
    h.run(&service(), refusal).unwrap();
    assert!(
        h.store
            .inbox(TEAM, 200, None)
            .unwrap()
            .rows
            .iter()
            .any(|i| i.kind.starts_with("cancel_refused:") && i.state == "open"),
        "the lead is told"
    );
    assert!(bound(&h, node));
}

#[test]
fn a_conflict_that_settles_decides_as_any_observation() {
    // Back on A, the attempt the node runs under: nothing.
    let (mut h, _, node, a) = handed();
    conflicted(&mut h, node, TaskStage::InProgress, 2).unwrap();
    seen(&mut h, node, TaskStage::InProgress, 2, &a).unwrap();
    assert_eq!(
        (h.store.generation(TEAM, node).unwrap(), pair(&h, node)),
        (Some(1), (2, a.clone()))
    );
    // On another attempt: a person's execution.
    conflicted(&mut h, node, TaskStage::InProgress, 3).unwrap();
    seen(&mut h, node, TaskStage::InProgress, 3, PERSON).unwrap();
    assert_eq!(h.store.generation(TEAM, node).unwrap(), Some(2));
    // Back on the person's attempt after another conflict: no second override.
    conflicted(&mut h, node, TaskStage::InProgress, 4).unwrap();
    seen(&mut h, node, TaskStage::InProgress, 4, PERSON).unwrap();
    assert_eq!(
        h.store.generation(TEAM, node).unwrap(),
        Some(2),
        "the remembered attempt"
    );
    // On the node's own effect: the team's own.
    conflicted(&mut h, node, TaskStage::InProgress, 5).unwrap();
    seen(&mut h, node, TaskStage::InProgress, 5, &a).unwrap();
    assert_eq!(h.store.generation(TEAM, node).unwrap(), Some(2));
}

#[test]
fn a_conflict_on_a_node_with_a_pending_accept_decides_nothing() {
    let (mut h, lead, node, _) = handed();
    h.report(node, "m1");
    h.run(&lead, Operation::Accept { node }).unwrap();
    conflicted(&mut h, node, TaskStage::InProgress, 2).unwrap();
    assert_eq!(h.store.generation(TEAM, node).unwrap(), Some(1));
    assert!(h
        .store
        .decisions(TEAM, 200, None)
        .unwrap()
        .rows
        .iter()
        .any(|d| d.kind == "accept" && d.state != crate::teams::store::DecisionState::Settled));
}

#[test]
fn the_legacy_pair_takes_only_a_default_cancel() {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    h.bind("m1");
    let node = h.add(&person(), 1);
    h.submit(node, "m1");
    assert_eq!(pair(&h, node), (0, String::new()));
    h.run(
        &lead,
        Operation::Cancel {
            node,
            reason: "Superseded".into(),
        },
    )
    .unwrap();
    let decision = h
        .store
        .pending_cancels(TEAM, 1, 0)
        .unwrap()
        .remove(0)
        .decision;
    refused(
        h.run(&service(), released(node, decision, 1, "x")),
        code::INVALID_STATE,
    );
    h.run(&service(), released(node, decision, 0, "")).unwrap();
    assert_eq!(h.node(node).state, NodeState::Cancelled);
}

#[test]
fn the_conflict_flag_defaults_to_false_and_is_kept_with_the_observation() {
    let old = r#"{"revision":"token-1","stage":"todo","rank":"a1","attempt":2}"#;
    let o: TaskObservation = serde_json::from_str(old).unwrap();
    assert!(!o.attempt_conflict);
    let mut h = Harness::new();
    let node = h.enqueue(&person(), 1);
    let mut o = h.observation(node, TaskStage::Todo);
    o.attempt = 2;
    o.attempt_conflict = true;
    h.run(
        &service(),
        Operation::ObserveTask {
            node,
            observation: o,
        },
    )
    .unwrap();
    let db = h.store.db.as_ref().unwrap();
    let kept: bool = db
        .query_row(
            "SELECT attempt_conflict FROM task_observations WHERE team=?1 AND node=?2",
            rusqlite::params![TEAM, node],
            |r| r.get(0),
        )
        .unwrap();
    assert!(kept);
    assert!(
        h.store
            .task_observation(TEAM, node)
            .unwrap()
            .unwrap()
            .attempt_conflict
    );
}

/// A refused first hand-out leaves (1, empty) with no conflict: an ordinary
/// pair, applied like any other.
fn refused_hand_out() -> (Harness, Actor, u32) {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    h.bind("m1");
    let node = h.add(&person(), 1);
    let id = h.intent(node, "m1");
    h.outcome(
        &id,
        HandoutOutcome::Refused {
            reason: "Not delivered".into(),
        },
    );
    seen(&mut h, node, TaskStage::Todo, 1, "").unwrap();
    (h, lead, node)
}

#[test]
fn an_empty_id_without_the_flag_is_an_ordinary_pair_and_is_applied() {
    let (mut h, lead, node) = refused_hand_out();
    let mut o = h.observation(node, TaskStage::Todo);
    o.title = "Renamed".into();
    h.run(
        &service(),
        Operation::ObserveTask {
            node,
            observation: o,
        },
    )
    .unwrap();
    assert_eq!(h.node(node).title, "Renamed", "a title change is applied");
    seen(&mut h, node, TaskStage::Backlog, 1, "").unwrap();
    assert_eq!(
        h.node(node).state,
        NodeState::Waiting,
        "a stage change is applied"
    );
    seen(&mut h, node, TaskStage::Todo, 1, "").unwrap();
    h.run(
        &lead,
        Operation::Cancel {
            node,
            reason: "Superseded".into(),
        },
    )
    .unwrap();
    let row = h.store.pending_cancels(TEAM, 1, 0).unwrap().remove(0);
    assert_eq!((row.attempt, row.attempt_id.as_str()), (1, ""));
    h.run(&service(), released(node, row.decision, 1, ""))
        .unwrap();
    assert_eq!(
        h.node(node).state,
        NodeState::Cancelled,
        "a released answer with that pair ends the node"
    );
    let (mut h, _, node) = refused_hand_out();
    let mut o = h.observation(node, TaskStage::Todo);
    o.cancelled = true;
    h.run(
        &service(),
        Operation::ObserveTask {
            node,
            observation: o,
        },
    )
    .unwrap();
    assert_eq!(
        h.node(node).state,
        NodeState::Cancelled,
        "a person's cancel is applied"
    );
}

#[test]
fn after_a_stop_the_observed_pair_is_recorded_with_or_without_a_conflict() {
    // After a stop a conflict of the newest observation holds no cancelled
    // answer back: nothing observes again, and the command answers cancelled
    // for a conflicting document only when it is already cancelled.
    for flagged in [false, true] {
        let (mut h, _, node, _) = handed();
        if flagged {
            conflicted(&mut h, node, TaskStage::InProgress, 2).unwrap();
        } else {
            seen(&mut h, node, TaskStage::InProgress, 2, "").unwrap();
        }
        h.run(&person(), Operation::Stop).unwrap();
        let row = h.store.pending_cancels(TEAM, 1, 0).unwrap().remove(0);
        assert_eq!((row.attempt, row.attempt_id.as_str()), (2, ""));
        h.run(&service(), cancel(node, row.decision, 2, ""))
            .unwrap();
        assert!(h.store.pending_cancels(TEAM, 200, 0).unwrap().is_empty());
    }
}

#[test]
fn an_ended_task_ends_its_node_whoever_executed_it() {
    for stage in [TaskStage::Todo, TaskStage::Done] {
        let mut ends = Vec::new();
        for flagged in [false, true] {
            let (mut h, lead, node, _) = handed();
            h.run(
                &lead,
                Operation::Cancel {
                    node,
                    reason: "Superseded".into(),
                },
            )
            .unwrap();
            let mut o = h.observation(node, stage);
            o.cancelled = stage == TaskStage::Todo;
            o.attempt = 2;
            o.attempt_conflict = flagged;
            h.run(
                &service(),
                Operation::ObserveTask {
                    node,
                    observation: o,
                },
            )
            .unwrap();
            let pending = h.store.pending_cancels(TEAM, 200, 0).unwrap().len();
            ends.push((
                h.node(node).state,
                h.store.generation(TEAM, node).unwrap(),
                pending,
                bound(&h, node),
            ));
        }
        assert_eq!(ends[0], ends[1], "{stage:?}: as today");
        if stage == TaskStage::Todo {
            assert_eq!(
                (ends[1].0, ends[1].2),
                (NodeState::Cancelled, 0),
                "the pending cancel settles"
            );
        }
    }
}

fn remembered(h: &Harness, node: u32) -> String {
    let db = h.store.db.as_ref().unwrap();
    db.query_row(
        "SELECT last_attempt_id FROM task_observations WHERE team=?1 AND node=?2",
        rusqlite::params![TEAM, node],
        |r| r.get(0),
    )
    .unwrap()
}

/// The reviewer's sequence: the team's attempt runs; a person finishes the
/// task, moves it back to To do and reserves a send P whose delivery is
/// unknown; the store observes P while the task does not run, then P runs.
/// P was never taken in, so it is judged when it starts.
#[test]
fn a_persons_attempt_observed_before_it_runs_is_taken_over_when_it_runs() {
    for case in ["cancel", "none", "reopen"] {
        let (mut h, lead, node, id) = handed();
        let mut attempt = 2;
        match case {
            "cancel" => drop(
                h.run(
                    &lead,
                    Operation::Cancel {
                        node,
                        reason: "Superseded".into(),
                    },
                )
                .unwrap(),
            ),
            "reopen" => {
                h.report(node, "m1");
                h.run(
                    &lead,
                    Operation::Return {
                        node,
                        reason: "Fix it".into(),
                    },
                )
                .unwrap();
                drop(h.reopen(node, "m1"));
                attempt = 3;
            }
            _ => (),
        }
        let before = h.store.generation(TEAM, node).unwrap().unwrap();
        seen(&mut h, node, TaskStage::Todo, attempt, PERSON).unwrap();
        assert_eq!(remembered(&h, node), id, "{case}: P is not taken in");
        assert_eq!(h.store.generation(TEAM, node).unwrap(), Some(before));
        seen(&mut h, node, TaskStage::InProgress, attempt, PERSON).unwrap();
        assert_eq!(
            state(&h, node),
            (
                before + 1,
                true,
                false,
                "person_override".into(),
                NodeState::InProgress
            ),
            "{case}"
        );
        assert!(
            h.store
                .effects(TEAM, 200, None)
                .unwrap()
                .rows
                .iter()
                .all(|e| !matches!(e.state.as_str(), "reserved" | "submitted")),
            "{case}: nothing of the earlier attempt stays open"
        );
        assert_eq!(remembered(&h, node), PERSON, "{case}: now taken in");
    }
}

/// A person's send that is reserved and never starts, then the node's own
/// attempt current again: nothing changes.
#[test]
fn a_reserved_attempt_that_never_starts_changes_nothing() {
    let (mut h, _, node, id) = handed();
    seen(&mut h, node, TaskStage::Todo, 2, PERSON).unwrap();
    seen(&mut h, node, TaskStage::InProgress, 2, &id).unwrap();
    assert_eq!(h.store.generation(TEAM, node).unwrap(), Some(1));
    assert_eq!(remembered(&h, node), id);
    assert!(bound(&h, node));
    assert!(h
        .store
        .journal(TEAM, 200, None)
        .unwrap()
        .rows
        .iter()
        .all(|e| e.kind != "person_override"));
}

/// The team's own effect observed while the task does not run yet is taken
/// in at once, and no override follows when it runs.
#[test]
fn the_teams_own_effect_is_remembered_before_it_runs() {
    let mut h = Harness::new();
    h.bind("lead");
    h.bind("m1");
    let node = h.add(&person(), 1);
    let id = h.intent(node, "m1");
    seen(&mut h, node, TaskStage::Todo, 1, &id).unwrap();
    assert_eq!(remembered(&h, node), id);
    h.outcome(
        &id,
        HandoutOutcome::Submitted {
            receipt: "receipt-example".into(),
        },
    );
    seen(&mut h, node, TaskStage::InProgress, 1, &id).unwrap();
    assert_eq!(h.store.generation(TEAM, node).unwrap(), Some(1));
    assert!(h
        .store
        .journal(TEAM, 200, None)
        .unwrap()
        .rows
        .iter()
        .all(|e| e.kind != "person_override"));
}

/// Today's restore of a finished node by a task that does not run takes no
/// execution in: the person's attempt is judged when it starts.
#[test]
fn a_restore_by_a_task_that_does_not_run_remembers_nothing() {
    let (mut h, lead, node, id) = handed();
    h.report(node, "m1");
    h.run(&lead, Operation::Accept { node }).unwrap();
    h.observe(node, TaskStage::Done);
    seen(&mut h, node, TaskStage::Todo, 2, PERSON).unwrap();
    assert_eq!(h.store.generation(TEAM, node).unwrap(), Some(2));
    assert_eq!(remembered(&h, node), id);
    seen(&mut h, node, TaskStage::InProgress, 2, PERSON).unwrap();
    assert_eq!(h.store.generation(TEAM, node).unwrap(), Some(3));
    assert_eq!(remembered(&h, node), PERSON);
    seen(&mut h, node, TaskStage::InProgress, 2, PERSON).unwrap();
    assert_eq!(h.store.generation(TEAM, node).unwrap(), Some(3), "once");
}

/// A store from before the extension holds no attempt of any task: after
/// the upgrade its first observation of a pair is judged against no
/// attempt. The node's own effect is the team's; a person's running attempt
/// is taken over.
#[test]
fn a_store_from_before_the_extension_judges_the_first_pair_against_no_attempt() {
    let (mut h, _, node, id) = handed();
    let db = h.store.db.as_ref().unwrap();
    for column in [
        "attempt_conflict",
        "last_attempt_id",
        "attempt_id",
        "attempt",
    ] {
        db.execute_batch(&format!("ALTER TABLE task_observations DROP COLUMN {column}; UPDATE task_observations SET data=json_remove(data,'$.{column}');")).unwrap();
    }
    db.execute_batch("DROP TABLE timers_extension;").unwrap();
    h.store = Store::open(h.temp.path()).unwrap();
    assert_eq!(
        (stored(&h, node), remembered(&h, node)),
        ((0, String::new()), String::new())
    );
    seen(&mut h, node, TaskStage::InProgress, 1, &id).unwrap();
    assert_eq!(
        h.store.generation(TEAM, node).unwrap(),
        Some(1),
        "the team's own"
    );
    seen(&mut h, node, TaskStage::InProgress, 2, PERSON).unwrap();
    assert_eq!(
        h.store.generation(TEAM, node).unwrap(),
        Some(2),
        "a person's"
    );
    assert_eq!(last_event(&h), "person_override");
}

/// A conflict recorded on a finished node changes nothing of it; the
/// observation that settles it then finds the node finished by its own
/// projection and takes today's restore, not a silent reopen.
#[test]
fn a_conflict_on_a_finished_node_then_a_running_task_takes_todays_restore() {
    let (mut h, lead, node, id) = handed();
    h.report(node, "m1");
    h.run(&lead, Operation::Accept { node }).unwrap();
    h.observe(node, TaskStage::Done);
    conflicted(&mut h, node, TaskStage::InProgress, 2).unwrap();
    assert_eq!(h.node(node).state, NodeState::Reported, "recorded only");
    seen(&mut h, node, TaskStage::InProgress, 2, &id).unwrap();
    assert_eq!(h.store.generation(TEAM, node).unwrap(), Some(2));
    assert_eq!(last_event(&h), "person_override");
    assert_eq!(h.node(node).state, NodeState::InProgress);
}

fn in_queue(h: &Harness, node: u32) -> bool {
    h.store
        .queue(TEAM, 200, None)
        .unwrap()
        .rows
        .iter()
        .any(|n| n.id == node)
}

/// Finding 15: the team's attempt reported and was accepted, the Done
/// observation left the node waiting for its merge, then the task showed
/// conflicting attempts. A late merge for that accept is recorded, and the
/// node stays in the live queue: the service observes only the live queue,
/// and the next observation decides.
#[test]
fn a_late_merge_under_a_conflict_keeps_the_node_for_the_next_observation() {
    for next in ["done", "person", "own"] {
        let (mut h, lead, node, id) = handed();
        h.report(node, "m1");
        h.run(&lead, Operation::Accept { node }).unwrap();
        h.observe(node, TaskStage::Done);
        assert_eq!(h.node(node).state, NodeState::Reported);
        conflicted(&mut h, node, TaskStage::InProgress, 2).unwrap();
        let decision = h.accept_decision(node);
        h.run(
            &service(),
            Operation::RecordIntegration {
                decision,
                node,
                commit: COMMIT.into(),
            },
        )
        .unwrap();
        let merged = h.node(node);
        assert!(merged.integrated, "{next}: the merge is recorded");
        assert_eq!(merged.state, NodeState::Accepted, "{next}");
        assert!(in_queue(&h, node), "{next}: still in the live queue");
        assert!(bound(&h, node), "{next}");
        match next {
            "done" => {
                seen(&mut h, node, TaskStage::Done, 2, &id).unwrap();
                assert!(!in_queue(&h, node), "Done again: it leaves the queue");
                assert_eq!(h.node(node).state, NodeState::Accepted);
                assert_eq!(h.store.generation(TEAM, node).unwrap(), Some(1));
            }
            "person" | "own" => {
                let attempt = if next == "person" {
                    PERSON
                } else {
                    id.as_str()
                };
                seen(&mut h, node, TaskStage::InProgress, 2, attempt).unwrap();
                assert_eq!(h.store.generation(TEAM, node).unwrap(), Some(2), "{next}");
                assert_eq!(last_event(&h), "person_override", "{next}");
                assert!(in_queue(&h, node), "{next}");
                assert!(!h.node(node).integrated, "{next}: a new attempt");
                assert_eq!(h.node(node).state, NodeState::InProgress, "{next}");
            }
            _ => unreachable!(),
        }
    }
}

/// The same with a conflict of a task that is Done: the end is applied, but
/// the attempts conflict, so the newest observation does not agree yet.
#[test]
fn a_merge_under_a_conflict_of_a_done_task_keeps_the_node() {
    let (mut h, lead, node, id) = handed();
    h.report(node, "m1");
    h.run(&lead, Operation::Accept { node }).unwrap();
    conflicted(&mut h, node, TaskStage::Done, 2).unwrap();
    let decision = h.accept_decision(node);
    h.run(
        &service(),
        Operation::RecordIntegration {
            decision,
            node,
            commit: COMMIT.into(),
        },
    )
    .unwrap();
    assert_eq!(h.node(node).state, NodeState::Accepted);
    assert!(in_queue(&h, node), "the attempts still conflict");
    seen(&mut h, node, TaskStage::Done, 2, &id).unwrap();
    assert!(!in_queue(&h, node), "the observation agrees");
}

fn outcome(h: &Harness, decision: u64) -> Option<String> {
    let db = h.store.db.as_ref().unwrap();
    db.query_row(
        "SELECT state FROM cancel_outcomes WHERE decision=?1",
        [decision],
        |r| r.get(0),
    )
    .optional()
    .unwrap()
}

/// Finding 17: a release reports that the key is gone, not who executed an
/// attempt. Its pair must equal the newest observation's, a conflict's
/// (N, empty) included, and it is recorded although that observation is a
/// conflict: in a running team the node ends and its binding goes, after a
/// stop the cleanup of the key proceeds.
#[test]
fn a_release_is_recorded_under_a_conflict_when_its_pair_is_equal() {
    for stopped in [false, true] {
        let (mut h, lead, node, id) = handed();
        if !stopped {
            h.run(
                &lead,
                Operation::Cancel {
                    node,
                    reason: "Superseded".into(),
                },
            )
            .unwrap();
        }
        conflicted(&mut h, node, TaskStage::Todo, 2).unwrap();
        if stopped {
            h.run(&person(), Operation::Stop).unwrap();
        }
        let row = h.store.pending_cancels(TEAM, 1, 0).unwrap().remove(0);
        assert_eq!((row.attempt, row.attempt_id.as_str()), (2, ""));
        refused(
            h.run(&service(), released(node, row.decision, 1, &id)),
            code::INVALID_STATE,
        );
        h.run(&service(), released(node, row.decision, 2, ""))
            .unwrap();
        assert_eq!(outcome(&h, row.decision).as_deref(), Some("released"));
        assert!(h.store.pending_cancels(TEAM, 200, 0).unwrap().is_empty());
        assert!(!bound(&h, node));
        if stopped {
            let keyed: Vec<u32> = h
                .store
                .keyed_tasks(TEAM, 0, 200)
                .unwrap()
                .iter()
                .map(|k| k.node)
                .collect();
            assert_eq!(keyed, vec![node], "the cleanup proceeds");
        } else {
            assert_eq!(h.node(node).state, NodeState::Cancelled);
        }
    }
}

/// A node kept in the queue by finding 15: accepted, merged, its newest
/// observation a conflict of its task running.
fn kept() -> (Harness, Actor, u32) {
    let (mut h, lead, node, _) = handed();
    h.report(node, "m1");
    h.run(&lead, Operation::Accept { node }).unwrap();
    h.observe(node, TaskStage::Done);
    conflicted(&mut h, node, TaskStage::InProgress, 2).unwrap();
    let decision = h.accept_decision(node);
    h.run(
        &service(),
        Operation::RecordIntegration {
            decision,
            node,
            commit: COMMIT.into(),
        },
    )
    .unwrap();
    assert!(in_queue(&h, node));
    (h, lead, node)
}
fn keyed(h: &Harness) -> Vec<u32> {
    h.store
        .keyed_tasks(TEAM, 0, 200)
        .unwrap()
        .iter()
        .map(|k| k.node)
        .collect()
}
fn archived(h: &Harness, node: u32) -> bool {
    let db = h.store.db.as_ref().unwrap();
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM node_archive WHERE team=?1 AND id=?2)",
        rusqlite::params![TEAM, node],
        |r| r.get(0),
    )
    .unwrap()
}

/// Finding 19: the person stops the team while a node is kept. The stop's
/// pending cancel is settled by its own answer, never by the archive: until
/// then the cancel step finds it and the key-clear frontier does not hold
/// it; after the answer the node is archived and enters the frontier once.
/// A cancelled answer under the conflict is refused by the gate.
#[test]
fn a_kept_node_waits_for_the_answer_to_its_stop_cancel() {
    for state in [
        CancelState::Cancelled,
        CancelState::Released,
        CancelState::Refused,
    ] {
        let (mut h, _, node) = kept();
        h.run(&person(), Operation::Stop).unwrap();
        let rows = h.store.pending_cancels(TEAM, 200, 0).unwrap();
        assert_eq!(rows.iter().map(|r| r.node).collect::<Vec<_>>(), vec![node]);
        assert!(!keyed(&h).contains(&node), "not yet in the frontier");
        assert!(!archived(&h, node));
        let row = rows[0].clone();
        let answer = Operation::RecordCancel {
            node,
            decision: row.decision,
            state,
            reason: if state == CancelState::Refused {
                "Delivery unknown".into()
            } else {
                String::new()
            },
            attempt: row.attempt,
            attempt_id: row.attempt_id.clone(),
        };
        h.run(&service(), answer).unwrap();
        assert!(h.store.pending_cancels(TEAM, 200, 0).unwrap().is_empty());
        assert!(archived(&h, node), "{state:?}");
        assert_eq!(keyed(&h), vec![node], "{state:?}: in the frontier once");
    }
}

/// In a running team a kept node takes no Cancel: it is accepted, and no
/// one cancels a decided node, so the stop's is the only cancel it can wait
/// for. It stays in the queue, with no cancel pending, across operations.
#[test]
fn a_kept_node_in_a_running_team_takes_no_cancel() {
    let (mut h, lead, node) = kept();
    for actor in [lead, person()] {
        let cancel = Operation::Cancel {
            node,
            reason: "Superseded".into(),
        };
        let refusal = h.run(&actor, cancel).unwrap_err();
        assert_eq!(refusal.message, "Task is already decided.");
    }
    h.run(&person(), Operation::Pause).unwrap();
    h.run(&person(), Operation::Resume).unwrap();
    assert!(h.store.pending_cancels(TEAM, 200, 0).unwrap().is_empty());
    assert!(in_queue(&h, node));
    assert!(bound(&h, node));
}

/// An ordinary accepted node whose Done observation agrees leaves at once.
#[test]
fn an_accepted_and_observed_node_is_still_archived_at_once() {
    let (mut h, lead, node, _) = handed();
    h.report(node, "m1");
    h.run(&lead, Operation::Accept { node }).unwrap();
    let decision = h.accept_decision(node);
    h.run(
        &service(),
        Operation::RecordIntegration {
            decision,
            node,
            commit: COMMIT.into(),
        },
    )
    .unwrap();
    h.observe(node, TaskStage::Done);
    assert!(archived(&h, node));
    assert!(!in_queue(&h, node));
}

fn reviewed(h: &Harness, node: u32, generation: u32) -> bool {
    let reviewed = h
        .store
        .reviewed_task_current(TEAM, node, generation)
        .unwrap();
    // Wherever the reviewed read holds, the accepted one does too.
    assert!(!reviewed || accepted(h, node, generation));
    reviewed
}
fn accepted(h: &Harness, node: u32, generation: u32) -> bool {
    h.store
        .accepted_task_current(TEAM, node, generation)
        .unwrap()
}
/// A row no writer leaves, built to pin one clause of the read: the
/// binding back on a node whose observation released it.
fn rebind(h: &Harness, node: u32) {
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute(
            "INSERT OR IGNORE INTO live_tasks SELECT task_project,task_uuid,team,id FROM node_identity WHERE team=?1 AND id=?2",
            rusqlite::params![TEAM, node],
        )
        .unwrap();
}

/// The actor's read: true for a reported task under the attempt the node
/// took in, the team's own or a person's it took over, and under the
/// legacy pair on both sides.
#[test]
fn a_reviewed_task_is_current_under_the_attempt_the_node_took_in() {
    let (mut h, _, node, id) = handed();
    assert!(!reviewed(&h, node, 1), "running");
    seen(&mut h, node, TaskStage::InReview, 1, &id).unwrap();
    assert!(reviewed(&h, node, 1), "the team's own attempt");
    assert!(!reviewed(&h, node, 2), "another generation named");
    assert!(!reviewed(&h, node, 0), "an earlier generation named");

    let (mut h, _, node, _) = handed();
    seen(&mut h, node, TaskStage::InProgress, 2, PERSON).unwrap();
    seen(&mut h, node, TaskStage::InReview, 2, PERSON).unwrap();
    assert!(reviewed(&h, node, 2), "a person's attempt taken over");
    assert!(
        !reviewed(&h, node, 1),
        "the generation before the take-over"
    );

    let mut h = Harness::new();
    h.bind("lead");
    h.bind("m1");
    let node = h.add(&person(), 1);
    h.submit(node, "m1");
    assert_eq!(pair(&h, node), (0, String::new()));
    h.observe(node, TaskStage::InReview);
    assert!(reviewed(&h, node, 1), "the legacy pair on both sides");
}

#[test]
fn a_reviewed_task_is_not_current_otherwise() {
    // Another current attempt: the same number, another ID.
    let (mut h, _, node, _) = handed();
    seen(&mut h, node, TaskStage::InReview, 1, "another-attempt").unwrap();
    assert!(!reviewed(&h, node, 1), "another current attempt");
    // A conflict.
    let (mut h, _, node, _) = handed();
    conflicted(&mut h, node, TaskStage::InReview, 2).unwrap();
    assert!(!reviewed(&h, node, 1), "a conflict");
    // A conflict of a row without attempts would match the legacy pair.
    let mut h = Harness::new();
    h.bind("m1");
    let node = h.add(&person(), 1);
    h.submit(node, "m1");
    conflicted(&mut h, node, TaskStage::InReview, 0).unwrap();
    assert!(!reviewed(&h, node, 1), "a conflict under the legacy pair");
    // An ordinary pair without a current attempt is not the legacy pair.
    let (mut h, _, node) = refused_hand_out();
    seen(&mut h, node, TaskStage::InReview, 1, "").unwrap();
    assert!(!reviewed(&h, node, 1), "(1, empty) without the flag");
    // Done, waiting for its merge, still bound.
    let (mut h, lead, node, _) = handed();
    h.report(node, "m1");
    h.run(&lead, Operation::Accept { node }).unwrap();
    h.observe(node, TaskStage::Done);
    assert!(bound(&h, node));
    assert!(!reviewed(&h, node, 1), "Done");
    // Cancelled: its observation releases the binding; rebound to pin the
    // clause itself.
    let (mut h, _, node, id) = handed();
    let mut o = h.observation(node, TaskStage::InReview);
    (o.attempt, o.attempt_id, o.cancelled) = (1, id.clone(), true);
    h.run(
        &service(),
        Operation::ObserveTask {
            node,
            observation: o,
        },
    )
    .unwrap();
    assert!(!reviewed(&h, node, 1), "cancelled");
    rebind(&h, node);
    assert!(!reviewed(&h, node, 1), "cancelled, bound");
    // The key left: the binding goes; rebound to pin the key clause.
    let (mut h, _, node, id) = handed();
    let mut o = h.observation(node, TaskStage::InReview);
    (o.attempt, o.attempt_id, o.key) = (1, id.clone(), KeyState::Absent);
    h.run(
        &service(),
        Operation::ObserveTask {
            node,
            observation: o,
        },
    )
    .unwrap();
    assert!(!reviewed(&h, node, 1), "the key left");
    rebind(&h, node);
    assert!(!reviewed(&h, node, 1), "the key left, bound");
    // No bound task: a released answer ends the node and its binding, the
    // observation still In review.
    let (mut h, lead, node, id) = handed();
    seen(&mut h, node, TaskStage::InReview, 1, &id).unwrap();
    h.run(
        &lead,
        Operation::Cancel {
            node,
            reason: "Superseded".into(),
        },
    )
    .unwrap();
    let decision = h
        .store
        .pending_cancels(TEAM, 1, 0)
        .unwrap()
        .remove(0)
        .decision;
    h.run(&service(), released(node, decision, 1, &id)).unwrap();
    assert!(!bound(&h, node));
    assert!(!reviewed(&h, node, 1), "no bound task");
    // No observation.
    let (mut h, _, node, id) = handed();
    seen(&mut h, node, TaskStage::InReview, 1, &id).unwrap();
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute(
            "DELETE FROM task_observations WHERE team=?1 AND node=?2",
            rusqlite::params![TEAM, node],
        )
        .unwrap();
    assert!(!reviewed(&h, node, 1), "no observation");
}

/// The actor's read for new merge writes: an observation agrees with an
/// acceptance when the task is In review under the execution the node
/// took in, or when it is Done, whoever executed it.
#[test]
fn an_accepted_task_is_current_in_review_or_done() {
    // Done before its merge, under the node's own attempt and under another
    // current attempt: still bound.
    for attempt in ["own", "another"] {
        let (mut h, lead, node, id) = handed();
        h.report(node, "m1");
        h.run(&lead, Operation::Accept { node }).unwrap();
        let current = if attempt == "own" {
            id.as_str()
        } else {
            PERSON
        };
        seen(&mut h, node, TaskStage::Done, 2, current).unwrap();
        assert!(bound(&h, node), "{attempt}");
        assert!(!reviewed(&h, node, 1), "{attempt}");
        assert!(accepted(&h, node, 1), "{attempt}");
        assert!(!accepted(&h, node, 0), "{attempt}: an earlier generation");
    }
    // In review under the node's own attempt, as the reviewed read.
    let (mut h, _, node, id) = handed();
    seen(&mut h, node, TaskStage::InReview, 1, &id).unwrap();
    assert!(reviewed(&h, node, 1) && accepted(&h, node, 1));
    // In review under another current attempt: no.
    seen(&mut h, node, TaskStage::InReview, 1, "another-attempt").unwrap();
    assert!(!accepted(&h, node, 1), "In review under another attempt");
}

#[test]
fn an_accepted_task_is_not_current_otherwise() {
    // Done with the conflict flag.
    let (mut h, lead, node, _) = handed();
    h.report(node, "m1");
    h.run(&lead, Operation::Accept { node }).unwrap();
    conflicted(&mut h, node, TaskStage::Done, 2).unwrap();
    assert!(bound(&h, node));
    assert!(!accepted(&h, node, 1), "Done with a conflict");
    // Running, and To do.
    let (mut h, _, node, id) = handed();
    assert!(!accepted(&h, node, 1), "running");
    seen(&mut h, node, TaskStage::Todo, 1, &id).unwrap();
    assert!(!accepted(&h, node, 1), "To do");
    // Cancelled: rebound to pin the clause itself.
    let (mut h, _, node, id) = handed();
    let mut o = h.observation(node, TaskStage::Done);
    (o.attempt, o.attempt_id, o.cancelled) = (1, id.clone(), true);
    h.run(
        &service(),
        Operation::ObserveTask {
            node,
            observation: o,
        },
    )
    .unwrap();
    rebind(&h, node);
    assert!(!accepted(&h, node, 1), "cancelled");
    // The key left: rebound to pin the key clause.
    let (mut h, _, node, id) = handed();
    let mut o = h.observation(node, TaskStage::Done);
    (o.attempt, o.attempt_id, o.key) = (1, id.clone(), KeyState::Absent);
    h.run(
        &service(),
        Operation::ObserveTask {
            node,
            observation: o,
        },
    )
    .unwrap();
    assert!(!accepted(&h, node, 1), "the key left");
    rebind(&h, node);
    assert!(!accepted(&h, node, 1), "the key left, bound");
    // No bound task, and no observation.
    let (mut h, lead, node, id) = handed();
    h.report(node, "m1");
    h.run(&lead, Operation::Accept { node }).unwrap();
    seen(&mut h, node, TaskStage::Done, 1, &id).unwrap();
    assert!(accepted(&h, node, 1));
    let db = h.store.db.as_ref().unwrap();
    db.execute(
        "DELETE FROM live_tasks WHERE team=?1 AND node=?2",
        rusqlite::params![TEAM, node],
    )
    .unwrap();
    assert!(!accepted(&h, node, 1), "no bound task");
    rebind(&h, node);
    db.execute(
        "DELETE FROM task_observations WHERE team=?1 AND node=?2",
        rusqlite::params![TEAM, node],
    )
    .unwrap();
    assert!(!accepted(&h, node, 1), "no observation");
}

fn own(h: &Harness, node: u32, generation: u32) -> bool {
    h.store.own_task_current(TEAM, node, generation).unwrap()
}

/// The read for a reopen and a hand-over: the newest observation leaves the
/// execution the node took in uncontradicted, at any stage.
#[test]
fn an_own_task_is_current_at_any_stage_under_the_attempt_the_node_took_in() {
    for stage in [TaskStage::InProgress, TaskStage::InReview, TaskStage::Done] {
        // The node's own attempt.
        let (mut h, lead, node, id) = handed();
        if stage == TaskStage::Done {
            h.report(node, "m1");
            h.run(&lead, Operation::Accept { node }).unwrap();
        }
        seen(&mut h, node, stage, 1, &id).unwrap();
        assert!(own(&h, node, 1), "{stage:?}: own attempt");
        assert!(!own(&h, node, 0), "{stage:?}: an earlier generation named");
        // A person's attempt the node took over.
        let (mut h, _, node, _) = handed();
        seen(&mut h, node, TaskStage::InProgress, 2, PERSON).unwrap();
        if stage != TaskStage::InProgress {
            seen(&mut h, node, stage, 2, PERSON).unwrap();
        }
        assert!(own(&h, node, 2), "{stage:?}: a person's attempt taken over");
        // A conflict at this stage.
        let (mut h, _, node, _) = handed();
        conflicted(&mut h, node, stage, 2).unwrap();
        if stage != TaskStage::Done {
            assert!(!own(&h, node, 1), "{stage:?}: a conflict");
        }
        // Another current attempt: the same number, another ID.
        let (mut h, _, node, _) = handed();
        seen(&mut h, node, TaskStage::InReview, 1, "another-attempt").unwrap();
        assert!(!own(&h, node, 1), "another current attempt");
    }
    // A Done task under a conflict: applied as an end, still a conflict.
    let (mut h, lead, node, _) = handed();
    h.report(node, "m1");
    h.run(&lead, Operation::Accept { node }).unwrap();
    conflicted(&mut h, node, TaskStage::Done, 2).unwrap();
    assert!(bound(&h, node));
    assert!(!own(&h, node, 1), "Done under a conflict");
    // No current attempt and no conflict: a refused first hand-out.
    let (mut h, _, node) = refused_hand_out();
    seen(&mut h, node, TaskStage::Todo, 1, "").unwrap();
    assert!(own(&h, node, 1), "no current attempt");
    // The legacy pair.
    let mut h = Harness::new();
    h.bind("m1");
    let node = h.add(&person(), 1);
    h.submit(node, "m1");
    assert!(own(&h, node, 1), "the legacy pair");
}

#[test]
fn an_own_task_is_not_current_otherwise() {
    // Cancelled: rebound to pin the clause.
    let (mut h, _, node, id) = handed();
    let mut o = h.observation(node, TaskStage::InProgress);
    (o.attempt, o.attempt_id, o.cancelled) = (1, id.clone(), true);
    h.run(
        &service(),
        Operation::ObserveTask {
            node,
            observation: o,
        },
    )
    .unwrap();
    rebind(&h, node);
    assert!(!own(&h, node, 1), "cancelled");
    // The key left: rebound to pin the key clause.
    let (mut h, _, node, id) = handed();
    let mut o = h.observation(node, TaskStage::InProgress);
    (o.attempt, o.attempt_id, o.key) = (1, id.clone(), KeyState::Absent);
    h.run(
        &service(),
        Operation::ObserveTask {
            node,
            observation: o,
        },
    )
    .unwrap();
    assert!(!own(&h, node, 1), "the key left");
    rebind(&h, node);
    assert!(!own(&h, node, 1), "the key left, bound");
    // No binding, and no observation.
    let (h, _, node, _) = handed();
    let db = h.store.db.as_ref().unwrap();
    db.execute(
        "DELETE FROM live_tasks WHERE team=?1 AND node=?2",
        rusqlite::params![TEAM, node],
    )
    .unwrap();
    assert!(!own(&h, node, 1), "no binding");
    rebind(&h, node);
    assert!(own(&h, node, 1));
    db.execute(
        "DELETE FROM task_observations WHERE team=?1 AND node=?2",
        rusqlite::params![TEAM, node],
    )
    .unwrap();
    assert!(!own(&h, node, 1), "no observation");
}
