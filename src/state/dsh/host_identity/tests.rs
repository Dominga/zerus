use super::*;
use std::os::unix::fs::symlink;

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
    fs::write(
        &script,
        "#!/bin/sh\nprintf called >> \"$HOME/launch-calls\"\nexit 41\n",
    )
    .unwrap();
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
        if matches!(case, "reused" | "legacy-same-pid") {
            let mut saved = published;
            // A valid, different native birth at the same currently live PID.
            #[cfg(target_os = "linux")]
            {
                saved["birth"] = (saved["birth"].as_str().unwrap().parse::<u64>().unwrap() + 1)
                    .to_string()
                    .into();
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
    } else {
        let error = ensure_host(&record).unwrap_err();
        if matches!(case.as_str(), "dead" | "reused" | "legacy-dead") {
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
    if !matches!(case.as_str(), "dead" | "reused" | "legacy-dead") {
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
