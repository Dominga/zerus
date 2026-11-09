//! The local half of the Windows SSH pipe protocol. Only selected Windows
//! commands use it; POSIX peers keep the existing SSH executable/argv.
use crate::platform;
use std::{
    io::{self, IsTerminal, Read, Write},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::{Duration, Instant},
};

pub(crate) const PREFIX: &str = "__hgs_ssh_bytes_v1__ ";
const CHUNK: usize = 16 * 1024;
const PING: u32 = u32::MAX;

/// The marker is local metadata, never part of the remote shell command.
/// A production self-exec is required only for Windows-target pipes.
pub(crate) fn command(remote: &str) -> io::Result<Command> {
    if remote.starts_with(PREFIX) {
        let mut cmd = Command::new(crate::self_exec::current()?);
        cmd.arg("--ssh-bytes-client");
        Ok(cmd)
    } else {
        Ok(platform::ssh())
    }
}

fn frame(out: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    out.write_all(&(bytes.len() as u32).to_le_bytes())?;
    out.write_all(bytes)?;
    out.flush()
}

fn pump_input(mut out: Box<dyn Write + Send>, stop: Arc<AtomicBool>) -> io::Result<()> {
    let (tx, rx) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut input = io::stdin().lock();
        loop {
            let mut bytes = vec![0; CHUNK];
            let value = input.read(&mut bytes).map(|n| {
                bytes.truncate(n);
                bytes
            });
            let end = value.as_ref().map_or(true, Vec::is_empty);
            if tx.send(value).is_err() || end {
                break;
            }
        }
    });
    let mut ended = false;
    while !stop.load(Ordering::Relaxed) {
        // Maintain the connection lease independently of caller input. A lost
        // SSH connection may leave Windows' redirected stdin open indefinitely.
        out.write_all(&PING.to_le_bytes())?;
        out.flush()?;
        if ended {
            thread::sleep(Duration::from_secs(2));
        } else {
            match rx.recv_timeout(Duration::from_secs(2)) {
                Ok(value) => {
                    let bytes = value?;
                    frame(&mut out, &bytes)?;
                    ended = bytes.is_empty();
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(io::Error::other("SSH input reader ended without EOF"))
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn run(mut args: Vec<String>) -> io::Result<i32> {
    let Some(remote) = args
        .last_mut()
        .and_then(|arg| arg.strip_prefix(PREFIX).map(str::to_owned))
    else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "missing Windows SSH pipe command",
        ));
    };
    *args.last_mut().unwrap() = remote;
    let interactive =
        args.iter().any(|arg| arg == "-t" || arg == "-tt") && io::stdin().is_terminal();
    let mut command = platform::ssh();
    if interactive {
        command.args(args);
        return platform::wait_interactive(&mut command).map_err(|e| io::Error::other(e.message));
    }
    // Last two arguments are always alias and the one fixed command string.
    // Override configured RequestTTY for opaque byte streams.
    if args.len() < 2 {
        return Err(io::Error::other("missing SSH target"));
    }
    args.insert(args.len() - 2, "-T".into());
    command.args(args).stdin(Stdio::piped());
    let _guard = platform::ShutdownGuard::install()?;
    let mut child = platform::spawn_ssh(&mut command, true, true, true)?;
    platform::forward_shutdown(&child.child);
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    let input = child.stdin.take().unwrap();
    let input_stop = stop.clone();
    let sent = tx.clone();
    thread::spawn(move || {
        let _ = sent.send((0, pump_input(input, input_stop)));
    });
    let sent = tx.clone();
    let mut output = child.stdout.take().unwrap();
    thread::spawn(move || {
        let _ = sent.send((
            1,
            io::copy(&mut output, &mut io::stdout().lock()).map(|_| ()),
        ));
    });
    let mut error = child.stderr.take().unwrap();
    thread::spawn(move || {
        let _ = tx.send((
            2,
            io::copy(&mut error, &mut io::stderr().lock()).map(|_| ()),
        ));
    });
    let outcome = (|| {
        let mut completed = [false; 3];
        let mut failed = false;
        let mut input_error_at = None;
        let mut exit = None;
        loop {
            while let Ok((index, result)) = rx.try_recv() {
                completed[index] = true;
                if result.is_err() {
                    if index == 0 {
                        input_error_at.get_or_insert_with(Instant::now);
                    } else {
                        failed = true;
                    }
                }
            }
            if exit.is_none() {
                if let Some(status) = child.child.try_wait()? {
                    // No signal handler may retain a reaped Unix PID while
                    // a healthy downstream reader keeps output backpressured.
                    platform::shutdown_child_exited();
                    exit = Some(status);
                }
            }
            if failed
                || platform::shutdown_signal() != 0
                || (exit.is_none()
                    && input_error_at.is_some_and(|at| at.elapsed() >= Duration::from_millis(100)))
            {
                failed = true;
                if exit.is_none() {
                    // Only the same original SSH Child; never a retained
                    // output descriptor's holder or a reaped/reused PID.
                    let _ = child.child.kill();
                    exit = Some(child.child.wait()?);
                    platform::shutdown_child_exited();
                }
                break;
            }
            if exit.is_some() {
                // A live downstream reader may apply backpressure indefinitely.
                // Its actual close is a copy error, not a drain deadline.
                stop.store(true, Ordering::Relaxed);
                if completed[1] && completed[2] {
                    break;
                }
            }
            thread::sleep(Duration::from_millis(10));
        }
        Ok(if failed {
            255
        } else {
            platform::code(exit.unwrap())
        })
    })();
    stop.store(true, Ordering::Relaxed);
    if outcome.is_err() {
        let _ = child.child.kill();
        let _ = child.child.wait();
    }
    platform::shutdown_child_exited();
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_preserves_opaque_bytes_and_distinguishes_eof_from_lease() {
        let mut bytes = Vec::new();
        frame(&mut bytes, &[0, 255, 10]).unwrap();
        frame(&mut bytes, &[]).unwrap();
        assert_eq!(bytes, [3, 0, 0, 0, 0, 255, 10, 0, 0, 0, 0]);
        assert_ne!(0u32, PING);
        assert!(command(&(PREFIX.to_owned() + "fixed")).is_err()); // libtest is never a helper
        assert_eq!(
            command("POSIX unchanged").unwrap().get_program(),
            platform::ssh_program()
        );
    }
}
