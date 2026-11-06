//! Live consent for one admitted deferred send original. Neither a database
//! row, a display readiness boolean nor a notification grant constructs it;
//! restart and import have no live value to borrow.
use super::ledger;
use crate::desktop::creation_clone::lease::Lease;
use anyhow::{ensure, Context, Result};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use zerus_client_core::desktop::deferred_send::Verified;
use zerus_desktop_ledger::operations::Ticket;

pub(super) struct Consent {
    pub original: Verified,
    pub lease: Arc<Lease>,
    pub cancel: AtomicBool,
}
impl Consent {
    pub fn admitted(original: Verified, lease: Arc<Lease>) -> Result<Arc<Self>> {
        // The send names a local session; the lease's native source still
        // binds this service and computer.
        ensure!(
            original.intent.scope == lease.scope,
            "deferred send target changed"
        );
        Ok(Arc::new(Self {
            original,
            lease,
            cancel: AtomicBool::new(false),
        }))
    }
    pub fn check(&self, row: &ledger::Row, ticket: Option<&Ticket>) -> Result<()> {
        ensure!(
            !self.cancel.load(Ordering::Acquire)
                && self.lease.alive()
                && self.original.binding == row.verified.binding
                && self.original.plan == row.verified.plan
                && self.original.intent.original == row.verified.intent.original
                && self.original.intent.checkpoint == row.verified.intent.checkpoint
                && self.lease.scope == row.verified.intent.scope
                && self.lease.writer_epoch == row.writer_epoch,
            "deferred send original/consent changed"
        );
        if let Some(ticket) = ticket {
            ticket.fingerprint()?;
            ensure!(
                row.ticket.as_ref() == Some(ticket)
                    && ticket.lease == self.lease.id
                    && ticket.binding_sha256
                        == self
                            .original
                            .binding
                            .fingerprint()
                            .context("invalid send binding")?,
                "deferred send reservation changed"
            );
        }
        Ok(())
    }
    pub fn stop(&self) {
        self.cancel.store(true, Ordering::Release);
    }
}
