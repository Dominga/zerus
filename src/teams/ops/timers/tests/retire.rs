//! zerus-63rru.38: a stage's retirement. A retired stage whose task is
//! bound may still run: it keeps one unsettled cancel (a person's pending
//! one, or the retirement's own) and waits where the cancel step finds it;
//! that cancel's answer or its task's observation ends it, and only then is
//! it archived and its key cleared. A stage without a bound task ends at
//! once, as before.
use super::*;
use crate::teams::ops::CancelState;

const PERSON: &str = "00000000-0000-4000-8000-00000000bbbb";

fn op(value: serde_json::Value) -> Operation {
    serde_json::from_value(value).unwrap()
}
fn pipeline() -> Harness {
    let mut h = Harness::empty();
    let mut create = create();
    if let Operation::Create { bundle, .. } = &mut create {
        let catalog = zerus_team_plan::builtin();
        bundle.roles = catalog.roles;
        bundle.pipelines = catalog.pipelines;
        bundle.template.default_pipeline = "reviewed".into();
    }
    h.run(&person(), create).unwrap();
    for member in ["lead", "m1", "m2", "m3"] {
        h.bind(member);
    }
    h
}
/// A reported parent, its review stage wanted, and that stage.
fn staged(h: &mut Harness) -> (u32, u32) {
    let parent = h.add(&person(), 1);
    h.submit(parent, "m1");
    h.report(parent, "m1");
    let stage = h
        .store
        .queue(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .find(|s| s.stage_of == Some(parent))
        .unwrap()
        .id;
    (parent, stage)
}
/// The stage's task created and bound; `running`: handed out to m3.
fn bound_stage(running: bool) -> (Harness, Actor, u32, u32) {
    let mut h = pipeline();
    let lead = Actor::Session {
        person: "local".into(),
        team: TEAM.into(),
        member: "lead".into(),
        session: h
            .store
            .member(TEAM, "lead")
            .unwrap()
            .unwrap()
            .session
            .unwrap(),
    };
    let (parent, stage) = staged(&mut h);
    let id = uuid::Uuid::new_v4().to_string();
    h.run(
        &service(),
        op(serde_json::json!({"op":"stage_task_intent","node":stage,"effect_request_id":id})),
    )
    .unwrap();
    h.run(
        &service(),
        op(serde_json::json!({"op":"stage_task_outcome","effect_request_id":id,"outcome":{"status":"created","task":task(1000 + stage)}})),
    )
    .unwrap();
    h.observe(stage, TaskStage::Todo);
    if running {
        h.submit(stage, "m3");
    }
    (h, lead, parent, stage)
}
fn return_parent(h: &mut Harness, lead: &Actor, parent: u32) {
    h.run(
        lead,
        Operation::Return {
            node: parent,
            reason: "Fix it".into(),
        },
    )
    .unwrap();
}
fn person_cancel(h: &mut Harness, stage: u32) -> Result<Reply> {
    h.run(
        &person(),
        Operation::Cancel {
            node: stage,
            reason: "The person cancels the stage".into(),
        },
    )
}
fn pending(h: &Harness, node: u32) -> Vec<crate::teams::store::timers::PendingCancel> {
    h.store
        .pending_cancels(TEAM, 200, 0)
        .unwrap()
        .into_iter()
        .filter(|r| r.node == node)
        .collect()
}
fn answer(h: &mut Harness, stage: u32, state: CancelState) -> Result<Reply> {
    let row = pending(h, stage).remove(0);
    h.run(
        &service(),
        Operation::RecordCancel {
            node: stage,
            decision: row.decision,
            state,
            reason: if state == CancelState::Refused {
                "Delivery unknown".into()
            } else {
                String::new()
            },
            attempt: row.attempt,
            attempt_id: row.attempt_id,
        },
    )
}
fn query<T: rusqlite::types::FromSql>(h: &Harness, sql: &str, node: u32) -> T {
    let db = h.store.db.as_ref().unwrap();
    db.query_row(sql, rusqlite::params![TEAM, node], |r| r.get(0))
        .unwrap()
}
fn bound(h: &Harness, node: u32) -> bool {
    query(
        h,
        "SELECT EXISTS(SELECT 1 FROM live_tasks WHERE team=?1 AND node=?2)",
        node,
    )
}
fn archived(h: &Harness, node: u32) -> bool {
    query(
        h,
        "SELECT EXISTS(SELECT 1 FROM node_archive WHERE team=?1 AND id=?2)",
        node,
    )
}
fn outcomes(h: &Harness, node: u32) -> u32 {
    query(
        h,
        "SELECT count(*) FROM cancel_outcomes WHERE team=?1 AND node=?2",
        node,
    )
}
fn released(h: &Harness) -> Vec<u32> {
    h.store
        .released_stages(TEAM, 200, 0)
        .unwrap()
        .iter()
        .map(|k| k.node)
        .collect()
}
/// The parent's pipeline as the retired stage's end must leave it.
fn parent_view(h: &Harness, parent: u32) -> (Node, String, usize, Vec<u32>) {
    let run: String = query(
        h,
        "SELECT state FROM pipeline_runs WHERE team=?1 AND node=?2",
        parent,
    );
    let open = h
        .store
        .inbox(TEAM, 200, None)
        .unwrap()
        .rows
        .iter()
        .filter(|i| i.node == parent && i.state == "open")
        .count();
    let stages = h
        .store
        .queue(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .filter(|n| n.stage_of == Some(parent))
        .map(|n| n.id)
        .collect();
    (h.node(parent), run, open, stages)
}

/// The reviewer's sequence: a person's cancel of a running stage is still
/// pending after the lead returns the parent; it is carried out, then the
/// stage is archived and its key released once.
#[test]
fn a_persons_cancel_survives_the_parents_return_and_is_carried_out() {
    for observed in [true, false] {
        let (mut h, lead, parent, stage) = bound_stage(true);
        person_cancel(&mut h, stage).unwrap();
        let before = pending(&h, stage);
        assert_eq!(before.len(), 1);
        return_parent(&mut h, &lead, parent);
        let after = pending(&h, stage);
        assert_eq!(after, before, "the same cancel, still pending");
        assert_eq!(outcomes(&h, stage), 0);
        assert!(bound(&h, stage) && !archived(&h, stage));
        assert!(released(&h).is_empty(), "not released while it may run");
        // The answer: cancelled for running work is observed instead.
        refused(
            answer(&mut h, stage, CancelState::Cancelled),
            code::INVALID_STATE,
        );
        if observed {
            h.cancel_fact(stage);
        } else {
            answer(&mut h, stage, CancelState::Released).unwrap();
        }
        assert!(pending(&h, stage).is_empty());
        assert!(
            archived(&h, stage) && !bound(&h, stage),
            "observed: {observed}"
        );
        assert_eq!(released(&h), vec![stage], "its key, once");
        h.run(
            &service(),
            Operation::RecordKeyClear {
                node: stage,
                state: KeyClearState::Cleared,
                reason: String::new(),
            },
        )
        .unwrap();
        assert!(released(&h).is_empty());
    }
}

/// Every caller that retires a running stage leaves it bound, not current,
/// with one pending cancel of its own: the lead's Return, Reassign and
/// Cancel of the parent, a person's decision on it, a person's execution of
/// it taken over, its task ended on the board, and its key gone.
#[test]
fn every_caller_retires_a_running_stage_into_one_pending_cancel() {
    let callers = [
        "return",
        "reassign",
        "cancel",
        "accept",
        "taken over",
        "done",
        "cancelled",
        "key left",
    ];
    for caller in callers {
        let (mut h, lead, parent, stage) = bound_stage(true);
        match caller {
            "return" => return_parent(&mut h, &lead, parent),
            "reassign" => drop(
                h.run(
                    &lead,
                    Operation::Reassign {
                        node: parent,
                        member: "m2".into(),
                        reason: "Fresh eyes".into(),
                    },
                )
                .unwrap(),
            ),
            "cancel" => drop(
                h.run(
                    &lead,
                    Operation::Cancel {
                        node: parent,
                        reason: "Superseded".into(),
                    },
                )
                .unwrap(),
            ),
            "accept" => drop(
                h.run(&person(), Operation::Accept { node: parent })
                    .unwrap(),
            ),
            "taken over" => {
                let mut o = h.observation(parent, TaskStage::InProgress);
                (o.attempt, o.attempt_id) = (2, PERSON.into());
                h.run(
                    &service(),
                    Operation::ObserveTask {
                        node: parent,
                        observation: o,
                    },
                )
                .unwrap();
            }
            "done" => h.observe(parent, TaskStage::Done),
            "cancelled" => h.cancel_fact(parent),
            _ => {
                let mut o = h.observation(parent, TaskStage::InReview);
                o.key = KeyState::Absent;
                h.run(
                    &service(),
                    Operation::ObserveTask {
                        node: parent,
                        observation: o,
                    },
                )
                .unwrap();
            }
        }
        let rows = pending(&h, stage);
        assert_eq!(rows.len(), 1, "{caller}");
        assert!(
            rows[0].task.is_some(),
            "{caller}: the cancel names its task"
        );
        assert!(bound(&h, stage) && !archived(&h, stage), "{caller}");
        assert!(released(&h).is_empty(), "{caller}");
        assert!(
            h.store
                .effects(TEAM, 200, None)
                .unwrap()
                .rows
                .iter()
                .filter(|e| e.node == stage)
                .all(|e| !matches!(e.state.as_str(), "reserved" | "submitted")),
            "{caller}: its open effects are superseded"
        );
        let holder = h.store.attempt_holder(TEAM, stage).unwrap();
        assert_eq!(
            holder, None,
            "{caller}: its holder's report is no longer awaited"
        );
    }
}

/// A bound stage with nothing running is retired the same way: the store
/// does not guess; the command answers released, and that ends it.
#[test]
fn a_bound_stage_with_nothing_running_ends_on_its_release() {
    let (mut h, lead, parent, stage) = bound_stage(false);
    return_parent(&mut h, &lead, parent);
    assert_eq!(pending(&h, stage).len(), 1);
    assert!(bound(&h, stage));
    answer(&mut h, stage, CancelState::Released).unwrap();
    assert!(archived(&h, stage) && !bound(&h, stage));
    assert_eq!(released(&h), vec![stage]);
}

/// A stage without a bound task is retired as before: archived at once,
/// with no cancel, and released for its key through the saved creation.
#[test]
fn a_stage_without_a_bound_task_ends_at_once() {
    let mut h = pipeline();
    let lead = Actor::Session {
        person: "local".into(),
        team: TEAM.into(),
        member: "lead".into(),
        session: h
            .store
            .member(TEAM, "lead")
            .unwrap()
            .unwrap()
            .session
            .unwrap(),
    };
    let (parent, stage) = staged(&mut h);
    return_parent(&mut h, &lead, parent);
    assert!(pending(&h, stage).is_empty());
    assert!(archived(&h, stage));
}

/// A person who cancels the stage and a lead who returns its parent end in
/// the same state in either order; the person's cancel that comes second
/// finds the retirement's cancel pending and is refused.
#[test]
fn a_person_s_cancel_and_the_parents_return_agree_in_either_order() {
    let mut ends = Vec::new();
    for cancel_first in [true, false] {
        let (mut h, lead, parent, stage) = bound_stage(true);
        if cancel_first {
            person_cancel(&mut h, stage).unwrap();
            return_parent(&mut h, &lead, parent);
        } else {
            return_parent(&mut h, &lead, parent);
            refused(person_cancel(&mut h, stage), code::INVALID_STATE);
        }
        let rows = pending(&h, stage);
        let effects: Vec<String> = h
            .store
            .effects(TEAM, 200, None)
            .unwrap()
            .rows
            .iter()
            .map(|e| format!("{}:{}", e.node, e.state))
            .collect();
        ends.push((
            rows.len(),
            rows[0].generation,
            rows[0].task.clone(),
            bound(&h, stage),
            archived(&h, stage),
            h.node(stage).state,
            h.node(stage).hold,
            parent_view(&h, parent),
            effects,
        ));
    }
    assert_eq!(ends[0], ends[1]);
}

/// The end of a retired stage does not do what the end of a current stage
/// does: nothing is handed back to the lead, no stage is created, the
/// parent is not touched; for the observation and for the answer.
#[test]
fn the_end_of_a_retired_stage_hands_nothing_back() {
    for end in ["observed cancelled", "observed done", "released"] {
        let (mut h, lead, parent, stage) = bound_stage(true);
        return_parent(&mut h, &lead, parent);
        let before = parent_view(&h, parent);
        match end {
            "observed cancelled" => h.cancel_fact(stage),
            "observed done" => h.observe(stage, TaskStage::Done),
            _ => drop(answer(&mut h, stage, CancelState::Released).unwrap()),
        }
        assert!(archived(&h, stage), "{end}");
        let after = parent_view(&h, parent);
        assert_eq!(after.0, before.0, "{end}: the parent is not touched");
        assert_eq!(after.1, before.1, "{end}: its run is not touched");
        assert_eq!(after.2, before.2, "{end}: nothing handed back to the lead");
        assert_eq!(after.3, Vec::<u32>::new(), "{end}: no stage created");
    }
}

/// A second retirement of a stage that already waits for its cancel opens
/// no second one.
#[test]
fn a_second_retirement_opens_no_second_cancel() {
    let (mut h, lead, parent, stage) = bound_stage(true);
    return_parent(&mut h, &lead, parent);
    let first = pending(&h, stage);
    h.cancel_fact(parent);
    assert_eq!(pending(&h, stage), first);
}

/// In a paused team the retirement is the same.
#[test]
fn a_paused_team_retires_into_a_pending_cancel() {
    let (mut h, lead, parent, stage) = bound_stage(true);
    h.run(&person(), Operation::Pause).unwrap();
    return_parent(&mut h, &lead, parent);
    assert_eq!(pending(&h, stage).len(), 1);
    assert!(bound(&h, stage));
}

/// A stop meets a retired stage that waits for its cancel: the stop's
/// cancel replaces it, one per node, and the stage's key is cleared once
/// after that answer.
#[test]
fn a_stop_after_a_retirement_waits_for_one_cancel() {
    let (mut h, lead, parent, stage) = bound_stage(true);
    return_parent(&mut h, &lead, parent);
    h.run(&person(), Operation::Stop).unwrap();
    assert_eq!(pending(&h, stage).len(), 1);
    answer(&mut h, stage, CancelState::Released).unwrap();
    assert!(pending(&h, stage).is_empty());
    let keyed: Vec<u32> = h
        .store
        .keyed_tasks(TEAM, 0, 200)
        .unwrap()
        .iter()
        .map(|k| k.node)
        .filter(|n| *n == stage)
        .collect();
    assert_eq!(keyed, vec![stage], "its key, once");
    assert!(released(&h).is_empty());
}

/// A refused answer (delivery unknown, or the task Done): the cancel
/// settles with its outcome, the lead is told, and the stage stays live,
/// bound and held, its key on its task. Nothing waits on the store: the
/// person may cancel it again, and the observation of its task's end (Done
/// or cancelled) ends it, archives it and releases its key once.
#[test]
fn a_refused_answer_leaves_the_stage_to_its_task_s_end() {
    for next in ["done", "cancel again"] {
        let (mut h, lead, parent, stage) = bound_stage(true);
        return_parent(&mut h, &lead, parent);
        answer(&mut h, stage, CancelState::Refused).unwrap();
        assert!(pending(&h, stage).is_empty());
        assert_eq!(outcomes(&h, stage), 1);
        assert!(bound(&h, stage) && !archived(&h, stage));
        assert!(h.node(stage).hold, "nothing hands it out again");
        assert!(h
            .store
            .inbox(TEAM, 200, None)
            .unwrap()
            .rows
            .iter()
            .any(|i| i.node == stage && i.kind.starts_with("cancel_refused:")));
        refused(
            h.run(
                &lead,
                Operation::Cancel {
                    node: stage,
                    reason: "Again".into(),
                },
            ),
            code::NOT_ALLOWED,
        );
        if next == "done" {
            h.observe(stage, TaskStage::Done);
        } else {
            person_cancel(&mut h, stage).unwrap();
            assert_eq!(pending(&h, stage).len(), 1);
            h.cancel_fact(stage);
        }
        assert!(archived(&h, stage) && !bound(&h, stage), "{next}");
        assert_eq!(released(&h), vec![stage], "{next}: its key, once");
    }
}

/// After a stop, the parent's end still retires its stage: its task
/// observed cancelled or Done, or its own cancel answered. The stage's own
/// stop cancel is not settled by that retirement (a stop has released every
/// binding, so the stage is no longer bound): it stays pending until its
/// own answer, which records its outcome and clears its key once.
#[test]
fn after_a_stop_the_parents_end_leaves_the_stages_own_cancel_pending() {
    for end in ["observed cancelled", "observed done", "answered"] {
        let (mut h, _, parent, stage) = bound_stage(true);
        h.run(&person(), Operation::Stop).unwrap();
        let before = pending(&h, stage);
        assert_eq!(before.len(), 1, "{end}: the stop's own cancel");
        match end {
            "observed cancelled" => h.cancel_fact(parent),
            "observed done" => h.observe(parent, TaskStage::Done),
            _ => drop(answer(&mut h, parent, CancelState::Released).unwrap()),
        }
        assert_eq!(pending(&h, stage), before, "{end}: still pending");
        assert_eq!(outcomes(&h, stage), 0, "{end}");
        assert!(!archived(&h, stage), "{end}");
        answer(&mut h, stage, CancelState::Released).unwrap();
        assert!(pending(&h, stage).is_empty(), "{end}");
        assert_eq!(outcomes(&h, stage), 1, "{end}: its answer is recorded");
        let keyed: Vec<u32> = h
            .store
            .keyed_tasks(TEAM, 0, 200)
            .unwrap()
            .iter()
            .map(|k| k.node)
            .filter(|n| *n == stage)
            .collect();
        assert_eq!(keyed, vec![stage], "{end}: its key, once");
    }
}

/// A person's pending cancel of a stage whose task was never bound is kept
/// by the retirement too, and carried out: its answer ends the stage, which
/// is then archived.
#[test]
fn a_persons_cancel_of_an_unbound_stage_survives_the_parents_return() {
    let mut h = pipeline();
    let lead = Actor::Session {
        person: "local".into(),
        team: TEAM.into(),
        member: "lead".into(),
        session: h
            .store
            .member(TEAM, "lead")
            .unwrap()
            .unwrap()
            .session
            .unwrap(),
    };
    let (parent, stage) = staged(&mut h);
    assert!(!bound(&h, stage));
    person_cancel(&mut h, stage).unwrap();
    let before = pending(&h, stage);
    assert_eq!(before.len(), 1);
    return_parent(&mut h, &lead, parent);
    assert_eq!(pending(&h, stage), before, "the same cancel, still pending");
    assert!(!archived(&h, stage));
    answer(&mut h, stage, CancelState::Cancelled).unwrap();
    assert!(pending(&h, stage).is_empty());
    assert_eq!(outcomes(&h, stage), 1);
    assert!(archived(&h, stage));
    assert_eq!(h.node(stage).state, NodeState::Cancelled);
}

/// The DeepSeek reviewer's reproducer, committed: a person's cancel of a
/// stage whose creation is reserved survives that creation's refused or
/// uncertain outcome. The stage is retired, not ended: the cancel stays
/// pending and the stage live, while its parent goes to the lead with the
/// creation's reason. The cancel's own answer then ends the stage.
#[test]
fn a_persons_cancel_of_a_reserved_stage_survives_a_refused_or_uncertain_creation() {
    for status in ["refused", "uncertain"] {
        let mut h = pipeline();
        let (parent, stage) = staged(&mut h);
        let id = uuid::Uuid::new_v4().to_string();
        h.run(
            &service(),
            op(serde_json::json!({"op":"stage_task_intent","node":stage,"effect_request_id":id})),
        )
        .unwrap();
        person_cancel(&mut h, stage).unwrap();
        let before = pending(&h, stage);
        assert_eq!(before.len(), 1, "{status}: the person's cancel is pending");
        h.run(
            &service(),
            op(serde_json::json!({"op":"stage_task_outcome","effect_request_id":id,"outcome":{"status":status,"reason":"The creation child refused"}})),
        )
        .unwrap();
        assert_eq!(pending(&h, stage), before, "{status}: still pending");
        assert_eq!(outcomes(&h, stage), 0, "{status}");
        assert!(!archived(&h, stage), "{status}: it waits for the answer");
        assert!(
            h.store
                .inbox(TEAM, 200, None)
                .unwrap()
                .rows
                .iter()
                .any(|i| i.node == parent && i.text.contains("The creation child refused")),
            "{status}: the parent goes to the lead"
        );
        answer(&mut h, stage, CancelState::Cancelled).unwrap();
        assert!(pending(&h, stage).is_empty(), "{status}");
        assert_eq!(outcomes(&h, stage), 1, "{status}");
        assert!(archived(&h, stage), "{status}: ended by its answer");
    }
}

/// Without a pending cancel, a refused creation ends its stage at once, as
/// before.
#[test]
fn a_refused_creation_without_a_cancel_ends_its_stage_at_once() {
    let mut h = pipeline();
    let (_, stage) = staged(&mut h);
    let id = uuid::Uuid::new_v4().to_string();
    h.run(
        &service(),
        op(serde_json::json!({"op":"stage_task_intent","node":stage,"effect_request_id":id})),
    )
    .unwrap();
    h.run(
        &service(),
        op(serde_json::json!({"op":"stage_task_outcome","effect_request_id":id,"outcome":{"status":"refused","reason":"The creation child refused"}})),
    )
    .unwrap();
    assert!(pending(&h, stage).is_empty());
    assert!(archived(&h, stage));
}
