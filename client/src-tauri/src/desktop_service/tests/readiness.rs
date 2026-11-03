//! Consume the GUI's real wakeups rather than giving every fixture phase a
//! polling window. Fake CLI milestones with no published frame poke that same
//! pump through a private socket; the socket carries no service protocol data.
use super::*;
use std::os::unix::net::UnixDatagram;
use zerus_client_core::Batch;

pub(super) struct Readiness {
    pump: crate::pump::Pump,
    until: Instant,
    headers: bool,
    group: String,
    pause: Option<Duration>,
    completed: Vec<(&'static str, Duration)>,
    _signals: Signals,
}

impl Readiness {
    pub(super) fn new(fixture: &Fixture, pump: crate::pump::Pump) -> Self {
        let socket = fixture.0.join("ready.sock");
        let listener = UnixDatagram::bind(&socket).unwrap();
        // This timeout only joins the owned listener on unwind; it never
        // samples a readiness condition or sets a phase's success deadline.
        listener
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let changed = pump.clone();
        let worker = std::thread::spawn(move || {
            let mut byte = [0u8; 1];
            while !done.load(Ordering::Acquire) {
                match listener.recv(&mut byte) {
                    Ok(_) => changed.poke(),
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::WouldBlock
                                | std::io::ErrorKind::TimedOut
                                | std::io::ErrorKind::Interrupted
                        ) => {}
                    Err(error) => return Err(error),
                }
            }
            Ok(())
        });
        Self {
            pump,
            // One deadlock watchdog for the whole scenario, rather than one
            // five-second scheduling allowance for each asynchronous phase.
            until: Instant::now() + Duration::from_secs(30),
            headers: false,
            group: String::new(),
            pause: Some(injected_delay("ZERUS_TEST_SERVICE_POLL_DELAY_MS")),
            completed: Vec::new(),
            _signals: Signals {
                stop,
                socket,
                worker: Some(worker),
            },
        }
    }

    /// Last grouping actually published to this one-row fixture's GUI reader.
    pub(super) fn grouped(&self) -> bool {
        self.headers && self.group == "alpha" && self.pump.row("", "row").is_some()
    }

    pub(super) fn checkpoint_grouped(&mut self) -> bool {
        self.pump.resend();
        let publication = self.pump.take(Duration::ZERO);
        self.observe(publication);
        let grouped = self.grouped();
        if !grouped {
            eprintln!(
                "fresh project checkpoint: headers={}, group={:?}, row={}",
                self.headers,
                self.group,
                self.pump.row("", "row").is_some()
            );
        }
        grouped
    }

    pub(super) async fn until(&mut self, label: &'static str, ready: impl Fn(&Self) -> bool) {
        let began = Instant::now();
        loop {
            // External service state can change before this reader consumes
            // its queued publication. Never accept a stale cached grouping.
            let publication = self.pump.take(Duration::ZERO);
            self.observe(publication);
            if ready(self) {
                self.completed.push((label, began.elapsed()));
                return;
            }
            if let Some(pause) = self.pause.take().filter(|pause| !pause.is_zero()) {
                // Fault control: only this observer is descheduled. Producers
                // keep running, and their coalesced publication stays queued.
                std::thread::sleep(pause);
                continue;
            }
            let remaining = self.until.saturating_duration_since(Instant::now());
            assert!(
                !remaining.is_zero(),
                "{label}; completed phases: {:?}",
                self.completed
            );
            let pump = self.pump.clone();
            let batch = tauri::async_runtime::spawn_blocking(move || pump.take(remaining))
                .await
                .unwrap();
            self.observe(batch);
        }
    }

    fn observe(&mut self, batch: Batch) {
        if let Some(grouping) = batch.grouping {
            self.headers =
                grouping.loaded && grouping.headers.iter().any(|header| header.id == "alpha");
        }
        for group in batch.groups {
            if group.computer.is_local() && group.key == "row" {
                self.group = group.group;
            }
        }
    }

    pub(super) fn report(&self) {
        eprintln!("reconnect readiness phases: {:?}", self.completed);
    }
}

struct Signals {
    stop: Arc<AtomicBool>,
    socket: PathBuf,
    worker: Option<std::thread::JoinHandle<std::io::Result<()>>>,
}
impl Drop for Signals {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Ok(wake) = UnixDatagram::unbound() {
            let _ = wake.set_nonblocking(true);
            let _ = wake.send_to(&[0], &self.socket);
        }
        let result = self.worker.take().unwrap().join();
        if !std::thread::panicking() {
            assert!(
                matches!(result, Ok(Ok(()))),
                "fixture readiness listener failed: {result:?}"
            );
        }
    }
}
