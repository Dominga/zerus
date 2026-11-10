use super::*;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};
use zerus_client_core::{ComputerStatus, MachineMeta};

const FOLDER: &str = "11111111-2222-4333-8444-555555555555";

fn call(hgs: &Path, args: &[&str], input: Value) -> Value {
    let mut child = Command::new(hgs)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&input).unwrap())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

struct Real {
    dir: PathBuf,
    /// The verified candidate CLI that every private wrapper runs.
    binary: PathBuf,
    hgs: PathBuf,
    snapshot: Value,
    machine: String,
}

/// The actual non-test CLI under test: the caller builds it from this source
/// and passes its exact path (zerus-h43u3). There is no repository fallback,
/// so a stale or unrelated build never stands in for the candidate.
fn candidate_cli() -> PathBuf {
    let Some(value) = std::env::var_os("HGS_TEST_BIN") else {
        panic!(
            "set HGS_TEST_BIN to the hgs built from this source; scripts/ci/client-native.sh does"
        );
    };
    let path = PathBuf::from(value);
    assert!(
        path.is_absolute(),
        "HGS_TEST_BIN must be an absolute path: {}",
        path.display()
    );
    let metadata = std::fs::metadata(&path)
        .unwrap_or_else(|error| panic!("HGS_TEST_BIN {}: {error}", path.display()));
    assert!(
        metadata.is_file() && (metadata.permissions().mode() & 0o111) != 0,
        "HGS_TEST_BIN is not an executable file: {}",
        path.display()
    );
    path.canonicalize().unwrap()
}

/// One POSIX shell word, whatever the path contains.
fn shell_word(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', r"'\''"))
}

impl Real {
    fn new() -> Self {
        let binary = candidate_cli();
        let dir = std::env::temp_dir().join(format!("zerus-native-drive-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let hgs = dir.join("hgs-private");
        let script = format!(
            "#!/bin/sh\nexport HOME={0}\nexport HGS_CONFIG_DIR={1}\nexport HGS_STATE_DIR={2}\nexport HGS_SELF='example-local'\nexport HGS_PEERS=''\nexec {3} \"$@\"\n",
            shell_word(&dir),
            shell_word(&dir.join("config")),
            shell_word(&dir.join("state")),
            shell_word(&binary),
        );
        // Never held open for writing here: parallel tests fork (zerus-0b9f).
        crate::write_executable(&hgs, script);
        let organization = json!({"version":2,"default_project":"alpha","projects":[
            {"id":"alpha","name":"Alpha","color":"#123456","folders":[],"sessions":[]}
        ]});
        let initialized = call(&hgs, &["swarm", "initialize", "--json"], organization);
        let mut desired = initialized["organization"].clone();
        desired["projects"][0]["folders"] = json!([{
            "id":"legacy-machine","machine":"windows-peer","path":"/srv/repo","name":""
        }]);
        let snapshot = call(
            &hgs,
            &["swarm", "apply", "--json"],
            json!({
                "base":initialized["organization"],
                "desired":desired,
                "versions":initialized["versions"]
            }),
        );
        let machine = snapshot["organization"]["projects"][0]["folders"][0]["machine_id"]
            .as_str()
            .unwrap()
            .to_owned();
        Self {
            dir,
            binary,
            hgs,
            snapshot,
            machine,
        }
    }

    fn drive_get(&self) -> Value {
        call(
            &self.hgs,
            &["swarm", "drive-folders", "get", "--json"],
            Value::Null,
        )
    }

    fn finish(self, organization: Organization) {
        let runs = organization.runs_writer();
        let shared = Arc::downgrade(&organization.0);
        drop(organization);
        wait_until("the real organization worker to end", || {
            shared.strong_count() == 0
        });
        runs.stop();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn card(pump: &Pump, computer: &str, host: &str, node: &str, platform: &str) {
    let computer = ComputerId(computer.to_owned());
    pump.push(Change::Machine {
        computer: computer.clone(),
        machine: MachineMeta {
            machine_id: Some(node.into()),
            host: Some(host.into()),
            platform: Some(platform.into()),
            ok: Some(true),
            ..Default::default()
        },
    });
    pump.push(Change::Computer {
        computer,
        status: ComputerStatus::Live,
    });
}

#[test]
fn project_folder_change_persists_add_and_edit_with_a_stable_id() {
    let real = Real::new();
    let main_before = std::fs::read(real.dir.join("config/swarm/catalog.json")).unwrap();
    let pump = Pump::default();
    card(
        &pump,
        "",
        "example-local",
        real.snapshot["node_id"].as_str().unwrap(),
        "linux",
    );
    card(
        &pump,
        "windows-peer",
        "windows-peer",
        &real.machine,
        "windows",
    );
    let organization = Organization::new(
        pump,
        real.hgs.clone(),
        true,
        real.dir.join("layout.json"),
        real.dir.join("draft.json"),
        false,
    );
    organization.set_source(snapshot_of(&real.snapshot));
    organization.observe(&state_of(&real.snapshot));
    let place = folders::Place {
        computer: "windows-peer".into(),
        host: "windows-peer".into(),
        machine_id: Some(real.machine.clone()),
    };
    let added = folders::change(
        organization.clone(),
        "alpha".into(),
        folders::Change::Add {
            place: place.clone(),
            path: "C:\\Work\\..\\Repo\\".into(),
            name: String::new(),
        },
        FOLDER.into(),
    );
    let added_json = serde_json::to_value(&added).unwrap();
    assert_eq!(added_json["outcome"], "done", "{added_json}");
    let observed = real.drive_get();
    assert_eq!(observed["records"][0]["id"], FOLDER);
    assert_eq!(observed["records"][0]["path"], "C:/Repo");
    let graph: Value = serde_json::from_slice(
        &std::fs::read(real.dir.join("config/swarm/drive-folders.json")).unwrap(),
    )
    .unwrap();
    assert!(graph["operations"]
        .as_object()
        .unwrap()
        .keys()
        .all(|id| id != FOLDER));

    let original = organization
        .project_folders("alpha")
        .into_iter()
        .find(|folder| folder.id == FOLDER)
        .expect("the capable native projection contains the new folder");
    assert_eq!(original.machine, "windows-peer");
    assert_eq!(original.machine_id, real.machine);
    assert!(organization.creation_projects("example-local")[0]
        .folders
        .iter()
        .all(|folder| folder.path != "C:/Repo"));
    let edited = folders::change(
        organization.clone(),
        "alpha".into(),
        folders::Change::Edit {
            original,
            place,
            path: "D:\\Edited\\.".into(),
            name: "Renamed".into(),
        },
        uuid::Uuid::new_v4().to_string(),
    );
    assert_eq!(serde_json::to_value(&edited).unwrap()["outcome"], "done");
    let observed = real.drive_get();
    assert_eq!(observed["records"][0]["id"], FOLDER);
    assert_eq!(observed["records"][0]["path"], "D:/Edited");
    assert_eq!(observed["records"][0]["name"], "Renamed");
    let graph: Value = serde_json::from_slice(
        &std::fs::read(real.dir.join("config/swarm/drive-folders.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(graph["operations"].as_object().unwrap().len(), 2);
    assert!(observed["versions"]
        .as_object()
        .unwrap()
        .values()
        .all(|heads| heads.as_array().unwrap().len() == 1));
    assert_eq!(
        std::fs::read(real.dir.join("config/swarm/catalog.json")).unwrap(),
        main_before
    );
    real.finish(organization);
}

impl Real {
    fn instrument_held_get(&self) -> (PathBuf, PathBuf) {
        let marker = self.dir.join("get-started");
        let calls = self.dir.join("calls.log");
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> {0}\nif [ \"$*\" = 'swarm drive-folders get --json' ]; then\n  touch {1}\n  sleep 0.2\nfi\nexport HOME={2}\nexport HGS_CONFIG_DIR={3}\nexport HGS_STATE_DIR={4}\nexport HGS_SELF='example-local'\nexport HGS_PEERS=''\nexec {5} \"$@\"\n",
            shell_word(&calls),
            shell_word(&marker),
            shell_word(&self.dir),
            shell_word(&self.dir.join("config")),
            shell_word(&self.dir.join("state")),
            shell_word(&self.binary),
        );
        // Replaced, never written through, while parallel tests fork (zerus-0b9f).
        crate::write_executable(&self.hgs, script);
        (marker, calls)
    }
}

#[test]
fn held_refresh_rejects_source_and_card_drift_before_durable_admission() {
    #[derive(Clone, Copy)]
    enum Drift {
        Offline,
        Host,
        Uuid,
        Generation,
        Digest,
    }

    for drift in [
        Drift::Offline,
        Drift::Host,
        Drift::Uuid,
        Drift::Generation,
        Drift::Digest,
    ] {
        let real = Real::new();
        let (marker, calls) = real.instrument_held_get();
        let pump = Pump::default();
        card(
            &pump,
            "",
            "example-local",
            real.snapshot["node_id"].as_str().unwrap(),
            "linux",
        );
        card(
            &pump,
            "windows-peer",
            "windows-peer",
            &real.machine,
            "windows",
        );
        let organization = Organization::new(
            pump.clone(),
            real.hgs.clone(),
            true,
            real.dir.join("layout.json"),
            real.dir.join("draft.json"),
            false,
        );
        let current = Arc::new(Mutex::new(real.snapshot.clone()));
        let source = current.clone();
        organization.set_source(Arc::new(move || {
            let value = source.lock().unwrap().clone();
            Some(OrganizationSnapshot {
                node_id: value["node_id"].as_str().unwrap().into(),
                swarm_id: value["swarm_id"].as_str().unwrap().into(),
                digest: value["digest"].as_str().unwrap().into(),
                base: value["organization"].clone(),
                versions: value["versions"].clone(),
            })
        }));
        organization.observe(&state_of(&real.snapshot));
        let changed = organization.clone();
        let changed_pump = pump.clone();
        let changed_source = current.clone();
        let changed_snapshot = real.snapshot.clone();
        let changed_machine = real.machine.clone();
        let interrupt = std::thread::spawn(move || {
            wait_until("the held drive get", || marker.exists());
            match drift {
                Drift::Offline => changed_pump.push(Change::Computer {
                    computer: ComputerId("windows-peer".into()),
                    status: ComputerStatus::Ended,
                }),
                Drift::Host | Drift::Uuid => {
                    let node = match drift {
                        Drift::Host => changed_machine,
                        _ => uuid::Uuid::new_v4().to_string(),
                    };
                    let host = match drift {
                        Drift::Host => "changed-host".into(),
                        _ => "windows-peer".into(),
                    };
                    changed_pump.push(Change::Machine {
                        computer: ComputerId("windows-peer".into()),
                        machine: MachineMeta {
                            machine_id: Some(node),
                            host: Some(host),
                            platform: Some("windows".into()),
                            ok: Some(true),
                            ..Default::default()
                        },
                    });
                }
                Drift::Generation => changed.set_source(snapshot_of(&changed_snapshot)),
                Drift::Digest => {
                    let mut value = changed_snapshot;
                    value["digest"] = json!("changed-catalog-digest");
                    *changed_source.lock().unwrap() = value.clone();
                    changed.observe(&state_of(&value));
                }
            }
        });
        let outcome = folders::change(
            organization.clone(),
            "alpha".into(),
            folders::Change::Add {
                place: folders::Place {
                    computer: "windows-peer".into(),
                    host: "windows-peer".into(),
                    machine_id: Some(real.machine.clone()),
                },
                path: "C:/Held".into(),
                name: String::new(),
            },
            FOLDER.into(),
        );
        interrupt.join().unwrap();
        assert_eq!(
            serde_json::to_value(&outcome).unwrap()["outcome"],
            "refused"
        );
        let calls = std::fs::read_to_string(calls).unwrap();
        assert!(!calls.contains("drive-folders apply"), "{calls}");
        assert!(!real.dir.join("organization-drive-draft.json").exists());
        assert!(!real.dir.join("config/swarm/drive-folders.json").exists());
        assert!(!real
            .dir
            .join("config/swarm/drive-folders-init.json")
            .exists());
        real.finish(organization);
    }
}

const LEGACY_NODE: &str = "10000000-0000-4000-8000-000000000001";
const LEGACY_PEER: &str = "20000000-0000-4000-8000-000000000002";

fn legacy_setup(folders: Value) -> (Fake, Pump, Organization) {
    let fake = Fake::landing();
    let pump = Pump::default();
    card(&pump, "", "local-host", LEGACY_NODE, "linux");
    card(&pump, "peer", "peer-host", LEGACY_PEER, "windows");
    let org = fake.organization(&pump, true);
    let mut value = full("remote-unc");
    value["node_id"] = json!(LEGACY_NODE);
    value["swarm_id"] = json!("catalog-one");
    value["organization"]["projects"][0]["folders"] = folders;
    org.set_source(snapshot_of(&value));
    org.observe(&state_of(&value));
    (fake, pump, org)
}

fn legacy_place() -> folders::Place {
    folders::Place {
        computer: "peer".into(),
        host: "peer-host".into(),
        machine_id: Some(LEGACY_PEER.into()),
    }
}

fn legacy_outcome(org: &Organization, change: folders::Change) -> String {
    let changed = folders::change(
        org.clone(),
        "alpha".into(),
        change,
        "11111111-2222-4333-8444-555555555555".into(),
    );
    serde_json::to_value(changed).unwrap()["outcome"]
        .as_str()
        .unwrap()
        .into()
}

#[test]
fn actual_change_routes_both_unc_spellings_and_posix_to_the_legacy_writer() {
    for (raw, stored) in [
        ("\\\\server\\share\\Repo\\", "//server/share/Repo"),
        ("//server/share/Repo/", "//server/share/Repo"),
    ] {
        let (fake, _pump, org) = legacy_setup(json!([]));
        assert_eq!(
            legacy_outcome(
                &org,
                folders::Change::Add {
                    place: legacy_place(),
                    path: raw.into(),
                    name: String::new(),
                },
            ),
            "done".to_owned()
        );
        let calls = fake.applies();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            input(&calls[0])["desired"]["projects"][0]["folders"][0]["path"],
            stored
        );
        fake.finish(org);
    }

    for raw in ["\\\\server\\share\\New\\", "//server/share/New/"] {
        let record = json!({"id":FOLDER,"machine":"peer","machine_id":LEGACY_PEER,"machine_name":"peer-host","path":"//server/share/Old","name":""});
        let (fake, _pump, org) = legacy_setup(json!([record.clone()]));
        let original = zerus_client_core::swarm::folder_value(&record).unwrap();
        assert_eq!(
            legacy_outcome(
                &org,
                folders::Change::Edit {
                    original,
                    place: legacy_place(),
                    path: raw.into(),
                    name: String::new(),
                },
            ),
            "done".to_owned()
        );
        let calls = fake.applies();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            input(&calls[0])["desired"]["projects"][0]["folders"][0]["path"],
            "//server/share/New"
        );
        fake.finish(org);
    }

    let (fake, pump, org) = legacy_setup(json!([]));
    card(&pump, "peer", "peer-host", LEGACY_PEER, "linux");
    assert_eq!(
        legacy_outcome(
            &org,
            folders::Change::Add {
                place: legacy_place(),
                path: "/srv/repo/../work/".into(),
                name: String::new(),
            },
        ),
        "done".to_owned()
    );
    assert_eq!(
        input(&fake.applies()[0])["desired"]["projects"][0]["folders"][0]["path"],
        "/srv/work"
    );
    fake.finish(org);
}

#[test]
fn actual_change_keeps_device_and_drive_relative_paths_refused() {
    for path in ["\\\\?\\C:\\Work", "C:Work", "\\Work"] {
        let (fake, _pump, org) = legacy_setup(json!([]));
        assert_eq!(
            legacy_outcome(
                &org,
                folders::Change::Add {
                    place: legacy_place(),
                    path: path.into(),
                    name: String::new(),
                },
            ),
            "refused"
        );
        assert!(fake.applies().is_empty());
        fake.finish(org);
    }
}
