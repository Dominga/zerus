//! Inactive original action-claim RESOURCE factory, not an operation permit.
//! Production cannot construct FreshExecution in this leaf. A later reviewed
//! original producer must supply that affine witness before any possible start;
//! UUID/hash/Scope, saved status or receipt absence cannot reconstruct one.
//! No installed caller, status recovery, SourceIo adapter or effect calls here.
use super::{receipt, request_hash, validate, Request, INPUT_LIMIT};
use crate::{platform, state::repositories::directory::Directory, state::storage};
use fs2::FileExt;
use std::{
    fs::File,
    os::unix::fs::{FileExt as _, MetadataExt},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

const REFUSED: &str = "The original action claim is unconfirmed; inspect its original result";
const RECEIPT_BYTES: usize = 64 << 10;
type Result<T> = std::result::Result<T, String>;

/// Affine ORIGINAL execution witness: no Clone, deserializer, saved-data
/// constructor or production mint. Its test-only aliases share consumption;
/// a failed/late/missing-after-start attempt cannot obtain another admission.
pub(super) struct FreshExecution {
    bound: Bound,
    consumed: Arc<AtomicBool>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ActionKind {
    Pause,
    Resume,
    Rename,
    Fork,
}
fn kind(request: &Request) -> Result<ActionKind> {
    match request.action.as_str() {
        "pause" => Ok(ActionKind::Pause),
        "resume" => Ok(ActionKind::Resume),
        "rename" => Ok(ActionKind::Rename),
        "fork" => Ok(ActionKind::Fork),
        _ => Err(REFUSED.into()),
    }
}

#[derive(Eq, PartialEq)]
struct Owner {
    pid: u32,
    birth: String,
}
impl Owner {
    fn current() -> Result<Self> {
        let pid = std::process::id();
        let native = platform::process_identity(pid).ok_or(REFUSED)?;
        if native.birth.is_empty() || native.birth.len() > 96 {
            return Err(REFUSED.into());
        }
        Ok(Self {
            pid,
            birth: native.birth,
        })
    }
}

/// Exact original correlation and native store pins; not target/image/consent
/// proof. In particular, this never reads a session or reconstructs Scope.
struct Bound {
    root: Arc<Directory>,
    directory: Arc<Directory>,
    configured_root: PathBuf,
    owner: Owner,
    request: Request,
    name: String,
    hash: String,
    began: Instant,
    until: Instant,
}
impl Bound {
    fn current(&self) -> Result<()> {
        let now = Instant::now();
        if now < self.began
            || now >= self.until
            || self
                .until
                .checked_duration_since(self.began)
                .is_none_or(|budget| budget > Duration::from_secs(30))
            || Owner::current()? != self.owner
            || storage::root() != self.configured_root
            || self.directory.path != self.root.path.join("action_receipts")
        {
            return Err(REFUSED.into());
        }
        validate(&self.request, &self.name).map_err(|_| REFUSED)?;
        kind(&self.request)?;
        if self.hash.len() != 64
            || serde_json::to_vec(&self.request)
                .map_err(|_| REFUSED)?
                .len()
                > INPUT_LIMIT
            || request_hash(&self.request, &self.name) != self.hash
        {
            return Err(REFUSED.into());
        }
        // A retained RootA alone cannot prove that an unchanged configured
        // alias still selects RootA. Pin the CURRENT referent, directory-only,
        // and compare native objects; never adopt it as the captured root.
        let selected_path = platform::canonical(&self.configured_root).map_err(|_| REFUSED)?;
        let selected = Directory::open(&selected_path).map_err(|_| REFUSED)?;
        if !selected.private().map_err(|_| REFUSED)?
            || selected.object().map_err(|_| REFUSED)? != self.root.object().map_err(|_| REFUSED)?
        {
            return Err(REFUSED.into());
        }
        #[cfg(test)]
        AFTER_ROOT_REFERENT.with(|slot| {
            if let Some(change) = slot.borrow_mut().take() {
                change();
            }
        });
        for directory in [&self.root, &self.directory] {
            directory.verify().map_err(|_| REFUSED)?;
            if !directory.private().map_err(|_| REFUSED)? {
                return Err(REFUSED.into());
            }
        }
        selected.verify().map_err(|_| REFUSED)?;
        if !selected.private().map_err(|_| REFUSED)?
            || selected.object().map_err(|_| REFUSED)? != self.root.object().map_err(|_| REFUSED)?
            || storage::root() != self.configured_root
            || platform::canonical(&self.configured_root).map_err(|_| REFUSED)? != selected_path
            || Instant::now() >= self.until
        {
            return Err(REFUSED.into());
        }
        Ok(())
    }
    fn leaf(&self) -> String {
        format!("{}.json", self.request.request_id)
    }
    fn lock_leaf(&self) -> String {
        format!("{}.lock", self.request.request_id)
    }
}

/// Native exclusion is close-only. NEVER wrap this in FileLock or explicitly
/// unlock a duplicate: the last same-original loan must retain its lock.
struct Lock {
    bound: Bound,
    file: File,
    identity: platform::FileIdentity,
}
impl Lock {
    fn current(&self) -> Result<()> {
        self.bound.current()?;
        let current = self
            .bound
            .directory
            .existing_private_lock(&self.bound.lock_leaf())
            .map_err(|_| REFUSED)?;
        for file in [&self.file, &current] {
            if !platform::verify_private_file(file).map_err(|_| REFUSED)?
                || platform::FileIdentity::of(file).map_err(|_| REFUSED)? != self.identity
                || file.metadata().map_err(|_| REFUSED)?.len() != 0
                || file.metadata().map_err(|_| REFUSED)?.mode() & 0o600 != 0o600
            {
                return Err(REFUSED.into());
            }
        }
        self.bound.current()
    }
    fn acquire(bound: Bound) -> Result<Self> {
        bound.current()?;
        let file = bound
            .directory
            .open_private_lock(&bound.lock_leaf())
            .map_err(|_| REFUSED)?;
        let original = Self {
            identity: platform::FileIdentity::of(&file).map_err(|_| REFUSED)?,
            bound,
            file,
        };
        loop {
            original.current()?;
            match original.file.try_lock_exclusive() {
                Ok(()) => {
                    original.current()?;
                    return Ok(original);
                }
                Err(error) if platform::lock_contended(&error) => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(_) => return Err(REFUSED.into()),
            }
        }
    }
}

struct Lease {
    lock: Lock,
    file: File,
    identity: platform::FileIdentity,
    bytes: Vec<u8>,
}
impl Lease {
    fn current(&self) -> Result<()> {
        self.lock.current()?;
        let current = self
            .lock
            .bound
            .directory
            .read_private(&self.lock.bound.leaf())
            .map_err(|_| REFUSED)?;
        for file in [&self.file, &current] {
            if !platform::verify_private_file(file).map_err(|_| REFUSED)?
                || platform::FileIdentity::of(file).map_err(|_| REFUSED)? != self.identity
                || file.metadata().map_err(|_| REFUSED)?.len() != self.bytes.len() as u64
            {
                return Err(REFUSED.into());
            }
            let mut bytes = vec![0; self.bytes.len()];
            let mut read = 0;
            while read < bytes.len() {
                self.lock.bound.current()?;
                let count = file
                    .read_at(&mut bytes[read..], read as u64)
                    .map_err(|_| REFUSED)?;
                if count == 0 {
                    return Err(REFUSED.into());
                }
                read += count;
            }
            if bytes != self.bytes
                || platform::FileIdentity::of(file).map_err(|_| REFUSED)? != self.identity
                || file.metadata().map_err(|_| REFUSED)?.len() != self.bytes.len() as u64
            {
                return Err(REFUSED.into());
            }
        }
        self.lock.current()
    }
}

/// Single original claim resource. No Clone/deserializer, finalizer, input,
/// signal, operation result, native authority or physical-row retirement API.
/// Drop never removes/rewrites the claim. Cross-process/other-writer/descendant
/// settlement and host durable receipt/no-replay/native checks stay external.
pub(super) struct OriginalClaim {
    original: Arc<Lease>,
}
impl OriginalClaim {
    pub(super) fn recheck(&self) -> Result<()> {
        self.original.current()
    }
    pub(super) fn kind(&self) -> Result<ActionKind> {
        kind(&self.original.lock.bound.request)
    }
    pub(super) fn request_id(&self) -> &str {
        &self.original.lock.bound.request.request_id
    }
    pub(super) fn request_hash(&self) -> &str {
        &self.original.lock.bound.hash
    }
    pub(super) fn deadline(&self) -> Instant {
        self.original.lock.bound.until
    }

    /// A loan of the SAME close-only lock AND claim/store handles, not a new
    /// open or stop permit. Retain through actual owned Child/EOF/worker joins.
    /// A free lock, physical report or missing writer row cannot settle this
    /// durable operation or mint a replacement FreshExecution.
    pub(super) fn resources(&self) -> Result<Arc<dyn Send + Sync>> {
        self.recheck()?;
        Ok(self.original.clone())
    }
}

/// Sole dormant minting CONSUMER; production minting of its input is absent.
/// No body here invokes an action, status recovery or the SourceIo bridge.
/// Persistence can block: the host must retain its ORIGINAL owned helper and
/// drains beyond timeout until physical settlement. Clock checks are not a
/// claim of an interruptible fsync or proof that an interrupted write vanished.
pub(super) fn claim(fresh: FreshExecution) -> Result<OriginalClaim> {
    let FreshExecution { bound, consumed } = fresh;
    if consumed.swap(true, Ordering::AcqRel) {
        return Err(REFUSED.into());
    }
    let lock = Lock::acquire(bound)?;
    // Absence is necessary, NEVER the freshness witness. Any existing result,
    // even claimed/failed/uncertain or malformed, refuses without rewriting it.
    if lock
        .bound
        .directory
        .read_private_optional(&lock.bound.leaf())
        .map_err(|_| REFUSED)?
        .is_some()
    {
        return Err(REFUSED.into());
    }
    lock.current()?;
    #[cfg(test)]
    AFTER_ABSENCE.with(|slot| {
        if let Some(change) = slot.borrow_mut().take() {
            change();
        }
    });
    let bytes = receipt(
        &lock.bound.request,
        &lock.bound.name,
        &lock.bound.hash,
        "claimed",
    )
    .to_string()
    .into_bytes();
    if bytes.len() > RECEIPT_BYTES {
        return Err(REFUSED.into());
    }
    lock.current()?;
    // Existing helper syncs the exact file and directory, publishes without
    // replacement, and returns THIS original file. No new format/namespace.
    let file = lock
        .bound
        .directory
        .write_new_file(&lock.bound.leaf(), &bytes)
        .map_err(|_| REFUSED)?;
    #[cfg(test)]
    AFTER_PUBLICATION.with(|slot| {
        if let Some(change) = slot.borrow_mut().take() {
            change();
        }
    });
    let identity = platform::FileIdentity::of(&file).map_err(|_| REFUSED)?;
    let original = OriginalClaim {
        original: Arc::new(Lease {
            lock,
            file,
            identity,
            bytes,
        }),
    };
    original.recheck()?;
    Ok(original)
}

#[cfg(test)]
thread_local! {
    static AFTER_ROOT_REFERENT: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
    static AFTER_ABSENCE: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
    static AFTER_PUBLICATION: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}
#[cfg(test)]
mod tests;
