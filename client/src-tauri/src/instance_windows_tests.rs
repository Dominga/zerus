use std::{
    fs,
    path::PathBuf,
    process::{Child as Process, Command, Stdio},
};

struct Child(Process);
impl Drop for Child {
    fn drop(&mut self) {
        if self.0.try_wait().unwrap().is_none() {
            self.0.kill().unwrap();
        }
        self.0.wait().unwrap();
    }
}
struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("zerus-gui-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
    fn launch(&self, scope: &str, name: &str) -> Child {
        self.launch_with_quit_hold(scope, name, false)
    }
    fn launch_with_quit_hold(&self, scope: &str, name: &str, hold_after_quit: bool) -> Child {
        let home = self.0.join(scope);
        fs::create_dir_all(&home).unwrap();
        let control = self.0.join(format!("{scope}.json"));
        fs::write(
            &control,
            serde_json::to_vec(&serde_json::json!({
                "root": self.0, "scope": scope, "canonical": name,
                "hold_after_quit": hold_after_quit
            }))
            .unwrap(),
        )
        .unwrap();
        Child(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "instance::tests::canonical_tests::launch_fixture",
                    "--nocapture",
                ])
                .env("ZERUS_GUI_INSTANCE_TEST_CONTROL", control)
                .env("USERPROFILE", &home)
                .env("LOCALAPPDATA", &home)
                .env("TEMP", &home)
                .env("TMP", &home)
                .env("TMPDIR", &home)
                .env("HGS_CONFIG_DIR", home.join("config"))
                .env("HGS_STATE_DIR", home.join("state"))
                .env("ZERUS_CLIENT_DESKTOP_SERVICE", "1")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap(),
        )
    }
    fn wait_for(&self, suffix: &str, child: &mut Child) {
        let deadline = Instant::now() + crate::test_wait::window(10);
        while !self.0.join(suffix).exists() {
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "fixture exited early"
            );
            assert!(Instant::now() < deadline, "fixture readiness deadline");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[test]
fn launch_fixture() {
    let Some(control) = std::env::var_os("ZERUS_GUI_INSTANCE_TEST_CONTROL") else {
        return;
    };
    let control: serde_json::Value = serde_json::from_slice(&fs::read(control).unwrap()).unwrap();
    let root = Path::new(control["root"].as_str().unwrap());
    let scope = control["scope"].as_str().unwrap();
    let name = control["canonical"].as_str().unwrap();
    let claimed = match claim_in(
        "hgs-tray",
        Activation::Sessions,
        None,
        Some(name.into()),
        || {
            fs::write(root.join(format!("{scope}.effects")), b"renderer admitted").unwrap();
        },
    ) {
        Claim::Claimed(claimed) => claimed,
        Claim::Forwarded => {
            fs::write(root.join(format!("{scope}.forwarded")), b"clean exit").unwrap();
            return;
        }
        Claim::Duplicate(reason) => {
            fs::write(root.join(format!("{scope}.duplicate")), reason).unwrap();
            return;
        }
        Claim::Failed(error) => panic!("fixture GUI owner: {error}"),
        Claim::Untrusted => {
            fs::write(root.join(format!("{scope}.untrusted")), b"refused").unwrap();
            return;
        }
    };
    fs::write(root.join(format!("{scope}.ready")), b"owner").unwrap();
    let deadline = Instant::now() + crate::test_wait::window(15);
    while !root.join(format!("{scope}.stop")).exists() {
        assert!(Instant::now() < deadline, "fixture exit deadline");
        if let Ok(activation) = claimed.activations.recv_timeout(Duration::from_millis(20)) {
            assert_eq!(activation, Activation::Sessions);
            fs::write(root.join(format!("{scope}.activation")), b"sessions\n").unwrap();
        }
    }
    InstanceState(Mutex::new(Some(claimed.listening))).release();
    assert!(
        Canonical::acquire(name.into()).unwrap().is_none(),
        "final teardown must not release GUI exclusion before actual process exit"
    );
    if control["hold_after_quit"] == true {
        fs::write(root.join(format!("{scope}.released")), b"").unwrap();
        let deadline = Instant::now() + crate::test_wait::window(8);
        while !root.join(format!("{scope}.exit")).exists() {
            assert!(Instant::now() < deadline, "quit fixture exit deadline");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[test]
fn distinct_temp_profile_and_config_scopes_handoff_before_gui_effects() {
    let fixture = Fixture::new();
    let name = format!("zerus-canonical-test-{}", uuid::Uuid::new_v4());
    let mut owner = fixture.launch("first", &name);
    fixture.wait_for("first.ready", &mut owner);
    let mut second = fixture.launch("second", &name);
    fixture.wait_for("second.forwarded", &mut second);
    fixture.wait_for("first.activation", &mut owner);
    assert!(!fixture.0.join("second.ready").exists());
    assert!(!fixture.0.join("second.effects").exists());
    assert!(!fixture
        .0
        .join("second/hgdev/hgs-tray")
        .read_dir()
        .unwrap()
        .any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".instance-lock")));
    fs::write(fixture.0.join("first.stop"), b"").unwrap();
    assert!(owner.0.wait().unwrap().success());
    assert!(second.0.wait().unwrap().success());
    let text = fs::read_to_string(fixture.0.join("second/hgdev/hgs-tray/hgs-tray.log")).unwrap();
    assert_eq!(text.lines().count(), 1);
    assert!(text.contains("outcome=forwarded reason=activation_written"));
}

#[test]
fn normal_process_quit_relaunches_immediately_and_after_twenty_seconds() {
    let fixture = Fixture::new();
    let name = format!("zerus-canonical-quit-{}", uuid::Uuid::new_v4());
    for (index, delay) in [Duration::ZERO, Duration::from_secs(20)]
        .into_iter()
        .enumerate()
    {
        let first = format!("quit-{index}");
        let second = format!("relaunch-{index}");
        let mut owner = fixture.launch(&first, &name);
        fixture.wait_for(&format!("{first}.ready"), &mut owner);
        fs::write(fixture.0.join(format!("{first}.stop")), b"").unwrap();
        assert!(owner.0.wait().unwrap().success());
        let log =
            fs::read_to_string(fixture.0.join(&first).join("hgdev/hgs-tray/hgs-tray.log")).unwrap();
        assert!(log.contains("outcome=quitting reason=listeners_closed_owner_retained"));
        std::thread::sleep(delay);
        let mut next = fixture.launch(&second, &name);
        fixture.wait_for(&format!("{second}.ready"), &mut next);
        assert!(!fixture.0.join(format!("{second}.forwarded")).exists());
        assert!(!fixture.0.join(format!("{second}.duplicate")).exists());
        fs::write(fixture.0.join(format!("{second}.stop")), b"").unwrap();
        assert!(next.0.wait().unwrap().success());
    }
}

#[test]
fn a_normal_quit_finishing_during_the_handoff_wait_is_reacquired() {
    let fixture = Fixture::new();
    let name = format!("zerus-canonical-exit-{}", uuid::Uuid::new_v4());
    let mut owner = fixture.launch_with_quit_hold("exiting", &name, true);
    fixture.wait_for("exiting.ready", &mut owner);
    fs::write(fixture.0.join("exiting.stop"), b"").unwrap();
    fixture.wait_for("exiting.released", &mut owner);
    let mut observed = false;
    let result = canonical_claim_observed(name, &Activation::Sessions, || {
        if !observed {
            observed = true;
            fs::write(fixture.0.join("exiting.exit"), b"").unwrap();
        }
    })
    .unwrap();
    assert!(
        observed,
        "the exiting process must hold the first acquisition"
    );
    assert!(matches!(result, CanonicalClaim::Owner(_)));
    assert!(owner.0.wait().unwrap().success());
}

#[test]
fn an_unresponsive_owner_exits_as_duplicate_without_claiming_a_handoff() {
    let fixture = Fixture::new();
    let name = format!("zerus-canonical-busy-{}", uuid::Uuid::new_v4());
    let _guard = Canonical::acquire(name.clone()).unwrap().unwrap();
    let mut duplicate = fixture.launch("busy", &name);
    fixture.wait_for("busy.duplicate", &mut duplicate);
    assert!(duplicate.0.wait().unwrap().success());
    assert!(!fixture.0.join("busy.effects").exists());
    let text = fs::read_to_string(fixture.0.join("busy/hgdev/hgs-tray/hgs-tray.log")).unwrap();
    assert_eq!(text.lines().count(), 1);
    assert!(text.contains("outcome=duplicate reason=canonical_owner_unresponsive"));
    assert!(!text.contains("outcome=forwarded"));
}

#[test]
fn a_crash_releases_canonical_kernel_exclusion_without_pid_cleanup() {
    let fixture = Fixture::new();
    let name = format!("zerus-canonical-crash-{}", uuid::Uuid::new_v4());
    let mut owner = fixture.launch("crashed", &name);
    fixture.wait_for("crashed.ready", &mut owner);
    owner.0.kill().unwrap();
    owner.0.wait().unwrap();
    let mut next = fixture.launch("next", &name);
    fixture.wait_for("next.ready", &mut next);
    fs::write(fixture.0.join("next.stop"), b"").unwrap();
    assert!(next.0.wait().unwrap().success());
}

#[test]
fn canonical_name_uses_token_sid_and_lifetime_not_profile_configuration() {
    assert_eq!(
        Canonical::name("S-1-5-21-123"),
        "com.hgdev.hgs-tray.gui-S-1-5-21-123"
    );
    let name = format!("zerus-canonical-unit-{}", uuid::Uuid::new_v4());
    let guard = Canonical::acquire(name.clone()).unwrap().unwrap();
    assert!(Canonical::acquire(name.clone()).unwrap().is_none());
    assert_eq!(
        crate::entry::notification_host(["--desktop-notification-host"], || 23),
        Some(23)
    );
    drop(guard);
    assert!(Canonical::acquire(name).unwrap().is_some());
}
