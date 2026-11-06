//! Sealed relay evidence under the original service owner. Never a permission.
use super::{Process, same_user};
use anyhow::{Context, Result, ensure};
use std::{
    fs::File,
    sync::{Arc, Weak, mpsc},
    thread,
    time::{Duration, Instant},
};
use zerus_client_core::image::linux::{PeerImage, ServiceImage};

const SETUP_WAIT: Duration = Duration::from_secs(5);

pub(super) struct Reference {
    image: Arc<ServiceImage>,
    owner: Weak<crate::platform::FileLock>,
}
impl Reference {
    /// Called exactly once, before publishing the listener. Failure/timeout
    /// permanently disables this branch for this Scope, without weakening the
    /// existing same-inode path. A late result can never be published to it.
    pub(super) fn capture(owner: &Arc<crate::platform::FileLock>) -> Option<Arc<Self>> {
        Self::capture_with(owner, SETUP_WAIT, ServiceImage::capture)
    }
    fn capture_with(
        owner: &Arc<crate::platform::FileLock>,
        wait: Duration,
        work: impl FnOnce() -> std::io::Result<ServiceImage> + Send + 'static,
    ) -> Option<Arc<Self>> {
        let weak = Arc::downgrade(owner);
        let loan = owner.clone();
        let (send, receive) = mpsc::sync_channel(1);
        // Start the custodian FIRST. A thread-start failure cannot strand an
        // already-started copy worker or drop its original owner loan.
        thread::Builder::new()
            .name("desktop-image-join".into())
            .spawn(move || {
                let original = thread::Builder::new()
                    .name("desktop-image-copy".into())
                    .spawn(work);
                let image = original.ok().and_then(|job| job.join().ok()?.ok());
                // The physical original worker has joined before sending or
                // releasing resources. Receiver timeout drops any late image.
                let _ = send.send(image);
                drop(loan);
            })
            .ok()?;
        let image = Arc::new(receive.recv_timeout(wait).ok()??);
        Some(Arc::new(Self { image, owner: weak }))
    }
    pub(super) fn observe(&self, caller: &Process, until: Instant) -> Result<Arc<Peer>> {
        let loan = self
            .owner
            .upgrade()
            .context("original service owner unavailable")?;
        ensure!(
            caller.current() && same_user(caller.pid)?,
            "relay changed before image check"
        );
        // Only the kernel-authenticated original peer PID selects this magic
        // link. Never reopen its reported executable text or accept a wire fd.
        let original = File::open(format!("/proc/{}/exe", caller.pid))?;
        let image = self.image.inspect_peer(original, until)?;
        ensure!(
            Instant::now() < until && caller.current() && same_user(caller.pid)?,
            "relay changed during image check"
        );
        let peer = Arc::new(Peer {
            image,
            _owner: loan,
        });
        ensure!(peer.current(caller.pid), "relay executable changed");
        Ok(peer)
    }
}

pub(super) struct Peer {
    image: PeerImage,
    _owner: Arc<crate::platform::FileLock>,
}
impl Peer {
    pub(super) fn current(&self, original_pid: u32) -> bool {
        same_user(original_pid).unwrap_or(false)
            && File::open(format!("/proc/{original_pid}/exe"))
                .is_ok_and(|actual| self.image.current(&actual))
            && same_user(original_pid).unwrap_or(false)
    }
}

#[cfg(test)]
mod tests;
