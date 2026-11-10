use super::*;
use std::{
    os::unix::fs::{symlink, PermissionsExt},
    process::{Command, Stdio},
};

fn run(case: &str) {
    let home = tempfile::tempdir().unwrap();
    let output = tempfile::tempfile().unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", home.path())
        .env("HGS_CONFIG_DIR", home.path().join("config"))
        .env("HGS_STATE_DIR", home.path().join("state"))
        .env("HGS_SELF", "example")
        .env("ZERUS_PICKER_CALLER_CASE", case)
        .args([
            "--exact",
            "state::effort::picker_scope::caller::tests::fixture",
            "--ignored",
            "--test-threads=1",
        ])
        .stdin(Stdio::null())
        .stdout(output.try_clone().unwrap())
        .stderr(output.try_clone().unwrap());
    let mut child = command.spawn().unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= until {
            let _ = child.kill();
            let _ = child.wait();
            panic!("private caller fixture timed out: {case}")
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let mut output = output;
    use std::io::{Read, Seek};
    output.rewind().unwrap();
    let mut bytes = String::new();
    output.read_to_string(&mut bytes).unwrap();
    assert!(status.success(), "{case}: {bytes}");
}

#[test]
fn caller_guard_rechecks_current_machine_without_initializing_or_saving() {
    for case in ["same", "changed", "missing"] {
        run(case)
    }
}
#[test]
fn caller_guard_preserves_original_alias_route_and_native_objects() {
    for case in ["inside-alias", "outside-alias", "configuration-replaced"] {
        run(case)
    }
}

#[test]
fn repeated_public_checks_keep_first_capture_and_never_erase_refusal() {
    for case in [
        "repeat-same",
        "repeat-different",
        "repeat-failure",
        "repeat-namespace",
    ] {
        run(case)
    }
}

#[test]
#[ignore = "private caller-machine fixture, invoked only by bounded controls"]
fn fixture() {
    let case = std::env::var("ZERUS_PICKER_CALLER_CASE").unwrap();
    let config = crate::config::Config::load().unwrap();
    let expected = crate::swarm::local_node_id(&config).unwrap();
    let swarm = config.dir.join("swarm");
    fs::set_permissions(&swarm, fs::Permissions::from_mode(0o700)).unwrap();
    if matches!(
        case.as_str(),
        "inside-alias" | "outside-alias" | "configuration-replaced"
    ) {
        let original = if case == "inside-alias" {
            config.dir.join("catalog-a")
        } else {
            config.dir.parent().unwrap().join("external-catalog")
        };
        fs::rename(&swarm, &original).unwrap();
        symlink(&original, &swarm).unwrap();
    }
    remember(&config, &expected);
    if case == "repeat-failure" {
        {
            let mut original = ORIGINAL.lock().unwrap();
            original.first = None;
            original.refused = None;
        }
        fs::set_permissions(&swarm, fs::Permissions::from_mode(0o755)).unwrap();
        remember(&config, &expected);
        assert!(Snapshot::capture().is_err());
        fs::set_permissions(&swarm, fs::Permissions::from_mode(0o700)).unwrap();
        remember(&config, &expected);
        assert!(Snapshot::capture().is_err());
        return;
    }
    if case == "repeat-different" {
        remember(&config, &uuid::Uuid::new_v4().to_string());
        assert!(Snapshot::capture().is_err());
        remember(&config, &expected);
        assert!(Snapshot::capture().is_err());
        return;
    }
    if case == "repeat-namespace" {
        let old = config.dir.join("old-swarm");
        fs::rename(&swarm, old).unwrap();
        private_dir(&swarm).unwrap();
        remember(&config, &expected);
        assert!(Snapshot::capture().is_err());
        return;
    }
    if case == "repeat-same" {
        let first = ORIGINAL.lock().unwrap().first.as_ref().unwrap().clone();
        remember(&config, &expected);
        let kept = ORIGINAL.lock().unwrap().first.as_ref().unwrap().clone();
        assert!(Arc::ptr_eq(&first, &kept));
        return;
    }
    let snapshot = Snapshot::capture().unwrap();
    snapshot.validate().unwrap();
    let held = snapshot.hold().unwrap();
    held.check().unwrap();
    let bytes = fs::read(swarm.join("catalog.json")).unwrap();
    match case.as_str() {
        "same" => {
            assert_eq!(crate::swarm::local_node_id(&config).unwrap(), expected);
            held.check().unwrap(); // ordinary atomic save, same namespace/UUID
        }
        "changed" => {
            let mut changed: Value = serde_json::from_slice(&bytes).unwrap();
            changed["node_id"] = json!(uuid::Uuid::new_v4().to_string());
            atomic(&swarm.join("catalog.json"), &changed.to_string()).unwrap();
            assert!(held.check().is_err());
        }
        "missing" => {
            fs::remove_file(swarm.join("catalog.json")).unwrap();
            assert!(held.check().is_err());
            assert!(!swarm.join("catalog.json").exists());
        }
        "inside-alias" | "outside-alias" => {
            let other = config.dir.join("catalog-b");
            private_dir(&other).unwrap();
            atomic(
                &other.join("catalog.json"),
                std::str::from_utf8(&bytes).unwrap(),
            )
            .unwrap();
            // Even copied UUID data cannot silently rebind an original route.
            fs::remove_file(&swarm).unwrap();
            symlink(&other, &swarm).unwrap();
            assert!(held.check().is_err());
        }
        "configuration-replaced" => {
            let retained = config.dir.with_extension("retained");
            fs::rename(&config.dir, &retained).unwrap();
            private_dir(&config.dir).unwrap();
            symlink(retained.join("swarm"), &swarm).unwrap();
            assert!(held.check().is_err());
        }
        _ => panic!("unknown private caller case"),
    }
    let mut original = ORIGINAL.lock().unwrap();
    original.first = None;
    original.refused = None;
}
