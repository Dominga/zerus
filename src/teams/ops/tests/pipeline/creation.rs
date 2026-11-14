use super::*;

#[test]
fn a_stage_creation_identity_cannot_be_reserved_for_another_stage() {
    let (mut h, _, stage) = wanted();
    let id = uuid::Uuid::new_v4().to_string();
    h.run(&service(), creation(stage, &id)).unwrap();
    let parent = h.add(&person(), 2);
    h.submit(parent, "m1");
    h.report(parent, "m1");
    let second = stages(&h, parent)[0].id;
    refused(
        h.run(&service(), creation(second, &id)),
        code::INVALID_STATE,
    );
    assert_eq!(
        h.store
            .stage_tasks(TEAM, 200, None)
            .unwrap()
            .rows
            .iter()
            .filter(|r| r.state == "reserved")
            .count(),
        1
    );
}

#[test]
fn a_stage_cannot_claim_a_task_already_bound_to_a_live_node() {
    let (mut h, _, stage) = wanted();
    let id = uuid::Uuid::new_v4().to_string();
    h.run(&service(), creation(stage, &id)).unwrap();
    refused(h.run(&service(), created(&id, 1)), code::INVALID_STATE);
    assert!(h.node(stage).task.is_none());
    h.run(&service(), created(&id, 77)).unwrap();
    assert_eq!(h.node(stage).task, Some(task(77)));
}

#[test]
fn a_report_wants_a_stage_without_a_task_or_a_handout() {
    let mut h = pipeline_harness("reviewed");
    h.bind("m1");
    h.bind("m3");
    let parent = h.add(&person(), 1);
    h.submit(parent, "m1");
    h.report(parent, "m1");
    let rows = stages(&h, parent);
    assert_eq!(rows.len(), 1);
    let n = &rows[0];
    assert_eq!(n.task, None);
    assert_eq!(n.role, "reviewer");
    assert_eq!(n.implementer.as_deref(), Some("m1"));
    assert_eq!((n.round, n.stage_index), (1, 0));
    assert!(!readiness(&h.store, TEAM)
        .unwrap()
        .nodes
        .iter()
        .any(|x| x.id == n.id && x.state == NodeState::Pending));
    refused(
        h.run(
            &service(),
            Operation::HandoutIntent {
                node: n.id,
                member: "m3".into(),
                effect_request_id: uuid::Uuid::new_v4().to_string(),
            },
        ),
        code::INVALID_STATE,
    );
    assert!(h.store.inbox(TEAM, 200, None).unwrap().rows.is_empty());
}

#[test]
fn review_focus_4_only_the_implementer_has_the_review_role() {
    let mut h = Harness::empty();
    let mut op = create();
    if let Operation::Create { bundle, .. } = &mut op {
        bundle.pipelines[0].implementer = "reviewer".into();
    }
    h.run(&person(), op).unwrap();
    h.bind("m3");
    let n = h.add(&person(), 1);
    h.run(
        &person(),
        Operation::Route {
            node: n,
            role: "reviewer".into(),
            pipeline: "reviewed".into(),
            deps: vec![],
            writes: vec![],
            assignee: None,
        },
    )
    .unwrap();
    h.submit(n, "m3");
    h.report(n, "m3");
    assert!(stages(&h, n).is_empty());
    let items = h.store.inbox(TEAM, 200, None).unwrap().rows;
    assert_eq!(items.len(), 1);
    assert!(items[0].text.contains("only member"));
    assert!(items[0].text.contains("m3"));
}

#[test]
fn stage_creation_is_a_service_fact_and_one_effect() {
    let (mut h, _, stage) = wanted();
    let lead = h.bind("lead");
    let member = h.bind("m2");
    let id = uuid::Uuid::new_v4().to_string();
    for actor in [person(), lead.clone(), member.clone(), other()] {
        refused(h.run(&actor, creation(stage, &id)), code::NOT_ALLOWED);
    }
    h.run(&service(), creation(stage, &id)).unwrap();
    refused(
        h.run(
            &service(),
            creation(stage, &uuid::Uuid::new_v4().to_string()),
        ),
        code::INVALID_STATE,
    );
    for actor in [person(), lead, member, other()] {
        refused(h.run(&actor, created(&id, 77)), code::NOT_ALLOWED);
    }
    h.run(&service(), created(&id, 77)).unwrap();
    assert_eq!(h.node(stage).task, Some(task(77)));
    assert_eq!(h.node(stage).implementer.as_deref(), Some("m1"));
    assert_eq!(h.node(stage).state, NodeState::Waiting);
    refused(h.run(&service(), created(&id, 77)), code::INVALID_STATE);
    h.observe(stage, TaskStage::Todo);
    h.submit(stage, "m3");
    assert_eq!(h.node(stage).implementer.as_deref(), Some("m1"));
}

#[test]
fn a_refused_or_uncertain_stage_creation_is_never_reserved_again() {
    for status in ["refused", "uncertain"] {
        let (mut h, parent, stage) = wanted();
        let id = uuid::Uuid::new_v4().to_string();
        h.run(&service(), creation(stage, &id)).unwrap();
        h.run(&service(),op(serde_json::json!({"op":"stage_task_outcome","effect_request_id":id,"outcome":{"status":status,"reason":"Writer could not confirm creation"}}))).unwrap();
        refused(
            h.run(
                &service(),
                creation(stage, &uuid::Uuid::new_v4().to_string()),
            ),
            code::INVALID_STATE,
        );
        assert_eq!(h.store.queue(TEAM, 200, None).unwrap().rows.len(), 1);
        assert!(h
            .store
            .inbox(TEAM, 200, None)
            .unwrap()
            .rows
            .iter()
            .any(|i| i.node == parent && i.text.contains("confirm creation")));
    }
}

#[test]
fn stage_task_reads_are_bounded_scoped_and_authorize_only_a_current_reserved_effect() {
    let (mut h, parent, stage) = wanted();
    let page = h.store.stage_tasks(TEAM, 1, None).unwrap();
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.rows[0].node, stage);
    assert_eq!(page.rows[0].state, "wanted");
    let cursor = page.next.unwrap();
    refused(
        h.store.stage_tasks(TEAM_B, 1, Some(&cursor)),
        code::INVALID_STATE,
    );
    refused(h.store.stage_tasks(TEAM, 201, None), code::OVER_CAP);
    assert!(h
        .store
        .stage_tasks(TEAM, 1, Some(&cursor))
        .unwrap()
        .rows
        .is_empty());
    let id = uuid::Uuid::new_v4().to_string();
    assert!(h
        .store
        .stage_task_effect(TEAM, stage, &id)
        .unwrap()
        .is_none());
    h.run(&service(), creation(stage, &id)).unwrap();
    assert!(h
        .store
        .stage_task_effect(TEAM, stage, &uuid::Uuid::new_v4().to_string())
        .unwrap()
        .is_none());
    let e = h
        .store
        .stage_task_effect(TEAM, stage, &id)
        .unwrap()
        .unwrap();
    assert_eq!(e.target, task(1));
    assert_eq!(e.target_title, h.node(parent).title);
    assert_eq!(e.title, h.node(stage).title);
    assert!(e.text.contains(COMMIT));
    assert!(e.text.contains("Implemented and checked"));
    assert_eq!(e.text, h.node(stage).text);
    assert_eq!(e.kind, NodeKind::Stage);
    assert_eq!(e.role, "reviewer");
    assert_eq!(e.round, 1);
    assert_eq!(e.project, "example-project");
    assert_eq!(e.home, fixture().team.home);
    assert!(h
        .store
        .stage_task_effect(TEAM_B, stage, &id)
        .unwrap()
        .is_none());
    assert!(h
        .store
        .stage_task_effect(TEAM, parent, &id)
        .unwrap()
        .is_none());
    h.run(&person(), Operation::Pause).unwrap();
    assert!(h
        .store
        .stage_task_effect(TEAM, stage, &id)
        .unwrap()
        .is_none());
    h.run(&person(), Operation::Resume).unwrap();
    h.run(&service(), created(&id, 77)).unwrap();
    assert!(h
        .store
        .stage_tasks(TEAM, 200, None)
        .unwrap()
        .rows
        .is_empty());
    assert!(h
        .store
        .stage_task_effect(TEAM, stage, &id)
        .unwrap()
        .is_none());
    let empty = Harness::empty();
    assert!(empty
        .store
        .stage_tasks(TEAM, 200, None)
        .unwrap()
        .rows
        .is_empty());
    assert!(empty
        .store
        .stage_task_effect(TEAM, 1, &id)
        .unwrap()
        .is_none());
    std::fs::remove_file(empty.temp.path().join("teams/preview")).unwrap();
    refused(empty.store.stage_tasks(TEAM, 1, None), code::TEAMS_INACTIVE);
    refused(
        empty.store.stage_task_effect(TEAM, 1, &id),
        code::TEAMS_INACTIVE,
    );
}

#[test]
fn a_persons_board_override_retires_stages_and_late_creation_is_provenance_only() {
    for action in ["reopen", "cancel", "done", "absent", "other"] {
        let (mut h, parent, stage) = wanted();
        let id = uuid::Uuid::new_v4().to_string();
        h.run(&service(), creation(stage, &id)).unwrap();
        let mut o = h.observation(
            parent,
            match action {
                "done" => TaskStage::Done,
                _ => TaskStage::Todo,
            },
        );
        o.cancelled = action == "cancel";
        o.key = match action {
            "absent" => KeyState::Absent,
            "other" => KeyState::Other,
            _ => KeyState::Ours,
        };
        h.run(
            &service(),
            Operation::ObserveTask {
                node: parent,
                observation: o,
            },
        )
        .unwrap();
        assert!(
            h.store
                .stage_tasks(TEAM, 200, None)
                .unwrap()
                .rows
                .is_empty(),
            "{action}"
        );
        assert!(h
            .store
            .stage_task_effect(TEAM, stage, &id)
            .unwrap()
            .is_none());
        h.run(&service(), created(&id, 77)).unwrap();
        assert_eq!(h.node(stage).task, None);
        assert!(!h
            .store
            .queue(TEAM, 200, None)
            .unwrap()
            .rows
            .iter()
            .any(|n| n.id == stage));
        if action == "reopen" {
            assert_eq!(h.node(parent).round, 2);
            h.submit(parent, "m1");
            h.report(parent, "m1");
            assert_eq!(stages(&h, parent).len(), 1);
        }
    }
}
#[test]
fn a_busy_reviewer_waits_but_a_removed_reviewer_escalates() {
    for removed in [false, true] {
        let mut h = pipeline_harness("reviewed");
        h.bind("m1");
        h.bind("m3");
        if removed {
            h.run(
                &person(),
                Operation::ArchiveMember {
                    member: "m3".into(),
                },
            )
            .unwrap();
        } else {
            h.run(
                &person(),
                Operation::PauseMember {
                    member: "m3".into(),
                },
            )
            .unwrap();
        }
        let parent = h.add(&person(), 1);
        h.submit(parent, "m1");
        h.report(parent, "m1");
        if removed {
            assert!(stages(&h, parent).is_empty());
            assert_eq!(h.store.inbox(TEAM, 200, None).unwrap().rows.len(), 1);
        } else {
            let stage = stages(&h, parent)[0].id;
            let id = uuid::Uuid::new_v4().to_string();
            h.run(&service(), creation(stage, &id)).unwrap();
            h.run(&service(), created(&id, 77)).unwrap();
            h.observe(stage, TaskStage::Todo);
            refused(
                h.run(
                    &service(),
                    Operation::HandoutIntent {
                        node: stage,
                        member: "m3".into(),
                        effect_request_id: uuid::Uuid::new_v4().to_string(),
                    },
                ),
                code::INVALID_STATE,
            );
            assert!(h.store.inbox(TEAM, 200, None).unwrap().rows.is_empty());
            h.run(
                &person(),
                Operation::ResumeMember {
                    member: "m3".into(),
                },
            )
            .unwrap();
            h.submit(stage, "m3");
        }
    }
}
#[test]
fn stage_creation_replays_its_receipt_without_reserving_or_binding_twice() {
    let (mut h, _, stage) = wanted();
    let id = uuid::Uuid::new_v4().to_string();
    let intent = request(
        h.store.team(TEAM).unwrap().unwrap().revision,
        creation(stage, &id),
    );
    let r = apply(&mut h.store, &service(), &intent, 1234).unwrap();
    assert_eq!(apply(&mut h.store, &service(), &intent, 1234).unwrap(), r);
    let outcome = request(r.revision, created(&id, 77));
    let r = apply(&mut h.store, &service(), &outcome, 1234).unwrap();
    assert_eq!(apply(&mut h.store, &service(), &outcome, 1234).unwrap(), r);
    assert_eq!(h.store.team(TEAM).unwrap().unwrap().revision, r.revision);
    assert_eq!(h.store.queue(TEAM, 200, None).unwrap().rows.len(), 2);
}

#[test]
fn changing_or_cancelling_a_reported_parent_closes_its_stage_creation() {
    for action in ["return", "reassign", "cancel"] {
        let (mut h, parent, stage) = wanted();
        h.bind("m2");
        let operation = match action {
            "return" => Operation::Return {
                node: parent,
                reason: "Person requests another implementation".into(),
            },
            "reassign" => Operation::Reassign {
                node: parent,
                member: "m2".into(),
                reason: "Person changes the implementer".into(),
            },
            _ => Operation::Cancel {
                node: parent,
                reason: "Person cancels this task".into(),
            },
        };
        h.run(&person(), operation).unwrap();
        assert!(
            h.store
                .stage_tasks(TEAM, 200, None)
                .unwrap()
                .rows
                .is_empty(),
            "{action}"
        );
        assert!(!h
            .store
            .queue(TEAM, 200, None)
            .unwrap()
            .rows
            .iter()
            .any(|n| n.id == stage));
    }
}

#[test]
fn a_report_before_the_delivery_outcome_still_excludes_its_implementer_from_review() {
    for result in ["reserved", "uncertain"] {
        let mut h = Harness::empty();
        let mut create = create();
        if let Operation::Create { bundle, .. } = &mut create {
            bundle.pipelines[0].implementer = "reviewer".into();
        }
        h.run(&person(), create).unwrap();
        h.bind("m3");
        let parent = h.add(&person(), 1);
        let id = h.intent(parent, "m3");
        if result == "uncertain" {
            h.outcome(
                &id,
                HandoutOutcome::Uncertain {
                    reason: "Delivery outcome was lost".into(),
                },
            );
        }
        h.observe(parent, TaskStage::InProgress);
        h.report(parent, "m3");
        assert!(stages(&h, parent).is_empty(), "{result}");
        assert_eq!(h.node(parent).implementer.as_deref(), Some("m3"));
        assert!(h
            .store
            .inbox(TEAM, 200, None)
            .unwrap()
            .rows
            .iter()
            .any(|i| i.text.contains("only member")));
    }
}
