use super::*;
use crate::teams::ops::{Operation, StageTaskOutcome};
pub(super) fn run() {
    delivery();
    finish();
    creation_and_clear();
    cancel::run();
    handover();
    empty_execution_handover();
    superseded();
    refusals::run();
}
fn delivery() {
    let mut f = Fixture::new();
    f.submitted();
    assert_eq!(f.inputs(), 1);
    let recovered = f.delivery(&f.id, "m1", true);
    assert_eq!(recovered["state"], "submitted");
    assert_eq!(f.inputs(), 1);
    f.reported();
    f.h.run(
        &person(),
        Operation::Return {
            node: f.node,
            reason: "Revise this work".into(),
        },
    )
    .unwrap();
    let id = f.h.reopen(f.node, "m1");
    let reopened = f.delivery(&id, "m1", false);
    assert_eq!(reopened["state"], "submitted");
    assert_eq!(f.inputs(), 2);
}
fn finish() {
    let mut f = Fixture::new();
    f.submitted();
    f.reported();
    f.h.run(&person(), Operation::Accept { node: f.node })
        .unwrap();
    let decision = f.h.accept_decision(f.node);
    f.h.run(
        &service(),
        Operation::RecordIntegration {
            node: f.node,
            decision,
            commit: COMMIT.into(),
        },
    )
    .unwrap();
    let (args, request) = f.finish_vector();
    for _ in 0..2 {
        let reply = f.call(&args, &request, false);
        assert_eq!(
            reply,
            json!({"schema":1,"team":TEAM,"node":f.node,"state":"done","error":""})
        );
    }
    let tasks = ProjectStore::open(&f.config.state.join("replica"), &f.task.project).unwrap();
    assert_eq!(
        tasks.load(&f.task.uuid).unwrap().view().unwrap().stage,
        Stage::Done
    );
}
fn reserve_stage(f: &mut Fixture) -> crate::teams::store::StageTaskEffect {
    let node = f.h.store.stage_tasks(TEAM, 1, None).unwrap().rows[0].node;
    let id = uuid::Uuid::new_v4().to_string();
    f.h.run(
        &service(),
        Operation::StageTaskIntent {
            node,
            effect_request_id: id.clone(),
        },
    )
    .unwrap();
    f.h.store
        .stage_task_effect(TEAM, node, &id)
        .unwrap()
        .unwrap()
}
fn created_stage(f: &mut Fixture) -> (u32, TaskRef) {
    let effect = reserve_stage(f);
    let node = effect.node;
    let id = effect.request_id.clone();
    let (args, request) = f.stage_vector(&effect);
    let crashed = f.process(&args, &request.to_string(), Some("stage-crash"));
    assert_eq!(crashed.status.code(), Some(78));
    let task = TaskRef {
        project: f.task.project.clone(),
        uuid: id.clone(),
    };
    {
        let tasks = ProjectStore::open(&f.config.state.join("replica"), &task.project).unwrap();
        let mut doc = tasks.load(&id).unwrap();
        doc.set_title("Person renamed this stage", &TaskActor::Person, 2)
            .unwrap();
        tasks.save(&mut doc).unwrap();
    }
    let before = {
        let tasks = ProjectStore::open(&f.config.state.join("replica"), &task.project).unwrap();
        tasks.load(&id).unwrap().revision()
    };
    let reply = f.call(&args, &request, false);
    assert_eq!(reply["state"], "created");
    assert_eq!(reply["id"], id);
    assert_eq!(reply["team"], TEAM);
    assert_eq!(reply["node"], node);
    assert_eq!(reply["error"], "");
    {
        let tasks = ProjectStore::open(&f.config.state.join("replica"), &task.project).unwrap();
        assert_eq!(tasks.load(&id).unwrap().revision(), before);
    }
    f.h.run(
        &service(),
        Operation::StageTaskOutcome {
            effect_request_id: id,
            outcome: StageTaskOutcome::Created { task: task.clone() },
        },
    )
    .unwrap();
    f.h.run(
        &person(),
        Operation::Return {
            node: f.node,
            reason: "Revise the task".into(),
        },
    )
    .unwrap();
    (node, task)
}
fn creation_and_clear() {
    let mut f = Fixture::new();
    f.submitted();
    f.reported();
    let (node, task) = created_stage(&mut f);
    let team = f.h.store.team(TEAM).unwrap().unwrap();
    let (args, request) = f.clear_vector(node, &task);
    {
        use crate::desktop::teams::timers::{Answer, ClearRequest, Effects, Native};
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut adapter = Native {
            config: &f.config,
            machine: &team.home,
            stop: &stop,
        };
        let answer = Native::fixture_program(&f.cli, || {
            adapter.clear(
                &task,
                &ClearRequest {
                    schema: 1,
                    team: TEAM.into(),
                    node,
                },
            )
        });
        assert_eq!(answer, Answer::Cleared);
        eprintln!("REAL_COMMAND package10 Native::clear => {answer:?}");
    }
    for _ in 0..2 {
        let reply = f.call(&args, &request, false);
        assert_eq!(
            reply,
            json!({"schema":1,"team":TEAM,"node":node,"state":"cleared","error":""})
        );
    }
    let tasks = ProjectStore::open(&f.config.state.join("replica"), &task.project).unwrap();
    assert!(tasks.load(&task.uuid).unwrap().team().unwrap().is_none());
}

mod cancel;
fn handover() {
    let mut f = Fixture::new();
    f.submitted();
    f.reported();
    f.h.run(
        &person(),
        Operation::Reassign {
            node: f.node,
            member: "m2".into(),
            reason: "New holder".into(),
        },
    )
    .unwrap();
    let id = f.h.reopen(f.node, "m2");
    assert_eq!(f.delivery(&id, "m2", false)["state"], "submitted");
    assert_eq!(f.inputs(), 2);
    let tasks = ProjectStore::open(&f.config.state.join("replica"), &f.task.project).unwrap();
    let current = tasks
        .load(&f.task.uuid)
        .unwrap()
        .view()
        .unwrap()
        .execution
        .unwrap()
        .current
        .unwrap();
    assert_eq!(current.executor.unwrap().session, "codex/command-m2");
    assert!(matches!(
        current.origin,
        zerus_replica::execution::Origin::Handoff
    ));
}

mod refusals;

fn empty_execution_handover() {
    let mut f = Fixture::new();
    crate::teams::tasks::observe(&f.config.state, &f.task).unwrap();
    f.h.run(
        &person(),
        Operation::Reassign {
            node: f.node,
            member: "m2".into(),
            reason: "A member must take over".into(),
        },
    )
    .unwrap();
    let generation = f.h.store.generation(TEAM, f.node).unwrap().unwrap();
    assert!(f
        .h
        .store
        .own_task_current(TEAM, f.node, generation)
        .unwrap());
    let id = f.h.reopen(f.node, "m2");
    let (args, input) = f.delivery_vector(&id, "m2");
    let reply = f.call(&args, &input, true);
    let outcome =
        super::super::super::native::decode_delivery(&reply.to_string().into_bytes(), &id);
    assert!(
        matches!(outcome, crate::teams::ops::HandoutOutcome::Refused { .. }),
        "{reply}"
    );
    f.h.reopen_outcome(&id, outcome);
    assert_eq!(f.inputs(), 0);
    f.h.run(
        &person(),
        Operation::Cancel {
            node: f.node,
            reason: "The decision settled".into(),
        },
    )
    .unwrap();
}

pub(super) fn superseded() {
    for shape in ["review", "conflict", "done conflict"] {
        let mut f = Fixture::new();
        f.submitted();
        f.reported();
        f.h.settings(|s| s.delegations.merge_accepted = false);
        f.h.run(&person(), Operation::Accept { node: f.node })
            .unwrap();
        let decision = f.h.accept_decision(f.node);
        assert!(f
            .h
            .store
            .cards(TEAM, 200, None)
            .unwrap()
            .rows
            .iter()
            .any(|c| c.node == Some(f.node) && c.kind == "integration" && c.state == "open"));
        let task = f.task.clone();
        let observation = if shape == "review" {
            let tasks =
                ProjectStore::open(&f.config.state.join("replica"), &task.project).unwrap();
            let mut doc = tasks.load(&task.uuid).unwrap();
            let at = chrono::Utc::now().timestamp_millis();
            let person = SessionKey {
                machine: "example-machine".into(),
                session_id: "launch_id:person".into(),
                session: "codex/person".into(),
            };
            doc.set_stage(Stage::Todo, &TaskActor::Person, at).unwrap();
            doc.record_start(&TaskActor::Agent(person.clone()), at + 1)
                .unwrap();
            let attempt = doc
                .view()
                .unwrap()
                .execution
                .and_then(|execution| execution.current)
                .unwrap()
                .id;
            let reported = doc
                .report_attempt(
                    &attempt,
                    "The person's own result",
                    zerus_replica::execution::ReportKind::Ready,
                    None,
                    &TaskActor::Agent(person),
                    at + 2,
                )
                .unwrap();
            assert!(reported.moves, "{shape}: the person's report did not move the task");
            tasks.save(&mut doc).unwrap();
            drop(tasks);
            crate::teams::tasks::observe(&f.config.state, &task).unwrap();
            None
        } else {
            let mut o = f.h.store.task_observation(TEAM, f.node).unwrap().unwrap();
            o.previous_revision = o.revision.clone();
            o.revision = uuid::Uuid::new_v4().to_string();
            o.attempt += 1;
            o.attempt_id.clear();
            o.attempt_conflict = true;
            o.stage = if shape == "conflict" {
                crate::teams::ops::TaskStage::InReview
            } else {
                crate::teams::ops::TaskStage::Done
            };
            Some(o)
        };
        let tasks = ProjectStore::open(&f.config.state.join("replica"), &task.project).unwrap();
        let before = tasks.load(&task.uuid).unwrap().revision();
        let stage = tasks.load(&task.uuid).unwrap().view().unwrap().stage;
        drop(tasks);
        if let Some(o) = &observation {
            f.h.run(
                &service(),
                Operation::ObserveTask {
                    node: f.node,
                    observation: o.clone(),
                },
            )
            .unwrap();
        }
        let tasks = ProjectStore::open(&f.config.state.join("replica"), &task.project).unwrap();
        let doc = tasks.load(&task.uuid).unwrap();
        assert_eq!(doc.revision(), before, "{shape}: the task document changed");
        assert_eq!(doc.view().unwrap().stage, stage, "{shape}: the task stage changed");
        drop(tasks);
        assert!(
            f.h.store.queue(TEAM, 200, None).unwrap().rows.is_empty(),
            "{shape}: the node is still live"
        );
        let items: Vec<_> = f
            .h
            .store
            .inbox(TEAM, 200, None)
            .unwrap()
            .rows
            .into_iter()
            .filter(|i| i.node == f.node && i.kind == "accepted_superseded")
            .collect();
        assert_eq!(items.len(), 1, "{shape}");
        assert!(
            items[0]
                .text
                .contains("the person's own run replaced the team's accepted result"),
            "{shape}"
        );
        assert!(
            !f.h.store
                .cards(TEAM, 200, None)
                .unwrap()
                .rows
                .iter()
                .any(|c| c.node == Some(f.node) && c.state == "open"),
            "{shape}: a card stayed open"
        );
        let decided = f
            .h
            .store
            .decisions(TEAM, 200, None)
            .unwrap()
            .rows
            .into_iter()
            .find(|d| d.seq == decision)
            .unwrap();
        assert_eq!(
            decided.state,
            crate::teams::store::DecisionState::Settled,
            "{shape}"
        );
        // A replay writes nothing: the same real observation, or the same
        // direct operation, leaves exactly one item.
        if let Some(o) = observation {
            f.h.run(
                &service(),
                Operation::ObserveTask {
                    node: f.node,
                    observation: o,
                },
            )
            .unwrap();
        } else {
            crate::teams::tasks::observe(&f.config.state, &task).unwrap();
        }
        assert_eq!(
            f.h
                .store
                .inbox(TEAM, 200, None)
                .unwrap()
                .rows
                .iter()
                .filter(|i| i.node == f.node && i.kind == "accepted_superseded")
                .count(),
            1,
            "{shape}: replay wrote another item"
        );
    }
}
