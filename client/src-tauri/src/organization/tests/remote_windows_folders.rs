use super::*;
use zerus_client_core::{ComputerStatus, MachineMeta};

const NODE: &str = "10000000-0000-4000-8000-000000000001";
const PEER_NODE: &str = "20000000-0000-4000-8000-000000000002";
const ID: &str = "11111111-2222-4333-8444-555555555555";

fn machine(pump: &Pump, computer: ComputerId, host: &str, node: &str, platform: Option<&str>) {
    pump.push(Change::Machine {
        computer: computer.clone(),
        machine: MachineMeta {
            machine_id: Some(node.into()),
            host: Some(host.into()),
            platform: platform.map(str::to_owned),
            ok: Some(true),
            ..Default::default()
        },
    });
    pump.push(Change::Computer {
        computer,
        status: ComputerStatus::Live,
    });
}

fn catalog(folders: Value) -> Value {
    let mut value = full("remote-windows");
    value["node_id"] = json!(NODE);
    value["swarm_id"] = json!("catalog-one");
    value["organization"]["projects"][0]["folders"] = folders;
    value
}

fn setup(folders: Value, local: Option<&str>, peer: Option<&str>) -> (Fake, Pump, Organization) {
    let fake = Fake::landing();
    let pump = Pump::default();
    machine(&pump, ComputerId::local(), "local-host", NODE, local);
    machine(
        &pump,
        ComputerId("peer".into()),
        "peer-host",
        PEER_NODE,
        peer,
    );
    let org = fake.organization(&pump, true);
    let value = catalog(folders);
    org.set_source(snapshot_of(&value));
    org.observe(&state_of(&value));
    (fake, pump, org)
}

fn peer_place() -> folders::Place {
    folders::Place {
        computer: "peer".into(),
        host: "peer-host".into(),
        machine_id: Some(PEER_NODE.into()),
    }
}

fn local_place() -> folders::Place {
    folders::Place {
        computer: String::new(),
        host: "local-host".into(),
        machine_id: Some(NODE.into()),
    }
}

fn add(place: folders::Place, path: &str) -> folders::Change {
    folders::Change::Add {
        place,
        path: path.into(),
        name: String::new(),
    }
}

fn submit(org: &Organization, change: folders::Change) -> EditOutcome {
    org.submit_checked(|pump| folders::intent(pump, "alpha".into(), change, ID.into()))
}

fn original(value: Value) -> Folder {
    zerus_client_core::swarm::folder_value(&value).unwrap()
}

#[test]
fn remote_windows_drive_and_unc_write_with_blank_names() {
    for (raw, stored) in [
        ("C:\\Work\\..\\Repo\\", "C:/Repo"),
        ("\\\\server\\share\\Repo\\", "//server/share/Repo"),
    ] {
        let (fake, _pump, org) = setup(json!([]), Some("linux"), Some("windows"));
        assert_eq!(
            submit(&org, add(peer_place(), raw)),
            EditOutcome::Done { conflicts: 0 }
        );
        let calls = fake.applies();
        assert_eq!(calls.len(), 1);
        let desired = input(&calls[0])["desired"].clone();
        let folder = &desired["projects"][0]["folders"][0];
        assert_eq!(folder["machine"], "peer", "{desired}");
        assert_eq!(
            desired["projects"][0]["folders"][0]["path"], stored,
            "{desired}"
        );
        assert_eq!(desired["projects"][0]["folders"][0]["name"], "");
        fake.finish(org);
    }
}

#[test]
fn legacy_posix_add_remove_and_transfer_continue() {
    let (fake, _pump, org) = setup(json!([]), Some("linux"), Some("linux"));
    let mut choice = add(peer_place(), "/srv/repo/../work/");
    if let folders::Change::Add { name, .. } = &mut choice {
        *name = "  Work  ".into();
    }
    assert_eq!(submit(&org, choice), EditOutcome::Done { conflicts: 0 });
    let desired = input(&fake.applies()[0])["desired"].clone();
    assert_eq!(desired["projects"][0]["folders"][0]["path"], "/srv/work");
    assert_eq!(desired["projects"][0]["folders"][0]["name"], "Work");
    fake.finish(org);

    for transfer in [false, true] {
        let record = json!({"id":"original","machine":"peer","machine_id":PEER_NODE,"path":"/srv/work","name":""});
        let (fake, _pump, org) = setup(json!([record.clone()]), Some("linux"), Some("linux"));
        let change = if transfer {
            folders::Change::Transfer {
                original: original(record),
                destination: "ungrouped".into(),
                copy: false,
            }
        } else {
            folders::Change::Remove {
                original: original(record),
            }
        };
        assert_eq!(submit(&org, change), EditOutcome::Done { conflicts: 0 });
        let desired = input(&fake.applies()[0])["desired"].clone();
        assert_eq!(desired["projects"][0]["folders"], json!([]));
        if transfer {
            assert_eq!(
                desired["projects"][1]["folders"].as_array().unwrap().len(),
                1
            );
        }
        fake.finish(org);
    }
}

#[test]
fn edit_uses_verified_styles_for_source_and_response_keys() {
    for id in ["", "stable-id"] {
        let record = json!({"id":id,"machine":"peer","machine_id":PEER_NODE,"machine_name":"peer-host","path":"C:/Work/Old","name":""});
        let (fake, pump, org) = setup(json!([record.clone()]), Some("linux"), Some("windows"));
        let change = folders::Change::Edit {
            original: original(record),
            place: peer_place(),
            path: "c:\\work\\new\\.".into(),
            name: String::new(),
        };
        let admitted = folders::intent(&pump, "alpha".into(), change.clone(), ID.into()).unwrap();
        let expected = if id.is_empty() {
            "peer\nc:/work/new".to_owned()
        } else {
            id.to_owned()
        };
        assert_eq!(
            folders::selected_folder(&admitted).as_deref(),
            Some(expected.as_str())
        );
        assert_eq!(submit(&org, change), EditOutcome::Done { conflicts: 0 });
        let desired = input(&fake.applies()[0])["desired"].clone();
        assert_eq!(desired["projects"][0]["folders"][0]["path"], "c:/work/new");
        fake.finish(org);
    }

    let record = json!({"machine":"posix-peer","machine_id":"30000000-0000-4000-8000-000000000003","machine_name":"posix-host","path":"/srv/old","name":""});
    let (fake, pump, org) = setup(json!([record.clone()]), Some("linux"), Some("windows"));
    machine(
        &pump,
        ComputerId("posix-peer".into()),
        "posix-host",
        "30000000-0000-4000-8000-000000000003",
        Some("linux"),
    );
    let change = folders::Change::Edit {
        original: original(record),
        place: peer_place(),
        path: "C:/Work/New".into(),
        name: String::new(),
    };
    assert_eq!(submit(&org, change), EditOutcome::Done { conflicts: 0 });
    fake.finish(org);
}

#[test]
fn windows_duplicates_and_invalid_paths_never_apply() {
    let record = json!({"id":"existing","machine":"peer","machine_id":PEER_NODE,"path":"C:/Work/Project","name":""});
    let (fake, _pump, org) = setup(json!([record]), Some("linux"), Some("windows"));
    assert!(matches!(
        submit(&org, add(peer_place(), "c:\\work\\project\\.")),
        EditOutcome::Refused { .. }
    ));
    for path in [
        "relative",
        "C:Work",
        "\\Work",
        "\\\\?\\C:\\Work",
        "C:/bad\n",
        "C:/bad\0",
    ] {
        assert!(
            matches!(
                submit(&org, add(peer_place(), path)),
                EditOutcome::Refused { .. }
            ),
            "{path:?}"
        );
    }
    assert!(fake.applies().is_empty());
    fake.finish(org);
}

#[test]
fn unknown_local_and_drifted_computers_never_apply() {
    let (fake, _pump, org) = setup(json!([]), Some("linux"), None);
    assert!(matches!(
        submit(&org, add(peer_place(), "C:/Work")),
        EditOutcome::Refused { .. }
    ));
    assert!(fake.applies().is_empty());
    fake.finish(org);

    let (fake, _pump, org) = setup(json!([]), Some("windows"), Some("windows"));
    assert!(matches!(
        submit(&org, add(local_place(), "C:/Work")),
        EditOutcome::Refused { .. }
    ));
    let mut wrong = peer_place();
    wrong.host = "other-host".into();
    assert!(matches!(
        submit(&org, add(wrong, "C:/Work")),
        EditOutcome::Refused { .. }
    ));
    let mut wrong = peer_place();
    wrong.machine_id = Some(NODE.into());
    assert!(matches!(
        submit(&org, add(wrong, "C:/Work")),
        EditOutcome::Refused { .. }
    ));
    assert!(fake.applies().is_empty());
    fake.finish(org);
}

#[test]
fn original_windows_identity_and_local_fences_never_apply() {
    let record =
        json!({"id":"local","machine":"local-host","machine_id":NODE,"path":"C:/Local","name":""});
    let (fake, _pump, org) = setup(json!([record.clone()]), Some("windows"), Some("windows"));
    let change = folders::Change::Edit {
        original: original(record),
        place: peer_place(),
        path: "C:/Remote".into(),
        name: String::new(),
    };
    assert!(matches!(submit(&org, change), EditOutcome::Refused { .. }));
    assert!(fake.applies().is_empty());
    fake.finish(org);

    let record =
        json!({"id":"peer","machine":"peer","machine_id":"drifted","path":"C:/Old","name":""});
    let (fake, _pump, org) = setup(json!([record.clone()]), Some("linux"), Some("windows"));
    let change = folders::Change::Edit {
        original: original(record),
        place: peer_place(),
        path: "C:/New".into(),
        name: String::new(),
    };
    assert!(matches!(submit(&org, change), EditOutcome::Refused { .. }));
    assert!(fake.applies().is_empty());
    fake.finish(org);
}

#[test]
fn corrupt_stale_and_legacy_windows_changes_never_apply() {
    let (fake, _pump, org) = setup(json!({"broken":true}), Some("linux"), Some("windows"));
    assert!(matches!(
        submit(&org, add(peer_place(), "C:/Work")),
        EditOutcome::Refused { .. }
    ));
    assert!(fake.applies().is_empty());
    fake.finish(org);

    let record =
        json!({"id":"original","machine":"peer","machine_id":PEER_NODE,"path":"C:/Old","name":""});
    let (fake, _pump, org) = setup(json!([record.clone()]), Some("linux"), Some("windows"));
    let mut stale = original(record.clone());
    stale.path = "C:/Stale".into();
    let change = folders::Change::Edit {
        original: stale,
        place: peer_place(),
        path: "C:/New".into(),
        name: String::new(),
    };
    assert!(matches!(submit(&org, change), EditOutcome::Refused { .. }));
    let remove = folders::Change::Remove {
        original: original(record),
    };
    assert!(matches!(submit(&org, remove), EditOutcome::Refused { .. }));
    assert!(fake.applies().is_empty());
    fake.finish(org);
}
