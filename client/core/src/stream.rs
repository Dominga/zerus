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
//! new stream, after their last cursor.
use crate::{state::CatalogState, Change, ComputerId, ComputerStatus};
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use zerus_watch_protocol::{
    decode, topic, Body, Control, Frame, FrameError, SessionRef, Subscription,
    ACTIVITY_SUBSCRIPTIONS, ACTIVITY_TAIL, HELP_MARKER, MAX_FRAME_BYTES, VERSION,
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
        }
    }
}

impl Timing {
    /// The default timing with restart and re-probe delays times `factor`.
    pub fn scaled(factor: f64) -> Self {
        let default = Self::default();
        Self {
            restart: default.restart.map(|delay| delay.mul_f64(factor)),
            reprobe: default.reprobe.mul_f64(factor),
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
    /// The computer already follows [`ACTIVITY_SUBSCRIPTIONS`] sessions.
    TooMany,
    /// The stream was stopped.
    Stopped,
}

impl std::fmt::Display for FollowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::UnknownComputer => "unknown computer",
            Self::KeyMismatch => "the key is not the session's catalog key",
            Self::TooMany => "a computer follows at most 16 sessions",
            Self::Stopped => "the stream was stopped",
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
    Line { generation: u64, line: String },
    Closed { generation: u64 },
    Subscribe { key: String },
    Unsubscribe { key: String, session: SessionRef },
    Control(Control),
    Stop,
}

#[derive(Clone, Debug)]
struct Follow {
    session: SessionRef,
    after: Option<i64>,
    tail: u32,
}

type Follows = Arc<Mutex<BTreeMap<String, Follow>>>;

fn lock(follows: &Follows) -> MutexGuard<'_, BTreeMap<String, Follow>> {
    follows
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Controls one computer's supervisor; dropping it stops the stream.
pub struct StreamHandle {
    computer: ComputerId,
    tx: SyncSender<Input>,
    follows: Follows,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl StreamHandle {
    pub fn computer(&self) -> &ComputerId {
        &self.computer
    }

    /// Writes a control line to the running stream (a `ping`, say); false
    /// once the supervisor stopped. Activity goes through [`Self::follow`].
    pub fn send(&self, control: Control) -> bool {
        !self.stopped() && self.tx.send(Input::Control(control)).is_ok()
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
    let stop = Arc::new(AtomicBool::new(false));
    let supervisor = Supervisor {
        hgs: hgs.to_owned(),
        computer: computer.clone(),
        timing,
        sink,
        rx,
        tx: tx.clone(),
        follows: follows.clone(),
        stop: stop.clone(),
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
        stop,
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
    stop: Arc<AtomicBool>,
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
            self.state.begin();
            let timeout = self.timing.probe_timeout(&self.computer);
            match probe_until(&self.hgs, &self.computer, timeout, &self.stop) {
                Probe::Supported => {}
                Probe::Unsupported => {
                    self.status(ComputerStatus::Unsupported);
                    if self.wait(self.timing.reprobe) {
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
        self.emit(Change::Replace {
            computer: self.computer.clone(),
            rows: Vec::new(),
        });
        self.status(ComputerStatus::Ended);
    }

    fn emit(&self, change: Change) {
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
        let deadline = Instant::now() + time;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return true;
            }
            match self.rx.recv_timeout(left) {
                Ok(Input::Stop) | Err(RecvTimeoutError::Disconnected) => return false,
                Ok(Input::Subscribe { key }) => self.state.follow(&key),
                Ok(Input::Unsubscribe { key, .. }) => self.state.unfollow(&key),
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => return true,
            }
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
                Ok(Input::Control(control)) => self.write(&control),
                Ok(Input::Stop) | Err(RecvTimeoutError::Disconnected) => return Outcome::Stopped,
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
    }

    fn handle(&mut self, frame: &Frame) {
        let changes = self.state.apply(&self.computer, frame);
        for change in changes {
            if let Change::Activity {
                key,
                update: crate::ActivityUpdate::Ended,
                ..
            } = &change
            {
                lock(&self.follows).remove(key);
            }
            self.emit(change);
        }
        match &frame.body {
            Body::Hello(_) => {
                self.greeted = true;
                let keys: Vec<String> = lock(&self.follows).keys().cloned().collect();
                for key in keys {
                    self.subscribe(&key);
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
