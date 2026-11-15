use super::*;
use crate::teams::ops::tests::*;

fn draft(ids: &[u32]) -> DraftInput {
    DraftInput {
        title: "Foundation".into(),
        barrier: true,
        nodes: ids
            .iter()
            .map(|id| DraftNode {
                id: *id,
                title: format!("Work {id}"),
                text: "Implement and check.\nKeep the existing behavior.".into(),
                criteria: vec!["The targeted checks pass".into()],
                role: "developer".into(),
                pipeline: "reviewed".into(),
                deps: vec![],
                assignee: None,
                writes: vec![format!("src/task{id}.rs")],
                estimate_minutes: 30,
                kind: NodeKind::Task,
            })
            .collect(),
    }
}
fn submit(h: &mut Harness, input: DraftInput) -> Result<Reply> {
    let phase = h.store.next_draft_phase(TEAM)?;
    h.run(
        &person(),
        Operation::SubmitDraftPhase {
            phase,
            draft: input,
        },
    )
}
fn edit(h: &mut Harness, phase: u32, generation: u64, input: DraftInput) -> Result<Reply> {
    h.run(
        &person(),
        Operation::EditDraftPhase {
            phase,
            generation,
            draft: input,
        },
    )
}

#[test]
fn drafts_and_edits_are_written_without_tasks_bindings_or_effects() {
    let mut h = Harness::new();
    submit(&mut h, draft(&[1, 2])).unwrap();
    let p = h.store.phase_draft(TEAM, 1).unwrap();
    assert_eq!(p.phase.state, PhaseState::Draft);
    assert!(p.editable && p.complete);
    assert_eq!(p.generation, Some(1));
    assert_eq!(p.next_node, 3);
    assert!(p
        .nodes
        .iter()
        .all(|n| n.node(1).task.is_none() && n.node(1).state == NodeState::Draft));
    assert!(readiness(&h.store, TEAM).unwrap().nodes.is_empty());
    let db = h.store.db.as_ref().unwrap();
    for table in [
        "nodes",
        "node_identity",
        "live_tasks",
        "effects",
        "stage_tasks",
        "decisions",
    ] {
        let count: u32 = db
            .query_row(
                &format!("SELECT count(*) FROM {table} WHERE team=?1"),
                [TEAM],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "{table}");
    }
    let mut input = draft(&[1, 3]);
    input.title = "Updated".into();
    edit(&mut h, 1, 1, input.clone()).unwrap();
    let p = h.store.phase_draft(TEAM, 1).unwrap();
    assert_eq!(p.phase.title, "Updated");
    assert_eq!(p.nodes, input.nodes);
    assert_eq!(p.generation, Some(2));
    assert_eq!(p.next_node, 4);
    refused(edit(&mut h, 1, 2, draft(&[1, 2])), code::INVALID_STATE);
    assert_eq!(h.store.phase_draft(TEAM, 1).unwrap(), p);
    let journal = h.store.journal(TEAM, 200, None).unwrap();
    assert!(journal
        .rows
        .iter()
        .any(|e| e.kind == "edit_draft_phase" && e.refs.contains(&"phase:1".to_owned())));
}

#[test]
fn only_person_and_exact_lead_write_drafts() {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    let member = h.bind("m1");
    for actor in [member, other(), service()] {
        refused(
            h.run(
                &actor,
                Operation::SubmitDraftPhase {
                    phase: 1,
                    draft: draft(&[1]),
                },
            ),
            code::NOT_ALLOWED,
        );
    }
    h.run(
        &lead,
        Operation::SubmitDraftPhase {
            phase: 1,
            draft: draft(&[1]),
        },
    )
    .unwrap();
    h.run(
        &lead,
        Operation::EditDraftPhase {
            phase: 1,
            generation: 1,
            draft: draft(&[1, 2]),
        },
    )
    .unwrap();
    assert_eq!(h.store.phase_draft(TEAM, 1).unwrap().generation, Some(2));
}

#[test]
fn receipt_replay_never_repeats_an_edit_and_stale_inputs_write_nothing() {
    let mut h = Harness::new();
    let r = request(
        1,
        Operation::SubmitDraftPhase {
            phase: 1,
            draft: draft(&[1]),
        },
    );
    let first = apply(&mut h.store, &person(), &r, 1234).unwrap();
    assert_eq!(apply(&mut h.store, &person(), &r, 1234).unwrap(), first);
    edit(&mut h, 1, 1, draft(&[1, 2])).unwrap();
    let actual = h.store.phase_draft(TEAM, 1).unwrap();
    assert_eq!(apply(&mut h.store, &person(), &r, 1234).unwrap(), first);
    assert_eq!(h.store.phase_draft(TEAM, 1).unwrap(), actual);
    let mut changed = r.clone();
    if let Operation::SubmitDraftPhase { draft, .. } = &mut changed.operation {
        draft.title = "Other".into();
    }
    refused(
        apply(&mut h.store, &person(), &changed, 1234),
        code::DUPLICATE,
    );
    refused(edit(&mut h, 1, 1, draft(&[1])), code::STALE_REVISION);
    let stale = request(
        1,
        Operation::EditDraftPhase {
            phase: 1,
            generation: 2,
            draft: draft(&[1]),
        },
    );
    refused(
        apply(&mut h.store, &person(), &stale, 1234),
        code::STALE_REVISION,
    );
    assert_eq!(h.store.phase_draft(TEAM, 1).unwrap(), actual);
}

#[test]
fn malformed_drafts_are_atomic_refusals_with_their_own_field_or_code() {
    for case in 0..12 {
        let mut h = Harness::new();
        let mut d = draft(&[1, 2]);
        let expected = match case {
            0 => {
                d.nodes[0].criteria.clear();
                code::INVALID_STATE
            }
            1 => {
                d.nodes[0].role = "missing".into();
                code::UNKNOWN_ROLE
            }
            2 => {
                d.nodes[0].pipeline = "missing".into();
                code::UNKNOWN_PIPELINE
            }
            3 => {
                d.nodes[0].assignee = Some("missing".into());
                code::UNKNOWN_MEMBER
            }
            4 => {
                d.nodes[0].deps = vec![Dep {
                    node: 2,
                    kind: DepKind::Hard,
                }];
                d.nodes[1].deps = vec![Dep {
                    node: 1,
                    kind: DepKind::Hard,
                }];
                code::DEPENDENCY_CYCLE
            }
            5 => {
                d.nodes[0].deps = vec![Dep {
                    node: 99,
                    kind: DepKind::Soft,
                }];
                code::INVALID_STATE
            }
            6 => {
                d.nodes[1].id = 1;
                code::INVALID_STATE
            }
            7 => {
                d.nodes[0].kind = NodeKind::Stage;
                code::INVALID_STATE
            }
            8 => {
                d.nodes[0].title = "unsafe\u{202e}title".into();
                code::INVALID_STATE
            }
            9 => {
                d.nodes[0].text.push('\0');
                code::INVALID_STATE
            }
            10 => {
                d.nodes[0].writes = vec!["../escape".into()];
                code::INVALID_STATE
            }
            _ => {
                d.nodes[0].assignee = Some("lead".into());
                code::INVALID_STATE
            }
        };
        refused(submit(&mut h, d), expected);
        assert_eq!(
            h.store.team(TEAM).unwrap().unwrap().revision,
            1,
            "case {case}"
        );
        assert!(h.store.phase_draft(TEAM, 1).is_err());
        assert_eq!(h.store.next_draft_node(TEAM).unwrap(), 1);
    }
}

#[test]
fn overlap_requires_a_dependency_and_cross_phase_hard_edges_are_refused() {
    let mut h = Harness::new();
    let mut d = draft(&[1, 2]);
    d.nodes[1].writes = d.nodes[0].writes.clone();
    refused(submit(&mut h, d.clone()), code::INVALID_STATE);
    d.nodes[1].deps.push(Dep {
        node: 1,
        kind: DepKind::Soft,
    });
    submit(&mut h, d).unwrap();
    let mut next = draft(&[3]);
    next.nodes[0].deps.push(Dep {
        node: 1,
        kind: DepKind::Hard,
    });
    refused(submit(&mut h, next.clone()), code::INVALID_STATE);
    next.nodes[0].deps[0].kind = DepKind::Soft;
    submit(&mut h, next).unwrap();
    let mut rewrite = draft(&[1, 2]);
    rewrite.nodes[0].deps.push(Dep {
        node: 3,
        kind: DepKind::Soft,
    });
    refused(edit(&mut h, 1, 1, rewrite), code::DEPENDENCY_CYCLE);
    refused(edit(&mut h, 1, 1, draft(&[2])), code::INVALID_STATE);
}

#[test]
fn a_later_barrier_orders_overlapping_phases_before_write_comparison() {
    let mut h = Harness::new();
    submit(&mut h, draft(&[1])).unwrap();
    let mut second = draft(&[2]);
    second.nodes[0].writes = draft(&[1]).nodes[0].writes.clone();
    second.barrier = false;
    refused(submit(&mut h, second.clone()), code::INVALID_STATE);
    second.barrier = true;
    submit(&mut h, second).unwrap();
    assert_eq!(h.store.phase_draft(TEAM, 2).unwrap().nodes.len(), 1);
}

#[test]
fn expensive_write_comparison_refuses_atomically_and_simpler_sets_can_be_saved() {
    let mut h = Harness::new();
    h.settings(|s| s.caps.max_phase_tasks = 20);
    let before = h.store.team(TEAM).unwrap().unwrap().revision;
    let mut d = draft(&(1..=20).collect::<Vec<_>>());
    for node in &mut d.nodes {
        node.writes = (0..200).map(|p| format!("{}/{p}", node.id)).collect();
    }
    assert!(serde_json::to_vec(&d).unwrap().len() < 262_000);
    let error = submit(&mut h, d.clone()).unwrap_err();
    assert_eq!(error.code, code::OVER_CAP);
    assert!(error.message.contains("write-set validation"));
    assert_eq!(h.store.team(TEAM).unwrap().unwrap().revision, before);
    assert_eq!(h.store.next_draft_node(TEAM).unwrap(), 1);
    assert!(h.store.phase_draft(TEAM, 1).is_err());
    for node in &mut d.nodes {
        node.writes.truncate(2);
    }
    submit(&mut h, d).unwrap();
    assert!(h.store.queue(TEAM, 200, None).unwrap().rows.is_empty());
}

#[test]
fn draft_and_queue_capacity_share_the_admitted_limit() {
    let mut h = Harness::new();
    h.settings(|s| s.caps.max_open_tasks = 2);
    submit(&mut h, draft(&[1, 2])).unwrap();
    refused(
        h.run(
            &person(),
            Operation::AddTask {
                task: task(99),
                role: None,
                pipeline: None,
            },
        ),
        code::OVER_CAP,
    );
    edit(&mut h, 1, 1, draft(&[1])).unwrap();
    h.enqueue(&person(), 99);
    refused(edit(&mut h, 1, 2, draft(&[1, 4])), code::OVER_CAP);
    let mut h = Harness::new();
    h.settings(|s| s.caps.max_phase_tasks = 1);
    refused(submit(&mut h, draft(&[1, 2])), code::OVER_CAP);
}

#[test]
fn draft_capacity_is_also_charged_when_a_report_wants_a_pipeline_stage() {
    for reserve_draft in [false, true] {
        let mut h = Harness::empty();
        let mut create = create();
        if let Operation::Create { bundle, .. } = &mut create {
            let catalog = zerus_team_plan::builtin();
            bundle.roles = catalog.roles;
            bundle.pipelines = catalog.pipelines;
            bundle.template.default_pipeline = "reviewed".into();
        }
        h.run(&person(), create).unwrap();
        h.bind("lead");
        h.bind("m1");
        h.bind("m3");
        let node = h.add(&person(), 1);
        h.settings(|s| s.caps.max_open_tasks = 2);
        h.submit(node, "m1");
        if reserve_draft {
            let id = h.store.next_draft_node(TEAM).unwrap();
            submit(&mut h, draft(&[id])).unwrap();
        }
        h.report(node, "m1");
        assert_eq!(h.node(node).state, NodeState::Reported);
        let stages = h.store.stage_tasks(TEAM, 200, None).unwrap().rows;
        assert_eq!(stages.len(), usize::from(!reserve_draft));
        if reserve_draft {
            assert!(h
                .store
                .inbox(TEAM, 200, None)
                .unwrap()
                .rows
                .iter()
                .any(|i| i.text.contains("cap is reached")));
            assert_eq!(h.store.queue(TEAM, 200, None).unwrap().rows.len(), 1);
        }
    }
}

#[test]
fn settings_cannot_reduce_the_admitted_cap_below_saved_drafts() {
    let mut h = Harness::new();
    submit(&mut h, draft(&[1, 2])).unwrap();
    let before = h.store.team(TEAM).unwrap().unwrap();
    let mut settings = before.settings.clone();
    settings.caps.max_open_tasks = 1;
    refused(
        h.run(&person(), Operation::ChangeSettings { settings }),
        code::OVER_CAP,
    );
    assert_eq!(h.store.team(TEAM).unwrap().unwrap(), before);
    assert_eq!(h.store.phase_draft(TEAM, 1).unwrap().nodes.len(), 2);
}

#[test]
fn stopping_or_archiving_a_team_makes_a_draft_read_only() {
    let mut h = Harness::new();
    submit(&mut h, draft(&[1])).unwrap();
    h.run(&person(), Operation::Stop).unwrap();
    assert!(!h.store.phase_draft(TEAM, 1).unwrap().editable);
    refused(edit(&mut h, 1, 1, draft(&[1])), code::INVALID_STATE);
    refused(submit(&mut h, draft(&[2])), code::INVALID_STATE);
}

#[test]
fn task_identity_and_runtime_facts_are_not_draft_input_fields() {
    let d = draft(&[1]);
    for field in ["task", "state", "integrated", "implementer", "admitted"] {
        let mut v = serde_json::to_value(&d).unwrap();
        v["nodes"][0][field] = serde_json::json!("forged");
        assert!(serde_json::from_value::<DraftInput>(v).is_err(), "{field}");
    }
}
