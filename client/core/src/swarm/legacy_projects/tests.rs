use super::*;

pub(crate) fn fixture() -> (Value, Binding) {
    let binding = Binding {
        machine: "11111111-2222-4333-8444-555555555555".into(),
        node: "22222222-2222-4333-8444-555555555555".into(),
        swarm: "33333333-2222-4333-8444-555555555555".into(),
        source: "a".repeat(64),
    };
    let snapshot = json!({"schema":1,"initialized":true,"node_id":binding.node,"swarm_id":binding.swarm,
        "digest":"b".repeat(64),"versions":{},"organization":{"version":2,"default_project":"next",
        "project_order":["next"],"unknown":"keep","projects":[{"id":"next","name":"Next","color":"#123456",
        "sessions":["fixture\nsession"],"custom":"unchanged","folders":[]}]}});
    (snapshot, binding)
}
pub(crate) fn one(snapshot: &Value, binding: Binding) -> Plan {
    plan(
        snapshot,
        binding,
        "fixture",
        &json!([{"name":"Legacy fixture","dir":"/srv/legacy-fixture","src":"local"}]),
        Style::Posix,
    )
    .unwrap()
}
pub(crate) fn observed(snapshot: &Value, plan: &Plan) -> Value {
    let mut next = snapshot.clone();
    next["organization"] = plan.input()["desired"].clone();
    next["digest"] = json!("c".repeat(64));
    next
}

#[test]
fn mixed_ansible_invalid_and_valid_rows_never_complete_a_partial_import() {
    let (snapshot, binding) = fixture();
    let plan = plan(
        &snapshot,
        binding.clone(),
        "fixture",
        &json!([
            {"name":"Managed","dir":"/srv/managed","src":"ansible"},
            {"name":"Rejected","dir":"relative","src":"local"},
            {"name":"Legacy fixture","dir":"/srv/legacy-fixture","src":"local"}
        ]),
        Style::Posix,
    )
    .unwrap();
    assert!(plan.changed());
    assert_eq!(
        plan.input()["desired"]["projects"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(plan.evidence().problem(), Some(Code::InvalidFolders));
    assert!(
        !plan
            .evidence()
            .observed(&binding, &observed(&snapshot, &plan))
            .unwrap()
    );
}

#[test]
fn an_existing_next_choice_and_order_survive_and_duplicate_paths_keep_the_id() {
    let (mut snapshot, binding) = fixture();
    let folder = json!({"id":"existing","machine":"fixture","machine_id":binding.machine,"path":"/srv/kept","name":"User label","extra":9});
    snapshot["organization"]["projects"][0]["folders"] = json!([folder]);
    let plan = plan(
        &snapshot,
        binding,
        "fixture",
        &json!([
            {"name":"Next","dir":"/srv/kept/../kept"},
            {"name":"Next","dir":"/srv/new"},
            {"name":"Next","dir":"/srv/new/"}
        ]),
        Style::Posix,
    )
    .unwrap();
    let desired = &plan.input()["desired"];
    assert_eq!(desired["default_project"], "next");
    assert!(desired.get("project_order").is_none());
    assert!(plan.input()["base"].get("project_order").is_none());
    assert_eq!(desired["unknown"], "keep");
    let next = &desired["projects"][0];
    for key in ["id", "name", "color", "sessions", "custom"] {
        assert_eq!(
            next[key], snapshot["organization"]["projects"][0][key],
            "{key}"
        );
    }
    assert_eq!(next["folders"][0], folder);
    assert_eq!(next["folders"].as_array().unwrap().len(), 2);
    assert_eq!(plan.evidence.witnesses.len(), 1);
}

#[test]
fn a_new_project_never_writes_the_existing_shared_order_register() {
    let (snapshot, binding) = fixture();
    let plan = one(&snapshot, binding);
    assert!(plan.changed());
    let input = plan.input();
    for side in ["base", "desired"] {
        assert!(input[side].get("project_order").is_none(), "{side}");
        assert_eq!(input[side]["default_project"], "next");
    }
    assert_eq!(
        input["desired"]["projects"][0],
        input["base"]["projects"][0]
    );
    assert_eq!(input["desired"]["projects"].as_array().unwrap().len(), 2);
    assert_eq!(snapshot["organization"]["project_order"], json!(["next"]));
}

#[test]
fn a_renamed_project_keeps_its_location_without_recreating_the_legacy_name() {
    let (mut snapshot, binding) = fixture();
    let project = &mut snapshot["organization"]["projects"][0];
    project["name"] = json!("Renamed by user");
    project["folders"] = json!([{"id":"existing","machine":"old-alias",
        "machine_id":binding.machine,"path":"/srv/legacy","name":"User label"}]);
    let plan = plan(
        &snapshot,
        binding.clone(),
        "fixture",
        &json!([{"name":"Legacy","dir":"/srv/legacy/"}]),
        Style::Posix,
    )
    .unwrap();
    assert!(!plan.changed());
    assert_eq!(plan.input()["desired"], plan.input()["base"]);
    assert!(plan.evidence.witnesses.is_empty());
    assert_eq!(plan.evidence().problem(), None);
    assert!(plan.evidence().observed(&binding, &snapshot).unwrap());
}

#[test]
fn a_folder_moved_to_another_project_is_not_added_back_to_its_legacy_project() {
    let (mut snapshot, binding) = fixture();
    snapshot["organization"]["projects"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"destination","name":"Destination","sessions":[],
            "folders":[{"id":"moved","machine":"fixture","path":"/srv/moved","name":"Kept label"}]}));
    let plan = plan(
        &snapshot,
        binding.clone(),
        "fixture",
        &json!([{"name":"Next","dir":"/srv/./moved"}]),
        Style::Posix,
    )
    .unwrap();
    assert!(!plan.changed());
    assert_eq!(plan.input()["desired"], plan.input()["base"]);
    assert!(plan.evidence.witnesses.is_empty());
    assert_eq!(plan.evidence().problem(), None);
    assert!(plan.evidence().observed(&binding, &snapshot).unwrap());

    // The same path on another physical computer does not settle this list.
    snapshot["organization"]["projects"][1]["folders"][0]["machine"] = json!("other");
    let other = plan_for_next(&snapshot, binding);
    assert!(other.changed());
    assert_eq!(
        other.input()["desired"]["projects"][0]["folders"][0]["path"],
        "/srv/moved"
    );
}

fn plan_for_next(snapshot: &Value, binding: Binding) -> Plan {
    plan(
        snapshot,
        binding,
        "fixture",
        &json!([{"name":"Next","dir":"/srv/moved"}]),
        Style::Posix,
    )
    .unwrap()
}

#[test]
fn a_project_or_folder_tombstone_is_never_bypassed_with_a_new_identity() {
    for kind in ["project", "folder"] {
        let (mut snapshot, binding) = fixture();
        let key = if kind == "project" {
            json!({"kind":kind,"id":"removed","field":"alive"})
        } else {
            json!({"kind":kind,"id":"removed"})
        };
        snapshot["versions"][key.to_string()] = json!(["original-removal"]);
        let plan = one(&snapshot, binding.clone());
        assert!(!plan.changed());
        assert_eq!(plan.input()["desired"], plan.input()["base"]);
        assert_eq!(plan.evidence().problem(), Some(Code::History));
        assert!(!plan.evidence().observed(&binding, &snapshot).unwrap());
    }
}

#[test]
fn evidence_requires_all_original_additions_and_the_original_scope() {
    let (snapshot, binding) = fixture();
    let plan = one(&snapshot, binding.clone());
    assert!(!plan.evidence().observed(&binding, &snapshot).unwrap());
    let next = observed(&snapshot, &plan);
    assert!(plan.evidence().valid());
    assert!(plan.evidence().observed(&binding, &next).unwrap());
    let mut changed = next.clone();
    changed["organization"]["projects"][1]["folders"][0]["path"] = json!("/srv/user-choice");
    assert!(!plan.evidence().observed(&binding, &changed).unwrap());
    for source in [
        Binding {
            machine: "44444444-2222-4333-8444-555555555555".into(),
            ..binding.clone()
        },
        Binding {
            node: "44444444-2222-4333-8444-555555555555".into(),
            ..binding.clone()
        },
        Binding {
            swarm: "44444444-2222-4333-8444-555555555555".into(),
            ..binding.clone()
        },
        Binding {
            source: "d".repeat(64),
            ..binding.clone()
        },
    ] {
        assert_eq!(plan.evidence().observed(&source, &next), Err(Code::Changed));
    }
    changed = next;
    changed["swarm_id"] = json!("44444444-2222-4333-8444-555555555555");
    assert_eq!(
        plan.evidence().observed(&binding, &changed),
        Err(Code::Changed)
    );
    let encoded = serde_json::to_string(plan.evidence()).unwrap();
    assert!(!encoded.contains("Legacy fixture") && !encoded.contains("/srv/legacy-fixture"));
}

#[test]
fn windows_lists_never_produce_a_shared_proposal() {
    let (snapshot, binding) = fixture();
    for dir in [
        "C:\\Work\\Fixture",
        "c:/work/fixture/",
        "C:relative",
        "/srv/fixture",
    ] {
        assert!(matches!(
            plan(
                &snapshot,
                binding.clone(),
                "fixture",
                &json!([{"name":"Win","dir":dir}]),
                Style::Windows,
            ),
            Err(Code::WindowsUnsupported)
        ));
    }
    assert_eq!(
        Code::WindowsUnsupported.as_str(),
        "legacy_projects_windows_unsupported"
    );
}

#[test]
fn an_uninitialized_catalog_never_plans_or_completes_an_import() {
    let (mut snapshot, binding) = fixture();
    let prepared = one(&snapshot, binding.clone());
    snapshot = observed(&snapshot, &prepared);
    for initialized in [json!(false), Value::Null] {
        snapshot["initialized"] = initialized;
        assert!(matches!(
            plan(
                &snapshot,
                binding.clone(),
                "fixture",
                &json!([]),
                Style::Posix
            ),
            Err(Code::Changed)
        ));
        assert_eq!(
            prepared.evidence().observed(&binding, &snapshot),
            Err(Code::Changed)
        );
    }
}

#[test]
fn the_history_key_bound_refuses_the_whole_history() {
    let (mut snapshot, _) = fixture();
    let versions = snapshot["versions"].as_object_mut().unwrap();
    for n in 0..=MAX_PROJECTS * (FOLDERS + 8) {
        versions.insert(
            json!({"kind":"folder","id":format!("historical-{n}")}).to_string(),
            json!(["original-head"]),
        );
    }
    // Exercise this bound independently of the earlier serialized input cap.
    assert_eq!(
        history(&snapshot, checked_projects(&snapshot).unwrap()),
        Err(Code::Capacity)
    );
}

#[test]
fn a_large_session_catalog_refuses_with_capacity_instead_of_a_partial_plan() {
    let (mut snapshot, binding) = fixture();
    let mut sessions = Vec::new();
    for n in 0..12_000 {
        let session = format!("work-{n:08}");
        sessions.push(json!(format!("{}\n{session}", binding.machine)));
        snapshot["versions"]
            [json!({"kind":"session","machine":binding.machine,"session":session}).to_string()] =
            json!(["f".repeat(64)]);
    }
    snapshot["organization"]["projects"][0]["sessions"] = json!(sessions);
    assert!(serde_json::to_vec(&snapshot).unwrap().len() > MAX_BYTES);
    assert!(matches!(
        plan(
            &snapshot,
            binding,
            "fixture",
            &json!([{"name":"Legacy","dir":"/srv/legacy"}]),
            Style::Posix
        ),
        Err(Code::Capacity)
    ));
    assert_eq!(Code::Capacity.as_str(), "legacy_projects_capacity");
}

#[test]
fn malformed_unknown_history_and_whole_input_caps_fail_closed() {
    let (mut snapshot, binding) = fixture();
    assert!(matches!(
        plan(
            &snapshot,
            binding.clone(),
            "fixture",
            &json!({}),
            Style::Posix
        ),
        Err(Code::Invalid)
    ));
    let mut rows = vec![json!({"src":"ansible"}); MAX_ROWS + 1];
    assert!(matches!(
        plan(
            &snapshot,
            binding.clone(),
            "fixture",
            &json!(rows),
            Style::Posix
        ),
        Err(Code::Capacity)
    ));
    rows = vec![json!({"name":"Next","dir":"/srv/new","ignored":"x".repeat(MAX_BYTES)})];
    assert!(matches!(
        plan(
            &snapshot,
            binding.clone(),
            "fixture",
            &json!(rows),
            Style::Posix
        ),
        Err(Code::Capacity)
    ));
    snapshot["versions"] = Value::Null;
    assert!(matches!(
        plan(&snapshot, binding, "fixture", &json!([]), Style::Posix),
        Err(Code::Invalid)
    ));
}

#[test]
fn malformed_source_or_causal_keys_never_look_like_missing_fields() {
    let (snapshot, binding) = fixture();
    assert!(matches!(
        plan(
            &snapshot,
            binding.clone(),
            "fixture",
            &json!([
                {"name":"Next","dir":"/srv/new","src":{}}
            ]),
            Style::Posix
        ),
        Err(Code::Invalid)
    ));
    for key in [
        json!({"kind":"project","id":"next"}),
        json!({"kind":"future","id":"next"}),
        json!({"kind":"folder","id":"old","future":true}),
    ] {
        let mut changed = snapshot.clone();
        changed["versions"][key.to_string()] = json!(["head"]);
        assert!(matches!(
            plan(
                &changed,
                binding.clone(),
                "fixture",
                &json!([]),
                Style::Posix
            ),
            Err(Code::Invalid)
        ));
    }
    let plan = one(&snapshot, binding);
    let mut proof = plan.evidence().clone();
    proof.witnesses.push(proof.witnesses[0].clone());
    assert!(
        !proof.valid(),
        "duplicate journal witnesses must not be adopted"
    );
}

#[test]
fn qts_local_completion_list_is_read_without_importing_its_values() {
    use crate::settings::{QtSnapshot, qt::QtValue};
    let mut local = fixture().0["organization"].clone();
    local["imported_machines"] = json!(["fixture", "fixture-peer", "fixture"]);
    let mut snapshot = QtSnapshot {
        values: Default::default(),
        source: "fixture",
        sha256: "a".repeat(64),
        modified_at: None,
    };
    snapshot.values.insert(
        super::super::qt_import::KEY.into(),
        QtValue::Bytes(local.to_string().into_bytes()),
    );
    assert_eq!(
        qt_imported_machines(Some(&snapshot)).unwrap(),
        BTreeSet::from(["fixture".into(), "fixture-peer".into()])
    );
    assert!(qt_imported_machines(None).unwrap().is_empty());
    for markers in [
        Value::Null,
        json!({}),
        json!(["fixture", 7]),
        json!(["fixture\npeer"]),
    ] {
        local["imported_machines"] = markers;
        snapshot.values.insert(
            super::super::qt_import::KEY.into(),
            QtValue::Text(local.to_string()),
        );
        assert_eq!(qt_imported_machines(Some(&snapshot)), Err(Code::Invalid));
    }
    local["imported_machines"] =
        json!((0..257).map(|n| format!("fixture-{n}")).collect::<Vec<_>>());
    snapshot.values.insert(
        super::super::qt_import::KEY.into(),
        QtValue::Text(local.to_string()),
    );
    assert_eq!(qt_imported_machines(Some(&snapshot)), Err(Code::Capacity));
}
