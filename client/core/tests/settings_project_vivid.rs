//! First-G3 fill preferences through synthetic Qt sources and Settings only.
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
use zerus_client_core::settings::{QtStore, Settings};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "zerus-project-fill-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn settings(&self, values: Value) -> PathBuf {
        let path = self.0.join("settings.json");
        fs::write(
            &path,
            serde_json::to_vec(&json!({"version":1,"values":values,
            "seed":{"at":1,"source":"qt-ini","keys":["workspace/theme"]},
            "future_extension":{"keep":true}}))
            .unwrap(),
        )
        .unwrap();
        path
    }
    fn qt(&self, organization: &Value) -> (QtStore, Vec<u8>) {
        let path = self.0.join("synthetic.plist");
        let data = STANDARD.encode(serde_json::to_vec(organization).unwrap());
        let bytes = format!("<plist version=\"1.0\"><dict><key>workspace.organization</key><data>{data}</data><key>workspace.allSwarmProjects</key><true/></dict></plist>").into_bytes();
        fs::write(&path, &bytes).unwrap();
        (QtStore::Plist { path, domain: None }, bytes)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn saved(path: &PathBuf, settings: Settings) -> Value {
    assert!(settings.error().is_none());
    assert!(settings.flush_within(Duration::from_secs(5)));
    drop(settings);
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

#[test]
fn first_g3_project_fill_uses_exact_ids_next_false_wins_and_rollback_never_reseeds() {
    let scratch = Scratch::new();
    let organization = json!({"version":2,"projects":[
        {"id":"alpha","name":"Same name","vivid":true,"private_future":{"keep":1}},
        {"id":"beta","name":"Same name","vivid":false},
        {"id":"nested/project","name":"Nested","vivid":true},
        {"id":"project\\id","name":"Backslash","vivid":false},
        {"id":"no-flag","name":"Default"}],"run_groups":{"keep":"original"}});
    let (qt, bytes) = scratch.qt(&organization);
    let mtime = fs::metadata(qt.path()).unwrap().modified().unwrap();
    let path = scratch.settings(json!({"projects/vivid/alpha":false,"projects/vivid/beta":true}));
    let settings = Settings::open(path.clone(), Some(qt.clone()));
    assert!(!settings.flag("projects/vivid/alpha"));
    assert!(settings.flag("projects/vivid/beta"));
    assert!(settings.flag("projects/vivid/nested/project"));
    assert!(settings.contains("projects/vivid/project\\id"));
    assert!(!settings.flag("projects/vivid/project\\id"));
    assert!(
        !settings.contains("projects/vivid/no-flag"),
        "no invented default"
    );
    let value = saved(&path, settings);
    let record = &value["g3"]["families"]["project-vivid"];
    assert_eq!(record["family"], "project-vivid");
    assert_eq!(record["version"], 1);
    assert_eq!(record["schema"], 1);
    assert_eq!(record["source"], "qt-plist");
    assert_eq!(record["source_path"], qt.path().to_string_lossy().as_ref());
    assert_eq!(record["keys"], json!(["workspace/organization"]));
    assert_eq!(record["sha256"], zerus_desktop_migration::sha256(&bytes));
    assert_eq!(record["source_mtime_after_switch"], true);
    assert!(record["at"].as_u64().unwrap() > 1);
    assert_eq!(value["future_extension"], json!({"keep":true}));
    assert!(value["values"].get("workspace/organization").is_none());
    assert!(!value.to_string().contains("private_future"));
    assert_eq!(fs::read(qt.path()).unwrap(), bytes);
    assert_eq!(fs::metadata(qt.path()).unwrap().modified().unwrap(), mtime);
    // A rollback can change Qt. A second switch does not even read it when
    // all known Settings families are handled, and never imports new IDs.
    fs::write(qt.path(), b"unreadable rollback copy").unwrap();
    let again = Settings::open(path.clone(), Some(qt));
    assert_eq!(saved(&path, again), value);
}

#[test]
fn legacy_groups_and_pins_use_the_existing_normalizer_and_ini_reader() {
    let scratch = Scratch::new();
    let organization = json!({"groups":[
        {"id":"alpha","name":"Alpha","vivid":true},
        {"id":"pinned","name":"Pinned","vivid":true,"sessions":["peer\ncodex/example/test"]}],
        "default_project":"alpha","before_pin":{"peer\ncodex/example/test":"alpha"}});
    let text = organization
        .to_string()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let original = format!("[workspace]\norganization=\"@ByteArray({text})\"\n").into_bytes();
    let qt = scratch.0.join("synthetic.conf");
    fs::write(&qt, &original).unwrap();
    let path = scratch.settings(json!({}));
    let settings = Settings::open(path.clone(), Some(QtStore::Ini(qt.clone())));
    assert!(settings.flag("projects/vivid/alpha"));
    assert!(!settings.contains("projects/vivid/pinned"));
    let value = saved(&path, settings);
    assert_eq!(value["g3"]["families"]["project-vivid"]["source"], "qt-ini");
    assert_eq!(
        value["g3"]["families"]["project-vivid"]["sha256"],
        zerus_desktop_migration::sha256(&original)
    );
    assert_eq!(fs::read(qt).unwrap(), original);
}

#[test]
fn malformed_or_ambiguous_fill_family_retries_with_no_partial_import() {
    let good = json!({"id":"alpha","name":"Alpha","vivid":true});
    for organization in [
        json!({"projects":[good,{"id":"beta","name":"Beta","vivid":"private-invalid"}]}),
        json!({"projects":[good,good]}),
        json!({"projects":[good,{"id":"bad\nidentity","name":"Bad","vivid":true}]}),
        json!({"projects":[good,{"id":"a".repeat(129),"name":"Bad","vivid":true}]}),
        json!({"projects":[good,{"id":"beta","name":"","vivid":true}]}),
        json!({"projects":null,"groups":[good]}),
        json!({"version":3,"projects":[good]}),
        json!({"projects":vec![good.clone();501]}),
    ] {
        let scratch = Scratch::new();
        let (qt, bytes) = scratch.qt(&organization);
        let path = scratch.settings(json!({}));
        let settings = Settings::open(path.clone(), Some(qt.clone()));
        assert!(!settings.contains("projects/vivid/alpha"));
        assert!(
            settings.flag("workspace/allSwarmProjects"),
            "independent family still commits"
        );
        let value = saved(&path, settings);
        assert!(value["g3"]["families"].get("project-vivid").is_none());
        assert_eq!(
            value["g3"]["quarantine"]["project-vivid"]["preserved_in"],
            "source"
        );
        assert!(!value.to_string().contains("private-invalid"));
        assert_eq!(fs::read(qt.path()).unwrap(), bytes);
        let (_, repaired) = scratch.qt(&json!({"projects":[good]}));
        let settings = Settings::open(path.clone(), Some(qt));
        assert!(settings.flag("projects/vivid/alpha"));
        let value = saved(&path, settings);
        assert_eq!(
            value["g3"]["families"]["project-vivid"]["sha256"],
            zerus_desktop_migration::sha256(&repaired)
        );
    }
}

#[test]
fn unused_bad_flag_and_old_seed_keys_cannot_override_an_existing_choice() {
    let scratch = Scratch::new();
    let (qt, _) = scratch.qt(&json!({"projects":[
        {"id":"alpha","name":"Alpha","vivid":"not-used"},
        {"id":"beta","name":"Beta","vivid":true}]}));
    let path = scratch.settings(json!({"projects/vivid/alpha":false}));
    let mut original: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    original["seed"]["keys"] = json!(["projects/vivid/beta"]);
    fs::write(&path, original.to_string()).unwrap();
    let settings = Settings::open(path.clone(), Some(qt));
    assert!(!settings.flag("projects/vivid/alpha"));
    assert!(!settings.contains("projects/vivid/beta"));
    let value = saved(&path, settings);
    assert!(value["g3"]["families"].get("project-vivid").is_some());
}
