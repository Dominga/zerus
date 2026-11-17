//! Bounded no-follow read of the native-selected connector configuration.
#[cfg(unix)]
use crate::fsguard;
use crate::{private_state, EndpointError};
use serde_json::{json, Value};
use std::{fs::File, io::Read, path::Path};
type Result<T> = std::result::Result<T, EndpointError>;
fn refused() -> EndpointError {
    EndpointError("Workspace configuration is unavailable or changed; reload before continuing")
}
fn read_bounded(file: File) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    file.take(private_state::MAX_STATE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| refused())?;
    if bytes.len() > private_state::MAX_STATE_BYTES {
        return Err(refused());
    }
    Ok(bytes)
}

#[cfg(unix)]
fn directory(path: &Path) -> Result<File> {
    use rustix::fs::{Mode, OFlags};
    use std::os::unix::fs::MetadataExt;
    let file: File = rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| refused())?
    .into();
    let metadata = file.metadata().map_err(|_| refused())?;
    if metadata.uid() != fsguard::euid() || metadata.mode() & 0o022 != 0 {
        return Err(refused());
    }
    Ok(file)
}
#[cfg(unix)]
fn read_at(directory: &File, name: &std::ffi::OsStr) -> Result<Option<Vec<u8>>> {
    use rustix::fs::{Mode, OFlags};
    use std::os::unix::fs::MetadataExt;
    let opened = rustix::fs::openat(
        directory,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    );
    let file: File = match opened {
        Ok(file) => file.into(),
        Err(error) if error == rustix::io::Errno::NOENT => return Ok(None),
        Err(_) => return Err(refused()),
    };
    let metadata = file.metadata().map_err(|_| refused())?;
    if !metadata.is_file()
        || metadata.uid() != fsguard::euid()
        || metadata.mode() & 0o022 != 0
        || metadata.nlink() != 1
    {
        return Err(refused());
    }
    read_bounded(file).map(Some)
}
pub fn read(path: &Path) -> Result<(Value, String)> {
    let parent = path.parent().ok_or_else(refused)?;
    if !parent.exists() {
        return Ok((json!({}), String::new()));
    }
    #[cfg(unix)]
    let bytes = read_at(&directory(parent)?, path.file_name().ok_or_else(refused)?)?;
    #[cfg(windows)]
    let bytes = match zerus_windows_private::open_file(path, false, false) {
        Ok(file) => Some(read_bounded(file)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err(refused()),
    };
    let revision = private_state::Store::revision(bytes.as_deref());
    let value = match bytes {
        Some(bytes) => serde_json::from_slice::<Value>(&bytes).map_err(|_| refused())?,
        None => json!({}),
    };
    if !value.is_object() {
        return Err(refused());
    }
    Ok((value, revision))
}
