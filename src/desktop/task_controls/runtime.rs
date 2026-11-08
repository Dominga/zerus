//! The sole service actor owns every SQLite operation. Pre-proof claims live
//! only in bounded memory. No native worker or pending request is replayed.
use super::{
    authority, guard,
    server::{self, Incoming},
    stop_import,
    wire::Envelope,
    worker,
};
use crate::{
    config::Config,
    desktop::{
        ownership,
        store::{HandoffLock, Location, Store},
    },
    platform,
    tasks::controls::wire::{Command, Plan, StatusScope},
};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{collections::VecDeque, sync::Arc, thread::JoinHandle, time::Instant};
use zerus_desktop_ledger::task_controls::{self as ledger, Control, Outcome};

const LIMIT: usize = crate::desktop::protocol::PENDING;
const CLOSED: &str = "Task-control effects are not qualified. Refused before any native effect.";

/// These are original resource pins, not actor or Person authority. The held
/// peer/native proof remains independent of service ownership/custody.
pub(in crate::desktop) struct Custody {
    pub service: Arc<platform::FileLock>,
    pub handoff: Arc<HandoffLock>,
    pub grant: ownership::Grant,
}
impl Custody {
    fn verify(&self, location: &Location, db: &rusqlite::Connection) -> Result<()> {
        self.service.identity()?;
        self.handoff.verify(location)?;
        self.handoff.identity()?;
        ensure!(
            self.grant.current_connection(db)?,
            "task-control service ownership changed"
        );
        self.grant.revalidate_image()
    }
}
struct Immediate {
    incoming: Incoming,
    value: Value,
}
struct Rejected {
    incoming: Incoming,
    error: anyhow::Error,
}
struct Queued {
    incoming: Incoming,
    guard: guard::Store,
    custody: Custody,
}
struct Active {
    id: String,
    admission: String,
    until: Instant,
    reply: std::sync::mpsc::SyncSender<Value>,
    guard: guard::Store,
    custody: Custody,
    handle: JoinHandle<(Envelope, Result<worker::Proof>)>,
}
#[derive(Default)]
pub(in crate::desktop) struct Runtime {
    queue: VecDeque<Queued>,
    active: Option<Active>,
}
impl Runtime {
    /// No DB/schema/worker/native read at construction, including at startup.
    #[allow(clippy::too_many_arguments)]
    pub fn admit(
        &mut self,
        config: &Config,
        location: &Location,
        store: &mut Store,
        incoming: Incoming,
        custody: Option<Custody>,
        epoch: &str,
        seq: u64,
    ) {
        let id = incoming.envelope.request_id.clone();
        let result = self.admit_inner(config, location, store, incoming, custody);
        match result {
            Ok(Some(Immediate { incoming, value })) => {
                server::response(&incoming.reply, epoch, seq, value)
            }
            Ok(None) => {}
            Err(error) => {
                let Rejected { incoming, error } = *error;
                server::response(
                    &incoming.reply,
                    epoch,
                    seq,
                    server::refused(&id, "task_control_admission_refused", &error.to_string()),
                );
            }
        }
        self.pump(config, location, store, epoch, seq);
    }
    fn admit_inner(
        &mut self,
        config: &Config,
        location: &Location,
        store: &mut Store,
        incoming: Incoming,
        custody: Option<Custody>,
    ) -> std::result::Result<Option<Immediate>, Box<Rejected>> {
        // Keep ownership of the exact responder/peer on every early error.
        let checked = (|| -> Result<_> {
            ensure!(super::enabled(), "task_controls_inactive");
            ensure!(
                Instant::now() < incoming.until,
                "task-control admission expired"
            );
            incoming
                .peer
                .command(&incoming.envelope.command, incoming.until)?;
            ensure!(
                !authority::requires_person(&incoming.envelope.command),
                "Person task controls require a separately verified native GUI action"
            );
            match &incoming.envelope.command {
                Command::Begin(_) | Command::Plan(_) => Ok(Decision::Read(server::refused(
                    &incoming.envelope.request_id,
                    "task_control_effect_path_unqualified",
                    CLOSED,
                ))),
                Command::Status(status) => {
                    let guard = guard::Store::capture(config, location, &store.db)?;
                    let value = lookup(&mut store.db, &status.target)?;
                    guard.verify(&store.db, &location.configuration)?;
                    incoming.peer.current(incoming.until)?;
                    Ok(Decision::Read(
                        json!({"schema":1,"request_id":incoming.envelope.request_id,
                        "ok":true,"attempted":false,"read_only":true,"code":"task_control_status","status":value}),
                    ))
                }
                Command::Execute(plan) => {
                    let guard = guard::Store::capture(config, location, &store.db)?;
                    let custody = custody
                        .context("Task controls require the original active service custody")?;
                    custody.verify(location, &store.db)?;
                    ensure!(
                        plan.configuration == location.configuration
                            && plan.store_identity == guard.digest()
                            && plan.cli_image == incoming.peer.image_guard(incoming.until)?,
                        "The original task-control configuration/store/image changed"
                    );
                    // Existing-only: a new/unproven request cannot initialize
                    // tables or reserve data. Durable orphan/Unknown is lookup.
                    if let Some(old) = original(&mut store.db, plan)? {
                        return Ok(Decision::Read(retained(&incoming.envelope.request_id, old)));
                    }
                    if let Some(digest) = self.claimed(&plan.request.id) {
                        ensure!(
                            digest == plan.request.admission,
                            "request UUID payload changed"
                        );
                        return Ok(Decision::Read(
                            json!({"schema":1,"request_id":plan.request.id,
                            "ok":false,"attempted":false,"pending":true,"code":"task_control_pending",
                            "error":"The original request is in flight; status never restarts it."}),
                        ));
                    }
                    ensure!(
                        self.queue.len() + usize::from(self.active.is_some()) < LIMIT,
                        "task-control admission is full"
                    );
                    Ok(Decision::Queue(guard, Box::new(custody)))
                }
            }
        })();
        match checked {
            Ok(Decision::Read(value)) => Ok(Some(Immediate { incoming, value })),
            Ok(Decision::Queue(guard, custody)) => {
                self.queue.push_back(Queued {
                    incoming,
                    guard,
                    custody: *custody,
                });
                Ok(None)
            }
            Err(error) => Err(Box::new(Rejected { incoming, error })),
        }
    }
    fn claimed(&self, id: &str) -> Option<&str> {
        if let Some(a) = self.active.as_ref().filter(|a| a.id == id) {
            return Some(&a.admission);
        }
        self.queue
            .iter()
            .find_map(|q| match &q.incoming.envelope.command {
                Command::Execute(p) if p.request.id == id => Some(p.request.admission.as_str()),
                _ => None,
            })
    }
    /// Poll never starts a worker for a durable original, even after restart.
    pub fn poll(
        &mut self,
        config: &Config,
        location: &Location,
        store: &mut Store,
        accepting: bool,
        epoch: &str,
        seq: u64,
    ) {
        if self.active.as_ref().is_some_and(|a| a.handle.is_finished()) {
            let active = self.active.take().unwrap();
            let value = match active.handle.join() {
                Ok((envelope, proof)) => {
                    if !accepting {
                        Err(anyhow::anyhow!("original task-control service is draining"))
                    } else {
                        match proof {
                            Err(error) => Err(error),
                            Ok(proof) => Ok(commit(
                                location,
                                &mut store.db,
                                &envelope,
                                &proof,
                                &active.guard,
                                &active.custody,
                                active.until,
                            )
                            .unwrap_or_else(|_| unknown(&active.id))),
                        }
                    }
                }
                Err(_) => Err(anyhow::anyhow!(
                    "original task-control proof worker did not settle"
                )),
            };
            let value = value.unwrap_or_else(|e| {
                server::refused(
                    &active.id,
                    "task_control_before_effect_refused",
                    &format!("No native effect was attempted by this admission: {e}"),
                )
            });
            server::response(&active.reply, epoch, seq, value);
        }
        if !accepting {
            for queued in self.queue.drain(..) {
                server::response(
                    &queued.incoming.reply,
                    epoch,
                    seq,
                    server::refused(
                        &queued.incoming.envelope.request_id,
                        "task_control_service_draining",
                        CLOSED,
                    ),
                );
            }
        } else {
            self.pump(config, location, store, epoch, seq);
        }
    }
    fn pump(
        &mut self,
        config: &Config,
        location: &Location,
        store: &mut Store,
        epoch: &str,
        seq: u64,
    ) {
        while self.active.is_none() {
            let Some(Queued {
                incoming,
                guard,
                custody,
            }) = self.queue.pop_front()
            else {
                break;
            };
            let id = incoming.envelope.request_id.clone();
            let checked = (|| -> Result<Option<Control>> {
                ensure!(
                    Instant::now() < incoming.until,
                    "original request expired while queued"
                );
                custody.verify(location, &store.db)?;
                guard.verify(&store.db, &location.configuration)?;
                incoming.peer.current(incoming.until)?;
                let Command::Execute(plan) = &incoming.envelope.command else {
                    anyhow::bail!(CLOSED)
                };
                original(&mut store.db, plan)
            })();
            match checked {
                Ok(Some(old)) => server::response(&incoming.reply, epoch, seq, retained(&id, old)),
                Err(error) => server::response(
                    &incoming.reply,
                    epoch,
                    seq,
                    server::refused(
                        &id,
                        "task_control_before_effect_refused",
                        &error.to_string(),
                    ),
                ),
                Ok(None) => {
                    let Command::Execute(plan) = &incoming.envelope.command else {
                        unreachable!()
                    };
                    let admission = plan.request.admission.clone();
                    let until = incoming.until;
                    let reply = incoming.reply;
                    match worker::spawn(config.clone(), incoming.envelope, incoming.peer, until) {
                        Ok(handle) => {
                            self.active = Some(Active {
                                id,
                                admission,
                                until,
                                reply,
                                guard,
                                custody,
                                handle,
                            })
                        }
                        Err(error) => server::response(
                            &reply,
                            epoch,
                            seq,
                            server::refused(
                                &id,
                                "task_control_worker_not_started",
                                &error.to_string(),
                            ),
                        ),
                    }
                }
            }
        }
    }
    /// Retain custody until the one original read worker physically settles.
    /// It has no provider/effect path, and no queued work survives this drain.
    pub fn drain(&mut self) {
        self.queue.clear();
        if let Some(active) = self.active.take() {
            let _ = active.handle.join();
        }
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        self.drain();
    }
}
enum Decision {
    Read(Value),
    Queue(guard::Store, Box<Custody>),
}

fn original(db: &mut rusqlite::Connection, plan: &Plan) -> Result<Option<Control>> {
    let Some(ledger) = ledger::existing(db)? else {
        return Ok(None);
    };
    let old = ledger.control(&plan.request.id)?;
    if let Some(old) = &old {
        ensure!(old.request == plan.request, "request UUID payload changed");
    }
    Ok(old)
}
fn lookup(db: &mut rusqlite::Connection, target: &StatusScope) -> Result<Value> {
    let Some(ledger) = ledger::existing(db)? else {
        return Ok(Value::Null);
    };
    Ok(match target {
        StatusScope::Request { request_id } => serde_json::to_value(ledger.control(request_id)?)?,
        StatusScope::Executor {
            executor,
            after,
            limit,
        } => serde_json::to_value(ledger.controls(executor, *after, *limit)?)?,
    })
}
fn retained(id: &str, control: Control) -> Value {
    let pending = matches!(control.outcome, Outcome::Reserved | Outcome::Unknown);
    json!({"schema":1,"request_id":id,"ok":!pending && control.outcome != Outcome::Refused,"attempted":false,"pending":pending,
        "read_only":true,"durable":true,"code":"task_control_retained","error":control.reason,"control":control})
}
fn commit(
    location: &Location,
    db: &mut rusqlite::Connection,
    envelope: &Envelope,
    proof: &worker::Proof,
    guard: &guard::Store,
    custody: &Custody,
    until: Instant,
) -> Result<Value> {
    let Command::Execute(plan) = &envelope.command else {
        anyhow::bail!(CLOSED)
    };
    let verify = |db: &rusqlite::Connection| -> Result<()> {
        ensure!(Instant::now() < until, "task-control deadline expired");
        custody.verify(location, db)?;
        guard.verify(db, &location.configuration)?;
        proof.verify(until)?;
        let (image, loaded) = proof.actor.guards(until)?;
        ensure!(
            plan.configuration == location.configuration
                && plan.store_identity == guard.digest()
                && plan.cli_image == image
                && plan.loaded_adapter == loaded
                && plan.request.executor == *proof.actor.executor(),
            "task-control frozen guards changed"
        );
        Ok(())
    };
    verify(db)?;
    let imports = proof
        .stops
        .facts()
        .iter()
        .map(|fact| {
            stop_import::entry(fact, &location.configuration, guard.digest())
                .map_err(anyhow::Error::msg)
        })
        .collect::<Result<Vec<_>>>()?;
    let mut ledger = ledger::open_checked(db, verify)?.context("task_controls_inactive")?;
    let control = ledger.refuse_with_stops(&plan.request, &imports, CLOSED, verify)?;
    Ok(retained(&envelope.request_id, control))
}

fn unknown(id: &str) -> Value {
    json!({"schema":1,"request_id":id,"ok":false,"attempted":false,
        "pending":true,"code":"task_control_outcome_unknown",
        "error":"The original durable result is unverified. Use status; do not resubmit."})
}
