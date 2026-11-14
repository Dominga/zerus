//! The worker's thread keeps the service owner lock it was given until the
//! thread itself ends, also after its Worker handle and the original owner
//! are dropped, and no poll starts a second thread beside a running one.
//! Two actor runtimes never act on one ledger because of this.
use super::{entry_hold::Hold, *};
use crate::desktop::store::Location;

const MACHINE: &str = "00000000-0000-4000-8000-0000000000bb";

/// A private state folder of the mode the desktop requires, with Teams on.
fn fixture() -> (tempfile::TempDir, Config, Location) {
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
    std::fs::create_dir_all(&config.dir).unwrap();
    let location = Location::new(&config).unwrap();
    location.prepare().unwrap();
    platform::private_dir(&config.state.join("teams")).unwrap();
    std::fs::write(config.state.join("teams/preview"), "on").unwrap();
    assert!(crate::teams::enabled(&config.state));
    (temp, config, location)
}

/// Owns the paused run for the test's whole life. On every exit, an
/// assertion failure included, it stops and releases every held thread and
/// joins the worker's thread.
struct Paused {
    holds: Vec<Hold>,
    worker: Option<Worker>,
    thread: Option<JoinHandle<()>>,
    /// The owner given to the worker, when the guard keeps it: settling waits
    /// until every clone handed out is back, a detached thread's included.
    loans: Option<Arc<platform::FileLock>>,
}
impl Paused {
    fn worker(&mut self) -> &mut Worker {
        self.worker
            .as_mut()
            .expect("the worker handle is still held")
    }
    /// Drops the Worker handle; only the thread's join handle is kept.
    fn drop_worker(&mut self) {
        let mut worker = self.worker.take().expect("the worker handle is held");
        self.thread = worker.work.take().map(|work| work.thread);
        drop(worker);
    }
    fn join(&mut self) -> thread::Result<()> {
        if let Some(worker) = &mut self.worker {
            worker.request_stop();
        }
        for hold in &mut self.holds {
            hold.release();
        }
        let thread = self
            .thread
            .take()
            .or_else(|| Some(self.worker.as_mut()?.work.take()?.thread));
        thread.map_or(Ok(()), JoinHandle::join)
    }
    /// Joins, drops the worker, then waits (bounded) for the owner's loans.
    /// Returns whether every loan came back.
    fn settle(&mut self) -> thread::Result<bool> {
        let joined = self.join();
        self.worker = None;
        let until = Instant::now() + Duration::from_secs(10);
        while self
            .loans
            .as_ref()
            .is_some_and(|owner| Arc::strong_count(owner) > 1)
        {
            if Instant::now() >= until {
                return joined.map(|()| false);
            }
            thread::sleep(Duration::from_millis(1));
        }
        joined.map(|()| true)
    }
}
impl Drop for Paused {
    fn drop(&mut self) {
        let _ = self.settle();
    }
}

#[test]
fn the_worker_thread_keeps_the_service_owner_after_its_handle_until_it_ends() {
    let (_temp, config, location) = fixture();
    let owner = Arc::new(location.lock().unwrap());
    let mut run = Paused {
        holds: vec![Hold::install(&config.state)],
        worker: Some(Worker::new(&config, "owner-pin", owner.clone())),
        thread: None,
        loans: None,
    };
    run.worker().fixture_machine(MACHINE);
    run.worker().poll();
    assert!(
        run.worker().work.is_some(),
        "poll starts the worker's real thread"
    );
    assert!(
        run.holds[0].entered(Duration::from_secs(10)),
        "the thread reaches its run entry, before any launch or native effect"
    );
    run.worker().request_stop();
    assert!(!run.worker().drained(), "a stop request is not a drain");
    run.drop_worker();
    drop(owner);
    // A second service on the same state folder takes its own lock.
    let second = Location::new(&config).unwrap();
    let Err(refused) = second.lock() else {
        panic!("the paused thread still holds the service owner");
    };
    assert!(
        format!("{refused:#}").contains("another desktop service owns this state"),
        "{refused:#}"
    );
    run.join().expect("the released thread returns");
    second
        .lock()
        .expect("the owner is free once the thread has ended");
}

/// A poll that falls due while the worker's thread still runs (a changed
/// machine, or the next interval) keeps that thread's Work: no second thread
/// starts beside it under the same owner.
#[test]
fn a_due_poll_never_starts_a_second_thread_beside_a_running_one() {
    let (_temp, config, location) = fixture();
    let owner = Arc::new(location.lock().unwrap());
    let mut run = Paused {
        holds: vec![Hold::install(&config.state)],
        worker: Some(Worker::new(&config, "owner-overlap", owner.clone())),
        thread: None,
        loans: Some(owner),
    };
    run.worker().fixture_machine(MACHINE);
    run.worker().poll();
    assert!(
        run.holds[0].entered(Duration::from_secs(10)),
        "the first thread reaches its run entry"
    );
    let first = run
        .worker()
        .work
        .as_ref()
        .map(|work| work.thread.thread().id());
    assert!(first.is_some(), "poll starts the worker's real thread");
    // Any thread that starts now pauses at its run entry too.
    run.holds.push(Hold::install(&config.state));
    run.worker().next = Instant::now();
    run.worker().poll();
    assert_eq!(
        run.worker()
            .work
            .as_ref()
            .map(|work| work.thread.thread().id()),
        first,
        "the due poll keeps the running thread's Work"
    );
    assert!(
        !run.holds[1].entered(Duration::from_millis(500)),
        "no second thread reaches its run entry"
    );
    assert!(
        run.settle().expect("the released thread returns"),
        "every owner loan is back"
    );
}
