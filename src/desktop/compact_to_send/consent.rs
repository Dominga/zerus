//! Live consent for one admitted compact-to-send original. Neither a
//! database row, a display readiness boolean nor a notification grant
//! constructs it; restart and import have no live value to borrow.
use super::ledger::{self, Child};
use crate::desktop::creation_clone::lease::Lease;
use crate::desktop::operation_clock::Clock;
use anyhow::{ensure, Context, Result};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use zerus_client_core::desktop::compact_to_send::Verified;
use zerus_desktop_ledger::operations::Ticket;

pub(super) struct Consent {
    pub original: Verified,
    pub lease: Arc<Lease>,
    pub cancel: AtomicBool,
    /// The whole operation's bound ([`super::runtime::CHAIN`]), its deadline
    /// fixed at the operation's own first turn, when it gets the work slot,
    /// and never renewed: nothing starts after it. An operation queued
    /// behind another's wait keeps its whole bound (zerus-qw7wp).
    clock: Clock,
}
impl Consent {
    pub fn admitted(original: Verified, lease: Arc<Lease>, length: Duration) -> Result<Arc<Self>> {
        // The operation names a local session; the lease's native source
        // still binds this service and computer.
        ensure!(
            original.intent.scope == lease.scope,
            "compact-to-send target changed"
        );
        Ok(Arc::new(Self {
            original,
            lease,
            cancel: AtomicBool::new(false),
            clock: Clock::new(length),
        }))
    }
    /// The operation's deadline; its first call, at the operation's first
    /// turn, starts it.
    pub fn chain(&self) -> Instant {
        self.clock.deadline()
    }
    #[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
    pub(super) fn fixture_chain_started(&self) -> Option<Instant> {
        self.clock.fixture_started()
    }
    /// The same live original, lease and writer as the row; with a ticket,
    /// exactly `child`'s own reservation.
    pub fn check(&self, row: &ledger::Row, reserved: Option<(Child, &Ticket)>) -> Result<()> {
        ensure!(
            !self.cancel.load(Ordering::Acquire)
                && self.lease.alive()
                && self.original.binding == row.verified.binding
                && self.original.plan == row.verified.plan
                && self.original.intent.compaction == row.verified.intent.compaction
                && self.original.intent.original == row.verified.intent.original
                && self.original.intent.checkpoint == row.verified.intent.checkpoint
                && self.lease.scope == row.verified.intent.scope
                && self.lease.writer_epoch == row.writer_epoch,
            "compact-to-send original/consent changed"
        );
        if let Some((child, ticket)) = reserved {
            ticket.fingerprint()?;
            ensure!(
                row.reserved(child).map(|r| &r.ticket) == Some(ticket)
                    && ticket.lease == self.lease.id
                    && ticket.binding_sha256
                        == self
                            .original
                            .binding
                            .fingerprint()
                            .context("invalid compact-to-send binding")?,
                "compact-to-send reservation changed"
            );
        }
        Ok(())
    }
    pub fn stop(&self) {
        self.cancel.store(true, Ordering::Release);
    }
}
