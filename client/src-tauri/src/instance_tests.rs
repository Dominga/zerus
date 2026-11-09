// These subprocesses exercise the real kernel lock and activation listeners,
// with a test-supplied namespace. No test opens the login user's GUI endpoint.
const FIXTURE: &str = "instance::tests::canonical_tests::launch_fixture";
const CONTROL: &str = "ZERUS_GUI_INSTANCE_TEST_CONTROL";

fn private_root() -> Scratch {
    // Darwin's socket path limit must not depend on the runner's TMPDIR.
    let directory = PathBuf::from("/tmp").join(format!("zgui-{}", uuid::Uuid::new_v4().simple()));
    DirBuilder::new().mode(0o700).create(&directory).unwrap();
    Scratch(directory)
}

struct Child(std::process::Child);
impl Drop for Child {
    fn drop(&mut self) {
        if self.0.try_wait().unwrap().is_none() {
            self.0.kill().unwrap();
        }
        self.0.wait().unwrap();
    }
}

fn wait_for(path: &Path, child: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while !path.exists() {
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "fixture exited early"
        );
        assert!(Instant::now() < deadline, "fixture readiness deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn launch(root: &Path, scope: &str) -> Child {
    launch_with_quit_hold(root, scope, false)
}
fn launch_with_quit_hold(root: &Path, scope: &str, hold_after_quit: bool) -> Child {
    let home = root.join(scope);
    fs::create_dir_all(home.join("tmp")).unwrap();
    fs::create_dir_all(home.join("runtime")).unwrap();
    let control = root.join(format!("{scope}.json"));
    fs::write(
        &control,
        serde_json::to_vec(&serde_json::json!({
            "root": root, "scope": scope, "hold_after_quit":hold_after_quit
        }))
        .unwrap(),
    )
    .unwrap();
    Child(
        Command::new(std::env::current_exe().unwrap())
            // Never re-exec libtest without this exact filter.
            .args(["--exact", FIXTURE, "--nocapture"])
            .env(CONTROL, control)
            .env("HOME", &home)
            .env("TMPDIR", home.join("tmp"))
            .env("XDG_RUNTIME_DIR", home.join("runtime"))
            .env("XDG_CONFIG_HOME", home.join("xdg"))
            .env("XDG_STATE_HOME", home.join("log-state"))
            .env("HGS_CONFIG_DIR", home.join("config"))
            .env("HGS_STATE_DIR", home.join("state"))
            // The incident used this GUI mode; it must not bypass exclusion.
            .env("ZERUS_CLIENT_DESKTOP_SERVICE", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    )
}

#[test]
fn launch_fixture() {
    let Some(control) = std::env::var_os(CONTROL) else {
        return;
    };
    let control: serde_json::Value = serde_json::from_slice(&fs::read(control).unwrap()).unwrap();
    let root = Path::new(control["root"].as_str().unwrap());
    let scope = control["scope"].as_str().unwrap();
    // The production startup path, with only the OS namespace supplied by
    // the fixture. All temporary/configuration discovery remains real.
    let claimed = match claim_in(
        "hgs-tray",
        None,
        Activation::Sessions,
        None,
        &root.join("canonical"),
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
    };
    // This is the boundary before UI/desktop effects in the actual caller.
    fs::write(root.join(format!("{scope}.ready")), b"owner").unwrap();
    let deadline = Instant::now() + Duration::from_secs(12);
    while !root.join(format!("{scope}.stop")).exists() {
        assert!(Instant::now() < deadline, "fixture exit deadline");
        if let Ok(activation) = claimed.activations.recv_timeout(Duration::from_millis(20)) {
            assert_eq!(activation, Activation::Sessions);
            fs::write(root.join(format!("{scope}.activation")), activation.line()).unwrap();
        }
    }
    InstanceState(Mutex::new(Some(claimed.listening))).release();
    assert!(
        canonical::acquire(&root.join("canonical"))
            .unwrap()
            .is_none(),
        "final teardown must not release GUI exclusion before actual process exit"
    );
    if control["hold_after_quit"] == true {
        fs::write(root.join(format!("{scope}.released")), b"").unwrap();
        let deadline = Instant::now() + Duration::from_secs(8);
        while !root.join(format!("{scope}.exit")).exists() {
            assert!(Instant::now() < deadline, "quit fixture exit deadline");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[test]
fn two_tmpdirs_homes_and_configs_handoff_before_second_gui_effects() {
    let _process = crate::PROCESS_TREE
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = private_root();
    let mut owner = launch(&root, "first");
    wait_for(&root.join("first.ready"), &mut owner);
    let mut second = launch(&root, "second");
    wait_for(&root.join("second.forwarded"), &mut second);
    wait_for(&root.join("first.activation"), &mut owner);
    assert!(!root.join("second.ready").exists());
    assert!(!root.join("second.effects").exists());
    assert!(!root.join("second/tmp/hgs-tray").exists());
    assert!(!root.join("second/runtime/hgs-tray-start.lock").exists());
    assert_eq!(
        fs::read(root.join("first.activation")).unwrap(),
        b"sessions\n"
    );
    fs::write(root.join("first.stop"), b"").unwrap();
    assert!(owner.0.wait().unwrap().success());
    assert!(second.0.wait().unwrap().success());
    let text = fs::read_to_string(log_path(&root, "second")).unwrap();
    assert_eq!(text.lines().count(), 1);
    assert!(text.contains("outcome=forwarded reason=activation_written"));
}

#[test]
fn a_crash_releases_kernel_exclusion_and_replaces_only_the_stale_socket() {
    let _process = crate::PROCESS_TREE
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = private_root();
    let mut owner = launch(&root, "crashed");
    wait_for(&root.join("crashed.ready"), &mut owner);
    owner.0.kill().unwrap();
    owner.0.wait().unwrap();
    assert!(root.join("canonical/gui.lock").exists());
    assert!(root.join("canonical/gui.sock").exists());
    let mut next = launch(&root, "replacement");
    wait_for(&root.join("replacement.ready"), &mut next);
    assert!(!root.join("replacement.forwarded").exists());
    fs::write(root.join("replacement.stop"), b"").unwrap();
    assert!(next.0.wait().unwrap().success());
    assert!(root.join("canonical/gui.lock").exists());
    assert!(!root.join("canonical/gui.sock").exists());
}

#[test]
fn canonical_exclusion_is_a_lifetime_lock_and_not_a_pid_or_age_heuristic() {
    let root = private_root();
    let directory = root.join("canonical");
    let first = canonical::acquire(&directory).unwrap().unwrap();
    fs::write(directory.join("gui.lock"), b"1\nexpired pid metadata\n").unwrap();
    assert!(canonical::acquire(&directory).unwrap().is_none());
    drop(first);
    assert!(canonical::acquire(&directory).unwrap().is_some());
    assert_eq!(
        canonical::directory(),
        PathBuf::from(format!("/tmp/com.hgdev.hgs-tray-{}", unsafe {
            libc::geteuid()
        }))
    );
}

#[test]
fn headless_dispatch_remains_available_with_the_gui_lock_held() {
    let root = private_root();
    let directory = root.join("canonical");
    let _gui = canonical::acquire(&directory).unwrap().unwrap();
    assert!(canonical::acquire(&directory).unwrap().is_none());
    let status = crate::entry::notification_host(["--desktop-notification-host"], || {
        // Production supplies notification_host::run, which authenticates the
        // inherited service channel before any SDK use. No GUI admission.
        23
    });
    assert_eq!(status, Some(23));
    assert!(canonical::acquire(&directory).unwrap().is_none());
}

#[test]
fn an_owner_not_yet_listening_still_excludes_a_second_gui() {
    let root = private_root();
    let directory = root.join("canonical");
    let _guard = canonical::acquire(&directory).unwrap().unwrap();
    assert!(matches!(
        canonical_claim(&directory, Activation::Sessions).unwrap(),
        CanonicalClaim::Busy
    ));
    assert!(!directory.join("gui.sock").exists());
}

#[test]
fn unsafe_canonical_files_refuse_instead_of_starting_without_exclusion() {
    use std::os::unix::fs::symlink;
    let root = private_root();
    let directory = root.join("canonical");
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, Permissions::from_mode(0o755)).unwrap();
    assert!(canonical::acquire(&directory).is_err());
    fs::set_permissions(&directory, Permissions::from_mode(0o700)).unwrap();
    let target = root.join("untouched");
    fs::write(&target, b"original").unwrap();
    symlink(&target, directory.join("gui.lock")).unwrap();
    assert!(canonical::acquire(&directory).is_err());
    fs::remove_file(directory.join("gui.lock")).unwrap();
    let mut guard = canonical::acquire(&directory).unwrap().unwrap();
    symlink(&target, directory.join("gui.sock")).unwrap();
    assert!(guard.bind().is_err());
    assert_eq!(fs::read(&target).unwrap(), b"original");
}

fn log_path(root: &Path, scope: &str) -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        root.join(scope).join("Library/Logs/hgs-tray.log")
    }
    #[cfg(not(target_os = "macos"))]
    {
        root.join(scope).join("log-state/hgs/hgs-tray.log")
    }
}

#[test]
fn normal_process_quit_relaunches_immediately_and_after_twenty_seconds() {
    let _process = crate::PROCESS_TREE
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = private_root();
    for (index, delay) in [Duration::ZERO, Duration::from_secs(20)]
        .into_iter()
        .enumerate()
    {
        let first = format!("quit-{index}");
        let second = format!("relaunch-{index}");
        let mut owner = launch(&root, &first);
        wait_for(&root.join(format!("{first}.ready")), &mut owner);
        fs::write(root.join(format!("{first}.stop")), b"").unwrap();
        assert!(owner.0.wait().unwrap().success());
        let text = fs::read_to_string(log_path(&root, &first)).unwrap();
        assert!(text.contains("outcome=claimed reason=instance_acquired"));
        assert!(text.contains("outcome=quitting reason=listeners_closed_owner_retained"));
        std::thread::sleep(delay);
        let mut next = launch(&root, &second);
        wait_for(&root.join(format!("{second}.ready")), &mut next);
        assert!(!root.join(format!("{second}.forwarded")).exists());
        assert!(!root.join(format!("{second}.duplicate")).exists());
        assert!(fs::read_to_string(log_path(&root, &second))
            .unwrap()
            .contains("outcome=claimed reason=instance_acquired"));
        fs::write(root.join(format!("{second}.stop")), b"").unwrap();
        assert!(next.0.wait().unwrap().success());
    }
}

#[test]
fn a_normal_quit_finishing_during_the_handoff_wait_is_reacquired() {
    let _process = crate::PROCESS_TREE
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = private_root();
    let mut owner = launch_with_quit_hold(&root, "exiting", true);
    wait_for(&root.join("exiting.ready"), &mut owner);
    fs::write(root.join("exiting.stop"), b"").unwrap();
    wait_for(&root.join("exiting.released"), &mut owner);
    let mut observed = false;
    let result = canonical_claim_observed(&root.join("canonical"), Activation::Sessions, || {
        if !observed {
            observed = true;
            fs::write(root.join("exiting.exit"), b"").unwrap();
        }
    })
    .unwrap();
    assert!(
        observed,
        "the old process must still hold the kernel lock on the first attempt"
    );
    assert!(
        matches!(result, CanonicalClaim::Owner(_)),
        "handoff failure after actual exit must allow a new owner"
    );
    assert!(owner.0.wait().unwrap().success());
}

#[test]
fn every_guard_disposition_is_written_to_the_private_gui_log() {
    let _process = crate::PROCESS_TREE
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = private_root();
    let directory = root.join("canonical");
    let guard = canonical::acquire(&directory).unwrap().unwrap();
    let mut duplicate = launch(&root, "busy");
    wait_for(&root.join("busy.duplicate"), &mut duplicate);
    assert!(duplicate.0.wait().unwrap().success());
    let text = fs::read_to_string(log_path(&root, "busy")).unwrap();
    assert_eq!(text.lines().count(), 1);
    assert!(text.contains("outcome=duplicate reason=canonical_owner_unresponsive"));
    assert!(!text.contains("outcome=forwarded"));
    drop(guard);
    fs::set_permissions(&directory, Permissions::from_mode(0o755)).unwrap();
    let mut failed = launch(&root, "failed");
    assert!(!failed.0.wait().unwrap().success());
    let text = fs::read_to_string(log_path(&root, "failed")).unwrap();
    assert_eq!(text.lines().count(), 1);
    assert!(text.contains("outcome=failed reason=instance_claim_failed"));
    assert!(text.contains("unsafe canonical GUI directory"));
}

#[cfg(target_os = "linux")]
#[test]
fn a_full_activation_backlog_cannot_pin_startup_before_its_guard_log() {
    use socket2::{Domain, SockAddr, Socket, Type};
    let root = private_root();
    let path = root.join("full.sock");
    let address = SockAddr::unix(&path).unwrap();
    let listener = Socket::new(Domain::UNIX, Type::STREAM, None).unwrap();
    listener.bind(&address).unwrap();
    listener.listen(1).unwrap();
    let mut clients = Vec::new();
    let mut full = false;
    for _ in 0..16 {
        let client = Socket::new(Domain::UNIX, Type::STREAM, None).unwrap();
        client.set_nonblocking(true).unwrap();
        match client.connect(&address) {
            Ok(()) => clients.push(client),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                full = true;
                break;
            }
            Err(error) => panic!("backlog fixture: {error}"),
        }
    }
    assert!(full, "the isolated listen backlog must be saturated");
    let (sent, received) = mpsc::channel();
    let probe = std::thread::spawn(move || {
        sent.send(forward(&path, Activation::Sessions)).unwrap();
    });
    let result = received.recv_timeout(Duration::from_secs(1));
    // Close only fixture sockets before asserting, even if connect regresses.
    drop(listener);
    drop(clients);
    probe.join().unwrap();
    assert_eq!(
        result,
        Ok(false),
        "the activation connect must have a deadline"
    );
}
