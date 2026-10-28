//! Runs one [`Invocation`] and reports how it ended as an [`Exit`].
use crate::actions::{Exit, Invocation};
use std::io::{ErrorKind, Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// A noisy child keeps no more than this much of its standard error.
const STDERR_CAP: usize = 64 * 1024;
/// How long to wait for the readers once the child has exited.
const DRAIN: Duration = Duration::from_secs(2);
/// How long a child may keep running after a failed stdin write.
const WRITE_GRACE: Duration = Duration::from_secs(1);
const POLL: Duration = Duration::from_millis(20);

/// Runs `hgs` with `inv`, blocking for at most `inv.timeout` plus two seconds.
pub fn run(hgs: &Path, inv: &Invocation) -> Exit {
    run_with_env(hgs, inv, &[])
}

/// [`run`] with extra environment variables for the child. The agent's
/// attribution (`HGS_SESSION`, `HGS_RUN_ID`) is never passed on: the client
/// acts for the person even when it was started from an agent's shell.
///
/// A failed stdin write is a failure to deliver only while the child keeps
/// running; a child that exits without reading its input (a usage or unknown
/// session error) reports its real exit, as the Qt client does.
pub fn run_with_env(hgs: &Path, inv: &Invocation, parent_env: &[(&str, &str)]) -> Exit {
    match run_limited(hgs, inv, parent_env, usize::MAX) {
        Ok(exit) => exit,
        // No output passes usize::MAX bytes.
        Err(TooLarge { stderr, .. }) => Exit::Abnormal { stderr },
    }
}

/// Standard output passed the cap of a [`run_limited`] call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TooLarge {
    pub cap: usize,
    /// The standard error read by then.
    pub stderr: String,
}

/// [`run_with_env`] for reads: standard output of more than `stdout_cap`
/// bytes is an error. It keeps at most `stdout_cap + 1` bytes and ends the
/// child as soon as more arrive, so a peer cannot fill the client's memory
/// within the timeout. Output of exactly `stdout_cap` bytes is whole. With
/// `usize::MAX` nothing ends early, as for [`run_with_env`].
pub fn run_limited(
    hgs: &Path,
    inv: &Invocation,
    parent_env: &[(&str, &str)],
    stdout_cap: usize,
) -> Result<Exit, TooLarge> {
    match run_inner(hgs, inv, parent_env, stdout_cap, None) {
        Ok(exit) => Ok(exit),
        Err(Stopped::TooLarge(too_large)) => Err(too_large),
        // Nothing sets a flag when there is none.
        Err(Stopped::Cancelled) => Ok(Exit::Abnormal {
            stderr: "cancelled".into(),
        }),
    }
}

/// Why a [`run_cancellable`] call gave no exit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stopped {
    TooLarge(TooLarge),
    /// The cancel flag was set: the child was killed and waited for.
    Cancelled,
}

/// [`run_limited`] that ends the child, within one poll (20 ms), once
/// `cancel` is set, and reports [`Stopped::Cancelled`].
pub fn run_cancellable(
    hgs: &Path,
    inv: &Invocation,
    parent_env: &[(&str, &str)],
    stdout_cap: usize,
    cancel: &AtomicBool,
) -> Result<Exit, Stopped> {
    run_inner(hgs, inv, parent_env, stdout_cap, Some(cancel))
}

fn run_inner(
    hgs: &Path,
    inv: &Invocation,
    parent_env: &[(&str, &str)],
    stdout_cap: usize,
    cancel: Option<&AtomicBool>,
) -> Result<Exit, Stopped> {
    let mut command = Command::new(hgs);
    command.args(&inv.args);
    for (key, value) in parent_env {
        command.env(key, value);
    }
    command
        .env_remove("HGS_SESSION")
        .env_remove("HGS_RUN_ID")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => return Ok(Exit::StartFailed(error.to_string())),
    };
    let deadline = Instant::now() + inv.timeout;

    let stdin = child.stdin.take();
    let input = inv.stdin.clone();
    let (write_tx, write_rx) = mpsc::channel();
    thread::spawn(move || {
        let ok = match (stdin, input) {
            (Some(mut pipe), Some(bytes)) => {
                pipe.write_all(&bytes).and_then(|_| pipe.flush()).is_ok()
            }
            _ => true,
        };
        let _ = write_tx.send(ok);
    });
    let (done_tx, done_rx) = mpsc::channel();
    let over = Arc::new(AtomicBool::new(false));
    let limit = (stdout_cap < usize::MAX).then(|| (stdout_cap, Arc::clone(&over)));
    let out = reader(
        child.stdout.take(),
        stdout_cap.saturating_add(1),
        done_tx.clone(),
        limit,
    );
    let err = reader(child.stderr.take(), STDERR_CAP, done_tx, None);
    let too_large = |err: &Shared| {
        Stopped::TooLarge(TooLarge {
            cap: stdout_cap,
            stderr: String::from_utf8_lossy(&take(err)).into_owned(),
        })
    };

    let mut written = false;
    // Set when a write failed: the child gets a short grace to show it exited.
    let mut write_failed_until = None;
    let status = loop {
        if !written {
            match write_rx.try_recv() {
                Ok(true) | Err(TryRecvError::Disconnected) => written = true,
                Ok(false) => {
                    written = true;
                    write_failed_until = Some((Instant::now() + WRITE_GRACE).min(deadline));
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
            let _ = stop(child, Exit::TimedOut);
            drain(&done_rx);
            return Err(Stopped::Cancelled);
        }
        if over.load(Ordering::Relaxed) {
            // Ended for its output: the exit that gives is not reported.
            let _ = stop(child, Exit::TimedOut);
            drain(&done_rx);
            return Err(too_large(&err));
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                return Ok(stop(
                    child,
                    Exit::Abnormal {
                        stderr: error.to_string(),
                    },
                ))
            }
        }
        let now = Instant::now();
        if write_failed_until.is_some_and(|until| now >= until) {
            return Ok(stop(child, Exit::WriteFailed));
        }
        if now >= deadline {
            return Ok(stop(child, Exit::TimedOut));
        }
        thread::sleep(POLL);
    };
    drain(&done_rx);
    let stdout = take(&out);
    // A child can exit before the loop sees its output pass the cap.
    if stdout.len() > stdout_cap {
        return Err(too_large(&err));
    }
    let stderr = String::from_utf8_lossy(&take(&err)).into_owned();
    Ok(match status.code() {
        Some(code) => Exit::Code {
            code,
            stdout,
            stderr,
        },
        None => Exit::Abnormal { stderr },
    })
}

/// Waits for both pipe readers to finish. One budget for both pipes: a
/// grandchild that keeps them open must not hold the call, and what was read
/// by then is still returned.
fn drain(done: &Receiver<()>) {
    let until = Instant::now() + DRAIN;
    for _ in 0..2 {
        let left = until.saturating_duration_since(Instant::now());
        if done.recv_timeout(left).is_err() {
            break;
        }
    }
}

fn stop(mut child: Child, exit: Exit) -> Exit {
    let _ = child.kill();
    let _ = child.wait();
    exit
}

type Shared = Arc<Mutex<Vec<u8>>>;

fn take(buffer: &Shared) -> Vec<u8> {
    std::mem::take(&mut *buffer.lock().unwrap_or_else(|e| e.into_inner()))
}

/// Reads a pipe to its end on its own thread into a shared buffer, keeping at
/// most `cap` bytes and draining the rest so the child never blocks on a full
/// pipe. The buffer holds what was read so far even if the pipe never closes;
/// `done` gets a signal at the end. With `limit`, the flag is set once more
/// than its byte count has been read.
fn reader(
    pipe: Option<impl Read + Send + 'static>,
    cap: usize,
    done: Sender<()>,
    limit: Option<(usize, Arc<AtomicBool>)>,
) -> Shared {
    let buffer: Shared = Arc::default();
    let shared = Arc::clone(&buffer);
    thread::spawn(move || {
        if let Some(mut pipe) = pipe {
            let mut chunk = [0u8; 8192];
            loop {
                match pipe.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => {
                        let mut kept = shared.lock().unwrap_or_else(|e| e.into_inner());
                        let room = cap.saturating_sub(kept.len()).min(n);
                        kept.extend_from_slice(&chunk[..room]);
                        if let Some((bytes, over)) = &limit {
                            if kept.len() > *bytes {
                                over.store(true, Ordering::Relaxed);
                            }
                        }
                    }
                    Err(error) if error.kind() == ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
        }
        let _ = done.send(());
    });
    buffer
}
