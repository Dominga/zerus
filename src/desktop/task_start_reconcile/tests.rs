//! Actual actor + child/pipe/custody mechanisms with synthetic native admission
//! and source fixtures. These controls do not qualify installed UI permission.
use super::*;
use std::{
    fs,
    os::unix::{fs::PermissionsExt, net::UnixListener},
    path::PathBuf,
    process::{Child, Stdio},
};
// Only the Linux image tests start a process of their own.
#[cfg(target_os = "linux")]
use std::process::Command;
use zerus_client_core::{desktop::ui_requests::Request, operation_recovery::task_start as policy};

const HELPER: &str = r#"#!/usr/bin/env python3
import json,os,pathlib,socket,sqlite3,sys,time
p=pathlib.Path(os.environ['HOME'])
assert 'HGS_SESSION' not in os.environ and 'HGS_RUN_ID' not in os.environ
body=sys.stdin.buffer.read()
db=sqlite3.connect('file:'+str(p/'state/desktop/state.sqlite3')+'?mode=ro',uri=True)
row=db.execute("select c.helper from desktop_operation_custody c join desktop_operation_attempts a using(attempt_id) where a.state='reserved' and a.drained=0").fetchone()
assert row and json.loads(row[0])['pid']==os.getpid(), 'stdin arrived before same-child registration'
db.close()
(p/'input').write_bytes(body)
(p/'args').write_text(json.dumps(sys.argv[1:]))
(p/'entered').write_text('registered actual helper')
mode=(p/'mode').read_text()
if mode=='wait':
 end=time.monotonic()+12
 while not (p/'release').exists():
  if time.monotonic()>=end: os._exit(71)
  time.sleep(.002)
if mode in ('pipe','pipe-no-lock','lock-only'):
 pid=os.fork()
 if pid==0:
  os.close(0);os.close(2)
  if mode=='lock-only': os.close(1)
  if mode=='pipe-no-lock':
   # InheritedLock preserves its original descriptor, not a fixed fd 3.
   # Keep only stdout so this branch tests the reader's resource custody.
   fds=os.listdir('/dev/fd')
   assert len(fds)<=8192
   for item in fds:
    fd=int(item)
    if fd>2:
     try: os.close(fd)
     except OSError: pass
  if os.environ.get('TASK_FIXTURE_DESCENDANT_CONTROL')=='1':
   # The original test owns the other endpoint. EOF also releases this child
   # after a panic/crash; scheduling alone must never retire its pipe custody.
   control=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM)
   control.connect(str(p/'c'))
   (p/'survivor').write_text(str(os.getpid()))
   assert control.recv(1)==b'', 'unexpected fixture release input'
  else:
   (p/'survivor').write_text(str(os.getpid()))
   end=time.monotonic()+12
   while not (p/'release').exists():
    if time.monotonic()>=end: os._exit(72)
    time.sleep(.002)
  os._exit(0)
sys.stdout.buffer.write((p/'reply').read_bytes());sys.stdout.buffer.flush()
"#;

struct Fixture {
    temp: tempfile::TempDir,
    config: Config,
    store: Store,
    runtime: Runtime,
    exclusion: Arc<HandoffLock>,
    agent: Child,
    intent: wire::Intent,
}
impl Fixture {
    fn new(mode: &str) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
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
        fs::create_dir(&config.dir).unwrap();
        let location = Location::new(&config).unwrap();
        location.prepare().unwrap();
        let epoch = uuid::Uuid::new_v4().to_string();
        let mut store = Store::open(&location, &epoch).unwrap();
        zerus_desktop_ledger::authorize(&mut store.db, &uuid::Uuid::new_v4().to_string(), &epoch)
            .unwrap();
        let owner = Arc::new(location.lock().unwrap());
        let exclusion = Arc::new(location.handoff_lock().unwrap());
        let machine = uuid::Uuid::new_v4().to_string();
        let run = uuid::Uuid::new_v4().to_string();
        let conversation = uuid::Uuid::new_v4().to_string();
        let name = "codex/private-task-start";
        // The stand-in is owned until Fixture::drop, not until an unrelated
        // wall-clock timeout expires while other tests occupy the scheduler.
        let mut agent = crate::test_process::echo_command();
        agent
            .env("HGS_RUN_ID", &run)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::process::CommandExt;
            let parent = std::process::id() as libc::pid_t;
            // This private fixture cannot leave its stand-in agent alive when
            // the crash control kills the original libtest actor process.
            unsafe {
                agent.pre_exec(move || {
                    if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    if libc::getppid() != parent {
                        return Err(std::io::ErrorKind::Interrupted.into());
                    }
                    Ok(())
                });
            }
        }
        let mut agent = agent.spawn().unwrap();
        // The private test image exposes its own environment on Darwin too;
        // the echo still proves exec before native identity capture.
        crate::test_process::echo_ready(&mut agent);
        let record = json!({"version":1,"run_identity_version":1,"name":name,"agent":"codex","run_id":run,
            "conversation_id":conversation,"pid":agent.id(),"process_start":platform::process_start_time(agent.id()).unwrap(),
            "supervisor":{"pid":std::process::id(),"start":platform::process_start_time(std::process::id()).unwrap()}});
        let path = config
            .state
            .join(format!("{}.json", operations::hash(name.as_bytes())));
        fs::write(&path, record.to_string()).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../client/fixtures/desktop-operation-port-v1.json"
        ))
        .unwrap();
        let mut raw: Value = serde_json::from_str(fixture["raw_record"].as_str().unwrap()).unwrap();
        let plan = &mut raw["intent"]["plan"];
        plan["selection"] = json!({"host":"","name":name,"session_id":format!("run_id:{run}"),"run_id":run,"conversation_id":conversation});
        plan["session"] =
            json!({"machine":machine,"session":name,"session_id":format!("run_id:{run}")});
        plan["request"]["expected_run_id"] = json!(run);
        plan["request"]["expected_conversation_id"] = json!(conversation);
        let input = format!(" \n{}\n ", serde_json::to_string_pretty(plan).unwrap());
        raw["intent"]["retained_input"] = json!(input);
        let mut binding: policy::Binding =
            serde_json::from_value(fixture["binding"].clone()).unwrap();
        binding.key.configuration = location.configuration.clone();
        binding.executor = policy::Executor {
            machine: machine.clone(),
            session_id: format!("run_id:{run}"),
            run_id: run,
            conversation_id: conversation,
        };
        let original = raw.to_string();
        binding.source_record_sha256 = operations::hash(original.as_bytes());
        binding.ui_snapshot_sha256 = zerus_client_core::tasks_write::outbox::snapshot(&raw);
        binding.task_start_reconcile_input_v1_sha256 = operations::hash(input.as_bytes());
        let intent = wire::Intent {
            scope: wire::Scope {
                configuration: location.configuration.clone(),
                service_epoch: epoch.clone(),
            },
            reconcile_id: uuid::Uuid::new_v4().to_string(),
            binding,
            original,
        };
        intent.clone().verify().unwrap();
        let image = root.join("private-helper");
        fs::write(&image, HELPER).unwrap();
        fs::set_permissions(&image, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(root.join("mode"), mode).unwrap();
        fs::write(root.join("reply"),json!({"schema":1,"id":intent.binding.key.request_id,"project":intent.binding.key.project,"uuid":intent.binding.key.task,"state":"submitted","task":{"uuid":intent.binding.key.task,"project":intent.binding.key.project}}).to_string()).unwrap();
        let mut runtime = Runtime::new(&config, &epoch, owner).unwrap();
        runtime.machine = Some(Machine::fixture(&machine));
        runtime.image = Some(image);
        for (key, leaf) in [
            ("XDG_CONFIG_HOME", "xdg-config"),
            ("XDG_DATA_HOME", "xdg-data"),
            ("XDG_STATE_HOME", "xdg-state"),
            ("XDG_CACHE_HOME", "xdg-cache"),
            ("XDG_RUNTIME_DIR", "runtime"),
            ("TMPDIR", "tmp"),
            ("TMUX_TMPDIR", "tmux"),
        ] {
            let path = root.join(leaf);
            fs::create_dir_all(&path).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            runtime
                .environment
                .push((key.into(), path.to_str().unwrap().into()));
        }
        runtime.poll(&mut store, Some(exclusion.clone()));
        Self {
            temp,
            config,
            store,
            runtime,
            exclusion,
            agent,
            intent,
        }
    }
    fn path(&self, name: &str) -> PathBuf {
        self.temp.path().join(name)
    }
    fn request(&mut self) -> Arc<Queue> {
        let request = Request::TaskStartReconcile {
            intent: Box::new(self.intent.clone()),
        };
        let queue = Arc::new(Queue::default());
        self.runtime
            .frontend(
                &mut self.store,
                ui_authority::Validated::fixture(&json!(request)),
                Some(self.exclusion.clone()),
                queue.clone(),
                1,
            )
            .unwrap();
        queue
    }
    fn expiring_request(&mut self, until: Instant) -> Arc<Queue> {
        let request = Request::TaskStartReconcile {
            intent: Box::new(self.intent.clone()),
        };
        let queue = Arc::new(Queue::default());
        self.runtime
            .frontend(
                &mut self.store,
                ui_authority::Validated::fixture_expiring(&json!(request), until),
                Some(self.exclusion.clone()),
                queue.clone(),
                1,
            )
            .unwrap();
        queue
    }
    fn poll(&mut self) {
        self.store.retry_operation_custody();
        self.runtime
            .poll(&mut self.store, Some(self.exclusion.clone()));
    }
    fn prepared(&mut self) {
        let until = Instant::now() + Duration::from_secs(1);
        while !matches!(&self.runtime.work, Some(Work::Preparing(p)) if p.worker.is_finished()) {
            assert!(
                Instant::now() < until,
                "original preparation did not finish"
            );
            thread::sleep(Duration::from_millis(1));
        }
        let Some(Work::Preparing(pending)) = &mut self.runtime.work else {
            unreachable!()
        };
        // Prove capture and custody succeeded before the test changes anything.
        // Return those SAME objects to the actor, without cloning/recreating a
        // native proof, altering a permit or letting poll admit them early.
        let prepared = pending.ready.try_recv().unwrap().unwrap();
        prepared
            .0
            .current(Instant::now() + Duration::from_secs(2))
            .unwrap();
        let (send, ready) = mpsc::sync_channel(1);
        assert!(send.send(Ok(prepared)).is_ok());
        pending.ready = ready;
    }
    #[track_caller]
    fn until(&mut self, done: impl Fn(&Self) -> bool) {
        let end = Instant::now() + Duration::from_secs(8);
        while !done(self) {
            assert!(
                Instant::now() < end,
                "actor timed out: {}; counts={:?}; entered={}; survivor={}",
                self.runtime.status(),
                self.counts(),
                self.path("entered").exists(),
                self.path("survivor").exists()
            );
            self.poll();
            thread::sleep(Duration::from_millis(2));
        }
    }
    fn state(&self) -> String {
        self.store
            .db
            .query_row("SELECT state FROM desktop_operation_attempts", [], |r| {
                r.get(0)
            })
            .unwrap()
    }
    fn counts(&self) -> (i64, i64, i64) {
        self.store.db.query_row("SELECT (SELECT count(*) FROM desktop_operations),(SELECT count(*) FROM desktop_operation_attempts),(SELECT count(*) FROM desktop_operation_custody)",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::write(self.path("release"), b"fixture cleanup");
        self.runtime.request_stop();
        let end = Instant::now() + Duration::from_secs(5);
        while self.runtime.work.is_some() && Instant::now() < end {
            self.poll();
            thread::sleep(Duration::from_millis(2));
        }
        let _ = self.agent.kill();
        let _ = self.agent.wait();
    }
}
#[test]
fn fixed_worker_registers_same_child_before_exact_input_and_original_lookup_never_replays() {
    let mut f = Fixture::new("reply");
    let queue = f.request();
    f.until(|f| f.runtime.work.is_none());
    let frame: Value = serde_json::from_slice(&queue.take().unwrap()).unwrap();
    assert_eq!(frame["value"]["accepted"], true, "{frame}");
    assert_eq!(f.state(), "submitted");
    assert_eq!(f.counts(), (1, 1, 1));
    let raw: Value = serde_json::from_str(&f.intent.original).unwrap();
    assert_eq!(
        fs::read(f.path("input")).unwrap(),
        raw["intent"]["retained_input"].as_str().unwrap().as_bytes()
    );
    let args: Vec<String> = serde_json::from_slice(&fs::read(f.path("args")).unwrap()).unwrap();
    assert_eq!(
        args,
        vec![
            "task",
            "start-deliver",
            &f.intent.binding.key.task,
            "--project",
            &f.intent.binding.key.project,
            "--json",
            "--recover-only"
        ]
    );
    let before = fs::metadata(f.path("input")).unwrap().modified().unwrap();
    let queue = f.request();
    let frame: Value = serde_json::from_slice(&queue.take().unwrap()).unwrap();
    assert_eq!(frame["value"]["read_only"], true, "{frame}");
    assert_eq!(
        fs::metadata(f.path("input")).unwrap().modified().unwrap(),
        before
    );
    assert_eq!(f.counts(), (1, 1, 1));
}
#[test]
fn refused_same_child_registration_never_releases_original_input() {
    let mut f = Fixture::new("reply");
    let helper = HELPER.replace(
        "body=sys.stdin.buffer.read()",
        "body=sys.stdin.buffer.read()\n(p/'registration-input').write_bytes(body)",
    );
    fs::write(f.runtime.image.as_ref().unwrap(), helper).unwrap();
    f.store.db.execute_batch("CREATE TRIGGER refuse_original_registration BEFORE UPDATE OF helper ON desktop_operation_custody BEGIN SELECT RAISE(ABORT,'fixture registration refused'); END;").unwrap();
    let queue = f.request();
    f.until(|f| f.runtime.work.is_none());
    let frame: Value = serde_json::from_slice(&queue.take().unwrap()).unwrap();
    assert_eq!(frame["value"]["code"], "reserved");
    assert_eq!(f.counts(), (1, 1, 1));
    assert_eq!(f.state(), "unknown");
    assert!(!f.runtime.failed);
    assert!(f.runtime.status()["ready"].as_bool().unwrap());
    let helper: Option<String> = f
        .store
        .db
        .query_row("SELECT helper FROM desktop_operation_custody", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(helper.is_none());
    let received = match fs::read(f.path("registration-input")) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => panic!("original helper input evidence unavailable: {error}"),
    };
    assert!(
        received.is_empty(),
        "refused registration released original input"
    );
    assert!(!f.path("input").exists());
    assert!(!f.path("entered").exists());
    assert_eq!(ledger::totals(&f.store.db).unwrap().undrained, 0);
}
#[test]
fn revoked_worker_retains_same_ticket_and_finishes_unknown_without_another_child() {
    let mut f = Fixture::new("wait");
    f.request();
    f.until(|f| f.path("entered").exists());
    let ticket: String = f
        .store
        .db
        .query_row("SELECT ticket FROM desktop_operation_attempts", [], |r| {
            r.get(0)
        })
        .unwrap();
    zerus_desktop_ledger::revoke(&f.store.db, "rollback").unwrap();
    f.runtime.request_stop();
    f.until(|f| f.runtime.drained());
    assert_eq!(f.state(), "unknown");
    assert_eq!(f.counts(), (1, 1, 1));
    assert_eq!(
        f.store
            .db
            .query_row::<String, _, _>("SELECT ticket FROM desktop_operation_attempts", [], |r| r
                .get(0))
            .unwrap(),
        ticket
    );
}
#[test]
fn descendant_pipe_keeps_original_serve_and_handoff_custody_after_outcome() {
    for mode in ["pipe", "pipe-no-lock"] {
        let mut f = Fixture::new(mode);
        // Controlled scheduling witness: a wall-clock backstop is already
        // expired. Only the explicitly owned EOF may release this descendant.
        let helper = HELPER.replace("end=time.monotonic()+12", "end=time.monotonic()-1");
        assert_ne!(helper, HELPER);
        fs::write(f.runtime.image.as_ref().unwrap(), helper).unwrap();
        // An original socket endpoint, rather than a timed descendant, pins
        // the arrange phase until all physical-custody assertions are made.
        let listener = UnixListener::bind(f.path("c")).unwrap();
        listener.set_nonblocking(true).unwrap();
        f.runtime
            .environment
            .push(("TASK_FIXTURE_DESCENDANT_CONTROL".into(), "1".into()));
        let reply = f.request();
        f.until(|f| {
            assert!(
                f.runtime.work.is_some(),
                "original request refused before descendant startup: {:?}",
                reply.take()
            );
            f.path("survivor").exists()
        });
        // The marker follows connect(), so accept cannot need another actor
        // turn. Keep the SAME accepted endpoint through the logical outcome.
        let (release, _) = listener.accept().unwrap();
        f.until(|f| matches!(f.runtime.work, Some(Work::Draining { .. })));
        assert_eq!(f.state(), "unknown");
        assert_eq!(ledger::totals(&f.store.db).unwrap().undrained, 1);
        let (_, record) = ledger::custody::pending(&f.store.db).unwrap().unwrap();
        let record = record.unwrap();
        assert!(matches!(
            custody::probe(&f.runtime.location, &record),
            custody::Observation::Held
        ));
        f.runtime.request_stop();
        for _ in 0..5 {
            f.poll();
        }
        assert!(!f.runtime.drained());
        assert_eq!(ledger::totals(&f.store.db).unwrap().undrained, 1);
        assert!(matches!(
            custody::probe(&f.runtime.location, &record),
            custody::Observation::Held
        ));
        drop(release); // EOF is the one explicit release of this descendant.
        f.until(|f| {
            f.runtime.drained()
                && matches!(
                    custody::probe(&f.runtime.location, &record),
                    custody::Observation::Released { .. }
                )
        });
        assert_eq!(ledger::totals(&f.store.db).unwrap().undrained, 0);
        assert!(f.runtime.work.is_none());
    }
}
#[test]
fn changed_executor_during_preparation_refuses_before_any_sql_or_helper() {
    let mut f = Fixture::new("reply");
    let queue = f.request();
    f.agent.kill().unwrap();
    f.agent.wait().unwrap();
    f.until(|f| f.runtime.work.is_none());
    assert_eq!(f.counts(), (0, 0, 0));
    assert!(!f.path("entered").exists());
    let frame: Value = serde_json::from_slice(&queue.take().unwrap()).unwrap();
    assert_eq!(frame["value"]["accepted"], false, "{frame}");
}
#[test]
fn executor_changed_after_successful_preparation_refuses_before_admission() {
    let mut f = Fixture::new("reply");
    let queue = f.request();
    f.prepared();
    f.agent.kill().unwrap();
    f.agent.wait().unwrap();
    f.until(|f| f.runtime.work.is_none());
    assert_eq!(f.counts(), (0, 0, 0));
    assert!(!f.path("entered").exists());
    let frame: Value = serde_json::from_slice(&queue.take().unwrap()).unwrap();
    assert_eq!(frame["value"]["accepted"], false, "{frame}");
}
#[test]
fn another_kind_admitted_after_preparation_blocks_the_reservation() {
    use zerus_desktop_ledger::operations::cache;
    let mut f = Fixture::new("reply");
    let queue = f.request();
    f.prepared();
    assert_eq!(f.counts(), (0, 0, 0));
    assert_eq!(f.store.operation_undrained().unwrap(), 0);
    // Use the real independent kind's intake transaction. Its charge is not
    // covered by the task-only partial unique index or task totals.
    let original = cache::Original {
        preparation_id: uuid::Uuid::new_v4().to_string(),
        operation_id: uuid::Uuid::new_v4().to_string(),
        configuration: f.intent.scope.configuration.clone(),
        root: f.path("private-cache").to_str().unwrap().into(),
        limit: 1,
        defer_ms: 0,
        policy_change: true,
        revision: 0,
        service_epoch: f.intent.scope.service_epoch.clone(),
        writer_epoch: f.runtime.lease.as_ref().unwrap().writer_epoch.clone(),
        lease: uuid::Uuid::new_v4().to_string(),
    };
    let tx = f
        .store
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    assert!(cache::begin(&tx, &original, &"f".repeat(64)).unwrap());
    tx.commit().unwrap();
    assert_eq!(cache::undrained(&f.store.db).unwrap(), 1);
    assert_eq!(ledger::totals(&f.store.db).unwrap().undrained, 0);
    assert_eq!(f.store.operation_undrained().unwrap(), 1);
    f.until(|f| f.runtime.work.is_none());
    assert_eq!(f.counts(), (0, 0, 0));
    assert_eq!(cache::undrained(&f.store.db).unwrap(), 1);
    assert!(!f.path("entered").exists());
    let frame: Value = serde_json::from_slice(&queue.take().unwrap()).unwrap();
    assert_eq!(frame["value"]["accepted"], false, "{frame}");
}
#[test]
fn durable_custody_refusal_rolls_back_original_and_attempt_and_never_spawns() {
    let mut f = Fixture::new("reply");
    f.store.db.execute_batch("CREATE TRIGGER refuse_private_custody BEFORE INSERT ON desktop_operation_custody BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    f.request();
    f.until(|f| f.runtime.work.is_none());
    assert_eq!(f.counts(), (0, 0, 0));
    assert!(!f.path("entered").exists());
}

#[test]
fn native_proof_expiring_while_begin_waits_cannot_admit_after_the_writer_lock() {
    let mut f = Fixture::new("reply");
    f.store.db.busy_timeout(Duration::from_secs(3)).unwrap();
    let expires = Instant::now() + Duration::from_secs(2);
    let queue = f.expiring_request(expires);
    let end = Instant::now() + Duration::from_secs(1);
    while !matches!(&f.runtime.work,Some(Work::Preparing(p)) if p.worker.is_finished()) {
        assert!(Instant::now() < end);
        thread::sleep(Duration::from_millis(1));
    }
    let path = f.runtime.location.root.join("state.sqlite3");
    let (send, receive) = mpsc::sync_channel(1);
    let blocker = thread::spawn(move || {
        let mut db = rusqlite::Connection::open(path).unwrap();
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        send.send(()).unwrap();
        thread::sleep(
            (expires + Duration::from_millis(10)).saturating_duration_since(Instant::now()),
        );
        tx.commit().unwrap();
    });
    receive.recv_timeout(Duration::from_secs(1)).unwrap();
    f.poll();
    blocker.join().unwrap();
    assert_eq!(f.counts(), (0, 0, 0));
    assert!(!f.path("entered").exists());
    let frame: Value = serde_json::from_slice(&queue.take().unwrap()).unwrap();
    assert_eq!(frame["value"]["accepted"], false, "{frame}");
}

#[test]
fn restart_unknown_is_immutable_and_a_new_actor_does_not_reconstruct_consent() {
    let mut f = Fixture::new("wait");
    f.request();
    f.until(|f| f.path("entered").exists());
    let next = uuid::Uuid::new_v4().to_string();
    assert_eq!(ledger::recover_batch(&mut f.store.db, &next).unwrap(), 1);
    let outcome: String = f
        .store
        .db
        .query_row(
            "SELECT outcome_json FROM desktop_operation_attempts",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let owner = f.runtime.owner.clone();
    let mut restarted = Runtime::new(&f.config, &next, owner).unwrap();
    restarted.machine = f.runtime.machine.clone();
    restarted.image = f.runtime.image.clone();
    restarted.poll(&mut f.store, Some(f.exclusion.clone()));
    assert!(restarted.work.is_none());
    assert_eq!(ledger::totals(&f.store.db).unwrap().undrained, 1);
    fs::write(
        f.path("release"),
        b"original helper completes after recovered Unknown",
    )
    .unwrap();
    f.until(|f| f.runtime.work.is_none());
    assert_eq!(f.state(), "unknown");
    assert_eq!(
        f.store
            .db
            .query_row::<String, _, _>(
                "SELECT outcome_json FROM desktop_operation_attempts",
                [],
                |r| r.get(0)
            )
            .unwrap(),
        outcome
    );
    assert_eq!(f.counts(), (1, 1, 1));
}
#[test]
fn stopped_runtime_without_work_keeps_old_epoch_custody_charged_until_actual_release() {
    let mut f = Fixture::new("wait");
    f.request();
    f.until(|f| f.path("entered").exists());
    let (_, record) = ledger::custody::pending(&f.store.db).unwrap().unwrap();
    let record = record.unwrap();
    assert!(matches!(
        custody::probe(&f.runtime.location, &record),
        custody::Observation::Held
    ));
    let next = uuid::Uuid::new_v4().to_string();
    assert_ne!(record.prepared.ticket.service_epoch, next);
    f.store = Store::open(&f.runtime.location, &next).unwrap();
    assert_eq!(f.state(), "unknown");
    let mut restarted = Runtime::new(&f.config, &next, f.runtime.owner.clone()).unwrap();
    restarted.request_stop();
    for _ in 0..3 {
        f.store.retry_operation_custody();
        restarted.poll(&mut f.store, Some(f.exclusion.clone()));
        assert!(restarted.work.is_none());
        assert_eq!(restarted.charges, Some(1));
        assert!(
            !restarted.drained(),
            "no in-memory work is not original drain"
        );
    }
    assert!(matches!(
        custody::probe(&f.runtime.location, &record),
        custody::Observation::Held
    ));
    fs::write(
        f.path("release"),
        b"original helper and its actual custody release",
    )
    .unwrap();
    f.until(|f| f.runtime.work.is_none());
    restarted.poll(&mut f.store, Some(f.exclusion.clone()));
    assert!(restarted.work.is_none());
    assert_eq!(restarted.charges, Some(0));
    assert!(restarted.drained());
    assert_eq!(f.state(), "unknown");
    assert_eq!(f.counts(), (1, 1, 1));
}

#[test]
fn captured_writer_is_strict_even_if_a_new_writer_is_current_before_reservation() {
    let mut f = Fixture::new("reply");
    f.request();
    zerus_desktop_ledger::authorize(
        &mut f.store.db,
        &uuid::Uuid::new_v4().to_string(),
        &f.intent.scope.service_epoch,
    )
    .unwrap();
    f.until(|f| f.runtime.work.is_none());
    assert_eq!(f.counts(), (0, 0, 0));
    assert!(!f.path("entered").exists());
}

#[test]
fn later_explicit_lookup_uses_old_original_admission_and_a_separate_attempt_after_drain() {
    let mut f = Fixture::new("wait");
    f.request();
    f.until(|f| f.path("entered").exists());
    ledger::recover_batch(&mut f.store.db, &uuid::Uuid::new_v4().to_string()).unwrap();
    fs::write(f.path("release"), b"original helper finishes").unwrap();
    f.until(|f| f.runtime.work.is_none());
    let first = f.intent.reconcile_id.clone();
    let before: (String, Vec<u8>, Vec<u8>, String) = f
        .store
        .db
        .query_row(
            "SELECT admission_id,raw_record,retained_input,provenance_json FROM desktop_operations",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    f.intent.reconcile_id = uuid::Uuid::new_v4().to_string();
    fs::write(f.path("mode"), b"reply").unwrap();
    f.request();
    f.until(|f| f.runtime.work.is_none());
    assert_eq!(f.counts(), (1, 2, 2));
    assert_eq!(f.store.db.query_row::<(String,Vec<u8>,Vec<u8>,String),_,_>("SELECT admission_id,raw_record,retained_input,provenance_json FROM desktop_operations",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).unwrap(),before);
    assert_eq!(
        f.store
            .db
            .query_row::<String, _, _>(
                "SELECT state FROM desktop_operation_attempts WHERE attempt_id=?1",
                [first],
                |r| r.get(0)
            )
            .unwrap(),
        "unknown"
    );
    assert_eq!(
        f.store
            .db
            .query_row::<String, _, _>(
                "SELECT state FROM desktop_operation_attempts WHERE attempt_id=?1",
                [&f.intent.reconcile_id],
                |r| r.get(0)
            )
            .unwrap(),
        "submitted"
    );
}

fn original_result_query(f: &Fixture) -> wire::Query {
    wire::Query {
        request_id: uuid::Uuid::new_v4().to_string(),
        scope: f.intent.scope.clone(),
        reconcile_id: f.intent.reconcile_id.clone(),
        binding: f.intent.binding.clone(),
    }
}
fn original_result(f: &Fixture, query: &wire::Query) -> wire::Lookup {
    let before = f.store.db.total_changes();
    let value = super::query(
        &f.store,
        &json!({"op":"task_start_reconcile_result","query":query}),
    );
    assert_eq!(f.store.db.total_changes(), before);
    let reply: wire::Lookup = serde_json::from_value(value.clone())
        .unwrap_or_else(|_| panic!("invalid result metadata: {value}"));
    assert!(reply.matches(query), "{value}");
    reply
}
#[test]
fn result_lookup_missing_and_completed_are_exact_reads_without_another_worker() {
    let mut f = Fixture::new("reply");
    let q = original_result_query(&f);
    let absent = original_result(&f, &q);
    assert!(!absent.found);
    assert!(absent.attempt.is_none());
    assert_eq!(f.counts(), (0, 0, 0));
    assert!(!f.path("entered").exists());
    f.request();
    f.until(|f| f.runtime.work.is_none());
    let before = f.counts();
    let changed = fs::metadata(f.path("input")).unwrap().modified().unwrap();
    let found = original_result(&f, &q);
    let attempt = found.attempt.unwrap();
    assert!(attempt.drained);
    assert_eq!(attempt.phase, wire::Phase::Submitted);
    let receipt = attempt.submitted(&q).unwrap();
    assert_eq!(receipt.id, f.intent.binding.key.request_id);
    assert_eq!(
        serde_json::to_value(&attempt.ticket).unwrap(),
        f.store
            .db
            .query_row::<String, _, _>("SELECT ticket FROM desktop_operation_attempts", [], |r| r
                .get(0))
            .map(|v| serde_json::from_str::<Value>(&v).unwrap())
            .unwrap()
    );
    assert_eq!(f.counts(), before);
    assert_eq!(
        fs::metadata(f.path("input")).unwrap().modified().unwrap(),
        changed
    );
    let mut absent = q.clone();
    absent.reconcile_id = uuid::Uuid::new_v4().to_string();
    let missing = original_result(&f, &absent);
    assert!(!missing.found);
    assert!(missing.attempt.is_none());
    assert_eq!(f.counts(), before);
}
#[test]
fn result_lookup_does_not_promote_submitted_metadata_while_original_kernel_custody_is_held() {
    let mut f = Fixture::new("lock-only");
    f.request();
    f.until(|f| f.path("survivor").exists());
    f.until(|f| matches!(f.runtime.work, Some(Work::Draining { .. })));
    let q = original_result_query(&f);
    let held = original_result(&f, &q).attempt.unwrap();
    assert_eq!(held.phase, wire::Phase::Submitted);
    assert!(!held.drained);
    assert!(held.submitted(&q).is_none());
    let (_, record) = ledger::custody::pending(&f.store.db).unwrap().unwrap();
    assert!(matches!(
        custody::probe(&f.runtime.location, &record.unwrap()),
        custody::Observation::Held
    ));
    fs::write(f.path("release"), b"original actual custody release").unwrap();
    f.until(|f| f.runtime.work.is_none());
    let drained = original_result(&f, &q).attempt.unwrap();
    assert!(drained.drained);
    assert_eq!(drained.ticket, held.ticket);
    assert_eq!(drained.outcome, held.outcome);
    assert!(drained.submitted(&q).is_some());
    assert_eq!(f.counts(), (1, 1, 1));
}
#[test]
fn result_lookup_keeps_reserved_and_restarted_unknown_separate_from_drain() {
    let mut f = Fixture::new("wait");
    f.request();
    f.until(|f| f.path("entered").exists());
    let mut q = original_result_query(&f);
    let reserved = original_result(&f, &q).attempt.unwrap();
    assert_eq!(reserved.phase, wire::Phase::Reserved);
    assert!(!reserved.drained);
    assert!(reserved.submitted(&q).is_none());
    let old = reserved.ticket.clone();
    let next = uuid::Uuid::new_v4().to_string();
    f.store = Store::open(&f.runtime.location, &next).unwrap();
    q.scope.service_epoch = next;
    let unknown = original_result(&f, &q).attempt.unwrap();
    assert_eq!(unknown.ticket, old);
    assert_eq!(unknown.phase, wire::Phase::Unknown);
    assert!(!unknown.drained);
    assert!(unknown.submitted(&q).is_none());
    fs::write(f.path("release"), b"original actual release").unwrap();
    f.until(|f| f.runtime.work.is_none());
    let drained = original_result(&f, &q).attempt.unwrap();
    assert_eq!(drained.ticket, old);
    assert_eq!(drained.phase, wire::Phase::Unknown);
    assert!(drained.drained);
    assert!(drained.submitted(&q).is_none());
    assert_eq!(f.counts(), (1, 1, 1));
}
#[test]
fn result_lookup_refuses_changed_scope_original_and_malformed_requests_without_writes() {
    let mut f = Fixture::new("reply");
    f.request();
    f.until(|f| f.runtime.work.is_none());
    let q = original_result_query(&f);
    let before = f.store.db.total_changes();
    for change in 0..7 {
        let mut bad = q.clone();
        match change {
            0 => bad.scope.service_epoch = uuid::Uuid::new_v4().to_string(),
            1 => bad.binding.source_record_sha256 = "f".repeat(64),
            2 => bad.binding.executor.run_id = uuid::Uuid::new_v4().to_string(),
            3 => bad.binding.key.request_id = uuid::Uuid::new_v4().to_string(),
            4 => bad.binding.key.task = uuid::Uuid::new_v4().to_string(),
            5 => bad.binding.key.project = "other".into(),
            _ => bad.binding.ui_snapshot_sha256 = "f".repeat(64),
        }
        let reply = super::query(
            &f.store,
            &json!({"op":"task_start_reconcile_result","query":bad}),
        );
        assert_eq!(
            reply["code"], "task_start_result_unavailable",
            "{change}:{reply}"
        );
    }
    let mut body = json!({"op":"task_start_reconcile_result","query":q});
    body["permit"] = json!(true);
    assert_eq!(
        super::query(&f.store, &body)["code"],
        "task_start_result_unavailable"
    );
    assert_eq!(f.store.db.total_changes(), before);
    assert_eq!(f.counts(), (1, 1, 1));
}

fn native_cli(f: &Fixture, args: &[&str], input: Option<Vec<u8>>) -> Value {
    use zerus_client_core::{actions::Exit, run::run_limited};
    let env: Vec<_> = f
        .runtime
        .environment
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let invocation = Invocation {
        args: args.iter().map(|v| v.to_string()).collect(),
        stdin: input,
        timeout: Duration::from_secs(10),
        request_id: None,
    };
    let exit = run_limited(
        f.runtime.image.as_ref().unwrap(),
        &invocation,
        &env,
        1 << 20,
    )
    .unwrap();
    let Exit::Code {
        code: 0, stdout, ..
    } = exit
    else {
        panic!("private CLI failed: {exit:?}")
    };
    serde_json::from_slice(&stdout).unwrap()
}
fn real_fixture(submitted: bool) -> Fixture {
    let mut f = Fixture::new("reply");
    let cli = std::fs::canonicalize(
        std::env::var_os("HGS_TEST_BIN").expect("explicit separately built production CLI"),
    )
    .unwrap();
    assert_ne!(
        platform::file_identity(&cli),
        platform::file_identity(&std::env::current_exe().unwrap()),
        "never run the libtest image as a CLI"
    );
    f.runtime.image = Some(cli);
    f.runtime.environment.extend([
        ("HGS_SELF".into(), "test".into()),
        ("HGS_PEERS".into(), String::new()),
    ]);
    let bin = f.path(".local/bin");
    fs::create_dir_all(&bin).unwrap();
    let tmux = bin.join("tmux");
    fs::write(
        &tmux,
        "#!/bin/sh\nprintf 'unexpected terminal call\\n' >> \"$HOME/terminal-called\"\nexit 93\n",
    )
    .unwrap();
    fs::set_permissions(tmux, fs::Permissions::from_mode(0o700)).unwrap();
    let project = "example-ёж";
    native_cli(&f,&["swarm","initialize"],Some(json!({"projects":[{"id":"private-project","name":project,"folders":[],"sessions":[]}]}).to_string().into_bytes()));
    let task = native_cli(
        &f,
        &["task", "add", "--project", project],
        Some(b"Private original task".to_vec()),
    )["task"]
        .clone();
    let request = json!({"schema":1,"id":f.intent.binding.key.request_id,"stage_at":task["stage_at"],"target":task["target"],
        "selection":{"host":"","name":"codex/private-task-start","session_id":f.intent.binding.executor.session_id,
            "run_id":f.intent.binding.executor.run_id,"conversation_id":f.intent.binding.executor.conversation_id}});
    let plan = native_cli(
        &f,
        &[
            "task",
            "start-plan",
            task["uuid"].as_str().unwrap(),
            "--project",
            project,
        ],
        Some(request.to_string().into_bytes()),
    )["plan"]
        .clone();
    // This test producer captures ONE actual delivery object before its private
    // checkpoint. The actor receives those bytes and never rebuilds this input.
    let input = plan.to_string();
    let mut record: Value = serde_json::from_str(&f.intent.original).unwrap();
    record["intent"]["plan"] = plan.clone();
    record["intent"]["retained_input"] = json!(input);
    f.intent.binding.key.task = task["uuid"].as_str().unwrap().into();
    // The native plan binds the canonical project ID, not its display name.
    f.intent.binding.key.project = plan["project"].as_str().unwrap().into();
    f.intent.binding.executor.machine = plan["session"]["machine"].as_str().unwrap().into();
    f.intent.original = record.to_string();
    f.intent.binding.source_record_sha256 = operations::hash(f.intent.original.as_bytes());
    f.intent.binding.ui_snapshot_sha256 = zerus_client_core::tasks_write::outbox::snapshot(&record);
    f.intent.binding.task_start_reconcile_input_v1_sha256 = operations::hash(input.as_bytes());
    f.intent.clone().verify().unwrap();
    f.runtime.machine = Some(Machine::fixture(&f.intent.binding.executor.machine));
    if submitted {
        let dir = f.config.state.join("input_receipts");
        fs::create_dir(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        let path = dir.join(format!("{}.json", f.intent.binding.key.request_id));
        // Existing native-receipt fixture: no terminal submission is performed
        // by setup. The real recover-only CLI must reconcile this exact record.
        // Every field `submit_core` writes, `first_message` included: a receipt
        // that does not decode whole is unknown evidence and settles nothing
        // (#1519 R4, zerus-kpj4c).
        fs::write(&path,json!({"status":"submitted","name":"codex/private-task-start",
            "run_id":f.intent.binding.executor.run_id,"conversation_id":f.intent.binding.executor.conversation_id,
            "first_message":f.intent.binding.executor.conversation_id.is_empty(),
            "request_id":f.intent.binding.key.request_id,"digest":operations::hash(plan["request"].to_string().as_bytes())}).to_string()).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    assert!(!f.path("terminal-called").exists());
    f
}

#[test]
#[ignore = "Explicit exact-source HGS_TEST_BIN after cargo build; synthetic native/source admission, real task CLI"]
fn real_cli_recover_only_reconciles_the_original_receipt_and_never_falls_back_to_terminal_input() {
    for submitted in [false, true] {
        let mut f = real_fixture(submitted);
        f.request();
        f.until(|f| f.runtime.work.is_none());
        let current = native_cli(
            &f,
            &[
                "task",
                "get",
                &f.intent.binding.key.task,
                "--project",
                &f.intent.binding.key.project,
            ],
            None,
        );
        assert_eq!(
            current["task"]["stage"],
            if submitted { "active" } else { "todo" }
        );
        assert_eq!(f.state(), if submitted { "submitted" } else { "unknown" });
        assert!(!f.path("terminal-called").exists());
        assert_eq!(f.counts(), (1, 1, 1));
        let original = ledger::load(
            &f.store.db,
            &operations::to_ledger(&f.intent.binding)
                .key
                .operation_id()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            original.retained_input,
            original_input(&f.intent.clone().verify().unwrap()).unwrap()
        );
        let before = current.clone();
        f.request();
        assert_eq!(
            native_cli(
                &f,
                &[
                    "task",
                    "get",
                    &f.intent.binding.key.task,
                    "--project",
                    &f.intent.binding.key.project
                ],
                None
            ),
            before
        );
        assert_eq!(f.counts(), (1, 1, 1));
    }
}

fn registered_helper(f: &Fixture) -> Option<Birth> {
    ledger::custody::pending(&f.store.db)
        .unwrap()
        .and_then(|(_, record)| record.and_then(|r| r.helper))
}
#[test]
#[ignore = "Explicit exact-source HGS_TEST_BIN; real owned CLI kill/revoke before input release"]
fn real_cli_original_child_kill_and_revoke_never_release_input_or_replay_after_reopen() {
    for kill in [true, false] {
        let mut f = real_fixture(true);
        let gate = Arc::new(AtomicBool::new(false));
        f.runtime.input_gate = Some(gate.clone());
        f.request();
        f.until(|f| registered_helper(f).is_some());
        let birth = registered_helper(&f).unwrap();
        let original = platform::PinnedProcess::pin(birth.pid, &birth.birth).unwrap();
        assert_eq!(
            platform::process_identity(birth.pid).unwrap().parent,
            std::process::id()
        );
        assert_eq!(
            platform::file_identity(std::path::Path::new(
                &platform::describe(birth.pid).unwrap().executable
            )),
            platform::file_identity(f.runtime.image.as_ref().unwrap())
        );
        if kill {
            original.kill().unwrap();
        } else {
            zerus_desktop_ledger::revoke(&f.store.db, "rollback").unwrap();
            f.runtime.request_stop();
        }
        gate.store(true, Ordering::Release);
        f.until(|f| f.runtime.work.is_none());
        assert!(!original.alive());
        assert_eq!(f.state(), "unknown");
        let original_outcome: String = f
            .store
            .db
            .query_row(
                "SELECT outcome_json FROM desktop_operation_attempts",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let next = uuid::Uuid::new_v4().to_string();
        let reopened = Store::open(&f.runtime.location, &next).unwrap();
        f.store = reopened;
        let mut actor = Runtime::new(&f.config, &next, f.runtime.owner.clone()).unwrap();
        actor.poll(&mut f.store, Some(f.exclusion.clone()));
        assert!(actor.work.is_none());
        assert_eq!(f.counts(), (1, 1, 1));
        assert_eq!(f.state(), "unknown");
        assert_eq!(
            f.store
                .db
                .query_row::<String, _, _>(
                    "SELECT outcome_json FROM desktop_operation_attempts",
                    [],
                    |r| r.get(0)
                )
                .unwrap(),
            original_outcome
        );
        let current = native_cli(
            &f,
            &[
                "task",
                "get",
                &f.intent.binding.key.task,
                "--project",
                &f.intent.binding.key.project,
            ],
            None,
        );
        assert_eq!(current["task"]["stage"], "todo");
        assert!(!f.path("terminal-called").exists());
    }
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "Private one-shot leaf, only the exact parent crash control may start it"]
fn real_cli_crash_fixture_child() {
    assert_eq!(
        std::env::var("TASK_START_CRASH_CHILD").as_deref(),
        Ok("task-start-crash-v1")
    );
    let control = PathBuf::from(
        std::env::var_os("TASK_START_CRASH_CONTROL").expect("private original parent control"),
    );
    assert!(platform::verify_private(&control, platform::Private::Directory).unwrap());
    let mut f = real_fixture(true);
    f.runtime.input_gate = Some(Arc::new(AtomicBool::new(false)));
    f.request();
    f.until(|f| registered_helper(f).is_some());
    let (attempt, record) = ledger::custody::pending(&f.store.db).unwrap().unwrap();
    let data = json!({"root":f.temp.path(),"configuration":f.runtime.scope.configuration,"service_epoch":f.runtime.scope.service_epoch,
        "ticket":attempt.ticket,"record":record,"task":f.intent.binding.key.task,"project":f.intent.binding.key.project,
        "agent":{"pid":f.agent.id(),"birth":platform::process_identity(f.agent.id()).unwrap().birth}});
    // Preserve only this private fixture's data until its original parent
    // verifies the exact custody and removes it after both originals exit.
    let staged = control.join("ready.tmp");
    let ready = control.join("ready.json");
    fs::write(&staged, data.to_string()).unwrap();
    fs::set_permissions(&staged, fs::Permissions::from_mode(0o600)).unwrap();
    fs::rename(staged, ready).unwrap();
    std::mem::forget(f);
    thread::sleep(Duration::from_secs(8));
    panic!("original crash parent did not settle its child within the fixed bound");
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "Explicit exact-source HGS_TEST_BIN; real actor process death and original CLI custody"]
fn real_cli_actor_sigkill_keeps_original_custody_and_restart_never_reconstructs_consent() {
    use std::process::Stdio;
    // This exact-filter private test owns one leaf. Reap its original agent and
    // helper after the leaf dies instead of leaving adopted zombie children.
    assert_eq!(unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1) }, 0);
    let control = tempfile::tempdir().unwrap();
    fs::set_permissions(control.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let cli =
        std::fs::canonicalize(std::env::var_os("HGS_TEST_BIN").expect("explicit production CLI"))
            .unwrap();
    let output = FileCapture::new();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "desktop::task_start_reconcile::tests::real_cli_crash_fixture_child",
            "--exact",
            "--ignored",
            "--test-threads=1",
        ])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .env("HGS_TEST_BIN", &cli)
        .env("TASK_START_CRASH_CHILD", "task-start-crash-v1")
        .env("TASK_START_CRASH_CONTROL", control.path())
        .stdin(Stdio::null())
        .stdout(output.file.try_clone().unwrap())
        .stderr(output.file.try_clone().unwrap())
        .spawn()
        .unwrap();
    let end = Instant::now() + Duration::from_secs(8);
    let ready = control.path().join("ready.json");
    while !ready.exists() {
        assert!(
            child.try_wait().unwrap().is_none(),
            "private crash leaf failed: {}",
            output.text()
        );
        assert!(
            Instant::now() < end,
            "private crash leaf did not become ready"
        );
        thread::sleep(Duration::from_millis(2));
    }
    let data: Value = serde_json::from_slice(&fs::read(&ready).unwrap()).unwrap();
    let root = PathBuf::from(data["root"].as_str().unwrap());
    let record: ledger::custody::Record = serde_json::from_value(data["record"].clone()).unwrap();
    let helper = record.helper.as_ref().unwrap();
    let identity = platform::process_identity(helper.pid).unwrap();
    assert_eq!(identity.parent, child.id());
    assert_eq!(identity.birth, helper.birth);
    let pinned = platform::PinnedProcess::pin(helper.pid, &helper.birth).unwrap();
    assert_eq!(
        platform::file_identity(std::path::Path::new(
            &platform::describe(helper.pid).unwrap().executable
        )),
        platform::file_identity(&cli)
    );
    let agent: Birth = serde_json::from_value(data["agent"].clone()).unwrap();
    let agent_identity = platform::process_identity(agent.pid).unwrap();
    assert_eq!(agent_identity.parent, child.id());
    assert_eq!(agent_identity.birth, agent.birth);
    let original_agent = platform::PinnedProcess::pin(agent.pid, &agent.birth).unwrap();
    child.kill().unwrap();
    child.wait().unwrap();
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
    let location = Location::new(&config).unwrap();
    let epoch = uuid::Uuid::new_v4().to_string();
    let mut store = Store::open(&location, &epoch).unwrap();
    assert_eq!(ledger::totals(&store.db).unwrap().undrained, 1);
    let ticket: Ticket = serde_json::from_value(data["ticket"].clone()).unwrap();
    let original: String = store
        .db
        .query_row(
            "SELECT outcome_json FROM desktop_operation_attempts WHERE attempt_id=?1",
            [&ticket.attempt_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&original).unwrap()["reason"],
        "service_restart"
    );
    let owner = Arc::new(location.lock().unwrap());
    let mut actor = Runtime::new(&config, &epoch, owner).unwrap();
    let end = Instant::now() + Duration::from_secs(8);
    while ledger::totals(&store.db).unwrap().undrained != 0 {
        assert!(Instant::now() < end, "original native CLI did not drain");
        store.retry_operation_custody();
        actor.poll(&mut store, None);
        thread::sleep(Duration::from_millis(2));
    }
    assert!(!pinned.alive());
    assert!(actor.work.is_none());
    for pid in [helper.pid, agent.pid] {
        let end = Instant::now() + Duration::from_secs(3);
        let mut status = 0;
        loop {
            let reaped = unsafe { libc::waitpid(pid as i32, &mut status, libc::WNOHANG) };
            if reaped == pid as i32 {
                break;
            }
            assert_eq!(reaped, 0, "could not reap the same original adopted child");
            assert!(Instant::now() < end);
            thread::sleep(Duration::from_millis(2));
        }
    }
    assert!(!original_agent.alive());
    assert_eq!(
        store
            .db
            .query_row::<String, _, _>(
                "SELECT outcome_json FROM desktop_operation_attempts WHERE attempt_id=?1",
                [&ticket.attempt_id],
                |r| r.get(0)
            )
            .unwrap(),
        original
    );
    assert_eq!(
        store
            .db
            .query_row::<i64, _, _>("SELECT count(*) FROM desktop_operation_attempts", [], |r| r
                .get(0))
            .unwrap(),
        1
    );
    assert!(!root.join("terminal-called").exists());
    drop(actor);
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[cfg(target_os = "linux")]
struct FileCapture {
    file: std::fs::File,
}
#[cfg(target_os = "linux")]
impl FileCapture {
    fn new() -> Self {
        Self {
            file: tempfile::tempfile().unwrap(),
        }
    }
    fn text(&self) -> String {
        use std::io::{Read, Seek};
        let mut file = self.file.try_clone().unwrap();
        file.rewind().unwrap();
        let mut text = String::new();
        file.take(16384).read_to_string(&mut text).unwrap();
        text
    }
}
