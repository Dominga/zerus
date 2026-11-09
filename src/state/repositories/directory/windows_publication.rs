//! Publish through the original file handle without reopening a pinned parent
//! for write access. The parent's denied write/delete sharing stays in force.
use super::{Directory, File, OpenOptions, Result};
use std::{io::Write, os::windows::fs::OpenOptionsExt, os::windows::io::AsRawHandle};
use windows_sys::{
    Wdk::Storage::FileSystem::{
        FileRenameInformation, FileRenameInformationEx, NtSetInformationFile,
        FILE_RENAME_INFORMATION,
    },
    Win32::{
        Foundation::{RtlNtStatusToDosError, GENERIC_READ, GENERIC_WRITE},
        Storage::FileSystem::{
            DELETE, FILE_FLAG_OPEN_REPARSE_POINT, FILE_FLAG_WRITE_THROUGH, FILE_READ_ATTRIBUTES,
            FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, READ_CONTROL, SYNCHRONIZE,
        },
        System::IO::IO_STATUS_BLOCK,
    },
};

#[cfg(test)]
thread_local! {
    static PHASE: std::cell::Cell<&'static str> = const { std::cell::Cell::new("not_entered") };
    static BEFORE_RENAME: std::cell::RefCell<Option<BeforeRename>> = const { std::cell::RefCell::new(None) };
    static UNSUPPORTED_EX: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
#[cfg(test)]
type BeforeRename = Box<dyn FnOnce(&Directory, &str)>;
macro_rules! phase {
    ($name:literal) => {
        #[cfg(test)]
        PHASE.with(|phase| phase.set($name));
    };
}

pub(super) fn publish(
    directory: &Directory,
    name: &str,
    bytes: &[u8],
    replace: bool,
) -> Result<File> {
    phase!("validate_name");
    // A bounded single native component; never accept a path, stream, device
    // prefix or a Win32 trailing-dot/space alias for the published name.
    if name.is_empty()
        || name.len() > 1020
        || name.encode_utf16().count() > 255
        || name.contains(['/', '\\', ':', '\0'])
        || name.ends_with(['.', ' '])
    {
        return Err("Invalid file name".into());
    }
    phase!("verify_parent_before_create");
    directory.verify()?;
    let staging = directory
        .path
        .join(format!(".hgs-new-{}", uuid::Uuid::new_v4()));
    // Errors retain the owned staging/published object for review. Never
    // unlink a guessed name after a potentially completed publication.
    // All flags are combined in ONE custom_flags call. A second setter would
    // replace the no-reparse/write-through flags instead of adding to them.
    phase!("create_source");
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .access_mode(GENERIC_READ | GENERIC_WRITE | DELETE | SYNCHRONIZE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_WRITE_THROUGH)
        .share_mode(FILE_SHARE_READ)
        .open(&staging)
        .map_err(|e| e.to_string())?;
    phase!("source_privacy");
    if !crate::platform::verify_private_file(&file).map_err(|e| e.to_string())? {
        return Err("The new file must be private, regular and singly linked".into());
    }
    phase!("write_source");
    file.write_all(bytes).map_err(|e| e.to_string())?;
    phase!("flush_source");
    file.sync_all().map_err(|e| e.to_string())?;
    phase!("verify_parent_before_rename");
    directory.verify()?;
    phase!("guard_target");
    let target = if replace {
        Target::open(directory, name)?
    } else {
        None
    };
    #[cfg(test)]
    if let Some(hook) = BEFORE_RENAME.with(|slot| slot.borrow_mut().take()) {
        hook(directory, name);
    }
    if let Some(target) = &target {
        target.verify(directory, name)?;
    }
    // SAFETY: this SAME CREATE_NEW handle is synchronous, has DELETE access,
    // and denies other writers/deleters throughout the native rename.
    phase!("native_rename");
    // Only an existing, verified, DELETE-share-compatible target uses Ex.
    // Observed absence always stays NOREPLACE, including a racing appearance.
    unsafe { rename(&file, name, target.is_some()) }.map_err(|e| e.to_string())?;
    // Flush the renamed original, not a path-selected replacement. On NTFS
    // write-through also requests metadata flushing. This is not a directory
    // fsync claim for arbitrary filesystems/hardware; see the rollback guide.
    phase!("flush_renamed_source");
    file.sync_all().map_err(|e| e.to_string())?;
    phase!("verify_parent_after_rename");
    directory.verify()?;
    phase!("open_named_result");
    let named = directory.read_private(name)?;
    phase!("compare_original_identity");
    if crate::platform::FileIdentity::of(&file).map_err(|e| e.to_string())?
        != crate::platform::FileIdentity::of(&named).map_err(|e| e.to_string())?
    {
        return Err("The published original file changed".into());
    }
    if let Some(target) = &target {
        if crate::platform::FileIdentity::of(&target.file).map_err(|e| e.to_string())?
            != target.identity
        {
            return Err("The original replacement target changed".into());
        }
    }
    // Return the original handle, never adopt the path-selected check above.
    phase!("complete");
    Ok(file)
}

/// An access/share barrier, not authority or an atomic target-name CAS. The
/// caller's original writer exclusion and held/current checks still apply.
/// DELETE access refuses a reader that withheld delete sharing BEFORE Ex can
/// replace an open target. Keep this SAME guard through the whole operation.
struct Target {
    file: File,
    identity: crate::platform::FileIdentity,
}
impl Target {
    fn open(directory: &Directory, name: &str) -> Result<Option<Self>> {
        let file = match OpenOptions::new()
            .access_mode(DELETE | FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .open(directory.path.join(name))
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.to_string()),
        };
        let identity = crate::platform::FileIdentity::of(&file).map_err(|e| e.to_string())?;
        let target = Self { file, identity };
        target.verify(directory, name)?;
        Ok(Some(target))
    }
    fn verify(&self, directory: &Directory, name: &str) -> Result<()> {
        directory.verify()?;
        if !crate::platform::verify_private_file(&self.file).map_err(|e| e.to_string())?
            || self
                .file
                .metadata()
                .map_err(|e| e.to_string())?
                .permissions()
                .readonly()
            || crate::platform::FileIdentity::of(&self.file).map_err(|e| e.to_string())?
                != self.identity
        {
            return Err("The replacement target must be unchanged, private, regular, singly linked and writable".into());
        }
        let named = directory.read_private(name)?;
        if crate::platform::FileIdentity::of(&named).map_err(|e| e.to_string())? != self.identity {
            return Err("The replacement target name changed".into());
        }
        directory.verify()
    }
}

/// # Safety
/// `file` is the original synchronous CREATE_NEW handle with DELETE access.
/// `name` is the bounded simple component validated by publish().
unsafe fn rename(file: &File, name: &str, replace: bool) -> std::io::Result<()> {
    #[cfg(test)]
    if replace && UNSUPPORTED_EX.with(std::cell::Cell::get) {
        return Err(std::io::Error::from_raw_os_error(50));
    }
    let name = name.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let bytes = std::mem::size_of::<FILE_RENAME_INFORMATION>() + name.len() * 2;
    let mut storage = vec![0u64; bytes.div_ceil(8)];
    let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
    let mut status = IO_STATUS_BLOCK::default();
    // SAFETY: aligned initialized storage contains the complete header/name.
    // NULL RootDirectory and a simple name select the original file's SAME
    // parent, never cwd or a pathname-opened destination directory.
    // https://learn.microsoft.com/windows-hardware/drivers/ddi/ntifs/ns-ntifs-_file_rename_information
    let result = unsafe {
        if replace {
            // Exactly REPLACE_IF_EXISTS | POSIX_SEMANTICS, never readonly
            // override. Original target handles keep their original bytes.
            (*info).Anonymous.Flags = 0x00000001 | 0x00000002;
        } else {
            (*info).Anonymous.ReplaceIfExists = false;
        }
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
            if replace {
                FileRenameInformationEx
            } else {
                FileRenameInformation
            },
        )
    };
    if result < 0 {
        return Err(std::io::Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosError(result) } as i32,
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "windows_publication/tests.rs"]
mod tests;
