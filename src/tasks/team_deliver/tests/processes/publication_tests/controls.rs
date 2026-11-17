//! Actual named task creation and task-document observations prove that a
//! published phase can be positively unstarted. No normal node is hand-written.
use super::*;
use crate::teams::store::phase_controls as facts;
use zerus_team_plan::{phase_control::*, Acceptance, PhaseState};
mod admission;
mod continuation;
mod order;

fn published() -> (
    Fixture,
    String,
    Vec<crate::teams::store::publication::PlannedTaskRequest>,
) {
    let (mut f, p, requests) = fixture();
    for r in &requests {
        assert_eq!(write(&f, r).unwrap(), "created");
    }
    f.h.run(
        &ops::tests::service(),
        Operation::ReconcilePublication {
            phase: 1,
            publication: p.clone(),
        },
    )
    .unwrap();
    (f, p, requests)
}
fn scope(f: &Fixture, n: u32) -> PhaseControlScope {
    f.h.store
        .phase_draft(TEAM, n)
        .unwrap()
        .controls
        .unwrap()
        .scope
}

fn another_published(f: &mut Fixture) -> Vec<crate::teams::store::publication::PlannedTaskRequest> {
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
    let requests = f.h.store.publication_work(TEAM).unwrap();
    assert!(!requests.is_empty() && requests.iter().all(|r| r.phase == 2));
    for r in &requests {
        assert_eq!(write(f, r).unwrap(), "created");
    }
    f.h.run(
        &ops::tests::service(),
        Operation::ReconcilePublication {
            phase: 2,
            publication: requests[0].publication.clone(),
        },
    )
    .unwrap();
    for r in &requests {
        crate::teams::tasks::observe(&f.config.state, &r.task).unwrap();
    }
    requests
}

#[test]
fn approved_unstarted_phase_can_move_and_change_acceptance_after_real_todo_observation() {
    let (mut f, _, requests) = published();
    assert!(
        !f.h.store
            .phase_draft(TEAM, 1)
            .unwrap()
            .controls
            .unwrap()
            .unstarted
            .allowed,
        "unobserved is unknown, never positively free"
    );
    for r in &requests {
        crate::teams::tasks::observe(&f.config.state, &r.task).unwrap();
    }
    let d = f.h.store.phase_draft(TEAM, 1).unwrap();
    assert_eq!(d.phase.state, PhaseState::Approved);
    assert!(d.controls.unwrap().unstarted.allowed);
    let next = f.h.store.next_draft_node(TEAM).unwrap();
    f.h.run(
        &person(),
        Operation::SubmitDraftPhase {
            phase: 2,
            draft: ops::publication::tests::input(&[next]),
        },
    )
    .unwrap();
    let s = scope(&f, 1);
    let target = scope(&f, 2);
    f.h.run(
        &person(),
        Operation::MovePhase {
            scope: s,
            relative_to: target,
            placement: PhasePlacement::After,
        },
    )
    .unwrap();
    let s = scope(&f, 1);
    f.h.run(
        &person(),
        Operation::SetPhaseAcceptance {
            scope: s,
            acceptance: Acceptance::Person,
        },
    )
    .unwrap();
    let d = f.h.store.phase_draft(TEAM, 1).unwrap();
    assert_eq!(d.phase.acceptance, Acceptance::Person);
    assert_eq!(d.phase.order, 2);
    assert!(d.controls.unwrap().unstarted.allowed);
    for r in &requests {
        let task = crate::tasks::store(&f.config, &r.task.project)
            .unwrap()
            .load(&r.task.uuid)
            .unwrap();
        assert_eq!(task.view().unwrap().stage, Stage::Todo);
        assert!(task.attempt_phases().unwrap().is_empty());
    }
}

#[test]
fn a_refused_handout_retains_start_history_and_pause_blocks_new_ordinary_intents() {
    let (mut f, _, requests) = published();
    for r in &requests {
        crate::teams::tasks::observe(&f.config.state, &r.task).unwrap();
    }
    let home = f.lead.machine.clone();
    f.bind("m2", &home);
    let node = requests[0].node.id;
    let before = f.h.store.phase_draft(TEAM, 1).unwrap().controls.unwrap();
    assert!(before.unstarted.allowed);
    let pause = ops::tests::request(
        f.h.store.team(TEAM).unwrap().unwrap().revision,
        Operation::PausePhase {
            scope: before.scope,
            pause: None,
            reason: "Check the original task scope.".into(),
        },
    );
    ops::tests::apply_fixture(&mut f.h.store, &person(), &pause, now()).unwrap();
    ops::tests::refused(
        f.h.run(
            &ops::tests::service(),
            Operation::HandoutIntent {
                node,
                member: "m2".into(),
                effect_request_id: uuid::Uuid::new_v4().to_string(),
            },
        ),
        zerus_team_plan::problem::code::INVALID_STATE,
    );
    let db = f.h.store.db.as_ref().unwrap();
    let count: u32 = db
        .query_row(
            "SELECT count(*) FROM effects WHERE team=?1 AND node=?2",
            rusqlite::params![TEAM, node],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
    let s = scope(&f, 1);
    f.h.run(
        &person(),
        Operation::ResumePhase {
            scope: s,
            pause: Some(pause.request_id),
        },
    )
    .unwrap();
    let id = f.h.intent(node, "m2");
    f.h.run(
        &ops::tests::service(),
        Operation::HandoutOutcome {
            effect_request_id: id,
            outcome: ops::HandoutOutcome::Refused {
                reason: "Fixture refused before native input.".into(),
            },
        },
    )
    .unwrap();
    let d = f.h.store.phase_draft(TEAM, 1).unwrap();
    assert!(!d.controls.unwrap().unstarted.allowed);
    let s = scope(&f, 1);
    ops::tests::refused(
        f.h.run(
            &person(),
            Operation::SetPhaseAcceptance {
                scope: s,
                acceptance: Acceptance::Person,
            },
        ),
        zerus_team_plan::problem::code::INVALID_STATE,
    );
    let p = facts::phase(f.h.store.db.as_ref().unwrap(), TEAM, 1).unwrap();
    assert_eq!(p.acceptance, Acceptance::Lead);
}

#[test]
fn new_engine_ordinary_admission_refuses_a_damaged_pause_instead_of_reading_it_free() {
    let (mut f, _, requests) = published();
    for r in &requests {
        crate::teams::tasks::observe(&f.config.state, &r.task).unwrap();
    }
    let home = f.lead.machine.clone();
    f.bind("m2", &home);
    let s = scope(&f, 1);
    f.h.run(
        &person(),
        Operation::PausePhase {
            scope: s,
            pause: None,
            reason: "Keep the current owner and pause together.".into(),
        },
    )
    .unwrap();
    // Deliberate corruption, not an old-engine compatibility claim. The real
    // PausePhase writer never commits this disagreement.
    let db = f.h.store.db.as_ref().unwrap();
    let mut p = facts::phase(db, TEAM, 1).unwrap();
    p.paused = false;
    db.execute(
        "UPDATE phases SET data=?2 WHERE team=?1 AND n=1",
        rusqlite::params![TEAM, crate::teams::store::encode(&p).unwrap()],
    )
    .unwrap();
    assert_eq!(
        ops::readiness(&f.h.store, TEAM).unwrap_err().code,
        zerus_team_plan::problem::code::STORAGE_FAILED
    );
    let node = requests[0].node.id;
    ops::tests::refused(
        f.h.run(
            &ops::tests::service(),
            Operation::HandoutIntent {
                node,
                member: "m2".into(),
                effect_request_id: uuid::Uuid::new_v4().to_string(),
            },
        ),
        zerus_team_plan::problem::code::STORAGE_FAILED,
    );
    assert_eq!(
        ops::handout_admissible(
            f.h.store.db.as_ref().unwrap(),
            &f.h.store.team(TEAM).unwrap().unwrap(),
            node,
            "m2"
        )
        .unwrap_err()
        .code,
        zerus_team_plan::problem::code::STORAGE_FAILED
    );
    let count: u32 =
        f.h.store
            .db
            .as_ref()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM effects WHERE team=?1 AND node=?2",
                rusqlite::params![TEAM, node],
                |r| r.get(0),
            )
            .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn a_real_person_send_reservation_cannot_be_hidden_by_a_cached_todo_observation() {
    let (mut f, _, requests) = published();
    for r in &requests {
        crate::teams::tasks::observe(&f.config.state, &r.task).unwrap();
    }
    let s = scope(&f, 1);
    f.h.run(
        &person(),
        Operation::SetPhaseAcceptance {
            scope: s,
            acceptance: Acceptance::Person,
        },
    )
    .unwrap();
    let r = &requests[0];
    let tasks = crate::tasks::store(&f.config, &r.task.project).unwrap();
    let mut doc = tasks.load(&r.task.uuid).unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    // The actual task-send guard permits Person (tasks/policy.rs); this is
    // the same document reservation writer before native input, not a SQL
    // fault or an agent's forbidden direct-start path. No input is offered.
    doc.reserve_attempt(
        &id,
        zerus_replica::execution::Origin::Send,
        &f.lead,
        zerus_replica::execution::NativeRequest {
            machine: f.lead.machine.clone(),
            name: f.lead.session.clone(),
            run_id: "example-person-send".into(),
            conversation_id: String::new(),
            digest: "0".repeat(64),
        },
        &zerus_replica::Actor::Person,
        now(),
    )
    .unwrap();
    tasks.save(&mut doc).unwrap();
    assert!(doc.held().unwrap().is_some());
    drop(tasks);
    assert!(
        !f.h.store
            .phase_draft(TEAM, 1)
            .unwrap()
            .controls
            .unwrap()
            .unstarted
            .allowed,
        "the read must not offer a cached Todo as positively unstarted"
    );
    // Deliberately do not invoke the later service observation. The real
    // task writer's durable reservation already excludes unstarted.
    let s = scope(&f, 1);
    ops::tests::refused(
        f.h.run(
            &person(),
            Operation::SetPhaseAcceptance {
                scope: s,
                acceptance: Acceptance::Lead,
            },
        ),
        zerus_team_plan::problem::code::INVALID_STATE,
    );
    assert_eq!(
        f.h.store.phase_draft(TEAM, 1).unwrap().phase.acceptance,
        Acceptance::Person
    );
}

#[test]
fn retained_project_custody_excludes_a_send_until_the_control_writer_commits() {
    use std::time::{Duration, Instant};
    use zerus_replica::ProjectStore;
    let (mut f, _, requests) = published();
    for r in &requests {
        crate::teams::tasks::observe(&f.config.state, &r.task).unwrap();
    }
    let request = ops::tests::request(
        f.h.store.team(TEAM).unwrap().unwrap().revision,
        Operation::SetPhaseAcceptance {
            scope: scope(&f, 1),
            acceptance: Acceptance::Person,
        },
    );
    let mut worker = crate::teams::store::Store::open(&f.config.state).unwrap();
    let blocker =
        rusqlite::Connection::open(f.config.state.join("teams/teams-v1.sqlite3")).unwrap();
    blocker.execute_batch("BEGIN IMMEDIATE").unwrap();
    let root = f.config.state.join("replica");
    let project = &requests[0].task.project;
    std::thread::scope(|s| {
        let request = &request;
        let join =
            s.spawn(move || ops::tests::apply_fixture(&mut worker, &person(), request, now()));
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            // The private fixture deliberately competes for its own lock.
            // None proves the new writer acquired project custody before
            // waiting on the Teams writer already held by this test.
            if ProjectStore::open_copy_review(&root, project, Duration::from_millis(10))
                .unwrap()
                .is_none()
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "the control never acquired task custody"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            ProjectStore::open_copy_review(&root, project, Duration::from_millis(20))
                .unwrap()
                .is_none()
        );
        blocker.execute_batch("ROLLBACK").unwrap();
        join.join().unwrap().unwrap();
    });
    let tasks = crate::tasks::store(&f.config, project).unwrap();
    let doc = tasks.load(&requests[0].task.uuid).unwrap();
    assert!(doc.attempt_phases().unwrap().is_empty());
    assert_eq!(
        facts::phase(f.h.store.db.as_ref().unwrap(), TEAM, 1)
            .unwrap()
            .acceptance,
        Acceptance::Person
    );
    // A known original remains replayable while its external task resources
    // are busy; this is receipt replay, never renewed control authority.
    let saved = ops::tests::apply_fixture(&mut f.h.store, &person(), &request, now()).unwrap();
    assert_eq!(saved.revision, request.expected_revision + 1);
}

#[test]
fn missing_task_copy_is_unavailable_and_a_saved_refusal_does_not_retry_after_restore() {
    let (mut f, _, requests) = published();
    for r in &requests {
        crate::teams::tasks::observe(&f.config.state, &r.task).unwrap();
    }
    let request = ops::tests::request(
        f.h.store.team(TEAM).unwrap().unwrap().revision,
        Operation::SetPhaseAcceptance {
            scope: scope(&f, 1),
            acceptance: Acceptance::Person,
        },
    );
    let root = f.config.state.join("replica");
    let held = f.config.state.join("retained-private-task-copy");
    // Explicit missing-copy fault; all normal rows/documents came from writers.
    std::fs::rename(&root, &held).unwrap();
    let before = f.h.store.team(TEAM).unwrap().unwrap().revision;
    let first = ops::tests::apply_fixture(&mut f.h.store, &person(), &request, now()).unwrap_err();
    assert_eq!(first.code, zerus_team_plan::problem::code::INVALID_STATE);
    assert!(
        !root.exists(),
        "inspection must never create a missing store"
    );
    assert!(
        !f.h.store
            .phase_draft(TEAM, 1)
            .unwrap()
            .controls
            .unwrap()
            .acceptance
            .allowed
    );
    assert!(!root.exists());
    std::fs::rename(&held, &root).unwrap();
    assert_eq!(
        ops::tests::apply_fixture(&mut f.h.store, &person(), &request, now()).unwrap_err(),
        first
    );
    assert_eq!(f.h.store.team(TEAM).unwrap().unwrap().revision, before);
    assert_eq!(
        facts::phase(f.h.store.db.as_ref().unwrap(), TEAM, 1)
            .unwrap()
            .acceptance,
        Acceptance::Lead
    );
}

#[test]
fn optional_control_proof_refuses_oversized_files_and_never_creates_a_missing_replica_identity() {
    use facts::task_custody::{Selection, TaskCustody};
    let (f, _, requests) = published();
    for r in &requests {
        crate::teams::tasks::observe(&f.config.state, &r.task).unwrap();
    }
    let db = f.h.store.db.as_ref().unwrap();
    let root = f.config.state.join("replica");
    let store = crate::tasks::store(&f.config, &requests[0].task.project).unwrap();
    let file = store
        .dir()
        .join("tasks")
        .join(format!("{}.loro", requests[0].task.uuid));
    drop(store);
    let original = file.with_extension("retained-test-snapshot");
    // Deliberate sparse-file fault, not a manually authored normal document.
    std::fs::rename(&file, &original).unwrap();
    std::fs::File::create(&file)
        .unwrap()
        .set_len(17 * 1024 * 1024)
        .unwrap();
    let error = match TaskCustody::acquire(db, &f.config.state, TEAM, Selection::One(1)) {
        Ok(_) => panic!("an oversized control scope was read"),
        Err(error) => error,
    };
    assert_eq!(error.code, zerus_team_plan::problem::code::OVER_CAP);
    std::fs::remove_file(&file).unwrap();
    std::fs::rename(&original, &file).unwrap();
    let identity = root.join("store-id");
    let retained = root.join("retained-test-store-id");
    std::fs::rename(&identity, &retained).unwrap();
    assert!(TaskCustody::acquire(db, &f.config.state, TEAM, Selection::One(1)).is_err());
    assert!(
        !identity.exists(),
        "a read must not initialize a replacement replica identity"
    );
    std::fs::rename(&retained, &identity).unwrap();
    assert!(
        f.h.store
            .phase_draft(TEAM, 1)
            .unwrap()
            .controls
            .unwrap()
            .unstarted
            .allowed
    );
}

#[test]
fn auto_approval_metadata_never_acquires_neighbor_task_custody_or_invents_free_facts() {
    let (mut f, _, requests) = published();
    for r in &requests {
        crate::teams::tasks::observe(&f.config.state, &r.task).unwrap();
    }
    let next = f.h.store.next_draft_node(TEAM).unwrap();
    f.h.run(
        &person(),
        Operation::SubmitDraftPhase {
            phase: 2,
            draft: ops::publication::tests::input(&[next]),
        },
    )
    .unwrap();
    let before = facts::task_custody::fixture::reads();
    let metadata = f.h.store.phase_draft_for_auto(TEAM, 2).unwrap();
    assert_eq!(facts::task_custody::fixture::reads(), before);
    assert_eq!(metadata.generation, Some(1));
    assert!(metadata.publication.is_none() && metadata.editable);
    let controls = metadata.controls.unwrap();
    assert!(!controls.unstarted.allowed && !controls.acceptance.allowed);
    let public = f.h.store.phase_draft(TEAM, 2).unwrap();
    assert_eq!(facts::task_custody::fixture::reads(), before + 1);
    assert!(public.controls.unwrap().unstarted.allowed);
}

#[test]
fn an_archived_accepted_task_keeps_the_approved_phase_unmovable_without_rewriting_history() {
    let (mut f, _, requests) = published();
    f.h.settings(|s| s.integration = zerus_team_plan::Integration::None);
    for r in &requests {
        crate::teams::tasks::observe(&f.config.state, &r.task).unwrap();
    }
    another_published(&mut f);
    super::completion::ended(&f, &requests[0], Stage::Done);
    let selected = f.h.store.phase_draft(TEAM, 1).unwrap();
    assert_eq!(selected.phase.state, PhaseState::Approved);
    assert!(selected.live.as_ref().unwrap().complete);
    assert!(selected.live.as_ref().unwrap().nodes.iter().any(|n| {
        n.id == requests[0].node.id && n.state == zerus_team_plan::NodeState::Accepted
    }));
    let archived: bool = f.h.store.db.as_ref().unwrap().query_row(
        "SELECT EXISTS(SELECT 1 FROM node_archive WHERE team=?1 AND id=?2) AND NOT EXISTS(SELECT 1 FROM nodes WHERE team=?1 AND id=?2)",
        rusqlite::params![TEAM, requests[0].node.id], |row| row.get(0),
    ).unwrap();
    assert!(
        archived,
        "the product writer must retain only the archival row"
    );
    assert!(!selected.controls.unwrap().unstarted.allowed);
    let task_revision = {
        let store = crate::tasks::store(&f.config, &requests[0].task.project).unwrap();
        store.load(&requests[0].task.uuid).unwrap().revision()
    };
    let old = facts::phase(f.h.store.db.as_ref().unwrap(), TEAM, 1).unwrap();
    let current = scope(&f, 2);
    let relative_to = scope(&f, 1);
    ops::tests::refused(
        f.h.run(
            &person(),
            Operation::MovePhase {
                scope: current,
                relative_to,
                placement: PhasePlacement::Before,
            },
        ),
        zerus_team_plan::problem::code::INVALID_STATE,
    );
    let current = scope(&f, 1);
    ops::tests::refused(
        f.h.run(
            &person(),
            Operation::SetPhaseAcceptance {
                scope: current,
                acceptance: Acceptance::Person,
            },
        ),
        zerus_team_plan::problem::code::INVALID_STATE,
    );
    assert_eq!(
        facts::phase(f.h.store.db.as_ref().unwrap(), TEAM, 1).unwrap(),
        old
    );
    let store = crate::tasks::store(&f.config, &requests[0].task.project).unwrap();
    assert_eq!(
        store.load(&requests[0].task.uuid).unwrap().revision(),
        task_revision
    );
}

#[test]
fn real_reported_pipeline_history_is_not_an_unstarted_phase() {
    // The existing fixture feeds native report observations to the real
    // service writers; it runs no native agent or hand-written SQL row.
    let (mut f, _, _, stage) = super::completion::reported_stage();
    let phase = f.h.store.phase_draft(TEAM, 1).unwrap();
    assert!(phase.live.as_ref().unwrap().complete);
    assert!(phase
        .live
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .any(|n| { n.id == stage && n.kind == zerus_team_plan::NodeKind::Stage }));
    assert!(!phase.controls.unwrap().unstarted.allowed);
    let current = scope(&f, 1);
    ops::tests::refused(
        f.h.run(
            &person(),
            Operation::SetPhaseAcceptance {
                scope: current,
                acceptance: Acceptance::Person,
            },
        ),
        zerus_team_plan::problem::code::INVALID_STATE,
    );
}
