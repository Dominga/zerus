//! Pinned acquisition directories. No cleanup follows a mutable pathname.
use super::*;
use std::fs::{File, OpenOptions};

pub(crate) struct Directory {
    pub path: PathBuf,
    file: File,
    #[cfg(windows)]
    _ancestors: Vec<File>,
}

// Per-thread, exact-private-directory fault injection at the publication sync
// boundary. It cannot affect a concurrent test or any production build.
#[cfg(all(test, unix))]
thread_local! { static SYNC_FAILURE: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) }; }
// Its callers are the fence probe (Linux) and the task refusal tests (Unix).
#[cfg(all(test, unix))]
pub(in crate::state) fn fail_publication_sync(path: Option<PathBuf>) {
    SYNC_FAILURE.with(|slot| *slot.borrow_mut() = path);
}

impl Directory {
    /// Namespace durability, rather than the platform's compatibility no-op.
    /// A caller requiring this proof must retain Unknown when unavailable.
    pub(in crate::state) fn sync(&self) -> Result<()> {
        self.verify()?;
        #[cfg(unix)]
        {
            self.file.sync_all().map_err(|e| e.to_string())?;
            #[cfg(test)]
            if SYNC_FAILURE.with(|slot| slot.borrow().as_ref() == Some(&self.path)) {
                return Err("fixture directory sync failure".into());
            }
            self.verify()
        }
        #[cfg(windows)]
        Err("Native namespace durability is unverified on this computer".into())
    }

    /// The native writer already exists. Task-control admission must not
    /// create it or follow an alias while trying to acquire its exclusion.
    pub(in crate::state) fn existing_private_lock(&self, name: &str) -> Result<File> {
        if name.is_empty() || name.contains(['/', '\\', ':']) || name == "." || name == ".." {
            return Err("Invalid existing exclusion filename".into());
        }
        self.verify()?;
        #[cfg(unix)]
        let file = {
            use std::os::fd::{AsRawFd, FromRawFd};
            let name = c_name(std::ffi::OsStr::new(name))?;
            // SAFETY: one fixed component below the held original parent;
            // no create, links, FIFO wait or inherited descriptor.
            let fd = unsafe {
                libc::openat(
                    self.file.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDWR
                        | libc::O_NOFOLLOW
                        | libc::O_NONBLOCK
                        | libc::O_NOCTTY
                        | libc::O_CLOEXEC,
                )
            };
            if fd < 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
            unsafe { File::from_raw_fd(fd) }
        };
        #[cfg(windows)]
        let file = crate::platform::no_follow(OpenOptions::new().read(true).write(true))
            .open(self.path.join(name))
            .map_err(|e| e.to_string())?;
        if !crate::platform::verify_private_file(&file).map_err(|e| e.to_string())?
            || file.metadata().map_err(|e| e.to_string())?.len() != 0
        {
            return Err("The original native exclusion file is unexpected".into());
        }
        self.verify()?;
        Ok(file)
    }

    /// One existing or newly created child under this original directory.
    /// A descendant alias is never resolved before the no-follow open, and
    /// creation uses the held parent rather than a second pathname walk.
    pub(in crate::state) fn ensure_child(&self, name: &str) -> Result<Self> {
        if name.is_empty() || name.contains(['/', '\\', ':']) || name == "." || name == ".." {
            return Err("Invalid directory entry name".into());
        }
        self.verify()?;
        let child = match std::fs::symlink_metadata(self.path.join(name)) {
            Ok(_) => self.child(name)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound =>
                self.create(name,&format!(".hgs-dir-{}",uuid::Uuid::new_v4()))?,
            Err(error) => return Err(error.to_string()),
        };
        self.verify()?; child.verify()?;
        crate::platform::sync_dir(&self.path).map_err(|e| e.to_string())?;
        Ok(child)
    }

    /// Native object identity, for metadata bindings. It is not a resource
    /// retirement, ownership, consent or effect permit.
    pub(crate) fn object(&self) -> Result<(u64, u64)> {
        crate::platform::FileIdentity::of(&self.file)
            .map(|id| (id.device, id.inode)).map_err(|e| e.to_string())
    }

    /// One auxiliary exclusion file in this original directory. Opening it
    /// never follows a link or waits on a FIFO; the caller must try the lock.
    pub(in crate::state) fn open_private_lock(&self, name: &str) -> Result<File> {
        if name.is_empty() || name.contains(['/', '\\', ':']) || name == "." || name == ".." {
            return Err("Invalid private lock filename".into());
        }
        self.verify()?;
        #[cfg(unix)]
        let file = {
            use std::os::fd::{AsRawFd, FromRawFd};
            let name = c_name(std::ffi::OsStr::new(name))?;
            // SAFETY: a single NUL-free entry under this held parent. Opening
            // RDWR is nonblocking even for a FIFO; its kind is then refused.
            let fd = unsafe { libc::openat(self.file.as_raw_fd(), name.as_ptr(),
                libc::O_RDWR | libc::O_CREAT | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_NOCTTY | libc::O_CLOEXEC,
                0o600 as libc::c_uint) };
            if fd < 0 { return Err(std::io::Error::last_os_error().to_string()); }
            // SAFETY: this call exclusively owns the descriptor just returned.
            unsafe { File::from_raw_fd(fd) }
        };
        #[cfg(windows)]
        let file = crate::platform::no_follow(OpenOptions::new().read(true).write(true).create(true).truncate(false))
            .open(self.path.join(name)).map_err(|e|e.to_string())?;
        if !crate::platform::verify_private_file(&file).map_err(|e|e.to_string())? {
            return Err("The exclusion file must be private, regular and singly linked".into());
        }
        self.verify()?;
        Ok(file)
    }

    /// Existing-directory reads share acquisition's pinned, no-link walk.
    /// No directory, lock or file is created by these methods.
    pub(crate) fn private(&self) -> Result<bool> {
        crate::platform::verify_private_directory(&self.file).map_err(|e| e.to_string())
    }

    /// Advisory worker locks also exist in legacy 0755 state directories.
    /// Require owner-only POSIX write access to their entries without changing
    /// that mode. This is weaker than `private()` and never a native root proof.
    #[cfg(unix)]
    pub(in crate::state) fn owner_controls_entries(&self) -> Result<bool> {
        use std::os::unix::fs::MetadataExt;
        self.verify()?;
        let metadata = self.file.metadata().map_err(|e| e.to_string())?;
        // SAFETY: geteuid only returns this process's effective user ID.
        Ok(metadata.is_dir()
            && metadata.uid() == unsafe { libc::geteuid() }
            && metadata.mode() & 0o022 == 0)
    }

    pub(crate) fn read_private(&self, name: &str) -> Result<File> {
        self.read_private_optional(name)?
            .ok_or_else(|| "Missing private file".into())
    }

    /// Absence is observed relative to this SAME pinned directory. The caller
    /// decides whether absence has any meaning; it never establishes history.
    pub(crate) fn read_private_optional(&self, name: &str) -> Result<Option<File>> {
        if name.is_empty() || name.contains(['/', '\\', ':']) || name == "." || name == ".." {
            return Err("Invalid existing file name".into());
        }
        #[cfg(unix)]
        let file = {
            use std::os::fd::{AsRawFd, FromRawFd};
            let name = c_name(std::ffi::OsStr::new(name))?;
            // SAFETY: the parent is pinned; name is one NUL-free component.
            let fd = unsafe {
                libc::openat(
                    self.file.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY
                        | libc::O_NOFOLLOW
                        | libc::O_NONBLOCK
                        | libc::O_NOCTTY
                        | libc::O_CLOEXEC,
                )
            };
            if fd < 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::NotFound {
                    return Ok(None);
                }
                return Err(error.to_string());
            }
            // SAFETY: this call owns the new descriptor.
            unsafe { File::from_raw_fd(fd) }
        };
        #[cfg(windows)]
        let file = match crate::platform::no_follow(OpenOptions::new().read(true))
            .open(self.path.join(name))
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.to_string()),
        };
        if !crate::platform::verify_private_file(&file).map_err(|e| e.to_string())? {
            return Err("The existing file must be private and regular".into());
        }
        Ok(Some(file))
    }

    pub fn verify(&self) -> Result<()> {
        let current = Self::open(&self.path)?;
        let identity = |file: &File| {
            crate::platform::FileIdentity::of(file)
                .map(|id| (id.device, id.inode))
                .map_err(|e| e.to_string())
        };
        if identity(&self.file)? != identity(&current.file)? {
            return Err("Clone directory identity changed; inspect the retained folders".into());
        }
        Ok(())
    }
}

#[cfg(unix)]
impl Directory {
    pub fn open(path: &Path) -> Result<Self> {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open("/")
            .map_err(|e| e.to_string())?;
        // Walk from a pinned root; intermediate symlinks cannot redirect opens.
        for component in path.components() {
            match component {
                std::path::Component::RootDir => (),
                std::path::Component::Normal(name) => file = open_at(&file, name)?,
                _ => return Err("Clone directory is not canonical".into()),
            }
        }
        Ok(Self {
            path: path.into(),
            file,
        })
    }

    pub fn create(&self, name: &str, staging: &str) -> Result<Self> {
        use std::os::fd::AsRawFd;
        self.verify()?;
        let from = c_name(std::ffi::OsStr::new(staging))?;
        let to = c_name(std::ffi::OsStr::new(name))?;
        // Pin the private, unpredictable staging directory before publishing the
        // selected destination name. A visible destination can never replace the
        // descriptor subsequently inherited by Git. Keep partial staging paths.
        if unsafe { libc::mkdirat(self.file.as_raw_fd(), from.as_ptr(), 0o700) } != 0 {
            return Err(format!(
                "Cannot create clone staging folder: {}",
                std::io::Error::last_os_error()
            ));
        }
        let file = open_at(&self.file, std::ffi::OsStr::new(staging))?;
        use std::os::unix::fs::MetadataExt;
        let metadata = file.metadata().map_err(|e| e.to_string())?;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
            return Err(
                "Clone staging folder is no longer private; inspect the retained folder".into(),
            );
        }
        #[cfg(target_os = "linux")]
        let rc = unsafe {
            libc::renameat2(
                self.file.as_raw_fd(),
                from.as_ptr(),
                self.file.as_raw_fd(),
                to.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        #[cfg(target_os = "macos")]
        let rc = unsafe {
            libc::renameatx_np(
                self.file.as_raw_fd(),
                from.as_ptr(),
                self.file.as_raw_fd(),
                to.as_ptr(),
                libc::RENAME_EXCL,
            )
        };
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        let rc = -1; // Refuse publication without an atomic no-replace primitive.
        if rc != 0 {
            return Err(format!(
                "Cannot reserve destination; staging folder retained: {}",
                std::io::Error::last_os_error()
            ));
        }
        let directory = Self {
            path: self.path.join(name),
            file,
        };
        self.verify()?;
        directory.verify()?;
        Ok(directory)
    }

    /// An existing child directory, opened relative to this pinned one
    /// without following a link: no pathname is resolved again.
    pub fn child(&self, name: &str) -> Result<Self> {
        if name.is_empty() || name.contains('/') || name == "." || name == ".." {
            return Err("Invalid directory entry name".into());
        }
        Ok(Self {
            path: self.path.join(name),
            file: open_at(&self.file, std::ffi::OsStr::new(name))?,
        })
    }

    /// At most `limit` bytes of a regular file in this pinned directory,
    /// opened relative to it without following a link.
    pub fn read_file(&self, name: &str, limit: usize) -> Result<Vec<u8>> {
        use std::io::Read;
        use std::os::fd::{AsRawFd, FromRawFd};
        use std::os::unix::fs::FileTypeExt;
        if name.is_empty() || name.contains('/') || name == "." || name == ".." {
            return Err("Invalid file name".into());
        }
        let name = c_name(std::ffi::OsStr::new(name))?;
        // SAFETY: the parent is pinned; name is one NUL-free component.
        let fd = unsafe {
            libc::openat(
                self.file.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_NOCTTY | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        // SAFETY: this call owns the new descriptor.
        let file = unsafe { File::from_raw_fd(fd) };
        let kind = file.metadata().map_err(|e| e.to_string())?.file_type();
        if !kind.is_file() || kind.is_fifo() {
            return Err("Expected a regular file".into());
        }
        let mut bytes = Vec::new();
        file.take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > limit {
            return Err("File is too large".into());
        }
        Ok(bytes)
    }

    /// Publishes a new private file in this pinned directory: written under a
    /// private staging name, synced, then renamed without replacing anything.
    pub fn write_new(&self, name: &str, bytes: &[u8]) -> Result<()> {
        self.write_new_file(name,bytes).map(drop)
    }

    /// The same publication, retaining the exact file created by this call.
    /// A later pathname open cannot stand in for that original object.
    pub fn write_new_file(&self, name: &str, bytes: &[u8]) -> Result<File> {
        self.publish_file(name, bytes, false)
    }

    /// Replace metadata under caller-held exclusion, through this original
    /// directory. This does not establish any action or consent authority.
    pub(in crate::state) fn replace_file(&self, name: &str, bytes: &[u8]) -> Result<()> {
        self.replace_file_retained(name, bytes).map(drop)
    }
    pub(in crate::state) fn replace_file_retained(&self, name: &str, bytes: &[u8]) -> Result<File> {
        self.publish_file(name, bytes, true)
    }

    fn publish_file(&self, name: &str, bytes: &[u8], replace: bool) -> Result<File> {
        use std::io::Write;
        use std::os::fd::{AsRawFd, FromRawFd};
        if name.is_empty() || name.contains('/') || name == "." || name == ".." {
            return Err("Invalid file name".into());
        }
        let staging = c_name(std::ffi::OsStr::new(&format!(
            ".{name}.{}",
            uuid::Uuid::new_v4()
        )))?;
        let to = c_name(std::ffi::OsStr::new(name))?;
        let dir = self.file.as_raw_fd();
        // SAFETY: the parent is pinned; the staging name is new and NUL-free.
        let fd = unsafe {
            libc::openat(
                dir,
                staging.as_ptr(),
                libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600 as libc::c_uint,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        // SAFETY: this call owns the new descriptor.
        let mut file = unsafe { File::from_raw_fd(fd) };
        let written = file
            .write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|e| e.to_string());
        #[cfg(target_os = "linux")]
        let rc = written.as_ref().map_or(-1, |_| unsafe {
            libc::renameat2(dir, staging.as_ptr(), dir, to.as_ptr(), if replace { 0 } else { libc::RENAME_NOREPLACE })
        });
        #[cfg(target_os = "macos")]
        let rc = written.as_ref().map_or(-1, |_| unsafe {
            libc::renameatx_np(dir, staging.as_ptr(), dir, to.as_ptr(), if replace { 0 } else { libc::RENAME_EXCL })
        });
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        let rc = -1; // Refuse publication without an atomic no-replace primitive.
        if rc != 0 {
            let error = std::io::Error::last_os_error().to_string();
            // SAFETY: removes only this call's own staging name in the pinned parent.
            unsafe { libc::unlinkat(dir, staging.as_ptr(), 0) };
            written?;
            return Err(format!("Cannot publish {}: {error}", to.to_string_lossy()));
        }
        let synced = self.file.sync_all();
        #[cfg(test)]
        let synced = if SYNC_FAILURE.with(|slot| slot.borrow().as_ref() == Some(&self.path)) {
            Err(std::io::Error::other("fixture directory sync failure"))
        } else { synced };
        synced.map_err(|e| e.to_string())?;
        self.verify()?;
        Ok(file)
    }

    /// This pinned directory's descriptor number in this process.
    pub fn descriptor(&self) -> std::os::fd::RawFd {
        use std::os::fd::AsRawFd;
        self.file.as_raw_fd()
    }

    /// Keeps this descriptor open, at its own number, in the program the
    /// command execs. Register it after any descriptor whitelist.
    pub fn share(&self, command: &mut Command) -> std::os::fd::RawFd {
        use std::os::{fd::AsRawFd, unix::process::CommandExt};
        let fd = self.file.as_raw_fd();
        // SAFETY: only fcntl and errno access run between fork and exec. The
        // pinned file outlives spawn, which returns after exec or its failure.
        unsafe {
            command.pre_exec(move || {
                let flags = libc::fcntl(fd, libc::F_GETFD);
                if flags < 0 || libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        fd
    }

    pub fn anchor(&self, command: &mut Command) -> Result<()> {
        use std::os::{fd::AsRawFd, unix::process::CommandExt};
        let fd = self.file.as_raw_fd();
        // Only async-signal-safe work between fork and exec. The owned file is
        // alive until spawn returns; CLOEXEC closes it after fchdir succeeds.
        unsafe {
            command.pre_exec(move || {
                if libc::fchdir(fd) == 0 {
                    Ok(())
                } else {
                    Err(std::io::Error::last_os_error())
                }
            });
        }
        Ok(())
    }
}

#[cfg(unix)]
fn c_name(name: &std::ffi::OsStr) -> Result<std::ffi::CString> {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::CString::new(name.as_bytes()).map_err(|e| e.to_string())
}

#[cfg(unix)]
fn open_at(parent: &File, name: &std::ffi::OsStr) -> Result<File> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let name = c_name(name)?;
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(format!(
            "Cannot pin clone directory: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

#[cfg(windows)]
impl Directory {
    pub fn child(&self, name: &str) -> Result<Self> {
        if name.is_empty() || name.contains(['/', '\\', ':']) || name == "." || name == ".." {
            return Err("Invalid directory entry name".into());
        }
        self.verify()?;
        let child=Self::open(&self.path.join(name))?;
        self.verify()?;
        Ok(child)
    }

    /// The retained Windows directory and ancestors prohibit rename/reparse
    /// replacement while this new entry is published without clobbering one.
    pub fn write_new(&self, name: &str, bytes: &[u8]) -> Result<()> {
        self.write_new_file(name,bytes).map(drop)
    }

    /// The same publication, retaining the exact file created by this call.
    /// A later pathname open cannot stand in for that original object.
    pub fn write_new_file(&self, name: &str, bytes: &[u8]) -> Result<File> {
        self.publish_file(name, bytes, false)
    }

    /// Replace metadata under caller-held exclusion, through this original
    /// directory. This does not establish any action or consent authority.
    pub(in crate::state) fn replace_file(&self, name: &str, bytes: &[u8]) -> Result<()> {
        self.replace_file_retained(name, bytes).map(drop)
    }
    pub(in crate::state) fn replace_file_retained(&self, name: &str, bytes: &[u8]) -> Result<File> {
        self.publish_file(name, bytes, true)
    }

    fn publish_file(&self, name: &str, bytes: &[u8], replace: bool) -> Result<File> {
        use std::io::Write;
        if name.is_empty() || name.contains(['/', '\\', ':']) || name == "." || name == ".." {
            return Err("Invalid file name".into());
        }
        self.verify()?;
        let mut file = tempfile::Builder::new().prefix(".hgs-new-")
            .tempfile_in(&self.path).map_err(|e| e.to_string())?;
        file.write_all(bytes).map_err(|e| e.to_string())?;
        file.as_file().sync_all().map_err(|e| e.to_string())?;
        let file = if replace {
            crate::platform::persist(file, self.path.join(name))
        } else {
            file.persist_noclobber(self.path.join(name))
        }.map_err(|e| e.error.to_string())?;
        self.verify()?;
        Ok(file)
    }

    pub fn open(path: &Path) -> Result<Self> {
        use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
        use windows_sys::Win32::Storage::FileSystem::*;
        let mut files = Vec::new();
        // Retain every ancestor without delete/write sharing. Besides renames,
        // refuse concurrent write handles that can change reparse data.
        for ancestor in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
            let file = OpenOptions::new()
                .read(true)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .open(ancestor)
                .map_err(|e| format!("Cannot pin clone directory: {e}"))?;
            let meta = file.metadata().map_err(|e| e.to_string())?;
            if !meta.is_dir() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                return Err("Clone path changed or contains a reparse point".into());
            }
            files.push(file);
        }
        let file = files.pop().ok_or("Missing clone directory")?;
        Ok(Self {
            path: path.into(),
            file,
            _ancestors: files,
        })
    }

    pub fn create(&self, name: &str, _staging: &str) -> Result<Self> {
        use std::os::windows::io::{AsRawHandle, FromRawHandle};
        use windows_sys::{
            Wdk::{Foundation::OBJECT_ATTRIBUTES, Storage::FileSystem::*},
            Win32::{
                Foundation::{HANDLE, UNICODE_STRING},
                Storage::FileSystem::*,
                System::IO::IO_STATUS_BLOCK,
            },
        };
        self.verify()?;
        let mut wide: Vec<u16> = name.encode_utf16().collect();
        let string = UNICODE_STRING {
            Length: (wide.len() * 2) as u16,
            MaximumLength: (wide.len() * 2) as u16,
            Buffer: wide.as_mut_ptr(),
        };
        let attrs = OBJECT_ATTRIBUTES {
            Length: std::mem::size_of::<OBJECT_ATTRIBUTES>() as u32,
            RootDirectory: self.file.as_raw_handle(),
            ObjectName: &string,
            Attributes: 0x40, // OBJ_CASE_INSENSITIVE
            SecurityDescriptor: std::ptr::null_mut(),
            SecurityQualityOfService: std::ptr::null_mut(),
        };
        let mut handle: HANDLE = std::ptr::null_mut();
        let mut status: IO_STATUS_BLOCK = unsafe { std::mem::zeroed() };
        // FILE_CREATE returns a handle to the new directory atomically. There
        // is no create/reopen window, nor delete sharing during native Git.
        let code = unsafe {
            NtCreateFile(
                &mut handle,
                // private() checks owner/DACL through this original handle;
                // GetSecurityInfo requires READ_CONTROL, not just attributes.
                FILE_LIST_DIRECTORY | FILE_TRAVERSE | FILE_READ_ATTRIBUTES | READ_CONTROL | 0x100000,
                &attrs,
                &mut status,
                std::ptr::null(),
                FILE_ATTRIBUTE_NORMAL,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                FILE_CREATE,
                FILE_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT | FILE_OPEN_REPARSE_POINT,
                std::ptr::null(),
                0,
            )
        };
        if code < 0 {
            return Err(format!(
                "Cannot reserve clone directory (NTSTATUS {code:#x})"
            ));
        }
        let file = unsafe { File::from_raw_handle(handle) };
        let directory = Self {
            path: self.path.join(name),
            file,
            _ancestors: Vec::new(),
        };
        directory.verify()?;
        Ok(directory)
    }

    pub fn anchor(&self, command: &mut Command) -> Result<()> {
        self.verify()?;
        command.current_dir(&self.path);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_destination_is_never_replaced() {
        let temp = tempfile::tempdir().unwrap();
        let path = crate::platform::canonical(temp.path()).unwrap();
        let parent = Directory::open(&path).unwrap();
        std::fs::create_dir(path.join("existing")).unwrap();
        std::fs::write(path.join("existing/foreign"), "retained").unwrap();
        assert!(parent.create("existing", ".staging").is_err());
        assert_eq!(
            std::fs::read_to_string(path.join("existing/foreign")).unwrap(),
            "retained"
        );
    }

    #[cfg(unix)]
    #[test]
    fn parent_replacement_before_creation_refuses_all_mutation() {
        let temp = tempfile::tempdir().unwrap();
        let path = crate::platform::canonical(temp.path()).unwrap();
        std::fs::create_dir(path.join("parent")).unwrap();
        let parent = Directory::open(&path.join("parent")).unwrap();
        std::fs::rename(path.join("parent"), path.join("owned")).unwrap();
        std::fs::create_dir(path.join("parent")).unwrap();
        std::fs::write(path.join("parent/foreign"), "retained").unwrap();
        assert!(parent.create("checkout", ".staging").is_err());
        assert_eq!(std::fs::read_dir(path.join("parent")).unwrap().count(), 1);
        assert_eq!(std::fs::read_dir(path.join("owned")).unwrap().count(), 0);
        // Causal control: the former pathname-based mkdir accepts this replacement.
        std::fs::create_dir(path.join("parent/checkout")).unwrap();
        assert!(path.join("parent/checkout").is_dir());
    }

    #[cfg(windows)]
    #[test]
    fn a_created_directory_handle_can_read_its_original_security() {
        let temp = tempfile::tempdir().unwrap();
        let path = crate::platform::canonical(temp.path()).unwrap();
        let parent = Directory::open(&path).unwrap();
        let created = parent.create("store", "unused").unwrap();
        // Keep the creation handle: reopening first would hide missing rights
        // because Directory::open requests GENERIC_READ (with READ_CONTROL).
        let expected =
            crate::platform::verify_private(&created.path, crate::platform::Private::Directory)
                .unwrap();
        assert_eq!(created.private().unwrap(), expected);
        assert_eq!(
            Directory::open(&created.path).unwrap().private().unwrap(),
            expected
        );
        created.verify().unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn pinned_ancestors_and_destination_deny_replacement() {
        let temp = tempfile::tempdir().unwrap();
        let path = crate::platform::canonical(temp.path()).unwrap();
        std::fs::create_dir(path.join("parent")).unwrap();
        let parent = Directory::open(&path.join("parent")).unwrap();
        let owned = parent.create("checkout", "unused").unwrap();
        assert!(std::fs::rename(&owned.path, path.join("moved-child")).is_err());
        assert!(std::fs::rename(&parent.path, path.join("moved-parent")).is_err());
        owned.verify().unwrap();
        drop(owned);
        drop(parent);
        // Causal control: identical renames succeed after releasing the handles.
        std::fs::rename(path.join("parent/checkout"), path.join("moved-child")).unwrap();
        std::fs::rename(path.join("parent"), path.join("moved-parent")).unwrap();
    }
}
