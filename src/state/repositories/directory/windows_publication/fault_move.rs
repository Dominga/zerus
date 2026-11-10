//! Private Windows fault establishment only. No product operation uses this
//! existing-source handle contract; publish keeps its CREATE_NEW contract.
use super::{Directory, File, Result, Target};
use crate::platform::FileIdentity;
use std::os::windows::io::AsRawHandle;
use windows_sys::{
    Wdk::Storage::FileSystem::{
        FileRenameInformation, NtSetInformationFile, FILE_RENAME_INFORMATION,
    },
    Win32::{Foundation::RtlNtStatusToDosError, System::IO::IO_STATUS_BLOCK},
};

/// Keep the extra original-object guard, caller reader and original parent
/// alive through replacement/refusal/no-repair assertions in the fixture.
pub(in crate::state) struct Moved<'a> {
    directory: &'a Directory,
    reader: &'a File,
    original: Target,
    source: String,
    destination: String,
}
impl Moved<'_> {
    fn verify_moved(&self) -> Result<()> {
        self.directory.verify()?;
        if FileIdentity::of(self.reader).map_err(|e| e.to_string())? != self.original.identity
            || FileIdentity::of(&self.original.file).map_err(|e| e.to_string())?
                != self.original.identity
            || !crate::platform::verify_private_file(&self.original.file)
                .map_err(|e| e.to_string())?
        {
            return Err("The private moved original changed".into());
        }
        let named = self.directory.read_private(&self.destination)?;
        if FileIdentity::of(&named).map_err(|e| e.to_string())? != self.original.identity {
            return Err("The private moved destination changed".into());
        }
        self.directory.verify()
    }
    pub(in crate::state) fn verify_replacement(&self) -> Result<()> {
        self.verify_moved()?;
        let replacement = self.directory.read_private(&self.source)?;
        if FileIdentity::of(&replacement).map_err(|e| e.to_string())? == self.original.identity {
            return Err("The private replacement must be a different native object".into());
        }
        self.directory.verify()
    }
}

fn simple(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 1020
        && name.encode_utf16().count() <= 255
        && !name.contains(['/', '\\', ':', '\0'])
        && !name.ends_with(['.', ' '])
}
fn absent(directory: &Directory, name: &str) -> Result<()> {
    match std::fs::symlink_metadata(directory.path.join(name)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
        Ok(_) => Err("Private fault destination must be absent".into()),
    }
}

pub(in crate::state) fn move_original<'a>(
    directory: &'a Directory,
    reader: &'a File,
    source: &str,
    destination: &str,
) -> Result<Moved<'a>> {
    if !simple(source) || !simple(destination) || source == destination {
        return Err("Invalid private fault basename".into());
    }
    directory.verify()?;
    let original = Target::open(directory, source)
        .map_err(|e| format!("Private fault original-guard acquisition: {e}"))?
        .ok_or_else(|| "Missing private fault source".to_owned())?;
    if FileIdentity::of(reader).map_err(|e| e.to_string())? != original.identity {
        return Err("The private source is not the retained original reader".into());
    }
    absent(directory, destination)?;
    original.verify(directory, source)?;
    // SAFETY: the same verified existing-source synchronous DELETE handle,
    // same original parent and two bounded basenames satisfy THIS test-only
    // contract. The production CREATE_NEW-only rename contract is not used.
    unsafe { move_no_replace(&original.file, destination) }
        .map_err(|e| format!("Private fault original-handle NOREPLACE: {e}"))?;
    let moved = Moved {
        directory,
        reader,
        original,
        source: source.into(),
        destination: destination.into(),
    };
    moved
        .verify_moved()
        .map_err(|e| format!("Private fault moved-original readback: {e}"))?;
    absent(directory, source)?;
    Ok(moved)
}

/// # Safety
/// The original existing-source handle is synchronous, has DELETE access and
/// remains bound to the caller's held private/single-link/writable original.
/// Its parent is pinned; destination is a checked simple same-parent basename.
unsafe fn move_no_replace(file: &File, destination: &str) -> std::io::Result<()> {
    let name = destination
        .encode_utf16()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let bytes = std::mem::size_of::<FILE_RENAME_INFORMATION>() + name.len() * 2;
    let mut storage = vec![0u64; bytes.div_ceil(8)];
    let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
    let mut status = IO_STATUS_BLOCK::default();
    // SAFETY: aligned bounded header/name, no path-selected parent and only
    // class10 NOREPLACE. No Ex, overwrite, fallback or retry is permitted.
    let result = unsafe {
        (*info).Anonymous.ReplaceIfExists = false;
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
        return Err(std::io::Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosError(result) } as i32,
        ));
    }
    Ok(())
}

#[test]
fn the_existing_original_noreplace_handle_never_overwrites_an_appearing_destination() {
    let temp = tempfile::tempdir().unwrap();
    crate::platform::private_dir(temp.path()).unwrap();
    let directory = Directory::open(&crate::platform::canonical(temp.path()).unwrap()).unwrap();
    directory.write_new("source", b"original source").unwrap();
    let reader = directory.read_private("source").unwrap();
    let guard = Target::open(&directory, "source").unwrap().unwrap();
    assert_eq!(FileIdentity::of(&reader).unwrap(), guard.identity);
    absent(&directory, "appearing").unwrap();
    // Establish a race after the same-parent absence observation, while the
    // reader, original DELETE guard and every parent pin remain retained.
    directory
        .write_new("appearing", b"untouched arrival")
        .unwrap();
    // SAFETY: exact verified original existing source and fixed simple name.
    let error = unsafe { move_no_replace(&guard.file, "appearing") }.unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
    guard.verify(&directory, "source").unwrap();
    assert_eq!(FileIdentity::of(&reader).unwrap(), guard.identity);
    assert_eq!(
        std::fs::read(directory.path.join("source")).unwrap(),
        b"original source"
    );
    assert_eq!(
        std::fs::read(directory.path.join("appearing")).unwrap(),
        b"untouched arrival"
    );
    directory.verify().unwrap();
}
