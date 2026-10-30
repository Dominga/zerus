//! Explicit real-CLI qualification, only synthetic Qt and isolated HGS stores.
#![cfg(unix)]
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use zerus_client_core::{
    settings::{OrganizationImport, QtStore, Settings},
    swarm::qt_import,
};

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn invoke(hgs: &Path, env: &[(&str, &str)], args: &[&str], input: Value) -> Value {
    let mut child = Command::new(hgs)
        .args(args)
        .envs(env.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
fn project(id: &str, name: &str) -> Value {
    json!({"id":id,"name":name,"color":"#123456","sessions":[],"folders":[]})
}

#[test]
#[ignore = "requires HGS_ORGANIZATION_TEST_BIN selecting the locally built G3 CLI"]
fn real_cli_recovers_missing_catalogs_and_preserves_existing_fields_and_tombstones() {
    let hgs = PathBuf::from(
        std::env::var_os("HGS_ORGANIZATION_TEST_BIN").expect("select the built isolated test CLI"),
    );
    assert!(hgs.is_absolute());
    for (existing, legacy) in [(false, false), (true, false), (false, true), (true, true)] {
        let root = std::env::temp_dir().join(format!(
            "zerus-g3-organization-native-{}-{existing}-{legacy}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let scratch = Scratch(root);
        let config = scratch.0.join("config");
        let state = scratch.0.join("state");
        let home = scratch.0.join("home");
        fs::create_dir(&config).unwrap();
        fs::create_dir(&home).unwrap();
        let env = [
            ("HGS_CONFIG_DIR", config.to_str().unwrap()),
            ("HGS_STATE_DIR", state.to_str().unwrap()),
            ("HGS_SELF", "example-local"),
            ("HGS_PEERS", ""),
            ("HOME", home.to_str().unwrap()),
        ];
        let catalog = config.join("swarm/catalog.json");
        let mut before = Value::Null;
        let node = if existing {
            let original = invoke(
                &hgs,
                &env,
                &["swarm", "initialize"],
                json!({"projects":[project("alpha","Next value")]}),
            );
            let mut desired = original["organization"].clone();
            desired["projects"] = json!([]);
            invoke(
                &hgs,
                &env,
                &["swarm", "apply"],
                json!({"base":original["organization"],"desired":desired,"versions":original["versions"]}),
            );
            before = serde_json::from_slice(&fs::read(&catalog).unwrap()).unwrap();
            before["imported"] = json!(false);
            fs::write(&catalog, serde_json::to_vec(&before).unwrap()).unwrap();
            before["node_id"].as_str().unwrap().to_owned()
        } else {
            String::new()
        };
        let qt = scratch.0.join("synthetic-qt.plist");
        let mut organization = json!({"version":2,"projects":[project("alpha","Qt old value"),project("beta","Recovered Qt project")],"future_private":"retain"});
        if legacy {
            let mut projects = organization
                .as_object_mut()
                .unwrap()
                .remove("projects")
                .unwrap();
            projects.as_array_mut().unwrap().push(
                json!({"id":"pinned","name":"Pinned","sessions":["peer\ncodex/example/restored"]}),
            );
            organization["groups"] = projects;
            organization["default_project"] = json!("alpha");
            organization["before_pin"] = json!({"peer\ncodex/example/restored":"beta"});
        }
        let data = STANDARD.encode(serde_json::to_vec(&organization).unwrap());
        let original=format!("<plist version=\"1.0\"><dict><key>workspace.organization</key><data>{data}</data></dict></plist>").into_bytes();
        fs::write(&qt, &original).unwrap();
        let mtime = fs::metadata(&qt).unwrap().modified().unwrap();
        let settings_file = scratch.0.join("next/settings.json");
        let settings = Settings::open(settings_file.clone(), None);
        qt_import::recover(
            &hgs,
            &env,
            &QtStore::Plist {
                path: qt.clone(),
                domain: None,
            },
            &settings,
            &node,
        )
        .unwrap();
        assert_eq!(
            settings.organization_import().unwrap(),
            OrganizationImport::Handled
        );
        let after: Value = serde_json::from_slice(&fs::read(&catalog).unwrap()).unwrap();
        assert_eq!(after["imported"], true);
        let ops = after["catalog"]["operations"].as_object().unwrap();
        if existing {
            for (id, operation) in before["catalog"]["operations"].as_object().unwrap() {
                assert_eq!(ops[id], *operation);
            }
            assert!(!ops.values().any(|op| op["key"]["id"] == "alpha"
                && op["key"]["field"] == "name"
                && op["value"] == "Qt old value"));
        }
        assert!(ops.values().any(|op| op["key"]["id"] == "beta"
            && op["key"]["field"] == "name"
            && op["value"] == "Recovered Qt project"));
        if legacy {
            let assignment = ops
                .values()
                .find(|op| op["key"]["session"] == "codex/example/restored")
                .unwrap();
            assert_eq!(assignment["value"], "beta");
            assert!(ops
                .values()
                .any(|op| op["key"]["id"] == assignment["key"]["machine"]
                    && op["key"]["field"] == "name"
                    && op["value"] == "peer"));
            assert!(!ops.values().any(|op| op["key"]["id"] == "pinned"));
        }
        assert_eq!(fs::read(&qt).unwrap(), original);
        assert_eq!(fs::metadata(&qt).unwrap().modified().unwrap(), mtime);
        let saved: Value = serde_json::from_slice(&fs::read(&settings_file).unwrap()).unwrap();
        assert_eq!(
            saved["g3"]["families"]["shared-organization"]["sha256"],
            zerus_desktop_migration::sha256(&original)
        );
        assert!(
            !config.join("swarm/worker.json").exists(),
            "no swarm service was supervised"
        );
        assert!(
            !state.join("desktop").exists(),
            "no desktop ownership or handoff files"
        );
        let status = invoke(
            &hgs,
            &env,
            &["swarm", "initialize", "--g3-status", "--json"],
            json!({"version":1,"expected_node_id":after["node_id"]}),
        );
        assert_eq!(status["gui_import_pending"], false);
        let unchanged = fs::read(&catalog).unwrap();
        fs::write(&qt, b"synthetic future/unreadable Qt store").unwrap();
        qt_import::recover(
            &scratch.0.join("must-not-launch"),
            &env,
            &QtStore::Ini(qt),
            &settings,
            after["node_id"].as_str().unwrap(),
        )
        .unwrap();
        assert_eq!(
            fs::read(&catalog).unwrap(),
            unchanged,
            "a later switch never re-seeds a handled family"
        );
    }
}
