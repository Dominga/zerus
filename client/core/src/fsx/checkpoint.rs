//! Shared explicit checkpoint I/O. No admissions, stores, discovery or authority.
use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    path::Path,
};
use zerus_desktop_migration::sha256;
fn refused(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
pub(crate) struct Disk {
    pub file: File,
    pub hash: String,
    pub bytes: usize,
}
#[derive(Clone, Copy)]
enum SourceAccess {
    Private,
    #[cfg(unix)]
    OwnedReadOnly,
}
impl SourceAccess {
    #[cfg(unix)]
    fn permits(self, uid: u32, links: u64, mode: u32) -> bool {
        let forbidden = match self {
            Self::Private => 0o077,
            Self::OwnedReadOnly => 0o022,
        };
        // SAFETY: geteuid has no preconditions. Ownership always comes from
        // metadata on the opened no-follow file, never a caller's claim.
        uid == unsafe { libc::geteuid() } && links == 1 && mode & forbidden == 0
    }
}

pub(crate) fn current(path: &Path, limit: usize) -> io::Result<Option<Disk>> {
    current_with_access(path, limit, SourceAccess::Private)
}

/// Owned, read-only legacy import source. Sharing read access cannot change its
/// bytes; sharing write access is refused. Never use this for a Zerus writer.
#[cfg(unix)]
pub(crate) fn read_only_source(path: &Path, limit: usize) -> io::Result<Option<Disk>> {
    current_with_access(path, limit, SourceAccess::OwnedReadOnly)
}

fn current_with_access(
    path: &Path,
    limit: usize,
    access: SourceAccess,
) -> io::Result<Option<Disk>> {
    #[cfg(not(unix))]
    let _ = access;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() > limit as u64 {
        return Err(refused("checkpoint file exceeds its bounds"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if !access.permits(meta.uid(), meta.nlink(), meta.mode()) {
            return Err(refused(
                "checkpoint source ownership, links or permissions refused",
            ));
        }
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(refused("checkpoint byte limit"));
    }
    Ok(Some(Disk {
        file,
        hash: sha256(&bytes),
        bytes: bytes.len(),
    }))
}
pub(crate) struct Bounded {
    pub bytes: Vec<u8>,
    pub limit: usize,
}
impl Write for Bounded {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(refused("checkpoint serialization limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
pub(crate) fn sync_parent(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        File::open(
            path.parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new(".")),
        )?
        .sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Err(io::ErrorKind::Unsupported.into())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn foreign_owner_is_refused_by_both_source_policies() {
        // Test kernel-metadata inputs independently of chown privileges.
        let owner = unsafe { libc::geteuid() };
        let foreign = owner.wrapping_add(1);
        assert!(SourceAccess::OwnedReadOnly.permits(owner, 1, 0o644));
        assert!(!SourceAccess::OwnedReadOnly.permits(foreign, 1, 0o644));
        assert!(!SourceAccess::Private.permits(foreign, 1, 0o600));
        assert!(!SourceAccess::Private.permits(owner, 1, 0o644));
    }
}
