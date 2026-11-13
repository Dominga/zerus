use super::*;

fn live_case(state: u32) -> (Harness, u32, Actor) {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    h.bind("m1");
    h.bind("m2");
    h.settings(|s| s.caps.max_open_tasks = 1);
    let node = h.add(&person(), 1);
    if state == 0 {
        h.intent(node, "m1");
    } else {
        h.submit(node, "m1");
    }
    if state >= 2 {
        h.report(node, "m1");
    }
    if state == 3 {
        h.run(&lead, Operation::Accept { node }).unwrap();
        h.observe(node, TaskStage::Done);
        assert!(!h.node(node).integrated);
    }
    (h, node, lead)
}
fn departure(h: &mut Harness, node: u32, key: KeyState) -> TaskObservation {
    let stage = h.store.task_observation(TEAM, node).unwrap().unwrap().stage;
    let mut o = h.observation(node, stage);
    o.key = key;
    o
}
fn assert_release_for_all_live_states(key: KeyState) {
    for state in 0..4 {
        let (mut h, node, lead) = live_case(state);
        let o = departure(&mut h, node, key);
        h.run(
            &service(),
            Operation::ObserveTask {
                node,
                observation: o.clone(),
            },
        )
        .unwrap();
        assert!(
            h.store.queue(TEAM, 200, None).unwrap().rows.is_empty(),
            "state {state}, key {key:?}"
        );
        assert_eq!(
            h.store.task_observation(TEAM, node).unwrap().unwrap().key,
            key
        );
        assert!(h
            .store
            .effects(TEAM, 200, None)
            .unwrap()
            .rows
            .iter()
            .all(|e| !matches!(
                e.state.as_str(),
                "reserved" | "submitted" | "uncertain" | "cutting_over"
            )));
        assert!(h
            .store
            .decisions(TEAM, 200, None)
            .unwrap()
            .rows
            .iter()
            .all(|d| d.state == DecisionState::Settled));
        let event = h
            .store
            .journal(TEAM, 200, None)
            .unwrap()
            .rows
            .pop()
            .unwrap();
        assert_eq!(event.kind, "person_override");
        assert!(event.text.contains("left the team"));
        // The old node cannot acquire a new plan decision after departure.
        refused(
            h.run(
                &lead,
                Operation::Reassign {
                    node,
                    member: "m2".into(),
                    reason: "Old assignment".into(),
                },
            ),
            code::INVALID_STATE,
        );
        let next = h.add(&person(), 1);
        assert_ne!(next, node);
        h.intent(next, "m1");
        // A late repeat on the old identity never releases the new binding.
        h.run(
            &service(),
            Operation::ObserveTask {
                node,
                observation: o,
            },
        )
        .unwrap();
        assert_eq!(h.enqueue(&person(), 1), next);
        assert_eq!(h.store.queue(TEAM, 200, None).unwrap().rows[0].id, next);
    }
}
#[test]
fn absent_key_releases_pending_in_progress_reported_and_accepted_nodes() {
    assert_release_for_all_live_states(KeyState::Absent);
}
#[test]
fn other_key_releases_pending_in_progress_reported_and_accepted_nodes() {
    assert_release_for_all_live_states(KeyState::Other);
}

#[test]
fn a_departed_tasks_ready_accept_keeps_its_late_integration_fact() {
    for key in [KeyState::Absent, KeyState::Other] {
        for conflict in [false, true] {
            let (mut h, node, _) = live_case(3);
            let decision = h.accept_decision(node);
            let o = departure(&mut h, node, key);
            h.run(
                &service(),
                Operation::ObserveTask {
                    node,
                    observation: o,
                },
            )
            .unwrap();
            assert!(h.store.queue(TEAM, 200, None).unwrap().rows.is_empty());
            assert!(h
                .store
                .decisions(TEAM, 200, None)
                .unwrap()
                .rows
                .iter()
                .all(|d| d.state == DecisionState::Settled));
            assert!(h
                .store
                .integration_accepts(TEAM, 200, None)
                .unwrap()
                .rows
                .is_empty());
            let next = h.add(&person(), 1);
            assert_ne!(next, node);
            let op = if conflict {
                Operation::RecordIntegrationConflict {
                    node,
                    decision,
                    conflicts: vec!["src/example.rs".into()],
                }
            } else {
                Operation::RecordIntegration {
                    node,
                    decision,
                    commit: COMMIT.into(),
                }
            };
            h.run(&service(), op.clone()).unwrap();
            assert_eq!(
                h.store
                    .integration_fact(TEAM, node)
                    .unwrap()
                    .unwrap()
                    .decision,
                decision
            );
            assert!(!h.node(next).integrated);
            assert_eq!(h.enqueue(&person(), 1), next);
            refused(h.run(&service(), op), code::INVALID_STATE);
        }
    }
}

#[test]
fn departure_closes_integration_cards_and_reserved_effects_without_reopening_them() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.settings(|s| s.delegations.merge_accepted = false);
    let card = h
        .run(&lead, Operation::Accept { node })
        .unwrap()
        .item
        .unwrap();
    let o = departure(&mut h, node, KeyState::Absent);
    h.run(
        &service(),
        Operation::ObserveTask {
            node,
            observation: o,
        },
    )
    .unwrap();
    let c = h.store.card(TEAM, card).unwrap().unwrap();
    assert_eq!(c.state, "closed");
    assert_eq!(c.decision.as_deref(), Some("stale"));
    let next = h.add(&person(), 2);
    let id = h.intent(next, "m1");
    let o = departure(&mut h, next, KeyState::Other);
    h.run(
        &service(),
        Operation::ObserveTask {
            node: next,
            observation: o,
        },
    )
    .unwrap();
    h.outcome(
        &id,
        HandoutOutcome::Submitted {
            receipt: "Late hand-out receipt".into(),
        },
    );
    assert!(h.node(next).implementer.is_none());
    assert!(h.store.queue(TEAM, 200, None).unwrap().rows.is_empty());
    assert!(h
        .store
        .effects(TEAM, 200, None)
        .unwrap()
        .rows
        .iter()
        .all(|e| e.state != "submitted"));
}

#[test]
fn archived_and_released_observations_record_facts_without_changing_the_plan() {
    for released in [false, true] {
        let mut h = Harness::new();
        let node = h.add(&person(), 1);
        if released {
            let o = departure(&mut h, node, KeyState::Absent);
            h.run(
                &service(),
                Operation::ObserveTask {
                    node,
                    observation: o,
                },
            )
            .unwrap();
        } else {
            h.settings(|s| s.integration = Integration::None);
            h.observe(node, TaskStage::Done);
        }
        let before = h.node(node);
        let decisions = h.store.decisions(TEAM, 200, None).unwrap().rows;
        let mut o = h.observation(node, TaskStage::InProgress);
        o.key = KeyState::Other;
        o.title = "New title outside the team".into();
        h.run(
            &service(),
            Operation::ObserveTask {
                node,
                observation: o.clone(),
            },
        )
        .unwrap();
        assert_eq!(h.node(node), before);
        assert_eq!(h.store.decisions(TEAM, 200, None).unwrap().rows, decisions);
        assert_eq!(
            h.store
                .task_observation(TEAM, node)
                .unwrap()
                .unwrap()
                .revision,
            o.revision
        );
        assert_eq!(
            h.store.task_observation(TEAM, node).unwrap().unwrap().key,
            KeyState::Other
        );
        assert!(h.store.queue(TEAM, 200, None).unwrap().rows.is_empty());
        if released {
            h.observe(node, TaskStage::Todo);
            assert_eq!(h.node(node), before);
            assert!(h.store.queue(TEAM, 200, None).unwrap().rows.is_empty());
        }
    }
}

#[test]
fn key_observations_keep_revision_checks_service_authority_and_exact_replay() {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    let member = h.bind("m1");
    let node = h.add(&person(), 1);
    let mut same = h.store.task_observation(TEAM, node).unwrap().unwrap();
    same.key = KeyState::Absent;
    refused(
        h.run(
            &service(),
            Operation::ObserveTask {
                node,
                observation: same,
            },
        ),
        code::STALE_REVISION,
    );
    let o = departure(&mut h, node, KeyState::Absent);
    for actor in [person(), lead, member, other()] {
        refused(
            h.run(
                &actor,
                Operation::ObserveTask {
                    node,
                    observation: o.clone(),
                },
            ),
            code::NOT_ALLOWED,
        );
    }
    let r = request(
        h.store.team(TEAM).unwrap().unwrap().revision,
        Operation::ObserveTask {
            node,
            observation: o.clone(),
        },
    );
    let first = super::super::apply(&mut h.store, &service(), &r, 1234).unwrap();
    let events = h.store.journal(TEAM, 200, None).unwrap().rows;
    assert_eq!(
        super::super::apply(&mut h.store, &service(), &r, 2345).unwrap(),
        first
    );
    assert_eq!(h.store.journal(TEAM, 200, None).unwrap().rows, events);
    let mut old = o;
    old.revision = "older-token".into();
    old.previous_revision = "obsolete-token".into();
    old.key = KeyState::Ours;
    refused(
        h.run(
            &service(),
            Operation::ObserveTask {
                node,
                observation: old,
            },
        ),
        code::STALE_REVISION,
    );
    assert!(h.store.queue(TEAM, 200, None).unwrap().rows.is_empty());
}

#[test]
fn ours_observations_are_unchanged_and_a_missing_key_defaults_to_ours() {
    let mut h = Harness::new();
    let node = h.add(&person(), 1);
    let mut o = h.observation(node, TaskStage::InProgress);
    assert_eq!(o.key, KeyState::Ours);
    let mut value = serde_json::to_value(&o).unwrap();
    assert!(
        value.get("key").is_none(),
        "Ours keeps legacy request content stable"
    );
    value.as_object_mut().unwrap().remove("key");
    let old_wire: TaskObservation = serde_json::from_value(value).unwrap();
    assert_eq!(old_wire.key, KeyState::Ours);
    h.run(
        &service(),
        Operation::ObserveTask {
            node,
            observation: old_wire,
        },
    )
    .unwrap();
    assert_eq!(h.node(node).state, NodeState::InProgress);
    assert_eq!(h.enqueue(&person(), 1), node);
    o = h.observation(node, TaskStage::InReview);
    h.run(
        &service(),
        Operation::ObserveTask {
            node,
            observation: o,
        },
    )
    .unwrap();
    assert_eq!(h.node(node).state, NodeState::Reported);
}

#[test]
fn a_departure_releases_uncertain_and_cutting_over_slots_without_retrying_delivery() {
    for outcome in [
        HandoutOutcome::Uncertain {
            reason: "Missing receipt".into(),
        },
        HandoutOutcome::CuttingOver {
            reason: "Writer changing over".into(),
        },
    ] {
        let mut h = Harness::new();
        h.bind("m1");
        let node = h.add(&person(), 1);
        let id = h.intent(node, "m1");
        h.outcome(&id, outcome);
        let o = departure(&mut h, node, KeyState::Absent);
        h.run(
            &service(),
            Operation::ObserveTask {
                node,
                observation: o,
            },
        )
        .unwrap();
        let next = h.add(&person(), 1);
        assert_ne!(next, node);
        h.intent(next, "m1");
        refused(
            h.run(
                &service(),
                Operation::HandoutOutcome {
                    effect_request_id: id,
                    outcome: HandoutOutcome::Submitted {
                        receipt: "Do not retry".into(),
                    },
                },
            ),
            code::INVALID_STATE,
        );
    }
}

#[test]
fn an_accepted_task_taken_back_by_the_person_ends_the_node_once() {
    for (name, stage, conflict, other) in [
        ("review", TaskStage::InReview, false, true),
        ("conflict", TaskStage::InReview, true, false),
        ("done conflict", TaskStage::Done, true, false),
    ] {
        let mut h = Harness::new();
        let lead = h.bind("lead");
        h.bind("m1");
        h.settings(|s| s.delegations.merge_accepted = false);
        let node = h.add(&person(), 1);
        h.submit(node, "m1");
        h.report(node, "m1");
        h.run(&lead, Operation::Accept { node }).unwrap();
        let decision = h.accept_decision(node);
        assert!(h
            .store
            .cards(TEAM, 200, None)
            .unwrap()
            .rows
            .iter()
            .any(|c| c.node == Some(node) && c.state == "open" && c.kind == "integration"), "{name}");
        let mut o = h.observation(node, stage);
        if other {
            o.attempt += 1;
            o.attempt_id = "person-attempt".into();
        }
        if conflict {
            o.attempt += 1;
            o.attempt_id.clear();
            o.attempt_conflict = true;
        }
        let request = Request {
            request_id: uuid::Uuid::new_v4().to_string(),
            team: TEAM.into(),
            expected_revision: h.store.team(TEAM).unwrap().unwrap().revision,
            operation: Operation::ObserveTask { node, observation: o },
        };
        apply(&mut h.store, &service(), &request, 1234).unwrap();
        assert!(h.store.queue(TEAM, 200, None).unwrap().rows.is_empty(), "{name}");
        let items: Vec<_> = h
            .store
            .inbox(TEAM, 200, None)
            .unwrap()
            .rows
            .into_iter()
            .filter(|i| i.node == node && i.kind == "accepted_superseded")
            .collect();
        assert_eq!(items.len(), 1, "{name}");
        assert!(items[0].text.contains("the person's own run replaced the team's accepted result"), "{name}");
        assert!(items[0].text.contains("The task document was left unchanged."), "{name}");
        assert!(!items[0].text.contains("stays In review"), "{name}");
        let decided = h
            .store
            .decisions(TEAM, 200, None)
            .unwrap()
            .rows
            .into_iter()
            .find(|d| d.seq == decision)
            .unwrap();
        assert_eq!(decided.state, DecisionState::Settled, "{name}");
        assert!(!h
            .store
            .cards(TEAM, 200, None)
            .unwrap()
            .rows
            .iter()
            .any(|c| c.node == Some(node) && c.state == "open"), "{name}");
        let journal = h.store.journal(TEAM, 200, None).unwrap().rows.pop().unwrap();
        assert_eq!(journal.kind, "accepted_superseded", "{name}");
        assert!(journal.text.contains("nothing was merged"), "{name}");
        apply(&mut h.store, &service(), &request, 1234).unwrap();
        assert_eq!(
            h.store
                .inbox(TEAM, 200, None)
                .unwrap()
                .rows
                .iter()
                .filter(|i| i.node == node && i.kind == "accepted_superseded")
                .count(),
            1,
            "{name}: replay wrote another item"
        );
    }
}

#[test]
fn a_remembered_person_attempt_under_an_acceptance_is_not_superseded() {
    use crate::teams::store::Store;
    let mut h = Harness::new();
    let _lead = h.bind("lead");
    h.bind("m1");
    let node = h.add(&person(), 1);
    h.submit(node, "m1");
    // The store sees the person's execution running, then In review.
    let mut running = h.observation(node, TaskStage::InProgress);
    running.attempt += 1;
    running.attempt_id = "person-attempt".into();
    h.run(
        &service(),
        Operation::ObserveTask {
            node,
            observation: running,
        },
    )
    .unwrap();
    h.observe(node, TaskStage::InReview);
    // The person accepts, the store is reopened, and the same remembered
    // attempt is observed In review again.
    h.run(&person(), Operation::Accept { node }).unwrap();
    h.store = Store::open(h.temp.path()).unwrap();
    let before = h.accept_decision(node);
    h.observe(node, TaskStage::InReview);
    assert!(!h.store.queue(TEAM, 200, None).unwrap().rows.is_empty());
    assert!(!h
        .store
        .inbox(TEAM, 200, None)
        .unwrap()
        .rows
        .iter()
        .any(|item| item.kind == "accepted_superseded"));
    assert_eq!(h.accept_decision(node), before);
}
