//! One offline rollback pair before the schema-35 migration. Recovery DATA
//! only: nothing here restores a database, settles custody or starts a job.
use super::{HandoffLock, Location};
use crate::{platform, state::repositories::directory::Directory};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::Write,
    sync::Arc,
    time::{Duration, Instant},
};

const BACKUP: &str = "schema-34-rollback";
const DATABASE: &str = "state.sqlite3";
const WAL: &str = "state.sqlite3-wal";
const MANIFEST: &str = "manifest.json";
const MAX_FILE: u64 = 1 << 30;
const READ_BUDGET: u64 = 4 << 30;
const META_LIMIT: u64 = 4096;

pub(super) struct Original<'a> {
    location: &'a Location,
    owner: &'a platform::FileLock,
    handoff: &'a HandoffLock,
    root: Arc<Directory>,
    pin: super::startup::Pin,
    database: Option<(u64, u64)>,
}
impl<'a> Original<'a> {
    pub(super) fn new(
        location: &'a Location,
        owner: &'a platform::FileLock,
        handoff: &'a HandoffLock,
    ) -> Result<Self> {
        let root = Arc::new(Directory::open(&location.root).map_err(anyhow::Error::msg)?);
        ensure!(
            root.private().map_err(anyhow::Error::msg)?,
            "desktop backup root is not private"
        );
        handoff.verify(location)?;
        for (name, expected) in [
            ("owner.lock", owner.identity()?),
            (zerus_desktop_migration::HANDOFF_LOCK, handoff.identity()?),
        ] {
            super::startup::check_exclusion(&root, name, expected)?;
        }
        let pin = super::startup::Pin::existing(Arc::clone(&root))?;
        let database = Some(pin.identity()?);
        let original = Self {
            location,
            owner,
            handoff,
            root,
            pin,
            database,
        };
        original.verify()?;
        Ok(original)
    }
    pub(super) fn verify(&self) -> Result<()> {
        self.root.verify().map_err(anyhow::Error::msg)?;
        self.pin.verify()?;
        self.handoff.verify(self.location)?;
        // These are exclusion files, NEVER raw descriptors of a live SQLite
        // database. Closing them cannot release SQLite's POSIX record locks.
        for (name, expected) in [
            ("owner.lock", self.owner.identity()?),
            (
                zerus_desktop_migration::HANDOFF_LOCK,
                self.handoff.identity()?,
            ),
        ] {
            super::startup::check_exclusion(&self.root, name, expected)?;
        }
        if let Some(original) = self.database {
            ensure!(
                platform::file_identity(&self.location.root.join(DATABASE))
                    .map(|v| (v.device, v.inode))
                    == Some(original),
                "desktop backup database replaced"
            );
        }
        Ok(())
    }
    pub(super) fn prepare(&self, schema: Option<i64>) -> Result<()> {
        self.verify()?;
        let Some(schema @ 1..=34) = schema else {
            return Ok(());
        };
        let mut budget = Budget {
            until: Instant::now() + Duration::from_secs(30),
            remaining: READ_BUDGET,
        };
        let path = self.location.root.join(BACKUP);
        match fs::symlink_metadata(&path) {
            Ok(_) => {
                return self.validate(
                    &self.root.child(BACKUP).map_err(anyhow::Error::msg)?,
                    &mut budget,
                )
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
        // A fixed destination is published NOREPLACE. The final manifest is
        // published last, also NOREPLACE: a partially copied directory never
        // qualifies, and later starts never repair or overwrite it.
        let backup = self
            .root
            .create(
                BACKUP,
                &format!(".schema-34-rollback-{}", uuid::Uuid::new_v4()),
            )
            .map_err(anyhow::Error::msg)?;
        // Never open/close an extra raw main-DB descriptor. Another local
        // SQLite connection may hold process-scoped locks on that same inode.
        let mut db = Input::pinned(&self.pin)?;
        let mut wal = Input::open(&self.root, WAL)?;
        no_journal(&self.root)?;
        let database = db.copy(&self.root, &backup, &mut budget)?;
        #[cfg(all(test, windows))]
        crash_backup("database");
        let wal_digest = wal
            .as_mut()
            .map(|v| v.copy(&self.root, &backup, &mut budget))
            .transpose()?;
        db.unchanged(&self.root, &mut budget, &database)?;
        if let Some(wal) = &mut wal {
            wal.unchanged(&self.root, &mut budget, wal_digest.as_ref().unwrap())?;
        } else {
            ensure!(
                super::startup::absent(&self.root, WAL)?,
                "desktop WAL appeared during backup"
            );
        }
        self.verify()?;
        let (root_device, root_inode) = self.root.object().map_err(anyhow::Error::msg)?;
        let (database_device, database_inode) =
            self.database.context("missing original desktop database")?;
        let manifest = Manifest {
            schema: 1,
            source_schema: schema,
            configuration: self.location.configuration.clone(),
            created_at: chrono::Utc::now().timestamp(),
            root_device,
            root_inode,
            database_device,
            database_inode,
            database,
            wal: wal_digest,
        };
        let bytes = serde_json::to_vec(&manifest)?;
        ensure!(
            bytes.len() <= META_LIMIT as usize,
            "desktop backup manifest too large"
        );
        #[cfg(all(test, windows))]
        crash_backup("manifest");
        backup
            .write_new(MANIFEST, &bytes)
            .map_err(anyhow::Error::msg)?;
        platform::sync_dir(&backup.path)?;
        platform::sync_dir(&self.location.root)?;
        self.validate(&backup, &mut budget)?;
        self.verify()
    }
    fn validate(&self, backup: &Directory, budget: &mut Budget) -> Result<()> {
        ensure!(
            backup.private().map_err(anyhow::Error::msg)?,
            "desktop backup is not private"
        );
        let metadata = Input::open(backup, MANIFEST)?
            .context("incomplete desktop rollback backup; retain it and inspect before retrying")?;
        ensure!(
            metadata.version.len <= META_LIMIT,
            "desktop backup manifest too large"
        );
        let mut bytes = vec![0u8; metadata.version.len as usize];
        let mut offset = 0;
        while offset < bytes.len() {
            let count = read_at(&metadata.file, &mut bytes[offset..], offset as u64)?;
            ensure!(count > 0, "desktop backup manifest shrank");
            offset += count;
        }
        ensure!(
            bytes.len() <= META_LIMIT as usize,
            "desktop backup manifest too large"
        );
        metadata.verify(backup)?;
        let manifest: Manifest = serde_json::from_slice(&bytes)?;
        ensure!(
            manifest.schema == 1
                && (1..=34).contains(&manifest.source_schema)
                && manifest.configuration == self.location.configuration
                && self.root.object().map_err(anyhow::Error::msg)?
                    == (manifest.root_device, manifest.root_inode)
                && self.database == Some((manifest.database_device, manifest.database_inode)),
            "desktop rollback backup provenance changed"
        );
        let expected = if manifest.wal.is_some() {
            vec![DATABASE, WAL, MANIFEST]
        } else {
            vec![DATABASE, MANIFEST]
        };
        let mut roster = Vec::new();
        for entry in fs::read_dir(&backup.path)?.take(5) {
            roster.push(entry?.file_name());
        }
        ensure!(
            roster.len() == expected.len()
                && expected.iter().all(|v| roster.iter().any(|n| n == v)),
            "incomplete or unexpected desktop backup files"
        );
        for (name, expected) in [
            (DATABASE, Some(&manifest.database)),
            (WAL, manifest.wal.as_ref()),
        ] {
            let input = Input::open(backup, name)?;
            match (input, expected) {
                (Some(mut input), Some(expected)) => ensure!(
                    &input.digest(backup, budget)? == expected,
                    "desktop rollback backup digest changed"
                ),
                (None, None) => (),
                _ => anyhow::bail!("desktop rollback backup WAL coverage changed"),
            }
        }
        self.verify()?;
        metadata.verify(backup)?;
        backup.verify().map_err(anyhow::Error::msg)
    }
}
fn no_journal(root: &Directory) -> Result<()> {
    ensure!(
        super::startup::absent(root, "state.sqlite3-journal")?,
        "desktop rollback journal requires recovery before backup"
    );
    Ok(())
}

struct Budget {
    until: Instant,
    remaining: u64,
}
impl Budget {
    fn read(&mut self, n: usize) -> Result<()> {
        ensure!(
            Instant::now() < self.until,
            "desktop rollback backup deadline exceeded"
        );
        self.remaining = self
            .remaining
            .checked_sub(n as u64)
            .context("desktop rollback backup read budget exceeded")?;
        Ok(())
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: u32,
    source_schema: i64,
    configuration: String,
    created_at: i64,
    root_device: u64,
    root_inode: u64,
    database_device: u64,
    database_inode: u64,
    database: DigestRecord,
    wal: Option<DigestRecord>,
}
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct DigestRecord {
    bytes: u64,
    sha256: String,
}
#[derive(PartialEq, Eq)]
struct Version {
    identity: platform::FileIdentity,
    len: u64,
    changed: (i64, i64),
}
fn version(file: &File) -> Result<Version> {
    let (identity, metadata) = platform::FileIdentity::with_metadata(file)?;
    Ok(metadata_version(identity, metadata))
}
fn metadata_version(identity: platform::FileIdentity, metadata: fs::Metadata) -> Version {
    #[cfg(unix)]
    let changed = {
        use std::os::unix::fs::MetadataExt;
        (metadata.ctime(), metadata.ctime_nsec())
    };
    #[cfg(not(unix))]
    let changed = (0, 0);
    Version {
        identity,
        len: metadata.len(),
        changed,
    }
}
fn named_version(root: &Directory, name: &str) -> Result<Version> {
    #[cfg(unix)]
    {
        let metadata = super::startup::named_stat(root, name)?
            .context("desktop rollback backup input disappeared")?;
        #[allow(clippy::unnecessary_cast)]
        Ok(Version {
            identity: platform::FileIdentity {
                device: metadata.st_dev as u64,
                inode: metadata.st_ino as u64,
                modified: metadata.st_mtime as i64,
                modified_nanos: metadata.st_mtime_nsec as i64,
            },
            len: u64::try_from(metadata.st_size)?,
            changed: (metadata.st_ctime as i64, metadata.st_ctime_nsec as i64),
        })
    }
    #[cfg(windows)]
    {
        let (identity, metadata) = platform::FileIdentity::at(&root.path.join(name))?;
        Ok(metadata_version(identity, metadata))
    }
}
fn read_at(file: &File, bytes: &mut [u8], offset: u64) -> std::io::Result<usize> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        file.read_at(bytes, offset)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        file.seek_read(bytes, offset)
    }
}
struct Input {
    name: &'static str,
    file: Arc<File>,
    pin: super::startup::Pin,
    #[cfg(test)]
    source: bool,
    version: Version,
}
impl Input {
    fn pinned(pin: &super::startup::Pin) -> Result<Self> {
        pin.verify()?;
        let file = pin.file();
        let version = version(&file)?;
        ensure!(
            version.len <= MAX_FILE,
            "desktop rollback backup file exceeds its limit"
        );
        Ok(Self {
            name: DATABASE,
            file,
            pin: pin.clone(),
            #[cfg(test)]
            source: true,
            version,
        })
    }
    fn open(root: &Directory, name: &'static str) -> Result<Option<Self>> {
        super::startup::Pin::input(root, name)?
            .map(|pin| {
                let file = pin.file();
                let version = version(&file)?;
                ensure!(
                    version.len <= MAX_FILE,
                    "desktop rollback backup file exceeds its limit"
                );
                Ok(Self {
                    name,
                    file,
                    pin,
                    #[cfg(test)]
                    source: false,
                    version,
                })
            })
            .transpose()
    }

    fn verify(&self, root: &Directory) -> Result<()> {
        ensure!(
            version(&self.file)? == self.version,
            "desktop rollback backup input changed"
        );
        #[cfg(test)]
        if let Some(hook) = BEFORE_INPUT_NAME.with(|slot| slot.borrow_mut().take()) {
            hook();
        }
        self.pin.verify()?;
        // Unix uses metadata only, never an extra raw alias open/close.
        let named = named_version(root, self.name)?;
        ensure!(
            named == self.version,
            "desktop rollback backup input replaced"
        );
        root.verify().map_err(anyhow::Error::msg)
    }
    fn scan(
        &mut self,
        root: &Directory,
        budget: &mut Budget,
        mut output: Option<&mut File>,
    ) -> Result<DigestRecord> {
        self.verify(root)?;
        let mut hash = Sha256::new();
        let mut total = 0u64;
        let mut buffer = [0u8; 65536];
        loop {
            budget.read(0)?;
            let count = read_at(&self.file, &mut buffer, total)?;
            budget.read(count)?;
            if count == 0 {
                break;
            }
            #[cfg(test)]
            if self.source {
                if let Some(hook) = DURING_SOURCE_COPY.with(|slot| slot.borrow_mut().take()) {
                    hook();
                }
            }
            total += count as u64;
            ensure!(
                total <= self.version.len,
                "desktop rollback backup input grew"
            );
            hash.update(&buffer[..count]);
            if let Some(out) = output.as_mut() {
                out.write_all(&buffer[..count])?;
            }
        }
        ensure!(
            total == self.version.len,
            "desktop rollback backup input shrank"
        );
        self.verify(root)?;
        Ok(DigestRecord {
            bytes: total,
            sha256: format!("{:x}", hash.finalize()),
        })
    }
    fn copy(
        &mut self,
        root: &Directory,
        backup: &Directory,
        budget: &mut Budget,
    ) -> Result<DigestRecord> {
        let mut file = backup
            .write_new_file(self.name, &[])
            .map_err(anyhow::Error::msg)?;
        let digest = self.scan(root, budget, Some(&mut file))?;
        file.sync_all()?;
        backup.verify().map_err(anyhow::Error::msg)?;
        Ok(digest)
    }
    fn digest(&mut self, root: &Directory, budget: &mut Budget) -> Result<DigestRecord> {
        self.scan(root, budget, None)
    }
    fn unchanged(
        &mut self,
        root: &Directory,
        budget: &mut Budget,
        expected: &DigestRecord,
    ) -> Result<()> {
        ensure!(
            &self.digest(root, budget)? == expected,
            "desktop rollback backup source digest changed"
        );
        Ok(())
    }
}

#[cfg(test)]
thread_local! { static INTERRUPT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
#[cfg(all(test, windows))]
thread_local! { static BACKUP_CRASH: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) }; }
#[cfg(all(test, windows))]
fn crash_backup(phase: &'static str) {
    if BACKUP_CRASH.with(std::cell::Cell::get) == Some(phase) {
        // The exact owned fixture child exits without running Rust destructors.
        std::process::exit(88);
    }
}
#[cfg(test)]
type CopyHook = Box<dyn FnOnce()>;
#[cfg(test)]
thread_local! { static BEFORE_INPUT_NAME: std::cell::RefCell<Option<CopyHook>> = const { std::cell::RefCell::new(None) }; }
#[cfg(test)]
thread_local! { static DURING_SOURCE_COPY: std::cell::RefCell<Option<CopyHook>> = const { std::cell::RefCell::new(None) }; }
#[cfg(test)]
pub(in crate::desktop) fn after_slot_rename() {
    if INTERRUPT.with(std::cell::Cell::get) {
        // Deliberately skip every Rust destructor, including Transaction::drop.
        // Only the parent's private fixture process can select this branch.
        #[cfg(unix)]
        // SAFETY: SIGKILL targets only this exact newly spawned test process.
        unsafe {
            libc::raise(libc::SIGKILL);
        }
        std::process::exit(86);
    }
}

#[cfg(test)]
#[path = "rollback_backup/tests.rs"]
mod tests;
