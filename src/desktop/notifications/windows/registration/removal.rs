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
use crate::{
    desktop::{
        revoke::hold::{self, Verification},
        store::{HandoffLock, Location},
    },
    platform,
};
use anyhow::{bail, ensure, Context, Result};
use serde_json::Value;

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

/// The held offline exclusion; dropping it releases the handoff lock, then
/// owner.lock.
pub(super) struct Offline {
    proof: Option<Proof>,
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
            handoff,
            _owner: owner,
        })
    }

    /// Effect admission, right before the registry removal: the same exclusion
    /// still holds, and the same database and proof are still current. No
    /// state may appear behind a no-state proof.
    pub(super) fn admit(&self, location: &Location, id: &str) -> Result<()> {
        self.handoff.verify(location)?;
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
        Ok(())
    }

    #[cfg(test)]
    fn receipt(&self) -> Option<&Value> {
        self.proof.as_ref().map(|proof| &proof.receipt)
    }
}

#[cfg(test)]
mod tests;
