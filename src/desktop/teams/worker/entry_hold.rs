//! Test-only pause of a worker thread at its run entry, before any launch,
//! store or native effect. A hold is keyed by its state folder, so tests that
//! run concurrently never pause each other's workers. Release builds compile
//! none of this.
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Mutex,
    },
    time::Duration,
};

struct Slot {
    id: u64,
    state: PathBuf,
    entered: mpsc::SyncSender<()>,
    release: mpsc::Receiver<()>,
}

static SLOTS: Mutex<Vec<Slot>> = Mutex::new(Vec::new());
static NEXT: AtomicU64 = AtomicU64::new(0);

/// The test's side of one pause. Dropping it releases the paused thread, or
/// withdraws the slot when no thread reached it, also while unwinding.
pub(in crate::desktop) struct Hold {
    id: u64,
    entered: mpsc::Receiver<()>,
    release: Option<mpsc::SyncSender<()>>,
}

impl Hold {
    /// Pauses the next worker thread that enters its run for `state`. A
    /// later hold for the same folder waits for the next thread after it.
    pub fn install(state: &Path) -> Self {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let (entered, reached) = mpsc::sync_channel(1);
        let (release, receive) = mpsc::sync_channel(1);
        let mut slots = SLOTS.lock().unwrap_or_else(|e| e.into_inner());
        assert!(
            slots.iter().all(|slot| slot.state != state),
            "one pending hold per state folder"
        );
        slots.push(Slot {
            id,
            state: state.to_owned(),
            entered,
            release: receive,
        });
        Self {
            id,
            entered: reached,
            release: Some(release),
        }
    }
    /// Whether the worker thread reached its run entry within `timeout`.
    pub fn entered(&self, timeout: Duration) -> bool {
        self.entered.recv_timeout(timeout).is_ok()
    }
    /// Lets the paused thread return without running.
    pub fn release(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.try_send(());
        }
    }
}

impl Drop for Hold {
    fn drop(&mut self) {
        SLOTS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|slot| slot.id != self.id);
        self.release();
    }
}

/// Called first in the worker thread's run. Returns true after a paused run
/// was released; the run then returns without any effect.
pub(super) fn pause(state: &Path) -> bool {
    let slot = {
        let mut slots = SLOTS.lock().unwrap_or_else(|e| e.into_inner());
        let index = slots.iter().position(|slot| slot.state == state);
        index.map(|index| slots.swap_remove(index))
    };
    let Some(slot) = slot else {
        return false;
    };
    let _ = slot.entered.try_send(());
    // A release, or the hold dropped by an unwinding test, ends the pause.
    let _ = slot.release.recv();
    true
}
