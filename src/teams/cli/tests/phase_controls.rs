//! Normal S5 contexts use product CLI/ops, named-task and document writers.
use super::harness::*;
use crate::teams::{cli::actor::Caller, ops::Operation};
use serde_json::{json, Value};
use zerus_replica::{Actor, ProjectStore, Stage};
use zerus_team_plan::{phase_control::*, Acceptance, Integration};

fn scope(h: &Harness) -> PhaseControlScope {
    h.store()
        .phase_draft(TEAM, 1)
        .unwrap()
        .controls
        .unwrap()
        .scope
}
fn pause_id(h: &Harness) -> String {
    let PhasePause::Paused { episode: Some(e) } = h
        .store()
        .phase_draft(TEAM, 1)
        .unwrap()
        .controls
        .unwrap()
        .pause
    else {
        panic!("expected a real attributed pause")
    };
    e.id
}
fn command(h: &Harness, name: &str, fields: Value) {
    let mut fields = fields.as_object().unwrap().clone();
    fields.insert("team".into(), TEAM.into());
    fields.insert("request_id".into(), uid(20000 + h.revision() as u32).into());
    fields.insert("expected_revision".into(), h.revision().into());
    let (v, exit) = h.cli(&Caller::Person, name, &Value::Object(fields).to_string());
    assert_eq!(exit, 0, "{v}");
}

fn produced() -> Value {
    let h = Harness::new();
    h.create();
    h.bind("lead", LEAD_SESSION);
    crate::swarm::fixture_project(&h.config, MACHINE, "example-project");
    let tasks = ProjectStore::open(&h.state().join("replica"), "example-project").unwrap();
    tasks
        .save_project(&zerus_replica::ProjectDoc::new())
        .unwrap();
    drop(tasks);
    let mut settings = h.store().team(TEAM).unwrap().unwrap().settings;
    settings.integration = Integration::None;
    settings.phase_acceptance = Acceptance::Person;
    h.apply(&person(), Operation::ChangeSettings { settings });
    for n in 1..=3 {
        let id = h.store().next_draft_node(TEAM).unwrap();
        h.apply(
            &person(),
            Operation::SubmitDraftPhase {
                phase: n,
                draft: crate::teams::ops::publication::tests::input(&[id]),
            },
        );
    }
    let moved = h
        .store()
        .phase_draft(TEAM, 3)
        .unwrap()
        .controls
        .unwrap()
        .scope;
    command(
        &h,
        "move-phase",
        json!({"scope":moved,"relative_to":scope(&h),"placement":"before"}),
    );
    let mut out = serde_json::Map::new();
    out.insert("draft".into(), super::publication::context(&h));
    // Pause exposes its timestamp. Supply the ops writer's explicit fixture
    // clock rather than rewriting the CLI's real-clock row after creation.
    h.apply(
        &person(),
        Operation::PausePhase {
            scope: scope(&h),
            pause: None,
            reason: "Review the exact phase publication.".into(),
        },
    );
    out.insert("paused_person".into(), super::publication::context(&h));
    command(
        &h,
        "resume-phase",
        json!({"scope":scope(&h),"pause":pause_id(&h)}),
    );
    h.apply(
        &member("lead", LEAD_SESSION),
        Operation::PausePhase {
            scope: scope(&h),
            pause: None,
            reason: "Wait for the retained review.".into(),
        },
    );
    out.insert("paused_lead".into(), super::publication::context(&h));
    command(
        &h,
        "resume-phase",
        json!({"scope":scope(&h),"pause":pause_id(&h)}),
    );
    command(
        &h,
        "set-phase-barrier",
        json!({"scope":scope(&h),"barrier":true}),
    );
    command(
        &h,
        "approve-phase",
        json!({"phase":1,"generation":1,"confirm_initial":false}),
    );
    let work = h.store().publication_work(TEAM).unwrap();
    assert_eq!(work.len(), 1);
    for r in &work {
        assert_eq!(
            crate::tasks::create_planned_task_for_test(
                &h.config,
                TEAM,
                r.node.id,
                &r.task.uuid,
                false
            )
            .unwrap(),
            "created"
        );
    }
    let publication = work[0].publication.clone();
    h.apply(
        &service(),
        Operation::ReconcilePublication {
            phase: 1,
            publication: publication.clone(),
        },
    );
    for r in &work {
        crate::teams::tasks::observe(h.state(), &r.task).unwrap();
    }
    out.insert("approved_unstarted".into(), super::publication::context(&h));
    let executor = Actor::Agent(zerus_replica::SessionKey {
        machine: MACHINE.into(),
        session: "claude/phase-controls-fixture".into(),
        session_id: LEAD_SESSION.into(),
    });
    for r in &work {
        let tasks = ProjectStore::open(&h.state().join("replica"), &r.task.project).unwrap();
        let mut doc = tasks.load(&r.task.uuid).unwrap();
        doc.record_start(&executor, NOW + 10).unwrap();
        tasks.save(&mut doc).unwrap();
        drop(tasks);
        crate::teams::tasks::observe(h.state(), &r.task).unwrap();
    }
    out.insert("started".into(), super::publication::context(&h));
    for r in &work {
        let tasks = ProjectStore::open(&h.state().join("replica"), &r.task.project).unwrap();
        let mut doc = tasks.load(&r.task.uuid).unwrap();
        doc.record_report("The phase controls passed.", &executor, NOW + 11)
            .unwrap();
        tasks
            .change_stage(&mut doc, Stage::Done, &Actor::Person, NOW + 12)
            .unwrap();
        tasks.save(&mut doc).unwrap();
        drop(tasks);
        crate::teams::tasks::observe(h.state(), &r.task).unwrap();
    }
    h.apply(
        &service(),
        Operation::ReconcilePhase {
            phase: 1,
            publication: publication.clone(),
        },
    );
    out.insert("awaiting".into(), super::publication::context(&h));
    command(
        &h,
        "send-back-phase",
        json!({"phase":1,"publication":publication,"comment":"Retain the gate; add an explicit correction."}),
    );
    let id = h.store().phase_draft(TEAM, 1).unwrap().rework.unwrap().id;
    let next = h.store().next_draft_node(TEAM).unwrap();
    let mut draft = crate::teams::ops::publication::tests::input(&[next]);
    draft.barrier = true;
    command(
        &h,
        "submit-phase-corrections",
        json!({"phase":1,"publication":publication,"rework":id,"draft":draft}),
    );
    h.apply(
        &person(),
        Operation::PausePhase {
            scope: scope(&h),
            pause: None,
            reason: "Pause only new correction hand-outs.".into(),
        },
    );
    out.insert("correction".into(), super::publication::context(&h));
    command(
        &h,
        "approve-phase-corrections",
        json!({"phase":1,"publication":publication,"rework":id,"generation":2}),
    );
    out.insert(
        "correction_publishing".into(),
        super::publication::context(&h),
    );
    Value::Object(out)
}

#[test]
fn controls_fixture_equals_the_product_writers_and_complete_selected_reads() {
    let actual = produced();
    assert_eq!(
        actual["approved_unstarted"]["draft"]["draft"]["controls"]["unstarted"]["allowed"],
        true
    );
    assert_eq!(
        actual["started"]["draft"]["draft"]["controls"]["unstarted"]["allowed"],
        false
    );
    let saved = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("crates/team-plan/fixtures/cli-phase-controls.json"),
    )
    .unwrap();
    assert_eq!(actual, serde_json::from_str::<Value>(&saved).unwrap());
}

#[test]
fn pause_cli_uses_the_actual_writer_clock_and_request_identity() {
    let h = Harness::new();
    h.create();
    h.apply(
        &person(),
        Operation::SubmitDraftPhase {
            phase: 1,
            draft: crate::teams::ops::publication::tests::input(&[1]),
        },
    );
    let id = uid(20000 + h.revision() as u32);
    let before = super::super::input::now_ms();
    command(
        &h,
        "pause-phase",
        json!({"scope":scope(&h),"pause":null,"reason":"An actual CLI clock control."}),
    );
    let after = super::super::input::now_ms();
    let PhasePause::Paused { episode: Some(e) } = h
        .store()
        .phase_draft(TEAM, 1)
        .unwrap()
        .controls
        .unwrap()
        .pause
    else {
        panic!("CLI pause has no owner")
    };
    assert_eq!(e.id, id);
    assert!((before..=after).contains(&e.created_ms));
}

#[test]
#[ignore = "writes S5 contexts through actual product CLI/task/archive writers"]
fn regenerate_phase_controls_fixture() {
    std::fs::write(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("crates/team-plan/fixtures/cli-phase-controls.json"),
        serde_json::to_string_pretty(&produced()).unwrap() + "\n",
    )
    .unwrap();
}
