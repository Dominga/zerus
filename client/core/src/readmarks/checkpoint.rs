//! Explicit durability barrier for the existing file. This is not admission
//! closure, a complete ReadDelivery drain, source discovery or role authority.
use super::ReadMarks;
use serde::ser::{SerializeSeq, Serializer};
use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    path::Path,
};
use zerus_desktop_migration::sha256;

pub const CHECKPOINT_BYTES: usize = 16 << 20;
pub const CHECKPOINT_MARKS: usize = 50_000;

#[derive(Debug, PartialEq, Eq)]
pub struct Checkpoint {
    pub sha256: String,
    pub bytes: usize,
    pub marks: usize,
}
#[derive(Clone, PartialEq, Eq)]
enum Source {
    Missing,
    Hash(String),
    Unavailable,
}
pub(super) struct State {
    initial_ok: bool,
    source: Source,
    pub dirty: bool,
}
impl State {
    pub fn loaded(bytes: &io::Result<Vec<u8>>, rows: Option<usize>) -> Self {
        let (initial_ok, source) = match bytes {
            Ok(bytes)
                if bytes.len() <= CHECKPOINT_BYTES
                    && rows.is_some_and(|n| n <= CHECKPOINT_MARKS) =>
            {
                (true, Source::Hash(sha256(bytes)))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => (true, Source::Missing),
            _ => (false, Source::Unavailable),
        };
        Self {
            initial_ok,
            source,
            dirty: false,
        }
    }
    pub fn saved(&mut self, bytes: &[u8]) {
        self.dirty = false;
        self.source = if bytes.len() <= CHECKPOINT_BYTES {
            Source::Hash(sha256(bytes))
        } else {
            Source::Unavailable
        };
    }
}
fn refused(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

struct Disk {
    file: File,
    hash: String,
    bytes: usize,
}
fn current(path: &Path) -> io::Result<Option<Disk>> {
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
    if !meta.is_file() || meta.len() > CHECKPOINT_BYTES as u64 {
        return Err(refused("pending read checkpoint file exceeds its bounds"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // SAFETY: geteuid has no preconditions. Never follow or sync a shared,
        // public or foreign leaf when proving this private source barrier.
        if meta.uid() != unsafe { libc::geteuid() } || meta.nlink() != 1 || meta.mode() & 0o077 != 0
        {
            return Err(refused("pending read checkpoint source is not private"));
        }
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take((CHECKPOINT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > CHECKPOINT_BYTES {
        return Err(refused("pending read checkpoint byte limit"));
    }
    Ok(Some(Disk {
        file,
        hash: sha256(&bytes),
        bytes: bytes.len(),
    }))
}
fn source(disk: &Option<Disk>) -> Source {
    disk.as_ref()
        .map(|d| Source::Hash(d.hash.clone()))
        .unwrap_or(Source::Missing)
}
struct Bounded {
    bytes: Vec<u8>,
    limit: usize,
}
impl Write for Bounded {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(refused("pending read checkpoint serialization limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn serialized(marks: &ReadMarks, limit: usize) -> io::Result<Vec<u8>> {
    let mut writer = Bounded {
        bytes: Vec::new(),
        limit,
    };
    let mut serializer = serde_json::Serializer::new(&mut writer);
    let mut list = serializer.serialize_seq(Some(marks.marks.len()))?;
    for mark in marks.marks.values() {
        list.serialize_element(mark)?;
    }
    list.end()?;
    Ok(writer.bytes)
}
fn sync_parent(path: &Path) -> io::Result<()> {
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
impl ReadMarks {
    /// Call off-thread, under the old owner's store mutex, AFTER closing and
    /// draining all of its writer admissions. No production caller exists yet.
    /// Unchanged bytes/unknown fields stay untouched. A failure grants nothing;
    /// a directory-sync failure can follow the atomic rename, so it is unknown.
    /// Windows is explicitly unqualified and refuses before checkpoint I/O.
    pub fn checkpoint(&mut self) -> io::Result<Checkpoint> {
        self.checkpoint_with(sync_parent)
    }
    fn checkpoint_with(
        &mut self,
        sync: impl FnOnce(&Path) -> io::Result<()>,
    ) -> io::Result<Checkpoint> {
        if !cfg!(unix) {
            return Err(io::ErrorKind::Unsupported.into());
        }
        if !self.checkpoint.initial_ok {
            return Err(refused(
                "pending read checkpoint source was not loaded reliably",
            ));
        }
        if self.marks.len() > CHECKPOINT_MARKS {
            return Err(refused("pending read checkpoint row limit"));
        }
        if self.checkpoint.source == Source::Unavailable {
            return Err(refused("pending read checkpoint source is unavailable"));
        }
        let mut disk = current(&self.path)?;
        if source(&disk) != self.checkpoint.source {
            return Err(refused("pending read checkpoint source changed"));
        }
        if self.checkpoint.dirty || disk.is_none() {
            let bytes = serialized(self, CHECKPOINT_BYTES)?;
            super::write_private_atomic(&self.path, &bytes, true)?;
            self.checkpoint.saved(&bytes);
            self.unsaved = false;
            disk = current(&self.path)?;
            if source(&disk) != self.checkpoint.source {
                return Err(refused(
                    "pending read checkpoint source changed after write",
                ));
            }
        }
        let disk = disk.ok_or_else(|| refused("pending read checkpoint missing file"))?;
        disk.file.sync_all()?;
        sync(&self.path)?;
        Ok(Checkpoint {
            sha256: disk.hash,
            bytes: disk.bytes,
            marks: self.marks.len(),
        })
    }
}

#[cfg(test)]
mod tests;
