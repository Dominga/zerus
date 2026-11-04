//! The one original `hgs task message` child and the custody it holds.
//!
//! The child is this computer's own `hgs` with the plan's exact arguments, in
//! its own process group. Its stdin is exactly the record's frozen input; its
//! output is drained, stdout kept within a bound for the exact reply and
//! stderr discarded. It inherits the effect's close-only custody lock and,
//! before anything else, marks it close-on-exec ([`adopt_custody`]): the lock
//! lives exactly as long as the child, and nothing it starts inherits it. Only
//! that child is ever signalled, and only at the deadline.
//!
//! The caller's resources guard (the same custody lock and the original's
//! consent, with its lease and serve-owner loan) outlives every original pipe:
//! an output pipe still open when this returns keeps a guard copy in its own
//! reader until its real end, whatever result was recorded. Nothing is
//! signalled for it. An end that can never be observed (a failed read or an
//! unverified wait) pins a guard copy until this process exits.
// macOS admission refuses every original until a qualified child seam attests
// the code its child runs, so only tests start this runner there.
#![cfg_attr(target_os = "macos", allow(dead_code))]
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
    // The same wait fault, once the child's first output was read.
    pub(super) static WAIT_ERROR_AFTER_OUTPUT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    pub(super) static SPAWNED: std::cell::Cell<Option<u32>> = const { std::cell::Cell::new(None) };
    /// Runs once on this thread after the final check, right before spawn.
    pub(super) static AFTER_FINAL_CHECK: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}

/// An original output pipe.
trait Pipe: Read + AsRawFd + Send {}
impl<T: Read + AsRawFd + Send> Pipe for T {}

/// Keeps `guard` as long as this process lives: the end it waits for can
/// never be observed.
fn pin(guard: &Arc<dyn Send + Sync>) {
    std::mem::forget(guard.clone());
}

/// Keeps an original output pipe, and with it a copy of `guard`, until the
/// pipe's real end. What it reads is discarded and nothing is signalled.
fn retain(mut pipe: Box<dyn Pipe>, guard: &Arc<dyn Send + Sync>) {
    let fd = pipe.as_raw_fd();
    // SAFETY: fcntl on a descriptor this reader now owns; reads block from here.
    unsafe {
        libc::fcntl(
            fd,
            libc::F_SETFL,
            libc::fcntl(fd, libc::F_GETFL) & !libc::O_NONBLOCK,
        );
    }
    let held = guard.clone();
    let reader = std::thread::Builder::new()
        .name("task-message-output".into())
        .spawn(move || {
            let mut chunk = [0u8; 8192];
            loop {
                match pipe.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10))
                    }
                    Err(_) => {
                        pin(&held);
                        return;
                    }
                }
            }
            // The real end: only now does this copy of the guard go.
            drop(held);
        });
    if reader.is_err() {
        pin(guard);
    }
}

/// Runs the original child to its end, or to the deadline. `resources` is the
/// guard that every still-open original pipe keeps until its real end.
/// `final_check` runs inside the spawn turn, last before the spawn: its
/// refusal means that no child ever existed.
#[cfg(test)]
pub(super) fn run(
    program: &Path,
    args: &[String],
    input: &[u8],
    custody: &Arc<InheritedLock>,
    resources: &Arc<dyn Send + Sync>,
    deadline: Instant,
    final_check: impl FnOnce(&mut Command) -> std::io::Result<()>,
) -> Ended {
    run_stdin(
        program,
        args,
        Stdin::Bytes(input),
        custody,
        resources,
        deadline,
        final_check,
    )
}

/// The original child's stdin: the record's frozen bytes, or the reference
/// form's exact custody input, read only through its kept handle.
pub(super) enum Stdin<'a> {
    Bytes(&'a [u8]),
    File(&'a std::fs::File, u64),
}
impl Stdin<'_> {
    fn len(&self) -> usize {
        match self {
            Self::Bytes(bytes) => bytes.len(),
            Self::File(_, bytes) => *bytes as usize,
        }
    }
    /// The bytes from `at` on that the next write offers; a file is read at
    /// that offset, never from a shared position.
    fn from<'b>(&'b self, at: usize, buffer: &'b mut [u8]) -> std::io::Result<&'b [u8]> {
        match self {
            Self::Bytes(bytes) => Ok(&bytes[at..]),
            Self::File(file, bytes) => {
                use std::os::unix::fs::FileExt;
                let left = (*bytes as usize).saturating_sub(at).min(buffer.len());
                let read = file.read_at(&mut buffer[..left], at as u64)?;
                if read == 0 && left > 0 {
                    return Err(std::io::ErrorKind::UnexpectedEof.into());
                }
                Ok(&buffer[..read])
            }
        }
    }
    /// All of it, for a test executor.
    #[cfg(test)]
    pub(super) fn to_vec(&self) -> std::io::Result<Vec<u8>> {
        let mut buffer = vec![0; self.len()];
        let mut at = 0;
        while at < self.len() {
            let mut chunk = vec![0; 64 << 10];
            let next = self.from(at, &mut chunk)?.to_vec();
            buffer[at..at + next.len()].copy_from_slice(&next);
            at += next.len();
        }
        Ok(buffer)
    }
}

/// [`run`] with either kind of stdin.
pub(super) fn run_stdin(
    program: &Path,
    args: &[String],
    input: Stdin<'_>,
    custody: &Arc<InheritedLock>,
    resources: &Arc<dyn Send + Sync>,
    deadline: Instant,
    final_check: impl FnOnce(&mut Command) -> std::io::Result<()>,
) -> Ended {
    let mut chunk = vec![0u8; 64 << 10];
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
        custody
            .inherit(&mut command)
            .and_then(|()| final_check(&mut command))
            .and_then(|()| {
                #[cfg(test)]
                if REFUSE_PRE_EXEC.with(|refuse| refuse.replace(false)) {
                    // SAFETY: returns an error only; nothing else runs after fork.
                    unsafe {
                        command.pre_exec(|| Err(std::io::Error::from_raw_os_error(libc::EPERM)));
                    }
                }
                #[cfg(test)]
                if let Some(hook) = AFTER_FINAL_CHECK.with(|hook| hook.borrow_mut().take()) {
                    hook();
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
    let mut pipes: [Option<Box<dyn Pipe>>; 2] = [
        stdout.map(|pipe| Box::new(pipe) as Box<dyn Pipe>),
        stderr_pipe.map(|pipe| Box::new(pipe) as Box<dyn Pipe>),
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
                let Ok(next) = input.from(written, &mut chunk) else {
                    // An input that cannot be read is never complete.
                    stdin = None;
                    break;
                };
                match pipe.write(next) {
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
                        // Its real end can never be observed now.
                        pin(resources);
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
                if fault.replace(false)
                    || WAIT_ERROR_AFTER_OUTPUT
                        .with(|armed| !reply.is_empty() && armed.replace(false))
                {
                    Err(std::io::Error::other("private task message wait fault"))
                } else {
                    observed
                }
            });
            match observed {
                Ok(found) => status = found,
                Err(_) => {
                    let _ = child.kill();
                    if child.wait().is_err() {
                        pin(resources);
                    }
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
                if child.wait().is_err() {
                    pin(resources);
                }
                break Ended::Unknown("task_message_timed_out");
            }
            break Ended::Unknown("task_message_output_retained");
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    drop(stdin);
    // A descendant may still hold an original output pipe: it keeps the guard
    // until that pipe's real end, separately from the result returned here.
    for pipe in pipes.into_iter().flatten() {
        retain(pipe, resources);
    }
    match ended {
        Ended::Exited { .. } if written < input.len() => {
            Ended::Unknown("task_message_input_unwritten")
        }
        ended => ended,
    }
}
