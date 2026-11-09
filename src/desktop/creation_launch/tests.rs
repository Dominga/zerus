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
    // Its own two tables, then the termination outcome, task message, task
    // message input, deferred send, deferred answer and compact-to-send
    // leaves.
    assert_eq!(tables(&store.db), before + 2 + 1 + 2 + 2 + 2 + 2 + 2 + 2);
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
        // A child that another test thread forks meanwhile shares the lock's
        // open file description until it execs: wait, bounded, for that copy
        // to close instead of probing once.
        let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let observation = loop {
            let observation = custody::probe(&location, &record);
            if !matches!(observation, custody::Observation::Held)
                || std::time::Instant::now() >= until
            {
                break observation;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
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
    /// Reports whether the descriptor named by `argv[1]` is the file `argv[2]`
    /// (`dev:ino`), on stdout unless `argv[3]` is `stderr`.
    const CUSTODY_SCRIPT: &str = r#"import os, sys
var = sys.argv[1]
want = sys.argv[2]
out = sys.stderr if sys.argv[3:] == ["stderr"] else sys.stdout
named = os.environ.get(var, "")
if not named.isdigit():
    print("custody: no variable", end="", file=out)
    sys.exit(3)
try:
    seen = os.fstat(int(named))
except OSError:
    print("custody: not inherited", end="", file=out)
    sys.exit(3)
print("custody: inherited" if f"{seen.st_dev}:{seen.st_ino}" == want else "custody: another file", end="", file=out)
"#;
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

    /// The runner's child holds the actor's exact custody descriptor, named
    /// by this kind's variable.
    #[test]
    fn the_runner_child_inherits_exactly_the_named_custody() {
        use std::os::fd::AsRawFd;
        let (_temp, _location, _record, held) = lock();
        // SAFETY: fstat into a zeroed struct on a descriptor `held` owns.
        let mut stat: libc::stat = unsafe { std::mem::zeroed() };
        assert_eq!(unsafe { libc::fstat(held.as_raw_fd(), &mut stat) }, 0);
        let mut args = vec!["python3".to_string()];
        args.extend(python(CUSTODY_SCRIPT));
        args.push(child::CUSTODY_FD.into());
        args.push(format!("{}:{}", stat.st_dev, stat.st_ino));
        // This kind keeps stderr.
        args.push("stderr".into());
        assert_eq!(
            child::run(
                std::path::Path::new("/usr/bin/env"),
                &args,
                &held,
                Instant::now() + Duration::from_secs(20)
            ),
            Ended::Exited {
                code: 0,
                stderr: "custody: inherited".into()
            }
        );
    }

    /// A launch child's input is closed: `/dev/null`, at its end at once.
    #[test]
    fn the_launch_child_reads_a_closed_input() {
        let (_temp, _location, _record, held) = lock();
        let mut args = vec!["python3".to_string()];
        // Read to the end, then name the input by its identity, which works
        // on macOS (no /proc) as on Linux.
        args.extend(python(
            "import os, sys\nd = sys.stdin.buffer.read()\ns, n = os.fstat(0), os.stat('/dev/null')\nname = '/dev/null' if (s.st_dev, s.st_ino) == (n.st_dev, n.st_ino) else 'other'\nsys.stderr.write(f'{name} eof:{len(d)}')",
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
                stderr: "/dev/null eof:0".into()
            }
        );
    }

    #[test]
    fn only_a_child_that_never_existed_is_not_started() {
        let (_temp, _location, _record, held) = lock();
        let deadline = Instant::now() + Duration::from_secs(20);
        // A missing program: the spawn itself fails before any child runs.
        assert_eq!(
            child::run(
                std::path::Path::new("/nonexistent/zerus-hgs"),
                &[],
                &held,
                deadline
            ),
            Ended::NotStarted
        );
        // A refusal inside pre_exec, where custody's own setup runs.
        child::REFUSE_PRE_EXEC.with(|refuse| refuse.set(true));
        assert_eq!(
            child::run(
                std::path::Path::new("/usr/bin/env"),
                &["true".into()],
                &held,
                deadline
            ),
            Ended::NotStarted
        );
        // Once a child exists, a later failure is never "not started".
        assert_eq!(
            child::run(
                std::path::Path::new("/bin/sh"),
                &["-c".into(), "kill -9 $$".into()],
                &held,
                deadline
            ),
            Ended::Unknown("creation_launch_signalled")
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
            Self::build(Some(Arc::new(execute)))
        }
        /// No executor: the original path runs, and this test image's own
        /// self-exec refusal stops it before any child.
        fn unexecuted() -> Self {
            Self::build(None)
        }
        #[allow(clippy::type_complexity)]
        fn build(
            execute: Option<
                Arc<
                    dyn Fn(
                            &wire::Plan,
                            &Arc<zerus_client_core::run::custody::InheritedLock>,
                        ) -> Ended
                        + Send
                        + Sync,
                >,
            >,
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
            match execute {
                Some(execute) => runtime.fixture_executor(MACHINE, execute),
                None => runtime.fixture_machine(MACHINE),
            }
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

    /// Exercises the real child/runtime result paths. The controller retains
    /// one copy of THAT original custody so a worker result cannot masquerade
    /// as kernel drain. Restart/readmission must keep the same settled ticket.
    fn post_child_unknown(
        code: &'static str,
        execute: impl Fn(&Arc<zerus_client_core::run::custody::InheritedLock>) -> Ended
            + Send
            + Sync
            + 'static,
    ) {
        let launches = Arc::new(AtomicUsize::new(0));
        let count = launches.clone();
        let retained = Arc::new(std::sync::Mutex::new(None));
        let keep = retained.clone();
        let mut f = Fixture::new(move |_, custody| {
            count.fetch_add(1, Ordering::SeqCst);
            *keep.lock().unwrap() = Some(custody.clone());
            execute(custody)
        });
        let original = f.original();
        assert!(f.admit(&original).matches(&original.intent));
        f.until("original worker result persisted", |f| {
            f.row(&original).receipt.phase == Phase::Unknown
        });
        let row = f.row(&original);
        assert_eq!(row.receipt.code, code);
        let ticket = row
            .ticket
            .clone()
            .expect("original ticket reserved before child");
        assert_eq!(ticket.admission_id, original.plan.launch_id);
        assert_eq!(
            ticket.binding_sha256,
            original.binding.fingerprint().unwrap()
        );
        assert_eq!(row.next_step, 1);
        assert_eq!(row.receipt.phase, Phase::Unknown);
        assert!(row.receipt.launch.is_none());
        assert!(!row.receipt.not_started(&original.plan));
        assert!(!row.receipt.drained);
        assert_eq!(f.store.operation_undrained().unwrap(), 1);
        assert!(matches!(
            crate::desktop::custody::probe(&f.location, row.custody.as_ref().unwrap()),
            crate::desktop::custody::Observation::Held
        ));
        assert_eq!(f.admit(&original).receipt.unwrap(), row.receipt);
        let held = retained.lock().unwrap().take().unwrap();
        f.runtime.request_stop();
        for _ in 0..8 {
            f.runtime.poll(&mut f.store, Some(f.exclusion.clone()));
        }
        assert!(
            !f.runtime.drained(),
            "original custody is still physically held"
        );
        assert_eq!(launches.load(Ordering::SeqCst), 1);
        drop(held);
        f.until("original custody and runtime drain", |f| {
            f.runtime.drained()
        });
        let settled = f.row(&original);
        assert_eq!(settled.ticket, Some(ticket.clone()));
        assert_eq!(settled.next_step, 1);
        assert_eq!(settled.receipt.phase, Phase::Unknown);
        assert_eq!(settled.receipt.code, code);
        assert!(settled.receipt.drained);
        assert!(settled.receipt.launch.is_none());
        assert!(!settled.receipt.not_started(&original.plan));
        assert_eq!(f.store.operation_undrained().unwrap(), 0);
        // A new runtime never gets execution consent from the unknown record.
        f.runtime = runtime::Runtime::new(&f.config, &f.epoch, f.owner.clone()).unwrap();
        let count = launches.clone();
        f.runtime.fixture_executor(
            MACHINE,
            Arc::new(move |_, _| {
                count.fetch_add(1, Ordering::SeqCst);
                panic!("unknown original must never restart")
            }),
        );
        let again = f.admit(&original);
        assert!(again.accepted);
        assert_eq!(again.receipt.unwrap(), settled.receipt);
        for _ in 0..8 {
            f.runtime.poll(&mut f.store, Some(f.exclusion.clone()));
        }
        assert_eq!(f.row(&original).ticket, Some(ticket));
        assert_eq!(launches.load(Ordering::SeqCst), 1);
        f.runtime.request_stop();
        f.until("restarted observer drained", |f| f.runtime.drained());
    }

    /// Reports OS cleanup separately: a worker assertion could panic into the
    /// SAME unobserved fallback and otherwise conceal a broken kill/wait path.
    fn child_fault(read: bool) {
        use super::super::child;
        use std::sync::atomic::AtomicBool;
        let reaped = Arc::new(AtomicBool::new(false));
        let spawned = Arc::new(AtomicUsize::new(0));
        let consumed = Arc::new(AtomicBool::new(false));
        let was_reaped = reaped.clone();
        let was_spawned = spawned.clone();
        let fault_consumed = consumed.clone();
        post_child_unknown(
            if read {
                "creation_launch_output_unreadable"
            } else {
                "creation_launch_unobserved"
            },
            move |custody| {
                if read {
                    child::READ_ERROR.with(|fault| fault.set(true));
                } else {
                    child::WAIT_ERROR.with(|fault| fault.set(true));
                }
                // No native agent, descendant or global SIGCHLD manipulation. The
                // wait-error helper self-exits in two seconds even under mutation.
                let ended = child::run(
                    std::path::Path::new("/usr/bin/env"),
                    &[
                        "python3".into(),
                        "-c".into(),
                        if read {
                            "pass".into()
                        } else {
                            "import time; time.sleep(2)".into()
                        },
                    ],
                    custody,
                    Instant::now() + Duration::from_secs(20),
                );
                let pid = child::SPAWNED.with(|spawned| spawned.take()).unwrap_or(0);
                was_spawned.store(pid as usize, Ordering::SeqCst);
                fault_consumed.store(
                    if read {
                        child::READ_ERROR.with(|v| !v.get())
                    } else {
                        child::WAIT_ERROR.with(|v| !v.get())
                    },
                    Ordering::SeqCst,
                );
                if pid != 0 {
                    let mut status = 0;
                    // SAFETY: exactly the original private child PID reported by
                    // Child::id; waitpid cannot observe an unrelated non-child.
                    let result = unsafe { libc::waitpid(pid as i32, &mut status, libc::WNOHANG) };
                    was_reaped.store(
                        result == -1
                            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ECHILD),
                        Ordering::SeqCst,
                    );
                    if result == 0 {
                        // Mutation cleanup only: that SAME still-unreaped private
                        // helper has its own bounded exit. Never signal by name.
                        unsafe {
                            libc::waitpid(pid as i32, &mut status, 0);
                        }
                    }
                }
                ended
            },
        );
        assert!(spawned.load(Ordering::SeqCst) > 0, "a real child existed");
        assert!(
            consumed.load(Ordering::SeqCst),
            "the actual observation boundary consumed the fault"
        );
        assert!(
            reaped.load(Ordering::SeqCst),
            "the production path reaped its original child before returning"
        );
    }

    #[test]
    fn post_child_output_read_error_is_unknown_and_never_replayed() {
        child_fault(true);
    }

    #[test]
    fn post_child_try_wait_error_is_unknown_and_never_replayed() {
        child_fault(false);
    }

    #[test]
    fn post_child_worker_panic_is_unknown_and_never_replayed() {
        post_child_unknown("creation_launch_unobserved", |_| {
            panic!("private launch worker lost its result")
        });
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

    /// A drained receipt of a launch that never started: Failed, the closed
    /// code, no exit; the client's never-sent proof.
    fn settled_not_started(f: &mut Fixture, original: &Verified) {
        f.until("settled", |f| f.row(original).receipt.drained);
        let receipt = f.row(original).receipt;
        assert_eq!(receipt.phase, Phase::Failed, "{receipt:?}");
        assert_eq!(receipt.code, wire::NOT_STARTED);
        assert!(receipt.launch.is_none());
        assert!(receipt.not_started(&original.plan));
        assert!(receipt.result(&original.plan).is_none(), "no exit to read");
        // The same original admitted again returns its settled receipt.
        assert_eq!(f.admit(original).receipt.unwrap(), receipt);
        f.runtime.request_stop();
        f.until("stopped", |f| f.runtime.drained());
    }

    #[test]
    fn a_launch_that_never_started_settles_failed_only_after_its_drain() {
        let launches = Arc::new(AtomicUsize::new(0));
        let count = launches.clone();
        // The executor reports the closed pre-child result, as child::run does.
        let mut f = Fixture::new(move |_, _| {
            count.fetch_add(1, Ordering::SeqCst);
            Ended::NotStarted
        });
        let original = f.original();
        assert!(f.admit(&original).accepted);
        settled_not_started(&mut f, &original);
        assert_eq!(launches.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn the_own_image_refusal_never_starts_the_launch() {
        let mut f = Fixture::unexecuted();
        let original = f.original();
        assert!(f.admit(&original).accepted);
        settled_not_started(&mut f, &original);
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

/// desktop-u's G1 on #1433: this kind's child runner checks the
/// never-renewed deadline last before the spawn and before any input.
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod spent_deadline {
    use crate::desktop::original_child::{OFFERED, SPAWNED};
    use std::path::Path;
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use zerus_client_core::run::custody::InheritedLock;

    pub(super) fn custody() -> Arc<InheritedLock> {
        InheritedLock::try_lock(tempfile::tempfile().unwrap()).unwrap()
    }
    pub(super) fn cat() -> &'static Path {
        Path::new("/bin/cat")
    }
    pub(super) fn clear() {
        SPAWNED.with(|spawned| spawned.set(None));
        OFFERED.with(|offered| offered.set(None));
    }
    pub(super) fn spawned() -> Option<u32> {
        SPAWNED.with(|spawned| spawned.take())
    }
    pub(super) fn expired() -> Instant {
        Instant::now() - Duration::from_millis(1)
    }
    pub(super) fn soon() -> Instant {
        Instant::now() + Duration::from_secs(20)
    }

    #[test]
    fn a_launch_child_never_starts_once_its_deadline_is_spent() {
        use super::super::child::{run, Ended};
        clear();
        assert_eq!(run(cat(), &[], &custody(), expired()), Ended::NotStarted);
        assert_eq!(spawned(), None);
        clear();
        assert!(matches!(
            run(cat(), &[], &custody(), soon()),
            Ended::Exited { code: 0, .. }
        ));
    }
}
