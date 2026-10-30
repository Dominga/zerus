//! Close-only, inherited flock custody for one host-admitted child exchange.
//! The host supplies an already verified private file; this module opens no path
//! and grants no operation, recovery or file-identity authority.
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::sync::Arc;

const FD_LIMIT: i32 = 1 << 20;
const ROSTER_LIMIT: usize = 8192;

/// A flock open-file description, released only when its last descriptor closes.
/// Do not wrap the input in a guard that explicitly unlocks on drop, or unlock
/// any duplicate. The host must retain this Arc with all four run_owned resources.
pub struct InheritedLock(File);

impl InheritedLock {
    /// Nonblocking exclusive acquisition. The supplied file must already have
    /// host-verified ownership, permissions, identity and durable ticket binding.
    /// Recovery must open the exact existing file, never create a missing one.
    pub fn try_lock(file: File) -> io::Result<Arc<Self>> {
        if !file.metadata()?.is_file() {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }
        // Atomic CLOEXEC duplication also keeps custody out of descriptors 0-2,
        // which Command replaces while arranging the child's standard streams.
        let fd = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 3) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: fcntl returned one new owned descriptor.
        let owned = unsafe { File::from_raw_fd(fd) };
        if unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Arc::new(Self(owned)))
    }

    /// Install the final pre-exec hook for the intended helper only. Call from
    /// the host's guarded spawn turn, without concurrently raising RLIMIT_NOFILE
    /// or adding later hooks which change descriptor inheritance. No parent
    /// descriptor flags change. All other non-stdio descriptors close on exec.
    ///
    /// This does not spawn or register a child. The host must durably prepare
    /// custody before spawning, then register identity before returning Child to
    /// run_owned (which starts writing stdin only after that return).
    pub fn inherit(self: &Arc<Self>, command: &mut Command) -> io::Result<()> {
        let end = descriptor_bound()?;
        let lock = self.clone();
        // SAFETY: only fcntl, close_range and errno access run after fork. The
        // closure allocates nothing and neither acquires locks nor drops Arc.
        // CLOEXEC, rather than close(), preserves Rust's exec-error pipe until
        // exec succeeds, so an exec failure still reaches Command::spawn.
        unsafe {
            command.pre_exec(move || whitelist(lock.as_raw_fd(), end));
        }
        Ok(())
    }
}

impl AsRawFd for InheritedLock {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}
// Intentionally no Drop impl / LOCK_UN: a child may still own the same flock.

fn descriptor_bound() -> io::Result<i32> {
    let mut limit = std::mem::MaybeUninit::<libc::rlimit>::uninit();
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, limit.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let limit = unsafe { limit.assume_init() }.rlim_cur;
    if limit > FD_LIMIT as libc::rlim_t {
        return Err(io::Error::from(io::ErrorKind::Unsupported));
    }
    // A descriptor opened before a soft-limit reduction can be above that
    // limit. Inspect only this process, with a bounded roster, before forking.
    let highest = highest_descriptor()?;
    let end = (limit as i32).max(highest + 1);
    if end > FD_LIMIT {
        return Err(io::Error::from(io::ErrorKind::Unsupported));
    }
    Ok(end)
}

#[cfg(target_os = "linux")]
fn highest_descriptor() -> io::Result<i32> {
    let mut highest = 2;
    for (count, entry) in std::fs::read_dir("/proc/self/fd")?.enumerate() {
        if count == ROSTER_LIMIT {
            return Err(io::Error::from(io::ErrorKind::Unsupported));
        }
        let fd = entry?
            .file_name()
            .to_str()
            .and_then(|text| text.parse::<i32>().ok())
            .ok_or(io::ErrorKind::InvalidData)?;
        if !(0..FD_LIMIT).contains(&fd) {
            return Err(io::Error::from(io::ErrorKind::Unsupported));
        }
        highest = highest.max(fd);
    }
    Ok(highest)
}

#[cfg(target_os = "macos")]
fn highest_descriptor() -> io::Result<i32> {
    let mut entries = vec![
        libc::proc_fdinfo {
            proc_fd: 0,
            proc_fdtype: 0
        };
        ROSTER_LIMIT + 1
    ];
    let bytes = std::mem::size_of_val(entries.as_slice());
    let used = unsafe {
        libc::proc_pidinfo(
            libc::getpid(),
            libc::PROC_PIDLISTFDS,
            0,
            entries.as_mut_ptr().cast(),
            bytes as i32,
        )
    };
    let width = std::mem::size_of::<libc::proc_fdinfo>();
    if used <= 0 || used as usize >= bytes || !(used as usize).is_multiple_of(width) {
        return Err(io::Error::from(io::ErrorKind::Unsupported));
    }
    let mut highest = 2;
    for entry in &entries[..used as usize / width] {
        if entry.proc_fd < 0 || entry.proc_fd >= FD_LIMIT {
            return Err(io::Error::from(io::ErrorKind::Unsupported));
        }
        highest = highest.max(entry.proc_fd);
    }
    Ok(highest)
}

fn whitelist(keep: RawFd, end: i32) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    if unsafe {
        libc::syscall(
            libc::SYS_close_range,
            3u32,
            u32::MAX,
            libc::CLOSE_RANGE_CLOEXEC,
        )
    } == 0
    {
        return set_cloexec(keep, false);
    }
    whitelist_fallback(keep, end)
}

fn whitelist_fallback(keep: RawFd, end: i32) -> io::Result<()> {
    // macOS and older Linux kernels: bounded, allocation-free fcntl loop.
    for fd in 3..end {
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        if flags < 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::EBADF) {
                continue;
            }
            return Err(error);
        }
        if unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    set_cloexec(keep, false)
}

fn set_cloexec(fd: RawFd, enabled: bool) -> io::Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0
        || unsafe {
            libc::fcntl(
                fd,
                libc::F_SETFD,
                if enabled {
                    flags | libc::FD_CLOEXEC
                } else {
                    flags & !libc::FD_CLOEXEC
                },
            )
        } < 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_whitelist_keeps_custody_and_exec_error_reporting() {
        // Make only the child's copy of an unrelated fd inheritable, then
        // exercise the fcntl fallback even on a current Linux kernel.
        let path =
            std::env::temp_dir().join(format!("hgs-custody-fallback-{}", std::process::id()));
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        let unrelated = file.try_clone().unwrap();
        let lock = InheritedLock::try_lock(file).unwrap();
        let keep = lock.as_raw_fd();
        let other = unrelated.as_raw_fd();
        let end = descriptor_bound().unwrap();
        let install = |command: &mut Command| unsafe {
            command.pre_exec(move || {
                set_cloexec(other, false)?;
                whitelist_fallback(keep, end)
            });
        };
        let mut command = Command::new("python3");
        command.args(["-c", "import os,sys\nos.fstat(int(sys.argv[1]))\ntry: os.fstat(int(sys.argv[2]))\nexcept OSError: sys.exit(0)\nsys.exit(9)"])
            .args([keep.to_string(), other.to_string()]);
        install(&mut command);
        assert!(command.status().unwrap().success());
        let mut missing = Command::new(path.join("no-executable"));
        install(&mut missing);
        assert_eq!(
            missing.spawn().unwrap_err().kind(),
            io::ErrorKind::NotADirectory
        );
        drop((command, missing, lock, unrelated));
        std::fs::remove_file(path).unwrap();
    }
}
