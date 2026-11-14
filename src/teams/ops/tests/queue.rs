use super::*;
#[test]
fn task_is_unknown_until_service_observation_and_duplicate_sends_share_a_node() {
    let mut h = Harness::new();
    let a = h.enqueue(&person(), 1);
    assert_eq!(h.node(a).state, NodeState::Waiting);
    assert!(h.node(a).rank.is_empty());
    assert_eq!(h.enqueue(&person(), 1), a);
    assert_eq!(h.store.queue(TEAM, 200, None).unwrap().rows.len(), 1);
    h.observe(a, TaskStage::Todo);
    assert_eq!(h.node(a).state, NodeState::Pending);
}
#[test]
fn other_agents_suggestions_never_skip_lead_routing() {
    for routing in [Routing::Direct, Routing::Lead] {
        let mut h = Harness::new();
        let lead = h.bind("lead");
        if routing == Routing::Lead {
            h.settings(|s| s.routing = Routing::Lead);
        }
        let node = h
            .run(
                &other(),
                Operation::AddTask {
                    task: task(1),
                    role: Some("developer".into()),
                    pipeline: Some("reviewed".into()),
                },
            )
            .unwrap()
            .node
            .unwrap();
        h.observe(node, TaskStage::Todo);
        h.run(&lead, Operation::Admit { node }).unwrap();
        assert_eq!(h.node(node).state, NodeState::Waiting);
        assert_eq!(
            h.store
                .suggestion(TEAM, node)
                .unwrap()
                .unwrap()
                .role
                .as_deref(),
            Some("developer")
        );
        h.run(&lead, route(node)).unwrap();
        assert_eq!(h.node(node).state, NodeState::Pending);
    }
}
#[test]
fn person_and_lead_can_supply_routing_when_the_template_uses_lead_routing() {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    h.settings(|s| s.routing = Routing::Lead);
    for (n, actor) in [(1, person()), (2, lead)] {
        let node = h
            .run(
                &actor,
                Operation::AddTask {
                    task: task(n),
                    role: Some("developer".into()),
                    pipeline: Some("reviewed".into()),
                },
            )
            .unwrap()
            .node
            .unwrap();
        h.observe(node, TaskStage::Todo);
        assert_eq!(h.node(node).state, NodeState::Pending);
    }
}
#[test]
fn admitted_and_unadmitted_nodes_have_separate_caps() {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    h.settings(|s| s.caps.max_open_tasks = 1);
    let node = h.add(&person(), 1);
    for n in 2..=101 {
        h.enqueue(&other(), n);
    }
    refused(
        h.run(
            &other(),
            Operation::AddTask {
                task: task(102),
                role: None,
                pipeline: None,
            },
        ),
        code::OVER_CAP,
    );
    refused(h.run(&lead, Operation::Admit { node: 2 }), code::OVER_CAP);
    h.cancel_fact(node);
    h.run(&lead, Operation::Admit { node: 2 }).unwrap();
    assert_eq!(h.store.queue(TEAM, 200, None).unwrap().rows.len(), 100);
    h.enqueue(&other(), 102);
}
#[test]
fn hold_release_and_route_bind_only_plan_assignees() {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    let node = h.add(&person(), 1);
    refused(
        h.run(&lead, Operation::Release { node }),
        code::INVALID_STATE,
    );
    h.run(&lead, Operation::Hold { node }).unwrap();
    refused(h.run(&lead, Operation::Hold { node }), code::INVALID_STATE);
    h.run(&lead, Operation::Release { node }).unwrap();
    let mut op = route(node);
    if let Operation::Route { assignee, .. } = &mut op {
        *assignee = Some("m2".into());
    }
    h.run(&lead, op).unwrap();
    assert_eq!(h.node(node).assignee.as_deref(), Some("m2"));
}
#[test]
fn authoritative_tokens_use_compare_and_swap_not_sort_order() {
    let mut h = Harness::new();
    let node = h.add(&person(), 1);
    let mut o = h.observation(node, TaskStage::InProgress);
    o.revision = "ffffffff".into();
    h.run(
        &service(),
        Operation::ObserveTask {
            node,
            observation: o,
        },
    )
    .unwrap();
    let mut next = h.observation(node, TaskStage::InReview);
    next.revision = "00000000".into();
    h.run(
        &service(),
        Operation::ObserveTask {
            node,
            observation: next.clone(),
        },
    )
    .unwrap();
    next.revision = "late".into();
    next.previous_revision = "old".into();
    refused(
        h.run(
            &service(),
            Operation::ObserveTask {
                node,
                observation: next,
            },
        ),
        code::STALE_REVISION,
    );
    assert_eq!(h.node(node).state, NodeState::Reported);
}
fn create_second(h: &mut Harness) {
    let mut op = create();
    if let Operation::Create {
        integration_branch, ..
    } = &mut op
    {
        *integration_branch = "zerus/team/second".into();
    }
    h.run_for(TEAM_B, &person(), op).unwrap();
}
#[test]
fn cancellation_releases_a_task_for_another_team_but_live_ownership_does_not() {
    let mut h = Harness::new();
    let node = h.add(&person(), 1);
    create_second(&mut h);
    let op = Operation::AddTask {
        task: task(1),
        role: None,
        pipeline: None,
    };
    refused(h.run_for(TEAM_B, &person(), op.clone()), code::DUPLICATE);
    h.cancel_fact(node);
    h.run_for(TEAM_B, &person(), op).unwrap();
}
#[test]
fn every_node_of_a_stopped_or_archived_team_releases_its_task() {
    for archive in [false, true] {
        let mut h = Harness::new();
        h.settings(|s| s.integration = Integration::None);
        h.add(&person(), 1);
        let done = h.add(&person(), 2);
        h.observe(done, TaskStage::Done);
        create_second(&mut h);
        h.run(&person(), Operation::Stop).unwrap();
        if archive {
            h.run(&person(), Operation::Archive).unwrap();
        }
        for n in [1, 2] {
            h.run_for(
                TEAM_B,
                &person(),
                Operation::AddTask {
                    task: task(n),
                    role: None,
                    pipeline: None,
                },
            )
            .unwrap();
        }
    }
}
#[test]
fn restore_clears_acceptance_hold_and_journals_the_person_override() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    h.run(
        &service(),
        Operation::RecordIntegration {
            decision: h.accept_decision(node),
            node,
            commit: COMMIT.into(),
        },
    )
    .unwrap();
    h.observe(node, TaskStage::Done);
    h.observe(node, TaskStage::Todo);
    let n = h.node(node);
    assert!(!n.hold && !n.integrated && n.decided_by.is_none());
    assert_eq!(
        h.store
            .journal(TEAM, 200, None)
            .unwrap()
            .rows
            .last()
            .unwrap()
            .kind,
        "person_override"
    );
    h.intent(node, "m1");
}
#[test]
fn restore_does_not_overfill_admitted_capacity() {
    let mut h = Harness::new();
    h.settings(|s| s.integration = Integration::None);
    let node = h.add(&person(), 1);
    h.observe(node, TaskStage::Done);
    h.settings(|s| s.caps.max_open_tasks = 1);
    h.add(&person(), 2);
    let o = h.observation(node, TaskStage::Todo);
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
    assert_eq!(h.node(node).state, NodeState::Accepted);
}

#[test]
fn a_person_reopened_task_can_be_reported_in_its_new_attempt() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    h.run(
        &service(),
        Operation::RecordIntegration {
            decision: h.accept_decision(node),
            node,
            commit: COMMIT.into(),
        },
    )
    .unwrap();
    h.observe(node, TaskStage::Done);
    h.observe(node, TaskStage::Todo);
    assert_eq!(h.store.generation(TEAM, node).unwrap(), Some(2));
    h.submit(node, "m1");
    h.report(node, "m1");
    h.run(&lead, Operation::Accept { node }).unwrap();
}

#[test]
fn route_refuses_absolute_write_paths_from_all_platforms() {
    for path in [
        "/workspace/example.rs",
        "C:\\example.rs",
        "\\\\server\\share\\example.rs",
    ] {
        let mut h = Harness::new();
        let node = h.add(&person(), 1);
        let mut op = route(node);
        if let Operation::Route { writes, .. } = &mut op {
            writes.push(path.into());
        }
        refused(h.run(&person(), op), code::INVALID_STATE);
    }
}

#[test]
fn inactive_teams_record_person_reopens_only_on_the_archive_row() {
    for (archive, terminal) in [(false, true), (true, true), (false, false), (true, false)] {
        let mut h = Harness::new();
        let node = h.add(&person(), 1);
        if terminal {
            h.cancel_fact(node);
        }
        h.run(&person(), Operation::Stop).unwrap();
        if archive {
            h.run(&person(), Operation::Archive).unwrap();
        }
        h.observe(node, TaskStage::Todo);
        h.observe(node, TaskStage::InProgress);
        assert_eq!(h.node(node).state, NodeState::InProgress);
        assert!(h.store.queue(TEAM, 200, None).unwrap().rows.is_empty());
        let rows: u32 = h
            .store
            .db
            .as_ref()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM node_archive WHERE team=?1 AND id=?2",
                params![TEAM, node],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(rows, 1);
        let bindings: u32 = h
            .store
            .db
            .as_ref()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM live_tasks WHERE team=?1 AND node=?2",
                params![TEAM, node],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(bindings, 0);
    }
}
