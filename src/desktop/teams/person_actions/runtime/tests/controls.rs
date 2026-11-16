//! Real producers and the admitted review/original writer path. Validated
//! explicitly models native consent; no physical GUI gesture is claimed.
use super::*;
use crate::teams::ops::tests::{person, service};
use store::phase_controls::task_custody;
use zerus_team_plan::phase_control::{PhaseControlScope, PhasePause};
use zerus_team_plan::{Acceptance, PhaseState};

fn local_sample() -> (Harness, Runtime, Arc<wire::Plan>, Arc<platform::FileLock>) {
    let (h, runtime, plan, owner) = super::sample();
    // All publication writers require this fixture's actual team home. The
    // parent rename-only fixture uses a separate synthetic node.
    crate::swarm::fixture_project(
        &runtime.config,
        &h.store.team(TEAM).unwrap().unwrap().home,
        "example-project",
    );
    (h, runtime, plan, owner)
}

fn draft(h: &mut Harness, n: u32) {
    let id = h.store.next_draft_node(TEAM).unwrap();
    h.run(
        &person(),
        ops::Operation::SubmitDraftPhase {
            phase: n,
            draft: ops::publication::tests::input(&[id]),
        },
    )
    .unwrap();
}
fn scope(h: &Harness, n: u32) -> PhaseControlScope {
    h.store
        .phase_draft(TEAM, n)
        .unwrap()
        .controls
        .unwrap()
        .scope
}
fn plan(h: &Harness, action: TeamAction) -> Arc<wire::Plan> {
    Arc::new(wire::Plan {
        request_id: uuid::Uuid::new_v4().to_string(),
        team: TEAM.into(),
        expected_revision: h.store.team(TEAM).unwrap().unwrap().revision,
        action: wire::Action::Team {
            change: Box::new(action),
        },
    })
}
fn prepare(runtime: &mut Runtime, plan: &wire::Plan) -> wire::Review {
    stage(runtime, plan);
    let queue = Arc::new(Queue::default());
    runtime.query(&json!({"op":"team_person_action_prepare","scope":runtime.scope,"plan":plan.reference()}),queue.clone(),1).unwrap();
    let value = take(&queue)["value"].clone();
    assert_eq!(value["ok"], true, "{value}");
    serde_json::from_value(value["review"].clone()).unwrap()
}
fn execute(runtime: &mut Runtime, review: &wire::Review) -> Value {
    let queue = Arc::new(Queue::default());
    runtime
        .action(
            Validated::fixture(&json!({"op":"team_person_action","intent":review.intent})),
            queue.clone(),
            2,
        )
        .unwrap();
    take(&queue)["value"].clone()
}
fn published(h: &mut Harness, runtime: &Runtime) -> store::publication::PlannedTaskRequest {
    h.bind("lead");
    h.settings(|s| s.integration = zerus_team_plan::Integration::None);
    let tasks =
        zerus_replica::ProjectStore::open(&runtime.config.state.join("replica"), "example-project")
            .unwrap();
    tasks
        .save_project(&zerus_replica::ProjectDoc::new())
        .unwrap();
    drop(tasks);
    draft(h, 1);
    h.run(
        &person(),
        ops::Operation::ApprovePhase {
            phase: 1,
            generation: 1,
            confirm_initial: false,
        },
    )
    .unwrap();
    let request = h.store.publication_work(TEAM).unwrap().remove(0);
    assert_eq!(
        crate::tasks::create_planned_task_for_test(
            &runtime.config,
            TEAM,
            request.node.id,
            &request.task.uuid,
            false
        )
        .unwrap(),
        "created"
    );
    h.run(
        &service(),
        ops::Operation::ReconcilePublication {
            phase: 1,
            publication: request.publication.clone(),
        },
    )
    .unwrap();
    crate::teams::tasks::observe(&runtime.config.state, &request.task).unwrap();
    request
}

#[test]
fn all_five_s5_actions_use_review_original_and_only_read_saved_results() {
    let (mut h, mut runtime, _, _owner) = local_sample();
    draft(&mut h, 1);
    draft(&mut h, 2);
    let actions = ["barrier", "pause", "resume", "move", "acceptance"];
    let mut first = None;
    for kind in actions {
        let current = h.store.phase_draft(TEAM, 1).unwrap();
        let current_scope = current.controls.as_ref().unwrap().scope.clone();
        let action = match kind {
            "barrier" => TeamAction::SetPhaseBarrier {
                scope: current_scope,
                barrier: !current.phase.barrier,
            },
            "pause" => TeamAction::PausePhase {
                scope: current_scope,
                pause: None,
                reason: "Review the exact native phase.".into(),
            },
            "resume" => {
                let PhasePause::Paused { episode: Some(e) } = current.controls.unwrap().pause
                else {
                    panic!("attributed pause")
                };
                TeamAction::ResumePhase {
                    scope: current_scope,
                    pause: Some(e.id),
                }
            }
            "move" => TeamAction::MovePhase {
                scope: scope(&h, 2),
                relative_to: current_scope,
                placement: zerus_team_plan::phase_control::PhasePlacement::Before,
            },
            _ => TeamAction::SetPhaseAcceptance {
                scope: current_scope,
                acceptance: Acceptance::Person,
            },
        };
        let plan = plan(&h, action);
        let review = prepare(&mut runtime, &plan);
        let response = execute(&mut runtime, &review);
        assert_eq!(response["reply"]["ok"], true, "{kind}: {response}");
        assert_eq!(response["reply"]["revision"], plan.expected_revision + 1);
        assert!(runtime.plans.get(&review.intent.plan).is_err());
        first.get_or_insert((review, response));
    }
    let (review, original) = first.unwrap();
    let revision = h.store.team(TEAM).unwrap().unwrap().revision;
    runtime.scope.service_epoch = uuid::Uuid::new_v4().to_string();
    let queue = Arc::new(Queue::default());
    runtime
        .query(
            &json!({"op":"team_person_action_result","intent":review.intent}),
            queue.clone(),
            3,
        )
        .unwrap();
    assert_eq!(take(&queue)["value"]["reply"], original["reply"]);
    assert_eq!(h.store.team(TEAM).unwrap().unwrap().revision, revision);
    assert_eq!(
        h.store.phase_draft(TEAM, 1).unwrap().phase.acceptance,
        Acceptance::Person
    );
}

#[test]
fn native_s5_preparation_rejects_wrong_nested_relative_generation_and_pause() {
    let (mut h, runtime, _, _owner) = local_sample();
    draft(&mut h, 1);
    draft(&mut h, 2);
    let mut wrong = scope(&h, 1);
    let PhaseControlScope::Draft { generation, .. } = &mut wrong else {
        unreachable!()
    };
    *generation += 1;
    let proposed = plan(
        &h,
        TeamAction::MovePhase {
            scope: scope(&h, 2),
            relative_to: wrong,
            placement: zerus_team_plan::phase_control::PhasePlacement::Before,
        },
    );
    let revision = h.store.team(TEAM).unwrap().unwrap().revision;
    assert_eq!(
        review(&runtime.config, &runtime.scope, proposed)
            .unwrap_err()
            .code,
        code::STALE_REVISION
    );
    h.run(
        &person(),
        ops::Operation::PausePhase {
            scope: scope(&h, 1),
            pause: None,
            reason: "Keep the native owner binding.".into(),
        },
    )
    .unwrap();
    let proposed = plan(
        &h,
        TeamAction::ResumePhase {
            scope: scope(&h, 1),
            pause: Some(uuid::Uuid::new_v4().to_string()),
        },
    );
    assert_eq!(
        review(&runtime.config, &runtime.scope, proposed)
            .unwrap_err()
            .code,
        code::STALE_REVISION
    );
    assert_eq!(h.store.team(TEAM).unwrap().unwrap().revision, revision + 1);
    assert!(h.store.phase_draft(TEAM, 1).unwrap().phase.paused);
}

#[test]
fn native_s5_preconsent_resource_proof_must_cover_the_exact_selected_interval() {
    use task_custody::{Selection, TaskCustody};
    let (mut h, runtime, _, _owner) = local_sample();
    draft(&mut h, 1);
    draft(&mut h, 2);
    let wanted = plan(
        &h,
        TeamAction::SetPhaseAcceptance {
            scope: scope(&h, 1),
            acceptance: Acceptance::Person,
        },
    );
    let prepared = Prepared::resolve(&runtime.config, wanted).unwrap();
    let db = h.store.db.as_ref().unwrap();
    for selection in [Selection::One(2), Selection::Interval(1, 2)] {
        let wrong = Ok(TaskCustody::acquire(db, &runtime.config.state, TEAM, selection).unwrap());
        let tx = db.unchecked_transaction().unwrap();
        assert!(prepared
            .bound_facts_with_phase_resources(&tx, Some(&runtime.config), Some(&wrong))
            .is_err());
    }
    let exact =
        Ok(TaskCustody::acquire(db, &runtime.config.state, TEAM, Selection::One(1)).unwrap());
    let tx = db.unchecked_transaction().unwrap();
    assert!(prepared
        .bound_facts_with_phase_resources(&tx, Some(&runtime.config), Some(&exact))
        .is_ok());
}

#[test]
fn native_s5_real_reservation_after_consent_refuses_cached_todo_and_new_consent() {
    let (mut h, mut runtime, _, _owner) = local_sample();
    let request = published(&mut h, &runtime);
    let proposed = plan(
        &h,
        TeamAction::SetPhaseAcceptance {
            scope: scope(&h, 1),
            acceptance: Acceptance::Person,
        },
    );
    let prepared = prepare(&mut runtime, &proposed);
    let revision = h.store.team(TEAM).unwrap().unwrap().revision;
    let tasks = zerus_replica::ProjectStore::open(
        &runtime.config.state.join("replica"),
        &request.task.project,
    )
    .unwrap();
    let mut doc = tasks.load(&request.task.uuid).unwrap();
    let executor = zerus_replica::SessionKey {
        machine: request.lead.machine.clone(),
        session: "codex/example/native-control-lead".into(),
        session_id: request.lead.session_id.clone(),
    };
    doc.reserve_attempt(
        &uuid::Uuid::new_v4().to_string(),
        zerus_replica::execution::Origin::Send,
        &executor,
        zerus_replica::execution::NativeRequest {
            machine: executor.machine.clone(),
            name: executor.session.clone(),
            run_id: "private-native-s5-reservation".into(),
            conversation_id: String::new(),
            digest: "0".repeat(64),
        },
        &zerus_replica::Actor::Person,
        1234,
    )
    .unwrap();
    tasks.save(&mut doc).unwrap();
    drop(tasks);
    let response = execute(&mut runtime, &prepared);
    assert_eq!(response["reply"]["ok"], false, "{response}");
    assert_eq!(h.store.team(TEAM).unwrap().unwrap().revision, revision);
    assert_eq!(
        h.store.phase_draft(TEAM, 1).unwrap().phase.acceptance,
        Acceptance::Lead
    );
    let next = plan(
        &h,
        TeamAction::SetPhaseAcceptance {
            scope: scope(&h, 1),
            acceptance: Acceptance::Person,
        },
    );
    assert!(review(&runtime.config, &runtime.scope, next).is_err());
}

#[test]
fn native_s5_writer_reuses_one_project_custody_and_saved_check_needs_no_task_copy() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let (mut h, mut runtime, _, _owner) = local_sample();
    let task = published(&mut h, &runtime);
    let proposed = plan(
        &h,
        TeamAction::SetPhaseAcceptance {
            scope: scope(&h, 1),
            acceptance: Acceptance::Person,
        },
    );
    let reviewed = prepare(&mut runtime, &proposed);
    let prepared = Arc::new(Prepared::resolve(&runtime.config, proposed).unwrap());
    let state = runtime.config.state.clone();
    let project = task.task.project.clone();
    let checked = Arc::new(AtomicUsize::new(0));
    let observed = checked.clone();
    h.store.db.as_ref().unwrap().authorizer(Some(
        move |context: rusqlite::hooks::AuthContext<'_>| {
            if let rusqlite::hooks::AuthAction::Update {
                table_name: "phases",
                ..
            } = context.action
            {
                assert!(zerus_replica::ProjectStore::open_copy_review(
                    &state.join("replica"),
                    &project,
                    std::time::Duration::ZERO
                )
                .unwrap()
                .is_none());
                observed.fetch_add(1, Ordering::Relaxed);
            }
            rusqlite::hooks::Authorization::Allow
        },
    ));
    let before = task_custody::fixture::reads();
    let admission = Admission::new(
        Validated::fixture(&json!({"op":"team_person_action","intent":reviewed.intent})),
        prepared.clone(),
        runtime.config.clone(),
        runtime.scope.clone(),
        runtime.stopped.clone(),
    );
    let receipt = ops::apply_with_native_context(
        &mut h.store,
        &prepared::actor(),
        &prepared.request,
        1234,
        Some(&runtime.config),
        &|_| Err(required()),
        admission.into(),
    )
    .unwrap();
    assert_eq!(
        task_custody::fixture::reads(),
        before + 1,
        "native fresh must reuse the common writer's resource proof"
    );
    assert!(checked.load(Ordering::Relaxed) > 0);
    h.store
        .db
        .as_ref()
        .unwrap()
        .authorizer(None::<fn(rusqlite::hooks::AuthContext<'_>) -> rusqlite::hooks::Authorization>);
    let replica = runtime.config.state.join("replica");
    let retained = runtime.config.state.join("retained-native-s5-replica");
    std::fs::rename(&replica, &retained).unwrap();
    let queue = Arc::new(Queue::default());
    runtime
        .query(
            &json!({"op":"team_person_action_result","intent":reviewed.intent}),
            queue.clone(),
            3,
        )
        .unwrap();
    let result = take(&queue)["value"].clone();
    assert_eq!(result["reply"]["revision"], receipt.revision, "{result}");
    assert_eq!(result["reply"]["ok"], true);
    std::fs::rename(retained, replica).unwrap();
}

#[test]
fn native_s5_correction_binds_base_episode_generation_and_reserved_new_separately() {
    let (mut h, runtime, _, _owner) = local_sample();
    let task = published(&mut h, &runtime);
    {
        let tasks = zerus_replica::ProjectStore::open(
            &runtime.config.state.join("replica"),
            &task.task.project,
        )
        .unwrap();
        let mut doc = tasks.load(&task.task.uuid).unwrap();
        let actor = zerus_replica::Actor::Agent(zerus_replica::SessionKey {
            machine: task.lead.machine.clone(),
            session: String::new(),
            session_id: task.lead.session_id.clone(),
        });
        doc.record_start(&actor, 1234).unwrap();
        doc.record_report("Completed the original phase.", &actor, 1234)
            .unwrap();
        tasks
            .change_stage(
                &mut doc,
                zerus_replica::Stage::Done,
                &zerus_replica::Actor::Person,
                1234,
            )
            .unwrap();
        tasks.save(&mut doc).unwrap();
    }
    crate::teams::tasks::observe(&runtime.config.state, &task.task).unwrap();
    h.run(
        &service(),
        ops::Operation::ReconcilePhase {
            phase: 1,
            publication: task.publication.clone(),
        },
    )
    .unwrap();
    assert_eq!(
        h.store.phase_draft(TEAM, 1).unwrap().phase.state,
        PhaseState::AwaitingAcceptance
    );
    let episode = crate::teams::ops::tests::request(
        h.store.team(TEAM).unwrap().unwrap().revision,
        ops::Operation::SendBackPhase {
            phase: 1,
            publication: task.publication.clone(),
            comment: "Add one explicit correction.".into(),
        },
    );
    crate::teams::ops::tests::apply_fixture(&mut h.store, &person(), &episode, 1234).unwrap();
    let id = h.store.next_draft_node(TEAM).unwrap();
    h.run(
        &person(),
        ops::Operation::SubmitPhaseCorrections {
            phase: 1,
            publication: task.publication.clone(),
            rework: episode.request_id.clone(),
            draft: ops::publication::tests::input(&[id]),
        },
    )
    .unwrap();
    h.run(
        &person(),
        ops::Operation::ApprovePhaseCorrections {
            phase: 1,
            publication: task.publication.clone(),
            rework: episode.request_id,
            generation: 2,
        },
    )
    .unwrap();
    let exact = scope(&h, 1);
    let expected = plan(
        &h,
        TeamAction::PausePhase {
            scope: exact.clone(),
            pause: None,
            reason: "Hold this exact correction publication.".into(),
        },
    );
    let response = review(&runtime.config, &runtime.scope, expected).unwrap();
    assert_eq!(response["ok"], true);
    let mut wrong = exact;
    let PhaseControlScope::Published {
        rework: Some(r), ..
    } = &mut wrong
    else {
        panic!("correction scope")
    };
    r.approval = Some(uuid::Uuid::new_v4().to_string());
    let refused = plan(
        &h,
        TeamAction::PausePhase {
            scope: wrong,
            pause: None,
            reason: "Never substitute the new publication.".into(),
        },
    );
    assert_eq!(
        review(&runtime.config, &runtime.scope, refused)
            .unwrap_err()
            .code,
        code::STALE_REVISION
    );
}
