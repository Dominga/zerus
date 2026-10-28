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
//! stop at a time per session.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};

use tauri::State;
use zerus_client_core::actions::{
    change_notifications, classify, invocation, parse_reference, reference_invocation,
    ALREADY_SENDING,
};
use zerus_client_core::details::{details_invocation, parse_details, Details};
use zerus_client_core::marks::{AttentionMarks, Mark};
use zerus_client_core::readmarks::{ReadMark, ReadMarks};
use zerus_client_core::reads::{self, ArchivePage};
use zerus_client_core::shell::{current_shell, open_command};
use zerus_client_core::{default_hgs, run, Action, Batch, ComputerId, Outcome, Target};

/// A session by computer id and session name, the action's lane and, for a
/// message, the agent it goes to ("" for the session's own agent).
type Key = (String, String, Lane, String);

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
}

impl Lane {
    pub fn of(action: &Action) -> Self {
        match action {
            Action::MarkRead { .. } => Self::Read,
            Action::Interrupt { .. } => Self::Interrupt,
            Action::Send { .. } => Self::Send,
            Action::StopProcess { .. } => Self::Process,
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

/// The managed state of the action commands; clones share it.
#[derive(Clone)]
pub struct Actions {
    flight: InFlight,
    marks: Arc<Mutex<ReadMarks>>,
    /// Wakes the read-mark delivery worker (`readsync.rs`).
    pub(crate) wake: crate::readsync::Wake,
}

impl Actions {
    /// `marks_path` is the read-marks file; a missing one gives no marks.
    pub fn new(marks_path: PathBuf) -> Self {
        Self {
            flight: InFlight::default(),
            marks: Arc::new(Mutex::new(ReadMarks::load(marks_path))),
            wake: crate::readsync::Wake::default(),
        }
    }

    /// `queue_read_marks`: merges each mark into the pending ones
    /// (`ReadMarkStore::add`), wakes the delivery worker and returns what is
    /// pending. No file I/O: the worker writes the marks before it sends any
    /// ([`Actions::flush_marks`]).
    pub fn queue_marks(&self, marks: Vec<ReadMark>) -> Vec<ReadMark> {
        let pending = {
            let mut store = self.marks.lock().unwrap_or_else(PoisonError::into_inner);
            store.add_all(marks);
            store.pending()
        };
        self.wake.notify();
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
        self.marks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drop_orphans(&peers);
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
        for out in batch.upserts.iter().chain(
            batch
                .replaces
                .iter()
                .flat_map(|replace| replace.rows.iter()),
        ) {
            marks.settle(&out.computer.0, &out.row);
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
            marks.drop_unlisted(&replace.computer.0, &names);
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
}

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
        Self { shared }
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
        let mut state = self.shared.loaded();
        if let Some(store) = state.marks.as_mut() {
            if store.update(marks, replies) {
                state.dirty = true;
                self.shared.signal.notify_all();
            }
        }
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

/// The refusal of a replay that would reach the real CLI.
const REPLAY_NEEDS_HGS: &str = "Actions in replay mode need ZERUS_CLIENT_HGS.";

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
    if blocked() {
        return Ok(failed(REPLAY_NEEDS_HGS));
    }
    if let Some(refusal) = refused_before_claim(&target, &action) {
        return Ok(failed(refusal));
    }
    // Pause all and Resume all are computer-wide: one per computer at a time,
    // whatever session name the caller sent.
    let mut key = Lane::key(&target, &action);
    if matches!(action, Action::PauseAll { .. } | Action::ResumeAll { .. }) {
        key.1 = String::new();
    }
    let Some(_claim) = state.flight.claim(key) else {
        return match Lane::of(&action) {
            Lane::Send => Ok(failed(ALREADY_SENDING)),
            _ => Err("busy".into()),
        };
    };
    let request_id = uuid::Uuid::new_v4().to_string();
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
        let exit = run(&hgs, &inv);
        Ok::<_, String>((inv, exit))
    })
    .await
    .map_err(|error| error.to_string())?;
    match ran {
        Ok((inv, exit)) => Ok(classify(&target, &action, &inv, exit)),
        Err(refusal) => Ok(failed(refusal)),
    }
}

/// Changes the shared notification preferences of a session (Mute and Unmute)
/// and returns the host's confirmed `preferences`. One change per session at a
/// time, in its own lane (`Err("busy")` otherwise). The core probes the local
/// `hgs --help` first.
#[tauri::command]
pub async fn set_notifications(
    state: State<'_, Actions>,
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
    tauri::async_runtime::spawn_blocking(move || {
        change_notifications(&hgs, &target, muted, expected_revision)
    })
    .await
    .map_err(|error| error.to_string())?
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
}
