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
    assert_eq!(desired["project_order"], json!(["next"]));
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
fn windows_paths_use_the_owning_computer_style_and_deduplicate_case() {
    let (snapshot, binding) = fixture();
    let p = plan(
        &snapshot,
        binding,
        "fixture",
        &json!([
            {"name":"Next","dir":"C:\\Work\\Fixture"},
            {"name":"Next","dir":"c:/work/fixture/"},
            {"name":"Next","dir":"C:relative"}
        ]),
        Style::Windows,
    )
    .unwrap();
    assert_eq!(
        p.input()["desired"]["projects"][0]["folders"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(p.evidence().problem(), Some(Code::InvalidFolders));
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
