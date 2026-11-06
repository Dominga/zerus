//! Offline Toast registration removal (zerus-5wmh.66.3.30.8.5, route A).
//!
//! With no live service, the actual owner.lock and then the handoff lock are
//! acquired and held through the exact registry removal; a missing endpoint,
//! stopped task, PID or status boolean is never the exclusion. Without any
//! desktop state the removal needs nothing more. Existing desktop state needs
//! the CURRENT settled rollback proof of the retained request
//! (`revoke::hold::verify_current`), also for a passive-only history: a
//! not-applicable revoke reply is not a receipt. Its checked database
//! Verification is retained, and the proof is checked again right before the
//! registry effect. Nothing here submits or reconciles a revoke, migrates,
//! checkpoints or deletes state. The online path stays refused elsewhere.
//!
//! The held locks stay bound to the actual objects at their pathnames: each
//! lock file and both location folders are pinned (on Windows without delete
//! sharing, so none of them can be renamed, replaced or deleted while held),
//! and every pathname must still name the held object before the proof, after
//! it, and after the registry effect.
use crate::{
    desktop::{
        revoke::hold::{self, Verification},
        store::{HandoffLock, Location},
    },
    platform,
};
use anyhow::{bail, ensure, Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Every desktop state leaf whose presence means the service has run.
const LEAVES: [&str; 5] = [
    "state.sqlite3",
    "state.sqlite3-wal",
    "state.sqlite3-shm",
    "state.sqlite3-journal",
    zerus_desktop_migration::HANDOFF_FILE,
];

struct Proof {
    receipt: Value,
    identity: (u64, u64),
    _verification: Verification,
}

fn identity(file: &std::fs::File) -> Result<(u64, u64)> {
    let id = platform::FileIdentity::of(file)?;
    Ok((id.device, id.inode))
}

/// Opens `path` itself, never through a final link, for its identity. On
/// Windows the handle shares read and write but not delete: while it is open,
/// the object cannot be renamed, replaced or deleted.
fn pin(path: &Path, folder: bool) -> Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
            FILE_SHARE_WRITE,
        };
        options
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(
                FILE_FLAG_OPEN_REPARSE_POINT
                    | if folder {
                        FILE_FLAG_BACKUP_SEMANTICS
                    } else {
                        0
                    },
            );
    }
    #[cfg(unix)]
    {
        let _ = folder;
        platform::refuse_link(&mut options);
    }
    options
        .open(path)
        .with_context(|| format!("desktop exclusion object unavailable: {}", path.display()))
}

/// A held exclusion object bound to its pathname.
struct Bound {
    path: PathBuf,
    folder: bool,
    identity: (u64, u64),
    _pin: std::fs::File,
}
impl Bound {
    /// `held` is the identity of the object this process holds; the pathname
    /// must name that same object.
    fn held(path: PathBuf, held: (u64, u64)) -> Result<Self> {
        let pinned = pin(&path, false)?;
        ensure!(
            identity(&pinned)? == held,
            "desktop exclusion changed; registration kept"
        );
        Ok(Self {
            path,
            folder: false,
            identity: held,
            _pin: pinned,
        })
    }
    fn folder(path: PathBuf) -> Result<Self> {
        let pinned = pin(&path, true)?;
        let identity = identity(&pinned)?;
        Ok(Self {
            path,
            folder: true,
            identity,
            _pin: pinned,
        })
    }
    fn check(&self) -> Result<()> {
        ensure!(
            identity(&pin(&self.path, self.folder)?)? == self.identity,
            "desktop exclusion changed; registration kept"
        );
        Ok(())
    }
}

/// The held offline exclusion. Dropping it releases the retained database
/// Verification, the pins, the handoff lock and then owner.lock.
pub(super) struct Offline {
    proof: Option<Proof>,
    bound: Vec<Bound>,
    handoff: HandoffLock,
    _owner: platform::FileLock,
}

fn present(location: &Location) -> Result<Vec<&'static str>> {
    let mut found = Vec::new();
    for leaf in LEAVES {
        match std::fs::symlink_metadata(location.root.join(leaf)) {
            Ok(_) => found.push(leaf),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(found)
}

impl Offline {
    pub(super) fn acquire(location: &Location, id: &str) -> Result<Self> {
        // No missing endpoint is treated as a stopped owner. The actual OS lock
        // must be acquired; existing state cannot be replaced with a baseline.
        location.prepare()?;
        let owner = location.lock()?;
        // A Windows Runtime, COM class object or delegate keeps the serve
        // owner's handoff resource until physically released: contention
        // refuses here.
        let handoff = location.handoff_lock()?;
        // Bind both locks to the objects at their pathnames, and pin the
        // location folders that name them, before any state is read.
        let bound = vec![
            Bound::folder(
                location
                    .root
                    .parent()
                    .context("missing state folder")?
                    .to_owned(),
            )?,
            Bound::folder(location.root.clone())?,
            Bound::held(location.root.join("owner.lock"), {
                let id = owner.identity()?;
                (id.device, id.inode)
            })?,
            Bound::held(location.root.join(zerus_desktop_migration::HANDOFF_LOCK), {
                let id = handoff.identity()?;
                (id.device, id.inode)
            })?,
        ];
        let found = present(location)?;
        let proof = if found.is_empty() {
            None
        } else if !found.contains(&"state.sqlite3") {
            bail!("desktop state is incomplete; registration kept")
        } else {
            let (receipt, verification) = hold::verify_current(location, id, &handoff)
                .context("retained desktop state requires the current settled rollback of its original request; registration kept")?;
            let identity = verification.identity()?;
            Some(Proof {
                receipt,
                identity,
                _verification: verification,
            })
        };
        Ok(Self {
            proof,
            bound,
            handoff,
            _owner: owner,
        })
    }

    /// Every held exclusion object is still the one its pathname names.
    fn exclusion(&self, location: &Location) -> Result<()> {
        self.handoff.verify(location)?;
        for bound in &self.bound {
            bound.check()?;
        }
        Ok(())
    }

    /// Effect admission, right before the registry removal: the same exclusion
    /// objects are still held at their pathnames, and the same database and
    /// proof are still current. No state may appear behind a no-state proof.
    pub(super) fn admit(&self, location: &Location, id: &str) -> Result<()> {
        self.exclusion(location)?;
        match &self.proof {
            None => ensure!(
                present(location)?.is_empty(),
                "desktop state appeared; registration kept"
            ),
            Some(proof) => {
                let (receipt, again) = hold::verify_current(location, id, &self.handoff)
                    .context("the settled rollback is no longer current; registration kept")?;
                ensure!(
                    receipt == proof.receipt && again.identity()? == proof.identity,
                    "desktop state changed; registration kept"
                );
            }
        }
        self.exclusion(location)
    }

    /// After the registry effect: the exclusion held throughout. A failure
    /// here leaves the effect's outcome unknown; it is never replayed.
    pub(super) fn confirm(&self, location: &Location) -> Result<()> {
        self.exclusion(location)
    }

    #[cfg(test)]
    fn receipt(&self) -> Option<&Value> {
        self.proof.as_ref().map(|proof| &proof.receipt)
    }
}

#[cfg(test)]
mod tests;
