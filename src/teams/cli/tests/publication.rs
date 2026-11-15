//! These contexts drive the core and UI fixtures. Task documents, phase rows,
//! review cards and archive rows all come from their product writers.
use super::harness::*;
use crate::teams::{cli::actor::Caller, ops::Operation};
use serde_json::{json, Value};
use zerus_replica::{Actor, ProjectStore, Stage};

fn context(h: &Harness) -> Value {
    let mut out = serde_json::Map::new();
    for (key, command) in [
        ("phases", format!("phases --team {TEAM}")),
        ("draft", format!("phase --team {TEAM} --phase 1")),
        ("status", format!("status --team {TEAM}")),
        ("queue", format!("queue --team {TEAM}")),
        ("waiting", "waiting".into()),
    ] {
        let (v, exit) = h.cli(&Caller::Person, &command, "");
        assert_eq!(exit, 0, "{v}");
        out.insert(key.into(), v);
    }
    Value::Object(out)
}

fn produced() -> Value {
    let h = Harness::new();
    crate::swarm::fixture_project(&h.config, MACHINE, "example-project");
    let tasks = ProjectStore::open(&h.state().join("replica"), "example-project").unwrap();
    tasks
        .save_project(&zerus_replica::ProjectDoc::new())
        .unwrap();
    drop(tasks);
    let mut create = serde_json::to_value(crate::teams::ops::tests::create()).unwrap();
    create["op"] = "start_planning".into();
    create["goal"] = "Deliver the reviewed foundation.".into();
    create["bundle"]["template"]["settings"]["integration"] = "none".into();
    create["bundle"]["pipelines"][0]["stages"] = json!([]);
    h.apply(&person(), serde_json::from_value(create).unwrap());
    h.bind("lead", LEAD_SESSION);
    let mut draft = crate::teams::ops::publication::tests::input(&[1, 2]);
    draft.nodes[1].deps = vec![zerus_team_plan::Dep {
        node: 1,
        kind: zerus_team_plan::DepKind::Hard,
    }];
    h.apply(
        &person(),
        Operation::SubmitDraftPhase {
            phase: 1,
            draft: draft.clone(),
        },
    );
    h.apply(
        &service(),
        Operation::ReconcilePhaseReview {
            phase: 1,
            generation: 1,
        },
    );
    let mut out = serde_json::Map::new();
    out.insert("draft".into(), context(&h));
    let command = |name: &str, fields: Value| {
        let mut body = fields.as_object().unwrap().clone();
        body.insert("request_id".into(), json!(uid(900 + h.revision() as u32)));
        body.insert("team".into(), json!(TEAM));
        body.insert("expected_revision".into(), json!(h.revision()));
        let (v, exit) = h.cli(&Caller::Person, name, &Value::Object(body).to_string());
        assert_eq!(exit, 0, "{v}");
    };
    command(
        "reject-phase",
        json!({"phase":1,"generation":1,"comment":"Make the regression test explicit."}),
    );
    out.insert("rejected".into(), context(&h));
    draft.nodes[0]
        .criteria
        .push("The regression is covered through the real writer".into());
    h.apply(
        &person(),
        Operation::EditDraftPhase {
            phase: 1,
            generation: 1,
            draft,
        },
    );
    h.apply(
        &service(),
        Operation::ReconcilePhaseReview {
            phase: 1,
            generation: 2,
        },
    );
    out.insert("revised".into(), context(&h));
    command(
        "approve-phase",
        json!({"phase":1,"generation":2,"confirm_initial":true}),
    );
    out.insert("publishing".into(), context(&h));
    let requests = h.store().publication_work(TEAM).unwrap();
    for (i, r) in requests.iter().enumerate() {
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
        if i == 0 {
            out.insert("partial".into(), context(&h));
        }
    }
    let p = h
        .store()
        .phase_draft(TEAM, 1)
        .unwrap()
        .publication
        .unwrap()
        .id;
    h.apply(
        &service(),
        Operation::ReconcilePublication {
            phase: 1,
            publication: p.clone(),
        },
    );
    for r in &requests {
        crate::teams::tasks::observe(h.state(), &r.task).unwrap();
    }
    out.insert("published".into(), context(&h));
    for r in &requests {
        let tasks = ProjectStore::open(&h.state().join("replica"), &r.task.project).unwrap();
        let mut doc = tasks.load(&r.task.uuid).unwrap();
        let executor = Actor::Agent(zerus_replica::SessionKey {
            machine: MACHINE.into(),
            session: "claude/publication-fixture".into(),
            session_id: LEAD_SESSION.into(),
        });
        doc.record_start(&executor, NOW + 100).unwrap();
        assert!(doc
            .record_report(
                "The fixture completed the planned work.",
                &executor,
                NOW + 101
            )
            .unwrap());
        tasks
            .change_stage(&mut doc, Stage::Done, &Actor::Person, NOW + 102)
            .unwrap();
        tasks.save(&mut doc).unwrap();
        drop(tasks);
        crate::teams::tasks::observe(h.state(), &r.task).unwrap();
    }
    h.apply(
        &service(),
        Operation::ReconcilePhase {
            phase: 1,
            publication: p.clone(),
        },
    );
    out.insert("awaiting".into(), context(&h));
    command("accept-phase", json!({"phase":1,"publication":p}));
    out.insert("accepted".into(), context(&h));
    Value::Object(out)
}

#[test]
fn publication_fixture_equals_real_command_task_and_archive_writers() {
    assert_eq!(
        produced(),
        serde_json::from_str::<Value>(include_str!(
            "../../../../crates/team-plan/fixtures/cli-phase-publication.json"
        ))
        .unwrap()
    );
}

#[test]
#[ignore = "writes the publication fixture through the real product writers"]
fn regenerate_publication_fixture() {
    std::fs::write(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("crates/team-plan/fixtures/cli-phase-publication.json"),
        serde_json::to_string_pretty(&produced()).unwrap() + "\n",
    )
    .unwrap();
}
