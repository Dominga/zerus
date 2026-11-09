//! The connector must keep ID twins separate before it can advertise labels.
use serde_json::{json, Value};
use zerus_mobile_connector::projects::{apply_memberships, normalize};

const ONE: &str = "11111111-1111-4111-8111-111111111111";
const TWO: &str = "22222222-2222-4222-8222-222222222222";
const NODE: &str = "33333333-3333-4333-8333-333333333333";
const LABEL: &str = "codex/example/task";

fn catalog() -> Value {
    json!({"schema":1,"initialized":true,"node_id":NODE,
        "swarm_id":"44444444-4444-4444-8444-444444444444",
        "machines":[{"id":NODE,"connection":"local"}],
        "membership_by_id":{"ready":true},
        "organization":{"default_project":"inbox","projects":[
            {"id":"one","name":"One","sessions":[format!("local\n{LABEL}")],"session_ids":[format!("local\n{ONE}")]},
            {"id":"two","name":"Two","session_ids":[format!("{NODE}\n{TWO}")]},
            {"id":"inbox","name":"Inbox"}]}})
}
fn normalized(value: &Value) -> Value {
    normalize(value, &json!({"host":"fixture"}), "fixture").unwrap()
}
fn rows() -> Vec<Value> {
    vec![
        json!({"name":LABEL,"session_id":ONE,"run_id":"one","parent":{"machine":NODE,"session_id":TWO},"human_request":{"text":"keep exact"}}),
        json!({"name":LABEL,"session_id":TWO,"run_id":"two"}),
    ]
}

#[test]
fn identical_labels_follow_their_id_memberships_and_keep_other_row_fields() {
    let catalog = normalized(&catalog());
    assert_eq!(catalog["membership_by_id"]["ready"], true);
    assert_eq!(catalog["projects"][0]["session_ids"], json!([ONE]));
    assert_eq!(catalog["projects"][1]["session_ids"], json!([TWO]));
    let mut rows = rows();
    let original = rows[0].clone();
    apply_memberships(&mut rows, &catalog);
    assert_eq!(rows[0]["mobile_project_id"], "one");
    assert_eq!(rows[1]["mobile_project_id"], "two");
    for key in ["name", "session_id", "run_id", "parent", "human_request"] {
        assert_eq!(rows[0][key], original[key]);
    }
}

#[test]
fn the_id_view_never_uses_a_label_for_missing_or_invalid_identity() {
    let catalog = normalized(&catalog());
    let mut rows = vec![
        json!({"name":LABEL,"mobile_project_id":"old"}),
        json!({"name":LABEL,"session_id":"not-an-id","mobile_project_id":"old"}),
        json!({"name":LABEL,"session_id":"55555555-5555-4555-8555-555555555555"}),
    ];
    apply_memberships(&mut rows, &catalog);
    assert!(rows[0].get("mobile_project_id").is_none());
    assert!(rows[1].get("mobile_project_id").is_none());
    // A valid session without an explicit ID membership belongs to the
    // catalog default, never the project the same label happens to name.
    assert_eq!(rows[2]["mobile_project_id"], "inbox");
}

#[test]
fn a_legacy_catalog_keeps_unique_labels_but_never_assigns_twins_by_name() {
    let mut raw = catalog();
    raw.as_object_mut().unwrap().remove("membership_by_id");
    let catalog = normalized(&raw);
    assert!(catalog.get("membership_by_id").is_none());
    assert!(catalog["projects"][0].get("session_ids").is_none());
    let mut twins = rows();
    apply_memberships(&mut twins, &catalog);
    assert!(twins
        .iter()
        .all(|row| row.get("mobile_project_id").is_none()));
    let mut unique = vec![rows().remove(0)];
    apply_memberships(&mut unique, &catalog);
    assert_eq!(unique[0]["mobile_project_id"], "one");
}

#[test]
fn only_a_positive_native_ready_flag_switches_to_id_membership() {
    for ready in [json!(false), json!("true"), json!(1), Value::Null] {
        let mut raw = catalog();
        raw["membership_by_id"]["ready"] = ready;
        let catalog = normalized(&raw);
        assert!(catalog.get("membership_by_id").is_none());
        let mut unique = vec![rows().remove(1)];
        apply_memberships(&mut unique, &catalog);
        assert_eq!(unique[0]["mobile_project_id"], "one");
    }
}

#[test]
fn invalid_or_duplicate_native_membership_never_selects_a_project() {
    for invalid in [
        json!(null),
        json!(["local\nnot-an-id"]),
        json!([format!("local\n{ONE}"), format!("local\n{ONE}")]),
        json!(["not-a-membership"]),
    ] {
        let mut raw = catalog();
        raw["organization"]["projects"][0]["session_ids"] = invalid;
        assert!(normalize(&raw, &json!({"host":"fixture"}), "fixture").is_err());
    }
    let mut raw = catalog();
    raw["organization"]["projects"][1]["session_ids"] = json!([format!("local\n{ONE}")]);
    assert!(normalize(&raw, &json!({"host":"fixture"}), "fixture").is_err());
}

#[test]
fn another_machines_id_membership_and_archive_names_cannot_select_a_live_twin() {
    let mut raw = catalog();
    raw["organization"]["projects"][0]["session_ids"] = json!([format!("remote\n{ONE}")]);
    raw["organization"]["projects"][1]["sessions"] = json!(["local\narchive\nold-archive"]);
    let catalog = normalized(&raw);
    let mut rows = vec![
        rows().remove(0),
        json!({"name":LABEL,"session_id":ONE,"state":"archived","archive_id":"old-archive"}),
    ];
    apply_memberships(&mut rows, &catalog);
    assert_eq!(rows[0]["mobile_project_id"], "inbox");
    assert_eq!(rows[1]["mobile_project_id"], "two");
}

#[test]
fn primary_null_conflict_and_missing_are_distinct_without_new_public_fields() {
    use zerus_mobile_connector::projects::PrimaryMemberships;
    let mut raw = catalog();
    raw["session_id_memberships"] = json!({"available":true,"records":[
        {"machine":NODE,"session_id":ONE,"project":null,"conflict":false},
        {"machine":NODE,"session_id":TWO,"project":null,"conflict":true}]});
    let primary = PrimaryMemberships::read(&raw).unwrap();
    let mut public = normalized(&raw);
    primary.project(&mut public);
    assert!(public.get("session_id_memberships").is_none());
    assert!(public["projects"]
        .as_array()
        .unwrap()
        .iter()
        .all(|p| p["session_ids"].as_array().is_none_or(|ids| ids.is_empty())));
    let mut twins = rows();
    primary.apply(&mut twins, &public);
    assert!(twins
        .iter()
        .all(|row| row.get("mobile_project_id").is_none()));
    let third = "55555555-5555-4555-8555-555555555555";
    let mut absent = vec![json!({"name":LABEL,"session_id":third,"run_id":"third"})];
    primary.apply(&mut absent, &public);
    assert_eq!(
        absent[0]["mobile_project_id"], "one",
        "only proven absence permits unique-label fallback"
    );
    raw["session_id_memberships"]["available"] = json!(false);
    PrimaryMemberships::read(&raw)
        .unwrap()
        .apply(&mut absent, &public);
    assert!(absent[0].get("mobile_project_id").is_none());
}

#[test]
fn primary_grouping_separates_twins_and_ignores_other_computers() {
    use zerus_mobile_connector::projects::PrimaryMemberships;
    let mut raw = catalog();
    raw["session_id_memberships"] = json!({"available":true,"records":[
        {"machine":NODE,"session_id":ONE,"project":"two","conflict":false},
        {"machine":NODE,"session_id":TWO,"project":"one","conflict":false},
        {"machine":"66666666-6666-4666-8666-666666666666","session_id":ONE,"project":"inbox","conflict":false}]});
    let primary = PrimaryMemberships::read(&raw).unwrap();
    let mut public = normalized(&raw);
    primary.project(&mut public);
    let mut twins = rows();
    let parent = twins[0]["parent"].clone();
    let human = twins[0]["human_request"].clone();
    primary.apply(&mut twins, &public);
    assert_eq!(twins[0]["mobile_project_id"], "two");
    assert_eq!(twins[1]["mobile_project_id"], "one");
    assert_eq!(twins[0]["parent"], parent);
    assert_eq!(twins[0]["human_request"], human);
    assert_eq!(public["projects"][0]["session_ids"], json!([TWO]));
    assert_eq!(public["projects"][1]["session_ids"], json!([ONE]));
    let record = raw["session_id_memberships"]["records"][0].clone();
    raw["session_id_memberships"]["records"]
        .as_array_mut()
        .unwrap()
        .push(record);
    assert!(PrimaryMemberships::read(&raw).is_err());
    raw["session_id_memberships"]["records"] =
        json!([{ "machine":NODE,"session_id":ONE,"project":"one","conflict":true}]);
    assert!(PrimaryMemberships::read(&raw).is_err());
    raw["session_id_memberships"]["records"] =
        json!([{ "machine":NODE,"session_id":ONE,"conflict":false}]);
    assert!(
        PrimaryMemberships::read(&raw).is_err(),
        "missing is not an explicit null"
    );
}
