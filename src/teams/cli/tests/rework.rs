//! Rework UI contexts come from the same CLI, named-task, document and
//! archival writers as publication. No application row is inserted by hand.
use super::harness::*;
use crate::teams::{cli::actor::Caller, ops::Operation};
use serde_json::{json, Value};
use zerus_replica::{Actor, ProjectStore, Stage};

pub(super) fn from_waiting(
    h: &Harness,
    base: String,
    original: Vec<crate::teams::store::publication::PlannedTaskRequest>,
) -> Value {
    let mut out = serde_json::Map::new();
    let command = |name: &str, fields: Value| {
        let mut body = fields.as_object().unwrap().clone();
        body.insert(
            "request_id".into(),
            json!(uid(10_000 + h.revision() as u32)),
        );
        body.insert("team".into(), json!(TEAM));
        body.insert("expected_revision".into(), json!(h.revision()));
        let (v, exit) = h.cli(&Caller::Person, name, &Value::Object(body).to_string());
        assert_eq!(exit, 0, "{v}");
    };
    command(
        "send-back-phase",
        json!({"phase":1,"publication":base,"comment":"Keep the accepted foundation; add the missing failure-case test."}),
    );
    let id = h.store().phase_draft(TEAM, 1).unwrap().rework.unwrap().id;
    out.insert("sent_back".into(), super::publication::context(h));
    let next = h.store().next_draft_node(TEAM).unwrap();
    let mut draft = crate::teams::ops::publication::tests::input(&[next, next + 1]);
    draft.title = "Foundation corrections".into();
    draft.barrier = h.store().phase_draft(TEAM, 1).unwrap().phase.barrier;
    draft.nodes[0]
        .criteria
        .push("The correction preserves every accepted task.".into());
    command(
        "submit-phase-corrections",
        json!({"phase":1,"publication":base,"rework":id,"draft":draft}),
    );
    out.insert("proposal".into(), super::publication::context(h));
    let generation = h
        .store()
        .phase_draft(TEAM, 1)
        .unwrap()
        .rework
        .unwrap()
        .draft
        .unwrap()
        .generation
        .unwrap();
    command(
        "reject-phase-corrections",
        json!({"phase":1,"publication":base,"rework":id,"generation":generation,"comment":"Make the failure case explicit."}),
    );
    out.insert("rejected".into(), super::publication::context(h));
    let rejection = h
        .store()
        .phase_draft(TEAM, 1)
        .unwrap()
        .rework
        .unwrap()
        .draft
        .unwrap()
        .rejection
        .unwrap()
        .id;
    command(
        "undo-phase-correction-rejection",
        json!({"phase":1,"publication":base,"rework":id,"generation":generation,"rejection":rejection}),
    );
    out.insert("undone".into(), super::publication::context(h));
    super::guides::correction_edit(h, &base, &id, generation);
    out.insert("revised".into(), super::publication::context(h));
    let generation = h
        .store()
        .phase_draft(TEAM, 1)
        .unwrap()
        .rework
        .unwrap()
        .draft
        .unwrap()
        .generation
        .unwrap();
    h.apply(
        &service(),
        Operation::ReconcileCorrectionReview {
            phase: 1,
            publication: base.clone(),
            rework: id.clone(),
            generation,
        },
    );
    command(
        "approve-phase-corrections",
        json!({"phase":1,"publication":base,"rework":id,"generation":generation}),
    );
    out.insert("reserved".into(), super::publication::context(h));
    let work = h.store().publication_work(TEAM).unwrap();
    assert_eq!(work.len(), 2);
    let publication = work[0].publication.clone();
    for (i, r) in work.iter().enumerate() {
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
            out.insert("partial".into(), super::publication::context(h));
        }
    }
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
    out.insert("published".into(), super::publication::context(h));
    for r in &work {
        let tasks = ProjectStore::open(&h.state().join("replica"), &r.task.project).unwrap();
        let mut doc = tasks.load(&r.task.uuid).unwrap();
        let executor = Actor::Agent(zerus_replica::SessionKey {
            machine: MACHINE.into(),
            session: "claude/rework-fixture".into(),
            session_id: LEAD_SESSION.into(),
        });
        doc.record_start(&executor, NOW + 200).unwrap();
        assert!(doc
            .record_report("The explicit corrections passed.", &executor, NOW + 201)
            .unwrap());
        tasks
            .change_stage(&mut doc, Stage::Done, &Actor::Person, NOW + 202)
            .unwrap();
        tasks.save(&mut doc).unwrap();
        drop(tasks);
        crate::teams::tasks::observe(h.state(), &r.task).unwrap();
    }
    for r in &original {
        assert_eq!(
            h.store()
                .node(TEAM, r.node.id)
                .unwrap()
                .unwrap()
                .task
                .as_ref(),
            Some(&r.task)
        );
    }
    h.apply(
        &service(),
        Operation::ReconcilePhase {
            phase: 1,
            publication: publication.clone(),
        },
    );
    out.insert("awaiting".into(), super::publication::context(h));
    command("accept-phase", json!({"phase":1,"publication":publication}));
    out.insert("accepted".into(), super::publication::context(h));
    Value::Object(out)
}

#[test]
#[ignore = "writes the rework contexts through real CLI/task/archive writers"]
fn regenerate_rework_fixture() {
    std::fs::write(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("crates/team-plan/fixtures/cli-phase-rework.json"),
        serde_json::to_string_pretty(&super::publication::correction_contexts()).unwrap() + "\n",
    )
    .unwrap();
}

#[test]
fn rework_fixture_equals_real_cli_task_and_archival_writer_output() {
    assert_eq!(
        super::publication::correction_contexts(),
        serde_json::from_str::<Value>(include_str!(
            "../../../../crates/team-plan/fixtures/cli-phase-rework.json"
        ))
        .unwrap()
    );
}
