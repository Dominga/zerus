//! The explicit review and release of charged custody inputs (zerus-pi2k9,
//! under G1's conditions).
//!
//! - The listing is read-only: one bounded page of `orphaned` and `failed`
//!   records, read on the actor, whose slots are observed off it.
//! - A release is one native action. On the actor, one short transaction
//!   rechecks its permit after BEGIN IMMEDIATE with the same Service,
//!   configuration and lease, checks the exact record (and, for `failed`, its
//!   drained original), and claims it in the one removal slot, which excludes
//!   the sweep, admission and a second review. A bounded worker that retains
//!   the lease, and with it the serve-owner loan, removes the exact reviewed
//!   object or vetoes an accounting release. Only after that worker joined
//!   does one more transaction record the observed effect and release the
//!   charge, once; it needs no new authority.
//! - Nothing is retried or replayed; an unknown effect needs a fresh review.
use super::*;
use crate::desktop::fanout::Queue;
use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::time::Instant;
use zerus_client_core::desktop::task_message::review::{
    self as wire_review, Cursor, Entry, List, Outcome, Page, Release, Released, Version,
};

/// Listings off the actor at once.
const LISTINGS: usize = 2;
static LISTING: AtomicUsize = AtomicUsize::new(0);

/// Another review or removal holds the slot.
const BUSY: &str = "task_message_input_review_busy";
/// The record, its state or its original is not what the review saw.
const REFUSED: &str = "task_message_input_review_refused";
/// The worker ended without an observed effect.
const LOST: &str = "task_message_input_review_lost";

/// One claimed review, held in the removal slot until its worker joins.
pub(in crate::desktop::task_message) struct Review {
    release: Release,
    queue: Arc<Queue>,
    seq: u64,
    worker: JoinHandle<()>,
    effect: mpsc::Receiver<staging::Effect>,
    /// The review's fence: a stop, revoke or lease replacement.
    stopped: Arc<AtomicBool>,
    /// Retained until the worker joined, with the serve-owner loan.
    _lease: Arc<Lease>,
}
impl Review {
    pub(super) fn request_id(&self) -> &str {
        &self.release.request_id
    }
    pub(super) fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
    }
}

/// Frames and offers one reply, then closes the connection's queue.
fn answer_on(epoch: &str, queue: &Queue, seq: u64, value: Value) {
    let frame = crate::desktop::frame(epoch, seq, "response", value);
    if let Ok(bytes) = crate::desktop::protocol::encode(&frame) {
        queue.offer(bytes.into());
    }
    queue.close();
}

impl Runtime {
    fn page_refusal(&self, code: &str) -> Value {
        json!(Page {
            abi: wire::ABI.into(),
            scope: self.scope.clone(),
            read_only: true,
            entries: Vec::new(),
            next: None,
            code: code.into(),
        })
    }
    fn released(&self, release: &Release, outcome: Outcome, code: &str) -> Value {
        json!(Released {
            abi: wire::ABI.into(),
            release_id: release.release_id.clone(),
            request_id: release.request_id.clone(),
            scope: self.scope.clone(),
            outcome,
            code: code.into(),
        })
    }

    /// One read-only listing page. The records are read here; their slots are
    /// observed off the actor, and the page follows on `queue`.
    pub fn task_message_inputs(
        &mut self,
        store: &Store,
        request: &Value,
        queue: Arc<Queue>,
        seq: u64,
    ) {
        let epoch = self.scope.service_epoch.clone();
        let list = match serde_json::from_value::<List>(request.clone()) {
            Ok(list) if list.valid() && list.scope == self.scope => list,
            _ => {
                let value = self.page_refusal("task_message_inputs_unavailable");
                return answer_on(&epoch, &queue, seq, value);
            }
        };
        let Some(roots) = self.roots.clone() else {
            let value = self.page_refusal("task_message_staging_unavailable");
            return answer_on(&epoch, &queue, seq, value);
        };
        if LISTING.fetch_add(1, Ordering::AcqRel) >= LISTINGS {
            LISTING.fetch_sub(1, Ordering::AcqRel);
            let value = self.page_refusal(BUSY);
            return answer_on(&epoch, &queue, seq, value);
        }
        let after = list
            .after
            .as_ref()
            .map(|cursor| (cursor.created, cursor.request_id.as_str()));
        let rows = match super::super::inputs::reviewable(
            &store.db,
            after,
            wire_review::PAGE as u32 + 1,
        ) {
            Ok(rows) => rows,
            Err(_) => {
                LISTING.fetch_sub(1, Ordering::AcqRel);
                let value = self.page_refusal("task_message_inputs_unavailable");
                return answer_on(&epoch, &queue, seq, value);
            }
        };
        let scope = self.scope.clone();
        let spawned = thread::Builder::new()
            .name("task-message-review".into())
            .spawn(move || {
                let more = rows.len() > wire_review::PAGE;
                let entries: Vec<Entry> = rows
                    .into_iter()
                    .take(wire_review::PAGE)
                    .map(|row| Entry {
                        observed: staging::observe(&roots, &row.request_id),
                        recorded: (row.state == "failed").then_some(row.identity).flatten(),
                        request_id: row.request_id,
                        state: row.state,
                        bytes: row.bytes,
                        input_sha256: row.input_sha256,
                        created: row.created,
                    })
                    .collect();
                let next = more.then(|| {
                    let last = entries.last().expect("a full page");
                    Cursor {
                        created: last.created,
                        request_id: last.request_id.clone(),
                    }
                });
                let page = Page {
                    abi: wire::ABI.into(),
                    scope,
                    read_only: true,
                    entries,
                    next,
                    code: String::new(),
                };
                answer_on(&epoch, &queue, seq, json!(page));
                LISTING.fetch_sub(1, Ordering::AcqRel);
            });
        if spawned.is_err() {
            LISTING.fetch_sub(1, Ordering::AcqRel);
        }
    }

    /// One explicit release from the native frontend lane. Its reply follows
    /// on `queue` once its worker joined and its effect was recorded.
    pub fn task_message_input_release(
        &mut self,
        store: &mut Store,
        authority: ui_authority::Validated,
        exclusion: Option<Arc<HandoffLock>>,
        queue: Arc<Queue>,
        seq: u64,
    ) {
        let epoch = self.scope.service_epoch.clone();
        let Ok(release) = authority.task_message_input_release_preview().cloned() else {
            return answer_on(
                &epoch,
                &queue,
                seq,
                json!({"abi": wire::ABI, "read_only": false, "code": REFUSED}),
            );
        };
        if self.mac_fence {
            let value = self.released(&release, Outcome::Kept, MAC_UNAVAILABLE);
            return answer_on(&epoch, &queue, seq, value);
        }
        self.refresh(store, exclusion);
        match self.claim_review(store, authority, &release, &queue, seq) {
            Ok(None) => {}
            Ok(Some(code)) => {
                let value = self.released(&release, Outcome::Kept, code);
                answer_on(&epoch, &queue, seq, value)
            }
            Err(_) => {
                let value = self.released(&release, Outcome::Kept, REFUSED);
                answer_on(&epoch, &queue, seq, value)
            }
        }
    }

    /// Phase A: the one exact claim, its permit consumed after BEGIN.
    /// `Some(code)` refuses before anything was claimed.
    fn claim_review(
        &mut self,
        store: &mut Store,
        authority: ui_authority::Validated,
        release: &Release,
        queue: &Arc<Queue>,
        seq: u64,
    ) -> Result<Option<&'static str>> {
        ensure!(
            !self.stopping && !self.failed && store.creation_recovery_ok(),
            "task message owner unavailable"
        );
        ensure!(
            release.scope == self.scope && release.valid(),
            "task message input release changed"
        );
        if self.removal.is_some() || self.holds(&release.request_id) {
            return Ok(Some(BUSY));
        }
        let roots = self
            .roots
            .clone()
            .context("task message staging unavailable")?;
        let lease = self
            .lease
            .clone()
            .context("task message lease unavailable")?;
        let tx = store
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        // The original permit, consumed AFTER the writer lock: an expired one
        // refuses, and nothing renews it.
        let reviewed = authority.task_message_input_release()?;
        ensure!(reviewed == *release, "task message input release changed");
        lease.current(
            &tx,
            &self.location,
            self.machine.as_ref(),
            self.exclusion.as_ref(),
        )?;
        let Some(record) = super::super::inputs::reviewable_one(&tx, &release.request_id)? else {
            return Ok(Some(REFUSED));
        };
        if record.state != release.state
            || record.bytes != release.bytes
            || record.input_sha256 != release.input_sha256
        {
            return Ok(Some(staging::REVIEW_CHANGED));
        }
        let original = ledger::load(&tx, &release.request_id)?;
        let eligible = match record.state.as_str() {
            // Published, never admitted: still no execution row.
            "orphaned" => original.is_none(),
            // Its original drained exactly; a present object is the
            // recorded one.
            "failed" => {
                original.is_some_and(|row| row.receipt.drained)
                    && release
                        .observed
                        .is_none_or(|v| record.identity == Some((v.device, v.inode)))
            }
            _ => false,
        };
        if !eligible {
            return Ok(Some(REFUSED));
        }
        tx.commit()?;
        let stopped = Arc::new(AtomicBool::new(false));
        let (send, effect) = mpsc::sync_channel(1);
        let fence = (stopped.clone(), lease.clone(), self.owner.clone());
        let work = release.clone();
        let worker = thread::Builder::new()
            .name("task-message-review".into())
            .spawn(move || {
                // The lease and this service's own exclusion stay with the
                // worker until it finished.
                let (stopped, retained, _owner) = fence;
                let current = || !stopped.load(Ordering::Acquire) && retained.alive();
                let effect = match work.observed {
                    Some(version) => staging::remove_reviewed(
                        &roots,
                        &work.request_id,
                        version,
                        work.bytes,
                        &work.input_sha256,
                        &current,
                        Instant::now() + staging::REMOVAL_TIME,
                    ),
                    None => staging::release_reviewed(
                        &roots,
                        &work.request_id,
                        work.bytes,
                        &work.input_sha256,
                        &current,
                    ),
                };
                let _ = send.send(effect);
            })?;
        self.removal = Some(Slot::Review(Box::new(Review {
            release: release.clone(),
            queue: queue.clone(),
            seq,
            worker,
            effect,
            stopped,
            _lease: lease,
        })));
        Ok(None)
    }

    /// Phase C: after the worker joined, record exactly its observed effect,
    /// once, then reply. A review whose worker still runs keeps the slot.
    pub(super) fn poll_review(&mut self, store: &mut Store, review: Box<Review>) -> Result<()> {
        if !review.worker.is_finished() {
            self.removal = Some(Slot::Review(review));
            return Ok(());
        }
        let Review {
            release,
            queue,
            seq,
            worker,
            effect,
            ..
        } = *review;
        let effect = match worker.join() {
            Ok(()) => effect.try_recv().unwrap_or(staging::Effect::Unknown(LOST)),
            Err(_) => staging::Effect::Unknown(LOST),
        };
        let recorded = |store: &mut Store, removed: Option<Version>| -> Result<()> {
            let tx = store
                .db
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            match removed {
                Some(version) => super::super::inputs::reviewed_removed(
                    &tx,
                    &release.request_id,
                    &release.state,
                    (version.device, version.inode),
                )?,
                None => super::super::inputs::reviewed_released(
                    &tx,
                    &release.request_id,
                    &release.state,
                )?,
            }
            tx.commit()?;
            Ok(())
        };
        let (outcome, code, result) = match effect {
            staging::Effect::Removed(identity) => {
                let version = release.observed.filter(|v| (v.device, v.inode) == identity);
                match version {
                    Some(version) => match recorded(store, Some(version)) {
                        Ok(()) => (Outcome::Removed, "", Ok(())),
                        Err(error) => (Outcome::Unknown, LOST, Err(error)),
                    },
                    None => (Outcome::Unknown, LOST, Ok(())),
                }
            }
            staging::Effect::Gone => match recorded(store, None) {
                Ok(()) => (Outcome::Released, "", Ok(())),
                Err(error) => (Outcome::Unknown, LOST, Err(error)),
            },
            staging::Effect::Kept(code) => (Outcome::Kept, code, Ok(())),
            staging::Effect::Unknown(code) => (Outcome::Unknown, code, Ok(())),
        };
        let value = self.released(&release, outcome, code);
        answer_on(&self.scope.service_epoch, &queue, seq, value);
        result
    }

    /// Whether a claimed review's worker has finished; `None` without one.
    #[cfg(test)]
    pub(in crate::desktop::task_message) fn review_finished(&self) -> Option<bool> {
        match &self.removal {
            Some(Slot::Review(review)) => Some(review.worker.is_finished()),
            _ => None,
        }
    }
}
