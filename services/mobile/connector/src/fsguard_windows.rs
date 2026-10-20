//! WhiteWh fork: private state on disk for a Windows gateway, the counterpart
//! of `fsguard.rs`. Windows has no owner uid or mode bits: the state directory
//! lives in the user's profile (%LOCALAPPDATA% or %USERPROFILE%), whose default
//! ACL admits only that user, SYSTEM and administrators. What stays checked is
//! the shape: a real directory and regular files, never a symlink or junction
//! (a reparse point), opened without following one. The exclusive lock is the
//! standard library's `LockFileEx` lock instead of `flock(2)`.
use crate::error::{ConnectorError, Result, IN_USE, PRIVATE_DIRECTORY, PRIVATE_FILE, UNSAFE_FILE};
use std::fs::{DirBuilder, File, Metadata, OpenOptions, TryLockError};
use std::io;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;

/// Windows has no uid; ownership is the profile's ACL (see the module notes).
pub fn euid() -> u32 {
    0
}

fn reparse(metadata: &Metadata) -> bool {
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

/// `create_dir_all`, accepting an existing directory; [`check_private_directory`]
/// refuses what is not one.
pub fn create_directory(path: &Path) -> io::Result<()> {
    match DirBuilder::new().recursive(true).create(path) {
        Err(error) if !path.is_dir() => Err(error),
        _ => Ok(()),
    }
}

/// The state directory itself, not a symlink or junction to one.
pub fn check_private_directory(path: &Path) -> Result<()> {
    let refused = || ConnectorError::new(PRIVATE_DIRECTORY);
    let metadata = path.symlink_metadata().map_err(|_| refused())?;
    if !metadata.file_type().is_dir() || reparse(&metadata) {
        return Err(refused());
    }
    Ok(())
}

/// Opens a reparse point itself instead of its target, as O_NOFOLLOW refuses to.
pub fn open_no_follow(options: &mut OpenOptions) -> &mut OpenOptions {
    options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
}

/// `fsguard::private_file` on Windows: read-write, created when `create` is
/// set, and only a regular file that is no reparse point.
pub fn private_file(path: &Path, create: bool) -> Result<Option<File>> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(create);
    let file = match open_no_follow(&mut options).open(path) {
        Ok(file) => file,
        Err(error) if !create && error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(ConnectorError::new(UNSAFE_FILE)),
    };
    let metadata = file.metadata().map_err(|_| ConnectorError::new(UNSAFE_FILE))?;
    if reparse(&metadata) {
        return Err(ConnectorError::new(UNSAFE_FILE));
    }
    if !metadata.file_type().is_file() {
        return Err(ConnectorError::new(PRIVATE_FILE));
    }
    Ok(Some(file))
}

/// What [`private_file`] would decide, from the entry alone.
pub fn inspect_private_file(path: &Path) -> std::result::Result<Option<Metadata>, &'static str> {
    let metadata = match path.symlink_metadata() {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(UNSAFE_FILE),
    };
    let kind = metadata.file_type();
    if kind.is_symlink() || kind.is_dir() || reparse(&metadata) || metadata.permissions().readonly() {
        return Err(UNSAFE_FILE);
    }
    if !kind.is_file() {
        return Err(PRIVATE_FILE);
    }
    Ok(Some(metadata))
}

/// The exclusive lock on `connector.lock`; it lasts while the file stays open.
pub fn lock_exclusive(file: &File) -> Result<()> {
    file.try_lock().map_err(|_| ConnectorError::new(IN_USE))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockState {
    /// No lock file: no connector has started on this directory.
    Absent,
    /// A connector holds the journal.
    Held,
    Free,
    /// The probe could not open the file (a reparse point, or no access).
    Unknown,
}

impl LockState {
    pub fn name(self) -> &'static str {
        match self {
            LockState::Absent => "absent",
            LockState::Held => "held",
            LockState::Free => "free",
            LockState::Unknown => "unknown",
        }
    }
}

/// A shared, non-blocking probe, only on an existing file.
pub fn probe_lock(path: &Path) -> LockState {
    let mut options = OpenOptions::new();
    options.read(true);
    let file = match open_no_follow(&mut options).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return LockState::Absent,
        Err(_) => return LockState::Unknown,
    };
    match file.metadata() {
        Ok(metadata) if !reparse(&metadata) => {}
        _ => return LockState::Unknown,
    }
    match file.try_lock_shared() {
        // Closing the file releases the probe at once.
        Ok(()) => LockState::Free,
        Err(TryLockError::WouldBlock) => LockState::Held,
        Err(_) => LockState::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_files_are_created_locked_and_probed() {
        let root = std::env::temp_dir().join(format!("zerus-fsguard-{}", std::process::id()));
        let directory = root.join("state");
        create_directory(&directory).unwrap();
        check_private_directory(&directory).unwrap();
        let path = directory.join("connector.lock");
        assert_eq!(probe_lock(&path), LockState::Absent);
        assert!(private_file(&path, false).unwrap().is_none());
        let lock = private_file(&path, true).unwrap().unwrap();
        lock_exclusive(&lock).unwrap();
        assert_eq!(probe_lock(&path), LockState::Held);
        let second = private_file(&path, false).unwrap().unwrap();
        assert!(lock_exclusive(&second).is_err());
        drop(lock);
        assert_eq!(probe_lock(&path), LockState::Free);
        assert!(inspect_private_file(&path).unwrap().is_some());
        assert!(matches!(inspect_private_file(&directory), Err(UNSAFE_FILE)));
        drop(second);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
