//! Socket audit-token and kernel loaded-code proof, never a bare PID/path.
//! The CLI must execute this service's actual trusted hgs image. A mismatch
//! refuses until the owning computer's CLI/service are updated together.
use std::{
    io,
    os::{fd::AsRawFd, unix::net::UnixStream},
    path::{Path, PathBuf},
    time::Instant,
};

const ANCESTORS: usize = 64;
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
unsafe extern "C" {
    fn csops_audittoken(
        pid: libc::pid_t,
        operation: u32,
        address: *mut libc::c_void,
        size: usize,
        token: *const Audit,
    ) -> i32;
    static mach_task_self_: u32;
}
#[repr(C)]
struct Unique {
    image: [u8; 16],
    birth: u64,
    parent_birth: u64,
    version: i32,
    parent_version: i32,
    reserved: [u64; 2],
}
const _: () = assert!(std::mem::size_of::<Audit>() == 32 && std::mem::size_of::<Unique>() == 56);

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
fn audit(socket: &UnixStream) -> io::Result<Audit> {
    let mut token = Audit { words: [0; 8] };
    let mut size = std::mem::size_of_val(&token) as libc::socklen_t;
    // SAFETY: a live socket and exactly sized writable audit token.
    if unsafe {
        libc::getsockopt(
            socket.as_raw_fd(),
            libc::SOL_LOCAL,
            libc::LOCAL_PEERTOKEN,
            (&mut token as *mut Audit).cast(),
            &mut size,
        )
    } != 0
        || size as usize != std::mem::size_of_val(&token)
    {
        return Err(refused());
    }
    validate_token(token)?;
    Ok(token)
}
fn validate_token(token: Audit) -> io::Result<u32> {
    let pid = unsafe { audit_token_to_pid(token) };
    if pid <= 1
        || unsafe { audit_token_to_pidversion(token) } == 0
        || unsafe { audit_token_to_euid(token) } != unsafe { libc::geteuid() }
        || unsafe { audit_token_to_ruid(token) } != unsafe { libc::getuid() }
    {
        return Err(refused());
    }
    Ok(pid as u32)
}
fn own_audit() -> io::Result<Audit> {
    let mut token = Audit { words: [0; 8] };
    let mut count = 8;
    // SAFETY: TASK_AUDIT_TOKEN and its exact eight-word output storage.
    if unsafe {
        libc::task_info(
            mach_task_self_,
            15,
            (&mut token as *mut Audit).cast(),
            &mut count,
        )
    } != 0
        || count != 8
    {
        return Err(refused());
    }
    if validate_token(token)? != std::process::id() {
        return Err(refused());
    }
    Ok(token)
}

/// Kernel-reported CodeDirectory hash for the actual loaded process, bound to
/// the socket token's pidversion. No executable-on-disk hash substitutes for it.
fn loaded(token: Audit) -> io::Result<[u8; 20]> {
    let pid = validate_token(token)? as libc::pid_t;
    let mut status: u32 = 0;
    let mut hash = [0u8; 20];
    // SAFETY: read-only CS_OPS_STATUS/CDHASH, exact fixed buffers; the native
    // audit-token variant verifies original process/exec identity in-kernel.
    if unsafe { csops_audittoken(pid, 0, (&mut status as *mut u32).cast(), 4, &token) } != 0
        || status & 1 == 0
        || unsafe { csops_audittoken(pid, 5, hash.as_mut_ptr().cast(), hash.len(), &token) } != 0
        || hash == [0; 20]
    {
        return Err(refused());
    }
    Ok(hash)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Process {
    pid: u32,
    parent: u32,
    start: (u64, u64),
    unique: u64,
    parent_unique: u64,
    version: i32,
}
impl Process {
    fn query(pid: u32, until: Instant) -> io::Result<Self> {
        budget(until)?;
        let mut bsd: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let mut unique: Unique = unsafe { std::mem::zeroed() };
        // SAFETY: fixed proc flavors and exactly sized writable native structs.
        if unsafe {
            libc::proc_pidinfo(
                pid as i32,
                libc::PROC_PIDTBSDINFO,
                0,
                (&mut bsd as *mut libc::proc_bsdinfo).cast(),
                std::mem::size_of_val(&bsd) as i32,
            )
        } != std::mem::size_of_val(&bsd) as i32
            || unsafe {
                libc::proc_pidinfo(pid as i32, 17, 0, (&mut unique as *mut Unique).cast(), 56)
            } != 56
            || bsd.pbi_pid != pid
            || bsd.pbi_uid != unsafe { libc::geteuid() }
            || bsd.pbi_ruid != unsafe { libc::getuid() }
            || matches!(bsd.pbi_status, libc::SZOMB | libc::SSTOP)
            || unique.birth == 0
            || unique.version == 0
        {
            return Err(refused());
        }
        budget(until)?;
        Ok(Self {
            pid,
            parent: bsd.pbi_ppid,
            start: (bsd.pbi_start_tvsec, bsd.pbi_start_tvusec),
            unique: unique.birth,
            parent_unique: unique.parent_birth,
            version: unique.version,
        })
    }
    fn current(&self, until: Instant) -> io::Result<()> {
        if Self::query(self.pid, until)? != *self {
            Err(refused())
        } else {
            Ok(())
        }
    }
    fn recorded_start(&self, until: Instant) -> io::Result<String> {
        self.current(until)?;
        let value = crate::platform::process_start_time(self.pid)
            .filter(|s| !s.is_empty())
            .ok_or_else(refused)?;
        self.current(until)?;
        Ok(value)
    }
    fn birth(&self) -> String {
        format!("{}:{}", self.start.0, self.start.1)
    }
}

pub(super) struct Peer {
    token: Audit,
    process: Process,
    code: [u8; 20],
    program: PathBuf,
    arguments: Vec<Vec<u8>>,
    ancestry: Vec<Process>,
}
impl Peer {
    pub fn arguments(&self, until: Instant) -> io::Result<Vec<Vec<u8>>> {
        self.current(until)?;
        Ok(self.arguments.clone())
    }
    /// Program comes from the service's selected CLI, not request JSON. Both
    /// peer and service loaded hashes must agree; older mixed images refuse.
    pub fn capture(socket: &UnixStream, program: &Path, until: Instant) -> io::Result<Self> {
        budget(until)?;
        let token = audit(socket)?;
        let process = Process::query(validate_token(token)?, until)?;
        if process.version != unsafe { audit_token_to_pidversion(token) } {
            return Err(refused());
        }
        let code = loaded(token)?;
        if code != loaded(own_audit()?)? {
            return Err(refused());
        }
        let program = std::fs::canonicalize(program)?;
        if image_path(process.pid)? != program {
            return Err(refused());
        }
        let arguments = command(process.pid)?;
        let peer = Self {
            token,
            process,
            code,
            program,
            arguments,
            ancestry: vec![],
        };
        peer.current(until)?;
        Ok(peer)
    }
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
    pub fn executor(&mut self, pid: u32, birth: &str, until: Instant) -> io::Result<()> {
        self.current(until)?;
        if !self.ancestry.is_empty() {
            return Err(refused());
        }
        let mut child = self.process.clone();
        let mut seen = std::collections::BTreeSet::from([child.pid]);
        for _ in 0..ANCESTORS {
            if child.pid == pid {
                return if self
                    .ancestry
                    .last()
                    .unwrap_or(&self.process)
                    .recorded_start(until)?
                    == birth
                {
                    self.current(until)
                } else {
                    Err(refused())
                };
            }
            if child.parent <= 1 || !seen.insert(child.parent) {
                return Err(refused());
            }
            let parent = Process::query(child.parent, until)?;
            if parent.start > child.start || parent.unique != child.parent_unique {
                return Err(refused());
            }
            child = parent.clone();
            self.ancestry.push(parent);
        }
        Err(refused())
    }
    pub fn current(&self, until: Instant) -> io::Result<()> {
        self.process.current(until)?;
        if self.process.version != unsafe { audit_token_to_pidversion(self.token) }
            || loaded(self.token)? != self.code
            || loaded(own_audit()?)? != self.code
            || image_path(self.process.pid)? != self.program
            || command(self.process.pid)? != self.arguments
        {
            return Err(refused());
        }
        let mut child = &self.process;
        for parent in &self.ancestry {
            parent.current(until)?;
            if child.parent != parent.pid
                || child.parent_unique != parent.unique
                || parent.start > child.start
            {
                return Err(refused());
            }
            child = parent;
        }
        self.process.current(until)
    }
}

fn image_path(pid: u32) -> io::Result<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let mut path = [0u8; 4096];
    // SAFETY: exactly bounded writable path; proc_pidpath never grants a handle.
    let size =
        unsafe { libc::proc_pidpath(pid as i32, path.as_mut_ptr().cast(), path.len() as u32) };
    if size <= 0 {
        return Err(refused());
    }
    let end = path.iter().position(|b| *b == 0).ok_or_else(refused)?;
    Ok(PathBuf::from(std::ffi::OsStr::from_bytes(&path[..end])))
}
fn command(pid: u32) -> io::Result<Vec<Vec<u8>>> {
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid as i32];
    let mut bytes = vec![0u8; super::args_macos::BYTES];
    let mut size = bytes.len();
    // SAFETY: read-only bounded procargs buffer, never logged or retained after
    // extracting argv. Environment bytes are not actor evidence and are dropped.
    if unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            bytes.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    } != 0
        || size < 5
        || size > bytes.len()
    {
        return Err(refused());
    }
    bytes.truncate(size);
    super::args_macos::decode(&bytes).ok_or_else(refused)
}

/// Native unique-ID/version pins plus kernel loaded-code checks, independent
/// of argv/environment labels. Real Mac qualification remains a prerequisite.
pub(super) struct Resident {
    agent: Process,
    supervisor: Process,
    token: Audit,
    code: [u8; 20],
    program: PathBuf,
    arguments: Vec<Vec<u8>>,
}
impl Resident {
    pub fn capture(
        agent: u32,
        agent_birth: &str,
        supervisor: u32,
        supervisor_start: &str,
        supervisor_birth: &str,
        program: &Path,
        until: Instant,
    ) -> io::Result<Self> {
        if agent == supervisor {
            return Err(refused());
        }
        let agent = Process::query(agent, until)?;
        let supervisor = Process::query(supervisor, until)?;
        if agent.recorded_start(until)? != agent_birth
            || supervisor.recorded_start(until)? != supervisor_start
            || supervisor.birth() != supervisor_birth
            || agent.parent != supervisor.pid
            || agent.parent_unique != supervisor.unique
            || supervisor.start > agent.start
        {
            return Err(refused());
        }
        // These two fields come from the native process query, never a caller
        // token. XNU csops_internal checks val[5]/val[7] under its process ref.
        // The socket caller still uses its original kernel-supplied token.
        let mut token = own_audit()?;
        token.words[5] = supervisor.pid;
        token.words[7] = supervisor.version as u32;
        let code = loaded(token)?;
        if code != loaded(own_audit()?)? {
            return Err(refused());
        }
        let program = std::fs::canonicalize(program)?;
        if image_path(supervisor.pid)? != program {
            return Err(refused());
        }
        let arguments = command(supervisor.pid)?;
        let resident = Self {
            agent,
            supervisor,
            token,
            code,
            program,
            arguments,
        };
        resident.current(until)?;
        Ok(resident)
    }
    pub fn current(&self, until: Instant) -> io::Result<()> {
        self.agent.current(until)?;
        self.supervisor.current(until)?;
        if loaded(self.token)? != self.code
            || loaded(own_audit()?)? != self.code
            || image_path(self.supervisor.pid)? != self.program
            || command(self.supervisor.pid)? != self.arguments
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
