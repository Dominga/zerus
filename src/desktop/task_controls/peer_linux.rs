//! Read-only native peer proof. Retain the kernel socket peer's pidfd and
//! pinned proc directory before reading its birth, image or command line.
//! Neither JSON labels nor HGS environment variables participate in this proof.
use crate::platform::FileIdentity;
use std::{
    fs::{File, OpenOptions},
    io::{self, Read},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            fs::{MetadataExt, OpenOptionsExt},
            net::UnixStream,
        },
    },
    time::Instant,
};

const ANCESTORS: usize = 64;
const COMMAND_BYTES: usize = 16 << 10;

fn refused() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "The original task-control peer cannot be verified",
    )
}
fn budget(until: Instant) -> io::Result<()> {
    if Instant::now() >= until {
        Err(io::ErrorKind::TimedOut.into())
    } else {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Birth {
    pid: u32,
    parent: u32,
    ticks: u64,
}

struct Process {
    handle: File,
    directory: File,
    birth: Birth,
}
impl Process {
    fn from_handle(handle: File, pid: u32, until: Instant) -> io::Result<Self> {
        budget(until)?;
        alive(&handle)?;
        let directory = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(format!("/proc/{pid}"))?;
        if directory.metadata()?.uid() != unsafe { libc::geteuid() } {
            return Err(refused());
        }
        let birth = birth(&directory)?;
        if birth.pid != pid || pid <= 1 {
            return Err(refused());
        }
        let process = Self {
            handle,
            directory,
            birth,
        };
        process.current(until)?;
        Ok(process)
    }
    fn parent(pid: u32, until: Instant) -> io::Result<Self> {
        budget(until)?;
        // SAFETY: a scalar kernel PID; the returned fd is owned below. No signal
        // API is exposed by this read-only pin.
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) } as i32;
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Self::from_handle(unsafe { File::from_raw_fd(fd) }, pid, until)
    }
    fn recorded_start(&self, until: Instant) -> io::Result<String> {
        // Legacy record text is second-resolution UTC lstart. Its PID lookup
        // is bracketed by this ORIGINAL pidfd/proc-directory pin, never used
        // to obtain the precise birth or replace the held kernel process.
        self.current(until)?;
        let value = crate::platform::process_start_time(self.birth.pid)
            .filter(|s| !s.is_empty())
            .ok_or_else(refused)?;
        self.current(until)?;
        Ok(value)
    }
    fn current(&self, until: Instant) -> io::Result<()> {
        budget(until)?;
        alive(&self.handle)?;
        if birth(&self.directory)? != self.birth {
            return Err(refused());
        }
        alive(&self.handle)?;
        budget(until)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Image {
    identity: FileIdentity,
    bytes: u64,
    changed: (i64, i64),
    modified: (i64, i64),
}
fn image(file: &File) -> io::Result<Image> {
    let m = file.metadata()?;
    if !m.is_file() || m.len() == 0 || m.mode() & 0o111 == 0 || m.mode() & 0o022 != 0 {
        return Err(refused());
    }
    Ok(Image {
        identity: FileIdentity::of(file)?,
        bytes: m.len(),
        changed: (m.ctime(), m.ctime_nsec()),
        modified: (m.mtime(), m.mtime_nsec()),
    })
}

/// The original socket peer, executing trusted CLI image, argv and each native
/// ancestor remain pinned until the original operation retires. This grants no
/// task, native input, interrupt or Person authority by itself.
pub(super) struct Peer {
    process: Process,
    executing: File,
    trusted: File,
    image: Image,
    arguments: Vec<Vec<u8>>,
    ancestry: Vec<Process>,
}
impl Peer {
    /// Stable executable-object identity, not a particular invocation/PID.
    pub fn image_guard(&self, until: Instant) -> io::Result<String> {
        self.current(until)?;
        image_guard(&self.image)
    }
    pub fn arguments(&self, until: Instant) -> io::Result<Vec<Vec<u8>>> {
        self.current(until)?;
        Ok(self.arguments.clone())
    }
    /// `trusted` comes from the service's selected CLI, not a request path.
    pub fn capture(socket: &UnixStream, trusted: File, until: Instant) -> io::Result<Self> {
        budget(until)?;
        let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
        let mut length = std::mem::size_of_val(&credentials) as libc::socklen_t;
        // SAFETY: live socket and exactly sized writable credentials.
        if unsafe {
            libc::getsockopt(
                socket.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&mut credentials as *mut libc::ucred).cast(),
                &mut length,
            )
        } != 0
            || length as usize != std::mem::size_of_val(&credentials)
            || credentials.pid <= 1
            || credentials.uid != unsafe { libc::geteuid() }
        {
            return Err(refused());
        }
        let mut fd: i32 = -1;
        let mut length = std::mem::size_of_val(&fd) as libc::socklen_t;
        // SAFETY: obtains a pidfd of the SOCKET'S original peer directly. This
        // avoids turning a stale SO_PEERCRED PID into a pin of its replacement.
        if unsafe {
            libc::getsockopt(
                socket.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERPIDFD,
                (&mut fd as *mut i32).cast(),
                &mut length,
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        if fd < 0 {
            return Err(refused());
        }
        let handle = unsafe { File::from_raw_fd(fd) };
        if length as usize != std::mem::size_of_val(&fd) {
            return Err(refused());
        }
        let process = Process::from_handle(handle, credentials.pid as u32, until)?;
        let executing = entry(&process.directory, b"exe\0", false)?;
        let original = image(&executing)?;
        if original != image(&trusted)? {
            return Err(refused());
        }
        let arguments = command(&process.directory)?;
        let peer = Self {
            process,
            executing,
            trusted,
            image: original,
            arguments,
            ancestry: vec![],
        };
        peer.current(until)?;
        Ok(peer)
    }

    /// Exact canonical arguments supplied by the service's closed operation
    /// parser. This does not accept an argv or actor claim from renderer JSON.
    pub fn command(&self, expected: &[&[u8]], until: Instant) -> io::Result<()> {
        self.current(until)?;
        if self
            .arguments
            .get(1..)
            .is_none_or(|args| args.iter().map(Vec::as_slice).ne(expected.iter().copied()))
        {
            return Err(refused());
        }
        Ok(())
    }

    /// The native agent PID/birth comes from that executor's verified run
    /// record. Every parent is held; a missing/reparented/reused link refuses.
    pub fn executor(&mut self, pid: u32, birth: &str, until: Instant) -> io::Result<()> {
        self.current(until)?;
        if !self.ancestry.is_empty() {
            return Err(refused());
        }
        let mut child = self.process.birth.clone();
        let mut seen = std::collections::BTreeSet::from([child.pid]);
        for _ in 0..ANCESTORS {
            if child.pid == pid {
                let matched = self.ancestry.last().unwrap_or(&self.process);
                if matched.recorded_start(until)? != birth {
                    return Err(refused());
                }
                return self.current(until);
            }
            if child.parent <= 1 || !seen.insert(child.parent) {
                return Err(refused());
            }
            let parent = Process::parent(child.parent, until)?;
            if parent.birth.ticks > child.ticks {
                return Err(refused());
            }
            child = parent.birth.clone();
            self.ancestry.push(parent);
        }
        Err(refused())
    }

    pub fn current(&self, until: Instant) -> io::Result<()> {
        self.process.current(until)?;
        if image(&self.executing)? != self.image
            || image(&self.trusted)? != self.image
            || image(&entry(&self.process.directory, b"exe\0", false)?)? != self.image
            || command(&self.process.directory)? != self.arguments
        {
            return Err(refused());
        }
        for process in &self.ancestry {
            process.current(until)?;
        }
        let mut child = &self.process.birth;
        for parent in &self.ancestry {
            if child.parent != parent.birth.pid || parent.birth.ticks > child.ticks {
                return Err(refused());
            }
            child = &parent.birth;
        }
        self.process.current(until)
    }
}

fn alive(handle: &File) -> io::Result<()> {
    let mut fd = libc::pollfd {
        fd: handle.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: one live owned pidfd; zero timeout never blocks the actor.
    let ready = unsafe { libc::poll(&mut fd, 1, 0) };
    if ready == 0 {
        Ok(())
    } else if ready < 0 {
        Err(io::Error::last_os_error())
    } else {
        Err(refused())
    }
}
fn entry(directory: &File, name: &'static [u8], no_follow: bool) -> io::Result<File> {
    let flags = libc::O_RDONLY
        | libc::O_CLOEXEC
        | libc::O_NONBLOCK
        | if no_follow { libc::O_NOFOLLOW } else { 0 };
    // SAFETY: fixed NUL-terminated source literal under a held proc directory.
    let fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr().cast(), flags) };
    if fd < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { File::from_raw_fd(fd) })
    }
}
fn bytes(directory: &File, name: &'static [u8], limit: usize) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    entry(directory, name, true)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(refused());
    }
    Ok(bytes)
}
fn birth(directory: &File) -> io::Result<Birth> {
    let bytes = bytes(directory, b"stat\0", 4096)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| refused())?;
    let pid = text
        .split_once('(')
        .and_then(|(pid, _)| pid.trim().parse().ok())
        .ok_or_else(refused)?;
    let fields: Vec<_> = text
        .rsplit_once(')')
        .ok_or_else(refused)?
        .1
        .split_whitespace()
        .collect();
    if fields.len() < 20 || matches!(fields[0], "Z" | "X" | "T" | "t") {
        return Err(refused());
    }
    Ok(Birth {
        pid,
        parent: fields[1].parse().map_err(|_| refused())?,
        ticks: fields[19].parse().map_err(|_| refused())?,
    })
}
fn command(directory: &File) -> io::Result<Vec<Vec<u8>>> {
    let mut bytes = bytes(directory, b"cmdline\0", COMMAND_BYTES)?;
    if bytes.pop() != Some(0) {
        return Err(refused());
    }
    let arguments: Vec<_> = bytes.split(|b| *b == 0).map(<[u8]>::to_vec).collect();
    if arguments.len() > 64 || arguments.first().is_none_or(Vec::is_empty) {
        return Err(refused());
    }
    Ok(arguments)
}

/// A separately held native agent and its original resident supervisor. The
/// loaded supervisor must be this service's selected trusted hgs image, not
/// merely a newly installed executable at the same path. This proves no Person.
pub(super) struct Resident {
    agent: Process,
    supervisor: Process,
    agent_image: File,
    agent_selected: Image,
    executing: File,
    trusted: File,
    selected: Image,
    arguments: Vec<Vec<u8>>,
}
impl Resident {
    pub fn guard(&self, until: Instant) -> io::Result<String> {
        self.current(until)?;
        crate::tasks::controls::wire::canonical::tagged_digest(
            b"zerus-task-control-resident-linux-v1\0",
            &serde_json::json!({"schema":1,"image":image_guard(&self.selected)?,
                "agent_image":image_guard(&self.agent_selected)?,
                "agent":{"pid":self.agent.birth.pid,"birth":self.agent.birth.ticks},
                "supervisor":{"pid":self.supervisor.birth.pid,"birth":self.supervisor.birth.ticks},
                "argv":self.arguments}),
        )
        .map_err(io::Error::other)
    }
    pub fn capture(
        agent: u32,
        agent_birth: &str,
        supervisor: u32,
        supervisor_start: &str,
        supervisor_birth: &str,
        trusted: File,
        until: Instant,
    ) -> io::Result<Self> {
        if agent == supervisor {
            return Err(refused());
        }
        let agent = Process::parent(agent, until)?;
        let supervisor = Process::parent(supervisor, until)?;
        if agent.recorded_start(until)? != agent_birth
            || supervisor.recorded_start(until)? != supervisor_start
            || supervisor.birth.ticks.to_string() != supervisor_birth
            || agent.birth.parent != supervisor.birth.pid
            || supervisor.birth.ticks > agent.birth.ticks
        {
            return Err(refused());
        }
        let agent_image = entry(&agent.directory, b"exe\0", false)?;
        let agent_selected = image(&agent_image)?;
        let executing = entry(&supervisor.directory, b"exe\0", false)?;
        let selected = image(&executing)?;
        if selected != image(&trusted)? {
            return Err(refused());
        }
        let arguments = command(&supervisor.directory)?;
        let resident = Self {
            agent,
            supervisor,
            agent_image,
            agent_selected,
            executing,
            trusted,
            selected,
            arguments,
        };
        resident.current(until)?;
        Ok(resident)
    }
    pub fn current(&self, until: Instant) -> io::Result<()> {
        self.agent.current(until)?;
        self.supervisor.current(until)?;
        if image(&self.agent_image)? != self.agent_selected
            || image(&entry(&self.agent.directory, b"exe\0", false)?)? != self.agent_selected
            || image(&self.executing)? != self.selected
            || image(&self.trusted)? != self.selected
            || image(&entry(&self.supervisor.directory, b"exe\0", false)?)? != self.selected
            || command(&self.supervisor.directory)? != self.arguments
        {
            return Err(refused());
        }
        self.agent.current(until)?;
        self.supervisor.current(until)
    }
    pub fn arguments(&self, until: Instant) -> io::Result<Vec<Vec<u8>>> {
        self.current(until)?;
        Ok(self.arguments.clone())
    }
}

fn image_guard(image: &Image) -> io::Result<String> {
    crate::tasks::controls::wire::canonical::tagged_digest(
        b"zerus-task-control-image-linux-v1\0",
        &serde_json::json!({"schema":1,"device":image.identity.device,
            "inode":image.identity.inode,"bytes":image.bytes,
            "changed":image.changed,"modified":image.modified}),
    )
    .map_err(io::Error::other)
}

#[cfg(test)]
mod tests;
