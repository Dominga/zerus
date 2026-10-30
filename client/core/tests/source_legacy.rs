//! Older-peer mode of the existing source supervisor, with private fixture
//! files and fake owned children. No installed service or production state.
mod common;
use common::{catalog, eventually, hello, FakeHgs};
use serde_json::json;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::PathBuf,
    process::{Child, Command},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Condvar, Mutex,
    },
    time::{Duration, Instant},
};
use zerus_client_core::{
    fsx,
    source::{self, Event, Legacy, Receiver, SpoolFactory},
    ComputerId, ComputerStatus, Timing,
};
use zerus_watch_protocol::Body;

fn spawn(command: &mut Command) -> io::Result<Child> {
    static LOCK: Mutex<()> = Mutex::new(());
    let _guard = LOCK.lock().unwrap();
    command.spawn()
}
fn timing() -> Timing {
    let mut timing = Timing::scaled(0.01);
    timing.reprobe = Duration::from_secs(30);
    timing.legacy.listing = Duration::from_secs(60);
    timing
}
struct Private {
    file: Option<File>,
    path: PathBuf,
    corrupt: bool,
}
impl Read for Private {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        let position = self.file.as_mut().unwrap().stream_position()?;
        let n = self.file.as_mut().unwrap().read(b)?;
        if self.corrupt && n > 0 && position > 4096 {
            b[0] ^= 1;
            self.corrupt = false;
        }
        Ok(n)
    }
}
impl Write for Private {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.file.as_mut().unwrap().write(b)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file.as_mut().unwrap().flush()
    }
}
impl Seek for Private {
    fn seek(&mut self, p: SeekFrom) -> io::Result<u64> {
        self.file.as_mut().unwrap().seek(p)
    }
}
impl Drop for Private {
    fn drop(&mut self) {
        self.file.take();
        fs::remove_file(&self.path).unwrap();
    }
}
fn factory(fake: &FakeHgs, corrupt: bool) -> SpoolFactory {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&fake.dir, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let dir = fake.dir.clone();
    let next = AtomicUsize::new(0);
    Arc::new(move || {
        let path = dir.join(format!(
            "private-spool-{}",
            next.fetch_add(1, Ordering::Relaxed)
        ));
        fsx::write_private_atomic(&path, b"", false)?;
        let file = OpenOptions::new().read(true).write(true).open(&path)?;
        Ok(Box::new(Private {
            file: Some(file),
            path,
            corrupt,
        }))
    })
}
fn receive(rx: &Receiver) -> source::Item {
    rx.recv_timeout(Duration::from_secs(20))
        .expect("source event")
}
fn drain(tx: &source::Sender) {
    eventually("source resources drained", || {
        let u = tx.usage();
        u.workers == 0 && u.readers == 0 && u.connecting == 0 && u.legacy_spools == 0
    });
}
fn write_listing(fake: &FakeHgs, host: &str, rows: usize) {
    fs::write(fake.dir.join(format!("ls-{host}.json")), json!({
        "sessions": (0..rows).map(|n|json!({"name":format!("codex/example/{n}"),
            "run_id":format!("r{n}"),"process_work":{"count":7,"known":true,"future":42}})).collect::<Vec<_>>(),
        "machine_id":"example-machine", "future_meta":{"kept":true}
    }).to_string()).unwrap();
}

#[test]
fn older_listing_uses_typed_bounded_pages_and_preserves_raw_facts() {
    let fake = FakeHgs::new("source-legacy-pages", json!({"studio":{"marker":false}}));
    write_listing(&fake, "studio", 100_000);
    let input_length = fs::metadata(fake.dir.join("ls-studio.json")).unwrap().len() as usize;
    let (tx, rx) = source::channel_with_legacy(spawn, factory(&fake, false));
    assert_eq!(tx.usage().legacy_spools, 0);
    let began = Instant::now();
    let handle = tx
        .spawn(&fake.path(), &ComputerId::from("studio"), timing())
        .unwrap();
    let identity = handle.identity().clone();
    let mut rows = 0;
    let mut page_index = 0;
    let mut observation = None;
    loop {
        let item = receive(&rx);
        match item.event() {
            Event::Legacy {
                source,
                generation,
                update,
                observed_at,
            } => {
                assert_eq!(*source, identity);
                assert_eq!(*generation, 1);
                assert!(*observed_at >= began && *observed_at <= Instant::now());
                assert_eq!(*observation.get_or_insert(*observed_at), *observed_at);
                match &**update {
                    Legacy::Begin {
                        metadata,
                        rows,
                        pages,
                        input_bytes,
                    } => {
                        assert_eq!(*rows, 100_000);
                        assert_eq!(*pages, 782);
                        assert_eq!(*input_bytes, input_length);
                        assert_eq!(metadata["machine_id"], "example-machine");
                        assert_eq!(metadata["future_meta"]["kept"], true);
                    }
                    Legacy::Page { index, items } => {
                        assert_eq!(*index, page_index);
                        assert!(items.len() <= 128);
                        for item in items {
                            assert_eq!(item.key, format!("run:r{rows}"));
                            assert_eq!(item.row["process_work"]["future"], 42);
                            rows += 1;
                        }
                        page_index += 1;
                    }
                    Legacy::Complete => break,
                }
            }
            Event::Frame { .. } => panic!("legacy listing must not manufacture watch frames"),
            _ => {}
        }
    }
    assert_eq!((rows, page_index), (100_000, 782));
    handle.stop();
    drain(&tx);
    assert_eq!(fake.runs("studio", &["ls", "--json"]).len(), 1);
    assert!(fake
        .log()
        .iter()
        .all(|e| e["args"] == json!(["-h"]) || e["args"] == json!(["ls", "--json"])));
    println!("legacy source executed rows={rows} pages={page_index} ingress_high_bytes={} ingress_high_items={}", tx.usage().high_bytes, tx.usage().high_items);
}

#[test]
fn a_failed_child_cannot_publish_even_when_its_json_is_valid() {
    let fake = FakeHgs::new(
        "source-legacy-exit",
        json!({"":{"marker":false,"listing_exit":7}}),
    );
    write_listing(&fake, "local", 1);
    let (tx, rx) = source::channel_with_legacy(spawn, factory(&fake, false));
    let handle = tx
        .spawn(&fake.path(), &ComputerId::local(), timing())
        .unwrap();
    loop {
        match receive(&rx).event() {
            Event::Legacy { .. } => panic!("failed child published a replacement"),
            Event::Status {
                status: ComputerStatus::Reconnecting { .. },
                ..
            } => break,
            _ => {}
        }
    }
    handle.stop();
    drain(&tx);
}

#[test]
fn a_late_spool_corruption_never_emits_completion() {
    let fake = FakeHgs::new("source-legacy-corrupt", json!({"":{"marker":false}}));
    write_listing(&fake, "local", 300);
    let (tx, rx) = source::channel_with_legacy(spawn, factory(&fake, true));
    let handle = tx
        .spawn(&fake.path(), &ComputerId::local(), timing())
        .unwrap();
    let mut began = false;
    let mut pages = 0;
    loop {
        match receive(&rx).event() {
            Event::Legacy { update, .. } => match &**update {
                Legacy::Begin { .. } => began = true,
                Legacy::Complete => panic!("corrupt spool committed"),
                Legacy::Page { .. } => pages += 1,
            },
            Event::Status {
                status: ComputerStatus::Reconnecting { .. },
                ..
            } => break,
            _ => {}
        }
    }
    assert!(began);
    assert_eq!(pages, 1, "a late fault must abandon already-emitted pages");
    handle.stop();
    drain(&tx);
}

#[test]
fn explicit_legacy_resnapshot_is_single_flight_with_a_generation_gap() {
    let fake = FakeHgs::new("source-legacy-resnapshot", json!({"":{"marker":false}}));
    write_listing(&fake, "local", 1);
    let (tx, rx) = source::channel_with_legacy(spawn, factory(&fake, false));
    let handle = tx
        .spawn(&fake.path(), &ComputerId::local(), timing())
        .unwrap();
    for generation in 1..=3 {
        loop {
            if matches!(receive(&rx).event(), Event::Legacy { generation:g, update,.. }
                if *g == generation && matches!(**update, Legacy::Complete))
            {
                break;
            }
        }
        if generation < 3 {
            assert!(!handle.resnapshot(generation + 1));
            assert!(handle.resnapshot(generation));
            assert!(!handle.resnapshot(generation));
        }
    }
    handle.stop();
    drain(&tx);
    let starts = fake.runs("", &["ls", "--json"]);
    assert_eq!(starts.len(), 3);
    for pair in starts.windows(2) {
        assert!(pair[1] - pair[0] >= 0.9);
    }
}

#[test]
fn reprobe_upgrades_the_same_alias_to_watch_without_parallel_listing() {
    let fake = FakeHgs::new("source-legacy-upgrade", json!({"":{"marker":false}}));
    write_listing(&fake, "local", 1);
    let (tx, rx) = source::channel_with_legacy(spawn, factory(&fake, false));
    let mut quick = timing();
    quick.reprobe = Duration::from_millis(500);
    let handle = tx.spawn(&fake.path(), &ComputerId::local(), quick).unwrap();
    loop {
        if matches!(receive(&rx).event(), Event::Legacy{update,..} if matches!(**update, Legacy::Complete))
        {
            break;
        }
    }
    let mut config: serde_json::Value =
        serde_json::from_slice(&fs::read(fake.dir.join("config.json")).unwrap()).unwrap();
    config["hosts"][""] = json!({"marker":true,"runs":[[{"lines":[hello("next"), catalog(&[("r","s")])]},{"serve":true}]]});
    fsx::write_private_atomic(
        &fake.dir.join("config.json"),
        config.to_string().as_bytes(),
        false,
    )
    .unwrap();
    loop {
        if let Event::Frame {
            generation, frame, ..
        } = receive(&rx).event()
        {
            if matches!(frame.body, Body::Hello(_)) {
                assert_eq!(*generation, 2);
                break;
            }
        }
    }
    handle.stop();
    drain(&tx);
    assert_eq!(fake.runs("", &["ls", "--json"]).len(), 1);
    assert_eq!(fake.runs("", &["watch", "--json"]).len(), 1);
}

#[test]
fn private_factory_failures_refuse_before_listing_spawn() {
    let fake = FakeHgs::new("source-legacy-storage-fail", json!({"":{"marker":false}}));
    let factory: SpoolFactory = Arc::new(|| Err(io::Error::other("synthetic private failure")));
    let (tx, rx) = source::channel_with_legacy(spawn, factory);
    let handle = tx
        .spawn(&fake.path(), &ComputerId::local(), timing())
        .unwrap();
    loop {
        match receive(&rx).event() {
            Event::Legacy { .. } => panic!("failed factory published"),
            Event::Status {
                status: ComputerStatus::Reconnecting { .. },
                ..
            } => break,
            _ => {}
        }
    }
    assert!(fake.runs("", &["ls", "--json"]).is_empty());
    handle.stop();
    drain(&tx);
}

#[test]
fn blocked_factories_keep_two_shared_spool_admissions_after_bounded_stop() {
    let fake = FakeHgs::new(
        "source-legacy-blocked",
        json!({"one":{"marker":false},"two":{"marker":false},"three":{"marker":false}}),
    );
    let entered = Arc::new(AtomicUsize::new(0));
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    let factory: SpoolFactory = {
        let (entered, release) = (entered.clone(), release.clone());
        Arc::new(move || {
            entered.fetch_add(1, Ordering::Relaxed);
            let (lock, wake) = &*release;
            let mut go = lock.lock().unwrap();
            while !*go {
                go = wake.wait(go).unwrap();
            }
            Err(io::Error::other("synthetic blocked factory refused"))
        })
    };
    let (tx, rx) = source::channel_with_legacy(spawn, factory);
    let handles: Vec<_> = ["one", "two", "three"]
        .into_iter()
        .map(|a| {
            tx.spawn(&fake.path(), &ComputerId::from(a), timing())
                .unwrap()
        })
        .collect();
    eventually("two factories admitted", || {
        entered.load(Ordering::Relaxed) == 2
    });
    assert_eq!(tx.usage().legacy_spools, 2);
    assert_eq!(tx.usage().legacy_reserved_bytes, 64 << 20);
    drop(rx);
    let began = Instant::now();
    for handle in handles {
        handle.stop();
    }
    assert!(began.elapsed() < Duration::from_secs(4));
    assert_eq!(entered.load(Ordering::Relaxed), 2);
    assert_eq!(tx.usage().workers, 2);
    assert_eq!(tx.usage().readers, 2);
    assert_eq!(tx.usage().legacy_spools, 2);
    *release.0.lock().unwrap() = true;
    release.1.notify_all();
    drain(&tx);
}

#[test]
fn receiver_drop_cancels_a_legacy_listing_child_and_releases_its_file() {
    let fake = FakeHgs::new(
        "source-legacy-cancel",
        json!({"":{"marker":false,"listing_wait_file":"never-created"}}),
    );
    let (tx, rx) = source::channel_with_legacy(spawn, factory(&fake, false));
    let handle = tx
        .spawn(&fake.path(), &ComputerId::local(), timing())
        .unwrap();
    eventually("owned listing started", || {
        !fake.runs("", &["ls", "--json"]).is_empty()
    });
    assert_eq!(tx.usage().legacy_spools, 1);
    let began = Instant::now();
    drop(rx);
    handle.stop();
    drain(&tx);
    assert!(began.elapsed() < Duration::from_secs(2));
    assert!(!fake.dir.join("private-spool-0").exists());
}

#[test]
fn periodic_listings_reuse_the_schedule_and_increment_replacement_generation() {
    let fake = FakeHgs::new("source-legacy-periodic", json!({"":{"marker":false}}));
    write_listing(&fake, "local", 1);
    let (tx, rx) = source::channel_with_legacy(spawn, factory(&fake, false));
    let mut quick = timing();
    quick.legacy.listing = Duration::from_millis(50);
    let handle = tx.spawn(&fake.path(), &ComputerId::local(), quick).unwrap();
    let mut completed = 0;
    while completed < 3 {
        if let Event::Legacy {
            generation, update, ..
        } = receive(&rx).event()
        {
            if matches!(**update, Legacy::Complete) {
                completed += 1;
                assert_eq!(*generation, completed);
            }
        }
    }
    handle.stop();
    drain(&tx);
    assert_eq!(fake.runs("", &["ls", "--json"]).len(), 3);
    assert!(fake
        .log()
        .iter()
        .all(|e| e["args"] == json!(["-h"]) || e["args"] == json!(["ls", "--json"])));
}

#[test]
fn legacy_replay_backpressure_is_shared_and_stop_remains_prompt() {
    let fake = FakeHgs::new("source-legacy-pressure", json!({"":{"marker":false}}));
    // Write incrementally so even fixture construction has one current row.
    let mut file = File::create(fake.dir.join("ls-local.json")).unwrap();
    file.write_all(b"{\"sessions\":[").unwrap();
    for n in 0..15_000 {
        if n > 0 {
            file.write_all(b",").unwrap();
        }
        serde_json::to_writer(
            &mut file,
            &json!({"name":format!("codex/example/{n}"),
            "run_id":format!("r{n}"), "padding":"x".repeat(1500)}),
        )
        .unwrap();
    }
    file.write_all(b"]}").unwrap();
    drop(file);
    let (tx, rx) = source::channel_with_legacy(spawn, factory(&fake, false));
    let handle = tx
        .spawn(&fake.path(), &ComputerId::local(), timing())
        .unwrap();
    eventually("legacy ingress pressure", || {
        tx.usage().bytes > source::INGRESS_BYTES - (512 << 10)
    });
    let usage = tx.usage();
    assert!(usage.bytes <= source::INGRESS_BYTES);
    assert!(usage.items <= source::INGRESS_ITEMS);
    assert_eq!(usage.legacy_spools, 1);
    assert_eq!(
        usage.connecting, 0,
        "finished child releases connection before replay"
    );
    let began = Instant::now();
    handle.stop();
    drain(&tx);
    assert!(began.elapsed() < Duration::from_secs(2));
    // The consumer still owns queued data permits after worker/file drain.
    assert!(tx.usage().bytes > 0);
    drop(rx);
    assert_eq!(tx.usage().bytes, 0);
    println!(
        "legacy backpressure high_bytes={} high_items={}",
        usage.high_bytes, usage.high_items
    );
}

#[test]
fn an_inherited_pipe_retains_worker_reader_and_spool_after_stop() {
    fn inherited(command: &mut Command) -> io::Result<Child> {
        if command.get_args().any(|a| a == "ls") {
            let ready = std::path::Path::new(command.get_program())
                .parent()
                .unwrap()
                .join("pipe-ready");
            let mut child = Command::new("python3");
            child.args(["-c", "import subprocess,sys,time;subprocess.Popen([sys.executable,'-c','import time;time.sleep(3)']);sys.stdout.write('{\"sessions\":[]}');sys.stdout.flush();open(sys.argv[1],'w').close();time.sleep(10)"])
                .arg(ready)
                .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
            spawn(&mut child)
        } else {
            spawn(command)
        }
    }
    let fake = FakeHgs::new("source-legacy-pipe", json!({"":{"marker":false}}));
    let (tx, rx) = source::channel_with_legacy(inherited, factory(&fake, false));
    let handle = tx
        .spawn(&fake.path(), &ComputerId::local(), timing())
        .unwrap();
    eventually("legacy private file", || {
        fake.dir.join("private-spool-0").exists()
    });
    // The descendant exits by itself; no process group is stopped.
    eventually("synthetic pipe inherited", || {
        fake.dir.join("pipe-ready").exists()
    });
    drop(rx);
    handle.stop();
    assert_eq!(tx.usage().workers, 1);
    assert_eq!(tx.usage().readers, 1);
    assert_eq!(tx.usage().legacy_spools, 1);
    drain(&tx);
    assert!(!fake.dir.join("private-spool-0").exists());
}
