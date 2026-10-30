use super::super::store::Store;
use super::*;
use std::{
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};

fn location(path: &Path) -> Location {
    let vector: serde_json::Value = serde_json::from_str(include_str!(
        "../../../client/fixtures/desktop-operation-port-v1.json"
    ))
    .unwrap();
    Location {
        root: zerus_desktop_migration::canonical(&path.join("state/desktop")).unwrap(),
        configuration: vector["binding"]["key"]["configuration"]
            .as_str()
            .unwrap()
            .into(),
    }
}
fn admitted(location: &Location) -> (Store, operations::Reserve, Ticket) {
    let epoch = uuid::Uuid::new_v4().to_string();
    let writer = uuid::Uuid::new_v4().to_string();
    location.prepare().unwrap();
    let mut store = Store::open(location, &epoch).unwrap();
    zerus_desktop_ledger::authorize(&mut store.db, &writer, &epoch).unwrap();
    let vector: serde_json::Value = serde_json::from_str(include_str!(
        "../../../client/fixtures/desktop-operation-port-v1.json"
    ))
    .unwrap();
    let binding: operations::Binding = serde_json::from_value(vector["binding"].clone()).unwrap();
    let admission = uuid::Uuid::new_v4().to_string();
    let original = operations::admit(
        &mut store.db,
        &operations::Admission {
            admission_id: &admission,
            binding: &binding,
            raw_record: vector["raw_record"].as_str().unwrap().as_bytes(),
            retained_input: vector["retained_input"].as_str().unwrap().as_bytes(),
            provenance: &serde_json::json!({"fixture":true}),
            writer_epoch: &writer,
            service_epoch: &epoch,
        },
    )
    .unwrap();
    let request = operations::Reserve {
        operation_id: original.operation_id.clone(),
        attempt_id: uuid::Uuid::new_v4().to_string(),
        revision: original.revision,
        writer_epoch: writer.clone(),
        service_epoch: epoch.clone(),
        lease: uuid::Uuid::new_v4().to_string(),
    };
    let ticket = Ticket {
        operation_id: original.operation_id,
        attempt_id: request.attempt_id.clone(),
        revision: original.revision,
        admission_id: original.admission_id,
        binding_sha256: original.binding_sha256,
        writer_epoch: writer,
        service_epoch: epoch,
        lease: request.lease.clone(),
    };
    (store, request, ticket)
}

#[test]
fn private_exact_file_is_create_new_and_close_only() {
    let temp = tempfile::tempdir().unwrap();
    let location = location(temp.path());
    let (_store, _request, ticket) = admitted(&location);
    let (prepared, lock) = prepare(&location, &ticket).unwrap();
    let record = Record {
        prepared,
        helper: None,
    };
    assert!(matches!(probe(&location, &record), Observation::Held));
    assert!(prepare(&location, &ticket).is_err());
    drop(lock);
    assert!(matches!(
        probe(&location, &record),
        Observation::Released { .. }
    ));
    let mut changed = record.clone();
    changed.prepared.file.inode += 1;
    assert!(matches!(
        probe(&location, &changed),
        Observation::Unavailable
    ));
    changed = record;
    changed.prepared.configuration = "f".repeat(64);
    assert!(matches!(
        probe(&location, &changed),
        Observation::Unavailable
    ));
}

#[test]
fn missing_replaced_linked_nonprivate_or_changed_custody_is_never_repaired() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    for kind in 0..6 {
        let temp = tempfile::tempdir().unwrap();
        let location = location(temp.path());
        let (_store, _request, ticket) = admitted(&location);
        let (prepared, lock) = prepare(&location, &ticket).unwrap();
        let record = Record {
            prepared,
            helper: None,
        };
        drop(lock);
        let path = location
            .root
            .join(DIRECTORY)
            .join(filename(&ticket).unwrap());
        let bytes = fs::read(&path).unwrap();
        match kind {
            0 => fs::remove_file(&path).unwrap(),
            1 => {
                fs::rename(&path, temp.path().join("old")).unwrap();
                fs::write(&path, &bytes).unwrap();
                fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
            }
            2 => {
                fs::remove_file(&path).unwrap();
                symlink(temp.path().join("missing"), &path).unwrap();
            }
            3 => fs::hard_link(&path, temp.path().join("extra-link")).unwrap(),
            4 => fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap(),
            _ => fs::write(&path, vec![b'x'; 4097]).unwrap(),
        }
        assert!(
            matches!(probe(&location, &record), Observation::Unavailable),
            "unsafe case {kind}"
        );
        if kind == 0 {
            assert!(!path.exists());
        }
        if kind == 2 {
            assert!(fs::symlink_metadata(&path)
                .unwrap()
                .file_type()
                .is_symlink());
        }
    }
}

fn poll(store: &mut Store, location: &Location, done: impl Fn(&Store) -> bool) {
    let until = Instant::now() + Duration::from_secs(8);
    while !done(store) && Instant::now() < until {
        store.retry_operation_custody();
        store.recover_operation_custody(location);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        done(store),
        "custody did not reach expected bounded observation: {}",
        store.operation_status()
    );
}

#[test]
fn isolated_host_crash_and_file_backed_reopen_keep_a_live_helper_charged() {
    let temp = tempfile::tempdir().unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "desktop::custody::tests::isolated_custody_host",
            "--nocapture",
        ])
        .env("CUSTODY_HOST_FIXTURE", temp.path())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(temp.path().join("registered").exists());
    let location = location(temp.path());
    let mut store = Store::open(&location, &uuid::Uuid::new_v4().to_string()).unwrap();
    assert_eq!(operations::totals(&store.db).unwrap().undrained, 1);
    let before = operations::custody::pending(&store.db).unwrap().unwrap();
    assert_eq!(before.0.state, "unknown");
    let ticket = before.0.ticket;
    poll(&mut store, &location, |s| {
        s.operation_status()["resource_custody"]["state"] == "held"
    });
    assert_eq!(
        store.operation_status()["resource_custody"]["helper_state"],
        "running"
    );
    assert_eq!(operations::totals(&store.db).unwrap().undrained, 1);
    fs::write(temp.path().join("release"), b"finish own synthetic helper").unwrap();
    poll(&mut store, &location, |s| {
        operations::totals(&s.db).unwrap().undrained == 0
    });
    assert!(operations::custody::pending(&store.db).unwrap().is_none());
    let state: String = store
        .db
        .query_row(
            "SELECT state FROM desktop_operation_attempts WHERE attempt_id=?1",
            [ticket.attempt_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        state, "unknown",
        "resource proof never rewrites an unknown outcome"
    );
}

#[test]
fn isolated_custody_host() {
    let Some(root) = std::env::var_os("CUSTODY_HOST_FIXTURE") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let location = location(&root);
    let (mut store, request, ticket) = admitted(&location);
    let (prepared, lock) = prepare(&location, &ticket).unwrap();
    operations::custody::reserve(&mut store.db, &request, &prepared).unwrap();
    let mut command = Command::new("python3");
    command.args(["-c","import pathlib,sys,time\np=pathlib.Path(sys.argv[1]);end=time.monotonic()+30\nwhile not p.exists() and time.monotonic()<end:time.sleep(.01)"])
        .arg(root.join("release")).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    lock.inherit(&mut command).unwrap();
    let helper = command.spawn().unwrap();
    let pid = helper.id();
    let birth = platform::process_identity(pid).unwrap().birth;
    operations::custody::register(&mut store.db, &ticket, &Birth { pid, birth }).unwrap();
    fs::write(root.join("registered"), b"durable registration completed").unwrap();
    // Exit only our isolated custody host. Its Python helper ends on its own
    // private release file; no native agent, group or service is signalled.
    std::process::exit(0);
}

#[test]
fn offline_revoke_releases_only_closed_exact_custody_before_settling() {
    use super::super::revoke;
    let temp = tempfile::tempdir().unwrap();
    let location = location(temp.path());
    let (mut store, request, ticket) = admitted(&location);
    let (prepared, lock) = prepare(&location, &ticket).unwrap();
    operations::custody::reserve(&mut store.db, &request, &prepared).unwrap();
    drop(store);
    let command = revoke::cli(&["--json", "--reason", "rollback"]).unwrap();
    let held = revoke::offline(&location, &command).unwrap();
    assert_eq!(held["revoked"], false);
    assert_eq!(held["resource_custody"]["attempt_id"], ticket.attempt_id);
    assert_eq!(held["resource_custody"]["revoke_request_id"], command.id());
    assert!(held["resource_custody"]["remediation"]
        .as_str()
        .unwrap()
        .contains("restart only hgs-desktop"));
    drop(lock);
    let settled = revoke::offline(&location, &command).unwrap();
    assert_eq!(settled["request_id"], command.id());
    for key in ["accepted", "durable", "revoked", "checkpointed"] {
        assert_eq!(settled[key], true);
    }
}
