//! The client's staging of a reference original's raw attachments into the
//! service's staging directory (zerus-5wmh.66.4.4.2, under G1's boundary).
//!
//! The directory comes only from the service's read-only discovery (routing
//! data, never permission). It is held as a pinned object, never followed:
//! every create and delete is anchored on it with `O_NOFOLLOW|O_NONBLOCK|
//! O_CLOEXEC` at open. It must be a private directory of this user. This
//! client creates one new private directory per request, `<request_id>`, and
//! in it one private file per distinct attachment, named by the SHA-256 of
//! its raw bytes, created once (`O_EXCL`) and synced.
//!
//! Staged files are disposable COPIES: the draft keeps the original
//! attachments. Only a definite never-sent answer removes them, and then only
//! the exact objects this client created; after a request was offered, only
//! the service's read-only lookup decides.
use sha2::{Digest, Sha256};
use std::ffi::CString;
use std::fs::File;
use std::io::{self, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::MetadataExt;
use std::path::Path;

/// The staging directory is missing, unsafe or not this request's.
pub const UNAVAILABLE: &str = "task_message_staging_unavailable";

type Identity = (u64, u64);

#[cfg(test)]
type Hook = Box<dyn FnOnce()>;
#[cfg(test)]
thread_local! {
    /// Runs once, right after the request's new folder is held, so a test can
    /// race an object into it.
    static AFTER_FOLDER: std::cell::RefCell<Option<Hook>> = const { std::cell::RefCell::new(None) };
}

fn identity(file: &File) -> io::Result<Identity> {
    let metadata = file.metadata()?;
    Ok((metadata.dev(), metadata.ino()))
}

fn private(file: &File, directory: bool) -> io::Result<bool> {
    let metadata = file.metadata()?;
    // SAFETY: geteuid has no preconditions.
    let owner = metadata.uid() == unsafe { libc::geteuid() };
    let kind = if directory {
        metadata.is_dir()
    } else {
        metadata.is_file() && metadata.nlink() == 1
    };
    Ok(kind && owner && metadata.mode() & 0o077 == 0)
}

fn component(name: &str) -> io::Result<CString> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    CString::new(name).map_err(|_| io::ErrorKind::InvalidInput.into())
}

fn open_at(parent: &File, name: &str, flags: libc::c_int, mode: libc::mode_t) -> io::Result<File> {
    let name = component(name)?;
    // SAFETY: a held parent and one NUL-free component; the new descriptor is
    // owned by the returned File.
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
            libc::c_uint::from(mode),
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: openat returned a new owned descriptor.
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn unlink_at(parent: &File, name: &str, flags: libc::c_int) -> io::Result<()> {
    let name = component(name)?;
    // SAFETY: as in `open_at`; no descriptor is created.
    if unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), flags) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// One request's staged copies, exactly as this client created them.
pub struct Staged {
    root: File,
    request: String,
    directory: Identity,
    files: Vec<(String, Identity)>,
}

impl Staged {
    /// Removes exactly the copies this client created, each only while its
    /// name still denotes it, then the request's directory if it is empty.
    pub fn remove(self) -> io::Result<()> {
        let dir = open_at(
            &self.root,
            &self.request,
            libc::O_RDONLY | libc::O_DIRECTORY,
            0,
        )?;
        if identity(&dir)? != self.directory {
            return Err(io::ErrorKind::InvalidData.into());
        }
        for (name, expected) in &self.files {
            match open_at(&dir, name, libc::O_RDONLY | libc::O_NOCTTY, 0) {
                Ok(file) if identity(&file)? == *expected => unlink_at(&dir, name, 0)?,
                Ok(_) => return Err(io::ErrorKind::InvalidData.into()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        dir.sync_all()?;
        match unlink_at(&self.root, &self.request, libc::AT_REMOVEDIR) {
            Err(error) if error.raw_os_error() == Some(libc::ENOTEMPTY) => {}
            other => other?,
        }
        self.root.sync_all()
    }
}

/// Stages `blobs` (raw bytes, in the original's order) for `request` under
/// the discovered `root`. A request directory that already exists is never
/// written into. Any failure removes what this call created.
pub fn stage(root: &Path, request: &str, blobs: &[&[u8]]) -> Result<Staged, &'static str> {
    use std::os::unix::fs::OpenOptionsExt;
    let held = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(root)
        .map_err(|_| UNAVAILABLE)?;
    if !private(&held, true).unwrap_or(false) {
        return Err(UNAVAILABLE);
    }
    let name = component(request).map_err(|_| UNAVAILABLE)?;
    // SAFETY: as in `open_at`; mkdirat creates no descriptor.
    if unsafe { libc::mkdirat(held.as_raw_fd(), name.as_ptr(), 0o700) } < 0 {
        return Err(UNAVAILABLE);
    }
    let dir = open_at(&held, request, libc::O_RDONLY | libc::O_DIRECTORY, 0);
    let dir = match dir {
        Ok(dir) if private(&dir, true).unwrap_or(false) => dir,
        _ => return Err(UNAVAILABLE),
    };
    #[cfg(test)]
    if let Some(hook) = AFTER_FOLDER.with(|hook| hook.borrow_mut().take()) {
        hook();
    }
    let mut staged = Staged {
        directory: identity(&dir).map_err(|_| UNAVAILABLE)?,
        root: held,
        request: request.into(),
        files: Vec::new(),
    };
    let written = (|| -> io::Result<()> {
        for blob in blobs {
            let digest = format!("{:x}", Sha256::digest(blob));
            if staged.files.iter().any(|(name, _)| *name == digest) {
                continue;
            }
            let mut file = open_at(
                &dir,
                &digest,
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
                0o600,
            )?;
            staged.files.push((digest, identity(&file)?));
            file.write_all(blob)?;
            file.sync_all()?;
        }
        dir.sync_all()?;
        staged.root.sync_all()
    })();
    match written {
        Ok(()) => Ok(staged),
        Err(_) => {
            let _ = staged.remove();
            Err(UNAVAILABLE)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    /// A private scratch folder, removed when dropped.
    struct Scratch(PathBuf);
    impl Scratch {
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn fresh() -> (Scratch, PathBuf) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let base = std::env::temp_dir().join(format!(
            "zerus-staging-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let temp = Scratch(base);
        let root = temp.path().join("task-message-staging");
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        (temp, root)
    }
    const REQUEST: &str = "22222222-2222-4222-8222-222222222222";

    #[test]
    fn copies_are_private_named_by_their_raw_digest_and_removed_exactly() {
        let (_temp, root) = fresh();
        let shot = vec![7u8; 5000];
        let staged = stage(&root, REQUEST, &[&shot, b"note", &shot]).unwrap();
        let dir = root.join(REQUEST);
        assert_eq!(
            std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let file = dir.join(format!("{:x}", Sha256::digest(&shot)));
        assert_eq!(std::fs::read(&file).unwrap(), shot);
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::read_dir(&dir).unwrap().count(),
            2,
            "one copy per digest"
        );
        staged.remove().unwrap();
        assert!(!dir.exists());
    }

    #[test]
    fn an_unsafe_root_or_an_existing_request_folder_is_never_written() {
        let (_temp, root) = fresh();
        // A request folder that already exists is not ours, even a private one.
        std::fs::create_dir(root.join(REQUEST)).unwrap();
        std::fs::set_permissions(root.join(REQUEST), std::fs::Permissions::from_mode(0o700))
            .unwrap();
        assert_eq!(stage(&root, REQUEST, &[b"x"]).err(), Some(UNAVAILABLE));
        assert_eq!(std::fs::read_dir(root.join(REQUEST)).unwrap().count(), 0);
        // A root that is open to others, or a link to one.
        let (_temp, root) = fresh();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(stage(&root, REQUEST, &[b"x"]).err(), Some(UNAVAILABLE));
        let (temp, real) = fresh();
        let link = temp.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert_eq!(stage(&link, REQUEST, &[b"x"]).err(), Some(UNAVAILABLE));
        assert!(!real.join(REQUEST).exists());
    }

    #[test]
    fn a_copy_raced_into_the_new_folder_is_never_written_over() {
        let (_temp, root) = fresh();
        let copy = root
            .join(REQUEST)
            .join(format!("{:x}", Sha256::digest(b"note")));
        let planted = copy.clone();
        AFTER_FOLDER.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                std::fs::write(&planted, b"planted").unwrap();
                std::fs::set_permissions(&planted, std::fs::Permissions::from_mode(0o600)).unwrap();
            }))
        });
        assert_eq!(stage(&root, REQUEST, &[b"note"]).err(), Some(UNAVAILABLE));
        assert_eq!(std::fs::read(&copy).unwrap(), b"planted");
    }

    #[test]
    fn a_failed_stage_removes_exactly_the_copies_it_created() {
        let (_temp, root) = fresh();
        let (first, second) = (vec![1u8; 300], vec![2u8; 300]);
        let planted = root
            .join(REQUEST)
            .join(format!("{:x}", Sha256::digest(&second)));
        let plant = planted.clone();
        AFTER_FOLDER.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                std::fs::write(&plant, b"planted").unwrap();
            }))
        });
        assert_eq!(
            stage(&root, REQUEST, &[&first, &second]).err(),
            Some(UNAVAILABLE)
        );
        let copy = root
            .join(REQUEST)
            .join(format!("{:x}", Sha256::digest(&first)));
        assert!(!copy.exists(), "its own first copy went");
        assert_eq!(std::fs::read(&planted).unwrap(), b"planted");
    }

    #[test]
    fn removal_never_touches_an_object_it_did_not_create() {
        let (_temp, root) = fresh();
        let staged = stage(&root, REQUEST, &[b"note"]).unwrap();
        let dir = root.join(REQUEST);
        let copy = dir.join(format!("{:x}", Sha256::digest(b"note")));
        std::fs::remove_file(&copy).unwrap();
        std::fs::write(&copy, b"another").unwrap();
        assert!(staged.remove().is_err());
        assert!(copy.exists());
        // Anything else in the folder keeps the folder.
        let (_temp, root) = fresh();
        let staged = stage(&root, REQUEST, &[b"note"]).unwrap();
        std::fs::write(root.join(REQUEST).join("unrelated"), b"kept").unwrap();
        staged.remove().unwrap();
        assert!(root.join(REQUEST).join("unrelated").exists());
    }
}
