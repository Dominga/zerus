//! SQLite database files opened without closing a raw descriptor of a live
//! one (zerus-iiee).
//!
//! On Unix, closing ANY descriptor of a file drops ALL of this process's
//! POSIX record locks on that file, including the locks of every other live
//! SQLite connection to it (#1332). Another process could then take a lock
//! SQLite still relies on, for example to delete a live WAL, or write during
//! a transaction this process believes it holds. So an existing database is
//! checked through its path only; a missing one is created owner-only under a
//! private temporary name and published, never replacing anything, once that
//! descriptor is closed; and each connection is then bound to the checked
//! file: SQLite's own filename names it, and SQLite reports that its open file
//! has not moved since (`SQLITE_FCNTL_HAS_MOVED`).
//!
//! Windows locks belong to the handle that took them, so Windows callers keep
//! their raw-handle checks.
#![cfg(unix)]

use rusqlite::Connection;
use std::{
    ffi::{CStr, CString, OsStr},
    fs, io,
    os::unix::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
    },
    path::Path,
};

/// A database file's device and inode: what a connection is bound to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Identity {
    pub device: u64,
    pub inode: u64,
}

fn identity(metadata: &fs::Metadata) -> Identity {
    Identity {
        device: metadata.dev(),
        inode: metadata.ino(),
    }
}

/// Creates an empty owner-only database file at `path` if none is there,
/// never replacing one and never closing a descriptor of the file once it is
/// at `path`: the file is made under a private temporary name beside it,
/// closed, and only then renamed into place if nothing is there yet.
pub fn create(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => (),
        other => return other.map(|_| ()),
    }
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "database path has no file name",
        ));
    };
    let staging = parent.join(format!(
        ".{}.{}.creating",
        name.to_string_lossy(),
        uuid::Uuid::new_v4()
    ));
    drop(
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&staging)?,
    );
    let published = publish(&staging, path);
    if published.is_err() {
        // Only this call's own staging name.
        let _ = fs::remove_file(&staging);
    }
    match published {
        // Another opener created it meanwhile; it is checked like any other.
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        other => other,
    }
}

/// Moves `from` to `to` only if nothing is at `to`.
fn publish(from: &Path, to: &Path) -> io::Result<()> {
    let source = CString::new(from.as_os_str().as_bytes())?;
    let target = CString::new(to.as_os_str().as_bytes())?;
    #[cfg(target_os = "linux")]
    // SAFETY: both arguments are NUL-terminated paths owned by this call.
    let rc = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            source.as_ptr(),
            libc::AT_FDCWD,
            target.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    #[cfg(target_os = "macos")]
    // SAFETY: both arguments are NUL-terminated paths owned by this call.
    let rc = unsafe { libc::renamex_np(source.as_ptr(), target.as_ptr(), libc::RENAME_EXCL) };
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let rc = {
        let _ = (&source, &target);
        -1
    };
    if rc == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        // A file system without the atomic no-replace rename: a hard link
        // never replaces either. Until the staging name goes, the file has two
        // links and `private` refuses it, so a concurrent opener fails closed.
        Some(libc::EINVAL | libc::ENOSYS | libc::ENOTSUP) => {
            let linked = fs::hard_link(from, to);
            let _ = fs::remove_file(from);
            linked
        }
        _ => Err(error),
    }
}

/// The identity of the file at `path`, following links, checked by path only.
pub fn existing(path: &Path) -> io::Result<Identity> {
    Ok(identity(&fs::metadata(path)?))
}

/// Checks the database file at `path` by path only: a single-link regular
/// file, never a symbolic link, owned by this user and closed to everyone
/// else. Returns its identity, to bind each connection to.
pub fn private(path: &Path) -> io::Result<Identity> {
    let metadata = fs::symlink_metadata(path)?;
    let private = metadata.file_type().is_file()
        && metadata.nlink() == 1
        // SAFETY: geteuid has no preconditions.
        && metadata.uid() == unsafe { libc::geteuid() }
        && metadata.mode() & 0o077 == 0;
    if !private {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "database file is not a private single-link regular file",
        ));
    }
    Ok(identity(&metadata))
}

/// Binds `db`'s main database to `expected`: SQLite's own filename names that
/// file, and SQLite reports that its open file has not moved since. No raw
/// descriptor is opened, so no lock of a live connection is dropped.
pub fn bind(db: &Connection, expected: Identity) -> io::Result<()> {
    let changed = || io::Error::other("database file changed");
    // SAFETY: the handle is this live connection's; SQLite owns the name.
    let name = unsafe { rusqlite::ffi::sqlite3_db_filename(db.handle(), c"main".as_ptr()) };
    if name.is_null() {
        return Err(changed());
    }
    // SAFETY: SQLite returned a NUL-terminated name valid while `db` lives.
    let path = Path::new(OsStr::from_bytes(
        unsafe { CStr::from_ptr(name) }.to_bytes(),
    ));
    let named = identity(&fs::metadata(path)?);
    let mut moved: std::ffi::c_int = 1;
    // SAFETY: HAS_MOVED writes one int through the pointer given.
    let code = unsafe {
        rusqlite::ffi::sqlite3_file_control(
            db.handle(),
            c"main".as_ptr(),
            rusqlite::ffi::SQLITE_FCNTL_HAS_MOVED,
            (&mut moved as *mut std::ffi::c_int).cast(),
        )
    };
    if code != rusqlite::ffi::SQLITE_OK || moved != 0 || named != expected {
        return Err(changed());
    }
    Ok(())
}

/// Whether another process is refused a write lock over SQLite's lock bytes
/// of `path` (the pending, reserved and shared bytes: 512 at 0x40000000), as
/// #1332's test probes: a lock of a live connection here is still held.
#[cfg(test)]
pub fn foreign_lock_refused(path: &Path) -> bool {
    const PROBE: &str = "import errno, fcntl, sys
with open(sys.argv[1], 'r+b') as f:
    try:
        fcntl.lockf(f, fcntl.LOCK_EX | fcntl.LOCK_NB, 512, 0x40000000)
    except OSError as e:
        sys.exit(3 if e.errno in (errno.EACCES, errno.EAGAIN) else 4)
";
    let output = std::process::Command::new("python3")
        .arg("-c")
        .arg(PROBE)
        .arg(path)
        .stdin(std::process::Stdio::null())
        .output()
        .expect("python3 runs the lock probe");
    match output.status.code() {
        Some(3) => true,
        Some(0) => false,
        _ => panic!(
            "lock probe failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ),
    }
}

#[cfg(test)]
mod tests;
