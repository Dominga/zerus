//! Catalog editor controls. Catalog rows are seeded by the production import writer.
use super::harness::*;
use crate::teams::cli::actor::Caller;
use serde_json::{json, Value};

fn call(h: &Harness, line: &str, input: Value) -> Value {
    h.cli(&Caller::Person, line, &input.to_string()).0
}

#[test]
fn editor_stale_revision_refuses_and_exact_duplicate_is_a_receipt_not_a_write() {
    let h = Harness::new();
    let snapshot = call(&h, "catalog snapshot", json!({}));
    assert_eq!(snapshot["ok"], true, "{snapshot}");
    let request = json!({"schema":1,"request_id":"00000000-0000-4000-8000-000000000148",
        "expected_revision":snapshot["revision"],"catalog":{"roles":[{"id":"builder","name":"Builder","agent":"codex"}]}});
    let saved = call(&h, "catalog import", request.clone());
    assert_eq!(saved["ok"], true, "{saved}");
    assert_ne!(saved["revision"], snapshot["revision"]);
    assert_eq!(call(&h, "catalog import", request.clone()), saved);
    assert_eq!(
        call(
            &h,
            "catalog receipt 00000000-0000-4000-8000-000000000148",
            json!({})
        ),
        saved
    );
    let mut changed = request.clone();
    changed["catalog"]["roles"][0]["name"] = json!("Changed");
    assert_eq!(
        call(&h, "catalog import", changed)["error"]["code"],
        "request_conflict"
    );
    let mut stale = request;
    stale["request_id"] = json!("00000000-0000-4000-8000-000000000149");
    assert_eq!(
        call(&h, "catalog import", stale)["error"]["code"],
        "stale_revision"
    );
    let after = call(&h, "catalog snapshot", json!({}));
    assert_eq!(after["person"]["roles"][0]["name"], "Builder");
}

#[test]
fn editor_is_person_only_and_invalid_catalog_writes_nothing() {
    let h = Harness::new();
    let (refused, _) = h.cli(&Caller::Session(key(OTHER_SESSION)), "catalog snapshot", "");
    assert_eq!(refused["error"]["code"], "not_allowed");
    let snapshot = call(&h, "catalog snapshot", json!({}));
    let request = json!({"schema":1,"request_id":"00000000-0000-4000-8000-000000000148",
        "expected_revision":snapshot["revision"],"catalog":{"roles":[{"id":"builder","name":"Builder","agent":"unknown"}]}});
    assert_eq!(call(&h, "catalog import", request)["ok"], false);
    assert_eq!(call(&h, "catalog snapshot", json!({}))["person"], json!({}));
}

#[test]
fn editor_snapshot_is_readonly_and_legacy_template_write_invalidates_its_revision() {
    let h = Harness::new();
    let before = call(&h, "catalog snapshot", json!({}));
    assert!(!h.config.dir.join("teams").exists());
    let catalog = serde_json::to_value(zerus_team_plan::builtin()).unwrap();
    let mut template = catalog["templates"][0].clone();
    template["name"] = json!("Personal template");
    assert_eq!(call(&h, "template save", template)["ok"], true);
    let edit = json!({"schema":1,"request_id":"00000000-0000-4000-8000-000000000150", "expected_revision":before["revision"],"catalog":{}});
    assert_eq!(
        call(&h, "catalog import", edit)["error"]["code"],
        "stale_revision"
    );
    let missing = call(
        &h,
        "catalog receipt 00000000-0000-4000-8000-000000000151",
        json!({}),
    );
    assert_eq!(missing["outcome"], "uncertain");
}

#[test]
fn editor_duplicate_receipt_never_overwrites_a_later_legacy_save() {
    let h = Harness::new();
    let start = call(&h, "catalog snapshot", json!({}));
    let request = json!({"schema":1,"request_id":"00000000-0000-4000-8000-000000000152","expected_revision":start["revision"],"catalog":{"planning_rules":["first"]}});
    let first = call(&h, "catalog import", request.clone());
    assert_eq!(first["outcome"], "done");
    assert_eq!(
        call(&h, "catalog import", json!({"planning_rules":["later"]}))["ok"],
        true
    );
    assert_eq!(call(&h, "catalog import", request), first);
    assert_eq!(
        call(&h, "catalog snapshot", json!({}))["person"]["planning_rules"],
        json!(["later"])
    );
}
