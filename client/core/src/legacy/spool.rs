//! Bounded ephemeral staging, using storage already secured by the host.
//! No paths, source activation, migration evidence or effect authority here.
use super::listing::{self, Limits, Listing};
use sha2::{Digest, Sha256};
use std::{
    fmt,
    io::{self, Read, Seek, SeekFrom, Write},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::Instant,
};
use zerus_watch_protocol::{CatalogItem, MAX_FRAME_BYTES};

pub const SPOOL_BYTES: usize = 32 << 20;
pub const SPOOL_COUNT: usize = 2;
const MAGIC: &[u8; 8] = b"HGS-LS1\n";
const HEADER: usize = 4 + 32;
const PAGE_LIMIT: usize = MAX_FRAME_BYTES;

/// A new, empty, ephemeral private file supplied through the host's existing
/// private directory/file checks. Its owner must remove it on drop (or supply
/// an already unlinked file). This module neither opens nor deletes paths.
pub trait Storage: Read + Write + Seek + Send {}
impl<T: Read + Write + Seek + Send> Storage for T {}

/// Create one pool per source configuration, shared by all its aliases.
/// Reserve before file creation or child spawn. At most two outstanding
/// reservations each charge the full 32 MiB, including while a caller holds a
/// completed spool or a blocked decoder still owns its file.
#[derive(Clone, Default)]
pub struct Pool(Arc<AtomicUsize>);
pub struct Reservation(Arc<AtomicUsize>);
impl Pool {
    pub fn reserve(&self) -> Option<Reservation> {
        self.0
            .try_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < SPOOL_COUNT).then_some(n + 1)
            })
            .ok()
            .map(|_| Reservation(self.0.clone()))
    }
    pub fn active(&self) -> usize {
        self.0.load(Ordering::Acquire)
    }
    pub fn reserved_bytes(&self) -> usize {
        self.active() * SPOOL_BYTES
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Decode(listing::Error),
    TooLarge,
    Storage,
    Corrupt,
    Cancelled,
    TimedOut,
    Sink,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Decode(_) => "legacy spool decode refused",
            Self::TooLarge => "legacy spool exceeds its bound",
            Self::Storage => "legacy spool storage failed",
            Self::Corrupt => "legacy spool verification failed",
            Self::Cancelled => "legacy spool cancelled",
            Self::TimedOut => "legacy spool deadline expired",
            Self::Sink => "legacy spool replay sink failed",
        })
    }
}
impl std::error::Error for Error {}

/// An opaque, one-use spool. Parsing success alone is not child completion:
/// when used with a child, return this from run::run_decoded and publish only
/// after that runner succeeds. Replay callbacks remain provisional until
/// replay itself succeeds; a failed replay must abandon the indexed staging.
pub struct Staged {
    storage: Box<dyn Storage>,
    listing: Listing,
    observed_at: Instant,
    bytes: usize,
    digest: [u8; 32],
    _reservation: Reservation,
}
fn check(cancel: &AtomicBool, until: Instant) -> Result<(), Error> {
    if cancel.load(Ordering::Relaxed) {
        Err(Error::Cancelled)
    } else if Instant::now() >= until {
        Err(Error::TimedOut)
    } else {
        Ok(())
    }
}
impl Reservation {
    /// Incremental decoding writes each bounded page once. Input is capped at
    /// 32 MiB even if the caller's listing limit is larger; encoded spool bytes
    /// have a separate 32 MiB bound. No whole-listing or page-offset map grows.
    /// Capture observed_at before spawning the read child; decoding and replay
    /// must not renew the age of data already waiting in its pipe.
    /// Run on a background worker; arbitrary blocked file I/O cannot be
    /// interrupted by a flag, and retains this reservation until it ends.
    pub fn stage(
        self,
        mut storage: Box<dyn Storage>,
        reader: impl Read,
        limits: Limits,
        observed_at: Instant,
        until: Instant,
        cancel: &AtomicBool,
    ) -> Result<Staged, Error> {
        check(cancel, until)?;
        if storage.seek(SeekFrom::End(0)).map_err(|_| Error::Storage)? != 0 {
            // Never truncate or overwrite a host-supplied preexisting object.
            return Err(Error::Storage);
        }
        storage.write_all(MAGIC).map_err(|_| Error::Storage)?;
        let mut bytes = MAGIC.len();
        let mut digest = Sha256::new();
        digest.update(MAGIC);
        let mut sink_error = None;
        let limits = Limits {
            bytes: limits.bytes.min(SPOOL_BYTES),
            ..limits
        };
        let listing = listing::decode(reader, limits, until, cancel, |page| {
            let mut write = || -> Result<(), Error> {
                check(cancel, until)?;
                let body = serde_json::to_vec(page).map_err(|_| Error::Storage)?;
                if body.len() > PAGE_LIMIT || bytes + HEADER + body.len() > SPOOL_BYTES {
                    return Err(Error::TooLarge);
                }
                let length = (body.len() as u32).to_le_bytes();
                let hash: [u8; 32] = Sha256::digest(&body).into();
                for part in [&length[..], &hash[..], &body[..]] {
                    storage.write_all(part).map_err(|_| Error::Storage)?;
                    digest.update(part);
                }
                bytes += HEADER + body.len();
                Ok(())
            };
            write().map_err(|error| {
                sink_error = Some(error);
                io::Error::other("legacy spool write refused")
            })
        })
        .map_err(|error| sink_error.unwrap_or(Error::Decode(error)))?;
        storage.flush().map_err(|_| Error::Storage)?;
        check(cancel, until)?;
        Ok(Staged {
            storage,
            listing,
            observed_at,
            bytes,
            digest: digest.finalize().into(),
            _reservation: self,
        })
    }
}
impl Staged {
    pub fn listing(&self) -> &Listing {
        &self.listing
    }
    /// The caller's pre-spawn observation time; replay never renews this age.
    pub fn observed_at(&self) -> Instant {
        self.observed_at
    }
    pub fn bytes(&self) -> usize {
        self.bytes
    }
    /// Read one page at a time, retaining the reservation through every sink
    /// callback. Consume self so a failed/partial replay cannot be retried.
    pub fn replay(
        mut self,
        until: Instant,
        cancel: &AtomicBool,
        mut page: impl FnMut(&[CatalogItem]) -> io::Result<()>,
    ) -> Result<Listing, Error> {
        check(cancel, until)?;
        self.storage
            .seek(SeekFrom::Start(0))
            .map_err(|_| Error::Storage)?;
        let mut magic = [0; MAGIC.len()];
        self.storage
            .read_exact(&mut magic)
            .map_err(|_| Error::Storage)?;
        if &magic != MAGIC {
            return Err(Error::Corrupt);
        }
        let mut digest = Sha256::new();
        digest.update(magic);
        let mut bytes = MAGIC.len();
        let mut rows = 0usize;
        for index in 0..self.listing.pages {
            check(cancel, until)?;
            let mut length = [0; 4];
            let mut hash = [0; 32];
            self.storage
                .read_exact(&mut length)
                .map_err(|_| Error::Storage)?;
            self.storage
                .read_exact(&mut hash)
                .map_err(|_| Error::Storage)?;
            let size = u32::from_le_bytes(length) as usize;
            if size > PAGE_LIMIT || bytes + HEADER + size > self.bytes {
                return Err(Error::Corrupt);
            }
            let mut body = vec![0; size];
            self.storage
                .read_exact(&mut body)
                .map_err(|_| Error::Storage)?;
            if Sha256::digest(&body)[..] != hash {
                return Err(Error::Corrupt);
            }
            for part in [&length[..], &hash[..], &body[..]] {
                digest.update(part);
            }
            bytes += HEADER + size;
            let items: Vec<CatalogItem> =
                serde_json::from_slice(&body).map_err(|_| Error::Corrupt)?;
            if items.len() > listing::PAGE_ROWS
                || (items.is_empty()
                    && (index != 0 || self.listing.pages != 1 || self.listing.rows != 0))
            {
                return Err(Error::Corrupt);
            }
            rows += items.len();
            if rows > self.listing.rows {
                return Err(Error::Corrupt);
            }
            check(cancel, until)?;
            page(&items).map_err(|_| Error::Sink)?;
        }
        let mut trailing = [0; 1];
        if bytes != self.bytes
            || rows != self.listing.rows
            || self
                .storage
                .read(&mut trailing)
                .map_err(|_| Error::Storage)?
                != 0
            || digest.finalize()[..] != self.digest
        {
            return Err(Error::Corrupt);
        }
        check(cancel, until)?;
        Ok(self.listing)
    }
}

#[cfg(test)]
mod tests;
