//! A foreign Qt writer has no Settings lease. Allow all sharing; detect source
//! changes without blocking Qt's reads, in-place writes or atomic replacement.
use super::{
    bytes, check_limit, filename, info, owned, refused, verify, wide, Parents, READ_CONTROL,
};
use std::{
    fs::File,
    io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::GENERIC_READ,
    Storage::FileSystem::{
        CreateFileW, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE,
        FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    },
};
use zerus_desktop_migration::sha256;

#[derive(PartialEq, Eq)]
struct Stamp {
    volume: u32,
    index: (u32, u32),
    size: (u32, u32),
    written: (u32, u32),
}
impl Stamp {
    fn read(file: &File) -> io::Result<Self> {
        let value = info(file)?;
        Ok(Self {
            volume: value.dwVolumeSerialNumber,
            index: (value.nFileIndexHigh, value.nFileIndexLow),
            size: (value.nFileSizeHigh, value.nFileSizeLow),
            written: (
                value.ftLastWriteTime.dwHighDateTime,
                value.ftLastWriteTime.dwLowDateTime,
            ),
        })
    }
}
fn open(path: &Path, attributes_only: bool) -> io::Result<File> {
    // GENERIC_READ includes READ_CONTROL; the path probe reads attributes and
    // ACL only. Neither handle has write/delete/create/timestamp permission.
    let access = if attributes_only {
        FILE_READ_ATTRIBUTES | READ_CONTROL
    } else {
        GENERIC_READ
    };
    let raw = unsafe {
        CreateFileW(
            wide(path)?.as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    let file = File::from(owned(raw)?);
    verify(&file, false)?;
    Ok(file)
}

pub(crate) struct Source {
    pub(crate) file: File,
    _parents: Parents,
    path: PathBuf,
    stamp: Stamp,
    hash: String,
    bytes: usize,
}
impl Source {
    pub(crate) fn unchanged_bytes(&mut self, path: &Path, limit: usize) -> io::Result<Vec<u8>> {
        if self.path != path || Stamp::read(&self.file)? != self.stamp {
            return Err(refused("Qt source identity or metadata changed"));
        }
        let data = bytes(&mut self.file, limit)?;
        // Probe the exact pinned path without following a replaced leaf. Do not
        // adopt this new handle or parse data from a replacement source.
        let current = open(path, true)?;
        if Stamp::read(&self.file)? != self.stamp
            || Stamp::read(&current)? != self.stamp
            || data.len() != self.bytes
            || sha256(&data) != self.hash
        {
            return Err(refused("Qt source identity, metadata or bytes changed"));
        }
        Ok(data)
    }
}
fn current(path: &Path, limit: usize) -> io::Result<Option<Source>> {
    check_limit(limit)?;
    filename(path)?;
    let parents = match Parents::open(
        path.parent()
            .ok_or_else(|| refused("Qt source parent missing"))?,
        false,
    ) {
        Ok(value) => value,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut file = match open(path, false) {
        Ok(value) => value,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let stamp = Stamp::read(&file)?;
    let data = bytes(&mut file, limit)?;
    let mut source = Source {
        file,
        _parents: parents,
        path: path.into(),
        stamp,
        hash: sha256(&data),
        bytes: data.len(),
    };
    source.unchanged_bytes(path, limit)?;
    Ok(Some(source))
}
pub(crate) fn capture_with_deadline(
    path: &Path,
    limit: usize,
    deadline: Instant,
) -> io::Result<Option<Source>> {
    for attempt in 0..=20 {
        if Instant::now() >= deadline {
            return Err(io::ErrorKind::TimedOut.into());
        }
        match current(path, limit) {
            Err(error) if error.raw_os_error() == Some(32) && attempt < 20 => {
                std::thread::sleep(
                    deadline
                        .saturating_duration_since(Instant::now())
                        .min(Duration::from_millis(10)),
                );
            }
            result => return result,
        }
    }
    unreachable!()
}
