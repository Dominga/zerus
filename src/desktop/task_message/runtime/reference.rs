//! The reference form of a task message original (zerus-5wmh.66.4.4.2):
//! preparation off the actor, a short admission, and custody-input removal.
//!
//! - Admission is asynchronous, as task-start reconciliation's is. Capacity
//!   is reserved first. A bounded worker that holds the lease (and with it the
//!   serve-owner loan) prepares the exact input from the staged copies. Then
//!   one short transaction rechecks the opaque native permit AFTER BEGIN
//!   IMMEDIATE, with the same Service, configuration, lease and the exact
//!   prepared binding, admits the original and holds its input. An expired
//!   permit, a changed lease or a stop refuses: nothing was admitted, and the
//!   exact published input is removed before its reservation is released.
//! - A custody input is removed only after its original drained physically
//!   (every resources guard, and with it every original pipe, gone) and no
//!   consent, preparation or worker of this process holds it. A failed
//!   removal stays visible and charged.
//! - A reservation whose preparation failed, or one left by an earlier process,
//!   is classified only once nothing here holds it: if its input exists it
//!   was published but never admitted, and is kept, charged and reviewable
//!   (`orphaned`); otherwise it is released.
use super::*;
use crate::desktop::fanout::Queue;
use std::time::{Duration, Instant};

/// The most a preparation may take. The native permit's own lifetime still
/// decides at admission.
const PREPARE: Duration = Duration::from_secs(5);
/// Inputs considered per removal sweep.
const SWEEP: u32 = 8;
/// A second request for an original that is still being prepared.
const PREPARING: &str = "task_message_preparing";

pub(in crate::desktop::task_message) struct Admitting {
    authority: ui_authority::Validated,
    verified: wire::Verified,
    lease: Arc<Lease>,
    queue: Arc<Queue>,
    seq: u64,
    worker: JoinHandle<()>,
    ready: mpsc::Receiver<std::result::Result<staging::Prepared, &'static str>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Job {
    /// A published input whose admission was refused.
    Refused,
    /// A held input whose original drained.
    Drained,
    /// A reservation left by an earlier process.
    Reserved,
}
pub(in crate::desktop::task_message) struct Removal {
    id: String,
    job: Job,
    worker: JoinHandle<()>,
    done: mpsc::Receiver<bool>,
}

impl Runtime {
    fn answer(&self, queue: &Queue, seq: u64, value: Value) {
        let frame = crate::desktop::frame(&self.scope.service_epoch, seq, "response", value);
        if let Ok(bytes) = crate::desktop::protocol::encode(&frame) {
            queue.offer(bytes.into());
        }
        queue.close();
    }
    fn refusal(&self, id: &str, code: &str) -> Value {
        json!(wire::Admission {
            abi: wire::ABI.into(),
            request_id: id.into(),
            scope: self.scope.clone(),
            accepted: false,
            durable: false,
            code: code.into(),
            receipt: None
        })
    }

    /// A reference original from the native frontend lane. Its reply follows
    /// on `queue` once it was admitted or refused.
    pub fn frontend_reference(
        &mut self,
        store: &mut Store,
        authority: ui_authority::Validated,
        exclusion: Option<Arc<HandoffLock>>,
        queue: Arc<Queue>,
        seq: u64,
    ) {
        let id = authority.id().to_owned();
        if self.mac_fence {
            let value = self.refusal(&id, MAC_UNAVAILABLE);
            return self.answer(&queue, seq, value);
        }
        self.refresh(store, exclusion);
        match self.start_reference(store, authority, &queue, seq) {
            Ok(Some(value)) => self.answer(&queue, seq, value),
            Ok(None) => {}
            Err(_) => {
                let value = self.refusal(&id, "task_message_unavailable");
                self.answer(&queue, seq, value)
            }
        }
    }

    fn start_reference(
        &mut self,
        store: &mut Store,
        authority: ui_authority::Validated,
        queue: &Arc<Queue>,
        seq: u64,
    ) -> Result<Option<Value>> {
        let id = authority.id().to_owned();
        ensure!(
            !self.stopping && !self.failed && store.creation_recovery_ok(),
            "task message owner unavailable"
        );
        // Neither refusal is definite: the first request may still be
        // admitted, and an earlier record decides on its own.
        if self
            .admitting
            .as_ref()
            .is_some_and(|a| a.verified.plan.request_id == id)
        {
            return Ok(Some(self.refusal(&id, PREPARING)));
        }
        ensure!(self.admitting.is_none(), "task message preparation busy");
        ensure!(
            self.known_charges(&store.db)?,
            "task message original resources retained"
        );
        let roots = self
            .roots
            .clone()
            .context("task message staging unavailable")?;
        let lease = self
            .lease
            .clone()
            .context("task message lease unavailable")?;
        let intent = authority.task_message_preview()?.clone();
        ensure!(
            intent.scope == self.scope && intent.form == wire::Form::ReferenceV1,
            "task message reference changed"
        );
        let verified = intent.verify().map_err(anyhow::Error::msg)?;
        // The same original again: only its own receipt, read after BEGIN
        // with the permit rechecked; it is never prepared again.
        if ledger::load(&store.db, &id)?.is_some() {
            let tx = store
                .db
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let intent = authority.task_message()?;
            let row = ledger::load(&tx, &id)?.context("task message disappeared")?;
            ensure!(
                intent == verified.intent && row.verified.binding == verified.binding,
                "task message original changed"
            );
            tx.commit()?;
            return Ok(Some(json!(wire::Admission {
                abi: wire::ABI.into(),
                request_id: id,
                scope: self.scope.clone(),
                accepted: true,
                durable: true,
                code: String::new(),
                receipt: Some(row.receipt),
            })));
        }
        if super::super::inputs::load(&store.db, &id)?.is_some() {
            return Ok(Some(self.refusal(&id, staging::EXISTS)));
        }
        let bytes = verified
            .input_len()
            .context("task message input size unknown")?;
        let tx = store
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        super::super::inputs::reserve(
            &tx,
            &id,
            bytes,
            &verified.plan.input_sha256,
            chrono::Utc::now().timestamp_millis(),
        )?;
        tx.commit()?;
        let (send, ready) = mpsc::sync_channel(1);
        let original = verified.intent.clone();
        let retained = lease.clone();
        let worker = thread::Builder::new()
            .name("task-message-prepare".into())
            .spawn(move || {
                let _retained = retained;
                let result =
                    original
                        .verify()
                        .map_err(|_| staging::MISMATCH)
                        .and_then(|verified| {
                            staging::prepare(&roots, &verified, Instant::now() + PREPARE)
                        });
                let _ = send.send(result);
            });
        let worker = match worker {
            Ok(worker) => worker,
            Err(error) => {
                let tx = store
                    .db
                    .transaction_with_behavior(TransactionBehavior::Immediate)?;
                super::super::inputs::release(&tx, &id)?;
                tx.commit()?;
                return Err(error.into());
            }
        };
        self.admitting = Some(Box::new(Admitting {
            authority,
            verified,
            lease,
            queue: queue.clone(),
            seq,
            worker,
            ready,
        }));
        Ok(None)
    }

    pub(super) fn poll_reference(&mut self, store: &mut Store) -> Result<()> {
        self.poll_admitting(store)?;
        self.poll_removal(store)
    }

    fn poll_admitting(&mut self, store: &mut Store) -> Result<()> {
        let Some(pending) = self.admitting.take() else {
            return Ok(());
        };
        if !pending.worker.is_finished() {
            self.admitting = Some(pending);
            return Ok(());
        }
        let Admitting {
            authority,
            verified,
            lease,
            queue,
            seq,
            worker,
            ready,
        } = *pending;
        let id = verified.plan.request_id.clone();
        let prepared = worker.join().ok().and_then(|()| ready.try_recv().ok());
        let prepared = match prepared {
            Some(Ok(prepared)) => prepared,
            failed => {
                // The input may still exist: its identity unread, its
                // removal failed or its worker gone. The record stays
                // reserved and charged; the reservation sweep keeps an
                // existing input as an exact orphan and releases one that is
                // gone.
                let code = match failed {
                    Some(Err(code)) => code,
                    _ => "task_message_unavailable",
                };
                let value = self.refusal(&id, code);
                self.answer(&queue, seq, value);
                return Ok(());
            }
        };
        let identity = prepared.input.identity;
        let staged = prepared.staged.clone();
        match self.admit_reference(store, authority, verified, &lease, prepared.input) {
            Ok(value) => {
                self.answer(&queue, seq, value);
                // Exactly the staged copies it read; they are disposable.
                if let Some(roots) = self.roots.clone() {
                    let request = id.clone();
                    let _ = thread::Builder::new()
                        .name("task-message-staging".into())
                        .spawn(move || {
                            let _ = staging::remove_staged(&roots, &request, &staged);
                        });
                }
            }
            Err(_) => {
                // Nothing was admitted. The exact published input is removed
                // before the reservation is released.
                self.refused.push((id.clone(), identity));
                let value = self.refusal(&id, "task_message_unavailable");
                self.answer(&queue, seq, value);
            }
        }
        Ok(())
    }

    fn admit_reference(
        &mut self,
        store: &mut Store,
        authority: ui_authority::Validated,
        verified: wire::Verified,
        lease: &Arc<Lease>,
        input: staging::Input,
    ) -> Result<Value> {
        ensure!(
            !self.stopping && !self.failed && store.creation_recovery_ok(),
            "task message owner unavailable"
        );
        ensure!(
            self.lease
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, lease)),
            "task message lease replaced"
        );
        let id = verified.plan.request_id.clone();
        let identity = input.identity;
        let tx = store
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        // Recheck the original opaque native permit AFTER the writer lock: an
        // expired permit refuses, and nothing renews it.
        let intent = authority.task_message()?;
        ensure!(intent == verified.intent, "task message connection changed");
        lease.current(
            &tx,
            &self.location,
            self.machine.as_ref(),
            self.exclusion.as_ref(),
        )?;
        ensure!(
            ledger::load(&tx, &id)?.is_none(),
            "task message original exists"
        );
        let consent = Consent::referenced(verified, lease.clone(), Arc::new(input))?;
        let row = ledger::admit(
            &tx,
            &consent.original,
            &lease.writer_epoch,
            chrono::Utc::now().timestamp_millis(),
        )?;
        super::super::inputs::hold(&tx, &id, identity)?;
        tx.commit()?;
        self.admitted.insert(id.clone(), consent);
        Ok(json!(wire::Admission {
            abi: wire::ABI.into(),
            request_id: id,
            scope: self.scope.clone(),
            accepted: true,
            durable: true,
            code: String::new(),
            receipt: Some(row.receipt),
        }))
    }

    /// One removal or recovery at a time, off the actor.
    fn poll_removal(&mut self, store: &mut Store) -> Result<()> {
        if let Some(removal) = self.removal.take() {
            if !removal.worker.is_finished() {
                self.removal = Some(removal);
                return Ok(());
            }
            let ok = removal.worker.join().is_ok() && removal.done.try_recv().unwrap_or(false);
            let tx = store
                .db
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            match (removal.job, ok) {
                (Job::Refused, true) => super::super::inputs::release(&tx, &removal.id)?,
                // An unremoved refused input stays reserved; the reservation
                // sweep then keeps it as an exact orphan.
                (Job::Refused, false) => {}
                (Job::Drained, true) => super::super::inputs::removed(&tx, &removal.id)?,
                (Job::Drained, false) => super::super::inputs::failed(&tx, &removal.id)?,
                (Job::Reserved, published) => {
                    super::super::inputs::recover_reserved(&tx, &removal.id, published)?
                }
            }
            tx.commit()?;
            return Ok(());
        }
        let Some(roots) = self.roots.clone() else {
            return Ok(());
        };
        let next = if let Some((id, identity)) = self.refused.pop() {
            Some((id, Job::Refused, Some(identity)))
        } else {
            self.next_drained(store)?
                .map(|(id, identity)| (id, Job::Drained, Some(identity)))
                .or(self
                    .next_reserved(store)?
                    .map(|id| (id, Job::Reserved, None)))
        };
        let Some((id, job, identity)) = next else {
            return Ok(());
        };
        let (send, done) = mpsc::sync_channel(1);
        let request = id.clone();
        let worker = thread::Builder::new()
            .name("task-message-input".into())
            .spawn(move || {
                let ok = match identity {
                    Some(identity) => staging::remove_input(&roots, &request, identity).is_ok(),
                    None => staging::input_exists(&roots, &request).unwrap_or(true),
                };
                let _ = send.send(ok);
            })?;
        self.removal = Some(Removal {
            id,
            job,
            worker,
            done,
        });
        Ok(())
    }

    /// Whether this process still holds `id` in any form.
    fn holds(&self, id: &str) -> bool {
        self.admitted.contains_key(id)
            || self
                .admitting
                .as_ref()
                .is_some_and(|a| a.verified.plan.request_id == id)
            || self.refused.iter().any(|(refused, _)| refused == id)
    }

    /// A held input whose original drained physically and that nothing here
    /// holds any more. Only drained originals are considered, so running ones
    /// never hide a newer drained one.
    fn next_drained(&self, store: &Store) -> Result<Option<(String, staging::Identity)>> {
        for id in super::super::inputs::held_drained(&store.db, SWEEP)? {
            if self.holds(&id) {
                continue;
            }
            let record = super::super::inputs::load(&store.db, &id)?;
            if let Some(identity) = record.and_then(|r| r.identity) {
                return Ok(Some((id, identity)));
            }
        }
        Ok(None)
    }

    /// A reservation that nothing here holds: a failed preparation's, or one
    /// left by an earlier process.
    fn next_reserved(&self, store: &Store) -> Result<Option<String>> {
        Ok(
            super::super::inputs::in_state(&store.db, "reserved", SWEEP)?
                .into_iter()
                .find(|id| !self.holds(id)),
        )
    }
}
