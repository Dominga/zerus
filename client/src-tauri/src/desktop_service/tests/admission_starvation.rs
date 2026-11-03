use super::*;

const CHILD: &str = "ZERUS_TEST_ADMISSION_ONE_WORKER";

/// zerus-bck0: a view callback runs under the catalog admission locks. An
/// async reader of those locks must not occupy the runtime's only worker:
/// a runtime timer and a command still complete while the callback waits.
#[test]
fn a_blocked_view_callback_does_not_starve_timers_or_commands() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let log_root = Fixture::new(json!({}));
    let path = log_root.0.join("admission-child.log");
    let log = fs::File::create(&path).unwrap();
    // The helper has a DIFFERENT exact name and cannot recursively launch this
    // test. A separate process owns Tauri's once-only global runtime handle.
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "desktop_service::tests::admission_starvation::one_worker_child",
            "--ignored",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .stdin(Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap();
    let until = Instant::now() + Duration::from_secs(120);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status);
        }
        if Instant::now() >= until {
            child.kill().unwrap(); // Only this original private Rust test child.
            child.wait().unwrap();
            break None;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(fs::metadata(&path).unwrap().len() <= 64 << 10);
    let output = fs::read_to_string(path).unwrap();
    assert!(
        status.is_some_and(|status| status.success()) && output.contains("1 passed"),
        "{status:?}\n{output}"
    );
}

#[test]
#[ignore = "owned subprocess with a one-worker Tauri runtime; run by the parent test"]
fn one_worker_child() {
    assert_eq!(std::env::var(CHILD).as_deref(), Ok("1"));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    tauri::async_runtime::set(runtime.handle().clone());
    let fixture = Fixture::new(json!({}));
    let pump = crate::pump::Pump::default();
    let (sink, rx) = std::sync::mpsc::sync_channel(256);
    let (entered, observed) = std::sync::mpsc::channel();
    let (release, resume) = std::sync::mpsc::channel::<()>();
    let first = AtomicBool::new(true);
    let drain = pump.drain_service(rx, move |change| {
        // CatalogDelivery::apply holds the snapshot and dirty admission locks
        // around this callback; hold it until the test releases it.
        if matches!(change, Change::Upsert { key, .. } if key == "row")
            && first.swap(false, Ordering::AcqRel)
        {
            let _ = entered.send(());
            let _ = resume.recv();
        }
    });
    let service = Service::start(
        fixture.0.join("hgs"),
        pump.clone(),
        sink,
        true,
        drain,
        None,
        vec![],
    );
    // No runtime timer below may decide the outcome: only std waits.
    observed
        .recv_timeout(Duration::from_secs(60))
        .expect("catalog row callback");
    // An async reader of the admission state waits behind the callback.
    let (read, reader) = std::sync::mpsc::channel();
    let blocked = service.clone();
    let _reader = tauri::async_runtime::spawn(async move {
        let connected = blocked.snapshot().connected;
        let _ = read.send(connected);
    });
    std::thread::sleep(Duration::from_millis(100));
    // A runtime timer and a command (a pump read, as commands do) still run.
    let (fired, timer) = std::sync::mpsc::channel();
    let _timer = tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        let _ = fired.send(());
    });
    let (answered, command) = std::sync::mpsc::channel();
    let rows = pump.clone();
    let _command = tauri::async_runtime::spawn(async move {
        let _ = answered.send(rows.row("", "row").is_none());
    });
    timer
        .recv_timeout(Duration::from_secs(5))
        .expect("a runtime timer fires while the view callback is blocked");
    assert!(
        command
            .recv_timeout(Duration::from_secs(5))
            .expect("a command completes while the view callback is blocked"),
        "the held row is not pushed yet"
    );
    assert!(
        reader.try_recv().is_err(),
        "the admission reader waits for the callback"
    );
    drop(release);
    assert!(reader
        .recv_timeout(Duration::from_secs(10))
        .expect("the admission reader completes after the callback"));
    runtime.block_on(async {
        eventually("the held row arrives", || pump.row("", "row").is_some()).await;
        service.join().await.unwrap();
    });
}

/// block_in_place panics on a current-thread runtime (#[tokio::test]'s
/// default); the admission helper must take the plain wait there, outside any
/// runtime and on a blocking-pool thread. (A LocalSet is not supported: see
/// `no_production_code_runs_a_local_set`.)
#[test]
fn admission_waits_without_block_in_place_outside_the_worker_pool() {
    let lock = Arc::new(Mutex::new(0u32));
    // Hold the lock briefly on another thread, so every call below contends.
    let contend = |lock: &Arc<Mutex<u32>>| {
        let (held, taken) = std::sync::mpsc::channel();
        let holder = lock.clone();
        let thread = std::thread::spawn(move || {
            let _guard = holder.lock().unwrap();
            held.send(()).unwrap();
            std::thread::sleep(Duration::from_millis(50));
        });
        taken.recv().unwrap();
        thread
    };
    let current = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let thread = contend(&lock);
    current.block_on(async { *admission(&lock) += 1 });
    thread.join().unwrap();
    let thread = contend(&lock);
    *admission(&lock) += 1;
    thread.join().unwrap();
    let pool = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let thread = contend(&lock);
    let blocking = lock.clone();
    // Spawned inside the runtime: a blocking-pool thread, then a pool worker.
    pool.block_on(
        async move { tokio::task::spawn_blocking(move || *admission(&blocking) += 1).await },
    )
    .unwrap();
    thread.join().unwrap();
    let thread = contend(&lock);
    let worker = lock.clone();
    pool.block_on(async move { tokio::spawn(async move { *admission(&worker) += 1 }).await })
        .unwrap();
    thread.join().unwrap();
    assert_eq!(*lock.lock().unwrap(), 4);
}

/// zerus-uq02: a LocalSet on the multi-thread runtime reports MultiThread, so
/// a contended admission() inside one would enter block_in_place and panic.
/// The helper cannot detect that context; no production code may create one.
#[test]
fn no_production_code_runs_a_local_set() {
    fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if path.is_dir() {
                if name != "tests" {
                    sources(&path, out);
                }
            } else if name.ends_with(".rs") && name != "tests.rs" && !name.ends_with("_tests.rs") {
                out.push(path);
            }
        }
    }
    let mut files = Vec::new();
    sources(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    assert!(files.len() > 50, "the source walk found the crate");
    let offenders: Vec<_> = files
        .iter()
        .filter(|path| {
            let text = fs::read_to_string(path).unwrap();
            ["LocalSet::", "task::LocalSet", "spawn_local("]
                .iter()
                .any(|use_| text.contains(use_))
        })
        .collect();
    assert!(offenders.is_empty(), "{offenders:?}");
}
