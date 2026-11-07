//! Same-parent publication of the original owned metadata receipt. Keep the
//! directory pins: a pathname rename reopens that parent for write access and
//! conflicts with their intentionally denied write sharing.
use std::{fs::File, io, os::windows::io::AsRawHandle};
use windows_sys::{
    Wdk::Storage::FileSystem::{
        FileRenameInformation, NtSetInformationFile, FILE_RENAME_INFORMATION,
    },
    Win32::{Foundation::RtlNtStatusToDosError, System::IO::IO_STATUS_BLOCK},
};

/// # Safety
/// `file` must be a synchronous handle (opened without OVERLAPPED) with DELETE
/// access. This is the original exclusive CREATE_NEW handle, not a path reopen.
pub(super) unsafe fn rename_same_parent(file: &File, name: &str) -> io::Result<()> {
    // The only caller supplies key(): a SHA-256 basename followed by .json.
    // Reject all paths, streams, separators and unexpected names before I/O.
    if name.len() != 69
        || !name.ends_with(".json")
        || !name.as_bytes()[..64]
            .iter()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid receipt basename",
        ));
    }
    let name = name.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let bytes = std::mem::size_of::<FILE_RENAME_INFORMATION>() + name.len() * 2;
    let mut storage = vec![0u64; bytes.div_ceil(8)];
    let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
    let mut status = IO_STATUS_BLOCK::default();
    // SAFETY: aligned initialized storage holds the full header and UTF-16
    // basename. The caller's CREATE_NEW handle is synchronous (no OVERLAPPED),
    // has DELETE access, and remains exclusively owned through this call.
    // RootDirectory=NULL plus a simple name is the native same-directory form:
    // no ambient cwd, directory reopen, or pathname-selected source file.
    // https://learn.microsoft.com/windows-hardware/drivers/ddi/ntifs/ns-ntifs-_file_rename_information
    let result = unsafe {
        (*info).Anonymous.ReplaceIfExists = true;
        (*info).RootDirectory = std::ptr::null_mut();
        (*info).FileNameLength = ((name.len() - 1) * 2) as u32;
        std::ptr::copy_nonoverlapping(
            name.as_ptr(),
            std::ptr::addr_of_mut!((*info).FileName).cast::<u16>(),
            name.len(),
        );
        NtSetInformationFile(
            file.as_raw_handle(),
            &mut status,
            info.cast(),
            bytes as u32,
            FileRenameInformation,
        )
    };
    if result < 0 {
        return Err(io::Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosError(result) } as i32,
        ));
    }
    Ok(())
}
