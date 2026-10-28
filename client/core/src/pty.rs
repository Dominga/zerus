//! The Terminal's attach client in its own pseudoterminal, ported from
//! `tray/src/PtyProcess.cpp`. `forkpty` gives the client a new session and
//! process group holding only `hgs a`, ssh and the tmux client, so a hang-up
//! of that group never reaches a tmux server or an agent.
//!
//! One I/O thread per attach polls the master: it reads at most 256 KiB per
//! round and hands the bytes on, writes queued input, and reaps the client.
//! Delivery waits while the consumer's bounded channel is full, so a flood of
//! agent output never grows the client's memory.
use std::collections::VecDeque;
use std::ffi::{CString, OsString};
use std::io::{Read, Write};
use std::os::fd::RawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::terminal::AttachEvent as Event;

/// Bytes read per round before the output is handed on.
pub(crate) const MAX_READ: usize = 256 * 1024;
/// Queued terminal input.
pub(crate) const MAX_QUEUED_INPUT: usize = 4 * 1024 * 1024;
/// Output kept for a view that is shown again.
pub(crate) const REPLAY_BYTES: usize = 1024 * 1024;
/// How often the client's exit is checked, as Qt's reaper timer.
const REAP: Duration = Duration::from_millis(75);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The size a terminal gets: 2-1000 columns and 2-500 rows.
pub(crate) fn window(cols: u16, rows: u16) -> libc::winsize {
    libc::winsize {
        ws_col: cols.clamp(2, 1000),
        ws_row: rows.clamp(2, 500),
        ws_xpixel: 0,
        ws_ypixel: 0,
    }
}

fn executable(path: &Path) -> bool {
    std::fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

/// `program` itself when absolute, otherwise the first executable of that
/// name on `PATH` (`QStandardPaths::findExecutable`).
fn resolve(program: &Path) -> Option<PathBuf> {
    if program.is_absolute() {
        return executable(program).then(|| program.to_path_buf());
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|folder| folder.join(program))
        .find(|candidate| candidate.is_absolute() && executable(candidate))
}

fn c_string(bytes: &[u8]) -> Result<CString, String> {
    CString::new(bytes)
        .map_err(|_| "Cannot open terminal: an argument contains a NUL byte".to_owned())
}

struct Shared {
    /// The master, until the I/O thread closes it.
    master: Mutex<Option<RawFd>>,
    /// The client, until it is reaped.
    pid: Mutex<Option<libc::pid_t>>,
    pending: Mutex<Vec<u8>>,
    replay: Mutex<VecDeque<u8>>,
    stopped: AtomicBool,
    wake: UnixStream,
}

impl Shared {
    fn wake(&self) {
        let _ = (&self.wake).write(&[0]);
    }
}

/// A running attach client.
pub(crate) struct Pty {
    shared: Arc<Shared>,
    io: Option<JoinHandle<()>>,
}

impl Pty {
    /// Start `program` with `args` and exactly `env` in a new pseudoterminal.
    pub(crate) fn start(
        program: &Path,
        args: &[String],
        env: &[(OsString, OsString)],
        cols: u16,
        rows: u16,
        events: SyncSender<Event>,
    ) -> Result<Pty, String> {
        let unavailable = || format!("Attach executable is unavailable: {}", program.display());
        let path = resolve(program).ok_or_else(unavailable)?;
        let mut argv = vec![c_string(path.as_os_str().as_bytes())?];
        for arg in args {
            argv.push(c_string(arg.as_bytes())?);
        }
        let mut envp = Vec::with_capacity(env.len());
        for (key, value) in env {
            let mut entry = key.as_bytes().to_vec();
            entry.push(b'=');
            entry.extend_from_slice(value.as_bytes());
            envp.push(c_string(&entry)?);
        }
        let mut argv_ptrs: Vec<*const libc::c_char> = argv.iter().map(|arg| arg.as_ptr()).collect();
        argv_ptrs.push(std::ptr::null());
        let mut envp_ptrs: Vec<*const libc::c_char> =
            envp.iter().map(|entry| entry.as_ptr()).collect();
        envp_ptrs.push(std::ptr::null());
        let (wake_read, wake) =
            UnixStream::pair().map_err(|error| format!("Cannot open terminal: {error}"))?;
        wake_read
            .set_nonblocking(true)
            .and_then(|()| wake.set_nonblocking(true))
            .map_err(|error| format!("Cannot open terminal: {error}"))?;
        // SAFETY: plain libc calls. Everything the child touches between fork
        // and exec was prepared above, and it calls only async-signal-safe
        // functions.
        let (master, pid) = unsafe {
            let max_fd = libc::sysconf(libc::_SC_OPEN_MAX).clamp(256, 65536) as libc::c_int;
            let mut size = window(cols, rows);
            // A very fast stop can arrive before the child resets its signal
            // handlers: termination stays blocked across the fork until then.
            let mut blocked: libc::sigset_t = std::mem::zeroed();
            let mut previous: libc::sigset_t = std::mem::zeroed();
            let mut empty: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut blocked);
            libc::sigemptyset(&mut empty);
            for signal in [libc::SIGHUP, libc::SIGTERM, libc::SIGINT, libc::SIGQUIT] {
                libc::sigaddset(&mut blocked, signal);
            }
            libc::pthread_sigmask(libc::SIG_BLOCK, &blocked, &mut previous);
            let mut master: libc::c_int = -1;
            let pid = libc::forkpty(
                &mut master,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut size as *mut libc::winsize as _,
            );
            let fork_error = std::io::Error::last_os_error();
            if pid == 0 {
                for signal in [
                    libc::SIGINT,
                    libc::SIGQUIT,
                    libc::SIGTERM,
                    libc::SIGHUP,
                    libc::SIGPIPE,
                    libc::SIGCHLD,
                    libc::SIGTSTP,
                    libc::SIGTTIN,
                    libc::SIGTTOU,
                ] {
                    libc::signal(signal, libc::SIG_DFL);
                }
                libc::sigprocmask(libc::SIG_SETMASK, &empty, std::ptr::null_mut());
                for fd in 3..max_fd {
                    libc::close(fd);
                }
                libc::execve(argv_ptrs[0], argv_ptrs.as_ptr(), envp_ptrs.as_ptr());
                const MESSAGE: &[u8] = b"Unable to start the session attach client.\r\n";
                libc::write(libc::STDERR_FILENO, MESSAGE.as_ptr().cast(), MESSAGE.len());
                libc::_exit(127);
            }
            libc::pthread_sigmask(libc::SIG_SETMASK, &previous, std::ptr::null_mut());
            if pid < 0 {
                return Err(format!("Cannot open terminal: {fork_error}"));
            }
            libc::fcntl(master, libc::F_SETFD, libc::FD_CLOEXEC);
            libc::fcntl(
                master,
                libc::F_SETFL,
                libc::fcntl(master, libc::F_GETFL) | libc::O_NONBLOCK,
            );
            (master, pid)
        };
        let shared = Arc::new(Shared {
            master: Mutex::new(Some(master)),
            pid: Mutex::new(Some(pid)),
            pending: Mutex::new(Vec::new()),
            replay: Mutex::new(VecDeque::new()),
            stopped: AtomicBool::new(false),
            wake,
        });
        let io_shared = Arc::clone(&shared);
        let io = std::thread::Builder::new()
            .name("zerus-terminal-io".into())
            .spawn(move || Io::new(io_shared, master, wake_read, events).run())
            .map_err(|error| {
                // SAFETY: the client was just started; end it with its group.
                unsafe {
                    libc::kill(-pid, libc::SIGHUP);
                    libc::close(master);
                }
                reap_later(pid);
                format!("Cannot open terminal: {error}")
            })?;
        Ok(Pty {
            shared,
            io: Some(io),
        })
    }

    pub(crate) fn pid(&self) -> Option<u32> {
        lock(&self.shared.pid).map(|pid| pid as u32)
    }

    /// Queue input. Over 4 MiB pending it fails, and nothing is queued.
    pub(crate) fn write(&self, bytes: &[u8]) -> Result<(), String> {
        if bytes.is_empty() || lock(&self.shared.master).is_none() {
            return Ok(());
        }
        {
            let mut pending = lock(&self.shared.pending);
            if bytes.len() > MAX_QUEUED_INPUT - pending.len() {
                return Err("Terminal input queue is full; wait before pasting more text.".into());
            }
            pending.extend_from_slice(bytes);
        }
        self.shared.wake();
        Ok(())
    }

    pub(crate) fn resize(&self, cols: u16, rows: u16) {
        let master = lock(&self.shared.master);
        if let Some(fd) = *master {
            let size = window(cols, rows);
            // SAFETY: the master stays open while its lock is held.
            unsafe {
                libc::ioctl(fd, libc::TIOCSWINSZ, &size);
            }
        }
    }

    pub(crate) fn replay(&self) -> Vec<u8> {
        lock(&self.shared.replay).iter().copied().collect()
    }

    /// Hang up the client's group and close the terminal. Nothing is reported
    /// after a stop.
    pub(crate) fn stop(&mut self) {
        if self.shared.stopped.swap(true, Ordering::SeqCst) {
            return;
        }
        if let Some(pid) = *lock(&self.shared.pid) {
            // SAFETY: the group created by forkpty, not yet reaped (the lock
            // keeps the I/O thread from reaping it meanwhile).
            unsafe {
                libc::kill(-pid, libc::SIGHUP);
            }
        }
        self.shared.wake();
        if let Some(io) = self.io.take() {
            let _ = io.join();
        }
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        self.stop();
    }
}

fn reap_later(pid: libc::pid_t) {
    std::thread::spawn(move || {
        let mut status = 0;
        // SAFETY: waits for our own child.
        while unsafe { libc::waitpid(pid, &mut status, 0) } < 0
            && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
        {}
    });
}

struct Io {
    shared: Arc<Shared>,
    fd: RawFd,
    wake: UnixStream,
    events: SyncSender<Event>,
    /// False once the master is closed: it read its end, or the client ended.
    open: bool,
}

impl Io {
    fn new(shared: Arc<Shared>, fd: RawFd, wake: UnixStream, events: SyncSender<Event>) -> Io {
        Io {
            shared,
            fd,
            wake,
            events,
            open: true,
        }
    }

    fn stopped(&self) -> bool {
        self.shared.stopped.load(Ordering::SeqCst)
    }

    fn drain_wake(&self) {
        let mut buffer = [0; 64];
        while (&self.wake).read(&mut buffer).is_ok_and(|n| n > 0) {}
    }

    /// Waits for the master (while open) or the wake socket, at most
    /// `timeout`.
    fn poll(&self, timeout: Duration, write: bool) {
        let mut events = libc::POLLIN;
        if write {
            events |= libc::POLLOUT;
        }
        let mut fds = [
            libc::pollfd {
                // poll skips a negative descriptor.
                fd: if self.open { self.fd } else { -1 },
                events,
                revents: 0,
            },
            libc::pollfd {
                fd: std::os::fd::AsRawFd::as_raw_fd(&self.wake),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        // SAFETY: both descriptors stay open while this thread runs.
        unsafe {
            libc::poll(fds.as_mut_ptr(), 2, timeout.as_millis() as libc::c_int);
        }
        self.drain_wake();
    }

    fn run(mut self) {
        loop {
            if self.stopped() {
                break;
            }
            let write = !lock(&self.shared.pending).is_empty();
            self.poll(REAP, write);
            if self.stopped() {
                break;
            }
            if !self.read_ready() {
                break;
            }
            self.write_ready();
            if let Some(code) = self.reap() {
                // The client's last output first, as Qt's `reap`.
                if self.read_ready() {
                    self.close();
                    self.deliver(Event::Finished { code });
                }
                return self.close();
            }
        }
        self.close();
        // SAFETY: our own child, under the lock that a stop also takes.
        let mut pid = lock(&self.shared.pid);
        if let Some(child) = pid.take() {
            let mut status = 0;
            if unsafe { libc::waitpid(child, &mut status, libc::WNOHANG) } == 0 {
                reap_later(child);
            }
        }
    }

    /// Reads at most 256 KiB and hands it on; false when a stop interrupted.
    fn read_ready(&mut self) -> bool {
        if !self.open {
            return true;
        }
        let mut bytes = Vec::new();
        let mut buffer = [0u8; 16384];
        while bytes.len() < MAX_READ {
            // Never more than 256 KiB in one round, unlike Qt's loop, which
            // may pass it by part of a buffer.
            let want = buffer.len().min(MAX_READ - bytes.len());
            // SAFETY: reads into a local buffer from the open master.
            let n = unsafe { libc::read(self.fd, buffer.as_mut_ptr().cast(), want) };
            if n > 0 {
                bytes.extend_from_slice(&buffer[..n as usize]);
                continue;
            }
            let error = std::io::Error::last_os_error();
            if n < 0 && error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            if !(n < 0 && error.kind() == std::io::ErrorKind::WouldBlock) {
                // The end of the terminal: every client closed it.
                self.close();
            }
            break;
        }
        if bytes.is_empty() {
            return true;
        }
        {
            let mut replay = lock(&self.shared.replay);
            replay.extend(&bytes);
            let excess = replay.len().saturating_sub(REPLAY_BYTES);
            replay.drain(..excess);
        }
        self.deliver(Event::Output { data: bytes })
    }

    /// Waits while the consumer's channel is full; false after a stop.
    fn deliver(&self, mut event: Event) -> bool {
        loop {
            if self.stopped() {
                return false;
            }
            match self.events.try_send(event) {
                Ok(()) => return true,
                // Nobody listens: the output stays in the replay buffer.
                Err(TrySendError::Disconnected(_)) => return true,
                Err(TrySendError::Full(back)) => {
                    event = back;
                    let mut fds = [libc::pollfd {
                        fd: std::os::fd::AsRawFd::as_raw_fd(&self.wake),
                        events: libc::POLLIN,
                        revents: 0,
                    }];
                    // SAFETY: the wake socket stays open while this thread runs.
                    unsafe {
                        libc::poll(fds.as_mut_ptr(), 1, 20);
                    }
                    self.drain_wake();
                }
            }
        }
    }

    fn write_ready(&mut self) {
        if !self.open {
            return;
        }
        let mut pending = lock(&self.shared.pending);
        while !pending.is_empty() {
            // SAFETY: writes queued bytes to the open master.
            let n = unsafe { libc::write(self.fd, pending.as_ptr().cast(), pending.len()) };
            if n > 0 {
                pending.drain(..n as usize);
                continue;
            }
            let error = std::io::Error::last_os_error();
            if n < 0 && error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            if !(n < 0 && error.kind() == std::io::ErrorKind::WouldBlock) {
                pending.clear();
            }
            break;
        }
    }

    /// The client's exit code once it ended: its status, or 128 plus the
    /// signal that ended it.
    fn reap(&self) -> Option<i32> {
        let mut pid = lock(&self.shared.pid);
        let child = (*pid)?;
        let mut status = 0;
        // SAFETY: our own child, under the lock that a stop also takes.
        let result = unsafe { libc::waitpid(child, &mut status, libc::WNOHANG) };
        if result == 0
            || (result < 0
                && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted)
        {
            return None;
        }
        *pid = None;
        Some(if result < 0 {
            -1
        } else if libc::WIFEXITED(status) {
            libc::WEXITSTATUS(status)
        } else {
            128 + libc::WTERMSIG(status)
        })
    }

    fn close(&mut self) {
        let mut master = lock(&self.shared.master);
        if let Some(fd) = master.take() {
            // SAFETY: only this thread closes the master, once.
            unsafe {
                libc::close(fd);
            }
        }
        self.open = false;
        lock(&self.shared.pending).clear();
    }
}
