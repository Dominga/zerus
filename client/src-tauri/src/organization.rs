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
//! before any new draft is written, and the list shows why until the first
//! state. [`Organization::flush`] writes both files as the app quits. The
//! webview never receives the organization itself.
//!
//! Locks: the writer's, then the hub's, then the pump's (a publish hands the
//! pump the presented organization). The writer orders the draft and layout
//! writes of the worker and `flush`. The snapshot source, `hgs`, the files,
//! the edits' answers and the Projects callback all run without the hub's
//! lock, `hgs` also without the writer's, and the worker is woken after the
//! hub's lock is released. The Projects callback runs under the `announced`
//! lock, one call at a time. A pass that applies or settles edits drops the
//! writer's lock before its call; the calls made where `save_draft` releases
//! the hub's lock, in an idle pass and in `flush` still run under it. So the
//! callback must never block on the main thread, which takes the writer's
//! lock in `flush` as the app quits, and must never take the writer's lock.

use std::collections::HashMap;
use std::io;
use std::panic::AssertUnwindSafe;
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
/// Hands the presented `swarm get` object (none before the first state),
/// the default project and the controller's status line to the Projects
/// screen, as Qt's `statusChanged` reaches its Projects page
/// (SessionsWindow.cpp:1653).
/// It runs without the hub's lock, one call at a time, and a call never
/// follows a newer one; it must not call back into [`Organization`]. A
/// panic in it is caught and logged.
///
/// It must never block on the main thread and never take the writer's lock:
/// some calls run under that lock (see the module's lock list), and some on
/// the main thread itself, in [`Organization::flush`] as the app quits. A
/// call that waited for the main thread there would deadlock the quit.
pub type Presented = Arc<dyn Fn(Option<Arc<Value>>, String, (String, bool)) + Send + Sync>;

/// One call of [`Presented`], made once the hub's lock is released.
type Announce = (Presented, Option<Arc<Value>>, String, (String, bool));

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
    /// Why the saved draft was not applied, shown until the first state is adopted.
    notice: Option<String>,
    /// The worker is gone: the list says so, and edits answer `Kept(STOPPED)`.
    stopped: bool,
}

struct Shared {
    hub: Mutex<Hub>,
    wake: Condvar,
    /// The last Projects call made, so a late one never overtakes a newer one.
    announced: Mutex<u64>,
    /// The draft and layout writes of the worker and of `flush`.
    writer: Mutex<Writer>,
    pump: Pump,
    hgs: PathBuf,
    allowed: bool,
    layout: PathBuf,
    draft: PathBuf,
    clock: Instant,
    /// Passes the worker started, so a test can tell when it acted.
    #[cfg(test)]
    passes: std::sync::atomic::AtomicUsize,
    /// Passes the worker finished with nothing due, so a test can tell it ran.
    #[cfg(test)]
    parked: std::sync::atomic::AtomicUsize,
    /// Makes the worker's next pass panic, so a test can stop it.
    #[cfg(test)]
    fail: std::sync::atomic::AtomicBool,
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
        let saved = if allowed {
            saved_draft(&draft_path)
        } else {
            Saved::default()
        };
        let organization = Self(Arc::new(Shared {
            hub: Mutex::default(),
            wake: Condvar::new(),
            announced: Mutex::new(0),
            writer: Mutex::new(Writer::new(saved.aside)),
            pump,
            hgs,
            allowed,
            layout,
            draft: draft_path,
            clock: Instant::now(),
            #[cfg(test)]
            passes: std::sync::atomic::AtomicUsize::new(0),
            #[cfg(test)]
            parked: std::sync::atomic::AtomicUsize::new(0),
            #[cfg(test)]
            fail: std::sync::atomic::AtomicBool::new(false),
        }));
        {
            let mut hub = organization.lock();
            // The last run's pending edits (Qt's start, SwarmController.cpp:76-89):
            // shown at once, sent after the first state.
            if let Some(draft) = saved.draft {
                hub.sync.restore(draft);
            }
            hub.notice = saved.notice;
            organization.publish(&mut hub);
            organization.release(hub);
        }
        organization.spawn();
        organization
    }

    fn lock(&self) -> MutexGuard<'_, Hub> {
        self.0.hub.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn writer(&self) -> MutexGuard<'_, Writer> {
        self.0.writer.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn now(&self) -> u64 {
        u64::try_from(self.0.clock.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    pub fn set_source(&self, source: SnapshotSource) {
        self.lock().source = Some(source);
    }

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
                if hub.sync.observe(observation, now) {
                    // The list shows a catalog now; the dropped draft's notice goes.
                    hub.notice = None;
                }
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
        view.notice = hub.notice.clone();
        if !self.0.allowed {
            view.ready = false;
            view.reason = Some(REPLAY_NEEDS_HGS.into());
        }
        if hub.stopped {
            view.ready = false;
            view.reason = Some(STOPPED.into());
        }
        view
    }

    /// Hands the pump what the list shows, and queues the Projects call when
    /// the revision or the status line changed. The call carries the
    /// presented catalog once there is one, and the status line always: a
    /// source that fails before its first state still reaches Projects.
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
        let Some(presented) = hub.presented.clone() else {
            return;
        };
        let value = hub.sync.presented_value();
        let status = (shown.1.clone(), shown.2);
        hub.announcements += 1;
        hub.announce = Some((
            hub.announcements,
            (presented, value, self.0.pump.default_project(), status),
        ));
        hub.shown = Some(shown);
    }

    /// Drops the lock, then answers the settled edits and makes the queued
    /// Projects call. A panic in that call is logged, so it stops neither
    /// the worker nor the drain thread.
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
                let call = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    presented(value, default, status)
                }));
                if call.is_err() {
                    eprintln!(
                        "zerus-client: the Projects screen's update panicked; project sync goes on"
                    );
                }
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

    /// Writes the pending draft and the layout now, as the app quits. It
    /// takes the worker's writer, so their writes never interleave, and it
    /// never runs `hgs`: an edit made while an apply runs is in the draft,
    /// which the next start sends. It runs on the main thread, and so does
    /// a Projects call it makes ([`Presented`]).
    pub fn flush(&self) {
        let mut writer = self.writer();
        let hub = self.save_draft(self.lock(), &mut writer);
        self.release(hub);
        writer.save_layout(&self.0.pump, &self.0.layout);
    }

    fn settle(hub: &mut Hub, settled: Vec<(Ticket, EditOutcome)>) {
        for (ticket, outcome) in settled {
            if let Some(sender) = hub.waiting.remove(&ticket) {
                hub.answers.push((sender, outcome));
            }
        }
    }

    fn spawn(&self) {
        let shared = Arc::downgrade(&self.0);
        std::thread::Builder::new()
            .name("zerus-organization".into())
            .spawn(move || {
                let _stop = Stop(shared.clone());
                // Each pass waits at most a second, so the thread ends soon
                // after the last handle.
                while let Some(shared) = shared.upgrade() {
                    Organization(shared).pass();
                }
            })
            .expect("the organization thread starts");
    }

    /// One pass of the worker: renames, the draft file, a due apply, the
    /// layout, then a wait until the next flush or tick (at most 1 s), or
    /// until a state or an edit wakes it.
    fn pass(&self) {
        #[cfg(test)]
        {
            use std::sync::atomic::Ordering::SeqCst;
            self.0.passes.fetch_add(1, SeqCst);
            if self.0.fail.load(SeqCst) {
                panic!("a test stops the organization worker");
            }
        }
        let mut writer = self.writer();
        let mut hub = self.lock();
        self.renames(&mut hub);
        hub = self.save_draft(hub, &mut writer);
        match hub.sync.due(self.now()) {
            Due::Apply(invocation) => {
                // The list shows the apply running while `hgs` runs without
                // the hub's lock, and `flush` may write meanwhile. The
                // writer's lock goes first, so `flush` never waits behind the
                // Projects call (ruling N1).
                self.publish(&mut hub);
                drop(writer);
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
                drop(writer);
                self.release(hub);
                return;
            }
            Due::Nothing => {}
        }
        self.release(hub);
        if writer.saved.elapsed() >= SAVE_EVERY {
            writer.save_layout(&self.0.pump, &self.0.layout);
        }
        drop(writer);
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

    /// Writes the draft file while it lags the pending edits, under the
    /// writer's lock and without the hub's: an edit is on disk before the
    /// apply that carries it, and the file goes once the edits settled
    /// (SessionsWindow.cpp:2496). Only a write that succeeded is confirmed;
    /// a failed one is offered again on the next pass.
    fn save_draft<'a>(
        &'a self,
        mut hub: MutexGuard<'a, Hub>,
        writer: &mut Writer,
    ) -> MutexGuard<'a, Hub> {
        while let Some((change, write)) = hub.sync.take_draft() {
            self.release(hub);
            let written = writer.store_draft(&self.0.draft, &write);
            hub = self.lock();
            if !written {
                break;
            }
            hub.sync.draft_written(change);
        }
        hub
    }
}

/// The draft of the last run, as `new` takes it up.
#[derive(Default)]
struct Saved {
    draft: Option<Draft>,
    /// An unusable draft is still in place: the writer moves it first.
    aside: bool,
    /// Why the draft was not applied, for the list.
    notice: Option<String>,
}

/// The draft of the last run. One that cannot be used is never applied: the
/// reason goes to the log and the list, and the file aside before any new
/// draft replaces it.
fn saved_draft(path: &Path) -> Saved {
    match draft::load(path) {
        Load::Missing => Saved::default(),
        Load::Loaded(draft) => Saved {
            draft: Some(draft),
            ..Saved::default()
        },
        Load::Dropped(reason) => {
            let notice = format!("The saved project changes were not applied: {reason}");
            eprintln!("zerus-client: {notice}");
            let aside = match set_aside(path) {
                Ok(()) => false,
                Err(error) => {
                    eprintln!("zerus-client: the unusable draft could not be moved aside: {error}");
                    true
                }
            };
            Saved {
                draft: None,
                aside,
                notice: Some(notice),
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

/// What the draft and layout writes keep between them, behind the writer's lock.
struct Writer {
    /// An unusable draft is still in place; no draft is written before it moved.
    aside: bool,
    saved: Instant,
    /// The newest layout whose write failed, written again with the next save.
    layout: Option<Vec<u8>>,
    draft_log: Throttle,
    layout_log: Throttle,
    /// Layout saves done, so a test can tell one more ran.
    #[cfg(test)]
    saves: usize,
}

impl Writer {
    fn new(aside: bool) -> Self {
        Self {
            aside,
            saved: Instant::now(),
            layout: None,
            draft_log: Throttle::default(),
            layout_log: Throttle::default(),
            #[cfg(test)]
            saves: 0,
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
        let bytes = match pump.take_layout() {
            // Newer than any layout kept from a failed write, which goes.
            Some(bytes) => {
                self.layout = None;
                Some(bytes)
            }
            None => self.layout.take(),
        };
        if let Some(bytes) = bytes {
            if let Err(error) = draft::write(path, &bytes) {
                if self.layout_log.allow() {
                    eprintln!("zerus-client: the project layout was not saved: {error}");
                }
                self.layout = Some(bytes);
            }
        }
        #[cfg(test)]
        {
            self.saves += 1;
        }
    }
}

/// Runs when the worker ends. After a panic the hub outlives it: the list
/// shows that project sync stopped, and the edits waiting on the worker, and
/// later ones, answer `Kept(STOPPED)`.
struct Stop(Weak<Shared>);

impl Drop for Stop {
    fn drop(&mut self) {
        let Some(shared) = self.0.upgrade() else {
            return;
        };
        let organization = Organization(shared);
        let mut hub = organization.lock();
        hub.stopped = true;
        let waiting = std::mem::take(&mut hub.waiting);
        for sender in waiting.into_values() {
            hub.answers.push((
                sender,
                EditOutcome::Kept {
                    detail: STOPPED.into(),
                },
            ));
        }
        organization.publish(&mut hub);
        organization.release(hub);
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

    /// The passes the worker started.
    fn passes(organization: &Organization) -> usize {
        organization.0.passes.load(Ordering::SeqCst)
    }

    /// The layout saves done; taken under the writer's lock, so a save in
    /// progress is counted once it ended.
    fn saves(organization: &Organization) -> usize {
        organization.writer().saves
    }

    /// Whether the list's headers name `project` `name`, taking the batch.
    fn header_named(pump: &Pump, project: &str, name: &str) -> bool {
        pump.take(Duration::ZERO).grouping.is_some_and(|grouping| {
            grouping
                .headers
                .iter()
                .any(|header| header.id == project && header.name == name)
        })
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

    // SwarmController.cpp:67-73, 96-101: the edit stays, and the topic's next
    // state sends it again. Projects shows the failure until the resend lands,
    // as Qt's statusChanged reaches it (SessionsWindow.cpp:1653).
    #[test]
    fn a_failed_apply_answers_kept_and_the_next_state_sends_it_once_more() {
        const FAILED: &str =
            "Project sync unavailable: hgs: session belongs to more than one project";
        let refused =
            json!({"exit": 1, "stderr": "hgs: session belongs to more than one project\n"});
        let fake = Fake::new(json!([refused, {"stdout": full("d2")}]));
        let organization = fake.organization(&Pump::default(), true);
        organization.set_source(snapshot("d1"));
        organization.observe(&state("d1"));
        let lines: Arc<Mutex<Vec<(String, bool)>>> = Arc::default();
        {
            let lines = Arc::clone(&lines);
            organization.set_presented(Arc::new(move |_, _, status| {
                lines.lock().unwrap().push(status);
            }));
        }
        let last = || lines.lock().unwrap().last().cloned();
        let outcome = organization.submit(move_to_inbox());
        assert_eq!(
            outcome,
            EditOutcome::Kept {
                detail: FAILED.into()
            }
        );
        // The answer goes out just before the Projects call, both after the lock.
        wait_until("Projects to get the failure", || {
            last() == Some((FAILED.into(), true))
        });
        assert_eq!(fake.applies().len(), 1);
        assert!(fake.draft().exists(), "the kept edit stays on disk");
        let before = passes(&organization);
        organization.observe(&state("d1"));
        // The 5 s tick would take five idle passes; the state sends it at once.
        wait_until("the resend, or three more passes", || {
            fake.applies().len() >= 2 || passes(&organization) > before + 2
        });
        assert!(
            fake.applies().len() >= 2,
            "the state sent it again within two passes"
        );
        wait_until("the resend to land", || fake.settled_after(2));
        // The worker made the call before the pass that removed the draft.
        assert_eq!(
            last(),
            Some((
                "Local catalog. Connect another computer to sync projects.".into(),
                false
            )),
            "the landed catalog's line replaces the failure"
        );
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
        let notice = pump.take(crate::pump::PARK).grouping.unwrap().sync.notice;
        assert!(
            notice.as_deref().is_some_and(|notice| notice.starts_with(
                "The saved project changes were not applied: the draft file is not a draft: "
            )),
            "the list says why: {notice:?}"
        );
        let aside: Vec<String> = std::fs::read_dir(&fake.dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .filter(|name| name.starts_with("draft.json.") && name.ends_with(".bad"))
            .collect();
        assert_eq!(aside.len(), 1, "kept beside it: {aside:?}");
        organization.set_source(snapshot("d1"));
        let passes = parked(&organization);
        organization.observe(&state("d1"));
        let grouping = pump.take(crate::pump::PARK).grouping.unwrap();
        assert!(grouping.loaded);
        assert_eq!(grouping.sync.notice, None, "gone with the first state");
        wait_until("a pass after the first state", || {
            parked(&organization) > passes
        });
        assert!(fake.applies().is_empty());
        assert!(!fake.draft().exists());
        fake.finish(organization);
    }

    // Review fix round 1: a layout kept from a failed write is older than one
    // taken later, and never overwrites it.
    #[test]
    fn a_layout_that_could_not_be_saved_never_overwrites_a_newer_one() {
        let fake = Fake::landing();
        let pump = Pump::default();
        let organization = fake.organization(&pump, true);
        // A folder in the layout's place: saves fail and keep their bytes.
        std::fs::create_dir(fake.layout()).unwrap();
        pump.set_collapsed("alpha", true);
        let first = saves(&organization);
        wait_until("a failed save", || saves(&organization) > first);
        assert!(fake.layout().is_dir());
        {
            // Between two saves, the folder goes and a newer layout comes.
            let _writer = organization.writer();
            std::fs::remove_dir(fake.layout()).unwrap();
            pump.set_collapsed("alpha", false);
            pump.set_collapsed("ungrouped", true);
        }
        wait_until("the newer layout", || fake.layout().is_file());
        let newer = std::fs::read(fake.layout()).unwrap();
        assert!(String::from_utf8_lossy(&newer).contains("ungrouped"));
        let after = saves(&organization);
        wait_until("one more save", || saves(&organization) > after);
        assert_eq!(
            std::fs::read(fake.layout()).unwrap(),
            newer,
            "the failed layout never comes back"
        );
        fake.finish(organization);
    }

    // Review fix round 1: quitting writes what the worker could not yet. An
    // edit made while an apply runs is on disk after `flush`, which runs no hgs.
    #[test]
    fn flush_writes_an_edit_made_while_an_apply_runs() {
        let fake = Fake::new(json!([{"stdout": full("d2"), "gate": "go"}]));
        let pump = Pump::default();
        let organization = fake.organization(&pump, true);
        organization.set_source(snapshot("d1"));
        organization.observe(&state("d1"));
        let first = {
            let organization = organization.clone();
            std::thread::spawn(move || organization.submit(move_to_inbox()))
        };
        wait_until("the apply to start", || fake.applies().len() == 1);
        let second = {
            let organization = organization.clone();
            std::thread::spawn(move || {
                organization.submit(Intent::Rename {
                    project: "alpha".into(),
                    name: "Beta".into(),
                })
            })
        };
        wait_until("the rename in the list", || {
            header_named(&pump, "alpha", "Beta")
        });
        let Load::Loaded(before) = draft::load(&fake.draft()) else {
            panic!("no draft before the apply");
        };
        assert_eq!(before.desired["projects"][0]["name"], "Alpha");
        organization.flush();
        let Load::Loaded(saved) = draft::load(&fake.draft()) else {
            panic!("no draft after flush");
        };
        assert_eq!(saved.desired["projects"][0]["name"], "Beta");
        assert_eq!(saved.desired["projects"][1]["sessions"], json!([R]));
        assert_eq!(fake.applies().len(), 1, "flush runs no hgs");
        std::fs::write(fake.dir.join("go"), b"").unwrap();
        assert_eq!(first.join().unwrap(), EditOutcome::Done { conflicts: 0 });
        assert_eq!(second.join().unwrap(), EditOutcome::Done { conflicts: 0 });
        fake.finish(organization);
    }

    // Review fix round 1: a worker that stopped shows in the list before
    // anyone tries an edit, and edits answer Kept.
    #[test]
    fn a_stopped_worker_shows_in_the_list_before_any_edit() {
        let fake = Fake::landing();
        let pump = Pump::default();
        let organization = fake.organization(&pump, true);
        organization.set_source(snapshot("d1"));
        organization.observe(&state("d1"));
        assert!(pump.take(crate::pump::PARK).grouping.unwrap().sync.ready);
        organization.0.fail.store(true, Ordering::SeqCst);
        organization.0.wake.notify_all();
        wait_until("the list to show it", || {
            pump.take(Duration::ZERO).grouping.is_some_and(|grouping| {
                !grouping.sync.ready && grouping.sync.reason.as_deref() == Some(STOPPED)
            })
        });
        assert_eq!(
            organization.submit(move_to_inbox()),
            EditOutcome::Kept {
                detail: STOPPED.into()
            }
        );
        assert!(fake.applies().is_empty());
        fake.finish(organization);
    }

    // Review fix round 1: a panic in the Projects callback is logged, and the
    // worker goes on to the next edit.
    #[test]
    fn a_panicking_projects_callback_leaves_the_worker_running() {
        let fake = Fake::landing();
        let organization = fake.organization(&Pump::default(), true);
        organization.set_source(snapshot("d1"));
        organization.observe(&state("d1"));
        organization.set_presented(Arc::new(|_, _, _| {
            if std::thread::current().name() == Some("zerus-organization") {
                panic!("the Projects callback panics on the worker");
            }
        }));
        assert_eq!(
            organization.submit(move_to_inbox()),
            EditOutcome::Done { conflicts: 0 }
        );
        let rename = Intent::Rename {
            project: "alpha".into(),
            name: "Beta".into(),
        };
        assert_eq!(
            organization.submit(rename),
            EditOutcome::Done { conflicts: 0 },
            "the worker still runs edits"
        );
        fake.finish(organization);
    }

    /// Makes `intent` in the controller without a publish, so the worker's
    /// next pass that acts on it makes the Projects call: at the start of the
    /// apply, or in a pass that settles it. That call blocks, on the worker
    /// only, until the test lets it go; meanwhile `flush` must finish.
    /// Answers the applies run once the edit settled.
    fn flush_beside_a_slow_projects_call(intent: Intent) -> usize {
        let fake = Fake::landing();
        let organization = fake.organization(&Pump::default(), true);
        organization.set_source(snapshot("d1"));
        organization.observe(&state("d1"));
        let (entered, on_entry) = sync_channel::<()>(1);
        let (go, gate) = sync_channel::<()>(1);
        let gate = Mutex::new(gate);
        organization.set_presented(Arc::new(move |_, _, _| {
            if std::thread::current().name() == Some("zerus-organization") {
                let _ = entered.try_send(());
                // Once `go` is dropped, later calls pass at once.
                let _ = gate.lock().unwrap().recv_timeout(Duration::from_secs(10));
            }
        }));
        {
            let mut hub = organization.lock();
            let now = organization.now();
            hub.sync.edit(&intent, now).unwrap();
        }
        organization.0.wake.notify_all();
        on_entry
            .recv_timeout(Duration::from_secs(10))
            .expect("the worker's Projects call");
        let (flushed, on_flush) = sync_channel(1);
        {
            let organization = organization.clone();
            std::thread::spawn(move || {
                organization.flush();
                let _ = flushed.send(());
            });
        }
        let quit = on_flush.recv_timeout(Duration::from_secs(5));
        drop(go);
        assert!(
            quit.is_ok(),
            "flush waited behind the Projects call on the worker"
        );
        wait_until("the edit to settle", || !fake.draft().exists());
        let applies = fake.applies().len();
        fake.finish(organization);
        applies
    }

    // Ruling N1 (Task 6's review): the main thread runs `flush` as the app
    // quits, and it never waits behind the Projects call at the start of an
    // apply, which runs after the writer's lock is dropped.
    #[test]
    fn flush_never_waits_behind_the_projects_call_at_the_start_of_an_apply() {
        assert_eq!(flush_beside_a_slow_projects_call(move_to_inbox()), 1);
    }

    // The same for a pass that settles an edit without an apply.
    #[test]
    fn flush_never_waits_behind_the_projects_call_of_a_settled_pass() {
        let same = Intent::Rename {
            project: "alpha".into(),
            name: "Alpha".into(),
        };
        assert_eq!(
            flush_beside_a_slow_projects_call(same),
            0,
            "nothing shared changed"
        );
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

    /// Runs `submit` on a thread; its answer comes back on the receiver.
    fn submitted(
        organization: &Organization,
        intent: Intent,
    ) -> std::sync::mpsc::Receiver<EditOutcome> {
        let (sender, receiver) = sync_channel(1);
        let organization = organization.clone();
        std::thread::spawn(move || {
            let _ = sender.send(organization.submit(intent));
        });
        receiver
    }

    /// The same move again while its apply runs leaves the desired state as
    /// sent: that apply answers both, and the second `submit` returns.
    #[test]
    fn an_edit_repeated_while_its_apply_runs_is_answered_by_that_apply() {
        let fake = Fake::new(json!([{"stdout": full("d2"), "gate": "go"}]));
        let organization = fake.organization(&Pump::default(), true);
        organization.set_source(snapshot("d1"));
        organization.observe(&state("d1"));
        let first = submitted(&organization, move_to_inbox());
        wait_until("the apply to start", || fake.applies().len() == 1);
        let second = submitted(&organization, move_to_inbox());
        wait_until("the second edit to wait", || {
            organization.lock().waiting.len() == 2
        });
        std::fs::write(fake.dir.join("go"), b"").unwrap();
        let done = Ok(EditOutcome::Done { conflicts: 0 });
        assert_eq!(first.recv_timeout(Duration::from_secs(10)), done);
        assert_eq!(
            second.recv_timeout(Duration::from_secs(10)),
            done,
            "the second submit returns with the apply that wrote its state"
        );
        wait_until("the draft to go", || fake.settled_after(1));
        assert_eq!(fake.applies().len(), 1, "no second apply");
        fake.finish(organization);
    }

    /// What Projects receives: whether a catalog came with the call, and the
    /// status line.
    type Calls = Arc<Mutex<Vec<(Option<String>, (String, bool))>>>;

    fn recording(organization: &Organization) -> Calls {
        let calls: Calls = Arc::default();
        let kept = Arc::clone(&calls);
        organization.set_presented(Arc::new(move |value, _, status| {
            let digest = value.map(|value| value["digest"].as_str().unwrap_or("").to_owned());
            kept.lock().unwrap().push((digest, status));
        }));
        calls
    }

    /// A source that fails before its first state: Projects gets Qt's
    /// "Project sync unavailable: …" line without a catalog, as the Qt
    /// GUI's `swarmFailed` reaches its Projects page.
    #[test]
    fn a_source_that_fails_before_any_state_reaches_projects() {
        let fake = Fake::landing();
        let organization = fake.organization(&Pump::default(), true);
        let calls = recording(&organization);
        organization.observe(&Change::Organization {
            computer: ComputerId::local(),
            update: OrganizationUpdate::Failed {
                code: "unavailable".into(),
                message: "hgs: unknown command swarm".into(),
            },
        });
        assert_eq!(
            calls.lock().unwrap().last().cloned(),
            Some((
                None,
                (
                    "Project sync unavailable: hgs: unknown command swarm".into(),
                    true
                )
            ))
        );
        fake.finish(organization);
    }

    /// A saved draft whose apply fails on every tick: Projects shows the
    /// first state's catalog with the failure line, not an empty screen.
    #[test]
    fn a_saved_draft_whose_apply_fails_reaches_projects() {
        let refused =
            json!({"exit": 1, "stderr": "hgs: unknown field version; reload the catalog\n"});
        let fake = Fake::new(json!([refused]));
        let base = full("d1")["organization"].clone();
        let mut desired = base.clone();
        desired["projects"][0]["sessions"] = json!([]);
        desired["projects"][1]["sessions"] = json!([R]);
        draft::write(
            &fake.draft(),
            &Draft::new(base, desired, serde_json::Map::new()).to_json(),
        )
        .unwrap();
        let organization = fake.organization(&Pump::default(), true);
        let calls = recording(&organization);
        organization.set_source(snapshot("d1"));
        organization.observe(&state("d1"));
        let failed = (
            Some("d1".to_owned()),
            (
                "Project sync unavailable: hgs: unknown field version; reload the catalog".into(),
                true,
            ),
        );
        wait_until("Projects to get the failure", || {
            calls.lock().unwrap().last() == Some(&failed)
        });
        fake.finish(organization);
    }
}
