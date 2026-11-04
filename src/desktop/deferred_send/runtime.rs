//! One fixed local deferred send executor (zerus-5wmh.66.4.4, slice 3),
//! independent of the UI lifetime. Native admission, the one original child
//! and the original resource drain are separate; only the drain, through the
//! CLI's own receipt, settles a send.
use super::child::{self, Ended};
use super::{consent::Consent, ledger};
use crate::desktop::creation_clone::lease::{Lease, Machine};
use crate::{
    config::Config,
    desktop::{
        custody, fleet,
        store::{HandoffLock, Location, Store},
        ui_authority,
    },
    platform,
};
use anyhow::{ensure, Context, Result};
use rusqlite::TransactionBehavior;
use serde_json::{json, Value};
#[cfg(target_os = "linux")]
use std::time::Instant;
use std::{
    collections::BTreeMap,
    sync::{atomic::Ordering, mpsc, Arc},
    thread::{self, JoinHandle},
    time::Duration,
};
#[cfg(target_os = "linux")]
use zerus_client_core::image::linux::Pin;
use zerus_client_core::{
    desktop::deferred_send::{self as wire, Phase, Scope},
    run::custody::InheritedLock,
};
use zerus_desktop_ledger::operations::{
    custody::{Prepared, Record},
    Ticket,
};

/// The refusal of every macOS original until a qualified child seam exists.
pub(super) const MAC_UNAVAILABLE: &str = "deferred_send_native_image_unavailable";
/// The whole send, the CLI's own lock wait included (G1 condition 6).
pub(super) const DEADLINE: Duration = Duration::from_secs(30);

/// Stands in for the one child in tests: it receives the plan, exactly the
/// arguments and stdin the child would be given, the custody lock and the
/// original's resources guard.
#[cfg(test)]
type Executor = Arc<
    dyn Fn(&wire::Plan, &[String], &[u8], &Arc<InheritedLock>, &Arc<dyn Send + Sync>) -> Ended
        + Send
        + Sync,
>;
/// The original's physical resources: the SAME custody lock and its consent,
/// which holds the lease and with it the serve-owner loan. Every original
/// pipe keeps a copy until its real end, so neither the custody drain nor a
/// takeover can pass while a descendant still holds one.
struct Original {
    _custody: Arc<InheritedLock>,
    _consent: Arc<Consent>,
}
enum Work {
    Preparing {
        ticket: Ticket,
        consent: Arc<Consent>,
        worker: JoinHandle<()>,
        result: mpsc::Receiver<Result<(Prepared, Arc<InheritedLock>)>>,
    },
    Running {
        ticket: Ticket,
        consent: Arc<Consent>,
        worker: JoinHandle<()>,
        result: mpsc::Receiver<Ended>,
        resources: Arc<Original>,
    },
    Draining {
        id: String,
        consent: Arc<Consent>,
    },
}
pub(in crate::desktop) struct Runtime {
    #[cfg(test)]
    executor: Option<Executor>,
    scope: Scope,
    location: Location,
    owner: Arc<platform::FileLock>,
    exclusion: Option<Arc<HandoffLock>>,
    machine: Option<Machine>,
    lease: Option<Arc<Lease>>,
    // Only an original native admission inserts here. Startup/import never does.
    admitted: BTreeMap<String, Arc<Consent>>,
    work: Option<Work>,
    stopping: bool,
    failed: bool,
    charges: Option<u64>,
    /// macOS refuses every original (see [`MAC_UNAVAILABLE`]).
    mac_fence: bool,
    /// This service's own running image, pinned once from the kernel's
    /// `/proc/self/exe`. Each child executes that pinned image; without it (a
    /// test image, or an image already gone) no child starts.
    #[cfg(target_os = "linux")]
    image: Option<Arc<Pin>>,
}
impl Runtime {
    pub fn new(config: &Config, epoch: &str, owner: Arc<platform::FileLock>) -> Result<Self> {
        let location = Location::new(config)?;
        Ok(Self {
            #[cfg(test)]
            executor: None,
            scope: Scope {
                configuration: location.configuration.clone(),
                service_epoch: epoch.into(),
            },
            location,
            owner,
            exclusion: None,
            machine: None,
            lease: None,
            admitted: BTreeMap::new(),
            work: None,
            stopping: false,
            failed: false,
            charges: None,
            mac_fence: cfg!(target_os = "macos"),
            #[cfg(target_os = "linux")]
            image: crate::self_exec::ensure()
                .and_then(|()| Pin::open(std::path::Path::new("/proc/self/exe")))
                .ok()
                .map(Arc::new),
        })
    }
    pub fn observe(&mut self, event: &fleet::FleetEvent) {
        let old = self.machine.clone();
        Machine::observe(&mut self.machine, event);
        if self.machine != old {
            self.invalidate();
        }
    }
    pub fn reject(&mut self, event: &fleet::FleetEvent) {
        let old = self.machine.clone();
        Machine::reject(&mut self.machine, event);
        if self.machine != old {
            self.invalidate();
        }
    }
    fn invalidate(&mut self) {
        if let Some(lease) = self.lease.take() {
            lease.invalidate();
        }
        for consent in self.admitted.values() {
            consent.stop();
        }
    }
    fn refresh(&mut self, store: &Store, exclusion: Option<Arc<HandoffLock>>) {
        if self.stopping {
            self.exclusion = None;
            self.invalidate();
            return;
        }
        self.exclusion = exclusion;
        if self.failed || !store.creation_recovery_ok() {
            self.invalidate();
            return;
        }
        if self.lease.as_ref().is_some_and(|lease| {
            lease
                .current(
                    &store.db,
                    &self.location,
                    self.machine.as_ref(),
                    self.exclusion.as_ref(),
                )
                .is_ok()
        }) {
            return;
        }
        self.invalidate();
        if let (Some(machine), Some(exclusion)) = (self.machine.clone(), self.exclusion.clone()) {
            self.lease = Lease::new(
                &store.db,
                self.scope.clone(),
                &self.location,
                machine,
                self.owner.clone(),
                exclusion,
            )
            .ok();
        }
    }
    fn refusal(&self, id: String, code: &str) -> Value {
        json!(wire::Admission {
            abi: wire::ABI.into(),
            request_id: id,
            scope: self.scope.clone(),
            accepted: false,
            durable: false,
            code: code.into(),
            receipt: None
        })
    }
    /// The one admission of an original from the native frontend lane. The
    /// permit is consumed after BEGIN IMMEDIATE, with the same lease.
    pub fn frontend(
        &mut self,
        store: &mut Store,
        authority: ui_authority::Validated,
        exclusion: Option<Arc<HandoffLock>>,
    ) -> Value {
        let id = authority.id().to_owned();
        // No qualified seam yet attests the code a macOS child runs before it
        // receives private input: refuse before any row or child.
        if self.mac_fence {
            return self.refusal(id, MAC_UNAVAILABLE);
        }
        self.refresh(store, exclusion);
        let result = (|| -> Result<wire::Admission> {
            ensure!(
                !self.stopping && !self.failed && store.creation_recovery_ok(),
                "deferred send owner unavailable"
            );
            ensure!(
                self.known_charges(&store.db)?,
                "deferred send original resources retained"
            );
            let lease = self
                .lease
                .as_ref()
                .context("deferred send lease unavailable")?
                .clone();
            let tx = store
                .db
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            // The original opaque permit, consumed AFTER the writer lock.
            let intent = authority.deferred_send()?;
            ensure!(
                intent.scope == self.scope,
                "deferred send connection changed"
            );
            let verified = intent.verify().map_err(anyhow::Error::msg)?;
            lease.current(
                &tx,
                &self.location,
                self.machine.as_ref(),
                self.exclusion.as_ref(),
            )?;
            let original = ledger::load(&tx, &id)?;
            let consent = Consent::admitted(verified, lease.clone())?;
            let row = ledger::admit(
                &tx,
                &consent.original,
                &lease.writer_epoch,
                chrono::Utc::now().timestamp_millis(),
            )?;
            tx.commit()?;
            // The same original again gets only its own receipt; only the
            // first admission holds live consent.
            if original.is_none() {
                self.admitted.insert(id.clone(), consent);
            }
            Ok(wire::Admission {
                abi: wire::ABI.into(),
                request_id: id.clone(),
                scope: self.scope.clone(),
                accepted: true,
                durable: true,
                code: String::new(),
                receipt: Some(row.receipt),
            })
        })();
        match result {
            Ok(value) => json!(value),
            Err(_) => self.refusal(id, "deferred_send_unavailable"),
        }
    }
    fn check(
        &self,
        db: &rusqlite::Connection,
        consent: &Consent,
        row: &ledger::Row,
        ticket: Option<&Ticket>,
    ) -> Result<()> {
        ensure!(
            !self.stopping && !self.failed,
            "deferred send owner stopping"
        );
        ensure!(
            self.lease
                .as_ref()
                .is_some_and(|lease| Arc::ptr_eq(lease, &consent.lease)),
            "deferred send lease replaced"
        );
        consent.lease.current(
            db,
            &self.location,
            self.machine.as_ref(),
            self.exclusion.as_ref(),
        )?;
        consent.check(row, ticket)
    }
    pub fn poll(&mut self, store: &mut Store, exclusion: Option<Arc<HandoffLock>>) {
        // Also advance exact original custody inside a bounded drain turn;
        // takeover must not depend on returning to the outer serve loop.
        store.recover_operation_custody(&self.location);
        self.refresh(store, exclusion);
        if self.poll_inner(store).is_err() {
            self.failed = true;
            self.invalidate();
        }
        self.charges = ledger::undrained(&store.db).ok();
    }
    fn poll_inner(&mut self, store: &mut Store) -> Result<()> {
        if let Some(work) = self.work.take() {
            match work {
                Work::Preparing {
                    ticket,
                    consent,
                    worker,
                    result,
                } => {
                    if !worker.is_finished() {
                        self.work = Some(Work::Preparing {
                            ticket,
                            consent,
                            worker,
                            result,
                        });
                        return Ok(());
                    }
                    let prepared = worker
                        .join()
                        .ok()
                        .and_then(|_| result.try_recv().ok())
                        .and_then(Result::ok);
                    let Some((prepared, held)) = prepared else {
                        self.retire(store, &ticket.admission_id)?;
                        return Ok(());
                    };
                    let record = Record {
                        prepared,
                        helper: None,
                    };
                    let reserved = (|| -> Result<()> {
                        let tx = store
                            .db
                            .transaction_with_behavior(TransactionBehavior::Immediate)?;
                        let row = ledger::load(&tx, &ticket.admission_id)?
                            .context("deferred send disappeared")?;
                        self.check(&tx, &consent, &row, None)?;
                        ensure!(
                            ticket.lease == consent.lease.id,
                            "deferred send lease changed"
                        );
                        ledger::reserve(&tx, &row, &ticket, &record)?;
                        tx.commit()?;
                        Ok(())
                    })();
                    if reserved.is_err() {
                        drop(held);
                        self.retire(store, &ticket.admission_id)?;
                        return Ok(());
                    }
                    let (done_send, result) = mpsc::sync_channel(1);
                    let selected = consent.clone();
                    let lock = held.clone();
                    let resources = Arc::new(Original {
                        _custody: held,
                        _consent: consent.clone(),
                    });
                    let guard: Arc<dyn Send + Sync> = resources.clone();
                    #[cfg(test)]
                    let executor = self.executor.clone();
                    #[cfg(target_os = "linux")]
                    let image = self.image.clone();
                    let spawned =
                        thread::Builder::new()
                            .name("deferred-send".into())
                            .spawn(move || {
                                let value = if selected.cancel.load(Ordering::Acquire) {
                                    Ended::NotStarted
                                } else {
                                    send(
                                        &selected,
                                        &lock,
                                        &guard,
                                        #[cfg(target_os = "linux")]
                                        image,
                                        #[cfg(test)]
                                        executor,
                                    )
                                };
                                let _ = done_send.send(value);
                            });
                    match spawned {
                        Ok(worker) => {
                            self.work = Some(Work::Running {
                                ticket,
                                consent,
                                worker,
                                result,
                                resources,
                            })
                        }
                        Err(_) => {
                            drop(resources);
                            self.retire(store, &ticket.admission_id)?;
                        }
                    }
                }
                Work::Running {
                    ticket,
                    consent,
                    worker,
                    result,
                    resources,
                } => {
                    if !worker.is_finished() {
                        self.work = Some(Work::Running {
                            ticket,
                            consent,
                            worker,
                            result,
                            resources,
                        });
                        return Ok(());
                    }
                    let ended = worker
                        .join()
                        .ok()
                        .and_then(|_| result.try_recv().ok())
                        .unwrap_or(Ended::Unknown("deferred_send_unobserved"));
                    let tx = store
                        .db
                        .transaction_with_behavior(TransactionBehavior::Immediate)?;
                    let row = ledger::load(&tx, &ticket.admission_id)?
                        .context("deferred send disappeared")?;
                    ensure!(
                        row.ticket.as_ref() == Some(&ticket),
                        "deferred send original ticket changed"
                    );
                    match ended {
                        // Durable before the drain, which alone settles it.
                        Ended::NotStarted => ledger::not_started(&tx, &row)?,
                        // The exit proves nothing; the CLI's receipt, read
                        // at the drain, decides.
                        Ended::Exited { .. } => ledger::ended(&tx, &row, ledger::AWAITING_DRAIN)?,
                        Ended::Unknown(code) => ledger::ended(&tx, &row, code)?,
                    }
                    tx.commit()?;
                    // Release this copy only after the worker joined: an
                    // original pipe that is still open keeps its own copy
                    // until its real end, and every copy holds the custody
                    // lock, so the drain waits for all of them.
                    drop(resources);
                    self.work = Some(Work::Draining {
                        id: ticket.admission_id,
                        consent,
                    });
                }
                Work::Draining { id, consent } => {
                    let row = ledger::load(&store.db, &id)?.context("deferred send disappeared")?;
                    if row.receipt.drained {
                        self.admitted.remove(&id);
                    } else {
                        self.work = Some(Work::Draining { id, consent });
                    }
                }
            }
            return Ok(());
        }
        let cancelled = self
            .admitted
            .iter()
            .find_map(|(id, c)| c.cancel.load(Ordering::Acquire).then_some(id.clone()));
        if let Some(id) = cancelled {
            self.retire(store, &id)?;
            return Ok(());
        }
        if self.stopping || self.failed {
            self.lease = None;
            self.exclusion = None;
            return Ok(());
        }
        if !self.known_charges(&store.db)? {
            return Ok(());
        }
        let Some(row) = ledger::pending(&store.db)? else {
            return Ok(());
        };
        if row.receipt.phase != Phase::Queued {
            return Ok(());
        }
        let id = row.verified.plan.request_id.clone();
        let Some(consent) = self.admitted.get(&id).cloned() else {
            self.retire(store, &id)?;
            return Ok(());
        };
        self.check(&store.db, &consent, &row, None)?;
        let ticket = Ticket {
            operation_id: row
                .verified
                .binding
                .operation_id()
                .context("invalid send key")?,
            attempt_id: uuid::Uuid::new_v4().to_string(),
            revision: row.receipt.revision + 1,
            admission_id: id,
            binding_sha256: row
                .verified
                .binding
                .fingerprint()
                .context("invalid send binding")?,
            writer_epoch: consent.lease.writer_epoch.clone(),
            service_epoch: self.scope.service_epoch.clone(),
            lease: consent.lease.id.clone(),
        };
        let original = ticket.clone();
        let held = consent.clone();
        let location = Location {
            root: self.location.root.clone(),
            configuration: self.location.configuration.clone(),
        };
        let (send, result) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("deferred-send-prepare".into())
            .spawn(move || {
                let _original = held;
                let _ = send.send(custody::prepare(&location, &original));
            })?;
        self.work = Some(Work::Preparing {
            ticket,
            consent,
            worker,
            result,
        });
        Ok(())
    }
    fn retire(&mut self, store: &mut Store, id: &str) -> Result<()> {
        let tx = store
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let row = ledger::load(&tx, id)?.context("deferred send disappeared")?;
        if !row.receipt.drained {
            ledger::retire(&tx, &row)?;
        }
        tx.commit()?;
        self.admitted.remove(id);
        Ok(())
    }
    pub fn request_stop(&mut self) {
        self.stopping = true;
        self.invalidate();
        self.exclusion = None;
    }
    fn known_charges(&self, db: &rusqlite::Connection) -> Result<bool> {
        Ok(ledger::charged_ids(db)?
            .iter()
            .all(|id| self.admitted.contains_key(id)))
    }
    pub fn drained(&self) -> bool {
        self.stopping && self.work.is_none() && self.admitted.is_empty() && self.charges == Some(0)
    }
    pub fn status(&self) -> Value {
        json!({"profile":wire::PROFILE,"ready":!self.stopping&&!self.failed&&self.lease.is_some(),
            "code":if self.failed {"deferred_send_owner_unavailable"}else if self.drained(){"drained"}else if self.stopping{"draining"}else if self.lease.is_some(){"running"}else{"awaiting_handoff"},
            "drained":self.drained(),"admitted":self.admitted.len(),"working":self.work.is_some(),"undrained":self.charges})
    }
    #[cfg(test)]
    pub(super) fn fixture_executor(&mut self, machine: &str, executor: Executor) {
        self.machine = Some(Machine::fixture(machine));
        self.executor = Some(executor);
    }
    /// The fixture source without an executor: the original path runs, and a
    /// test image's own-image refusal stops it before any child.
    #[cfg(test)]
    pub(super) fn fixture_machine(&mut self, machine: &str) {
        self.machine = Some(Machine::fixture(machine));
    }
    #[cfg(test)]
    pub(super) fn fixture_mac_fence(&mut self, on: bool) {
        self.mac_fence = on;
    }
    #[cfg(all(test, target_os = "linux"))]
    pub(super) fn fixture_image(&mut self, path: &std::path::Path) {
        self.image = Some(Arc::new(Pin::open(path).unwrap()));
    }
}
/// The one original child: `hgs send <session> --json` with the frozen
/// stdin. Nothing exists before the spawn, so every refusal here never
/// started.
fn send(
    consent: &Consent,
    lock: &Arc<InheritedLock>,
    resources: &Arc<dyn Send + Sync>,
    #[cfg(target_os = "linux")] image: Option<Arc<Pin>>,
    #[cfg(test)] executor: Option<Executor>,
) -> Ended {
    let plan = &consent.original.plan;
    let args = plan.args().to_vec();
    let input = consent.original.intent.original.as_bytes();
    // The test executor receives exactly what the child would.
    #[cfg(test)]
    if let Some(executor) = executor {
        return executor(plan, &args, input, lock, resources);
    }
    #[cfg(target_os = "linux")]
    if let Some(image) = image {
        let deadline = Instant::now() + DEADLINE;
        return child::run(
            image.executable(),
            &args,
            input,
            lock,
            resources,
            deadline,
            |command| {
                // Last before the spawn: a consent stopped since the worker
                // began (a revoke, a lost source, a stop) starts nothing.
                if consent.cancel.load(Ordering::Acquire) || !image.current() {
                    return Err(std::io::ErrorKind::PermissionDenied.into());
                }
                image.inherit(command)
            },
        );
    }
    // No pinned image (a unit-test image, or one already gone), and macOS,
    // whose admission refuses first: nothing starts.
    let _ = (args, input, lock, resources);
    Ended::NotStarted
}
impl Drop for Runtime {
    fn drop(&mut self) {
        self.request_stop();
    }
}
