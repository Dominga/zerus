//! Cancel decisions carried out by the service, and their order with the
//! cleanup of a stopped team's task keys.
use super::*;
use crate::teams::store::timers::PendingCancel;
use rusqlite::OptionalExtension;

fn record(node: u32, decision: u64, state: CancelState, reason: &str) -> Operation {
    Operation::RecordCancel {
        node,
        decision,
        state,
        reason: reason.into(),
        attempt: 0,
        attempt_id: String::new(),
    }
}
fn pending(h: &Harness) -> Vec<PendingCancel> {
    h.store.pending_cancels(TEAM, 200, 0).unwrap()
}
fn decision(h: &Harness, node: u32) -> u64 {
    pending(h)
        .into_iter()
        .find(|p| p.node == node)
        .unwrap()
        .decision
}
fn outcomes(h: &Harness) -> u32 {
    let db = h.store.db.as_ref().unwrap();
    db.query_row("SELECT count(*) FROM cancel_outcomes", [], |r| r.get(0))
        .unwrap()
}
/// A lead's Cancel of a running task, still pending.
fn cancelling() -> (Harness, u32, Actor) {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    h.bind("m1");
    let node = h.add(&person(), 1);
    h.submit(node, "m1");
    h.run(
        &lead,
        Operation::Cancel {
            node,
            reason: "Superseded".into(),
        },
    )
    .unwrap();
    (h, node, lead)
}

#[test]
fn a_leads_cancel_is_pending_once_with_its_task() {
    let (h, node, _) = cancelling();
    let rows = pending(&h);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].node, rows[0].generation, rows[0].task.clone()),
        (node, 1, Some(task(1)))
    );
}

#[test]
fn the_observation_then_the_operation_which_writes_nothing() {
    let (mut h, node, _) = cancelling();
    let d = decision(&h, node);
    h.cancel_fact(node);
    assert_eq!(h.node(node).state, NodeState::Cancelled);
    assert!(pending(&h).is_empty());
    let rev = h.store.team(TEAM).unwrap().unwrap().revision;
    refused(
        h.run(&service(), record(node, d, CancelState::Cancelled, "")),
        code::INVALID_STATE,
    );
    assert_eq!(h.store.team(TEAM).unwrap().unwrap().revision, rev);
    assert_eq!(outcomes(&h), 0);
}

#[test]
fn a_refused_cancel_in_a_live_team_goes_back_to_the_lead() {
    let (mut h, node, lead) = cancelling();
    let d = decision(&h, node);
    refused(
        h.run(&service(), record(node, d, CancelState::Refused, " ")),
        code::INVALID_STATE,
    );
    refused(
        h.run(&service(), record(node, d, CancelState::Cancelled, "why")),
        code::INVALID_STATE,
    );
    refused(
        h.run(
            &lead,
            record(node, d, CancelState::Refused, "Delivery unknown"),
        ),
        code::NOT_ALLOWED,
    );
    h.run(
        &service(),
        record(node, d, CancelState::Refused, "Delivery unknown"),
    )
    .unwrap();
    let item = h
        .store
        .inbox(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .find(|i| i.kind.starts_with("cancel_refused:"))
        .unwrap();
    assert!(item.text.contains(&node.to_string()) && item.text.contains("Delivery unknown"));
    assert_eq!(h.node(node).state, NodeState::InProgress);
    assert!(
        h.node(node).hold,
        "nothing is handed out until the lead decides again"
    );
    assert!(pending(&h).is_empty());
    refused(
        h.run(&service(), record(node, d, CancelState::Refused, "Again")),
        code::INVALID_STATE,
    );
    h.run(
        &lead,
        Operation::Cancel {
            node,
            reason: "Try again".into(),
        },
    )
    .unwrap();
    assert_eq!(pending(&h).len(), 1, "the lead may decide again");
}

#[test]
fn a_cancel_of_an_older_generation_is_not_pending_or_recordable() {
    let (mut h, node, _) = cancelling();
    let d = decision(&h, node);
    let db = h.store.db.as_ref().unwrap();
    db.execute(
        "UPDATE nodes SET generation=generation+1 WHERE team=?1 AND id=?2",
        rusqlite::params![TEAM, node],
    )
    .unwrap();
    assert!(pending(&h).is_empty());
    refused(
        h.run(&service(), record(node, d, CancelState::Cancelled, "")),
        code::INVALID_STATE,
    );
}

/// A stopped team with tasks in every stage and a stage node without a task.
fn stopped() -> (Harness, Vec<u32>) {
    let mut h = Harness::new();
    h.bind("lead");
    h.bind("m1");
    h.bind("m2");
    let waiting = h.enqueue(&person(), 1);
    let pending = h.add(&person(), 2);
    let running = h.add(&person(), 3);
    h.submit(running, "m1");
    let reported = h.add(&person(), 4);
    h.submit(reported, "m2");
    h.report(reported, "m2");
    h.run(&person(), Operation::Stop).unwrap();
    (h, vec![waiting, pending, running, reported])
}

#[test]
fn a_stop_cancels_first_and_clears_each_key_only_after_its_cancel_settles() {
    let (mut h, nodes) = stopped();
    let rows = pending(&h);
    assert_eq!(rows.iter().map(|p| p.node).collect::<Vec<_>>(), nodes);
    assert!(rows.iter().all(|p| p.task.is_some()));
    assert!(
        h.store.keyed_tasks(TEAM, 0, 200).unwrap().is_empty(),
        "no key before its cancel"
    );
    let before = h.node(nodes[2]);
    h.run(
        &service(),
        record(nodes[2], rows[2].decision, CancelState::Cancelled, ""),
    )
    .unwrap();
    assert_eq!(
        h.node(nodes[2]).state,
        before.state,
        "after a stop nothing else of the node changes"
    );
    h.run(
        &service(),
        record(
            nodes[3],
            rows[3].decision,
            CancelState::Refused,
            "Delivery unknown",
        ),
    )
    .unwrap();
    assert_eq!(
        h.store.cancel_refusals(TEAM, (0, 0), 200).unwrap(),
        vec![(nodes[3], rows[3].decision, "Delivery unknown".to_owned())]
    );
    assert!(h
        .store
        .inbox(TEAM, 200, None)
        .unwrap()
        .rows
        .iter()
        .all(|i| !i.kind.starts_with("cancel_refused")));
    let keyed: Vec<u32> = h
        .store
        .keyed_tasks(TEAM, 0, 200)
        .unwrap()
        .iter()
        .map(|k| k.node)
        .collect();
    assert_eq!(
        keyed,
        nodes[2..].to_vec(),
        "the page takes a node once its cancel is recorded"
    );
    for node in &nodes[2..] {
        h.run(
            &service(),
            Operation::RecordKeyClear {
                node: *node,
                state: KeyClearState::Cleared,
                reason: String::new(),
            },
        )
        .unwrap();
    }
    assert_eq!(
        h.store.keyed_teams("", 10).unwrap(),
        vec![TEAM.to_owned()],
        "two cancels still wait"
    );
    for (node, row) in nodes[..2].iter().zip(&rows) {
        h.run(
            &service(),
            record(*node, row.decision, CancelState::Cancelled, ""),
        )
        .unwrap();
    }
    assert_eq!(
        h.store.keyed_teams("", 10).unwrap(),
        vec![TEAM.to_owned()],
        "two keys still wait"
    );
    for node in &nodes[..2] {
        h.run(
            &service(),
            Operation::RecordKeyClear {
                node: *node,
                state: KeyClearState::Cleared,
                reason: String::new(),
            },
        )
        .unwrap();
    }
    assert!(
        h.store.keyed_teams("", 10).unwrap().is_empty(),
        "only at the end"
    );
}

#[test]
fn a_node_without_a_task_is_cancelled_without_a_child() {
    let mut create = create();
    if let Operation::Create { bundle, .. } = &mut create {
        let catalog = zerus_team_plan::builtin();
        bundle.roles = catalog.roles;
        bundle.pipelines = catalog.pipelines;
        bundle.template.default_pipeline = "reviewed".into();
    }
    let mut p = Harness::empty();
    p.run(&person(), create).unwrap();
    for member in ["lead", "m1", "m3"] {
        p.bind(member);
    }
    let parent = p.add(&person(), 1);
    p.submit(parent, "m1");
    p.report(parent, "m1");
    p.run(&person(), Operation::Stop).unwrap();
    let rows = pending(&p);
    let stage = rows.iter().find(|r| r.node != parent).unwrap().clone();
    let first = rows.iter().find(|r| r.node == parent).unwrap().clone();
    assert_eq!(stage.task, None, "the wanted stage has no task");
    p.run(
        &service(),
        record(parent, first.decision, CancelState::Cancelled, ""),
    )
    .unwrap();
    let clear = Operation::RecordKeyClear {
        node: parent,
        state: KeyClearState::Cleared,
        reason: String::new(),
    };
    p.run(&service(), clear).unwrap();
    assert_eq!(
        p.store.keyed_teams("", 10).unwrap(),
        vec![TEAM.to_owned()],
        "the team stays while a cancel waits, even one without a key"
    );
    p.run(
        &service(),
        record(stage.node, stage.decision, CancelState::Cancelled, ""),
    )
    .unwrap();
    assert!(p.store.keyed_tasks(TEAM, 0, 200).unwrap().is_empty());
    assert!(p.store.keyed_teams("", 10).unwrap().is_empty());
}

#[test]
fn pending_cancels_page_in_node_order() {
    let (h, nodes) = stopped();
    let mut seen = Vec::new();
    let mut after = 0;
    loop {
        let page = h.store.pending_cancels(TEAM, 1, after).unwrap();
        let Some(row) = page.first() else { break };
        after = row.node;
        seen.push(row.node);
    }
    assert_eq!(seen, nodes);
    refused(h.store.pending_cancels(TEAM, 201, 0), code::OVER_CAP);
}

#[test]
fn a_page_boundary_between_two_refused_cancels_of_one_node_loses_neither() {
    let (mut h, node, lead) = cancelling();
    let first = decision(&h, node);
    h.run(
        &service(),
        record(node, first, CancelState::Refused, "Delivery unknown"),
    )
    .unwrap();
    h.run(
        &lead,
        Operation::Cancel {
            node,
            reason: "Try again".into(),
        },
    )
    .unwrap();
    let second = decision(&h, node);
    h.run(
        &service(),
        record(node, second, CancelState::Refused, "Still unknown"),
    )
    .unwrap();
    let all = h.store.cancel_refusals(TEAM, (0, 0), 200).unwrap();
    assert_eq!(all.len(), 2);
    let mut paged = Vec::new();
    let mut after = (0, 0);
    while let Some(row) = h.store.cancel_refusals(TEAM, after, 1).unwrap().pop() {
        after = (row.0, row.1);
        paged.push(row);
    }
    assert_eq!(paged, all, "a page of one returns both rows of the node");
    assert_eq!((all[0].1, all[1].1), (first, second));
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
fn last_event(h: &Harness) -> String {
    let rows = h.store.journal(TEAM, 200, None).unwrap().rows;
    rows.last().unwrap().kind.clone()
}
const PERSON: &str = "00000000-0000-4000-8000-00000000bbbb";

/// In a live team a task's end is recorded by its observation alone.
#[test]
fn a_cancelled_answer_for_a_bound_live_node_is_refused_and_writes_nothing() {
    for paused in [false, true] {
        let (mut h, node, _) = cancelling();
        if paused {
            h.run(&person(), Operation::Pause).unwrap();
        }
        let d = decision(&h, node);
        let rev = h.store.team(TEAM).unwrap().unwrap().revision;
        let refusal = h
            .run(&service(), record(node, d, CancelState::Cancelled, ""))
            .unwrap_err();
        assert_eq!(refusal.code, code::INVALID_STATE);
        assert!(
            refusal.message.contains("observed instead"),
            "{}",
            refusal.message
        );
        assert_eq!(h.store.team(TEAM).unwrap().unwrap().revision, rev);
        assert_eq!(pending(&h).len(), 1, "the decision stays pending");
        assert!(bound(&h, node));
        assert_eq!(outcome(&h, d), None);
        assert_eq!(h.node(node).state, NodeState::InProgress);
    }
}

/// The new shape end to end at the store's port: the command answered
/// cancelled, the service observes the task, and the observation of the
/// cancelled task ends the node and settles the decision.
#[test]
fn the_observation_of_the_cancelled_task_ends_the_node_and_settles_the_cancel() {
    let (mut h, node, _) = cancelling();
    let d = decision(&h, node);
    refused(
        h.run(&service(), record(node, d, CancelState::Cancelled, "")),
        code::INVALID_STATE,
    );
    h.cancel_fact(node);
    assert_eq!(h.node(node).state, NodeState::Cancelled);
    assert!(
        pending(&h).is_empty(),
        "the observation settles the decision"
    );
    assert!(!bound(&h, node));
    assert!(h
        .store
        .effects(TEAM, 200, None)
        .unwrap()
        .rows
        .iter()
        .all(|e| e.state != "submitted"));
    for state in [CancelState::Cancelled, CancelState::Released] {
        refused(
            h.run(&service(), record(node, d, state, "")),
            code::INVALID_STATE,
        );
    }
    assert_eq!(outcome(&h, d), None);
}

/// A person revived the task before the service observed the cancel: the
/// observation takes the person's execution over and settles the decision.
#[test]
fn the_observation_of_a_revived_task_takes_it_over() {
    let (mut h, node, _) = cancelling();
    let mut o = h.observation(node, TaskStage::InProgress);
    o.attempt = 2;
    o.attempt_id = PERSON.into();
    h.run(
        &service(),
        Operation::ObserveTask {
            node,
            observation: o,
        },
    )
    .unwrap();
    assert_eq!(h.store.generation(TEAM, node).unwrap(), Some(2));
    assert!(bound(&h, node));
    assert!(pending(&h).is_empty());
    assert_eq!(last_event(&h), "person_override");
    assert_eq!(h.node(node).state, NodeState::InProgress);
}

/// A live node without a task has nothing to observe: its cancel is
/// applied as before.
#[test]
fn a_cancelled_answer_for_a_live_node_without_a_task_is_applied() {
    let mut create = create();
    if let Operation::Create { bundle, .. } = &mut create {
        let catalog = zerus_team_plan::builtin();
        bundle.roles = catalog.roles;
        bundle.pipelines = catalog.pipelines;
        bundle.template.default_pipeline = "reviewed".into();
    }
    let mut p = Harness::empty();
    p.run(&person(), create).unwrap();
    for member in ["lead", "m1", "m3"] {
        p.bind(member);
    }
    let parent = p.add(&person(), 1);
    p.submit(parent, "m1");
    p.report(parent, "m1");
    let stage = p
        .store
        .queue(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .find(|n| n.id != parent)
        .unwrap()
        .id;
    assert!(!bound(&p, stage), "the wanted stage has no task yet");
    p.run(
        &person(),
        Operation::Cancel {
            node: stage,
            reason: "Not needed".into(),
        },
    )
    .unwrap();
    let row = pending(&p).into_iter().find(|r| r.node == stage).unwrap();
    assert_eq!(row.task, None);
    p.run(
        &service(),
        record(stage, row.decision, CancelState::Cancelled, ""),
    )
    .unwrap();
    assert_eq!(p.node(stage).state, NodeState::Cancelled);
    assert_eq!(outcome(&p, row.decision).as_deref(), Some("cancelled"));
}

/// Released and refused in every team state, and cancelled after a stop or
/// an archive. A released answer ends the node in a live team and its
/// binding goes with the record; a refusal goes to the lead in a live team
/// and is kept for the terminal page after a stop.
#[test]
fn released_refused_and_cancelled_answers_in_every_team_state() {
    for status in ["running", "paused", "stopped", "archived"] {
        for state in [
            CancelState::Released,
            CancelState::Refused,
            CancelState::Cancelled,
        ] {
            let (mut h, node, _) = cancelling();
            match status {
                "paused" => drop(h.run(&person(), Operation::Pause).unwrap()),
                "stopped" => drop(h.run(&person(), Operation::Stop).unwrap()),
                "archived" => {
                    h.run(&person(), Operation::Stop).unwrap();
                    h.run(&person(), Operation::Archive).unwrap();
                }
                _ => (),
            }
            let live = matches!(status, "running" | "paused");
            let d = decision(&h, node);
            let before = h.node(node).state;
            let why = if state == CancelState::Refused {
                "Delivery unknown"
            } else {
                ""
            };
            let result = h.run(&service(), record(node, d, state, why));
            let case = format!("{status} {state:?}");
            if live && state == CancelState::Cancelled {
                refused(result, code::INVALID_STATE);
                assert!(bound(&h, node), "{case}");
                continue;
            }
            result.unwrap();
            assert!(pending(&h).iter().all(|p| p.node != node), "{case}");
            let recorded = serde_json::to_value(state).unwrap();
            assert_eq!(
                outcome(&h, d).as_deref(),
                recorded.as_str(),
                "{case}: recorded as it came"
            );
            match (state, live) {
                (CancelState::Released, true) => {
                    assert_eq!(h.node(node).state, NodeState::Cancelled, "{case}");
                    assert!(!bound(&h, node), "{case}: released with the record");
                }
                (CancelState::Refused, true) => {
                    assert_eq!(h.node(node).state, before, "{case}");
                    assert!(h.node(node).hold, "{case}");
                    assert!(bound(&h, node), "{case}");
                    assert!(h
                        .store
                        .inbox(TEAM, 200, None)
                        .unwrap()
                        .rows
                        .iter()
                        .any(|i| i.kind == format!("cancel_refused:{d}")));
                }
                (_, false) => {
                    assert_eq!(h.node(node).state, before, "{case}");
                    assert!(!bound(&h, node), "{case}: the stop released it");
                    let refusals = h.store.cancel_refusals(TEAM, (0, 0), 10).unwrap();
                    assert_eq!(
                        refusals.iter().any(|r| r.1 == d),
                        state == CancelState::Refused,
                        "{case}"
                    );
                }
                _ => unreachable!(),
            }
        }
    }
}

#[test]
fn a_released_answer_about_another_attempt_is_refused() {
    let (mut h, node, _) = cancelling();
    let d = decision(&h, node);
    let other = Operation::RecordCancel {
        node,
        decision: d,
        state: CancelState::Released,
        reason: String::new(),
        attempt: 2,
        attempt_id: PERSON.into(),
    };
    refused(h.run(&service(), other), code::INVALID_STATE);
    refused(
        h.run(&service(), record(node, d, CancelState::Released, "why")),
        code::INVALID_STATE,
    );
    assert!(bound(&h, node));
    assert_eq!(pending(&h).len(), 1);
}

#[test]
fn a_repeated_answer_is_refused_and_writes_nothing() {
    let (mut h, node, _) = cancelling();
    let d = decision(&h, node);
    h.run(&service(), record(node, d, CancelState::Released, ""))
        .unwrap();
    let rev = h.store.team(TEAM).unwrap().unwrap().revision;
    for state in [
        CancelState::Released,
        CancelState::Cancelled,
        CancelState::Refused,
    ] {
        let why = if state == CancelState::Refused {
            "Again"
        } else {
            ""
        };
        refused(
            h.run(&service(), record(node, d, state, why)),
            code::INVALID_STATE,
        );
    }
    assert_eq!(h.store.team(TEAM).unwrap().unwrap().revision, rev);
    assert_eq!(outcome(&h, d).as_deref(), Some("released"));
}

/// An answer as team-cancel gives it: its state and the attempt it acted on.
fn acted(node: u32, decision: u64, state: CancelState, attempt: u64, id: &str) -> Operation {
    Operation::RecordCancel {
        node,
        decision,
        state,
        reason: String::new(),
        attempt,
        attempt_id: id.into(),
    }
}
/// The node handed out to m1 and delivered as (1, E); the newest
/// observation still (0, empty, To do). `end`: stop, or stop and archive.
fn delivered_unobserved(end: &str) -> (Harness, u32, String) {
    let mut h = Harness::new();
    h.bind("lead");
    h.bind("m1");
    let node = h.add(&person(), 1);
    let id = h.intent(node, "m1");
    h.outcome(
        &id,
        HandoutOutcome::Submitted {
            receipt: "receipt-example".into(),
        },
    );
    terminal(&mut h, end);
    (h, node, id)
}
fn terminal(h: &mut Harness, end: &str) {
    match end {
        "running" => (),
        "stopped" => drop(h.run(&person(), Operation::Stop).unwrap()),
        _ => {
            h.run(&person(), Operation::Stop).unwrap();
            h.run(&person(), Operation::Archive).unwrap();
        }
    }
}
fn keyed_once(h: &Harness, node: u32) -> bool {
    let keyed: Vec<u32> = h
        .store
        .keyed_tasks(TEAM, 0, 200)
        .unwrap()
        .iter()
        .map(|k| k.node)
        .filter(|n| *n == node)
        .collect();
    keyed == vec![node]
}

/// The reviewer's sequence: the team's own hand-out was delivered and not
/// yet observed when the person stopped the team. team-cancel acts on that
/// attempt and answers with its pair; the store takes it by the command's
/// own rule, in a stopped and in an archived team, cancelled and released.
#[test]
fn a_stopped_teams_cancel_of_its_own_delivered_effect_is_recorded() {
    for end in ["stopped", "archived"] {
        for state in [CancelState::Cancelled, CancelState::Released] {
            let (mut h, node, id) = delivered_unobserved(end);
            let row = pending(&h).remove(0);
            assert_eq!((row.attempt, row.attempt_id.as_str()), (0, ""));
            h.run(&service(), acted(node, row.decision, state, 1, &id))
                .unwrap();
            assert!(pending(&h).is_empty(), "{end} {state:?}");
            assert_eq!(outcomes(&h), 1, "{end} {state:?}");
            assert!(keyed_once(&h, node), "{end} {state:?}");
            let rev = h.store.team(TEAM).unwrap().unwrap().revision;
            refused(
                h.run(&service(), acted(node, row.decision, state, 1, &id)),
                code::INVALID_STATE,
            );
            assert_eq!(h.store.team(TEAM).unwrap().unwrap().revision, rev);
        }
    }
}

/// A reopen's attempt: the lead returned the node, the reopen was delivered
/// and not observed, then the stop.
#[test]
fn a_stopped_teams_cancel_of_its_own_reopen_is_recorded() {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    h.bind("m1");
    let node = h.add(&person(), 1);
    h.submit(node, "m1");
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
    h.reopen_outcome(
        &reopen,
        HandoutOutcome::Submitted {
            receipt: "receipt-example".into(),
        },
    );
    h.run(&person(), Operation::Stop).unwrap();
    let row = pending(&h).remove(0);
    h.run(
        &service(),
        acted(node, row.decision, CancelState::Cancelled, 2, &reopen),
    )
    .unwrap();
    assert!(pending(&h).is_empty());
    assert!(keyed_once(&h, node));
}

/// The second door: the newest observation of a stopped team is a
/// conflict, and the person settles the document onto the node's own
/// delivered effect. The command acts on it, and the store takes the
/// answer; the observation's own pair is taken as well, since a conflict
/// holds no cancelled answer back after a stop.
#[test]
fn a_stopped_teams_own_effect_is_recorded_under_a_conflict() {
    for own_effect in [true, false] {
        let mut h = Harness::new();
        h.bind("m1");
        let node = h.add(&person(), 1);
        let id = h.submit(node, "m1");
        let mut o = h.observation(node, TaskStage::InProgress);
        (o.attempt, o.attempt_id, o.attempt_conflict) = (2, String::new(), true);
        h.run(
            &service(),
            Operation::ObserveTask {
                node,
                observation: o,
            },
        )
        .unwrap();
        h.run(&person(), Operation::Stop).unwrap();
        let row = pending(&h).remove(0);
        assert_eq!((row.attempt, row.attempt_id.as_str()), (2, ""));
        let acted_on = if own_effect { id.as_str() } else { "" };
        h.run(
            &service(),
            acted(node, row.decision, CancelState::Cancelled, 2, acted_on),
        )
        .unwrap();
        assert!(pending(&h).is_empty());
        assert!(keyed_once(&h, node));
    }
}

/// The reviewer's f3 sequence: two person sends conflict, the observation
/// records the task running as (1, empty, conflict), and the person stops
/// the team and cancels the task on the board in either order; the store
/// sees the same in both, the stop and then the command's true answer,
/// cancelled with (1, empty). Stopped and archived: recorded, settled, one
/// outcome, the key in the cleanup frontier once. A conflicting document
/// that is not cancelled ends as a refused outcome.
#[test]
fn a_conflicting_task_cancelled_after_a_stop_is_recorded() {
    for end in ["stopped", "archived"] {
        for state in [CancelState::Cancelled, CancelState::Refused] {
            let mut h = Harness::new();
            h.bind("m1");
            let node = h.add(&person(), 1);
            let mut o = h.observation(node, TaskStage::InProgress);
            (o.attempt, o.attempt_id, o.attempt_conflict) = (1, String::new(), true);
            h.run(
                &service(),
                Operation::ObserveTask {
                    node,
                    observation: o,
                },
            )
            .unwrap();
            terminal(&mut h, end);
            let row = pending(&h).remove(0);
            let answer = Operation::RecordCancel {
                node,
                decision: row.decision,
                state,
                reason: if state == CancelState::Refused {
                    "The document runs with conflicting attempts".into()
                } else {
                    String::new()
                },
                attempt: 1,
                attempt_id: String::new(),
            };
            h.run(&service(), answer).unwrap();
            assert!(pending(&h).is_empty(), "{end} {state:?}");
            assert_eq!(outcomes(&h), 1, "{end} {state:?}");
            assert_eq!(
                outcome(&h, row.decision).as_deref(),
                Some(if state == CancelState::Refused {
                    "refused"
                } else {
                    "cancelled"
                })
            );
            if state == CancelState::Cancelled {
                assert!(keyed_once(&h, node), "{end}");
            }
        }
    }
}

/// The reviewer's reopen variant: the observation still (1, A, In review),
/// the reopen B delivered and not observed (the document runs (2, B)), then
/// the stop: cancelled with (2, B) is recorded. And its released variant: a
/// hand-out delivered and reported with no observation, the store still at
/// (0, empty, To do), then the stop: released with (1, E) is recorded.
#[test]
fn the_reviewers_reopen_and_reported_variants_are_recorded() {
    for end in ["stopped", "archived"] {
        let mut h = Harness::new();
        let lead = h.bind("lead");
        h.bind("m1");
        let node = h.add(&person(), 1);
        let a = h.intent(node, "m1");
        h.outcome(
            &a,
            HandoutOutcome::Submitted {
                receipt: "receipt-example".into(),
            },
        );
        let mut o = h.observation(node, TaskStage::InReview);
        (o.attempt, o.attempt_id) = (1, a.clone());
        h.run(
            &service(),
            Operation::ObserveTask {
                node,
                observation: o,
            },
        )
        .unwrap();
        h.run(
            &service(),
            Operation::RecordReport {
                node,
                generation: 1,
                member: "m1".into(),
                text: "Implemented and checked".into(),
                commit: COMMIT.into(),
                notes: Vec::new(),
            },
        )
        .unwrap();
        h.run(
            &lead,
            Operation::Return {
                node,
                reason: "Fix it".into(),
            },
        )
        .unwrap();
        let b = h.reopen(node, "m1");
        h.reopen_outcome(
            &b,
            HandoutOutcome::Submitted {
                receipt: "receipt-example".into(),
            },
        );
        terminal(&mut h, end);
        let row = pending(&h).remove(0);
        assert_eq!((row.attempt, row.attempt_id.clone()), (1, a.clone()));
        h.run(
            &service(),
            acted(node, row.decision, CancelState::Cancelled, 2, &b),
        )
        .unwrap();
        assert!(pending(&h).is_empty(), "{end}");
        assert!(keyed_once(&h, node), "{end}");

        let mut h = Harness::new();
        h.bind("m1");
        let node = h.add(&person(), 1);
        let e = h.intent(node, "m1");
        h.outcome(
            &e,
            HandoutOutcome::Submitted {
                receipt: "receipt-example".into(),
            },
        );
        // The member's report is on the task document only: the store takes
        // a report after it observes In review, and it observed nothing.
        terminal(&mut h, end);
        let row = pending(&h).remove(0);
        assert_eq!((row.attempt, row.attempt_id.as_str()), (0, ""));
        h.run(
            &service(),
            acted(node, row.decision, CancelState::Released, 1, &e),
        )
        .unwrap();
        assert!(pending(&h).is_empty(), "{end}");
        assert!(keyed_once(&h, node), "{end}");
    }
}

/// What stays refused, with nothing written: a person's attempt ID, an
/// effect of another node, of an earlier generation, of another kind, a
/// number below the observed one; and the own-effect answer itself in a
/// running team.
#[test]
fn an_answer_about_any_other_attempt_is_still_refused() {
    // A person's attempt ID, and a number below the observed one.
    for end in ["stopped", "archived", "running"] {
        let (mut h, node, id) = delivered_unobserved(end);
        if end == "running" {
            h.run(
                &person(),
                Operation::Cancel {
                    node,
                    reason: "Superseded".into(),
                },
            )
            .unwrap();
        }
        let row = pending(&h).remove(0);
        let rev = h.store.team(TEAM).unwrap().unwrap().revision;
        let mut wrong = vec![(1, PERSON.to_owned())];
        if end == "running" {
            wrong.push((1, id.clone()));
        }
        for (attempt, attempt_id) in wrong {
            for state in [CancelState::Cancelled, CancelState::Released] {
                refused(
                    h.run(
                        &service(),
                        acted(node, row.decision, state, attempt, &attempt_id),
                    ),
                    code::INVALID_STATE,
                );
            }
        }
        assert_eq!(h.store.team(TEAM).unwrap().unwrap().revision, rev, "{end}");
        assert_eq!(outcomes(&h), 0, "{end}");
    }
    // A number below the observed one.
    let mut h = Harness::new();
    h.bind("m1");
    let node = h.add(&person(), 1);
    let id = h.submit(node, "m1");
    // The node's own effect observed with a higher number (attempts that
    // never started claimed generations 2 and 3): no take-over, the
    // generation stays, and only the number tells the answer apart.
    let mut o = h.observation(node, TaskStage::InProgress);
    (o.attempt, o.attempt_id) = (3, id.clone());
    h.run(
        &service(),
        Operation::ObserveTask {
            node,
            observation: o,
        },
    )
    .unwrap();
    assert_eq!(h.store.generation(TEAM, node).unwrap(), Some(1));
    h.run(&person(), Operation::Stop).unwrap();
    let row = pending(&h).remove(0);
    refused(
        h.run(
            &service(),
            acted(node, row.decision, CancelState::Cancelled, 2, &id),
        ),
        code::INVALID_STATE,
    );
    // An effect of another node.
    let (mut h, node, _) = delivered_unobserved("running");
    let other = h.add(&person(), 2);
    h.bind("m2");
    let foreign = h.intent(other, "m2");
    terminal(&mut h, "stopped");
    let row = pending(&h).into_iter().find(|r| r.node == node).unwrap();
    refused(
        h.run(
            &service(),
            acted(node, row.decision, CancelState::Cancelled, 1, &foreign),
        ),
        code::INVALID_STATE,
    );
    // An effect of an earlier generation: the first hand-out, after a
    // return and a reopen moved the node on.
    let mut h = Harness::new();
    let lead = h.bind("lead");
    h.bind("m1");
    let node = h.add(&person(), 1);
    let first = h.submit(node, "m1");
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
    h.reopen_outcome(
        &reopen,
        HandoutOutcome::Submitted {
            receipt: "receipt-example".into(),
        },
    );
    h.run(&person(), Operation::Stop).unwrap();
    assert_eq!(h.store.generation(TEAM, node).unwrap(), Some(2));
    let row = pending(&h).remove(0);
    refused(
        h.run(
            &service(),
            acted(node, row.decision, CancelState::Cancelled, 2, &first),
        ),
        code::INVALID_STATE,
    );
    // An effect of another kind: a row no writer makes, built to pin the
    // kind condition.
    let (mut h, node, id) = delivered_unobserved("stopped");
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute("UPDATE effects SET kind='clear' WHERE request_id=?1", [&id])
        .unwrap();
    let row = pending(&h).remove(0);
    refused(
        h.run(
            &service(),
            acted(node, row.decision, CancelState::Cancelled, 1, &id),
        ),
        code::INVALID_STATE,
    );
}
