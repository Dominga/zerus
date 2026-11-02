//! Managed local-file checkpoint. Ancestors cannot be renamed while pinned;
//! the leaf denies all read/write/delete sharing until the capture is dropped.
//! No ACL repair, path fallback, volume handle, helper process or worker.
use super::{
    refused, serialized, Checkpoint, ReadMarks, Source, CHECKPOINT_BYTES, CHECKPOINT_MARKS,
};
use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Seek, Write},
    os::windows::{
        ffi::OsStrExt,
        fs::OpenOptionsExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Component, Path, PathBuf, Prefix},
    sync::Mutex,
};
use windows_sys::Win32::{
    Foundation::{
        LocalFree, ERROR_ALREADY_EXISTS, ERROR_SUCCESS, GENERIC_READ, GENERIC_WRITE, HANDLE,
        INVALID_HANDLE_VALUE,
    },
    Security::{
        Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
            GetSecurityInfo, SE_FILE_OBJECT,
        },
        GetAce, GetLengthSid, GetTokenInformation, IsWellKnownSid, TokenUser,
        WinBuiltinAdministratorsSid, WinCreatorOwnerRightsSid, WinLocalSystemSid,
        ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, DACL_SECURITY_INFORMATION, INHERIT_ONLY_ACE,
        OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES, TOKEN_QUERY,
        TOKEN_USER,
    },
    Storage::FileSystem::*,
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};
use zerus_desktop_migration::sha256;

const READ_CONTROL: u32 = 0x0002_0000;
const DELETE: u32 = 0x0001_0000;

struct Allocation(PSECURITY_DESCRIPTOR);
impl Drop for Allocation {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}
fn owned(raw: HANDLE) -> io::Result<OwnedHandle> {
    if raw.is_null() || raw == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a successful call transfers its one new native handle.
    Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
}
fn user() -> io::Result<Vec<u8>> {
    let mut raw = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = owned(raw)?;
    let mut size = 0;
    unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            std::ptr::null_mut(),
            0,
            &mut size,
        );
    }
    if size == 0 || size > 64 << 10 {
        return Err(refused("checkpoint user is unavailable"));
    }
    let mut data = vec![0u64; (size as usize).div_ceil(8)];
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            data.as_mut_ptr().cast(),
            size,
            &mut size,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: TOKEN_USER and SID occupy this aligned initialized native output.
    let sid = unsafe { (*(data.as_ptr().cast::<TOKEN_USER>())).User.Sid };
    Ok(unsafe { sid_bytes(sid) }.to_vec())
}
unsafe fn sid_bytes<'a>(sid: PSID) -> &'a [u8] {
    // SAFETY: caller supplies a SID from a successful native security query.
    unsafe { std::slice::from_raw_parts(sid.cast(), GetLengthSid(sid) as usize) }
}
fn descriptor(directory: bool) -> io::Result<Allocation> {
    let sid = user()?;
    let mut text = std::ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(sid.as_ptr() as PSID, &mut text) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let _text = Allocation(text.cast());
    let mut length = 0;
    while length < 256 && unsafe { *text.add(length) } != 0 {
        length += 1;
    }
    if length == 256 {
        return Err(refused("checkpoint SID exceeds bound"));
    }
    let sid = String::from_utf16(unsafe { std::slice::from_raw_parts(text, length) })
        .map_err(|_| refused("checkpoint SID is invalid"))?;
    // New directories protect and inherit the same user/SYSTEM/Admin policy as
    // managed Windows state. Files inherit that validated ACL, with explicit
    // individual ownership even when created by an elevated token.
    let sddl = if directory {
        format!("O:{sid}D:P(A;OICI;FA;;;{sid})(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)")
    } else {
        format!("O:{sid}")
    };
    let wide: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
    let mut raw = std::ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide.as_ptr(),
            1,
            &mut raw,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(Allocation(raw))
}
fn wide(path: &Path) -> io::Result<Vec<u16>> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    if wide.is_empty() || wide.len() > 32767 || wide.contains(&0) {
        return Err(refused("checkpoint path exceeds bound"));
    }
    Ok(wide.into_iter().chain(Some(0)).collect())
}
fn info(file: &File) -> io::Result<BY_HANDLE_FILE_INFORMATION> {
    let mut value = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut value) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(value)
}
fn verify(file: &File, directory: bool) -> io::Result<()> {
    let value = info(file)?;
    if value.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || (value.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0) != directory
        || (!directory && value.nNumberOfLinks != 1)
    {
        return Err(refused("checkpoint reparse, kind or link count refused"));
    }
    let mut owner = std::ptr::null_mut();
    let mut acl: *mut ACL = std::ptr::null_mut();
    let mut raw = std::ptr::null_mut();
    let code = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            std::ptr::null_mut(),
            &mut acl,
            std::ptr::null_mut(),
            &mut raw,
        )
    };
    if code != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(code as i32));
    }
    let _descriptor = Allocation(raw);
    let sid = user()?;
    if owner.is_null() || acl.is_null() || unsafe { sid_bytes(owner) } != sid {
        return Err(refused("checkpoint source owner or ACL refused"));
    }
    for index in 0..u32::from(unsafe { (*acl).AceCount }) {
        let mut ace = std::ptr::null_mut();
        if unsafe { GetAce(acl, index, &mut ace) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let header = unsafe { &*(ace as *const ACE_HEADER) };
        if (!directory && u32::from(header.AceFlags) & INHERIT_ONLY_ACE != 0) || header.AceType == 1
        {
            continue;
        }
        if header.AceType != 0 {
            return Err(refused("checkpoint ACL kind refused"));
        }
        let recipient =
            unsafe { std::ptr::addr_of!((*(ace as *const ACCESS_ALLOWED_ACE)).SidStart) } as PSID;
        if !unsafe {
            sid_bytes(recipient) == sid
                || IsWellKnownSid(recipient, WinLocalSystemSid) != 0
                || IsWellKnownSid(recipient, WinBuiltinAdministratorsSid) != 0
                || IsWellKnownSid(recipient, WinCreatorOwnerRightsSid) != 0
        } {
            return Err(refused("checkpoint ACL is not private"));
        }
    }
    Ok(())
}

#[derive(Debug)]
struct Parents {
    _handles: Vec<File>,
}
impl Parents {
    fn open(path: &Path, create: bool) -> io::Result<Self> {
        // No UNC/device/remote namespace, ADS, traversal or unbounded walk.
        if !path.is_absolute()
            || !matches!(path.components().next(),Some(Component::Prefix(p)) if matches!(p.kind(),Prefix::Disk(_)|Prefix::VerbatimDisk(_)))
        {
            return Err(refused("local absolute managed checkpoint path required"));
        }
        wide(path)?;
        let mut current = PathBuf::new();
        let mut handles = Vec::new();
        for part in path.components() {
            if handles.len() == 128 || matches!(part, Component::ParentDir | Component::CurDir) {
                return Err(refused("checkpoint parent bound or traversal refused"));
            }
            if let Component::Normal(name) = part {
                let s = name.to_string_lossy();
                if s.contains(':') || s.ends_with(['.', ' ']) {
                    return Err(refused("checkpoint path alias refused"));
                }
            }
            current.push(part);
            if matches!(part, Component::Prefix(_)) || !current.has_root() {
                continue;
            }
            if matches!(part, Component::RootDir)
                && !matches!(unsafe { GetDriveTypeW(wide(&current)?.as_ptr()) }, 2 | 3)
            {
                // DRIVE_REMOVABLE / DRIVE_FIXED only: do not turn a managed
                // local checkpoint into an unbounded remote-drive open.
                return Err(refused("local checkpoint drive required"));
            }
            let open = || {
                OpenOptions::new()
                    .access_mode(READ_CONTROL | FILE_READ_ATTRIBUTES)
                    .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                    .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                    .open(&current)
            };
            let file = match open() {
                Ok(file) => file,
                Err(e) if create && e.kind() == io::ErrorKind::NotFound => {
                    let sd = descriptor(true)?;
                    let sa = SECURITY_ATTRIBUTES {
                        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                        lpSecurityDescriptor: sd.0,
                        bInheritHandle: 0,
                    };
                    if unsafe { CreateDirectoryW(wide(&current)?.as_ptr(), &sa) } == 0
                        && io::Error::last_os_error().raw_os_error()
                            != Some(ERROR_ALREADY_EXISTS as i32)
                    {
                        return Err(io::Error::last_os_error());
                    }
                    open()?
                }
                Err(e) => return Err(e),
            };
            let value = info(&file)?;
            if value.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
                || value.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0
            {
                return Err(refused(
                    "checkpoint parent is a reparse point or non-directory",
                ));
            }
            if current == path {
                verify(&file, true)?;
            }
            handles.push(file);
        }
        if handles.is_empty() {
            return Err(refused("checkpoint parent is unavailable"));
        }
        Ok(Self { _handles: handles })
    }
}
fn filename(path: &Path) -> io::Result<()> {
    let name = path
        .file_name()
        .ok_or_else(|| refused("checkpoint leaf missing"))?
        .to_string_lossy();
    if name.contains(':') || name.ends_with(['.', ' ']) {
        return Err(refused("checkpoint leaf alias refused"));
    }
    Ok(())
}
fn file(path: &Path, create: bool) -> io::Result<File> {
    let sd = if create {
        Some(descriptor(false)?)
    } else {
        None
    };
    let sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.as_ref().map_or(std::ptr::null_mut(), |v| v.0),
        bInheritHandle: 0,
    };
    let raw = unsafe {
        CreateFileW(
            wide(path)?.as_ptr(),
            GENERIC_READ | GENERIC_WRITE | READ_CONTROL | if create { DELETE } else { 0 },
            0,
            if create { &sa } else { std::ptr::null() },
            if create { CREATE_NEW } else { OPEN_EXISTING },
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_WRITE_THROUGH,
            std::ptr::null_mut(),
        )
    };
    let raw = owned(raw)?;
    // SAFETY: ownership passes directly from the successful original handle.
    let file = File::from(raw);
    if let Err(error) = verify(&file, false) {
        return Err(if create {
            cleanup_failed_temporary(&file, error)
        } else {
            error
        });
    }
    Ok(file)
}

// Only for an unpublished CREATE_NEW file whose original DELETE-capable
// handle we still own. Never select a cleanup target by a later path lookup.
// https://learn.microsoft.com/windows/win32/api/winbase/ns-winbase-file_disposition_info
fn cleanup_failed_temporary(file: &File, primary: io::Error) -> io::Error {
    let mut disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    // SAFETY: initialized native input; this synchronous call borrows the
    // original owned handle. The caller closes it after this function returns.
    if unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileDispositionInfo,
            (&mut disposition as *mut FILE_DISPOSITION_INFO).cast(),
            std::mem::size_of_val(&disposition) as u32,
        )
    } == 0
    {
        // Keep both failures. A failed cleanup is not retryable sharing
        // contention, and never authorizes another delete or a path fallback.
        io::Error::other(format!(
            "{primary}; owned checkpoint temporary cleanup failed: {}",
            io::Error::last_os_error()
        ))
    } else {
        primary
    }
}
fn bytes(file: &mut File, limit: usize) -> io::Result<Vec<u8>> {
    verify(file, false)?;
    if file.metadata()?.len() > limit as u64 {
        return Err(refused("checkpoint file exceeds its bounds"));
    }
    file.rewind()?;
    let mut data = Vec::new();
    (&mut *file)
        .take((limit + 1) as u64)
        .read_to_end(&mut data)?;
    if data.len() > limit {
        return Err(refused("checkpoint byte limit"));
    }
    verify(file, false)?;
    Ok(data)
}

#[derive(Debug)]
pub(super) struct Opened {
    file: File,
    _parents: Parents,
    path: PathBuf,
    hash: String,
    bytes: usize,
}
fn current(path: &Path) -> io::Result<Option<Opened>> {
    filename(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| refused("checkpoint parent missing"))?;
    let parents = match Parents::open(parent, false) {
        Ok(v) => v,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let mut file = match file(path, false) {
        Ok(v) => v,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let data = bytes(&mut file, CHECKPOINT_BYTES)?;
    Ok(Some(Opened {
        file,
        _parents: parents,
        path: path.into(),
        hash: sha256(&data),
        bytes: data.len(),
    }))
}

// Rename the ORIGINAL owned temporary handle, never a later path-selected file.
// https://learn.microsoft.com/windows/win32/api/winbase/ns-winbase-file_rename_info
fn rename(file: &File, path: &Path, replace: bool) -> io::Result<()> {
    let name = wide(path)?;
    let offset = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
    let size = (offset + name.len() * 2).max(std::mem::size_of::<FILE_RENAME_INFO>());
    let mut storage = vec![0u64; size.div_ceil(8)];
    let data = storage.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    // SAFETY: aligned initialized allocation holds the header and full UTF-16
    // name, including its terminator, for this one synchronous native call.
    unsafe {
        (*data).Anonymous.ReplaceIfExists = replace;
        (*data).RootDirectory = std::ptr::null_mut();
        (*data).FileNameLength = ((name.len() - 1) * 2) as u32;
        std::ptr::copy_nonoverlapping(
            name.as_ptr(),
            std::ptr::addr_of_mut!((*data).FileName).cast::<u16>(),
            name.len(),
        );
        if SetFileInformationByHandle(
            file.as_raw_handle(),
            FileRenameInfo,
            data.cast(),
            size as u32,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}
#[derive(Debug)]
enum PublicationFailure {
    Before(io::Error),
    Unknown(io::Error),
}
impl From<io::Error> for PublicationFailure {
    fn from(error: io::Error) -> Self {
        Self::Before(error)
    }
}
fn publish(path: &Path, data: &[u8], old: Option<Opened>) -> Result<Opened, PublicationFailure> {
    publish_with(path, data, old, |file, data| {
        file.write_all(data)?;
        file.sync_all()?;
        verify(file, false)
    })
}
fn publish_with(
    path: &Path,
    data: &[u8],
    old: Option<Opened>,
    prepare: impl FnOnce(&mut File, &[u8]) -> io::Result<()>,
) -> Result<Opened, PublicationFailure> {
    filename(path)?;
    let parents = Parents::open(
        path.parent()
            .ok_or_else(|| refused("checkpoint parent missing"))?,
        true,
    )?;
    let mut created = None;
    for _ in 0..16 {
        let temporary = crate::fsx::temporary(path);
        match file(&temporary, true) {
            Ok(file) => {
                created = Some(file);
                break;
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
    let mut file = created.ok_or_else(|| refused("checkpoint temporary-name capacity"))?;
    // Before publication, preserve the primary failure and delete only this
    // newly created temp via its original handle. Once rename is attempted the
    // outcome may be unknown: neither source nor temp may be deleted as recovery.
    if let Err(error) = prepare(&mut file, data) {
        return Err(PublicationFailure::Before(cleanup_failed_temporary(
            &file, error,
        )));
    }
    let replace = old.is_some();
    drop(old);
    rename(&file, path, replace).map_err(PublicationFailure::Unknown)?;
    file.sync_all().map_err(PublicationFailure::Unknown)?;
    let written = bytes(&mut file, CHECKPOINT_BYTES).map_err(PublicationFailure::Unknown)?;
    if written != data {
        return Err(PublicationFailure::Unknown(refused(
            "checkpoint changed after publication",
        )));
    }
    Ok(Opened {
        file,
        _parents: parents,
        path: path.into(),
        hash: sha256(data),
        bytes: data.len(),
    })
}

pub(super) fn checkpoint(marks: &mut ReadMarks) -> io::Result<Checkpoint> {
    checkpoint_with(marks, publish)
}
fn checkpoint_with(
    marks: &mut ReadMarks,
    publisher: impl FnOnce(&Path, &[u8], Option<Opened>) -> Result<Opened, PublicationFailure>,
) -> io::Result<Checkpoint> {
    if !marks.checkpoint.initial_ok || marks.checkpoint.source == Source::Unavailable {
        return Err(refused(
            "pending read checkpoint source was not loaded reliably",
        ));
    }
    if marks.marks.len() > CHECKPOINT_MARKS {
        return Err(refused("pending read checkpoint row limit"));
    }
    let mut disk = current(&marks.path)?;
    let observed = disk
        .as_ref()
        .map(|v| Source::Hash(v.hash.clone()))
        .unwrap_or(Source::Missing);
    if observed != marks.checkpoint.source {
        return Err(refused("pending read checkpoint source changed"));
    }
    if marks.checkpoint.dirty || disk.is_none() {
        let data = serialized(marks, CHECKPOINT_BYTES)?;
        disk = Some(match publisher(&marks.path, &data, disk) {
            Ok(published) => published,
            Err(PublicationFailure::Before(error)) => return Err(error),
            Err(PublicationFailure::Unknown(error)) => {
                // A rename/flush outcome cannot authorize another rewrite,
                // even if a later hash happens to equal the old source.
                marks.checkpoint.source = Source::Unavailable;
                // Do not expose a retryable native sharing code for an
                // uncertain publication, even if the underlying error was 32.
                return Err(io::Error::other(format!(
                    "checkpoint publication outcome is unknown: {error}"
                )));
            }
        });
        marks.checkpoint.saved(&data);
        marks.unsaved = false;
    }
    let disk = disk.ok_or_else(|| refused("pending read checkpoint missing file"))?;
    // Only pre-publication sharing contention can request an automatic retry.
    // A completed checkpoint's final sync failure stays an explicit refusal.
    disk.file
        .sync_all()
        .map_err(|error| io::Error::other(format!("checkpoint final sync failed: {error}")))?;
    Ok(Checkpoint {
        sha256: disk.hash.clone(),
        bytes: disk.bytes,
        marks: marks.marks.len(),
        original: Mutex::new(disk),
    })
}
pub(super) fn checkpoint_bytes(path: &Path, checkpoint: &Checkpoint) -> io::Result<Vec<u8>> {
    let mut disk = checkpoint
        .original
        .try_lock()
        .map_err(|_| refused("checkpoint source is busy or unavailable"))?;
    if disk.path != path || disk.hash != checkpoint.sha256 || disk.bytes != checkpoint.bytes {
        return Err(refused("checkpoint source identity changed"));
    }
    let data = bytes(&mut disk.file, CHECKPOINT_BYTES)?;
    if data.len() != checkpoint.bytes || sha256(&data) != checkpoint.sha256 {
        return Err(refused("pending read checkpoint changed during capture"));
    }
    Ok(data)
}

#[cfg(test)]
mod tests;
