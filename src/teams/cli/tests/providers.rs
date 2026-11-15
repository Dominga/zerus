use super::harness::*;
use crate::teams::cli::actor::Caller;
use serde_json::json;

#[test]
fn provider_inspection_is_a_read_and_diagnoses_an_unselected_project() {
    let h = Harness::new();
    h.create();
    let before = h.revision();
    let (reply, exit) = h.cli(
        &Caller::Person,
        "validation-config",
        &json!({"team":TEAM}).to_string(),
    );
    assert_eq!(exit, 0, "{reply}");
    assert_eq!(reply["inspection"]["schema"], 1);
    assert_eq!(reply["inspection"]["valid"], false);
    assert_eq!(
        reply["inspection"]["diagnostics"][0]["code"],
        "project_unavailable"
    );
    assert_eq!(h.revision(), before);
}

#[cfg(unix)]
mod cases {
    use super::*;
    use crate::teams::ops::{self, Operation};
    use std::{fs, path::PathBuf};
    const FILE: &str = "schema = 1\n[[provider]]\nname = 'cargo-test'\nprovider = 'cargo'\nkind = 'test'\nscopes = ['full', 'focused']\nreports = ['junit']\n";
    fn project(h: &Harness) -> PathBuf {
        let path = PathBuf::from(&h.config.home).join("checkout");
        fs::create_dir(&path).unwrap();
        crate::swarm::provider_fixture::seed(&h.config, MACHINE, "example-project", &path);
        path
    }
    fn read(h: &Harness, caller: &Caller, extra: serde_json::Value) -> serde_json::Value {
        let mut input = json!({"team":TEAM});
        input
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let (r, e) = h.cli(caller, "validation-config", &input.to_string());
        assert_eq!(e, 0, "{r}");
        r
    }
    fn selection(reply: &serde_json::Value) -> serde_json::Value {
        let r = &reply["inspection"];
        json!({"source_digest":r["source_digest"],"digest":r["digest"],"override_config":r["override_config"]})
    }
    fn save(h: &Harness, sel: serde_json::Value, caller: &Caller) -> (serde_json::Value, i32) {
        h.cli(caller,"validation-settings",&json!({"team":TEAM,"request_id":uid(0xf000+h.revision() as u32),"expected_revision":h.revision(),"selection":sel}).to_string())
    }
    fn file(root: &std::path::Path, text: &str) {
        fs::create_dir_all(root.join(".zerus")).unwrap();
        fs::write(root.join(".zerus/tests.toml"), text).unwrap();
    }
    #[test]
    fn detection_file_and_override_precedence_never_execute_or_save() {
        let h = Harness::new();
        h.create();
        let root = project(&h);
        let before = h.revision();
        fs::write(root.join("Cargo.toml"), "[package]\nname='example'\n").unwrap();
        let r = read(&h, &Caller::Person, json!({}));
        assert_eq!(r["inspection"]["source"], "detected");
        assert_eq!(r["inspection"]["providers"].as_array().unwrap().len(), 2);
        // The checked-in core/UI fixture is normalized only for the private path/object digests.
        let mut fixture = r.clone();
        fixture["inspection"]["checkout"] = json!("/workspace/example-project");
        fixture["inspection"]["source_digest"] = json!("a".repeat(64));
        fixture["inspection"]["digest"] = json!("b".repeat(64));
        let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("client/core/src/teams/fixtures/providers.json");
        if std::env::var_os("ZERUS_UPDATE_PROVIDER_FIXTURE").is_some() {
            fs::write(
                &fixture_path,
                format!("{}\n", serde_json::to_string_pretty(&fixture).unwrap()),
            )
            .unwrap();
        }
        assert_eq!(
            fixture,
            serde_json::from_slice::<serde_json::Value>(&fs::read(fixture_path).unwrap()).unwrap()
        );
        file(&root, FILE);
        let from_file = read(&h, &Caller::Person, json!({}));
        assert_eq!(from_file["inspection"]["source"], "file");
        assert_eq!(
            from_file["inspection"]["providers"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let mut override_config =
            json!({"schema":1,"provider":from_file["inspection"]["providers"]});
        override_config["provider"][0]["disabled"] = json!(true);
        let checked = read(
            &h,
            &Caller::Person,
            json!({"override_config":override_config}),
        );
        assert_eq!(checked["inspection"]["providers"][0]["disabled"], true);
        assert_eq!(h.revision(), before);
        assert_eq!(
            h.store()
                .db
                .as_ref()
                .unwrap()
                .query_row("SELECT count(*) FROM validation_settings", [], |r| r
                    .get::<_, u64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            read(&h, &Caller::Person, json!({}))["inspection"]["override_config"],
            serde_json::Value::Null
        );
        assert_eq!(save(&h, selection(&checked), &Caller::Person).1, 0);
        assert_eq!(
            read(&h, &Caller::Person, json!({}))["inspection"]["saved_current"],
            true
        );
        assert!(!root.join("executed").exists());
    }
    #[test]
    fn malformed_oversize_unknown_fields_and_unknown_provider_are_diagnostics() {
        let h = Harness::new();
        h.create();
        let root = project(&h);
        for text in [
            "invalid [".into(),
            format!("{FILE}#{}", "x".repeat(65537)),
            FILE.replace("'cargo'", "'shell'"),
            FILE.replace("schema = 1", "schema = 2"),
            format!("{FILE}\nprogram = 'touch executed'"),
            FILE.replace("cargo-test", "../escape"),
        ] {
            file(&root, &text);
            let r = read(&h, &Caller::Person, json!({}));
            assert_eq!(r["inspection"]["valid"], false, "{r}");
            assert!(!r["inspection"]["diagnostics"]
                .as_array()
                .unwrap()
                .is_empty());
            let (_, e) = save(&h, selection(&r), &Caller::Person);
            assert_eq!(e, 1);
        }
        assert!(!root.join("executed").exists());
    }
    #[test]
    fn symlinks_in_both_config_components_and_detection_are_refused() {
        use std::os::unix::fs::symlink;
        let h = Harness::new();
        h.create();
        let root = project(&h);
        file(&root, FILE);
        let target = root.join("config-copy");
        fs::rename(root.join(".zerus/tests.toml"), &target).unwrap();
        symlink(&target, root.join(".zerus/tests.toml")).unwrap();
        assert_eq!(
            read(&h, &Caller::Person, json!({}))["inspection"]["valid"],
            false
        );
        fs::remove_file(root.join(".zerus/tests.toml")).unwrap();
        fs::remove_dir(root.join(".zerus")).unwrap();
        let dir = root.join("other");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("tests.toml"), FILE).unwrap();
        symlink(&dir, root.join(".zerus")).unwrap();
        assert_eq!(
            read(&h, &Caller::Person, json!({}))["inspection"]["valid"],
            false
        );
        fs::remove_file(root.join(".zerus")).unwrap();
        symlink(&target, root.join("Cargo.toml")).unwrap();
        assert_eq!(
            read(&h, &Caller::Person, json!({}))["inspection"]["valid"],
            false
        );
    }
    #[test]
    fn changed_config_checkout_or_native_project_choice_requires_a_new_review() {
        let h = Harness::new();
        h.create();
        let root = project(&h);
        file(&root, FILE);
        let before = read(&h, &Caller::Person, json!({}));
        let sel = selection(&before);
        file(&root, &FILE.replace("'full', 'focused'", "'full'"));
        let (r, e) = save(&h, sel.clone(), &Caller::Person);
        assert_eq!(
            (e, r["error"]["code"].as_str()),
            (1, Some("config_changed"))
        );
        file(&root, FILE);
        fs::rename(&root, root.with_extension("old")).unwrap();
        fs::create_dir(&root).unwrap();
        file(&root, FILE);
        let (r, e) = save(&h, sel.clone(), &Caller::Person);
        assert_eq!(
            (e, r["error"]["code"].as_str()),
            (1, Some("config_changed"))
        );
        let other = root.with_extension("selected");
        fs::create_dir(&other).unwrap();
        file(&other, FILE);
        crate::swarm::provider_fixture::seed(&h.config, MACHINE, "example-project", &other);
        let (r, e) = save(&h, sel, &Caller::Person);
        assert_eq!(
            (e, r["error"]["code"].as_str()),
            (1, Some("config_changed"))
        );
    }
    #[test]
    fn person_lead_members_other_and_service_have_exact_authority() {
        let h = Harness::new();
        h.create();
        let root = project(&h);
        file(&root, FILE);
        h.bind("lead", LEAD_SESSION);
        h.bind("m1", M1_SESSION);
        let lead = Caller::Session(key(LEAD_SESSION));
        let member = Caller::Session(key(M1_SESSION));
        let other = Caller::Session(key(OTHER_SESSION));
        let sel = selection(&read(&h, &lead, json!({})));
        for actor in [&member, &other] {
            let (r, e) = h.cli(
                actor,
                "validation-config",
                &json!({"team":TEAM}).to_string(),
            );
            assert_eq!((e, r["error"]["code"].as_str()), (1, Some("not_allowed")));
        }
        for caller in [&lead, &member, &other] {
            let (r, e) = save(&h, sel.clone(), caller);
            assert_eq!((e, r["error"]["code"].as_str()), (1, Some("not_allowed")));
        }
        let op = Operation::SetValidationConfig {
            selection: serde_json::from_value(sel.clone()).unwrap(),
        };
        let req = h.request(op);
        let r = ops::apply_with_config(&mut h.store(), &service(), &req, NOW, Some(&h.config));
        assert_eq!(r.unwrap_err().code, "not_allowed");
        assert_eq!(save(&h, sel, &Caller::Person).1, 0);
    }
    #[test]
    fn selection_is_revisioned_journaled_replayed_and_preserved_by_old_settings() {
        let h = Harness::new();
        h.create();
        let root = project(&h);
        file(&root, FILE);
        let sel = selection(&read(&h, &Caller::Person, json!({})));
        let input = json!({"team":TEAM,"request_id":uid(0xe001),"expected_revision":h.revision(),"selection":sel});
        let before = h.revision();
        let (r, e) = h.cli(&Caller::Person, "validation-settings", &input.to_string());
        assert_eq!(e, 0, "{r}");
        assert_eq!(h.revision(), before + 1);
        file(&root, &FILE.replace("cargo-test", "changed"));
        let (replay, e) = h.cli(&Caller::Person, "validation-settings", &input.to_string());
        assert_eq!(e, 0);
        assert_eq!(r, replay);
        assert_eq!(h.revision(), before + 1);
        let mut stale = input.clone();
        stale["request_id"] = json!(uid(0xe002));
        let (r, e) = h.cli(&Caller::Person, "validation-settings", &stale.to_string());
        assert_eq!(
            (e, r["error"]["code"].as_str()),
            (1, Some("stale_revision"))
        );
        let saved = crate::teams::providers::saved(h.store().db.as_ref().unwrap(), TEAM).unwrap();
        let mut settings = h.store().team(TEAM).unwrap().unwrap().settings;
        settings.timers.start_timeout_seconds += 1;
        h.apply(&person(), Operation::ChangeSettings { settings });
        assert_eq!(
            saved,
            crate::teams::providers::saved(h.store().db.as_ref().unwrap(), TEAM).unwrap()
        );
        let team = h.store().team(TEAM).unwrap().unwrap();
        let old: zerus_team_plan::model::Team =
            serde_json::from_value(serde_json::to_value(team).unwrap()).unwrap();
        assert_eq!(old.id, TEAM);
        assert_eq!(
            h.store()
                .db
                .as_ref()
                .unwrap()
                .query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
                .unwrap(),
            2
        );
        assert_eq!(
            h.store()
                .db
                .as_ref()
                .unwrap()
                .query_row(
                    "SELECT count(*) FROM journal WHERE team=?1 AND kind='set_validation_config'",
                    [TEAM],
                    |r| r.get::<_, u32>(0)
                )
                .unwrap(),
            1
        );
        assert_eq!(
            read(&h, &Caller::Person, json!({}))["inspection"]["saved_current"],
            false
        );
    }
    #[test]
    fn a_changed_computer_or_foreign_state_never_selects_the_project() {
        let h = Harness::new();
        h.create();
        let root = project(&h);
        file(&root, FILE);
        let sel = selection(&read(&h, &Caller::Person, json!({})));
        let request = h.request(Operation::SetValidationConfig {
            selection: serde_json::from_value(sel).unwrap(),
        });
        let mut foreign = h.config.clone();
        foreign.state = PathBuf::from(&h.config.home);
        assert!(
            ops::apply_with_config(&mut h.store(), &person(), &request, NOW, Some(&foreign))
                .is_err()
        );
        crate::swarm::provider_fixture::change_home(&h.config, TEAM_B);
        let r = read(&h, &Caller::Person, json!({}));
        assert_eq!(r["inspection"]["valid"], false);
        assert_eq!(
            r["inspection"]["diagnostics"][0]["code"],
            "project_unavailable"
        );
    }
    #[test]
    fn unknown_read_fields_and_oversized_requests_are_refused_before_inspection() {
        let h = Harness::new();
        h.create();
        let (r, e) = h.cli(
            &Caller::Person,
            "validation-config",
            &json!({"team":TEAM,"program":"touch executed"}).to_string(),
        );
        assert_eq!((e, r["error"]["code"].as_str()), (1, Some("unknown_field")));
        let (r, e) = h.cli(&Caller::Person, "validation-config", &" ".repeat(262145));
        assert_eq!((e, r["error"]["code"].as_str()), (1, Some("over_cap")));
    }
}
