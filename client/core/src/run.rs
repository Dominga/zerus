//! Runs one [`Invocation`] and reports how it ended as an [`Exit`].
use crate::actions::{Exit, Invocation};
use std::io::{ErrorKind, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
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
    match run_inner(
        hgs,
        inv,
        parent_env,
        &[],
        stdout_cap,
        None,
        Pipes::default(),
    ) {
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
    run_inner(
        hgs,
        inv,
        parent_env,
        &[],
        stdout_cap,
        Some(cancel),
        Pipes::default(),
    )
}

/// [`run_limited`] of another program (a connector) that also leaves out
/// the variables in `remove`, such as the terminal's, even when
/// `parent_env` sets them.
pub fn run_without(
    program: &Path,
    inv: &Invocation,
    parent_env: &[(&str, &str)],
    remove: &[&str],
    stdout_cap: usize,
) -> Result<Exit, TooLarge> {
    match run_inner(
        program,
        inv,
        parent_env,
        remove,
        stdout_cap,
        None,
        Pipes::default(),
    ) {
        Ok(exit) => Ok(exit),
        Err(Stopped::TooLarge(too_large)) => Err(too_large),
        Err(Stopped::Cancelled) => Ok(Exit::Abnormal {
            stderr: "cancelled".into(),
        }),
    }
}

type StreamInput = Box<dyn FnOnce(ChildStdin) -> bool + Send>;
type StreamOutput = Box<dyn FnOnce(ChildStdout) -> bool + Send>;
type Spawn = fn(&mut Command) -> std::io::Result<Child>;

#[derive(Default)]
struct Pipes {
    input: Option<StreamInput>,
    output: Option<StreamOutput>,
    spawn: Option<Spawn>,
    readers: Option<Arc<dyn Send + Sync>>,
    stop: Option<Arc<AtomicBool>>,
}

// Also cancels a decoder that is still blocked in an OS read when the runner
// returns. Its reader guard remains held until that read actually ends.
struct CancelOutput(Option<Arc<AtomicBool>>);
impl Drop for CancelOutput {
    fn drop(&mut self) {
        if let Some(stop) = &self.0 {
            stop.store(true, Ordering::Relaxed);
        }
    }
}

/// A streamed decoder never grants completion on a partial/failed child.
/// Process and pipe errors are static categories, without child output.
#[derive(Debug, PartialEq, Eq)]
pub enum DecodeError<E> {
    Decode(E),
    Cancelled,
    TimedOut,
    StartFailed,
    ChildFailed,
    TooLarge,
    Incomplete,
}

/// Decode stdout on the existing runner's reader thread instead of collecting
/// it. The callback must bound input, memory and sink work, require real EOF,
/// and poll its stop flag/deadline. Its output/pages remain provisional until
/// this function succeeds: both pipes must end and the owned child must exit 0.
/// Stderr is capped at 64 KiB. `inv.stdin` must be absent.
///
/// The host supplies its guarded spawn function and an admitted reader-group
/// guard shared by both pipes. Admission must bound global jobs and exclude a
/// second reader group for the same source while an old group remains alive.
/// A descendant retaining a pipe keeps the guard after cancellation/deadline;
/// only the owned child is killed. No callback or unrelated process is killed.
pub fn run_decoded<T: Send + 'static, E: Send + 'static>(
    hgs: &Path,
    inv: &Invocation,
    parent_env: &[(&str, &str)],
    cancel: &AtomicBool,
    spawn: Spawn,
    readers: Arc<dyn Send + Sync>,
    decode: impl FnOnce(ChildStdout, &AtomicBool, Instant) -> Result<T, E> + Send + 'static,
) -> Result<T, DecodeError<E>> {
    assert!(inv.stdin.is_none(), "a decoded read cannot have stdin");
    // Refuse before spawn if the caller has already cancelled.
    if cancel.load(Ordering::Relaxed) {
        return Err(DecodeError::Cancelled);
    }
    let stop = Arc::new(AtomicBool::new(false));
    let decoder_stop = stop.clone();
    let deadline = Instant::now() + inv.timeout;
    let (tx, rx) = mpsc::sync_channel(1);
    let pipes = Pipes {
        output: Some(Box::new(move |pipe| {
            let result = decode(pipe, &decoder_stop, deadline);
            let ok = result.is_ok();
            tx.send(result).is_ok() && ok
        })),
        spawn: Some(spawn),
        readers: Some(readers),
        stop: Some(stop),
        ..Pipes::default()
    };
    let exit = run_inner(hgs, inv, parent_env, &[], STDERR_CAP, Some(cancel), pipes);
    if cancel.load(Ordering::Relaxed) {
        return Err(DecodeError::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(DecodeError::TimedOut);
    }
    match exit {
        Err(Stopped::Cancelled) => return Err(DecodeError::Cancelled),
        Err(Stopped::TooLarge(_)) => return Err(DecodeError::TooLarge),
        Ok(Exit::TimedOut) => return Err(DecodeError::TimedOut),
        Ok(Exit::StartFailed(_)) => return Err(DecodeError::StartFailed),
        _ => {}
    }
    match rx.try_recv() {
        Ok(Err(error)) => Err(DecodeError::Decode(error)),
        Ok(Ok(value)) if matches!(exit, Ok(Exit::Code { code: 0, .. })) => Ok(value),
        Ok(Ok(_)) => Err(DecodeError::ChildFailed),
        Err(_) => Err(DecodeError::Incomplete),
    }
}

/// Streams bounded input on the existing runner's writer thread. Both output
/// pipes have a hard byte limit; exceeding either ends only this owned child.
/// The callback must bound its own work and returns false on encoding/write
/// failure. `inv.stdin` must be absent. A successful process exit alone does
/// not establish that the callback completed; the caller validates its result.
pub(crate) fn run_streamed(
    hgs: &Path,
    inv: &Invocation,
    parent_env: &[(&str, &str)],
    output_cap: usize,
    input: impl FnOnce(ChildStdin) -> bool + Send + 'static,
) -> Result<Exit, TooLarge> {
    assert!(
        inv.stdin.is_none(),
        "streamed input cannot have a second body"
    );
    match run_inner(
        hgs,
        inv,
        parent_env,
        &[],
        output_cap,
        None,
        Pipes {
            input: Some(Box::new(input)),
            ..Pipes::default()
        },
    ) {
        Ok(exit) => Ok(exit),
        Err(Stopped::TooLarge(error)) => Err(error),
        Err(Stopped::Cancelled) => unreachable!("no cancellation flag"),
    }
}

fn run_inner(
    hgs: &Path,
    inv: &Invocation,
    parent_env: &[(&str, &str)],
    remove: &[&str],
    stdout_cap: usize,
    cancel: Option<&AtomicBool>,
    pipes: Pipes,
) -> Result<Exit, Stopped> {
    let consumed = pipes.output.is_some();
    let strict_outputs = pipes.input.is_some() || consumed;
    let _cancel_output = CancelOutput(pipes.stop);
    let mut command = Command::new(hgs);
    command.args(&inv.args);
    for (key, value) in parent_env {
        command.env(key, value);
    }
    for key in remove {
        command.env_remove(key);
    }
    command
        .env_remove("HGS_SESSION")
        .env_remove("HGS_RUN_ID")
        .stdin(if consumed {
            Stdio::null()
        } else {
            Stdio::piped()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match pipes.spawn.unwrap_or(Command::spawn)(&mut command) {
        Ok(child) => child,
        Err(error) => return Ok(Exit::StartFailed(error.to_string())),
    };
    let deadline = Instant::now() + inv.timeout;

    let stdin = child.stdin.take();
    let input = inv.stdin.clone();
    let (write_tx, write_rx) = mpsc::channel();
    thread::spawn(move || {
        let ok = match (stdin, pipes.input, input) {
            (Some(pipe), Some(write), _) => write(pipe),
            (Some(mut pipe), None, Some(bytes)) => {
                pipe.write_all(&bytes).and_then(|_| pipe.flush()).is_ok()
            }
            _ => true,
        };
        let _ = write_tx.send(ok);
    });
    let (done_tx, done_rx) = mpsc::channel();
    let over = Arc::new(AtomicBool::new(false));
    let limit = (stdout_cap < usize::MAX).then(|| (stdout_cap, Arc::clone(&over)));
    let decoded = Arc::new(AtomicU8::new(0));
    let out = if let Some(consume) = pipes.output {
        let stdout = child.stdout.take();
        let done = done_tx.clone();
        let completed = decoded.clone();
        let guard = pipes.readers.clone();
        thread::spawn(move || {
            let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                stdout.is_some_and(consume)
            }))
            .unwrap_or(false);
            // Release the pipe group before waking a possible replacement.
            drop(guard);
            completed.store(if ok { 1 } else { 2 }, Ordering::Release);
            let _ = done.send(());
        });
        Arc::default()
    } else {
        reader(
            child.stdout.take(),
            if strict_outputs {
                stdout_cap
            } else {
                stdout_cap.saturating_add(1)
            },
            done_tx.clone(),
            limit,
            None,
        )
    };
    let err = reader(
        child.stderr.take(),
        if strict_outputs {
            stdout_cap
        } else {
            STDERR_CAP
        },
        done_tx,
        strict_outputs.then(|| (stdout_cap, Arc::clone(&over))),
        pipes.readers,
    );
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
        if consumed && decoded.load(Ordering::Acquire) == 2 {
            let exit = stop(
                child,
                Exit::Abnormal {
                    stderr: "stdout decoder failed".into(),
                },
            );
            drain(&done_rx);
            return Ok(exit);
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
            let exit = stop(child, Exit::TimedOut);
            if consumed {
                drain(&done_rx);
            }
            return Ok(exit);
        }
        thread::sleep(POLL);
    };
    if consumed {
        let until = deadline.min(Instant::now() + DRAIN);
        let mut ended = 0;
        while ended < 2 {
            if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
                return Err(Stopped::Cancelled);
            }
            if over.load(Ordering::Relaxed) {
                return Err(too_large(&err));
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(if Instant::now() >= deadline {
                    Exit::TimedOut
                } else {
                    Exit::Abnormal {
                        stderr: "stdout decoder or pipe incomplete".into(),
                    }
                });
            }
            if done_rx.recv_timeout(left.min(POLL)).is_ok() {
                ended += 1;
            }
        }
        if decoded.load(Ordering::Acquire) != 1 {
            return Ok(Exit::Abnormal {
                stderr: "stdout decoder failed".into(),
            });
        }
    } else {
        drain(&done_rx);
    }
    let stdout = take(&out);
    // A child can exit before the loop sees its output pass the cap.
    if stdout.len() > stdout_cap || over.load(Ordering::Relaxed) {
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
    guard: Option<Arc<dyn Send + Sync>>,
) -> Shared {
    let buffer: Shared = Arc::default();
    let shared = Arc::clone(&buffer);
    thread::spawn(move || {
        if let Some(mut pipe) = pipe {
            let mut chunk = [0u8; 8192];
            let mut received = 0usize;
            loop {
                match pipe.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => {
                        received = received.saturating_add(n);
                        let mut kept = shared.lock().unwrap_or_else(|e| e.into_inner());
                        let room = cap.saturating_sub(kept.len()).min(n);
                        kept.extend_from_slice(&chunk[..room]);
                        if let Some((bytes, over)) = &limit {
                            if received > *bytes {
                                over.store(true, Ordering::Relaxed);
                            }
                        }
                    }
                    Err(error) if error.kind() == ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
        }
        drop(guard);
        let _ = done.send(());
    });
    buffer
}

#[cfg(all(test, unix))]
mod stream_tests {
    use super::*;

    fn python(code: &str, timeout: Duration) -> Invocation {
        Invocation {
            args: vec!["-c".into(), code.into()],
            stdin: None,
            timeout,
            request_id: None,
        }
    }

    #[test]
    fn streamed_eof_and_both_exact_output_limits_are_preserved() {
        let code = "import sys,os; assert sys.stdin.buffer.read() == b'bounded input'; assert 'HGS_SESSION' not in os.environ; sys.stdout.write('x'*16384); sys.stderr.write('y'*16384)";
        let exit = run_streamed(
            Path::new("python3"),
            &python(code, Duration::from_secs(5)),
            &[("HGS_SESSION", "discard")],
            16384,
            |mut pipe| pipe.write_all(b"bounded input").is_ok(),
        )
        .unwrap();
        match exit {
            Exit::Code {
                code: 0,
                stdout,
                stderr,
            } => {
                assert_eq!(stdout.len(), 16384);
                assert_eq!(stderr.len(), 16384);
            }
            other => panic!("{other:?}"),
        }
        for noisy in ["stdout", "stderr"] {
            let began = Instant::now();
            let code = format!("import sys,time; sys.{noisy}.write('x'*16385); sys.{noisy}.flush(); time.sleep(10)");
            let error = run_streamed(
                Path::new("python3"),
                &python(&code, Duration::from_secs(5)),
                &[],
                16384,
                |_| true,
            )
            .unwrap_err();
            assert_eq!(error.cap, 16384);
            assert!(error.stderr.len() <= 16384);
            assert!(began.elapsed() < Duration::from_secs(3));
        }
    }

    #[test]
    fn deadline_ends_owned_child_and_releases_its_blocked_pipe_writer() {
        let (done, completed) = mpsc::channel();
        let began = Instant::now();
        let exit = run_streamed(
            Path::new("python3"),
            &python("import time; time.sleep(10)", Duration::from_millis(250)),
            &[],
            16384,
            move |mut pipe| {
                let chunk = [0; 8192];
                let result = (0..16384).try_for_each(|_| pipe.write_all(&chunk));
                let _ = done.send(result.is_err());
                result.is_ok()
            },
        )
        .unwrap();
        assert_eq!(exit, Exit::TimedOut);
        assert!(began.elapsed() < Duration::from_secs(3));
        assert!(completed.recv_timeout(Duration::from_secs(2)).unwrap());
    }
}
