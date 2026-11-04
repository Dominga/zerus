use super::*;
use crate::swarm::legacy_projects::tests::{fixture, observed, one};

#[test]
fn an_unknown_apply_remains_pending_and_a_second_prepare_never_readmits_it() {
    let (snapshot, binding) = fixture();
    let plan = one(&snapshot, binding.clone());
    let mut extra = Map::new();
    assert!(prepare(&mut extra, &plan).unwrap());
    let saved = extra.clone();
    assert!(!observe(&mut extra, &binding, &snapshot).unwrap());
    assert!(!prepare(&mut extra, &plan).unwrap());
    assert_eq!(
        extra, saved,
        "no retry authority or rewritten original proof"
    );
    assert_eq!(
        read(&extra, &binding.machine).unwrap().unwrap().code(),
        Some(Code::Pending)
    );
    assert!(observe(&mut extra, &binding, &observed(&snapshot, &plan)).unwrap());
    assert!(read(&extra, &binding.machine).unwrap().unwrap().complete());
    let mut moved = binding.clone();
    moved.source = "d".repeat(64);
    let complete = extra.clone();
    assert!(!observe(&mut extra, &moved, &json!({})).unwrap());
    assert_eq!(extra, complete, "completed records are not re-observed");
    let plan = one(&snapshot, moved);
    assert!(
        !prepare(&mut extra, &plan).unwrap(),
        "one time per physical machine, not alias"
    );
}

#[test]
fn a_replaced_pending_scope_refuses_and_malformed_or_future_metadata_is_kept() {
    let (snapshot, binding) = fixture();
    let plan = one(&snapshot, binding.clone());
    let mut extra = Map::new();
    prepare(&mut extra, &plan).unwrap();
    let mut changed = binding;
    changed.source = "e".repeat(64);
    assert_eq!(
        prepare(&mut extra, &one(&snapshot, changed)),
        Err(Code::Changed)
    );
    for bad in [
        json!({"version":2,"records":{}}),
        json!({"version":1,"records":[],"future":true}),
    ] {
        extra.insert(KEY.into(), bad);
        let original = extra.clone();
        assert_eq!(prepare(&mut extra, &plan), Err(Code::Invalid));
        assert_eq!(extra, original);
        assert_eq!(report(&extra)["reasons"], json!([Code::Invalid.as_str()]));
    }
}

#[test]
fn a_record_cannot_move_to_another_machine_or_complete_with_a_problem() {
    let (snapshot, binding) = fixture();
    let plan = one(&snapshot, binding.clone());
    let mut good = Map::new();
    prepare(&mut good, &plan).unwrap();
    for changed_machine in [true, false] {
        let mut extra = good.clone();
        if changed_machine {
            let records = extra.get_mut(KEY).unwrap()["records"]
                .as_object_mut()
                .unwrap();
            let record = records.remove(&binding.machine).unwrap();
            records.insert("44444444-2222-4333-8444-555555555555".into(), record);
        } else {
            let record = &mut extra.get_mut(KEY).unwrap()["records"][&binding.machine];
            record["complete"] = json!(true);
            record["evidence"]["problem"] = json!("history");
        }
        let original = extra.clone();
        assert_eq!(read(&extra, &binding.machine), Err(Code::Invalid));
        assert_eq!(prepare(&mut extra, &plan), Err(Code::Invalid));
        assert_eq!(observe(&mut extra, &binding, &snapshot), Err(Code::Invalid));
        assert_eq!(extra, original);
    }
}

#[test]
fn reports_use_one_shape_and_distinguish_empty_waiting_blocked_and_imported() {
    let (mut snapshot, binding) = fixture();
    let mut extra = Map::new();
    assert_eq!(
        report(&extra),
        json!({"family":"legacy-projects","state":"empty","keys":[],"reasons":[]})
    );
    let plan = one(&snapshot, binding.clone());
    prepare(&mut extra, &plan).unwrap();
    assert_eq!(report(&extra)["state"], "waiting");
    assert_eq!(report(&extra)["reasons"], json!([Code::Pending.as_str()]));
    observe(&mut extra, &binding, &observed(&snapshot, &plan)).unwrap();
    assert_eq!(report(&extra)["state"], "imported");
    assert_eq!(report(&extra)["reasons"], json!([]));
    snapshot["versions"][json!({"kind":"folder","id":"removed"}).to_string()] = json!(["head"]);
    extra.clear();
    prepare(&mut extra, &one(&snapshot, binding)).unwrap();
    assert_eq!(report(&extra)["state"], "blocked");
    assert_eq!(report(&extra)["reasons"], json!([Code::History.as_str()]));
    for code in [Code::Invalid, Code::Capacity, Code::WindowsUnsupported] {
        assert_eq!(
            unavailable(code),
            json!({"family":"legacy-projects","state":"blocked",
            "keys":["projects","folders"],"reasons":[code.as_str()]})
        );
    }
}

#[test]
fn capacity_never_evicts_completion_or_pending_records_and_report_has_no_values() {
    let (snapshot, mut binding) = fixture();
    let mut extra = Map::new();
    for n in 1..=MAX_MACHINES {
        binding.machine = uuid::Uuid::from_u128(n as u128).to_string();
        prepare(&mut extra, &one(&snapshot, binding.clone())).unwrap();
    }
    let original = extra.clone();
    binding.machine = uuid::Uuid::from_u128(999).to_string();
    assert_eq!(
        prepare(&mut extra, &one(&snapshot, binding)),
        Err(Code::Capacity)
    );
    assert_eq!(extra, original);
    let encoded = report(&extra).to_string();
    assert_eq!(report(&extra)["keys"], json!(["projects", "folders"]));
    for secret in [
        "Legacy fixture",
        "/srv/legacy-fixture",
        "fixture",
        "22222222-2222-4333-8444-555555555555",
    ] {
        assert!(!encoded.contains(secret), "{encoded}");
    }
}

#[test]
fn settings_reservation_survives_reopen_and_never_changes_other_families() {
    let dir = std::env::temp_dir().join(format!(
        "zerus-legacy-project-record-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir(&dir).unwrap();
    struct Keep(std::path::PathBuf);
    impl Drop for Keep {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _keep = Keep(dir.clone());
    let path = dir.join("settings.json");
    std::fs::write(
        &path,
        json!({"version":1,"values":{"workspace/inspectorOpen":true},"future":{"keep":7}})
            .to_string(),
    )
    .unwrap();
    let (snapshot, binding) = fixture();
    let plan = one(&snapshot, binding.clone());
    {
        let store = crate::settings::Settings::open(path.clone(), None);
        assert!(store.prepare_legacy_project_import(&plan).unwrap());
    }
    let store = crate::settings::Settings::open(path.clone(), None);
    assert!(!store.prepare_legacy_project_import(&plan).unwrap());
    assert!(!store
        .legacy_project_import(&binding.machine)
        .unwrap()
        .unwrap()
        .complete());
    assert!(store
        .observe_legacy_project_import(&binding, &observed(&snapshot, &plan))
        .unwrap());
    drop(store);
    let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved["future"], json!({"keep":7}));
    assert_eq!(saved["values"]["workspace/inspectorOpen"], true);
    assert!(!saved.to_string().contains("Legacy fixture"));
    assert!(!saved.to_string().contains("/srv/legacy-fixture"));
    let store = crate::settings::Settings::open(path, None);
    assert!(store
        .legacy_project_import(&binding.machine)
        .unwrap()
        .unwrap()
        .complete());
}

#[test]
fn an_incomplete_family_never_becomes_complete_after_its_valid_prefix_arrives() {
    let (snapshot, binding) = fixture();
    let plan = crate::swarm::legacy_projects::plan(
        &snapshot,
        binding.clone(),
        "fixture",
        &json!([
            {"name":"Valid","dir":"/srv/valid"}, {"name":"Invalid","dir":"relative"}
        ]),
        crate::paths::Style::Posix,
    )
    .unwrap();
    let mut extra = Map::new();
    assert!(prepare(&mut extra, &plan).unwrap());
    assert!(!observe(&mut extra, &binding, &observed(&snapshot, &plan)).unwrap());
    assert_eq!(
        read(&extra, &binding.machine).unwrap().unwrap().code(),
        Some(Code::InvalidFolders)
    );
    assert_eq!(
        report(&extra)["reasons"],
        json!(["legacy_projects_invalid_folders"])
    );
}

#[test]
fn the_encoded_family_bound_refuses_the_whole_new_record() {
    let (snapshot, mut binding) = fixture();
    let rows = json!((0..crate::swarm::FOLDERS)
        .map(|i| json!({"name":"Next","dir":format!("/srv/fixture-{i}")}))
        .collect::<Vec<_>>());
    let mut extra = Map::new();
    let mut bounded = false;
    for n in 1..=MAX_MACHINES {
        binding.machine = uuid::Uuid::from_u128(n as u128).to_string();
        let plan = crate::swarm::legacy_projects::plan(
            &snapshot,
            binding.clone(),
            "fixture",
            &rows,
            crate::paths::Style::Posix,
        )
        .unwrap();
        let original = extra.clone();
        match prepare(&mut extra, &plan) {
            Ok(true) => (),
            Err(Code::Capacity) => {
                assert!(
                    n < MAX_MACHINES,
                    "exercise the byte bound before the row bound"
                );
                assert_eq!(extra, original);
                bounded = true;
                break;
            }
            other => panic!("unexpected admission: {other:?}"),
        }
    }
    assert!(bounded);
}
