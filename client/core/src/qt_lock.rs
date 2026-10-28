//! Qt's QLockFile (qtbase 6.12 src/corelib/io/qlockfile.cpp and
//! qlockfile_unix.cpp, qlockfile_win.cpp), for a folder the Qt client and
//! this client share: either can hold the lock, and each judges the
//! other's leftover lock stale by Qt's rule.
//!
//! A lock file holds five lines: the PID, the process name Qt derives for
//! it, the host name, QSysInfo::machineUniqueId() and bootUniqueId(). The
//! holder keeps it open, on Unix with flock(LOCK_EX), on Windows with read
//! sharing only, so a lock judged stale is removed only when nothing holds
//! it. With no stale age (Qt's setStaleLockTime(0)) a lock is stale only
//! when its process is gone, another process took the PID, or the machine
//! booted since.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// What a lock file says about its holder (QLockFilePrivate::LockFileInfo).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockInfo {
    pub pid: i64,
    pub appname: String,
    pub hostname: String,
    pub hostid: Vec<u8>,
    pub bootid: Vec<u8>,
}

/// This machine as Qt names it in lock files.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Machine {
    /// QSysInfo::machineHostName() (COMPUTERNAME on Windows).
    pub hostname: String,
    /// QSysInfo::machineUniqueId(); empty when unknown.
    pub hostid: Vec<u8>,
    /// QSysInfo::bootUniqueId(); empty when unknown.
    pub bootid: Vec<u8>,
}

/// One line without its newline, as QIODevice::readLine then chop(1) gives it.
fn lines(bytes: &[u8]) -> impl Iterator<Item = &[u8]> {
    bytes
        .split_inclusive(|byte| *byte == b'\n')
        .map(|line| &line[..line.len() - 1])
}

/// QLockFilePrivate::getLockInfo_helper: None without a positive PID.
pub fn parse(bytes: &[u8]) -> Option<LockInfo> {
    let mut lines = lines(bytes);
    let pid: i64 = std::str::from_utf8(lines.next()?).ok()?.parse().ok()?;
    if pid <= 0 {
        return None;
    }
    let mut next = || lines.next().map(<[u8]>::to_vec).unwrap_or_default();
    let appname = String::from_utf8_lossy(&next()).into_owned();
    let hostname = String::from_utf8_lossy(&next()).into_owned();
    Some(LockInfo {
        pid,
        appname,
        hostname,
        hostid: next(),
        bootid: next(),
    })
}

/// QLockFilePrivate::lockFileContents.
pub fn contents(pid: i64, appname: &str, machine: &Machine) -> Vec<u8> {
    let mut bytes = format!("{pid}\n{appname}\n{}\n", machine.hostname).into_bytes();
    bytes.extend_from_slice(&machine.hostid);
    bytes.push(b'\n');
    bytes.extend_from_slice(&machine.bootid);
    bytes.push(b'\n');
    bytes
}

/// QLockFilePrivate::isApparentlyStale with no stale age: on this machine
/// (by host ID when both know one, else by name), a lock from an earlier
/// boot, or whose process `running` denies, is stale. An unreadable lock
/// and another machine's lock never are.
pub fn apparently_stale(
    info: Option<&LockInfo>,
    here: &Machine,
    running: impl Fn(i64, &str) -> bool,
) -> bool {
    let Some(info) = info else {
        return false;
    };
    let mut same_host = info.hostname.is_empty() || info.hostname == here.hostname;
    if !info.hostid.is_empty() && !here.hostid.is_empty() {
        same_host = here.hostid == info.hostid;
    }
    if !same_host {
        return false;
    }
    if !info.bootid.is_empty() && info.bootid != here.bootid {
        return true;
    }
    !running(info.pid, &info.appname)
}

/// QLockFilePrivate::processNameByPid: the name Qt derives for a process,
/// None when it cannot tell (no name check then). On Linux a PID whose
/// executable cannot be read is named "/ERROR/", as Qt names it.
pub fn process_name(pid: i64) -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let target = match fs::read_link(format!("/proc/{pid}/exe")) {
            Ok(target) => target,
            Err(_) => return Some("/ERROR/".into()),
        };
        let text = target.to_string_lossy();
        let text = text.strip_suffix(" (deleted)").unwrap_or(&text);
        Some(text.rsplit('/').next().unwrap_or("").to_owned())
    }
    #[cfg(target_os = "macos")]
    {
        let mut name = [0u8; 1024];
        // SAFETY: the buffer is valid for its length.
        let length = unsafe {
            libc::proc_name(
                pid as libc::c_int,
                name.as_mut_ptr().cast(),
                name.len() as u32,
            )
        };
        let length = usize::try_from(length).unwrap_or(0).min(name.len());
        Some(String::from_utf8_lossy(&name[..length]).into_owned())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = pid;
        None
    }
}

/// QLockFilePrivate::isProcessRunning.
pub fn process_running(pid: i64, appname: &str) -> bool {
    #[cfg(unix)]
    {
        // SAFETY: signal 0 only checks for the process.
        if unsafe { libc::kill(pid as libc::pid_t, 0) } == -1
            && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
        {
            return false;
        }
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
        use windows_sys::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_INFORMATION,
        };
        // SAFETY: the handle is checked and closed.
        let running = unsafe {
            let handle = OpenProcess(PROCESS_QUERY_INFORMATION, 0, pid as u32);
            if handle.is_null() {
                return false;
            }
            let mut code = 0u32;
            let known = GetExitCodeProcess(handle, &mut code) != 0;
            CloseHandle(handle);
            known && code == STILL_ACTIVE as u32
        };
        if !running {
            return false;
        }
    }
    match process_name(pid) {
        Some(name) if !name.is_empty() => {
            // Qt compares with the stored name's last path part.
            let stored = appname.rsplit(['/', '\\']).next().unwrap_or(appname);
            name == stored
        }
        _ => true,
    }
}

/// This process's name as Qt would write it (processNameByPid of our PID).
pub fn own_name() -> String {
    if let Some(name) = process_name(i64::from(std::process::id()))
        .filter(|name| !name.is_empty() && name != "/ERROR/")
    {
        return name;
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| {
            exe.file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
        })
        .unwrap_or_default()
}

/// This machine as Qt names it.
pub fn this_machine() -> Machine {
    #[cfg(target_os = "linux")]
    {
        // SAFETY: uname fills the struct it is given.
        let hostname = unsafe {
            let mut name: libc::utsname = std::mem::zeroed();
            if libc::uname(&mut name) == 0 {
                std::ffi::CStr::from_ptr(name.nodename.as_ptr())
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            }
        };
        let first = |path: &str, length: usize| {
            let mut bytes = Vec::new();
            File::open(path)
                .and_then(|file| file.take(length as u64).read_to_end(&mut bytes))
                .ok()
                .map(|_| bytes)
        };
        let hostid = first("/var/lib/dbus/machine-id", 32)
            .or_else(|| first("/usr/local/var/lib/dbus/machine-id", 32))
            .unwrap_or_default();
        let bootid = first("/proc/sys/kernel/random/boot_id", 36)
            .filter(|id| id.len() == 36)
            .unwrap_or_default();
        Machine {
            hostname,
            hostid,
            bootid,
        }
    }
    #[cfg(target_os = "macos")]
    {
        let mut name = [0u8; 256];
        // SAFETY: the buffer is valid for its length.
        let hostname = if unsafe { libc::gethostname(name.as_mut_ptr().cast(), name.len()) } == 0 {
            let end = name
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(name.len());
            String::from_utf8_lossy(&name[..end]).into_owned()
        } else {
            String::new()
        };
        let mut uuid = [0u8; 37];
        let mut length = uuid.len();
        // SAFETY: the name is a C string and the buffer is valid for `length`.
        let found = unsafe {
            libc::sysctlbyname(
                c"kern.bootsessionuuid".as_ptr(),
                uuid.as_mut_ptr().cast(),
                &mut length,
                std::ptr::null_mut(),
                0,
            )
        } == 0
            && length == uuid.len();
        // The platform UUID needs IOKit; without it Qt's host name rule applies.
        Machine {
            hostname,
            hostid: Vec::new(),
            bootid: if found {
                uuid[..36].to_vec()
            } else {
                Vec::new()
            },
        }
    }
    #[cfg(windows)]
    {
        Machine {
            hostname: std::env::var("COMPUTERNAME").unwrap_or_default(),
            hostid: Vec::new(),
            bootid: Vec::new(),
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        Machine::default()
    }
}

/// A held lock; dropping it removes the file, as QLockFile::unlock does.
pub struct QtLock {
    path: PathBuf,
    file: Option<File>,
}

impl Drop for QtLock {
    fn drop(&mut self) {
        // Unlinked while still held, then closed (releaseLockFile).
        let _ = fs::remove_file(&self.path);
        self.file.take();
    }
}

enum Attempt {
    Held(File),
    Busy,
    Failed(io::Error),
}

#[cfg(unix)]
fn native_lock(file: &File) -> bool {
    use std::os::fd::AsRawFd;
    // SAFETY: the descriptor belongs to `file`.
    let locked = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0;
    // Only a lock someone holds fails (setNativeLocks); no lock support does not.
    locked
        || !matches!(
            io::Error::last_os_error().raw_os_error(),
            Some(libc::EWOULDBLOCK) | Some(libc::EACCES)
        )
}

/// QLockFilePrivate::tryLock_sys.
fn try_lock(path: &Path, bytes: &[u8]) -> Attempt {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o666);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_SHARE_READ: nobody deletes it while it is held.
        options.share_mode(0x1);
    }
    let mut file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => return Attempt::Busy,
        #[cfg(windows)]
        Err(error) if error.raw_os_error() == Some(32) => return Attempt::Busy,
        Err(error) => return Attempt::Failed(error),
    };
    #[cfg(unix)]
    if !native_lock(&file) {
        return Attempt::Busy;
    }
    if file
        .write_all(bytes)
        .and_then(|()| file.sync_data())
        .is_err()
    {
        let _ = fs::remove_file(path);
        return Attempt::Failed(io::Error::other("the lock file could not be written"));
    }
    #[cfg(unix)]
    {
        // Still the file we wrote: not replaced from under us.
        use std::os::unix::fs::MetadataExt;
        let ours = file.metadata().ok();
        let named = fs::metadata(path).ok();
        match (ours, named) {
            (Some(ours), Some(named)) if ours.dev() == named.dev() && ours.ino() == named.ino() => {
            }
            _ => return Attempt::Busy,
        }
    }
    Attempt::Held(file)
}

/// QLockFilePrivate::removeStaleLock: only a lock nobody holds goes.
fn remove_stale(path: &Path) -> bool {
    #[cfg(unix)]
    {
        let file = match OpenOptions::new().write(true).open(path) {
            Ok(file) => file,
            Err(error) => return error.kind() == io::ErrorKind::NotFound,
        };
        if !native_lock(&file) {
            return false;
        }
        let removed = fs::remove_file(path).is_ok();
        drop(file);
        removed
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // No sharing and delete on close: fails while Qt holds it.
        match OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .custom_flags(0x0400_0000)
            .open(path)
        {
            Ok(file) => {
                drop(file);
                true
            }
            Err(error) => error.kind() == io::ErrorKind::NotFound,
        }
    }
}

fn read_info(path: &Path) -> Option<LockInfo> {
    let mut bytes = Vec::new();
    File::open(path)
        .ok()?
        .take(64 * 1024)
        .read_to_end(&mut bytes)
        .ok()?;
    parse(&bytes)
}

/// QLockFile::tryLock(timeout) with no stale age: waits up to `timeout`,
/// removing a stale lock under the `.rmlock` guard as Qt does. Err when the
/// lock could not be created at all; Ok(None) when someone holds it.
pub fn lock(path: &Path, timeout: Duration) -> io::Result<Option<QtLock>> {
    let deadline = Instant::now() + timeout;
    let bytes = contents(i64::from(std::process::id()), &own_name(), &this_machine());
    let mut sleep = Duration::from_millis(100);
    loop {
        match try_lock(path, &bytes) {
            Attempt::Held(file) => {
                return Ok(Some(QtLock {
                    path: path.to_owned(),
                    file: Some(file),
                }))
            }
            Attempt::Failed(error) => return Err(error),
            Attempt::Busy => {
                let here = this_machine();
                if apparently_stale(read_info(path).as_ref(), &here, process_running) {
                    let guard = path.with_file_name(format!(
                        "{}.rmlock",
                        path.file_name()
                            .map(|name| name.to_string_lossy())
                            .unwrap_or_default()
                    ));
                    if let Attempt::Held(file) = try_lock(&guard, &bytes) {
                        let held = QtLock {
                            path: guard,
                            file: Some(file),
                        };
                        let removed =
                            apparently_stale(read_info(path).as_ref(), &here, process_running)
                                && remove_stale(path);
                        drop(held);
                        if removed {
                            continue;
                        }
                    }
                }
            }
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Ok(None);
        }
        std::thread::sleep(sleep.min(left));
        if sleep < Duration::from_secs(5) {
            sleep *= 2;
        }
    }
}

#[cfg(test)]
mod tests;
