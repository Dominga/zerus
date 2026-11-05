//! The CLI's own state store as one original of a deferred kind reads it
//! (zerus-5wmh.66.4.4, slice 3), shared by the deferred send and deferred
//! answer actors. Each kind keeps its receipt folder, its reasons and its
//! matching rule in its own `receipt` module; the mechanics here are one.
//!
//! The CLI writes `<folder>/<request_id>.json` under its state root
//! atomically. The state root (and the kind's receipt folder, when it
//! exists) is opened as a no-follow, non-blocking directory descriptor
//! before the original child exists ([`Roots::open`]), and the receipt is
//! read after the physical drain from those exact objects ([`read`]). The
//! read is bracketed by the opened receipt's version, and the canonical
//! name, folder and root are checked again afterwards. A missing receipt is
//! [`Failure::Missing`] only while the held folder and root are still the
//! original's; anything else is [`Failure::Changed`] or
//! [`Failure::Unavailable`], never an absence.
//!
//! [`session_current`] is the selected session's actual backend and run,
//! read from the same held root without following a link: the CLI's own
//! session record for the name (its `name_routes` route or the legacy file)
//! must name this session, a terminal agent, and the run and conversation;
//! and neither an OpenCode nor a DeepSeek binding may exist for the name,
//! since the CLI routes those names to their own native backends. Anything
//! else, an unreadable record included, is not current.
use crate::platform;
use sha2::{Digest, Sha256};
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
use zerus_client_core::desktop::creation_launch::AGENTS;

/// Why a receipt read proved nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::desktop) enum Failure {
    /// No receipt of the request exists in the original, unchanged store.
    Missing,
    /// The receipt could not be read safely: an unsafe folder or file, an
    /// oversized one, or an I/O error.
    Unavailable,
    /// The original root, folder or receipt was replaced, moved or changed
    /// around the read.
    Changed,
}

/// The objects one read used, kept through the settlement's commit.
#[derive(Default)]
pub(in crate::desktop) struct Evidence {
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

/// The original receipt store: the CLI state root, and its kind's receipt
/// folder when it already existed, opened before the original child exists.
pub(in crate::desktop) struct Roots {
    path: PathBuf,
    root: File,
    root_id: Id,
    name: &'static str,
    folder: Option<(File, Id)>,
}
impl Roots {
    /// Opens the CLI state root `path` (its last component never a link) and
    /// its existing private receipt folder `name`.
    pub(in crate::desktop) fn open(path: &Path, name: &'static str) -> io::Result<Self> {
        // Never blocking (as `platform::never_block`), and a folder only.
        let parent = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_DIRECTORY)
            .open(path.parent().ok_or(io::ErrorKind::InvalidInput)?)?;
        let last = path
            .file_name()
            .and_then(|last| last.to_str())
            .ok_or(io::ErrorKind::InvalidInput)?;
        let root = directory(&parent, last)?;
        // The CLI keeps its state root owner-only.
        if !platform::verify_private_directory(&root)? {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        let root_id = id(&root.metadata()?);
        let folder = match directory(&root, name) {
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
            name,
            folder,
        };
        // The path still names the opened root.
        if !roots.root_current() {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        Ok(roots)
    }
    /// The state path, every ancestor included, still names the held root.
    pub(in crate::desktop) fn root_current(&self) -> bool {
        std::fs::symlink_metadata(&self.path)
            .is_ok_and(|metadata| metadata.is_dir() && id(&metadata) == self.root_id)
    }
    /// The held root still names `folder` as its receipt folder.
    fn folder_current(&self, folder: Id) -> bool {
        id_at(&self.root, self.name).is_ok_and(|current| current == folder)
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
    pub(in crate::desktop) fn absent(&self, path: &[&str]) -> io::Result<bool> {
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
    pub(in crate::desktop) fn open_file(&self, path: &[&str]) -> io::Result<File> {
        let (name, folders) = path.split_last().ok_or(io::ErrorKind::InvalidInput)?;
        let file = open_at(&self.walk(folders)?, name, libc::O_RDONLY)?;
        if !platform::verify_private_file(&file)? {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        Ok(file)
    }
}

/// Reads the receipt of `request` (a canonical UUID), at most `limit`
/// bytes, from `roots` after the physical drain. `between` runs after the
/// bytes are read and before they are checked again (tests change the store
/// there; it does nothing otherwise).
pub(in crate::desktop) fn read(
    roots: &Roots,
    request: &uuid::Uuid,
    limit: usize,
    between: &dyn Fn(),
) -> (Result<Vec<u8>, Failure>, Evidence) {
    let name = format!("{request}.json");
    let mut held = Vec::new();
    let observed = (|| -> Result<Vec<u8>, Failure> {
        // The held folder, or the one created since, opened from the held
        // root without following a link. Whether the path still names the
        // held root is checked after every observation, absence included.
        let (folder, folder_id) = match &roots.folder {
            Some((folder, folder_id)) => (
                folder.try_clone().map_err(|_| Failure::Unavailable)?,
                *folder_id,
            ),
            None => match directory(&roots.root, roots.name) {
                Ok(folder) => {
                    if !platform::verify_private_directory(&folder)
                        .map_err(|_| Failure::Unavailable)?
                    {
                        return Err(Failure::Unavailable);
                    }
                    let folder_id = id(&folder.metadata().map_err(|_| Failure::Unavailable)?);
                    (folder, folder_id)
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    // A safe absence: the held root is still the original.
                    return Err(if roots.root_current() {
                        Failure::Missing
                    } else {
                        Failure::Changed
                    });
                }
                Err(_) => return Err(Failure::Unavailable),
            },
        };
        if !roots.folder_current(folder_id) {
            return Err(Failure::Changed);
        }
        let file = match open_at(&folder, &name, libc::O_RDONLY) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                held.push(folder);
                return Err(if roots.folder_current(folder_id) && roots.root_current() {
                    Failure::Missing
                } else {
                    Failure::Changed
                });
            }
            Err(_) => return Err(Failure::Unavailable),
        };
        let before = file.metadata().map_err(|_| Failure::Unavailable)?;
        if !platform::verify_private_file(&file).map_err(|_| Failure::Unavailable)?
            || before.len() > limit as u64
        {
            return Err(Failure::Unavailable);
        }
        let mut bytes = Vec::new();
        (&file)
            .take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| Failure::Unavailable)?;
        between();
        // The opened version did not change during the read, its canonical
        // name still names it, and the folder and root are still the
        // original's.
        let after = file.metadata().map_err(|_| Failure::Unavailable)?;
        let unchanged = version(&before) == version(&after)
            && bytes.len() as u64 == after.len()
            && id_at(&folder, &name).is_ok_and(|current| current == id(&before))
            && roots.folder_current(folder_id)
            && roots.root_current();
        held.push(folder);
        held.push(file);
        if !unchanged {
            return Err(Failure::Changed);
        }
        if bytes.len() > limit {
            return Err(Failure::Unavailable);
        }
        Ok(bytes)
    })();
    (observed, Evidence { _held: held })
}

/// The largest session record read.
const RECORD_BYTES: u64 = 1 << 20;
/// The largest label-index manifest read, as the CLI bounds it.
const MANIFEST_BYTES: u64 = 16 << 20;

fn string<'a>(value: &'a serde_json::Value, key: &str) -> &'a str {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
}

/// Whether exactly one of `session`'s records is a terminal agent's, running
/// `run` and `conversation`, with no native binding. Those records are the
/// CLI's own record for the name and, where the ID layout exists (zerus-ucck
/// P3a), every session the label index proves for it.
pub(in crate::desktop) fn session_current(
    roots: &Roots,
    session: &str,
    run: &str,
    conversation: &str,
) -> bool {
    let file = format!("{:x}.json", Sha256::digest(session.as_bytes()));
    // `hgs send` sends an OpenCode or DeepSeek name natively.
    for backend in ["opencode", "dsh"] {
        match roots.absent(&[backend, "bindings", &file]) {
            Ok(true) => (),
            _ => return false,
        }
    }
    // The CLI's record path: a routed record lives in `bindings`.
    let routed = match roots.absent(&["name_routes", &file]) {
        Ok(absent) => !absent,
        Err(_) => return false,
    };
    let path: &[&str] = if routed {
        &["bindings", &file]
    } else {
        &[&file]
    };
    let names_plan = |record: &serde_json::Value| {
        string(record, "name") == session
            && AGENTS.contains(&string(record, "agent"))
            && session
                .split('/')
                .next()
                .is_some_and(|agent| agent == string(record, "agent"))
            && string(record, "run_id") == run
            && string(record, "conversation_id") == conversation
    };
    // The label's own record, read whole: one that is there but cannot be
    // read refuses, as an unreadable candidate does.
    let legacy = match bytes_at(roots, path, RECORD_BYTES) {
        Ok(None) => false,
        Ok(Some(bytes)) => match serde_json::from_slice::<serde_json::Value>(&bytes) {
            Ok(record) => names_plan(&record),
            Err(_) => return false,
        },
        Err(_) => return false,
    };
    // With no `sessions/` there is no ID layout (zerus-ucck P3a): the label's
    // own record decides, as before.
    match roots.absent(&["sessions"]) {
        Ok(true) => return legacy,
        Ok(false) => {}
        Err(_) => return false,
    }
    // Otherwise the label index must prove the label's sessions in the ID
    // layout before any answer (#1437 review), through the CLI's own proof.
    // Each candidate is opened at its fixed `sessions/<id>.json`, never a
    // path from elsewhere; one that is there but cannot be read refuses. The
    // label's own record and the candidates count together: exactly one may
    // carry the run and conversation.
    let Some(manifest) = bytes_at(roots, &["labels", ".complete"], MANIFEST_BYTES).ok().flatten() else {
        return false;
    };
    let Some(ids) = crate::state::prove_label_index(session, &manifest, || {
        bytes_at(roots, &["labels", &file], RECORD_BYTES).ok().flatten()
    }) else {
        return false;
    };
    let mut found = usize::from(legacy);
    for id in ids {
        match bytes_at(roots, &["sessions", &format!("{id}.json")], RECORD_BYTES) {
            // A stale entry: the session left the label.
            Ok(None) => {}
            Ok(Some(bytes)) => {
                let Ok(record) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                    return false;
                };
                if record["layout"] == "id"
                    && string(&record, "session_id") == id
                    && names_plan(&record)
                {
                    found += 1;
                }
            }
            Err(_) => return false,
        }
    }
    found == 1
}

/// The bytes at `path` under the held root, bounded: `Ok(None)` only when it
/// verifiably does not exist; anything there that cannot be read is an error.
fn bytes_at(roots: &Roots, path: &[&str], limit: u64) -> io::Result<Option<Vec<u8>>> {
    let file = match roots.open_file(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    (&file).take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(Some(bytes))
}
