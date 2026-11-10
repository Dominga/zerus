use super::*;

const CHILD: &str = "ZERUS_TEST_QUIT_DEADLINE_ONE_WORKER";

/// zerus-5wmh.66.3.31.11: with its only runtime worker occupied while a view
/// callback is stalled inside catalog admission, no runtime timer can fire.
/// The quit deadline must still finish on time.
#[test]
fn a_starved_runtime_cannot_hold_the_quit_completion_past_its_deadline() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let log_root = Fixture::new(json!({}));
    let path = log_root.0.join("quit-child.log");
    let log = fs::File::create(&path).unwrap();
    // The helper has a DIFFERENT exact name and cannot recursively launch this
    // test. A separate process owns Tauri's once-only global runtime handle.
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "desktop_service::tests::quit_deadline::one_worker_child",
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
    let (release, resume) = std::sync::mpsc::channel();
    let release = ReleaseView(release);
    let first = AtomicBool::new(true);
    let drain = pump.drain_service(rx, move |change| {
        // The fixture's catalog row: CatalogDelivery::apply holds the
        // snapshot and dirty admission locks around this callback.
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
    // Occupy the only runtime worker outright. An admission reader no longer
    // does: since zerus-bck0 it waits inside block_in_place.
    let (unblock, held) = std::sync::mpsc::channel::<()>();
    let _starved = tauri::async_runtime::spawn(async move {
        let _ = held.recv();
    });
    let ran = Arc::new(AtomicBool::new(false));
    let probe = ran.clone();
    let _probe = tauri::async_runtime::spawn(async move { probe.store(true, Ordering::Release) });
    std::thread::sleep(Duration::from_millis(200));
    assert!(
        !ran.load(Ordering::Acquire),
        "the only runtime worker is occupied"
    );
    let (finished, done) = std::sync::mpsc::channel();
    let started = Instant::now();
    crate::exit_flush::finish_service_exit_with_deadline(
        service.clone(),
        move || {
            let _ = finished.send(());
        },
        Duration::from_millis(40),
    );
    // Task #60: without its 40 ms deadline the quit waits for the occupied
    // worker, which never frees; the floor tells the two apart under load.
    done.recv_timeout(crate::test_wait::FLOOR)
        .expect("quit completion at its deadline");
    assert!(started.elapsed() < crate::test_wait::FLOOR);
    assert!(
        !ran.load(Ordering::Acquire),
        "completed without any runtime worker"
    );
    drop(unblock);
    drop(release);
    runtime.block_on(async {
        eventually("starved worker resumed", || ran.load(Ordering::Acquire)).await;
        service.join().await.unwrap();
    });
}
