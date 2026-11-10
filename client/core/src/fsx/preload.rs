//! Fixed native pre-load archive I/O. No effect queue or source mutation.
use serde::{Deserialize, Serialize};
#[cfg(unix)]
use std::path::PathBuf;
use std::{
    fs::File,
    io::{self, Read, Seek, Write},
    path::Path,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Stamp {
    object: [u64; 2],
    size: u64,
    modified: [i64; 2],
}
impl Stamp {
    /// The leaf's recorded length in bytes.
    pub(crate) fn size(&self) -> u64 {
        self.size
    }
}
fn refused() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "pre-load archive identity refused",
    )
}
#[cfg(unix)]
fn stamp(file: &File) -> io::Result<Stamp> {
    stamp_with_access(file, false)
}
#[cfg(unix)]
fn stamp_with_access(file: &File, legacy_cache: bool) -> io::Result<Stamp> {
    use std::os::unix::fs::MetadataExt;
    let m = file.metadata()?;
    let forbidden = if legacy_cache { 0o022 } else { 0o077 };
    if !m.is_file()
        || m.uid() != unsafe { libc::geteuid() }
        || m.nlink() != 1
        || m.mode() & forbidden != 0
    {
        return Err(refused());
    }
    Ok(Stamp {
        object: [m.dev(), m.ino()],
        size: m.len(),
        modified: [m.mtime(), m.mtime_nsec()],
    })
}
#[cfg(windows)]
fn stamp(file: &File) -> io::Result<Stamp> {
    super::windows::verify(file, false)?;
    let m = super::windows::info(file)?;
    Ok(Stamp {
        object: [
            u64::from(m.dwVolumeSerialNumber),
            (u64::from(m.nFileIndexHigh) << 32) | u64::from(m.nFileIndexLow),
        ],
        size: (u64::from(m.nFileSizeHigh) << 32) | u64::from(m.nFileSizeLow),
        modified: [
            i64::from(m.ftLastWriteTime.dwHighDateTime),
            i64::from(m.ftLastWriteTime.dwLowDateTime),
        ],
    })
}
#[cfg(not(any(unix, windows)))]
fn stamp(_: &File) -> io::Result<Stamp> {
    Err(io::ErrorKind::Unsupported.into())
}

pub(crate) struct Parents {
    #[cfg(unix)]
    entries: Vec<(PathBuf, File, u64, u64)>,
    #[cfg(windows)]
    _native: super::windows::Parents,
}
impl Parents {
    pub fn open(path: &Path, create: bool) -> io::Result<Self> {
        Self::open_with(path, create, true)
    }
    fn open_with(path: &Path, create: bool, private: bool) -> io::Result<Self> {
        #[cfg(windows)]
        {
            let _ = private;
            Ok(Self {
                _native: super::windows::Parents::open(path, create)?,
            })
        }
        #[cfg(unix)]
        {
            use std::{
                fs::{self, OpenOptions},
                os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
                path::Component,
            };
            if !path.is_absolute() {
                return Err(refused());
            }
            let mut current = PathBuf::new();
            let mut entries = Vec::new();
            for part in path.components() {
                if entries.len() == 128
                    || !matches!(part, Component::RootDir | Component::Normal(_))
                {
                    return Err(refused());
                }
                current.push(part);
                let open = || {
                    OpenOptions::new()
                        .read(true)
                        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
                        .open(&current)
                };
                let f = match open() {
                    Ok(f) => f,
                    Err(e) if create && current == path && e.kind() == io::ErrorKind::NotFound => {
                        fs::DirBuilder::new().mode(0o700).create(path)?;
                        super::checkpoint::sync_parent(path)?;
                        open()?
                    }
                    Err(e) => return Err(e),
                };
                let m = f.metadata()?;
                // The canonical existing data directory needs owner identity,
                // not a restrictive umask. The archive slot itself stays 0700.
                if !m.is_dir()
                    || current == path
                        && (m.uid() != unsafe { libc::geteuid() }
                            || private && m.mode() & 0o077 != 0)
                {
                    return Err(refused());
                }
                entries.push((current.clone(), f, m.dev(), m.ino()));
            }
            Ok(Self { entries })
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (path, create, private);
            Err(io::ErrorKind::Unsupported.into())
        }
    }
    pub fn recheck(&self) -> io::Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            for (p, f, dev, ino) in &self.entries {
                let m = std::fs::symlink_metadata(p)?;
                let held = f.metadata()?;
                if !m.is_dir()
                    || m.dev() != *dev
                    || m.ino() != *ino
                    || held.dev() != *dev
                    || held.ino() != *ino
                {
                    return Err(refused());
                }
            }
        }
        Ok(())
    }
}
/// Resolve the NATIVE managed data directory once, off the GUI thread. Home,
/// XDG_DATA_HOME and .local/share may be symlinks. Subsequent retries keep this
/// same canonical object; retargeting an alias never selects a new source.
#[cfg(unix)]
pub(crate) struct ManagedData {
    path: PathBuf,
    parents: Parents,
}
#[cfg(unix)]
impl ManagedData {
    pub fn open(path: &Path) -> io::Result<Self> {
        let path = std::fs::canonicalize(path)?;
        let parents = Parents::open_with(&path, false, false)?;
        Ok(Self { path, parents })
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn recheck(&self) -> io::Result<()> {
        self.parents.recheck()
    }
}
fn parent(path: &Path) -> io::Result<Parents> {
    Parents::open(path.parent().ok_or_else(refused)?, false)
}
pub(crate) fn read(path: &Path, limit: usize) -> io::Result<Option<(Vec<u8>, Stamp)>> {
    read_with(path, limit, false, true, false)
}
/// The stamp of a private leaf without reading its bytes: opened without
/// following links and checked like a read (one link, owner only). `None`:
/// no such leaf.
pub(crate) fn stat(path: &Path) -> io::Result<Option<Stamp>> {
    let parents = parent(path)?;
    #[cfg(unix)]
    let opened = {
        use std::{fs::OpenOptions, os::unix::fs::OpenOptionsExt};
        OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
    };
    #[cfg(windows)]
    let opened = super::windows::preload_stat_file(path);
    #[cfg(not(any(unix, windows)))]
    let opened: io::Result<File> = Err(io::ErrorKind::Unsupported.into());
    let file = match opened {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let actual = stamp(&file)?;
    parents.recheck()?;
    Ok(Some(actual))
}
fn read_with(
    path: &Path,
    limit: usize,
    sync: bool,
    private: bool,
    legacy_cache: bool,
) -> io::Result<Option<(Vec<u8>, Stamp)>> {
    let parents = Parents::open_with(path.parent().ok_or_else(refused)?, false, private)?;
    if std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file() && m.len() > limit as u64) {
        return Err(io::ErrorKind::FileTooLarge.into());
    }
    #[cfg(windows)]
    let opened = if sync {
        super::windows::current(path, limit)?
    } else {
        super::windows::capture(path, limit)?
    };
    #[cfg(windows)]
    let Some(mut opened) = opened
    else {
        return Ok(None);
    };
    #[cfg(windows)]
    let file = &mut opened.file;
    #[cfg(unix)]
    let opened = if legacy_cache {
        super::checkpoint::read_only_source(path, limit)?
    } else {
        super::checkpoint::current(path, limit)?
    };
    #[cfg(unix)]
    let read_stamp = |file: &File| stamp_with_access(file, legacy_cache);
    #[cfg(not(unix))]
    let read_stamp = |file: &File| {
        let _ = legacy_cache;
        stamp(file)
    };
    #[cfg(unix)]
    let Some(mut opened) = opened
    else {
        return Ok(None);
    };
    #[cfg(unix)]
    let file = &mut opened.file;
    #[cfg(not(any(unix, windows)))]
    let file = &mut File::open(path)?;
    let before = read_stamp(file)?;
    if before.size > limit as u64 {
        return Err(refused());
    }
    file.rewind()?;
    let mut bytes = Vec::new();
    (&mut *file)
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit || bytes.len() as u64 != before.size || read_stamp(file)? != before {
        return Err(refused());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let m = std::fs::symlink_metadata(path)?;
        if !m.is_file() || [m.dev(), m.ino()] != before.object {
            return Err(refused());
        }
    }
    if sync {
        file.sync_all()?;
    }
    parents.recheck()?;
    Ok(Some((bytes, before)))
}

/// No overwrite or cleanup after an uncertain write. A partial fixed leaf is
/// deliberately reviewable and cannot be adopted as a different snapshot.
pub(crate) fn create(path: &Path, bytes: &[u8]) -> io::Result<Stamp> {
    let parents = parent(path)?;
    #[cfg(unix)]
    let mut file = {
        use std::{fs::OpenOptions, os::unix::fs::OpenOptionsExt};
        OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)?
    };
    #[cfg(windows)]
    let mut file = super::windows::preload_file(path, true)?;
    #[cfg(not(any(unix, windows)))]
    let mut file = return Err(io::Error::from(io::ErrorKind::Unsupported));
    file.write_all(bytes)?;
    file.sync_all()?;
    let actual = stamp(&file)?;
    file.rewind()?;
    let mut copied = Vec::new();
    (&mut file)
        .take(bytes.len() as u64 + 1)
        .read_to_end(&mut copied)?;
    if copied != bytes || stamp(&file)? != actual {
        return Err(refused());
    }
    drop(file);
    #[cfg(unix)]
    super::checkpoint::sync_parent(path)?;
    parents.recheck()?;
    let (check, identity) = read(path, bytes.len().max(1))?.ok_or_else(refused)?;
    // Windows may finalize last-write metadata when the last writer closes.
    // Keep the original object/size binding; record the post-close stamp once.
    if check != bytes || identity.object != actual.object || identity.size != actual.size {
        return Err(refused());
    }
    Ok(identity)
}

pub(crate) struct Lock {
    _file: File,
    _parents: Parents,
}
impl Lock {
    pub fn take(path: &Path) -> io::Result<Self> {
        Self::take_with(path, true)
    }
    fn take_with(path: &Path, private: bool) -> io::Result<Self> {
        let parents = Parents::open_with(path.parent().ok_or_else(refused)?, false, private)?;
        #[cfg(unix)]
        let file = {
            use std::{
                fs::OpenOptions,
                os::unix::{fs::OpenOptionsExt, io::AsRawFd},
            };
            let f = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW)
                .open(path)?;
            if stamp(&f)?.size != 0 {
                return Err(refused());
            }
            if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                return Err(io::Error::last_os_error());
            }
            f
        };
        #[cfg(windows)]
        let file = match super::windows::preload_file(path, false) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                super::windows::preload_file(path, true)?
            }
            Err(e) => return Err(e),
        };
        #[cfg(not(any(unix, windows)))]
        let file = return Err(io::Error::from(io::ErrorKind::Unsupported));
        let held = Self {
            _file: file,
            _parents: parents,
        };
        if stamp(&held._file)?.size != 0 {
            return Err(refused());
        }
        held._parents.recheck()?;
        Ok(held)
    }
}

#[cfg(unix)]
impl Drop for Lock {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;
        // Another thread may have forked while this description was locked.
        // Close alone leaves its flock held by that child until exec/exit.
        let _ = unsafe { libc::flock(self._file.as_raw_fd(), libc::LOCK_UN) };
    }
}

#[cfg(all(test, unix))]
mod lock_tests {
    use super::*;
    #[test]
    fn dropping_the_lock_releases_it_even_while_an_inherited_description_remains_open() {
        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("zerus-preload-lock-{}", uuid::Uuid::new_v4()));
        drop(Parents::open(&root, true).unwrap());
        let path = root.join("capture.lock");
        {
            let take = || Lock::take(&path);
            let held = take().unwrap();
            // fork inherits this SAME open file description, just as dup does.
            // Keep it alive without creating a process in the parallel suite.
            let inherited = held._file.try_clone().unwrap();
            assert!(matches!(take(), Err(e) if e.kind()==io::ErrorKind::WouldBlock));
            drop(held);
            let next = take().expect("the original owner explicitly released its flock");
            assert_eq!(inherited.metadata().unwrap().len(), 0);
            drop(next);
            drop(inherited);
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
