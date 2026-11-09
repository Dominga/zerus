use super::*;
use std::os::unix::fs::symlink;

type MarkerObserver = Box<dyn FnMut(&Path, StartPhase)>;
type ChildObserver = Box<dyn FnOnce(&mut Child)>;
thread_local! {
    static MARKER_OBSERVER: std::cell::RefCell<Option<MarkerObserver>> = const { std::cell::RefCell::new(None) };
    static CHILD_OBSERVER: std::cell::RefCell<Option<ChildObserver>> = const { std::cell::RefCell::new(None) };
}
pub(super) fn after_marker(dir: &Path, phase: StartPhase) {
    MARKER_OBSERVER.with_borrow_mut(|observer| {
        if let Some(observer) = observer {
            observer(dir, phase);
        }
    });
}
pub(super) fn after_child(child: &mut Child) {
    if let Some(observer) = CHILD_OBSERVER.with_borrow_mut(Option::take) {
        observer(child);
    }
}
struct Observers;
impl Drop for Observers {
    fn drop(&mut self) {
        MARKER_OBSERVER.with_borrow_mut(|observer| {
            observer.take();
        });
        CHILD_OBSERVER.with_borrow_mut(|observer| {
            observer.take();
        });
    }
}

struct Owned(Child);
impl Drop for Owned {
    fn drop(&mut self) {
        if self.0.try_wait().unwrap().is_none() {
            self.0.kill().unwrap();
        }
        self.0.wait().unwrap();
    }
}
fn host(dir: &Path) -> Owned {
    Owned(spawn(dir, Command::new("/bin/sleep").arg("300")).unwrap())
}
fn run(case: &str) {
    let temp = tempfile::tempdir().unwrap();
    let bin = temp.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let script = bin.join("dsh");
    let version = if case.starts_with("spawn-refused") {
        format!(
            "#!/bin/sh\nprintf called >> \"$HOME/launch-calls\"\nif [ \"$1\" = --version ]; then /bin/rm -- \"$0\"; printf '%s\\n' '{}'; exit 0; fi\nprintf unexpected >> \"$HOME/unexpected-host\"\nexit 41\n",
            REVIEWED_VERSIONS.last().unwrap()
        )
    } else {
        "#!/bin/sh\nprintf called >> \"$HOME/launch-calls\"\nexit 41\n".into()
    };
    fs::write(&script, version).unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(script, fs::Permissions::from_mode(0o700)).unwrap();
    let record = json!({"agent_home":temp.path().join("native")});
    let dir = temp.path().join("state/dsh/hosts").join(
        &format!(
            "{:x}",
            Sha256::digest(string(&record, "agent_home").as_bytes())
        )[..16],
    );
    private_dir(&dir).unwrap();
    // The parent retains the actual original Child even if the isolated probe
    // hangs or fails. No saved PID is adopted for fixture teardown.
    let mut original = (case != "irregular").then(|| host(&dir));
    let mut replacement = None;
    if let Some(original) = original.as_mut() {
        let published: Value = serde_json::from_slice(&fs::read(dir.join(FILE)).unwrap()).unwrap();
        assert_eq!(published["pid"], original.0.id());
        assert_eq!(
            published["birth"],
            crate::platform::process_identity(original.0.id())
                .unwrap()
                .birth
        );
        assert_eq!(
            fs::read_to_string(dir.join("host.pid")).unwrap(),
            original.0.id().to_string()
        );
        if case.starts_with("legacy") || case == "healthy" {
            fs::remove_file(dir.join(FILE)).unwrap();
        }
        if case == "refused" {
            drop(std::os::unix::net::UnixListener::bind(dir.join("host.sock")).unwrap());
        }
        if matches!(case, "dead" | "legacy-dead" | "legacy-rewrite") {
            original.0.kill().unwrap();
            original.0.wait().unwrap();
        }
        if case == "legacy-rewrite" {
            // An older launcher writes only host.pid after our original exits.
            let child = Owned(
                crate::platform::spawn_detached(Command::new("/bin/sleep").arg("300")).unwrap(),
            );
            atomic(&dir.join("host.pid"), &child.0.id().to_string()).unwrap();
            replacement = Some(child);
        }
        if case == "corrupt-birth" {
            let mut saved = published.clone();
            saved["birth"] = "not a native birth".into();
            atomic(&dir.join(FILE), &saved.to_string()).unwrap();
        }
        if matches!(
            case,
            "reused" | "legacy-same-pid" | "spawn-refused" | "spawn-refused-v1"
        ) {
            let mut saved = published.clone();
            // A valid, different native birth at the same currently live PID.
            #[cfg(target_os = "linux")]
            {
                saved["birth"] = (saved["birth"].as_str().unwrap().parse::<u64>().unwrap() + 1)
                    .to_string()
                    .into();
            }
            if case == "spawn-refused-v1" {
                saved["version"] = 1.into();
                saved.as_object_mut().unwrap().remove("boot");
            }
            #[cfg(target_os = "macos")]
            {
                let (sec, usec) = saved["birth"].as_str().unwrap().split_once(':').unwrap();
                saved["birth"] = format!("{}:{}", sec.parse::<u64>().unwrap() + 1, usec).into();
            }
            atomic(&dir.join(FILE), &saved.to_string()).unwrap();
            if case == "legacy-same-pid" {
                atomic(&dir.join("host.pid"), &original.0.id().to_string()).unwrap();
            }
        }
        if matches!(case, "old-boot" | "legacy-old-boot") {
            // Model an older boot with the SAME current PID and birth ticks.
            // This is not an actual reboot or native PID-reuse claim.
            let mut saved = published;
            saved["boot"] = other_boot().into();
            atomic(&dir.join(FILE), &saved.to_string()).unwrap();
            if case == "legacy-old-boot" {
                atomic(&dir.join("host.pid"), &original.0.id().to_string()).unwrap();
            }
        }
    }
    let identity_before = fs::read(dir.join(FILE)).ok();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "state::dsh::host_identity::tests::isolated_host_probe",
            "--exact",
            "--nocapture",
        ])
        .env_clear()
        .env("HOME", temp.path())
        .env("PATH", bin)
        .env("HGS_STATE_DIR", temp.path().join("state"))
        .env("HGS_CONFIG_DIR", temp.path().join("config"))
        .env("HGS_TEST_DSH_HOST", case)
        .current_dir(temp.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let until = Instant::now() + Duration::from_secs(45);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= until {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!(
                "{case}: fixture did not settle: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{case}: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if case != "irregular" {
        assert_eq!(
            fs::read(dir.join(FILE)).ok(),
            identity_before,
            "checking an existing host must not rewrite its identity"
        );
    }
    if case != "irregular" && !matches!(case, "dead" | "legacy-dead" | "legacy-rewrite") {
        assert!(
            original.as_mut().unwrap().0.try_wait().unwrap().is_none(),
            "the original must remain untouched"
        );
    }
    if let Some(replacement) = replacement.as_mut() {
        assert!(replacement.0.try_wait().unwrap().is_none());
    }
}

#[test]
fn a_later_legacy_launcher_cannot_hide_behind_a_stale_birth_record() {
    run("legacy-rewrite");
}

#[test]
fn a_legacy_rewrite_of_the_same_pid_invalidates_an_old_birth_record() {
    run("legacy-same-pid");
}
#[test]
fn a_corrupt_birth_cannot_masquerade_as_pid_reuse() {
    run("corrupt-birth");
}

#[test]
fn missing_adapter_never_replaces_a_live_original() {
    run("live");
}
#[test]
fn refused_adapter_never_replaces_a_live_original() {
    run("refused");
}
#[test]
fn a_dead_original_allows_normal_startup() {
    run("dead");
}
#[test]
fn a_reused_pid_allows_startup_without_touching_its_current_process() {
    run("reused");
}
#[test]
fn an_old_boot_cannot_identify_current_pid_and_ticks_as_its_live_original() {
    run("old-boot");
}
#[test]
fn an_old_boot_never_hides_a_current_legacy_publication() {
    run("legacy-old-boot");
}
#[test]
fn known_spawn_refusal_preserves_the_original_reused_pid_proof_and_retry() {
    run("spawn-refused");
    run("spawn-refused-v1");
}
#[test]
fn legacy_live_pid_without_birth_is_not_restart_authority() {
    run("legacy-live");
}
#[test]
fn legacy_dead_pid_allows_normal_startup() {
    run("legacy-dead");
}
#[test]
fn a_healthy_legacy_adapter_needs_no_record_rewrite_or_restart() {
    run("healthy");
}

#[test]
fn isolated_host_probe() {
    let Ok(case) = std::env::var("HGS_TEST_DSH_HOST") else {
        return;
    };
    if case == "irregular" {
        irregular();
        return;
    }
    let record = json!({"agent_home":home().join("native")});
    let dir = host_dir(&record);
    private_dir(&dir).unwrap();
    if case == "healthy" {
        use std::io::{BufRead, BufReader};
        let listener = std::os::unix::net::UnixListener::bind(dir.join("host.sock")).unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&line).unwrap()["method"],
                "ping"
            );
            writeln!(
                stream,
                "{}",
                json!({"ok":true,"value":{"protocol":1,"version":ADAPTER_VERSION}})
            )
            .unwrap();
        });
        assert!(ensure_host(&record).is_ok());
        server.join().unwrap();
        assert!(!dir.join(FILE).exists());
    } else if case.starts_with("spawn-refused") {
        let original = fs::read(dir.join(FILE)).unwrap();
        let pid_file = legacy(&dir).unwrap();
        assert!(before_start(&dir).is_ok());
        let error = ensure_host(&record).unwrap_err();
        assert!(error.contains("os error 2"), "{error}");
        assert_eq!(fs::read(dir.join(FILE)).unwrap(), original);
        assert_eq!(legacy(&dir).unwrap(), pid_file);
        assert!(!home().join("unexpected-host").exists());
        let script = home().join("bin/dsh");
        assert!(!script.exists());
        fs::write(
            &script,
            "#!/bin/sh\nprintf called >> \"$HOME/launch-calls\"\nexit 41\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(before_start(&dir).is_ok());
        let retry = ensure_host(&record).unwrap_err();
        assert!(retry.contains("installed version differs"), "{retry}");
        assert_eq!(
            fs::read_to_string(home().join("launch-calls")).unwrap(),
            "calledcalled"
        );
    } else {
        let error = ensure_host(&record).unwrap_err();
        if matches!(
            case.as_str(),
            "dead" | "reused" | "legacy-dead" | "old-boot"
        ) {
            // Reached the real startup call site and its installed-version check.
            assert!(error.contains("installed version differs"), "{error}");
            assert_eq!(
                fs::read_to_string(home().join("launch-calls")).unwrap(),
                "called"
            );
        } else if case == "corrupt-birth" || case.starts_with("legacy") {
            assert_eq!(error, UNVERIFIABLE);
        } else {
            assert_eq!(error, STILL_RUNNING);
        }
    }
    if !matches!(
        case.as_str(),
        "dead" | "reused" | "legacy-dead" | "old-boot" | "spawn-refused" | "spawn-refused-v1"
    ) {
        assert!(!home().join("launch-calls").exists());
    }
}

#[test]
fn incomplete_corrupt_or_irregular_identity_is_never_an_absent_host() {
    run("irregular");
}
fn irregular() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    assert!(before_start(dir).is_ok());
    for bytes in [
        "",
        "{}",
        "{",
        "{\"state\":\"starting\",\"version\":1}",
        "{\"state\":\"running\",\"version\":1,\"pid\":1,\"birth\":\"not a birth\"}",
    ] {
        atomic(&dir.join(FILE), bytes).unwrap();
        assert_eq!(before_start(dir).unwrap_err(), UNVERIFIABLE);
    }
    fs::write(dir.join(FILE), vec![b'x'; 1025]).unwrap();
    assert_eq!(before_start(dir).unwrap_err(), UNVERIFIABLE);
    fs::remove_file(dir.join(FILE)).unwrap();
    let other = dir.join("other");
    atomic(&other, "{}").unwrap();
    symlink(&other, dir.join(FILE)).unwrap();
    assert_eq!(before_start(dir).unwrap_err(), UNVERIFIABLE);
    fs::remove_file(dir.join(FILE)).unwrap();
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(dir.join(FILE).as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    assert_eq!(before_start(dir).unwrap_err(), UNVERIFIABLE);
}

#[test]
fn failed_spawn_clears_only_its_known_unstarted_marker() {
    let temp = tempfile::tempdir().unwrap();
    assert!(spawn(temp.path(), &mut Command::new(temp.path().join("absent"))).is_err());
    assert!(!temp.path().join(FILE).exists());
    assert!(before_start(temp.path()).is_ok());
}

fn other_boot() -> String {
    let first = "11111111-1111-4111-8111-111111111111";
    if boot().unwrap() == first {
        "22222222-2222-4222-8222-222222222222".into()
    } else {
        first.into()
    }
}

fn crash_marker(phase: &str, dir: &Path) {
    let mut probe = Owned(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "state::dsh::host_identity::tests::isolated_starting_crash",
                "--exact",
                "--test-threads=1",
                "--nocapture",
            ])
            .env_clear()
            .env("HOME", dir)
            .env("HGS_TEST_DSH_CRASH_DIR", dir)
            .env("HGS_TEST_DSH_CRASH_PHASE", phase)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    // The original fixture Child waits for this one release; retain its birth
    // before allowing the real startup path to publish and exit.
    let identity = crate::platform::process_identity(probe.0.id()).unwrap();
    probe.0.stdin.take().unwrap().write_all(b"g").unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = probe.0.try_wait().unwrap() {
            assert_eq!(status.code(), Some(76));
            break;
        }
        assert!(
            Instant::now() < until,
            "original crash fixture did not exit"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    let saved: Value = serde_json::from_slice(&fs::read(dir.join(FILE)).unwrap()).unwrap();
    assert_eq!(saved["launcher"]["pid"], probe.0.id());
    assert_eq!(saved["launcher"]["birth"], identity.birth);
    assert_eq!(saved["boot"], boot().unwrap());
    assert_eq!(saved["phase"], phase);
    assert!(
        !dir.join("spawned").exists(),
        "no host command ran before the crash"
    );
}

#[test]
fn isolated_starting_crash() {
    let Some(dir) = std::env::var_os("HGS_TEST_DSH_CRASH_DIR") else {
        return;
    };
    let dir = PathBuf::from(dir);
    let phase = std::env::var("HGS_TEST_DSH_CRASH_PHASE").unwrap();
    let mut gate = [0];
    io::stdin().read_exact(&mut gate).unwrap();
    assert_eq!(gate, [b'g']);
    MARKER_OBSERVER.with_borrow_mut(|hook| {
        *hook = Some(Box::new(move |_, seen| {
            if (phase == "prepared" && seen == StartPhase::Prepared)
                || (phase == "spawning" && seen == StartPhase::Spawning)
            {
                std::process::exit(76);
            }
        }))
    });
    let script = dir.join("fake-host");
    fs::write(
        &script,
        "#!/bin/sh\nprintf started > \"$HOME/spawned\"\nexec /bin/sleep 1\n",
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    if let Ok(mut child) = spawn(&dir, &mut Command::new(&script)) {
        child.wait().unwrap();
    }
    panic!("original publication observer did not end the launcher");
}

#[test]
fn a_dead_launcher_before_the_possible_spawn_boundary_can_start_again() {
    let dir = tempfile::tempdir().unwrap();
    crash_marker("prepared", dir.path());
    let before = fs::read(dir.path().join(FILE)).unwrap();
    assert!(before_start(dir.path()).is_ok());
    // A known unstarted retry preserves the recovered proof for another try.
    assert!(spawn(dir.path(), &mut Command::new(dir.path().join("absent"))).is_err());
    assert_eq!(fs::read(dir.path().join(FILE)).unwrap(), before);
    assert!(before_start(dir.path()).is_ok());
    let _original = host(dir.path());
    assert_eq!(before_start(dir.path()).unwrap_err(), STILL_RUNNING);
}

#[test]
fn a_dead_launcher_after_the_possible_spawn_boundary_is_still_unknown() {
    let dir = tempfile::tempdir().unwrap();
    crash_marker("spawning", dir.path());
    let before = fs::read(dir.path().join(FILE)).unwrap();
    // Model a detached host with no published PID. It is a retained synthetic
    // Child, never an adopted PID or an owner host.
    let mut original =
        Owned(crate::platform::spawn_detached(Command::new("/bin/sleep").arg("300")).unwrap());
    for _ in 0..2 {
        assert_eq!(before_start(dir.path()).unwrap_err(), UNCONFIRMED);
    }
    assert!(original.0.try_wait().unwrap().is_none());
    assert_eq!(fs::read(dir.path().join(FILE)).unwrap(), before);
}

#[test]
fn an_older_boot_retires_the_start_but_never_a_new_legacy_publication() {
    let dir = tempfile::tempdir().unwrap();
    crash_marker("spawning", dir.path());
    let mut saved: Value =
        serde_json::from_slice(&fs::read(dir.path().join(FILE)).unwrap()).unwrap();
    saved["boot"] = other_boot().into();
    atomic(&dir.path().join(FILE), &saved.to_string()).unwrap();
    assert!(before_start(dir.path()).is_ok());
    assert!(spawn(dir.path(), &mut Command::new(dir.path().join("absent"))).is_err());
    assert!(before_start(dir.path()).is_ok());
    let before = fs::read(dir.path().join(FILE)).unwrap();
    let mut original =
        Owned(crate::platform::spawn_detached(Command::new("/bin/sleep").arg("300")).unwrap());
    atomic(&dir.path().join("host.pid"), &original.0.id().to_string()).unwrap();
    assert_eq!(before_start(dir.path()).unwrap_err(), UNVERIFIABLE);
    assert!(original.0.try_wait().unwrap().is_none());
    assert_eq!(fs::read(dir.path().join(FILE)).unwrap(), before);
}

#[test]
fn both_live_launcher_phases_refuse_and_missing_or_corrupt_evidence_is_not_absence() {
    let dir = tempfile::tempdir().unwrap();
    let observations = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let saved = observations.clone();
    let _observers = Observers;
    MARKER_OBSERVER.with_borrow_mut(|hook| {
        *hook = Some(Box::new(move |dir, _| {
            assert_eq!(before_start(dir).unwrap_err(), UNCONFIRMED);
            saved
                .lock()
                .unwrap()
                .push(fs::read(dir.join(FILE)).unwrap());
        }))
    });
    assert!(spawn(dir.path(), &mut Command::new(dir.path().join("absent"))).is_err());
    let rows = observations.lock().unwrap();
    assert_eq!(rows.len(), 2);
    let prepared: Value = serde_json::from_slice(&rows[0]).unwrap();
    for field in [
        "boot", "launcher", "phase", "pid_file", "previous", "attempt",
    ] {
        let mut missing = prepared.clone();
        missing.as_object_mut().unwrap().remove(field);
        atomic(&dir.path().join(FILE), &missing.to_string()).unwrap();
        assert_eq!(
            before_start(dir.path()).unwrap_err(),
            UNVERIFIABLE,
            "{field}"
        );
    }
    for (field, value) in [
        ("boot", json!("not a boot")),
        ("attempt", json!("00000000-0000-0000-0000-000000000000")),
        ("launcher", json!({"pid":0,"birth":"bad"})),
        ("phase", json!("done")),
    ] {
        let mut corrupt = prepared.clone();
        corrupt[field] = value;
        atomic(&dir.path().join(FILE), &corrupt.to_string()).unwrap();
        assert_eq!(
            before_start(dir.path()).unwrap_err(),
            UNVERIFIABLE,
            "{field}"
        );
    }
}

#[test]
fn isolated_empty_host_fixture() {}

#[test]
fn an_unidentified_successful_exec_stays_unknown_even_after_the_original_exits() {
    let dir = tempfile::tempdir().unwrap();
    let _observers = Observers;
    CHILD_OBSERVER.with_borrow_mut(|hook| {
        *hook = Some(Box::new(|child| {
            child.wait().unwrap();
        }))
    });
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "state::dsh::host_identity::tests::isolated_empty_host_fixture",
            "--exact",
            "--test-threads=1",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    assert_eq!(spawn(dir.path(), &mut command).unwrap_err(), UNCONFIRMED);
    assert!(!dir.path().join("host.pid").exists());
    assert_eq!(before_start(dir.path()).unwrap_err(), UNCONFIRMED);
}

#[test]
fn a_replaced_starting_record_is_not_overwritten_by_rollback() {
    let dir = tempfile::tempdir().unwrap();
    let saved = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let observed = saved.clone();
    let owned = std::sync::Arc::new(std::sync::Mutex::new(None));
    let capture = owned.clone();
    let _observers = Observers;
    MARKER_OBSERVER.with_borrow_mut(|hook| {
        *hook = Some(Box::new(move |dir, phase| {
            if phase == StartPhase::Spawning {
                let bytes = fs::read(dir.join(FILE)).unwrap();
                let Record::Starting(original) = parse_record(&bytes).unwrap() else {
                    panic!("original starting record");
                };
                *capture.lock().unwrap() = Some(original);
                let mut record: Value = serde_json::from_slice(&bytes).unwrap();
                record["attempt"] = uuid::Uuid::new_v4().to_string().into();
                atomic(&dir.join(FILE), &record.to_string()).unwrap();
                *observed.lock().unwrap() = fs::read(dir.join(FILE)).unwrap();
            }
        }))
    });
    assert_eq!(
        spawn(dir.path(), &mut Command::new(dir.path().join("absent"))).unwrap_err(),
        UNVERIFIABLE
    );
    assert_eq!(
        &fs::read(dir.path().join(FILE)).unwrap(),
        &*saved.lock().unwrap()
    );
    assert_eq!(
        restore_unstarted(dir.path(), owned.lock().unwrap().as_ref().unwrap(), &None).unwrap_err(),
        UNVERIFIABLE
    );
    assert_eq!(
        &fs::read(dir.path().join(FILE)).unwrap(),
        &*saved.lock().unwrap()
    );
    assert_eq!(before_start(dir.path()).unwrap_err(), UNCONFIRMED);
}

#[test]
fn a_legacy_publication_before_spawn_vetoes_the_original_without_signalling_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut original =
        Owned(crate::platform::spawn_detached(Command::new("/bin/sleep").arg("300")).unwrap());
    let pid = original.0.id();
    let _observers = Observers;
    MARKER_OBSERVER.with_borrow_mut(|hook| {
        *hook = Some(Box::new(move |dir, phase| {
            if phase == StartPhase::Spawning {
                atomic(&dir.join("host.pid"), &pid.to_string()).unwrap();
            }
        }))
    });
    match spawn(dir.path(), Command::new("/bin/sleep").arg("1")) {
        Err(error) => assert_eq!(error, UNVERIFIABLE),
        Ok(child) => {
            let _unexpected = Owned(child); // Retain/reap even a regression's own child.
            panic!("changed legacy evidence admitted another fixture");
        }
    }
    assert!(!dir.path().join(FILE).exists());
    assert_eq!(legacy(dir.path()).unwrap().unwrap().0, pid);
    assert!(original.0.try_wait().unwrap().is_none());
    assert_eq!(before_start(dir.path()).unwrap_err(), UNVERIFIABLE);
}

#[test]
fn legacy_running_has_no_synthetic_null_boot_domain() {
    let mut saved = json!({"state":"running","version":1,"pid":42,"birth":"1","pid_file":[1,2,3,4],"boot":null});
    assert!(parse_record(&serde_json::to_vec(&saved).unwrap()).is_err());
    saved.as_object_mut().unwrap().remove("boot");
    assert!(parse_record(&serde_json::to_vec(&saved).unwrap()).is_ok());
}
