use super::super::{Origin, SelectedFrontend};
use super::*;
use std::{fs, os::unix::fs::PermissionsExt, sync::Mutex};
use wire::{Open, Target};

static SERIAL: Mutex<()> = Mutex::new(());
struct Fixture {
    _root: tempfile::TempDir,
    prefix: std::path::PathBuf,
}
impl Fixture {
    fn new(help: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let prefix = root.path().canonicalize().unwrap().join("private install");
        for dir in ["bin", "libexec", "share/hgs"] {
            fs::create_dir_all(prefix.join(dir)).unwrap();
            fs::set_permissions(prefix.join(dir), fs::Permissions::from_mode(0o755)).unwrap();
        }
        fs::set_permissions(&prefix, fs::Permissions::from_mode(0o755)).unwrap();
        fs::copy("/bin/sleep", prefix.join("bin/hgs-tray")).unwrap();
        fs::write(prefix.join("bin/hgs"), b"private CLI image fixture").unwrap();
        fs::set_permissions(prefix.join("bin/hgs"), fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(prefix.join("share/hgs/local-webkit.json"), b"{}").unwrap();
        // No token/target output or file storage. Shell assertions qualify the
        // received argv/environment before execing the SAME owned child's ELF.
        let target = wire::encode_navigation(&open().target).unwrap();
        fs::write(prefix.join("libexec/hgs-tray-launch"), format!(
            "#!/bin/sh\nif [ \"$1\" = --help ]; then printf '%s\\n' '{help}'; exit 0; fi\n\
            [ \"$#\" = 5 ] && [ \"$1\" = --hgs ] && [ \"$2\" = '{}/bin/hgs' ] && \\\n            [ \"$3\" = --sessions ] && [ \"$4\" = --desktop-target ] && [ \"$5\" = '{target}' ] && \\\n            [ \"${{XDG_ACTIVATION_TOKEN-}}\" = synthetic-wayland ] && \\\n            [ \"${{ZERUS_CLIENT_DESKTOP_SERVICE-}}\" = 1 ] && [ -z \"${{HGS_SESSION-}}${{HGS_RUN_ID-}}\" ] || exit 31\n\
            exec '{}/bin/hgs-tray' 0.8\n", prefix.display(), prefix.display())).unwrap();
        fs::set_permissions(
            prefix.join("libexec/hgs-tray-launch"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        Self {
            _root: root,
            prefix,
        }
    }
    fn select(&self) -> SelectedFrontend {
        SelectedFrontend::capture(
            self.prefix.join("bin/hgs"),
            self.prefix.join("bin/hgs-tray"),
            "a".repeat(64),
            Origin::Fixture,
        )
        .unwrap()
    }
}
fn open() -> Open {
    let target = Target {
        computer: "peer.example".into(),
        run: "original-run".into(),
        conversation: "original-conversation".into(),
    };
    let token = zerus_client_core::attention::encode_token(
        serde_json::json!({
            "host":target.computer, "run":target.run, "conversation":target.conversation,
            "name":"not-a-selector", "notice":"fixture"
        })
        .as_object()
        .unwrap(),
    );
    Open {
        activation_id: uuid::Uuid::new_v4().to_string(),
        post_request_id: uuid::Uuid::new_v4().to_string(),
        target,
        token,
        platform_context: Some(PlatformContext::Wayland {
            activation_token: "synthetic-wayland".into(),
        }),
    }
}
fn launch() -> Launch {
    Launch::fixture(open(), "a".repeat(64))
}
fn finish(owned: &mut OwnedFrontend) -> bool {
    let until = Instant::now() + Duration::from_secs(5);
    let mut observed = false;
    while !owned.exited() {
        owned.poll();
        observed |= owned.image_observed();
        assert!(
            Instant::now() < until,
            "original private child did not exit"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(!owned.error());
    assert!(owned.exit.unwrap().success());
    observed
}
#[test]
fn original_wrapper_exec_exact_target_and_child_only_token_are_retained_to_exit() {
    let _serial = SERIAL.lock().unwrap();
    let f = Fixture::new(wire::NAVIGATION_HELP_MARKER);
    let mut owned = OwnedFrontend::launch(f.select(), launch()).unwrap();
    assert!(
        OwnedFrontend::launch(f.select(), launch()).is_err(),
        "one original admission"
    );
    assert!(
        finish(&mut owned),
        "actual held ELF observed on the original Child"
    );
    drop(owned);
    let mut next = OwnedFrontend::launch(f.select(), launch()).unwrap();
    assert!(finish(&mut next));
}
#[test]
fn startup_timeout_retains_original_child_until_actual_exit_without_a_second_launch() {
    let _serial = SERIAL.lock().unwrap();
    let f = Fixture::new(wire::NAVIGATION_HELP_MARKER);
    let mut owned = OwnedFrontend::launch(f.select(), launch()).unwrap();
    owned.until = Instant::now();
    owned.poll();
    assert!(owned.error());
    assert!(!owned.exited());
    assert!(owned.child.is_some());
    assert!(OwnedFrontend::launch(f.select(), launch()).is_err());
    let until = Instant::now() + Duration::from_secs(5);
    while !owned.exited() {
        owned.poll();
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(
        owned.exit_status().unwrap().success(),
        "timeout did not terminate the GUI"
    );
    drop(owned);
    assert!(!ACTIVE.load(Ordering::Acquire));
}

#[test]
fn missing_or_inexact_navigation_abi_refuses_before_target_start() {
    let _serial = SERIAL.lock().unwrap();
    for help in [
        "Usage: old frontend",
        "prefix Desktop target navigation ABI: desktop-target-v1",
    ] {
        let f = Fixture::new(help);
        assert!(OwnedFrontend::launch(f.select(), launch()).is_err());
        assert!(!ACTIVE.load(Ordering::Acquire));
    }
}
#[test]
fn changed_selection_or_configuration_refuses_and_no_token_enters_argv() {
    let _serial = SERIAL.lock().unwrap();
    let f = Fixture::new(wire::NAVIGATION_HELP_MARKER);
    let selected = f.select();
    let event = launch();
    let command = command(&selected, &event).unwrap();
    let args: Vec<_> = command.get_args().map(|v| v.to_str().unwrap()).collect();
    assert_eq!(args.len(), 5);
    assert!(!args.join(" ").contains("synthetic-wayland"));
    assert!(!args.join(" ").contains(&event.open().post_request_id));
    let env: std::collections::BTreeMap<_, _> = command.get_envs().collect();
    assert_eq!(
        env[std::ffi::OsStr::new("XDG_ACTIVATION_TOKEN")],
        Some(std::ffi::OsStr::new("synthetic-wayland"))
    );
    assert_eq!(env[std::ffi::OsStr::new("HGS_SESSION")], None);
    let mut no_context = open();
    no_context.platform_context = None;
    let absent = Launch::fixture(no_context, "a".repeat(64));
    assert!(command_env_removed(&selected, &absent));
    let foreign = Launch::fixture(open(), "b".repeat(64));
    assert!(OwnedFrontend::launch(f.select(), foreign).is_err());
    fs::write(selected.entry(), b"changed").unwrap();
    assert!(OwnedFrontend::launch(selected, event).is_err());
}
fn command_env_removed(selected: &SelectedFrontend, event: &Launch) -> bool {
    super::command(selected, event)
        .unwrap()
        .get_envs()
        .any(|(name, value)| name == "XDG_ACTIVATION_TOKEN" && value.is_none())
}
