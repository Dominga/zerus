//! Scope release for an ordinary file exclusion acquired by this process.
//!
//! Acquisition, contention, identity and filesystem validation stay with the
//! caller. This is not a last-descriptor custody lease: intentional inherited
//! native/writer locks and Git removal loans must retain their separate types.
#![forbid(unsafe_code)]

use std::{fs::File, io};

/// Explicitly releases an acquired lock when its acquisition owner's scope ends.
/// A fork child's copy only closes its descriptor; it cannot unlock the parent.
pub struct OwnedLock {
    file: File,
    owner_pid: u32,
}

impl OwnedLock {
    /// Wrap immediately after this process acquires the file's lock, before any
    /// fallible work. This constructor does not acquire or validate a lock.
    pub fn acquired(file: File) -> Self {
        Self {
            file,
            owner_pid: std::process::id(),
        }
    }

    /// The retained object, for a caller's identity checks and file operations.
    pub fn file(&self) -> &File {
        &self.file
    }

    /// A fallible normal release. Even an error is final: Drop must not retry an
    /// unlock whose outcome the caller has already observed.
    pub fn release(mut self) -> io::Result<()> {
        let result = self.unlock_owner();
        self.owner_pid = 0;
        result
    }

    fn unlock_owner(&self) -> io::Result<()> {
        if self.owner_pid != std::process::id() {
            return Ok(());
        }
        #[cfg(any(test, feature = "test-support"))]
        {
            testing::UNLOCK_ATTEMPTS.with(|attempts| attempts.set(attempts.get() + 1));
            if testing::FAIL_UNLOCK.with(std::cell::Cell::get) {
                return Err(io::Error::other("injected unlock failure"));
            }
        }
        // Name fs2 explicitly: File::unlock is a different standard-library API.
        fs2::FileExt::unlock(&self.file)
    }
}

impl Drop for OwnedLock {
    fn drop(&mut self) {
        let _ = self.unlock_owner();
    }
}

#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub mod testing {
    thread_local! {
        pub static UNLOCK_ATTEMPTS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
        pub static FAIL_UNLOCK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    pub fn set_owner_pid(lock: &mut super::OwnedLock, pid: u32) {
        lock.owner_pid = pid;
    }
}

#[cfg(test)]
mod tests;
