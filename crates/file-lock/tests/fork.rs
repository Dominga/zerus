#![cfg(unix)]

use std::{
    fs::File,
    io::{self, Read},
    os::{fd::AsRawFd, unix::net::UnixStream},
    time::Duration,
};
use zerus_file_lock::OwnedLock;

struct HeldFork {
    pid: libc::pid_t,
    control: UnixStream,
}

impl HeldFork {
    fn after(action: impl FnOnce()) -> Self {
        let (control, child) = UnixStream::pair().unwrap();
        // The child performs only the specified guard drop and async-signal-safe
        // close/read/write/_exit calls, never another Rust test or allocation.
        let pid = unsafe { libc::fork() };
        assert!(pid >= 0, "fork: {}", io::Error::last_os_error());
        if pid == 0 {
            action();
            unsafe {
                libc::close(control.as_raw_fd());
                if libc::write(child.as_raw_fd(), b"r".as_ptr().cast(), 1) != 1 {
                    libc::_exit(2);
                }
                let mut byte = 0u8;
                loop {
                    let n = libc::read(child.as_raw_fd(), (&mut byte as *mut u8).cast(), 1);
                    if n >= 0 {
                        libc::_exit(0);
                    }
                    if io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
                        libc::_exit(3);
                    }
                }
            }
        }
        drop(child);
        let mut held = Self { pid, control };
        held.control
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut byte = [0];
        held.control.read_exact(&mut byte).unwrap();
        assert_eq!(byte, *b"r");
        held
    }
}

impl Drop for HeldFork {
    fn drop(&mut self) {
        // Shutdown also wakes the child if a parallel fork copied this endpoint.
        let _ = self.control.shutdown(std::net::Shutdown::Both);
        let mut status = 0;
        loop {
            let result = unsafe { libc::waitpid(self.pid, &mut status, 0) };
            if result == self.pid {
                assert_eq!(status, 0);
                break;
            }
            assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::EINTR));
        }
    }
}

fn acquired(path: &std::path::Path) -> OwnedLock {
    let file = File::create(path).unwrap();
    fs2::FileExt::lock_exclusive(&file).unwrap();
    OwnedLock::acquired(file)
}

#[test]
fn owner_drop_releases_while_an_unrelated_fork_retains_the_description() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("lock");
    let owner = acquired(&path);
    let contender = File::open(&path).unwrap();
    let _held = HeldFork::after(|| {});
    assert!(fs2::FileExt::try_lock_exclusive(&contender).is_err());
    drop(owner);
    assert!(fs2::FileExt::try_lock_exclusive(&contender).is_ok());
}

#[test]
fn a_child_copy_drop_cannot_unlock_the_original_owner() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("lock");
    let mut owner = acquired(&path);
    let contender = File::open(&path).unwrap();
    // Only the child's distinct address space drops this copy. The parent's
    // value retains its ordinary destructor; the child exits without unwinding.
    let _held = HeldFork::after(|| unsafe { std::ptr::drop_in_place(&mut owner) });
    assert!(fs2::FileExt::try_lock_exclusive(&contender).is_err());
    drop(owner);
    assert!(fs2::FileExt::try_lock_exclusive(&contender).is_ok());
}

#[test]
fn an_early_error_releases_before_the_retained_description_closes() {
    fn failed(lock: OwnedLock) -> io::Result<()> {
        let _held = lock;
        Err(io::Error::other("private failure after acquisition"))
    }
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("lock");
    let owner = acquired(&path);
    let contender = File::open(&path).unwrap();
    let _held = HeldFork::after(|| {});
    assert!(failed(owner).is_err());
    assert!(fs2::FileExt::try_lock_exclusive(&contender).is_ok());
}
