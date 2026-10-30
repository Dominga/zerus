//! One computer's `hgs [@host] watch --json`, kept running.
//!
//! [`spawn`] starts a supervisor thread for one computer. Before every start it
//! probes `hgs [@host] -h` for [`HELP_MARKER`]: without it the computer is
//! [`ComputerStatus::Unsupported`], no stream starts and the probe repeats every
//! 10 minutes. A stream keeps its stdin open (EOF ends the server), its stdout
//! is read line by line on a reader thread and decoded with
//! `zerus_watch_protocol::decode`, and its stderr is drained. When the stream
//! ends, fails, sends a fatal `error` or stays silent for 30 s, the computer is
//! [`ComputerStatus::Reconnecting`] with its rows kept, and the next start
//! follows after 5 s, 15 s, 60 s, then every 5 minutes (failures more than ten
//! minutes apart start over). Followed sessions are subscribed again on every
//! new stream, after their last cursor, and so are followed processes when the
//! stream offers that topic. A followed organization comes from a stream that
//! offers its topic; while none does, this computer's is polled with
//! `hgs swarm get` every 5 s.
use crate::{state::CatalogState, Change, ComputerId, ComputerStatus};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use zerus_watch_protocol::{
    decode, topic, Body, Control, Frame, FrameError, SessionRef, Subscription,
    ACTIVITY_SUBSCRIPTIONS, ACTIVITY_TAIL, HELP_MARKER, MAX_FRAME_BYTES, PROCESSES_SUBSCRIPTIONS,
    TASKS_SUBSCRIPTIONS, VERSION,
};

/// Lines and commands a supervisor may have queued; a full queue makes the
/// reader wait, so a fast server cannot outgrow the client's memory.
const QUEUE_LINES: usize = 1024;
/// The most help text a probe reads.
const PROBE_BYTES: u64 = 1 << 20;

/// The delays and bounds of a stream. [`Timing::scaled`] shrinks the restart
/// and re-probe delays for tests.
#[derive(Clone, Debug, PartialEq)]
pub struct Timing {
    /// The delay before restart 1, 2, 3 and every later one.
    pub restart: [Duration; 4],
    /// A stream without any frame for this long restarts.
    pub silence: Duration,
    /// How often a computer without the marker is probed again.
    pub reprobe: Duration,
    /// How long `hgs -h` may take on this computer and on a peer.
    pub probe_timeout: Duration,
    pub peer_probe_timeout: Duration,
    /// How long a stream may take to exit after EOF on its stdin.
    pub stop_grace: Duration,
    /// How often a followed organization without a stream offering it is
    /// polled with `hgs swarm get`.
    pub organization_poll: Duration,
    /// How often a computer without the stream is polled (see
    /// [`crate::legacy`]).
    pub legacy: crate::legacy::Intervals,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            restart: [5, 15, 60, 300].map(Duration::from_secs),
            silence: Duration::from_secs(30),
            reprobe: Duration::from_secs(600),
            probe_timeout: Duration::from_secs(5),
            peer_probe_timeout: Duration::from_secs(15),
            stop_grace: Duration::from_secs(1),
            organization_poll: crate::organization::POLL,
            legacy: crate::legacy::Intervals::default(),
        }
    }
}

impl Timing {
    /// The default timing with restart, re-probe and organization poll
    /// delays times `factor`.
    pub fn scaled(factor: f64) -> Self {
        let default = Self::default();
        Self {
            restart: default.restart.map(|delay| delay.mul_f64(factor)),
            reprobe: default.reprobe.mul_f64(factor),
            organization_poll: default.organization_poll.mul_f64(factor),
            legacy: crate::legacy::Intervals {
                listing: default.legacy.listing.mul_f64(factor),
                activity: default.legacy.activity.mul_f64(factor),
                processes: default.legacy.processes.mul_f64(factor),
            },
            ..default
        }
    }

    /// The delay before the restart after `attempt` failures.
    pub fn restart_delay(&self, attempt: u32) -> Duration {
        let index = attempt.saturating_sub(1).min(3) as usize;
        self.restart[index]
    }

    fn probe_timeout(&self, computer: &ComputerId) -> Duration {
        if computer.is_local() {
            self.probe_timeout
        } else {
            self.peer_probe_timeout
        }
    }
}

/// What a supervisor reports.
// Nearly every event is a change; it moves through the channel unboxed.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
pub enum StreamEvent {
    Change(Change),
    /// The SSH aliases this computer's `catalog.meta` lists in `peers`; sent
    /// for this computer only, and only when `peers` is an array.
    Peers {
        computer: ComputerId,
        peers: Vec<String>,
    },
}

/// Why a follow was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FollowError {
    /// The fleet has no such computer.
    UnknownComputer,
    /// The key is not the session's catalog key.
    KeyMismatch,
    /// The computer already follows [`ACTIVITY_SUBSCRIPTIONS`] sessions (for
    /// processes [`PROCESSES_SUBSCRIPTIONS`], for tasks [`TASKS_SUBSCRIPTIONS`]
    /// projects, or the stream's lower limit).
    TooMany,
    /// The stream was stopped.
    Stopped,
    /// Not a project ID: 1 to 128 bytes without control characters.
    InvalidProject,
}

impl std::fmt::Display for FollowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::UnknownComputer => "unknown computer",
            Self::KeyMismatch => "the key is not the session's catalog key",
            Self::TooMany => "the computer already follows as many as its stream allows",
            Self::Stopped => "the stream was stopped",
            Self::InvalidProject => "a project ID is 1 to 128 bytes without control characters",
        })
    }
}

impl std::error::Error for FollowError {}

/// The answer of `hgs [@host] -h`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Probe {
    /// The help text has the watch-v1 marker.
    Supported,
    /// The CLI answered without the marker (an older `hgs`).
    Unsupported,
    /// It did not answer: it failed to start, exited with an error or timed out.
    Failed(String),
}

/// `hgs [@alias] <args>` without an agent's attribution identity: the client
/// acts for the person even when it was started from an agent's shell.
pub(crate) fn hgs_command(hgs: &Path, computer: &ComputerId, args: &[&str]) -> Command {
    let mut command = Command::new(hgs);
    if let Some(alias) = computer.alias() {
        command.arg(format!("@{alias}"));
    }
    command
        .args(args)
        .env_remove("HGS_SESSION")
        .env_remove("HGS_RUN_ID");
    command
}

/// Runs `hgs [@host] -h` and looks for [`HELP_MARKER`].
pub fn probe(hgs: &Path, computer: &ComputerId, timeout: Duration) -> Probe {
    probe_until(hgs, computer, timeout, &AtomicBool::new(false))
}

fn probe_until(hgs: &Path, computer: &ComputerId, timeout: Duration, stop: &AtomicBool) -> Probe {
    let mut command = hgs_command(hgs, computer, &["-h"]);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => return Probe::Failed(format!("cannot start hgs: {error}")),
    };
    let (tx, rx) = mpsc::channel();
    if let Some(stdout) = child.stdout.take() {
        thread::spawn(move || {
            let mut text = Vec::new();
            let _ = stdout.take(PROBE_BYTES).read_to_end(&mut text);
            let _ = tx.send(text);
        });
    }
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline && !stop.load(Ordering::Relaxed) => {
                thread::sleep(Duration::from_millis(10));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let Some(status) = status else {
        return Probe::Failed("hgs -h did not answer in time".into());
    };
    if !status.success() {
        return Probe::Failed(format!("hgs -h exited with {status}"));
    }
    let text = rx.recv_timeout(Duration::from_secs(1)).unwrap_or_default();
    if String::from_utf8_lossy(&text).contains(HELP_MARKER) {
        Probe::Supported
    } else {
        Probe::Unsupported
    }
}

/// A line of the stream, classified.
#[derive(Debug, PartialEq)]
pub(crate) enum Line {
    Frame(Box<Frame>),
    /// A frame type or shape this client does not know.
    Skip,
    /// The stream cannot continue: not JSON, another version or too large.
    Fatal(String),
}

pub(crate) fn classify(line: &str) -> Line {
    if line.trim().is_empty() {
        return Line::Skip;
    }
    match decode(line) {
        Ok(frame) if frame.v == VERSION => Line::Frame(Box::new(frame)),
        Ok(frame) => Line::Fatal(format!("unsupported stream version {}", frame.v)),
        Err(FrameError::TooLarge(bytes)) => Line::Fatal(format!("a frame of {bytes} bytes")),
        Err(_) => match serde_json::from_str::<Value>(line) {
            Ok(Value::Object(frame)) if frame.get("v").and_then(Value::as_u64) == Some(1) => {
                // A fatal error always applies, even in a shape not known yet.
                if frame.get("t").and_then(Value::as_str) == Some("error")
                    && frame.get("fatal").and_then(Value::as_bool) == Some(true)
                {
                    Line::Fatal("fatal error".into())
                } else {
                    Line::Skip
                }
            }
            Ok(Value::Object(_)) => Line::Fatal("unsupported stream version".into()),
            _ => Line::Fatal("invalid frame".into()),
        },
    }
}

#[derive(Debug, PartialEq)]
pub(crate) enum LineRead {
    Line,
    Eof,
    TooLong,
}

/// Reads one line without its newline into `out`, never holding more than
/// `max` bytes of it. A last line without a newline still counts.
pub(crate) fn read_line(
    reader: &mut impl BufRead,
    out: &mut Vec<u8>,
    max: usize,
) -> io::Result<LineRead> {
    out.clear();
    loop {
        let available = match reader.fill_buf() {
            Ok(available) => available,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if available.is_empty() {
            return Ok(if out.is_empty() {
                LineRead::Eof
            } else {
                LineRead::Line
            });
        }
        match available.iter().position(|byte| *byte == b'\n') {
            Some(end) => {
                let fits = out.len() + end < max;
                if fits {
                    out.extend_from_slice(&available[..end]);
                }
                reader.consume(end + 1);
                return Ok(if fits {
                    LineRead::Line
                } else {
                    LineRead::TooLong
                });
            }
            None => {
                let length = available.len();
                if out.len() + length >= max {
                    return Ok(LineRead::TooLong);
                }
                out.extend_from_slice(available);
                reader.consume(length);
            }
        }
    }
}

enum Input {
    Reconnect,
    Line {
        generation: u64,
        line: String,
    },
    Closed {
        generation: u64,
    },
    Subscribe {
        key: String,
    },
    Unsubscribe {
        key: String,
        session: SessionRef,
    },
    SubscribeProcesses {
        key: String,
    },
    UnsubscribeProcesses {
        key: String,
        session: SessionRef,
    },
    RefreshProcesses {
        key: String,
    },
    SubscribeTasks {
        key: String,
    },
    UnsubscribeTasks {
        key: String,
    },
    SubscribeOrganization,
    UnsubscribeOrganization,
    /// A `swarm get` of the organization poll.
    OrganizationPolled(Result<Value, crate::ReadError>),
    /// A read of the polling of a computer without the stream.
    Legacy {
        read: crate::legacy::Read,
        result: Result<Value, String>,
    },
    Control(Control),
    /// Preferences the host confirmed for a Mute or Unmute.
    Notifications {
        target: crate::actions::Target,
        preferences: Value,
    },
    Stop,
}

/// The organization edit snapshot the supervisor keeps current.
type OrganizationShared = Arc<Mutex<Option<crate::OrganizationSnapshot>>>;

/// One coalesced wake of the existing offline supervisor. A full input queue
/// already wakes the receiver, which checks this state before waiting again.
#[derive(Default)]
struct Retry(AtomicU8);

impl Retry {
    const WAITING: u8 = 1;
    const REQUESTED: u8 = 2;

    fn request(&self, tx: &SyncSender<Input>) -> bool {
        if self
            .0
            .compare_exchange(
                Self::WAITING,
                Self::REQUESTED,
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .is_err()
        {
            return false;
        }
        match tx.try_send(Input::Reconnect) {
            Ok(()) | Err(TrySendError::Full(_)) => true,
            Err(TrySendError::Disconnected(_)) => {
                self.begin();
                false
            }
        }
    }

    fn waiting(&self) {
        let _ = self
            .0
            .compare_exchange(0, Self::WAITING, Ordering::SeqCst, Ordering::SeqCst);
    }

    fn begin(&self) {
        self.0.store(0, Ordering::SeqCst);
    }

    fn requested(&self) -> bool {
        self.0.load(Ordering::SeqCst) == Self::REQUESTED
    }
}

/// Polls `hgs swarm get` on its own thread until dropped.
struct Poller {
    stop: Arc<AtomicBool>,
}

impl Drop for Poller {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[derive(Clone, Debug)]
struct Follow {
    session: SessionRef,
    after: Option<i64>,
    tail: u32,
}

type Follows = Arc<Mutex<BTreeMap<String, Follow>>>;
#[derive(Clone, Debug)]
struct ProcessFollow {
    session: SessionRef,
    interval_ms: Option<u64>,
}

/// Sessions whose processes are followed, by key, with their requested cadence.
type ProcessFollows = Arc<Mutex<BTreeMap<String, ProcessFollow>>>;
/// Projects whose task lists are followed.
type TaskFollows = Arc<Mutex<BTreeSet<String>>>;

fn lock<T>(follows: &Mutex<T>) -> MutexGuard<'_, T> {
    follows
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Controls one computer's supervisor; dropping it stops the stream.
pub struct StreamHandle {
    computer: ComputerId,
    tx: SyncSender<Input>,
    follows: Follows,
    processes: ProcessFollows,
    /// The processes follows the latest stream accepts.
    processes_limit: Arc<AtomicUsize>,
    tasks: TaskFollows,
    /// The tasks follows the latest stream accepts.
    tasks_limit: Arc<AtomicUsize>,
    organization: Arc<AtomicBool>,
    organization_snapshot: OrganizationShared,
    stop: Arc<AtomicBool>,
    retry: Arc<Retry>,
    thread: Option<JoinHandle<()>>,
}

impl StreamHandle {
    /// Requests one immediate offline retry without replacing this supervisor,
    /// its catalog or subscriptions. False online, stopped or already pending.
    pub fn reconnect(&self) -> bool {
        !self.stopped() && self.retry.request(&self.tx)
    }

    pub fn computer(&self) -> &ComputerId {
        &self.computer
    }

    /// Writes a control line to the running stream (a `ping`, say); false
    /// once the supervisor stopped. Activity goes through [`Self::follow`].
    pub fn send(&self, control: Control) -> bool {
        !self.stopped() && self.tx.send(Input::Control(control)).is_ok()
    }

    /// Applies the preferences the host confirmed for a Mute or Unmute to the
    /// session's row at once (`FleetState::applyNotificationPreferences`);
    /// the row goes out as a change. False once the supervisor stopped.
    pub fn apply_notifications(&self, target: crate::actions::Target, preferences: Value) -> bool {
        !self.stopped()
            && self
                .tx
                .send(Input::Notifications {
                    target,
                    preferences,
                })
                .is_ok()
    }

    /// Follows a session's Activity on this computer, now and on every later
    /// stream; returns its key. Without `after`, it starts with the newest
    /// `tail` events (at most 100). At most 16 sessions per computer.
    pub fn follow(
        &self,
        session: SessionRef,
        after: Option<i64>,
        tail: u32,
    ) -> Result<String, FollowError> {
        if self.stopped() {
            return Err(FollowError::Stopped);
        }
        let key = session.key();
        {
            let mut follows = lock(&self.follows);
            if let Some(old) = follows.get(&key) {
                if old.session == session {
                    return Ok(key);
                }
            } else if follows.len() >= ACTIVITY_SUBSCRIPTIONS {
                return Err(FollowError::TooMany);
            }
            follows.insert(
                key.clone(),
                Follow {
                    session,
                    after: after.filter(|after| *after > 0),
                    tail: tail.min(ACTIVITY_TAIL as u32),
                },
            );
        }
        self.tx
            .send(Input::Subscribe { key: key.clone() })
            .map_err(|_| FollowError::Stopped)?;
        Ok(key)
    }

    /// Stops following; false when the key was not followed.
    pub fn unfollow(&self, key: &str) -> bool {
        let Some(follow) = lock(&self.follows).remove(key) else {
            return false;
        };
        let _ = self.tx.send(Input::Unsubscribe {
            key: key.to_owned(),
            session: follow.session,
        });
        true
    }

    /// The followed keys.
    pub fn followed(&self) -> Vec<String> {
        lock(&self.follows).keys().cloned().collect()
    }

    /// Follows a session's processes on this computer, now and on every later
    /// stream that offers the topic; returns its key. Independent of
    /// [`Self::follow`]. At most [`PROCESSES_SUBSCRIPTIONS`] per computer, or
    /// the lower limit of the latest stream.
    pub fn follow_processes(&self, session: SessionRef) -> Result<String, FollowError> {
        if self.stopped() {
            return Err(FollowError::Stopped);
        }
        let key = session.key();
        {
            let mut follows = lock(&self.processes);
            if let Some(old) = follows.get(&key) {
                if old.session == session {
                    return Ok(key);
                }
            } else if follows.len() >= self.processes_limit.load(Ordering::Relaxed) {
                return Err(FollowError::TooMany);
            }
            let interval_ms = follows.get(&key).and_then(|follow| follow.interval_ms);
            follows.insert(
                key.clone(),
                ProcessFollow {
                    session,
                    interval_ms,
                },
            );
        }
        self.tx
            .send(Input::SubscribeProcesses { key: key.clone() })
            .map_err(|_| FollowError::Stopped)?;
        Ok(key)
    }

    /// Changes a followed session's cadence without dropping its current state.
    /// The request survives reconnects; older watch servers keep their default.
    pub fn set_processes_interval(&self, key: &str, interval_ms: u64) -> Result<(), FollowError> {
        if self.stopped() {
            return Err(FollowError::Stopped);
        }
        {
            let mut follows = lock(&self.processes);
            let follow = follows.get_mut(key).ok_or(FollowError::KeyMismatch)?;
            let interval = Some(interval_ms.clamp(1_000, 600_000));
            if follow.interval_ms == interval {
                return Ok(());
            }
            follow.interval_ms = interval;
        }
        self.tx
            .send(Input::SubscribeProcesses { key: key.into() })
            .map_err(|_| FollowError::Stopped)
    }

    /// Requests one fresh inventory for an existing subscription. Repeated
    /// requests coalesce on the server and retain the current state meanwhile.
    pub fn refresh_processes(&self, key: &str) -> Result<(), FollowError> {
        if self.stopped() {
            return Err(FollowError::Stopped);
        }
        if !lock(&self.processes).contains_key(key) {
            return Err(FollowError::KeyMismatch);
        }
        self.tx
            .send(Input::RefreshProcesses { key: key.into() })
            .map_err(|_| FollowError::Stopped)
    }

    /// Stops following a session's processes and drops their state without an
    /// update; false when the key was not followed.
    pub fn unfollow_processes(&self, key: &str) -> bool {
        let Some(session) = lock(&self.processes).remove(key) else {
            return false;
        };
        let _ = self.tx.send(Input::UnsubscribeProcesses {
            key: key.to_owned(),
            session: session.session,
        });
        true
    }

    /// The keys whose processes are followed.
    pub fn followed_processes(&self) -> Vec<String> {
        lock(&self.processes).keys().cloned().collect()
    }

    /// Follows a project's task list on this computer, now and on every later
    /// stream that offers the topic; returns its key, the project ID. At most
    /// [`TASKS_SUBSCRIPTIONS`] per computer, or the lower limit of the latest
    /// stream.
    pub fn follow_tasks(&self, project: &str) -> Result<String, FollowError> {
        if self.stopped() {
            return Err(FollowError::Stopped);
        }
        if !crate::tasks::valid_project(project) {
            return Err(FollowError::InvalidProject);
        }
        {
            let mut follows = lock(&self.tasks);
            if follows.contains(project) {
                return Ok(project.to_owned());
            }
            if follows.len() >= self.tasks_limit.load(Ordering::Relaxed) {
                return Err(FollowError::TooMany);
            }
            follows.insert(project.to_owned());
        }
        self.tx
            .send(Input::SubscribeTasks {
                key: project.to_owned(),
            })
            .map_err(|_| FollowError::Stopped)?;
        Ok(project.to_owned())
    }

    /// Stops following a project's task list and forgets its digest, so a
    /// later follow delivers the list again; false when it was not followed.
    pub fn unfollow_tasks(&self, project: &str) -> bool {
        if !lock(&self.tasks).remove(project) {
            return false;
        }
        let _ = self.tx.send(Input::UnsubscribeTasks {
            key: project.to_owned(),
        });
        true
    }

    /// The projects whose task lists are followed.
    pub fn followed_tasks(&self) -> Vec<String> {
        lock(&self.tasks).iter().cloned().collect()
    }

    /// Follows this computer's organization (see [`crate::organization`]):
    /// from a stream that offers the topic, otherwise by polling
    /// `hgs swarm get` every 5 s when the computer is this one.
    pub fn follow_organization(&self) -> Result<(), FollowError> {
        if self.stopped() {
            return Err(FollowError::Stopped);
        }
        if !self.organization.swap(true, Ordering::Relaxed) {
            self.tx
                .send(Input::SubscribeOrganization)
                .map_err(|_| FollowError::Stopped)?;
        }
        Ok(())
    }

    /// Stops following the organization and drops it without an update;
    /// false when it was not followed.
    pub fn unfollow_organization(&self) -> bool {
        if !self.organization.swap(false, Ordering::Relaxed) {
            return false;
        }
        *lock(&self.organization_snapshot) = None;
        let _ = self.tx.send(Input::UnsubscribeOrganization);
        true
    }

    /// The organization the view shows and the versions observed with it,
    /// for an edit; None before the first state.
    pub fn organization_snapshot(&self) -> Option<crate::OrganizationSnapshot> {
        lock(&self.organization_snapshot).clone()
    }

    /// Ends the stream (EOF on its stdin), reports the computer
    /// [`ComputerStatus::Ended`] with no rows, and waits for the supervisor.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.tx.send(Input::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for StreamHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Starts following one computer; see the module documentation.
pub fn spawn(
    hgs: &Path,
    computer: &ComputerId,
    timing: Timing,
    sink: Sender<StreamEvent>,
) -> StreamHandle {
    let (tx, rx) = mpsc::sync_channel(QUEUE_LINES);
    let follows = Follows::default();
    let processes = ProcessFollows::default();
    let processes_limit = Arc::new(AtomicUsize::new(PROCESSES_SUBSCRIPTIONS));
    let tasks = TaskFollows::default();
    let tasks_limit = Arc::new(AtomicUsize::new(TASKS_SUBSCRIPTIONS));
    let organization = Arc::new(AtomicBool::new(false));
    let organization_snapshot = OrganizationShared::default();
    let stop = Arc::new(AtomicBool::new(false));
    let retry = Arc::new(Retry::default());
    let supervisor = Supervisor {
        hgs: hgs.to_owned(),
        computer: computer.clone(),
        timing,
        sink,
        rx,
        tx: tx.clone(),
        follows: follows.clone(),
        processes: processes.clone(),
        processes_limit: processes_limit.clone(),
        tasks: tasks.clone(),
        tasks_limit: tasks_limit.clone(),
        organization_snapshot: organization_snapshot.clone(),
        poller: None,
        worker_fixed: None,
        legacy: None,
        legacy_windows: BTreeSet::new(),
        stop: stop.clone(),
        retry: retry.clone(),
        state: CatalogState::new(),
        generation: 0,
        child: None,
        stdin: None,
        greeted: false,
        last_frame: Instant::now(),
    };
    let name = match computer.alias() {
        Some(alias) => format!("hgs-watch-{alias}"),
        None => "hgs-watch".to_owned(),
    };
    let thread = thread::Builder::new()
        .name(name)
        .spawn(move || supervisor.run())
        .expect("start a stream supervisor");
    StreamHandle {
        computer: computer.clone(),
        tx,
        follows,
        processes,
        processes_limit,
        tasks,
        tasks_limit,
        organization,
        organization_snapshot,
        stop,
        retry,
        thread: Some(thread),
    }
}

#[derive(PartialEq)]
enum Outcome {
    Ended,
    Stopped,
}

struct Supervisor {
    hgs: PathBuf,
    computer: ComputerId,
    timing: Timing,
    sink: Sender<StreamEvent>,
    rx: Receiver<Input>,
    tx: SyncSender<Input>,
    follows: Follows,
    processes: ProcessFollows,
    processes_limit: Arc<AtomicUsize>,
    tasks: TaskFollows,
    tasks_limit: Arc<AtomicUsize>,
    organization_snapshot: OrganizationShared,
    /// Polls `swarm get` while no stream offers the organization.
    poller: Option<Poller>,
    /// When an outdated catalog worker last got its `swarm get`.
    worker_fixed: Option<Instant>,
    /// The polling of this computer while its CLI lacks the stream; kept
    /// across re-probes and dropped when a stream starts.
    legacy: Option<crate::legacy::Schedule>,
    /// Followed sessions whose first window the polling sent.
    legacy_windows: BTreeSet<String>,
    stop: Arc<AtomicBool>,
    retry: Arc<Retry>,
    state: CatalogState,
    generation: u64,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    /// The running stream sent its `hello`.
    greeted: bool,
    last_frame: Instant,
}

impl Supervisor {
    fn run(mut self) {
        self.status(ComputerStatus::Probing);
        loop {
            if self.stop.load(Ordering::Relaxed) {
                break;
            }
            // A retry is in flight now; extra Refresh presses coalesce with it.
            self.retry.begin();
            self.state.begin();
            let timeout = self.timing.probe_timeout(&self.computer);
            match probe_until(&self.hgs, &self.computer, timeout, &self.stop) {
                Probe::Supported => {
                    self.legacy = None;
                    self.legacy_windows.clear();
                }
                Probe::Unsupported => {
                    // Polled already: the status stays until a listing fails.
                    if !self
                        .legacy
                        .as_ref()
                        .is_some_and(|legacy| legacy.has_listed())
                    {
                        self.status(ComputerStatus::Unsupported);
                    }
                    if self.poll(self.timing.reprobe) {
                        continue;
                    }
                    break;
                }
                Probe::Failed(_) => {
                    if self.stop.load(Ordering::Relaxed) || !self.fail() {
                        break;
                    }
                    continue;
                }
            }
            if self.launch().is_err() {
                if !self.fail() {
                    break;
                }
                continue;
            }
            let outcome = self.pump();
            self.close();
            if outcome == Outcome::Stopped || !self.fail() {
                break;
            }
        }
        self.close();
        self.poller = None;
        if let Some(change) = self.state.organization_mut().end(&self.computer) {
            *lock(&self.organization_snapshot) = None;
            self.emit(change);
        }
        self.emit(Change::Replace {
            computer: self.computer.clone(),
            rows: Vec::new(),
        });
        self.status(ComputerStatus::Ended);
    }

    /// Applies confirmed notification preferences to the session's row and
    /// sends the changed row on, as a frame's changes go.
    fn notifications(&mut self, target: &crate::actions::Target, preferences: &Value) {
        for change in self
            .state
            .apply_notification_preferences(&self.computer, target, preferences)
        {
            self.emit(change);
        }
    }

    fn emit(&self, change: Change) {
        if let Change::Computer { status, .. } = &change {
            if matches!(status, ComputerStatus::Reconnecting { .. }) {
                self.retry.waiting();
            } else {
                self.retry.begin();
            }
        }
        let _ = self.sink.send(StreamEvent::Change(change));
    }

    fn status(&self, status: ComputerStatus) {
        self.emit(Change::Computer {
            computer: self.computer.clone(),
            status,
        });
    }

    /// Reports the failure of this attempt and waits out its delay; false
    /// when the stream was stopped meanwhile.
    fn fail(&mut self) -> bool {
        for change in self.state.end(&self.computer, Instant::now()) {
            self.emit(change);
        }
        self.wait(self.timing.restart_delay(self.state.attempt()))
    }

    /// Waits without a stream, keeping follows current; false on stop.
    fn wait(&mut self, time: Duration) -> bool {
        // No stream: a followed organization is polled meanwhile.
        self.sync_poller();
        let deadline = Instant::now() + time;
        loop {
            if self.retry.requested() {
                return true;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return true;
            }
            match self.rx.recv_timeout(left) {
                Ok(Input::Stop) | Err(RecvTimeoutError::Disconnected) => return false,
                Ok(Input::Subscribe { key }) => self.state.follow(&key),
                Ok(Input::Unsubscribe { key, .. }) => self.state.unfollow(&key),
                Ok(Input::SubscribeProcesses { key }) => self.state.processes_mut().follow(&key),
                Ok(Input::UnsubscribeProcesses { key, .. }) => {
                    self.state.processes_mut().unfollow(&key)
                }
                Ok(Input::SubscribeTasks { key }) => self.state.tasks_mut().follow(&key),
                Ok(Input::UnsubscribeTasks { key }) => self.state.tasks_mut().unfollow(&key),
                Ok(Input::SubscribeOrganization) => self.subscribe_organization(),
                Ok(Input::UnsubscribeOrganization) => self.unsubscribe_organization(),
                Ok(Input::OrganizationPolled(result)) => self.organization_polled(result),
                Ok(Input::Notifications {
                    target,
                    preferences,
                }) => self.notifications(&target, &preferences),
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => return true,
            }
        }
    }

    /// Waits like [`Self::wait`] while polling this computer as the Qt client
    /// does (see [`crate::legacy`]); false on stop.
    fn poll(&mut self, time: Duration) -> bool {
        self.sync_poller();
        let deadline = Instant::now() + time;
        loop {
            let now = Instant::now();
            if now >= deadline {
                return true;
            }
            if self.retry.requested() {
                self.retry.begin();
                if let Some(legacy) = &mut self.legacy {
                    legacy.refresh_listing(now);
                }
            }
            let activity: Vec<(String, String, Option<i64>)> = lock(&self.follows)
                .iter()
                .filter(|(key, _)| self.state.is_followed(key))
                .map(|(key, follow)| {
                    let after = self
                        .legacy_windows
                        .contains(key)
                        .then(|| self.state.cursor(key).unwrap_or(0));
                    (key.clone(), follow.session.name.clone(), after)
                })
                .collect();
            let processes: Vec<(String, String)> = lock(&self.processes)
                .iter()
                .filter(|(key, _)| self.state.processes().is_followed(key))
                .map(|(key, follow)| (key.clone(), follow.session.name.clone()))
                .collect();
            let follows = crate::legacy::Follows {
                activity: &activity,
                processes: &processes,
            };
            let every = self.timing.legacy;
            let legacy = self
                .legacy
                .get_or_insert_with(|| crate::legacy::Schedule::new(every));
            let reads = legacy.due(now, &follows);
            // A capability re-probe temporarily disabled Refresh. Restore it
            // from the listing's outcome, unless a scheduled read owns it now.
            if legacy.listing_retryable() {
                self.retry.waiting();
            } else {
                self.retry.begin();
            }
            for read in reads {
                self.spawn_legacy(read);
            }
            let wake = self
                .legacy
                .as_ref()
                .and_then(crate::legacy::Schedule::next)
                .map_or(deadline, |at| at.min(deadline));
            let left = wake
                .saturating_duration_since(Instant::now())
                .max(Duration::from_millis(1));
            match self.rx.recv_timeout(left) {
                Ok(Input::Stop) | Err(RecvTimeoutError::Disconnected) => return false,
                Ok(Input::Subscribe { key }) => self.state.follow(&key),
                Ok(Input::Unsubscribe { key, .. }) => {
                    self.state.unfollow(&key);
                    self.legacy_windows.remove(&key);
                }
                Ok(Input::SubscribeProcesses { key }) => self.state.processes_mut().follow(&key),
                Ok(Input::RefreshProcesses { key }) => {
                    if let Some(schedule) = &mut self.legacy {
                        schedule.refresh_processes(&key, Instant::now());
                    }
                }
                Ok(Input::UnsubscribeProcesses { key, .. }) => {
                    self.state.processes_mut().unfollow(&key)
                }
                Ok(Input::SubscribeTasks { key }) => self.state.tasks_mut().follow(&key),
                Ok(Input::UnsubscribeTasks { key }) => self.state.tasks_mut().unfollow(&key),
                Ok(Input::SubscribeOrganization) => self.subscribe_organization(),
                Ok(Input::UnsubscribeOrganization) => self.unsubscribe_organization(),
                Ok(Input::OrganizationPolled(result)) => self.organization_polled(result),
                Ok(Input::Legacy { read, result }) => self.legacy_reply(read, result),
                Ok(Input::Notifications {
                    target,
                    preferences,
                }) => self.notifications(&target, &preferences),
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
    }

    /// Runs one polling read on its own thread; the reply comes back as input.
    fn spawn_legacy(&mut self, read: crate::legacy::Read) {
        let (hgs, computer, tx) = (self.hgs.clone(), self.computer.clone(), self.tx.clone());
        let failed = read.clone();
        let spawned = thread::Builder::new()
            .name("hgs-legacy-poll".into())
            .spawn(move || {
                let result = crate::legacy::run(&hgs, &computer, &read);
                let _ = tx.send(Input::Legacy { read, result });
            });
        if spawned.is_err() {
            if let Some(legacy) = self.legacy.as_mut() {
                legacy.done(&failed, Instant::now());
            }
        }
    }

    /// Applies a polling reply as the stream's frames would be.
    fn legacy_reply(&mut self, read: crate::legacy::Read, result: Result<Value, String>) {
        let Some(legacy) = self.legacy.as_mut() else {
            return;
        };
        legacy.done(&read, Instant::now());
        let computer = self.computer.clone();
        let changes = match (&read, result) {
            (crate::legacy::Read::Listing, Ok(listing)) => {
                let (_, first) = legacy.listed(true);
                let mut changes = self.state.listing(&computer, &listing);
                // This computer's listing names the peers, as its meta would.
                if computer.is_local() {
                    if let Some(peers) = self.state.peers() {
                        let _ = self.sink.send(StreamEvent::Peers {
                            computer: computer.clone(),
                            peers,
                        });
                    }
                }
                if first {
                    changes.push(Change::Computer {
                        computer: computer.clone(),
                        status: ComputerStatus::Polled,
                    });
                }
                changes
            }
            (crate::legacy::Read::Listing, Err(_)) => {
                let (attempt, _) = legacy.listed(false);
                vec![Change::Computer {
                    computer: computer.clone(),
                    status: ComputerStatus::Reconnecting {
                        attempt: attempt.unwrap_or(1),
                    },
                }]
            }
            (crate::legacy::Read::Activity { key, .. }, Ok(inspection)) => {
                let run = lock(&self.follows)
                    .get(key)
                    .and_then(|follow| follow.session.run_id.clone());
                let window = !self.legacy_windows.contains(key);
                let changes =
                    self.state
                        .activity_polled(&computer, key, run.as_deref(), &inspection, window);
                if self.state.is_followed(key) {
                    self.legacy_windows.insert(key.clone());
                }
                changes
            }
            (crate::legacy::Read::Processes { key, .. }, Ok(inspection)) => self
                .state
                .processes_polled(&computer, key, &inspection)
                .into_iter()
                .collect(),
            (crate::legacy::Read::Processes { key, .. }, Err(message)) => {
                let error = zerus_watch_protocol::ErrorFrame {
                    code: "inspect_failed".into(),
                    message,
                    topic: Some(topic::PROCESSES.into()),
                    key: Some(key.clone()),
                    fatal: false,
                };
                self.state
                    .processes_mut()
                    .error(&computer, &error)
                    .into_iter()
                    .collect()
            }
            // A failed Activity read is retried on the next tick.
            (crate::legacy::Read::Activity { .. }, Err(_)) => Vec::new(),
        };
        for change in changes {
            match &change {
                Change::Activity {
                    key,
                    update: crate::ActivityUpdate::Ended,
                    ..
                } => {
                    lock(&self.follows).remove(key);
                    self.legacy_windows.remove(key);
                }
                Change::Processes {
                    key,
                    update: crate::ProcessesUpdate::Ended,
                    ..
                } => {
                    lock(&self.processes).remove(key);
                }
                _ => {}
            }
            self.emit(change);
        }
    }

    fn launch(&mut self) -> io::Result<()> {
        let mut command = hgs_command(&self.hgs, &self.computer, &["watch", "--json"]);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn()?;
        self.generation += 1;
        let generation = self.generation;
        if let Some(stdout) = child.stdout.take() {
            let tx = self.tx.clone();
            thread::spawn(move || read_stream(stdout, generation, tx));
        }
        if let Some(mut stderr) = child.stderr.take() {
            thread::spawn(move || {
                let _ = io::copy(&mut stderr, &mut io::sink());
            });
        }
        self.stdin = child.stdin.take();
        self.child = Some(child);
        self.greeted = false;
        self.last_frame = Instant::now();
        Ok(())
    }

    /// Runs the stream until it ends or the handle stops it.
    fn pump(&mut self) -> Outcome {
        loop {
            let left = self
                .timing
                .silence
                .saturating_sub(self.last_frame.elapsed());
            if left.is_zero() {
                return Outcome::Ended;
            }
            match self.rx.recv_timeout(left) {
                Ok(Input::Line { generation, line }) if generation == self.generation => {
                    self.last_frame = Instant::now();
                    match classify(&line) {
                        Line::Frame(frame) => {
                            self.handle(&frame);
                            if self.state.failed() {
                                return Outcome::Ended;
                            }
                        }
                        Line::Skip => {}
                        Line::Fatal(_) => return Outcome::Ended,
                    }
                }
                Ok(Input::Closed { generation }) if generation == self.generation => {
                    return Outcome::Ended;
                }
                Ok(Input::Line { .. } | Input::Closed { .. }) => {}
                Ok(Input::Subscribe { key }) => self.subscribe(&key),
                Ok(Input::Unsubscribe { key, session }) => {
                    self.state.unfollow(&key);
                    if self.greeted {
                        self.write(&Control::Unsubscribe(Subscription {
                            topic: topic::ACTIVITY.into(),
                            session: Some(session),
                            ..Subscription::default()
                        }));
                    }
                }
                Ok(Input::SubscribeProcesses { key }) => self.subscribe_processes(&key),
                Ok(Input::RefreshProcesses { key }) => self.send_processes_subscription(&key, true),
                Ok(Input::UnsubscribeProcesses { key, session }) => {
                    self.state.processes_mut().unfollow(&key);
                    self.unsubscribe_processes(session);
                }
                Ok(Input::SubscribeOrganization) => self.subscribe_organization(),
                Ok(Input::UnsubscribeOrganization) => self.unsubscribe_organization(),
                Ok(Input::OrganizationPolled(result)) => self.organization_polled(result),
                // A polling read that finished after the stream started.
                Ok(Input::Legacy { .. }) => {}
                Ok(Input::SubscribeTasks { key }) => self.subscribe_tasks(&key),
                Ok(Input::UnsubscribeTasks { key }) => {
                    self.state.tasks_mut().unfollow(&key);
                    self.unsubscribe_tasks(&key);
                }
                Ok(Input::Control(control)) => self.write(&control),
                // A queued wake from an older attempt must never restart a
                // source that is online now, or a later offline attempt.
                Ok(Input::Reconnect) => {}
                Ok(Input::Notifications {
                    target,
                    preferences,
                }) => self.notifications(&target, &preferences),
                Ok(Input::Stop) | Err(RecvTimeoutError::Disconnected) => return Outcome::Stopped,
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
    }

    fn handle(&mut self, frame: &Frame) {
        let changes = self.state.apply(&self.computer, frame);
        for change in changes {
            if matches!(change, Change::Organization { .. }) {
                self.organization_changed();
            }
            match &change {
                Change::Activity {
                    key,
                    update: crate::ActivityUpdate::Ended,
                    ..
                } => {
                    lock(&self.follows).remove(key);
                }
                Change::Processes {
                    key,
                    update: crate::ProcessesUpdate::Ended,
                    ..
                } => {
                    // The server may still follow a key the catalog dropped.
                    let removed = lock(&self.processes).remove(key);
                    if let Some(follow) = removed {
                        self.unsubscribe_processes(follow.session);
                    }
                }
                Change::Tasks {
                    key,
                    update: crate::TasksUpdate::Ended,
                    ..
                } => {
                    lock(&self.tasks).remove(key);
                }
                _ => {}
            }
            self.emit(change);
        }
        match &frame.body {
            Body::Hello(_) => {
                self.greeted = true;
                let limit = self
                    .state
                    .processes_limit()
                    .map_or(PROCESSES_SUBSCRIPTIONS, |limit| {
                        limit.min(PROCESSES_SUBSCRIPTIONS)
                    });
                self.processes_limit.store(limit, Ordering::Relaxed);
                let limit = self
                    .state
                    .tasks_limit()
                    .map_or(TASKS_SUBSCRIPTIONS, |limit| limit.min(TASKS_SUBSCRIPTIONS));
                self.tasks_limit.store(limit, Ordering::Relaxed);
                let keys: Vec<String> = lock(&self.follows).keys().cloned().collect();
                for key in keys {
                    self.subscribe(&key);
                }
                let keys: Vec<String> = lock(&self.processes).keys().cloned().collect();
                for key in keys {
                    self.subscribe_processes(&key);
                }
                let keys: Vec<String> = lock(&self.tasks).iter().cloned().collect();
                for key in keys {
                    self.subscribe_tasks(&key);
                }
                if self.state.organization().is_followed() {
                    self.subscribe_organization();
                }
            }
            Body::Snapshot(snapshot)
                if snapshot.topic == topic::CATALOG_META
                    && snapshot.last
                    && self.computer.is_local() =>
            {
                if let Some(peers) = self.state.peers() {
                    let _ = self.sink.send(StreamEvent::Peers {
                        computer: self.computer.clone(),
                        peers,
                    });
                }
            }
            _ => {}
        }
    }

    /// Accepts the key's frames and, on a greeted stream, subscribes it after
    /// its last cursor (or the caller's), else with its tail.
    fn subscribe(&mut self, key: &str) {
        let Some(follow) = lock(&self.follows).get(key).cloned() else {
            return;
        };
        self.state.follow(key);
        if !self.greeted {
            return;
        }
        let after = self
            .state
            .cursor(key)
            .filter(|cursor| *cursor > 0)
            .or(follow.after);
        self.write(&Control::Subscribe(Subscription {
            topic: topic::ACTIVITY.into(),
            session: Some(follow.session),
            after,
            tail: after.is_none().then_some(follow.tail),
            ..Subscription::default()
        }));
    }

    /// Accepts the key's processes frames and, on a greeted stream that offers
    /// the topic, subscribes it. A key the live catalog lacks ends at once, and
    /// so does one beyond the stream's limit.
    fn subscribe_processes(&mut self, key: &str) {
        if !lock(&self.processes).contains_key(key) {
            return;
        }
        self.state.processes_mut().follow(key);
        if !self.greeted {
            return;
        }
        let Some(limit) = self.state.processes_limit() else {
            return;
        };
        let beyond = self.state.processes().keys().position(|known| known == key) >= Some(limit);
        if (self.state.live() && !self.state.rows().contains_key(key)) || beyond {
            if let Some(change) = self.state.processes_mut().end(&self.computer, key) {
                lock(&self.processes).remove(key);
                self.emit(change);
            }
            return;
        }
        self.send_processes_subscription(key, false);
    }

    fn send_processes_subscription(&mut self, key: &str, refresh: bool) {
        if !self.greeted || self.state.processes_limit().is_none() {
            return;
        }
        let Some(follow) = lock(&self.processes).get(key).cloned() else {
            return;
        };
        self.write(&Control::Subscribe(Subscription {
            topic: topic::PROCESSES.into(),
            session: Some(follow.session),
            interval_ms: follow.interval_ms,
            refresh: refresh.then_some(true),
            ..Subscription::default()
        }));
    }

    /// Whether the running stream delivers the organization.
    fn streams_organization(&self) -> bool {
        self.greeted && self.state.organization_offered()
    }

    /// Polls this computer's `swarm get` while a followed organization has no
    /// stream that offers it, and stops otherwise.
    fn sync_poller(&mut self) {
        let wanted = self.state.organization().is_followed()
            && !self.streams_organization()
            && self.computer.is_local()
            && !self.stop.load(Ordering::Relaxed);
        if !wanted {
            self.poller = None;
            return;
        }
        if self.poller.is_some() {
            return;
        }
        let stop = Arc::new(AtomicBool::new(false));
        let (hgs, tx, flag) = (self.hgs.clone(), self.tx.clone(), stop.clone());
        let every = self.timing.organization_poll;
        let spawned = thread::Builder::new()
            .name("hgs-swarm-poll".into())
            .spawn(move || {
                while !flag.load(Ordering::Relaxed) {
                    let reply = crate::reads::read(&hgs, &ComputerId::local(), &crate::Read::Swarm);
                    if flag.load(Ordering::Relaxed)
                        || tx.send(Input::OrganizationPolled(reply)).is_err()
                    {
                        return;
                    }
                    let until = Instant::now() + every;
                    while Instant::now() < until && !flag.load(Ordering::Relaxed) {
                        thread::sleep(every.min(Duration::from_millis(50)));
                    }
                }
            });
        if spawned.is_ok() {
            self.poller = Some(Poller { stop });
        }
    }

    /// Follows the organization from the running stream when it offers the
    /// topic, otherwise by polling.
    fn subscribe_organization(&mut self) {
        self.state.organization_mut().follow();
        if self.streams_organization() {
            self.write(&Control::Subscribe(Subscription {
                topic: topic::ORGANIZATION.into(),
                ..Subscription::default()
            }));
        }
        self.sync_poller();
    }

    fn unsubscribe_organization(&mut self) {
        if self.streams_organization() {
            self.write(&Control::Unsubscribe(Subscription {
                topic: topic::ORGANIZATION.into(),
                ..Subscription::default()
            }));
        }
        self.state.organization_mut().unfollow();
        *lock(&self.organization_snapshot) = None;
        self.sync_poller();
    }

    /// A poll's reply; one that arrives after the stream took over is late.
    fn organization_polled(&mut self, reply: Result<Value, crate::ReadError>) {
        if self.streams_organization() {
            return;
        }
        let computer = self.computer.clone();
        let organization = self.state.organization_mut();
        let change = match reply {
            Ok(value) => organization.polled(&computer, value),
            Err(error) => {
                let code = serde_json::to_value(&error)
                    .ok()
                    .and_then(|value| value["error"].as_str().map(str::to_owned))
                    .unwrap_or_else(|| "failed".into());
                organization.failed(&computer, &code, &error.to_string())
            }
        };
        if let Some(change) = change {
            self.organization_changed();
            self.emit(change);
        }
    }

    /// Keeps the edit snapshot current and gives an outdated catalog worker
    /// one `swarm get`, which restarts it as before (at most every 10 min).
    fn organization_changed(&mut self) {
        *lock(&self.organization_snapshot) = self.state.organization().snapshot();
        let due = self
            .worker_fixed
            .is_none_or(|at| at.elapsed() >= Duration::from_secs(600));
        if self.computer.is_local() && due && self.state.organization().worker_outdated() {
            self.worker_fixed = Some(Instant::now());
            let hgs = self.hgs.clone();
            let _ = thread::Builder::new()
                .name("hgs-swarm-worker".into())
                .spawn(move || {
                    let _ = crate::reads::read(&hgs, &ComputerId::local(), &crate::Read::Swarm);
                });
        }
    }

    /// Accepts the project's tasks frames and, on a greeted stream that offers
    /// the topic, subscribes it; one beyond the stream's limit ends at once.
    fn subscribe_tasks(&mut self, key: &str) {
        if !lock(&self.tasks).contains(key) {
            return;
        }
        self.state.tasks_mut().follow(key);
        if !self.greeted {
            return;
        }
        let Some(limit) = self.state.tasks_limit() else {
            return;
        };
        if self.state.tasks().keys().position(|known| known == key) >= Some(limit) {
            if let Some(change) = self.state.tasks_mut().end(&self.computer, key) {
                lock(&self.tasks).remove(key);
                self.emit(change);
            }
            return;
        }
        self.write(&Control::Subscribe(Subscription {
            topic: topic::TASKS.into(),
            project: Some(key.to_owned()),
            ..Subscription::default()
        }));
    }

    fn unsubscribe_tasks(&mut self, key: &str) {
        if self.greeted && self.state.tasks_limit().is_some() {
            self.write(&Control::Unsubscribe(Subscription {
                topic: topic::TASKS.into(),
                project: Some(key.to_owned()),
                ..Subscription::default()
            }));
        }
    }

    fn unsubscribe_processes(&mut self, session: SessionRef) {
        if self.greeted && self.state.processes_limit().is_some() {
            self.write(&Control::Unsubscribe(Subscription {
                topic: topic::PROCESSES.into(),
                session: Some(session),
                ..Subscription::default()
            }));
        }
    }

    fn write(&mut self, control: &Control) {
        let Some(stdin) = self.stdin.as_mut() else {
            return;
        };
        let Ok(mut line) = serde_json::to_string(control) else {
            return;
        };
        line.push('\n');
        // A failed write shows as the end of the stream.
        let _ = stdin
            .write_all(line.as_bytes())
            .and_then(|()| stdin.flush());
    }

    /// Ends the running stream: EOF first, then a kill after the grace time.
    fn close(&mut self) {
        self.stdin = None;
        self.greeted = false;
        self.sync_poller();
        let Some(mut child) = self.child.take() else {
            return;
        };
        let deadline = Instant::now() + self.timing.stop_grace;
        loop {
            match child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return;
                }
            }
        }
    }
}

fn read_stream(stdout: impl io::Read, generation: u64, tx: SyncSender<Input>) {
    let mut reader = BufReader::with_capacity(64 << 10, stdout);
    let mut line = Vec::new();
    loop {
        match read_line(&mut reader, &mut line, MAX_FRAME_BYTES) {
            Ok(LineRead::Line) => {
                let text = String::from_utf8_lossy(&line).into_owned();
                if tx
                    .send(Input::Line {
                        generation,
                        line: text,
                    })
                    .is_err()
                {
                    return;
                }
            }
            Ok(LineRead::Eof | LineRead::TooLong) | Err(_) => {
                let _ = tx.send(Input::Closed { generation });
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reconnect_is_coalesced_and_still_pending_when_the_input_queue_is_full() {
        let retry = Retry::default();
        let (tx, rx) = mpsc::sync_channel(1);
        tx.send(Input::Reconnect).unwrap();
        retry.waiting();
        assert!(retry.request(&tx));
        for _ in 0..1000 {
            assert!(!retry.request(&tx));
        }
        assert!(
            retry.requested(),
            "the existing queued input wakes the waiter"
        );
        assert!(rx.try_recv().is_ok());
        assert!(rx.try_recv().is_err(), "no unbounded wake messages");
        retry.begin();
        assert!(
            !retry.request(&tx),
            "an in-flight/online source is unchanged"
        );
        retry.waiting();
        assert!(
            !retry.requested(),
            "an old queued wake never retries a later failure"
        );
    }

    #[test]
    fn a_reconnect_after_the_receiver_ends_is_not_left_pending() {
        let retry = Retry::default();
        let (tx, rx) = mpsc::sync_channel(1);
        drop(rx);
        retry.waiting();
        assert!(!retry.request(&tx));
        assert!(!retry.requested());
    }

    #[test]
    fn restart_delays_are_5_15_60_then_300_seconds() {
        let timing = Timing::default();
        let delays: Vec<u64> = (0..7)
            .map(|attempt| timing.restart_delay(attempt).as_secs())
            .collect();
        assert_eq!(delays, vec![5, 5, 15, 60, 300, 300, 300]);
        let scaled = Timing::scaled(0.001);
        assert_eq!(scaled.restart_delay(1), Duration::from_millis(5));
        assert_eq!(scaled.restart_delay(4), Duration::from_millis(300));
        assert_eq!(scaled.reprobe, Duration::from_millis(600));
        assert_eq!(scaled.organization_poll, Duration::from_millis(5));
        assert_eq!(scaled.silence, Duration::from_secs(30));
    }

    #[test]
    fn lines_classify_by_version_and_known_types() {
        assert!(matches!(
            classify(r#"{"v":1,"seq":1,"t":"heartbeat","at":1.0}"#),
            Line::Frame(_)
        ));
        assert_eq!(
            classify(r#"{"v":1,"seq":2,"t":"sparkle","glitter":true}"#),
            Line::Skip
        );
        assert_eq!(
            classify(r#"{"v":1,"seq":3,"t":"gap"}"#),
            Line::Skip,
            "a known type in an unknown shape"
        );
        assert_eq!(classify(""), Line::Skip);
        assert!(matches!(
            classify(r#"{"v":1,"seq":4,"t":"error","fatal":true}"#),
            Line::Fatal(_)
        ));
        assert!(matches!(
            classify(r#"{"v":2,"seq":5,"t":"heartbeat","at":1.0}"#),
            Line::Fatal(_)
        ));
        assert!(matches!(
            classify(r#"{"seq":5,"t":"heartbeat"}"#),
            Line::Fatal(_)
        ));
        assert!(matches!(classify("not json"), Line::Fatal(_)));
        assert!(matches!(classify("[1,2]"), Line::Fatal(_)));
        let large = format!(
            r#"{{"v":1,"seq":6,"t":"heartbeat","pad":"{}"}}"#,
            "x".repeat(MAX_FRAME_BYTES)
        );
        assert!(matches!(classify(&large), Line::Fatal(_)));
    }

    #[test]
    fn lines_are_read_whole_and_bounded() {
        let mut reader = BufReader::with_capacity(4, &b"first\nsecond line\n\nlast"[..]);
        let mut line = Vec::new();
        let mut read = Vec::new();
        loop {
            match read_line(&mut reader, &mut line, 64).unwrap() {
                LineRead::Eof => break,
                kind => read.push((kind, String::from_utf8(line.clone()).unwrap())),
            }
        }
        assert_eq!(
            read,
            vec![
                (LineRead::Line, "first".into()),
                (LineRead::Line, "second line".into()),
                (LineRead::Line, String::new()),
                (LineRead::Line, "last".into())
            ]
        );
        let mut reader = BufReader::with_capacity(4, &b"0123456789\nok\n"[..]);
        assert_eq!(
            read_line(&mut reader, &mut line, 10).unwrap(),
            LineRead::TooLong
        );
        let mut reader = BufReader::with_capacity(64, &b"012345678\nok\n"[..]);
        assert_eq!(
            read_line(&mut reader, &mut line, 10).unwrap(),
            LineRead::Line
        );
        assert_eq!(line, b"012345678");
        let mut reader = BufReader::with_capacity(64, &b"0123456789\n"[..]);
        assert_eq!(
            read_line(&mut reader, &mut line, 10).unwrap(),
            LineRead::TooLong
        );
    }

    #[test]
    fn peer_commands_put_the_alias_first_and_drop_agent_identity() {
        let command = hgs_command(
            Path::new("hgs"),
            &ComputerId::from("studio"),
            &["watch", "--json"],
        );
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args, vec!["@studio", "watch", "--json"]);
        let removed: Vec<_> = command
            .get_envs()
            .filter(|(_, value)| value.is_none())
            .map(|(name, _)| name.to_string_lossy().into_owned())
            .collect();
        assert_eq!(removed, vec!["HGS_RUN_ID", "HGS_SESSION"]);
        let local = hgs_command(Path::new("hgs"), &ComputerId::local(), &["-h"]);
        assert_eq!(local.get_args().count(), 1);
    }
}
