//! Immutable native image observations. No process lookup, role, consent,
//! caller authentication, descriptor execution or user-selected reference.
use super::{sealed, snapshot, stamp, Stamp, MAX_IMAGE_BYTES};
use std::{
    fs::File,
    io::{self, Seek},
    os::unix::fs::FileExt,
    sync::Arc,
    time::Instant,
};

fn native(file: &File) -> io::Result<()> {
    let mut magic = [0; 4];
    file.read_exact_at(&mut magic, 0)?;
    if magic != *b"\x7fELF" {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    Ok(())
}

struct Frozen {
    file: File,
    original: Stamp,
}
impl Frozen {
    fn new(file: File) -> io::Result<Self> {
        let original = stamp(&file)?;
        if original.bytes == 0 || original.bytes > MAX_IMAGE_BYTES || !sealed(&file) {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        native(&file)?;
        Ok(Self { file, original })
    }
    fn current(&self) -> bool {
        sealed(&self.file) && stamp(&self.file).ok().as_ref() == Some(&self.original)
    }
    fn same(&self, actual: &File) -> bool {
        self.current() && sealed(actual) && stamp(actual).ok().as_ref() == Some(&self.original)
    }
}

/// One immutable startup reference of the ACTUAL running service image.
/// Creation must run on the service's owned setup worker before admission.
/// No path, replacement version, hash or caller-supplied reference is accepted.
pub struct ServiceImage {
    _source: File,
    _source_version: Stamp,
    frozen: Frozen,
}
impl ServiceImage {
    pub fn capture() -> io::Result<Self> {
        let source = File::open("/proc/self/exe")?;
        Self::from_original(source)
    }
    fn from_original(mut source: File) -> io::Result<Self> {
        let original = stamp(&source)?;
        native(&source)?;
        source.rewind()?;
        let frozen = Frozen::new(snapshot(&mut source, &original)?)?;
        Ok(Self {
            _source: source,
            _source_version: original,
            frozen,
        })
    }
    /// Observe only a kernel-opened original peer exe. The server separately
    /// brackets acquisition with kernel peer/user/birth checks and retains its
    /// actual owner loan. This supplies only the image-equivalence predicate.
    pub fn inspect_peer(self: &Arc<Self>, original: File, until: Instant) -> io::Result<PeerImage> {
        let peer = Frozen::new(original)?;
        if !self.frozen.current() || peer.original.bytes != self.frozen.original.bytes {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        let mut left = [0; 64 << 10];
        let mut right = [0; 64 << 10];
        let mut offset = 0;
        while offset < peer.original.bytes {
            if Instant::now() >= until {
                return Err(io::ErrorKind::TimedOut.into());
            }
            let n = (peer.original.bytes - offset).min(left.len() as u64) as usize;
            self.frozen.file.read_exact_at(&mut left[..n], offset)?;
            peer.file.read_exact_at(&mut right[..n], offset)?;
            if left[..n] != right[..n] {
                return Err(io::ErrorKind::PermissionDenied.into());
            }
            offset += n as u64;
        }
        if Instant::now() >= until || !self.frozen.current() || !peer.current() {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        Ok(PeerImage {
            reference: self.clone(),
            peer,
        })
    }
}

/// Retained original kernel object evidence, never a renewable permission.
/// Each server freshness check must open the peer's ACTUAL exe again and
/// bracket this comparison with its unchanged process/birth/parent/user checks.
pub struct PeerImage {
    reference: Arc<ServiceImage>,
    peer: Frozen,
}
impl PeerImage {
    pub fn current(&self, actual: &File) -> bool {
        self.reference.frozen.current() && self.peer.same(actual)
    }
}

#[cfg(test)]
mod tests;
