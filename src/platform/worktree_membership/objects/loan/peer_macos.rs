//! Native identity binding, NOT live-code authentication or consent. The
//! original private owner must obtain Expected's token from its SAME unreaped
//! Child's genuine paired live-code observation before writing the arm frame.
//! A caller-supplied PID/token or a same-UID socket cannot establish that origin.
use crate::platform::{PinnedProcess, ProcessIdentity, ProcessState};
use std::{
    io,
    os::{fd::AsRawFd, unix::net::UnixStream},
    time::Instant,
};

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
struct Audit {
    words: [u32; 8],
}
#[link(name = "bsm")]
unsafe extern "C" {
    fn audit_token_to_pid(token: Audit) -> libc::pid_t;
    fn audit_token_to_pidversion(token: Audit) -> i32;
    fn audit_token_to_euid(token: Audit) -> libc::uid_t;
    fn audit_token_to_ruid(token: Audit) -> libc::uid_t;
}
#[repr(C)]
struct Unique {
    _image: [u8; 16],
    birth: u64,
    _parent_birth: u64,
    version: i32,
    _parent_version: i32,
    _reserved: [u64; 2],
}
const _: () = assert!(std::mem::size_of::<Audit>() == 32 && std::mem::size_of::<Unique>() == 56);
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "the original native loan helper changed",
    )
}
fn check(until: Instant) -> io::Result<()> {
    if Instant::now() >= until {
        Err(io::ErrorKind::TimedOut.into())
    } else {
        Ok(())
    }
}
fn token(token: Audit, pid: u32) -> io::Result<Audit> {
    if pid <= 1
        || pid > i32::MAX as u32
        || unsafe { audit_token_to_pid(token) } != pid as i32
        || unsafe { audit_token_to_pidversion(token) } == 0
        || unsafe { audit_token_to_euid(token) } != unsafe { libc::geteuid() }
        || unsafe { audit_token_to_ruid(token) } != unsafe { libc::getuid() }
    {
        return Err(invalid());
    }
    Ok(token)
}
fn version(pid: u32) -> io::Result<i32> {
    let mut value: Unique = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of_val(&value) as i32;
    if unsafe { libc::proc_pidinfo(pid as i32, 17, 0, (&mut value as *mut Unique).cast(), size) }
        != size
        || value.birth == 0
        || value.version == 0
    {
        return Err(invalid());
    }
    Ok(value.version)
}
fn socket_token(socket: &UnixStream) -> io::Result<Audit> {
    let mut value = Audit { words: [0; 8] };
    let mut size = std::mem::size_of_val(&value) as libc::socklen_t;
    if unsafe {
        libc::getsockopt(
            socket.as_raw_fd(),
            libc::SOL_LOCAL,
            libc::LOCAL_PEERTOKEN,
            (&mut value as *mut Audit).cast(),
            &mut size,
        )
    } != 0
        || size as usize != std::mem::size_of_val(&value)
    {
        return Err(invalid());
    }
    Ok(value)
}
pub(crate) fn own_audit() -> io::Result<Option<[u32; 8]>> {
    unsafe extern "C" {
        static mach_task_self_: u32;
    }
    let mut value = Audit { words: [0; 8] };
    let mut count = 8;
    if unsafe {
        libc::task_info(
            mach_task_self_,
            15,
            (&mut value as *mut Audit).cast(),
            &mut count,
        )
    } != 0
        || count != 8
    {
        return Err(invalid());
    }
    Ok(Some(token(value, std::process::id())?.words))
}
pub(crate) struct Peer {
    pid: u32,
    original: ProcessIdentity,
    expected: Audit,
    pin: PinnedProcess,
}
impl Peer {
    pub(in crate::platform::worktree_membership) fn pid(&self) -> u32 {
        self.pid
    }
    pub(crate) fn pin(
        pid: u32,
        birth: &str,
        parent: u32,
        audit: Option<&[u32; 8]>,
        until: Instant,
    ) -> io::Result<Self> {
        check(until)?;
        let original = crate::platform::process_identity(pid).ok_or_else(invalid)?;
        if pid == std::process::id()
            || original.birth != birth
            || parent == 0
            || original.parent != parent
        {
            return Err(invalid());
        }
        let expected = token(
            Audit {
                words: *audit.ok_or_else(invalid)?,
            },
            pid,
        )?;
        let pin = PinnedProcess::pin(pid, birth).map_err(io::Error::other)?;
        let peer = Self {
            pid,
            original,
            expected,
            pin,
        };
        peer.process_current(until)?;
        Ok(peer)
    }
    #[cfg(test)]
    pub(crate) fn original(
        socket: &UnixStream,
        pid: u32,
        birth: &str,
        audit: Option<&[u32; 8]>,
        until: Instant,
    ) -> io::Result<Self> {
        let peer = Self::pin(pid, birth, std::process::id(), audit, until)?;
        peer.current(socket, until)?;
        Ok(peer)
    }
    fn process_current(&self, until: Instant) -> io::Result<()> {
        check(until)?;
        if crate::platform::process_identity(self.pid).as_ref() != Some(&self.original)
            || version(self.pid)? != unsafe { audit_token_to_pidversion(self.expected) }
            || crate::platform::process_identity(self.pid).as_ref() != Some(&self.original)
        {
            return Err(invalid());
        }
        check(until)
    }
    pub(crate) fn current(&self, socket: &UnixStream, until: Instant) -> io::Result<()> {
        self.process_current(until)?;
        if token(socket_token(socket)?, self.pid)? != self.expected {
            return Err(invalid());
        }
        // Socket credentials were captured at connect. A SAME-birth exec is
        // refused by a fresh native idversion read on BOTH sides of the token.
        self.process_current(until)
    }
    pub(crate) fn exited(&self) -> io::Result<bool> {
        self.pin
            .state()
            .map(|state| state == ProcessState::Exited)
            .map_err(io::Error::other)
    }
}

#[cfg(test)]
#[path = "peer/tests.rs"]
pub(crate) mod tests;
