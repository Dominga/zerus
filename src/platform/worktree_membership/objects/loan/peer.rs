//! Binding of a local loan socket to an ORIGINAL guarded helper. The expected
//! PID/birth must come from the selected pipeline's original spawn, over its
//! private control stream. Neither this observation nor a loan is consent.
use crate::platform::{PinnedProcess, ProcessIdentity, ProcessState};
use std::{
    fs::{File, OpenOptions},
    io,
    os::{
        fd::AsRawFd,
        unix::{
            fs::{MetadataExt, OpenOptionsExt},
            net::UnixStream,
        },
    },
    time::Instant,
};

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

#[derive(PartialEq, Eq)]
struct Image {
    device: u64,
    inode: u64,
    bytes: u64,
    changed: (i64, i64),
}
fn image(file: &File) -> io::Result<Image> {
    let m = file.metadata()?;
    if !m.is_file() || m.len() == 0 || m.ino() == 0 || m.mode() & 0o111 == 0 {
        return Err(invalid());
    }
    Ok(Image {
        device: m.dev(),
        inode: m.ino(),
        bytes: m.len(),
        changed: (m.ctime(), m.ctime_nsec()),
    })
}
fn executable(pid: u32) -> io::Result<File> {
    // This is the kernel's executing image object, never the displayed path.
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(format!("/proc/{pid}/exe"))
}
fn credentials(stream: &UnixStream) -> io::Result<libc::ucred> {
    let mut value: libc::ucred = unsafe { std::mem::zeroed() };
    let mut length = std::mem::size_of_val(&value) as libc::socklen_t;
    if unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut value as *mut libc::ucred).cast(),
            &mut length,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    if length as usize != std::mem::size_of_val(&value)
        || value.pid <= 1
        || value.uid != unsafe { libc::geteuid() }
        || value.gid != unsafe { libc::getegid() }
    {
        return Err(invalid());
    }
    Ok(value)
}

/// Non-serializable native evidence. Keep its pidfd and executing image open
/// until the original mutating attempt actually retires. A closed socket, a
/// release message or a failed current() is NOT a positive exit observation.
pub(crate) struct Peer {
    pid: u32,
    original: ProcessIdentity,
    pin: PinnedProcess,
    file: File,
    image: Image,
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
        if audit.is_some() {
            return Err(invalid());
        }
        let before = crate::platform::process_identity(pid).ok_or_else(invalid)?;
        if pid == std::process::id()
            || before.birth != birth
            || parent == 0
            || before.parent != parent
        {
            return Err(invalid());
        }
        let pin = PinnedProcess::pin(pid, birth).map_err(io::Error::other)?;
        let file = executable(pid)?;
        let image = image(&file)?;
        // Every phase executes the SAME retained Source image as this holder.
        // Equal bytes, a path, a memfd name or same UID is insufficient here.
        if image != self::image(&executable(std::process::id())?)? {
            return Err(invalid());
        }
        let peer = Self {
            pid,
            original: before,
            pin,
            file,
            image,
        };
        peer.process_current(until)?;
        Ok(peer)
    }
    #[cfg(test)]
    pub(crate) fn original(
        stream: &UnixStream,
        pid: u32,
        birth: &str,
        audit: Option<&[u32; 8]>,
        until: Instant,
    ) -> io::Result<Self> {
        let peer = Self::pin(pid, birth, std::process::id(), audit, until)?;
        peer.current(stream, until)?;
        Ok(peer)
    }
    pub(crate) fn current(&self, stream: &UnixStream, until: Instant) -> io::Result<()> {
        check(until)?;
        let credential = credentials(stream)?;
        if credential.pid as u32 != self.pid {
            return Err(invalid());
        }
        self.process_current(until)
    }
    fn process_current(&self, until: Instant) -> io::Result<()> {
        if crate::platform::process_identity(self.pid).as_ref() != Some(&self.original)
            || image(&self.file)? != self.image
            || image(&executable(self.pid)?)? != self.image
            || crate::platform::process_identity(self.pid).as_ref() != Some(&self.original)
        {
            return Err(invalid());
        }
        check(until)
    }
    pub(crate) fn exited(&self) -> io::Result<bool> {
        self.pin.exit_observed()
    }
    /// Admission retains the old currentness refusal as well as requiring no
    /// physical exit. A false physical-exit observation supplies no live-code
    /// or socket authority; current()/pin() still own those independent gates.
    pub(crate) fn refuses_admission(&self) -> io::Result<bool> {
        Ok(self.pin.state().map_err(io::Error::other)? == ProcessState::Exited || self.exited()?)
    }
}
pub(crate) fn own_audit() -> io::Result<Option<[u32; 8]>> {
    Ok(None)
}

#[cfg(test)]
pub(crate) mod tests;
