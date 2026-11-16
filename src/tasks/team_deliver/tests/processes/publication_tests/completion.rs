use super::*;

pub(super) fn reported_stage() -> (
    Fixture,
    String,
    Vec<crate::teams::store::publication::PlannedTaskRequest>,
    u32,
) {
    let (mut f, p, requests) = fixture();
    let home = f.lead.machine.clone();
    f.bind("m2", &home);
    f.bind("m3", &home);
    for r in &requests {
        write(&f, r).unwrap();
    }
    f.h.run(
        &ops::tests::service(),
        Operation::ReconcilePublication {
            phase: 1,
            publication: p.clone(),
        },
    )
    .unwrap();
    for r in &requests {
        crate::teams::tasks::observe(&f.config.state, &r.task).unwrap();
    }
    let target = requests[0].node.id;
    // Native report/attempt observations are supplied to the real operation
    // writers; this test runs no native agent.
    f.h.submit(target, "m2");
    f.h.report(target, "m2");
    let stage = f.h.store.stage_tasks(TEAM, 1, None).unwrap().rows[0].node;
    (f, p, requests, stage)
}

#[test]
fn the_complete_live_snapshot_keeps_pipeline_stages_and_their_unknown_estimate() {
    let (f, _, requests, stage) = reported_stage();
    let target = requests[0].node.id;
    let snapshot = f.h.store.phase_draft(TEAM, 1).unwrap().live.unwrap();
    assert!(snapshot.complete);
    assert_eq!(snapshot.nodes.len(), 3);
    let row = snapshot.nodes.iter().find(|n| n.id == stage).unwrap();
    assert_eq!(row.kind, zerus_team_plan::NodeKind::Stage);
    assert_eq!(row.stage_of, Some(target));
    assert_eq!(row.phase, 1);
    assert_eq!(row.estimate_minutes, 0);
    let metrics =
        zerus_client_core::teams::view::phase_metrics::project(&snapshot.nodes, snapshot.complete);
    assert_eq!(metrics.total_nodes, 3);
    assert_eq!(metrics.known_estimates, 2);
    assert!(metrics.critical_path_minutes.is_none());
    assert!(metrics.average_parallelism_tenths.is_none());
}

#[test]
fn a_retired_uncreated_stage_is_complete_only_with_its_own_superseded_record() {
    let (mut f, p, requests, stage) = reported_stage();
    let mut settings = f.h.store.team(TEAM).unwrap().unwrap().settings;
    settings.integration = zerus_team_plan::Integration::None;
    f.h.run(&person(), Operation::ChangeSettings { settings })
        .unwrap();
    for r in &requests {
        f.h.observe(r.node.id, ops::TaskStage::Done);
    }
    let snapshot = f.h.store.phase_draft(TEAM, 1).unwrap().live.unwrap();
    let row = snapshot.nodes.iter().find(|n| n.id == stage).unwrap();
    assert!(snapshot.complete && row.task.is_none());
    assert_eq!(row.state, zerus_team_plan::NodeState::Waiting);
    let state: String =
        f.h.store
            .db
            .as_ref()
            .unwrap()
            .query_row(
                "SELECT state FROM stage_tasks WHERE team=?1 AND node=?2",
                rusqlite::params![TEAM, stage],
                |r| r.get(0),
            )
            .unwrap();
    assert_eq!(state, "superseded");
    assert!(ops::publication::complete(f.h.store.db.as_ref().unwrap(), TEAM, 1).unwrap());
    f.h.run(
        &ops::tests::service(),
        Operation::ReconcilePhase {
            phase: 1,
            publication: p.clone(),
        },
    )
    .unwrap();
    // Fault injection removes the positive lifecycle proof; archived alone
    // must never prove the stage ended. Normal rows above use only writers.
    f.h.store
        .db
        .as_ref()
        .unwrap()
        .execute(
            "UPDATE stage_tasks SET state='refused' WHERE team=?1 AND node=?2",
            rusqlite::params![TEAM, stage],
        )
        .unwrap();
    assert!(f
        .h
        .run(
            &person(),
            Operation::AcceptPhase {
                phase: 1,
                publication: p.clone()
            }
        )
        .is_err());
    f.h.store
        .db
        .as_ref()
        .unwrap()
        .execute(
            "UPDATE stage_tasks SET state='superseded' WHERE team=?1 AND node=?2",
            rusqlite::params![TEAM, stage],
        )
        .unwrap();
    f.h.run(
        &person(),
        Operation::AcceptPhase {
            phase: 1,
            publication: p,
        },
    )
    .unwrap();
}

#[test]
fn a_completed_tasks_old_hold_does_not_trap_phase_acceptance() {
    let (mut f, p, requests) = fixture();
    let mut settings = f.h.store.team(TEAM).unwrap().unwrap().settings;
    settings.integration = zerus_team_plan::Integration::None;
    f.h.run(&person(), Operation::ChangeSettings { settings })
        .unwrap();
    for r in &requests {
        write(&f, r).unwrap();
    }
    f.h.run(
        &ops::tests::service(),
        Operation::ReconcilePublication {
            phase: 1,
            publication: p.clone(),
        },
    )
    .unwrap();
    f.h.run(
        &person(),
        Operation::Hold {
            node: requests[0].node.id,
        },
    )
    .unwrap();
    for r in &requests {
        ended(&f, r, Stage::Done);
    }
    assert!(
        f.h.store
            .node(TEAM, requests[0].node.id)
            .unwrap()
            .unwrap()
            .hold
    );
    f.h.run(
        &ops::tests::service(),
        Operation::ReconcilePhase {
            phase: 1,
            publication: p.clone(),
        },
    )
    .unwrap();
    f.h.run(
        &person(),
        Operation::AcceptPhase {
            phase: 1,
            publication: p,
        },
    )
    .unwrap();
}

#[test]
fn recovery_keeps_the_original_request_and_lead_after_the_lead_is_rebound() {
    let (mut f, _, requests) = fixture();
    let r = &requests[0];
    let mut child = paused(&f, r, "publication_saved");
    child.child.kill().unwrap();
    child.child.wait().unwrap();
    drop(child);
    f.h.run(
        &ops::tests::service(),
        Operation::UnbindMember {
            member: "lead".into(),
        },
    )
    .unwrap();
    let home = f.lead.machine.clone();
    let replacement = f.bind("lead", &home);
    assert_ne!(replacement.session_id, r.lead.session_id);
    assert_eq!(write(&f, r).unwrap(), "created");
    assert_eq!(
        f.h.store
            .planned_task(TEAM, r.node.id, &r.task.uuid)
            .unwrap()
            .as_ref(),
        Some(r)
    );
    let tasks = crate::tasks::store(&f.config, &r.task.project).unwrap();
    let doc = tasks.load(&r.task.uuid).unwrap();
    let view = doc.view().unwrap();
    assert_eq!(view.author, Actor::Agent(f.lead.clone()));
    assert_eq!(view.text, r.text);
}

pub(super) fn ended(
    f: &Fixture,
    r: &crate::teams::store::publication::PlannedTaskRequest,
    stage: Stage,
) {
    let tasks = crate::tasks::store(&f.config, &r.task.project).unwrap();
    let mut doc = tasks.load(&r.task.uuid).unwrap();
    if stage == Stage::Done {
        let executor = Actor::Agent(f.lead.clone());
        doc.record_start(&executor, now()).unwrap();
        assert!(doc
            .record_report("The fixture completed the planned work.", &executor, now())
            .unwrap());
    }
    tasks
        .change_stage(&mut doc, stage, &Actor::Person, now())
        .unwrap();
    tasks.save(&mut doc).unwrap();
    drop(tasks);
    crate::teams::tasks::observe(&f.config.state, &r.task).unwrap();
}

#[test]
fn archived_completion_gates_later_handouts_before_reconciliation_and_acceptance_rechecks_it() {
    let (mut f, p, requests) = fixture();
    let mut settings = f.h.store.team(TEAM).unwrap().unwrap().settings;
    settings.integration = zerus_team_plan::Integration::None;
    f.h.run(&person(), Operation::ChangeSettings { settings })
        .unwrap();
    for r in &requests {
        write(&f, r).unwrap();
    }
    f.h.run(
        &ops::tests::service(),
        Operation::ReconcilePublication {
            phase: 1,
            publication: p.clone(),
        },
    )
    .unwrap();
    for r in &requests {
        ended(&f, r, Stage::Done);
    }
    let scope = f.h.store.phase_draft(TEAM, 1).unwrap().live.unwrap();
    assert!(
        scope.complete
            && scope
                .nodes
                .iter()
                .all(|n| n.state == zerus_team_plan::NodeState::Accepted)
    );
    assert!(f
        .h
        .store
        .queue(TEAM, 200, None)
        .unwrap()
        .rows
        .iter()
        .all(|n| n.phase != 1));
    assert_eq!(
        f.h.store.phase_draft(TEAM, 1).unwrap().phase.state,
        zerus_team_plan::PhaseState::Approved
    );
    let id = f.h.store.next_draft_node(TEAM).unwrap();
    f.h.run(
        &person(),
        Operation::SubmitDraftPhase {
            phase: 2,
            draft: ops::publication::tests::input(&[id]),
        },
    )
    .unwrap();
    f.h.run(
        &person(),
        Operation::ApprovePhase {
            phase: 2,
            generation: 1,
            confirm_initial: false,
        },
    )
    .unwrap();
    for r in f.h.store.publication_work(TEAM).unwrap() {
        write(&f, &r).unwrap();
    }
    let p2 =
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
            publication: p2,
        },
    )
    .unwrap();
    let task = f.h.store.node(TEAM, id).unwrap().unwrap().task.unwrap();
    crate::teams::tasks::observe(&f.config.state, &task).unwrap();
    let home = f.lead.machine.clone();
    f.bind("m2", &home);
    let handout = || Operation::HandoutIntent {
        node: id,
        member: "m2".into(),
        effect_request_id: uuid::Uuid::new_v4().to_string(),
    };
    assert!(
        f.h.run(&ops::tests::service(), handout()).is_err(),
        "the fresh kernel must see completion before its fact is recorded"
    );
    f.h.run(
        &ops::tests::service(),
        Operation::ReconcilePhase {
            phase: 1,
            publication: p.clone(),
        },
    )
    .unwrap();
    let cards = f.h.store.waiting(200, None).unwrap().rows;
    assert!(cards.iter().any(|c| c.card.kind == "phase_acceptance"
        && f.h
            .store
            .phase_card_ref(TEAM, c.card.seq)
            .unwrap()
            .unwrap()
            .phase
            == 1));
    ended(&f, &requests[0], Stage::Todo);
    assert!(!f.h.store.phase_draft(TEAM, 1).unwrap().accept_available);
    assert!(f
        .h
        .run(
            &person(),
            Operation::AcceptPhase {
                phase: 1,
                publication: p.clone()
            }
        )
        .is_err());
    ended(&f, &requests[0], Stage::Done);
    assert!(f.h.store.phase_draft(TEAM, 1).unwrap().accept_available);
    f.h.run(
        &person(),
        Operation::AcceptPhase {
            phase: 1,
            publication: p,
        },
    )
    .unwrap();
    f.h.run(&ops::tests::service(), handout()).unwrap();
}

#[test]
fn an_edited_task_during_lost_answer_recovery_does_not_bind_the_changed_contents() {
    let (mut f, p, requests) = fixture();
    let r = &requests[0];
    let mut child = paused(&f, r, "publication_saved");
    child.child.kill().unwrap();
    child.child.wait().unwrap();
    drop(child);
    let tasks = crate::tasks::store(&f.config, &r.task.project).unwrap();
    let mut doc = tasks.load(&r.task.uuid).unwrap();
    tasks
        .edit_task(
            &mut doc,
            "A different intent",
            &Actor::Person,
            &r.task.project,
            now(),
        )
        .unwrap();
    tasks.save(&mut doc).unwrap();
    drop(tasks);
    assert!(write(&f, r)
        .unwrap_err()
        .message
        .contains("no longer match approval"));
    assert!(f
        .h
        .run(
            &ops::tests::service(),
            Operation::ReconcilePublication {
                phase: 1,
                publication: p
            }
        )
        .is_err());
    assert!(f.h.store.node(TEAM, r.node.id).unwrap().is_none());
}
