//! One fixed publication candidate, with its original object recorded durably
//! in the already-held metadata lock. This is not an execution/drain receipt.
use super::*;

pub(super) const BYTES: usize = 4096;
const ABI: &str = "task-start-publication-v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Clear,
    Recoverable,
    NeedsReview,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Pending {
    abi: String,
    directories: [ObjectVersion; 3],
    before: FileVersion,
    before_id: String,
    before_sha256: String,
    after_id: String,
    after_sha256: String,
    candidate: FileVersion,
    #[serde(skip)]
    lock_version: Option<FileVersion>,
}
pub(super) fn name(slot: &Slot) -> OsString {
    OsString::from(format!("{}.tmp", slot.name.to_string_lossy()))
}
fn optional_candidate(slot: &Slot) -> io::Result<Option<File>> {
    match open_at(&slot.folder, &name(slot), false) {
        Ok(file) => Ok(Some(file)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}
fn tracking(slot: &Slot, lock: &File) -> io::Result<Option<Pending>> {
    slot.current()?;
    lock_current(slot, lock)?;
    let (bytes, version) = read(lock, BYTES)?;
    if bytes.is_empty() {
        return Ok(None);
    }
    let value = wire::original_value(std::str::from_utf8(&bytes).map_err(|_| refused())?)
        .map_err(|_| refused())?;
    let mut record: Pending = serde_json::from_value(value).map_err(|_| refused())?;
    if record.abi != ABI
        || record.directories != slot.versions
        || !crate::creation::canonical_uuid(&record.before_id)
        || !crate::creation::canonical_uuid(&record.after_id)
    {
        return Err(refused());
    }
    record.lock_version = Some(version);
    fence(slot, lock, &record)?;
    Ok(Some(record))
}
fn writable(slot: &Slot, lock: &File) -> io::Result<File> {
    lock_current(slot, lock)?;
    // Duplicate the SAME open file description which holds the flock. There
    // is no reopened lock/path capability or second lock owner to substitute.
    let file = lock.try_clone()?;
    file_version(&file, BYTES)?;
    lock_current(slot, lock)?;
    Ok(file)
}
pub(super) fn candidate(slot: &Slot, pending: &Pending, held: &File) -> io::Result<()> {
    slot.current()?;
    let opened = open_at(&slot.folder, &name(slot), false)?;
    let (bytes, version) = read(held, super::BYTES)?;
    if version != pending.candidate
        || file_version(&opened, super::BYTES)? != pending.candidate
        || sha256(&bytes) != pending.after_sha256
    {
        return Err(refused());
    }
    let value = wire::original_value(std::str::from_utf8(&bytes).map_err(|_| refused())?)
        .map_err(|_| refused())?;
    let next: Record = serde_json::from_value(value).map_err(|_| refused())?;
    // The durable hash and native object identify the ORIGINAL candidate;
    // decoding a same-task record alone never permits deleting another file.
    if next.intent.reconcile_id != pending.after_id {
        return Err(refused());
    }
    let key = &next.intent.binding.key;
    if !next.valid(&key.project, &key.task)
        || outbox::path(Path::new(""), &key.project, &key.task)
            .with_extension("reconcile")
            .file_name()
            != Some(slot.name.as_os_str())
    {
        return Err(refused());
    }
    slot.current()
}
fn before(disk: &Disk, pending: &Pending) -> bool {
    disk.version == pending.before
        && disk.record.intent.reconcile_id == pending.before_id
        && sha256(&disk.bytes) == pending.before_sha256
}
fn published(disk: &Disk, pending: &Pending) -> bool {
    // rename changes ctime. This recognizes the same published object/bytes
    // ONLY to retire tracking; the journal itself is never unlinked/replaced.
    disk.version.object == pending.candidate.object
        && disk.record.intent.reconcile_id == pending.after_id
        && sha256(&disk.bytes) == pending.after_sha256
}
struct Inspection {
    status: Status,
    pending: Option<Pending>,
    file: Option<File>,
}
fn inspect(slot: &Slot, disk: &Disk, lock: &File) -> io::Result<Inspection> {
    disk.current(slot)?;
    let pending = tracking(slot, lock)?;
    let candidate_file = optional_candidate(slot)?;
    let result = match (&pending, &candidate_file) {
        (None, None) => Status::Clear,
        (Some(pending), Some(file)) if before(disk, pending) => {
            candidate(slot, pending, file)?;
            Status::Recoverable
        }
        (Some(pending), None) if published(disk, pending) => Status::Recoverable,
        _ => Status::NeedsReview,
    };
    disk.current(slot)?;
    lock_current(slot, lock)?;
    Ok(Inspection {
        status: result,
        pending,
        file: candidate_file,
    })
}
pub(super) fn status(slot: &Slot, disk: &Disk) -> Status {
    (|| {
        let lock = open_at(&slot.folder, &slot.lock_name, false)?;
        inspect(slot, disk, &lock).map(|observed| observed.status)
    })()
    .unwrap_or(Status::NeedsReview)
}
pub(super) fn fence(slot: &Slot, lock: &File, pending: &Pending) -> io::Result<()> {
    lock_current(slot, lock)?;
    if pending.lock_version.as_ref() != Some(&file_version(lock, BYTES)?) {
        return Err(refused());
    }
    Ok(())
}
pub(super) fn clear(
    slot: &Slot,
    lock: &File,
    pending: &Pending,
    sync: &mut impl FnMut(&File, bool) -> io::Result<()>,
) -> io::Result<()> {
    fence(slot, lock, pending)?;
    let file = writable(slot, lock)?;
    fence(slot, lock, pending)?;
    file.set_len(0)?;
    sync(&file, false)?;
    lock_current(slot, lock)
}
pub(super) fn recover(slot: &Slot, disk: &Disk, lock: &File) -> io::Result<()> {
    reconcile(slot, disk, lock, inspect(slot, disk, lock)?)
}
fn reconcile(slot: &Slot, disk: &Disk, lock: &File, observed: Inspection) -> io::Result<()> {
    match observed.status {
        Status::Clear => return Ok(()),
        Status::NeedsReview => return Err(refused()),
        Status::Recoverable => (),
    }
    let pending = observed.pending.ok_or_else(refused)?;
    fence(slot, lock, &pending)?;
    if let Some(file) = observed.file {
        if !before(disk, &pending) {
            return Err(refused());
        }
        candidate(slot, &pending, &file)?;
        disk.current(slot)?;
        fence(slot, lock, &pending)?;
        candidate(slot, &pending, &file)?;
        fence(slot, lock, &pending)?;
        let name = CString::new(name(slot).as_bytes()).map_err(|_| refused())?;
        // SAFETY: the exact durably recorded candidate, rechecked above,
        // under the SAME original metadata flock and retained folder.
        if unsafe { libc::unlinkat(slot.folder.as_raw_fd(), name.as_ptr(), 0) } != 0 {
            return Err(io::Error::last_os_error());
        }
        slot.folder.sync_all()?;
    } else if !published(disk, &pending) {
        return Err(refused());
    }
    disk.current(slot)?;
    fence(slot, lock, &pending)?;
    if optional_candidate(slot)?.is_some() {
        return Err(refused());
    }
    clear(slot, lock, &pending, &mut |file, _| file.sync_all())
}
pub(super) fn track(
    slot: &Slot,
    disk: &Disk,
    lock: &File,
    file: &File,
    next: &Record,
    bytes: &[u8],
    sync: &mut impl FnMut(&File, bool) -> io::Result<()>,
) -> io::Result<Pending> {
    if tracking(slot, lock)?.is_some() {
        return Err(refused());
    }
    let mut pending = Pending {
        abi: ABI.into(),
        directories: slot.versions.clone(),
        before: disk.version.clone(),
        before_id: disk.record.intent.reconcile_id.clone(),
        before_sha256: sha256(&disk.bytes),
        after_id: next.intent.reconcile_id.clone(),
        after_sha256: sha256(bytes),
        candidate: file_version(file, super::BYTES)?,
        lock_version: None,
    };
    candidate(slot, &pending, file)?;
    disk.current(slot)?;
    let bytes = serde_json::to_vec(&pending).map_err(|_| refused())?;
    if bytes.len() > BYTES {
        return Err(refused());
    }
    let writer = writable(slot, lock)?;
    if !read(&writer, BYTES)?.0.is_empty() {
        return Err(refused());
    }
    writer.write_all_at(&bytes, 0)?;
    sync(&writer, false)?;
    // The fixed metadata lock's directory entry is durable before rename.
    slot.folder.sync_all()?;
    let (actual, version) = read(&writer, BYTES)?;
    if actual != bytes {
        return Err(refused());
    }
    pending.lock_version = Some(version);
    fence(slot, lock, &pending)?;
    candidate(slot, &pending, file)?;
    Ok(pending)
}
/// An ordinary pre-tracking error can clean only this still-held exact object.
/// A partial/malformed tracking record or a changed file remains for review.
pub(super) fn discard_live(
    slot: &Slot,
    disk: &Disk,
    lock: &File,
    file: &File,
    version: &FileVersion,
) -> io::Result<()> {
    if tracking(slot, lock)?.is_some() || file_version(file, super::BYTES)? != *version {
        return Err(refused());
    }
    let opened = open_at(&slot.folder, &name(slot), false)?;
    if file_version(&opened, super::BYTES)? != *version {
        return Err(refused());
    }
    disk.current(slot)?;
    lock_current(slot, lock)?;
    let name = CString::new(name(slot).as_bytes()).map_err(|_| refused())?;
    // SAFETY: the original new file is still held and exactly linked; no
    // filename/age/owner-only predicate authorizes this removal.
    if unsafe { libc::unlinkat(slot.folder.as_raw_fd(), name.as_ptr(), 0) } != 0 {
        return Err(io::Error::last_os_error());
    }
    slot.folder.sync_all()
}

#[cfg(test)]
mod tests;
