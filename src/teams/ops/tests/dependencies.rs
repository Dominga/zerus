use super::*;

fn deps(node: u32, ids: &[u32]) -> Operation {
    Operation::SetDeps {
        node,
        deps: ids
            .iter()
            .map(|node| Dep {
                node: *node,
                kind: DepKind::Hard,
            })
            .collect(),
    }
}

#[test]
fn set_deps_is_person_only_even_for_the_bound_lead_and_service() {
    let mut h = Harness::new();
    let a = h.enqueue(&person(), 1);
    let b = h.enqueue(&person(), 2);
    let lead = h.bind("lead");
    let member = h.bind("m1");
    for actor in [lead, member, service(), other()] {
        refused(h.run(&actor, deps(b, &[a])), code::NOT_ALLOWED);
        assert!(h.node(b).deps.is_empty());
    }
    h.run(&person(), deps(b, &[a])).unwrap();
    assert_eq!(h.node(b).deps[0].node, a);
}

#[test]
fn set_deps_preserves_the_route_assignment_and_hold_and_can_remove_all_edges() {
    let mut h = Harness::new();
    let a = h.enqueue(&person(), 1);
    let b = h.enqueue(&person(), 2);
    h.run(&person(), Operation::Hold { node: b }).unwrap();
    let original = h.node(b);
    let mut soft = deps(b, &[a]);
    if let Operation::SetDeps { deps, .. } = &mut soft {
        deps[0].kind = DepKind::Soft;
    }
    h.run(&person(), soft).unwrap();
    let mut changed = h.node(b);
    assert_eq!(changed.deps[0].kind, DepKind::Soft);
    changed.deps.clear();
    assert_eq!(changed, original);
    h.run(&person(), deps(b, &[])).unwrap();
    assert_eq!(h.node(b), original);
}

#[test]
fn set_deps_refuses_transitive_and_soft_cycles_and_keeps_the_old_edges() {
    let mut h = Harness::new();
    let a = h.enqueue(&person(), 1);
    let b = h.enqueue(&person(), 2);
    let c = h.enqueue(&person(), 3);
    h.run(&person(), deps(b, &[a])).unwrap();
    h.run(&person(), deps(c, &[b])).unwrap();
    let mut candidate = deps(a, &[c]);
    if let Operation::SetDeps { deps, .. } = &mut candidate {
        deps[0].kind = DepKind::Soft;
    }
    refused(h.run(&person(), candidate), code::DEPENDENCY_CYCLE);
    assert!(h.node(a).deps.is_empty());
    assert_eq!(h.node(c).deps[0].node, b);
}

#[test]
fn set_deps_rejects_missing_repeated_self_and_excess_dependencies() {
    let mut h = Harness::new();
    let a = h.enqueue(&person(), 1);
    let b = h.enqueue(&person(), 2);
    for ids in [vec![a, a], vec![b], vec![999]] {
        refused(h.run(&person(), deps(b, &ids)), code::INVALID_STATE);
    }
    refused(h.run(&person(), deps(b, &[a; 201])), code::OVER_CAP);
    assert!(h.node(b).deps.is_empty());
}

#[test]
fn set_deps_binds_revision_and_replays_one_receipt_and_journal_row() {
    let mut h = Harness::new();
    let a = h.enqueue(&person(), 1);
    let b = h.enqueue(&person(), 2);
    let rev = h.store.team(TEAM).unwrap().unwrap().revision;
    let stale = request(rev - 1, deps(b, &[a]));
    refused(
        apply(&mut h.store, &person(), &stale, 1234),
        code::STALE_REVISION,
    );
    assert!(h.node(b).deps.is_empty());
    let req = request(rev, deps(b, &[a]));
    let first = apply(&mut h.store, &person(), &req, 1234).unwrap();
    assert_eq!(apply(&mut h.store, &person(), &req, 1235).unwrap(), first);
    let rows = h.store.journal(TEAM, 200, None).unwrap().rows;
    assert_eq!(rows.iter().filter(|row| row.kind == "set_deps").count(), 1);
    let changed = Request {
        operation: deps(b, &[]),
        ..req
    };
    refused(
        apply(&mut h.store, &person(), &changed, 1235),
        code::DUPLICATE,
    );
}

#[test]
fn set_deps_refuses_decided_tasks_and_stopped_teams() {
    let mut h = Harness::new();
    let a = h.enqueue(&person(), 1);
    let b = h.enqueue(&person(), 2);
    h.run(
        &person(),
        Operation::Cancel {
            node: a,
            reason: "No longer needed.".into(),
        },
    )
    .unwrap();
    h.cancel_fact(a);
    refused(h.run(&person(), deps(a, &[b])), code::INVALID_STATE);
    h.run(&person(), Operation::Stop).unwrap();
    refused(h.run(&person(), deps(b, &[])), code::INVALID_STATE);
}

#[test]
fn set_deps_refuses_a_pending_decision() {
    let mut h = Harness::new();
    let a = h.enqueue(&person(), 1);
    let b = h.enqueue(&person(), 2);
    h.run(
        &person(),
        Operation::Cancel {
            node: b,
            reason: "No longer needed.".into(),
        },
    )
    .unwrap();
    refused(h.run(&person(), deps(b, &[a])), code::INVALID_STATE);
}

#[test]
fn set_deps_can_edit_started_work_without_reassigning_its_holder() {
    let mut h = Harness::new();
    h.bind("m1");
    let a = h.enqueue(&person(), 1);
    let b = h.add(&person(), 2);
    h.submit(b, "m1");
    let original = h.node(b);
    h.run(&person(), deps(b, &[a])).unwrap();
    let mut changed = h.node(b);
    changed.deps.clear();
    assert_eq!(changed, original);
}

#[test]
fn set_deps_refuses_a_stage_created_by_the_pipeline() {
    let mut h = Harness::empty();
    h.run(&person(), create()).unwrap();
    h.bind("m1");
    h.bind("m3");
    let parent = h.add(&person(), 1);
    h.submit(parent, "m1");
    h.report(parent, "m1");
    let stage = h
        .store
        .queue(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .find(|node| node.kind == NodeKind::Stage && node.stage_of == Some(parent))
        .unwrap();
    refused(h.run(&person(), deps(stage.id, &[])), code::INVALID_STATE);
}

#[test]
fn set_deps_checks_edges_retained_by_an_archived_dependency() {
    let mut h = Harness::new();
    let a = h.enqueue(&person(), 1);
    let b = h.enqueue(&person(), 2);
    h.run(&person(), deps(a, &[b])).unwrap();
    h.run(
        &person(),
        Operation::Cancel {
            node: a,
            reason: "No longer needed.".into(),
        },
    )
    .unwrap();
    h.cancel_fact(a);
    assert_eq!(h.node(a).state, NodeState::Cancelled);
    assert_eq!(h.node(a).deps[0].node, b);
    refused(h.run(&person(), deps(b, &[a])), code::DEPENDENCY_CYCLE);
    assert!(h.node(b).deps.is_empty());
}
