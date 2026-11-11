#![cfg(windows)]
//! Native Windows filesystem, lock, configuration and generation controls.
//! Synthetic private state only; this suite neither installs nor contacts a relay.
use serde_json::json;
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::os::windows::io::AsHandle;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use zerus_mobile_connector::{
    config::Environment,
    fsguard::{probe_lock, LockState},
    journal::Journal,
    private_state::Store,
    start,
    windows_management::manage,
    workspace::Profile,
    workspace_cli,
    workspace_provenance::{admit, record_start, Receipt, Target, RECEIPT},
};
use zerus_windows_private::{identity_of, OriginalProcess, ProcessIdentity};
struct Scratch(PathBuf, tempfile::TempDir);
impl Scratch {
    fn new() -> Self {
        Self::with_prefix("zerus-mobile-windows-")
    }
    fn with_prefix(prefix: &str) -> Self {
        let owner = tempfile::Builder::new().prefix(prefix).tempdir().unwrap();
        let path = owner.path().join("private");
        zerus_windows_private::directory(&path, true).unwrap();
        Self(path, owner)
    }
}

#[test]
fn the_packaged_desktop_workspace_probe_reads_an_unconfigured_windows_profile() {
    let scratch = Scratch::new();
    for config in [
        scratch.0.join("connector.json"),
        scratch.0.join("absent/mobile/connector.json"),
    ] {
        let before: Vec<_> = std::fs::read_dir(&scratch.0)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        let result = std::process::Command::new(env!("CARGO_BIN_EXE_zerus-mobile-connector"))
            .args(["--workspace", "status", "--config"])
            .arg(&config)
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "workspace status: {:?} {:?}",
            result.status,
            String::from_utf8_lossy(&result.stderr)
        );
        let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(
            value,
            json!({"state":"unconfigured", "encrypted":false, "config_revision":""})
        );
        assert!(
            !config.exists(),
            "read-only status must not create a configuration"
        );
        let after: Vec<_> = std::fs::read_dir(&scratch.0)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(before, after);
    }
}
#[test]
fn equal_scratch_prefixes_keep_independent_owned_roots() {
    let first = Scratch::with_prefix("zerus-mobile-same-prefix-");
    let second = Scratch::with_prefix("zerus-mobile-same-prefix-");
    let first_root = first.1.path().to_owned();
    let second_root = second.1.path().to_owned();
    assert_ne!(first_root, second_root);
    let sentinel = second.0.join("sentinel");
    std::fs::write(&sentinel, b"second-owner").unwrap();
    drop(first);
    assert!(!first_root.exists());
    assert!(second_root.exists());
    assert_eq!(std::fs::read(&sentinel).unwrap(), b"second-owner");
    drop(second);
    assert!(!second_root.exists());
}
#[test]
fn private_files_reject_hardlinks_and_world_readable_credentials() {
    let scratch = Scratch::new();
    let path = scratch.0.join("credential.json");
    let file = zerus_windows_private::open_file(&path, true, true).unwrap();
    zerus_windows_private::verify(&file, false).unwrap();
    drop(file);
    let alias = scratch.0.join("alias");
    std::fs::hard_link(&path, &alias).unwrap();
    assert!(zerus_windows_private::open_file(&path, false, false).is_err());
    std::fs::remove_file(alias).unwrap();
    let result = std::process::Command::new("icacls")
        .arg(&path)
        .args(["/grant", "*S-1-1-0:(R)"])
        .output()
        .unwrap();
    assert!(result.status.success());
    assert!(zerus_windows_private::open_file(&path, false, false).is_err());
}
#[test]
fn journal_lock_has_one_writer_and_preserves_binding() {
    let scratch = Scratch::new();
    let state = scratch.0.join("receipts");
    let first = Journal::open(&state, "original-binding", 1.0).unwrap();
    assert!(Journal::open(&state, "original-binding", 2.0).is_err());
    assert_eq!(
        zerus_mobile_connector::fsguard::probe_lock(&state.join("connector.lock")).name(),
        "held"
    );
    drop(first);
    assert!(Journal::open(&state, "foreign-binding", 3.0).is_err());
    assert!(Journal::open(&state, "original-binding", 4.0).is_ok());
}
#[test]
fn inheritable_world_allow_refuses_existing_database_before_sidecar_creation() {
    let scratch = Scratch::new();
    let state = scratch.0.join("receipts");
    drop(Journal::open(&state, "fixture", 1.0).unwrap());
    let db = state.join("journal.sqlite3");
    let lock = state.join("connector.lock");
    for file in [&db, &lock] {
        let protected = std::process::Command::new("icacls")
            .arg(file)
            .args(["/inheritance:d"])
            .output()
            .unwrap();
        assert!(protected.status.success());
        zerus_windows_private::open_file(file, false, false).unwrap();
    }
    let before = std::fs::read(&db).unwrap();
    for suffix in ["-wal", "-shm", "-journal"] {
        assert!(!state.join(format!("journal.sqlite3{suffix}")).exists());
    }
    let result = std::process::Command::new("icacls")
        .arg(&state)
        .args(["/grant", "*S-1-1-0:(OI)(CI)(IO)(R)"])
        .output()
        .unwrap();
    assert!(result.status.success());
    // Both existing files remain private. Refusal cannot accidentally come
    // from the lock inheriting the unsafe ACE instead of directory admission.
    for file in [&db, &lock] {
        zerus_windows_private::open_file(file, false, false).unwrap();
    }
    assert!(Journal::open(&state, "fixture", 2.0).is_err());
    assert_eq!(std::fs::read(&db).unwrap(), before);
    for suffix in ["-wal", "-shm", "-journal"] {
        assert!(!state.join(format!("journal.sqlite3{suffix}")).exists());
    }
}
#[test]
fn saved_revision_cannot_win_a_held_child_admission() {
    let scratch = Scratch::new();
    let path = scratch.0.join("mobile/connector.json");
    let first=manage("save", &path, &json!({"revision":"","server_url":"https://relay.example.test","node_token":"synthetic-token-a"})).unwrap();
    let revision = first["revision"].as_str().unwrap().to_owned();
    let (release, held) = std::sync::mpsc::channel();
    let child_path = path.clone();
    // The child is held just before the exact admission routine called by
    // --admit-revision. No runtime/native/network object is constructed here.
    let child = std::thread::spawn(move || {
        held.recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        zerus_mobile_connector::start_bound(
            &child_path,
            false,
            &Environment::current().unwrap(),
            1.0,
            Some(&revision),
        )
        .is_err()
    });
    let second=manage("save", &path, &json!({"revision":first["revision"],"server_url":"https://relay.example.test","node_token":"synthetic-token-b"})).unwrap();
    release.send(()).unwrap();
    assert!(child.join().unwrap());
    assert_eq!(
        manage("status", &path, &json!({})).unwrap()["running"],
        false
    );
    for state in [&first["state_dir"], &second["state_dir"]] {
        let state = PathBuf::from(state.as_str().unwrap());
        for file in [
            "connector-control.json",
            "connector.lock",
            "journal.sqlite3",
        ] {
            assert!(!state.join(file).exists());
        }
    }
}
#[test]
fn configuration_is_redacted_revision_guarded_and_stop_is_generation_bound() {
    let scratch = Scratch::new();
    let path = scratch.0.join("mobile/connector.json");
    let missing = manage("status", &path, &json!({})).unwrap();
    assert_eq!(missing["configured"], false);
    assert!(!path.parent().unwrap().exists());
    let saved=manage("save",&path,&json!({"revision":"","server_url":"https://relay.example.test","node_token":"synthetic-windows-node-token"})).unwrap();
    assert_eq!(saved["configured"], true);
    assert!(!saved.to_string().contains("synthetic-windows-node-token"));
    assert!(manage(
        "save",
        &path,
        &json!({"revision":"","server_url":"https://relay.example.test","node_token":""})
    )
    .is_err());
    let environment = Environment::current().unwrap();
    let running = start(&path, false, &environment, 1.0).unwrap();
    let status = manage("status", &path, &json!({})).unwrap();
    assert_eq!(
        status["running"],
        true,
        "redacted status={status}; Started state={:?}; config={path:?} present={}; lock={:?} present={}; post-failure probe={:?}; state path matches={}",
        running.config.state_dir,
        path.exists(),
        running.config.state_dir.join("connector.lock"),
        running.config.state_dir.join("connector.lock").exists(),
        zerus_mobile_connector::fsguard::probe_lock(&running.config.state_dir.join("connector.lock")),
        status["state_dir"].as_str().map(PathBuf::from).as_ref() == Some(&running.config.state_dir),
    );
    assert_eq!(
        manage("start", &path, &json!({"revision":status["revision"]})).unwrap(),
        status
    );
    assert!(manage("save",&path,&json!({"revision":status["revision"],"server_url":"https://relay.example.test","node_token":""})).is_err());
    assert!(manage(
        "stop",
        &path,
        &json!({"revision":status["revision"],"generation":"stale"})
    )
    .is_err());
    let stopped = manage(
        "stop",
        &path,
        &json!({"revision":status["revision"],"generation":status["generation"]}),
    )
    .unwrap();
    assert_eq!(stopped["stopping"], true);
    drop(running);
    let restarted = start(&path, false, &environment, 2.0).unwrap();
    let fresh = manage("status", &path, &json!({})).unwrap();
    assert_ne!(fresh["generation"], status["generation"]);
    assert!(manage(
        "stop",
        &path,
        &json!({"revision":status["revision"],"generation":status["generation"]})
    )
    .is_err());
    drop(restarted);
    let rotated=manage("save",&path,&json!({"revision":saved["revision"],"server_url":"https://relay.example.test","node_token":"synthetic-replacement-token"})).unwrap();
    assert_ne!(rotated["state_dir"], saved["state_dir"]);
    assert!(PathBuf::from(saved["state_dir"].as_str().unwrap())
        .join("journal.sqlite3")
        .exists());
}

// zerus-9ysdg: which running connector a Stop may address, through the real binary.
// The relay address is reserved and never reached; the connector keeps retrying it.
const CONNECTOR: &str = env!("CARGO_BIN_EXE_zerus-mobile-connector");

struct Encrypted {
    config: PathBuf,
    state: PathBuf,
    revision: String,
    scratch: Scratch,
}

impl Encrypted {
    fn new() -> Self {
        use zerus_mobile_crypto::{
            encode, Identity, MembershipAction, MembershipState, MembershipStatement, Role,
            SignedMembership,
        };
        let scratch = Scratch::new();
        let config = scratch.0.join("connector.json");
        let private = scratch.0.join("workspace-private");
        let node = "00000000-0000-4000-8000-000000000350";
        let identity = Identity::generate(node.into(), Role::Computer).unwrap();
        let genesis = SignedMembership::sign(
            MembershipStatement {
                workspace_id: uuid::Uuid::new_v4().to_string(),
                revision: 0,
                previous_head: encode(&[0; 32]),
                issuer_device_id: node.into(),
                issuer_key_generation: 1,
                action: MembershipAction::Add,
                device: Some(identity.public().clone()),
                revoked_device_id: None,
            },
            &identity,
        )
        .unwrap();
        let membership = MembershipState::from_genesis(&genesis).unwrap();
        let state = private.join(format!("receipts-{node}"));
        let profile = Profile {
            server_url: "https://relay.example.test".into(),
            workspace_name: "Fixture".into(),
            computer_name: "Fixture".into(),
            node_token: encode(&[32; 32]),
            identity: identity.to_record(),
            historical_heads: std::collections::BTreeMap::from([(0, membership.head.clone())]),
            membership,
            state_dir: state.clone(),
        };
        let store = Store::lock(&private, true).unwrap();
        store
            .replace(
                "active.json",
                &Store::revision(None),
                &serde_json::to_vec(&profile).unwrap(),
            )
            .unwrap();
        drop(store);
        let revision = workspace_cli::replace_config(
            &config,
            "",
            &json!({"server_url":profile.server_url,"state_dir":state,"hgs_path":"hgs",
                "encrypted_workspace":{"version":1,"state_dir":private}}),
        )
        .unwrap();
        Self {
            config,
            state,
            revision,
            scratch,
        }
    }

    /// One `--workspace` call. Its reply goes to a file, because a Start's detached
    /// child inherits the handles of the call that launched it.
    fn call(&self, action: &str, input: &serde_json::Value) -> (i32, serde_json::Value) {
        let reply = tempfile::NamedTempFile::new_in(self.scratch.1.path()).unwrap();
        let mut command = Command::new(CONNECTOR);
        command
            .args(["--workspace", action, "--config"])
            .arg(&self.config)
            .stdin(Stdio::piped())
            .stdout(reply.reopen().unwrap())
            .stderr(Stdio::null());
        #[cfg(feature = "test-hooks")]
        if action == "start" {
            command.env(
                zerus_mobile_connector::hooks::STARTUP_STDERR,
                self.stderr_path(),
            );
        }
        let mut call = command.spawn().unwrap();
        let mut stdin = call.stdin.take().unwrap();
        stdin.write_all(input.to_string().as_bytes()).unwrap();
        drop(stdin);
        let code = call.wait().unwrap().code().unwrap();
        let bytes = std::fs::read(reply.path()).unwrap();
        (code, serde_json::from_slice(&bytes).unwrap())
    }

    fn stderr_path(&self) -> PathBuf {
        self.scratch.0.join("fixture-startup.stderr")
    }

    fn startup_diagnostics(&self) -> String {
        use std::io::Read;
        use zerus_mobile_connector::startup::{redacted_stderr, STDERR_LIMIT};
        let mut bytes = Vec::new();
        if let Ok(file) = std::fs::File::open(self.stderr_path()) {
            let _ = file.take(STDERR_LIMIT as u64).read_to_end(&mut bytes);
        }
        let diagnostics = redacted_stderr(&bytes);
        if diagnostics.is_empty() {
            "no recognized startup diagnostic".into()
        } else {
            diagnostics.join("; ")
        }
    }

    fn generation(&self) -> Option<String> {
        let (_, status) = self.call("status", &json!({}));
        (status["running"] == true)
            .then(|| status["generation"].as_str().map(str::to_owned))
            .flatten()
    }

    /// A generation other than `previous`. Just after a restart, status can still name
    /// the last run's generation (zerus-vsfy0), so every launch after the first must pass
    /// it. Prints each change in what status says: that's the evidence on a machine where
    /// these timings can't be watched.
    fn running(
        &self,
        label: &str,
        previous: Option<&str>,
        mut child: Option<&mut Child>,
    ) -> String {
        let started = Instant::now();
        let deadline = started + Duration::from_secs(60);
        let (mut stale, mut last) = (0, String::new());
        loop {
            if let Some(exit) = child.as_mut().and_then(|child| child.try_wait().unwrap()) {
                panic!(
                    "{label}: connector exited before publishing ({exit}); {}",
                    self.startup_diagnostics()
                );
            }
            let (code, status) = self.call("status", &json!({}));
            let reading = format!(
                "exit {code}, state {}, running {}, generation {}, error {}",
                status["connector_state"], status["running"], status["generation"], status["error"]
            );
            if reading != last {
                println!(
                    "{label}: status at {} ms: {reading}",
                    started.elapsed().as_millis()
                );
                last = reading;
            }
            if status["running"] == true {
                if let Some(generation) = status["generation"].as_str() {
                    if Some(generation) != previous {
                        if stale > 0 {
                            println!(
                                "{label}: status named the previous generation {stale} time(s) first"
                            );
                        }
                        return generation.to_owned();
                    }
                    stale += 1;
                }
            }
            assert!(
                Instant::now() < deadline,
                "{label}: the connector never ran; {}",
                self.startup_diagnostics()
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// What the private state shows now. Every branch of a plain Stop failure says the
    /// same text, so this is how a run tells them apart. Call it only once this run has
    /// published: it takes the state store for a moment.
    fn report(&self, label: &str, moment: &str) {
        let control = std::fs::read(self.state.join("workspace-control.json"))
            .map_err(|error| error.to_string())
            .and_then(|bytes| {
                serde_json::from_slice::<serde_json::Value>(&bytes).map_err(|e| e.to_string())
            });
        let control = match control {
            Ok(control) => format!(
                "generation {}, pid {}, birth {}",
                control["generation"], control["pid"], control["birth"]
            ),
            Err(error) => format!("unreadable: {error}"),
        };
        let (_, status) = self.call("status", &json!({}));
        let store = match Store::lock(&self.state, false) {
            Ok(_) => "free".to_owned(),
            Err(error) => format!("not taken: {error}"),
        };
        println!(
            "{label}: {moment}: control {control}; status generation {}; marker {}; journal lock {}; state store {store}",
            status["generation"],
            self.marker()
                .map_or("absent".to_owned(), |m| String::from_utf8_lossy(&m).into_owned()),
            probe_lock(&self.state.join("connector.lock")).name(),
        );
    }

    fn stopped(&self) {
        let deadline = Instant::now() + Duration::from_secs(60);
        while self.generation().is_some() {
            assert!(Instant::now() < deadline, "the connector never stopped");
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn marker(&self) -> Option<Vec<u8>> {
        std::fs::read(self.state.join("workspace-stop.json")).ok()
    }

    fn stop(&self, generation: &str) -> (i32, serde_json::Value) {
        self.call(
            "stop",
            &json!({"expected_revision":self.revision,"generation":generation}),
        )
    }

    /// Launched without a Start, and refused: nothing changes, and it still runs.
    fn direct_launch_is_refused(
        &self,
        label: &str,
        arguments: &[&std::ffi::OsStr],
        previous: Option<&str>,
    ) -> String {
        let marker = self.marker();
        let mut command = Command::new(CONNECTOR);
        command
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(feature = "test-hooks")]
        {
            let path = self.stderr_path();
            let file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&path)
                .unwrap();
            command
                .env(zerus_mobile_connector::hooks::STARTUP_STDERR, path)
                .stderr(file);
        }
        let mut direct = Direct(command.spawn().unwrap());
        println!("{label}: launched, previous generation {previous:?}");
        let generation = self.running(label, previous, Some(&mut direct.0));
        self.report(label, "before Stop");
        let (code, reply) = self.stop(&generation);
        println!("{label}: Stop on {generation}: exit {code}, {reply}");
        self.report(label, "after Stop");
        assert_eq!(code, 1, "{label}: {reply}");
        assert_eq!(reply["ok"], false, "{label}: {reply}");
        assert_eq!(
            reply["refusal"], "stop-refused-before-effect",
            "{label}: {reply}"
        );
        assert_eq!(reply["generation"], generation.as_str(), "{label}: {reply}");
        assert_eq!(self.marker(), marker, "{label}");
        assert_eq!(
            self.generation().as_deref(),
            Some(generation.as_str()),
            "{label}"
        );
        direct.0.kill().unwrap();
        direct.0.wait().unwrap();
        self.stopped();
        generation
    }

    /// Writes the Stop marker for the generation in the control record, or says why not.
    fn ask_to_stop(&self) -> Result<String, String> {
        let control = std::fs::read(self.state.join("workspace-control.json"))
            .map_err(|error| format!("control unreadable: {error}"))?;
        let control: serde_json::Value = serde_json::from_slice(&control)
            .map_err(|error| format!("control invalid: {error}"))?;
        let generation = control["generation"]
            .as_str()
            .ok_or("control has no generation")?
            .to_owned();
        let marker = json!({"version":1,"generation":generation}).to_string();
        let name = "workspace-stop.json";
        let store =
            Store::lock(&self.state, false).map_err(|error| format!("state store: {error}"))?;
        let previous = store
            .read(name)
            .map_err(|error| format!("marker unreadable: {error}"))?;
        store
            .replace(
                name,
                &Store::revision(previous.as_deref()),
                marker.as_bytes(),
            )
            .map_err(|error| format!("marker not written: {error}"))?;
        Ok(generation)
    }

    /// Ends a detached fixture connector through its own Stop marker, and never signals:
    /// a PID read from a file could be another process by now, and a stale control under
    /// another lock holder names a generation that holder ignores. Anything it can't
    /// confirm within the bound is an error, so a cleanup that didn't happen can't pass
    /// for one that did. Returns how many lock probes came back unknown on the way.
    fn end(&self) -> Result<u32, String> {
        let lock = self.state.join("connector.lock");
        let started = Instant::now();
        let deadline = started + Duration::from_secs(30);
        let mut unknown = 0;
        let mut asked: Option<String> = None;
        let (mut last_probe, mut last_failure) = (None, String::new());
        loop {
            let probe = probe_lock(&lock);
            if last_probe != Some(probe) {
                println!(
                    "cleanup: lock {} at {} ms",
                    probe.name(),
                    started.elapsed().as_millis()
                );
                last_probe = Some(probe);
            }
            match probe {
                LockState::Absent | LockState::Free => return Ok(unknown),
                LockState::Unknown => unknown += 1,
                LockState::Held => {
                    // Asks again if the control record moved on, as it does when the
                    // first read landed before this connector published.
                    let current = std::fs::read(self.state.join("workspace-control.json"))
                        .ok()
                        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                        .and_then(|control| control["generation"].as_str().map(str::to_owned));
                    if asked.is_none() || (current.is_some() && current != asked) {
                        match self.ask_to_stop() {
                            Ok(generation) => {
                                println!(
                                    "cleanup: marker for {generation} written at {} ms",
                                    started.elapsed().as_millis()
                                );
                                asked = Some(generation);
                            }
                            Err(failure) if failure != last_failure => {
                                println!("cleanup: marker not written: {failure}");
                                last_failure = failure;
                            }
                            Err(_) => {}
                        }
                    }
                }
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "lock {} after 30 s, marker for {asked:?}, {unknown} unknown probes, {last_failure}",
                    probe_lock(&lock).name()
                ));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

/// Now, in the FILETIME ticks the kernel's exit time uses.
fn wall_ticks() -> i64 {
    let unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    (unix.as_secs() as i64 + 11_644_473_600) * 10_000_000 + i64::from(unix.subsec_nanos() / 100)
}

/// A fixture connector this test launched itself.
struct Direct(Child);
impl Drop for Direct {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A failed assertion must not leave a detached fixture connector holding the test
/// runner's inherited handles. A cleanup that couldn't be confirmed fails the test, or
/// is reported if the test is already failing.
impl Drop for Encrypted {
    fn drop(&mut self) {
        if let Err(error) = self.end() {
            if std::thread::panicking() {
                eprintln!("fixture cleanup failed: {error}");
            } else {
                panic!("fixture cleanup failed: {error}");
            }
        }
    }
}

#[test]
fn cleanup_ends_a_detached_connector_through_its_own_stop_marker() {
    let encrypted = Encrypted::new();
    let (code, started) = encrypted.call("start", &json!({"expected_revision":encrypted.revision}));
    assert_eq!(code, 0, "{started}; {}", encrypted.startup_diagnostics());
    let private = encrypted.state.parent().unwrap();
    let bytes = std::fs::read(private.join("lifecycle").join(RECEIPT)).unwrap();
    let receipt: Receipt = serde_json::from_slice(&bytes).unwrap();
    let original = OriginalProcess::open(&ProcessIdentity {
        pid: receipt.pid,
        birth: receipt.birth,
        image: receipt.image,
    })
    .unwrap();
    original.verify().unwrap();
    let unknown = encrypted.end().unwrap();
    // The journal lock frees before the process has finished ending, so wait for the
    // exit itself, and record it from the kernel. An error is missing evidence, not an exit.
    let freed = wall_ticks();
    // What the check before zerus-wv3b1 would have seen at this moment, for comparison.
    let verify_now = original.verify();
    let waited = Instant::now();
    let exited = original.wait_for_exit(Duration::from_secs(30)).unwrap();
    println!(
        "cleanup: {unknown} unknown lock probes; verify at lock free: {}; waited {} us for the exit; exited {exited}",
        if verify_now.is_ok() { "still running" } else { "ended" },
        waited.elapsed().as_micros()
    );
    assert!(exited, "the connector is still running");
    let (code, at) = original.exit_record().unwrap();
    println!(
        "cleanup: exit code {code}, kernel exit time {} ms after the lock was seen free",
        (at as i64 - freed) / 10_000
    );
}

#[test]
fn only_the_exact_child_of_a_full_start_can_be_stopped() {
    let encrypted = Encrypted::new();
    let revision = OsString::from(&encrypted.revision);
    let config = encrypted.config.as_os_str();
    let copied_flag = [
        OsStr::new("--admit-revision"),
        revision.as_os_str(),
        OsStr::new("--config"),
        config,
    ];
    let bare = [OsStr::new("--config"), config];
    let first = encrypted.direct_launch_is_refused("copied flag", &copied_flag, None);
    let second = encrypted.direct_launch_is_refused("bare config", &bare, Some(&first));

    let (code, started) = encrypted.call("start", &json!({"expected_revision":encrypted.revision}));
    assert_eq!(code, 0, "{started}; {}", encrypted.startup_diagnostics());
    let generation = encrypted.running("full Start", Some(&second), None);
    assert_eq!(started["generation"], generation.as_str());
    encrypted.report("full Start", "before Stop");
    let (code, stopped) = encrypted.stop(&generation);
    println!("full Start: Stop on {generation}: exit {code}, {stopped}");
    assert_eq!(code, 0, "{stopped}");
    assert!(stopped.get("refusal").is_none(), "{stopped}");
    encrypted.stopped();
    let marker = encrypted.marker().unwrap();
    assert!(String::from_utf8_lossy(&marker).contains(&generation));

    // The earlier Start's receipt proves nothing about a later process, and this
    // refusal leaves the earlier marker byte for byte.
    encrypted.direct_launch_is_refused("replayed receipt", &copied_flag, Some(&generation));
    assert_eq!(encrypted.marker().unwrap(), marker);

    // A full Start again gets its own receipt.
    let (code, started) = encrypted.call("start", &json!({"expected_revision":encrypted.revision}));
    assert_eq!(code, 0, "{started}; {}", encrypted.startup_diagnostics());
    let again = encrypted.running("second full Start", Some(&generation), None);
    let (code, stopped) = encrypted.stop(&again);
    println!("second full Start: Stop on {again}: exit {code}, {stopped}");
    assert_eq!(code, 0, "{stopped}");
    encrypted.stopped();
}

#[test]
fn a_stop_admission_needs_the_receipt_the_control_and_the_running_process_to_agree() {
    let scratch = Scratch::new();
    let lifecycle = scratch.0.join("lifecycle");
    let store = Store::lock(&lifecycle, true).unwrap();
    let (config, state) = (scratch.0.join("connector.json"), scratch.0.join("state"));
    let target = Target {
        revision: "r",
        config: &config,
        state_dir: &state,
        node_id: "node",
    };
    let mut child = Command::new("cmd.exe")
        .args(["/C", "more"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let identity = identity_of(child.id(), child.as_handle()).unwrap();
    let control = json!({"version":1,"generation":"g","pid":identity.pid,"birth":identity.birth});

    // Absent: a launch no Start recorded.
    assert!(admit(&lifecycle, &target, &control).is_err());
    record_start(&store, &target, &identity).unwrap();
    let original = admit(&lifecycle, &target, &control).unwrap();
    original.verify().unwrap();

    // Another holder that replaced the control, and a reused PID's creation time.
    let another =
        json!({"version":1,"generation":"h","pid":std::process::id(),"birth":identity.birth});
    assert!(admit(&lifecycle, &target, &another).is_err());
    let reused =
        json!({"version":1,"generation":"g","pid":identity.pid,"birth":identity.birth + 1});
    assert!(admit(&lifecycle, &target, &reused).is_err());
    let stale = ProcessIdentity {
        birth: identity.birth + 1,
        ..identity.clone()
    };
    record_start(&store, &target, &stale).unwrap();
    let stale_control = json!({"version":1,"generation":"g","pid":stale.pid,"birth":stale.birth});
    assert!(admit(&lifecycle, &target, &stale_control).is_err());
    record_start(&store, &target, &identity).unwrap();

    // Another configuration.
    let other = Target {
        revision: "other",
        config: &config,
        state_dir: &state,
        node_id: "node",
    };
    assert!(admit(&lifecycle, &other, &control).is_err());

    // The original ended, even though this test still holds a handle to it.
    drop(child.stdin.take());
    child.wait().unwrap();
    assert!(original.verify().is_err());
    assert!(admit(&lifecycle, &target, &control).is_err());
}
