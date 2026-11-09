//! One original read-only custody probe at a time, including in a passive
//! actor, for the originals no live runtime holds: those of an earlier
//! service run. SQLite stays on the actor; unknown effects are never started
//! again.
//!
//! Such an original has no observation of its original receipt store, so no
//! receipt is read for it: its drain settles it Unknown
//! ([`super::receipt::UNHELD`]). A live runtime drains its own originals
//! itself, from the store objects it opened before their child
//! ([`super::runtime`]); they are [`Owned`] and skipped here.
use super::ledger;
use crate::desktop::{custody, store::Location};
use anyhow::{ensure, Context, Result};
use rusqlite::{Connection, TransactionBehavior};
use std::{
    collections::BTreeMap,
    sync::{mpsc, Arc, Mutex, Weak},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use zerus_desktop_ledger::operations::custody::Record;

/// The originals a live runtime drains itself, by request ID: each while its
/// runtime work or drain worker still holds the token.
pub(in crate::desktop) type Owned = Arc<Mutex<BTreeMap<String, Weak<()>>>>;

struct Probed {
    record: Record,
    original: custody::Observation,
}
struct Pending {
    id: String,
    record: Record,
    worker: JoinHandle<()>,
    result: mpsc::Receiver<Probed>,
}
pub(in crate::desktop) struct Recovery {
    pending: Option<Pending>,
    after: String,
    next: Instant,
    owned: Owned,
}
impl Default for Recovery {
    fn default() -> Self {
        Self {
            pending: None,
            after: String::new(),
            next: Instant::now(),
            owned: Owned::default(),
        }
    }
}
impl Recovery {
    /// The set a live runtime marks its own originals in.
    pub(in crate::desktop) fn owned(&self) -> Owned {
        self.owned.clone()
    }
    fn owns(&self, id: &str) -> bool {
        let mut owned = self
            .owned
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        owned.retain(|_, token| token.strong_count() > 0);
        owned.contains_key(id)
    }
    /// The next undrained original with custody after `after` that no live
    /// runtime holds (at most the active bound of them).
    fn next_unowned(&self, db: &Connection, after: &str) -> Result<Option<ledger::Row>> {
        let mut after = after.to_owned();
        for _ in 0..33 {
            let Some(row) = ledger::custody_pending(db, &after)? else {
                return Ok(None);
            };
            let id = row.id().to_owned();
            if !self.owns(&id) {
                return Ok(Some(row));
            }
            after = id;
        }
        Ok(None)
    }
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
                .map_err(|_| anyhow::anyhow!("session resume custody observer panicked"))?;
            let observed = pending
                .result
                .try_recv()
                .context("session resume custody observation missing")?;
            self.after = pending.id.clone();
            self.next = Instant::now() + Duration::from_millis(250);
            let row = ledger::load(db, &pending.id)?.context("session resume original missing")?;
            ensure!(
                custody(&row) == Some(&pending.record) && pending.record == observed.record,
                "session resume custody scope changed"
            );
            match observed.original {
                original @ custody::Observation::Released { .. } => {
                    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
                    let row =
                        ledger::load(&tx, &pending.id)?.context("session resume disappeared")?;
                    if row.receipt.drained || self.owns(&pending.id) {
                        tx.commit()?;
                        return Ok(());
                    }
                    // No original store observation exists for it: Unknown,
                    // never read from a path again, and nothing after it
                    // ever starts.
                    ledger::unheld(tx, row, original)?;
                }
                custody::Observation::Held => (),
                custody::Observation::Unavailable => {
                    anyhow::bail!("session resume original custody unavailable")
                }
            }
        }
        if Instant::now() < self.next {
            return Ok(());
        }
        let row = match self.next_unowned(db, &self.after.clone())? {
            Some(row) => Some(row),
            None => {
                self.after.clear();
                self.next_unowned(db, "")?
            }
        };
        let Some(row) = row else { return Ok(()) };
        let id = row.id().to_owned();
        let record = custody(&row)
            .cloned()
            .context("session resume has no original custody")?;
        let held = record.clone();
        let location = Location {
            root: location.root.clone(),
            configuration: location.configuration.clone(),
        };
        let (send, result) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("session resume-custody".into())
            .spawn(move || {
                let original = custody::probe(&location, &held);
                let _ = send.send(Probed {
                    record: held,
                    original,
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
/// The custody of the operation's running child: the one a drain settles.
fn custody(row: &ledger::Row) -> Option<&Record> {
    row.draining()
        .and_then(|child| row.reserved(child))
        .map(|reserved| &reserved.custody)
}
