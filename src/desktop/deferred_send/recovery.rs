//! One original read-only custody probe at a time, including in a passive
//! actor. SQLite stays on the actor; unknown effects are never started again.
//!
//! At the original's physical drain the probe worker, still holding the
//! released custody lock, reads the CLI's own receipt of the request off the
//! actor ([`super::receipt::observe`]); the actor then settles the receipt in
//! one transaction before that lock goes.
use super::{
    ledger,
    receipt::{self, Observed as Cli},
};
use crate::desktop::{custody, store::Location};
use anyhow::{ensure, Context, Result};
use rusqlite::{Connection, TransactionBehavior};
use std::{
    sync::mpsc,
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use zerus_desktop_ledger::operations::custody::Record;

struct Observed {
    record: Record,
    original: custody::Observation,
    /// Read only after a release; `None` while the custody is held.
    cli: Option<Cli>,
}
struct Pending {
    id: String,
    record: Record,
    worker: JoinHandle<()>,
    result: mpsc::Receiver<Observed>,
}
pub(in crate::desktop) struct Recovery {
    pending: Option<Pending>,
    after: String,
    next: Instant,
}
impl Default for Recovery {
    fn default() -> Self {
        Self {
            pending: None,
            after: String::new(),
            next: Instant::now(),
        }
    }
}
impl Recovery {
    pub fn tick(&mut self, location: &Location, db: &mut Connection) -> Result<()> {
        if self
            .pending
            .as_ref()
            .is_some_and(|v| !v.worker.is_finished())
        {
            return Ok(());
        }
        if let Some(pending) = self.pending.take() {
            pending
                .worker
                .join()
                .map_err(|_| anyhow::anyhow!("deferred send custody observer panicked"))?;
            let observed = pending
                .result
                .try_recv()
                .context("deferred send custody observation missing")?;
            self.after = pending.id.clone();
            self.next = Instant::now() + Duration::from_millis(250);
            let row = ledger::load(db, &pending.id)?.context("deferred send original missing")?;
            ensure!(
                row.custody.as_ref() == Some(&pending.record) && pending.record == observed.record,
                "deferred send custody scope changed"
            );
            match (observed.original, observed.cli) {
                (original @ custody::Observation::Released { .. }, Some(cli)) => {
                    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
                    let row =
                        ledger::load(&tx, &pending.id)?.context("deferred send disappeared")?;
                    if row.receipt.drained {
                        tx.commit()?;
                        return Ok(());
                    }
                    // Only the CLI's own matching receipt, read after the
                    // drain, records the send; anything else drains Unknown.
                    ledger::drained(tx, row, original, &cli)?;
                }
                (custody::Observation::Released { .. }, None) => {
                    anyhow::bail!("deferred send receipt observation missing")
                }
                (custody::Observation::Held, _) => (),
                (custody::Observation::Unavailable, _) => {
                    anyhow::bail!("deferred send original custody unavailable")
                }
            }
        }
        if Instant::now() < self.next {
            return Ok(());
        }
        let row = match ledger::custody_pending(db, &self.after)? {
            Some(row) => Some(row),
            None => {
                self.after.clear();
                ledger::custody_pending(db, "")?
            }
        };
        let Some(row) = row else { return Ok(()) };
        let id = row.verified.plan.request_id.clone();
        let plan = row.verified.plan.clone();
        let record = row
            .custody
            .context("deferred send has no original custody")?;
        let held = record.clone();
        let location = Location {
            root: location.root.clone(),
            configuration: location.configuration.clone(),
        };
        // The CLI's state root, of which this service's root is a folder.
        let state = location
            .root
            .parent()
            .context("deferred send state unavailable")?
            .to_path_buf();
        let (send, result) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("deferred-send-custody".into())
            .spawn(move || {
                let original = custody::probe(&location, &held);
                // Read while the released lock is held: no child of the
                // original can still write its receipt.
                let cli = matches!(original, custody::Observation::Released { .. })
                    .then(|| receipt::observe(&state, &plan));
                let _ = send.send(Observed {
                    record: held,
                    original,
                    cli,
                });
            })?;
        self.pending = Some(Pending {
            id,
            record,
            worker,
            result,
        });
        Ok(())
    }
}
