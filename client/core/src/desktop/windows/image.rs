//! Read/execute file custody and loaded-image binding. A process pathname is
//! only a discovery hint; the native mapping check supplies the identity proof.
use std::{
    fs::{File, OpenOptions},
    io, mem,
    os::windows::{
        ffi::OsStringExt,
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    },
    path::{Path, PathBuf},
};
use windows_sys::{
    Wdk::System::Threading::{NtQueryInformationProcess, ProcessImageFileMapping},
    Win32::{Foundation::*, Storage::FileSystem::*, System::Threading::*},
};

pub type FileId = (u64, u128);

pub fn file_id(file: &impl AsRawHandle) -> io::Result<FileId> {
    let mut info: FILE_ID_INFO = unsafe { mem::zeroed() };
    // SAFETY: a held file handle and a writable block of the specified size.
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileIdInfo,
            (&mut info as *mut FILE_ID_INFO).cast(),
            mem::size_of_val(&info) as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok((
        info.VolumeSerialNumber,
        u128::from_le_bytes(info.FileId.Identifier),
    ))
}
fn require(ok: bool, message: &'static str) -> io::Result<()> {
    if ok {
        Ok(())
    } else {
        Err(io::Error::other(message))
    }
}
fn open_file(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        // GENERIC_EXECUTE supplies FILE_EXECUTE and SYNCHRONIZE, which the
        // ProcessImageFileMapping query requires on the actual held handle.
        .access_mode(GENERIC_READ | GENERIC_EXECUTE)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let metadata = file.metadata()?;
    require(
        metadata.is_file() && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0,
        "image is not a plain executable file",
    )?;
    Ok(file)
}
fn pin_folder(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let metadata = file.metadata()?;
    require(
        metadata.is_dir() && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0,
        "image ancestor is not a plain directory",
    )?;
    Ok(file)
}
fn final_path(file: &File) -> io::Result<PathBuf> {
    let mut buffer = vec![0u16; 32_768];
    // SAFETY: a live file and a writable UTF-16 buffer.
    let length = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle(),
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
        )
    } as usize;
    require(
        length > 0 && length < buffer.len(),
        "image final path unavailable",
    )?;
    let prefix: Vec<u16> = r"\\?\".encode_utf16().collect();
    let unc: Vec<u16> = r"UNC\".encode_utf16().collect();
    let local = buffer[..length]
        .strip_prefix(prefix.as_slice())
        .filter(|rest| !rest.starts_with(&unc))
        .ok_or_else(|| io::Error::other("image is not on a local volume"))?;
    let path = PathBuf::from(std::ffi::OsString::from_wide(local));
    require(path.is_absolute(), "image final path is not absolute")?;
    Ok(path)
}
fn local_path(path: &Path) -> io::Result<PathBuf> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::WindowsProgramming::{
        DRIVE_FIXED, DRIVE_RAMDISK, DRIVE_REMOVABLE,
    };
    let all = path.as_os_str().encode_wide().collect::<Vec<_>>();
    let verbatim: Vec<u16> = r"\\?\".encode_utf16().collect();
    let units = all.strip_prefix(verbatim.as_slice()).unwrap_or(&all);
    require(
        units.len() >= 3
            && units[0] <= 127
            && (units[0] as u8).is_ascii_alphabetic()
            && units[1] == b':' as u16
            && units[2] == b'\\' as u16,
        "executable path must use an absolute local drive",
    )?;
    let root = [units[0], units[1], units[2], 0];
    // SAFETY: one NUL-terminated drive root. Remote/unknown drives are refused
    // before any directory or executable open can follow a network redirect.
    require(
        matches!(
            unsafe { GetDriveTypeW(root.as_ptr()) },
            DRIVE_FIXED | DRIVE_REMOVABLE | DRIVE_RAMDISK
        ),
        "executable volume is remote or unavailable",
    )?;
    let path = PathBuf::from(std::ffi::OsString::from_wide(units));
    require(
        !path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir)),
        "executable path has parent traversal",
    )?;
    Ok(path)
}

/// Generic executable support, including native test images. Naming/catalog
/// policy belongs to each caller. Neither this file nor its normalized ancestor
/// chain can be replaced while these handles are retained.
#[derive(Debug)]
pub struct PinnedImage {
    file: File,
    path: PathBuf,
    id: FileId,
    bytes: u64,
    _folders: Vec<File>,
}
impl PinnedImage {
    pub fn open(path: &Path) -> io::Result<Self> {
        require(path.is_absolute(), "image selection must be absolute")?;
        let local = local_path(path)?;
        let path = local.as_path();
        // Pin the input chain before opening the leaf. Normalizing only after
        // an open could already have followed an ancestor into a remote share.
        let mut ancestors = path.ancestors().skip(1).collect::<Vec<_>>();
        require(ancestors.len() <= 128, "executable ancestor limit exceeded")?;
        ancestors.reverse();
        let mut folders = ancestors
            .into_iter()
            .map(pin_folder)
            .collect::<io::Result<Vec<_>>>()?;
        let file = open_file(path)?;
        let id = file_id(&file)?;
        let bytes = file.metadata()?.len();
        let path = final_path(&file)?;
        let mut ancestors = path.ancestors().skip(1).collect::<Vec<_>>();
        require(ancestors.len() <= 128, "executable ancestor limit exceeded")?;
        ancestors.reverse();
        folders.extend(
            ancestors
                .into_iter()
                .map(pin_folder)
                .collect::<io::Result<Vec<_>>>()?,
        );
        let result = Self {
            file,
            path,
            id,
            bytes,
            _folders: folders,
        };
        result.revalidate()?;
        Ok(result)
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn id(&self) -> FileId {
        self.id
    }
    /// The pinned file's length, which the deny-write handle keeps fixed.
    pub fn bytes(&self) -> u64 {
        self.bytes
    }
    pub fn try_clone(&self) -> io::Result<Self> {
        Ok(Self {
            file: self.file.try_clone()?,
            path: self.path.clone(),
            id: self.id,
            bytes: self.bytes,
            _folders: self
                ._folders
                .iter()
                .map(File::try_clone)
                .collect::<io::Result<Vec<_>>>()?,
        })
    }
    /// The SHA-256 of the pinned bytes, read through this held handle, which
    /// denies writing and deletion: no other path open can substitute. Refuses
    /// a file larger than `limit` and stops at `until`.
    pub fn sha256(&self, limit: u64, until: std::time::Instant) -> io::Result<String> {
        use sha2::{Digest, Sha256};
        use std::os::windows::fs::FileExt;
        require(
            self.bytes <= limit,
            "pinned executable exceeds the read limit",
        )?;
        let mut hash = Sha256::new();
        let mut buffer = vec![0u8; 64 * 1024];
        let mut offset = 0u64;
        while offset < self.bytes {
            require(
                std::time::Instant::now() < until,
                "pinned executable read deadline",
            )?;
            let want = buffer.len().min((self.bytes - offset) as usize);
            let read = self.file.seek_read(&mut buffer[..want], offset)?;
            require(read > 0, "pinned executable ended early")?;
            hash.update(&buffer[..read]);
            offset += read as u64;
        }
        self.revalidate()?;
        Ok(format!("{:x}", hash.finalize()))
    }
    pub fn revalidate(&self) -> io::Result<()> {
        require(
            file_id(&self.file)? == self.id && self.file.metadata()?.len() == self.bytes,
            "pinned executable changed",
        )?;
        require(
            final_path(&self.file)? == self.path && file_id(&open_file(&self.path)?)? == self.id,
            "pinned executable path changed",
        )
    }
    /// The kernel compares this held file with the original image section of
    /// this process. The HANDLE is an INPUT, not an output handle. No filename,
    /// byte/header comparison or reopened process can substitute for this proof.
    /// Unsupported/denied queries fail closed; there is no pathname fallback.
    pub fn matches_process(&self, process: &impl AsRawHandle) -> io::Result<()> {
        let mut file = self.file.as_raw_handle();
        // SAFETY: the original live process and read/execute file handle remain
        // held through the native call. The input buffer is exactly one HANDLE.
        let status = unsafe {
            NtQueryInformationProcess(
                process.as_raw_handle(),
                ProcessImageFileMapping,
                (&mut file as *mut HANDLE).cast(),
                mem::size_of::<HANDLE>() as u32,
                std::ptr::null_mut(),
            )
        };
        require(
            status == 0,
            "held file is not proven to be the loaded process image",
        )
    }
    pub fn for_process(process: &impl AsRawHandle) -> io::Result<Self> {
        let mut path = vec![0u16; 32_768];
        let mut length = path.len() as u32;
        // SAFETY: held process handle and a bounded writable filename buffer.
        if unsafe {
            QueryFullProcessImageNameW(
                process.as_raw_handle(),
                PROCESS_NAME_WIN32,
                path.as_mut_ptr(),
                &mut length,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        require(
            length > 0 && (length as usize) < path.len(),
            "process image path unavailable",
        )?;
        let hint = PathBuf::from(std::ffi::OsString::from_wide(&path[..length as usize]));
        let image = Self::open(&hint)?;
        image.matches_process(process)?;
        Ok(image)
    }
}

#[cfg(test)]
mod tests;
