use super::*;
use std::{collections::BTreeMap, fs, io::Write, path::Path};

struct OwnedChild(Option<platform::Spawned>);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let Some(mut child) = self.0.take() else {
            return;
        };
        child.terminate();
        if unsafe { WaitForSingleObject(child.as_raw_handle(), 2000) } == WAIT_OBJECT_0
            && child.wait_tree(Duration::ZERO).unwrap_or(false)
        {
            let _ = child.wait();
            return;
        }
        eprintln!("foreign-client fixture cleanup unknown; retaining original child/job");
        thread::spawn(move || loop {
            if unsafe { WaitForSingleObject(child.as_raw_handle(), 20) } == WAIT_OBJECT_0
                && child.wait_tree(Duration::ZERO).unwrap_or(false)
            {
                let _ = child.wait();
                break;
            }
            thread::sleep(Duration::from_millis(20));
        });
    }
}
fn accept(listener: &Arc<crate::host::pipe::PipeListener>) -> crate::host::pipe::PipeStream {
    let listening = listener.clone();
    let (send, receive) = std::sync::mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        let _ = send.send(listening.accept());
    });
    let result = receive.recv_timeout(Duration::from_secs(3));
    listener.close();
    worker.join().unwrap();
    result.expect("bounded fixture accept").unwrap()
}
#[test]
fn restart_authenticates_the_actual_client_not_its_own_server_image() {
    use crate::host::{pipe, token};
    use zerus_client_core::desktop::windows::{verify_own_client_until, Server};
    let identity = token::Identity::current().unwrap();
    let name = format!(r"\\.\pipe\hgs-swarm-proof-{}", uuid::Uuid::new_v4());
    let listener = Arc::new(pipe::PipeListener::bind(&name, &identity).unwrap());
    let own_name = name.clone();
    let (done, wait) = std::sync::mpsc::sync_channel(1);
    let client = thread::spawn(move || {
        let mut stream = pipe::connect(&own_name, Instant::now() + Duration::from_secs(3)).unwrap();
        stream.write_all(b"hello").unwrap();
        let _ = wait.recv_timeout(Duration::from_secs(5));
    });
    let stream = accept(&listener);
    let retained = match verify_own_client_until(
        &stream.verification_handle().unwrap(),
        Instant::now() + Duration::from_secs(2),
    ) {
        Server::Verified(peer) => peer,
        _ => panic!("same-image current client was not admitted"),
    };
    assert!(retained.current());
    done.send(()).unwrap();
    client.join().unwrap();
    drop(stream);
    drop(retained);

    let name = format!(r"\\.\pipe\hgs-swarm-proof-{}", uuid::Uuid::new_v4());
    let listener = Arc::new(pipe::PipeListener::bind(&name, &identity).unwrap());
    let mut command = Command::new(
        std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32/cmd.exe"),
    );
    command
        .args(["/D", "/C", &format!("echo hello>{name}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let _child = OwnedChild(Some(platform::spawn_hidden(&mut command).unwrap()));
    let stream = accept(&listener);
    assert!(
        !matches!(
            verify_own_client_until(
                &stream.verification_handle().unwrap(),
                Instant::now() + Duration::from_secs(2)
            ),
            Server::Verified(_)
        ),
        "a foreign client must not borrow this server's matching image"
    );
}

fn until(mut check: impl FnMut() -> bool, seconds: u64) {
    let end = Instant::now() + Duration::from_secs(seconds);
    while !check() {
        assert!(
            Instant::now() < end,
            "owned worker did not settle within its bound"
        );
        thread::sleep(Duration::from_millis(10));
    }
}
fn config(root: &Path) -> Config {
    Config {
        home: root.to_str().unwrap().into(),
        dir: root.join("config"),
        state: root.join("state"),
        selves: vec![],
        peers: vec![],
        base_peers: vec![],
        peers_from_environment: false,
        machines: BTreeMap::new(),
        tab: false,
        tab_colors: String::new(),
    }
}
#[test]
fn backoff_is_bounded_and_a_healthy_worker_resets_it() {
    assert_eq!(delay(0, Duration::ZERO), 1000);
    assert_eq!(delay(1000, Duration::ZERO), 2000);
    assert_eq!(delay(300_000, Duration::ZERO), 300_000);
    assert_eq!(delay(u32::MAX, Duration::ZERO), 300_000);
    assert_eq!(delay(300_000, Duration::from_secs(60)), 1000);
}
#[test]
fn delayed_restart_acquisition_is_bounded_and_keeps_its_original_admission() {
    let _serial = crate::desktop::swarm_cli_test::SERIAL
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path());
    let released = Arc::new(AtomicBool::new(false));
    struct Release(Arc<AtomicBool>);
    impl Drop for Release {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    let resource = Release(released.clone());
    let started = Instant::now();
    assert!(crate::desktop::bounded_swarm_restart(
        &config,
        Instant::now() + Duration::from_millis(250),
        move |_, _| {
            thread::sleep(Duration::from_millis(1500));
            drop(resource);
            Ok(json!({"accepted":true}))
        }
    )
    .is_err());
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(!released.load(Ordering::Acquire));
    let replayed = Arc::new(AtomicBool::new(false));
    let called = replayed.clone();
    assert!(crate::desktop::bounded_swarm_restart(
        &config,
        Instant::now() + Duration::from_millis(250),
        move |_, _| {
            called.store(true, Ordering::Release);
            Ok(json!({"accepted":true}))
        }
    )
    .is_err());
    assert!(!replayed.load(Ordering::Acquire));
    until(|| released.load(Ordering::Acquire), 3);
    // Completion can follow the captured resource's drop by one scheduling turn.
    until(
        || !crate::desktop::SWARM_RESTART_ACQUIRING.load(Ordering::Acquire),
        2,
    );
}
#[test]
#[ignore = "exact owned child selected only by its native supervisor parent"]
fn owned_worker_child() {
    let marker = std::env::var_os("HGS_SWARM_TEST_CHILD").expect("owned fixture marker");
    let marker = Path::new(&marker);
    assert!(marker.is_absolute());
    assert_eq!(marker.file_name().unwrap(), "started");
    assert!(std::env::var_os("HGS_SESSION").is_none());
    assert!(std::env::var_os("HGS_RUN_ID").is_none());
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(marker)
        .unwrap();
    writeln!(file, "{}", std::process::id()).unwrap();
    file.sync_all().unwrap();
    loop {
        thread::sleep(Duration::from_secs(1));
    }
}
#[test]
fn restart_and_unknown_cleanup_retain_only_the_original_worker_and_service_owner() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path());
    fs::create_dir_all(config.dir.join("swarm")).unwrap();
    // This diagnostic marker must never become termination authority.
    fs::write(
        config.dir.join("swarm/worker.json"),
        json!({"pid":std::process::id(),"version":"0.0.0"}).to_string(),
    )
    .unwrap();
    let file = fs::File::create(root.path().join("owner.lock")).unwrap();
    fs2::FileExt::try_lock_exclusive(&file).unwrap();
    let owner = Arc::new(platform::FileLock::held(file));
    let mut runtime = Runtime::new(&config, owner.clone());
    let marker = root.path().join("started");
    runtime.start(Launch::Fixture(marker.clone())).unwrap();
    until(|| runtime.status()["starts"] == 1 && marker.exists(), 10);
    let first = runtime.status()["pid"].as_u64().unwrap() as u32;
    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, first) };
    assert!(!handle.is_null());
    let original = unsafe { OwnedHandle::from_raw_handle(handle) };
    assert_eq!(
        unsafe { WaitForSingleObject(original.as_raw_handle(), 0) },
        WAIT_TIMEOUT
    );
    assert_eq!(runtime.restart()["accepted"], true);
    until(
        || {
            runtime.status()["starts"] == 2
                && fs::read_to_string(&marker)
                    .unwrap_or_default()
                    .lines()
                    .count()
                    == 2
        },
        10,
    );
    assert_eq!(
        unsafe { WaitForSingleObject(original.as_raw_handle(), 0) },
        WAIT_OBJECT_0
    );
    assert_ne!(
        runtime.status()["pid"].as_u64().unwrap() as u32,
        std::process::id()
    );

    // Withhold the exit observation: even an already-signalled child cannot
    // refund custody, admit another worker or complete the actor's drain.
    runtime.shared.hold_cleanup.store(true, Ordering::Release);
    runtime.request_stop();
    until(|| runtime.status()["cleanup_unknown"] == true, 8);
    assert!(!runtime.drained());
    assert!(!runtime.confirmed_drained());
    assert_eq!(runtime.restart()["accepted"], false);
    assert_eq!(runtime.status()["starts"], 2);
    assert!(Arc::strong_count(&owner) >= 3);
    runtime.shared.hold_cleanup.store(false, Ordering::Release);
    until(|| runtime.drained(), 3);
    assert!(runtime.confirmed_drained());
    assert_eq!(runtime.status()["cleanup_unknown"], false);
    assert_eq!(runtime.status()["pid"], Value::Null);
    assert_eq!(Arc::strong_count(&owner), 2);
    drop(runtime);
    assert_eq!(Arc::strong_count(&owner), 1);
}
