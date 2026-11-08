//! Storage exclusion only, never an operation permission or a clear result.
//!
//! The future host must still verify its original request, consent, record,
//! native target/image/configuration and actual writer/IO custody. No caller,
//! marker producer, migration or graceful controller is connected here.
//!
//! Acquire the run gate BEFORE the global writer: existing writers may need
//! completion hooks under that writer. A free gate with any pending row still
//! refuses. Releasing the state writer retains the SAME native run exclusion;
//! it neither creates a terminating marker nor proves that one exists.
use super::*;
use crate::{platform, state::storage};
use fs2::FileExt;
use std::fs::File;
use std::time::Duration;

/// An original native exclusion resource, not serializable or reconstructible
/// from status data. The original budget is captured once, never renewed.
pub(super) struct RunGate {
    store: Store,
    _lease: Lease,
    state_lock: File,
    owner: Owner,
    until: Instant,
}
impl RunGate {
    fn recheck(&self) -> Result<()> {
        current(self.until)?;
        if Owner::current()? != self.owner || !self.store.pending(self.until)?.is_empty() {
            return Err(REFUSED.into());
        }
        // Pinning a path string is insufficient after lock/root replacement.
        // The caller's normal writer is checked separately while it is held.
        let now = self.store.state_lock()?;
        use std::os::unix::fs::MetadataExt;
        let (old, now) = (
            self.state_lock.metadata().map_err(|_| REFUSED)?,
            now.metadata().map_err(|_| REFUSED)?,
        );
        if old.dev() != now.dev() || old.ino() != now.ino() {
            return Err(REFUSED.into());
        }
        current(self.until)
    }
}

/// Holds BOTH this original run's exclusion and the normal state writer.
/// Host code may perform its fresh source/record checks while it retains this
/// value. Merely obtaining it grants no input, signal, archive or cleanup.
pub(super) struct RunWriterGate {
    run: RunGate,
    writer: storage::Guard,
}
impl RunWriterGate {
    pub(super) fn acquire(scope: &Run, until: Instant) -> Result<Self> {
        current(until)?;
        let owner = Owner::current()?;
        let store = Store::open(scope, until)?;
        // No global writer is held while an earlier native writer drains.
        let lease = store.exclusive(until)?;
        if !store.pending(until)?.is_empty() {
            return Err(REFUSED.into());
        }
        let original = store.state_lock()?;
        let identity = platform::FileIdentity::of(&original).map_err(|_| REFUSED)?;
        // Lock this original descriptor, not a reopened path. Recovery must
        // not precede the original-lock/configuration identity check.
        let file = original.try_clone().map_err(|_| REFUSED)?;
        let writer = loop {
            current(until)?;
            match file.try_lock_exclusive() {
                Ok(()) => break storage::Guard::held(file),
                Err(error) if platform::lock_contended(&error) => std::thread::sleep(
                    Duration::from_millis(2).min(until.saturating_duration_since(Instant::now())),
                ),
                Err(_) => return Err(REFUSED.into()),
            }
        };
        store.check_writer(&original, &writer)?;
        storage::recover_run_writer(&writer, identity, until)?;
        let value = Self {
            run: RunGate {
                store,
                _lease: lease,
                state_lock: original,
                owner,
                until,
            },
            writer,
        };
        value.recheck()?;
        Ok(value)
    }
    pub(super) fn recheck(&self) -> Result<()> {
        self.run.recheck()?;
        self.run
            .store
            .check_writer(&self.run.state_lock, &self.writer)?;
        current(self.run.until)
    }
    /// For the future host, ONLY after its own durable marker/source checks.
    /// This function does not inspect, create, repair or retire a marker.
    pub(super) fn release_writer(self) -> RunGate {
        let Self { run, writer } = self;
        drop(writer);
        run
    }
}

#[cfg(test)]
static CONTENDED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(test)]
pub(super) fn contended() {
    CONTENDED.store(true, std::sync::atomic::Ordering::SeqCst);
}

#[cfg(test)]
mod tests;
