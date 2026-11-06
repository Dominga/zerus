//! One freshly created group. Its leader stays unreaped until group cleanup;
//! caller deadlines never drop that original child or the actual I/O charge.
#[path = "../../../core/src/helper_group.rs"]
mod group;
use std::{
    collections::VecDeque,
    io::{self, Read},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
const POLL: Duration = Duration::from_millis(10);
const DRAIN: Duration = Duration::from_secs(2);
const OUT: usize = 1 << 20;
const ERR: usize = 64 << 10;

pub(super) struct Watchdog {
    until: Mutex<Instant>,
    cancelled: AtomicBool,
}
impl Watchdog {
    pub(super) fn new(until: Instant) -> Arc<Self> {
        Arc::new(Self {
            until: Mutex::new(until),
            cancelled: AtomicBool::new(false),
        })
    }
    fn reason(&self) -> Option<Stopped> {
        if self.cancelled.load(Ordering::Acquire) {
            Some(Stopped::Cancelled)
        } else if Instant::now() >= self.until() {
            Some(Stopped::TimedOut)
        } else {
            None
        }
    }
    fn until(&self) -> Instant {
        *self.until.lock().unwrap()
    }
    #[cfg(target_os = "linux")]
    pub(super) fn expire(&self) {
        *self.until.lock().unwrap() = Instant::now();
    }
    #[cfg(target_os = "linux")]
    pub(super) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Stopped {
    TimedOut,
    Cancelled,
    OutputLimit,
    Io,
    Incomplete,
}
impl Stopped {
    fn name(self) -> &'static str {
        match self {
            Self::TimedOut => "timed out",
            Self::Cancelled => "cancelled",
            Self::OutputLimit => "stdout limit exceeded",
            Self::Io => "helper I/O unavailable",
            Self::Incomplete => "helper pipes did not drain",
        }
    }
}
#[derive(Default)]
struct Output {
    out: VecDeque<u8>,
    err: VecDeque<u8>,
    out_bytes: usize,
}
impl Output {
    fn append(&mut self, err: bool, bytes: &[u8]) {
        if !err {
            self.out_bytes = self.out_bytes.saturating_add(bytes.len());
        }
        let (buffer, cap) = if err {
            (&mut self.err, ERR)
        } else {
            (&mut self.out, OUT)
        };
        buffer.extend(bytes);
        if buffer.len() > cap {
            buffer.drain(..buffer.len() - cap);
        }
    }
    fn text(&self, err: bool) -> String {
        let buffer = if err { &self.err } else { &self.out };
        String::from_utf8_lossy(&buffer.iter().copied().collect::<Vec<_>>()).into_owned()
    }
}
struct Capture {
    output: Mutex<Output>,
    pid: AtomicU32,
}
#[derive(Debug)]
pub(super) struct Report {
    pub(super) status: Option<ExitStatus>,
    pub(super) stopped: Option<Stopped>,
    pub(super) complete: bool,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(super) pid: u32,
    pub(super) stdout: String,
    pub(super) stderr: String,
}
impl Report {
    pub(super) fn success(&self) -> bool {
        self.stopped.is_none()
            && self.complete
            && self.status.is_some_and(|s| s.success())
            && self.stdout.contains("1 passed; 0 failed")
    }
    pub(super) fn diagnostic(&self) -> String {
        let reason = self
            .stopped
            .map(Stopped::name)
            .map(str::to_owned)
            .unwrap_or_else(|| {
                self.status
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "exit unobserved".into())
            });
        let tail = |s: &str, count: usize| {
            s.chars()
                .rev()
                .take(count)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<String>()
        };
        // Keep the direct child's assertion header AND the latest output: a
        // nested fixture diagnostic may itself contain a second panic header.
        let separator = "\n[stderr tail]\n";
        let stderr = if let Some(at) = self.stderr.find("panicked at") {
            format!(
                "{}{separator}{}",
                self.stderr[at..].chars().take(1024).collect::<String>(),
                tail(&self.stderr, 4096 - 1024 - separator.len())
            )
        } else {
            tail(&self.stderr, 4096)
        };
        format!(
            "{reason}; I/O {}\nstdout:\n{}\nstderr:\n{stderr}",
            if self.complete { "settled" } else { "pending" },
            tail(&self.stdout, 4096)
        )
    }
}
struct Original {
    child: Option<Child>,
    scope: Option<Arc<dyn Send + Sync>>,
}
impl Original {
    fn retain(&mut self) {
        std::mem::forget(self.child.take());
        std::mem::forget(self.scope.take());
    }
    fn poll(&mut self) -> io::Result<Option<ExitStatus>> {
        match group::try_wait(self.child.as_mut().unwrap()) {
            Ok(Some(status)) => {
                self.child.take();
                Ok(Some(status))
            }
            Ok(None) => Ok(None),
            Err(error) => {
                self.retain();
                Err(error)
            }
        }
    }
    fn stop(&mut self) -> io::Result<ExitStatus> {
        match group::terminate(self.child.as_mut().unwrap()) {
            Ok(status) => {
                self.child.take();
                Ok(status)
            }
            Err(error) => {
                self.retain();
                Err(error)
            }
        }
    }
}
impl Drop for Original {
    fn drop(&mut self) {
        if self.child.is_some() {
            let _ = self.stop();
        }
    }
}
fn reader(
    pipe: impl Read + Send + 'static,
    err: bool,
    capture: Arc<Capture>,
    done: mpsc::Sender<bool>,
    scope: Arc<dyn Send + Sync>,
) -> io::Result<()> {
    thread::Builder::new()
        .name("fixture-output".into())
        .spawn(move || {
            let mut pipe = pipe;
            let mut buffer = [0; 8192];
            let ok = loop {
                match pipe.read(&mut buffer) {
                    Ok(0) => break true,
                    Ok(size) => capture.output.lock().unwrap().append(err, &buffer[..size]),
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => (),
                    Err(_) => break false,
                }
            };
            drop(pipe);
            drop(scope);
            let _ = done.send(ok);
        })
        .map(|_| ())
}
fn report(
    capture: &Capture,
    status: Option<ExitStatus>,
    stopped: Option<Stopped>,
    complete: bool,
) -> Report {
    let output = capture.output.lock().unwrap();
    Report {
        status,
        stopped,
        complete,
        pid: capture.pid.load(Ordering::Acquire),
        stdout: output.text(false),
        stderr: output.text(true),
    }
}
fn supervise(
    mut command: Command,
    watchdog: &Watchdog,
    capture: &Arc<Capture>,
    scope: Arc<dyn Send + Sync>,
) -> Report {
    group::prepare(&mut command);
    let Ok(child) = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    else {
        return report(capture, None, Some(Stopped::Io), true);
    };
    capture.pid.store(child.id(), Ordering::Release);
    let mut original = Original {
        child: Some(child),
        scope: Some(scope.clone()),
    };
    let (done, receive) = mpsc::channel();
    let child = original.child.as_mut().unwrap();
    let readers = match (child.stdout.take(), child.stderr.take()) {
        (Some(out), Some(err)) => reader(out, false, capture.clone(), done.clone(), scope.clone())
            .and_then(|_| reader(err, true, capture.clone(), done, scope.clone())),
        _ => Err(io::ErrorKind::BrokenPipe.into()),
    };
    let (status, mut stopped) = if readers.is_err() {
        (original.stop().ok(), Some(Stopped::Io))
    } else {
        loop {
            let reason = watchdog.reason().or_else(|| {
                (capture.output.lock().unwrap().out_bytes > OUT).then_some(Stopped::OutputLimit)
            });
            if let Some(reason) = reason {
                break (original.stop().ok(), Some(reason));
            }
            match original.poll() {
                Ok(Some(status)) => break (Some(status), None),
                Ok(None) => thread::sleep(POLL),
                Err(_) => break (None, Some(Stopped::Io)),
            }
        }
    };
    let end = Instant::now() + DRAIN;
    let mut complete = readers.is_ok() && status.is_some();
    if readers.is_ok() {
        for _ in 0..2 {
            let left = end.saturating_duration_since(Instant::now());
            if !matches!(receive.recv_timeout(left), Ok(true)) {
                complete = false;
                break;
            }
        }
    }
    stopped = stopped.or_else(|| watchdog.reason());
    if stopped.is_none() && capture.output.lock().unwrap().out_bytes > OUT {
        stopped = Some(Stopped::OutputLimit);
    }
    if stopped.is_none() && !complete {
        stopped = Some(Stopped::Incomplete);
    }
    report(capture, status, stopped, complete)
}
pub(super) fn run(
    command: Command,
    watchdog: Arc<Watchdog>,
    scope: Arc<dyn Send + Sync>,
) -> Report {
    let capture = Arc::new(Capture {
        output: Mutex::new(Output::default()),
        pid: AtomicU32::new(0),
    });
    let (send, receive) = mpsc::sync_channel(1);
    let (observed, clock) = (capture.clone(), watchdog.clone());
    let worker = thread::Builder::new()
        .name("fixture-supervisor".into())
        .spawn(move || {
            let result = supervise(command, &clock, &observed, scope);
            let _ = send.send(result);
        });
    if worker.is_err() {
        return report(&capture, None, Some(Stopped::Io), true);
    }
    let mut stop_at = None;
    loop {
        match receive.try_recv() {
            Ok(result) => return result,
            Err(mpsc::TryRecvError::Disconnected) => {
                return report(&capture, None, Some(Stopped::Io), false)
            }
            Err(mpsc::TryRecvError::Empty) => (),
        }
        if watchdog.reason().is_some() {
            stop_at.get_or_insert_with(Instant::now);
        }
        let end = stop_at
            .map(|at| at + DRAIN + POLL * 2)
            .unwrap_or(watchdog.until() + DRAIN + POLL * 2);
        if Instant::now() >= end {
            return report(
                &capture,
                None,
                watchdog.reason().or(Some(Stopped::Incomplete)),
                false,
            );
        }
        thread::sleep(POLL);
    }
}

#[cfg(target_os = "linux")]
mod tests;
