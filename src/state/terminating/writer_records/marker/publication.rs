//! One descriptor-relative metadata replacement, not an operation permit.
//! Recheck every original source AFTER staging, immediately before commit.
use super::*;
use crate::state::repositories::directory::Directory;
use std::{ffi::CString, fs::File, io::Write, os::fd::FromRawFd};

pub(super) fn replace(
    parent: &Directory,
    name: &str,
    bytes: &[u8],
    check: impl FnOnce() -> Result<()>,
) -> Result<()> {
    if name.is_empty() || name.contains(['/', '\\', ':']) || name == "." || name == ".." {
        return Err(super::super::REFUSED.into());
    }
    parent.verify()?;
    let staged = CString::new(format!(".hgs-marker-{}", uuid::Uuid::new_v4()))
        .map_err(|_| super::super::REFUSED)?;
    let target = CString::new(name).map_err(|_| super::super::REFUSED)?;
    let directory = parent.descriptor();
    // SAFETY: one new fixed leaf below the held original private directory.
    let raw = unsafe {
        libc::openat(
            directory,
            staged.as_ptr(),
            libc::O_RDWR
                | libc::O_CREAT
                | libc::O_EXCL
                | libc::O_CLOEXEC
                | libc::O_NOFOLLOW
                | libc::O_NONBLOCK,
            0o600 as libc::c_uint,
        )
    };
    if raw < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    // SAFETY: this call exclusively owns the descriptor just returned.
    let mut file = unsafe { File::from_raw_fd(raw) };
    let outcome = (|| {
        #[cfg(test)]
        fault("write")?;
        file.write_all(bytes).map_err(|e| e.to_string())?;
        #[cfg(test)]
        fault("file-sync")?;
        file.sync_all().map_err(|e| e.to_string())?;
        #[cfg(test)]
        AFTER_STAGE.with(|slot| {
            if let Some(change) = slot.borrow_mut().take() {
                change();
            }
        });
        parent.verify()?;
        check()?;
        #[cfg(test)]
        fault("rename")?;
        #[cfg(target_os = "linux")]
        // SAFETY: both NUL-free fixed leaves use the same held directory.
        let result =
            unsafe { libc::renameat2(directory, staged.as_ptr(), directory, target.as_ptr(), 0) };
        #[cfg(target_os = "macos")]
        // SAFETY: both NUL-free fixed leaves use the same held directory.
        let result = unsafe {
            libc::renameatx_np(directory, staged.as_ptr(), directory, target.as_ptr(), 0)
        };
        if result != 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        parent.sync()?;
        #[cfg(test)]
        AFTER_SYNC.with(|slot| {
            if let Some(change) = slot.borrow_mut().take() {
                change();
            }
        });
        parent.verify()
    })();
    // Removes only this call's own staging name. After rename it is absent;
    // uncertain durability never causes removal of the published target.
    unsafe { libc::unlinkat(directory, staged.as_ptr(), 0) };
    outcome
}

#[cfg(test)]
thread_local! {
    pub(super) static FAIL_AT: std::cell::Cell<Option<&'static str>> = const {std::cell::Cell::new(None)};
    pub(super) static AFTER_STAGE: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const {std::cell::RefCell::new(None)};
    pub(super) static AFTER_SYNC: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const {std::cell::RefCell::new(None)};
}

#[cfg(test)]
fn fault(phase: &str) -> Result<()> {
    if FAIL_AT.with(|slot| slot.get() == Some(phase)) {
        Err(format!("fixture marker {phase} failure"))
    } else {
        Ok(())
    }
}
