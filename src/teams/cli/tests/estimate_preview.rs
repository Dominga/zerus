//! Draft estimates and an empty observation page from existing product writers.
use super::harness::*;
use crate::teams::{cli::actor::Caller, ops::Operation};
use serde_json::Value;

fn produced() -> Value {
    let h = Harness::new();
    let mut create = serde_json::to_value(crate::teams::ops::tests::create()).unwrap();
    create["op"] = "start_planning".into();
    create["goal"] = "Review implementation estimates before approval.".into();
    h.apply(&person(), serde_json::from_value(create).unwrap());
    h.apply(
        &person(),
        Operation::SubmitDraftPhase {
            phase: 1,
            draft: crate::teams::ops::publication::tests::input(&[1, 2]),
        },
    );
    let tables = || -> u32 {
        h.store().db.as_ref().unwrap().query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name LIKE 'observed_timing_%'", [], |r| r.get(0)).unwrap()
    };
    assert_eq!(tables(), 0);
    let revision = h.revision();
    let (value, exit) = h.cli(
        &Caller::Person,
        &format!("observed --team {TEAM} --phase 1"),
        "",
    );
    assert_eq!(exit, 0, "{value}");
    assert!(value["observed"]["rows"].as_array().unwrap().is_empty());
    assert!(value["observed"]["cohort"]["units"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(value["observed"]["as_of_ms"].is_null());
    assert!(value["observed"]["scope"]["window"]["to_ms"].is_null());
    assert_eq!(
        tables(),
        0,
        "a selected read never creates observation companions"
    );
    assert_eq!(h.revision(), revision);
    let mut context = serde_json::Map::new();
    for (key, command) in [
        ("status", format!("status --team {TEAM}")),
        ("queue", format!("queue --team {TEAM}")),
        ("phases", format!("phases --team {TEAM}")),
        ("draft", format!("phase --team {TEAM} --phase 1")),
    ] {
        let (reply, exit) = h.cli(&Caller::Person, &command, "");
        assert_eq!(exit, 0, "{reply}");
        context.insert(key.into(), reply);
    }
    let mut out = value.as_object().unwrap().clone();
    out.insert("context".into(), Value::Object(context));
    Value::Object(out)
}
#[test]
fn unapproved_estimate_preview_has_no_observation_companion_or_read_side_effect() {
    let value = produced();
    assert_eq!(
        value["context"]["draft"]["draft"]["nodes"][0]["estimate_minutes"],
        30
    );
    assert!(value["context"]["draft"]["draft"]["publication"].is_null());
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("crates/team-plan/fixtures/cli-estimate-preview.json");
    if std::env::var_os("ZERUS_UPDATE_ESTIMATE_PREVIEW_FIXTURE").is_some() {
        std::fs::write(path, serde_json::to_string_pretty(&value).unwrap() + "\n").unwrap();
    } else {
        assert_eq!(
            value,
            serde_json::from_slice::<Value>(&std::fs::read(path).unwrap()).unwrap()
        );
    }
}
