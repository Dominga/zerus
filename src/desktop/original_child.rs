//! The bounded runner of one original child of a desktop actor, shared by the
//! creation launch and task message leaves (zerus-5wmh.66.4.4.3). Each kind
//! keeps its own custody variable, input, retained output, reason codes and
//! result type in its own `child` module; the mechanics here are one.
//!
//! The child is this computer's own `hgs` with the plan's exact arguments, in
//! its own process group. It inherits the effect's close-only custody lock
//! and, before anything else, marks it close-on-exec ([`adopt`]): the lock
//! lives exactly as long as the child, and nothing it starts inherits it.
//! Its input is closed or exactly the kind's frozen bytes, written without
//! blocking; both outputs are drained without blocking, and one of them is
//! kept within the kind's bound. Only that child is ever signalled, and only
//! at the deadline or when its wait cannot be observed; its process group,
//! tmux and agents never are.
//!
//! A kind that names a resources guard keeps it beyond this call: an output
//! pipe still open on return keeps a guard copy in its own reader until its
//! real end, and an end that can never be observed (a failed read or an
//! unverified wait) pins a copy until this process exits. A kind without one
//! closes its pipes on return.
use crate::platform;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};
use zerus_client_core::run::custody::InheritedLock;

/// Called first in `main` for each kind's variable `var`. A child started by
/// a desktop actor holds the actor's close-only custody lock for exactly its
/// own lifetime: the inherited descriptor is marked close-on-exec before
/// anything else runs, and the variable naming it is removed. A malformed
/// value refuses with `invalid`, an unusable descriptor with `unavailable`,
/// before the child starts anything.
pub(in crate::desktop) fn adopt(var: &str, invalid: &str, unavailable: &str) -> Result<(), String> {
    let Some(value) = std::env::var_os(var) else {
        return Ok(());
    };
    // Single-threaded: main has started nothing yet.
    std::env::remove_var(var);
    let fd = value
        .to_str()
        .filter(|text| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|text| text.parse::<i32>().ok())
        .filter(|fd| *fd > 2)
        .ok_or(invalid)?;
    // SAFETY: fcntl on a descriptor number; failure is reported, never ignored.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFD);
        if flags < 0 || libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) < 0 {
            return Err(unavailable.into());
        }
    }
    Ok(())
}

/// Frozen stdin: the record's bytes, or an exact input file read only
/// through its kept handle.
// Only the task message leaf, fenced on macOS, constructs one there.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub(in crate::desktop) enum Stdin<'a> {
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
    pub(in crate::desktop) fn to_vec(&self) -> std::io::Result<Vec<u8>> {
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

/// The child's input.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub(in crate::desktop) enum Input<'a> {
    /// Closed (`/dev/null`).
    Closed,
    /// Exactly `stdin`; an exit before all of it was written is
    /// `Unknown(unwritten)`.
    Frozen {
        stdin: Stdin<'a>,
        unwritten: &'static str,
    },
}

/// The one output kept; the other is drained and discarded.
#[cfg_attr(target_os = "macos", allow(dead_code))]
#[derive(Clone, Copy)]
pub(in crate::desktop) enum Output {
    /// The first `n` bytes of stderr; more is discarded.
    StderrPrefix(usize),
    /// All of stdout up to `bytes`; more makes the result
    /// `Unknown(too_large)`.
    Stdout {
        bytes: usize,
        too_large: &'static str,
    },
}

/// One kind's reasons for an Unknown result.
#[derive(Clone, Copy)]
pub(in crate::desktop) struct Codes {
    /// The wait could not be observed (the child was then killed).
    pub unobserved: &'static str,
    /// An output pipe could not be read; its end is never assumed.
    pub unreadable: &'static str,
    /// A signal ended the child.
    pub signalled: &'static str,
    /// The deadline passed while the child ran (the child alone was killed).
    pub timed_out: &'static str,
    /// The child exited, but another process kept its output open past the
    /// deadline.
    pub retained: &'static str,
}

/// The guard a kind's still-open pipes keep until their real end, and the
/// name of the reader that keeps it.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub(in crate::desktop) struct Retain<'a> {
    pub guard: &'a Arc<dyn Send + Sync>,
    pub reader: &'static str,
}

/// One kind's child.
pub(in crate::desktop) struct Spec<'a> {
    /// Names the inherited custody descriptor.
    pub custody_env: &'static str,
    pub input: Input<'a>,
    pub output: Output,
    pub codes: Codes,
    /// `None` closes every original pipe on return.
    pub retain: Option<Retain<'a>>,
}

/// How the original child ended.
#[derive(Debug, PartialEq, Eq)]
pub(in crate::desktop) enum Ended {
    /// It exited with `code` within the deadline, both output pipes ended and
    /// its whole input was written; `output` is the kept output.
    Exited { code: i32, output: Vec<u8> },
    /// It never started: refused before any child existed (the own image, its
    /// custody, the kind's final check, or the spawn itself, which reports a
    /// fork or exec failure before any program code runs). Nothing ran.
    NotStarted,
    /// Its result is unknown, for the kind's named reason.
    Unknown(&'static str),
}

#[cfg(test)]
thread_local! {
    /// Makes the next spawn on this thread fail inside `pre_exec`, as a custody
    /// refusal there would.
    pub(in crate::desktop) static REFUSE_PRE_EXEC: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    // One-shot observation faults on the original executor thread. Anonymous
    // pipes do not safely provide deterministic EIO, and provoking wait errors
    // by stealing waitpid/SIGCHLD would interfere with other test children.
    pub(in crate::desktop) static READ_ERROR: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    pub(in crate::desktop) static WAIT_ERROR: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    // The same wait fault, once the child's first kept output was read.
    pub(in crate::desktop) static WAIT_ERROR_AFTER_OUTPUT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    pub(in crate::desktop) static SPAWNED: std::cell::Cell<Option<u32>> = const { std::cell::Cell::new(None) };
    /// Runs once on this thread after the final check, right before spawn.
    pub(in crate::desktop) static AFTER_FINAL_CHECK: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}

/// An original output pipe.
trait Pipe: Read + AsRawFd + Send {}
impl<T: Read + AsRawFd + Send> Pipe for T {}

/// Keeps `guard` as long as this process lives: the end it waits for can
/// never be observed.
fn pin(guard: &Arc<dyn Send + Sync>) {
    std::mem::forget(guard.clone());
}

/// Keeps an original output pipe, and with it a copy of the guard, until the
/// pipe's real end. What it reads is discarded and nothing is signalled.
fn retain(mut pipe: Box<dyn Pipe>, retain: &Retain<'_>) {
    let fd = pipe.as_raw_fd();
    // SAFETY: fcntl on a descriptor this reader now owns; reads block from here.
    unsafe {
        libc::fcntl(
            fd,
            libc::F_SETFL,
            libc::fcntl(fd, libc::F_GETFL) & !libc::O_NONBLOCK,
        );
    }
    let held = retain.guard.clone();
    let reader = std::thread::Builder::new()
        .name(retain.reader.into())
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
        pin(retain.guard);
    }
}

/// Runs `program args` as `spec`'s original child under `custody`, to its end
/// or to `deadline`. `final_check` runs inside the spawn turn, last before the
/// spawn: its refusal means that no child ever existed.
pub(in crate::desktop) fn run(
    program: &Path,
    args: &[String],
    spec: Spec<'_>,
    custody: &Arc<InheritedLock>,
    deadline: Instant,
    final_check: impl FnOnce(&mut Command) -> std::io::Result<()>,
) -> Ended {
    start(program, args, spec, Some(custody), deadline, final_check)
}

/// Runs a child that holds no custody, by the same rules: a read without
/// any effect of its own (the deferred answer's observation), which no
/// drain waits for. It inherits no lock and names none in `spec`'s variable.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub(in crate::desktop) fn run_unheld(
    program: &Path,
    args: &[String],
    spec: Spec<'_>,
    deadline: Instant,
    final_check: impl FnOnce(&mut Command) -> std::io::Result<()>,
) -> Ended {
    start(program, args, spec, None, deadline, final_check)
}

fn start(
    program: &Path,
    args: &[String],
    spec: Spec<'_>,
    custody: Option<&Arc<InheritedLock>>,
    deadline: Instant,
    final_check: impl FnOnce(&mut Command) -> std::io::Result<()>,
) -> Ended {
    let Spec {
        custody_env,
        input,
        output,
        codes,
        retain: kept,
    } = spec;
    let pin_guard = || {
        if let Some(retain) = &kept {
            pin(retain.guard);
        }
    };
    let (input, unwritten) = match input {
        Input::Closed => (None, ""),
        Input::Frozen { stdin, unwritten } => (Some(stdin), unwritten),
    };
    let mut command = Command::new(program);
    command
        .args(args)
        .process_group(0)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(custody) = custody {
        command.env(custody_env, custody.as_raw_fd().to_string());
    }
    let spawned = {
        let _turn = platform::spawn_turn();
        custody
            .map_or(Ok(()), |custody| custody.inherit(&mut command))
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
    let mut chunk = vec![0u8; if input.is_some() { 64 << 10 } else { 0 }];
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
    let mut kept_output = Vec::new();
    let mut overflow = None;
    let mut status = None;
    // A pipe that cannot be read is never taken as its end.
    let mut unreadable = false;
    let ended = loop {
        if let (Some(pipe), Some(input)) = (stdin.as_mut(), input.as_ref()) {
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
                        Err(std::io::Error::other("private original output read fault"))
                    } else {
                        read
                    }
                });
                match read {
                    Ok(0) => {
                        *slot = None;
                        break;
                    }
                    Ok(n) => match output {
                        Output::StderrPrefix(bound) => {
                            if index == 1 && kept_output.len() < bound {
                                let keep = n.min(bound - kept_output.len());
                                kept_output.extend_from_slice(&chunk[..keep]);
                            }
                        }
                        Output::Stdout { bytes, too_large } => {
                            if index == 0 {
                                if kept_output.len() + n > bytes {
                                    overflow = Some(too_large);
                                } else {
                                    kept_output.extend_from_slice(&chunk[..n]);
                                }
                            }
                        }
                    },
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => {
                        // Its real end can never be observed now.
                        pin_guard();
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
                        .with(|armed| !kept_output.is_empty() && armed.replace(false))
                {
                    Err(std::io::Error::other("private original wait fault"))
                } else {
                    observed
                }
            });
            match observed {
                Ok(found) => status = found,
                Err(_) => {
                    let _ = child.kill();
                    if child.wait().is_err() {
                        pin_guard();
                    }
                    break Ended::Unknown(codes.unobserved);
                }
            }
        }
        if let Some(status) = status {
            if pipes.iter().all(Option::is_none) {
                if unreadable {
                    break Ended::Unknown(codes.unreadable);
                }
                if let Some(too_large) = overflow {
                    break Ended::Unknown(too_large);
                }
                break match status.code() {
                    Some(code) => Ended::Exited {
                        code,
                        output: std::mem::take(&mut kept_output),
                    },
                    None => Ended::Unknown(codes.signalled),
                };
            }
        }
        if Instant::now() >= deadline {
            if status.is_none() {
                // The original child alone; never its group or anything else.
                let _ = child.kill();
                if child.wait().is_err() {
                    pin_guard();
                }
                break Ended::Unknown(codes.timed_out);
            }
            break Ended::Unknown(codes.retained);
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    drop(stdin);
    // A descendant may still hold an original output pipe: with a guard, it
    // keeps that guard until the pipe's real end, separately from the result
    // returned here; without one, the pipe closes now.
    if let Some(retain_with) = &kept {
        for pipe in pipes.into_iter().flatten() {
            retain(pipe, retain_with);
        }
    }
    match ended {
        Ended::Exited { .. } if written < input.as_ref().map_or(0, Stdin::len) => {
            Ended::Unknown(unwritten)
        }
        ended => ended,
    }
}
