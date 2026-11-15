use super::*;

#[test]
fn three_unreadable_recoveries_stay_uncertain_and_never_become_created() {
    let (mut f, _, requests) = fixture();
    let r = &requests[0];
    let mut child = paused(&f, r, "publication_claim");
    child.child.kill().unwrap();
    child.child.wait().unwrap();
    drop(child);
    for _ in 0..3 {
        f.h.run(
            &ops::tests::service(),
            Operation::PublicationTaskProblem {
                publication: r.publication.clone(),
                node: r.node.id,
                reason: "The private task read is unavailable.".into(),
                kind: ops::PublicationProblemKind::Uncertain,
                clear: false,
            },
        )
        .unwrap();
    }
    let p = f.h.store.phase_draft(TEAM, 1).unwrap().publication.unwrap();
    assert_eq!(p.created, 0);
    assert_eq!(p.blocked, 1);
    assert!(f
        .h
        .store
        .publication_work(TEAM)
        .unwrap()
        .iter()
        .all(|n| n.task.uuid != r.task.uuid));
    assert!(write(&f, r).is_err());
    assert!(!super::super::super::super::task_present(&f.config, &r.task).unwrap());
}

#[test]
fn a_pause_winning_after_the_claim_records_a_non_write_before_resuming() {
    let (mut f, _, requests) = fixture();
    let r = &requests[0];
    let mut child = paused(&f, r, "publication_claim");
    f.h.run(&person(), Operation::Pause).unwrap();
    std::fs::write(f.config.state.join("publication_claim.resume"), "resume").unwrap();
    assert!(child.child.wait().unwrap().success());
    drop(child);
    assert!(!super::super::super::super::task_present(&f.config, &r.task).unwrap());
    let state: String =
        f.h.store
            .db
            .as_ref()
            .unwrap()
            .query_row(
                "SELECT state FROM phase_publication_nodes WHERE team=?1 AND node=?2",
                rusqlite::params![TEAM, r.node.id],
                |row| row.get(0),
            )
            .unwrap();
    assert_eq!(state, "pending");
    f.h.run(
        &ops::tests::service(),
        Operation::PublicationTaskProblem {
            publication: r.publication.clone(),
            node: r.node.id,
            reason: "Pause won before the document write.".into(),
            kind: ops::PublicationProblemKind::Refused,
            clear: false,
        },
    )
    .unwrap();
    f.h.run(&person(), Operation::Resume).unwrap();
    assert_eq!(write(&f, r).unwrap(), "created");
    assert_eq!(
        f.h.store
            .planned_task(TEAM, r.node.id, &r.task.uuid)
            .unwrap()
            .as_ref(),
        Some(r)
    );
}

#[test]
fn a_final_cleanup_refusal_is_kept_and_not_silently_retried() {
    let (mut f, _, requests) = fixture();
    let r = &requests[0];
    write(&f, r).unwrap();
    f.h.run(&person(), Operation::Stop).unwrap();
    let tasks = crate::tasks::store(&f.config, &r.task.project).unwrap();
    let mut doc = tasks.load(&r.task.uuid).unwrap();
    doc.set_team_named(
        Some(&zerus_replica::TaskTeam {
            team: ops::tests::TEAM_B.into(),
            home: f.lead.machine.clone(),
        }),
        "Another team",
        &Actor::Person,
        now(),
    )
    .unwrap();
    tasks.save(&mut doc).unwrap();
    let revision = doc.revision();
    drop(tasks);
    assert!(super::super::super::super::planned::write(
        &f.config,
        TEAM,
        r.node.id,
        &r.task.uuid,
        true
    )
    .is_err());
    f.h.run(
        &ops::tests::service(),
        Operation::PublicationTaskProblem {
            publication: r.publication.clone(),
            node: r.node.id,
            reason: "The task has another team's key.".into(),
            kind: ops::PublicationProblemKind::Refused,
            clear: true,
        },
    )
    .unwrap();
    assert!(f
        .h
        .store
        .publication_cleanup(TEAM)
        .unwrap()
        .iter()
        .all(|n| n.task.uuid != r.task.uuid));
    assert!(super::super::super::super::planned::write(
        &f.config,
        TEAM,
        r.node.id,
        &r.task.uuid,
        true
    )
    .is_err());
    let tasks = crate::tasks::store(&f.config, &r.task.project).unwrap();
    assert_eq!(tasks.load(&r.task.uuid).unwrap().revision(), revision);
}

#[test]
fn published_dependencies_still_constrain_a_later_draft() {
    let mut f = Fixture::new();
    let first = f.h.store.next_draft_node(TEAM).unwrap();
    f.h.run(
        &person(),
        Operation::SubmitDraftPhase {
            phase: 1,
            draft: ops::publication::tests::input(&[first]),
        },
    )
    .unwrap();
    f.h.run(
        &person(),
        Operation::SubmitDraftPhase {
            phase: 2,
            draft: ops::publication::tests::input(&[first + 1, first + 2]),
        },
    )
    .unwrap();
    let mut a = ops::publication::tests::input(&[first]);
    a.nodes[0].deps.push(zerus_team_plan::Dep {
        node: first + 1,
        kind: zerus_team_plan::DepKind::Soft,
    });
    f.h.run(
        &person(),
        Operation::EditDraftPhase {
            phase: 1,
            generation: 1,
            draft: a,
        },
    )
    .unwrap();
    f.h.run(
        &person(),
        Operation::ApprovePhase {
            phase: 1,
            generation: 2,
            confirm_initial: false,
        },
    )
    .unwrap();
    let requests = f.h.store.publication_work(TEAM).unwrap();
    for r in &requests {
        write(&f, r).unwrap();
    }
    let p =
        f.h.store
            .phase_draft(TEAM, 1)
            .unwrap()
            .publication
            .unwrap()
            .id;
    f.h.run(
        &ops::tests::service(),
        Operation::ReconcilePublication {
            phase: 1,
            publication: p,
        },
    )
    .unwrap();
    let original_edge = f.h.store.node(TEAM, first).unwrap().unwrap().deps;
    assert_eq!(original_edge[0].node, first + 1);
    assert!(f.h.store.node(TEAM, first + 1).unwrap().is_none());
    crate::teams::tasks::observe(&f.config.state, &requests[0].task).unwrap();
    let readiness = ops::readiness(&f.h.store, TEAM).unwrap();
    let settings = f.h.store.team(TEAM).unwrap().unwrap().settings;
    let view = zerus_team_plan::ready::PlanView {
        phases: &readiness.phases,
        nodes: &readiness.nodes,
        members: &[],
        lead: "lead",
        integration: settings.integration,
    };
    assert!(!zerus_team_plan::ready::ready(&view).contains(&first));
    let mut b = ops::publication::tests::input(&[first + 1, first + 2]);
    b.nodes[0].deps.push(zerus_team_plan::Dep {
        node: first,
        kind: zerus_team_plan::DepKind::Soft,
    });
    assert_eq!(
        f.h.run(
            &person(),
            Operation::EditDraftPhase {
                phase: 2,
                generation: 1,
                draft: b
            }
        )
        .unwrap_err()
        .code,
        zerus_team_plan::problem::code::DEPENDENCY_CYCLE
    );
    assert!(f
        .h
        .run(
            &person(),
            Operation::EditDraftPhase {
                phase: 2,
                generation: 1,
                draft: ops::publication::tests::input(&[first + 2])
            }
        )
        .is_err());
    let mut edited = ops::publication::tests::input(&[first + 1, first + 2]);
    edited.nodes[0].text = "A legitimate edit keeps the referenced draft identity.".into();
    f.h.run(
        &person(),
        Operation::EditDraftPhase {
            phase: 2,
            generation: 1,
            draft: edited,
        },
    )
    .unwrap();
    f.h.run(
        &person(),
        Operation::ApprovePhase {
            phase: 2,
            generation: 2,
            confirm_initial: false,
        },
    )
    .unwrap();
    for r in f.h.store.publication_work(TEAM).unwrap() {
        write(&f, &r).unwrap();
    }
    let p =
        f.h.store
            .phase_draft(TEAM, 2)
            .unwrap()
            .publication
            .unwrap()
            .id;
    f.h.run(
        &ops::tests::service(),
        Operation::ReconcilePublication {
            phase: 2,
            publication: p,
        },
    )
    .unwrap();
    assert_eq!(
        f.h.store.node(TEAM, first).unwrap().unwrap().deps,
        original_edge
    );
    assert!(f.h.store.node(TEAM, first + 1).unwrap().is_some());
    let db = f.h.store.db.as_ref().unwrap();
    let pending: u32 = db
        .query_row(
            "SELECT count(*) FROM phase_draft_dependencies WHERE team=?1",
            [TEAM],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(pending, 0);
    let violations: u32 = db
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(violations, 0);
}

#[test]
fn a_late_creation_refusal_after_stop_does_not_answer_the_cleanup() {
    let (mut f, _, requests) = fixture();
    let r = &requests[0];
    write(&f, r).unwrap();
    f.h.run(&person(), Operation::Stop).unwrap();
    f.h.run(
        &ops::tests::service(),
        Operation::PublicationTaskProblem {
            publication: r.publication.clone(),
            node: r.node.id,
            reason: "Stop won the creation command's fresh admission.".into(),
            kind: ops::PublicationProblemKind::Refused,
            clear: false,
        },
    )
    .unwrap();
    assert!(f
        .h
        .store
        .publication_cleanup(TEAM)
        .unwrap()
        .iter()
        .any(|n| n.task.uuid == r.task.uuid));
    assert_eq!(
        super::super::super::super::planned::write(&f.config, TEAM, r.node.id, &r.task.uuid, true)
            .unwrap(),
        "cleared"
    );
    let tasks = crate::tasks::store(&f.config, &r.task.project).unwrap();
    assert!(tasks.load(&r.task.uuid).unwrap().team().unwrap().is_none());
}

#[test]
fn an_active_creation_refusal_is_final_before_the_claim() {
    let (mut f, _, requests) = fixture();
    let r = &requests[0];
    f.h.run(
        &ops::tests::service(),
        Operation::PublicationTaskProblem {
            publication: r.publication.clone(),
            node: r.node.id,
            reason: "The command refused the stored request before creation.".into(),
            kind: ops::PublicationProblemKind::Refused,
            clear: false,
        },
    )
    .unwrap();
    assert!(f
        .h
        .store
        .publication_work(TEAM)
        .unwrap()
        .iter()
        .all(|n| n.task.uuid != r.task.uuid));
    assert!(write(&f, r).is_err());
    assert!(!super::super::super::super::task_present(&f.config, &r.task).unwrap());
}

#[test]
fn cleanup_has_its_own_unknown_budget_and_clears_only_the_key_without_history() {
    for cleanup_failures in [1, 3] {
        let (mut f, _, requests) = fixture();
        let r = &requests[0];
        write(&f, r).unwrap();
        for _ in 0..3 {
            f.h.run(
                &ops::tests::service(),
                Operation::PublicationTaskProblem {
                    publication: r.publication.clone(),
                    node: r.node.id,
                    reason: "The creation answer was lost.".into(),
                    kind: ops::PublicationProblemKind::Uncertain,
                    clear: false,
                },
            )
            .unwrap();
        }
        f.h.run(&person(), Operation::Stop).unwrap();
        for _ in 0..cleanup_failures {
            f.h.run(
                &ops::tests::service(),
                Operation::PublicationTaskProblem {
                    publication: r.publication.clone(),
                    node: r.node.id,
                    reason: "The cleanup answer is unavailable.".into(),
                    kind: ops::PublicationProblemKind::Uncertain,
                    clear: true,
                },
            )
            .unwrap();
        }
        let selected =
            f.h.store
                .publication_cleanup(TEAM)
                .unwrap()
                .iter()
                .any(|n| n.task.uuid == r.task.uuid);
        assert_eq!(selected, cleanup_failures < 3);
        let tasks = crate::tasks::store(&f.config, &r.task.project).unwrap();
        let before = tasks
            .load(&r.task.uuid)
            .unwrap()
            .view_with_history_ids()
            .unwrap();
        drop(tasks);
        let result = super::super::super::super::planned::write(
            &f.config,
            TEAM,
            r.node.id,
            &r.task.uuid,
            true,
        );
        if cleanup_failures == 3 {
            assert!(result.is_err());
            continue;
        }
        assert_eq!(result.unwrap(), "cleared");
        let tasks = crate::tasks::store(&f.config, &r.task.project).unwrap();
        let doc = tasks.load(&r.task.uuid).unwrap();
        let revision = doc.revision();
        let after = doc.view_with_history_ids().unwrap();
        assert!(after.0.team.is_none());
        assert_eq!(before.1, after.1);
        assert_eq!(
            serde_json::to_value(before.0.log).unwrap(),
            serde_json::to_value(after.0.log).unwrap()
        );
        drop(tasks);
        assert_eq!(
            super::super::super::super::planned::write(
                &f.config,
                TEAM,
                r.node.id,
                &r.task.uuid,
                true
            )
            .unwrap(),
            "cleared"
        );
        let tasks = crate::tasks::store(&f.config, &r.task.project).unwrap();
        assert_eq!(tasks.load(&r.task.uuid).unwrap().revision(), revision);
    }
}
