//! The reads behind the Overview, Projects, Machines and Accounts screens:
//! one hub that knows which screens are visible, runs the due `hgs` reads off
//! the main thread (`zerus_client_core::reads`, bounded and timed out there),
//! keeps the replies and hands the webview projected view models
//! (`zerus_client_core::views`). Which read runs when is decided in
//! `views::schedule`; this file wires the clock, the threads and the event.
//!
//! Every command answers at once with the current view and starts what is
//! due in the background. A screen's reads run only while it shows, or on an
//! explicit Refresh, selection or "Test connection"; hidden, only Qt's
//! account warm-up runs (one `account ls` per newly listed host). A changed
//! view emits `screens-changed` with the screen's name; the frontend then
//! pulls it again. A replay reads only with an explicit `ZERUS_CLIENT_HGS`,
//! as the actions do.
//!
//! New session reads one computer's accounts on the same hub
//! (`launch_accounts`): its `account ls` and the limits of its profiles go
//! through the hub's catalogs and usage queue, never a second store. The open
//! dialog's computer is watched until `launch_accounts_close`: its rows are
//! grouped when a good catalog is filed and later limits only refresh them,
//! as Qt's dialog does; each of its replies emits `launch-accounts-changed`
//! with the computer, and the dialog pulls its rows again
//! (`launch_account_rows`), as Qt's dialog follows the usage store.
//!
//! The one-second ticker does no per-profile work: it compares the fleet and
//! steps the account reads only when the fleet changed, a catalog arrived, an
//! account screen showed or the earliest due time passed. A view is
//! projected again only after a read finished, the fleet changed, or account
//! reads started while Overview or Accounts shows (their busy state).

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;
use tauri::State;
use zerus_client_core::launch;
use zerus_client_core::paths::{self, Style};
use zerus_client_core::reads::{self, Read, ReadError};
use zerus_client_core::views::accounts::{
    self, Account, AccountsDisplay, AccountsState, AccountsView, Profile, UsageRef,
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
/// Told when a reply for New session's computer was filed.
pub type LaunchNotify = Arc<dyn Fn(&ComputerId) + Send + Sync>;

/// The most profiles of one computer New session lists.
const LAUNCH_PROFILES: usize = 256;
/// How long `launch_accounts` waits for a catalog read: its 25 s limit and
/// the read helper's margin.
const LAUNCH_WAIT: Duration = Duration::from_secs(40);

/// New session's account rows of one computer (`NewSessionDialog::loadAccounts`
/// and `updateAccounts`).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct LaunchAccounts {
    /// A catalog reply (good or failed) was filed since the dialog chose
    /// this computer.
    pub loaded: bool,
    /// "Could not load accounts: %1" for the last reply's failure (Qt's
    /// `failureDetail`); the rows stay those of the last good catalog since
    /// the dialog chose this computer, none without one.
    pub error: Option<String>,
    /// The catalog's raw rows, at most 256, for the OpenCode fields.
    pub profiles: Vec<Value>,
    /// `views::accounts::group` over its launchable profiles, each member with
    /// the dialog's usage and each group with its members' best.
    pub groups: Vec<Account>,
}

/// The open dialog's computer (`m_accountHost` with `m_accounts`,
/// `m_accountGroups` and the status line).
struct LaunchWatch {
    computer: ComputerId,
    /// The dialog's agents in Qt's order: its requested agent joins them.
    agents: Vec<String>,
    /// A catalog reply was filed since the dialog chose this computer.
    replied: bool,
    error: Option<String>,
    /// The last good catalog's groups and the folder style they were made
    /// for; later limits refresh them, the next catalog or style regroups.
    groups: Option<(Style, Vec<Account>)>,
    /// `launch_accounts` calls still waiting for this computer's catalog: they
    /// ask for its limits themselves once they wake.
    waiting: usize,
    /// A reply was filed since the last ask (`launch_start`).
    answered: bool,
    /// The last ask was Refresh: the limits are forced.
    force: bool,
}

impl LaunchWatch {
    fn agents(&self) -> Vec<&str> {
        self.agents.iter().map(String::as_str).collect()
    }
}

/// The dialog's agents (221-222): `launch::AGENTS`, then a requested agent
/// that is neither among them nor the shell.
fn launch_agents(agent: Option<&str>) -> Vec<String> {
    let mut agents: Vec<String> = launch::AGENTS.iter().map(|a| (*a).to_owned()).collect();
    if let Some(agent) = agent.filter(|a| !a.is_empty() && *a != "sh") {
        if !agents.iter().any(|a| a == agent) {
            agents.push(agent.to_owned());
        }
    }
    agents
}

/// A computer's folder style (`folderStyle`, NewSessionDialog.cpp:108-110).
fn style_of(entry: &FleetEntry) -> Style {
    paths::of(entry.platform.as_deref(), entry.computer.is_local())
}

/// The watched computer's fleet entry. One outside the fleet (its stream
/// ended, or no computer of the fleet is called so) is a peer whose stream
/// ended: its folders are POSIX and its limits are not read.
fn launch_entry(hub: &Hub, computer: &ComputerId) -> FleetEntry {
    hub.fleet
        .iter()
        .find(|entry| entry.computer == *computer)
        .cloned()
        .unwrap_or_else(|| FleetEntry {
            computer: computer.clone(),
            status: "ended".into(),
            attempt: 0,
            local_sessions: true,
            local_host: false,
            host: None,
            platform: None,
        })
}

/// The hosts whose account reads the hub keeps: every machine that keeps
/// accounts, and New session's computer while the dialog watches it, which Qt
/// reads directly whatever the Accounts page lists (865-874).
fn hub_hosts(hub: &Hub) -> Vec<ComputerId> {
    let mut hosts = account_hosts(&hub.fleet);
    if let Some(watch) = &hub.launch {
        if !hosts.contains(&watch.computer) {
            hosts.push(watch.computer.clone());
        }
    }
    hosts
}

/// The launch profiles of the watched computer's current catalog.
fn launch_members(hub: &Hub, watch: &LaunchWatch) -> Option<(FleetEntry, Vec<Profile>)> {
    let entry = launch_entry(hub, &watch.computer);
    let data = hub.accounts.catalogs.get(&watch.computer)?.data.as_ref()?;
    let members = accounts::host_profiles(&entry, data, &hub.accounts, &watch.agents())
        .into_iter()
        .map(|(profile, _)| profile)
        .collect();
    Some((entry, members))
}

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
    /// The open New session dialog's computer.
    launch: Option<LaunchWatch>,
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
    /// Woken whenever an `account ls` reply is filed.
    catalog_done: Condvar,
    launch_notify: OnceLock<LaunchNotify>,
}

/// The managed state of the screen commands; clones share it.
#[derive(Clone)]
pub struct Screens(Arc<Shared>);

/// A read the hub decided to start.
enum Run {
    Catalog(ComputerId),
    Usage(UsageRef),
    Swarm,
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
            catalog_done: Condvar::new(),
            launch_notify: OnceLock::new(),
        }))
    }

    /// Where New session hears of its computer's replies; set once.
    pub fn on_launch_change(&self, notify: LaunchNotify) {
        let _ = self.0.launch_notify.set(notify);
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
        let launch_app = app.clone();
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
        screens.on_launch_change(Arc::new(move |computer: &ComputerId| {
            let _ = launch_app.emit("launch-accounts-changed", computer.0.clone());
        }));
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
            let mut runs = self.accounts_due(&mut hub, now, false);
            runs.extend(self.projects_due(&mut hub, now));
            runs
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
        let hosts = hub_hosts(hub);
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

    /// The shared project catalog, every five seconds while Projects shows.
    fn projects_due(&self, hub: &mut Hub, now: u64) -> Option<Run> {
        let due = self.0.allowed
            && hub.visible.contains(&Screen::Projects)
            && hub.swarm.poll.due(now, false);
        due.then_some(Run::Swarm)
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
        let mut heard: Option<ComputerId> = None;
        let mut limits: Vec<Run> = Vec::new();
        let runs = match job {
            Run::Catalog(host) => {
                // Qt's requestAccounts texts, for the Accounts page as for New session.
                let reply = launch::read_accounts(hgs, &host);
                let failure = reply.as_ref().err().cloned();
                let now = self.now();
                let mut hub = self.lock();
                let hosts = hub_hosts(&hub);
                let again = hub.accounts.finish_catalog(&host, reply, now, &hosts);
                hub.stale = true;
                hub.dirty.extend(ACCOUNT_SCREENS);
                if hub
                    .launch
                    .as_ref()
                    .is_some_and(|watch| watch.computer == host)
                {
                    heard = Some(host.clone());
                    let grouped = match &failure {
                        // Grouped when the catalog arrives (`m_regroupAccounts`, 380-386, 883).
                        None => hub.launch.as_ref().and_then(|watch| {
                            let (entry, members) = launch_members(&hub, watch)?;
                            Some((style_of(&entry), accounts::group(&members)))
                        }),
                        Some(_) => None,
                    };
                    let mut late: Option<bool> = None;
                    if let Some(watch) = hub.launch.as_mut() {
                        watch.replied = true;
                        watch.answered = true;
                        match failure {
                            None => {
                                watch.error = None;
                                watch.groups = grouped;
                                // No `launch_accounts` waits for it any more: the
                                // catalog arrived late, and Qt's accountsReady
                                // asks for its limits (380-388).
                                late = (watch.waiting == 0).then_some(watch.force);
                            }
                            Some(detail) => {
                                watch.error = Some(format!("Could not load accounts: {detail}"));
                            }
                        }
                    }
                    if let Some(force) = late {
                        limits = self.launch_limits(&mut hub, &host, force);
                    }
                }
                let mut runs: Vec<Run> = again.then_some(Run::Catalog(host)).into_iter().collect();
                runs.append(&mut limits);
                runs.extend(self.accounts_due(&mut hub, now, false));
                self.0.catalog_done.notify_all();
                runs
            }
            Run::Usage(r) => {
                let reply =
                    reads::read(hgs, &r.host, &Read::AccountUsage { id: r.id.clone() }).ok();
                let mut hub = self.lock();
                hub.accounts.finish_usage(&r, reply);
                if hub
                    .launch
                    .as_ref()
                    .is_some_and(|watch| watch.computer == r.host)
                {
                    heard = Some(r.host.clone());
                }
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
            Run::Swarm => {
                let reply = message(reads::read(hgs, &local, &Read::Swarm));
                let mut hub = self.lock();
                hub.swarm.finish(reply);
                hub.dirty.insert(Screen::Projects);
                Vec::new()
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
        if let (Some(computer), Some(notify)) = (heard, self.0.launch_notify.get()) {
            notify(&computer);
        }
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
        let mut view = hub.swarm.view(&peers);
        if !self.0.allowed {
            view.status = REPLAY_NEEDS_HGS.into();
            view.status_error = true;
        }
        view
    }

    /// A screen shown or hidden. Showing Machines reads the list, showing
    /// Projects the catalog; Overview and Accounts make the account reads
    /// due. Hiding both of those drops their queued reads; reads already
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
                        Screen::Projects => {
                            if hub.swarm.poll.due(now, true) {
                                runs.push(Run::Swarm);
                            }
                        }
                        Screen::Overview | Screen::Accounts => hub.stale = true,
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

    /// New session's computer (`loadAccounts`, 865-874): its `account ls`
    /// starts on every open, computer change and Refresh, sharing one that
    /// runs. Another computer, or a new dialog (not `refresh`), starts with no
    /// rows until a reply is filed (Qt clears `m_accounts`). A computer the
    /// Accounts page does not list is read too, as Qt's dialog reads the
    /// selected computer directly (865-874): a computer that cannot be read
    /// answers with hgs's error. The caller waits for the catalog until
    /// [`Self::launch_waited`].
    fn launch_start(&self, computer: &ComputerId, refresh: bool, agent: Option<&str>) -> Vec<Run> {
        let now = self.now();
        let mut hub = self.current();
        let agents = launch_agents(agent);
        match hub.launch.as_mut() {
            Some(watch) if refresh && watch.computer == *computer => {
                watch.agents = agents;
                watch.waiting += 1;
                watch.answered = false;
                watch.force = true;
            }
            _ => {
                hub.launch = Some(LaunchWatch {
                    computer: computer.clone(),
                    agents,
                    replied: false,
                    error: None,
                    groups: None,
                    waiting: 1,
                    answered: false,
                    force: refresh,
                })
            }
        }
        if hub.accounts.ensure_catalog(computer, now) {
            hub.dirty.extend(ACCOUNT_SCREENS);
            vec![Run::Catalog(computer.clone())]
        } else {
            Vec::new()
        }
    }

    /// The dialog closed: its computer is no longer watched, and its replies
    /// tell nobody.
    pub fn launch_close(&self) {
        let mut hub = self.lock();
        hub.launch = None;
        // The next step forgets its reads when the Accounts page lists no such computer.
        hub.stale = true;
    }

    /// A `launch_accounts` call stopped waiting: once its ask was answered it
    /// asks for the limits of the catalog (`requestUsage`, forced on Refresh).
    /// A reply filed after the last caller stopped waiting asks for them itself.
    fn launch_waited(&self, computer: &ComputerId, force: bool) -> Vec<Run> {
        let mut hub = self.lock();
        let answered = match hub.launch.as_mut() {
            Some(watch) if watch.computer == *computer => {
                watch.waiting = watch.waiting.saturating_sub(1);
                watch.answered
            }
            _ => return Vec::new(),
        };
        if !answered {
            return Vec::new();
        }
        self.launch_limits(&mut hub, computer, force)
    }

    /// Waits while the computer's catalog reads, at most [`LAUNCH_WAIT`].
    fn launch_wait(&self, computer: &ComputerId) {
        let until = Instant::now() + LAUNCH_WAIT;
        let mut hub = self.lock();
        while hub
            .accounts
            .catalogs
            .get(computer)
            .is_some_and(|slot| slot.pending)
        {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            hub = self
                .0
                .catalog_done
                .wait_timeout(hub, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// The limits of the computer's installed launch profiles
    /// (`requestUsage`): those not checked within five minutes, all on
    /// Refresh. Returns the inspects that start now.
    #[cfg(test)]
    fn launch_usage(&self, computer: &ComputerId, force: bool) -> Vec<Run> {
        let mut hub = self.lock();
        self.launch_limits(&mut hub, computer, force)
    }

    /// [`Self::launch_usage`] on the locked hub.
    fn launch_limits(&self, hub: &mut Hub, computer: &ComputerId, force: bool) -> Vec<Run> {
        let now = self.now();
        let Some(watch) = hub
            .launch
            .as_ref()
            .filter(|watch| watch.computer == *computer)
        else {
            return Vec::new();
        };
        let Some((entry, members)) = launch_members(hub, watch) else {
            return Vec::new();
        };
        let reachable = accounts::reachable(&entry);
        for profile in members {
            if profile.installed {
                let r = UsageRef {
                    host: profile.host,
                    id: profile.id,
                    home: profile.home,
                    provider: profile.provider,
                    auth_revision: profile.auth_revision,
                };
                hub.accounts.ensure_usage(&r, reachable, now, force);
            }
        }
        hub.dirty.extend(ACCOUNT_SCREENS);
        hub.dispatch()
    }

    /// The watched computer's rows; reads nothing. Members and each row's
    /// best usage are current (`refreshUsage`, 990-1000); the grouping is the
    /// last good catalog's, made again only when the folder style changed
    /// (883). Another computer than the dialog's has none.
    pub fn launch_rows(&self, computer: &ComputerId) -> LaunchAccounts {
        let mut hub = self.lock();
        let Some(watch) = hub
            .launch
            .as_ref()
            .filter(|watch| watch.computer == *computer)
        else {
            return LaunchAccounts::default();
        };
        if !watch.replied {
            return LaunchAccounts::default();
        }
        let error = watch.error.clone();
        let Some((style, groups)) = watch.groups.as_ref() else {
            return LaunchAccounts {
                loaded: true,
                error,
                ..LaunchAccounts::default()
            };
        };
        let Some((entry, members)) = launch_members(&hub, watch) else {
            return LaunchAccounts {
                loaded: true,
                error,
                ..LaunchAccounts::default()
            };
        };
        let groups = if style_of(&entry) == *style {
            accounts::refresh_groups(groups, &members)
        } else {
            let regrouped = accounts::group(&members);
            if let Some(watch) = hub.launch.as_mut() {
                watch.groups = Some((style_of(&entry), regrouped.clone()));
            }
            regrouped
        };
        let profiles: Vec<Value> = hub
            .accounts
            .catalogs
            .get(computer)
            .and_then(|slot| slot.data.as_ref())
            .and_then(|data| data.get("profiles"))
            .and_then(Value::as_array)
            .map(|rows| rows.iter().take(LAUNCH_PROFILES).cloned().collect())
            .unwrap_or_default();
        LaunchAccounts {
            loaded: true,
            error,
            profiles,
            groups,
        }
    }

    /// New session's accounts of one computer (`loadAccounts` with
    /// `requestUsage`): reads its catalog (sharing a running read) and waits
    /// for it, then asks for its profiles' limits (forced on `refresh`) and
    /// answers the rows. `refresh` keeps the dialog's rows on failure; an
    /// open or another computer starts without them. `agent` is the dialog's
    /// requested agent. Blocks; the limits arrive later.
    pub fn launch_accounts(
        &self,
        computer: &ComputerId,
        refresh: bool,
        agent: Option<&str>,
    ) -> Result<LaunchAccounts, String> {
        if !self.0.allowed {
            return Err(REPLAY_NEEDS_HGS.into());
        }
        let runs = self.launch_start(computer, refresh, agent);
        self.run(runs);
        self.launch_wait(computer);
        let runs = self.launch_waited(computer, refresh);
        self.run(runs);
        self.publish();
        Ok(self.launch_rows(computer))
    }

    /// The Projects view; while it shows, a pull also starts a due poll.
    pub fn projects(&self) -> ProjectsView {
        let run = {
            let now = self.now();
            let mut hub = self.current();
            self.projects_due(&mut hub, now)
        };
        self.run(run);
        let mut hub = self.lock();
        let view = self.projects_of(&hub);
        remember(&mut hub, &[Screen::Projects], &view);
        view
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

/// New session's accounts of one computer, on its open, a computer change
/// (`refresh` false) and Refresh (`refresh` true); `agent` is the dialog's
/// requested agent.
#[tauri::command]
pub async fn launch_accounts(
    state: State<'_, Screens>,
    computer: String,
    refresh: bool,
    agent: Option<String>,
) -> Result<LaunchAccounts, String> {
    let screens = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        screens.launch_accounts(&ComputerId(computer), refresh, agent.as_deref())
    })
    .await
    .map_err(|error| error.to_string())?
}

/// The dialog closed: `launch-accounts-changed` stops.
#[tauri::command(async)]
pub fn launch_accounts_close(state: State<'_, Screens>) {
    state.launch_close();
}

/// The rows again, after `launch-accounts-changed` or the dialog's minute;
/// reads nothing.
#[tauri::command(async)]
pub fn launch_account_rows(state: State<'_, Screens>, computer: String) -> LaunchAccounts {
    state.launch_rows(&ComputerId(computer))
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

    impl Fake {
        /// Replaces one argv's reply.
        fn set_reply(&self, argv: &str, reply: Value) {
            let path = self.dir.join("config.json");
            let mut config: Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            config["replies"][argv] = reply;
            std::fs::write(path, config.to_string()).unwrap();
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

    fn local_accounts() -> Value {
        json!({
            "account ls": {"file": "accounts-local.json"},
            "@studio account ls": {"file": "accounts-studio.json"},
            "account inspect claude-work --refresh": {"file": "inspect-claude-work.json"},
            "account inspect native-claude --refresh": {"stderr": "hgs: inspect failed\n", "exit": 1},
            "account inspect native-codex --refresh": {"file": "inspect-native-codex.json"},
        })
    }

    // Review Focus 5.
    #[test]
    fn projects_read_only_while_shown_every_five_seconds_and_never_twice_at_once() {
        let h = harness(json!({"swarm get": {"file": "swarm.json"}}), true);
        for second in 1..=12 {
            h.at(second * 1000);
            h.screens.tick();
        }
        h.screens.projects();
        assert_eq!(h.queued(), 1, "hidden: only the account warm-up");
        h.drain();
        assert_eq!(h.count("account ls"), 1);
        assert_eq!(h.count("swarm get"), 0, "hidden: no read");
        h.at(13_000);
        h.screens.set_visible(Screen::Projects, true);
        assert_eq!(h.queued(), 1, "showing reads the catalog");
        h.at(18_100);
        h.screens.tick();
        assert!(h.screens.projects().refreshing);
        assert_eq!(h.queued(), 1, "the first read still runs");
        h.drain();
        assert_eq!(h.count("swarm get"), 1);
        assert_eq!(h.events(), [Screen::Projects], "the catalog arrived");
        assert!(h.screens.projects().loaded);
        h.screens.tick();
        assert_eq!(h.queued(), 1, "due 5 s after the last start");
        h.at(30_000);
        h.screens.tick();
        h.screens.projects();
        assert_eq!(h.queued(), 1, "never two at once");
        h.drain();
        assert_eq!(h.count("swarm get"), 2);
        h.screens.set_visible(Screen::Projects, false);
        h.at(60_000);
        h.screens.tick();
        h.screens.projects();
        assert_eq!(h.queued(), 0, "hidden again: no read");
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
        assert_eq!(
            (projects.status.as_str(), projects.status_error),
            (REPLAY_NEEDS_HGS, true)
        );
    }

    /// An `account ls` reply of the Qt fixture's shape (test_newsessiondialog.cpp).
    fn launch_catalog(rows: &[(&str, &str, &str)]) -> Value {
        let profiles: Vec<Value> = rows
            .iter()
            .map(|(id, provider, email)| {
                json!({"id": id, "provider": provider, "label": "Review", "home": format!("/profiles/{id}"),
                       "installed": true, "is_default": false,
                       "account_status": {"status": if *provider == "dsh" { "signed_out" } else { "ok" },
                                          "checked_at": 100.0, "identity": {"email": email}}})
            })
            .collect();
        json!({"profiles": profiles, "revision": "r1"})
    }

    fn reply(value: &Value) -> Value {
        json!({ "stdout": value.to_string() })
    }

    fn inspect(id: &str, provider: &str, email: &str) -> Value {
        reply(
            &json!({"id": id, "provider": provider, "home": format!("/profiles/{id}"),
            "status": if provider == "dsh" { "signed_out" } else { "ok" }, "checked_at": 200.0,
            "identity": {"email": email}, "windows": [], "balances": []}),
        )
    }

    fn launch_replies(duplicates: bool) -> Value {
        let mut local = vec![
            ("work", "codex", "shared@example.invalid"),
            ("claude", "claude", "shared@example.invalid"),
            ("native-dsh", "dsh", "shared@example.invalid"),
            ("native-opencode", "opencode", ""),
        ];
        if duplicates {
            local.push(("claude-copy", "claude", "shared@example.invalid"));
        }
        let studio = [
            ("mac-claude", "claude", "mac@example.invalid"),
            ("native-dsh", "dsh", "mac@example.invalid"),
        ];
        let mut replies = json!({
            "account ls": reply(&launch_catalog(&local)),
            "@studio account ls": reply(&launch_catalog(&studio)),
        });
        for (id, provider, email) in local {
            replies[format!("account inspect {id} --refresh")] = inspect(id, provider, email);
        }
        for (id, provider, email) in studio {
            replies[format!("@studio account inspect {id} --refresh")] =
                inspect(id, provider, email);
        }
        replies
    }

    fn members(view: &LaunchAccounts) -> Vec<Vec<String>> {
        view.groups
            .iter()
            .map(|group| group.members.iter().map(|m| m.id.clone()).collect())
            .collect()
    }

    // accountAliasesMatchAccountsGroupingAndKeepExactBinding and
    // chooserAndAccountsPageShowSameAccountsForEachComputer: one computer's
    // rows group as the Accounts page groups that computer's profiles.
    #[test]
    fn launch_accounts_group_one_computer_as_the_accounts_page_does() {
        let h = build(launch_replies(true), true, true);
        h.set_fleet(vec![local(), peer("studio", "live", 0)]);
        let view = h
            .screens
            .launch_accounts(&ComputerId::local(), false, None)
            .unwrap();
        assert!(view.loaded && view.error.is_none());
        assert_eq!(view.profiles.len(), 5, "the raw rows");
        assert_eq!(
            members(&view),
            [
                vec!["work"],
                vec!["claude", "claude-copy"],
                vec!["native-dsh"],
                vec!["native-opencode"]
            ]
        );
        eventually("the limits arrived", || {
            let rows = h.screens.launch_rows(&ComputerId::local());
            rows.groups
                .iter()
                .all(|g| g.usage.checked_at == Some(200.0) || g.provider == "opencode")
        });
        h.screens.set_visible(Screen::Accounts, true);
        eventually("the Accounts page read everything", || {
            let page = h.screens.accounts(false);
            page.pending == 0 && !page.busy && page.snapshot
        });
        let page = h.screens.accounts(false);
        for computer in [ComputerId::local(), ComputerId::from("studio")] {
            let view = h.screens.launch_accounts(&computer, false, None).unwrap();
            let shown: BTreeSet<String> = view.groups.iter().map(|g| g.key.clone()).collect();
            let listed: BTreeSet<String> = page
                .accounts
                .iter()
                .filter(|a| a.members.iter().any(|m| m.host == computer))
                .map(|a| a.key.clone())
                .collect();
            assert_eq!(shown, listed, "{computer:?}");
            assert!(view
                .groups
                .iter()
                .flat_map(|g| &g.members)
                .all(|m| m.host == computer));
        }
    }

    // localWindowsHidesDeepSeekAndOpenCode.
    #[test]
    fn the_local_windows_computer_offers_neither_deepseek_nor_opencode() {
        let h = build(launch_replies(false), true, true);
        let mut windows = local();
        windows.platform = Some("windows".into());
        h.set_fleet(vec![windows, peer("studio", "live", 0)]);
        let here = h
            .screens
            .launch_accounts(&ComputerId::local(), false, None)
            .unwrap();
        let providers: Vec<&str> = here.groups.iter().map(|g| g.provider.as_str()).collect();
        assert_eq!(providers, ["codex", "claude"]);
        assert_eq!(
            here.profiles.len(),
            4,
            "the raw rows stay for the OpenCode fields"
        );
        let studio = h
            .screens
            .launch_accounts(&ComputerId::from("studio"), false, None)
            .unwrap();
        assert!(studio.groups.iter().any(|g| g.provider == "dsh"));
        assert!(h
            .fake
            .calls()
            .iter()
            .all(|call| !call.contains("inspect native-opencode")
                && !call.starts_with("account inspect native-dsh")));
    }

    // emptyComputerCatalogOffersOnlyShell.
    #[test]
    fn an_empty_catalog_gives_no_rows_and_reads_no_limits() {
        let h = build(
            json!({"account ls": reply(&json!({"profiles": [], "revision": "r"}))}),
            true,
            true,
        );
        let view = h
            .screens
            .launch_accounts(&ComputerId::local(), false, None)
            .unwrap();
        assert!(view.loaded && view.groups.is_empty() && view.profiles.is_empty());
        std::thread::sleep(Duration::from_millis(100));
        assert!(h.fake.calls().iter().all(|call| !call.contains("inspect")));
    }

    // NewSessionDialog::loadAccounts (865-874): every open, computer change
    // and Refresh reads the catalog; a running read is shared.
    #[test]
    fn every_open_reads_the_catalog_and_shares_a_running_one() {
        let h = harness(launch_replies(false), true);
        let local_id = ComputerId::local();
        let runs = h.screens.launch_start(&local_id, false, None);
        assert_eq!(runs.len(), 1);
        h.screens.run(runs);
        assert!(
            h.screens.launch_start(&local_id, false, None).is_empty(),
            "the running read is shared"
        );
        assert!(h.screens.launch_start(&local_id, true, None).is_empty());
        h.drain();
        assert_eq!(h.count("account ls"), 1, "no follow-up of a shared read");
        let runs = h.screens.launch_start(&local_id, false, None);
        assert_eq!(runs.len(), 1, "a moment later a new dialog reads again");
        h.screens.run(runs);
        h.drain();
        let runs = h.screens.launch_start(&local_id, true, None);
        assert_eq!(runs.len(), 1, "and so does Refresh");
        h.screens.run(runs);
        h.drain();
        assert_eq!(h.count("account ls"), 3);
        let usage = h.screens.launch_usage(&local_id, false);
        assert_eq!(usage.len(), 2, "two inspects at once");
        h.screens.run(usage);
        h.drain();
        assert_eq!(h.count("account inspect work --refresh"), 1);
        assert!(
            h.screens.launch_usage(&local_id, false).is_empty(),
            "limits checked within five minutes"
        );
        assert_eq!(
            h.screens.launch_usage(&local_id, true).len(),
            2,
            "Refresh forces them"
        );
        h.drain();
        // Rows only read nothing.
        h.screens.launch_rows(&local_id);
        assert_eq!(h.queued(), 0);
        // A computer the Accounts page does not list is read for the dialog too (865-874).
        let gone = ComputerId::from("gone");
        assert_eq!(h.screens.launch_start(&gone, false, None).len(), 1);
        assert!(
            !h.screens.launch_rows(&gone).loaded,
            "until its reply is filed"
        );
    }

    // accountsReady (380-388) asks for limits whenever the catalog arrives:
    // after the waiting `launch_accounts` stopped waiting, the filed reply
    // asks itself; while one waits, it asks on waking. Either way once.
    #[test]
    fn a_catalog_filed_after_the_wait_asks_for_its_limits_once() {
        let inspects = |h: &Harness| -> Vec<usize> {
            ["work", "claude", "native-dsh", "native-opencode"]
                .iter()
                .map(|id| h.count(&format!("account inspect {id} --refresh")))
                .collect()
        };
        let here = ComputerId::local();
        // Late: the caller gave up before the reply was filed.
        let h = harness(launch_replies(false), true);
        let runs = h.screens.launch_start(&here, false, None);
        h.screens.run(runs);
        assert!(
            h.screens.launch_waited(&here, false).is_empty(),
            "no catalog yet, no limits"
        );
        h.drain();
        assert_eq!(
            inspects(&h),
            [1, 1, 1, 1],
            "the late catalog asked for its limits"
        );
        assert_eq!(h.screens.launch_rows(&here).groups.len(), 4);
        h.drain();
        assert_eq!(inspects(&h), [1, 1, 1, 1], "once");
        // Direct: the reply is filed while the caller waits; it asks on waking.
        let h = harness(launch_replies(false), true);
        let runs = h.screens.launch_start(&here, false, None);
        h.screens.run(runs);
        h.drain();
        assert_eq!(
            inspects(&h),
            [0, 0, 0, 0],
            "the waiting caller asks, not the reply"
        );
        let runs = h.screens.launch_waited(&here, false);
        h.screens.run(runs);
        h.drain();
        assert_eq!(inspects(&h), [1, 1, 1, 1]);
        // A late catalog of a Refresh forces its limits.
        let runs = h.screens.launch_start(&here, true, None);
        h.screens.run(runs);
        assert!(
            h.screens.launch_waited(&here, true).is_empty(),
            "the earlier catalog is not this ask's answer"
        );
        h.drain();
        assert_eq!(
            inspects(&h),
            [2, 2, 2, 2],
            "the late Refresh catalog forced them, once"
        );
    }

    // Qt's dialog reads the selected computer directly (865-874): one the
    // Accounts page does not list is read, kept while the dialog watches it
    // and forgotten after; one hgs cannot read answers with its error.
    #[test]
    fn a_computer_outside_the_accounts_hosts_is_read_for_the_dialog() {
        let mut replies = launch_replies(false);
        replies["@lab account ls"] = reply(&launch_catalog(&[(
            "lab-claude",
            "claude",
            "lab@example.invalid",
        )]));
        replies["@gone account ls"] = json!({"stderr": "hgs: Unknown machine gone.\n", "exit": 2});
        let h = build(replies, true, true);
        let lab = ComputerId::from("lab");
        let view = h.screens.launch_accounts(&lab, false, None).unwrap();
        assert!(view.loaded && view.error.is_none(), "{view:?}");
        assert_eq!(members(&view), [vec!["lab-claude"]]);
        h.screens.tick();
        assert_eq!(
            members(&h.screens.launch_rows(&lab)),
            [vec!["lab-claude"]],
            "the hub's step keeps it while the dialog watches it"
        );
        assert_eq!(h.count("@lab account ls"), 1, "no warm-up read of its own");
        std::thread::sleep(Duration::from_millis(100));
        assert!(
            h.fake
                .calls()
                .iter()
                .all(|call| !call.starts_with("@lab account inspect")),
            "a computer without a live stream reads no limits"
        );
        let gone = h
            .screens
            .launch_accounts(&ComputerId::from("gone"), false, None)
            .unwrap();
        assert!(gone.loaded && gone.groups.is_empty());
        assert_eq!(
            gone.error.as_deref(),
            Some("Could not load accounts: hgs: Unknown machine gone.")
        );
        h.screens.launch_close();
        h.screens.tick();
        let hub = h.screens.lock();
        assert!(
            !hub.accounts.catalogs.contains_key(&lab)
                && !hub
                    .accounts
                    .catalogs
                    .contains_key(&ComputerId::from("gone")),
            "forgotten once the dialog closed"
        );
    }

    // Review: updateAccounts groups only when the catalog arrives (879-883);
    // later limits refresh those rows (refreshUsage 990-1000).
    #[test]
    fn later_limits_never_regroup_the_dialogs_rows_until_the_next_catalog() {
        let rows = [
            ("claude-a", "claude", "a@example.invalid"),
            ("claude-b", "claude", "b@example.invalid"),
        ];
        let mut replies = json!({"account ls": reply(&launch_catalog(&rows))});
        for (id, provider, _) in rows {
            replies[format!("account inspect {id} --refresh")] =
                inspect(id, provider, "same@example.invalid");
        }
        let h = build(replies, true, true);
        let local_id = ComputerId::local();
        let first = h.screens.launch_accounts(&local_id, false, None).unwrap();
        assert_eq!(members(&first), [vec!["claude-a"], vec!["claude-b"]]);
        eventually("both limits arrived", || {
            h.screens
                .launch_rows(&local_id)
                .groups
                .iter()
                .all(|g| g.usage.identity.email == "same@example.invalid")
        });
        let later = h.screens.launch_rows(&local_id);
        assert_eq!(
            members(&later),
            [vec!["claude-a"], vec!["claude-b"]],
            "still two rows"
        );
        assert_eq!(later.groups[0].key, first.groups[0].key);
        assert_eq!(later.groups[1].members[0].usage.checked_at, Some(200.0));
        // The next catalog groups them as one account.
        let joined = h.screens.launch_accounts(&local_id, true, None).unwrap();
        assert_eq!(members(&joined), [vec!["claude-a", "claude-b"]]);
    }

    // loadAccounts clears the rows for another computer (869-871); a failure
    // reads "Could not load accounts: %1" with failureDetail, and Refresh
    // keeps the rows it had.
    #[test]
    fn a_computer_change_clears_the_rows_and_a_failure_reads_as_qt() {
        let h = build(launch_replies(false), true, true);
        h.set_fleet(vec![local(), peer("studio", "live", 0)]);
        let studio = ComputerId::from("studio");
        let here = ComputerId::local();
        assert_eq!(
            h.screens
                .launch_accounts(&studio, false, None)
                .unwrap()
                .groups
                .len(),
            2
        );
        h.fake.set_reply("@studio account ls", json!({"exit": 3}));
        h.screens.launch_accounts(&here, false, None).unwrap();
        let failed = h.screens.launch_accounts(&studio, false, None).unwrap();
        assert!(failed.loaded);
        assert_eq!(
            failed.error.as_deref(),
            Some("Could not load accounts: exit code 3")
        );
        assert!(
            failed.groups.is_empty() && failed.profiles.is_empty(),
            "nothing of the earlier catalog"
        );
        assert!(
            h.screens.launch_rows(&here).groups.is_empty(),
            "another computer has no rows now"
        );
        let kept = h.screens.launch_accounts(&here, false, None).unwrap();
        assert_eq!(kept.groups.len(), 4);
        h.fake.set_reply("account ls", json!({"stdout": "[]"}));
        let refreshed = h.screens.launch_accounts(&here, true, None).unwrap();
        assert_eq!(
            refreshed.error.as_deref(),
            Some("Could not load accounts: Invalid account catalog response")
        );
        assert_eq!(refreshed.groups.len(), 4, "Refresh keeps the rows it had");
        h.fake.set_reply(
            "account ls",
            json!({"stderr": "hgs: account catalog busy\n", "exit": 1}),
        );
        let busy = h.screens.launch_accounts(&here, true, None).unwrap();
        assert_eq!(
            busy.error.as_deref(),
            Some("Could not load accounts: hgs: account catalog busy")
        );
        h.fake
            .set_reply("account ls", launch_replies(false)["account ls"].clone());
        assert_eq!(
            h.screens.launch_accounts(&here, true, None).unwrap().error,
            None
        );
    }

    #[test]
    fn the_dialogs_computer_hears_of_its_replies_until_it_closes() {
        let h = harness(launch_replies(false), true);
        let heard = Arc::new(Mutex::new(Vec::new()));
        {
            let heard = Arc::clone(&heard);
            h.screens
                .on_launch_change(Arc::new(move |computer: &ComputerId| {
                    heard.lock().unwrap().push(computer.clone());
                }));
        }
        h.set_fleet(vec![local(), peer("studio", "live", 0)]);
        let runs = h
            .screens
            .launch_start(&ComputerId::from("studio"), false, None);
        h.screens.run(runs);
        h.drain();
        let runs = h.screens.launch_usage(&ComputerId::from("studio"), false);
        h.screens.run(runs);
        h.drain();
        let got = heard.lock().unwrap().clone();
        assert!(got.len() >= 3, "the catalog and both inspects: {got:?}");
        assert!(
            got.iter()
                .all(|computer| computer == &ComputerId::from("studio")),
            "only the dialog's computer"
        );
        // Closed: its later replies tell nobody.
        let runs = h.screens.launch_usage(&ComputerId::from("studio"), true);
        h.screens.launch_close();
        h.screens.run(runs);
        h.drain();
        assert_eq!(heard.lock().unwrap().len(), got.len());
        assert!(!h.screens.launch_rows(&ComputerId::from("studio")).loaded);
    }

    // NewSessionDialog 221-222: the requested agent joins the dialog's agents.
    #[test]
    fn a_requested_agent_joins_the_provider_order() {
        let rows = [
            ("alpha-a", "alpha", "a@example.invalid"),
            ("zeta-a", "zeta", "z@example.invalid"),
            ("work", "codex", "w@example.invalid"),
        ];
        let h = build(
            json!({"account ls": reply(&launch_catalog(&rows))}),
            true,
            true,
        );
        let order = |agent: Option<&str>| -> Vec<String> {
            let view = h
                .screens
                .launch_accounts(&ComputerId::local(), false, agent)
                .unwrap();
            view.groups.iter().map(|g| g.provider.clone()).collect()
        };
        assert_eq!(order(Some("zeta")), ["codex", "zeta", "alpha"]);
        assert_eq!(order(None), ["codex", "alpha", "zeta"]);
        assert_eq!(
            order(Some("sh")),
            ["codex", "alpha", "zeta"],
            "the shell is never a provider"
        );
    }

    #[test]
    fn a_replay_without_hgs_reads_no_launch_accounts() {
        let h = harness(json!({}), false);
        assert_eq!(
            h.screens.launch_accounts(&ComputerId::local(), true, None),
            Err(REPLAY_NEEDS_HGS.to_owned())
        );
        assert_eq!(h.queued(), 0);
        assert!(h.fake.calls().is_empty());
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
