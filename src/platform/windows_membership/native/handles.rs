//! Read only the source process's handle table and metadata-only duplicates.
//! No global kernel-pointer table, source-handle close, write or stop operation.
use super::super::handles::{
    after_metadata_retry, with_details, Details, Handle, Metadata, Object, Snapshot,
};
use super::*;
use windows_sys::Wdk::System::Threading::ProcessHandleInformation;
use windows_sys::Win32::{
    Foundation::{DuplicateHandle, SetLastError, ERROR_SUCCESS},
    Storage::FileSystem::{
        FileIdInfo, FileStandardInfo, GetFileInformationByHandleEx, GetFileType, FILE_ID_INFO,
        FILE_READ_ATTRIBUTES, FILE_STANDARD_INFO, FILE_TYPE_CHAR, FILE_TYPE_DISK, FILE_TYPE_PIPE,
    },
    System::Threading::PROCESS_DUP_HANDLE,
};

fn snapshot(api: &Api, pin: &Pinned, until: Instant) -> Result<Snapshot> {
    pin.current(until)?;
    let query = |handle| {
        bounded_table(until, TableKind::ProcessHandles, |buffer| {
            let mut returned = 0;
            // SAFETY: the original retained query handle, fixed read-only native
            // information class and aligned bounded writable storage.
            let status = unsafe {
                (api.process)(
                    handle,
                    ProcessHandleInformation,
                    buffer.as_mut_ptr().cast(),
                    (buffer.len() * 8) as u32,
                    &mut returned,
                )
            };
            (status, returned as usize)
        })
    };
    let bytes = match query(pin.handle.as_raw_handle()) {
        Err(Error::Denied) => {
            let handle = pin.additional(PROCESS_QUERY_INFORMATION, until)?;
            query(handle.as_raw_handle())
        }
        other => other,
    };
    pin.current(until)?;
    Snapshot::decode(&bytes?, until)
}

struct Files<'a> {
    pin: &'a Pinned,
    source: OwnedHandle,
}
enum Access {
    Identity,
    Attributes,
}
impl Access {
    fn bits(&self) -> u32 {
        match self {
            Self::Identity => 0,
            Self::Attributes => FILE_READ_ATTRIBUTES,
        }
    }
}
impl<'a> Files<'a> {
    fn open(pin: &'a Pinned, until: Instant) -> Result<Self> {
        // DUP_HANDLE is requested independently, never bundled with VM_READ or
        // query rights. A denial cannot erase a separately readable cwd/image.
        Ok(Self {
            pin,
            source: pin.additional(PROCESS_DUP_HANDLE, until)?,
        })
    }
    fn copy(&self, original: Handle, access: Access, until: Instant) -> Result<OwnedHandle> {
        self.pin.current(until)?;
        let mut copy = std::ptr::null_mut();
        let source = usize::try_from(original.value).map_err(|_| Error::Incomplete)? as HANDLE;
        // SAFETY: original process object stays held. Ask for metadata access
        // only, with NO DUPLICATE_SAME_ACCESS or DUPLICATE_CLOSE_SOURCE flags.
        let ok = unsafe {
            DuplicateHandle(
                self.source.as_raw_handle(),
                source,
                GetCurrentProcess(),
                &mut copy,
                access.bits(),
                0,
                0,
            )
        } != 0;
        let failure = (!ok).then(win32_error);
        // Own the result before any later validation can fail.
        let copy = if ok && !copy.is_null() {
            // SAFETY: successful DuplicateHandle transfers this ONE new handle.
            Some(unsafe { OwnedHandle::from_raw_handle(copy) })
        } else {
            None
        };
        self.pin.current(until)?;
        if let Some(error) = failure {
            return Err(error);
        }
        copy.ok_or(Error::Incomplete)
    }
    fn metadata(&self, original: Handle, until: Instant) -> Result<Option<Metadata>> {
        let copy = self.copy(original, Access::Identity, until)?;
        let first = metadata(copy.as_raw_handle(), until);
        self.pin.current(until)?;
        let denied = matches!(
            first,
            Err(Error::Denied)
                | Ok(Some(Metadata {
                    details: Details::Denied,
                    ..
                }))
        );
        if !denied {
            return first;
        }
        // Try stronger METADATA rights only after a genuine lower-rights read
        // denial, retaining the first copy and every readable object identity.
        let attributes = match self.copy(original, Access::Attributes, until) {
            Err(Error::Denied) => return first,
            other => other?,
        };
        // SAFETY: both owned file copies remain live. A reused source handle
        // must not lend a replacement object to this original observation.
        if unsafe { CompareObjectHandles(copy.as_raw_handle(), attributes.as_raw_handle()) } == 0 {
            return Err(Error::Incomplete);
        }
        let second = metadata(attributes.as_raw_handle(), until);
        self.pin.current(until)?;
        after_metadata_retry(first, second)
    }
}
fn metadata(handle: HANDLE, until: Instant) -> Result<Option<Metadata>> {
    live(until)?;
    // FILE_TYPE_UNKNOWN can also be a successful result. Clear last-error so
    // that only this native read's actual denial can become a denied half.
    unsafe {
        SetLastError(ERROR_SUCCESS);
    }
    // SAFETY: caller retains the original owned handle for this entire call.
    let kind = unsafe { GetFileType(handle) };
    let unknown = (kind != FILE_TYPE_DISK && kind != FILE_TYPE_PIPE && kind != FILE_TYPE_CHAR)
        .then(win32_error);
    live(until)?;
    match kind {
        FILE_TYPE_PIPE | FILE_TYPE_CHAR => return Ok(None),
        FILE_TYPE_DISK => {}
        // Unsupported/invalid types remain incomplete; a genuine access denial
        // is preserved without borrowing an earlier native call's LastError.
        _ => return Err(unknown.unwrap_or(Error::Incomplete)),
    }
    let mut id = FILE_ID_INFO::default();
    // SAFETY: correct native information class, full typed output storage.
    if unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileIdInfo,
            (&mut id as *mut FILE_ID_INFO).cast(),
            mem::size_of::<FILE_ID_INFO>() as u32,
        )
    } == 0
    {
        let error = win32_error();
        live(until)?;
        return Err(error);
    }
    live(until)?;
    if id.FileId.Identifier == [0; 16] {
        return Err(Error::Incomplete);
    }
    let object = Object {
        volume: id.VolumeSerialNumber,
        file: id.FileId.Identifier,
    };
    let mut standard = FILE_STANDARD_INFO::default();
    // SAFETY: same retained metadata-only handle and correct typed output.
    if unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileStandardInfo,
            (&mut standard as *mut FILE_STANDARD_INFO).cast(),
            mem::size_of::<FILE_STANDARD_INFO>() as u32,
        )
    } == 0
    {
        let error = win32_error();
        live(until)?;
        return with_details(object, Err(error)).map(Some);
    }
    live(until)?;
    if standard.NumberOfLinks == 0 || standard.DeletePending {
        return Err(Error::Incomplete);
    }
    with_details(
        object,
        Ok(Details::Known {
            links: standard.NumberOfLinks,
            directory: standard.Directory,
        }),
    )
    .map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs::File,
        io::{Seek, SeekFrom, Write},
        time::Duration,
    };
    use windows_sys::Win32::{Foundation::ERROR_ACCESS_DENIED, Storage::FileSystem::WriteFile};
    #[test]
    fn real_local_handle_table_metadata_copy_and_hard_link_identity_agree() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("original");
        let mut original = File::create(&path).unwrap();
        original.write_all(b"original bytes").unwrap();
        std::fs::hard_link(&path, root.path().join("alias")).unwrap();
        let alias = File::open(root.path().join("alias")).unwrap();
        let until = Instant::now() + Duration::from_secs(2);
        let api = Api::new().unwrap();
        let census = api.census(until).unwrap();
        let pin = Pinned::open(census[&std::process::id()], until).unwrap();
        let before = snapshot(&api, &pin, until).unwrap();
        let kind = before.file_kind(original.as_raw_handle() as u64).unwrap();
        // This calibration handle is actually a local disk file, not a guessed
        // object-type index. Both source file handles remain charged/listed.
        assert_eq!(
            unsafe { GetFileType(original.as_raw_handle()) },
            FILE_TYPE_DISK
        );
        let members: Vec<_> = before.files(kind).collect();
        let a = members
            .iter()
            .find(|h| h.value == original.as_raw_handle() as u64)
            .copied()
            .unwrap();
        let b = members
            .iter()
            .find(|h| h.value == alias.as_raw_handle() as u64)
            .copied()
            .unwrap();
        let files = Files::open(&pin, until).unwrap();
        let first = files.metadata(a, until).unwrap().unwrap();
        assert_eq!(first, files.metadata(b, until).unwrap().unwrap());
        assert_eq!(
            first.details,
            Details::Known {
                links: 2,
                directory: false
            }
        );
        let copy = files.copy(a, Access::Identity, until).unwrap();
        assert_eq!(
            metadata(copy.as_raw_handle(), until)
                .unwrap()
                .unwrap()
                .object,
            first.object
        );
        let mut written = 0;
        // A broad read/write source must not lend its write access to the
        // metadata copy. This attempted write targets only the private fixture.
        assert_eq!(
            unsafe {
                WriteFile(
                    copy.as_raw_handle(),
                    b"x".as_ptr(),
                    1,
                    &mut written,
                    std::ptr::null_mut(),
                )
            },
            0
        );
        assert_eq!(unsafe { GetLastError() }, ERROR_ACCESS_DENIED);
        drop(copy);
        original.seek(SeekFrom::Start(0)).unwrap();
        original.write_all(b"original bytes").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"original bytes");
        assert_eq!(api.census(until).unwrap()[&pin.process.pid], pin.process);
    }
}
