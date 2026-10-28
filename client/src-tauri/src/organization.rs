//! The shared project organization on the Tauri side (zerus-5wmh.31, .18).
//! This computer's `organization` topic feeds the core's write controller
//! (`swarm::sync::Controller`). The presented organization goes to the pump's
//! grouping, which sends the webview each row's project and the headers.
//! Edits run as one `hgs swarm apply` at a time on this module's thread,
//! which also saves the local layout. A failed apply stays pending in the
//! controller, and this thread sends it again on the next tick: 5 s later, or
//! when the topic's next state arrives (SwarmController.cpp:67-73, 96-101).
//! Pending edits are kept in a draft file, written before their apply and
//! removed once they settled; a restart loads it and sends it after the first
//! state, as Qt does (SessionsWindow.cpp:2496, 1661; SwarmController.cpp:76-89).
//! A draft file that cannot be used is never applied: it is moved aside
//! before any new draft is written. The webview never receives the
//! organization itself.
//!
//! Locks: the hub's, then the pump's (a publish hands the pump the presented
//! organization). The snapshot source, `hgs`, the draft and layout files,
//! the edits' answers and the Projects callback all run without the hub's
//! lock, and the worker is woken after it is released.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;
use tauri::State;
use zerus_client_core::reads::{Read, ReadError};
use zerus_client_core::swarm::draft::{self, Draft, DraftWrite, Load};
use zerus_client_core::swarm::edit::Intent;
use zerus_client_core::swarm::grouping::{by_name, SyncView};
use zerus_client_core::swarm::sync::{Controller, Due, EditOutcome, Observation, Ticket, STOPPED};
use zerus_client_core::{run_limited, Change, Exit, OrganizationSnapshot, OrganizationUpdate};

use crate::actions::REPLAY_NEEDS_HGS;
use crate::pump::Pump;

/// The layout is saved at most this often.
const SAVE_EVERY: Duration = Duration::from_secs(1);
/// The longest the worker waits, in milliseconds, when nothing is due sooner.
const IDLE_MS: u64 = 1_000;
/// A failing write logs at most one line in this long.
const LOG_EVERY: Duration = Duration::from_secs(60);

/// The base and versions of the latest state: `Fleet::organization_snapshot`
/// live, the replay's own catalog in a replay.
pub type SnapshotSource = Arc<dyn Fn() -> Option<OrganizationSnapshot> + Send + Sync>;
/// Hands the presented `swarm get` object, the default project and the
/// controller's status line to the Projects screen (Task 9), as Qt's
/// `statusChanged` reaches its Projects page (SessionsWindow.cpp:1653).
/// It runs without the hub's lock, one call at a time, and a call never
/// follows a newer one; it must not call back into [`Organization`].
pub type Presented = Arc<dyn Fn(Arc<Value>, String, (String, bool)) + Send + Sync>;

/// One call of [`Presented`], made once the hub's lock is released.
type Announce = (Presented, Arc<Value>, String, (String, bool));

/// The layout file in the app data directory; a replay keeps its own.
pub fn layout_file(replay: bool) -> &'static str {
    if replay {
        "organization-layout-replay.json"
    } else {
        "organization-layout.json"
    }
}

/// The pending draft beside the layout; a replay keeps its own.
pub fn draft_file(replay: bool) -> &'static str {
    if replay {
        draft::REPLAY_FILE
    } else {
        draft::FILE
    }
}

#[derive(Default)]
struct Hub {
    sync: Controller,
    source: Option<SnapshotSource>,
    waiting: HashMap<Ticket, SyncSender<EditOutcome>>,
    presented: Option<Presented>,
    /// The revision and status line Projects last received.
    shown: Option<(u64, String, bool)>,
    /// Settled edits' answers, sent once the lock is released.
    answers: Vec<(SyncSender<EditOutcome>, EditOutcome)>,
    /// The Projects call, numbered, made once the lock is released.
    announce: Option<(u64, Announce)>,
    announcements: u64,
    /// The worker is gone: edits answer `Kept(STOPPED)`.
    stopped: bool,
}

struct Shared {
    hub: Mutex<Hub>,
    wake: Condvar,
    /// The last Projects call made, so a late one never overtakes a newer one.
    announced: Mutex<u64>,
    pump: Pump,
    hgs: PathBuf,
    allowed: bool,
    layout: PathBuf,
    draft: PathBuf,
    clock: Instant,
    /// Passes the worker finished, so a test can tell it ran.
    #[cfg(test)]
    parked: std::sync::atomic::AtomicUsize,
}

/// The managed state of the organization commands; clones share it. The
/// worker thread runs while a clone is left.
#[derive(Clone)]
pub struct Organization(Arc<Shared>);

impl Organization {
    pub fn new(
        pump: Pump,
        hgs: PathBuf,
        allowed: bool,
        layout: PathBuf,
        draft_path: PathBuf,
    ) -> Self {
        if let Ok(bytes) = std::fs::read(&layout) {
            pump.load_layout(&bytes);
        }
        // A blocked replay sends nothing, so it takes up no draft either.
        let (saved, aside) = if allowed {
            saved_draft(&draft_path)
        } else {
            (None, false)
        };
        let organization = Self(Arc::new(Shared {
            hub: Mutex::default(),
            wake: Condvar::new(),
            announced: Mutex::new(0),
            pump,
            hgs,
            allowed,
            layout,
            draft: draft_path,
            clock: Instant::now(),
            #[cfg(test)]
            parked: std::sync::atomic::AtomicUsize::new(0),
        }));
        {
            let mut hub = organization.lock();
            // The last run's pending edits (Qt's start, SwarmController.cpp:76-89):
            // shown at once, sent after the first state.
            if let Some(saved) = saved {
                hub.sync.restore(saved);
            }
            organization.publish(&mut hub);
            organization.release(hub);
        }
        organization.spawn(aside);
        organization
    }

    fn lock(&self) -> MutexGuard<'_, Hub> {
        self.0.hub.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn now(&self) -> u64 {
        u64::try_from(self.0.clock.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    pub fn set_source(&self, source: SnapshotSource) {
        self.lock().source = Some(source);
    }

    #[allow(
        dead_code,
        reason = "Task 9 hands the Projects screen the presented organization"
    )]
    pub fn set_presented(&self, presented: Presented) {
        let mut hub = self.lock();
        hub.presented = Some(presented);
        hub.shown = None;
        self.publish(&mut hub);
        self.release(hub);
    }

    /// This computer's organization changes, from the drain thread.
    pub fn observe(&self, change: &Change) {
        let Change::Organization { computer, update } = change else {
            return;
        };
        if !computer.is_local() {
            return;
        }
        let mut hub = match update {
            OrganizationUpdate::State {
                value,
                digest,
                membership,
            } => {
                // Asked without the hub's lock: the source takes its own.
                let source = self.lock().source.clone();
                let Some(snapshot) = source.and_then(|source| source()) else {
                    return;
                };
                // Base and versions of one digest; a newer state is on its way otherwise.
                if snapshot.digest != *digest {
                    return;
                }
                let observation = Observation {
                    digest: digest.clone(),
                    value: Arc::clone(value),
                    versions: snapshot.versions,
                    membership: Arc::clone(membership),
                };
                let mut hub = self.lock();
                let now = self.now();
                hub.sync.observe(observation, now);
                hub
            }
            OrganizationUpdate::Failed { message, .. } => {
                let mut hub = self.lock();
                hub.sync.source_failed(message);
                hub
            }
            OrganizationUpdate::Ended => {
                let mut hub = self.lock();
                hub.sync.source_ended();
                hub
            }
        };
        // Every update, since a state can change readiness without a new
        // revision (the first one after a restored draft, or after an end).
        self.publish(&mut hub);
        self.release(hub);
        // After a failed apply, a state is the tick that sends it again.
        self.0.wake.notify_all();
    }

    fn view(&self, hub: &Hub) -> SyncView {
        let mut view = hub.sync.view();
        if !self.0.allowed {
            view.ready = false;
            view.reason = Some(REPLAY_NEEDS_HGS.into());
        }
        view
    }

    /// Hands the pump what the list shows, and queues the Projects call when
    /// the revision or the status line changed.
    fn publish(&self, hub: &mut Hub) {
        let view = self.view(hub);
        let shown = (hub.sync.revision(), view.status.clone(), view.status_error);
        match hub.sync.presented() {
            Some((organization, membership)) => {
                self.0
                    .pump
                    .present(organization, membership, hub.sync.revision(), view)
            }
            None => self.0.pump.set_sync(view),
        }
        if hub.shown.as_ref() == Some(&shown) {
            return;
        }
        let (Some(presented), Some(value)) = (hub.presented.clone(), hub.sync.presented_value())
        else {
            return;
        };
        let status = (shown.1.clone(), shown.2);
        hub.announcements += 1;
        hub.announce = Some((
            hub.announcements,
            (presented, value, self.0.pump.default_project(), status),
        ));
        hub.shown = Some(shown);
    }

    /// Drops the lock, then answers the settled edits and makes the queued
    /// Projects call.
    fn release(&self, mut hub: MutexGuard<'_, Hub>) {
        let answers = std::mem::take(&mut hub.answers);
        let announce = hub.announce.take();
        drop(hub);
        for (sender, outcome) in answers {
            let _ = sender.send(outcome);
        }
        if let Some((number, (presented, value, default, status))) = announce {
            let mut announced = self
                .0
                .announced
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if number > *announced {
                *announced = number;
                presented(value, default, status);
            }
        }
    }

    /// Runs one edit; answers once the first apply that carries it ended.
    /// A failed one answers `Kept`, and the edit stays pending for the next tick.
    pub fn submit(&self, intent: Intent) -> EditOutcome {
        if !self.0.allowed {
            return EditOutcome::Refused {
                detail: REPLAY_NEEDS_HGS.into(),
            };
        }
        let answer = {
            let mut hub = self.lock();
            if hub.stopped {
                return EditOutcome::Kept {
                    detail: STOPPED.into(),
                };
            }
            let ticket = match hub.sync.edit(&intent, self.now()) {
                Ok(ticket) => ticket,
                Err(detail) => return EditOutcome::Refused { detail },
            };
            self.0.pump.edited(&intent);
            let (sender, receiver) = sync_channel(1);
            hub.waiting.insert(ticket, sender);
            self.publish(&mut hub);
            self.release(hub);
            receiver
        };
        self.0.wake.notify_all();
        answer.recv().unwrap_or(EditOutcome::Kept {
            detail: STOPPED.into(),
        })
    }

    fn settle(hub: &mut Hub, settled: Vec<(Ticket, EditOutcome)>) {
        for (ticket, outcome) in settled {
            if let Some(sender) = hub.waiting.remove(&ticket) {
                hub.answers.push((sender, outcome));
            }
        }
    }

    fn spawn(&self, aside: bool) {
        let shared = Arc::downgrade(&self.0);
        std::thread::Builder::new()
            .name("zerus-organization".into())
            .spawn(move || {
                let _stop = Stop(shared.clone());
                let mut worker = Worker::new(aside);
                // Each pass waits at most a second, so the thread ends soon
                // after the last handle.
                while let Some(shared) = shared.upgrade() {
                    Organization(shared).pass(&mut worker);
                }
            })
            .expect("the organization thread starts");
    }

    /// One pass of the worker: renames, the draft file, a due apply, the
    /// layout, then a wait until the next flush or tick (at most 1 s), or
    /// until a state or an edit wakes it.
    fn pass(&self, worker: &mut Worker) {
        let mut hub = self.lock();
        self.renames(&mut hub);
        hub = self.save_draft(hub, worker);
        match hub.sync.due(self.now()) {
            Due::Apply(invocation) => {
                // The list shows the apply running while `hgs` runs without the lock.
                self.publish(&mut hub);
                self.release(hub);
                let exit = run_limited(&self.0.hgs, &invocation, &[], Read::Swarm.limit())
                    .unwrap_or_else(|too| Exit::Abnormal {
                        stderr: ReadError::TooLarge { limit: too.cap }.to_string(),
                    });
                let mut hub = self.lock();
                // A failure keeps the edits; the controller arms the next tick.
                let settled = hub.sync.finish(exit, self.now());
                Self::settle(&mut hub, settled);
                self.publish(&mut hub);
                self.release(hub);
                return;
            }
            // Edits that changed nothing shared, or (no tickets) a state the
            // guard held back adopted at its end: a new revision either way.
            Due::Settled(settled) => {
                Self::settle(&mut hub, settled);
                self.publish(&mut hub);
                self.release(hub);
                return;
            }
            Due::Nothing => {}
        }
        self.release(hub);
        if worker.saved.elapsed() >= SAVE_EVERY {
            worker.save_layout(&self.0.pump, &self.0.layout);
        }
        let hub = self.lock();
        #[cfg(test)]
        self.0
            .parked
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let now = self.now();
        let wait = hub
            .sync
            .next_due()
            .map_or(IDLE_MS, |at| at.saturating_sub(now))
            .clamp(1, IDLE_MS);
        drop(
            self.0
                .wake
                .wait_timeout(hub, Duration::from_millis(wait))
                .unwrap_or_else(PoisonError::into_inner),
        );
    }

    /// Renames the list saw become `Renamed` edits while edits are accepted
    /// (renameSession). Until then they stay with the grouping, and a late
    /// one changes nothing; a blocked replay, which edits nothing, drops them.
    fn renames(&self, hub: &mut Hub) {
        if !self.0.allowed {
            let _ = self.0.pump.take_renames();
            return;
        }
        if !hub.sync.ready() {
            return;
        }
        let mut renamed = false;
        for (from, to) in self.0.pump.take_renames() {
            let intent = Intent::Renamed { from, to };
            if hub.sync.edit(&intent, self.now()).is_ok() {
                self.0.pump.edited(&intent);
                renamed = true;
            }
        }
        if renamed {
            self.publish(hub);
        }
    }

    /// Writes the draft file while it lags the pending edits, without the
    /// hub's lock: an edit is on disk before the apply that carries it, and
    /// the file goes once the edits settled (SessionsWindow.cpp:2496). Only
    /// a write that succeeded is confirmed; a failed one is offered again on
    /// the next pass.
    fn save_draft<'a>(
        &'a self,
        mut hub: MutexGuard<'a, Hub>,
        worker: &mut Worker,
    ) -> MutexGuard<'a, Hub> {
        while let Some((change, write)) = hub.sync.take_draft() {
            self.release(hub);
            let written = worker.store_draft(&self.0.draft, &write);
            hub = self.lock();
            if !written {
                break;
            }
            hub.sync.draft_written(change);
        }
        hub
    }
}

/// The draft of the last run. One that cannot be used is never applied: the
/// reason goes to the log and the file aside before any new draft replaces
/// it. The flag says it is still in place, so the worker moves it first.
fn saved_draft(path: &Path) -> (Option<Draft>, bool) {
    match draft::load(path) {
        Load::Missing => (None, false),
        Load::Loaded(draft) => (Some(draft), false),
        Load::Dropped(reason) => {
            eprintln!("zerus-client: the saved project changes were not applied: {reason}");
            match set_aside(path) {
                Ok(()) => (None, false),
                Err(error) => {
                    eprintln!("zerus-client: the unusable draft could not be moved aside: {error}");
                    (None, true)
                }
            }
        }
    }
}

/// Moves an unusable draft file aside (`draft::set_aside`); one already
/// gone is fine.
fn set_aside(path: &Path) -> io::Result<()> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    match draft::set_aside(path, seconds) {
        Ok(aside) => {
            eprintln!(
                "zerus-client: the unusable draft was moved to {}",
                aside.display()
            );
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Lets a repeating failure through at most once per [`LOG_EVERY`].
#[derive(Default)]
struct Throttle(Option<Instant>);

impl Throttle {
    fn allow(&mut self) -> bool {
        let allowed = self.0.is_none_or(|at| at.elapsed() >= LOG_EVERY);
        if allowed {
            self.0 = Some(Instant::now());
        }
        allowed
    }
}

/// The worker's state between passes.
struct Worker {
    /// An unusable draft is still in place; no draft is written before it moved.
    aside: bool,
    saved: Instant,
    /// A layout whose write failed, written again with the next save.
    layout: Option<Vec<u8>>,
    draft_log: Throttle,
    layout_log: Throttle,
}

impl Worker {
    fn new(aside: bool) -> Self {
        Self {
            aside,
            saved: Instant::now(),
            layout: None,
            draft_log: Throttle::default(),
            layout_log: Throttle::default(),
        }
    }

    /// Whether the draft file now holds `write`.
    fn store_draft(&mut self, path: &Path, write: &DraftWrite) -> bool {
        if self.aside {
            if let Err(error) = set_aside(path) {
                if self.draft_log.allow() {
                    eprintln!("zerus-client: the unusable draft could not be moved aside: {error}");
                }
                return false;
            }
            self.aside = false;
        }
        match draft::store(path, write) {
            Ok(()) => true,
            Err(error) => {
                if self.draft_log.allow() {
                    eprintln!("zerus-client: the pending project changes were not saved: {error}");
                }
                false
            }
        }
    }

    /// Saves a changed layout through a temporary file and a rename.
    fn save_layout(&mut self, pump: &Pump, path: &Path) {
        self.saved = Instant::now();
        let Some(bytes) = pump.take_layout().or_else(|| self.layout.take()) else {
            return;
        };
        if let Err(error) = draft::write(path, &bytes) {
            if self.layout_log.allow() {
                eprintln!("zerus-client: the project layout was not saved: {error}");
            }
            self.layout = Some(bytes);
        }
    }
}

/// Runs when the worker ends. After a panic the hub outlives it: the edits
/// waiting on it, and later ones, answer `Kept(STOPPED)`.
struct Stop(Weak<Shared>);

impl Drop for Stop {
    fn drop(&mut self) {
        let Some(shared) = self.0.upgrade() else {
            return;
        };
        let (waiting, answers) = {
            let mut hub = shared.hub.lock().unwrap_or_else(PoisonError::into_inner);
            hub.stopped = true;
            (
                std::mem::take(&mut hub.waiting),
                std::mem::take(&mut hub.answers),
            )
        };
        for (sender, outcome) in answers {
            let _ = sender.send(outcome);
        }
        for sender in waiting.into_values() {
            let _ = sender.send(EditOutcome::Kept {
                detail: STOPPED.into(),
            });
        }
    }
}

async fn edit(
    state: State<'_, Organization>,
    intent: impl FnOnce(&Organization) -> Intent + Send + 'static,
) -> Result<EditOutcome, String> {
    let organization = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let intent = intent(&organization);
        organization.submit(intent)
    })
    .await
    .map_err(|error| error.to_string())
}

/// Move to project for one session or a selection: one `swarm apply` with
/// every member, from one snapshot. Members are `"<host>\n<session name>"`;
/// `before` places them before that member (a drop).
#[tauri::command]
pub async fn move_to_project(
    state: State<'_, Organization>,
    members: Vec<String>,
    project: String,
    before: Option<String>,
) -> Result<EditOutcome, String> {
    edit(state, move |_| Intent::Move {
        members,
        project,
        before,
    })
    .await
}

/// New project…, with the sessions it takes (none from the list's menu).
#[tauri::command]
pub async fn create_project(
    state: State<'_, Organization>,
    name: String,
    members: Vec<String>,
) -> Result<EditOutcome, String> {
    edit(state, move |_| Intent::Create {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        members,
    })
    .await
}

#[tauri::command]
pub async fn rename_project(
    state: State<'_, Organization>,
    project: String,
    name: String,
) -> Result<EditOutcome, String> {
    edit(state, move |_| Intent::Rename { project, name }).await
}

/// Delete project…: its members and the rows it lists go to `destination`.
#[tauri::command]
pub async fn delete_project(
    state: State<'_, Organization>,
    project: String,
    destination: String,
) -> Result<EditOutcome, String> {
    edit(state, move |organization| {
        let members = organization
            .0
            .pump
            .members_of(&project)
            .into_iter()
            .map(|(member, _)| member)
            .collect();
        Intent::Delete {
            project,
            destination,
            members,
        }
    })
    .await
}

#[tauri::command]
pub async fn move_project(
    state: State<'_, Organization>,
    project: String,
    before: Option<String>,
) -> Result<EditOutcome, String> {
    edit(state, move |_| Intent::Reorder { project, before }).await
}

/// Sort sessions by name: the project's listed rows, by title.
#[tauri::command]
pub async fn sort_project(
    state: State<'_, Organization>,
    project: String,
) -> Result<EditOutcome, String> {
    edit(state, move |organization| {
        let members = by_name(organization.0.pump.members_of(&project));
        Intent::Sort { project, members }
    })
    .await
}

/// Expand or collapse a project: this device's layout, no `hgs`.
#[tauri::command]
pub fn set_project_collapsed(state: State<'_, Organization>, project: String, collapsed: bool) {
    state.0.pump.set_collapsed(&project, collapsed);
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use zerus_client_core::swarm::edit::shared;
    use zerus_client_core::{ComputerId, ViewRow};

    const R: &str = "workstation\nclaude/sample-project/review";
    const R2: &str = "workstation\nclaude/sample-project/review-2";
    static NEXT: AtomicUsize = AtomicUsize::new(0);

    fn full(digest: &str) -> Value {
        json!({"schema": 1, "node_id": "node-1", "digest": digest, "initialized": true, "machines": [], "peers": {}, "conflicts": [],
               "organization": {"version": 2, "default_project": "ungrouped", "projects": [
                   {"id": "alpha", "name": "Alpha", "color": "#3fb950", "accessible": true, "folders": [], "sessions": [R]},
                   {"id": "ungrouped", "name": "Inbox", "color": "#8d9baa", "accessible": true, "folders": [], "sessions": []}]},
               "versions": {}})
    }

    /// A scratch folder with the core's fake hgs, its `swarm apply` answering
    /// `apply` in turn. The fake runs as a child of the tests, so it holds
    /// `crate::PROCESS_TREE` shared.
    struct Fake {
        dir: PathBuf,
        hgs: PathBuf,
        _tree: std::sync::RwLockReadGuard<'static, ()>,
    }

    impl Fake {
        fn new(apply: Value) -> Self {
            let tree = crate::PROCESS_TREE
                .read()
                .unwrap_or_else(PoisonError::into_inner);
            let dir = std::env::temp_dir().join(format!(
                "zerus-organization-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let hgs = dir.join("hgs");
            // A symlink to the checked-in script, never a copy (see
            // core/tests/actions.rs): a new executable written while another
            // test thread forks fails to exec with ETXTBSY.
            std::os::unix::fs::symlink(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../core/tests/support/fake_hgs_swarm.py"),
                &hgs,
            )
            .unwrap();
            let config = json!({"get": [{"stdout": full("d1")}], "apply": apply});
            std::fs::write(
                dir.join("config.json"),
                serde_json::to_vec(&config).unwrap(),
            )
            .unwrap();
            Self {
                dir,
                hgs,
                _tree: tree,
            }
        }

        /// Every apply answers with the catalog at `d2`.
        fn landing() -> Self {
            Self::new(json!([{"stdout": full("d2")}]))
        }

        fn layout(&self) -> PathBuf {
            self.dir.join("layout.json")
        }

        fn draft(&self) -> PathBuf {
            self.dir.join("draft.json")
        }

        fn organization(&self, pump: &Pump, allowed: bool) -> Organization {
            Organization::new(
                pump.clone(),
                self.hgs.clone(),
                allowed,
                self.layout(),
                self.draft(),
            )
        }

        /// The `swarm apply` lines of the fake's log.
        fn applies(&self) -> Vec<String> {
            std::fs::read_to_string(self.dir.join("log.ndjson"))
                .unwrap_or_default()
                .lines()
                .filter(|line| line.contains("\"apply\""))
                .map(str::to_owned)
                .collect()
        }

        /// The edits settled after `count` applies: the draft written before
        /// the first one is gone.
        fn settled_after(&self, count: usize) -> bool {
            self.applies().len() >= count && !self.draft().exists()
        }

        /// Drops the hub and waits for its worker to end, so nothing writes
        /// into the folder or runs the fake once it is removed.
        fn finish(self, organization: Organization) {
            let shared = Arc::downgrade(&organization.0);
            drop(organization);
            wait_until("the worker to end", || shared.strong_count() == 0);
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// Polls `done` every 20 ms for at most 10 s.
    fn wait_until(what: &str, done: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The passes the worker finished.
    fn parked(organization: &Organization) -> usize {
        organization.0.parked.load(Ordering::SeqCst)
    }

    /// The input of a logged `swarm apply`.
    fn input(line: &str) -> Value {
        let entry: Value = serde_json::from_str(line).unwrap();
        serde_json::from_str(entry["stdin"].as_str().unwrap()).unwrap()
    }

    fn state(digest: &str) -> Change {
        let value = full(digest);
        let mut without = value.clone();
        without.as_object_mut().unwrap().remove("versions");
        Change::Organization {
            computer: ComputerId::local(),
            update: OrganizationUpdate::State {
                value: Arc::new(without),
                digest: digest.into(),
                membership: Arc::new(zerus_client_core::swarm::members(&value["organization"])),
            },
        }
    }

    fn snapshot(digest: &'static str) -> SnapshotSource {
        Arc::new(move || {
            Some(OrganizationSnapshot {
                digest: digest.into(),
                base: full(digest)["organization"].clone(),
                versions: json!({}),
            })
        })
    }

    fn row(key: &str, name: &str) -> Change {
        Change::Upsert {
            computer: ComputerId::local(),
            key: key.into(),
            row: ViewRow {
                machine: "workstation".into(),
                name: name.into(),
                ..ViewRow::default()
            },
        }
    }

    fn move_to_inbox() -> Intent {
        Intent::Move {
            members: vec![R.into()],
            project: "ungrouped".into(),
            before: None,
        }
    }

    #[test]
    fn a_blocked_replay_refuses_edits_with_the_replay_text() {
        let fake = Fake::landing();
        let pump = Pump::default();
        let organization = fake.organization(&pump, false);
        organization.set_source(snapshot("d1"));
        organization.observe(&state("d1"));
        let outcome = organization.submit(move_to_inbox());
        assert_eq!(
            outcome,
            EditOutcome::Refused {
                detail: REPLAY_NEEDS_HGS.into()
            }
        );
        let grouping = pump.take(crate::pump::PARK).grouping.unwrap();
        assert!(grouping.loaded && !grouping.sync.ready);
        assert_eq!(grouping.sync.reason.as_deref(), Some(REPLAY_NEEDS_HGS));
        assert!(fake.applies().is_empty());
        fake.finish(organization);
    }

    // A replay without ZERUS_CLIENT_HGS would run the real CLI: it sends no draft.
    #[test]
    fn a_blocked_replay_never_sends_a_saved_draft() {
        let fake = Fake::landing();
        let base = full("d1")["organization"].clone();
        let mut desired = base.clone();
        desired["projects"][0]["sessions"] = json!([]);
        let saved = Draft::new(base, desired, serde_json::Map::new()).to_json();
        draft::write(&fake.draft(), &saved).unwrap();
        let pump = Pump::default();
        let organization = fake.organization(&pump, false);
        assert!(
            !pump
                .take(crate::pump::PARK)
                .grouping
                .is_some_and(|g| g.loaded),
            "the draft is not taken up, so the list shows none"
        );
        organization.set_source(snapshot("d1"));
        let passes = parked(&organization);
        organization.observe(&state("d1"));
        wait_until("a pass after the first state", || {
            parked(&organization) > passes
        });
        assert!(fake.applies().is_empty());
        assert_eq!(
            std::fs::read(fake.draft()).unwrap(),
            saved,
            "left as it was"
        );
        fake.finish(organization);
    }

    #[test]
    fn a_state_of_another_digest_than_the_snapshot_is_skipped() {
        let fake = Fake::landing();
        let pump = Pump::default();
        let organization = fake.organization(&pump, true);
        organization.set_source(snapshot("d2"));
        organization.observe(&state("d1"));
        assert!(
            !pump
                .take(crate::pump::PARK)
                .grouping
                .is_some_and(|g| g.loaded),
            "versions of another state are never paired"
        );
        organization.observe(&state("d2"));
        assert!(pump.take(crate::pump::PARK).grouping.unwrap().loaded);
        fake.finish(organization);
    }

    #[test]
    fn an_edit_runs_one_apply_and_answers_with_its_outcome() {
        let fake = Fake::landing();
        let pump = Pump::default();
        pump.push(row("run:1", "claude/sample-project/review"));
        let organization = fake.organization(&pump, true);
        organization.set_source(snapshot("d1"));
        organization.observe(&state("d1"));
        let outcome = organization.submit(move_to_inbox());
        assert_eq!(outcome, EditOutcome::Done { conflicts: 0 });
        assert_eq!(fake.applies().len(), 1, "nothing is pending once it landed");
        wait_until("the layout to be saved", || fake.layout().exists());
        wait_until("the draft to go", || fake.settled_after(1));
        assert_eq!(fake.applies().len(), 1);
        fake.finish(organization);
    }

    // SwarmController.cpp:67-73, 96-101: the edit stays, and the topic's next state sends it again.
    #[test]
    fn a_failed_apply_answers_kept_and_the_next_state_sends_it_once_more() {
        let refused =
            json!({"exit": 1, "stderr": "hgs: session belongs to more than one project\n"});
        let fake = Fake::new(json!([refused, {"stdout": full("d2")}]));
        let organization = fake.organization(&Pump::default(), true);
        organization.set_source(snapshot("d1"));
        organization.observe(&state("d1"));
        let outcome = organization.submit(move_to_inbox());
        assert_eq!(
            outcome,
            EditOutcome::Kept {
                detail: "Project sync unavailable: hgs: session belongs to more than one project"
                    .into()
            }
        );
        assert_eq!(fake.applies().len(), 1);
        assert!(fake.draft().exists(), "the kept edit stays on disk");
        organization.observe(&state("d1"));
        wait_until("the resend to land", || fake.settled_after(2));
        let log = fake.applies();
        assert_eq!(log.len(), 2, "one resend, and no more once it landed");
        assert_eq!(
            input(&log[0]),
            input(&log[1]),
            "the same base, desired and versions"
        );
        fake.finish(organization);
    }

    // Only a write that succeeded is confirmed (`Controller::draft_written`);
    // a failed one is offered again on a later pass, and the apply goes anyway.
    #[test]
    fn a_draft_that_could_not_be_written_is_written_on_a_later_pass() {
        let refused = json!({"exit": 1, "stderr": "hgs: the catalog is busy\n"});
        let fake = Fake::new(json!([refused]));
        let organization = fake.organization(&Pump::default(), true);
        // A folder in the draft's place once the hub started: every write fails.
        std::fs::create_dir(fake.draft()).unwrap();
        organization.set_source(snapshot("d1"));
        organization.observe(&state("d1"));
        let outcome = organization.submit(move_to_inbox());
        assert_eq!(
            outcome,
            EditOutcome::Kept {
                detail: "Project sync unavailable: hgs: the catalog is busy".into()
            }
        );
        assert!(fake.draft().is_dir(), "nothing was written");
        std::fs::remove_dir(fake.draft()).unwrap();
        wait_until("the draft to be written", || fake.draft().is_file());
        let Load::Loaded(saved) = draft::load(&fake.draft()) else {
            panic!("not a draft");
        };
        assert_eq!(saved.desired["projects"][1]["sessions"], json!([R]));
        fake.finish(organization);
    }

    // Qt loads its draft at start (SessionsWindow.cpp:275-276, 1661) and sends it
    // once its first read answered (SwarmController.cpp:76-89, 197, 203-205).
    #[test]
    fn a_saved_draft_waits_for_the_first_state_then_one_apply_sends_it() {
        let fake = Fake::landing();
        let base = full("d1")["organization"].clone();
        let mut desired = base.clone();
        desired["projects"][0]["sessions"] = json!([]);
        desired["projects"][1]["sessions"] = json!([R]);
        let versions = serde_json::Map::new();
        draft::write(
            &fake.draft(),
            &Draft::new(base.clone(), desired.clone(), versions.clone()).to_json(),
        )
        .unwrap();
        let pump = Pump::default();
        let organization = fake.organization(&pump, true);
        organization.set_source(snapshot("d1"));
        assert!(
            pump.take(crate::pump::PARK)
                .grouping
                .is_some_and(|g| g.loaded),
            "the list shows the draft before any state"
        );
        wait_until("a pass of the worker", || parked(&organization) >= 1);
        assert!(fake.applies().is_empty(), "nothing before the first state");
        assert!(fake.draft().exists());
        organization.observe(&state("d1"));
        wait_until("the draft to land", || fake.settled_after(1));
        let log = fake.applies();
        assert_eq!(log.len(), 1, "exactly one apply");
        assert_eq!(
            input(&log[0]),
            json!({"base": shared(&base), "desired": shared(&desired), "versions": versions}),
            "the saved draft"
        );
        fake.finish(organization);
    }

    #[test]
    fn an_unusable_draft_is_moved_aside_and_never_sent() {
        let fake = Fake::landing();
        std::fs::write(fake.draft(), br#"{"version": 1, "base": {"projects": ["#).unwrap();
        let pump = Pump::default();
        let organization = fake.organization(&pump, true);
        assert!(!fake.draft().exists(), "moved before the worker starts");
        let aside: Vec<String> = std::fs::read_dir(&fake.dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .filter(|name| name.starts_with("draft.json.") && name.ends_with(".bad"))
            .collect();
        assert_eq!(aside.len(), 1, "kept beside it: {aside:?}");
        organization.set_source(snapshot("d1"));
        let passes = parked(&organization);
        organization.observe(&state("d1"));
        wait_until("a pass after the first state", || {
            parked(&organization) > passes
        });
        assert!(fake.applies().is_empty());
        assert!(!fake.draft().exists());
        fake.finish(organization);
    }

    // The grouping keeps a rename it saw before the first state (Task 3);
    // the worker hands it over only once edits are accepted (sync.rs).
    #[test]
    fn a_rename_seen_before_the_first_state_is_sent_once_edits_are_accepted() {
        let fake = Fake::landing();
        let pump = Pump::default();
        pump.push(row("run:1", "claude/sample-project/review"));
        pump.push(row("run:1", "claude/sample-project/review-2"));
        let organization = fake.organization(&pump, true);
        organization.set_source(snapshot("d1"));
        wait_until("a pass before the first state", || {
            parked(&organization) >= 1
        });
        assert!(fake.applies().is_empty());
        organization.observe(&state("d1"));
        wait_until("the rename to land", || fake.settled_after(1));
        let log = fake.applies();
        assert_eq!(log.len(), 1);
        let sent = input(&log[0]);
        assert_eq!(sent["base"]["projects"][0]["sessions"], json!([R]));
        assert_eq!(sent["desired"]["projects"][0]["sessions"], json!([R2]));
        fake.finish(organization);
    }
}
