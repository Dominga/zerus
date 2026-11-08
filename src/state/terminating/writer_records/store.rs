//! Private per-target storage, not a process census or a termination permit.
//! The immutable session/run key survives label renames; no all-profile scan.
use super::*;
use crate::{platform, state::storage};
use fs2::FileExt;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::{fs::MetadataExt, fs::OpenOptionsExt},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const GATE: &[u8] = b"HGS-WRITER-GATE-1\n";

#[derive(Clone, Copy, Eq, PartialEq)]
struct Identity(u64, u64);
fn identity(file: &File) -> Result<Identity> {
    let meta = file.metadata().map_err(|_| REFUSED)?;
    if meta.ino() == 0 {
        return Err(REFUSED.into());
    }
    Ok(Identity(meta.dev(), meta.ino()))
}
fn read_file(path: &Path) -> Result<File> {
    let file = platform::no_follow(OpenOptions::new().read(true))
        .open(path)
        .map_err(|_| REFUSED)?;
    if !platform::verify_private_file(&file).map_err(|_| REFUSED)?
        || file.metadata().map_err(|_| REFUSED)?.mode() & 0o400 == 0
    {
        return Err(REFUSED.into());
    }
    Ok(file)
}
fn write_file(path: &Path) -> Result<File> {
    let file = platform::no_follow(OpenOptions::new().read(true).write(true))
        .open(path)
        .map_err(|_| REFUSED)?;
    if !platform::verify_private_file(&file).map_err(|_| REFUSED)?
        || file.metadata().map_err(|_| REFUSED)?.mode() & 0o600 != 0o600
    {
        return Err(REFUSED.into());
    }
    Ok(file)
}
fn create_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = platform::no_follow(platform::private_open(
        OpenOptions::new().read(true).write(true).create_new(true),
    ))
    .open(path)
    .map_err(|_| REFUSED)?;
    file.write_all(bytes).map_err(|_| REFUSED)?;
    file.sync_all().map_err(|_| REFUSED.into())
}

pub(super) struct Directory {
    path: PathBuf,
    file: File,
    identity: Identity,
}
impl Directory {
    pub(super) fn open(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
            .map_err(|_| REFUSED)?;
        if !platform::verify_private_directory(&file).map_err(|_| REFUSED)?
            || file.metadata().map_err(|_| REFUSED)?.mode() & 0o700 != 0o700
        {
            return Err(REFUSED.into());
        }
        let value = Self {
            path: path.into(),
            identity: identity(&file)?,
            file,
        };
        value.current()?;
        Ok(value)
    }
    pub(super) fn current(&self) -> Result<()> {
        if !platform::verify_private_directory(&self.file).map_err(|_| REFUSED)?
            || identity(&self.file)? != self.identity
            || self.file.metadata().map_err(|_| REFUSED)?.mode() & 0o700 != 0o700
        {
            return Err(REFUSED.into());
        }
        let now = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&self.path)
            .map_err(|_| REFUSED)?;
        if identity(&now)? != self.identity {
            return Err(REFUSED.into());
        }
        Ok(())
    }
}

/// A close-only shared open-file-description lock, never FileLock (whose
/// explicit unlock would also unlock an inherited/borrowed descriptor).
pub(super) struct Lease {
    _file: File,
    _directory: Arc<Directory>,
}

#[derive(Clone)]
pub(super) struct Store {
    scope: Run,
    root: Arc<Directory>,
    directory: Arc<Directory>,
    gate: Identity,
    lock: Identity,
}
impl Store {
    fn location(scope: &Run) -> Result<(Arc<Directory>, PathBuf)> {
        Run::new(&scope.session, &scope.run)?;
        // Lookup never creates even the state root. Initialization is for a
        // supervisor whose original binding already has an existing root.
        let root = Arc::new(Directory::open(
            &platform::canonical(storage::root()).map_err(|_| REFUSED)?,
        )?);
        let path = root
            .path
            .join("terminating-writers")
            .join(&scope.session)
            .join(&scope.run);
        Ok((root, path))
    }
    pub(super) fn initialize(scope: &Run, until: Instant) -> Result<()> {
        current(until)?;
        let (root, path) = Self::location(scope)?;
        let session = path.parent().ok_or(REFUSED)?;
        let base = session.parent().ok_or(REFUSED)?;
        platform::private_dir(base).map_err(|_| REFUSED)?;
        Directory::open(base)?;
        platform::private_dir(session).map_err(|_| REFUSED)?;
        Directory::open(session)?;
        current(until)?;
        // Never reset an existing or half-created run. Its original storage
        // must be inspected; initialization is not crash recovery.
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .map_err(|_| REFUSED)?;
        let dir = Directory::open(&path)?;
        create_file(&path.join("gate"), GATE)?;
        create_file(&path.join("lock"), &[])?;
        create_file(
            &path.join("pending.json"),
            &Ledger {
                version: VERSION,
                scope: scope.clone(),
                pending: Vec::new(),
            }
            .encode()?,
        )?;
        dir.file.sync_all().map_err(|_| REFUSED)?;
        platform::sync_dir(session).map_err(|_| REFUSED)?;
        dir.current()?;
        root.current()?;
        current(until)
    }
    pub(super) fn open(scope: &Run, until: Instant) -> Result<Self> {
        current(until)?;
        let (root, path) = Self::location(scope)?;
        let directory = Arc::new(Directory::open(&path)?);
        let gate = read_file(&directory.path.join("gate"))?;
        let mut bytes = Vec::new();
        (&gate)
            .take(GATE.len() as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| REFUSED)?;
        if bytes != GATE {
            return Err(REFUSED.into());
        }
        let value = Self {
            scope: scope.clone(),
            root,
            gate: identity(&gate)?,
            lock: identity(&read_file(&directory.path.join("lock"))?)?,
            directory,
        };
        let _guard = value.lock(until)?;
        value.load(until)?;
        Ok(value)
    }
    fn validate(&self) -> Result<()> {
        self.root.current()?;
        if platform::canonical(storage::root()).map_err(|_| REFUSED)? != self.root.path {
            return Err(REFUSED.into());
        }
        self.directory.current()?;
        let gate = read_file(&self.directory.path.join("gate"))?;
        let mut bytes = Vec::new();
        (&gate)
            .take(GATE.len() as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| REFUSED)?;
        if bytes != GATE
            || identity(&gate)? != self.gate
            || identity(&read_file(&self.directory.path.join("lock"))?)? != self.lock
        {
            return Err(REFUSED.into());
        }
        Ok(())
    }
    /// Only exclusion on this original store. An empty ledger plus this lock
    /// is not consent, a target proof, or a cleanup/termination capability.
    pub(super) fn exclusive(&self, until: Instant) -> Result<Lease> {
        current(until)?;
        self.validate()?;
        let file = write_file(&self.directory.path.join("gate"))?;
        if identity(&file)? != self.gate {
            return Err(REFUSED.into());
        }
        loop {
            current(until)?;
            match file.try_lock_exclusive() {
                Ok(()) => {
                    self.validate()?;
                    current(until)?;
                    return Ok(Lease {
                        _file: file,
                        _directory: self.directory.clone(),
                    });
                }
                Err(error) if platform::lock_contended(&error) => {
                    #[cfg(test)]
                    super::gate::contended();
                    std::thread::sleep(
                        Duration::from_millis(2)
                            .min(until.saturating_duration_since(Instant::now())),
                    );
                }
                Err(_) => return Err(REFUSED.into()),
            }
        }
    }
    pub(super) fn state_lock(&self) -> Result<File> {
        self.validate()?;
        // Never manufacture the lock while observing an old/missing scope.
        write_file(&self.root.path.join(".lock"))
    }
    pub(super) fn check_writer(&self, original: &File, writer: &storage::Guard) -> Result<()> {
        let now = identity(&self.state_lock()?)?;
        let held = writer.identity().map_err(|_| REFUSED)?;
        if identity(original)? != now || Identity(held.device, held.inode) != now {
            return Err(REFUSED.into());
        }
        Ok(())
    }
    fn lock(&self, until: Instant) -> Result<platform::FileLock> {
        self.validate()?;
        let file = write_file(&self.directory.path.join("lock"))?;
        if identity(&file)? != self.lock {
            return Err(REFUSED.into());
        }
        loop {
            current(until)?;
            match file.try_lock_exclusive() {
                Ok(()) => {
                    self.validate()?;
                    return Ok(platform::FileLock::held(file));
                }
                Err(error) if platform::lock_contended(&error) => {
                    std::thread::sleep(
                        Duration::from_millis(2)
                            .min(until.saturating_duration_since(Instant::now())),
                    );
                }
                Err(_) => return Err(REFUSED.into()),
            }
        }
    }
    fn load(&self, until: Instant) -> Result<Ledger> {
        current(until)?;
        self.validate()?;
        let file = read_file(&self.directory.path.join("pending.json"))?;
        let mut bytes = Vec::new();
        file.take(BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| REFUSED)?;
        self.validate()?;
        current(until)?;
        Ledger::decode(&bytes, &self.scope)
    }
    fn save(&self, ledger: &Ledger, until: Instant) -> Result<()> {
        current(until)?;
        self.validate()?;
        // Existing data must remain readable/valid. Missing/corrupt data never
        // gets overwritten by an empty reconstruction.
        self.load(until)?;
        let _writable = write_file(&self.directory.path.join("pending.json"))?;
        let bytes = ledger.encode()?;
        let text = std::str::from_utf8(&bytes).map_err(|_| REFUSED)?;
        storage::atomic(&self.directory.path.join("pending.json"), text).map_err(|_| REFUSED)?;
        self.validate()?;
        current(until)
    }
    pub(super) fn pending(&self, until: Instant) -> Result<Vec<Entry>> {
        let _guard = self.lock(until)?;
        Ok(self.load(until)?.pending)
    }
    pub(super) fn reserve(self, entry: Entry, until: Instant) -> Result<Reservation> {
        if !entry.valid() {
            return Err(REFUSED.into());
        }
        let _guard = self.lock(until)?;
        let mut ledger = self.load(until)?;
        if ledger.pending.len() >= ROWS
            || ledger
                .pending
                .iter()
                .any(|old| old.request == entry.request)
        {
            return Err(REFUSED.into());
        }
        let file = write_file(&self.directory.path.join("gate"))?;
        if identity(&file)? != self.gate {
            return Err(REFUSED.into());
        }
        file.try_lock_shared().map_err(|_| REFUSED)?;
        let lease = Arc::new(Lease {
            _file: file,
            _directory: self.directory.clone(),
        });
        ledger.pending.push(entry.clone());
        self.save(&ledger, until)?;
        Ok(Reservation {
            store: self,
            entry,
            lease: Some(lease),
            until,
        })
    }
    pub(super) fn borrow(&self, entry: &Entry, until: Instant) -> Result<()> {
        let _guard = self.lock(until)?;
        if !self.load(until)?.pending.iter().any(|old| old == entry) {
            return Err(REFUSED.into());
        }
        Ok(())
    }
    pub(super) fn retire(&self, entry: &Entry, until: Instant) -> Result<()> {
        let _guard = self.lock(until)?;
        let mut ledger = self.load(until)?;
        let slot = ledger
            .pending
            .iter()
            .position(|old| old == entry)
            .ok_or(REFUSED)?;
        ledger.pending.remove(slot);
        self.save(&ledger, until)
    }
}
