//! The one original `hgs task message` child and the custody it holds.
//!
//! The child is this computer's own `hgs` with the plan's exact arguments, in
//! its own process group. Its stdin is exactly the record's frozen input; its
//! output is drained, stdout kept within a bound for the exact reply and
//! stderr discarded. It inherits the effect's close-only custody lock and,
//! before anything else, marks it close-on-exec ([`adopt_custody`]): the lock
//! lives exactly as long as the child, and nothing it starts inherits it. Only
//! that child is ever signalled, and only at the deadline.
use crate::platform;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};
use zerus_client_core::run::custody::InheritedLock;

/// Names the inherited custody descriptor of a task message child.
pub(super) const CUSTODY_FD: &str = "HGS_TASK_MESSAGE_CUSTODY_FD";
/// The CLI reply retained to read its exact outcome; a larger one is Unknown.
pub(super) const STDOUT_BYTES: usize = 1 << 20;

/// Called first in `main`, after the raw-input capture. A task message child
/// started by the desktop actor holds the actor's close-only custody lock for
/// exactly its own lifetime: the inherited descriptor is marked close-on-exec
/// before anything else runs, and the variable naming it is removed. A
/// malformed or unusable value refuses the call before it starts anything.
pub(crate) fn adopt_custody() -> Result<(), String> {
    let Some(value) = std::env::var_os(CUSTODY_FD) else {
        return Ok(());
    };
    // Single-threaded: main has started nothing yet.
    std::env::remove_var(CUSTODY_FD);
    let fd = value
        .to_str()
        .filter(|text| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|text| text.parse::<i32>().ok())
        .filter(|fd| *fd > 2)
        .ok_or("Invalid task message custody")?;
    // SAFETY: fcntl on a descriptor number; failure is reported, never ignored.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFD);
        if flags < 0 || libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) < 0 {
            return Err("Task message custody is unavailable".into());
        }
    }
    Ok(())
}

/// How the original task message child ended.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Ended {
    /// It read all of its stdin and exited with `code`, and both output pipes
    /// ended, within the deadline; `stdout` is its whole reply.
    Exited { code: i32, stdout: Vec<u8> },
    /// It never started: refused before any child existed (the own image, its
    /// custody, or the spawn itself, which reports a fork or exec failure
    /// before any program code runs). Nothing ran.
    NotStarted,
    /// Its result is unknown, for the named reason: a signal ended it, the
    /// deadline passed (the child alone was then killed), its input or output
    /// was incomplete, or another process kept its output open.
    Unknown(&'static str),
}

#[cfg(test)]
thread_local! {
    /// Makes the next spawn on this thread fail inside `pre_exec`, as a custody
    /// refusal there would.
    pub(super) static REFUSE_PRE_EXEC: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    // One-shot observation faults on this thread, as in the launch runner:
    // anonymous pipes give no deterministic EIO, and stealing waitpid would
    // interfere with other test children.
    pub(super) static READ_ERROR: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    pub(super) static WAIT_ERROR: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    pub(super) static SPAWNED: std::cell::Cell<Option<u32>> = const { std::cell::Cell::new(None) };
}

/// Runs the original child to its end, or to the deadline.
pub(super) fn run(
    program: &Path,
    args: &[String],
    input: &[u8],
    custody: &Arc<InheritedLock>,
    deadline: Instant,
) -> Ended {
    let mut command = Command::new(program);
    command
        .args(args)
        .process_group(0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env(CUSTODY_FD, custody.as_raw_fd().to_string());
    let spawned = {
        let _turn = platform::spawn_turn();
        custody.inherit(&mut command).and_then(|()| {
            #[cfg(test)]
            if REFUSE_PRE_EXEC.with(|refuse| refuse.replace(false)) {
                // SAFETY: returns an error only; nothing else runs after fork.
                unsafe {
                    command.pre_exec(|| Err(std::io::Error::from_raw_os_error(libc::EPERM)));
                }
            }
            command.spawn()
        })
    };
    // Only these pre-child branches mean "not started"; once a child exists,
    // every outcome is its exit or Unknown.
    let Ok(mut child) = spawned else {
        return Ended::NotStarted;
    };
    #[cfg(test)]
    SPAWNED.with(|spawned| spawned.set(Some(child.id())));
    // The frozen stdin, written without blocking inside the same drain loop,
    // so neither side can block the other and no writer outlives this call.
    let mut stdin = child.stdin.take();
    let mut written = 0;
    let stdout = child.stdout.take();
    let stderr_pipe = child.stderr.take();
    for fd in [
        stdin.as_ref().map(AsRawFd::as_raw_fd),
        stdout.as_ref().map(AsRawFd::as_raw_fd),
        stderr_pipe.as_ref().map(AsRawFd::as_raw_fd),
    ]
    .into_iter()
    .flatten()
    {
        // The input is written and both outputs drained while the child runs.
        // SAFETY: fcntl on descriptors this function owns.
        unsafe {
            libc::fcntl(
                fd,
                libc::F_SETFL,
                libc::fcntl(fd, libc::F_GETFL) | libc::O_NONBLOCK,
            );
        }
    }
    let mut pipes: [Option<Box<dyn Read>>; 2] = [
        stdout.map(|pipe| Box::new(pipe) as Box<dyn Read>),
        stderr_pipe.map(|pipe| Box::new(pipe) as Box<dyn Read>),
    ];
    let mut reply = Vec::new();
    let mut overflow = false;
    let mut status = None;
    // A pipe that cannot be read is never taken as its end.
    let mut unreadable = false;
    let ended = loop {
        if let Some(pipe) = stdin.as_mut() {
            loop {
                if written == input.len() {
                    // All of it: close the input so the child sees its end.
                    stdin = None;
                    break;
                }
                match pipe.write(&input[written..]) {
                    Ok(0) => {
                        stdin = None;
                        break;
                    }
                    Ok(n) => written += n,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => {
                        stdin = None;
                        break;
                    }
                }
            }
        }
        for (index, slot) in pipes.iter_mut().enumerate() {
            let Some(pipe) = slot else { continue };
            loop {
                let mut chunk = [0u8; 4096];
                let read = pipe.read(&mut chunk);
                #[cfg(test)]
                let read = READ_ERROR.with(|fault| {
                    if fault.replace(false) {
                        Err(std::io::Error::other("private task message read fault"))
                    } else {
                        read
                    }
                });
                match read {
                    Ok(0) => {
                        *slot = None;
                        break;
                    }
                    Ok(n) => {
                        if index == 0 {
                            if reply.len() + n > STDOUT_BYTES {
                                overflow = true;
                            } else {
                                reply.extend_from_slice(&chunk[..n]);
                            }
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => {
                        *slot = None;
                        unreadable = true;
                        break;
                    }
                }
            }
        }
        if status.is_none() {
            let observed = child.try_wait();
            #[cfg(test)]
            let observed = WAIT_ERROR.with(|fault| {
                if fault.replace(false) {
                    Err(std::io::Error::other("private task message wait fault"))
                } else {
                    observed
                }
            });
            match observed {
                Ok(found) => status = found,
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Ended::Unknown("task_message_unobserved");
                }
            }
        }
        if let Some(status) = status {
            if pipes.iter().all(Option::is_none) {
                if unreadable {
                    break Ended::Unknown("task_message_output_unreadable");
                }
                if overflow {
                    break Ended::Unknown("task_message_reply_too_large");
                }
                break match status.code() {
                    Some(code) => Ended::Exited {
                        code,
                        stdout: std::mem::take(&mut reply),
                    },
                    None => Ended::Unknown("task_message_signalled"),
                };
            }
        }
        if Instant::now() >= deadline {
            if status.is_none() {
                // The original child alone; never its group or anything else.
                let _ = child.kill();
                let _ = child.wait();
                break Ended::Unknown("task_message_timed_out");
            }
            break Ended::Unknown("task_message_output_retained");
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    drop(stdin);
    match ended {
        Ended::Exited { .. } if written < input.len() => {
            Ended::Unknown("task_message_input_unwritten")
        }
        ended => ended,
    }
}
