//! The ReadDelivery store's one-way barrier. The native service-mode importer
//! retains this opaque handle through checkpoint/capture and owned helper I/O.
use super::Actions;
use std::{
    sync::{Arc, Mutex},
    time::Instant,
};
use zerus_client_core::{
    desktop::drain::{Charge, Closed, Observation, Role},
    readmarks::{Checkpoint, ReadMarks},
};

pub(super) const MARKS_CLOSED: &str = "Read marks are not accepting changes.";

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Refused {
    #[cfg(not(unix))]
    Unsupported,
    #[cfg(unix)]
    Closed,
    Pending,
    Unavailable,
    Checkpoint,
}

/// One-use, non-serializing and not a handoff or capture permit. The charge
/// survives a controller timeout while its checkpoint thread still owns self.
pub(crate) struct FrozenReads {
    marks: Arc<Mutex<ReadMarks>>,
    mutations: Closed,
    native: Closed,
    _checkpoint: Charge,
}

pub(crate) struct CapturedReads {
    frozen: FrozenReads,
    checkpoint: Checkpoint,
    pub capture: zerus_client_core::desktop::pending_reads::Capture,
}
impl CapturedReads {
    pub fn unchanged(&self) -> Result<(), Refused> {
        let marks = self
            .frozen
            .marks
            .try_lock()
            .map_err(|_| Refused::Unavailable)?;
        marks
            .checkpoint_bytes(&self.checkpoint)
            .map(|_| ())
            .map_err(|_| Refused::Checkpoint)
    }
}
impl Actions {
    pub(super) fn admit_marks(&self) -> Result<Charge, String> {
        self.mark_admissions
            .admit(Role::ReadDelivery)
            .map_err(|_| MARKS_CLOSED.into())
    }

    /// Close every mark writer before cancelling native calls. A failed wait
    /// never reopens anything. Unsupported platforms refuse before any close.
    pub(crate) fn freeze_reads(&self) -> Result<FrozenReads, Refused> {
        #[cfg(not(unix))]
        {
            Err(Refused::Unsupported)
        }
        #[cfg(unix)]
        {
            let charge = self
                .checkpoint_admission
                .admit(Role::ReadDelivery)
                .map_err(|error| match error {
                    zerus_client_core::desktop::drain::Refused::Closed => Refused::Closed,
                    _ => Refused::Unavailable,
                })?;
            self.checkpoint_admission.close();
            let mutations = self.mark_admissions.close();
            let native = self.read_delivery.stop();
            Ok(FrozenReads {
                marks: self.marks.clone(),
                mutations,
                native,
                _checkpoint: charge,
            })
        }
    }
}
impl FrozenReads {
    pub(crate) fn capture(self, deadline: Instant) -> Result<CapturedReads, Refused> {
        let checkpoint = self.checkpoint_inner(deadline, ReadMarks::checkpoint)?;
        let bytes = self
            .marks
            .try_lock()
            .map_err(|_| Refused::Unavailable)?
            .checkpoint_bytes(&checkpoint)
            .map_err(|_| Refused::Checkpoint)?;
        let capture = zerus_client_core::desktop::pending_reads::Capture::from_bytes(&bytes)
            .map_err(|_| Refused::Checkpoint)?;
        if Instant::now() >= deadline {
            return Err(Refused::Pending);
        }
        Ok(CapturedReads {
            frozen: self,
            checkpoint,
            capture,
        })
    }
    /// Blocking, off the GUI thread. The same absolute deadline bounds both
    /// drains. Filesystem I/O may outlive it; self retains the barrier charge
    /// throughout, and a late completed sync still cannot return success.
    #[allow(dead_code)] // Checkpoint-only callers keep the original consuming API.
    pub(crate) fn checkpoint(self, deadline: Instant) -> Result<Checkpoint, Refused> {
        self.checkpoint_with(deadline, ReadMarks::checkpoint)
    }
    fn checkpoint_with(
        self,
        deadline: Instant,
        sync: impl FnOnce(&mut ReadMarks) -> std::io::Result<Checkpoint>,
    ) -> Result<Checkpoint, Refused> {
        self.checkpoint_inner(deadline, sync)
    }
    fn checkpoint_inner(
        &self,
        deadline: Instant,
        sync: impl FnOnce(&mut ReadMarks) -> std::io::Result<Checkpoint>,
    ) -> Result<Checkpoint, Refused> {
        for closed in [&self.mutations, &self.native] {
            match closed.wait_until(deadline) {
                Observation::Idle => (),
                Observation::Pending => return Err(Refused::Pending),
                Observation::Unavailable => return Err(Refused::Unavailable),
            }
        }
        if Instant::now() >= deadline {
            return Err(Refused::Pending);
        }
        // Read-only snapshots can hold the store too. Never block beyond the
        // drain wait or recover a poisoned store into a successful checkpoint.
        let mut marks = self.marks.try_lock().map_err(|_| Refused::Unavailable)?;
        let checkpoint = sync(&mut marks).map_err(|_| Refused::Checkpoint)?;
        if Instant::now() >= deadline {
            return Err(Refused::Pending);
        }
        Ok(checkpoint)
    }
}

#[cfg(all(test, unix))]
mod tests;

#[cfg(all(test, not(unix)))]
#[test]
fn unsupported_freeze_changes_no_admission() {
    let actions = Actions::new(
        std::env::temp_dir().join(format!("read-barrier-{}.json", uuid::Uuid::new_v4())),
    );
    assert!(matches!(actions.freeze_reads(), Err(Refused::Unsupported)));
    assert!(!actions.mark_admissions.snapshot().unwrap().closed);
    assert!(!actions.checkpoint_admission.snapshot().unwrap().closed);
    assert!(actions.read_delivery.admit().is_some());
}
