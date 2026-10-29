//! Guarded session actions: `run_action`, `inspect_session` and
//! `pending_read_marks`. The commands run the `hgs` CLI through the core's
//! `run`; this module owns the in-flight guard and the read-mark file.
//! `queue_read_marks` hands marks to the delivery worker (`readsync.rs`);
//! `attention_marks` and `update_attention_marks` keep the device's attention
//! marks and read replies.
//!
//! One write per session at a time: Qt's `updating` disables all of a
//! session's actions while one runs (`tray/src/`). An interrupt and a read
//! mark each have their own lane: Qt guards Stop only by its own request
//! (`m_interruptRequest`), and a quiet retry never makes a confirmed write
//! answer busy. A message has its own lane per agent, as Qt's
//! `m_messagesInFlight` keys it by host, name and agent. A process stop has
//! its own lane too: Qt's Processes tab stops beside a pause or rename, one
//! stop at a time per session. Send now and an OpenCode inbox removal have
//! their own lanes, as Qt's `m_queueSendRequest` and `m_removeQueuedRequest`,
//! and so does an answer, one per session (`m_answersInFlight`).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use tauri::State;
use zerus_client_core::actions::{
    change_notifications, classify, invocation, parse_reference, reference_invocation,
    ALREADY_ANSWERING, ALREADY_SENDING,
};
use zerus_client_core::details::{details_invocation, parse_details, Details};
use zerus_client_core::marks::{AttentionMarks, Mark};
use zerus_client_core::readmarks::{ReadMark, ReadMarks};
use zerus_client_core::reads::{self, ArchivePage};
use zerus_client_core::shell::{current_shell, open_command};
use zerus_client_core::{default_hgs, run, Action, Batch, ComputerId, Outcome, Target, ViewRow};

/// A session by computer id and session name, the action's lane and, for a
/// message, the agent it goes to ("" for the session's own agent).
type Key = (String, String, Lane, String);

/// Hears that this device's marks changed: the menu bar's desk reads them
/// again at once (`desk::marks_changed`), as Qt's window hands the tray its
/// read state at each change. Clones share it; the call runs on the thread
/// that made the change, after its lock is released.
#[derive(Clone, Default)]
pub struct MarksListener(Arc<Mutex<Option<Listener>>>);

/// What a [`MarksListener`] calls.
pub type Listener = Box<dyn Fn() + Send + Sync>;

impl MarksListener {
    pub fn set(&self, listener: Listener) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(listener);
    }

    fn changed(&self) {
        if let Some(listener) = &*self.0.lock().unwrap_or_else(PoisonError::into_inner) {
            listener();
        }
    }
}

/// Writes, interrupts, read marks, messages and process stops run apart: at
/// most one of each per session (messages: per agent).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lane {
    Write,
    Interrupt,
    Read,
    /// A shared notification change: Qt tracks these apart from `m_pending`.
    Notifications,
    Send,
    Process,
    Queue,
    Inbox,
    Answer,
}

impl Lane {
    pub fn of(action: &Action) -> Self {
        match action {
            Action::MarkRead { .. } => Self::Read,
            Action::Interrupt { .. } => Self::Interrupt,
            Action::Send { .. } => Self::Send,
            Action::StopProcess { .. } => Self::Process,
            Action::SendNow { .. } => Self::Queue,
            Action::RemoveQueued { .. } => Self::Inbox,
            Action::Answer { .. } => Self::Answer,
            _ => Self::Write,
        }
    }

    /// The in-flight key of `action` on `target`.
    pub fn key(target: &Target, action: &Action) -> Key {
        let agent = match action {
            Action::Send { agent_id, .. } => agent_id.clone().unwrap_or_default(),
            _ => String::new(),
        };
        (
            target.computer.clone(),
            target.name.clone(),
            Self::of(action),
            agent,
        )
    }
}

/// The sessions that have an action running.
#[derive(Clone, Default)]
pub struct InFlight {
    keys: Arc<Mutex<HashSet<Key>>>,
}

/// Holds a session's claim; dropping it frees the session.
pub struct Claim {
    keys: Arc<Mutex<HashSet<Key>>>,
    key: Key,
}

impl InFlight {
    /// None while an action already runs for that session.
    pub fn claim(&self, key: Key) -> Option<Claim> {
        let mut keys = self.keys.lock().unwrap_or_else(PoisonError::into_inner);
        keys.insert(key.clone()).then(|| Claim {
            keys: Arc::clone(&self.keys),
            key,
        })
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        self.keys
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.key);
    }
}

/// What a running write means for the send rules (`BlockFacts`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteKind {
    Update,
    Terminate,
    /// A context reset (Qt's `m_clearKey` while `m_clearRequest` runs): it
    /// blocks only the session it resets, never another one.
    Clear,
}

impl WriteKind {
    pub fn of(action: &Action) -> Self {
        match action {
            Action::Terminate | Action::Kill => Self::Terminate,
            Action::ClearContext { .. } => Self::Clear,
            _ => Self::Update,
        }
    }
}

/// The running writes by computer and session name ("" for a computer-wide one).
type Writes = Arc<Mutex<HashMap<(String, String), WriteKind>>>;

/// Called whenever a write mark appears or goes (`Actions::on_writes`).
pub type WritesHook = Arc<dyn Fn() + Send + Sync>;

/// Clears a session's write kind when its call ends.
pub struct WriteMark {
    writes: Writes,
    hook: Arc<OnceLock<WritesHook>>,
    key: (String, String),
}

impl Drop for WriteMark {
    fn drop(&mut self) {
        self.writes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.key);
        // After the lock is released: the hook may read the marks.
        if let Some(hook) = self.hook.get() {
            hook();
        }
    }
}

/// A call's claimed lane and, for a session write, its kind; both end when
/// it drops. The mark goes first (fields drop in order), so a write that
/// claims the lane next can never have its own mark removed by this one.
pub(crate) struct Running {
    _write: Option<WriteMark>,
    _claim: Claim,
}

/// Whether a call blocks messages while it runs: a write that sets Qt's
/// window-wide `m_pending` (`SessionsWindow.h:554`; set at
/// `SessionsWindow.cpp:2876, 2881, 4699, 4758, 5554, 5575, 5693, 5752, 5808,
/// 5815`). A context reset and a compaction have their own per-session block
/// (`m_clearKey`, `m_compacts`, 4668 and 4544): `begin` marks a reset as
/// `WriteKind::Clear` for its own session, and compact-to-send keeps its
/// own fact in the composer. A Terminal file drop (`m_terminalDropRequest`,
/// 2657) blocks nothing, though all three share the write lane.
fn blocks_messages(action: &Action) -> bool {
    Lane::of(action) == Lane::Write
        && !matches!(
            action,
            Action::StageFiles { .. } | Action::ClearContext { .. } | Action::CompactContext
        )
}

/// Terminations that succeeded, kept as Qt keeps `m_terminating` after the
/// call (`SessionsWindow.cpp:1932-1934`) until the listing no longer shows
/// that run running (`reconcileTerminations`, 3106-3115). At most this many.
const TERMINATED: usize = 64;

/// The managed state of the action commands; clones share it.
#[derive(Clone)]
pub struct Actions {
    flight: InFlight,
    marks: Arc<Mutex<ReadMarks>>,
    /// Wakes the read-mark delivery worker (`readsync.rs`).
    pub(crate) wake: crate::readsync::Wake,
    writes: Writes,
    /// Wakes the composer when a write mark appears or goes.
    hook: Arc<OnceLock<WritesHook>>,
    /// Computer, session name and the run each succeeded termination ended,
    /// oldest first.
    terminated: Arc<Mutex<VecDeque<(String, String, String)>>>,
    /// Hears every change of the pending marks.
    pub(crate) listener: MarksListener,
}

impl Actions {
    /// `marks_path` is the read-marks file; a missing one gives no marks.
    pub fn new(marks_path: PathBuf) -> Self {
        Self {
            flight: InFlight::default(),
            marks: Arc::new(Mutex::new(ReadMarks::load(marks_path))),
            wake: crate::readsync::Wake::default(),
            writes: Arc::default(),
            hook: Arc::default(),
            terminated: Arc::default(),
            listener: MarksListener::default(),
        }
    }

    /// Sets, once, what runs whenever a write mark appears or goes: the
    /// composers' wake, so a block reason shows at once (Qt redraws when a
    /// write starts and in `writeDone`). Later calls change nothing.
    pub fn on_writes(&self, hook: WritesHook) {
        let _ = self.hook.set(hook);
    }

    /// The running write as the send rules see it from the session
    /// `(computer, name)`: its own Terminate or Kill is a termination
    /// (`isTerminating`), and any other message-blocking write, of any
    /// session or computer, is an update, since Qt's `m_pending` is one
    /// window-wide flag (`messageBlockReason`, `SessionsWindow.cpp:4394`).
    /// A context reset is the session's own (Qt keys it by `m_clearKey`) and
    /// comes first, as `messageBlockReason` reads it first (4390); another
    /// session's reset is no update anywhere.
    pub fn write_kind(&self, computer: &str, name: &str) -> Option<WriteKind> {
        let writes = self.writes.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(own @ (WriteKind::Terminate | WriteKind::Clear)) =
            writes.get(&(computer.to_owned(), name.to_owned()))
        {
            return Some(*own);
        }
        writes
            .values()
            .any(|kind| *kind != WriteKind::Clear)
            .then_some(WriteKind::Update)
    }

    fn mark_write(&self, computer: &str, name: &str, action: &Action) -> WriteMark {
        let key = (computer.to_owned(), name.to_owned());
        self.writes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key.clone(), WriteKind::of(action));
        if let Some(hook) = self.hook.get() {
            hook();
        }
        WriteMark {
            writes: Arc::clone(&self.writes),
            hook: Arc::clone(&self.hook),
            key,
        }
    }

    /// Claims `key`'s lane for `action`, None while a call already runs in
    /// it. A message-blocking write, and a context reset for its own
    /// session, also marks its kind (`write_kind`) under the claimed
    /// computer and name.
    pub(crate) fn begin(&self, key: Key, action: &Action) -> Option<Running> {
        let (computer, name) = (key.0.clone(), key.1.clone());
        let claim = self.flight.claim(key)?;
        let marked = blocks_messages(action) || WriteKind::of(action) == WriteKind::Clear;
        let write = marked.then(|| self.mark_write(&computer, &name, action));
        Some(Running {
            _write: write,
            _claim: claim,
        })
    }

    /// A call ended: a termination that succeeded stays one for the send
    /// rules (`session_write`). Called before its `Running` drops, so the
    /// session never looks free in between. A session without a run (an
    /// untracked one) is never held: hgs gives every start, resume, restore
    /// and fresh start a new run id, and without one nothing tells its run.
    pub(crate) fn finished(&self, target: &Target, action: &Action, outcome: &Outcome) {
        if WriteKind::of(action) != WriteKind::Terminate
            || !matches!(outcome, Outcome::Done { .. })
            || target.run_id.is_empty()
        {
            return;
        }
        let mut terminated = self
            .terminated
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        terminated
            .retain(|(computer, name, _)| computer != &target.computer || name != &target.name);
        terminated.push_back((
            target.computer.clone(),
            target.name.clone(),
            target.run_id.clone(),
        ));
        while terminated.len() > TERMINATED {
            terminated.pop_front();
        }
    }

    /// The write the send rules see from the session: its own termination,
    /// running or succeeded while its run still shows, else any running
    /// write as an update (`write_kind`). `live` gives the computer's live
    /// row of that name and whether the computer is online; it is asked
    /// only while such a termination is kept. As in Qt, the row must be the
    /// run the termination ended (`isTerminating`, `SessionsWindow.cpp:3100-3104`),
    /// and only an online computer whose row no longer runs that run lets it
    /// go (`reconcileTerminations`).
    pub fn session_write(
        &self,
        computer: &str,
        name: &str,
        live: impl FnOnce() -> (Option<ViewRow>, bool),
    ) -> Option<WriteKind> {
        let running = self.write_kind(computer, name);
        if matches!(running, Some(WriteKind::Terminate | WriteKind::Clear)) {
            return running;
        }
        let Some(run) = self
            .terminated
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .find(|(c, n, _)| c == computer && n == name)
            .map(|(_, _, run)| run.clone())
        else {
            return running;
        };
        let (row, online) = live();
        let same_run = row
            .as_ref()
            .filter(|row| row.run_id.as_deref() == Some(run.as_str()));
        if online && same_run.is_none_or(|row| row.state != "running") {
            self.terminated
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .retain(|(c, n, r)| c != computer || n != name || r != &run);
            return running;
        }
        same_run.map(|_| WriteKind::Terminate).or(running)
    }

    /// `queue_read_marks`: merges each mark into the pending ones
    /// (`ReadMarkStore::add`), wakes the delivery worker and returns what is
    /// pending. No file I/O: the worker writes the marks before it sends any
    /// ([`Actions::flush_marks`]).
    pub fn queue_marks(&self, marks: Vec<ReadMark>) -> Vec<ReadMark> {
        let (added, pending) = {
            let mut store = self.marks.lock().unwrap_or_else(PoisonError::into_inner);
            (store.add_all(marks), store.pending())
        };
        self.wake.notify();
        if added {
            self.listener.changed();
        }
        pending
    }

    /// Writes the marks `queue_read_marks` added since the last save.
    pub(crate) fn flush_marks(&self) {
        self.marks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .flush();
    }

    /// Drops the marks for computers the local listing no longer names
    /// (`publishReadMarks`): they can never be written. `peers` is asked only
    /// while marks wait, and None (no listing named its peers yet) drops
    /// nothing. This computer's marks stay.
    pub fn drop_orphans(&self, peers: impl FnOnce() -> Option<BTreeSet<String>>) {
        let waiting = !self
            .marks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty();
        let Some(peers) = waiting.then(peers).flatten() else {
            return;
        };
        let dropped = self
            .marks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drop_orphans(&peers);
        if dropped {
            self.listener.changed();
        }
    }

    /// The marks not settled yet.
    pub(crate) fn pending_marks(&self) -> Vec<ReadMark> {
        self.marks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pending()
    }

    /// The session's lane, None while an action runs in it.
    pub(crate) fn claim_lane(&self, computer: &str, name: &str, lane: Lane) -> Option<Claim> {
        self.flight
            .claim((computer.to_owned(), name.to_owned(), lane, String::new()))
    }

    /// A mark as `run_action` saves it, without `queue_read_marks`' cleanup.
    #[cfg(test)]
    pub(crate) fn add_mark_for_test(&self, mark: ReadMark) {
        self.marks.lock().unwrap().add(mark);
    }

    /// Settles pending read marks with a batch the pump hands out: each row,
    /// single or inside a computer's snapshot, drops the mark it shows
    /// settled, and a snapshot drops its computer's marks for sessions it no
    /// longer lists. An empty replace is not taken as a snapshot: a stopped
    /// stream and a resend for a computer that never listed sessions send one,
    /// and Qt drops marks only after a successful listing. Nothing to do (the
    /// usual case) costs one lock.
    pub fn observe(&self, batch: &Batch) {
        let mut marks = self.marks.lock().unwrap_or_else(PoisonError::into_inner);
        if marks.is_empty() {
            return;
        }
        let mut changed = false;
        for out in batch.upserts.iter().chain(
            batch
                .replaces
                .iter()
                .flat_map(|replace| replace.rows.iter()),
        ) {
            changed |= marks.settle(&out.computer.0, &out.row);
        }
        for replace in batch
            .replaces
            .iter()
            .filter(|replace| !replace.rows.is_empty())
        {
            let names: HashSet<&str> = replace
                .rows
                .iter()
                .map(|out| out.row.name.as_str())
                .collect();
            changed |= marks.drop_unlisted(&replace.computer.0, &names);
        }
        drop(marks);
        if changed {
            self.listener.changed();
        }
    }
}

/// The read-marks file in the app data directory: a replay keeps its own, so
/// replay runs never touch the marks of real sessions.
pub fn marks_file(replay: bool) -> &'static str {
    if replay {
        "read-marks-replay.json"
    } else {
        "read-marks.json"
    }
}

/// The attention-marks file beside the read marks; a replay keeps its own.
pub fn attention_file(replay: bool) -> &'static str {
    if replay {
        "attention-marks-replay.json"
    } else {
        "attention-marks.json"
    }
}

/// The device's attention marks and read replies (`attention_marks`,
/// `update_attention_marks`). Updates change memory in call order; a writer
/// thread reads the file at start and writes the latest state after each
/// change, so the main thread never waits for the disk. Dropping it lets the
/// writer save what is pending, then end.
pub struct Attention {
    shared: Arc<AttentionShared>,
    /// Hears every change of the marks.
    pub(crate) listener: MarksListener,
}

/// Both maps by key, as the frontend receives them, and the order their
/// keys were written in (a JSON object's key order does not survive serde).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct AttentionSnapshot {
    pub marks: BTreeMap<String, Mark>,
    pub replies: BTreeMap<String, String>,
    pub order: AttentionOrder,
}

/// The keys of each map, oldest written first: the frontend's cap drops the
/// same entries as this one's.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct AttentionOrder {
    pub marks: Vec<String>,
    pub replies: Vec<String>,
}

struct AttentionShared {
    state: Mutex<AttentionState>,
    /// Signals a load, a change, a finished write and the end.
    signal: Condvar,
}

#[derive(Default)]
struct AttentionState {
    /// None until the writer thread has read the file.
    marks: Option<AttentionMarks>,
    /// A change the file does not have yet.
    dirty: bool,
    writing: bool,
    closed: bool,
    /// Counts the changes, so a reader copies the maps only after one.
    version: u64,
}

/// Both attention maps by key, and the version they were copied at.
pub type MarkMaps = (u64, BTreeMap<String, Mark>, BTreeMap<String, String>);

impl Attention {
    pub fn new(path: PathBuf) -> Self {
        let shared = Arc::new(AttentionShared {
            state: Mutex::new(AttentionState::default()),
            signal: Condvar::new(),
        });
        let writer = Arc::clone(&shared);
        let file = path.clone();
        let spawned = std::thread::Builder::new()
            .name("zerus-attention-marks".into())
            .spawn(move || writer.write_changes(file));
        if let Err(error) = spawned {
            // Kept in memory only: commands still answer.
            eprintln!("zerus-client: the attention-marks writer did not start: {error}");
            shared.lock().marks = Some(AttentionMarks::load(path));
            shared.signal.notify_all();
        }
        Self {
            shared,
            listener: MarksListener::default(),
        }
    }

    /// Both maps by key once the file has been read, with their version;
    /// None while the version is still `seen`. The maps hold up to 20 000
    /// entries each and the window's updates take this lock on the main
    /// thread, so a reader copies them only after they changed.
    pub fn marks_since(&self, seen: Option<u64>) -> Option<MarkMaps> {
        let state = self.shared.loaded();
        if seen == Some(state.version) {
            return None;
        }
        let (marks, replies) = state
            .marks
            .as_ref()
            .map(AttentionMarks::snapshot)
            .unwrap_or_default();
        Some((state.version, marks, replies))
    }

    #[cfg(test)]
    pub fn snapshot(&self) -> AttentionSnapshot {
        self.shared.snapshot()
    }

    /// A delta: `Some` writes an entry, `None` removes it. Memory only; the
    /// writer thread saves a change.
    pub fn update(
        &self,
        marks: BTreeMap<String, Option<Mark>>,
        replies: BTreeMap<String, Option<String>>,
    ) {
        let changed = {
            let mut state = self.shared.loaded();
            let changed = state
                .marks
                .as_mut()
                .is_some_and(|store| store.update(marks, replies));
            if changed {
                state.dirty = true;
                state.version += 1;
                self.shared.signal.notify_all();
            }
            changed
        };
        if changed {
            self.listener.changed();
        }
    }

    /// Waits at most `limit` until the file holds every change made so far,
    /// and says whether it does. A write in progress is never interrupted: the
    /// writer saves to a temporary file and renames it, so a process that ends
    /// mid-write leaves the old file whole.
    pub fn flush_within(&self, limit: Duration) -> bool {
        let state = self.shared.lock();
        let (state, _) = self
            .shared
            .signal
            .wait_timeout_while(state, limit, |state| {
                state.marks.is_none() || state.dirty || state.writing
            })
            .unwrap_or_else(PoisonError::into_inner);
        !(state.marks.is_none() || state.dirty || state.writing)
    }

    /// Waits until the file holds every change made so far.
    #[cfg(test)]
    pub fn flush(&self) {
        let state = self.shared.lock();
        let _state = self
            .shared
            .signal
            .wait_while(state, |state| {
                state.marks.is_none() || state.dirty || state.writing
            })
            .unwrap_or_else(PoisonError::into_inner);
    }
}

impl Drop for Attention {
    fn drop(&mut self) {
        self.shared.lock().closed = true;
        self.shared.signal.notify_all();
    }
}

impl AttentionShared {
    fn lock(&self) -> std::sync::MutexGuard<'_, AttentionState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The state once the file has been read.
    fn loaded(&self) -> std::sync::MutexGuard<'_, AttentionState> {
        self.signal
            .wait_while(self.lock(), |state| state.marks.is_none())
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn snapshot(&self) -> AttentionSnapshot {
        let state = self.loaded();
        let Some(store) = state.marks.as_ref() else {
            return AttentionSnapshot::default();
        };
        let (marks, replies) = store.snapshot();
        let (mark_order, reply_order) = store.order();
        AttentionSnapshot {
            marks,
            replies,
            order: AttentionOrder {
                marks: mark_order,
                replies: reply_order,
            },
        }
    }

    /// The writer thread: reads the file, then writes the latest state after
    /// each change (changes made meanwhile go into one write). A copy is taken
    /// under the lock and written without it.
    fn write_changes(&self, path: PathBuf) {
        let loaded = AttentionMarks::load(path);
        self.lock().marks = Some(loaded);
        self.signal.notify_all();
        loop {
            let contents = {
                let mut state = self
                    .signal
                    .wait_while(self.lock(), |state| !state.dirty && !state.closed)
                    .unwrap_or_else(PoisonError::into_inner);
                if !state.dirty {
                    return;
                }
                state.dirty = false;
                state.writing = true;
                state.marks.as_ref().map(AttentionMarks::contents)
            };
            if let Some(contents) = contents {
                if let Err(error) = contents.write() {
                    eprintln!(
                        "zerus-client: could not save attention marks to {}: {error}",
                        contents.path().display()
                    );
                }
            }
            self.lock().writing = false;
            self.signal.notify_all();
        }
    }
}

/// How long the exit waits for the attention writer.
#[cfg(feature = "live")]
const EXIT_FLUSH_LIMIT: Duration = Duration::from_millis(750);

/// For the app's exit: Tauri ends the process after `RunEvent::Exit`, so
/// `Attention`'s drop never runs. Saves the unsent read marks (a no-op when
/// none), then waits a bounded time for the attention writer.
#[cfg(feature = "live")]
pub fn flush_on_exit(app: &tauri::AppHandle) {
    use tauri::Manager;
    if let Some(actions) = app.try_state::<Actions>() {
        actions.flush_marks();
    }
    if let Some(attention) = app.try_state::<Attention>() {
        if !attention.flush_within(EXIT_FLUSH_LIMIT) {
            eprintln!("zerus-client: attention marks were not saved before exit");
        }
    }
}

/// The refusal of a replay that would reach the real CLI (actions and
/// project edits).
pub(crate) const REPLAY_NEEDS_HGS: &str = "Actions in replay mode need ZERUS_CLIENT_HGS.";

fn set(value: Option<&str>) -> bool {
    value.is_some_and(|v| !v.is_empty())
}

/// Whether actions may run: in replay mode (`ZERUS_CLIENT_REPLAY` set) only
/// with an explicit `ZERUS_CLIENT_HGS`, so a replay never drives the real CLI.
pub fn actions_allowed(replay: Option<&str>, hgs: Option<&str>) -> bool {
    !set(replay) || set(hgs)
}

/// Whether the actions smoke may start: replay mode, an explicit fake `hgs`
/// and a measurement run (`ZERUS_CLIENT_PERF`, which also allows `exit_app`).
pub fn smoke_allowed(replay: Option<&str>, hgs: Option<&str>, perf: Option<&str>) -> bool {
    set(replay) && set(hgs) && set(perf)
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

pub(crate) fn blocked() -> bool {
    !actions_allowed(
        env("ZERUS_CLIENT_REPLAY").as_deref(),
        env("ZERUS_CLIENT_HGS").as_deref(),
    )
}

/// For `perf_config`: whether the environment allows the actions smoke.
pub fn smoke_env_allowed() -> bool {
    smoke_allowed(
        env("ZERUS_CLIENT_REPLAY").as_deref(),
        env("ZERUS_CLIENT_HGS").as_deref(),
        env("ZERUS_CLIENT_PERF").as_deref(),
    )
}

fn failed(detail: impl Into<String>) -> Outcome {
    Outcome::Failed {
        detail: detail.into(),
    }
}

/// The refusal `dispatch` returns as a failed outcome before it claims the
/// session or starts a process: a restore or archived forget without its
/// archive id, refused before any pending check as Qt's `restoreArchive` and
/// `forgetArchive` do (`HgsClient.cpp:1303-1324`). Other calls are built off
/// the async workers after the claim, since building one may read files.
fn refused_before_claim(target: &Target, action: &Action) -> Option<String> {
    match action {
        Action::Restore { .. } | Action::ForgetArchive { .. } => {
            invocation(target, action, "").err()
        }
        _ => None,
    }
}

/// Runs one action on a session. `Err("busy")` while another runs for it.
#[tauri::command]
pub async fn run_action(
    state: State<'_, Actions>,
    target: Target,
    action: Action,
) -> Result<Outcome, String> {
    dispatch(&state, target, action).await
}

/// The one dispatcher of session actions, for `run_action` and for commands
/// that build an action in Rust (a message from the held draft). A message
/// already in flight to the same agent fails with Qt's text; any other lane
/// answers `Err("busy")`.
pub async fn dispatch(state: &Actions, target: Target, action: Action) -> Result<Outcome, String> {
    dispatch_as(state, target, action, uuid::Uuid::new_v4().to_string()).await
}

/// `dispatch` under the caller's request id: the composer's send makes one
/// per delivery, right before its one call, and records its local row under
/// it. The id reaches the CLI request and the receipt check unchanged, and
/// nothing here runs the call twice.
pub async fn dispatch_as(
    state: &Actions,
    target: Target,
    action: Action,
    request_id: String,
) -> Result<Outcome, String> {
    if blocked() {
        return Ok(failed(REPLAY_NEEDS_HGS));
    }
    dispatch_with(state, target, action, request_id, run).await
}

/// `dispatch_as` past its replay guard, with the runner that starts `hgs`:
/// the core's `run`, or in tests a stand-in that starts no process.
async fn dispatch_with(
    state: &Actions,
    target: Target,
    action: Action,
    request_id: String,
    runner: impl FnOnce(&std::path::Path, &zerus_client_core::Invocation) -> zerus_client_core::Exit
        + Send
        + 'static,
) -> Result<Outcome, String> {
    if let Some(refusal) = refused_before_claim(&target, &action) {
        return Ok(failed(refusal));
    }
    // Pause all and Resume all are computer-wide: one per computer at a time,
    // whatever session name the caller sent.
    let mut key = Lane::key(&target, &action);
    if matches!(action, Action::PauseAll { .. } | Action::ResumeAll { .. }) {
        key.1 = String::new();
    }
    // The claim, and for a session write its kind for the send rules
    // (`write_kind`), held until the call ends.
    let Some(_running) = state.begin(key, &action) else {
        return match Lane::of(&action) {
            Lane::Send => Ok(failed(ALREADY_SENDING)),
            Lane::Answer => Ok(failed(ALREADY_ANSWERING)),
            _ => Err(BUSY.into()),
        };
    };
    if let Action::MarkRead {
        reply_id,
        attention_signature,
    } = &action
    {
        // Saved before the call, so the mark survives a crash during it.
        state
            .marks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .add(ReadMark {
                computer: target.computer.clone(),
                name: target.name.clone(),
                run_id: target.run_id.clone(),
                conversation_id: target.conversation_id.clone(),
                reply_id: reply_id.clone(),
                attention_signature: attention_signature.clone(),
            });
    }
    // Building the call may read dropped files, so it runs off the async
    // workers with the call itself.
    let hgs = default_hgs();
    let (call_target, call_action) = (target.clone(), action.clone());
    let ran = tauri::async_runtime::spawn_blocking(move || {
        let inv = invocation(&call_target, &call_action, &request_id)?;
        let exit = runner(&hgs, &inv);
        Ok::<_, String>((inv, exit))
    })
    .await
    .map_err(|error| error.to_string())?;
    match ran {
        Ok((inv, exit)) => {
            let outcome = classify(&target, &action, &inv, exit);
            state.finished(&target, &action, &outcome);
            Ok(outcome)
        }
        Err(refusal) => Ok(failed(refusal)),
    }
}

/// Changes the shared notification preferences of a session (Mute and Unmute)
/// and returns the host's confirmed `preferences`. One change per session at a
/// time, in its own lane (`Err("busy")` otherwise). The core probes the local
/// `hgs --help` first. The confirmed preferences go to the session's row at
/// once (`FleetState::applyNotificationPreferences`), so the menu bar's
/// count and the attention notifications follow before the next listing.
#[tauri::command]
pub async fn set_notifications(
    state: State<'_, Actions>,
    app: State<'_, crate::commands::AppState>,
    target: Target,
    muted: bool,
    expected_revision: Option<i64>,
) -> Result<serde_json::Value, String> {
    if blocked() {
        return Err(REPLAY_NEEDS_HGS.into());
    }
    let Some(_claim) = state.claim_lane(&target.computer, &target.name, Lane::Notifications) else {
        return Err("busy".into());
    };
    let hgs = default_hgs();
    let call_target = target.clone();
    let confirmed = tauri::async_runtime::spawn_blocking(move || {
        change_notifications(&hgs, &call_target, muted, expected_revision)
    })
    .await
    .map_err(|error| error.to_string())??;
    app.apply_notifications(&target, &confirmed);
    Ok(confirmed)
}

/// The session's read-only details, None when it cannot be read. Not guarded:
/// the store keeps at most one inspect in flight per session.
#[tauri::command]
pub async fn inspect_session(target: Target, archive_id: Option<String>) -> Option<Details> {
    if blocked() {
        return None;
    }
    let inv = details_invocation(&target, archive_id.as_deref());
    let hgs = default_hgs();
    let exit = tauri::async_runtime::spawn_blocking(move || run(&hgs, &inv))
        .await
        .ok()?;
    parse_details(&target, exit)
}

/// `hgs reference` for the session: its `zerus://session?...` URI, or why it
/// could not be read. A read, so it needs no in-flight guard.
#[tauri::command]
pub async fn session_reference(
    target: Target,
    archive_id: Option<String>,
) -> Result<String, String> {
    if blocked() {
        return Err(REPLAY_NEEDS_HGS.to_owned());
    }
    let inv = reference_invocation(&target, archive_id.as_deref());
    let hgs = default_hgs();
    let exit = tauri::async_runtime::spawn_blocking(move || run(&hgs, &inv))
        .await
        .map_err(|error| error.to_string())?;
    parse_reference(exit)
}

/// The archive page requests in flight: per computer the newest one's
/// request number and cancel flag, and the number through which every
/// request is cancelled. Request numbers come from the frontend's archive
/// store and only grow, as Qt's `m_archiveRequest`, so the order in which the
/// commands start does not matter: a late older request never cancels a
/// newer one, and a request issued before a cancel stays cancelled however
/// late it starts (`HgsClient::requestArchivePage`, `cancelArchivePages`).
#[derive(Default)]
pub struct ArchiveReads {
    state: Mutex<ArchiveRegistry>,
}

#[derive(Default)]
struct ArchiveRegistry {
    latest: std::collections::HashMap<String, (u64, Arc<AtomicBool>)>,
    cancelled_through: u64,
}

impl ArchiveReads {
    /// Starts request `request` for `computer`. It starts cancelled when the
    /// cancel mark covers it or the computer has a request at least as new;
    /// otherwise it cancels the computer's older request and takes its place.
    fn begin(&self, computer: &str, request: u64) -> Arc<AtomicBool> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let newer = state
            .latest
            .get(computer)
            .is_some_and(|(current, _)| request <= *current);
        if request <= state.cancelled_through || newer {
            return Arc::new(AtomicBool::new(true));
        }
        let flag = Arc::new(AtomicBool::new(false));
        if let Some((_, older)) = state
            .latest
            .insert(computer.to_owned(), (request, Arc::clone(&flag)))
        {
            older.store(true, Ordering::Relaxed);
        }
        flag
    }

    /// Cancels every request numbered `through` or lower, also those that
    /// have not started yet; later requests keep running.
    fn cancel_through(&self, through: u64) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.cancelled_through = state.cancelled_through.max(through);
        state.latest.retain(|_, (request, flag)| {
            let cancelled = *request <= through;
            if cancelled {
                flag.store(true, Ordering::Relaxed);
            }
            !cancelled
        });
    }

    /// Forgets a finished request unless a newer one replaced it.
    fn finish(&self, computer: &str, request: u64) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state
            .latest
            .get(computer)
            .is_some_and(|(current, _)| *current == request)
        {
            state.latest.remove(computer);
        }
    }
}

/// One checked page of the computer's archive (30 s, Qt's validation).
/// `request` is the archive store's request number. A newer request for the
/// same computer, or a cancel through this number, ends this one's `hgs` and
/// answers it `Err("superseded")`; the store ignores it. A reply that fails
/// Qt's checks is `Err` with one of Qt's three texts and no rows.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn archive_page(
    state: State<'_, ArchiveReads>,
    computer: String,
    from: f64,
    until: f64,
    cursor: String,
    query: String,
    request: u64,
) -> Result<ArchivePage, String> {
    if blocked() {
        return Err(REPLAY_NEEDS_HGS.to_owned());
    }
    let flag = state.begin(&computer, request);
    if flag.load(Ordering::Relaxed) {
        return Err("superseded".into());
    }
    let hgs = default_hgs();
    let id = ComputerId::from(computer.as_str());
    let job = Arc::clone(&flag);
    let result = tauri::async_runtime::spawn_blocking(move || {
        reads::archive_page_cancellable(&hgs, &id, from, until, &cursor, &query, &job)
    })
    .await
    .map_err(|error| error.to_string());
    state.finish(&computer, request);
    // An answer that arrived as a newer request or a cancel came is dropped too.
    if flag.load(Ordering::Relaxed) {
        return Err("superseded".into());
    }
    result?.map_err(|error| error.to_string())
}

/// The archived Activity reads in flight, one per computer, name and archive
/// id (Qt's `m_inspectionsInFlight`).
#[derive(Default)]
pub struct ActivityReads {
    running: Mutex<HashSet<String>>,
}

/// An archived Activity read's place in [`ActivityReads`]; dropping it frees
/// the key.
struct ActivitySlot<'a> {
    reads: &'a ActivityReads,
    key: String,
}

impl ActivityReads {
    /// Takes the key of a read, or None when that read is already running.
    fn begin(&self, computer: &str, name: &str, archive_id: &str) -> Option<ActivitySlot<'_>> {
        let key = format!("{computer}\n{name}\n{archive_id}");
        let mut running = self.running.lock().unwrap_or_else(PoisonError::into_inner);
        running
            .insert(key.clone())
            .then(|| ActivitySlot { reads: self, key })
    }
}

impl Drop for ActivitySlot<'_> {
    fn drop(&mut self) {
        self.reads
            .running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.key);
    }
}

/// The text the frontend ignores: a read of that row is already running.
pub const ACTIVITY_BUSY: &str = "busy";

/// `dispatch`'s `Err` when the session's lane is taken: refused before any
/// call ran, so a definite refusal, never an uncertain outcome.
pub const BUSY: &str = "busy";

/// The recorded Activity of one archived session after cursor `after`
/// (`hgs [@alias] inspect NAME --after N --skip-processes --archive ID`).
/// One read per computer, name and archive id: a second answers `Err("busy")`
/// at once, which the store ignores (Qt's `requestInspection` returns false).
/// Any other `Err` is Qt's failure text.
#[tauri::command]
pub async fn archived_activity(
    state: State<'_, ActivityReads>,
    computer: String,
    name: String,
    archive_id: String,
    after: i64,
) -> Result<reads::ArchivedActivity, String> {
    if blocked() {
        return Err(crate::screens::REPLAY_NEEDS_HGS.to_owned());
    }
    let Some(_slot) = state.begin(&computer, &name, &archive_id) else {
        return Err(ACTIVITY_BUSY.to_owned());
    };
    let hgs = default_hgs();
    let id = ComputerId::from(computer.as_str());
    tauri::async_runtime::spawn_blocking(move || {
        reads::archived_activity(&hgs, &id, &name, &archive_id, after).map(|mut activity| {
            annotate_events(&mut activity.events);
            activity
        })
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

/// Codex's question replies of recorded events become `question_replies`
/// (R7's `annotate_replies`), as `next_batch` does for live ones.
fn annotate_events(events: &mut Vec<serde_json::Value>) {
    let mut payload = serde_json::json!({ "events": std::mem::take(events) });
    zerus_client_core::questions::annotate_replies(&mut payload);
    if let serde_json::Value::Array(list) = payload["events"].take() {
        *events = list;
    }
}

/// Cancels every archive page request numbered `through` or lower, where Qt
/// calls `cancelArchivePages`: their `hgs` ends and they answer
/// `Err("superseded")`, also one that starts after this.
#[tauri::command]
pub fn cancel_archive_pages(state: State<'_, ArchiveReads>, through: u64) {
    state.cancel_through(through);
}

/// The command that opens the session in a terminal, written for this
/// computer's shell (`TrayAgent.cpp:742-745`).
#[tauri::command]
pub fn copy_command(computer: String, name: String, archive_id: Option<String>) -> String {
    open_command(
        &default_hgs().to_string_lossy(),
        &computer,
        &name,
        archive_id.as_deref(),
        current_shell(),
    )
}

/// Read marks the host has not shown yet, for the store to apply optimistically.
#[tauri::command]
pub fn pending_read_marks(state: State<'_, Actions>) -> Vec<ReadMark> {
    state
        .marks
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .pending()
}

/// Queues read marks for their hosts (`FleetState::queueRead`): each is merged
/// into the pending mark of its session; the worker (`readsync.rs`) writes
/// them to the file before it sends any, then delivers them. Returns the
/// pending marks. Not async, so calls apply in the order the frontend makes
/// them; it does no file I/O.
#[tauri::command]
pub fn queue_read_marks(state: State<'_, Actions>, marks: Vec<ReadMark>) -> Vec<ReadMark> {
    state.queue_marks(marks)
}

/// The device's attention marks and read replies by key. Async: the first
/// call may wait for the writer thread's read of the file, and a snapshot at
/// the cap is large, so neither runs on the main thread.
#[tauri::command]
pub async fn attention_marks(state: State<'_, Attention>) -> Result<AttentionSnapshot, String> {
    let shared = Arc::clone(&state.shared);
    tauri::async_runtime::spawn_blocking(move || shared.snapshot())
        .await
        .map_err(|error| error.to_string())
}

/// Writes (`Some`) or removes (`None`) attention marks and read replies; each
/// map keeps its 20 000 newest written entries. Not async, so updates apply in
/// the order the frontend makes them; the change is in memory at once and the
/// writer thread saves it.
#[tauri::command]
pub fn update_attention_marks(
    state: State<'_, Attention>,
    marks: BTreeMap<String, Option<Mark>>,
    replies: BTreeMap<String, Option<String>>,
) {
    state.update(marks, replies);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorded_question_replies_are_annotated() {
        let detail = "<send_user_message_question_reply>{\"questionItemId\":\"q1\",\"question\":\"Which checks?\",\"answer\":\"Unit\"}</send_user_message_question_reply>";
        let mut events =
            vec![serde_json::json!({"seq": 1, "type": "UserPromptSubmit", "detail": detail})];
        annotate_events(&mut events);
        assert_eq!(
            events[0]["question_replies"],
            serde_json::json!([{"question": "Which checks?", "answer": "Unit"}])
        );
    }

    #[test]
    fn a_running_write_reports_its_kind() {
        let actions = Actions::new(
            std::env::temp_dir().join(format!("zerus-kinds-{}.json", std::process::id())),
        );
        assert_eq!(actions.write_kind("", "claude/sample-project/review"), None);
        let guard = actions.mark_write("", "claude/sample-project/review", &Action::Terminate);
        assert_eq!(
            actions.write_kind("", "claude/sample-project/review"),
            Some(WriteKind::Terminate)
        );
        drop(guard);
        assert_eq!(actions.write_kind("", "claude/sample-project/review"), None);
        assert_eq!(WriteKind::of(&Action::Pause), WriteKind::Update);
        assert_eq!(
            WriteKind::of(&Action::ClearContext {
                dismiss_prompt: false
            }),
            WriteKind::Clear
        );
    }

    fn review() -> Target {
        Target {
            computer: "studio".into(),
            name: "claude/sample-project/review".into(),
            run_id: "run-1".into(),
            conversation_id: "conv-1".into(),
        }
    }

    fn other() -> Target {
        Target {
            name: "claude/sample-project/other".into(),
            run_id: "run-2".into(),
            ..review()
        }
    }

    #[test]
    fn a_dispatched_write_marks_its_kind_until_it_ends() {
        let actions = Actions::new(std::env::temp_dir().join("zerus-begin-test-marks.json"));
        let target = review();
        let kind = || actions.write_kind("studio", "claude/sample-project/review");
        let kill = actions
            .begin(Lane::key(&target, &Action::Kill), &Action::Kill)
            .unwrap();
        assert_eq!(kind(), Some(WriteKind::Terminate));
        assert!(
            actions
                .begin(Lane::key(&target, &Action::Pause), &Action::Pause)
                .is_none(),
            "one write per session"
        );
        assert_eq!(
            kind(),
            Some(WriteKind::Terminate),
            "a refused claim marks nothing"
        );
        drop(kill);
        assert_eq!(kind(), None);
        let rename = Action::Rename {
            new_name: "claude/sample-project/review-2".into(),
            archive_id: None,
        };
        let renaming = actions.begin(Lane::key(&target, &rename), &rename).unwrap();
        assert_eq!(kind(), Some(WriteKind::Update));
        drop(renaming);
        // Other lanes block no message, nor do the two writes with a block
        // of their own or none: a compaction and a file drop.
        for action in [
            Action::Interrupt { turn_started: 1.0 },
            Action::MarkRead {
                reply_id: None,
                attention_signature: None,
            },
            Action::StopProcess {
                process_id: "p1".into(),
                archive_id: None,
                generation: None,
            },
            Action::StageFiles {
                paths: vec![],
                expected_created: None,
            },
            Action::CompactContext,
        ] {
            let running = actions.begin(Lane::key(&target, &action), &action).unwrap();
            assert_eq!(kind(), None, "{action:?}");
            drop(running);
        }
        // A context reset blocks its own session only (`m_clearKey`).
        let clear = Action::ClearContext {
            dismiss_prompt: true,
        };
        let clearing = actions.begin(Lane::key(&target, &clear), &clear).unwrap();
        assert_eq!(kind(), Some(WriteKind::Clear));
        drop(clearing);
        assert_eq!(kind(), None);
    }

    #[test]
    fn a_context_reset_is_its_own_sessions_and_no_update_elsewhere() {
        let actions = Actions::new(std::env::temp_dir().join("zerus-clear-test-marks.json"));
        let mine = || actions.write_kind("studio", "claude/sample-project/review");
        let theirs = || actions.write_kind("studio", "claude/sample-project/other");
        let clear = Action::ClearContext {
            dismiss_prompt: false,
        };
        let clearing = actions.begin(Lane::key(&other(), &clear), &clear).unwrap();
        assert_eq!(mine(), None, "another session's reset blocks nothing here");
        assert_eq!(theirs(), Some(WriteKind::Clear));
        // Its own reset comes before another session's update, as Qt's
        // `messageBlockReason` reads `m_clearKey` before `m_pending`.
        let pause = actions
            .begin(Lane::key(&review(), &Action::Pause), &Action::Pause)
            .unwrap();
        assert_eq!(mine(), Some(WriteKind::Update));
        assert_eq!(theirs(), Some(WriteKind::Clear));
        // A retained termination does not hide the reset either.
        let done = Outcome::Done {
            receipt: serde_json::Value::Null,
            cleanup: None,
        };
        actions.finished(&other(), &Action::Terminate, &done);
        assert_eq!(
            actions.session_write("studio", "claude/sample-project/other", || (
                live("running", "run-2"),
                true
            )),
            Some(WriteKind::Clear)
        );
        drop((clearing, pause));
        assert_eq!(theirs(), None);
        assert_eq!(
            actions.session_write("studio", "claude/sample-project/other", || (
                live("running", "run-2"),
                true
            )),
            Some(WriteKind::Terminate),
            "the retained one stays"
        );
        assert_eq!(mine(), None);
    }

    #[test]
    fn any_write_anywhere_is_an_update_and_only_ones_own_end_a_termination() {
        let actions = Actions::new(std::env::temp_dir().join("zerus-global-test-marks.json"));
        let mine = || actions.write_kind("studio", "claude/sample-project/review");
        // Another session's pause, as Qt's one `m_pending`.
        let pause = actions
            .begin(Lane::key(&other(), &Action::Pause), &Action::Pause)
            .unwrap();
        assert_eq!(mine(), Some(WriteKind::Update));
        drop(pause);
        assert_eq!(mine(), None);
        // Another computer's rename.
        let mut away = review();
        away.computer = "laptop".into();
        let rename = Action::Rename {
            new_name: "claude/sample-project/review-2".into(),
            archive_id: None,
        };
        let renaming = actions.begin(Lane::key(&away, &rename), &rename).unwrap();
        assert_eq!(mine(), Some(WriteKind::Update));
        drop(renaming);
        // Another session's termination is an update here, its own there.
        let ending = actions
            .begin(Lane::key(&other(), &Action::Terminate), &Action::Terminate)
            .unwrap();
        assert_eq!(mine(), Some(WriteKind::Update));
        assert_eq!(
            actions.write_kind("studio", "claude/sample-project/other"),
            Some(WriteKind::Terminate)
        );
        // Its own termination wins over another's update.
        let killing = actions
            .begin(Lane::key(&review(), &Action::Kill), &Action::Kill)
            .unwrap();
        assert_eq!(mine(), Some(WriteKind::Terminate));
        drop((ending, killing));
        assert_eq!(mine(), None);
        // Pause all of another computer.
        let mut key = Lane::key(&away, &Action::PauseAll { count: 2 });
        key.1 = String::new();
        let all = actions.begin(key, &Action::PauseAll { count: 2 }).unwrap();
        assert_eq!(mine(), Some(WriteKind::Update));
        drop(all);
        assert_eq!(mine(), None);
        // A retained termination of its own wins over another's update.
        let done = Outcome::Done {
            receipt: serde_json::Value::Null,
            cleanup: None,
        };
        actions.finished(&review(), &Action::Terminate, &done);
        let pause = actions
            .begin(Lane::key(&other(), &Action::Pause), &Action::Pause)
            .unwrap();
        let seen =
            |name: &str| actions.session_write("studio", name, || (live("running", "run-1"), true));
        assert_eq!(
            seen("claude/sample-project/review"),
            Some(WriteKind::Terminate)
        );
        assert_eq!(seen("claude/sample-project/other"), Some(WriteKind::Update));
        drop(pause);
    }

    #[test]
    fn a_write_mark_wakes_the_hook_and_ends_before_its_claim() {
        let actions = Actions::new(std::env::temp_dir().join("zerus-hook-test-marks.json"));
        let key = Lane::key(&review(), &Action::Pause);
        let (probe, probed, calls) = (
            actions.clone(),
            key.clone(),
            Arc::new(Mutex::new(Vec::new())),
        );
        let record = Arc::clone(&calls);
        actions.on_writes(Arc::new(move || {
            // Whether the lane is still claimed and the mark is there.
            let claimed = probe.flight.claim(probed.clone()).is_none();
            let marked = probe.write_kind("studio", "claude/sample-project/review");
            record
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push((claimed, marked));
        }));
        actions.on_writes(Arc::new(|| panic!("the first hook stays")));
        let running = actions.begin(key.clone(), &Action::Pause).unwrap();
        let seen = calls.lock().unwrap().clone();
        assert_eq!(
            seen,
            [(true, Some(WriteKind::Update))],
            "the mark shows at once"
        );
        drop(running);
        let seen = calls.lock().unwrap().clone();
        assert_eq!(
            seen.get(1),
            Some(&(true, None)),
            "the mark went while the lane was still claimed"
        );
        // A lane that marks nothing wakes nothing.
        let interrupt = Action::Interrupt { turn_started: 1.0 };
        drop(
            actions
                .begin(Lane::key(&review(), &interrupt), &interrupt)
                .unwrap(),
        );
        assert_eq!(calls.lock().unwrap().len(), 2);
    }

    #[test]
    fn dispatch_marks_a_write_for_the_length_of_its_call() {
        use zerus_client_core::Exit;
        let actions = Actions::new(std::env::temp_dir().join("zerus-dispatch-test-marks.json"));
        let call = |action: Action, code: i32| {
            let (seen, during) = (actions.clone(), Arc::new(Mutex::new(None)));
            let record = Arc::clone(&during);
            let outcome = tauri::async_runtime::block_on(dispatch_with(
                &actions,
                review(),
                action,
                uuid::Uuid::new_v4().to_string(),
                move |_, _| {
                    *record.lock().unwrap() =
                        Some(seen.write_kind("studio", "claude/sample-project/review"));
                    Exit::Code {
                        code,
                        stdout: Vec::new(),
                        stderr: String::new(),
                    }
                },
            ))
            .unwrap();
            let during = during.lock().unwrap().expect("the stand-in ran");
            (outcome, during)
        };
        let still_running = || {
            actions.session_write("studio", "claude/sample-project/review", || {
                (live("running", "run-1"), true)
            })
        };
        let (outcome, during) = call(Action::Pause, 1);
        assert!(matches!(outcome, Outcome::Failed { .. }));
        assert_eq!(during, Some(WriteKind::Update), "marked while it runs");
        assert_eq!(
            actions.write_kind("studio", "claude/sample-project/review"),
            None
        );
        let (outcome, during) = call(Action::Terminate, 1);
        assert!(matches!(outcome, Outcome::Failed { .. }));
        assert_eq!(during, Some(WriteKind::Terminate));
        assert_eq!(still_running(), None, "a failed termination is over");
        let (outcome, during) = call(Action::Terminate, 0);
        assert!(matches!(outcome, Outcome::Done { .. }));
        assert_eq!(during, Some(WriteKind::Terminate));
        assert_eq!(
            actions.write_kind("studio", "claude/sample-project/review"),
            None
        );
        assert_eq!(
            still_running(),
            Some(WriteKind::Terminate),
            "a done termination holds while its run still runs"
        );
        let (_, during) = call(Action::Interrupt { turn_started: 1.0 }, 0);
        assert_eq!(during, None, "an interrupt is no write");
    }

    #[test]
    fn a_send_carries_the_callers_request_id_into_its_one_call() {
        use zerus_client_core::Exit;
        let actions = Actions::new(std::env::temp_dir().join("zerus-dispatch-as-test-marks.json"));
        let send = Action::Send {
            text: "hello".into(),
            attachments: Vec::new(),
            agent_id: None,
            expected_compaction_id: None,
        };
        // As the composer's dispatch dep runs it: `block_on` on a blocking
        // thread of the app's runtime.
        let call = |id: &str, echo: Option<&str>| {
            let calls = Arc::new(Mutex::new(Vec::new()));
            let seen = Arc::clone(&calls);
            let (state, action, id) = (actions.clone(), send.clone(), id.to_owned());
            let echo = echo.map(str::to_owned);
            let outcome =
                tauri::async_runtime::block_on(tauri::async_runtime::spawn_blocking(move || {
                    tauri::async_runtime::block_on(dispatch_with(
                        &state,
                        review(),
                        action,
                        id,
                        move |_, inv| {
                            let input: serde_json::Value =
                                serde_json::from_slice(inv.stdin.as_deref().unwrap()).unwrap();
                            let asked = input["request_id"].as_str().unwrap().to_owned();
                            seen.lock()
                                .unwrap()
                                .push((inv.request_id.clone(), asked.clone()));
                            let receipt = serde_json::json!({
                                "status": "submitted",
                                "request_id": echo.unwrap_or(asked),
                                "name": "claude/sample-project/review",
                                "run_id": "run-1",
                                "conversation_id": "conv-1",
                            });
                            Exit::Code {
                                code: 0,
                                stdout: serde_json::to_vec(&receipt).unwrap(),
                                stderr: String::new(),
                            }
                        },
                    ))
                }))
                .unwrap()
                .unwrap();
            let calls = calls.lock().unwrap().clone();
            (outcome, calls)
        };
        let id = uuid::Uuid::new_v4().to_string();
        let (outcome, calls) = call(&id, None);
        assert_eq!(
            calls,
            vec![(Some(id.clone()), id.clone())],
            "one call, under the caller's id"
        );
        let Outcome::Done { receipt, .. } = outcome else {
            panic!("not done: {outcome:?}")
        };
        assert_eq!(receipt["request_id"], id.as_str());
        let other = uuid::Uuid::new_v4().to_string();
        let (outcome, calls) = call(&id, Some(&other));
        assert_eq!(calls.len(), 1, "an unconfirmed receipt is never sent again");
        assert!(
            matches!(outcome, Outcome::Uncertain { .. }),
            "a receipt of another request: {outcome:?}"
        );
    }

    fn live(state: &str, run: &str) -> Option<ViewRow> {
        Some(ViewRow {
            name: "claude/sample-project/review".into(),
            state: state.into(),
            run_id: Some(run.into()),
            ..ViewRow::default()
        })
    }

    #[test]
    fn a_finished_termination_holds_until_its_run_stops_running() {
        let actions = Actions::new(std::env::temp_dir().join("zerus-ended-test-marks.json"));
        let target = review();
        let done = Outcome::Done {
            receipt: serde_json::Value::Null,
            cleanup: None,
        };
        let write = |live: Option<ViewRow>, online: bool| {
            actions.session_write("studio", "claude/sample-project/review", || (live, online))
        };
        actions.finished(&target, &Action::Terminate, &failed("refused"));
        actions.finished(&target, &Action::Pause, &done);
        assert_eq!(
            write(live("running", "run-1"), true),
            None,
            "only a done termination stays"
        );
        actions.finished(&target, &Action::Terminate, &done);
        assert_eq!(
            write(live("running", "run-1"), true),
            Some(WriteKind::Terminate)
        );
        assert_eq!(
            write(live("running", "run-1"), false),
            Some(WriteKind::Terminate)
        );
        assert_eq!(
            write(live("paused", "run-1"), false),
            Some(WriteKind::Terminate),
            "an offline listing proves nothing"
        );
        assert_eq!(write(live("running", "run-2"), false), None, "another run");
        assert_eq!(
            write(live("running", "run-1"), true),
            Some(WriteKind::Terminate)
        );
        // Online, and the run no longer runs: it is let go for good.
        assert_eq!(write(live("paused", "run-1"), true), None);
        assert_eq!(write(live("running", "run-1"), true), None);
        // Online, and no live row of that name at all: likewise.
        actions.finished(&target, &Action::Kill, &done);
        assert_eq!(write(None, false), None, "offline: nothing to compare");
        assert_eq!(
            write(live("running", "run-1"), false),
            Some(WriteKind::Terminate),
            "still kept"
        );
        assert_eq!(write(None, true), None);
        assert_eq!(write(live("running", "run-1"), true), None);
        // Nothing kept: the row is never asked for.
        assert_eq!(
            actions.session_write("studio", "claude/sample-project/review", || unreachable!()),
            None
        );
    }

    #[test]
    fn a_session_without_a_run_is_never_held() {
        let actions = Actions::new(std::env::temp_dir().join("zerus-untracked-test-marks.json"));
        let done = Outcome::Done {
            receipt: serde_json::Value::Null,
            cleanup: None,
        };
        let mut untracked = review();
        untracked.run_id = String::new();
        actions.finished(&untracked, &Action::Kill, &done);
        assert!(actions.terminated.lock().unwrap().is_empty());
        let row = ViewRow {
            name: "claude/sample-project/review".into(),
            state: "running".into(),
            run_id: None,
            ..ViewRow::default()
        };
        assert_eq!(
            actions.session_write("studio", "claude/sample-project/review", || (
                Some(row),
                false
            )),
            None
        );
    }

    #[test]
    fn finished_terminations_are_bounded() {
        let actions = Actions::new(std::env::temp_dir().join("zerus-bound-test-marks.json"));
        let done = Outcome::Done {
            receipt: serde_json::Value::Null,
            cleanup: None,
        };
        for n in 0..TERMINATED + 6 {
            let mut target = review();
            target.name = format!("claude/sample-project/s-{n}");
            actions.finished(&target, &Action::Kill, &done);
        }
        let kept = actions.terminated.lock().unwrap().len();
        assert_eq!(kept, TERMINATED);
        let held = |n: usize| {
            let name = format!("claude/sample-project/s-{n}");
            let row = ViewRow {
                name: name.clone(),
                state: "running".into(),
                run_id: Some("run-1".into()),
                ..ViewRow::default()
            };
            actions.session_write("studio", &name, || (Some(row), true))
        };
        assert_eq!(held(0), None, "the oldest went first");
        assert_eq!(held(TERMINATED + 5), Some(WriteKind::Terminate));
    }

    #[test]
    fn one_archived_activity_read_runs_per_computer_name_and_archive() {
        let reads = ActivityReads::default();
        let first = reads.begin("studio", "alpha", "a1").unwrap();
        assert!(reads.begin("studio", "alpha", "a1").is_none());
        assert!(reads.begin("studio", "alpha", "a2").is_some());
        assert!(reads.begin("studio", "beta", "a1").is_some());
        assert!(reads.begin("laptop", "alpha", "a1").is_some());
        drop(first);
        assert!(reads.begin("studio", "alpha", "a1").is_some());
    }

    #[test]
    fn a_newer_archive_request_of_a_computer_cancels_the_older() {
        let reads = ArchiveReads::default();
        let first = reads.begin("studio", 1);
        let other = reads.begin("laptop", 2);
        assert!(!first.load(Ordering::Relaxed));
        let second = reads.begin("studio", 3);
        assert!(first.load(Ordering::Relaxed));
        assert!(!second.load(Ordering::Relaxed));
        assert!(
            !other.load(Ordering::Relaxed),
            "computers do not cancel each other"
        );
        reads.finish("studio", 1);
        let third = reads.begin("studio", 4);
        assert!(
            second.load(Ordering::Relaxed),
            "a stale finish forgot nothing"
        );
        assert!(!third.load(Ordering::Relaxed));
    }

    #[test]
    fn a_late_older_archive_request_never_cancels_the_newer() {
        let reads = ArchiveReads::default();
        let newer = reads.begin("studio", 5);
        let older = reads.begin("studio", 4);
        assert!(older.load(Ordering::Relaxed), "the older starts cancelled");
        assert!(!newer.load(Ordering::Relaxed));
        let duplicate = reads.begin("studio", 5);
        assert!(
            duplicate.load(Ordering::Relaxed),
            "an equal number is not newer"
        );
        assert!(!newer.load(Ordering::Relaxed));
        reads.finish("studio", 4);
        let next = reads.begin("studio", 6);
        assert!(
            newer.load(Ordering::Relaxed),
            "finishing the older kept the newer registered"
        );
        assert!(!next.load(Ordering::Relaxed));
    }

    #[test]
    fn cancelling_archive_pages_cancels_through_the_mark() {
        let reads = ArchiveReads::default();
        let studio = reads.begin("studio", 1);
        let laptop = reads.begin("laptop", 2);
        let later = reads.begin("desk", 4);
        reads.cancel_through(3);
        assert!(studio.load(Ordering::Relaxed));
        assert!(laptop.load(Ordering::Relaxed));
        assert!(
            !later.load(Ordering::Relaxed),
            "a request issued after the cancel keeps running"
        );
        // Issued before the cancel, started after it.
        assert!(reads.begin("studio", 3).load(Ordering::Relaxed));
        assert!(reads.begin("other", 2).load(Ordering::Relaxed));
        assert!(!reads.begin("studio", 5).load(Ordering::Relaxed));
        // A lower mark arriving late lowers nothing.
        reads.cancel_through(1);
        assert!(reads.begin("other", 3).load(Ordering::Relaxed));
        reads.finish("studio", 1);
        assert!(!later.load(Ordering::Relaxed));
    }

    #[test]
    fn replay_needs_an_explicit_hgs() {
        assert!(actions_allowed(None, None));
        assert!(actions_allowed(Some(""), None));
        assert!(actions_allowed(None, Some("/x/hgs")));
        assert!(!actions_allowed(Some("f.ndjson"), None));
        assert!(!actions_allowed(Some("f.ndjson"), Some("")));
        assert!(actions_allowed(Some("f.ndjson"), Some("/x/hgs")));
    }

    #[test]
    fn a_replay_keeps_its_own_read_marks() {
        assert_eq!(marks_file(false), "read-marks.json");
        assert_eq!(marks_file(true), "read-marks-replay.json");
    }

    #[test]
    fn attention_marks_keep_their_own_file() {
        assert_eq!(attention_file(false), "attention-marks.json");
        assert_eq!(attention_file(true), "attention-marks-replay.json");
    }

    #[test]
    fn queued_read_marks_are_merged_saved_and_wake_the_worker() {
        let path =
            std::env::temp_dir().join(format!("zerus-actions-queue-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let actions = Actions::new(path.clone());
        let mark = |name: &str, reply: Option<&str>, signature: Option<&str>| ReadMark {
            computer: "mac".into(),
            name: name.into(),
            run_id: "run-1".into(),
            conversation_id: "conv-1".into(),
            reply_id: reply.map(Into::into),
            attention_signature: signature.map(Into::into),
        };
        assert!(!actions.wake.wait(std::time::Duration::ZERO));
        let pending = actions.queue_marks(vec![
            mark("claude/sample-project/review", Some("r1"), None),
            mark("claude/sample-project/review", None, Some("s1")),
            // Nothing to mark: not stored (`ReadMarkStore::add`).
            mark("claude/sample-project/other", None, None),
        ]);
        let expected = vec![mark("claude/sample-project/review", Some("r1"), Some("s1"))];
        assert_eq!(pending, expected);
        assert_eq!(actions.pending_marks(), expected);
        // The command does no file I/O; the worker flushes before it sends.
        assert!(!path.exists());
        assert!(actions.wake.wait(std::time::Duration::ZERO));
        actions.flush_marks();
        assert_eq!(ReadMarks::load(path.clone()).pending(), expected);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn marks_for_computers_no_longer_configured_are_dropped_once_peers_are_known() {
        let path =
            std::env::temp_dir().join(format!("zerus-actions-orphans-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let actions = Actions::new(path.clone());
        let on = |computer: &str| ReadMark {
            computer: computer.into(),
            name: "claude/sample-project/review".into(),
            run_id: "run-1".into(),
            conversation_id: "conv-1".into(),
            reply_id: Some("r1".into()),
            attention_signature: None,
        };
        actions.queue_marks(vec![on(""), on("laptop"), on("removed")]);
        // Peers not known yet (no listing named them): nothing is dropped.
        actions.drop_orphans(|| None);
        assert_eq!(actions.pending_marks().len(), 3);
        let peers: BTreeSet<String> = ["laptop".to_owned()].into();
        actions.drop_orphans(|| Some(peers.clone()));
        assert_eq!(actions.pending_marks(), vec![on(""), on("laptop")]);
        assert_eq!(ReadMarks::load(path.clone()).pending().len(), 2);
        // Without marks the peer list is not even asked for.
        let empty = Actions::new(path.with_extension("empty"));
        empty.drop_orphans(|| panic!("asked for peers"));
        let _ = std::fs::remove_file(&path);
    }

    fn attention_path(test: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "zerus-actions-attention-{}-{test}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn attention_updates_merge_and_persist() {
        use zerus_client_core::marks::Mark;
        let path = attention_path("merge");
        let key = r#"["mac","claude","conv-1"]"#.to_owned();
        let other = r#"["mac","claude","conv-2"]"#.to_owned();
        let attention = Attention::new(path.clone());
        assert_eq!(attention.snapshot(), AttentionSnapshot::default());
        let review = Mark {
            review: true,
            acknowledged: String::new(),
        };
        attention.update(
            BTreeMap::from([
                (key.clone(), Some(review.clone())),
                (other.clone(), Some(Mark::default())),
            ]),
            BTreeMap::from([(key.clone(), Some("r1".to_owned()))]),
        );
        attention.update(BTreeMap::from([(other, None)]), BTreeMap::new());
        let expected = AttentionSnapshot {
            marks: BTreeMap::from([(key.clone(), review)]),
            replies: BTreeMap::from([(key.clone(), "r1".to_owned())]),
            order: AttentionOrder {
                marks: vec![key.clone()],
                replies: vec![key],
            },
        };
        assert_eq!(attention.snapshot(), expected);
        attention.flush();
        assert_eq!(Attention::new(path.clone()).snapshot(), expected);
        // What the frontend receives.
        assert_eq!(
            serde_json::to_value(&expected).unwrap(),
            serde_json::json!({
                "marks": {r#"["mac","claude","conv-1"]"#: {"review": true, "acknowledged": ""}},
                "replies": {r#"["mac","claude","conv-1"]"#: "r1"},
                "order": {"marks": [r#"["mac","claude","conv-1"]"#], "replies": [r#"["mac","claude","conv-1"]"#]},
            })
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn the_snapshot_lists_the_write_order_across_a_restart() {
        let path = attention_path("order");
        let (older, newer) = (
            r#"["mac","claude","conv-9"]"#,
            r#"["mac","claude","conv-1"]"#,
        );
        let attention = Attention::new(path.clone());
        let mark = |key: &str| BTreeMap::from([(key.to_owned(), Some(Mark::default()))]);
        let reply = |key: &str| BTreeMap::from([(key.to_owned(), Some("r".to_owned()))]);
        attention.update(mark(older), reply(older));
        attention.update(mark(newer), reply(newer));
        let order = AttentionOrder {
            marks: vec![older.to_owned(), newer.to_owned()],
            replies: vec![older.to_owned(), newer.to_owned()],
        };
        assert_eq!(attention.snapshot().order, order);
        attention.flush();
        drop(attention);
        assert_eq!(Attention::new(path.clone()).snapshot().order, order);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn flush_within_returns_at_once_when_nothing_is_dirty() {
        let path = attention_path("within-clean");
        let attention = Attention::new(path.clone());
        let started = std::time::Instant::now();
        assert!(attention.flush_within(std::time::Duration::from_secs(30)));
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
        assert!(!path.exists());
    }

    #[test]
    fn flush_within_is_true_once_a_pending_change_is_written() {
        let path = attention_path("within-write");
        let key = r#"["mac","claude","conv-1"]"#.to_owned();
        let attention = Attention::new(path.clone());
        attention.update(
            BTreeMap::new(),
            BTreeMap::from([(key.clone(), Some("r1".to_owned()))]),
        );
        assert!(attention.flush_within(std::time::Duration::from_secs(30)));
        assert_eq!(Attention::new(path.clone()).snapshot().replies[&key], "r1");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn flush_within_gives_up_at_the_limit_while_the_writer_is_busy() {
        let path = attention_path("within-busy");
        let attention = Attention::new(path.clone());
        attention.snapshot();
        // A write in progress that does not finish.
        attention.shared.lock().writing = true;
        let started = std::time::Instant::now();
        let limit = std::time::Duration::from_millis(100);
        assert!(!attention.flush_within(limit));
        let waited = started.elapsed();
        assert!(waited >= limit, "waited the whole limit: {waited:?}");
        assert!(waited < std::time::Duration::from_secs(10));
        // The write ends: everything is on file.
        attention.shared.lock().writing = false;
        attention.shared.signal.notify_all();
        assert!(attention.flush_within(std::time::Duration::from_secs(30)));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn the_writer_thread_saves_only_changes() {
        let path = attention_path("changes");
        let key = r#"["mac","claude","conv-1"]"#.to_owned();
        let attention = Attention::new(path.clone());
        let reply = |value: &str| BTreeMap::from([(key.clone(), Some(value.to_owned()))]);
        attention.update(BTreeMap::new(), reply("r1"));
        attention.flush();
        assert!(path.exists());
        std::fs::remove_file(&path).unwrap();
        // The value it holds already: no change, nothing written.
        attention.update(BTreeMap::new(), reply("r1"));
        attention.flush();
        assert!(!path.exists());
        attention.update(BTreeMap::new(), reply("r2"));
        attention.flush();
        assert_eq!(Attention::new(path.clone()).snapshot().replies[&key], "r2");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn the_file_is_read_off_the_calling_thread_and_written_when_dropped() {
        let path = attention_path("load");
        std::fs::write(&path, r#"{"replies":[["k","r1"]]}"#).unwrap();
        let attention = Attention::new(path.clone());
        // The first answer waits for the writer thread's read.
        assert_eq!(
            attention.snapshot().replies,
            BTreeMap::from([("k".to_owned(), "r1".to_owned())])
        );
        attention.update(
            BTreeMap::new(),
            BTreeMap::from([("k".to_owned(), Some("r2".to_owned()))]),
        );
        drop(attention);
        // Dropped with a change pending: the writer still saves it.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while Attention::new(path.clone()).snapshot().replies["k"] != "r2" {
            assert!(std::time::Instant::now() < deadline, "the change was saved");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn the_smoke_needs_replay_hgs_and_perf() {
        assert!(smoke_allowed(Some("f"), Some("h"), Some("smoke")));
        assert!(!smoke_allowed(None, Some("h"), Some("smoke")));
        assert!(!smoke_allowed(Some("f"), None, Some("smoke")));
        assert!(!smoke_allowed(Some("f"), Some("h"), None));
        assert!(!smoke_allowed(Some("f"), Some("h"), Some("")));
    }

    #[test]
    fn an_empty_archive_identity_is_refused_before_any_claim_or_process() {
        let target = Target {
            computer: "mac".into(),
            name: "claude/sample-project/review".into(),
            run_id: "r".into(),
            conversation_id: "c".into(),
        };
        for action in [
            Action::Restore {
                archive_id: String::new(),
            },
            Action::ForgetArchive {
                archive_id: String::new(),
            },
        ] {
            assert_eq!(
                refused_before_claim(&target, &action),
                Some("Missing archive identity".to_owned())
            );
        }
        for action in [
            Action::Restore {
                archive_id: "a1".into(),
            },
            Action::ForgetArchive {
                archive_id: "a1".into(),
            },
            Action::Pause,
        ] {
            assert_eq!(refused_before_claim(&target, &action), None);
        }
    }

    #[test]
    fn one_action_per_session_and_lane_at_a_time() {
        let key = |computer: &str, lane| {
            (
                computer.to_owned(),
                "claude/sample-project/review".to_owned(),
                lane,
                String::new(),
            )
        };
        let flight = InFlight::default();
        let first = flight.claim(key("", Lane::Write)).unwrap();
        assert!(flight.claim(key("", Lane::Write)).is_none());
        assert!(flight.claim(key("mac", Lane::Write)).is_some());
        // A quiet read-mark retry never blocks a confirmed write, nor the reverse.
        let read = flight.claim(key("", Lane::Read)).unwrap();
        assert!(flight.claim(key("", Lane::Read)).is_none());
        drop(first);
        assert!(flight.claim(key("", Lane::Write)).is_some());
        drop(read);
        assert!(flight.claim(key("", Lane::Read)).is_some());
    }

    #[test]
    fn a_notification_change_has_its_own_lane() {
        let actions = Actions::new(std::env::temp_dir().join("zerus-lane-test-marks.json"));
        let write = actions.claim_lane("", "a/b/c", Lane::Write).unwrap();
        let change = actions
            .claim_lane("", "a/b/c", Lane::Notifications)
            .unwrap();
        assert!(actions
            .claim_lane("", "a/b/c", Lane::Notifications)
            .is_none());
        assert!(actions.claim_lane("", "a/b/c", Lane::Write).is_none());
        drop(write);
        assert!(actions.claim_lane("", "a/b/c", Lane::Write).is_some());
        drop(change);
        assert!(actions
            .claim_lane("", "a/b/c", Lane::Notifications)
            .is_some());
    }

    #[test]
    fn mark_read_and_interrupt_take_their_own_lanes() {
        assert_eq!(
            Lane::of(&Action::MarkRead {
                reply_id: None,
                attention_signature: None
            }),
            Lane::Read
        );
        assert_eq!(
            Lane::of(&Action::Interrupt { turn_started: 1.0 }),
            Lane::Interrupt
        );
        for action in [
            Action::Pause,
            Action::Terminate,
            Action::Kill,
            Action::StartFresh,
            Action::Forget,
            Action::ForgetBinding,
            Action::Restore {
                archive_id: "a1".into(),
            },
            Action::ForgetArchive {
                archive_id: "a1".into(),
            },
            Action::PauseAll { count: 1 },
            Action::ResumeAll { count: 1 },
            Action::StageFiles {
                paths: vec![],
                expected_created: None,
            },
            // Clear and Compact start after the view's confirmation and run
            // as session writes.
            Action::ClearContext {
                dismiss_prompt: true,
            },
            Action::CompactContext,
            Action::StartFresh,
        ] {
            assert_eq!(Lane::of(&action), Lane::Write);
        }
    }

    #[test]
    fn a_message_takes_its_own_lane_per_agent() {
        let target = Target {
            computer: String::new(),
            name: "claude/sample-project/review".into(),
            run_id: "run-1".into(),
            conversation_id: "conv-1".into(),
        };
        let send = |agent: Option<&str>| Action::Send {
            text: "hi".into(),
            attachments: vec![],
            agent_id: agent.map(str::to_owned),
            expected_compaction_id: None,
        };
        assert_eq!(Lane::of(&send(None)), Lane::Send);
        let flight = InFlight::default();
        let main = flight.claim(Lane::key(&target, &send(None))).unwrap();
        assert!(flight.claim(Lane::key(&target, &send(None))).is_none());
        // A subagent's message and the session's other lanes stay free.
        assert!(flight
            .claim(Lane::key(&target, &send(Some("agent-7"))))
            .is_some());
        assert!(flight.claim(Lane::key(&target, &Action::Pause)).is_some());
        drop(main);
        assert!(flight.claim(Lane::key(&target, &send(None))).is_some());
    }

    #[test]
    fn a_process_stop_runs_beside_session_writes_one_at_a_time() {
        let target = Target {
            computer: "studio".into(),
            name: "claude/sample-project/review".into(),
            run_id: "run-1".into(),
            conversation_id: "conv-1".into(),
        };
        let stop = |id: &str| Action::StopProcess {
            process_id: id.into(),
            archive_id: None,
            generation: None,
        };
        assert_eq!(Lane::of(&stop("job-1")), Lane::Process);
        let flight = InFlight::default();
        let pause = flight.claim(Lane::key(&target, &Action::Pause)).unwrap();
        let first = flight.claim(Lane::key(&target, &stop("job-1"))).unwrap();
        // A second stop for the same session waits, whichever process it names.
        assert!(flight.claim(Lane::key(&target, &stop("job-2"))).is_none());
        drop(first);
        assert!(flight.claim(Lane::key(&target, &stop("job-2"))).is_some());
        drop(pause);
    }

    #[test]
    fn one_answer_per_session() {
        let target = |computer: &str| Target {
            computer: computer.into(),
            name: "claude/sample-project/review".into(),
            run_id: "run-1".into(),
            conversation_id: "conv-1".into(),
        };
        let answer = |id: &str| Action::Answer {
            question: serde_json::json!({"question_id": id}),
            answers: Vec::new(),
        };
        assert_eq!(Lane::of(&answer("q-1")), Lane::Answer);
        let flight = InFlight::default();
        let send = flight
            .claim(Lane::key(
                &target("studio"),
                &Action::Send {
                    text: "hi".into(),
                    attachments: Vec::new(),
                    agent_id: None,
                    expected_compaction_id: None,
                },
            ))
            .unwrap();
        let first = flight
            .claim(Lane::key(&target("studio"), &answer("q-1")))
            .unwrap();
        // A second answer to the same session waits, whichever question.
        assert!(flight
            .claim(Lane::key(&target("studio"), &answer("q-2")))
            .is_none());
        assert!(flight
            .claim(Lane::key(&target("mac"), &answer("q-2")))
            .is_some());
        drop((send, first));
    }

    #[test]
    fn send_now_and_inbox_removal_take_their_own_lanes() {
        let target = Target {
            computer: "studio".into(),
            name: "claude/sample-project/review".into(),
            run_id: "run-1".into(),
            conversation_id: "conv-1".into(),
        };
        let send_now = |id: &str| Action::SendNow {
            queue_id: id.into(),
        };
        let remove = |id: &str| Action::RemoveQueued {
            message_id: id.into(),
        };
        assert_eq!(Lane::of(&send_now("q-1")), Lane::Queue);
        assert_eq!(Lane::of(&remove("msg_a")), Lane::Inbox);
        let flight = InFlight::default();
        let write = flight.claim(Lane::key(&target, &Action::Pause)).unwrap();
        let queue = flight.claim(Lane::key(&target, &send_now("q-1"))).unwrap();
        let inbox = flight.claim(Lane::key(&target, &remove("msg_a"))).unwrap();
        // One Send now and one removal per session, whichever queue or message.
        assert!(flight.claim(Lane::key(&target, &send_now("q-2"))).is_none());
        assert!(flight.claim(Lane::key(&target, &remove("msg_b"))).is_none());
        drop((write, queue, inbox));
    }

    #[test]
    fn a_snapshot_drops_marks_of_sessions_it_no_longer_lists() {
        use zerus_client_core::readmarks::ReadMark;
        use zerus_client_core::{ComputerId, ReplaceOut, RowOut, ViewRow};
        let path =
            std::env::temp_dir().join(format!("zerus-actions-observe-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let actions = Actions::new(path.clone());
        let mark = |name: &str| ReadMark {
            computer: "mac".into(),
            name: name.into(),
            run_id: "run-1".into(),
            conversation_id: "conv-1".into(),
            reply_id: Some("r1".into()),
            attention_signature: None,
        };
        {
            let mut marks = actions.marks.lock().unwrap();
            marks.add(mark("claude/sample-project/review"));
            marks.add(mark("claude/sample-project/gone"));
        }
        let row = |name: &str| RowOut {
            computer: ComputerId::from("mac"),
            key: format!("run:{name}"),
            row: ViewRow {
                name: name.into(),
                run_id: Some("run-1".into()),
                conversation_id: Some("conv-1".into()),
                reply_id: Some("r1".into()),
                ..ViewRow::default()
            },
        };
        let batch = Batch {
            replaces: vec![ReplaceOut {
                computer: ComputerId::from("mac"),
                rows: vec![row("claude/sample-project/review")],
            }],
            ..Batch::default()
        };
        actions.observe(&batch);
        let left: Vec<String> = actions
            .marks
            .lock()
            .unwrap()
            .pending()
            .into_iter()
            .map(|m| m.name)
            .collect();
        assert_eq!(left, ["claude/sample-project/review"]);
        // A stopped stream or a resend sends an empty replace: not a listing.
        actions.observe(&Batch {
            replaces: vec![ReplaceOut {
                computer: ComputerId::from("mac"),
                rows: Vec::new(),
            }],
            ..Batch::default()
        });
        assert_eq!(actions.marks.lock().unwrap().pending().len(), 1);
        let _ = std::fs::remove_file(&path);
    }

    /// A listener that counts its calls.
    fn counted(listener: &MarksListener) -> Arc<std::sync::atomic::AtomicUsize> {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = Arc::clone(&calls);
        listener.set(Box::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }));
        calls
    }

    #[test]
    fn every_change_of_the_pending_read_marks_is_heard_once() {
        use zerus_client_core::{ComputerId, ReplaceOut, RowOut, ViewRow};
        let path = std::env::temp_dir().join(format!(
            "zerus-actions-listener-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let actions = Actions::new(path.clone());
        let calls = counted(&actions.listener);
        let heard = || calls.swap(0, Ordering::SeqCst);
        let mark = |computer: &str, name: &str| ReadMark {
            computer: computer.into(),
            name: name.into(),
            run_id: "run-1".into(),
            conversation_id: "conv-1".into(),
            reply_id: Some("r1".into()),
            attention_signature: None,
        };
        actions.queue_marks(vec![
            mark("mac", "claude/sample-project/a"),
            mark("gone", "claude/sample-project/b"),
        ]);
        assert_eq!(heard(), 1, "queued marks: one call for the call");
        let mut empty = mark("mac", "claude/sample-project/a");
        empty.reply_id = None;
        actions.queue_marks(vec![empty]);
        assert_eq!(heard(), 0, "nothing to mark changes nothing");
        actions.drop_orphans(|| Some(["mac".to_owned(), "gone".to_owned()].into()));
        assert_eq!(heard(), 0, "every peer still listed");
        actions.drop_orphans(|| Some(["mac".to_owned()].into()));
        assert_eq!(heard(), 1, "a removed peer's mark");
        let row = |read: Option<&str>| RowOut {
            computer: ComputerId::from("mac"),
            key: "run:a".into(),
            row: ViewRow {
                name: "claude/sample-project/a".into(),
                run_id: Some("run-1".into()),
                conversation_id: Some("conv-1".into()),
                reply_id: Some("r1".into()),
                read_reply_id: read.map(Into::into),
                ..ViewRow::default()
            },
        };
        let upsert = |read: Option<&str>| Batch {
            upserts: vec![row(read)],
            ..Batch::default()
        };
        actions.observe(&upsert(None));
        assert_eq!(heard(), 0, "the host has not shown it yet");
        actions.observe(&upsert(Some("r1")));
        assert_eq!(heard(), 1, "the mark settled");
        assert!(actions.pending_marks().is_empty());
        actions.queue_marks(vec![mark("mac", "claude/sample-project/c")]);
        heard();
        actions.observe(&Batch {
            replaces: vec![ReplaceOut {
                computer: ComputerId::from("mac"),
                rows: vec![row(None)],
            }],
            ..Batch::default()
        });
        assert_eq!(heard(), 1, "a listing without the session drops its mark");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn every_change_of_the_attention_marks_is_heard_once() {
        let path = attention_path("listener");
        let attention = Attention::new(path.clone());
        let calls = counted(&attention.listener);
        let key = r#"["mac","claude","conv-1"]"#.to_owned();
        let review = Mark {
            review: true,
            acknowledged: String::new(),
        };
        attention.update(
            BTreeMap::from([(key.clone(), Some(review.clone()))]),
            BTreeMap::new(),
        );
        assert_eq!(calls.swap(0, Ordering::SeqCst), 1);
        attention.update(
            BTreeMap::from([(key.clone(), Some(review.clone()))]),
            BTreeMap::new(),
        );
        assert_eq!(calls.swap(0, Ordering::SeqCst), 0, "the same mark");
        attention.update(
            BTreeMap::new(),
            BTreeMap::from([(key.clone(), Some("r1".to_owned()))]),
        );
        assert_eq!(calls.swap(0, Ordering::SeqCst), 1, "a read reply");
        let (version, marks, replies) = attention.marks_since(None).expect("a first copy");
        assert_eq!(marks, BTreeMap::from([(key.clone(), review.clone())]));
        assert_eq!(replies, BTreeMap::from([(key.clone(), "r1".to_owned())]));
        assert_eq!(
            attention.marks_since(Some(version)),
            None,
            "unchanged: nothing is copied"
        );
        attention.update(
            BTreeMap::from([(key.clone(), Some(review))]),
            BTreeMap::new(),
        );
        assert_eq!(
            attention.marks_since(Some(version)),
            None,
            "an update that changes nothing keeps the version"
        );
        attention.update(BTreeMap::from([(key, None)]), BTreeMap::new());
        let (next, marks, _) = attention.marks_since(Some(version)).expect("changed");
        assert_ne!(next, version);
        assert!(marks.is_empty());
        attention.flush();
        let _ = std::fs::remove_file(&path);
    }
}
