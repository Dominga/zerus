//! Startup observations are DATA. Only an existing-data migration takes the
//! original handoff exclusion; ordinary passive startup does not acquire it.
use super::{database_files, Location, SCHEMA};
use crate::{platform, state::repositories::directory::Directory};
use anyhow::{ensure, Result};
use rusqlite::{Connection, OpenFlags};
#[cfg(any(windows, test))]
use std::fs;
use std::{fs::File, sync::Arc};

const DATABASE: &str = "state.sqlite3";
#[cfg(unix)]
pub(super) const PIN_LIMIT: usize = 64;

/// The registry, rather than an individual Store, owns every original Unix
/// descriptor until process teardown. Closing ANY raw descriptor of a SQLite
/// inode could release a DIFFERENT connection's process-scoped record locks.
/// There is deliberately no eviction, clearing, cloning of File, or rollback
/// of an installed entry, including when later validation fails.
#[cfg(unix)]
static PINS: std::sync::Mutex<Vec<Pin>> = std::sync::Mutex::new(Vec::new());

#[derive(Clone)]
pub(super) struct Pin {
    root: Arc<Directory>,
    file: Arc<File>,
    name: &'static str,
}
impl Pin {
    pub(super) fn root(&self) -> &Directory {
        &self.root
    }
    pub(super) fn file(&self) -> Arc<File> {
        Arc::clone(&self.file)
    }
    pub(super) fn identity(&self) -> Result<(u64, u64)> {
        let id = platform::FileIdentity::of(&self.file)?;
        ensure!(id.inode != 0, "desktop database identity unavailable");
        Ok((id.device, id.inode))
    }
    pub(super) fn existing(root: Arc<Directory>) -> Result<Self> {
        Self::acquire(root, DATABASE, false)
    }
    /// Existing backup inputs may be aliases of a live SQLite file too.
    /// Retain their raw descriptor before validation, using this SAME ceiling.
    pub(super) fn input(root: &Directory, name: &'static str) -> Result<Option<Self>> {
        root.verify().map_err(anyhow::Error::msg)?;
        if absent(root, name)? {
            return Ok(None);
        }
        let held = Arc::new(Directory::open(&root.path).map_err(anyhow::Error::msg)?);
        ensure!(
            held.object().map_err(anyhow::Error::msg)?
                == root.object().map_err(anyhow::Error::msg)?,
            "desktop input root changed"
        );
        root.verify().map_err(anyhow::Error::msg)?;
        #[cfg(unix)]
        {
            Self::acquire(held, name, false).map(Some)
        }
        #[cfg(windows)]
        {
            // Preserve the original backup input's no-follow access/sharing.
            // Windows locks are handle-scoped; only Unix needs retention.
            let Some(file) = held
                .read_private_optional(name)
                .map_err(anyhow::Error::msg)?
            else {
                return Ok(None);
            };
            let pin = Self {
                root: held,
                file: Arc::new(file),
                name,
            };
            pin.verify()?;
            Ok(Some(pin))
        }
    }
    fn acquire(root: Arc<Directory>, name: &'static str, create: bool) -> Result<Self> {
        root.verify().map_err(anyhow::Error::msg)?;
        #[cfg(unix)]
        {
            let mut pins = PINS
                .lock()
                .map_err(|_| anyhow::anyhow!("desktop pin registry unavailable"))?;
            if !create {
                let named = named_file(&root, name)?
                    .ok_or_else(|| anyhow::anyhow!("missing original desktop database"))?;
                let object = root.object().map_err(anyhow::Error::msg)?;
                for pin in pins.iter() {
                    if pin.name == name
                        && pin.root.object().map_err(anyhow::Error::msg)? == object
                        && pin.identity()? == named
                    {
                        pin.verify()?;
                        return Ok(pin.clone());
                    }
                }
            }
            // Reserve BOTH the ceiling and allocation before opening. A later
            // metadata/private/schema failure cannot close the new descriptor.
            ensure!(
                pins.len() < PIN_LIMIT,
                "desktop database pin limit reached; start a new hgs process"
            );
            pins.try_reserve(1)?;
            #[cfg(test)]
            if let Some(hook) = BEFORE_RAW_OPEN.with(|slot| slot.borrow_mut().take()) {
                hook(&root, name);
            }
            let file = open_original(&root, name, create)?;
            pins.push(Self {
                root,
                file: Arc::new(file),
                name,
            });
            let pin = pins.last().unwrap().clone();
            after_pin(&pin);
            pin.verify()?;
            Ok(pin)
        }
        #[cfg(windows)]
        {
            let file = open_original(&root, name, create)?;
            let pin = Self {
                root,
                file: Arc::new(file),
                name,
            };
            pin.verify()?;
            Ok(pin)
        }
    }
    pub(super) fn verify(&self) -> Result<()> {
        self.root.verify().map_err(anyhow::Error::msg)?;
        ensure!(
            platform::verify_private_file(&self.file)?,
            "desktop database must be a private single-link regular file"
        );
        #[cfg(unix)]
        let named = named_file(&self.root, self.name)?
            .ok_or_else(|| anyhow::anyhow!("missing original desktop database"))?;
        #[cfg(windows)]
        let named = {
            let id = platform::FileIdentity::at(&self.root.path.join(self.name))?.0;
            (id.device, id.inode)
        };
        ensure!(self.identity()? == named, "desktop database replaced");
        Ok(())
    }
    pub(super) fn bind(&self, db: &Connection) -> Result<()> {
        ensure!(self.name == DATABASE, "non-database startup pin");
        self.verify()?;
        #[cfg(unix)]
        {
            let (device, inode) = self.identity()?;
            crate::sqlite_file::bind_private(
                db,
                &self.root.path.join(self.name),
                crate::sqlite_file::Identity { device, inode },
            )?;
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::BorrowedHandle;
            let mut handle: *mut std::ffi::c_void = std::ptr::null_mut();
            // SAFETY: this live SQLite connection writes one borrowed HANDLE.
            let code = unsafe {
                rusqlite::ffi::sqlite3_file_control(
                    db.handle(),
                    c"main".as_ptr(),
                    rusqlite::ffi::SQLITE_FCNTL_WIN32_GET_HANDLE,
                    (&mut handle as *mut *mut std::ffi::c_void).cast(),
                )
            };
            ensure!(
                code == rusqlite::ffi::SQLITE_OK && !handle.is_null(),
                "desktop SQLite handle unavailable"
            );
            // Only Windows duplicates a native handle: locks are handle-scoped.
            let actual: File = unsafe { BorrowedHandle::borrow_raw(handle) }
                .try_clone_to_owned()?
                .into();
            let id = platform::FileIdentity::of(&actual)?;
            ensure!(
                self.identity()? == (id.device, id.inode),
                "desktop SQLite database changed"
            );
        }
        self.verify()
    }
}

#[cfg(unix)]
fn open_original(root: &Directory, name: &str, create: bool) -> Result<File> {
    use std::os::fd::FromRawFd;
    let name = std::ffi::CString::new(name)?;
    let flags = libc::O_RDONLY
        | libc::O_NOFOLLOW
        | libc::O_NONBLOCK
        | libc::O_NOCTTY
        | libc::O_CLOEXEC
        | if create {
            libc::O_CREAT | libc::O_EXCL
        } else {
            0
        };
    // SAFETY: one fixed component under the original pinned directory. The
    // returned descriptor is installed into process custody BEFORE validation.
    let fd = unsafe {
        libc::openat(
            root.descriptor(),
            name.as_ptr(),
            flags,
            0o600 as libc::c_uint,
        )
    };
    ensure!(
        fd >= 0,
        "open original desktop database: {}",
        std::io::Error::last_os_error()
    );
    Ok(unsafe { File::from_raw_fd(fd) })
}
#[cfg(windows)]
fn open_original(root: &Directory, name: &str, create: bool) -> Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};
    let mut options = fs::OpenOptions::new();
    options.read(true).write(create).create_new(create);
    // Root and all its original ancestors prohibit namespace replacement.
    platform::refuse_link(&mut options).share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE);
    Ok(options.open(root.path.join(name))?)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mode {
    Empty,
    Current,
    Migrate(i64),
}

pub(super) struct Startup<'a> {
    location: &'a Location,
    owner: &'a platform::FileLock,
    pin: Pin,
    mode: Mode,
}
impl<'a> Startup<'a> {
    pub(super) fn observe(location: &'a Location, owner: &'a platform::FileLock) -> Result<Self> {
        let root = Arc::new(Directory::open(&location.root).map_err(anyhow::Error::msg)?);
        ensure!(
            root.private().map_err(anyhow::Error::msg)?,
            "desktop startup root is not private"
        );
        verify_owner(&root, owner)?;
        database_files(location)?;
        let missing = absent(&root, DATABASE)?;
        if missing {
            empty_sidecars(&root)?;
            #[cfg(test)]
            BEFORE_CREATE.with(|hook| {
                if let Some(hook) = hook.borrow_mut().as_mut() {
                    hook(&root);
                }
            });
        }
        let pin = Pin::acquire(root, DATABASE, missing)?;
        let mode = inspect(&pin, location)?;
        #[cfg(test)]
        AFTER_OBSERVATION.with(|hook| {
            if let Some(hook) = hook.borrow_mut().as_mut() {
                hook(&pin);
            }
        });
        if missing {
            ensure!(mode == Mode::Empty, "new desktop database is not empty");
        }
        let startup = Self {
            location,
            owner,
            pin,
            mode,
        };
        startup.verify()?;
        Ok(startup)
    }
    pub(super) fn mode(&self) -> Mode {
        self.mode
    }
    pub(super) fn verify(&self) -> Result<()> {
        self.pin.verify()?;
        verify_owner(self.pin.root(), self.owner)
    }
    pub(super) fn reobserve(&self) -> Result<()> {
        self.verify()?;
        ensure!(
            inspect(&self.pin, self.location)? == self.mode,
            "desktop startup schema or shape changed"
        );
        self.verify()
    }
    pub(super) fn pin(&self) -> Pin {
        self.pin.clone()
    }
    pub(super) fn bind(&self, db: &Connection) -> Result<()> {
        self.verify()?;
        self.pin.bind(db)?;
        ensure!(
            inspect_connection(db, self.location, self.pin.root())? == self.mode,
            "desktop startup schema or shape changed"
        );
        self.verify()
    }
}

fn verify_owner(root: &Directory, owner: &platform::FileLock) -> Result<()> {
    check_exclusion(root, "owner.lock", owner.identity()?)
}
pub(super) fn check_exclusion(
    root: &Directory,
    name: &str,
    expected: platform::FileIdentity,
) -> Result<()> {
    root.verify().map_err(anyhow::Error::msg)?;
    #[cfg(unix)]
    {
        // A replaced lock name could alias the LIVE database. Never open and
        // then close that alias just to discover that its identity is wrong.
        let metadata = named_stat(root, name)?
            .ok_or_else(|| anyhow::anyhow!("desktop exclusion disappeared"))?;
        ensure!(
            metadata.st_mode & libc::S_IFMT == libc::S_IFREG
                && metadata.st_nlink == 1
                && metadata.st_uid == unsafe { libc::geteuid() }
                && metadata.st_mode & 0o077 == 0
                && metadata.st_mode & 0o400 != 0,
            "desktop exclusion must remain private and readable"
        );
        ensure!(
            platform::FileIdentity::at(&root.path.join(name))?.0 == expected,
            "desktop exclusion changed"
        );
        let after = named_stat(root, name)?
            .ok_or_else(|| anyhow::anyhow!("desktop exclusion disappeared"))?;
        #[allow(clippy::unnecessary_cast)]
        let object = (after.st_dev as u64, after.st_ino as u64);
        ensure!(
            object == (expected.device, expected.inode)
                && after.st_mode & libc::S_IFMT == libc::S_IFREG,
            "desktop exclusion changed"
        );
    }
    #[cfg(windows)]
    {
        let named = root.read_private(name).map_err(anyhow::Error::msg)?;
        ensure!(
            platform::FileIdentity::of(&named)? == expected,
            "desktop exclusion changed"
        );
    }
    root.verify().map_err(anyhow::Error::msg)
}
fn inspect(pin: &Pin, location: &Location) -> Result<Mode> {
    pin.verify()?;
    let db = Connection::open_with_flags(
        location.root.join(DATABASE),
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    pin.bind(&db)?;
    let tx = db.unchecked_transaction()?;
    let mode = inspect_connection(&tx, location, pin.root())?;
    pin.bind(&db)?;
    tx.commit()?;
    Ok(mode)
}
fn inspect_connection(db: &Connection, location: &Location, root: &Directory) -> Result<Mode> {
    let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    ensure!(
        (0..=SCHEMA).contains(&version),
        "desktop state requires a newer hgs version"
    );
    if version == 0 {
        // LIKE's underscore is a wildcard: sqliteLookalike is a user object.
        let objects: i64 = db.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*'",
            [],
            |r| r.get(0),
        )?;
        ensure!(objects == 0, "unversioned desktop state is not empty");
        empty_sidecars(root)?;
        return Ok(Mode::Empty);
    }
    let configuration: String =
        db.query_row("SELECT configuration FROM service WHERE id=1", [], |r| {
            r.get(0)
        })?;
    ensure!(
        configuration == location.configuration,
        "desktop state belongs to another configuration"
    );
    Ok(if version == SCHEMA {
        Mode::Current
    } else {
        Mode::Migrate(version)
    })
}
fn empty_sidecars(root: &Directory) -> Result<()> {
    for name in [
        "state.sqlite3-wal",
        "state.sqlite3-shm",
        "state.sqlite3-journal",
    ] {
        ensure!(
            absent(root, name)?,
            "unversioned desktop state has an ambiguous sidecar"
        );
    }
    Ok(())
}

#[cfg(unix)]
pub(super) fn named_stat(root: &Directory, name: &str) -> std::io::Result<Option<libc::stat>> {
    let name = std::ffi::CString::new(name)?;
    let mut metadata: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: fixed startup components below the SAME retained parent. Stat
    // never opens/closes a raw database descriptor, and never follows a link.
    if unsafe {
        libc::fstatat(
            root.descriptor(),
            name.as_ptr(),
            &mut metadata,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } == 0
    {
        return Ok(Some(metadata));
    }
    let error = std::io::Error::last_os_error();
    if error.kind() == std::io::ErrorKind::NotFound {
        Ok(None)
    } else {
        Err(error)
    }
}
#[cfg(unix)]
fn named_file(root: &Directory, name: &str) -> Result<Option<(u64, u64)>> {
    let Some(metadata) = named_stat(root, name)? else {
        return Ok(None);
    };
    ensure!(
        metadata.st_mode & libc::S_IFMT == libc::S_IFREG
            && metadata.st_nlink == 1
            && metadata.st_uid == unsafe { libc::geteuid() }
            && metadata.st_mode & 0o077 == 0,
        "desktop database must be a private single-link regular file"
    );
    // These C integer types differ between Darwin and Linux.
    #[allow(clippy::unnecessary_cast)]
    Ok(Some((metadata.st_dev as u64, metadata.st_ino as u64)))
}
pub(super) fn absent(root: &Directory, name: &str) -> Result<bool> {
    #[cfg(unix)]
    {
        Ok(named_stat(root, name)?.is_none())
    }
    #[cfg(windows)]
    {
        match fs::symlink_metadata(root.path.join(name)) {
            Ok(_) => Ok(false),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(true),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(all(test, unix))]
thread_local! { static AFTER_PIN: std::cell::RefCell<Option<PinHook>> = const { std::cell::RefCell::new(None) }; }
#[cfg(test)]
type PinHook = Box<dyn FnMut(&Pin)>;
#[cfg(test)]
type RootHook = Box<dyn FnMut(&Directory)>;
#[cfg(test)]
thread_local! {
    static AFTER_OBSERVATION: std::cell::RefCell<Option<PinHook>> = const { std::cell::RefCell::new(None) };
    static BEFORE_CREATE: std::cell::RefCell<Option<RootHook>> = const { std::cell::RefCell::new(None) };
}
#[cfg(unix)]
fn after_pin(_pin: &Pin) {
    #[cfg(test)]
    AFTER_PIN.with(|hook| {
        if let Some(hook) = hook.borrow_mut().as_mut() {
            hook(_pin);
        }
    });
}

#[cfg(test)]
#[path = "store_startup/tests.rs"]
mod tests;

#[cfg(all(test, unix))]
pub(super) type OpenHook = Box<dyn FnOnce(&Directory, &str)>;
#[cfg(all(test, unix))]
thread_local! { pub(super) static BEFORE_RAW_OPEN: std::cell::RefCell<Option<OpenHook>> = const { std::cell::RefCell::new(None) }; }
#[cfg(all(test, unix))]
pub(super) fn retained_count() -> usize {
    PINS.lock().unwrap().len()
}
