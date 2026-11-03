//! One explicitly admitted metadata reconciliation. No Start or input replay.
use super::{
    creation_clone::{Lease, Machine},
    custody,
    fanout::Queue,
    fleet, operations,
    store::{self, HandoffLock, Location, Store},
    ui_authority,
};
use crate::{config::Config, platform, state::task_reconcile::Original};
use anyhow::{ensure, Context, Result};
use rusqlite::{OptionalExtension, TransactionBehavior};
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Weak,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use zerus_client_core::{
    desktop::{creation_clone::Scope as LeaseScope, task_start_reconcile as wire},
    run::{custody::InheritedLock, run_owned, OwnedError},
    Invocation,
};
use zerus_desktop_ledger::operations::{
    self as ledger,
    custody::{Birth, Prepared},
    Ticket,
};

struct Consent {
    original: operations::VerifiedOriginal,
    executor: Original,
    lease: Arc<Lease>,
    cancel: AtomicBool,
}
struct Resources {
    _consent: Arc<Consent>,
    _lock: Arc<InheritedLock>,
}
struct Register {
    birth: Birth,
    reply: mpsc::SyncSender<bool>,
}
struct Pending {
    authority: ui_authority::Validated,
    verified: wire::Verified,
    original: operations::VerifiedOriginal,
    admitted: ledger::Admitted,
    ticket: Ticket,
    lease: Arc<Lease>,
    queue: Arc<Queue>,
    seq: u64,
    worker: JoinHandle<()>,
    ready: mpsc::Receiver<Result<(Original, Prepared, Arc<InheritedLock>)>>,
}
enum Work {
    Preparing(Box<Pending>),
    Running {
        ticket: Ticket,
        consent: Arc<Consent>,
        worker: JoinHandle<()>,
        register: mpsc::Receiver<Register>,
        result: mpsc::Receiver<Result<Vec<u8>, OwnedError>>,
        resources: Weak<Resources>,
    },
    Draining {
        ticket: Ticket,
        consent: Arc<Consent>,
        resources: Weak<Resources>,
    },
}
pub(super) struct Runtime {
    #[cfg(test)]
    image: Option<std::path::PathBuf>,
    #[cfg(test)]
    input_gate: Option<Arc<AtomicBool>>,
    scope: wire::Scope,
    location: Location,
    owner: Arc<platform::FileLock>,
    exclusion: Option<Arc<HandoffLock>>,
    machine: Option<Machine>,
    lease: Option<Arc<Lease>>,
    work: Option<Work>,
    stopping: bool,
    failed: bool,
    charges: Option<u64>,
    environment: Vec<(String, String)>,
}
fn original_input(verified: &wire::Verified) -> Result<Vec<u8>> {
    use zerus_client_core::operation_recovery::task_start::{Decision, Input};
    let Decision::Proposed(proposal) = verified.original.step(Input::LookupRequested, false) else {
        anyhow::bail!("task Start original is not pending");
    };
    Ok(proposal.input().to_vec())
}
impl Runtime {
    pub fn new(config: &Config, epoch: &str, owner: Arc<platform::FileLock>) -> Result<Self> {
        let location = Location::new(config)?;
        Ok(Self {
            #[cfg(test)]
            image: None,
            #[cfg(test)]
            input_gate: None,
            scope: wire::Scope {
                configuration: location.configuration.clone(),
                service_epoch: epoch.into(),
            },
            location,
            owner,
            exclusion: None,
            machine: None,
            lease: None,
            work: None,
            stopping: false,
            failed: false,
            charges: None,
            environment: vec![
                ("HOME".into(), config.home.clone()),
                (
                    "HGS_CONFIG_DIR".into(),
                    config
                        .dir
                        .to_str()
                        .context("task Start configuration path unavailable")?
                        .into(),
                ),
                (
                    "HGS_STATE_DIR".into(),
                    config
                        .state
                        .to_str()
                        .context("task Start state path unavailable")?
                        .into(),
                ),
            ],
        })
    }
    pub fn observe(&mut self, event: &fleet::FleetEvent) {
        let old = self.machine.clone();
        Machine::observe(&mut self.machine, event);
        if old != self.machine {
            self.invalidate();
        }
    }
    pub fn reject(&mut self, event: &fleet::FleetEvent) {
        let old = self.machine.clone();
        Machine::reject(&mut self.machine, event);
        if old != self.machine {
            self.invalidate();
        }
    }
    fn invalidate(&mut self) {
        if let Some(lease) = self.lease.take() {
            lease.invalidate();
        }
        if let Some(Work::Running { consent, .. } | Work::Draining { consent, .. }) = &self.work {
            consent.cancel.store(true, Ordering::Release);
        }
    }
    fn refresh(&mut self, store: &Store, exclusion: Option<Arc<HandoffLock>>) {
        self.exclusion = exclusion;
        if self.stopping || self.failed || !store.operation_recovery_ok() {
            self.invalidate();
            return;
        }
        if self
            .lease
            .as_ref()
            .is_some_and(|lease| self.current(&store.db, lease).is_ok())
        {
            return;
        }
        self.invalidate();
        if let (Some(machine), Some(exclusion)) = (self.machine.clone(), self.exclusion.clone()) {
            self.lease = Lease::new(
                &store.db,
                LeaseScope {
                    configuration: self.scope.configuration.clone(),
                    service_epoch: self.scope.service_epoch.clone(),
                },
                &self.location,
                machine,
                self.owner.clone(),
                exclusion,
            )
            .ok();
        }
    }
    fn current(&self, db: &rusqlite::Connection, lease: &Arc<Lease>) -> Result<()> {
        ensure!(
            !self.stopping
                && !self.failed
                && self
                    .lease
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(current, lease)),
            "task Start owner unavailable"
        );
        lease.current(
            db,
            &self.location,
            self.machine.as_ref(),
            self.exclusion.as_ref(),
        )
    }
    fn reply(&self, queue: &Queue, seq: u64, value: Value) {
        let frame = super::frame(&self.scope.service_epoch, seq, "response", value);
        if let Ok(bytes) = super::protocol::encode(&frame) {
            queue.offer(bytes.into());
        }
        queue.close();
    }
    /// Retain the reply queue and original opaque permit while bounded native
    /// preparation runs off actor. No database mutation or helper starts here.
    pub fn frontend(
        &mut self,
        store: &mut Store,
        authority: ui_authority::Validated,
        exclusion: Option<Arc<HandoffLock>>,
        queue: Arc<Queue>,
        seq: u64,
    ) -> Result<()> {
        self.refresh(store, exclusion);
        let verified = authority
            .task_start_preview()?
            .clone()
            .verify()
            .map_err(anyhow::Error::msg)?;
        ensure!(
            verified.intent.scope == self.scope,
            "task Start service changed"
        );
        let observed: Option<(String, String, String)> = store
            .db
            .query_row(
                "SELECT ticket,state,fingerprint FROM desktop_operation_attempts WHERE attempt_id=?1",
                [&verified.intent.reconcile_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        if let Some((encoded, state, fingerprint)) = observed {
            ensure!(
                encoded.len() <= ledger::METADATA_BYTES,
                "task Start ticket too large"
            );
            let ticket: Ticket = serde_json::from_str(&encoded)?;
            ensure!(
                ticket.fingerprint()? == fingerprint,
                "task Start stored ticket changed"
            );
            let tx = store
                .db
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let intent = authority.task_start_reconcile()?;
            let frozen = ledger::load(&tx, &ticket.operation_id)?;
            ensure!(
                intent == verified.intent
                    && ticket.attempt_id == intent.reconcile_id
                    && ticket.admission_id == frozen.admitted.admission_id
                    && ticket.binding_sha256 == frozen.admitted.binding_sha256
                    && ticket.revision == frozen.admitted.revision
                    && frozen.binding == operations::to_ledger(&intent.binding)
                    && frozen.raw_record == intent.original.as_bytes(),
                "task Start lookup changed"
            );
            tx.commit()?;
            self.reply(
                &queue,
                seq,
                json!({"request_id":ticket.attempt_id,"accepted":true,"durable":true,
                "read_only":true,"code":"original_attempt_retained","state":state}),
            );
            return Ok(());
        }
        ensure!(
            self.work.is_none() && store.operation_undrained()? == 0,
            "task Start original resources retained"
        );
        ensure!(
            verified.intent.scope == self.scope
                && self
                    .machine
                    .as_ref()
                    .is_some_and(|m| m.id == verified.intent.binding.executor.machine),
            "task Start source changed"
        );
        let lease = self
            .lease
            .as_ref()
            .context("task Start lease missing")?
            .clone();
        self.current(&store.db, &lease)?;
        let id = operations::to_ledger(&verified.intent.binding)
            .key
            .operation_id()?;
        let exists: bool = store.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM desktop_operations WHERE operation_id=?1)",
            [&id],
            |r| r.get(0),
        )?;
        let (original, admitted) = if exists {
            let frozen = ledger::load(&store.db, &id)?;
            ensure!(
                frozen.binding == operations::to_ledger(&verified.intent.binding)
                    && frozen.raw_record == verified.intent.original.as_bytes(),
                "task Start original changed"
            );
            let original = operations::VerifiedOriginal::verify(
                verified.intent.binding.clone(),
                &frozen.raw_record,
                &frozen.retained_input,
                &frozen.provenance,
            )
            .map_err(|_| anyhow::anyhow!("task Start frozen original invalid"))?;
            (original, frozen.admitted)
        } else {
            let input = original_input(&verified)?;
            let original = operations::VerifiedOriginal::verify(
                verified.intent.binding.clone(),
                verified.intent.original.as_bytes(),
                &input,
                &json!({"profile":wire::PROFILE}),
            )
            .map_err(|_| anyhow::anyhow!("task Start original invalid"))?;
            let admission = original
                .admission(
                    &verified.intent.reconcile_id,
                    &lease.writer_epoch,
                    &self.scope.service_epoch,
                )
                .map_err(|_| anyhow::anyhow!("task Start admission invalid"))?;
            let admitted = ledger::Admitted {
                operation_id: id,
                admission_id: verified.intent.reconcile_id.clone(),
                binding_sha256: operations::hash(&serde_json::to_vec(admission.binding)?),
                revision: 1,
            };
            (original, admitted)
        };
        let ticket = Ticket {
            operation_id: admitted.operation_id.clone(),
            attempt_id: verified.intent.reconcile_id.clone(),
            revision: admitted.revision,
            admission_id: admitted.admission_id.clone(),
            binding_sha256: admitted.binding_sha256.clone(),
            writer_epoch: lease.writer_epoch.clone(),
            service_epoch: self.scope.service_epoch.clone(),
            lease: lease.id.clone(),
        };
        let location = Location {
            root: self.location.root.clone(),
            configuration: self.location.configuration.clone(),
        };
        let preparing = ticket.clone();
        let retained = lease.clone();
        let input = original_input(&verified)?;
        let executor = verified.intent.binding.executor.clone();
        let (send, ready) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("task-start-prepare".into())
            .spawn(move || {
                let _retained = retained;
                let result = (|| {
                    let executor = Original::capture(
                        location
                            .root
                            .parent()
                            .context("task Start state unavailable")?,
                        &executor,
                        &input,
                        Instant::now() + Duration::from_secs(2),
                    )?;
                    let (prepared, lock) = custody::prepare(&location, &preparing)?;
                    Ok((executor, prepared, lock))
                })();
                let _ = send.send(result);
            })?;
        self.work = Some(Work::Preparing(Box::new(Pending {
            authority,
            verified,
            original,
            admitted,
            ticket,
            lease,
            queue,
            seq,
            worker,
            ready,
        })));
        Ok(())
    }
    pub fn poll(&mut self, store: &mut Store, exclusion: Option<Arc<HandoffLock>>) {
        store.recover_operation_custody(&self.location);
        self.refresh(store, exclusion);
        if self.poll_inner(store).is_err() {
            self.failed = true;
            self.invalidate();
        }
        self.charges = ledger::totals(&store.db).ok().map(|v| v.undrained);
        if self.stopping && self.work.is_none() {
            self.lease = None;
            self.exclusion = None;
        }
    }
    fn poll_inner(&mut self, store: &mut Store) -> Result<()> {
        let Some(work) = self.work.take() else {
            return Ok(());
        };
        match work {
            Work::Preparing(pending) => {
                if !pending.worker.is_finished() {
                    self.work = Some(Work::Preparing(pending));
                    return Ok(());
                }
                let Pending {
                    authority,
                    verified,
                    original,
                    admitted,
                    ticket,
                    lease,
                    queue,
                    seq,
                    worker,
                    ready,
                } = *pending;
                let prepared = worker
                    .join()
                    .ok()
                    .and_then(|_| ready.try_recv().ok())
                    .and_then(Result::ok);
                let reservation = (|| -> Result<_> {
                    let (executor, prepared, lock) =
                        prepared.context("task Start preparation unavailable")?;
                    let tx = store
                        .db
                        .transaction_with_behavior(TransactionBehavior::Immediate)?;
                    let intent = authority.task_start_reconcile()?;
                    ensure!(
                        intent == verified.intent,
                        "task Start original request changed"
                    );
                    self.current(&tx, &lease)?;
                    executor.current(Instant::now() + Duration::from_secs(2))?;
                    ensure!(
                        store::operation_undrained(&tx)? == 0,
                        "task Start resources pending"
                    );
                    let frozen = ledger::admit_in(
                        &tx,
                        &original
                            .admission(
                                &admitted.admission_id,
                                &lease.writer_epoch,
                                &self.scope.service_epoch,
                            )
                            .map_err(|_| anyhow::anyhow!("task Start admission changed"))?,
                    )?;
                    ensure!(frozen == admitted, "task Start admission changed");
                    let reserved = ledger::custody::reserve_in(
                        &tx,
                        &ledger::Reserve {
                            operation_id: ticket.operation_id.clone(),
                            attempt_id: ticket.attempt_id.clone(),
                            revision: ticket.revision,
                            writer_epoch: ticket.writer_epoch.clone(),
                            service_epoch: ticket.service_epoch.clone(),
                            lease: ticket.lease.clone(),
                        },
                        &prepared,
                    )?;
                    ensure!(
                        reserved.fresh && reserved.attempt.ticket == ticket,
                        "task Start original attempt already reserved"
                    );
                    tx.commit()?;
                    Ok((executor, lock))
                })();
                match reservation {
                    Ok((executor,lock))=>{
                        let consent=Arc::new(Consent {original,executor,lease,cancel:AtomicBool::new(false)});
                        self.reply(&queue,seq,json!({"request_id":ticket.attempt_id,"accepted":true,"durable":true,"code":"reserved","ticket":ticket}));
                        self.spawn(store,ticket,consent,lock)?;
                    },
                    Err(_)=>self.reply(&queue,seq,json!({"request_id":ticket.attempt_id,"accepted":false,"durable":false,"code":"task_start_reconcile_unavailable"})),
                }
            }
            Work::Running {
                ticket,
                consent,
                worker,
                register,
                result,
                resources,
            } => {
                if let Ok(request) = register.try_recv() {
                    let accepted = (|| -> Result<()> {
                        let tx = store
                            .db
                            .transaction_with_behavior(TransactionBehavior::Immediate)?;
                        self.current(&tx, &consent.lease)?;
                        ensure!(
                            !consent.cancel.load(Ordering::Acquire),
                            "task Start cancelled"
                        );
                        consent
                            .original
                            .check_stored(
                                &tx,
                                &ledger::Admitted {
                                    operation_id: ticket.operation_id.clone(),
                                    admission_id: ticket.admission_id.clone(),
                                    binding_sha256: ticket.binding_sha256.clone(),
                                    revision: ticket.revision,
                                },
                            )
                            .map_err(|_| anyhow::anyhow!("task Start original changed"))?;
                        consent
                            .executor
                            .current(Instant::now() + Duration::from_secs(2))?;
                        tx.commit()?;
                        ledger::custody::register(&mut store.db, &ticket, &request.birth)?;
                        // Final actor fence before the same Child is returned to
                        // the runner's stdin writer. No decoded bool substitutes
                        // for these retained objects or the durable registration.
                        self.current(&store.db, &consent.lease)?;
                        consent
                            .executor
                            .current(Instant::now() + Duration::from_secs(2))?;
                        Ok(())
                    })()
                    .is_ok();
                    let _ = request.reply.send(accepted);
                }
                if !worker.is_finished() {
                    self.work = Some(Work::Running {
                        ticket,
                        consent,
                        worker,
                        register,
                        result,
                        resources,
                    });
                    return Ok(());
                }
                let value = worker.join().ok().and_then(|_| result.try_recv().ok());
                // Keep the original owner/exclusion loan even if settlement
                // fails. Kernel-custody recovery, not a result error, retires it.
                self.work = Some(Work::Draining {
                    ticket: ticket.clone(),
                    consent: consent.clone(),
                    resources: resources.clone(),
                });
                let loaded = operations::Loaded::load(
                    &store.db,
                    &ledger::load(&store.db, &ticket.operation_id)?.admitted,
                )
                .map_err(|_| anyhow::anyhow!("task Start original changed"))?;
                let state: String = store.db.query_row(
                    "SELECT state FROM desktop_operation_attempts WHERE attempt_id=?1",
                    [&ticket.attempt_id],
                    |r| r.get(0),
                )?;
                if state != "reserved" {
                    // Restart/uncertainty already settled this exact attempt.
                    // Its terminal bytes cannot be upgraded by a late reply.
                } else if let Some(Ok(bytes)) = value {
                    if loaded
                        .finish_submitted(&mut store.db, &ticket, &bytes)
                        .is_err()
                    {
                        ledger::finish(
                            &mut store.db,
                            &ticket,
                            &ledger::Outcome::Unknown {
                                reason: ledger::Unknown::LostResult,
                            },
                        )?;
                    }
                } else {
                    ledger::finish(
                        &mut store.db,
                        &ticket,
                        &ledger::Outcome::Unknown {
                            reason: if consent.cancel.load(Ordering::Acquire) {
                                ledger::Unknown::Cancelled
                            } else {
                                ledger::Unknown::LostResult
                            },
                        },
                    )?;
                }
            }
            Work::Draining {
                ticket,
                consent,
                resources,
            } => {
                let drained = store.db.query_row(
                    "SELECT drained FROM desktop_operation_attempts WHERE attempt_id=?1",
                    [&ticket.attempt_id],
                    |r| r.get::<_, bool>(0),
                );
                let released = drained.as_ref().is_ok_and(|v| *v) && resources.upgrade().is_none();
                if !released {
                    self.work = Some(Work::Draining {
                        ticket,
                        consent,
                        resources,
                    });
                }
                drained?;
            }
        }
        Ok(())
    }
    fn spawn(
        &mut self,
        store: &mut Store,
        ticket: Ticket,
        consent: Arc<Consent>,
        lock: Arc<InheritedLock>,
    ) -> Result<()> {
        let prepare = (|| -> Result<_> {
            let loaded = operations::Loaded::load(
                &store.db,
                &ledger::load(&store.db, &ticket.operation_id)?.admitted,
            )
            .map_err(|_| anyhow::anyhow!("task Start frozen input unavailable"))?;
            let proposal = loaded
                .proposal(false)
                .map_err(|_| anyhow::anyhow!("task Start proposal unavailable"))?;
            let invocation = Invocation {
                args: proposal.args().to_vec(),
                stdin: Some(proposal.input().to_vec()),
                timeout: Duration::from_secs(30),
                request_id: None,
            };
            #[cfg(not(test))]
            let image = crate::self_exec::current()?;
            #[cfg(test)]
            let image = self.image.clone().context(crate::self_exec::TEST_IMAGE)?;
            Ok((invocation, image))
        })();
        let (invocation, image) = match prepare {
            Ok(value) => value,
            Err(_) => {
                self.work = Some(Work::Draining {
                    ticket: ticket.clone(),
                    consent,
                    resources: Weak::new(),
                });
                ledger::finish(
                    &mut store.db,
                    &ticket,
                    &ledger::Outcome::Unknown {
                        reason: ledger::Unknown::LostResult,
                    },
                )?;
                return Ok(());
            }
        };
        let environment = self.environment.clone();
        let resources = Arc::new(Resources {
            _consent: consent.clone(),
            _lock: lock.clone(),
        });
        let weak = Arc::downgrade(&resources);
        let (registration, register) = mpsc::sync_channel(1);
        let (done, result) = mpsc::sync_channel(1);
        let selected = consent.clone();
        #[cfg(test)]
        let input_gate = self.input_gate.clone();
        let worker = thread::Builder::new()
            .name("task-start-reconcile".into())
            .spawn(move || {
                let current = selected.clone();
                let env: Vec<_> = environment
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.as_str()))
                    .collect();
                let value = run_owned(
                    &image,
                    &invocation,
                    &env,
                    &selected.cancel,
                    move |command| {
                        if current.cancel.load(Ordering::Acquire)
                            || current
                                .executor
                                .current(Instant::now() + Duration::from_secs(2))
                                .is_err()
                        {
                            return Err(std::io::Error::other("task Start current guard refused"));
                        }
                        lock.inherit(command)?;
                        let mut child = platform::spawn_in_turn(command)?;
                        let bound = (|| -> Option<bool> {
                            let birth = platform::process_identity(child.id())?;
                            let (reply, receive) = mpsc::sync_channel(1);
                            registration
                                .send(Register {
                                    birth: Birth {
                                        pid: child.id(),
                                        birth: birth.birth,
                                    },
                                    reply,
                                })
                                .ok()?;
                            receive.recv_timeout(Duration::from_secs(2)).ok()
                        })()
                        .unwrap_or(false);
                        #[cfg(test)]
                        let bound = if let Some(gate) = input_gate {
                            let end = Instant::now() + Duration::from_secs(5);
                            while !gate.load(Ordering::Acquire)
                                && !current.cancel.load(Ordering::Acquire)
                                && Instant::now() < end
                            {
                                thread::sleep(Duration::from_millis(1));
                            }
                            bound && gate.load(Ordering::Acquire)
                        } else {
                            bound
                        };
                        if !bound
                            || current.cancel.load(Ordering::Acquire)
                            || current
                                .executor
                                .current(Instant::now() + Duration::from_secs(2))
                                .is_err()
                        {
                            drop(child.stdin.take());
                        }
                        // After spawn even a failed registration returns this SAME
                        // child for exact stop/wait. Missing stdin prevents delivery.
                        Ok(child)
                    },
                    resources,
                );
                let _ = done.send(value);
            });
        match worker {
            Ok(worker) => {
                self.work = Some(Work::Running {
                    ticket,
                    consent,
                    worker,
                    register,
                    result,
                    resources: weak,
                })
            }
            Err(_) => {
                self.work = Some(Work::Draining {
                    ticket: ticket.clone(),
                    consent,
                    resources: weak,
                });
                ledger::finish(
                    &mut store.db,
                    &ticket,
                    &ledger::Outcome::Unknown {
                        reason: ledger::Unknown::LostResult,
                    },
                )?;
            }
        }
        Ok(())
    }
    pub fn request_stop(&mut self) {
        self.stopping = true;
        self.invalidate();
    }
    pub fn drained(&self) -> bool {
        self.stopping && self.work.is_none() && self.charges == Some(0)
    }
    pub fn status(&self) -> Value {
        json!({"profile":wire::PROFILE,"ready":!self.stopping&&!self.failed&&self.lease.is_some(),
        "working":self.work.is_some(),"undrained":self.charges,"drained":self.drained(),"failed":self.failed})
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        self.request_stop();
        if let Some(work) = self.work.take() {
            match work {
                // An unadmitted preparation has no helper or effect. Its own
                // thread retains the lease through any unfinished native I/O.
                Work::Preparing(_) => (),
                // An unexpected actor drop is not physical drain. Keep the
                // original owner/exclusion loan until this host exits; normal
                // drain polling retires Work first and never takes this path.
                work => std::mem::forget(work),
            }
        }
    }
}

#[cfg(test)]
mod tests;
