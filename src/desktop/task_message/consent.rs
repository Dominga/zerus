//! Live consent for one admitted task message original. Neither a database row, a
//! display readiness boolean nor a notification grant constructs it; restart
//! and import have no live value to borrow.
use super::ledger;
use crate::desktop::creation_clone::lease::Lease;
use anyhow::{ensure, Context, Result};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use zerus_client_core::desktop::task_message::Verified;
use zerus_desktop_ledger::operations::Ticket;

pub(super) struct Consent {
    pub original: Verified,
    pub lease: Arc<Lease>,
    pub cancel: AtomicBool,
    /// The reference form's exact custody input, read only through this
    /// handle; `None` for an inline original, whose stdin is in its record.
    pub input: Option<Arc<super::staging::Input>>,
}
impl Consent {
    pub fn admitted(original: Verified, lease: Arc<Lease>) -> Result<Arc<Self>> {
        // A task message runs on this computer's own task store; it names no
        // other machine. The lease's native source still binds it.
        ensure!(
            original.intent.scope == lease.scope,
            "task message target changed"
        );
        ensure!(
            original.intent.form.is_inline(),
            "task message reference form needs its input"
        );
        Ok(Arc::new(Self {
            original,
            lease,
            cancel: AtomicBool::new(false),
            input: None,
        }))
    }
    /// Consent for a reference original whose exact input was prepared: its
    /// size and digest must be the plan's frozen stdin.
    pub fn referenced(
        original: Verified,
        lease: Arc<Lease>,
        input: Arc<super::staging::Input>,
    ) -> Result<Arc<Self>> {
        ensure!(
            original.intent.scope == lease.scope
                && !original.intent.form.is_inline()
                && input.sha256 == original.plan.input_sha256
                && Some(input.bytes) == original.input_len(),
            "task message prepared input changed"
        );
        Ok(Arc::new(Self {
            original,
            lease,
            cancel: AtomicBool::new(false),
            input: Some(input),
        }))
    }
    pub fn check(&self, row: &ledger::Row, ticket: Option<&Ticket>) -> Result<()> {
        ensure!(
            !self.cancel.load(Ordering::Acquire)
                && self.lease.alive()
                && self.original.binding == row.verified.binding
                && self.original.plan == row.verified.plan
                && self.original.intent.original == row.verified.intent.original
                && self.lease.scope == row.verified.intent.scope
                && self.lease.writer_epoch == row.writer_epoch,
            "task message original/consent changed"
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
                            .context("invalid message binding")?,
                "task message reservation changed"
            );
        }
        Ok(())
    }
    pub fn stop(&self) {
        self.cancel.store(true, Ordering::Release);
    }
}
