use super::*;
use crate::task_start::capture::tests::{retry_busy, Fixture};
use std::{fs, io::Read, os::unix::net::UnixStream, time::Duration};

/// An exact owned child held before exec. Child-side code only closes the
/// optional metadata guard and uses async-signal-safe read/write/_exit calls.
pub(super) struct HeldFork {
    pid: libc::pid_t,
    control: UnixStream,
}
impl HeldFork {
    pub(super) fn new() -> Self {
        Self::after(|| {})
    }
    fn after(action: impl FnOnce()) -> Self {
        let (control, child) = UnixStream::pair().unwrap();
        let pid = unsafe { libc::fork() };
        assert!(pid >= 0, "fork: {}", io::Error::last_os_error());
        if pid == 0 {
            action();
            unsafe {
                libc::close(control.as_raw_fd());
                let fd = child.as_raw_fd();
                if libc::write(fd, b"r".as_ptr().cast(), 1) != 1 {
                    libc::_exit(2);
                }
                let mut byte = 0u8;
                loop {
                    let n = libc::read(fd, (&mut byte as *mut u8).cast(), 1);
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
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut byte = [0];
        held.control.read_exact(&mut byte).unwrap();
        assert_eq!(byte, *b"r");
        held
    }
}
impl Drop for HeldFork {
    fn drop(&mut self) {
        // Shutdown signals even if another parallel fork inherited this end.
        let _ = self.control.shutdown(std::net::Shutdown::Both);
        let mut status = 0;
        loop {
            let n = unsafe { libc::waitpid(self.pid, &mut status, 0) };
            if n == self.pid {
                assert_eq!(status, 0, "held child failed");
                break;
            }
            assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::EINTR));
        }
    }
}
fn prepared(f: &Fixture) -> Journal {
    retry_busy(|| {
        Journal::prepare(
            f.capture().unwrap(),
            &CheckId::fixture_initial(&f.intent.reconcile_id),
        )
    })
    .unwrap()
}

#[test]
fn fork_metadata_owner_drop_releases_only_the_metadata_exclusion() {
    let f = Fixture::new();
    let owner = prepared(&f);
    assert_ne!(
        unsafe { libc::fcntl(owner.lock.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC,
        0
    );
    let original = fs::read(&f.file).unwrap();
    let journal = fs::read(f.file.with_extension("reconcile")).unwrap();
    let _held = HeldFork::new();
    drop(owner);
    // No Busy retry: the unrelated child is deliberately still alive.
    let next = Journal::prepare(
        f.capture().unwrap(),
        &CheckId::fixture_initial(&f.intent.reconcile_id),
    )
    .unwrap();
    assert_eq!(next.saved().phase(), Phase::Prepared);
    assert_eq!(fs::read(&f.file).unwrap(), original);
    assert_eq!(
        fs::read(f.file.with_extension("reconcile")).unwrap(),
        journal
    );
    drop(next);
}

#[test]
fn fork_metadata_child_drop_cannot_unlock_the_original_owner() {
    let f = Fixture::new();
    let owner = prepared(&f);
    assert_eq!(owner.saved.slot.lock(false).err(), Some(Error::Busy));
    drop(owner);
    let key = &f.intent.binding.key;
    let slot = Slot::open(&f.store, &key.project, &key.task).unwrap();
    let mut lock = retry_busy(|| slot.lock(false)).unwrap();
    // SAFETY: only the fork child runs this closure, then _exit without Rust
    // destructors. The parent's distinct copy keeps normal RAII on every path.
    let _held = HeldFork::after(|| unsafe { std::ptr::drop_in_place(&mut lock) });
    assert_eq!(slot.lock(false).err(), Some(Error::Busy));
    drop(lock);
    assert!(slot.lock(false).is_ok());
}
