//! Original native reconciliation metadata and exact submitted/drained settlement.
//! No IPC or task call. An offered original can never be offered again here.
use super::*;
use std::{ffi::OsString, io::Write};

mod completion;
pub use completion::Settlement;

pub const BYTES: usize = 96 << 10;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Unavailable,
    Busy,
    OfferedUnknown,
    Cancelled,
    PublicationUnknown,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Prepared,
    OfferedUnknown,
    CancelledBeforeOffer,
    SettlementPrepared,
    Completed,
}
#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: u32,
    phase: Phase,
    version: Version,
    previous_cancelled: Option<String>,
    completion: Option<wire::Lookup>,
    intent: wire::Intent,
}
impl Record {
    fn valid(&self, project: &str, task: &str) -> bool {
        self.schema == 1
            && self.previous_cancelled.as_ref().is_none_or(|id| {
                crate::creation::canonical_uuid(id) && id != &self.intent.reconcile_id
            })
            && self.intent.binding.key.project == project
            && self.intent.binding.key.task == task
            && self.intent.clone().verify().is_ok()
            && self.valid_completion()
    }
}
struct Slot {
    managed: ManagedData,
    selected: PathBuf,
    store: File,
    root: File,
    folder: File,
    versions: [ObjectVersion; 3],
    name: OsString,
    lock_name: OsString,
}
impl Slot {
    fn open(store: &Path, project: &str, task: &str) -> io::Result<Self> {
        crate::task_details::document_call(project, task).map_err(|_| refused())?;
        if !store.is_absolute() {
            return Err(refused());
        }
        let managed = ManagedData::open(store)?;
        let selected = store.to_path_buf();
        let store = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(managed.path())?;
        let root = open_at(&store, OsStr::new("task-start"), true)?;
        let folder = open_at(&root, OsStr::new("task-delivery"), true)?;
        let path = outbox::path(Path::new(""), project, task);
        let name = path
            .with_extension("reconcile")
            .file_name()
            .ok_or_else(refused)?
            .to_owned();
        let lock_name = path
            .with_extension("reconcile-lock")
            .file_name()
            .ok_or_else(refused)?
            .to_owned();
        let versions = [
            directory_version(&store, false)?,
            directory_version(&root, true)?,
            directory_version(&folder, true)?,
        ];
        let slot = Self {
            managed,
            selected,
            store,
            root,
            folder,
            versions,
            name,
            lock_name,
        };
        slot.current()?;
        Ok(slot)
    }
    fn current(&self) -> io::Result<()> {
        self.managed.recheck()?;
        if std::fs::canonicalize(&self.selected)? != self.managed.path() {
            return Err(refused());
        }
        let store = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(self.managed.path())?;
        let root = open_at(&self.store, OsStr::new("task-start"), true)?;
        let folder = open_at(&self.root, OsStr::new("task-delivery"), true)?;
        for (index, opened, held) in [
            (0, &store, &self.store),
            (1, &root, &self.root),
            (2, &folder, &self.folder),
        ] {
            if directory_version(opened, index != 0)? != self.versions[index]
                || directory_version(held, index != 0)? != self.versions[index]
            {
                return Err(refused());
            }
        }
        Ok(())
    }
    fn create(&self, name: &OsStr, exclusive: bool) -> io::Result<File> {
        let name = CString::new(name.as_bytes()).map_err(|_| refused())?;
        let flags = libc::O_RDWR
            | libc::O_NOFOLLOW
            | libc::O_NONBLOCK
            | libc::O_CLOEXEC
            | libc::O_CREAT
            | if exclusive { libc::O_EXCL } else { 0 };
        // SAFETY: fixed/locally generated basename below the retained folder.
        let fd = unsafe { libc::openat(self.folder.as_raw_fd(), name.as_ptr(), flags, 0o600) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: this is the new owned descriptor returned by openat.
        let file = unsafe { File::from_raw_fd(fd) };
        file_version(&file, BYTES)?;
        Ok(file)
    }
    fn lock(&self, create: bool) -> Result<File, Error> {
        self.current().map_err(|_| Error::Unavailable)?;
        let file = if create {
            self.create(&self.lock_name, true)
        } else {
            open_at(&self.folder, &self.lock_name, false)
        }
        .map_err(|_| Error::Unavailable)?;
        if file_version(&file, 0).is_err() {
            return Err(Error::Unavailable);
        }
        // SAFETY: only this retained metadata lock, never a service/native lock.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            let error = io::Error::last_os_error();
            return Err(if matches!(error.raw_os_error(), Some(libc::EWOULDBLOCK)) {
                Error::Busy
            } else {
                Error::Unavailable
            });
        }
        Ok(file)
    }
}
struct Disk {
    file: File,
    version: FileVersion,
    bytes: Vec<u8>,
    record: Record,
}
impl Disk {
    fn open(slot: &Slot, project: &str, task: &str) -> io::Result<Option<Self>> {
        slot.current()?;
        let file = match open_at(&slot.folder, &slot.name, false) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let (bytes, version) = read(&file, BYTES)?;
        // The journal itself is closed and duplicate-free, including nested
        // source/input which also receive the existing wire verification.
        let value = wire::original_value(std::str::from_utf8(&bytes).map_err(|_| refused())?)
            .map_err(|_| refused())?;
        let record: Record = serde_json::from_value(value).map_err(|_| refused())?;
        if !record.valid(project, task) {
            return Err(refused());
        }
        let disk = Self {
            file,
            version,
            bytes,
            record,
        };
        disk.current(slot)?;
        Ok(Some(disk))
    }
    fn current(&self, slot: &Slot) -> io::Result<()> {
        slot.current()?;
        let file = open_at(&slot.folder, &slot.name, false)?;
        let (bytes, version) = read(&self.file, BYTES)?;
        if bytes != self.bytes
            || version != self.version
            || file_version(&file, BYTES)? != self.version
        {
            return Err(refused());
        }
        slot.current()
    }
}

/// Bounded saved metadata, independent of executor liveness or an extant outbox.
/// Queries cannot offer; exact submitted/drained settlement has separate checks.
pub struct Saved {
    slot: Slot,
    disk: Disk,
}
impl Saved {
    pub fn read(store: &Path, project: &str, task: &str) -> Result<Option<Self>, Error> {
        let slot = Slot::open(store, project, task).map_err(|_| Error::Unavailable)?;
        let disk = Disk::open(&slot, project, task).map_err(|_| Error::Unavailable)?;
        if disk.is_none() {
            match open_at(&slot.folder, &slot.lock_name, false) {
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
                _ => return Err(Error::Unavailable), // Retained history may be missing.
            }
        }
        Ok(disk.map(|disk| Self { slot, disk }))
    }
    pub fn phase(&self) -> Phase {
        self.disk.record.phase
    }
    pub fn reconcile_id(&self) -> &str {
        &self.disk.record.intent.reconcile_id
    }
    pub fn original_id(&self) -> &str {
        &self.disk.record.intent.binding.key.request_id
    }
    pub fn current(&self) -> bool {
        self.disk.current(&self.slot).is_ok()
    }
    pub fn query(&self, scope: wire::Scope, request_id: &str) -> Result<wire::Query, Error> {
        if !self.current() {
            return Err(Error::Unavailable);
        }
        let query = wire::Query {
            scope,
            request_id: request_id.into(),
            reconcile_id: self.reconcile_id().into(),
            binding: self.disk.record.intent.binding.clone(),
        };
        if !query.valid() {
            return Err(Error::Unavailable);
        }
        Ok(query)
    }
    /// Explicit confirmation discards only the unsent CHECK, not the original
    /// task/outbox. Preserve a tombstone; missing metadata never authorizes a
    /// replacement ID. Never call this from open, query or automatic recovery.
    pub fn discard_prepared(self, reconcile_id: &str) -> Result<(), Error> {
        if self.reconcile_id() != reconcile_id {
            return Err(Error::Unavailable);
        }
        if self.phase() != Phase::Prepared && self.phase() != Phase::CancelledBeforeOffer {
            return Err(Error::OfferedUnknown);
        }
        let lock = self.slot.lock(false)?;
        self.disk
            .current(&self.slot)
            .map_err(|_| Error::Unavailable)?;
        lock_current(&self.slot, &lock).map_err(|_| Error::Unavailable)?;
        if self.phase() == Phase::CancelledBeforeOffer {
            return Ok(());
        }
        let mut next = self.disk.record.clone();
        next.phase = Phase::CancelledBeforeOffer;
        publish(
            &self.slot,
            &self.disk,
            &next,
            || lock_current(&self.slot, &lock),
            |file, _| file.sync_all(),
        )?;
        Ok(())
    }
}

/// A metadata writer for one exact captured original. Its private lock is
/// CLOEXEC and supplies no service/native handoff or execution authority.
pub struct Journal {
    saved: Saved,
    source: Captured,
    lock: File,
    poisoned: bool,
}
impl Journal {
    /// Explicit preparation only. Existing originals retain their reconcile
    /// ID and phase; a missing lookup never calls this on an offered record.
    pub fn prepare(source: Captured, reconcile_id: &str) -> Result<Self, Error> {
        Self::prepare_in(source, reconcile_id, None)
    }
    /// A separate explicit Check AFTER a confirmed discard of an unsent check.
    /// Never infer this choice from missing data or an unavailable result.
    pub fn prepare_after_discard(
        source: Captured,
        reconcile_id: &str,
        cancelled_id: &str,
    ) -> Result<Self, Error> {
        Self::prepare_in(source, reconcile_id, Some(cancelled_id))
    }
    fn prepare_in(
        source: Captured,
        reconcile_id: &str,
        cancelled_id: Option<&str>,
    ) -> Result<Self, Error> {
        let candidate = source
            .intent(reconcile_id)
            .map_err(|_| Error::Unavailable)?;
        let key = &source.binding.key;
        let slot = Slot::open(&source.selected, &key.project, &key.task)
            .map_err(|_| Error::Unavailable)?;
        let before = Disk::open(&slot, &key.project, &key.task).map_err(|_| Error::Unavailable)?;
        if before.is_none() && cancelled_id.is_some() {
            return Err(Error::Unavailable);
        }
        // First use creates the lock exclusively. A leftover lock plus missing
        // journal is uncertainty, never evidence that another offer is safe.
        let lock = slot.lock(before.is_none())?;
        let disk = Disk::open(&slot, &key.project, &key.task).map_err(|_| Error::Unavailable)?;
        let new_record = |previous: Option<String>| Record {
            schema: 1,
            phase: Phase::Prepared,
            version: source.version.clone(),
            previous_cancelled: previous,
            completion: None,
            intent: candidate.clone(),
        };
        let disk = match disk {
            Some(disk) if disk.record.phase == Phase::CancelledBeforeOffer => {
                if cancelled_id.is_none() {
                    return Err(Error::Cancelled);
                }
                if cancelled_id != Some(disk.record.intent.reconcile_id.as_str())
                    || cancelled_id == Some(reconcile_id)
                {
                    return Err(Error::Unavailable);
                }
                let next = new_record(cancelled_id.map(str::to_owned));
                publish(
                    &slot,
                    &disk,
                    &next,
                    || {
                        source.recheck()?;
                        lock_current(&slot, &lock)
                    },
                    |file, _| file.sync_all(),
                )?
            }
            Some(disk) => {
                if let Some(cancelled) = cancelled_id {
                    if disk.record.previous_cancelled.as_deref() != Some(cancelled)
                        || disk.record.intent.reconcile_id != reconcile_id
                    {
                        return Err(Error::Unavailable);
                    }
                }
                disk
            }
            None => {
                if before.is_some() || cancelled_id.is_some() {
                    return Err(Error::Unavailable);
                }
                let record = new_record(None);
                let bytes = encode(&record)?;
                let mut file = slot
                    .create(&slot.name, true)
                    .map_err(|_| Error::Unavailable)?;
                file.write_all(&bytes)
                    .and_then(|_| file.sync_all())
                    .and_then(|_| slot.folder.sync_all())
                    .map_err(|_| Error::PublicationUnknown)?;
                Disk::open(&slot, &key.project, &key.task)
                    .map_err(|_| Error::PublicationUnknown)?
                    .ok_or(Error::PublicationUnknown)?
            }
        };
        let frozen = &disk.record;
        if frozen.version != source.version
            || frozen.intent.binding != source.binding
            || frozen.intent.original != source.original
            || !source.current()
        {
            return Err(Error::Unavailable);
        }
        let journal = Self {
            saved: Saved { slot, disk },
            source,
            lock,
            poisoned: false,
        };
        journal.check_current()?;
        Ok(journal)
    }
    pub fn saved(&self) -> &Saved {
        &self.saved
    }
    /// Source and metadata currentness only; never native admission.
    pub fn current(&self) -> bool {
        self.check_current().is_ok()
    }
    fn check_current(&self) -> Result<(), Error> {
        if self.poisoned || !self.source.current() || !self.saved.current() {
            return Err(Error::Unavailable);
        }
        self.lock_current()
    }
    fn lock_current(&self) -> Result<(), Error> {
        lock_current(&self.saved.slot, &self.lock).map_err(|_| Error::Unavailable)
    }
    /// Returns only data after the exact OfferedUnknown phase is durable.
    /// Calling it again, reloading, or observing a missing result cannot
    /// return another offer. Fresh native admission is still external.
    pub fn offer(&mut self) -> Result<wire::Intent, Error> {
        self.offer_with_sync(|file, _| file.sync_all())
    }
    fn offer_with_sync(
        &mut self,
        mut sync: impl FnMut(&File, bool) -> io::Result<()>,
    ) -> Result<wire::Intent, Error> {
        match self.saved.phase() {
            Phase::Prepared => {}
            Phase::OfferedUnknown => return Err(Error::OfferedUnknown),
            Phase::CancelledBeforeOffer => return Err(Error::Cancelled),
            Phase::SettlementPrepared | Phase::Completed => return Err(Error::OfferedUnknown),
        }
        self.check_current()?;
        let intent = self
            .source
            .intent(self.saved.reconcile_id())
            .map_err(|_| Error::Unavailable)?;
        let mut next = self.saved.disk.record.clone();
        next.phase = Phase::OfferedUnknown;
        // Only the fresh service scope may change before the first offer;
        // original reconcile/delivery/binding/raw/version remain frozen.
        next.intent = intent.clone();
        self.poisoned = true;
        self.saved.disk = publish(
            &self.saved.slot,
            &self.saved.disk,
            &next,
            || {
                self.source.recheck()?;
                lock_current(&self.saved.slot, &self.lock)
            },
            &mut sync,
        )?;
        self.poisoned = false;
        self.check_current()?;
        Ok(intent)
    }
}
fn lock_current(slot: &Slot, lock: &File) -> io::Result<()> {
    let opened = open_at(&slot.folder, &slot.lock_name, false)?;
    if file_version(&opened, 0)? != file_version(lock, 0)? {
        return Err(refused());
    }
    Ok(())
}
fn publish(
    slot: &Slot,
    old: &Disk,
    next: &Record,
    mut check: impl FnMut() -> io::Result<()>,
    mut sync: impl FnMut(&File, bool) -> io::Result<()>,
) -> Result<Disk, Error> {
    let key = &old.record.intent.binding.key;
    if !next.valid(&key.project, &key.task) {
        return Err(Error::Unavailable);
    }
    let bytes = encode(next)?;
    old.current(slot).map_err(|_| Error::Unavailable)?;
    check().map_err(|_| Error::Unavailable)?;
    let temp = OsString::from(format!(
        "{}.{}.tmp",
        slot.name.to_string_lossy(),
        uuid::Uuid::new_v4()
    ));
    let mut file = slot.create(&temp, true).map_err(|_| Error::Unavailable)?;
    let written = (|| -> io::Result<()> {
        file.write_all(&bytes)?;
        sync(&file, false)?;
        old.current(slot)?;
        check()?;
        let opened = open_at(&slot.folder, &temp, false)?;
        if file_version(&opened, BYTES)? != file_version(&file, BYTES)? {
            return Err(refused());
        }
        let from = CString::new(temp.as_bytes()).map_err(|_| refused())?;
        let to = CString::new(slot.name.as_bytes()).map_err(|_| refused())?;
        // SAFETY: native basenames in the same retained private directory.
        if unsafe {
            libc::renameat(
                slot.folder.as_raw_fd(),
                from.as_ptr(),
                slot.folder.as_raw_fd(),
                to.as_ptr(),
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        sync(&slot.folder, true)
    })();
    if written.is_err() {
        // Cleanup only the exact still-linked original temporary object. An
        // unknown/replaced temporary or original journal is never removed.
        if let (Ok(opened), Ok(held)) = (
            open_at(&slot.folder, &temp, false),
            file_version(&file, BYTES),
        ) {
            if file_version(&opened, BYTES).ok() == Some(held) {
                if let Ok(name) = CString::new(temp.as_bytes()) {
                    // SAFETY: exact verified own temporary in the private directory.
                    let _ = unsafe { libc::unlinkat(slot.folder.as_raw_fd(), name.as_ptr(), 0) };
                }
            }
        }
        return Err(Error::PublicationUnknown);
    }
    let key = &next.intent.binding.key;
    let disk = Disk::open(slot, &key.project, &key.task)
        .map_err(|_| Error::PublicationUnknown)?
        .ok_or(Error::PublicationUnknown)?;
    if disk.record != *next || disk.bytes != bytes {
        return Err(Error::PublicationUnknown);
    }
    check().map_err(|_| Error::PublicationUnknown)?;
    Ok(disk)
}
fn encode(record: &Record) -> Result<Vec<u8>, Error> {
    let bytes = serde_json::to_vec(record).map_err(|_| Error::Unavailable)?;
    if bytes.len() > BYTES {
        return Err(Error::Unavailable);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests;
