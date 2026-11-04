//! Bounded write-ahead allocation for one private journal candidate. Complete
//! frames are synced BEFORE create, body writes and rename respectively.
use super::*;

pub(super) const BYTES: usize = 4096;
const ABI: &str = "task-start-publication-v2";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Clear,
    Recoverable,
    NeedsReview,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Allocation {
    abi: String,
    nonce: String,
    directories: [ObjectVersion; 3],
    before: FileVersion,
    before_id: String,
    before_sha256: String,
    after_id: String,
    after_sha256: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Created {
    object: ObjectVersion,
    born: Option<[u64; 2]>,
}
#[derive(Deserialize, Serialize)]
#[serde(tag = "stage", rename_all = "snake_case", deny_unknown_fields)]
enum Frame {
    Allocated { allocation: Allocation },
    Created { nonce: String, object: Created },
    Ready { nonce: String, file: FileVersion },
}
pub(super) struct Pending {
    allocation: Allocation,
    created: Option<Created>,
    ready: Option<FileVersion>,
}
pub(super) struct Snapshot {
    bytes: Vec<u8>,
    version: FileVersion,
    pending: Option<Pending>,
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
fn born(file: &File) -> Option<[u64; 2]> {
    let time = file
        .metadata()
        .ok()?
        .created()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    Some([time.as_secs(), time.subsec_nanos().into()])
}
fn decode(slot: &Slot, bytes: Vec<u8>, version: FileVersion) -> io::Result<Snapshot> {
    let mut pending: Option<Pending> = None;
    for (count, line) in bytes.split_inclusive(|b| *b == b'\n').enumerate() {
        if count >= 3 {
            return Err(refused());
        }
        let expected = [
            b"{\"stage\":\"allocated\"".as_slice(),
            b"{\"stage\":\"created\"",
            b"{\"stage\":\"ready\"",
        ][count];
        if line.last() != Some(&b'\n') {
            // Only a syntactically incomplete final frame, or a complete
            // expected frame lacking its final LF, is an uncommitted tail.
            // No corresponding filesystem step may have started yet.
            if !(expected.starts_with(line) || line.starts_with(expected)) {
                return Err(refused());
            }
            match serde_json::from_slice::<serde_json::Value>(line) {
                Err(e) if e.is_eof() => (),
                Ok(_) => {
                    let _: Frame = serde_json::from_value(
                        wire::original_value(std::str::from_utf8(line).map_err(|_| refused())?)
                            .map_err(|_| refused())?,
                    )
                    .map_err(|_| refused())?;
                }
                _ => return Err(refused()),
            }
            break;
        }
        let value = wire::original_value(
            std::str::from_utf8(&line[..line.len() - 1]).map_err(|_| refused())?,
        )
        .map_err(|_| refused())?;
        let frame: Frame = serde_json::from_value(value).map_err(|_| refused())?;
        match (count, frame) {
            (0, Frame::Allocated { allocation }) => {
                if allocation.abi != ABI
                    || allocation.directories != slot.versions
                    || !crate::creation::canonical_uuid(&allocation.nonce)
                    || !crate::creation::canonical_uuid(&allocation.before_id)
                    || !crate::creation::canonical_uuid(&allocation.after_id)
                {
                    return Err(refused());
                }
                pending = Some(Pending {
                    allocation,
                    created: None,
                    ready: None,
                });
            }
            (1, Frame::Created { nonce, object }) => {
                let p = pending.as_mut().ok_or_else(refused)?;
                if nonce != p.allocation.nonce {
                    return Err(refused());
                }
                p.created = Some(object);
            }
            (2, Frame::Ready { nonce, file }) => {
                let p = pending.as_mut().ok_or_else(refused)?;
                if nonce != p.allocation.nonce
                    || p.created
                        .as_ref()
                        .is_none_or(|created| created.object != file.object)
                    || file.length == 0
                    || file.length > super::BYTES as u64
                {
                    return Err(refused());
                }
                p.ready = Some(file);
            }
            _ => return Err(refused()),
        }
    }
    Ok(Snapshot {
        bytes,
        version,
        pending,
    })
}
fn tracking(slot: &Slot, lock: &File) -> io::Result<Snapshot> {
    lock_current(slot, lock)?;
    let (bytes, version) = read(lock, BYTES)?;
    let observed = decode(slot, bytes, version)?;
    fence(slot, lock, &observed)?;
    Ok(observed)
}
fn writable(slot: &Slot, lock: &File) -> io::Result<File> {
    lock_current(slot, lock)?;
    // Duplicate the SAME open file description, never a new lock owner.
    let file = lock.try_clone()?;
    file_version(&file, BYTES)?;
    Ok(file)
}
pub(super) fn fence(slot: &Slot, lock: &File, expected: &Snapshot) -> io::Result<()> {
    lock_current(slot, lock)?;
    let (bytes, version) = read(lock, BYTES)?;
    if version != expected.version || bytes != expected.bytes {
        return Err(refused());
    }
    Ok(())
}
fn append(
    slot: &Slot,
    lock: &File,
    old: &Snapshot,
    frame: Frame,
    sync: &mut impl FnMut(&File, bool) -> io::Result<()>,
) -> io::Result<Snapshot> {
    fence(slot, lock, old)?;
    let mut bytes = old.bytes.clone();
    bytes.extend(serde_json::to_vec(&frame).map_err(|_| refused())?);
    bytes.push(b'\n');
    if bytes.len() > BYTES {
        return Err(refused());
    }
    let file = writable(slot, lock)?;
    fence(slot, lock, old)?;
    file.write_all_at(&bytes[old.bytes.len()..], old.bytes.len() as u64)?;
    sync(&file, false)?;
    slot.folder.sync_all()?;
    let now = tracking(slot, lock)?;
    if now.bytes != bytes {
        return Err(refused());
    }
    Ok(now)
}
pub(super) fn allocate(
    slot: &Slot,
    disk: &Disk,
    lock: &File,
    next: &Record,
    bytes: &[u8],
    sync: &mut impl FnMut(&File, bool) -> io::Result<()>,
) -> io::Result<Snapshot> {
    let old = tracking(slot, lock)?;
    if !old.bytes.is_empty() || optional_candidate(slot)?.is_some() {
        return Err(refused());
    }
    disk.current(slot)?;
    let allocation = Allocation {
        abi: ABI.into(),
        nonce: uuid::Uuid::new_v4().to_string(),
        directories: slot.versions.clone(),
        before: disk.version.clone(),
        before_id: disk.record.intent.reconcile_id.clone(),
        before_sha256: sha256(&disk.bytes),
        after_id: next.intent.reconcile_id.clone(),
        after_sha256: sha256(bytes),
    };
    // The managed staging namespace is reserved durably BEFORE O_EXCL.
    append(slot, lock, &old, Frame::Allocated { allocation }, sync)
}
pub(super) fn created(
    slot: &Slot,
    lock: &File,
    allocated: &Snapshot,
    file: &File,
    sync: &mut impl FnMut(&File, bool) -> io::Result<()>,
) -> io::Result<Snapshot> {
    let p = allocated.pending.as_ref().ok_or_else(refused)?;
    let version = file_version(file, super::BYTES)?;
    if version.length != 0 || p.created.is_some() || p.ready.is_some() {
        return Err(refused());
    }
    let opened = open_at(&slot.folder, &name(slot), false)?;
    if file_version(&opened, super::BYTES)? != version {
        return Err(refused());
    }
    file.sync_all()?;
    append(
        slot,
        lock,
        allocated,
        Frame::Created {
            nonce: p.allocation.nonce.clone(),
            object: Created {
                object: version.object,
                born: born(file),
            },
        },
        sync,
    )
}
fn owned(pending: &Pending, file: &File) -> io::Result<()> {
    let current = file_version(file, super::BYTES)?;
    match &pending.created {
        Some(created)
            if current.object == created.object
                && created.born.is_none_or(|v| born(file) == Some(v)) =>
        {
            Ok(())
        }
        // No complete Created frame means the protocol has written NO bytes.
        // The durable allocation owns only this empty staging namespace.
        None if current.length == 0 => Ok(()),
        _ => Err(refused()),
    }
}
fn candidate(slot: &Slot, pending: &Pending, held: &File) -> io::Result<()> {
    slot.current()?;
    owned(pending, held)?;
    let opened = open_at(&slot.folder, &name(slot), false)?;
    let held_version = file_version(held, super::BYTES)?;
    if file_version(&opened, super::BYTES)? != held_version {
        return Err(refused());
    }
    if let Some(expected) = &pending.ready {
        let (bytes, version) = read(held, super::BYTES)?;
        if &version != expected || sha256(&bytes) != pending.allocation.after_sha256 {
            return Err(refused());
        }
        let value = wire::original_value(std::str::from_utf8(&bytes).map_err(|_| refused())?)
            .map_err(|_| refused())?;
        let next: Record = serde_json::from_value(value).map_err(|_| refused())?;
        let key = &next.intent.binding.key;
        if next.intent.reconcile_id != pending.allocation.after_id
            || !next.valid(&key.project, &key.task)
            || outbox::path(Path::new(""), &key.project, &key.task)
                .with_extension("reconcile")
                .file_name()
                != Some(slot.name.as_os_str())
        {
            return Err(refused());
        }
    }
    slot.current()
}
pub(super) fn ready(
    slot: &Slot,
    lock: &File,
    created: &Snapshot,
    file: &File,
    sync: &mut impl FnMut(&File, bool) -> io::Result<()>,
) -> io::Result<Snapshot> {
    let p = created.pending.as_ref().ok_or_else(refused)?;
    if p.created.is_none() || p.ready.is_some() {
        return Err(refused());
    }
    candidate(slot, p, file)?;
    let file_version = file_version(file, super::BYTES)?;
    let now = append(
        slot,
        lock,
        created,
        Frame::Ready {
            nonce: p.allocation.nonce.clone(),
            file: file_version,
        },
        sync,
    )?;
    verify_candidate(slot, lock, &now, file)?;
    Ok(now)
}
pub(super) fn verify_candidate(
    slot: &Slot,
    lock: &File,
    snapshot: &Snapshot,
    file: &File,
) -> io::Result<()> {
    fence(slot, lock, snapshot)?;
    let p = snapshot.pending.as_ref().ok_or_else(refused)?;
    if p.ready.is_none() {
        return Err(refused());
    }
    candidate(slot, p, file)?;
    fence(slot, lock, snapshot)
}
fn before(disk: &Disk, p: &Pending) -> bool {
    disk.version == p.allocation.before
        && disk.record.intent.reconcile_id == p.allocation.before_id
        && sha256(&disk.bytes) == p.allocation.before_sha256
}
fn published(disk: &Disk, p: &Pending) -> bool {
    p.ready
        .as_ref()
        .is_some_and(|v| disk.version.object == v.object)
        && p.created.as_ref().is_some_and(|created| {
            created
                .born
                .is_none_or(|value| born(&disk.file) == Some(value))
        })
        && disk.record.intent.reconcile_id == p.allocation.after_id
        && sha256(&disk.bytes) == p.allocation.after_sha256
}
struct Inspection {
    status: Status,
    snapshot: Snapshot,
    file: Option<File>,
}
fn inspect(slot: &Slot, disk: &Disk, lock: &File) -> io::Result<Inspection> {
    disk.current(slot)?;
    let snapshot = tracking(slot, lock)?;
    let file = optional_candidate(slot)?;
    let status = match (&snapshot.pending, &file) {
        (None, None) if snapshot.bytes.is_empty() => Status::Clear,
        (None, None) => Status::Recoverable, // Torn allocation: create never started.
        (Some(p), Some(file)) if before(disk, p) => {
            candidate(slot, p, file)?;
            Status::Recoverable
        }
        (Some(p), None) if before(disk, p) || published(disk, p) => Status::Recoverable,
        _ => Status::NeedsReview,
    };
    disk.current(slot)?;
    fence(slot, lock, &snapshot)?;
    Ok(Inspection {
        status,
        snapshot,
        file,
    })
}
pub(super) fn status(slot: &Slot, disk: &Disk) -> Status {
    (|| {
        let lock = open_at(&slot.folder, &slot.lock_name, false)?;
        inspect(slot, disk, &lock).map(|v| v.status)
    })()
    .unwrap_or(Status::NeedsReview)
}
pub(super) fn clear(
    slot: &Slot,
    lock: &File,
    snapshot: &Snapshot,
    sync: &mut impl FnMut(&File, bool) -> io::Result<()>,
) -> io::Result<()> {
    fence(slot, lock, snapshot)?;
    let file = writable(slot, lock)?;
    fence(slot, lock, snapshot)?;
    file.set_len(0)?;
    sync(&file, false)?;
    lock_current(slot, lock)
}
pub(super) fn recover(slot: &Slot, disk: &Disk, lock: &File) -> io::Result<()> {
    reconcile(slot, disk, lock, inspect(slot, disk, lock)?)
}
fn reconcile(slot: &Slot, disk: &Disk, lock: &File, observed: Inspection) -> io::Result<()> {
    reconcile_with_sync(slot, disk, lock, observed, &mut |file, _| file.sync_all())
}
fn reconcile_with_sync(
    slot: &Slot,
    disk: &Disk,
    lock: &File,
    observed: Inspection,
    sync: &mut impl FnMut(&File, bool) -> io::Result<()>,
) -> io::Result<()> {
    match observed.status {
        Status::Clear => return Ok(()),
        Status::NeedsReview => return Err(refused()),
        Status::Recoverable => (),
    }
    fence(slot, lock, &observed.snapshot)?;
    if let Some(file) = observed.file {
        let p = observed.snapshot.pending.as_ref().ok_or_else(refused)?;
        if !before(disk, p) {
            return Err(refused());
        }
        candidate(slot, p, &file)?;
        disk.current(slot)?;
        fence(slot, lock, &observed.snapshot)?;
        candidate(slot, p, &file)?;
        let name = CString::new(name(slot).as_bytes()).map_err(|_| refused())?;
        // SAFETY: the durably allocated unpublished staging object, rechecked
        // under the original flock; never the journal, checkpoint or marker.
        if unsafe { libc::unlinkat(slot.folder.as_raw_fd(), name.as_ptr(), 0) } != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    disk.current(slot)?;
    fence(slot, lock, &observed.snapshot)?;
    if optional_candidate(slot)?.is_some() {
        return Err(refused());
    }
    // Also commits a page-cache-only rename/absence BEFORE retiring its WAL.
    sync(&slot.folder, true)?;
    disk.current(slot)?;
    fence(slot, lock, &observed.snapshot)?;
    clear(slot, lock, &observed.snapshot, sync)
}
#[cfg(test)]
mod tests;
