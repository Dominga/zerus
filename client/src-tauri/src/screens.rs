//! The reads behind the Overview, Machines and Accounts screens, and the
//! Projects screen: one hub that knows which screens are visible, runs the
//! due `hgs` reads off the main thread (`zerus_client_core::reads`, bounded
//! and timed out there), keeps the replies and hands the webview projected
//! view models (`zerus_client_core::views`). Which read runs when is decided
//! in `views::schedule`; this file wires the clock, the threads and the event.
//!
//! Every command answers at once with the current view and starts what is
//! due in the background. A screen's reads run only while it shows, or on an
//! explicit Refresh, selection or "Test connection"; hidden, only Qt's
//! account warm-up runs (one `account ls` per newly listed host). Projects
//! reads nothing: [`Screens::organization`] hands it the organization the
//! Sessions list groups by, with the write controller's status line. A
//! changed view emits `screens-changed` with the screen's name; the frontend
//! then pulls it again. A replay reads only with an explicit
//! `ZERUS_CLIENT_HGS`, as the actions do.
//!
//! The one-second ticker does no per-profile work: it compares the fleet and
//! steps the account reads only when the fleet changed, a catalog arrived, an
//! account screen showed or the earliest due time passed. A view is
//! projected again only after a read finished, the fleet changed, or account
//! reads started while Overview or Accounts shows (their busy state).

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;
use tauri::State;
use zerus_client_core::reads::{self, Read, ReadError};
use zerus_client_core::views::accounts::{
    self, AccountsDisplay, AccountsState, AccountsView, UsageRef,
};
use zerus_client_core::views::machines::{MachinesState, MachinesView};
use zerus_client_core::views::projects::{ProjectsView, SwarmState};
use zerus_client_core::views::schedule::{Screen, USAGE_INTERVAL};
use zerus_client_core::views::FleetEntry;
use zerus_client_core::ComputerId;

/// What a replay without an explicit `hgs` shows instead of reading.
pub const REPLAY_NEEDS_HGS: &str = "Reads in replay mode need ZERUS_CLIENT_HGS.";

/// The screens that show the accounts view.
const ACCOUNT_SCREENS: [Screen; 2] = [Screen::Overview, Screen::Accounts];
/// The screens whose view follows the fleet.
const FLEET_SCREENS: [Screen; 3] = [Screen::Overview, Screen::Projects, Screen::Accounts];

/// One read, run off the UI thread.
pub type Job = Box<dyn FnOnce() + Send>;
pub type Spawn = Arc<dyn Fn(Job) + Send + Sync>;
pub type Notify = Arc<dyn Fn(Screen) + Send + Sync>;
pub type FleetSource = Arc<dyn Fn() -> Vec<FleetEntry> + Send + Sync>;
/// Milliseconds since the hub started.
pub type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;

#[derive(Default)]
struct Hub {
    visible: BTreeSet<Screen>,
    fleet: Vec<FleetEntry>,
    accounts: AccountsState,
    display: AccountsDisplay,
    machines: MachinesState,
    swarm: SwarmState,
    /// The view each visible screen last showed, so a change emits once.
    shown: BTreeMap<Screen, Value>,
    /// Screens a reply, the fleet or starting account reads touched since the
    /// last publish.
    dirty: BTreeSet<Screen>,
    /// The account reads need a fresh look: the fleet changed, a catalog
    /// arrived or an account screen showed.
    stale: bool,
    /// When the next catalog or inspect falls due, while an account screen shows.
    next_due: Option<u64>,
    #[cfg(test)]
    work: Work,
}

/// What the hub did so far: account steps and projections for an event.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Work {
    steps: usize,
    projections: usize,
}

impl Hub {
    fn accounts_shown(&self) -> bool {
        ACCOUNT_SCREENS
            .iter()
            .any(|screen| self.visible.contains(screen))
    }

    /// Takes the fleet as the pump has it now. A change steps the account
    /// reads again and re-projects the screens that show the fleet.
    fn follow(&mut self, fleet: Vec<FleetEntry>) {
        if self.fleet != fleet {
            self.fleet = fleet;
            self.stale = true;
            self.dirty.extend(FLEET_SCREENS);
        }
    }

    /// The queued inspects that start now: at most two in flight, each on a
    /// host that is still reachable.
    fn dispatch(&mut self) -> Vec<Run> {
        let Self {
            fleet,
            accounts: state,
            ..
        } = self;
        let reachable = |host: &ComputerId| {
            fleet
                .iter()
                .any(|entry| &entry.computer == host && accounts::reachable(entry))
        };
        state
            .dispatch(&reachable)
            .into_iter()
            .map(Run::Usage)
            .collect()
    }
}

struct Shared {
    hub: Mutex<Hub>,
    hgs: PathBuf,
    allowed: bool,
    spawn: Spawn,
    notify: Notify,
    fleet: FleetSource,
    clock: Clock,
}

/// The managed state of the screen commands; clones share it.
#[derive(Clone)]
pub struct Screens(Arc<Shared>);

/// A read the hub decided to start.
enum Run {
    Catalog(ComputerId),
    Usage(UsageRef),
    Machines,
    Resolve(String),
    Check(String),
}

/// Every machine that keeps accounts. The hub always passes this full list:
/// the account state forgets every host it is not given.
fn account_hosts(fleet: &[FleetEntry]) -> Vec<ComputerId> {
    accounts::hosts(fleet)
        .into_iter()
        .map(|host| host.computer)
        .collect()
}

fn message(reply: Result<Value, ReadError>) -> Result<Value, String> {
    reply.map_err(|error| error.to_string())
}

/// Records a view the webview now holds for the visible screens that show it.
fn remember<T: Serialize>(hub: &mut Hub, screens: &[Screen], view: &T) {
    if let Ok(value) = serde_json::to_value(view) {
        for screen in screens {
            if hub.visible.contains(screen) {
                hub.shown.insert(*screen, value.clone());
            }
        }
    }
}

impl Screens {
    pub fn new(
        hgs: PathBuf,
        allowed: bool,
        spawn: Spawn,
        notify: Notify,
        fleet: FleetSource,
        clock: Clock,
    ) -> Self {
        Self(Arc::new(Shared {
            hub: Mutex::new(Hub::default()),
            hgs,
            allowed,
            spawn,
            notify,
            fleet,
            clock,
        }))
    }

    /// The app's hub: `default_hgs()` on Tauri's blocking pool, the fleet from
    /// the pump's mirror, `screens-changed` to the webview, and a ticker.
    pub fn tauri(app: tauri::AppHandle, pump: crate::pump::Pump) -> Self {
        use tauri::Emitter;
        let var = |name: &str| std::env::var(name).ok();
        let allowed = crate::actions::actions_allowed(
            var("ZERUS_CLIENT_REPLAY").as_deref(),
            var("ZERUS_CLIENT_HGS").as_deref(),
        );
        let started = Instant::now();
        let screens = Self::new(
            zerus_client_core::default_hgs(),
            allowed,
            Arc::new(|job: Job| {
                tauri::async_runtime::spawn_blocking(job);
            }),
            Arc::new(move |screen: Screen| {
                let _ = app.emit("screens-changed", screen.name());
            }),
            Arc::new(move || pump.fleet()),
            Arc::new(move || u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)),
        );
        screens.start_ticker();
        screens
    }

    fn lock(&self) -> MutexGuard<'_, Hub> {
        self.0.hub.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn now(&self) -> u64 {
        (self.0.clock)()
    }

    /// The hub, following the fleet as the pump has it now.
    fn current(&self) -> MutexGuard<'_, Hub> {
        let fleet = (self.0.fleet)();
        let mut hub = self.lock();
        hub.follow(fleet);
        hub
    }

    fn start_ticker(&self) {
        let screens = self.clone();
        std::thread::Builder::new()
            .name("zerus-screens".into())
            .spawn(move || loop {
                std::thread::sleep(Duration::from_secs(1));
                screens.tick();
            })
            .expect("the screens ticker starts");
    }

    /// One policy step, every second: follow the fleet, keep the warm-up and
    /// the visible screens' reads due, report changed views. With nothing
    /// changed and nothing due it costs a fleet comparison.
    pub fn tick(&self) {
        let runs = {
            let now = self.now();
            let mut hub = self.current();
            self.accounts_due(&mut hub, now, false)
        };
        self.run(runs);
        self.publish();
    }

    /// The account reads that start now: one `account ls` per newly listed
    /// host always, and while Overview or Accounts shows (or on Refresh,
    /// `force`) the catalogs and inspects due. Looks only when something may
    /// have changed: the fleet, a catalog, a shown screen or the due time.
    fn accounts_due(&self, hub: &mut Hub, now: u64, force: bool) -> Vec<Run> {
        let due = force || hub.stale || hub.next_due.is_some_and(|at| now >= at);
        if !self.0.allowed || !due {
            return Vec::new();
        }
        #[cfg(test)]
        {
            hub.work.steps += 1;
        }
        hub.stale = false;
        hub.next_due = None;
        let hosts = account_hosts(&hub.fleet);
        let mut runs: Vec<Run> = hub
            .accounts
            .warm(&hosts, now)
            .into_iter()
            .map(Run::Catalog)
            .collect();
        let shown = hub.accounts_shown();
        if shown || force {
            runs.extend(
                hub.accounts
                    .ensure_catalogs(&hosts, now, force)
                    .into_iter()
                    .map(Run::Catalog),
            );
            let refs = accounts::refs(&hub.fleet, &hub.accounts);
            for (r, reachable) in &refs {
                hub.accounts.ensure_usage(r, *reachable, now, force);
            }
            if shown {
                hub.next_due = hub.accounts.next_due(&hosts, &refs);
            }
        }
        runs.extend(hub.dispatch());
        // Reads that start show at once (AccountsPage::request calls
        // updateRefreshState): the busy state, "Updating… N remaining" and a
        // disabled Refresh. A step that starts nothing publishes nothing.
        if !runs.is_empty() && shown {
            hub.dirty.extend(ACCOUNT_SCREENS);
        }
        runs
    }

    fn run(&self, runs: impl IntoIterator<Item = Run>) {
        for job in runs {
            let screens = self.clone();
            (self.0.spawn)(Box::new(move || screens.read(job)));
        }
    }

    /// Runs one read (blocking, off the UI thread), files its reply and
    /// starts what it made due.
    fn read(&self, job: Run) {
        let hgs = &self.0.hgs;
        let local = ComputerId::local();
        let runs = match job {
            Run::Catalog(host) => {
                let reply = message(reads::read(hgs, &host, &Read::Accounts));
                let now = self.now();
                let mut hub = self.lock();
                let hosts = account_hosts(&hub.fleet);
                let again = hub.accounts.finish_catalog(&host, reply, now, &hosts);
                hub.stale = true;
                hub.dirty.extend(ACCOUNT_SCREENS);
                let mut runs: Vec<Run> = again.then_some(Run::Catalog(host)).into_iter().collect();
                runs.extend(self.accounts_due(&mut hub, now, false));
                runs
            }
            Run::Usage(r) => {
                let reply =
                    reads::read(hgs, &r.host, &Read::AccountUsage { id: r.id.clone() }).ok();
                let mut hub = self.lock();
                hub.accounts.finish_usage(&r, reply);
                // The profile falls due again five minutes after this attempt.
                let attempted = hub
                    .accounts
                    .usage
                    .get(&r.key())
                    .and_then(|slot| slot.attempted);
                if let Some(at) = attempted.filter(|_| hub.accounts_shown()) {
                    let at = at.saturating_add(USAGE_INTERVAL);
                    hub.next_due = Some(hub.next_due.map_or(at, |due| due.min(at)));
                }
                hub.dirty.extend(ACCOUNT_SCREENS);
                hub.dispatch()
            }
            Run::Machines => {
                let reply = message(reads::read(hgs, &local, &Read::Machines));
                let mut hub = self.lock();
                hub.machines.finish_list(reply);
                hub.dirty.insert(Screen::Machines);
                Vec::new()
            }
            Run::Resolve(alias) => {
                let read = Read::MachineResolve {
                    alias: alias.clone(),
                };
                let reply = message(reads::read(hgs, &local, &read));
                let mut hub = self.lock();
                hub.machines.finish_resolve(&alias, reply);
                hub.dirty.insert(Screen::Machines);
                Vec::new()
            }
            Run::Check(alias) => {
                let read = Read::MachineCheck {
                    alias: alias.clone(),
                };
                let reply = message(reads::read(hgs, &local, &read));
                let mut hub = self.lock();
                hub.machines.finish_check(&alias, reply);
                hub.dirty.insert(Screen::Machines);
                Vec::new()
            }
        };
        self.run(runs);
        self.publish();
    }

    /// One `screens-changed` per visible screen whose view changed since the
    /// webview last saw it. Only the screens a reply, the fleet or starting
    /// account reads touched are projected again.
    fn publish(&self) {
        let changed: Vec<Screen> = {
            let mut hub = self.lock();
            let mut changed = Vec::new();
            for screen in std::mem::take(&mut hub.dirty) {
                if !hub.visible.contains(&screen) {
                    continue;
                }
                let value = self.view_of(&mut hub, screen);
                if hub.shown.get(&screen) != Some(&value) {
                    hub.shown.insert(screen, value);
                    changed.push(screen);
                }
            }
            changed
        };
        for screen in changed {
            (self.0.notify)(screen);
        }
    }

    fn view_of(&self, hub: &mut Hub, screen: Screen) -> Value {
        #[cfg(test)]
        {
            hub.work.projections += 1;
        }
        match screen {
            Screen::Overview | Screen::Accounts => serde_json::to_value(self.accounts_of(hub)),
            Screen::Machines => serde_json::to_value(self.machines_of(hub)),
            Screen::Projects => serde_json::to_value(self.projects_of(hub)),
        }
        .unwrap_or(Value::Null)
    }

    fn accounts_of(&self, hub: &mut Hub) -> AccountsView {
        let mut view = accounts::project(&hub.fleet, &hub.accounts, &mut hub.display);
        if !self.0.allowed {
            view.notices.insert(0, REPLAY_NEEDS_HGS.into());
            view.pending = 0;
            view.busy = false;
            view.snapshot = true;
        }
        view
    }

    fn machines_of(&self, hub: &Hub) -> MachinesView {
        let mut view = hub.machines.view();
        if !self.0.allowed {
            view.error = Some(REPLAY_NEEDS_HGS.into());
        }
        view
    }

    fn projects_of(&self, hub: &Hub) -> ProjectsView {
        let peers: Vec<String> = hub
            .fleet
            .iter()
            .filter_map(|entry| entry.computer.alias().map(str::to_owned))
            .collect();
        // A blocked replay keeps the controller's line: Projects reads
        // nothing, and the organization's own `ready` blocks its edits.
        hub.swarm.view(&peers)
    }

    /// A screen shown or hidden. Showing Machines reads the list; Overview
    /// and Accounts make the account reads due; Projects reads nothing.
    /// Hiding both account screens drops their queued reads; reads already
    /// running finish.
    pub fn set_visible(&self, screen: Screen, visible: bool) {
        let runs = {
            let now = self.now();
            let mut hub = self.current();
            let mut runs = Vec::new();
            if visible {
                if hub.visible.insert(screen) && self.0.allowed {
                    match screen {
                        Screen::Machines => {
                            if hub.machines.start_list(now) {
                                runs.push(Run::Machines);
                            }
                        }
                        Screen::Overview | Screen::Accounts => hub.stale = true,
                        Screen::Projects => {}
                    }
                }
            } else if hub.visible.remove(&screen) {
                hub.shown.remove(&screen);
                if ACCOUNT_SCREENS.contains(&screen) && !hub.accounts_shown() {
                    hub.accounts.cancel_queued();
                    hub.next_due = None;
                }
            }
            runs.extend(self.accounts_due(&mut hub, now, false));
            runs
        };
        self.run(runs);
        self.publish();
    }

    /// The accounts view. `force` is Refresh: forced `ls` and inspects,
    /// ignored while account reads run (Qt's `reload`). A pull while neither
    /// Overview nor Accounts shows starts nothing.
    pub fn accounts(&self, force: bool) -> AccountsView {
        let runs = {
            let now = self.now();
            let mut hub = self.current();
            if force && !hub.accounts.reading() {
                self.accounts_due(&mut hub, now, true)
            } else if !force && hub.accounts_shown() {
                self.accounts_due(&mut hub, now, false)
            } else {
                Vec::new()
            }
        };
        self.run(runs);
        let mut hub = self.lock();
        let view = self.accounts_of(&mut hub);
        remember(&mut hub, &ACCOUNT_SCREENS, &view);
        view
    }

    fn machines_answer(&self) -> MachinesView {
        let mut hub = self.lock();
        let view = self.machines_of(&hub);
        remember(&mut hub, &[Screen::Machines], &view);
        view
    }

    /// The Machines view; `force` is Refresh (skipped while a list read or a
    /// check runs). The list has no poll, so a plain pull starts nothing.
    pub fn machines(&self, force: bool) -> MachinesView {
        let start = force && self.0.allowed && {
            let now = self.now();
            self.lock().machines.start_list(now)
        };
        if start {
            self.run([Run::Machines]);
        }
        self.machines_answer()
    }

    /// The selected peer's effective connection, read on selection.
    pub fn resolve(&self, alias: String) -> MachinesView {
        let start = self.0.allowed && self.lock().machines.start_resolve(&alias);
        if start {
            self.run([Run::Resolve(alias)]);
        }
        self.machines_answer()
    }

    /// "Test connection" (`machine check`, up to 25 s): never while the list
    /// reads or another check runs.
    pub fn check(&self, alias: String) -> MachinesView {
        let start = self.0.allowed && self.lock().machines.start_check(&alias);
        if start {
            self.run([Run::Check(alias)]);
        }
        self.machines_answer()
    }

    /// The Projects view. It follows the fleet (its unconnected computers)
    /// and starts no read.
    pub fn projects(&self) -> ProjectsView {
        let mut hub = self.current();
        let view = self.projects_of(&hub);
        remember(&mut hub, &[Screen::Projects], &view);
        view
    }

    /// The organization the Sessions list groups by, from the organization
    /// hub (`crate::organization::Presented`): its `swarm get` object, this
    /// device's default project and the write controller's status line, as
    /// Qt's `SwarmController::statusChanged` reaches
    /// `ProjectsDialog::setSwarmStatus` (SessionsWindow.cpp:1653). An empty
    /// status keeps the line the object gives.
    ///
    /// It takes only this hub's lock, projects and emits: it runs on the
    /// organization's threads, sometimes under its writer's lock, which the
    /// main thread takes in `flush` as the app quits. Tauri's emit queues
    /// the event for the event loop and never waits for the main thread (as
    /// long as tauri's `tracing` feature stays off: with it, `eval_script`
    /// waits for the event loop's answer).
    pub fn organization(&self, value: Arc<Value>, default_project: String, status: (String, bool)) {
        let value = (*value).clone();
        {
            let mut hub = self.lock();
            hub.swarm.set_default(Some(default_project));
            hub.swarm.finish(Ok(value));
            let (status, error) = status;
            if !status.is_empty() {
                hub.swarm.set_status(status, error);
            }
            hub.dirty.insert(Screen::Projects);
        }
        self.publish();
    }
}

#[tauri::command(async)]
pub fn screen_visible(
    state: State<'_, Screens>,
    screen: Screen,
    visible: bool,
) -> Result<(), String> {
    state.set_visible(screen, visible);
    Ok(())
}

#[tauri::command(async)]
pub fn accounts_view(state: State<'_, Screens>, force: bool) -> Result<AccountsView, String> {
    Ok(state.accounts(force))
}

#[tauri::command(async)]
pub fn machines_view(state: State<'_, Screens>, force: bool) -> Result<MachinesView, String> {
    Ok(state.machines(force))
}

#[tauri::command(async)]
pub fn machine_resolve(state: State<'_, Screens>, alias: String) -> Result<MachinesView, String> {
    Ok(state.resolve(alias))
}

#[tauri::command(async)]
pub fn machine_check(state: State<'_, Screens>, alias: String) -> Result<MachinesView, String> {
    Ok(state.check(alias))
}

#[tauri::command(async)]
pub fn projects_view(state: State<'_, Screens>) -> Result<ProjectsView, String> {
    Ok(state.projects())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::VecDeque;
    use std::path::Path;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    /// The core's fake hgs (client/core/tests/support/fake_hgs_reads.py) with the view fixtures.
    struct Fake {
        dir: PathBuf,
        hgs: PathBuf,
    }

    impl Fake {
        fn new(replies: Value) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "zerus-screens-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&dir).unwrap();
            let core = Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/tests");
            let hgs = dir.join("hgs");
            std::os::unix::fs::symlink(core.join("support/fake_hgs_reads.py"), &hgs).unwrap();
            let config = json!({"fixtures": core.join("fixtures/views"), "replies": replies});
            std::fs::write(dir.join("config.json"), config.to_string()).unwrap();
            Self { dir, hgs }
        }

        /// Every call so far, as its argv joined by spaces.
        fn calls(&self) -> Vec<String> {
            std::fs::read_to_string(self.dir.join("log.ndjson"))
                .unwrap_or_default()
                .lines()
                .map(|line| {
                    let call: Value = serde_json::from_str(line).unwrap();
                    call["argv"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|a| a.as_str().unwrap().to_owned())
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .collect()
        }
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// A hub on the injected clock. Its reads wait in `jobs` until the test
    /// runs them (deterministic), except on `threaded` hubs.
    struct Harness {
        screens: Screens,
        clock: Arc<AtomicU64>,
        notified: Arc<Mutex<Vec<Screen>>>,
        fleet: Arc<Mutex<Vec<FleetEntry>>>,
        jobs: Arc<Mutex<VecDeque<Job>>>,
        fake: Fake,
        /// The fake runs as a child of the tests (see `crate::PROCESS_TREE`).
        _tree: std::sync::RwLockReadGuard<'static, ()>,
    }

    impl Harness {
        fn at(&self, ms: u64) {
            self.clock.store(ms, Ordering::SeqCst);
        }

        fn count(&self, argv: &str) -> usize {
            self.fake
                .calls()
                .iter()
                .filter(|call| call.as_str() == argv)
                .count()
        }

        /// Reads started and not run yet.
        fn queued(&self) -> usize {
            self.jobs.lock().unwrap().len()
        }

        /// Runs the read queued at `index` while the others stay in flight;
        /// what it starts queues behind them.
        fn run(&self, index: usize) {
            let job = self.jobs.lock().unwrap().remove(index);
            job.expect("a queued read")();
        }

        /// Runs every queued read, and the reads they start, until none is left.
        fn drain(&self) {
            loop {
                let next = self.jobs.lock().unwrap().pop_front();
                let Some(job) = next else { break };
                job();
            }
        }

        /// The events since the last call.
        fn events(&self) -> Vec<Screen> {
            std::mem::take(&mut *self.notified.lock().unwrap())
        }

        fn set_fleet(&self, fleet: Vec<FleetEntry>) {
            *self.fleet.lock().unwrap() = fleet;
        }

        fn work(&self) -> Work {
            self.screens.lock().work
        }

        /// The view the last event or pull handed the webview for `screen`.
        fn published(&self, screen: Screen) -> Value {
            let hub = self.screens.lock();
            hub.shown.get(&screen).cloned().unwrap_or_default()
        }
    }

    fn local() -> FleetEntry {
        FleetEntry {
            computer: ComputerId::local(),
            status: "live".into(),
            attempt: 0,
            local_sessions: true,
            local_host: false,
            host: Some("workstation".into()),
            platform: Some("linux".into()),
        }
    }

    fn peer(alias: &str, status: &str, attempt: u32) -> FleetEntry {
        FleetEntry {
            computer: ComputerId::from(alias),
            status: status.into(),
            attempt,
            local_sessions: true,
            local_host: false,
            host: Some(format!("{alias}.example")),
            platform: Some("linux".into()),
        }
    }

    fn build(replies: Value, allowed: bool, threaded: bool) -> Harness {
        let tree = crate::PROCESS_TREE
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let fake = Fake::new(replies);
        let clock = Arc::new(AtomicU64::new(0));
        let notified = Arc::new(Mutex::new(Vec::new()));
        let fleet = Arc::new(Mutex::new(vec![local()]));
        let jobs: Arc<Mutex<VecDeque<Job>>> = Arc::new(Mutex::new(VecDeque::new()));
        let spawn: Spawn = if threaded {
            Arc::new(|job: Job| {
                std::thread::spawn(job);
            })
        } else {
            let jobs = jobs.clone();
            Arc::new(move |job: Job| jobs.lock().unwrap().push_back(job))
        };
        let screens = Screens::new(
            fake.hgs.clone(),
            allowed,
            spawn,
            {
                let notified = notified.clone();
                Arc::new(move |screen: Screen| notified.lock().unwrap().push(screen))
            },
            {
                let fleet = fleet.clone();
                Arc::new(move || fleet.lock().unwrap().clone())
            },
            {
                let clock = clock.clone();
                Arc::new(move || clock.load(Ordering::SeqCst))
            },
        );
        Harness {
            screens,
            clock,
            notified,
            fleet,
            jobs,
            fake,
            _tree: tree,
        }
    }

    fn harness(replies: Value, allowed: bool) -> Harness {
        build(replies, allowed, false)
    }

    fn member(view: &AccountsView, id: &str) -> accounts::Profile {
        view.accounts
            .iter()
            .flat_map(|a| &a.members)
            .find(|m| m.id == id)
            .cloned()
            .unwrap_or_else(|| panic!("{id} is listed"))
    }

    const INSPECTS: [&str; 3] = [
        "account inspect claude-work --refresh",
        "account inspect native-claude --refresh",
        "account inspect native-codex --refresh",
    ];

    /// The core's `swarm get` fixture (`core/tests/fixtures/views/swarm.json`).
    fn swarm_fixture() -> Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../core/tests/fixtures/views/swarm.json");
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn local_accounts() -> Value {
        json!({
            "account ls": {"file": "accounts-local.json"},
            "@studio account ls": {"file": "accounts-studio.json"},
            "account inspect claude-work --refresh": {"file": "inspect-claude-work.json"},
            "account inspect native-claude --refresh": {"stderr": "hgs: inspect failed\n", "exit": 1},
            "account inspect native-codex --refresh": {"file": "inspect-native-codex.json"},
        })
    }

    // Review Focus 5 of phase 3a, now "never" (the organization plan, Task 9).
    #[test]
    fn projects_show_the_organization_they_are_handed_and_never_read_it() {
        let h = harness(json!({"swarm get": {"file": "swarm.json"}}), true);
        h.screens.set_visible(Screen::Projects, true);
        for second in 1..=12 {
            h.at(second * 1000);
            h.screens.tick();
        }
        // Every read started so far runs: the account warm-up, and nothing else.
        h.drain();
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(
            h.count("swarm get"),
            0,
            "the organization topic feeds Projects"
        );
        assert!(!h.screens.projects().loaded);
        let swarm = swarm_fixture();
        let failed = "Project sync unavailable: hgs did not answer in time";
        // Showing Projects published its empty view; only the call's event counts.
        h.events();
        h.screens
            .organization(Arc::new(swarm), "sample".into(), (failed.into(), true));
        let view = h.screens.projects();
        assert!(view.loaded);
        assert!(view
            .projects
            .iter()
            .any(|p| p.id == "sample" && p.is_default));
        assert_eq!(
            (view.status.as_str(), view.status_error),
            (failed, true),
            "the controller's status line"
        );
        eventually("a change event", || {
            h.notified.lock().unwrap().contains(&Screen::Projects)
        });
    }

    // Review Focus 4.
    #[test]
    fn a_failing_or_slow_inspect_leaves_the_others_and_waits_five_minutes() {
        let h = harness(local_accounts(), true);
        h.screens.set_visible(Screen::Accounts, true);
        assert_eq!(h.queued(), 1, "the catalog first");
        h.run(0);
        assert_eq!(h.queued(), 2, "two inspects at once, the third waits");
        // claude-work is slow: the failing one answers and the third takes its turn.
        h.run(1);
        assert_eq!(h.queued(), 2);
        h.run(1);
        let view = h.screens.accounts(false);
        assert_eq!(
            member(&view, "native-codex").usage.status,
            "signed_out",
            "the others still load"
        );
        let failed = member(&view, "native-claude");
        assert!(failed.usage.refresh_error);
        assert_eq!(failed.usage.status, "unavailable");
        assert_eq!(view.pending, 1, "only the slow one is still read");
        h.drain();
        let view = h.screens.accounts(false);
        assert!(view.pending == 0 && !view.busy && view.snapshot);
        assert_eq!(member(&view, "claude-work").usage.status, "ok");
        for call in INSPECTS {
            assert_eq!(h.count(call), 1, "{call}");
        }
        // The catalog is read again after 30 s, the inspects only after 5 minutes.
        h.at(60_000);
        h.screens.tick();
        assert_eq!(h.queued(), 1);
        h.drain();
        assert_eq!(h.count("account ls"), 2);
        h.at(299_999);
        h.screens.tick();
        h.drain();
        assert_eq!(h.count("account ls"), 3);
        assert_eq!(
            h.count("account inspect native-claude --refresh"),
            1,
            "no retry inside five minutes"
        );
        h.at(300_000);
        h.events();
        h.screens.tick();
        assert_eq!(h.queued(), 2, "the re-inspect batch starts");
        assert_eq!(h.events(), [Screen::Accounts], "and shows at once");
        let shown = h.published(Screen::Accounts);
        assert_eq!(
            (shown["busy"].clone(), shown["pending"].clone()),
            (json!(true), json!(3))
        );
        h.drain();
        for call in INSPECTS {
            assert_eq!(h.count(call), 2, "{call}");
        }
        // Refresh forces both, and is ignored while account reads run (Qt's reload).
        h.at(301_000);
        assert!(h.screens.accounts(true).busy);
        assert_eq!(h.queued(), 3, "the catalog and two inspects");
        h.screens.accounts(true);
        assert_eq!(h.queued(), 3);
        h.drain();
        assert_eq!(h.count("account ls"), 4);
        for call in INSPECTS {
            assert_eq!(h.count(call), 3, "{call}");
        }
    }

    // Controller ruling: no reads for hidden screens.
    #[test]
    fn hidden_account_screens_drop_queued_reads_and_only_warm_new_hosts() {
        let h = harness(local_accounts(), true);
        h.screens.set_visible(Screen::Overview, true);
        h.run(0);
        assert_eq!(h.queued(), 2);
        h.screens.set_visible(Screen::Overview, false);
        h.drain();
        assert_eq!(h.count(INSPECTS[0]), 1, "running reads finish");
        assert_eq!(h.count(INSPECTS[2]), 0, "the queued inspect is dropped");
        for at in [40_000, 400_000] {
            h.at(at);
            h.screens.tick();
            h.screens.accounts(false);
        }
        assert_eq!(h.queued(), 0, "hidden: nothing falls due");
        // A newly listed host is still warmed, once, without inspects.
        h.set_fleet(vec![local(), peer("studio", "live", 0)]);
        h.screens.accounts(false);
        assert_eq!(h.queued(), 0, "a hidden pull starts nothing");
        h.screens.tick();
        assert_eq!(h.queued(), 1);
        h.drain();
        assert_eq!(h.count("@studio account ls"), 1);
        assert!(h
            .fake
            .calls()
            .iter()
            .all(|call| !call.starts_with("@studio account inspect")));
        h.at(500_000);
        h.screens.tick();
        assert_eq!(h.queued(), 0, "a host is warmed once");
        // Showing Accounts makes every read due again, the dropped inspect among them.
        h.screens.set_visible(Screen::Accounts, true);
        h.drain();
        assert_eq!(h.count("account ls"), 2);
        assert_eq!(h.count("@studio account ls"), 2);
        assert_eq!(h.count(INSPECTS[2]), 1);
        assert_eq!(
            h.count("@studio account inspect claude-studio --refresh"),
            1
        );
    }

    // Controller ruling: no per-profile work on the ticker.
    #[test]
    fn the_ticker_works_only_after_a_fleet_change_a_reply_or_a_due_time() {
        let h = harness(local_accounts(), true);
        h.set_fleet(vec![local(), peer("studio", "live", 0)]);
        h.screens.set_visible(Screen::Accounts, true);
        h.drain();
        h.screens.accounts(false);
        h.events();
        let idle = h.work();
        for second in 1..=29 {
            h.at(second * 1000);
            h.screens.tick();
        }
        assert_eq!(
            h.work(),
            idle,
            "idle ticks step no account read and project no view"
        );
        assert!(h.events().is_empty() && h.queued() == 0);
        // A peer that drops out changes the view: one event, then quiet again.
        h.set_fleet(vec![local(), peer("studio", "reconnecting", 1)]);
        h.screens.tick();
        h.screens.tick();
        assert_eq!(h.events(), [Screen::Accounts]);
        let changed = h.work();
        assert_eq!(
            (changed.steps, changed.projections),
            (idle.steps + 1, idle.projections + 1)
        );
        // The catalogs fall due 30 s after they started; the reads that
        // start show at once (Qt's updateRefreshState), in one projection.
        h.at(30_000);
        h.screens.tick();
        assert_eq!(h.queued(), 2);
        assert_eq!(h.events(), [Screen::Accounts]);
        assert_eq!(h.published(Screen::Accounts)["busy"], json!(true));
        let busy = h.work();
        assert_eq!(busy.projections, changed.projections + 1);
        h.screens.tick();
        assert_eq!(h.work(), busy, "nothing due: nothing published");
        assert!(h.events().is_empty() && h.queued() == 2);
        h.drain();
        assert_eq!(h.count("account ls"), 2);
        assert_eq!(h.published(Screen::Accounts)["busy"], json!(false));
    }

    #[test]
    fn machines_list_on_show_resolve_on_selection_and_check_only_on_request() {
        let h = harness(
            json!({
                "machine ls": {"file": "machines.json"},
                "machine resolve studio": {"file": "resolve-studio.json"},
                "machine check studio": {"file": "check-studio.json"},
            }),
            true,
        );
        h.screens.tick();
        h.drain();
        assert_eq!(h.count("account ls"), 1, "the account warm-up");
        h.screens.machines(false);
        assert_eq!(h.queued(), 0, "a hidden pull reads nothing");
        h.screens.set_visible(Screen::Machines, true);
        h.screens.set_visible(Screen::Machines, true);
        assert!(h.screens.machines(false).refreshing);
        assert_eq!(h.queued(), 1, "one list read on show");
        h.drain();
        let view = h.screens.machines(false);
        assert!(view.loaded && !view.refreshing);
        assert_eq!(view.machines.len(), 3);
        assert_eq!(h.events(), [Screen::Machines]);
        h.screens.resolve("studio".into());
        h.drain();
        assert!(h
            .screens
            .machines(false)
            .effective
            .is_some_and(|e| !e.pending));
        assert_eq!(
            h.count("machine check studio"),
            0,
            "a check runs only on request"
        );
        assert!(h.screens.check("studio".into()).busy);
        h.screens.check("studio".into());
        h.screens.machines(true);
        assert_eq!(
            h.queued(),
            1,
            "one check at a time, and no list read meanwhile"
        );
        h.drain();
        assert!(h.screens.machines(false).check.is_some_and(|c| !c.pending));
        h.screens.machines(true);
        h.screens.check("studio".into());
        assert_eq!(h.queued(), 1, "no check while the list reads");
        h.drain();
        assert_eq!(h.count("machine ls"), 2);
        assert_eq!(h.count("machine check studio"), 1);
    }

    #[test]
    fn a_replay_without_hgs_never_reads() {
        let h = harness(json!({}), false);
        for screen in [
            Screen::Overview,
            Screen::Machines,
            Screen::Projects,
            Screen::Accounts,
        ] {
            h.screens.set_visible(screen, true);
        }
        h.at(40_000);
        h.screens.tick();
        assert_eq!(
            h.screens.resolve("studio".into()).error.as_deref(),
            Some(REPLAY_NEEDS_HGS)
        );
        h.screens.check("studio".into());
        h.screens.machines(true);
        let accounts = h.screens.accounts(true);
        let projects = h.screens.projects();
        assert_eq!(h.queued(), 0, "no read ever starts");
        assert!(h.fake.calls().is_empty());
        assert_eq!(accounts.notices, [REPLAY_NEEDS_HGS]);
        assert!(accounts.snapshot && accounts.pending == 0 && !accounts.busy);
        assert!(!projects.loaded && projects.status.is_empty());
        // Projects reads nothing, so it keeps the project sync line it is
        // handed; the fixture's own line names a conflict.
        let line = "Local catalog. Connect another computer to sync projects.";
        h.screens.organization(
            Arc::new(swarm_fixture()),
            "sample".into(),
            (line.into(), false),
        );
        let projects = h.screens.projects();
        assert!(projects.loaded);
        assert_eq!(
            (projects.status.as_str(), projects.status_error),
            (line, false)
        );
        assert!(h.fake.calls().is_empty(), "and still reads nothing");
    }

    fn eventually(what: &str, done: impl Fn() -> bool) {
        let until = Instant::now() + Duration::from_secs(20);
        while !done() {
            assert!(Instant::now() < until, "{what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The one test on real threads, as the app runs its reads: two slow
    /// inspects overlap, and every read still answers once.
    #[test]
    fn reads_run_on_their_own_threads() {
        let mut replies = local_accounts();
        for call in [INSPECTS[0], INSPECTS[2]] {
            replies[call]["sleep"] = json!(0.3);
        }
        let h = build(replies, true, true);
        h.screens.set_visible(Screen::Accounts, true);
        eventually("every inspect answered", || {
            let view = h.screens.accounts(false);
            view.pending == 0 && !view.busy && view.snapshot
        });
        for call in INSPECTS {
            assert_eq!(h.count(call), 1, "{call}");
        }
        assert_eq!(
            member(&h.screens.accounts(false), "claude-work")
                .usage
                .status,
            "ok"
        );
    }
}
