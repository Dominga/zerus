//! Process-local Unix spawn and endpoint preparation. This module changes no
//! child setup, executable selection, guard decision, wait or termination rule.
//! In particular it installs no pre-exec hook and preserves std's spawn backend.
use std::{
    fs::OpenOptions,
    io,
    os::{
        fd::{AsFd, OwnedFd},
        unix::net::UnixStream,
    },
    process::{Child, Command},
    sync::{Mutex, MutexGuard, PoisonError, TryLockError, TryLockResult},
};

static GATE: Mutex<()> = Mutex::new(());

/// One parent-side turn for endpoint preparation and the actual OS spawn.
/// Do not acquire another turn while holding this one; use the `_in_turn`
/// variants instead. Release before waiting or running post-spawn guards.
pub struct Turn {
    _guard: MutexGuard<'static, ()>,
}

/// Same poison-recovery rule as the root platform's original spawn turn.
pub fn turn() -> Turn {
    Turn {
        _guard: GATE.lock().unwrap_or_else(PoisonError::into_inner),
    }
}

/// A caller with an existing deadline/cancellation loop keeps that loop and
/// its original checks. This function supplies no new timeout or retry policy.
pub fn try_turn() -> TryLockResult<Turn> {
    match GATE.try_lock() {
        Ok(guard) => Ok(Turn { _guard: guard }),
        Err(TryLockError::Poisoned(error)) => Err(TryLockError::Poisoned(PoisonError::new(Turn {
            _guard: error.into_inner(),
        }))),
        Err(TryLockError::WouldBlock) => Err(TryLockError::WouldBlock),
    }
}

/// Spawn only: the caller keeps its Child, I/O, guards and wait policy.
pub fn spawn(command: &mut Command) -> io::Result<Child> {
    let turn = turn();
    spawn_in_turn(&turn, command)
}

/// The same std spawn when the caller already owns endpoint preparation's turn.
pub fn spawn_in_turn(_turn: &Turn, command: &mut Command) -> io::Result<Child> {
    command.spawn()
}

/// std supplies CLOEXEC; the shared turn also covers platforms where setting
/// that flag follows socketpair creation. No extra descriptor is retained.
pub fn socket_pair() -> io::Result<(UnixStream, UnixStream)> {
    let turn = turn();
    socket_pair_in_turn(&turn)
}
pub fn socket_pair_in_turn(_turn: &Turn) -> io::Result<(UnixStream, UnixStream)> {
    UnixStream::pair()
}

/// Empty helper input retains its existing /dev/null writer marker for guards.
pub(crate) fn null_pair_in_turn(_turn: &Turn) -> io::Result<(OwnedFd, OwnedFd)> {
    let null = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/null")?;
    let writer = null.try_clone()?;
    Ok((OwnedFd::from(null), OwnedFd::from(writer)))
}

/// A CLOEXEC duplicate created in a caller's existing preparation turn.
pub fn duplicate_in_turn(_turn: &Turn, fd: impl AsFd) -> io::Result<OwnedFd> {
    fd.as_fd().try_clone_to_owned()
}

/// A nonblocking native stream socket, with the existing Linux atomic flags
/// and Darwin fcntl setup. The descriptor becomes owned before fallible setup.
pub fn nonblocking_stream() -> io::Result<UnixStream> {
    #[cfg(target_os = "macos")]
    use std::os::fd::AsRawFd;
    use std::os::fd::FromRawFd;
    let _turn = turn();
    #[cfg(not(target_os = "macos"))]
    let kind = libc::SOCK_STREAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK;
    #[cfg(target_os = "macos")]
    let kind = libc::SOCK_STREAM;
    let raw = unsafe { libc::socket(libc::AF_UNIX, kind, 0) };
    if raw < 0 {
        return Err(io::Error::last_os_error());
    }
    let socket = unsafe { UnixStream::from_raw_fd(raw) };
    #[cfg(target_os = "macos")]
    {
        let raw = socket.as_raw_fd();
        if unsafe { libc::fcntl(raw, libc::F_SETFD, libc::FD_CLOEXEC) } < 0
            || unsafe { libc::fcntl(raw, libc::F_SETFL, libc::O_NONBLOCK) } < 0
        {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(socket)
}

/// Tauri's existing Linux pidfd-spawn backend prepares this pipe in its turn.
/// It still owns its file actions, pidfd identity and wait behavior.
#[cfg(target_os = "linux")]
pub fn pipe_in_turn(_turn: &Turn) -> io::Result<(OwnedFd, OwnedFd)> {
    use std::os::fd::FromRawFd;
    let mut ends = [0; 2];
    if unsafe { libc::pipe2(ends.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { (OwnedFd::from_raw_fd(ends[0]), OwnedFd::from_raw_fd(ends[1])) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{os::fd::AsRawFd, process::Stdio, sync::mpsc, time::Duration};

    fn cloexec(fd: &impl AsFd) {
        let flags = unsafe { libc::fcntl(fd.as_fd().as_raw_fd(), libc::F_GETFD) };
        assert!(flags >= 0);
        assert_ne!(flags & libc::FD_CLOEXEC, 0);
    }
    #[test]
    fn prepared_endpoints_keep_cloexec_and_empty_input_marker() {
        let turn = turn();
        let (read, write) = socket_pair_in_turn(&turn).unwrap();
        cloexec(&read);
        cloexec(&write);
        let duplicate = duplicate_in_turn(&turn, &write).unwrap();
        cloexec(&duplicate);
        let (null, marker) = null_pair_in_turn(&turn).unwrap();
        cloexec(&null);
        cloexec(&marker);
        use std::os::unix::fs::FileTypeExt;
        assert!(std::fs::File::from(null)
            .metadata()
            .unwrap()
            .file_type()
            .is_char_device());
        #[cfg(target_os = "linux")]
        {
            let (read, write) = pipe_in_turn(&turn).unwrap();
            cloexec(&read);
            cloexec(&write);
        }
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn native_stream_keeps_nonblocking_and_cloexec() {
        let socket = nonblocking_stream().unwrap();
        cloexec(&socket);
        let flags = unsafe { libc::fcntl(socket.as_raw_fd(), libc::F_GETFL) };
        assert!(flags >= 0);
        assert_ne!(flags & libc::O_NONBLOCK, 0);
    }
    #[test]
    fn a_live_child_does_not_retain_its_parents_spawn_turn() {
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "read value || true"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut first = spawn(&mut command).unwrap();
        assert!(first.try_wait().unwrap().is_none());
        let (sent, received) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let mut command = Command::new("/usr/bin/true");
            let mut child = spawn(&mut command).unwrap();
            sent.send(child.wait().unwrap().success()).unwrap();
        });
        let second = received.recv_timeout(Duration::from_secs(2));
        drop(first.stdin.take());
        assert!(first.wait().unwrap().success());
        assert_eq!(
            second,
            Ok(true),
            "the turn must end before any wait or guard"
        );
        worker.join().unwrap();
    }
}
