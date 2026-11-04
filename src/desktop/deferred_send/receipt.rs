//! The read-only observation of one original's own CLI input receipt
//! (zerus-5wmh.66.4.4, slice 3, G1 condition 2), from the original receipt
//! store only.
//!
//! `hgs send` writes `input_receipts/<request_id>.json` under the CLI's state
//! root, atomically: `in_progress` before the first input byte reaches the
//! terminal, then `submitted` after its Enter. Only an exact `submitted`
//! receipt of this original is a submission; it still proves nothing about
//! whether the agent read the message.
//!
//! The state root (and the receipt folder, when it exists) is opened as a
//! no-follow, non-blocking directory descriptor before the original child
//! exists ([`Roots::open`]), and the receipt is read after the physical drain
//! from those exact objects ([`read`]). The read is bracketed by the opened
//! receipt's version, and the canonical name, folder and root are checked
//! again afterwards. Every other observation is Unknown and never authorizes
//! a resend: `in_progress`, a missing, corrupt, oversized, unsafe, replaced,
//! changing or mismatched receipt, folder or root.
use crate::platform;
use std::{
    ffi::CString,
    fs::{File, Metadata},
    io::{self, Read},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
};
use zerus_client_core::desktop::deferred_send::{Outcome, Plan, RECEIPT_BYTES, RECEIPT_MISMATCH};

/// No receipt of the request exists in the original, unchanged store.
pub(super) const MISSING: &str = "deferred_send_receipt_missing";
/// The receipt could not be read safely: an unsafe folder or file, or an
/// I/O error.
pub(super) const UNAVAILABLE: &str = "deferred_send_receipt_unavailable";
/// The original root, folder or receipt was replaced, moved or changed
/// around the read.
pub(super) const CHANGED: &str = "deferred_send_receipt_changed";
/// The read did not end within its one bounded deadline.
pub(super) const TIMED_OUT: &str = "deferred_send_receipt_timed_out";
/// No original store observation is held for it (a restart): never read.
pub(super) const UNHELD: &str = "deferred_send_receipt_root_unheld";
/// The receipt folder's name under the CLI state root.
const FOLDER: &str = "input_receipts";

/// What the original's own receipt says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Observed {
    /// The exact original's receipt is `submitted`.
    Submitted(Outcome),
    /// Anything else, for the named reason. Never resent.
    Unknown(&'static str),
}

/// The objects one read used, kept through the settlement's commit.
#[derive(Default)]
pub(super) struct Evidence {
    _held: Vec<File>,
}

/// An object's identity.
type Id = (u64, u64);
fn id(metadata: &Metadata) -> Id {
    (metadata.dev(), metadata.ino())
}
/// A file's identity and version: identity, size, change and modification
/// times.
fn version(metadata: &Metadata) -> (Id, u64, i64, i64, i64, i64) {
    (
        id(metadata),
        metadata.size(),
        metadata.mtime(),
        metadata.mtime_nsec(),
        metadata.ctime(),
        metadata.ctime_nsec(),
    )
}

/// Opens `name` in the held `parent` without following a link.
fn open_at(parent: &File, name: &str, flags: libc::c_int) -> io::Result<File> {
    let name = CString::new(name).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    // SAFETY: the parent is a held descriptor and the name one NUL-free
    // component; the new descriptor is owned by the returned File.
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC | libc::O_NOCTTY,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: openat returned a new owned descriptor.
    Ok(unsafe { File::from_raw_fd(fd) })
}

/// The identity `name` in the held `parent` names now, without following a
/// link.
fn id_at(parent: &File, name: &str) -> io::Result<Id> {
    let name = CString::new(name).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: a held descriptor, one NUL-free component and a stat buffer
    // that fstatat fills on success.
    let done = unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if done != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fstatat succeeded.
    let stat = unsafe { stat.assume_init() };
    #[allow(clippy::unnecessary_cast)]
    Ok((stat.st_dev as u64, stat.st_ino as u64))
}

fn directory(parent: &File, name: &str) -> io::Result<File> {
    open_at(parent, name, libc::O_RDONLY | libc::O_DIRECTORY)
}

/// The original receipt store: the CLI state root, and its receipt folder
/// when it already existed, opened before the original child exists.
pub(super) struct Roots {
    path: PathBuf,
    root: File,
    root_id: Id,
    folder: Option<(File, Id)>,
}
impl Roots {
    /// Opens the CLI state root `path` (its last component never a link) and
    /// its existing private receipt folder.
    pub(super) fn open(path: &Path) -> io::Result<Self> {
        // Never blocking (as `platform::never_block`), and a folder only.
        let parent = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_DIRECTORY)
            .open(path.parent().ok_or(io::ErrorKind::InvalidInput)?)?;
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(io::ErrorKind::InvalidInput)?;
        let root = directory(&parent, name)?;
        // The CLI keeps its state root owner-only.
        if !platform::verify_private_directory(&root)? {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        let root_id = id(&root.metadata()?);
        let folder = match directory(&root, FOLDER) {
            Ok(folder) => {
                if !platform::verify_private_directory(&folder)? {
                    return Err(io::ErrorKind::PermissionDenied.into());
                }
                let folder_id = id(&folder.metadata()?);
                Some((folder, folder_id))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        let roots = Self {
            path: path.to_path_buf(),
            root,
            root_id,
            folder,
        };
        // The path still names the opened root.
        if !roots.root_current() {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        Ok(roots)
    }
    /// The state path, every ancestor included, still names the held root.
    fn root_current(&self) -> bool {
        std::fs::symlink_metadata(&self.path)
            .is_ok_and(|metadata| metadata.is_dir() && id(&metadata) == self.root_id)
    }
    /// The held root still names `folder` as its receipt folder.
    fn folder_current(&self, folder: Id) -> bool {
        id_at(&self.root, FOLDER).is_ok_and(|current| current == folder)
    }
    /// The folder `path` names under the held root, each component opened
    /// without following a link.
    fn walk(&self, path: &[&str]) -> io::Result<File> {
        let mut folder = self.root.try_clone()?;
        for name in path {
            folder = directory(&folder, name)?;
        }
        Ok(folder)
    }
    /// Whether nothing exists at `path` under the held root, through
    /// folders only; a link anywhere is an error, never an absence.
    pub(super) fn absent(&self, path: &[&str]) -> io::Result<bool> {
        let (name, folders) = path.split_last().ok_or(io::ErrorKind::InvalidInput)?;
        let folder = match self.walk(folders) {
            Ok(folder) => folder,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(true),
            Err(error) => return Err(error),
        };
        match id_at(&folder, name) {
            Ok(_) => Ok(false),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(true),
            Err(error) => Err(error),
        }
    }
    /// The private regular file at `path` under the held root, opened
    /// without following a link or blocking.
    pub(super) fn open_file(&self, path: &[&str]) -> io::Result<File> {
        let (name, folders) = path.split_last().ok_or(io::ErrorKind::InvalidInput)?;
        let file = open_at(&self.walk(folders)?, name, libc::O_RDONLY)?;
        if !platform::verify_private_file(&file)? {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        Ok(file)
    }
}

/// Reads the original's receipt from `roots` after the physical drain.
/// `between` runs after the bytes are read and before they are checked
/// again (tests change the store there; it does nothing otherwise).
pub(super) fn read(roots: &Roots, plan: &Plan, between: &dyn Fn()) -> (Observed, Evidence) {
    let Ok(request) = uuid::Uuid::parse_str(&plan.request_id) else {
        return (Observed::Unknown(RECEIPT_MISMATCH), Evidence::default());
    };
    let name = format!("{request}.json");
    let mut held = Vec::new();
    let observed = (|| -> Result<Vec<u8>, &'static str> {
        // The held folder, or the one created since, opened from the held
        // root without following a link. Whether the path still names the
        // held root is checked after every observation, absence included.
        let (folder, folder_id) = match &roots.folder {
            Some((folder, folder_id)) => (folder.try_clone().map_err(|_| UNAVAILABLE)?, *folder_id),
            None => match directory(&roots.root, FOLDER) {
                Ok(folder) => {
                    if !platform::verify_private_directory(&folder).map_err(|_| UNAVAILABLE)? {
                        return Err(UNAVAILABLE);
                    }
                    let folder_id = id(&folder.metadata().map_err(|_| UNAVAILABLE)?);
                    (folder, folder_id)
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    // A safe absence: the held root is still the original.
                    return Err(if roots.root_current() {
                        MISSING
                    } else {
                        CHANGED
                    });
                }
                Err(_) => return Err(UNAVAILABLE),
            },
        };
        if !roots.folder_current(folder_id) {
            return Err(CHANGED);
        }
        let file = match open_at(&folder, &name, libc::O_RDONLY) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                held.push(folder);
                return Err(if roots.folder_current(folder_id) && roots.root_current() {
                    MISSING
                } else {
                    CHANGED
                });
            }
            Err(_) => return Err(UNAVAILABLE),
        };
        let before = file.metadata().map_err(|_| UNAVAILABLE)?;
        if !platform::verify_private_file(&file).map_err(|_| UNAVAILABLE)?
            || before.len() > RECEIPT_BYTES as u64
        {
            return Err(UNAVAILABLE);
        }
        let mut bytes = Vec::new();
        (&file)
            .take(RECEIPT_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| UNAVAILABLE)?;
        between();
        // The opened version did not change during the read, its canonical
        // name still names it, and the folder and root are still the
        // original's.
        let after = file.metadata().map_err(|_| UNAVAILABLE)?;
        let unchanged = version(&before) == version(&after)
            && bytes.len() as u64 == after.len()
            && id_at(&folder, &name).is_ok_and(|current| current == id(&before))
            && roots.folder_current(folder_id)
            && roots.root_current();
        held.push(folder);
        held.push(file);
        if !unchanged {
            return Err(CHANGED);
        }
        if bytes.len() > RECEIPT_BYTES {
            return Err(UNAVAILABLE);
        }
        Ok(bytes)
    })();
    let observed = match observed {
        // The wire owns the matching rule: the original's own `submitted`
        // record, or no outcome.
        Ok(bytes) => match Outcome::from_receipt(plan, &bytes) {
            Ok(outcome) => Observed::Submitted(outcome),
            Err(code) => Observed::Unknown(code),
        },
        Err(code) => Observed::Unknown(code),
    };
    (observed, Evidence { _held: held })
}
