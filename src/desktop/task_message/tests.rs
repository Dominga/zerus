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
                soon()
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
                soon()
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
                soon()
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
                soon()
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
                Instant::now() + Duration::from_millis(300)
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
                soon()
            ),
            Ended::Unknown("task_message_signalled")
        );
    }

    #[test]
    fn only_a_child_that_never_existed_is_not_started() {
        let (_temp, _location, _record, held) = lock();
        assert_eq!(
            child::run(Path::new("/nonexistent/zerus-hgs"), &[], b"", &held, soon()),
            Ended::NotStarted
        );
        child::REFUSE_PRE_EXEC.with(|refuse| refuse.set(true));
        assert_eq!(
            child::run(
                Path::new("/usr/bin/env"),
                &["true".into()],
                b"",
                &held,
                soon()
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
            let ended = child::run(
                Path::new("/usr/bin/env"),
                &python(if read {
                    "pass"
                } else {
                    "import time; time.sleep(2)"
                }),
                b"",
                &held,
                soon(),
            );
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
        }
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
                soon()
            ),
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
        let mut f = Fixture::new(move |plan, args, input, _| {
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
            let mut f = Fixture::new(move |_, _, _, _| result.lock().unwrap().take().unwrap());
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
    fn a_reference_form_original_is_refused_before_any_row_until_it_can_be_prepared() {
        let mut f = Fixture::new(|_, _, _, _| panic!("no reference child"));
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
        let mut f = Fixture::new(move |_, _, _, _| {
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
        let mut f = Fixture::new(|_, _, _, _| panic!("recovered message must not execute"));
        let original = f.original();
        assert!(f.admit(&original).accepted);
        f.runtime = runtime::Runtime::new(&f.config, &f.epoch, f.owner.clone()).unwrap();
        f.runtime.fixture_executor(
            MACHINE,
            Arc::new(|_, _, _, _| panic!("restart must not execute")),
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
