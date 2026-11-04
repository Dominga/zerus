//! The staging and input-custody directories of reference originals, and the
//! one preparation that turns staged attachments into an exact input
//! (zerus-5wmh.66.4.4.2, under G1's boundary).
//!
//! Both directories live under this service's own desktop state root and are
//! held open from startup. Every open, create and delete is anchored on a held
//! directory, with `O_NOFOLLOW|O_NONBLOCK|O_CLOEXEC` at open, never a later
//! fixup. Every object must be owner-only: a directory `0700` of this user,
//! a regular file `0600` of this user with exactly one link. A replaced
//! directory, an unsafe mode, another owner, a special file or a hard link is
//! refused, never followed or repaired.
//!
//! Staged attachments are disposable COPIES: the client's draft keeps the
//! original attachments, so removing a staged copy never loses user data. The
//! custody input is the exact stdin of one admitted original. It is created
//! once (`O_EXCL`), synced, then read only through the handle kept here; it is
//! never reopened by name for execution and never overwritten.
// Preparation runs on a bounded worker and its caller lands with the runtime
// integration; until then only the tests use it.
#![cfg_attr(not(test), allow(dead_code))]
use crate::platform;
use sha2::{Digest, Sha256};
use std::ffi::CString;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::Instant;
use zerus_client_core::desktop::task_message::Verified;

/// The staging directory's name under the service's desktop state root.
pub(super) const STAGING: &str = "task-message-staging";
/// The input-custody directory's name under the same root.
pub(super) const INPUTS: &str = "task-message-inputs";

/// The staged attachments are missing, unsafe or not this request's.
pub(super) const UNAVAILABLE: &str = "task_message_staging_unavailable";
/// A staged attachment or the rebuilt input is not the original's.
pub(super) const MISMATCH: &str = "task_message_reference_mismatch";
/// The custody input already exists: it is never overwritten.
pub(super) const EXISTS: &str = "task_message_input_exists";
/// Preparation ran past its deadline.
pub(super) const EXPIRED: &str = "task_message_preparation_expired";

/// A device and inode: what a name must still denote.
pub(super) type Identity = (u64, u64);

fn identity(file: &File) -> io::Result<Identity> {
    let metadata = file.metadata()?;
    Ok((metadata.dev(), metadata.ino()))
}

/// One path component, never `.`, `..` or anything with a separator.
fn component(name: &str) -> io::Result<CString> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    CString::new(name).map_err(|_| io::ErrorKind::InvalidInput.into())
}

/// `openat` relative to a held directory, with the flags at open.
fn open_at(parent: &File, name: &str, flags: libc::c_int, mode: libc::mode_t) -> io::Result<File> {
    let name = component(name)?;
    // SAFETY: the parent is a held descriptor and the name one NUL-free
    // component; the new descriptor is owned by the returned File.
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
    // SAFETY: as in `open_at`; unlinkat creates no descriptor.
    if unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), flags) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// A held owner-only directory under the service root.
pub(super) struct Held {
    path: PathBuf,
    file: File,
    identity: Identity,
}
impl Held {
    /// Creates the directory `0700` if it is missing, then holds it. An
    /// existing one must already be a private directory of this user.
    fn open(root: &File, root_path: &Path, name: &str) -> io::Result<Self> {
        let c = component(name)?;
        // SAFETY: as in `open_at`.
        if unsafe { libc::mkdirat(root.as_raw_fd(), c.as_ptr(), 0o700) } < 0 {
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::AlreadyExists {
                return Err(error);
            }
        }
        let file = open_at(root, name, libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
        if !platform::verify_private_directory(&file)? {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        Ok(Self {
            path: root_path.join(name),
            identity: identity(&file)?,
            file,
        })
    }
    pub(super) fn path(&self) -> &Path {
        &self.path
    }
    fn sync(&self) -> io::Result<()> {
        self.file.sync_all()
    }
}

/// The two held directories of this service.
pub(super) struct Roots {
    root: File,
    root_path: PathBuf,
    pub(super) staging: Held,
    pub(super) inputs: Held,
}
impl Roots {
    /// Holds (creating if missing) both directories under `root`, which must
    /// itself be a private directory of this user.
    pub(super) fn open(root: &Path) -> io::Result<Self> {
        use std::os::unix::fs::OpenOptionsExt;
        let held = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(root)?;
        if !platform::verify_private_directory(&held)? {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        let staging = Held::open(&held, root, STAGING)?;
        let inputs = Held::open(&held, root, INPUTS)?;
        held.sync_all()?;
        Ok(Self {
            root: held,
            root_path: root.into(),
            staging,
            inputs,
        })
    }
    /// Whether both names still denote the held, still private directories.
    pub(super) fn current(&self) -> bool {
        [&self.staging, &self.inputs].into_iter().all(|held| {
            let name = held.path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            open_at(&self.root, name, libc::O_RDONLY | libc::O_DIRECTORY, 0)
                .and_then(|file| identity(&file))
                .is_ok_and(|now| now == held.identity)
                && platform::verify_private_directory(&held.file).unwrap_or(false)
        }) && std::fs::metadata(&self.root_path)
            .is_ok_and(|m| (m.dev(), m.ino()) == identity(&self.root).unwrap_or((0, 0)))
    }
}

/// The exact stdin of one admitted original, held through its own handle.
pub(super) struct Input {
    pub(super) file: File,
    pub(super) identity: Identity,
    pub(super) bytes: u64,
    pub(super) sha256: String,
}

/// One prepared reference original.
pub(super) struct Prepared {
    /// The original call's arguments; its stdin is `input`, not a string.
    pub(super) args: Vec<String>,
    pub(super) input: Input,
    /// The staged copies it read, by name and identity, for exact removal.
    pub(super) staged: Vec<(String, Identity)>,
}

/// Reads one staged attachment: a private regular file of exactly `size`
/// bytes whose digest is `sha256`.
fn staged(dir: &File, sha256: &str, size: u64) -> Result<(Vec<u8>, Identity), &'static str> {
    let mut file =
        open_at(dir, sha256, libc::O_RDONLY | libc::O_NOCTTY, 0).map_err(|_| UNAVAILABLE)?;
    if !platform::verify_private_file(&file).unwrap_or(false) {
        return Err(UNAVAILABLE);
    }
    if file.metadata().map_err(|_| UNAVAILABLE)?.len() != size {
        return Err(MISMATCH);
    }
    let mut bytes = Vec::with_capacity(size as usize);
    (&mut file)
        .take(size + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| UNAVAILABLE)?;
    if bytes.len() as u64 != size || format!("{:x}", Sha256::digest(&bytes)) != sha256 {
        return Err(MISMATCH);
    }
    Ok((bytes, identity(&file).map_err(|_| UNAVAILABLE)?))
}

/// The one preparation of a reference original: reads its staged copies from
/// `<staging>/<request_id>/<sha256>`, rebuilds the exact call, then creates
/// and syncs `<inputs>/<request_id>` once with the complete stdin. Nothing
/// is admitted here, and no child starts. Any refusal before the custody
/// input exists leaves nothing behind; a refusal after it was created removes
/// exactly that new file.
pub(super) fn prepare(
    roots: &Roots,
    verified: &Verified,
    deadline: Instant,
) -> Result<Prepared, &'static str> {
    let expired = || Instant::now() >= deadline;
    if !roots.current() {
        return Err(UNAVAILABLE);
    }
    let request = &verified.plan.request_id;
    let dir = open_at(
        &roots.staging.file,
        request,
        libc::O_RDONLY | libc::O_DIRECTORY,
        0,
    )
    .map_err(|_| UNAVAILABLE)?;
    if !platform::verify_private_directory(&dir).unwrap_or(false) {
        return Err(UNAVAILABLE);
    }
    let mut blobs = Vec::with_capacity(verified.references.len());
    let mut copies: Vec<(String, Identity)> = Vec::new();
    for reference in &verified.references {
        if expired() {
            return Err(EXPIRED);
        }
        let (bytes, id) = staged(&dir, &reference.sha256, reference.size)?;
        if !copies.iter().any(|(name, _)| *name == reference.sha256) {
            copies.push((reference.sha256.clone(), id));
        }
        blobs.push(bytes);
    }
    let views: Vec<&[u8]> = blobs.iter().map(Vec::as_slice).collect();
    let call = verified.rebuild(&views)?;
    drop(blobs);
    if Some(call.input.len() as u64) != verified.input_len()
        || format!("{:x}", Sha256::digest(call.input.as_bytes())) != verified.plan.input_sha256
    {
        return Err(MISMATCH);
    }
    if expired() || !roots.current() {
        return Err(EXPIRED);
    }
    let mut file = open_at(
        &roots.inputs.file,
        request,
        libc::O_RDWR | libc::O_CREAT | libc::O_EXCL,
        0o600,
    )
    .map_err(|error| {
        if error.kind() == io::ErrorKind::AlreadyExists {
            EXISTS
        } else {
            UNAVAILABLE
        }
    })?;
    let created = identity(&file).map_err(|_| UNAVAILABLE)?;
    let written = file
        .write_all(call.input.as_bytes())
        .and_then(|()| file.sync_all())
        .and_then(|()| roots.inputs.sync());
    if written.is_err() || expired() {
        // Exactly the file this call created, never another one.
        let _ = remove_input(roots, request, created);
        return Err(if written.is_err() {
            UNAVAILABLE
        } else {
            EXPIRED
        });
    }
    let invocation = call.invocation();
    Ok(Prepared {
        args: invocation.args,
        input: Input {
            file,
            identity: created,
            bytes: call.input.len() as u64,
            sha256: verified.plan.input_sha256.clone(),
        },
        staged: copies,
    })
}

/// Removes exactly the staged copies a preparation read, each only while its
/// name still denotes the same object, then the request's directory if it is
/// empty. Anything else in it stays.
pub(super) fn remove_staged(
    roots: &Roots,
    request: &str,
    staged: &[(String, Identity)],
) -> io::Result<()> {
    let dir = open_at(
        &roots.staging.file,
        request,
        libc::O_RDONLY | libc::O_DIRECTORY,
        0,
    )?;
    for (name, expected) in staged {
        match open_at(&dir, name, libc::O_RDONLY | libc::O_NOCTTY, 0) {
            Ok(file) if identity(&file)? == *expected => unlink_at(&dir, name, 0)?,
            Ok(_) => return Err(io::ErrorKind::InvalidData.into()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    dir.sync_all()?;
    match unlink_at(&roots.staging.file, request, libc::AT_REMOVEDIR) {
        Err(error) if error.raw_os_error() == Some(libc::ENOTEMPTY) => {}
        other => other?,
    }
    roots.staging.sync()
}

/// Removes the custody input of `request` only while its name still denotes
/// `expected`. A missing input was already removed.
pub(super) fn remove_input(roots: &Roots, request: &str, expected: Identity) -> io::Result<()> {
    match open_at(
        &roots.inputs.file,
        request,
        libc::O_RDONLY | libc::O_NOCTTY,
        0,
    ) {
        Ok(file) if identity(&file)? == expected => unlink_at(&roots.inputs.file, request, 0)?,
        Ok(_) => return Err(io::ErrorKind::InvalidData.into()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    roots.inputs.sync()
}
