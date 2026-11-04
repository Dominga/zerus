use std::{
    fs::{File, OpenOptions},
    io,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            fs::{MetadataExt, OpenOptionsExt},
            process::CommandExt,
        },
    },
    path::{Path, PathBuf},
    process::Command,
};

#[derive(PartialEq, Eq)]
struct Stamp {
    device: u64,
    inode: u64,
    bytes: u64,
    modified: (i64, i64),
    changed: (i64, i64),
    mode: u32,
}
fn stamp(file: &File) -> io::Result<Stamp> {
    let m = file.metadata()?;
    if !m.is_file() || m.ino() == 0 || m.mode() & 0o111 == 0 {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    Ok(Stamp {
        device: m.dev(),
        inode: m.ino(),
        bytes: m.len(),
        modified: (m.mtime(), m.mtime_nsec()),
        changed: (m.ctime(), m.ctime_nsec()),
        mode: m.mode(),
    })
}
fn open(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
}
fn identity(file: &File) -> io::Result<(u64, u64)> {
    let m = file.metadata()?;
    if !m.is_dir() {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok((m.dev(), m.ino()))
}
struct Folder {
    path: PathBuf,
    file: File,
    id: (u64, u64),
}
impl Folder {
    fn open(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)?;
        let id = identity(&file)?;
        Ok(Self {
            path: path.into(),
            file,
            id,
        })
    }
    fn current(&self) -> bool {
        identity(&self.file).ok() == Some(self.id)
            && Self::open(&self.path).is_ok_and(|now| now.id == self.id)
    }
}
pub(super) struct Pin {
    requested: PathBuf,
    resolved: PathBuf,
    file: File,
    original: Stamp,
    program: PathBuf,
    script: bool,
    folders: Vec<Folder>,
}
impl Pin {
    pub(super) fn open(path: &Path) -> io::Result<Self> {
        if !path.is_absolute() {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let resolved = path.canonicalize()?;
        let ancestors = resolved.ancestors().skip(1).collect::<Vec<_>>();
        if ancestors.len() > 128 {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let folders = ancestors
            .into_iter()
            .rev()
            .map(Folder::open)
            .collect::<io::Result<Vec<_>>>()?;
        let source = open(&resolved)?;
        let original = stamp(&source)?;
        use std::os::unix::fs::FileExt;
        let mut prefix = [0; 2];
        let script = source.read_at(&mut prefix, 0)? == 2 && &prefix == b"#!";
        // One original descriptor, never 0-2 and never inherited by unrelated
        // spawns. Only this child's pre-exec hook clears its own CLOEXEC copy.
        let fd = unsafe { libc::fcntl(source.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 3) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: fcntl returned a new owned descriptor.
        let file = unsafe { File::from_raw_fd(fd) };
        let pin = Self {
            requested: path.into(),
            resolved,
            file,
            original,
            program: format!("/proc/self/fd/{fd}").into(),
            script,
            folders,
        };
        if !pin.current() {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        Ok(pin)
    }
    pub(super) fn current(&self) -> bool {
        self.requested
            .canonicalize()
            .is_ok_and(|p| p == self.resolved)
            && stamp(&self.file).ok().as_ref() == Some(&self.original)
            && open(&self.resolved).and_then(|f| stamp(&f)).ok().as_ref() == Some(&self.original)
            && self.folders.iter().all(Folder::current)
    }
    pub(super) fn executable(&self) -> &Path {
        &self.program
    }
    pub(super) fn inherit(&self, command: &mut Command) -> io::Result<()> {
        let fd = self.file.as_raw_fd();
        command.arg0(&self.requested);
        if !self.script {
            // Native ELF resolves its executable descriptor before exec closes
            // CLOEXEC files. It needs no inherited execution descriptor.
            return Ok(());
        }
        // SAFETY: allocation-free fcntl only after fork. No parent flags change;
        // the live Context retains this descriptor through spawn and all I/O.
        // Scripts need this one descriptor after exec to read their exact source.
        unsafe {
            command.pre_exec(move || {
                let flags = libc::fcntl(fd, libc::F_GETFD);
                if flags < 0 || libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(())
                }
            });
        }
        Ok(())
    }
}
