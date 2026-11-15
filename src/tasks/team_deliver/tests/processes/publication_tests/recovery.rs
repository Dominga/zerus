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
    for r in f.h.store.publication_work(TEAM).unwrap() {
        write(&f, &r).unwrap();
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
}
