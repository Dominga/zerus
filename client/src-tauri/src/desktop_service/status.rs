//! The service status (zerus-hj2s): the locked snapshot that admission
//! decisions read, and a copy that status readers take without that lock.
//!
//! Catalog delivery holds the snapshot's admission lock around its view
//! callback. A status read through the lock would wait for that callback,
//! however long it runs. Every change made through [`Status::lock`] is copied
//! when its guard drops, while the lock is still held, so copies follow the
//! locked state in the same order. Admission decisions keep reading the
//! locked state.
use super::admission;
use std::ops::{Deref, DerefMut};
use std::sync::{Mutex, MutexGuard};
use tokio::sync::watch;
use zerus_client_core::desktop::service::Snapshot;

pub(super) struct Status {
    locked: Mutex<Snapshot>,
    copy: watch::Sender<Snapshot>,
}

impl Status {
    pub(super) fn new(snapshot: Snapshot) -> Self {
        let (copy, _) = watch::channel(snapshot.clone());
        Self {
            locked: Mutex::new(snapshot),
            copy,
        }
    }

    /// The locked snapshot, taken as every admission lock is (`admission`).
    pub(super) fn lock(&self) -> StatusGuard<'_> {
        StatusGuard {
            guard: admission(&self.locked),
            copy: &self.copy,
            changed: false,
        }
    }

    /// The latest status, without waiting for the lock.
    pub(super) fn copy(&self) -> Snapshot {
        self.copy.borrow().clone()
    }
}

/// The locked snapshot. A mutable borrow marks it changed, and dropping the
/// guard then publishes the copy before the lock is released.
pub(super) struct StatusGuard<'a> {
    guard: MutexGuard<'a, Snapshot>,
    copy: &'a watch::Sender<Snapshot>,
    changed: bool,
}

impl Deref for StatusGuard<'_> {
    type Target = Snapshot;
    fn deref(&self) -> &Snapshot {
        &self.guard
    }
}

impl DerefMut for StatusGuard<'_> {
    fn deref_mut(&mut self) -> &mut Snapshot {
        self.changed = true;
        &mut self.guard
    }
}

impl Drop for StatusGuard<'_> {
    fn drop(&mut self) {
        if self.changed {
            self.copy.send_replace(self.guard.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_copy_follows_every_change_and_answers_while_the_lock_is_held() {
        let status = Status::new(Snapshot::default());
        assert!(!status.copy().connected);
        {
            let read = status.lock();
            assert!(!read.connected);
        }
        status.lock().connected = true;
        assert!(status.copy().connected);
        *status.lock() = Snapshot {
            code: "replaced".into(),
            ..Snapshot::default()
        };
        assert_eq!(status.copy().code, "replaced");
        // A held lock, as a view callback holds it, never delays the copy.
        let held = status.lock();
        let (answered, answer) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            scope.spawn(|| answered.send(status.copy().code).unwrap());
            assert_eq!(
                answer.recv_timeout(std::time::Duration::from_secs(5)),
                Ok("replaced".to_owned())
            );
        });
        drop(held);
    }
}
