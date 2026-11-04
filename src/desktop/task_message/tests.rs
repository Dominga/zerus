use super::*;
use crate::desktop::store::{Location, Store};
use rusqlite::TransactionBehavior;
use serde_json::json;
use zerus_client_core::desktop::task_message::{self as wire, Intent, Phase, Verified};
use zerus_client_core::tasks_write::{input_digest, message_input, message_intent};

#[cfg(any(target_os = "linux", target_os = "macos"))]
const MACHINE: &str = "22222222-2222-4222-8222-222222222222";
const TASK: &str = "11111111-1111-4111-8111-111111111111";

/// An outbox record as `reserve_message` saves a canonical frozen stdin.
fn record(request: &str) -> serde_json::Value {
    let intent = message_intent("hello", &[], "queue", "");
    json!({"id": request, "intent": intent, "input_sha256": input_digest(&message_input(&intent))})
}
fn original(configuration: &str, epoch: &str) -> Verified {
    let request = uuid::Uuid::new_v4().to_string();
    Intent {
        scope: wire::Scope {
            configuration: configuration.into(),
            service_epoch: epoch.into(),
        },
        project: "example".into(),
        task_id: TASK.into(),
        request_id: request.clone(),
        original: record(&request).to_string(),
        form: Default::default(),
    }
    .verify()
    .unwrap()
}
/// The exact reply of `verified`'s original `hgs task message` call.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn reply(verified: &Verified) -> Vec<u8> {
    serde_json::to_vec(&json!({"task": {"uuid": TASK, "project": "example"},
        "message": {"kind": "message", "request_id": verified.plan.request_id, "session": null,
            "delivery": {"mode": "queue", "state": "queued", "stage": "todo", "stage_at": 7}}}))
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
fn tables(db: &rusqlite::Connection) -> i64 {
    db.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE type='table'",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn exact_previous_store_upgrade_adds_only_the_independent_message_leaf() {
    assert_eq!(
        SCHEMA - 1,
        crate::desktop::termination_outcome::SCHEMA,
        "rebase this test onto the actual merge-order predecessor"
    );
    let (_temp, location, store, _) = fixture();
    remove_fixture(&store.db);
    store
        .db
        .pragma_update(None, "user_version", SCHEMA - 1)
        .unwrap();
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
fn admission_is_immutable_and_the_same_id_returns_only_its_original_receipt() {
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
        store.operation_status()["kinds"]["task_message"]["undrained"],
        1
    );
    for statement in [
        "UPDATE desktop_task_messages SET source='{}'",
        "DELETE FROM desktop_task_messages",
    ] {
        assert!(store.db.execute(statement, []).is_err(), "{statement}");
    }
    // Another original under the same request ID never replaces it.
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
        ledger::load(&store.db, &changed.plan.request_id)
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
        .execute("UPDATE desktop_task_message_budget SET bytes=16777216", [])
        .unwrap();
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    assert!(ledger::admit(&tx, &verified, &writer, 1).is_err());
    drop(tx);
    assert!(ledger::load(&store.db, &verified.plan.request_id)
        .unwrap()
        .is_none());
    store
        .db
        .execute("UPDATE desktop_task_message_budget SET bytes=0", [])
        .unwrap();
    let row = admit(&mut store, &verified, &writer);
    let tx = store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    ledger::retire(&tx, &row).unwrap();
    tx.commit().unwrap();
    assert_eq!(ledger::undrained(&store.db).unwrap(), 0);
    let retired = ledger::load(&store.db, &verified.plan.request_id)
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
        admission_id: verified.plan.request_id.clone(),
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
fn only_the_released_custody_settles_a_message_and_its_reply_never_changes() {
    use crate::desktop::custody;
    enum Ending {
        Exit(ledger::Exit),
        NotStarted,
        Unknown,
    }
    for (index, expected) in [
        Phase::Recorded,
        Phase::Failed,
        Phase::Failed,
        Phase::Unknown,
    ]
    .into_iter()
    .enumerate()
    {
        let (_temp, location, mut store, epoch) = fixture();
        let verified = original(&location.configuration, &epoch);
        let (record, held) = running(&mut store, &location, &verified, &epoch);
        let ending = match index {
            0 => Ending::Exit(ledger::Exit {
                code: 0,
                outcome: wire::Outcome::from_reply(&verified.plan, &reply(&verified)),
            }),
            1 => Ending::Exit(ledger::Exit {
                code: 2,
                outcome: None,
            }),
            2 => Ending::NotStarted,
            _ => Ending::Unknown,
        };
        let row = ledger::pending(&store.db).unwrap().unwrap();
        let tx = store
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        match &ending {
            Ending::Exit(exit) => {
                ledger::result(&tx, &row, Some(exit), "awaiting_resource_drain").unwrap()
            }
            Ending::NotStarted => ledger::not_started(&tx, &row).unwrap(),
            Ending::Unknown => ledger::result(&tx, &row, None, "task_message_timed_out").unwrap(),
        }
        tx.commit().unwrap();
        // The recorded reply is immutable, and nothing settles before drain.
        if matches!(ending, Ending::Exit(_)) {
            assert!(store
                .db
                .execute("UPDATE desktop_task_messages SET reply=NULL", [])
                .is_err());
        }
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
        assert_eq!(settled.phase, expected, "{index}");
        match index {
            0 => {
                assert_eq!(settled.result(&verified.plan), Some(Ok(())));
                assert_eq!(settled.outcome.as_ref().unwrap().state, "queued");
            }
            1 => assert_eq!(
                settled.result(&verified.plan),
                Some(Err(wire::EXITED.into()))
            ),
            2 => assert_eq!(
                settled.result(&verified.plan),
                Some(Err(wire::NOT_STARTED.into()))
            ),
            _ => {
                assert!(settled.result(&verified.plan).is_none());
                assert_eq!(settled.code, "task_message_timed_out");
            }
        }
        assert_eq!(ledger::undrained(&store.db).unwrap(), 0);
    }
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn only_a_proven_outcome_or_a_definite_exit_is_ever_recorded() {
    let (_temp, location, mut store, epoch) = fixture();
    let verified = original(&location.configuration, &epoch);
    let (_record, _held) = running(&mut store, &location, &verified, &epoch);
    let row = ledger::pending(&store.db).unwrap().unwrap();
    let mut wrong = wire::Outcome::from_reply(&verified.plan, &reply(&verified)).unwrap();
    wrong.state = "note".into();
    for exit in [
        // Exit 0 without the outcome it proved, or with another one.
        ledger::Exit {
            code: 0,
            outcome: None,
        },
        ledger::Exit {
            code: 0,
            outcome: Some(wrong),
        },
        // A failed exit never carries an outcome.
        ledger::Exit {
            code: 1,
            outcome: wire::Outcome::from_reply(&verified.plan, &reply(&verified)),
        },
    ] {
        let tx = store
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        assert!(ledger::result(&tx, &row, Some(&exit), "awaiting_resource_drain").is_err());
    }
    // No Unknown code can pose as the closed not-started result.
    for code in [wire::NOT_STARTED, "awaiting_not_started_drain"] {
        let tx = store
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        assert!(ledger::result(&tx, &row, None, code).is_err(), "{code}");
    }
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn the_lookup_reads_only_its_exact_original() {
    let (_temp, location, mut store, epoch) = fixture();
    let verified = original(&location.configuration, &epoch);
    let (_record, _held) = running(&mut store, &location, &verified, &epoch);
    let query = wire::Query::for_plan(verified.intent.scope.clone(), &verified.plan).unwrap();
    let read = |store: &Store, query: &wire::Query| {
        super::query(store, &json!({"op": "task_message_result", "query": query}))
    };
    let lookup: wire::Lookup = serde_json::from_value(read(&store, &query)).unwrap();
    assert!(lookup.matches(&query));
    assert_eq!(lookup.receipt.unwrap().phase, Phase::Running);
    let mut changed = query.clone();
    changed.scope.service_epoch = uuid::Uuid::new_v4().to_string();
    assert_eq!(
        read(&store, &changed)["code"],
        "task_message_result_unavailable"
    );
    let mut missing = query;
    missing.request_id = uuid::Uuid::new_v4().to_string();
    let lookup: wire::Lookup = serde_json::from_value(read(&store, &missing)).unwrap();
    assert!(!lookup.found && lookup.receipt.is_none());
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod message_child {
    use super::super::child::{self, Ended};
    use super::*;
    use std::path::Path;
    use std::sync::Arc;
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
        vec!["python3".into(), "-c".into(), script.into()]
    }
    fn guard() -> Arc<dyn Send + Sync> {
        Arc::new(())
    }
    fn soon() -> Instant {
        Instant::now() + Duration::from_secs(20)
    }

    #[test]
    fn the_child_reads_exactly_its_frozen_input_and_returns_its_whole_reply() {
        let (_temp, _location, _record, held) = lock();
        let input = b"frozen \xff stdin".to_vec();
        assert_eq!(
            child::run(
                Path::new("/usr/bin/env"),
                &python("import sys\nsys.stdout.buffer.write(sys.stdin.buffer.read()[::-1])"),
                &input,
                &held,
                &guard(),
                soon(),
                |_| Ok(())
            ),
            Ended::Exited {
                code: 0,
                stdout: input.iter().rev().copied().collect()
            }
        );
        // A non-zero exit is reported as such, with its reply.
        assert_eq!(
            child::run(
                Path::new("/bin/sh"),
                &["-c".into(), "cat >/dev/null; printf no; exit 3".into()],
                b"x",
                &held,
                &guard(),
                soon(),
                |_| Ok(())
            ),
            Ended::Exited {
                code: 3,
                stdout: b"no".to_vec()
            }
        );
    }

    #[test]
    fn incomplete_input_or_reply_and_the_deadline_are_unknown() {
        let (_temp, _location, _record, held) = lock();
        // A child that exits without reading more than a pipe's worth.
        assert_eq!(
            child::run(
                Path::new("/bin/sh"),
                &["-c".into(), "exit 0".into()],
                &vec![b'x'; 1 << 20],
                &held,
                &guard(),
                soon(),
                |_| Ok(())
            ),
            Ended::Unknown("task_message_input_unwritten")
        );
        // A reply larger than the retained bound.
        assert_eq!(
            child::run(
                Path::new("/usr/bin/env"),
                &python(&format!(
                    "import sys\nsys.stdout.buffer.write(b'x' * {})",
                    child::STDOUT_BYTES + 1
                )),
                b"",
                &held,
                &guard(),
                soon(),
                |_| Ok(())
            ),
            Ended::Unknown("task_message_reply_too_large")
        );
        // The deadline kills only the original child.
        let started = Instant::now();
        assert_eq!(
            child::run(
                Path::new("/bin/sleep"),
                &["30".into()],
                b"",
                &held,
                &guard(),
                Instant::now() + Duration::from_millis(300),
                |_| Ok(())
            ),
            Ended::Unknown("task_message_timed_out")
        );
        assert!(started.elapsed() < Duration::from_secs(10));
        // A signal after the child started is never "not started".
        assert_eq!(
            child::run(
                Path::new("/bin/sh"),
                &["-c".into(), "kill -9 $$".into()],
                b"",
                &held,
                &guard(),
                soon(),
                |_| Ok(())
            ),
            Ended::Unknown("task_message_signalled")
        );
    }

    #[test]
    fn only_a_child_that_never_existed_is_not_started() {
        let (_temp, _location, _record, held) = lock();
        assert_eq!(
            child::run(
                Path::new("/nonexistent/zerus-hgs"),
                &[],
                b"",
                &held,
                &guard(),
                soon(),
                |_| Ok(())
            ),
            Ended::NotStarted
        );
        child::REFUSE_PRE_EXEC.with(|refuse| refuse.set(true));
        assert_eq!(
            child::run(
                Path::new("/usr/bin/env"),
                &["true".into()],
                b"",
                &held,
                &guard(),
                soon(),
                |_| Ok(())
            ),
            Ended::NotStarted
        );
    }

    #[test]
    fn an_unreadable_output_or_unobserved_exit_is_unknown_and_the_child_is_reaped() {
        let (_temp, _location, _record, held) = lock();
        for (read, expected) in [
            (true, "task_message_output_unreadable"),
            (false, "task_message_unobserved"),
        ] {
            if read {
                child::READ_ERROR.with(|fault| fault.set(true));
            } else {
                child::WAIT_ERROR.with(|fault| fault.set(true));
            }
            // The wait-fault helper exits on its own in two seconds, even
            // under mutation; nothing is signalled by name.
            let resources = Arc::new(());
            let weak = Arc::downgrade(&resources);
            let guard: Arc<dyn Send + Sync> = resources;
            let ended = child::run(
                Path::new("/usr/bin/env"),
                &python(if read {
                    "pass"
                } else {
                    "import time; time.sleep(2)"
                }),
                b"",
                &held,
                &guard,
                soon(),
                |_| Ok(()),
            );
            drop(guard);
            assert_eq!(ended, Ended::Unknown(expected));
            assert!(
                child::READ_ERROR.with(|v| !v.get()) && child::WAIT_ERROR.with(|v| !v.get()),
                "the fault was observed"
            );
            let pid = child::SPAWNED.with(|spawned| spawned.take()).unwrap() as i32;
            let mut status = 0;
            // SAFETY: exactly the private child PID reported by Child::id;
            // waitpid cannot observe an unrelated non-child.
            let result = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
            let reaped = result == -1
                && std::io::Error::last_os_error().raw_os_error() == Some(libc::ECHILD);
            if result == 0 {
                // Mutation cleanup only: the same helper's own bounded exit.
                unsafe {
                    libc::waitpid(pid, &mut status, 0);
                }
            }
            assert!(reaped, "the original child was waited for");
            if read {
                // That pipe's end can never be observed: its guard stays.
                std::thread::sleep(Duration::from_millis(50));
                assert!(weak.strong_count() > 0, "an unreadable pipe pins the guard");
            } else {
                // A confirmed exit and real pipe ends release every copy.
                let until = Instant::now() + Duration::from_secs(10);
                while weak.strong_count() > 0 {
                    assert!(Instant::now() < until, "the guard outlived the pipes");
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        }
    }

    /// review-n's probe at f5791071: after a timeout or an unobserved exit, the
    /// recovery drained the row while a descendant still held the original
    /// stdout. Now the pipe's guard keeps the SAME lock until that pipe ends.
    #[test]
    fn a_timed_out_or_unobserved_original_keeps_custody_until_its_descendant_pipe_ends() {
        for wait_error in [false, true] {
            let (_temp, location, mut store, epoch) = fixture();
            let original = original(&location.configuration, &epoch);
            let (record, held) = running(&mut store, &location, &original, &epoch);
            let finish = location.root.join("descendant-finished");
            // The descendant keeps the original stdout for two seconds, then
            // records its end; it never inherits the custody lock.
            let script = format!(
                "import subprocess,sys,time\nsubprocess.Popen([sys.executable,'-c',\"import pathlib,sys,time\\ntime.sleep(2)\\npathlib.Path(sys.argv[1]).write_text('finished')\",{}],stdout=sys.stdout,stderr=subprocess.DEVNULL)\nsys.stdout.write('descendant-ready')\nsys.stdout.flush()\ntime.sleep(5)",
                serde_json::to_string(&finish).unwrap()
            );
            child::WAIT_ERROR_AFTER_OUTPUT.with(|armed| armed.set(wait_error));
            let guard: Arc<dyn Send + Sync> = Arc::new(held.clone());
            let ended = child::run(
                Path::new("/usr/bin/env"),
                &python(&script),
                b"frozen",
                &held,
                &guard,
                Instant::now() + Duration::from_millis(200),
                |_| Ok(()),
            );
            let expected = if wait_error {
                "task_message_unobserved"
            } else {
                "task_message_timed_out"
            };
            assert_eq!(ended, Ended::Unknown(expected));
            let row = ledger::load(&store.db, &original.plan.request_id)
                .unwrap()
                .unwrap();
            let tx = store
                .db
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .unwrap();
            ledger::result(&tx, &row, None, expected).unwrap();
            tx.commit().unwrap();
            drop((guard, held));
            let drained = |store: &mut Store| {
                store.recover_operation_custody(&location);
                ledger::load(&store.db, &original.plan.request_id)
                    .unwrap()
                    .unwrap()
                    .receipt
                    .drained
            };
            let until = Instant::now() + Duration::from_millis(800);
            while Instant::now() < until {
                assert!(
                    !drained(&mut store) || finish.exists(),
                    "drained before the descendant's pipe ended ({expected})"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(matches!(
                crate::desktop::custody::probe(&location, &record),
                crate::desktop::custody::Observation::Held
            ));
            let until = Instant::now() + Duration::from_secs(10);
            while !drained(&mut store) {
                assert!(Instant::now() < until, "never drained after the real end");
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(finish.exists());
        }
    }

    /// The original child prints a reply, starts a descendant that keeps its
    /// stdout open and exits. Like `hgs` (adopt_custody), it never lets the
    /// descendant inherit the custody lock. Only an EOF on the test's accepted
    /// socket ends the descendant; a panic or crash closes it too.
    pub(super) const RETAINING: &str = r#"import os,subprocess,sys
sys.stdin.buffer.read()
sys.stdout.buffer.write(b'{}')
sys.stdout.flush()
subprocess.Popen([sys.executable,'-c',"import socket,sys\nc=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM)\nc.connect(sys.argv[1])\nassert c.recv(1)==b''",sys.argv[1]],stdin=subprocess.DEVNULL,close_fds=True)
os._exit(0)"#;

    /// The one connection of the descendant, accepted within a bound.
    pub(super) fn accept(
        listener: &std::os::unix::net::UnixListener,
        mut turn: impl FnMut(),
    ) -> std::os::unix::net::UnixStream {
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            match listener.accept() {
                Ok((stream, _)) => return stream,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < until, "the descendant never connected");
                    turn();
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(e) => panic!("{e}"),
            }
        }
    }

    #[test]
    fn an_original_pipe_held_by_a_descendant_keeps_the_guard_until_its_real_end() {
        let (_temp, _location, _record, held) = lock();
        let control = tempfile::tempdir().unwrap();
        let socket = control.path().join("c");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let resources = Arc::new(());
        let weak = Arc::downgrade(&resources);
        let guard: Arc<dyn Send + Sync> = resources;
        let mut args = python(RETAINING);
        args.push(socket.to_str().unwrap().into());
        assert_eq!(
            child::run(
                Path::new("/usr/bin/env"),
                &args,
                b"",
                &held,
                &guard,
                Instant::now() + Duration::from_millis(500),
                |_| Ok(())
            ),
            Ended::Unknown("task_message_output_retained")
        );
        let release = accept(&listener, || ());
        drop(guard);
        std::thread::sleep(Duration::from_millis(50));
        assert!(
            weak.strong_count() > 0,
            "the still-open original pipe keeps its own guard copy"
        );
        drop(release); // EOF is the one explicit release of the descendant.
        let until = Instant::now() + Duration::from_secs(10);
        while weak.strong_count() > 0 {
            assert!(Instant::now() < until, "the guard outlived the pipe's end");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// A script image: the original reads its input and answers `original`;
    /// a replacement records that it ran and echoes the private input.
    #[cfg(target_os = "linux")]
    fn image(dir: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("hgs");
        std::fs::write(
            &path,
            "#!/usr/bin/env python3\nimport sys\nsys.stdin.buffer.read()\nsys.stdout.write('original')\n",
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        (path, dir.join("replacement-ran"))
    }
    #[cfg(target_os = "linux")]
    fn replace(path: &Path, ran: &Path) {
        use std::os::unix::fs::PermissionsExt;
        let next = path.with_extension("next");
        std::fs::write(
            &next,
            format!(
                "#!/usr/bin/env python3\nimport pathlib,sys\npathlib.Path({:?}).write_text('ran')\nsys.stdout.buffer.write(b'replacement received: '+sys.stdin.buffer.read())\n",
                ran.to_str().unwrap()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&next, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::rename(&next, path).unwrap();
    }

    /// The same inode rewritten in place, as another writer of the selected
    /// image could.
    #[cfg(target_os = "linux")]
    fn rewrite(path: &Path, ran: &Path) {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(path)
            .unwrap();
        write!(
            file,
            "#!/usr/bin/env python3\nimport pathlib,sys\npathlib.Path({:?}).write_text('ran')\nsys.stdout.buffer.write(b'rewrite received: '+sys.stdin.buffer.read())\n",
            ran.to_str().unwrap()
        )
        .unwrap();
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn an_image_rewritten_in_place_before_the_final_check_starts_no_child() {
        let (_temp, _location, _record, held) = lock();
        let dir = tempfile::tempdir().unwrap();
        let (path, ran) = image(dir.path());
        let pin = zerus_client_core::image::linux::Pin::open(&path).unwrap();
        let inode = std::os::unix::fs::MetadataExt::ino(&std::fs::metadata(&path).unwrap());
        rewrite(&path, &ran);
        assert_eq!(
            std::os::unix::fs::MetadataExt::ino(&std::fs::metadata(&path).unwrap()),
            inode,
            "the same inode"
        );
        assert_eq!(
            super::super::runtime::pinned(
                &pin,
                &[],
                b"private frozen input",
                &held,
                &guard(),
                soon()
            ),
            Ended::NotStarted
        );
        assert!(!ran.exists(), "the rewrite never ran");
    }

    /// review-win2-c's same-inode probe on #1307: after the final check, an
    /// in-place rewrite must never run. The child executes the sealed snapshot.
    #[test]
    #[cfg(target_os = "linux")]
    fn an_image_rewritten_in_place_after_the_final_check_never_runs_or_receives_the_input() {
        let (_temp, _location, _record, held) = lock();
        let dir = tempfile::tempdir().unwrap();
        let (path, ran) = image(dir.path());
        let pin = zerus_client_core::image::linux::Pin::open(&path).unwrap();
        let (late, marker) = (path.clone(), ran.clone());
        child::AFTER_FINAL_CHECK
            .with(|hook| *hook.borrow_mut() = Some(Box::new(move || rewrite(&late, &marker))));
        assert_eq!(
            super::super::runtime::pinned(
                &pin,
                &[],
                b"private frozen input",
                &held,
                &guard(),
                soon()
            ),
            Ended::Exited {
                code: 0,
                stdout: b"original".to_vec()
            }
        );
        assert!(child::AFTER_FINAL_CHECK.with(|hook| hook.borrow().is_none()));
        assert!(!ran.exists(), "the rewrite never ran");
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn an_image_replaced_before_the_final_check_starts_no_child() {
        let (_temp, _location, _record, held) = lock();
        let dir = tempfile::tempdir().unwrap();
        let (path, ran) = image(dir.path());
        let pin = zerus_client_core::image::linux::Pin::open(&path).unwrap();
        replace(&path, &ran);
        assert_eq!(
            super::super::runtime::pinned(
                &pin,
                &[],
                b"private frozen input",
                &held,
                &guard(),
                soon()
            ),
            Ended::NotStarted
        );
        assert!(!ran.exists(), "the replacement never ran");
    }

    /// review-n's late-swap probe at f5791071: the replacement ran and echoed
    /// the private frozen input. Now the child executes the pinned descriptor.
    #[test]
    #[cfg(target_os = "linux")]
    fn an_image_replaced_after_the_final_check_never_runs_or_receives_the_input() {
        let (_temp, _location, _record, held) = lock();
        let dir = tempfile::tempdir().unwrap();
        let (path, ran) = image(dir.path());
        let pin = zerus_client_core::image::linux::Pin::open(&path).unwrap();
        let (late, marker) = (path.clone(), ran.clone());
        child::AFTER_FINAL_CHECK
            .with(|hook| *hook.borrow_mut() = Some(Box::new(move || replace(&late, &marker))));
        assert_eq!(
            super::super::runtime::pinned(
                &pin,
                &[],
                b"private frozen input",
                &held,
                &guard(),
                soon()
            ),
            Ended::Exited {
                code: 0,
                stdout: b"original".to_vec()
            }
        );
        assert!(child::AFTER_FINAL_CHECK.with(|hook| hook.borrow().is_none()));
        assert!(!ran.exists(), "the replacement never ran");
    }

    #[test]
    fn the_child_leads_its_own_process_group() {
        let (_temp, _location, _record, held) = lock();
        assert_eq!(
            child::run(
                Path::new("/usr/bin/env"),
                &python("import os,sys\nsys.stdout.write('own' if os.getpgid(0)==os.getpid() else 'shared')"),
                b"",
                &held,
                &guard(),
                soon(), |_| Ok(())),
            Ended::Exited {
                code: 0,
                stdout: b"own".to_vec()
            }
        );
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
            execute: impl Fn(
                    &wire::Plan,
                    &[String],
                    &[u8],
                    &Arc<zerus_client_core::run::custody::InheritedLock>,
                    &Arc<dyn Send + Sync>,
                ) -> Ended
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
                            &[String],
                            &[u8],
                            &Arc<zerus_client_core::run::custody::InheritedLock>,
                            &Arc<dyn Send + Sync>,
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
            // The shared actor logic is tested on every platform; the macOS
            // fence has its own test.
            runtime.fixture_mac_fence(false);
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
            let request = zerus_client_core::desktop::ui_requests::Request::TaskMessage {
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
            ledger::load(&self.store.db, &original.plan.request_id)
                .unwrap()
                .unwrap()
        }
    }

    #[test]
    fn an_admitted_message_runs_once_and_settles_only_from_its_exact_reply() {
        let sent = Arc::new(AtomicUsize::new(0));
        let count = sent.clone();
        let mut f = Fixture::new(move |plan, args, input, _, _| {
            count.fetch_add(1, Ordering::SeqCst);
            // The one `hgs task message` call, with the record's frozen stdin.
            assert_eq!(
                args,
                [
                    "task",
                    "message",
                    TASK,
                    "--request-id",
                    &plan.request_id,
                    "--mode",
                    "queue",
                    "--expect-dispatch",
                    "",
                    "--content-json",
                    "--project",
                    "example",
                ]
            );
            let frozen = message_input(&message_intent("hello", &[], "queue", ""));
            assert_eq!(input, frozen.as_bytes());
            assert_eq!(input_digest(&frozen), plan.input_sha256);
            Ended::Exited {
                code: 0,
                stdout: reply_for(plan),
            }
        });
        let original = f.original();
        assert!(f.admit(&original).matches(&original.intent));
        f.until("settled", |f| f.row(&original).receipt.drained);
        let receipt = f.row(&original).receipt;
        assert_eq!(receipt.phase, Phase::Recorded);
        assert_eq!(receipt.result(&original.plan), Some(Ok(())));
        assert_eq!(receipt.outcome.as_ref().unwrap().state, "queued");
        // The same original admitted again returns its settled receipt.
        assert_eq!(f.admit(&original).receipt.unwrap(), receipt);
        f.runtime.request_stop();
        f.until("stopped", |f| f.runtime.drained());
        assert_eq!(sent.load(Ordering::SeqCst), 1, "never sent again");
    }

    fn reply_for(plan: &wire::Plan) -> Vec<u8> {
        serde_json::to_vec(&json!({"task": {"uuid": plan.task_id, "project": plan.project},
            "message": {"kind": "message", "request_id": plan.request_id, "session": null,
                "delivery": {"mode": plan.mode, "state": "queued", "stage": "todo", "stage_at": 7}}}))
        .unwrap()
    }

    #[test]
    fn an_unproven_reply_a_failed_exit_and_the_own_image_refusal_settle_as_such() {
        for (ended, phase, code) in [
            (
                Ended::Exited {
                    code: 0,
                    stdout: b"{}".to_vec(),
                },
                Phase::Unknown,
                "task_message_reply_unconfirmed",
            ),
            (
                Ended::Exited {
                    code: 2,
                    stdout: b"hgs: refused".to_vec(),
                },
                Phase::Failed,
                wire::EXITED,
            ),
            (
                Ended::Unknown("task_message_timed_out"),
                Phase::Unknown,
                "task_message_timed_out",
            ),
        ] {
            let result = std::sync::Mutex::new(Some(ended));
            let mut f = Fixture::new(move |_, _, _, _, _| result.lock().unwrap().take().unwrap());
            let original = f.original();
            assert!(f.admit(&original).accepted);
            f.until("settled", |f| f.row(&original).receipt.drained);
            let receipt = f.row(&original).receipt;
            assert_eq!((receipt.phase, receipt.code.as_str()), (phase, code));
            assert!(receipt.outcome.is_none());
            f.runtime.request_stop();
            f.until("stopped", |f| f.runtime.drained());
        }
        // The real original path: this test image refuses its own image.
        let mut f = Fixture::unexecuted();
        let original = f.original();
        assert!(f.admit(&original).accepted);
        f.until("settled", |f| f.row(&original).receipt.drained);
        let receipt = f.row(&original).receipt;
        assert_eq!(
            (receipt.phase, receipt.code.as_str()),
            (Phase::Failed, wire::NOT_STARTED)
        );
    }

    #[test]
    fn a_descendant_pipe_keeps_custody_charge_and_takeover_until_its_eof() {
        use super::message_child::{accept, RETAINING};
        use crate::desktop::custody::{self, Observation};
        let control = tempfile::tempdir().unwrap();
        let socket = control.path().join("c");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let path = socket.to_str().unwrap().to_owned();
        // A real original child; its descendant keeps the original stdout
        // open past the deadline and holds no custody lock of its own.
        let mut f = Fixture::new(move |_, _, input, lock, resources| {
            child::run(
                std::path::Path::new("/usr/bin/env"),
                &[
                    "python3".into(),
                    "-c".into(),
                    RETAINING.into(),
                    path.clone(),
                ],
                input,
                lock,
                resources,
                Instant::now() + Duration::from_millis(500),
                |_| Ok(()),
            )
        });
        let original = f.original();
        assert!(f.admit(&original).accepted);
        // Keep the SAME accepted endpoint through the logical outcome.
        let release = accept(&listener, || {
            f.runtime.poll(&mut f.store, Some(f.exclusion.clone()));
        });
        f.until("logical Unknown", |f| {
            f.row(&original).receipt.code == "task_message_output_retained"
        });
        let row = f.row(&original);
        assert_eq!(row.receipt.phase, Phase::Unknown);
        assert!(!row.receipt.drained);
        let record = row.custody.clone().unwrap();
        assert!(matches!(
            custody::probe(&f.location, &record),
            Observation::Held
        ));
        // Neither revoke nor a stop request releases the original resources.
        f.store
            .db
            .execute("UPDATE desktop_writer SET revoked=1 WHERE id=1", [])
            .unwrap();
        f.runtime.request_stop();
        for _ in 0..40 {
            f.runtime.poll(&mut f.store, Some(f.exclusion.clone()));
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!f.runtime.drained(), "takeover stays blocked");
        assert_eq!(ledger::undrained(&f.store.db).unwrap(), 1, "charged");
        assert!(matches!(
            custody::probe(&f.location, &record),
            Observation::Held
        ));
        drop(release); // EOF is the one explicit release of the descendant.
        f.until("drained", |f| {
            f.runtime.drained()
                && matches!(
                    custody::probe(&f.location, &record),
                    Observation::Released { .. }
                )
        });
        assert_eq!(ledger::undrained(&f.store.db).unwrap(), 0);
        let row = f.row(&original);
        assert!(row.receipt.drained);
        assert_eq!(
            (row.receipt.phase, row.receipt.code.as_str()),
            (Phase::Unknown, "task_message_output_retained")
        );
    }

    #[test]
    fn the_macos_fence_refuses_before_any_row_lease_or_child() {
        let mut f = Fixture::new(|_, _, _, _, _| panic!("no fenced child"));
        f.runtime.fixture_mac_fence(true);
        let original = f.original();
        let admission = f.admit(&original);
        assert!(!admission.accepted && !admission.durable);
        assert_eq!(admission.code, runtime::MAC_UNAVAILABLE);
        assert!(admission.receipt.is_none());
        assert!(ledger::load(&f.store.db, &original.plan.request_id)
            .unwrap()
            .is_none());
        assert_eq!(ledger::undrained(&f.store.db).unwrap(), 0);
        for _ in 0..20 {
            f.runtime.poll(&mut f.store, Some(f.exclusion.clone()));
        }
        assert!(!f.runtime.status()["working"].as_bool().unwrap());
    }

    /// review-n's probes at f5791071, kept as regressions.
    #[test]
    #[cfg(target_os = "linux")]
    fn a_native_permit_that_expires_while_begin_waits_admits_nothing() {
        let mut f = Fixture::new(|_, _, _, _, _| panic!("expired permit must not execute"));
        let original = f.original();
        let request = zerus_client_core::desktop::ui_requests::Request::TaskMessage {
            intent: Box::new(original.intent.clone()),
        };
        let authority = ui_authority::Validated::fixture_expiring(
            &json!(request),
            Instant::now() + Duration::from_millis(150),
        );
        let database = f.location.root.join("state.sqlite3");
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let writer = std::thread::spawn(move || {
            let mut db = rusqlite::Connection::open(database).unwrap();
            let tx = db
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .unwrap();
            ready_tx.send(()).unwrap();
            std::thread::sleep(Duration::from_millis(300));
            tx.commit().unwrap();
        });
        ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let answer = f
            .runtime
            .frontend(&mut f.store, authority, Some(f.exclusion.clone()));
        writer.join().unwrap();
        assert_eq!(answer["accepted"], false, "consumed after it expired");
        assert!(ledger::load(&f.store.db, &original.plan.request_id)
            .unwrap()
            .is_none());
    }

    #[test]
    fn a_revoke_after_reservation_prevents_the_message_step() {
        let calls = Arc::new(AtomicUsize::new(0));
        let called = calls.clone();
        let mut f = Fixture::new(move |_, _, _, _, _| {
            called.fetch_add(1, Ordering::SeqCst);
            Ended::Unknown("lost")
        });
        let original = f.original();
        assert!(f.admit(&original).accepted);
        f.until("reserved before the step", |f| {
            f.row(&original).receipt.phase == Phase::Running
        });
        f.store
            .db
            .execute("UPDATE desktop_writer SET revoked=1 WHERE id=1", [])
            .unwrap();
        f.until("refused and drained", |f| f.row(&original).receipt.drained);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(f.row(&original).receipt.phase, Phase::Unknown);
    }

    #[test]
    fn duplicates_while_running_and_after_a_lost_result_never_dispatch_again() {
        let calls = Arc::new(AtomicUsize::new(0));
        let called = calls.clone();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let release_rx = std::sync::Mutex::new(release_rx);
        let mut f = Fixture::new(move |_, _, _, _, _| {
            called.fetch_add(1, Ordering::SeqCst);
            release_rx
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            Ended::Unknown("task_message_reply_lost")
        });
        let original = f.original();
        assert!(f.admit(&original).accepted);
        f.until("running child", |_| calls.load(Ordering::SeqCst) == 1);
        for _ in 0..3 {
            assert!(f.admit(&original).accepted);
        }
        release_tx.send(()).unwrap();
        f.until("Unknown drained", |f| f.row(&original).receipt.drained);
        assert_eq!(f.row(&original).receipt.phase, Phase::Unknown);
        for _ in 0..3 {
            assert!(f.admit(&original).accepted);
            f.runtime.poll(&mut f.store, Some(f.exclusion.clone()));
        }
        f.runtime.request_stop();
        f.until("drained", |f| f.runtime.drained());
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "only the original dispatch"
        );
    }

    /// The real original path, without an executor: the runtime's pinned
    /// image (a script here) receives the exact call and frozen stdin.
    #[test]
    #[cfg(target_os = "linux")]
    fn the_pinned_image_runs_the_original_call_and_its_exact_reply_settles() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hgs");
        let seen = dir.path().join("stdin");
        std::fs::write(
            &path,
            format!(
                "#!/usr/bin/env python3\nimport json,pathlib,sys\npathlib.Path({:?}).write_bytes(sys.stdin.buffer.read())\na=sys.argv\nprint(json.dumps({{'task':{{'uuid':a[a.index('message')+1],'project':a[a.index('--project')+1]}},'message':{{'kind':'message','request_id':a[a.index('--request-id')+1],'session':None,'delivery':{{'mode':a[a.index('--mode')+1],'state':'queued','stage':'todo','stage_at':7}}}}}}))\n",
                seen.to_str().unwrap()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut f = Fixture::unexecuted();
        f.runtime.fixture_image(&path);
        let original = f.original();
        assert!(f.admit(&original).accepted);
        f.until("settled", |f| f.row(&original).receipt.drained);
        let receipt = f.row(&original).receipt;
        assert_eq!(
            (receipt.phase, receipt.code.as_str()),
            (Phase::Recorded, "")
        );
        assert_eq!(
            std::fs::read_to_string(&seen).unwrap(),
            message_input(&message_intent("hello", &[], "queue", ""))
        );
    }

    #[test]
    fn a_reference_form_original_is_refused_before_any_row_until_it_can_be_prepared() {
        let mut f = Fixture::new(|_, _, _, _, _| panic!("no reference child"));
        let intent = zerus_client_core::tasks_write::message_intent(
            "see this",
            &[zerus_client_core::attachments::Attachment {
                name: "screen.png".into(),
                mime: "image/png".into(),
                bytes: vec![7; 3000].into(),
                reference: "[screen.png]".into(),
            }],
            "queue",
            "",
        );
        let request = uuid::Uuid::new_v4().to_string();
        let record = json!({"id": request, "intent": intent,
            "input_sha256": input_digest(&message_input(&intent))});
        let (projection, _) = zerus_client_core::tasks_write::references::project(&record).unwrap();
        let referenced = Intent {
            scope: wire::Scope {
                configuration: f.location.configuration.clone(),
                service_epoch: f.epoch.clone(),
            },
            project: "example".into(),
            task_id: TASK.into(),
            request_id: request.clone(),
            original: projection.to_string(),
            form: wire::Form::ReferenceV1,
        }
        .verify()
        .unwrap();
        let admission = f.admit(&referenced);
        assert!(!admission.accepted);
        assert_eq!(admission.code, "task_message_unavailable");
        assert!(ledger::load(&f.store.db, &request).unwrap().is_none());
    }

    #[test]
    fn a_revoked_writer_refuses_the_step_and_no_child_starts() {
        let sent = Arc::new(AtomicUsize::new(0));
        let count = sent.clone();
        let mut f = Fixture::new(move |_, _, _, _, _| {
            count.fetch_add(1, Ordering::SeqCst);
            Ended::Exited {
                code: 0,
                stdout: Vec::new(),
            }
        });
        let original = f.original();
        f.store
            .db
            .execute("UPDATE desktop_writer SET revoked=1 WHERE id=1", [])
            .unwrap();
        let _ = f.admit(&original);
        for _ in 0..50 {
            f.runtime.poll(&mut f.store, Some(f.exclusion.clone()));
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(sent.load(Ordering::SeqCst), 0, "no child after revoke");
        assert!(!f.admit(&f.original()).accepted);
    }

    #[test]
    fn restart_retires_a_queued_message_and_never_reconstructs_execution_consent() {
        let mut f = Fixture::new(|_, _, _, _, _| panic!("recovered message must not execute"));
        let original = f.original();
        assert!(f.admit(&original).accepted);
        f.runtime = runtime::Runtime::new(&f.config, &f.epoch, f.owner.clone()).unwrap();
        f.runtime.fixture_executor(
            MACHINE,
            Arc::new(|_, _, _, _, _| panic!("restart must not execute")),
        );
        f.store = Store::open(&f.location, &f.epoch).unwrap();
        zerus_desktop_ledger::authorize(
            &mut f.store.db,
            &uuid::Uuid::new_v4().to_string(),
            &f.epoch,
        )
        .unwrap();
        f.until("retired", |f| f.row(&original).receipt.drained);
        assert_eq!(f.row(&original).receipt.phase, Phase::Unknown);
        assert_eq!(f.row(&original).receipt.code, "task_message_interrupted");
    }
}
