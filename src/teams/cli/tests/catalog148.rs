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

/// The review148 fixture uses real catalog writers; only the input roles are synthetic.
fn review148_catalog(pretty_bytes: usize) -> Value {
    let roles: Vec<Value> = (0..200)
        .map(|i| json!({"id":format!("r{i:03}"),"name":"Role","agent":"codex","prompt":"x".repeat(1102)}))
        .collect();
    let mut catalog: zerus_team_plan::catalog::Catalog =
        serde_json::from_value(json!({"roles":roles})).unwrap();
    let original = serde_json::to_vec_pretty(&catalog).unwrap().len() + 1;
    assert_eq!(original, 240_020, "the reviewed pretty-file reproduction");
    assert!(pretty_bytes <= original);
    catalog
        .roles
        .last_mut()
        .unwrap()
        .prompt
        .truncate(1102 - (original - pretty_bytes));
    assert_eq!(
        serde_json::to_vec_pretty(&catalog).unwrap().len() + 1,
        pretty_bytes
    );
    serde_json::to_value(catalog).unwrap()
}

#[test]
fn review148_compact_fits_but_pretty_does_not_refuses_before_publication() {
    let h = Harness::new();
    let snapshot = call(&h, "catalog snapshot", json!({}));
    let catalog = review148_catalog(240_020);
    assert_eq!(serde_json::to_vec(&catalog).unwrap().len(), 231_611);
    let edit = json!({"schema":1,"request_id":"00000000-0000-4000-8000-000000000170","expected_revision":snapshot["revision"],"catalog":catalog});
    let refused = call(&h, "catalog import", edit);
    assert_eq!(refused["error"]["code"], "over_cap", "{refused}");
    assert!(!crate::teams::catalog::path(&h.config.dir).exists());
    assert!(!h
        .config
        .dir
        .join("teams/catalog-requests/00000000-0000-4000-8000-000000000170.json")
        .exists());
    assert_eq!(call(&h, "catalog snapshot", json!({}))["person"], json!({}));
}

#[test]
fn editor_writer_accepts_exact_stored_limit_and_reopens_but_refuses_one_more_byte() {
    let h = Harness::new();
    let snapshot = call(&h, "catalog snapshot", json!({}));
    let catalog = review148_catalog(240_000);
    let edit = json!({"schema":1,"request_id":"00000000-0000-4000-8000-000000000172","expected_revision":snapshot["revision"],"catalog":catalog});
    let saved = call(&h, "catalog import", edit);
    assert_eq!(saved["outcome"], "done", "{saved}");
    let path = crate::teams::catalog::path(&h.config.dir);
    let before = std::fs::read(&path).unwrap();
    assert_eq!(before.len(), 240_000);
    let reopened = call(&h, "catalog snapshot", json!({}));
    assert_eq!(reopened["ok"], true, "{reopened}");
    assert_eq!(reopened["revision"], saved["revision"]);
    let edit = json!({"schema":1,"request_id":"00000000-0000-4000-8000-000000000173","expected_revision":saved["revision"],"catalog":review148_catalog(240_001)});
    let refused = call(&h, "catalog import", edit);
    assert_eq!(refused["error"]["code"], "over_cap", "{refused}");
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(!h
        .config
        .dir
        .join("teams/catalog-requests/00000000-0000-4000-8000-000000000173.json")
        .exists());
    let mut next = reopened["person"].clone();
    next["roles"][0]["name"] = json!("Next");
    let edit = json!({"schema":1,"request_id":"00000000-0000-4000-8000-000000000174","expected_revision":saved["revision"],"catalog":next});
    let saved = call(&h, "catalog import", edit);
    assert_eq!(saved["outcome"], "done", "{saved}");
    assert_eq!(
        call(&h, "catalog snapshot", json!({}))["revision"],
        saved["revision"]
    );
}
