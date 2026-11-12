//! Canonical GUI lifetime exclusion. This namespace never consults HOME,
//! TMPDIR, XDG paths, hgs configuration or the desktop-service GUI flag.
//! Keep the inode after exit: unlinking a flock file can split ownership.
use std::{
    fs::{self, DirBuilder, File, OpenOptions},
    io,
    os::unix::{
        fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt},
        net::UnixListener,
    },
    path::{Path, PathBuf},
};

pub fn directory() -> PathBuf {
    // /tmp is the OS namespace, deliberately not std::env::temp_dir(). On
    // macOS /tmp and /private/tmp name the same system-owned directory.
    PathBuf::from(format!("/tmp/com.hgdev.hgs-tray-{}", unsafe {
        libc::geteuid()
    }))
}

/// The guard directory of this start. Only a measurement start
/// (`ZERUS_CLIENT_PERF`) may name a private root (`ZERUS_CLIENT_GUARD_ROOT`,
/// absolute); `acquire` applies the same ownership and mode checks there.
/// Every other start keeps the canonical directory whatever its environment,
/// so a changed HOME or TMPDIR still cannot create a second GUI (zerus-5a99f).
pub fn directory_for(measurement: bool, root: Option<&std::ffi::OsStr>) -> io::Result<PathBuf> {
    match root.filter(|root| !root.is_empty()) {
        Some(root) if measurement => {
            let root = PathBuf::from(root);
            if !root.is_absolute() {
                return Err(io::Error::other(
                    "ZERUS_CLIENT_GUARD_ROOT must be an absolute path",
                ));
            }
            Ok(root)
        }
        _ => Ok(directory()),
    }
}

pub struct Guard {
    file: File,
    pub socket: PathBuf,
    inode: Option<u64>,
}

pub fn acquire(directory: &Path) -> io::Result<Option<Guard>> {
    match DirBuilder::new().mode(0o700).create(directory) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    let uid = unsafe { libc::geteuid() };
    let metadata = fs::symlink_metadata(directory)?;
    if !metadata.is_dir() || metadata.uid() != uid || metadata.mode() & 0o077 != 0 {
        return Err(io::Error::other("unsafe canonical GUI directory"));
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(directory.join("gui.lock"))?;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != uid
        || metadata.mode() & 0o077 != 0
        || metadata.nlink() != 1
    {
        return Err(io::Error::other("unsafe canonical GUI lock"));
    }
    match file.try_lock() {
        Ok(()) => Ok(Some(Guard {
            file,
            socket: directory.join("gui.sock"),
            inode: None,
        })),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(error)) => Err(error),
    }
}

impl Guard {
    pub fn bind(&mut self) -> io::Result<UnixListener> {
        // Only a kernel-lock owner may replace the crashed owner's socket.
        match fs::symlink_metadata(&self.socket) {
            Ok(metadata)
                if metadata.file_type().is_socket()
                    && metadata.uid() == unsafe { libc::geteuid() } =>
            {
                fs::remove_file(&self.socket)?;
            }
            Ok(_) => return Err(io::Error::other("unsafe canonical GUI socket")),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let listener = UnixListener::bind(&self.socket)?;
        self.inode = Some(fs::symlink_metadata(&self.socket)?.ino());
        fs::set_permissions(
            &self.socket,
            <fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o600),
        )?;
        Ok(listener)
    }

    fn remove_socket(&mut self) {
        if self.inode.is_some_and(|inode| {
            fs::symlink_metadata(&self.socket).is_ok_and(|metadata| metadata.ino() == inode)
        }) {
            let _ = fs::remove_file(&self.socket);
        }
        self.inode = None;
    }

    pub fn retain_until_exit(mut self) {
        self.remove_socket();
        // The final GUI exit hook closes listeners before the OS has ended
        // the process. Do not admit a replacement during that interval.
        std::mem::forget(self);
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.remove_socket();
        // A concurrent fork may briefly retain a close-on-exec duplicate.
        // Explicit release also makes normal shutdown independent of that
        // transient inherited description. Crashes still release in kernel.
        let _ = self.file.unlock();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::AsRawFd;
    #[test]
    fn only_a_measurement_start_names_a_private_guard_root() {
        let private = std::ffi::OsStr::new("/tmp/zguard-private");
        assert_eq!(
            directory_for(true, Some(private)).unwrap(),
            PathBuf::from("/tmp/zguard-private")
        );
        // An ordinary start ignores it: a changed environment still finds the owner.
        assert_eq!(directory_for(false, Some(private)).unwrap(), directory());
        assert_eq!(directory_for(true, None).unwrap(), directory());
        assert_eq!(
            directory_for(true, Some(std::ffi::OsStr::new(""))).unwrap(),
            directory()
        );
        assert!(directory_for(true, Some(std::ffi::OsStr::new("relative/root"))).is_err());
    }
    #[test]
    fn canonical_lock_is_close_on_exec() {
        let root = PathBuf::from("/tmp").join(format!("zguard-{}", uuid::Uuid::new_v4().simple()));
        let guard = acquire(&root).unwrap().unwrap();
        let flags = unsafe { libc::fcntl(guard.file.as_raw_fd(), libc::F_GETFD) };
        assert!(flags >= 0);
        assert_ne!(flags & libc::FD_CLOEXEC, 0);
        drop(guard);
        fs::remove_dir_all(root).unwrap();
    }
}
