//! The private Store's original directory creation and nonblocking lock rules.
#[cfg(unix)]
pub fn euid() -> u32 {
    rustix::process::geteuid().as_raw()
}
#[cfg(unix)]
pub fn lock_exclusive(file: &std::fs::File) -> std::io::Result<()> {
    rustix::fs::flock(file, rustix::fs::FlockOperation::NonBlockingLockExclusive)
        .map_err(Into::into)
}
#[cfg(windows)]
pub fn lock_exclusive(file: &std::fs::File) -> std::io::Result<()> {
    file.try_lock().map_err(std::io::Error::other)
}
#[cfg(unix)]
use std::{fs::DirBuilder, io, os::unix::fs::DirBuilderExt, path::Path};
#[cfg(unix)]
pub fn create_directory(path: &Path) -> io::Result<()> {
    make_directory(path, 0o700)
}

#[cfg(unix)]
fn make_directory(path: &Path, mode: u32) -> io::Result<()> {
    match DirBuilder::new().mode(mode).create(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let parent = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .ok_or(error)?;
            make_directory(parent, 0o777)?;
            match DirBuilder::new().mode(mode).create(path) {
                Err(error) if !path.is_dir() => Err(error),
                _ => Ok(()),
            }
        }
        // Python does not trust EEXIST alone: any failure is fine for a directory.
        Err(error) => {
            if path.is_dir() {
                Ok(())
            } else {
                Err(error)
            }
        }
    }
}
