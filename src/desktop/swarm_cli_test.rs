//! Lookup faults exercise the real CLI dispatcher, including its preflight.
use super::*;
use std::{
    path::PathBuf,
    sync::{Mutex, PoisonError},
};

pub(super) static SERIAL: Mutex<()> = Mutex::new(());
static SELECTED: Mutex<Option<(PathBuf, Arc<Lookup>)>> = Mutex::new(None);
struct Lookup {
    calls: AtomicUsize,
    finished: AtomicBool,
}
fn selected(config: &Config) -> Option<Arc<Lookup>> {
    SELECTED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .filter(|(dir, _)| dir == &config.dir)
        .map(|(_, lookup)| lookup.clone())
}
pub(super) fn budget(config: &Config) -> Option<Duration> {
    selected(config).map(|_| Duration::from_millis(250))
}
/// A scoped test shadow leaves the production Location implementation intact.
pub(super) struct Location;
impl Location {
    #[allow(clippy::new_ret_no_self)] // Match the production call without changing its result type.
    pub(super) fn new(config: &Config) -> Result<super::store::Location> {
        let lookup = selected(config);
        if let Some(lookup) = &lookup {
            lookup.calls.fetch_add(1, Ordering::AcqRel);
            thread::sleep(Duration::from_millis(1500));
        }
        let result = super::store::Location::new(config);
        if let Some(lookup) = lookup {
            lookup.finished.store(true, Ordering::Release);
        }
        result
    }
}
struct Selection;
impl Drop for Selection {
    fn drop(&mut self) {
        let until = Instant::now() + Duration::from_secs(4);
        while SWARM_RESTART_ACQUIRING.load(Ordering::Acquire) && Instant::now() < until {
            thread::sleep(Duration::from_millis(10));
        }
        SELECTED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
    }
}
#[test]
fn cli_restart_bounds_its_first_lookup_and_validates_before_any_lookup() {
    let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let root = tempfile::tempdir().unwrap();
    let config = Config {
        home: root.path().to_str().unwrap().into(),
        dir: root.path().join("config"),
        state: root.path().join("state"),
        selves: vec![],
        peers: vec![],
        base_peers: vec![],
        peers_from_environment: false,
        machines: Default::default(),
        tab: false,
        tab_colors: String::new(),
    };
    let lookup = Arc::new(Lookup {
        calls: AtomicUsize::new(0),
        finished: AtomicBool::new(false),
    });
    *SELECTED.lock().unwrap_or_else(PoisonError::into_inner) =
        Some((config.dir.clone(), lookup.clone()));
    let _selection = Selection;
    let args = ["swarm-restart", "--json"].map(str::to_owned);
    assert!(dispatch(&config, "", &args, true).is_err());
    assert!(dispatch(&config, "other.example", &args, false).is_err());
    assert_eq!(lookup.calls.load(Ordering::Acquire), 0);
    assert!(!SWARM_RESTART_ACQUIRING.load(Ordering::Acquire));

    let started = Instant::now();
    let result = dispatch(&config, "", &args, false);
    assert!(result
        .unwrap_err()
        .message
        .contains("outcome is unconfirmed"));
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "CLI location preflight escaped the watchdog"
    );
    let entered_by = Instant::now() + Duration::from_secs(1);
    while lookup.calls.load(Ordering::Acquire) == 0 && Instant::now() < entered_by {
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(lookup.calls.load(Ordering::Acquire), 1);
    assert!(!lookup.finished.load(Ordering::Acquire));
    assert!(SWARM_RESTART_ACQUIRING.load(Ordering::Acquire));
}
