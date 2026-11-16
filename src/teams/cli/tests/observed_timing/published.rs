//! A real published phase and real planned-task writers; no native input.
use super::*;
use crate::teams::ops::Operation;
use serde_json::{json, Value};

fn recorded() -> Harness {
    let _stream = crate::teams::store::observed_timing::fixture_stream();
    let h = Harness::new();
    crate::swarm::fixture_project(&h.config, MACHINE, "example-project");
    let tasks =
        zerus_replica::ProjectStore::open(&h.state().join("replica"), "example-project").unwrap();
    tasks
        .save_project(&zerus_replica::ProjectDoc::new())
        .unwrap();
    drop(tasks);
    let mut create = serde_json::to_value(crate::teams::ops::tests::create()).unwrap();
    create["op"] = "start_planning".into();
    create["goal"] = "Observe the reviewed fixture phase.".into();
    create["bundle"]["template"]["settings"]["integration"] = "none".into();
    create["bundle"]["pipelines"][0]["stages"] = json!([]);
    for role in create["bundle"]["roles"].as_array_mut().unwrap() {
        if role["id"] == "developer" {
            role["account"] = json!({"mode":"named","account":"example-planning-account"});
        }
    }
    h.apply(&person(), serde_json::from_value(create).unwrap());
    h.bind("lead", LEAD_SESSION);
    h.apply(
        &person(),
        Operation::SubmitDraftPhase {
            phase: 1,
            draft: crate::teams::ops::publication::tests::input(&[1]),
        },
    );
    h.apply(
        &service(),
        Operation::ReconcilePhaseReview {
            phase: 1,
            generation: 1,
        },
    );
    let (reply,exit)=h.native_command("approve-phase",&json!({"request_id":uid(50_000),"team":TEAM,"expected_revision":h.revision(),"phase":1,"generation":1,"confirm_initial":true}).to_string());
    assert_eq!(exit, 0, "{reply}");
    for r in h.store().publication_work(TEAM).unwrap() {
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
    let publication = h
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
            publication: publication.clone(),
        },
    );
    h.bind("m1", M1_SESSION);
    let task = h.store().node(TEAM, 1).unwrap().unwrap().task.unwrap();
    crate::teams::tasks::observe(h.state(), &task).unwrap();
    h.apply(
        &service(),
        Operation::HandoutIntent {
            node: 1,
            member: "m1".into(),
            effect_request_id: uid(50_001),
        },
    );
    h.apply(
        &service(),
        Operation::HandoutOutcome {
            effect_request_id: uid(50_001),
            outcome: HandoutOutcome::Refused {
                reason: "Synthetic fixture offered no native input.".into(),
            },
        },
    );
    for _ in 0..2 {
        h.apply(&service(), timing(&h, 1));
    }
    h
}
pub(super) fn context(h: &Harness) -> Value {
    let publication = h
        .store()
        .phase_draft(TEAM, 1)
        .unwrap()
        .publication
        .unwrap()
        .id;
    let (mut page, exit) = h.cli(
        &Caller::Person,
        &format!("observed --team {TEAM} --phase 1"),
        "",
    );
    assert_eq!(exit, 0, "{page}");
    assert_eq!(page["observed"]["scope"]["publication"]["id"], publication);
    assert!(page["observed"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["key"]["publication"].is_null() && r["implementation_observed_ms"].is_null()));
    for (key, command) in [
        ("status", format!("status --team {TEAM}")),
        ("queue", format!("queue --team {TEAM}")),
        ("phases", format!("phases --team {TEAM}")),
        ("draft", format!("phase --team {TEAM} --phase 1")),
    ] {
        let (value, exit) = h.cli(&Caller::Person, &command, "");
        assert_eq!(exit, 0, "{value}");
        page["context"][key] = value;
    }
    let core: u32 = h
        .store()
        .db
        .as_ref()
        .unwrap()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(core, crate::teams::store::SCHEMA_VERSION);
    page
}
pub(super) fn fixture_source() -> Harness {
    recorded()
}
fn produced() -> Value {
    context(&recorded())
}
#[test]
fn published_phase_observations_are_product_written_and_never_backfill_original_membership() {
    let value = produced();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("crates/team-plan/fixtures/cli-observed-phase.json");
    if std::env::var_os("ZERUS_UPDATE_OBSERVED_PHASE_FIXTURE").is_some() {
        std::fs::write(path, serde_json::to_string_pretty(&value).unwrap() + "\n").unwrap();
    } else {
        assert_eq!(
            value,
            serde_json::from_str::<Value>(&std::fs::read_to_string(path).unwrap()).unwrap()
        );
    }
}
