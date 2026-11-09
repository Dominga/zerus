//! Bounded native writer receipts for reference metadata. Cache write hooks
//! must not open SQLite (its read-only fallback can also block on a FIFO).
//! Ordinary catalog reconciliation absorbs these per-ID updates; cleanup only
//! verifies and reads them, never rebuilds or writes reference coverage.
use super::super::repositories::directory::Directory;
use super::*;
const DIRECTORY: &str = "archive-reference-writes-v1";
const CAP: usize = META + 2048;
#[cfg(windows)]
mod windows;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Update {
    version: u8,
    stamp: Version,
    metadata: Metadata,
    digest: String,
}
fn key(id: &str) -> Result<String> {
    uuid::Uuid::parse_str(id).map_err(fail)?;
    Ok(format!("{:x}.json", Sha256::digest(id.as_bytes())))
}
fn directory(create: bool) -> Result<Directory> {
    let root = Directory::open(&fs::canonicalize(root()).map_err(fail)?)?;
    let path = root.path.join(DIRECTORY);
    if create {
        #[cfg(unix)]
        let result = {
            let name = std::ffi::CString::new(DIRECTORY).map_err(fail)?;
            let rc = unsafe { libc::mkdirat(root.descriptor(), name.as_ptr(), 0o700) };
            if rc == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        };
        #[cfg(windows)]
        let result = fs::create_dir(&path);
        if let Err(error) = result {
            if error.kind() != std::io::ErrorKind::AlreadyExists {
                return Err(fail(error));
            }
        }
    }
    let directory = Directory::open(&path)?;
    root.verify()?;
    if !directory.private()? {
        return Err(INVALID.into());
    }
    Ok(directory)
}
pub(super) fn write(metadata: &Metadata, stamp: &Version) -> Result<()> {
    let raw = serde_json::to_string(metadata).map_err(fail)?;
    if raw.len() > META {
        return Err(INVALID.into());
    }
    let hash = digest(&serde_json::to_string(stamp).map_err(fail)?, &raw);
    let bytes = serde_json::to_vec(&Update {
        version: 1,
        metadata: metadata.clone(),
        stamp: stamp.clone(),
        digest: hash,
    })
    .map_err(fail)?;
    if bytes.len() > CAP {
        return Err(INVALID.into());
    }
    let directory = directory(true)?;
    let name = key(&metadata.id)?;
    let staging = format!(".{}.{}", name, uuid::Uuid::new_v4());
    directory.verify()?;
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::fd::FromRawFd;
        let from = std::ffi::CString::new(staging).map_err(fail)?;
        let to = std::ffi::CString::new(name).map_err(fail)?;
        let dir = directory.descriptor();
        let fd = unsafe {
            libc::openat(
                dir,
                from.as_ptr(),
                libc::O_WRONLY
                    | libc::O_CREAT
                    | libc::O_EXCL
                    | libc::O_NOFOLLOW
                    | libc::O_CLOEXEC
                    | libc::O_NONBLOCK,
                0o600,
            )
        };
        if fd < 0 {
            return Err(fail(std::io::Error::last_os_error()));
        }
        let mut file = unsafe { fs::File::from_raw_fd(fd) };
        let result = file.write_all(&bytes).and_then(|()| file.sync_all());
        drop(file);
        let rc = if result.is_ok() {
            unsafe { libc::renameat(dir, from.as_ptr(), dir, to.as_ptr()) }
        } else {
            -1
        };
        if rc != 0 {
            let error = std::io::Error::last_os_error();
            unsafe { libc::unlinkat(dir, from.as_ptr(), 0) };
            result.map_err(fail)?;
            return Err(fail(error));
        }
        if unsafe { libc::fsync(dir) } != 0 {
            return Err(fail(std::io::Error::last_os_error()));
        }
    }
    #[cfg(windows)]
    {
        use std::io::Write;
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::{
            Foundation::GENERIC_WRITE,
            Storage::FileSystem::{DELETE, SYNCHRONIZE},
        };
        let from = directory.path.join(staging);
        let mut file = crate::platform::refuse_link(
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .access_mode(GENERIC_WRITE | DELETE | SYNCHRONIZE)
                .share_mode(0),
        )
        .open(&from)
        .map_err(fail)?;
        let result = file
            .write_all(&bytes)
            .and_then(|()| file.sync_all())
            .and_then(|()| {
                // SAFETY: this exclusive CREATE_NEW file was opened synchronously
                // above, with DELETE access, and stays live through publication.
                unsafe { windows::rename_same_parent(&file, &name) }
            });
        drop(file);
        if let Err(error) = result {
            let _ = fs::remove_file(from);
            return Err(fail(error));
        }
    }
    directory.verify()
}
pub(super) fn read(id: &str) -> Result<Option<(Version, Metadata, usize)>> {
    let directory = match directory(false) {
        Ok(dir) => dir,
        Err(_) => return Ok(None),
    };
    let path = directory.path.join(key(id)?);
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(fail(e)),
        Ok(_) => (),
    }
    let file = open(&path)?;
    let before = version(&file)?;
    let mut bytes = Vec::new();
    (&file)
        .take(CAP as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(fail)?;
    if bytes.len() > CAP || version(&file)? != before || file_version(&path)? != before {
        return Err(INVALID.into());
    }
    let update: Update = serde_json::from_slice(&bytes).map_err(fail)?;
    let raw = serde_json::to_string(&update.metadata).map_err(fail)?;
    let stamp = serde_json::to_string(&update.stamp).map_err(fail)?;
    if update.version != 1
        || update.metadata.version != 1
        || update.metadata.id != id
        || update.metadata.name.is_empty()
        || raw.len() > META
        || stamp.len() > 1024
        || digest(&stamp, &raw) != update.digest
    {
        return Err(INVALID.into());
    }
    directory.verify()?;
    Ok(Some((update.stamp, update.metadata, bytes.len())))
}
pub(super) fn forget(id: &str) -> Result<()> {
    let directory = directory(false)?;
    let name = key(id)?;
    directory.verify()?;
    #[cfg(unix)]
    {
        let name = std::ffi::CString::new(name).map_err(fail)?;
        let rc = unsafe { libc::unlinkat(directory.descriptor(), name.as_ptr(), 0) };
        if rc != 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::NotFound {
                return Err(fail(error));
            }
        }
    }
    #[cfg(windows)]
    if let Err(error) = fs::remove_file(directory.path.join(name)) {
        if error.kind() != std::io::ErrorKind::NotFound {
            return Err(fail(error));
        }
    }
    directory.verify()
}

#[cfg(all(test, windows))]
pub(super) fn test_native_publication(root: &Path) {
    use std::{io::Write, os::windows::fs::OpenOptionsExt};
    use windows_sys::Win32::{
        Foundation::{ERROR_ACCESS_DENIED, ERROR_SHARING_VIOLATION, GENERIC_WRITE},
        Storage::FileSystem::{DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, SYNCHRONIZE},
    };
    let path = root.join("native-publication");
    fs::create_dir(&path).unwrap();
    let directory = Directory::open(&path).unwrap();
    let name = key("11111111-1111-4111-8111-111111111111").unwrap();
    let target = path.join(&name);
    let stage = path.join(".original-stage");
    assert_ne!(std::env::current_dir().unwrap(), path);
    let mut file = crate::platform::refuse_link(
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .access_mode(GENERIC_WRITE | DELETE | SYNCHRONIZE)
            .share_mode(0),
    )
    .open(&stage)
    .unwrap();
    file.write_all(b"first receipt").unwrap();
    file.sync_all().unwrap();
    let wrong_case = format!("{}.json", "A".repeat(64));
    let escaped = format!("{}\\.json", "a".repeat(63));
    for invalid in [
        "",
        "..",
        "/absolute",
        "relative\\path",
        "stream:name",
        "a.json",
        &wrong_case,
        &escaped,
    ] {
        assert_eq!(
            unsafe { windows::rename_same_parent(&file, invalid) }
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::InvalidInput
        );
        assert!(stage.exists());
        assert!(!target.exists());
    }
    unsafe { windows::rename_same_parent(&file, &name) }.unwrap();
    directory.verify().unwrap();
    assert!(!stage.exists());
    drop(file);
    assert_eq!(fs::read(&target).unwrap(), b"first receipt");
    assert!(
        !std::env::current_dir().unwrap().join(&name).exists(),
        "the native basename must not resolve through cwd"
    );
    assert!(
        fs::rename(&path, path.with_file_name("replaced-parent")).is_err(),
        "publication must retain original directory custody"
    );
    let reader = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(&target)
        .unwrap();
    let mut next = crate::platform::refuse_link(
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .access_mode(GENERIC_WRITE | DELETE | SYNCHRONIZE)
            .share_mode(0),
    )
    .open(&stage)
    .unwrap();
    next.write_all(b"replacement receipt").unwrap();
    next.sync_all().unwrap();
    let error = unsafe { windows::rename_same_parent(&next, &name) }.unwrap_err();
    // Native replacement of an open target can report access denied instead
    // of a sharing violation. Both must leave its bytes and pins unchanged.
    assert!(
        matches!(
            error.raw_os_error().map(|code| code as u32),
            Some(ERROR_ACCESS_DENIED | ERROR_SHARING_VIOLATION)
        ),
        "{error}"
    );
    assert_eq!(fs::read(&target).unwrap(), b"first receipt");
    directory.verify().unwrap();
    drop(reader);
    unsafe { windows::rename_same_parent(&next, &name) }.unwrap();
    drop(next);
    assert_eq!(fs::read(&target).unwrap(), b"replacement receipt");
    directory.verify().unwrap();
}
