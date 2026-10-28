//! Runs one [`Invocation`] and reports how it ended as an [`Exit`].
use crate::actions::{Exit, Invocation};
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

/// A noisy child keeps no more than this much of its standard error.
const STDERR_CAP: usize = 64 * 1024;
/// How long to wait for the readers once the child has exited.
const DRAIN: Duration = Duration::from_secs(2);
const POLL: Duration = Duration::from_millis(20);

/// Runs `hgs` with `inv`, blocking for at most `inv.timeout` plus two seconds.
pub fn run(hgs: &Path, inv: &Invocation) -> Exit {
    run_with_env(hgs, inv, &[])
}

/// [`run`] with extra environment variables for the child. The agent's
/// attribution (`HGS_SESSION`, `HGS_RUN_ID`) is never passed on: the client
/// acts for the person even when it was started from an agent's shell.
pub fn run_with_env(hgs: &Path, inv: &Invocation, parent_env: &[(&str, &str)]) -> Exit {
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
        Err(error) => return Exit::StartFailed(error.to_string()),
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
    let out_rx = reader(child.stdout.take(), usize::MAX);
    let err_rx = reader(child.stderr.take(), STDERR_CAP);

    let mut written = false;
    let status = loop {
        if !written {
            match write_rx.try_recv() {
                Ok(true) => written = true,
                Ok(false) => return stop(child, Exit::WriteFailed),
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => written = true,
            }
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(POLL),
            Ok(None) => return stop(child, Exit::TimedOut),
            Err(error) => return stop(child, Exit::StartFailed(error.to_string())),
        }
    };
    // The child may have exited before the writer reported; a failed write
    // is still a failure to deliver the request.
    if !written && write_rx.recv_timeout(DRAIN) == Ok(false) {
        return Exit::WriteFailed;
    }
    let stdout = out_rx.recv_timeout(DRAIN).unwrap_or_default();
    let stderr =
        String::from_utf8_lossy(&err_rx.recv_timeout(DRAIN).unwrap_or_default()).into_owned();
    match status.code() {
        Some(code) => Exit::Code {
            code,
            stdout,
            stderr,
        },
        None => Exit::Abnormal { stderr },
    }
}

fn stop(mut child: Child, exit: Exit) -> Exit {
    let _ = child.kill();
    let _ = child.wait();
    exit
}

/// Reads a pipe to its end on its own thread, keeping at most `cap` bytes and
/// draining the rest so the child never blocks on a full pipe.
fn reader(pipe: Option<impl Read + Send + 'static>, cap: usize) -> Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut kept = Vec::new();
        if let Some(mut pipe) = pipe {
            let mut chunk = [0u8; 8192];
            while let Ok(n) = pipe.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                let room = cap.saturating_sub(kept.len()).min(n);
                kept.extend_from_slice(&chunk[..room]);
            }
        }
        let _ = tx.send(kept);
    });
    rx
}
