//! The one original launch child and the custody it holds.
//!
//! The child is this computer's own `hgs` with the plan's exact arguments, in
//! its own process group, with closed input and drained output. It inherits
//! the effect's close-only custody lock and, before anything else, marks it
//! close-on-exec ([`adopt_custody`]): the lock lives exactly as long as the
//! child, and nothing it starts (tmux, its server or the agent) inherits it.
//! Only that child is ever signalled, and only at the deadline; its process
//! group, tmux and agents never are.
use crate::platform;
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};
use zerus_client_core::run::custody::InheritedLock;

/// Names the inherited custody descriptor of a creation launch child.
pub(super) const CUSTODY_FD: &str = "HGS_CREATION_LAUNCH_CUSTODY_FD";
/// Standard error retained before the wire bounds its encoded text.
const STDERR_BYTES: usize = 64 << 10;

/// Called first in `main`. A launch child started by the desktop actor holds
/// the actor's close-only custody lock for exactly its own lifetime: the
/// inherited descriptor is marked close-on-exec before anything else runs,
/// and the variable naming it is removed. A malformed or unusable value
/// refuses the launch before it starts anything.
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
        .ok_or("Invalid creation launch custody")?;
    // SAFETY: fcntl on a descriptor number; failure is reported, never ignored.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFD);
        if flags < 0 || libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) < 0 {
            return Err("Creation launch custody is unavailable".into());
        }
    }
    Ok(())
}

/// How the original launch child ended.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Ended {
    /// It exited with `code`, and both output pipes ended, within the deadline.
    Exited { code: i32, stderr: String },
    /// Its result is unknown, for the named reason: it never started, a signal
    /// ended it, the deadline passed (the child alone was then killed), or
    /// another process kept its output open past the deadline.
    Unknown(&'static str),
}

/// Runs `program args` as the original launch child under `custody`.
pub(super) fn run(
    program: &Path,
    args: &[String],
    custody: &Arc<InheritedLock>,
    deadline: Instant,
) -> Ended {
    let mut command = Command::new(program);
    command
        .args(args)
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env(CUSTODY_FD, custody.as_raw_fd().to_string());
    let spawned = {
        let _turn = platform::spawn_turn();
        custody.inherit(&mut command).and_then(|()| command.spawn())
    };
    let Ok(mut child) = spawned else {
        return Ended::Unknown("creation_launch_not_started");
    };
    let stdout = child.stdout.take();
    let stderr_pipe = child.stderr.take();
    for fd in [
        stdout.as_ref().map(AsRawFd::as_raw_fd),
        stderr_pipe.as_ref().map(AsRawFd::as_raw_fd),
    ]
    .into_iter()
    .flatten()
    {
        // Both pipes are drained while the child runs.
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
    let mut stderr = Vec::new();
    let mut status = None;
    loop {
        for (index, slot) in pipes.iter_mut().enumerate() {
            let Some(pipe) = slot else { continue };
            loop {
                let mut chunk = [0u8; 4096];
                match pipe.read(&mut chunk) {
                    Ok(0) => {
                        *slot = None;
                        break;
                    }
                    Ok(n) => {
                        if index == 1 && stderr.len() < STDERR_BYTES {
                            let keep = n.min(STDERR_BYTES - stderr.len());
                            stderr.extend_from_slice(&chunk[..keep]);
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => {
                        *slot = None;
                        break;
                    }
                }
            }
        }
        if status.is_none() {
            match child.try_wait() {
                Ok(found) => status = found,
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Ended::Unknown("creation_launch_unobserved");
                }
            }
        }
        if let Some(status) = status {
            if pipes.iter().all(Option::is_none) {
                return match status.code() {
                    Some(code) => Ended::Exited {
                        code,
                        stderr: String::from_utf8_lossy(&stderr).into_owned(),
                    },
                    None => Ended::Unknown("creation_launch_signalled"),
                };
            }
        }
        if Instant::now() >= deadline {
            if status.is_none() {
                // The original child alone; never its group, tmux or agents.
                let _ = child.kill();
                let _ = child.wait();
                return Ended::Unknown("creation_launch_timed_out");
            }
            return Ended::Unknown("creation_launch_output_retained");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
