//! Actual admitted core6 read seam. Every row comes from product operations;
//! task start/report are synthetic source inputs, not native execution proof.
use super::*;
use serde_json::{json, Value};
use zerus_replica::{Actor, ProjectStore, SessionKey, Stage};

fn produced() -> Value {
    let h = super::published::fixture_source();
    let base = h
        .store()
        .phase_draft(TEAM, 1)
        .unwrap()
        .publication
        .unwrap()
        .id;
    let task = h.store().node(TEAM, 1).unwrap().unwrap().task.unwrap();
    let tasks = ProjectStore::open(&h.state().join("replica"), &task.project).unwrap();
    let mut doc = tasks.load(&task.uuid).unwrap();
    let actor = Actor::Agent(SessionKey {
        machine: MACHINE.into(),
        session: "claude/observed-fixture".into(),
        session_id: LEAD_SESSION.into(),
    });
    doc.record_start(&actor, NOW + 100).unwrap();
    assert!(doc
        .record_report(
            "Synthetic source report, no native execution.",
            &actor,
            NOW + 101
        )
        .unwrap());
    tasks
        .change_stage(&mut doc, Stage::Done, &Actor::Person, NOW + 102)
        .unwrap();
    tasks.save(&mut doc).unwrap();
    drop(tasks);
    crate::teams::tasks::observe(h.state(), &task).unwrap();
    h.apply(
        &service(),
        Operation::ReconcilePhase {
            phase: 1,
            publication: base.clone(),
        },
    );
    h.apply(
        &person(),
        Operation::SendBackPhase {
            phase: 1,
            publication: base.clone(),
            comment: "Keep the original observation provenance.".into(),
        },
    );
    let rework = h.store().phase_draft(TEAM, 1).unwrap().rework.unwrap().id;
    let next = h.store().next_draft_node(TEAM).unwrap();
    let mut draft = crate::teams::ops::publication::tests::input(&[next]);
    draft.barrier = h.store().phase_draft(TEAM, 1).unwrap().phase.barrier;
    h.apply(
        &person(),
        Operation::SubmitPhaseCorrections {
            phase: 1,
            publication: base.clone(),
            rework: rework.clone(),
            draft,
        },
    );
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
            rework: rework.clone(),
            generation,
        },
    );
    h.apply(
        &person(),
        Operation::ApprovePhaseCorrections {
            phase: 1,
            publication: base.clone(),
            rework,
            generation,
        },
    );
    let work = h.store().publication_work(TEAM).unwrap();
    assert_eq!(work.len(), 1);
    let next_publication = work[0].publication.clone();
    assert_ne!(next_publication, base);
    let outer = h.store().phase_draft(TEAM, 1).unwrap();
    assert_eq!(outer.publication.unwrap().id, base);
    let page = h
        .store()
        .observed_timing(TEAM, 1, None, 86_400_000, 200, None)
        .unwrap();
    assert_eq!(page.scope.publication.as_ref().unwrap().id,base,"an open rework must use the same retained BASE as the phase read, never latest reserved NEW");
    assert!(page
        .rows
        .iter()
        .all(|f| f.key.publication.is_none() && f.implementation_observed_ms.is_none()));
    let reserved = super::published::context(&h);
    for r in work {
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
    h.apply(
        &service(),
        Operation::ReconcilePublication {
            phase: 1,
            publication: next_publication.clone(),
        },
    );
    let published = super::published::context(&h);
    assert_eq!(
        published["observed"]["scope"]["publication"]["id"],
        next_publication
    );
    assert_ne!(
        published["observed"]["scope"]["publication"],
        reserved["observed"]["scope"]["publication"]
    );
    json!({"reserved":reserved,"published":published})
}

#[test]
fn admitted_rework_read_and_observed_scope_keep_base_until_the_settled_switch() {
    let value = produced();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("crates/team-plan/fixtures/cli-observed-rework.json");
    if std::env::var_os("ZERUS_UPDATE_OBSERVED_REWORK_FIXTURE").is_some() {
        std::fs::write(path, serde_json::to_string_pretty(&value).unwrap() + "\n").unwrap();
    } else {
        assert_eq!(
            value,
            serde_json::from_str::<Value>(&std::fs::read_to_string(path).unwrap()).unwrap()
        );
    }
}
