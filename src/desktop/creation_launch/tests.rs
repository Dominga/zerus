use super::*;
use crate::desktop::store::{Location, Store};
use rusqlite::TransactionBehavior;
use serde_json::json;
use zerus_client_core::desktop::creation_launch::{self as wire, Intent, Phase, Verified};

const MACHINE: &str = "22222222-2222-4222-8222-222222222222";

fn original(configuration: &str, epoch: &str) -> Verified {
    let creation = uuid::Uuid::new_v4().to_string();
    let value = json!({"id":creation,"creation_schema":1,"created":1,"revision":6,
        "cmd":"codex","tag":"example","host":"","state":"launching","auto":false,
        "target":"/srv/repo","launch_id":creation,"launch_dispatched":true,
        "expected_machine":MACHINE});
    Intent {
        scope: wire::Scope {
            configuration: configuration.into(),
            service_epoch: epoch.into(),
        },
        creation_id: creation.clone(),
        launch_id: creation,
        revision: 6,
        original: value.to_string(),
    }
    .verify()
    .unwrap()
}
fn fixture() -> (tempfile::TempDir, Location, Store, String) {
    let temp = tempfile::tempdir().unwrap();
    let location = Location {
        root: temp.path().canonicalize().unwrap().join("state/desktop"),
        configuration: "a".repeat(64),
    };
    location.prepare().unwrap();
    let epoch = uuid::Uuid::new_v4().to_string();
    let store = Store::open(&location, &epoch).unwrap();
    (temp, location, store, epoch)
}

#[test]
fn exact_previous_store_upgrade_adds_only_the_independent_launch_leaf() {
    assert_eq!(
        SCHEMA - 1,
        crate::desktop::creation_worktree::SCHEMA,
        "rebase this test onto the actual merge-order predecessor"
    );
    let (_temp, location, store, _) = fixture();
    remove_fixture(&store.db);
    store
        .db
        .pragma_update(None, "user_version", SCHEMA - 1)
        .unwrap();
    let tables = |db: &rusqlite::Connection| -> i64 {
        db.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE type='table'",
            [],
            |r| r.get(0),
        )
        .unwrap()
    };
    let before = tables(&store.db);
    drop(store);
    let store = Store::open(&location, &uuid::Uuid::new_v4().to_string()).unwrap();
    assert_eq!(
        store
            .db
            .query_row::<i64, _, _>("PRAGMA user_version", [], |r| r.get(0))
            .unwrap(),
        crate::desktop::store::SCHEMA
    );
    assert_eq!(tables(&store.db), before + 2);
    assert_eq!(ledger::undrained(&store.db).unwrap(), 0);
}

fn admit(store: &mut Store, verified: &Verified, writer: &str) -> ledger::Row {
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    let row = ledger::admit(&tx, verified, writer, 1).unwrap();
    tx.commit().unwrap();
    row
}

#[test]
fn admission_is_immutable_and_same_id_returns_only_its_original_receipt() {
    let (_temp, location, mut store, epoch) = fixture();
    let verified = original(&location.configuration, &epoch);
    let writer = uuid::Uuid::new_v4().to_string();
    let row = admit(&mut store, &verified, &writer);
    assert_eq!(row.receipt.phase, Phase::Queued);
    let again = admit(&mut store, &verified, &writer);
    assert_eq!(again.receipt, row.receipt);
    assert_eq!(ledger::undrained(&store.db).unwrap(), 1);
    assert_eq!(store.operation_undrained().unwrap(), 1);
    assert_eq!(
        store.operation_status()["kinds"]["creation_launch"]["undrained"],
        1
    );
    for statement in [
        "UPDATE desktop_creation_launches SET source='{}'",
        "DELETE FROM desktop_creation_launches",
    ] {
        assert!(store.db.execute(statement, []).is_err(), "{statement}");
    }
    let mut intent = verified.intent;
    let mut value: serde_json::Value = serde_json::from_str(&intent.original).unwrap();
    value["opaque_future"] = json!("changed original");
    intent.original = value.to_string();
    let changed = intent.verify().unwrap();
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    assert!(ledger::admit(&tx, &changed, &writer, 3).is_err());
    drop(tx);
    assert_eq!(
        ledger::load(&store.db, &changed.plan.launch_id)
            .unwrap()
            .unwrap()
            .receipt
            .binding,
        row.receipt.binding
    );
}

#[test]
fn capacity_refusal_is_atomic_and_retirement_never_rearms_a_queued_original() {
    let (_temp, location, mut store, epoch) = fixture();
    let verified = original(&location.configuration, &epoch);
    let writer = uuid::Uuid::new_v4().to_string();
    store
        .db
        .execute(
            "UPDATE desktop_creation_launch_budget SET bytes=16777216",
            [],
        )
        .unwrap();
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    assert!(ledger::admit(&tx, &verified, &writer, 1).is_err());
    drop(tx);
    assert!(ledger::load(&store.db, &verified.plan.launch_id)
        .unwrap()
        .is_none());
    store
        .db
        .execute("UPDATE desktop_creation_launch_budget SET bytes=0", [])
        .unwrap();
    let row = admit(&mut store, &verified, &writer);
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    ledger::retire(&tx, &row).unwrap();
    tx.commit().unwrap();
    assert_eq!(ledger::undrained(&store.db).unwrap(), 0);
    let retired = ledger::load(&store.db, &verified.plan.launch_id)
        .unwrap()
        .unwrap();
    assert_eq!(retired.receipt.phase, Phase::Unknown);
    assert!(retired.receipt.drained);
    let replay = admit(&mut store, &verified, &writer);
    assert_eq!(replay.receipt, retired.receipt);
}

/// A reserved, running original with its exact custody, as the actor leaves it.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn running(
    store: &mut Store,
    location: &Location,
    verified: &Verified,
    epoch: &str,
) -> (
    zerus_desktop_ledger::operations::custody::Record,
    std::sync::Arc<zerus_client_core::run::custody::InheritedLock>,
) {
    use zerus_desktop_ledger::operations::{custody::Record, Ticket};
    let writer = uuid::Uuid::new_v4().to_string();
    let row = admit(store, verified, &writer);
    let ticket = Ticket {
        operation_id: verified.binding.operation_id().unwrap(),
        attempt_id: uuid::Uuid::new_v4().to_string(),
        revision: 2,
        admission_id: verified.plan.launch_id.clone(),
        binding_sha256: verified.binding.fingerprint().unwrap(),
        writer_epoch: writer,
        service_epoch: epoch.into(),
        lease: uuid::Uuid::new_v4().to_string(),
    };
    let (prepared, held) = crate::desktop::custody::prepare(location, &ticket).unwrap();
    let record = Record {
        prepared,
        helper: None,
    };
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    ledger::reserve(&tx, &row, &ticket, &record).unwrap();
    tx.commit().unwrap();
    let row = ledger::pending(&store.db).unwrap().unwrap();
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    ledger::running(&tx, &row).unwrap();
    tx.commit().unwrap();
    (record, held)
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn only_the_released_original_custody_settles_a_launch_and_its_exit_never_changes() {
    use crate::desktop::custody;
    for (code, expected) in [
        (Some(0), Phase::Launched),
        (Some(2), Phase::Failed),
        (None, Phase::Unknown),
    ] {
        let (_temp, location, mut store, epoch) = fixture();
        let verified = original(&location.configuration, &epoch);
        let (record, held) = running(&mut store, &location, &verified, &epoch);
        let row = ledger::pending(&store.db).unwrap().unwrap();
        let exit = code.map(|code| wire::Exited::new(&verified.plan, code, "detail").unwrap());
        let tx = store
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        ledger::result(
            &tx,
            &row,
            exit.as_ref(),
            if code.is_some() {
                "awaiting_resource_drain"
            } else {
                "creation_launch_timed_out"
            },
        )
        .unwrap();
        tx.commit().unwrap();
        // The recorded exit is immutable, and it never settles before drain.
        assert!(
            store
                .db
                .execute("UPDATE desktop_creation_launches SET exit=NULL", [])
                .is_err()
                || code.is_none()
        );
        let row = ledger::pending(&store.db).unwrap().unwrap();
        assert!(row.receipt.result(&verified.plan).is_none());
        let observation = custody::probe(&location, &record);
        assert!(matches!(observation, custody::Observation::Held));
        let tx = store
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        assert!(ledger::drained(tx, row, observation).is_err());
        drop(held);
        let row = ledger::pending(&store.db).unwrap().unwrap();
        let observation = custody::probe(&location, &record);
        let tx = store
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let settled = ledger::drained(tx, row, observation).unwrap();
        assert_eq!(settled.phase, expected, "{code:?}");
        match code {
            Some(0) => assert_eq!(settled.result(&verified.plan), Some(Ok(()))),
            Some(_) => assert!(settled.result(&verified.plan).unwrap().is_err()),
            None => {
                assert!(settled.result(&verified.plan).is_none());
                assert_eq!(settled.code, "creation_launch_timed_out");
            }
        }
        assert_eq!(ledger::undrained(&store.db).unwrap(), 0);
    }
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn the_lookup_adds_evidence_only_beside_an_unknown_original() {
    let (_temp, location, mut store, epoch) = fixture();
    let verified = original(&location.configuration, &epoch);
    let (_record, _held) = running(&mut store, &location, &verified, &epoch);
    let scope = verified.intent.scope.clone();
    let query = wire::Query::for_plan(
        scope.clone(),
        verified.intent.creation_id.clone(),
        &verified.plan,
    )
    .unwrap();
    let read = |store: &Store| -> wire::Lookup {
        serde_json::from_value(query_reply(store, &query)).unwrap()
    };
    let running = read(&store);
    assert!(running.matches(&query));
    assert!(
        running.observation.is_none(),
        "a running original has no evidence"
    );
    let row = ledger::pending(&store.db).unwrap().unwrap();
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    ledger::result(&tx, &row, None, "creation_launch_unobserved").unwrap();
    tx.commit().unwrap();
    let unknown = read(&store);
    assert!(unknown.matches(&query));
    assert_eq!(unknown.receipt.as_ref().unwrap().phase, Phase::Unknown);
    assert!(unknown.observation.is_some(), "evidence beside Unknown");
    let mut changed = query.clone();
    changed.scope.service_epoch = uuid::Uuid::new_v4().to_string();
    assert_eq!(
        query_reply(&store, &changed)["code"],
        "creation_result_unavailable"
    );
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn query_reply(store: &Store, query: &wire::Query) -> serde_json::Value {
    super::query(store, &json!({"op":"creation_launch_result","query":query}))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod launch_child {
    use super::super::child::{self, Ended};
    use super::*;
    use std::process::Command;
    use std::time::{Duration, Instant};

    fn lock() -> (
        tempfile::TempDir,
        Location,
        zerus_desktop_ledger::operations::custody::Record,
        std::sync::Arc<zerus_client_core::run::custody::InheritedLock>,
    ) {
        let (temp, location, mut store, epoch) = fixture();
        let verified = original(&location.configuration, &epoch);
        let (record, held) = running(&mut store, &location, &verified, &epoch);
        (temp, location, record, held)
    }
    fn python(script: &str) -> Vec<String> {
        vec!["-c".into(), script.into()]
    }
    /// Running, not merely an unreaped zombie of a killed process.
    fn alive(pid: i32) -> bool {
        let state = Command::new("ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
            .unwrap_or_default();
        !state.is_empty() && !state.starts_with('Z')
    }

    #[test]
    fn an_exit_is_reported_only_after_both_pipes_drain_and_stderr_is_bounded() {
        let (_temp, _location, _record, held) = lock();
        let program = std::path::Path::new("/usr/bin/env");
        let mut args = vec!["python3".to_string()];
        args.extend(python(
            "import sys\nsys.stdout.write('o'*(1<<20))\nsys.stderr.write('e'*(1<<20))\nsys.exit(3)",
        ));
        let ended = child::run(
            program,
            &args,
            &held,
            Instant::now() + Duration::from_secs(20),
        );
        let Ended::Exited { code, stderr } = ended else {
            panic!("{ended:?}")
        };
        assert_eq!(code, 3);
        assert!(stderr.len() <= 64 << 10 && stderr.starts_with('e'));
        let mut ok = vec!["python3".to_string()];
        ok.extend(python("pass"));
        assert_eq!(
            child::run(
                program,
                &ok,
                &held,
                Instant::now() + Duration::from_secs(20)
            ),
            Ended::Exited {
                code: 0,
                stderr: String::new()
            }
        );
    }

    #[test]
    fn the_launch_child_leads_its_own_process_group() {
        let (_temp, _location, _record, held) = lock();
        let mut args = vec!["python3".to_string()];
        args.extend(python(
            "import os,sys\nsys.stderr.write('own' if os.getpgid(0)==os.getpid() else 'shared')",
        ));
        assert_eq!(
            child::run(
                std::path::Path::new("/usr/bin/env"),
                &args,
                &held,
                Instant::now() + Duration::from_secs(20)
            ),
            Ended::Exited {
                code: 0,
                stderr: "own".into()
            }
        );
    }

    #[test]
    fn the_deadline_kills_only_the_original_child_never_its_group() {
        let (_temp, _location, _record, held) = lock();
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("grandchild");
        let mut args = vec!["python3".to_string()];
        // A same-group grandchild (no new session), then the child waits.
        args.extend(python(&format!(
            "import subprocess,time\np=subprocess.Popen(['sleep','30'],stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)\nopen({:?},'w').write(str(p.pid))\ntime.sleep(30)",
            pid_file.to_str().unwrap()
        )));
        let ended = child::run(
            std::path::Path::new("/usr/bin/env"),
            &args,
            &held,
            Instant::now() + Duration::from_secs(2),
        );
        assert_eq!(ended, Ended::Unknown("creation_launch_timed_out"));
        let grandchild: i32 = std::fs::read_to_string(&pid_file).unwrap().parse().unwrap();
        assert!(alive(grandchild), "the group member was never signalled");
        // SAFETY: this test's own grandchild.
        unsafe { libc::kill(grandchild, libc::SIGKILL) };
    }

    #[test]
    fn output_kept_open_by_another_process_is_unknown_and_never_stopped() {
        let (_temp, _location, _record, held) = lock();
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("holder");
        let mut args = vec!["python3".to_string()];
        // The child exits at once; a detached holder keeps its stdout open.
        args.extend(python(&format!(
            "import subprocess\np=subprocess.Popen(['sleep','30'],stdin=subprocess.DEVNULL,start_new_session=True)\nopen({:?},'w').write(str(p.pid))",
            pid_file.to_str().unwrap()
        )));
        let ended = child::run(
            std::path::Path::new("/usr/bin/env"),
            &args,
            &held,
            Instant::now() + Duration::from_secs(2),
        );
        assert_eq!(ended, Ended::Unknown("creation_launch_output_retained"));
        let holder: i32 = std::fs::read_to_string(&pid_file).unwrap().parse().unwrap();
        assert!(alive(holder), "the holder was never stopped");
        // SAFETY: this test's own helper.
        unsafe { libc::kill(holder, libc::SIGKILL) };
    }

    /// The launch child holds the custody lock for its own lifetime only:
    /// after adopting it, nothing it starts inherits the descriptor.
    #[test]
    fn the_launch_child_holds_custody_but_its_programs_never_inherit_it() {
        use crate::desktop::custody;
        use std::io::{BufRead, BufReader};
        use std::os::fd::AsRawFd;
        let (_temp, location, record, held) = lock();
        let release = tempfile::tempdir().unwrap();
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "desktop::creation_launch::tests::launch_child::launch_custody_probe",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(child::CUSTODY_FD, held.as_raw_fd().to_string())
            .env("HGS_LAUNCH_CUSTODY_RELEASE", release.path())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        let mut probe = {
            let _turn = crate::platform::spawn_turn();
            held.inherit(&mut command).unwrap();
            command.spawn().unwrap()
        };
        // inherit() keeps a lock clone in the command's pre-exec hook. Dropping
        // both leaves the probe child as the only holder.
        drop(command);
        drop(held);
        let mut lines = BufReader::new(probe.stdout.take().unwrap()).lines();
        // libtest prints the test name on the same line as the report.
        let report = lines
            .by_ref()
            .map(Result::unwrap)
            .find_map(|line| line.find("custody:").map(|at| line[at..].to_owned()))
            .unwrap();
        assert_eq!(report, "custody: adopted, variable removed, program closed");
        assert!(matches!(
            custody::probe(&location, &record),
            custody::Observation::Held
        ));
        std::fs::write(release.path().join("go"), b"").unwrap();
        assert!(probe.wait().unwrap().success());
        let until = Instant::now() + Duration::from_secs(10);
        while !matches!(
            custody::probe(&location, &record),
            custody::Observation::Released { .. }
        ) {
            assert!(Instant::now() < until, "released after the child exited");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    #[test]
    #[ignore = "run by the_launch_child_holds_custody_but_its_programs_never_inherit_it"]
    fn launch_custody_probe() {
        let fd: i32 = std::env::var(child::CUSTODY_FD).unwrap().parse().unwrap();
        child::adopt_custody().unwrap();
        let removed = std::env::var_os(child::CUSTODY_FD).is_none();
        // A program the child starts must not have the descriptor.
        let output = Command::new("python3")
            .args([
                "-c",
                "import os,sys\ntry:\n os.fstat(int(sys.argv[1]))\n print('open')\nexcept OSError:\n print('closed')",
                &fd.to_string(),
            ])
            .output()
            .unwrap();
        let program = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        println!(
            "custody: adopted, variable {}, program {program}",
            if removed { "removed" } else { "kept" }
        );
        let release =
            std::path::PathBuf::from(std::env::var("HGS_LAUNCH_CUSTODY_RELEASE").unwrap());
        let until = Instant::now() + Duration::from_secs(20);
        while !release.join("go").exists() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod actor {
    use super::super::child::Ended;
    use super::*;
    use crate::{config::Config, desktop::ui_authority};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use std::time::{Duration, Instant};

    struct Fixture {
        _temp: tempfile::TempDir,
        location: Location,
        store: Store,
        runtime: runtime::Runtime,
        exclusion: Arc<crate::desktop::store::HandoffLock>,
        epoch: String,
        config: Config,
        owner: Arc<crate::platform::FileLock>,
    }
    impl Fixture {
        fn new(
            execute: impl Fn(&wire::Plan, &Arc<zerus_client_core::run::custody::InheritedLock>) -> Ended
                + Send
                + Sync
                + 'static,
        ) -> Self {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().canonicalize().unwrap();
            let config = Config {
                home: root.to_str().unwrap().into(),
                dir: root.join("config"),
                state: root.join("state"),
                selves: vec![],
                peers: vec![],
                base_peers: vec![],
                peers_from_environment: false,
                machines: Default::default(),
                tab: false,
                tab_colors: String::new(),
            };
            std::fs::create_dir(&config.dir).unwrap();
            let location = Location::new(&config).unwrap();
            location.prepare().unwrap();
            let epoch = uuid::Uuid::new_v4().to_string();
            let mut store = Store::open(&location, &epoch).unwrap();
            zerus_desktop_ledger::authorize(
                &mut store.db,
                &uuid::Uuid::new_v4().to_string(),
                &epoch,
            )
            .unwrap();
            let owner = Arc::new(location.lock().unwrap());
            let exclusion = Arc::new(location.handoff_lock().unwrap());
            let mut runtime = runtime::Runtime::new(&config, &epoch, owner.clone()).unwrap();
            // This synthetic source/Validated seam tests the actor, not native
            // authentication or an installed frontend authority positive.
            runtime.fixture_executor(MACHINE, Arc::new(execute));
            runtime.poll(&mut store, Some(exclusion.clone()));
            Self {
                _temp: temp,
                location,
                store,
                runtime,
                exclusion,
                epoch,
                config,
                owner,
            }
        }
        fn original(&self) -> Verified {
            super::original(&self.location.configuration, &self.epoch)
        }
        fn admit(&mut self, original: &Verified) -> wire::Admission {
            let request = zerus_client_core::desktop::ui_requests::Request::CreationLaunch {
                intent: Box::new(original.intent.clone()),
            };
            serde_json::from_value(self.runtime.frontend(
                &mut self.store,
                ui_authority::Validated::fixture(&json!(request)),
                Some(self.exclusion.clone()),
            ))
            .unwrap()
        }
        fn until(&mut self, what: &str, done: impl Fn(&Self) -> bool) {
            let until = Instant::now() + Duration::from_secs(8);
            while !done(self) {
                assert!(
                    Instant::now() < until,
                    "timed out: {what}, {}",
                    self.runtime.status()
                );
                self.runtime
                    .poll(&mut self.store, Some(self.exclusion.clone()));
                std::thread::sleep(Duration::from_millis(2));
            }
        }
        fn row(&self, original: &Verified) -> ledger::Row {
            ledger::load(&self.store.db, &original.plan.launch_id)
                .unwrap()
                .unwrap()
        }
    }

    #[test]
    fn an_admitted_launch_runs_once_without_gui_and_waits_for_its_own_custody() {
        use std::process::{Command, Stdio};
        let launches = Arc::new(AtomicUsize::new(0));
        let count = launches.clone();
        let children = Arc::new(std::sync::Mutex::new(Vec::new()));
        let held = children.clone();
        let release = tempfile::tempdir().unwrap();
        let go = release.path().join("go");
        let mut f = Fixture::new(move |_plan, custody| {
            count.fetch_add(1, Ordering::SeqCst);
            // A helper keeps the exact close-only lock across exec until told.
            let mut command = Command::new("python3");
            command
                .args([
                    "-c",
                    "import os,sys,time\nuntil=time.monotonic()+10\nwhile not os.path.exists(sys.argv[1]) and time.monotonic()<until: time.sleep(.005)",
                ])
                .arg(&go)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            let mut child = {
                let _spawn = crate::platform::spawn_turn();
                custody.inherit(&mut command).unwrap();
                command.spawn().unwrap()
            };
            held.lock()
                .unwrap()
                .push(std::thread::spawn(move || child.wait().unwrap().success()));
            Ended::Exited {
                code: 0,
                stderr: String::new(),
            }
        });
        let original = f.original();
        assert!(f.admit(&original).matches(&original.intent));
        f.until("original exit recorded", |f| {
            f.row(&original).receipt.phase == Phase::Unknown
        });
        assert_eq!(launches.load(Ordering::SeqCst), 1);
        assert_eq!(f.row(&original).next_step, 1);
        assert!(!f.row(&original).receipt.drained);
        assert!(f.row(&original).receipt.result(&original.plan).is_none());
        assert_eq!(f.store.operation_undrained().unwrap(), 1);
        f.runtime.request_stop();
        for _ in 0..8 {
            f.runtime.poll(&mut f.store, Some(f.exclusion.clone()));
        }
        assert!(!f.runtime.drained(), "the helper still holds custody");
        std::fs::write(release.path().join("go"), b"").unwrap();
        f.until("original kernel drain", |f| f.runtime.drained());
        assert_eq!(f.row(&original).receipt.phase, Phase::Launched);
        assert_eq!(
            f.row(&original).receipt.result(&original.plan),
            Some(Ok(()))
        );
        assert_eq!(f.store.operation_undrained().unwrap(), 0);
        assert_eq!(launches.load(Ordering::SeqCst), 1, "never launched again");
        for child in children.lock().unwrap().drain(..) {
            assert!(child.join().unwrap());
        }
    }

    #[test]
    fn a_failed_or_unknown_child_settles_as_such_and_is_never_replayed() {
        for (ended, phase, code) in [
            (
                Ended::Exited {
                    code: 1,
                    stderr: "hgs: no such agent".into(),
                },
                Phase::Failed,
                "",
            ),
            (
                Ended::Unknown("creation_launch_timed_out"),
                Phase::Unknown,
                "creation_launch_timed_out",
            ),
        ] {
            let launches = Arc::new(AtomicUsize::new(0));
            let count = launches.clone();
            let result = std::sync::Mutex::new(Some(ended));
            let mut f = Fixture::new(move |_, _| {
                count.fetch_add(1, Ordering::SeqCst);
                result.lock().unwrap().take().unwrap()
            });
            let original = f.original();
            assert!(f.admit(&original).accepted);
            f.until("settled", |f| f.row(&original).receipt.drained);
            let receipt = f.row(&original).receipt;
            assert_eq!(receipt.phase, phase);
            assert_eq!(receipt.code, code);
            if phase == Phase::Failed {
                assert_eq!(
                    receipt.result(&original.plan).unwrap().unwrap_err(),
                    "hgs: no such agent"
                );
            }
            // The same original admitted again returns its settled receipt.
            assert_eq!(f.admit(&original).receipt.unwrap(), receipt);
            f.runtime.request_stop();
            f.until("stopped", |f| f.runtime.drained());
            assert_eq!(launches.load(Ordering::SeqCst), 1);
        }
    }

    #[test]
    fn a_revoked_writer_refuses_the_launch_step_and_no_child_starts() {
        let launches = Arc::new(AtomicUsize::new(0));
        let count = launches.clone();
        let mut f = Fixture::new(move |_, _| {
            count.fetch_add(1, Ordering::SeqCst);
            Ended::Exited {
                code: 0,
                stderr: String::new(),
            }
        });
        let original = f.original();
        // Revoked after admission, before the actor reserves and steps.
        f.store
            .db
            .execute("UPDATE desktop_writer SET revoked=1 WHERE id=1", [])
            .unwrap();
        let _ = f.admit(&original);
        for _ in 0..50 {
            f.runtime.poll(&mut f.store, Some(f.exclusion.clone()));
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(launches.load(Ordering::SeqCst), 0, "no child after revoke");
        assert!(!f.admit(&f.original()).accepted);
    }

    #[test]
    fn restart_retires_a_queued_plan_and_never_reconstructs_execution_consent() {
        let mut f = Fixture::new(|_, _| panic!("recovered plan must not execute"));
        let original = f.original();
        assert!(f.admit(&original).accepted);
        f.runtime = runtime::Runtime::new(&f.config, &f.epoch, f.owner.clone()).unwrap();
        f.runtime
            .fixture_executor(MACHINE, Arc::new(|_, _| panic!("restart must not execute")));
        f.store = Store::open(&f.location, &f.epoch).unwrap();
        zerus_desktop_ledger::authorize(
            &mut f.store.db,
            &uuid::Uuid::new_v4().to_string(),
            &f.epoch,
        )
        .unwrap();
        f.until("retired", |f| f.row(&original).receipt.drained);
        assert_eq!(f.row(&original).receipt.phase, Phase::Unknown);
        assert_eq!(f.row(&original).receipt.code, "creation_interrupted");
    }
}
