//! One client per user, by Qt's protocol (`tray/src/SingleInstance.{h,cpp}`,
//! `tray/src/main.cpp:337-361` and `:424-433`). Concurrent starts take a
//! per-user lock in turn; the running instance listens on a Unix socket and
//! reads one line, `sessions` or `show`; a start that reaches it forwards its
//! line and exits. The switch bundle uses Qt's names, so
//! `tray/update-installed.py` finds the socket (a path ending in `/hgs-tray`)
//! and sends `sessions` to the new build.
use std::ffi::OsString;
use std::fs::{self, DirBuilder, File, OpenOptions, Permissions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
#[cfg(not(feature = "live"))]
use std::path::Component;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use crate::identity::Platform;

#[path = "instance_guard.rs"]
mod canonical;

/// The line a start sends to the running instance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Activation {
    Show,
    Sessions,
    /// Public UI navigation only; never carries a callback token or permission.
    #[cfg(all(feature = "live", target_os = "linux"))]
    Navigation(zerus_client_core::desktop::activation::Target),
}

impl Activation {
    pub fn line(&self) -> Vec<u8> {
        match self {
            Self::Show => b"show\n".to_vec(),
            Self::Sessions => b"sessions\n".to_vec(),
            #[cfg(all(feature = "live", target_os = "linux"))]
            Self::Navigation(target) => format!(
                "target {}\n",
                match zerus_client_core::desktop::activation::encode_navigation(target) {
                    Some(encoded) => encoded,
                    None => return Vec::new(),
                }
            )
            .into_bytes(),
        }
    }

    /// Qt reads one line and trims it (`main.cpp:428`): `QByteArray::trimmed`
    /// drops the six ASCII blanks, vertical tab included.
    pub fn parse(line: &[u8]) -> Option<Self> {
        let blank = |byte: &u8| matches!(byte, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r');
        let start = line
            .iter()
            .position(|byte| !blank(byte))
            .unwrap_or(line.len());
        let end = line
            .iter()
            .rposition(|byte| !blank(byte))
            .map_or(start, |last| last + 1);
        match &line[start..end] {
            b"sessions" => Some(Self::Sessions),
            b"show" => Some(Self::Show),
            _ => {
                #[cfg(all(feature = "live", target_os = "linux"))]
                {
                    std::str::from_utf8(&line[start..end])
                        .ok()
                        .and_then(|line| line.strip_prefix("target "))
                        .and_then(zerus_client_core::desktop::activation::decode_navigation)
                        .map(Self::Navigation)
                }
                #[cfg(not(all(feature = "live", target_os = "linux")))]
                {
                    None
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paths {
    pub socket: PathBuf,
    pub lock: PathBuf,
}

/// QLocalServer puts a relative name in `QDir::tempPath()`; the start lock is
/// `QStandardPaths::RuntimeLocation` + `/<name>-start.lock` (`SingleInstance.cpp:143-146`).
pub fn paths(name: &str, temp: &Path, runtime: &Path) -> Paths {
    Paths {
        socket: temp.join(name),
        lock: runtime.join(format!("{name}-start.lock")),
    }
}

/// `QDir::tempPath()`: a non-empty `TMPDIR`, else the per-user `confstr`
/// folder on macOS (`NSTemporaryDirectory`) and `/tmp` elsewhere. Qt cleans
/// the path only lexically and keeps symbolic links, so the socket is
/// `<TMPDIR as given>/<name>`, where Qt's own QLocalSocket connects; no
/// `/private` is added toward Darwin's 104-byte socket path limit.
pub fn temp_dir() -> PathBuf {
    temp_dir_from(std::env::var_os("TMPDIR"))
}

#[cfg(feature = "live")]
fn temp_dir_from(tmpdir: Option<OsString>) -> PathBuf {
    zerus_client_core::desktop::temporary_dir_from(tmpdir)
}

#[cfg(not(feature = "live"))]
fn temp_dir_from(tmpdir: Option<OsString>) -> PathBuf {
    let temp = tmpdir
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(user_temp_dir)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    clean_path(&temp)
}

/// `QDir::cleanPath`: no repeated or trailing separators, `.` and `..`
/// resolved by name only; symbolic links stay as they are.
#[cfg(not(feature = "live"))]
fn clean_path(path: &Path) -> PathBuf {
    let mut parts: Vec<Component> = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match parts.last() {
                Some(Component::Normal(_)) => {
                    parts.pop();
                }
                Some(Component::RootDir) => {}
                _ => parts.push(component),
            },
            other => parts.push(other),
        }
    }
    parts.iter().collect()
}

#[cfg(all(not(feature = "live"), target_os = "macos"))]
fn user_temp_dir() -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let mut buffer = [0 as libc::c_char; 1024];
    // SAFETY: the buffer is writable for its length.
    let length = unsafe {
        libc::confstr(
            libc::_CS_DARWIN_USER_TEMP_DIR,
            buffer.as_mut_ptr(),
            buffer.len(),
        )
    };
    if length == 0 || length > buffer.len() {
        return None;
    }
    // SAFETY: confstr wrote a NUL-terminated string that fits the buffer.
    let text = unsafe { std::ffi::CStr::from_ptr(buffer.as_ptr()) };
    Some(PathBuf::from(std::ffi::OsStr::from_bytes(text.to_bytes())))
}

#[cfg(all(not(feature = "live"), not(target_os = "macos")))]
fn user_temp_dir() -> Option<PathBuf> {
    None
}

/// `QStandardPaths::RuntimeLocation`: `~/Library/Application Support` on
/// macOS; on Linux a non-empty `XDG_RUNTIME_DIR`, else Qt's
/// `<temp>/runtime-<user>`.
pub fn runtime_dir(platform: Platform, home: &Path, xdg_runtime: Option<&Path>) -> PathBuf {
    match platform {
        Platform::Mac => home.join("Library/Application Support"),
        _ => xdg_runtime
            .filter(|path| !path.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| temp_dir().join(format!("runtime-{}", user_name()))),
    }
}

/// This user's account name (`QFileSystemEngine::resolveUserName`), else the UID.
fn user_name() -> String {
    // SAFETY: geteuid cannot fail.
    let uid = unsafe { libc::geteuid() };
    let mut buffer = vec![0 as libc::c_char; 16 * 1024];
    // SAFETY: a zeroed passwd is a valid out-parameter; getpwuid_r fills it
    // with pointers into the buffer, which outlives their use below.
    let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
    let mut found: *mut libc::passwd = std::ptr::null_mut();
    let status = unsafe {
        libc::getpwuid_r(
            uid,
            &mut entry,
            buffer.as_mut_ptr(),
            buffer.len(),
            &mut found,
        )
    };
    if status == 0 && !found.is_null() && !entry.pw_name.is_null() {
        // SAFETY: pw_name points at a NUL-terminated string inside the buffer.
        let name = unsafe { std::ffi::CStr::from_ptr(entry.pw_name) };
        if !name.to_bytes().is_empty() {
            return name.to_string_lossy().into_owned();
        }
    }
    uid.to_string()
}

/// QLockFile's first three lines (`qlockfile_unix.cpp`): PID, process name, host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockInfo {
    pub pid: i64,
    pub app: String,
    pub host: String,
}

impl LockInfo {
    pub fn contents(&self) -> String {
        format!("{}\n{}\n{}\n", self.pid, self.app, self.host)
    }

    pub fn parse(text: &str) -> Option<Self> {
        let mut lines = text.split('\n');
        let pid = lines
            .next()?
            .trim()
            .parse::<i64>()
            .ok()
            .filter(|pid| *pid > 0)?;
        Some(Self {
            pid,
            app: lines.next().unwrap_or("").to_owned(),
            host: lines.next().unwrap_or("").to_owned(),
        })
    }

    /// This process, named as QLockFile names it.
    pub fn own() -> Self {
        let pid = i64::from(std::process::id());
        Self {
            pid,
            app: process_name(pid)
                .flatten()
                .unwrap_or_else(|| "zerus-client".to_owned()),
            host: host_name(),
        }
    }
}

/// QLockFile's default stale time.
pub const STALE_AFTER: Duration = Duration::from_secs(30);

/// QLockFile's `isApparentlyStale`: on this host, a holder that has exited
/// or whose PID now runs another program; on any host, a file older than
/// 30 s. `process` answers None for no such process, `Some(None)` for a
/// process whose name is unknown and `Some(Some(name))` otherwise.
pub fn is_stale(
    info: Option<&LockInfo>,
    age: Duration,
    host: &str,
    process: impl Fn(i64) -> Option<Option<String>>,
) -> bool {
    if let Some(info) = info {
        if info.host.is_empty() || info.host == host {
            let running = match process(info.pid) {
                None => false,
                Some(Some(name)) if !name.is_empty() => Path::new(&info.app)
                    .file_name()
                    .is_some_and(|app| app == name.as_str()),
                Some(_) => true,
            };
            if !running {
                return true;
            }
        }
    }
    age > STALE_AFTER
}

/// None when no process has this PID; its name when the system tells it:
/// `proc_name` on macOS, the executable's file name on Linux, as QLockFile's
/// `processNameByPid` reads them. On Linux a process whose executable link
/// cannot be read counts as gone, as in Qt.
pub fn process_name(pid: i64) -> Option<Option<String>> {
    let raw = libc::pid_t::try_from(pid).ok()?;
    // SAFETY: signal 0 only checks that the process exists.
    if unsafe { libc::kill(raw, 0) } == -1
        && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
    {
        return None;
    }
    running_name(raw)
}

#[cfg(target_os = "macos")]
fn running_name(pid: libc::pid_t) -> Option<Option<String>> {
    let mut buffer = [0u8; 1024];
    // SAFETY: the buffer is writable for its length.
    let length = unsafe { libc::proc_name(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
    Some((length > 0).then(|| String::from_utf8_lossy(&buffer[..length as usize]).into_owned()))
}

#[cfg(not(target_os = "macos"))]
fn running_name(pid: libc::pid_t) -> Option<Option<String>> {
    exe_name(fs::read_link(format!("/proc/{pid}/exe")))
}

/// QLockFile's Linux `processNameByPid`: the file name of `/proc/<pid>/exe`
/// without the ` (deleted)` that marks a replaced binary. None when the
/// link cannot be read.
#[cfg(not(target_os = "macos"))]
fn exe_name(link: io::Result<PathBuf>) -> Option<Option<String>> {
    let link = link.ok()?;
    let text = link.to_string_lossy();
    let path = Path::new(text.strip_suffix(" (deleted)").unwrap_or(&text));
    Some(
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned()),
    )
}

/// `QSysInfo::machineHostName()`.
pub fn host_name() -> String {
    let mut buffer = [0u8; 512];
    // SAFETY: the buffer is writable for its length.
    if unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len()) } != 0 {
        return String::new();
    }
    let end = buffer
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(buffer.len());
    String::from_utf8_lossy(&buffer[..end]).into_owned()
}

/// A taken start lock; dropping it removes the file, as `QLockFile::unlock` does.
pub struct StartLock {
    path: PathBuf,
    _file: File,
}

impl Drop for StartLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub enum LockAttempt {
    Taken(StartLock),
    Busy,
    Unavailable,
}

/// The file's age; QLockFile counts a modification time in the future too.
fn age(path: &Path) -> Duration {
    let Ok(modified) = fs::metadata(path).and_then(|metadata| metadata.modified()) else {
        return Duration::ZERO;
    };
    let now = SystemTime::now();
    now.duration_since(modified)
        .or_else(|_| modified.duration_since(now))
        .unwrap_or_default()
}

fn stale_now(path: &Path, host: &str) -> bool {
    let info = fs::read_to_string(path)
        .ok()
        .and_then(|text| LockInfo::parse(&text));
    is_stale(info.as_ref(), age(path), host, process_name)
}

/// Takes QLockFile's advisory lock; false when a live holder keeps it.
fn flock_now(file: &File) -> bool {
    // SAFETY: an advisory lock on an open descriptor.
    unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) == 0 }
}

/// QLockFile guards a removal with `<lock>.rmlock`, itself a lock file taken
/// by the same rules, and removes only a file that no live holder keeps locked.
fn remove_stale(path: &Path, own: &LockInfo) -> bool {
    let mut guard = OsString::from(path.as_os_str());
    guard.push(".rmlock");
    let LockAttempt::Taken(_guard) = try_lock(Path::new(&guard), own) else {
        return false;
    };
    stale_now(path, &own.host)
        && OpenOptions::new()
            .write(true)
            .open(path)
            .is_ok_and(|file| flock_now(&file))
        && fs::remove_file(path).is_ok()
}

/// One attempt to take the lock, removing a stale one first (`QLockFile::tryLock(0)`).
pub fn try_lock(path: &Path, own: &LockInfo) -> LockAttempt {
    for _ in 0..2 {
        match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o666)
            .open(path)
        {
            Ok(mut file) => {
                // Held while the file exists, so a remover can tell a live
                // holder from a crashed one (QLockFile's native lock).
                let _ = flock_now(&file);
                if file.write_all(own.contents().as_bytes()).is_err() {
                    let _ = fs::remove_file(path);
                    return LockAttempt::Unavailable;
                }
                let _ = file.sync_data();
                return LockAttempt::Taken(StartLock {
                    path: path.to_owned(),
                    _file: file,
                });
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                if !(stale_now(path, &own.host) && remove_stale(path, own)) {
                    return LockAttempt::Busy;
                }
            }
            Err(_) => return LockAttempt::Unavailable,
        }
    }
    LockAttempt::Busy
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ServiceState {
    #[default]
    Absent,
    Starting,
    Running,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Service {
    pub state: ServiceState,
    pub pid: i64,
    /// systemd's InvocationID of the unit's current run, when it has one.
    pub invocation: Option<u128>,
}

/// The login service's systemd unit on Linux (`tray/packaging/hgs-tray.service`).
pub const SYSTEMD_UNIT: &str = "hgs-tray.service";

/// A systemd invocation ID: 32 hex digits, as `INVOCATION_ID` and
/// `systemctl show` print it (dashes, if any, are ignored).
pub fn invocation_id(text: &str) -> Option<u128> {
    let digits: String = text.trim().chars().filter(|c| *c != '-').collect();
    (digits.len() == 32 && digits.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| u128::from_str_radix(&digits, 16).ok())
        .flatten()
        .filter(|id| *id != 0)
}

/// `systemctl --user show --property=ActiveState,MainPID,InvocationID`, as
/// Qt's `SingleInstance::systemdService` reads it (`SingleInstance.cpp:48-71`).
#[cfg(any(target_os = "linux", test))]
pub fn systemd_service(properties: &str) -> Service {
    let (mut active, mut pid, mut invocation) = ("", 0_i64, None);
    for line in properties.lines() {
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        match name.trim() {
            "ActiveState" => active = value.trim(),
            "MainPID" => pid = value.trim().parse().unwrap_or(0),
            "InvocationID" => invocation = invocation_id(value),
            _ => {}
        }
    }
    let state = match active {
        "active" | "reloading" | "refreshing" => ServiceState::Running,
        "activating" | "deactivating" => ServiceState::Starting,
        _ => return Service::default(),
    };
    Service {
        state,
        pid: pid.max(0),
        invocation,
    }
}

/// `launchctl print gui/<uid>/<label>`: only the job's own properties, one
/// level inside its braces (`SingleInstance.cpp:74-105`).
pub fn launchd_service(description: &str) -> Service {
    let (mut state, mut pid, mut depth) = ("", 0_i64, 0_i32);
    for raw in description.lines() {
        let line = raw.trim();
        if line == "}" {
            depth -= 1;
            continue;
        }
        if depth == 1 {
            if let Some((name, value)) = line.split_once(" = ") {
                match name {
                    "state" => state = value,
                    "pid" => pid = value.trim().parse().unwrap_or(0),
                    _ => {}
                }
            }
        }
        if line.ends_with('{') {
            depth += 1;
        }
    }
    let state = match state {
        "running" => ServiceState::Running,
        "spawn scheduled" => ServiceState::Starting,
        _ => return Service::default(),
    };
    Service {
        state,
        pid: pid.max(0),
        invocation: None,
    }
}

/// A start that finds no instance leaves the socket to a service another
/// process runs or is about to run (`SingleInstance::waitsForService`). The
/// unit's own run never waits on itself: its main process, or any process of
/// that run (a wrapper's child) by its `INVOCATION_ID`.
pub fn waits_for_service(service: &Service, own_pid: i64, own_invocation: Option<u128>) -> bool {
    service.state != ServiceState::Absent
        && service.pid != own_pid
        && !(own_invocation.is_some() && service.invocation == own_invocation)
}

/// The service this start belongs to: launchd's job on macOS, the systemd
/// user unit on Linux; none for a build that is not the switch (no label).
pub fn platform_service(
    label: Option<&str>,
    launchd: impl FnOnce(&str) -> Service,
    systemd: impl FnOnce(&str) -> Service,
) -> Service {
    match label {
        Some(label) if cfg!(target_os = "macos") => launchd(label),
        Some(_) if cfg!(target_os = "linux") => systemd(SYSTEMD_UNIT),
        _ => Service::default(),
    }
}

/// The standard output of `command` when it exits successfully within
/// `limit`; otherwise None, with the child killed and reaped. Its standard
/// input is /dev/null, its error output is dropped, at most 64 KiB is read
/// and, on Linux, it inherits no descriptor beyond those three.
#[cfg(target_os = "linux")]
pub fn bounded_read(program: &str, args: &[&str], limit: Duration) -> Option<String> {
    bounded_read_with(program, args, limit, |_| {}, children_auto_reaped)
}

/// Whether a SIGCHLD disposition reaps children without a wait: ignored, or
/// SA_NOCLDWAIT (zerus-5zo1o).
#[cfg(target_os = "linux")]
fn auto_reaps(handler: libc::sighandler_t, flags: libc::c_int) -> bool {
    handler == libc::SIG_IGN || flags & libc::SA_NOCLDWAIT != 0
}

/// Whether this process's children are reaped without a wait. A bounded read
/// refuses then: once the kernel reaped its child, the child's PID may name
/// another process, which must never be signalled.
#[cfg(target_os = "linux")]
fn children_auto_reaped() -> bool {
    // SAFETY: a null new action only reads the current disposition into a
    // zeroed, properly sized sigaction.
    let mut current: libc::sigaction = unsafe { std::mem::zeroed() };
    if unsafe { libc::sigaction(libc::SIGCHLD, std::ptr::null(), &mut current) } != 0 {
        return true;
    }
    auto_reaps(current.sa_sigaction, current.sa_flags)
}

/// What [`settle`] does to a child, so tests can script it.
#[cfg(target_os = "linux")]
trait Owned {
    fn try_wait(&mut self) -> io::Result<Option<std::process::ExitStatus>>;
    fn kill(&mut self) -> io::Result<()>;
    fn wait(&mut self) -> io::Result<std::process::ExitStatus>;
}

/// A bounded read's child, held by a pidfd from the moment it starts. It is
/// waited for and signalled only through that descriptor, never by its
/// numeric PID: whatever SIGCHLD policy or other reaper retires it, a
/// cleanup reaches that process or nothing, never one that reused its PID,
/// and no wait collects another child by PID (zerus-5zo1o).
#[cfg(target_os = "linux")]
struct Helper {
    pidfd: std::os::fd::OwnedFd,
}

/// glibc's `pidfd_spawnp` (2.39) and `posix_spawn_file_actions_addclosefrom_np`
/// (2.34), looked up when used, so an older glibc still loads the app; the read
/// then waits for nothing.
#[cfg(target_os = "linux")]
type PidfdSpawnp = unsafe extern "C" fn(
    *mut libc::c_int,
    *const libc::c_char,
    *const libc::posix_spawn_file_actions_t,
    *const libc::posix_spawnattr_t,
    *const *mut libc::c_char,
    *const *mut libc::c_char,
) -> libc::c_int;
#[cfg(target_os = "linux")]
type AddClosefrom =
    unsafe extern "C" fn(*mut libc::posix_spawn_file_actions_t, libc::c_int) -> libc::c_int;
#[cfg(target_os = "linux")]
type PidfdGetpid = unsafe extern "C" fn(libc::c_int) -> libc::pid_t;

#[cfg(target_os = "linux")]
fn glibc<T: Copy>(name: &std::ffi::CStr) -> Option<T> {
    // SAFETY: dlsym only looks the name up; the caller names the matching type.
    let found = unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) };
    (!found.is_null()).then(|| unsafe { std::mem::transmute_copy::<*mut libc::c_void, T>(&found) })
}

/// Starts `program` with `args` and returns it held by a pidfd that the
/// kernel created with the child (`pidfd_spawnp`, CLONE_PIDFD): the
/// descriptor names that child from its first instant, whatever reaps it
/// later, and no number is ever looked up again (zerus-5zo1o). Its stdin and
/// stderr are /dev/null, its stdout is the returned pipe, and it inherits no
/// other descriptor. None when that cannot be had, with no child left.
#[cfg(target_os = "linux")]
fn spawn_held(program: &str, args: &[&str]) -> Option<(Helper, std::os::fd::OwnedFd)> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;
    let spawn: PidfdSpawnp = glibc(c"pidfd_spawnp")?;
    let close_from: AddClosefrom = glibc(c"posix_spawn_file_actions_addclosefrom_np")?;
    let argv: Vec<CString> = std::iter::once(program)
        .chain(args.iter().copied())
        .map(|word| CString::new(word).ok())
        .collect::<Option<_>>()?;
    // The environment as it is now: the child's own copy.
    let envp: Vec<CString> = std::env::vars_os()
        .filter_map(|(name, value)| {
            let mut pair = name.as_bytes().to_vec();
            pair.push(b'=');
            pair.extend_from_slice(value.as_bytes());
            CString::new(pair).ok()
        })
        .collect();
    let argv_ptrs: Vec<*mut libc::c_char> = argv
        .iter()
        .map(|a| a.as_ptr() as *mut _)
        .chain([std::ptr::null_mut()])
        .collect();
    let envp_ptrs: Vec<*mut libc::c_char> = envp
        .iter()
        .map(|e| e.as_ptr() as *mut _)
        .chain([std::ptr::null_mut()])
        .collect();
    let mut ends = [0; 2];
    // SAFETY: pipe2 fills two new descriptors, owned below.
    if unsafe { libc::pipe2(ends.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return None;
    }
    // SAFETY: fresh descriptors that nothing else owns.
    let (read, write) = unsafe { (OwnedFd::from_raw_fd(ends[0]), OwnedFd::from_raw_fd(ends[1])) };
    // SAFETY: the actions and attributes are initialised before use and
    // destroyed after the spawn; every pointer outlives the call.
    unsafe {
        let mut actions: libc::posix_spawn_file_actions_t = std::mem::zeroed();
        let mut attr: libc::posix_spawnattr_t = std::mem::zeroed();
        if libc::posix_spawn_file_actions_init(&mut actions) != 0 {
            return None;
        }
        if libc::posix_spawnattr_init(&mut attr) != 0 {
            libc::posix_spawn_file_actions_destroy(&mut actions);
            return None;
        }
        // As std spawns: an empty signal mask and SIGPIPE at its default.
        let mut empty: libc::sigset_t = std::mem::zeroed();
        let mut pipe: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut empty);
        libc::sigemptyset(&mut pipe);
        libc::sigaddset(&mut pipe, libc::SIGPIPE);
        let prepared = libc::posix_spawn_file_actions_addopen(
            &mut actions,
            0,
            c"/dev/null".as_ptr(),
            libc::O_RDONLY,
            0,
        ) == 0
            && libc::posix_spawn_file_actions_adddup2(&mut actions, write.as_raw_fd(), 1) == 0
            && libc::posix_spawn_file_actions_addopen(
                &mut actions,
                2,
                c"/dev/null".as_ptr(),
                libc::O_WRONLY,
                0,
            ) == 0
            && close_from(&mut actions, 3) == 0
            && libc::posix_spawnattr_setsigmask(&mut attr, &empty) == 0
            && libc::posix_spawnattr_setsigdefault(&mut attr, &pipe) == 0
            && libc::posix_spawnattr_setflags(
                &mut attr,
                (libc::POSIX_SPAWN_SETSIGMASK | libc::POSIX_SPAWN_SETSIGDEF) as libc::c_short,
            ) == 0;
        let mut pidfd = -1;
        let started = prepared
            && spawn(
                &mut pidfd,
                argv[0].as_ptr(),
                &actions,
                &attr,
                argv_ptrs.as_ptr(),
                envp_ptrs.as_ptr(),
            ) == 0;
        libc::posix_spawn_file_actions_destroy(&mut actions);
        libc::posix_spawnattr_destroy(&mut attr);
        if !started || pidfd < 0 {
            return None;
        }
        drop(write);
        Some((
            Helper {
                pidfd: OwnedFd::from_raw_fd(pidfd),
            },
            read,
        ))
    }
}

#[cfg(target_os = "linux")]
impl Helper {
    /// The child's PID, for tests and reports only: never waited for or signalled.
    fn pid(&self) -> Option<u32> {
        use std::os::fd::AsRawFd;
        let getpid: PidfdGetpid = glibc(c"pidfd_getpid")?;
        // SAFETY: pidfd_getpid reads this helper's own pidfd.
        let pid = unsafe { getpid(self.pidfd.as_raw_fd()) };
        u32::try_from(pid).ok().filter(|pid| *pid > 0)
    }

    fn wait_id(&self, options: libc::c_int) -> io::Result<Option<std::process::ExitStatus>> {
        use std::os::fd::AsRawFd;
        use std::os::unix::process::ExitStatusExt;
        // SAFETY: waitid on this helper's pidfd into a zeroed siginfo.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let id = self.pidfd.as_raw_fd() as libc::id_t;
        if unsafe { libc::waitid(libc::P_PIDFD, id, &mut info, options) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: waitid filled a SIGCHLD siginfo (or left it zeroed).
        let (pid, status) = unsafe { (info.si_pid(), info.si_status()) };
        if pid == 0 {
            return Ok(None);
        }
        Ok(Some(std::process::ExitStatus::from_raw(
            match info.si_code {
                libc::CLD_EXITED => (status & 0xff) << 8,
                libc::CLD_DUMPED => (status & 0x7f) | 0x80,
                _ => status & 0x7f,
            },
        )))
    }
}

#[cfg(target_os = "linux")]
impl Owned for Helper {
    fn try_wait(&mut self) -> io::Result<Option<std::process::ExitStatus>> {
        self.wait_id(libc::WEXITED | libc::WNOHANG)
    }
    fn kill(&mut self) -> io::Result<()> {
        use std::os::fd::AsRawFd;
        // SAFETY: pidfd_send_signal on this helper's pidfd, no siginfo.
        let sent = unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                self.pidfd.as_raw_fd(),
                libc::SIGKILL,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            )
        };
        if sent == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
    /// Reaps it after the signal, for at most a second.
    fn wait(&mut self) -> io::Result<std::process::ExitStatus> {
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            match self.try_wait()? {
                Some(status) => return Ok(status),
                None if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(5)),
                None => return Err(io::ErrorKind::TimedOut.into()),
            }
        }
    }
}

/// The most a bounded read takes from its child's output.
#[cfg(target_os = "linux")]
const READ_CAP: usize = 64 << 10;

/// The child's whole output when it exits successfully and its output ends,
/// both by `deadline`; otherwise None. `out` does not block, so no reader
/// outlives the call, even while another process holds the pipe open
/// (zerus-fp1m7): the pipe is simply closed. Only a child that `try_wait`
/// has just reported running is signalled, then waited for: no wait has
/// collected it, so its PID is still its own. Once waiting fails (the kernel
/// reaped it, for example) nothing is signalled (zerus-5zo1o).
#[cfg(target_os = "linux")]
fn drain(
    child: &mut impl Owned,
    out: &mut impl Read,
    deadline: Instant,
    pause: Duration,
) -> Option<Vec<u8>> {
    let (mut text, mut ended, mut exited) = (Vec::new(), false, None);
    let mut chunk = [0_u8; 4096];
    loop {
        while !ended {
            match out.read(&mut chunk) {
                Ok(0) => ended = true,
                Ok(read) if text.len() + read <= READ_CAP => text.extend_from_slice(&chunk[..read]),
                Ok(_) => {
                    // Too much: stop the child as at the deadline.
                    if exited.is_none() && matches!(child.try_wait(), Ok(None)) {
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                    return None;
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => ended = true,
            }
        }
        if exited.is_none() {
            match child.try_wait() {
                Ok(Some(status)) => exited = Some(status),
                Ok(None) => {}
                Err(_) => return None,
            }
        }
        match exited {
            Some(status) if ended => return status.success().then_some(text),
            _ if Instant::now() >= deadline => {
                if exited.is_none() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                return None;
            }
            _ => std::thread::sleep(pause),
        }
    }
}

#[cfg(target_os = "linux")]
fn bounded_read_with(
    program: &str,
    args: &[&str],
    limit: Duration,
    spawned: impl FnOnce(u32),
    auto_reaped: impl FnOnce() -> bool,
) -> Option<String> {
    use std::os::fd::AsRawFd;
    if auto_reaped() {
        return None;
    }
    let deadline = Instant::now() + limit;
    // Every wait and signal goes through the pidfd the child was born with.
    let (mut helper, out) = spawn_held(program, args)?;
    if let Some(pid) = helper.pid() {
        spawned(pid);
    }
    // SAFETY: F_GETFL and F_SETFL on this call's own pipe descriptor.
    let nonblocking = unsafe {
        let flags = libc::fcntl(out.as_raw_fd(), libc::F_GETFL);
        flags >= 0 && libc::fcntl(out.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) == 0
    };
    let text = if nonblocking {
        drain(
            &mut helper,
            &mut File::from(out),
            deadline,
            Duration::from_millis(10),
        )
    } else {
        // No pipe to read: end the child now, as at the deadline.
        drain(
            &mut helper,
            &mut io::empty(),
            Instant::now(),
            Duration::ZERO,
        )
        .and(None)
    }?;
    Some(String::from_utf8_lossy(&text).into_owned())
}

/// `systemctl --user show` for `unit`, read-only and bounded at 2 s like
/// `SingleInstance::currentService`; a missing systemctl, or a system that
/// is not systemd, reads as no service at once.
#[cfg(target_os = "linux")]
fn systemd_now(unit: &str) -> Service {
    bounded_read(
        "systemctl",
        &[
            "--user",
            "show",
            "--property=ActiveState,MainPID,InvocationID",
            unit,
        ],
        Duration::from_secs(2),
    )
    .map(|text| systemd_service(&text))
    .unwrap_or_default()
}

/// `launchctl print`, bounded at 2 s like `SingleInstance::currentService`.
fn launchd_now(label: &str) -> Service {
    // SAFETY: getuid cannot fail.
    let uid = unsafe { libc::getuid() };
    let Ok(child) = Command::new("launchctl")
        .args(["print", &format!("gui/{uid}/{label}")])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return Service::default();
    };
    let pid = child.id();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(child.wait_with_output());
    });
    match receiver.recv_timeout(Duration::from_secs(2)) {
        Ok(Ok(output)) if output.status.success() => {
            launchd_service(&String::from_utf8_lossy(&output.stdout))
        }
        Ok(_) => Service::default(),
        Err(_) => {
            // SAFETY: the child is ours and still running.
            unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
            Service::default()
        }
    }
}

/// `SingleInstance::Startup`'s defaults.
pub struct Timing {
    pub lock_wait: Duration,
    pub service_wait: Duration,
    pub poll: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            lock_wait: Duration::from_secs(10),
            service_wait: Duration::from_secs(30),
            poll: Duration::from_millis(200),
        }
    }
}

/// What `start` does to the world, so tests can script it.
pub trait Start {
    fn lock(&mut self, wait: Duration) -> Result<(), String>;
    fn unlock(&mut self);
    fn forward(&mut self, activation: Activation) -> bool;
    fn service(&mut self) -> Service;
    /// This process's systemd invocation (`INVOCATION_ID`), when it has one.
    fn own_invocation(&self) -> Option<u128> {
        None
    }
    fn claim(&mut self) -> io::Result<()>;
    fn pause(&mut self, pause: Duration);
    fn elapsed(&self) -> Duration;
    fn report(&mut self, line: &str);
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Forwarded,
    Claimed,
    Failed(String),
}

/// `SingleInstance::start` (`SingleInstance.cpp:148-191`): probing the
/// socket, removing a stale one and listening are one step under the lock; a
/// start that launchd does not run leaves the socket to the service while
/// it runs or is about to.
pub fn start(
    effects: &mut impl Start,
    activation: Activation,
    own_pid: i64,
    timing: &Timing,
) -> Outcome {
    let mut lockable = true;
    let mut wait_reported = false;
    loop {
        if lockable {
            if let Err(why) = effects.lock(timing.lock_wait) {
                lockable = false;
                effects.report(&why);
            }
        }
        if effects.forward(activation.clone()) {
            effects.unlock();
            return Outcome::Forwarded;
        }
        let service = effects.service();
        if waits_for_service(&service, own_pid, effects.own_invocation()) {
            if effects.elapsed() < timing.service_wait {
                if !wait_reported {
                    wait_reported = true;
                    effects.report(&if service.pid > 0 {
                        format!(
                            "waiting for the Zerus service (pid {}) to open its socket",
                            service.pid
                        )
                    } else {
                        "waiting for the Zerus service to start".to_owned()
                    });
                }
                effects.unlock();
                effects.pause(timing.poll);
                continue;
            }
            effects.report(&format!(
                "the Zerus service did not open its socket within {} s; starting here",
                timing.service_wait.as_secs_f64()
            ));
        }
        let outcome = match effects.claim() {
            Ok(()) => Outcome::Claimed,
            Err(error) => Outcome::Failed(error.to_string()),
        };
        effects.unlock();
        return outcome;
    }
}

/// Hands the activation to an instance that answers (`SingleInstance.cpp:22-31`).
pub fn forward(socket: &Path, activation: Activation) -> bool {
    let line = activation.line();
    if line.is_empty() {
        return false;
    }
    let stream = (|| -> io::Result<UnixStream> {
        let connection = socket2::Socket::new(socket2::Domain::UNIX, socket2::Type::STREAM, None)?;
        connection.connect_timeout(
            &socket2::SockAddr::unix(socket)?,
            Duration::from_millis(200),
        )?;
        let fd: std::os::fd::OwnedFd = connection.into();
        let stream = UnixStream::from(fd);
        stream.set_write_timeout(Some(Duration::from_millis(200)))?;
        Ok(stream)
    })();
    let Ok(mut stream) = stream else {
        return false;
    };
    stream.write_all(&line).is_ok()
}

struct UnixStart<'a> {
    paths: &'a Paths,
    own: &'a LockInfo,
    label: Option<&'a str>,
    lock: Option<StartLock>,
    started: Instant,
    listener: Option<UnixListener>,
}

impl Start for UnixStart<'_> {
    fn lock(&mut self, wait: Duration) -> Result<(), String> {
        if let Some(folder) = self.paths.lock.parent() {
            // Qt's runtime folder is the user's own (`QStandardPaths`).
            let _ = DirBuilder::new().recursive(true).mode(0o700).create(folder);
        }
        let deadline = Instant::now() + wait;
        loop {
            match try_lock(&self.paths.lock, self.own) {
                LockAttempt::Taken(lock) => {
                    self.lock = Some(lock);
                    return Ok(());
                }
                LockAttempt::Busy if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20))
                }
                LockAttempt::Busy => {
                    return Err(format!(
                        "another start keeps {} locked; starting without it",
                        self.paths.lock.display()
                    ))
                }
                LockAttempt::Unavailable => {
                    return Err(format!(
                        "cannot create {}; concurrent starts are not serialized",
                        self.paths.lock.display()
                    ))
                }
            }
        }
    }

    fn unlock(&mut self) {
        self.lock = None;
    }

    fn forward(&mut self, activation: Activation) -> bool {
        forward(&self.paths.socket, activation)
    }

    fn service(&mut self) -> Service {
        #[cfg(target_os = "linux")]
        let systemd = systemd_now;
        #[cfg(not(target_os = "linux"))]
        let systemd = |_: &str| Service::default();
        platform_service(self.label, launchd_now, systemd)
    }

    fn own_invocation(&self) -> Option<u128> {
        std::env::var("INVOCATION_ID")
            .ok()
            .and_then(|id| invocation_id(&id))
    }

    fn claim(&mut self) -> io::Result<()> {
        // Nothing answers: the socket, if any, is a crashed instance's.
        match fs::remove_file(&self.paths.socket) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
        let listener = UnixListener::bind(&self.paths.socket)?;
        fs::set_permissions(&self.paths.socket, Permissions::from_mode(0o600))?;
        self.listener = Some(listener);
        Ok(())
    }

    fn pause(&mut self, pause: Duration) {
        std::thread::sleep(pause);
    }

    fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    fn report(&mut self, line: &str) {
        eprintln!("zerus-client: {line}");
    }
}

/// The claimed socket; dropping it removes the file it bound, as
/// `QLocalServer::close` does, unless another instance replaced it.
pub struct Listening {
    socket: PathBuf,
    inode: u64,
    // Stop both listeners before releasing lifetime exclusion.
    _servers: Vec<Server>,
    _canonical: Option<canonical::Guard>,
}

impl Drop for Listening {
    fn drop(&mut self) {
        if fs::metadata(&self.socket).is_ok_and(|metadata| metadata.ino() == self.inode) {
            let _ = fs::remove_file(&self.socket);
        }
    }
}

pub struct Claimed {
    pub listening: Listening,
    /// Lines from later starts, queued until the app takes them.
    pub activations: Receiver<Activation>,
}

pub enum Claim {
    Claimed(Claimed),
    Forwarded,
    Duplicate(&'static str),
    Failed(String),
}

/// The claimed socket, kept in the app state until the app exits.
pub struct InstanceState(pub Mutex<Option<Listening>>);

impl InstanceState {
    /// The exit hook's last step: the socket goes, unless another instance
    /// replaced it. Tauri never drops its managed state, so this is the only
    /// place the file is removed.
    pub fn release(&self) {
        if let Some(mut listening) = self.0.lock().unwrap_or_else(PoisonError::into_inner).take() {
            let canonical = listening._canonical.is_some();
            if let Some(guard) = listening._canonical.take() {
                guard.retain_until_exit();
            }
            drop(listening);
            if canonical {
                crate::startup_log::record("quitting", "listeners_closed_owner_retained", None);
            }
        }
    }
}

/// Takes the single-instance socket named `name`, or hands `activation` to
/// the instance that holds it.
pub type Reply = std::sync::Arc<dyn Fn(&UnixStream, &[u8]) -> Option<Vec<u8>> + Send + Sync>;

pub fn claim(
    name: &str,
    label: Option<&str>,
    activation: Activation,
    reply: Option<Reply>,
    admitted: impl FnOnce(),
) -> Claim {
    claim_in(
        name,
        label,
        activation,
        reply,
        &canonical::directory(),
        admitted,
    )
}

fn claim_in(
    name: &str,
    label: Option<&str>,
    activation: Activation,
    reply: Option<Reply>,
    directory: &Path,
    admitted: impl FnOnce(),
) -> Claim {
    let result = claim_inner(name, label, activation, reply, directory, admitted);
    crate::startup_log::guard(&result);
    result
}

fn claim_inner(
    name: &str,
    label: Option<&str>,
    activation: Activation,
    reply: Option<Reply>,
    directory: &Path,
    admitted: impl FnOnce(),
) -> Claim {
    let guard = if name == "hgs-tray" {
        match canonical_claim(directory, activation.clone()) {
            Ok(CanonicalClaim::Owner(guard)) => Some(guard),
            Ok(CanonicalClaim::Forwarded) => return Claim::Forwarded,
            Ok(CanonicalClaim::Busy) => return Claim::Duplicate("canonical_owner_unresponsive"),
            Err(error) => return Claim::Failed(error.to_string()),
        }
    } else {
        None
    };
    let xdg_runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
    let runtime = runtime_dir(
        Platform::current(),
        &crate::identity::home_dir(),
        xdg_runtime.as_deref(),
    );
    claim_guarded(
        &paths(name, &temp_dir(), &runtime),
        label,
        activation,
        &LockInfo::own(),
        &Timing::default(),
        reply,
        (guard, admitted),
    )
}

enum CanonicalClaim {
    Owner(canonical::Guard),
    Forwarded,
    Busy,
}
fn canonical_claim(directory: &Path, activation: Activation) -> io::Result<CanonicalClaim> {
    canonical_claim_observed(directory, activation, || {})
}
fn canonical_claim_observed(
    directory: &Path,
    activation: Activation,
    mut busy: impl FnMut(),
) -> io::Result<CanonicalClaim> {
    // A concurrent start may still be opening its listeners. Failure to focus
    // is a clean duplicate exit, never permission to create another GUI.
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(guard) = canonical::acquire(directory)? {
            return Ok(CanonicalClaim::Owner(guard));
        }
        busy();
        if forward(&directory.join("gui.sock"), activation.clone()) {
            return Ok(CanonicalClaim::Forwarded);
        }
        if Instant::now() >= deadline {
            return Ok(CanonicalClaim::Busy);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(test)]
pub fn claim_at(
    paths: &Paths,
    label: Option<&str>,
    activation: Activation,
    own: &LockInfo,
    timing: &Timing,
) -> Claim {
    claim_with_reply(paths, label, activation, own, timing, None)
}

#[cfg(test)]
fn claim_with_reply(
    paths: &Paths,
    label: Option<&str>,
    activation: Activation,
    own: &LockInfo,
    timing: &Timing,
    reply: Option<Reply>,
) -> Claim {
    claim_guarded(paths, label, activation, own, timing, reply, (None, || {}))
}

fn claim_guarded(
    paths: &Paths,
    label: Option<&str>,
    activation: Activation,
    own: &LockInfo,
    timing: &Timing,
    reply: Option<Reply>,
    admission: (Option<canonical::Guard>, impl FnOnce()),
) -> Claim {
    let (mut guard, admitted) = admission;
    let mut effects = UnixStart {
        paths,
        own,
        label,
        lock: None,
        started: Instant::now(),
        listener: None,
    };
    match start(&mut effects, activation, own.pid, timing) {
        Outcome::Forwarded => Claim::Forwarded,
        Outcome::Failed(error) => Claim::Failed(error),
        Outcome::Claimed => {
            // Renderer setup may write a crash marker and change environment.
            // Only an admitted GUI does it, before listener/GTK threads exist.
            admitted();
            let Some(listener) = effects.listener.take() else {
                return Claim::Failed("the socket was not kept".to_owned());
            };
            let inode = fs::metadata(&paths.socket)
                .map(|metadata| metadata.ino())
                .unwrap_or(0);
            let (sender, activations) = mpsc::sync_channel(32);
            let mut servers = Vec::new();
            if let Some(guard) = &mut guard {
                let listener = match guard.bind() {
                    Ok(listener) => listener,
                    Err(error) => {
                        let _ = fs::remove_file(&paths.socket);
                        return Claim::Failed(error.to_string());
                    }
                };
                let sender = sender.clone();
                // The canonical endpoint carries public navigation only.
                // Configuration-bound desktop Hello remains on Qt's socket.
                servers.push(listen(
                    listener,
                    move |activation| {
                        let _ = sender.try_send(activation);
                    },
                    None,
                ));
            }
            servers.push(listen(
                listener,
                move |activation| {
                    let _ = sender.try_send(activation);
                },
                reply,
            ));
            Claim::Claimed(Claimed {
                listening: Listening {
                    socket: paths.socket.clone(),
                    inode,
                    _servers: servers,
                    _canonical: guard,
                },
                activations,
            })
        }
    }
}

/// How long one connection may take to send its line.
const LINE_WAIT: Duration = Duration::from_secs(1);

struct Server {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    wake: UnixStream,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        let _ = self.wake.shutdown(std::net::Shutdown::Write);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Reads the first line of each connection (`main.cpp:424-433`). Qt acts only
/// on a complete line (`canReadLine`); a connection gets at most 1 s in all
/// and 64 bytes, so a silent or slow client cannot hold the next start back.
fn listen(
    listener: UnixListener,
    deliver: impl Fn(Activation) + Send + 'static,
    reply: Option<Reply>,
) -> Server {
    listener
        .set_nonblocking(true)
        .expect("instance listener is nonblocking");
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stopped = stop.clone();
    let (wake, awakened) = UnixStream::pair().expect("instance wake socket");
    let thread = std::thread::Builder::new()
        .name("zerus-client-instance".into())
        .spawn(move || {
            while !stopped.load(std::sync::atomic::Ordering::Acquire) {
                // Sleep in the kernel until a launch arrives or Drop wakes
                // shutdown. Two GUI endpoints must not add idle polling.
                let mut ready = [
                    libc::pollfd {
                        fd: listener.as_raw_fd(),
                        events: libc::POLLIN,
                        revents: 0,
                    },
                    libc::pollfd {
                        fd: awakened.as_raw_fd(),
                        events: libc::POLLIN,
                        revents: 0,
                    },
                ];
                // SAFETY: both descriptors and the writable array outlive poll.
                if unsafe { libc::poll(ready.as_mut_ptr(), ready.len() as libc::nfds_t, -1) } < 0 {
                    if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                        continue;
                    }
                    return;
                }
                if ready[1].revents != 0 || stopped.load(std::sync::atomic::Ordering::Acquire) {
                    return;
                }
                let mut stream = match listener.accept() {
                    Ok((stream, _)) => stream,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        continue;
                    }
                    Err(_) => return,
                };
                // BSD may inherit the listener file flags. Keep accepted I/O
                // bounded by the explicit deadlines, as on Linux.
                let _ = stream.set_nonblocking(false);
                let deadline = Instant::now() + LINE_WAIT;
                let mut line = Vec::new();
                let mut byte = [0u8; 1];
                // macOS refuses a timeout once the peer has closed, as a second
                // start does right after its line. Its bytes are buffered then:
                // read them without blocking, so no read can wait unbounded.
                if stream.set_read_timeout(Some(LINE_WAIT)).is_err() {
                    let _ = stream.set_nonblocking(true);
                }
                let complete = loop {
                    let left = deadline.saturating_duration_since(Instant::now());
                    #[cfg(all(feature = "live", target_os = "linux"))]
                    let cap = zerus_client_core::desktop::activation::NAVIGATION_BYTES + 8;
                    #[cfg(not(all(feature = "live", target_os = "linux")))]
                    let cap = 64;
                    if line.len() >= cap || left.is_zero() {
                        break false;
                    }
                    // macOS refuses the option once the peer has closed; the
                    // remaining bytes then read without waiting.
                    let _ = stream.set_read_timeout(Some(left));
                    match stream.read(&mut byte) {
                        Ok(1) if byte[0] == b'\n' => break true,
                        Ok(1) => line.push(byte[0]),
                        _ => break false,
                    }
                };
                if let Some(activation) = complete.then(|| Activation::parse(&line)).flatten() {
                    deliver(activation);
                } else if complete {
                    if let Some(bytes) = reply.as_ref().and_then(|reply| reply(&stream, &line)) {
                        let left = deadline.saturating_duration_since(Instant::now());
                        if !left.is_zero()
                            && bytes.len() <= 4096
                            && stream.set_write_timeout(Some(left)).is_ok()
                        {
                            let _ = stream.write_all(&bytes);
                        }
                    }
                }
            }
        })
        .expect("the instance thread starts");
    Server {
        stop,
        wake,
        thread: Some(thread),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    mod canonical_tests {
        use super::*;
        include!("instance_tests.rs");
    }

    /// A test's own folder, removed when the test ends (also on a panic).
    struct Scratch(PathBuf);

    impl std::ops::Deref for Scratch {
        type Target = Path;

        fn deref(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn scratch(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("zci-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    #[test]
    fn a_tests_folder_goes_with_it() {
        let dir = scratch("drop");
        fs::write(dir.join("socket-like"), b"").unwrap();
        let path = dir.to_path_buf();
        drop(dir);
        assert!(!path.exists(), "scratch folders are removed");
    }

    fn fast() -> Timing {
        Timing {
            lock_wait: Duration::from_millis(300),
            service_wait: Duration::from_millis(400),
            poll: Duration::from_millis(20),
        }
    }

    #[cfg(all(feature = "live", target_os = "linux"))]
    #[test]
    fn navigation_instance_wire_is_target_only_and_retains_exact_identity() {
        let value = Activation::Navigation(zerus_client_core::desktop::activation::Target {
            computer: "peer.example".into(),
            run: "original-run".into(),
            conversation: "original-conversation".into(),
            startup_question: None,
        });
        assert_eq!(Activation::parse(&value.line()), Some(value));
        assert!(Activation::parse(br#"target {"computer":"peer","run":"r","conversation":"c","activation_token":"injected"}"#).is_none());
        let dir = scratch("navigation");
        let paths = paths("navigation", &dir, &dir);
        let Claim::Claimed(first) =
            claim_at(&paths, None, Activation::Show, &LockInfo::own(), &fast())
        else {
            panic!("first claims");
        };
        let target = zerus_client_core::desktop::activation::Target {
            computer: "peer.example".into(),
            run: "r".repeat(1024),
            conversation: "c".repeat(1024),
            startup_question: None,
        };
        let value = Activation::Navigation(target);
        assert!(forward(&paths.socket, value.clone()));
        assert_eq!(
            first
                .activations
                .recv_timeout(Duration::from_secs(2))
                .unwrap(),
            value
        );
    }

    #[test]
    fn activation_lines_are_qt_lines() {
        assert_eq!(Activation::Sessions.line(), b"sessions\n");
        assert_eq!(Activation::Show.line(), b"show\n");
        assert_eq!(
            Activation::parse(b" sessions\r"),
            Some(Activation::Sessions)
        );
        assert_eq!(Activation::parse(b"show"), Some(Activation::Show));
        assert_eq!(Activation::parse(b"quit"), None);
        assert_eq!(
            Activation::parse(b"\x0bsessions\x0c"),
            Some(Activation::Sessions),
            "QByteArray::trimmed"
        );
        assert_eq!(Activation::parse(b"session"), None);
        assert_eq!(Activation::parse(b""), None);
    }

    #[test]
    fn paths_use_qt_names() {
        let p = paths(
            "hgs-tray",
            Path::new("/private/var/folders/x/T"),
            Path::new("/Users/example/Library/Application Support"),
        );
        assert_eq!(p.socket, Path::new("/private/var/folders/x/T/hgs-tray"));
        assert_eq!(
            p.lock,
            Path::new("/Users/example/Library/Application Support/hgs-tray-start.lock")
        );
        assert_eq!(
            runtime_dir(Platform::Mac, Path::new("/Users/example"), None),
            Path::new("/Users/example/Library/Application Support")
        );
        assert_eq!(
            runtime_dir(
                Platform::Linux,
                Path::new("/home/example"),
                Some(Path::new("/run/user/1000"))
            ),
            Path::new("/run/user/1000")
        );
        // Qt's fallback without XDG_RUNTIME_DIR.
        let fallback = temp_dir().join(format!("runtime-{}", user_name()));
        assert_eq!(
            runtime_dir(Platform::Linux, Path::new("/home/example"), None),
            fallback
        );
        assert_eq!(
            runtime_dir(
                Platform::Linux,
                Path::new("/home/example"),
                Some(Path::new(""))
            ),
            fallback
        );
    }

    #[test]
    fn the_socket_folder_is_qts_lexically_cleaned_temp_path() {
        let dir = scratch("temp");
        let real = dir.join("real");
        fs::create_dir(&real).unwrap();
        let link = dir.join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let mut given = link.clone().into_os_string();
        given.push("/");
        assert_eq!(
            temp_dir_from(Some(given)),
            link,
            "a symbolic link stays, the trailing slash goes"
        );
        assert_eq!(
            temp_dir_from(Some("/var/folders/x/T/".into())),
            Path::new("/var/folders/x/T"),
            "never /private/var"
        );
        assert_eq!(
            temp_dir_from(Some("/tmp//a/./b/../c/".into())),
            Path::new("/tmp/a/c")
        );
        assert_eq!(temp_dir_from(Some("/../tmp".into())), Path::new("/tmp"));
        assert_eq!(
            paths(
                "hgs-tray",
                &temp_dir_from(Some("/var/folders/x/T/".into())),
                Path::new("/r")
            )
            .socket,
            Path::new("/var/folders/x/T/hgs-tray")
        );
        assert_eq!(
            temp_dir_from(Some(OsString::new())),
            temp_dir_from(None),
            "an empty TMPDIR counts as unset"
        );
        assert!(temp_dir_from(None).is_absolute());
    }

    #[test]
    fn lock_staleness_follows_qlockfile() {
        let info = LockInfo {
            pid: 42,
            app: "hgs-tray".into(),
            host: "mac".into(),
        };
        let young = Duration::from_secs(1);
        assert!(
            is_stale(Some(&info), young, "mac", |_| None),
            "the holder exited"
        );
        assert!(
            is_stale(Some(&info), young, "mac", |_| Some(Some("Safari".into()))),
            "its PID runs another program"
        );
        assert!(!is_stale(Some(&info), young, "mac", |_| Some(Some(
            "hgs-tray".into()
        ))));
        assert!(!is_stale(Some(&info), young, "mac", |_| Some(None)));
        assert!(
            !is_stale(Some(&info), young, "other", |_| None),
            "another host's holder is not judged by PID"
        );
        assert!(
            is_stale(Some(&info), Duration::from_secs(31), "mac", |_| Some(Some(
                "hgs-tray".into()
            ))),
            "QLockFile's 30 s"
        );
        assert!(is_stale(None, Duration::from_secs(31), "mac", |_| None));
        assert!(!is_stale(None, young, "mac", |_| None));
        assert_eq!(
            LockInfo::parse("42\nhgs-tray\nmac\nmachine-id\nboot-id\n"),
            Some(info.clone())
        );
        assert_eq!(LockInfo::parse(&info.contents()), Some(info));
        assert_eq!(LockInfo::parse("x\n"), None);
    }

    #[test]
    fn launchd_print_reads_only_the_jobs_own_state() {
        let text = |state: &str, pid: &str| {
            let state = if state.is_empty() {
                String::new()
            } else {
                format!("\tstate = {state}\n")
            };
            let pid = if pid.is_empty() {
                String::new()
            } else {
                format!("\tpid = {pid}\n")
            };
            format!(
                "gui/501/com.hgdev.hgs-tray = {{\n\tactive count = 1\n\tpath = /Users/example/Library/LaunchAgents/com.hgdev.hgs-tray.plist\n\ttype = LaunchAgent\n{state}\n\tprogram = /Applications/Zerus.app/Contents/MacOS/hgs-tray\n\tevent triggers = {{\n\t\tcom.example.trigger => {{\n\t\t\tstate = running\n\t\t\tpid = 7\n\t\t}}\n\t}}\n{pid}\tlast exit code = (never exited)\n}}\n"
            )
        };
        assert_eq!(
            launchd_service(&text("running", "4242")),
            Service {
                state: ServiceState::Running,
                pid: 4242,
                invocation: None,
            }
        );
        assert_eq!(
            launchd_service(&text("spawn scheduled", "")),
            Service {
                state: ServiceState::Starting,
                pid: 0,
                invocation: None,
            }
        );
        assert_eq!(
            launchd_service(&text("not running", "")),
            Service::default()
        );
        assert_eq!(launchd_service(&text("", "")), Service::default());
        assert_eq!(launchd_service(""), Service::default());
    }

    #[test]
    fn only_another_service_process_keeps_the_socket() {
        assert!(!waits_for_service(&Service::default(), 100, None));
        assert!(!waits_for_service(
            &Service {
                state: ServiceState::Running,
                pid: 100,
                invocation: None,
            },
            100,
            None
        ));
        assert!(waits_for_service(
            &Service {
                state: ServiceState::Running,
                pid: 200,
                invocation: None,
            },
            100,
            None
        ));
        assert!(waits_for_service(
            &Service {
                state: ServiceState::Starting,
                pid: 0,
                invocation: None,
            },
            100,
            None
        ));
        assert!(waits_for_service(
            &Service {
                state: ServiceState::Starting,
                pid: 200,
                invocation: None,
            },
            100,
            None
        ));
    }

    #[test]
    fn systemctl_show_reads_qts_states_and_the_run() {
        let show = |active: &str, pid: &str, id: &str| {
            systemd_service(&format!(
                "ActiveState={active}\nMainPID={pid}\nInvocationID={id}\n"
            ))
        };
        let id = "0123456789abcdef0123456789abcdef";
        let run = invocation_id(id);
        assert!(run.is_some());
        for active in ["active", "reloading", "refreshing"] {
            assert_eq!(
                show(active, "4242", id),
                Service {
                    state: ServiceState::Running,
                    pid: 4242,
                    invocation: run,
                },
                "{active}"
            );
        }
        for active in ["activating", "deactivating"] {
            assert_eq!(
                show(active, "0", "").state,
                ServiceState::Starting,
                "{active}"
            );
            assert_eq!(show(active, "0", "").pid, 0);
            assert_eq!(show(active, "0", "").invocation, None);
        }
        for active in ["inactive", "failed", "", "maintenance"] {
            assert_eq!(show(active, "4242", id), Service::default(), "{active}");
        }
        assert_eq!(systemd_service(""), Service::default());
        assert_eq!(show("active", "-5", "nonsense").pid, 0);
        assert_eq!(show("active", "1", "nonsense").invocation, None);
        assert_eq!(invocation_id("01234567-89ab-cdef-0123-456789abcdef"), run);
        assert_eq!(invocation_id(&"0".repeat(32)), None);
        assert_eq!(invocation_id("0123"), None);
    }

    #[test]
    fn the_units_own_run_never_waits_on_itself() {
        let unit = |pid: i64, invocation: Option<u128>| Service {
            state: ServiceState::Running,
            pid,
            invocation,
        };
        // ExecStart through a wrapper: MainPID is the wrapper, the run is ours.
        assert!(!waits_for_service(&unit(200, Some(7)), 100, Some(7)));
        assert!(
            waits_for_service(&unit(200, Some(7)), 100, Some(8)),
            "another run"
        );
        assert!(
            waits_for_service(&unit(200, Some(7)), 100, None),
            "a launch from the menu"
        );
        assert!(waits_for_service(&unit(200, None), 100, None));
        assert!(
            !waits_for_service(&unit(100, Some(7)), 100, None),
            "the main process"
        );
    }

    #[test]
    fn only_the_switch_asks_its_platforms_service() {
        let asked = std::cell::RefCell::new(Vec::new());
        let running = Service {
            state: ServiceState::Running,
            pid: 9,
            invocation: None,
        };
        let launchd = |label: &str| {
            asked.borrow_mut().push(format!("launchd {label}"));
            running
        };
        let systemd = |unit: &str| {
            asked.borrow_mut().push(format!("systemd {unit}"));
            running
        };
        assert_eq!(platform_service(None, launchd, systemd), Service::default());
        assert!(
            asked.borrow().is_empty(),
            "a build that is not the switch asks nothing"
        );
        let found = platform_service(Some("com.hgdev.hgs-tray"), launchd, systemd);
        let expected: Vec<String> = if cfg!(target_os = "macos") {
            vec!["launchd com.hgdev.hgs-tray".into()]
        } else if cfg!(target_os = "linux") {
            vec!["systemd hgs-tray.service".into()]
        } else {
            vec![]
        };
        assert_eq!(found == running, !expected.is_empty());
        assert_eq!(*asked.borrow(), expected);
    }

    #[cfg(target_os = "linux")]
    /// A scripted child for `settle`.
    struct Scripted {
        waits: VecDeque<io::Result<Option<std::process::ExitStatus>>>,
        signalled: usize,
        reaped: usize,
    }

    #[cfg(target_os = "linux")]
    impl Owned for Scripted {
        fn try_wait(&mut self) -> io::Result<Option<std::process::ExitStatus>> {
            self.waits.pop_front().unwrap_or(Ok(None))
        }
        fn kill(&mut self) -> io::Result<()> {
            self.signalled += 1;
            Ok(())
        }
        fn wait(&mut self) -> io::Result<std::process::ExitStatus> {
            self.reaped += 1;
            Ok(std::os::unix::process::ExitStatusExt::from_raw(9))
        }
    }

    #[cfg(target_os = "linux")]
    /// A pipe that answers from a script, then would block (or ends).
    struct Pipe {
        reads: VecDeque<io::Result<Vec<u8>>>,
        ends: bool,
    }

    #[cfg(target_os = "linux")]
    impl Read for Pipe {
        fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
            match self.reads.pop_front() {
                Some(Ok(bytes)) => {
                    into[..bytes.len()].copy_from_slice(&bytes);
                    Ok(bytes.len())
                }
                Some(Err(error)) => Err(error),
                None if self.ends => Ok(0),
                None => Err(io::ErrorKind::WouldBlock.into()),
            }
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_child_whose_wait_was_lost_is_never_signalled() {
        let exited = |code: i32| -> io::Result<Option<std::process::ExitStatus>> {
            Ok(Some(std::os::unix::process::ExitStatusExt::from_raw(code)))
        };
        let script = |waits: Vec<io::Result<Option<std::process::ExitStatus>>>| Scripted {
            waits: waits.into(),
            signalled: 0,
            reaped: 0,
        };
        let pipe = |reads: Vec<&[u8]>, ends: bool| Pipe {
            reads: reads.into_iter().map(|bytes| Ok(bytes.to_vec())).collect(),
            ends,
        };
        let later = Instant::now() + Duration::from_secs(60);
        // The kernel already reaped it (SIGCHLD ignored): ECHILD, and its old PID is not ours.
        let mut lost = script(vec![Err(io::Error::from_raw_os_error(libc::ECHILD))]);
        assert_eq!(
            drain(&mut lost, &mut pipe(vec![], false), later, Duration::ZERO),
            None
        );
        assert_eq!((lost.signalled, lost.reaped), (0, 0));
        let mut lost_later = script(vec![
            Ok(None),
            Ok(None),
            Err(io::Error::from_raw_os_error(libc::ECHILD)),
        ]);
        assert_eq!(
            drain(
                &mut lost_later,
                &mut pipe(vec![], false),
                later,
                Duration::ZERO
            ),
            None
        );
        assert_eq!(lost_later.signalled, 0);
        // Still running at the deadline: signalled once, then waited for.
        let mut hung = script(vec![Ok(None)]);
        assert_eq!(
            drain(
                &mut hung,
                &mut pipe(vec![], false),
                Instant::now(),
                Duration::ZERO
            ),
            None
        );
        assert_eq!((hung.signalled, hung.reaped), (1, 1));
        // Exited, and its output ended: the whole answer.
        let mut quick = script(vec![Ok(None), exited(0)]);
        let answer = drain(
            &mut quick,
            &mut pipe(vec![b"ActiveState=", b"active\n"], true),
            later,
            Duration::ZERO,
        );
        assert_eq!(answer.as_deref(), Some(&b"ActiveState=active\n"[..]));
        assert_eq!((quick.signalled, quick.reaped), (0, 0));
        // Exited, but another process keeps its output open: no answer by the
        // deadline, nothing signalled and nothing left reading (zerus-fp1m7).
        let mut held = script(vec![exited(0)]);
        let soon = Instant::now() + Duration::from_millis(30);
        assert_eq!(
            drain(
                &mut held,
                &mut pipe(vec![b"ActiveState=active\n"], false),
                soon,
                Duration::from_millis(5)
            ),
            None
        );
        assert_eq!((held.signalled, held.reaped), (0, 0));
        // A failed show answers nothing.
        let mut failed = script(vec![exited(3 << 8)]);
        assert_eq!(
            drain(
                &mut failed,
                &mut pipe(vec![b"partial"], true),
                later,
                Duration::ZERO
            ),
            None
        );
        // More than the cap: the running child is stopped.
        let mut chatty = script(vec![Ok(None), Ok(None)]);
        let flood = vec![b'x'; 4096];
        let mut big = Pipe {
            reads: (0..20).map(|_| Ok(flood.clone())).collect(),
            ends: false,
        };
        assert_eq!(drain(&mut chatty, &mut big, later, Duration::ZERO), None);
        assert_eq!((chatty.signalled, chatty.reaped), (1, 1));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_read_returns_while_another_process_holds_the_pipe() {
        // A background child keeps the pipe open after the shell exits.
        let begun = Instant::now();
        let script = "(sleep 3) & printf 'ActiveState=active\\n'";
        assert_eq!(
            bounded_read("/bin/sh", &["-c", script], Duration::from_millis(200)),
            None
        );
        assert!(
            begun.elapsed() < Duration::from_secs(2),
            "{:?}",
            begun.elapsed()
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn an_auto_reaping_sigchld_policy_refuses_the_read_before_any_child() {
        assert!(auto_reaps(libc::SIG_IGN, 0));
        assert!(auto_reaps(libc::SIG_DFL, libc::SA_NOCLDWAIT));
        assert!(!auto_reaps(libc::SIG_DFL, 0));
        assert!(!auto_reaps(libc::SIG_DFL, libc::SA_RESTART));
        let read = bounded_read_with(
            "/bin/sh",
            &["-c", "printf answered"],
            Duration::from_secs(5),
            |_| panic!("no child under SIG_IGN"),
            || true,
        );
        assert_eq!(read, None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_child_is_held_by_the_pidfd_it_was_born_with() {
        // Reaped through its pidfd: a signal then reaches nothing at all.
        let (mut helper, _out) = spawn_held("/bin/sh", &["-c", "exit 0"]).expect("pidfd_spawnp");
        assert!(helper.pid().is_some());
        assert!(helper.wait().unwrap().success());
        assert_eq!(helper.kill().unwrap_err().raw_os_error(), Some(libc::ESRCH));
        // Exit codes and signals read as std reports them.
        let (mut failing, _out) = spawn_held("/bin/sh", &["-c", "exit 3"]).unwrap();
        assert_eq!(failing.wait().unwrap().code(), Some(3));
        let (mut hung, _out) = spawn_held("/bin/sh", &["-c", "exec sleep 30"]).unwrap();
        hung.kill().unwrap();
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(hung.wait().unwrap().signal(), Some(libc::SIGKILL));
        // A program that cannot start leaves no child and no handle.
        assert!(spawn_held("/nonexistent/systemctl", &["--user"]).is_none());
        // Only stdin, stdout and stderr reach the child.
        let (mut listing, out) = spawn_held("/bin/sh", &["-c", "ls /proc/self/fd"]).unwrap();
        assert!(listing.wait().unwrap().success());
        let mut text = String::new();
        File::from(out).read_to_string(&mut text).unwrap();
        let open: Vec<&str> = text.split_whitespace().collect();
        // ls itself opens one directory descriptor to list it.
        assert!(
            open.len() <= 4 && ["0", "1", "2"].iter().all(|fd| open.contains(fd)),
            "{open:?}"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_child_reaped_elsewhere_is_never_signalled() {
        // Reaped by another waiter by its number (as an auto-reaping policy or
        // another reaper would): waiting through the pidfd reports the loss and
        // the deadline path signals nothing.
        let (mut helper, _out) = spawn_held("/bin/sh", &["-c", "exit 0"]).unwrap();
        let pid = helper.pid().unwrap() as libc::pid_t;
        // SAFETY: waitpid on this test's own child.
        assert_eq!(unsafe { libc::waitpid(pid, std::ptr::null_mut(), 0) }, pid);
        assert!(helper.try_wait().is_err());
        assert_eq!(helper.kill().unwrap_err().raw_os_error(), Some(libc::ESRCH));
        assert_eq!(
            drain(
                &mut helper,
                &mut io::empty(),
                Instant::now(),
                Duration::ZERO
            ),
            None
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_bounded_read_answers_kills_or_refuses() {
        let sh = |script: &str| bounded_read("/bin/sh", &["-c", script], Duration::from_secs(5));
        assert_eq!(
            sh("printf 'ActiveState=active\\n'").as_deref(),
            Some("ActiveState=active\n")
        );
        assert_eq!(sh("printf partial; exit 3"), None, "a failed show");
        assert_eq!(
            bounded_read("/nonexistent/systemctl", &[], Duration::from_secs(5)),
            None,
            "no systemctl"
        );
        let begun = Instant::now();
        let mut pid = 0;
        let read = bounded_read_with(
            "/bin/sh",
            &["-c", "exec sleep 30"],
            Duration::from_millis(200),
            |id| pid = id,
            || false,
        );
        assert_eq!(read, None);
        assert!(
            begun.elapsed() < Duration::from_secs(5),
            "the bound ends a hung show"
        );
        // Killed and reaped: not even a zombie keeps its PID.
        // SAFETY: signal 0 only checks that the process exists.
        let alive = unsafe { libc::kill(pid as libc::pid_t, 0) };
        assert_eq!(
            (alive, io::Error::last_os_error().raw_os_error()),
            (-1, Some(libc::ESRCH))
        );
    }

    /// Scripted effects for `start`.
    #[derive(Default)]
    struct Script {
        lock_error: Option<String>,
        forwards: VecDeque<bool>,
        services: VecDeque<Service>,
        invocation: Option<u128>,
        claim_error: Option<String>,
        elapsed: Duration,
        locked: bool,
        log: Vec<String>,
    }

    impl Start for Script {
        fn lock(&mut self, _wait: Duration) -> Result<(), String> {
            self.log.push("lock".into());
            match &self.lock_error {
                Some(error) => Err(error.clone()),
                None => {
                    self.locked = true;
                    Ok(())
                }
            }
        }
        fn unlock(&mut self) {
            if self.locked {
                self.log.push("unlock".into());
            }
            self.locked = false;
        }
        fn forward(&mut self, activation: Activation) -> bool {
            let reached = self.forwards.pop_front().unwrap_or(false);
            self.log.push(format!("forward {activation:?} {reached}"));
            reached
        }
        fn service(&mut self) -> Service {
            self.services.pop_front().unwrap_or_default()
        }
        fn own_invocation(&self) -> Option<u128> {
            self.invocation
        }
        fn claim(&mut self) -> io::Result<()> {
            self.log.push("claim".into());
            match &self.claim_error {
                Some(error) => Err(io::Error::other(error.clone())),
                None => Ok(()),
            }
        }
        fn pause(&mut self, pause: Duration) {
            self.elapsed += pause;
        }
        fn elapsed(&self) -> Duration {
            self.elapsed
        }
        fn report(&mut self, line: &str) {
            self.log.push(format!("report {line}"));
        }
    }

    #[test]
    fn the_units_own_start_through_a_wrapper_claims_at_once() {
        let mut script = Script {
            services: [Service {
                state: ServiceState::Running,
                pid: 500,
                invocation: Some(7),
            }]
            .into(),
            invocation: Some(7),
            ..Script::default()
        };
        assert_eq!(
            start(&mut script, Activation::Show, 100, &fast()),
            Outcome::Claimed
        );
        assert_eq!(script.elapsed, Duration::ZERO, "no wait on its own socket");
        assert!(
            !script.log.iter().any(|line| line.starts_with("report")),
            "{:?}",
            script.log
        );
    }

    #[test]
    fn a_start_waits_for_the_starting_service_then_forwards() {
        let mut script = Script {
            forwards: [false, false, true].into(),
            services: [
                Service {
                    state: ServiceState::Starting,
                    pid: 0,
                    invocation: None,
                },
                Service {
                    state: ServiceState::Running,
                    pid: 500,
                    invocation: None,
                },
            ]
            .into(),
            ..Script::default()
        };
        assert_eq!(
            start(&mut script, Activation::Sessions, 100, &fast()),
            Outcome::Forwarded
        );
        assert!(
            !script.log.iter().any(|line| line == "claim"),
            "{:?}",
            script.log
        );
        assert_eq!(
            script
                .log
                .iter()
                .filter(|line| line.starts_with("report waiting"))
                .count(),
            1,
            "reported once"
        );
        assert!(!script.locked, "the lock is released");
        // Each pause leaves the lock to other starts, as Qt's `lock.unlock()` does.
        let locking: Vec<&str> = script
            .log
            .iter()
            .map(String::as_str)
            .filter(|line| *line == "lock" || *line == "unlock")
            .collect();
        assert_eq!(
            locking,
            ["lock", "unlock", "lock", "unlock", "lock", "unlock"]
        );
    }

    #[test]
    fn the_services_own_process_claims_at_once() {
        let mut script = Script {
            services: [Service {
                state: ServiceState::Running,
                pid: 100,
                invocation: None,
            }]
            .into(),
            ..Script::default()
        };
        assert_eq!(
            start(&mut script, Activation::Show, 100, &fast()),
            Outcome::Claimed
        );
        assert_eq!(
            script.log,
            ["lock", "forward Show false", "claim", "unlock"]
        );
    }

    #[test]
    fn a_service_that_never_listens_is_given_up_after_the_wait() {
        let mut script = Script {
            services: std::iter::repeat_n(
                Service {
                    state: ServiceState::Running,
                    pid: 500,
                    invocation: None,
                },
                100,
            )
            .collect(),
            ..Script::default()
        };
        assert_eq!(
            start(&mut script, Activation::Show, 100, &fast()),
            Outcome::Claimed
        );
        assert!(
            script.log.iter().any(|line| line
                == "report the Zerus service did not open its socket within 0.4 s; starting here"),
            "{:?}",
            script.log
        );
    }

    #[test]
    fn a_lock_that_cannot_be_taken_is_reported_once_and_skipped() {
        let mut script = Script {
            lock_error: Some("another start keeps x locked; starting without it".into()),
            services: [Service {
                state: ServiceState::Starting,
                pid: 0,
                invocation: None,
            }]
            .into(),
            forwards: [false, true].into(),
            ..Script::default()
        };
        assert_eq!(
            start(&mut script, Activation::Show, 100, &fast()),
            Outcome::Forwarded
        );
        assert_eq!(script.log.iter().filter(|line| *line == "lock").count(), 1);
    }

    #[test]
    fn a_failed_listen_is_reported() {
        let mut script = Script {
            claim_error: Some("address in use".into()),
            ..Script::default()
        };
        assert_eq!(
            start(&mut script, Activation::Show, 100, &fast()),
            Outcome::Failed("address in use".into())
        );
    }

    #[cfg(feature = "live")]
    #[test]
    fn delegation_challenge_leaves_show_and_sessions_on_the_original_socket() {
        use zerus_client_core::desktop::{self, Build, Observation, Responder};
        let dir = scratch("delegation");
        let paths = paths("zerus-test", &dir, &dir);
        let responder = Responder::new(
            "com.hgdev.hgs-tray".into(),
            Build {
                version: "1.2.3".into(),
                source_commit: "b".repeat(40),
                source_dirty: Some(false),
            },
            "22222222-2222-4222-8222-222222222222".into(),
            true,
            false,
        );
        assert!(responder.configure_once("a".repeat(64)));
        let reply: Reply = std::sync::Arc::new(move |stream, line| {
            desktop::same_user(stream)
                .then(|| responder.reply(line))
                .flatten()
        });
        let Claim::Claimed(first) = claim_with_reply(
            &paths,
            None,
            Activation::Show,
            &LockInfo::own(),
            &fast(),
            Some(reply),
        ) else {
            panic!("first claims")
        };
        assert!(matches!(
            desktop::probe_at(
                &paths.socket,
                "11111111-1111-4111-8111-111111111111",
                &"a".repeat(64)
            ),
            Observation::LiveNext { .. }
        ));
        assert!(
            first.activations.try_recv().is_err(),
            "a query never activates the window"
        );
        for activation in [Activation::Show, Activation::Sessions] {
            assert!(matches!(
                claim_at(&paths, None, activation.clone(), &LockInfo::own(), &fast()),
                Claim::Forwarded
            ));
            assert_eq!(
                first
                    .activations
                    .recv_timeout(Duration::from_secs(2))
                    .unwrap(),
                activation
            );
        }
        assert!(paths.socket.exists());
    }

    #[test]
    fn a_second_start_forwards_its_line_to_the_first() {
        let dir = scratch("forward");
        let paths = paths("zerus-test", &dir, &dir);
        let own = LockInfo::own();
        let Claim::Claimed(first) = claim_at(&paths, None, Activation::Show, &own, &fast()) else {
            panic!("the first start claims")
        };
        assert!(matches!(
            claim_at(&paths, None, Activation::Sessions, &own, &fast()),
            Claim::Forwarded
        ));
        assert_eq!(
            first
                .activations
                .recv_timeout(Duration::from_secs(5))
                .unwrap(),
            Activation::Sessions
        );
        assert!(!paths.lock.exists(), "the start lock is released");
        assert_eq!(
            fs::metadata(&paths.socket).unwrap().permissions().mode() & 0o777,
            0o600,
            "only this user may connect"
        );
        // The exit hook's path: the app state hands the socket back.
        let state = InstanceState(Mutex::new(Some(first.listening)));
        state.release();
        assert!(!paths.socket.exists(), "quitting removes the socket");
        state.release();
    }

    #[test]
    fn quitting_keeps_a_socket_another_instance_put_there() {
        let dir = scratch("replaced");
        let paths = paths("zerus-test", &dir, &dir);
        let Claim::Claimed(first) =
            claim_at(&paths, None, Activation::Show, &LockInfo::own(), &fast())
        else {
            panic!("the first start claims")
        };
        fs::remove_file(&paths.socket).unwrap();
        let _replacement = UnixListener::bind(&paths.socket).unwrap();
        InstanceState(Mutex::new(Some(first.listening))).release();
        assert!(paths.socket.exists(), "the replacement's socket stays");
    }

    #[test]
    fn a_crashed_instance_socket_is_claimed() {
        let dir = scratch("crashed");
        let paths = paths("zerus-test", &dir, &dir);
        // Bound but never listening, so the file refuses connections as a
        // crashed instance's does. A listener would not: a child that another
        // test forks meanwhile, with or without `PROCESS_TREE`, keeps a copy
        // listening until it execs (zerus-cqbmw).
        let socket =
            socket2::Socket::new(socket2::Domain::UNIX, socket2::Type::STREAM, None).unwrap();
        socket
            .bind(&socket2::SockAddr::unix(&paths.socket).unwrap())
            .unwrap();
        drop(socket);
        assert!(paths.socket.exists(), "a socket file nobody listens on");
        let Claim::Claimed(claimed) =
            claim_at(&paths, None, Activation::Show, &LockInfo::own(), &fast())
        else {
            panic!("a crashed instance's socket is replaced")
        };
        assert!(forward(&paths.socket, Activation::Sessions));
        assert_eq!(
            claimed
                .activations
                .recv_timeout(Duration::from_secs(5))
                .unwrap(),
            Activation::Sessions
        );
    }

    #[test]
    fn a_dead_holders_lock_is_taken() {
        let dir = scratch("dead");
        let lock = dir.join("zerus-test-start.lock");
        let own = LockInfo::own();
        fs::write(
            &lock,
            LockInfo {
                pid: exited_pid(),
                app: own.app.clone(),
                host: own.host.clone(),
            }
            .contents(),
        )
        .unwrap();
        assert!(matches!(try_lock(&lock, &own), LockAttempt::Taken(_)));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn a_linux_holder_is_named_as_qlockfile_names_it() {
        let info = LockInfo {
            pid: 42,
            app: "hgs-tray".into(),
            host: "arch".into(),
        };
        let young = Duration::from_secs(1);
        let replaced = |_| exe_name(Ok("/opt/zerus/hgs-tray (deleted)".into()));
        assert_eq!(replaced(0), Some(Some("hgs-tray".to_owned())));
        assert!(
            !is_stale(Some(&info), young, "arch", replaced),
            "a holder whose binary was replaced still runs"
        );
        let unreadable = |_| exe_name(Err(io::Error::from(io::ErrorKind::PermissionDenied)));
        assert!(
            is_stale(Some(&info), young, "arch", unreadable),
            "an unreadable executable link counts as gone"
        );
    }

    #[cfg(target_os = "linux")]
    fn check_deleted_holder(
        fixture: &str,
        start: impl FnOnce(&Path) -> std::process::Child,
        mut before_exec: impl FnMut(&mut std::process::Child),
    ) {
        use std::os::unix::fs::MetadataExt;
        struct Owned(std::process::Child);
        impl Drop for Owned {
            fn drop(&mut self) {
                if self.0.try_wait().unwrap().is_none() {
                    self.0.kill().unwrap();
                }
                self.0.wait().unwrap();
            }
        }
        let dir = scratch(fixture);
        let holder = dir.join("zci-holder");
        // Shared, as every test that starts children holds it (`lib.rs`).
        let _tree = crate::PROCESS_TREE
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // `cp` writes the copy, so this process never holds it open for
        // writing. A child that another test forks meanwhile, with or without
        // `PROCESS_TREE`, would keep such a descriptor until it execs, and
        // starting the copy then fails with ETXTBSY (zerus-cqbmw).
        let copied = Command::new("cp")
            .arg("/bin/sleep")
            .arg(&holder)
            .stdin(Stdio::null())
            .status()
            .unwrap();
        assert!(copied.success(), "cp /bin/sleep: {copied}");
        let mut child = Owned(start(&holder));
        let expected = fs::metadata(&holder).unwrap();
        let until = Instant::now() + Duration::from_secs(3);
        // Spawn completion is not the fixture's native executable observation.
        // Keep the file until this original Child actually runs its object.
        loop {
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "holder exited before exec"
            );
            if fs::metadata(format!("/proc/{}/exe", child.0.id()))
                .is_ok_and(|seen| (seen.dev(), seen.ino()) == (expected.dev(), expected.ino()))
            {
                break;
            }
            before_exec(&mut child.0);
            assert!(
                Instant::now() < until,
                "original holder executable was not observed"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        fs::remove_file(&holder).unwrap();
        let pid = i64::from(child.0.id());
        let name = process_name(pid);
        let own = LockInfo::own();
        let info = LockInfo {
            pid,
            app: "zci-holder".into(),
            host: own.host.clone(),
        };
        let stale = is_stale(Some(&info), Duration::from_secs(1), &own.host, process_name);
        let _ = child.0.kill();
        let _ = child.0.wait();
        assert_eq!(
            name,
            Some(Some("zci-holder".to_owned())),
            "the deleted binary keeps its name"
        );
        assert!(!stale, "its holder still runs");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_linux_holder_with_a_deleted_binary_runs_and_an_unreadable_one_is_gone() {
        check_deleted_holder(
            "deleted",
            |holder| {
                Command::new(holder)
                    .arg("30")
                    .stdin(Stdio::null())
                    .spawn()
                    .unwrap()
            },
            |_| {},
        );
        // SAFETY: geteuid cannot fail.
        if unsafe { libc::geteuid() } != 0 {
            let _tree = crate::PROCESS_TREE
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            // PID 1 can be our own readable namespace init in local CI. Use
            // an original live, non-dumpable child for the permission refusal.
            let mut unreadable = Command::new("python3")
                .args([
                    "-I",
                    "-c",
                    "import ctypes,os,time; assert ctypes.CDLL(None).prctl(4,0,0,0,0)==0; os.write(1,b'ready'); time.sleep(30)",
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            let mut ready = [0; 5];
            let started = unreadable.stdout.take().unwrap().read_exact(&mut ready);
            let before = unreadable.try_wait();
            let link = fs::read_link(format!("/proc/{}/exe", unreadable.id()));
            let observed = process_name(i64::from(unreadable.id()));
            let after = unreadable.try_wait();
            let _ = unreadable.kill();
            unreadable.wait().unwrap();
            started.unwrap();
            assert_eq!(&ready, b"ready");
            assert!(before.unwrap().is_none() && after.unwrap().is_none());
            assert_eq!(link.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
            assert_eq!(
                observed, None,
                "the original live child's executable link is unreadable"
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn deleted_holder_waits_for_the_original_exec_before_unlinking() {
        let mut released = false;
        check_deleted_holder(
            "exec-gate",
            |holder| {
                Command::new("/bin/sh")
                    .args(["-c", "read -r gate || exit 71; exec \"$1\" 30", "fixture"])
                    .arg(holder)
                    .stdin(Stdio::piped())
                    .spawn()
                    .unwrap()
            },
            |child| {
                if !released {
                    child.stdin.take().unwrap().write_all(b"go\n").unwrap();
                    released = true;
                }
            },
        );
        assert!(
            released,
            "the original was observed before its fixture exec"
        );
    }

    /// A PID whose process has exited and been reaped.
    fn exited_pid() -> i64 {
        let _tree = crate::PROCESS_TREE
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut child = Command::new("true").spawn().unwrap();
        let pid = i64::from(child.id());
        child.wait().unwrap();
        pid
    }

    #[test]
    fn a_dead_removers_guard_does_not_keep_a_stale_lock() {
        let dir = scratch("guard");
        let lock = dir.join("zerus-test-start.lock");
        let guard = dir.join("zerus-test-start.lock.rmlock");
        let own = LockInfo::own();
        let dead = LockInfo {
            pid: exited_pid(),
            app: own.app.clone(),
            host: own.host.clone(),
        }
        .contents();
        fs::write(&lock, &dead).unwrap();
        fs::write(&guard, &dead).unwrap();
        assert!(
            matches!(try_lock(&lock, &own), LockAttempt::Taken(_)),
            "QLockFile judges its .rmlock like any lock file"
        );
        assert!(!guard.exists() && !lock.exists());
    }

    #[test]
    fn a_lock_older_or_newer_than_30_s_is_stale() {
        let dir = scratch("age");
        let lock = dir.join("zerus-test-start.lock");
        let own = LockInfo::own();
        fs::write(&lock, own.contents()).unwrap();
        assert!(!stale_now(&lock, &own.host), "this process holds it");
        let file = File::options().write(true).open(&lock).unwrap();
        file.set_modified(SystemTime::now() - Duration::from_secs(40))
            .unwrap();
        assert!(stale_now(&lock, &own.host), "QLockFile's 30 s");
        file.set_modified(SystemTime::now() + Duration::from_secs(40))
            .unwrap();
        assert!(
            stale_now(&lock, &own.host),
            "QLockFile compares the absolute age"
        );
    }

    #[test]
    fn only_a_complete_line_activates() {
        let dir = scratch("line");
        let paths = paths("zerus-test", &dir, &dir);
        let Claim::Claimed(claimed) =
            claim_at(&paths, None, Activation::Show, &LockInfo::own(), &fast())
        else {
            panic!("the first start claims")
        };
        let mut unfinished = UnixStream::connect(&paths.socket).unwrap();
        unfinished.write_all(b"sessions").unwrap();
        drop(unfinished);
        assert!(forward(&paths.socket, Activation::Show));
        assert_eq!(
            claimed
                .activations
                .recv_timeout(Duration::from_secs(5))
                .unwrap(),
            Activation::Show,
            "Qt acts on a line only once canReadLine"
        );
    }

    #[test]
    fn a_slow_client_holds_the_next_start_back_at_most_a_second() {
        let dir = scratch("drip");
        let paths = paths("zerus-test", &dir, &dir);
        let Claim::Claimed(claimed) =
            claim_at(&paths, None, Activation::Show, &LockInfo::own(), &fast())
        else {
            panic!("the first start claims")
        };
        let mut slow = UnixStream::connect(&paths.socket).unwrap();
        let started = Instant::now();
        // 1.8 s for the whole line, each byte well within a 1 s read timeout.
        let dripping = std::thread::spawn(move || {
            for byte in b"sessions\n" {
                if slow.write_all(&[*byte]).is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(200));
            }
        });
        assert!(forward(&paths.socket, Activation::Show));
        let first = claimed
            .activations
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        let waited = started.elapsed();
        dripping.join().unwrap();
        assert_eq!(
            first,
            Activation::Show,
            "the slow line is dropped at its deadline"
        );
        assert!(
            waited < Duration::from_millis(1700),
            "the next start waited {waited:?}"
        );
    }

    #[test]
    fn a_live_holders_lock_is_busy_until_released() {
        let dir = scratch("live");
        let lock = dir.join("zerus-test-start.lock");
        let own = LockInfo::own();
        let LockAttempt::Taken(held) = try_lock(&lock, &own) else {
            panic!("the first attempt takes it")
        };
        assert!(matches!(try_lock(&lock, &own), LockAttempt::Busy));
        drop(held);
        assert!(!lock.exists());
        assert!(matches!(try_lock(&lock, &own), LockAttempt::Taken(_)));
    }
}
