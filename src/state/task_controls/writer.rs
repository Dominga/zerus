//! Native-only exclusion. It cannot access the desktop DB or lend its guard
//! to an actor transaction. Workers must release it before returning evidence.
use super::stop_facts::verify_writer;
use crate::{
    platform,
    state::{repositories::directory::Directory, storage},
};
use fs2::FileExt;
use std::{
    path::Path,
    time::{Duration, Instant},
};
use zerus_desktop_ledger::task_controls::TASK_CONTROLS;

type Result<T> = std::result::Result<T, String>;
pub(in crate::state) struct Writer {
    root: Directory,
    guard: storage::Guard,
    until: Instant,
}
pub(in crate::state) fn open(root: &Path, until: Instant) -> Result<Option<Writer>> {
    if !TASK_CONTROLS {
        return Ok(None);
    }
    Writer::open_enabled(root, until).map(Some)
}

#[cfg(test)]
pub(in crate::state) fn test_open(root: &Path, until: Instant) -> Result<Writer> {
    Writer::open_enabled(root, until)
}
impl Writer {
    #[cfg(windows)]
    pub(in crate::state) fn deadline(&self) -> Instant {
        self.until
    }

    fn open_enabled(root: &Path, until: Instant) -> Result<Self> {
        if Instant::now() >= until {
            return Err("The native task-control admission expired".into());
        }
        let root = Directory::open(root)?;
        if !root.private()? {
            return Err("The original native store is not private".into());
        }
        let file = root.existing_private_lock(".lock")?;
        loop {
            match file.try_lock_exclusive() {
                Ok(()) => break,
                Err(e) if !platform::lock_contended(&e) => return Err(e.to_string()),
                Err(_) if Instant::now() >= until => {
                    return Err("The original native writer is busy".into())
                }
                Err(_) => std::thread::sleep(Duration::from_millis(5)),
            }
            root.verify()?;
        }
        let this = Self {
            root,
            guard: platform::FileLock::held(file),
            until,
        };
        this.verify()?;
        Ok(this)
    }
    pub fn verify(&self) -> Result<()> {
        if Instant::now() >= self.until {
            return Err("The original native admission expired".into());
        }
        self.root.verify()?;
        if !self.root.private()? {
            return Err("The native store privacy changed".into());
        }
        verify_writer(&self.root, &self.guard)
    }
    pub fn guard(&self) -> Result<&storage::Guard> {
        self.verify()?;
        Ok(&self.guard)
    }
    pub fn root(&self) -> Result<&Path> {
        self.verify()?;
        Ok(&self.root.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::fs;
    use std::fs::OpenOptions;
    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir_in(platform::canonical(std::env::temp_dir()).unwrap()).unwrap();
        platform::restrict_dir(dir.path()).unwrap();
        platform::private_open(OpenOptions::new().create_new(true).write(true))
            .open(dir.path().join(".lock"))
            .unwrap();
        dir
    }
    fn until() -> Instant {
        Instant::now() + Duration::from_millis(100)
    }
    #[test]
    fn off_is_no_access_and_an_active_lookup_never_creates_a_missing_writer() {
        let dir = tempfile::tempdir_in(platform::canonical(std::env::temp_dir()).unwrap()).unwrap();
        let missing = dir.path().join("absent");
        assert!(open(&missing, Instant::now()).unwrap().is_none());
        assert!(!missing.exists());
        platform::restrict_dir(dir.path()).unwrap();
        assert!(Writer::open_enabled(dir.path(), until()).is_err());
        assert!(!dir.path().join(".lock").exists());
    }
    #[test]
    fn a_held_writer_is_bounded_and_release_permits_only_a_new_admission() {
        let dir = fixture();
        let original = Writer::open_enabled(dir.path(), until()).unwrap();
        assert!(
            Writer::open_enabled(dir.path(), Instant::now() + Duration::from_millis(10)).is_err()
        );
        original.verify().unwrap();
        drop(original);
        Writer::open_enabled(dir.path(), until())
            .unwrap()
            .verify()
            .unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn substituted_lock_root_alias_and_special_files_never_lend_the_writer() {
        let dir = fixture();
        let writer = Writer::open_enabled(dir.path(), until()).unwrap();
        fs::rename(dir.path().join(".lock"), dir.path().join(".kept")).unwrap();
        platform::private_open(OpenOptions::new().create_new(true).write(true))
            .open(dir.path().join(".lock"))
            .unwrap();
        assert!(writer.verify().is_err());
        drop(writer);
        fs::remove_file(dir.path().join(".lock")).unwrap();
        std::os::unix::fs::symlink(dir.path().join(".kept"), dir.path().join(".lock")).unwrap();
        assert!(Writer::open_enabled(dir.path(), until()).is_err());
        fs::remove_file(dir.path().join(".lock")).unwrap();
        let path = std::ffi::CString::new(dir.path().join(".lock").to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        assert!(Writer::open_enabled(dir.path(), until()).is_err());
        let alias = dir.path().join("alias");
        std::os::unix::fs::symlink(dir.path(), &alias).unwrap();
        assert!(Writer::open_enabled(&alias, until()).is_err());
    }
}
