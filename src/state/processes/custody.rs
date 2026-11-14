//! Dormant Linux ownership; no signal, grace, archive or cleanup authority.
//! Only a fresh dedicated single-threaded executable may use this resource.
//! Its trusted caller must remain this process's sole spawner/waiter, create
//! no threads and install no handlers. No attach/adopt/restore constructor.
//! The original-supervisor adapter remains const-false; its records refuse the
//! legacy census, which misses adopted siblings. Generic completion is never
//! source attestation. Drop cannot reconstruct forfeited ownership.
use std::{
    collections::BTreeMap,
    fs::OpenOptions,
    io::{self, Read},
    marker::PhantomData,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::fs::OpenOptionsExt,
    },
    process::{Child, Command},
    rc::Rc,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
const IDENTITIES: usize = 4096;
const EVENTS_PER_POLL: usize = 64;
static USED: AtomicBool = AtomicBool::new(false);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Failure {
    Policy,
    Identity,
    Wait,
    Limit,
    Deadline,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Snapshot {
    Pending,
    Unresolved(Failure),
    /// Diagnostic data: exit code, or negative terminating signal. This is
    /// neither a transferable custody capability nor cleanup permission.
    Complete {
        root: i32,
        adopted: usize,
    },
}
struct Pin {
    pid: u32,
    birth: u64,
    handle: OwnedFd,
}
impl Pin {
    fn new(pid: u32) -> io::Result<Self> {
        let (birth, _) = identity(pid)?;
        // SAFETY: pidfd_open returns a new owned descriptor or -1.
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let pin = Self {
            pid,
            birth,
            handle: unsafe { OwnedFd::from_raw_fd(fd as i32) },
        };
        pin.check()?;
        Ok(pin)
    }
    fn check(&self) -> io::Result<u32> {
        let (birth, parent) = identity(self.pid)?;
        if birth != self.birth {
            return Err(io::Error::other("native birth changed"));
        }
        Ok(parent)
    }
}
/// Not Send/Sync/Clone/serializable; only its original live dedicated owner.
pub struct Custody {
    root: Child,
    owner: Pin,
    children: BTreeMap<u32, Pin>,
    seen: usize,
    adopted: usize,
    root_exit: Option<i32>,
    root_pin: Option<Pin>,
    failure: Option<Failure>,
    complete: bool,
    until: Option<Instant>,
    _local: PhantomData<Rc<()>>,
}
impl Custody {
    /// Host-verified direct executable only; no custom pre_exec. Spawn itself
    /// is not a physically bounded helper/IO adapter. Production launch/source
    /// admission is separate. The caller may spawn/wait on NO other children.
    pub fn spawn(command: &mut Command, until: Instant) -> io::Result<Self> {
        let now = Instant::now();
        if until <= now
            || until.duration_since(now) > Duration::from_secs(30)
            || !policy()
            || !single_thread()?
            || subreaper()? != 0
            || !children()?.is_empty()
            || USED.swap(true, Ordering::AcqRel)
        {
            return Err(io::Error::other("dedicated fresh owner unavailable"));
        }
        let owner = Pin::new(std::process::id())?;
        // SAFETY: this dedicated owner has no child yet.
        if unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) } != 0 {
            return Err(io::Error::last_os_error());
        }
        if subreaper()? != 1 {
            return Err(io::Error::other("subreaper unavailable"));
        }
        let root = command.spawn()?;
        let mut children = BTreeMap::new();
        let failure = match Pin::new(root.id()) {
            Ok(pin) => {
                children.insert(root.id(), pin);
                None
            }
            Err(_) => Some(Failure::Identity),
        };
        Ok(Self {
            root,
            owner,
            children,
            seen: 1,
            adopted: 0,
            root_exit: None,
            root_pin: None,
            failure,
            complete: false,
            until: Some(until),
            _local: PhantomData,
        })
    }
    pub fn poll(&mut self) -> Snapshot {
        if let Some(failure) = self.failure {
            return Snapshot::Unresolved(failure);
        }
        if let Err(failure) = self.check().and_then(|()| {
            if self.complete {
                Ok(())
            } else {
                self.observe()
            }
        }) {
            self.failure = Some(failure);
        }
        self.snapshot()
    }
    fn snapshot(&self) -> Snapshot {
        if let Some(failure) = self.failure {
            Snapshot::Unresolved(failure)
        } else if self.complete {
            Snapshot::Complete {
                root: self.root_exit.expect("positive root wait"),
                adopted: self.adopted,
            }
        } else {
            Snapshot::Pending
        }
    }
    fn expired(&self) -> bool {
        self.until.is_some_and(|until| Instant::now() >= until)
    }
    fn check(&self) -> Result<(), Failure> {
        if self.expired() {
            return Err(Failure::Deadline);
        }
        if !policy()
            || subreaper().ok() != Some(1)
            || (self.until.is_some() && single_thread().ok() != Some(true))
        {
            return Err(Failure::Policy);
        }
        if self.owner.pid != std::process::id() || self.owner.check().is_err() {
            return Err(Failure::Identity);
        }
        Ok(())
    }
    fn remember(&mut self, pid: u32) -> Result<(), Failure> {
        if !self.children.contains_key(&pid) {
            if self.expired() {
                return Err(Failure::Deadline);
            }
            if (if self.until.is_some() {
                self.seen
            } else {
                self.children.len() + usize::from(self.root_pin.is_some())
            }) >= IDENTITIES
            {
                return Err(Failure::Limit);
            }
            let pin = Pin::new(pid).map_err(|_| Failure::Identity)?;
            if pin.check().ok() != Some(self.owner.pid) {
                return Err(Failure::Identity);
            }
            self.children.insert(pid, pin);
            self.seen = self.seen.saturating_add(1);
        }
        Ok(())
    }
    fn observe(&mut self) -> Result<(), Failure> {
        self.check()?;
        for pid in children().map_err(|_| Failure::Limit)? {
            self.remember(pid)?;
        }
        // A missing pinned child without OUR positive wait is lost custody.
        for pin in self.children.values() {
            if self.expired() {
                return Err(Failure::Deadline);
            }
            if pin.check().ok() != Some(self.owner.pid) {
                return Err(Failure::Wait);
            }
        }
        for _ in 0..EVENTS_PER_POLL {
            self.check()?;
            let info = match wait(libc::P_ALL, 0, libc::WNOWAIT) {
                Ok(Some(info)) => info,
                Ok(None) => return Ok(()), // si_pid==0 means live children!
                Err(e) if e.raw_os_error() == Some(libc::ECHILD) => {
                    if self.root_exit.is_none() || !self.children.is_empty() {
                        return Err(Failure::Wait);
                    }
                    self.check()?;
                    self.complete = true;
                    return Ok(());
                }
                Err(_) => return Err(Failure::Wait),
            };
            let pid = unsafe { info.si_pid() } as u32;
            self.remember(pid)?;
            let pin = &self.children[&pid];
            if pin.check().ok() != Some(self.owner.pid) {
                return Err(Failure::Identity);
            }
            let status = exit(&info).ok_or(Failure::Wait)?;
            // Same retained pidfd, no PID adoption or blocking wait.
            let reaped = wait(libc::P_PIDFD, pin.handle.as_raw_fd() as u32, 0)
                .map_err(|_| Failure::Wait)?
                .ok_or(Failure::Wait)?;
            if unsafe { reaped.si_pid() } as u32 != pid || exit(&reaped) != Some(status) {
                return Err(Failure::Wait);
            }
            let pin = self.children.remove(&pid);
            if pid == self.root.id() && self.root_exit.is_none() {
                self.root_exit = Some(status);
                self.root_pin = pin; // Retain the original handle after reaping.
            } else {
                self.adopted = self.adopted.saturating_add(1);
            }
        }
        Ok(()) // Fairness cap, not overflow: another bounded poll must drain.
    }
    // Only the real supervisor adapter uses an unexpired original lifetime.
    // This is kernel ownership DATA, never source/continuity attestation.
    pub(crate) fn prepare_original() -> io::Result<Prepared> {
        if !policy()
            || !single_thread()?
            || subreaper()? != 0
            || !children()?.is_empty()
            || USED.swap(true, Ordering::AcqRel)
        {
            return Err(io::Error::other("fresh original supervisor unavailable"));
        }
        let owner = Pin::new(std::process::id())?;
        if unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) } != 0
            || subreaper()? != 1
        {
            return Err(io::Error::other("original subreaper unavailable"));
        }
        Ok(Prepared { owner })
    }
    /// Non-consuming original-root status for the sealed startup observer.
    /// The host clears its legacy signal registration BEFORE consumption.
    pub(crate) fn ready_root(&mut self) -> io::Result<bool> {
        if self.root_exit.is_some() {
            return Ok(true);
        }
        self.check()
            .map_err(|_| io::Error::other("original policy changed"))?;
        let pin = self
            .children
            .get(&self.root.id())
            .ok_or_else(|| io::Error::other("lost root pin"))?;
        pin.check()?;
        Ok(wait(libc::P_PIDFD, pin.handle.as_raw_fd() as u32, libc::WNOWAIT)?.is_some())
    }
    pub(crate) fn consume_root(&mut self) -> io::Result<Option<i32>> {
        if self.root_exit.is_some() {
            return Ok(self.root_exit);
        }
        if !self.ready_root()? {
            return Ok(None);
        }
        let pin = self
            .children
            .get(&self.root.id())
            .ok_or_else(|| io::Error::other("lost root pin"))?;
        let info = wait(libc::P_PIDFD, pin.handle.as_raw_fd() as u32, 0)?
            .ok_or_else(|| io::Error::other("original root wait lost"))?;
        if unsafe { info.si_pid() } as u32 != self.root.id() {
            self.failure.get_or_insert(Failure::Wait);
            return Err(io::Error::other("original root identity changed"));
        }
        self.root_exit = exit(&info);
        self.root_pin = self.children.remove(&self.root.id());
        self.root_exit
            .map(Some)
            .ok_or_else(|| io::Error::other("invalid root status"))
    }
    pub fn root_id(&self) -> u32 {
        self.root.id()
    }
    pub fn root_exit(&self) -> Option<i32> {
        self.root_exit
    }
    #[cfg(test)]
    pub fn exhaust_test_capacity(&mut self) {
        self.seen = IDENTITIES;
    }
    #[cfg(test)]
    pub fn lose_test_owner(&mut self) {
        self.owner.birth = self.owner.birth.wrapping_add(1);
    }
    #[cfg(test)]
    pub fn lose_test_root(&mut self) {
        self.children.get_mut(&self.root.id()).unwrap().birth += 1;
    }
    #[cfg(test)]
    pub fn test_exit_count(&self) -> usize {
        self.adopted
    }
}
/// Pre-spawn one-use preparation, never recoverable from records or PIDs.
/// Kept crate-private; only the reviewed supervisor path supplies its Child.
pub(crate) struct Prepared {
    owner: Pin,
}
impl Prepared {
    pub(crate) fn attach(self, root: Child) -> Custody {
        let mut children = BTreeMap::new();
        let failure = match Pin::new(root.id()) {
            Ok(pin) if pin.check().ok() == Some(self.owner.pid) => {
                children.insert(root.id(), pin);
                None
            }
            _ => Some(Failure::Identity),
        };
        Custody {
            root,
            owner: self.owner,
            children,
            seen: 1,
            adopted: 0,
            root_exit: None,
            root_pin: None,
            failure,
            complete: false,
            until: None,
            _local: PhantomData,
        }
    }
}

fn wait(kind: libc::idtype_t, id: u32, extra: i32) -> io::Result<Option<libc::siginfo_t>> {
    let mut info = unsafe { std::mem::zeroed::<libc::siginfo_t>() };
    // Include non-SIGCHLD clone children. WNOWAIT reserves status while pinning.
    let result = unsafe {
        libc::waitid(
            kind,
            id,
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::__WALL | extra,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((unsafe { info.si_pid() } != 0).then_some(info))
}
fn exit(info: &libc::siginfo_t) -> Option<i32> {
    let status = unsafe { info.si_status() };
    match info.si_code {
        libc::CLD_EXITED if (0..=255).contains(&status) => Some(status),
        libc::CLD_KILLED | libc::CLD_DUMPED if (1..=127).contains(&status) => Some(-status),
        _ => None,
    }
}
fn subreaper() -> io::Result<i32> {
    let mut value = 0i32;
    if unsafe { libc::prctl(libc::PR_GET_CHILD_SUBREAPER, &mut value, 0, 0, 0) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(value)
}
fn policy() -> bool {
    let mut p = unsafe { std::mem::zeroed::<libc::sigaction>() };
    let queried = unsafe { libc::sigaction(libc::SIGCHLD, std::ptr::null(), &mut p) == 0 };
    queried && p.sa_sigaction == libc::SIG_DFL && p.sa_flags & libc::SA_NOCLDWAIT == 0
}
fn read(path: &str, cap: u64) -> io::Result<Vec<u8>> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("nonregular proc metadata"));
    }
    let mut data = Vec::new();
    file.take(cap + 1).read_to_end(&mut data)?;
    if data.len() as u64 > cap {
        return Err(io::Error::other("proc metadata limit"));
    }
    Ok(data)
}
fn identity(pid: u32) -> io::Result<(u64, u32)> {
    let raw = read(&format!("/proc/{pid}/stat"), 4096)?;
    let text = std::str::from_utf8(&raw).map_err(io::Error::other)?;
    let (_, fields) = text
        .rsplit_once(") ")
        .ok_or_else(|| io::Error::other("proc identity"))?;
    let fields: Vec<_> = fields.split_ascii_whitespace().take(20).collect();
    let parent = fields.get(1).and_then(|v| v.parse().ok());
    let birth = fields.get(19).and_then(|v| v.parse().ok());
    birth
        .zip(parent)
        .ok_or_else(|| io::Error::other("proc birth"))
}
fn single_thread() -> io::Result<bool> {
    Ok(std::fs::read_dir("/proc/self/task")?
        .take(2)
        .collect::<io::Result<Vec<_>>>()?
        .len()
        == 1)
}
fn children() -> io::Result<Vec<u32>> {
    let raw = read(
        &format!("/proc/self/task/{}/children", std::process::id()),
        64 << 10,
    )?;
    let text = std::str::from_utf8(&raw).map_err(io::Error::other)?;
    let mut children = Vec::new();
    for pid in text.split_ascii_whitespace() {
        if children.len() >= IDENTITIES {
            return Err(io::Error::other("child limit"));
        }
        children.push(pid.parse().map_err(io::Error::other)?);
    }
    Ok(children)
}
