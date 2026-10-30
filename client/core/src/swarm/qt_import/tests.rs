use super::*;
use crate::settings::qt::QtValue;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

const NODE: &str = "11111111-1111-4111-8111-111111111111";
const OTHER: &str = "22222222-2222-4222-8222-222222222222";
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "zerus-qt-organization-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn source(&self) -> QtStore {
        QtStore::Plist {
            domain: None,
            path: self.0.join("synthetic-qt.plist"),
        }
    }
    fn write(&self, value: &Value) {
        let data = STANDARD.encode(serde_json::to_vec(value).unwrap());
        fs::write(
            self.source().path(),
            format!(
                "<plist version=\"1.0\"><dict><key>workspace.organization</key><data>{data}</data></dict></plist>"
            ),
        )
        .unwrap();
    }
    fn settings(&self) -> Settings {
        Settings::open(self.0.join("settings.json"), None)
    }
    fn saved(&self) -> Value {
        serde_json::from_slice(&fs::read(self.0.join("settings.json")).unwrap()).unwrap()
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn organization() -> Value {
    json!({"version":2,"default_project":"alpha","runs":{"local\nrun":"local\ncodex/example/work"},
        "run_groups":{"local\nrun":"alpha"},"imported_machines":["local"],
        "unassigned_sessions":["local\ncodex/example/implicit"],"future_private":"untouched",
        "projects":[{"id":"alpha","name":"Alpha","color":"#123456","collapsed":true,"vivid":true,
            "sessions":["local\ncodex/example/work","local\narchive\noriginal-archive","local\ncodex/example/implicit"],
            "folders":[{"id":"posix","machine":"local","path":"/srv/example","name":"Source","machine_id":NODE,"private":"retain"},
                {"id":"windows","machine":"local","path":"C:/example","name":"Local only","machine_id":NODE},
                {"id":"peer","machine":"peer","path":"//server/share/example","name":"Peer POSIX","machine_id":OTHER}]}]})
}
fn snapshot(value: &Value) -> QtSnapshot {
    QtSnapshot {
        values: [(
            KEY.into(),
            QtValue::Bytes(serde_json::to_vec(value).unwrap()),
        )]
        .into(),
        source: "qt-plist",
        sha256: "a".repeat(64),
        modified_at: Some(2),
    }
}
fn status(initialized: bool, pending: bool) -> Value {
    json!({"version":1,"node_id":NODE,"initialized":initialized,"gui_import_pending":pending})
}
fn receipt(request: &Value) -> Value {
    json!({"schema":1,"node_id":NODE,"initialized":true,"organization":request["organization"],
    "g3_import":{"version":1,"source_sha256":request["source_sha256"]}})
}

#[test]
fn only_shared_fields_explicit_members_and_nonlocal_windows_folders_cross_the_boundary() {
    let snapshot = snapshot(&organization());
    let before = snapshot.values.clone();
    let shared = shared(Some(&snapshot), NODE).unwrap();
    assert_eq!(shared["project_order"], json!(["alpha"]));
    assert_eq!(
        shared["projects"][0]["sessions"],
        json!([
            "local\ncodex/example/work",
            "local\narchive\noriginal-archive"
        ])
    );
    let folders = shared["projects"][0]["folders"].as_array().unwrap();
    assert_eq!(
        folders
            .iter()
            .map(|f| f["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["posix", "peer"]
    );
    assert!(shared.get("runs").is_none() && shared.get("future_private").is_none());
    assert!(shared["projects"][0].get("collapsed").is_none());
    assert!(folders[0].get("private").is_none());
    assert_eq!(snapshot.values, before, "source values never change");
}

#[test]
fn unsupported_or_ambiguous_sources_never_become_an_empty_success() {
    let mut future = organization();
    future["version"] = json!(99);
    let mut duplicate = organization();
    let project = duplicate["projects"][0].clone();
    duplicate["projects"].as_array_mut().unwrap().push(project);
    let mut bad_member = organization();
    bad_member["projects"][0]["sessions"] = json!(["missing host identity"]);
    let mut legacy = organization();
    legacy["groups"] = legacy["projects"].take();
    let mut oversized = organization();
    oversized["projects"][0]["folders"] = json!((0..=FOLDERS)
        .map(|n| json!({"id":format!("f{n}")}))
        .collect::<Vec<_>>());
    for value in [
        future,
        duplicate,
        bad_member,
        legacy,
        oversized,
        json!({}),
        json!([]),
    ] {
        assert_eq!(
            shared(Some(&snapshot(&value)), NODE).unwrap_err(),
            SOURCE_FAILED
        );
    }
    let empty = QtSnapshot {
        values: Default::default(),
        source: "qt-ini",
        sha256: "a".repeat(64),
        modified_at: None,
    };
    assert_eq!(shared(Some(&empty), NODE).unwrap_err(), SOURCE_FAILED);
    assert_eq!(shared(None, NODE).unwrap()["projects"][0]["id"], INBOX);
}

#[test]
fn import_is_reserved_durably_before_the_call_and_completed_in_the_existing_settings_journal() {
    let scratch = Scratch::new();
    scratch.write(&organization());
    fs::write(scratch.0.join("settings.json"),serde_json::to_vec(&json!({"version":1,"values":{"workspace/compact":false},"seed":{"at":1,"keys":[]},"unknown":{"kept":true}})).unwrap()).unwrap();
    let source = scratch.source();
    let settings = scratch.settings();
    let original = fs::read(source.path()).unwrap();
    let mtime = fs::metadata(source.path()).unwrap().modified().unwrap();
    let mut calls = 0;
    recover_with(&source, &settings, NODE, |read_only, request| {
        calls += 1;
        if read_only {
            return Ok(status(false, false));
        }
        let pending = scratch.saved();
        assert_eq!(pending["g3_shared_organization_pending"]["node_id"], NODE);
        assert_eq!(
            pending["g3_shared_organization_pending"]["record"]["sha256"],
            zerus_desktop_migration::sha256(&original)
        );
        assert!(pending["g3"]["families"]
            .get("shared-organization")
            .is_none());
        assert_eq!(request["expected_node_id"], NODE);
        Ok(receipt(&request))
    })
    .unwrap();
    assert_eq!(calls, 2);
    let saved = scratch.saved();
    assert!(saved.get("g3_shared_organization_pending").is_none());
    let record = &saved["g3"]["families"]["shared-organization"];
    assert_eq!(record["keys"], json!([KEY]));
    assert_eq!(record["schema"], 1);
    assert_eq!(record["source_mtime_after_switch"], true);
    assert_eq!(saved["values"]["workspace/compact"], false);
    assert_eq!(saved["unknown"]["kept"], true);
    assert_eq!(fs::read(source.path()).unwrap(), original);
    assert_eq!(
        fs::metadata(source.path()).unwrap().modified().unwrap(),
        mtime
    );
    scratch.write(&json!({"version":99}));
    recover_with(&source, &settings, NODE, |_, _| {
        panic!("a handled family never calls again")
    })
    .unwrap();
    assert_eq!(scratch.saved(), saved);
}

#[test]
fn a_lost_reply_reconciles_the_same_initialized_node_after_restart_without_rereading_qt() {
    let scratch = Scratch::new();
    scratch.write(&organization());
    let source = scratch.source();
    let settings = scratch.settings();
    assert!(
        recover_with(&source, &settings, NODE, |read_only, _| if read_only {
            Ok(status(false, false))
        } else {
            Err(IMPORT_FAILED.into())
        })
        .is_err()
    );
    let pending = scratch.saved()["g3_shared_organization_pending"]["record"].clone();
    drop(settings);
    let settings = scratch.settings();
    let unreadable = QtStore::Ini(source.path().join("cannot-read-through-a-file"));
    let mut calls = 0;
    recover_with(&unreadable, &settings, NODE, |read_only, _| {
        calls += 1;
        assert!(read_only, "never repeat a confirmed import");
        Ok(status(true, false))
    })
    .unwrap();
    assert_eq!(calls, 1);
    assert_eq!(
        scratch.saved()["g3"]["families"]["shared-organization"],
        pending
    );
}

#[test]
fn a_cli_bootstrap_still_needs_the_gui_import_even_when_initialized_is_true() {
    let scratch = Scratch::new();
    scratch.write(&organization());
    let settings = scratch.settings();
    let mut writes = 0;
    recover_with(&scratch.source(), &settings, NODE, |read_only, request| {
        if read_only {
            Ok(status(true, true))
        } else {
            writes += 1;
            Ok(receipt(&request))
        }
    })
    .unwrap();
    assert_eq!(writes, 1);
    assert_eq!(
        settings.organization_import().unwrap(),
        OrganizationImport::Handled
    );
}

#[test]
fn a_changed_source_or_mismatched_receipt_leaves_the_family_pending() {
    let scratch = Scratch::new();
    scratch.write(&organization());
    let source = scratch.source();
    let settings = scratch.settings();
    let mut reads = 0;
    let result = recover_read(
        &source,
        &settings,
        NODE,
        || {
            reads += 1;
            if reads == 2 {
                let mut changed = organization();
                changed["projects"][0]["name"] = json!("changed");
                scratch.write(&changed);
            }
            source.snapshot()
        },
        |read_only, _| {
            assert!(read_only, "changed source must never reach initialize");
            Ok(status(false, false))
        },
    );
    assert_eq!(result.unwrap_err(), CHANGED);
    assert!(matches!(
        settings.organization_import().unwrap(),
        OrganizationImport::Pending { .. }
    ));
    assert!(recover_with(&source, &settings, OTHER, |_, _| panic!(
        "wrong node never invokes CLI"
    ))
    .is_err());
    assert!(recover_with(
        &source,
        &settings,
        NODE,
        |read_only, request| if read_only {
            Ok(status(false, false))
        } else {
            let mut reply = receipt(&request);
            reply["g3_import"]["source_sha256"] = json!("b".repeat(64));
            Ok(reply)
        }
    )
    .is_err());
    assert!(scratch.saved()["g3"]["families"]
        .get("shared-organization")
        .is_none());
}

#[test]
fn failed_settings_barriers_and_future_metadata_refuse_before_any_import() {
    let scratch = Scratch::new();
    scratch.write(&organization());
    let target = scratch.0.join("settings.json");
    fs::write(&target, br#"{"version":99,"private":"retain"}"#).unwrap();
    let future = fs::read(&target).unwrap();
    let settings = scratch.settings();
    assert!(
        recover_with(&scratch.source(), &settings, NODE, |_, _| panic!(
            "unknown settings cannot admit an import"
        ))
        .is_err()
    );
    assert_eq!(fs::read(&target).unwrap(), future);
    drop(settings);
    fs::write(&target, br#"{"version":1,"values":{}}"#).unwrap();
    let settings = scratch.settings();
    assert_eq!(
        settings.organization_import().unwrap(),
        OrganizationImport::Unhandled
    );
    fs::remove_file(&target).unwrap();
    fs::create_dir(&target).unwrap();
    fs::write(target.join("unknown"), b"retain").unwrap();
    assert!(
        recover_with(&scratch.source(), &settings, NODE, |read_only, _| {
            assert!(read_only, "unsaved metadata never admits initialize");
            Ok(status(false, false))
        })
        .is_err()
    );
    assert_eq!(fs::read(target.join("unknown")).unwrap(), b"retain");
}

#[test]
fn already_imported_catalogs_do_not_read_qt_or_create_a_new_family_record() {
    let scratch = Scratch::new();
    let settings = scratch.settings();
    recover_with(
        &QtStore::Ini(scratch.0.join("unknown")),
        &settings,
        NODE,
        |read_only, _| {
            assert!(read_only);
            Ok(status(true, false))
        },
    )
    .unwrap();
    assert_eq!(
        settings.organization_import().unwrap(),
        OrganizationImport::Unhandled
    );
    assert!(!scratch.0.join("settings.json").exists());
}
