//! Ephemeral, service-owned click delivery. Only an original authenticated
//! frontend child can wait here; generic requests and display watches cannot.
#![allow(dead_code)] // Callback/launcher attachment follows the platform lifecycle.
use super::{AdmittedClick, Identity, Outbox, Target};
use crate::desktop::{fanout::Queue, ownership, store::Store, ui_authority};
use anyhow::{ensure, Context, Result};
use rusqlite::OptionalExtension;
use serde_json::{json, Value};
use std::{collections::VecDeque, sync::Arc, time::Instant};
use zerus_client_core::desktop::{
    self,
    activation::{self as wire, Open, Reply},
};

struct Pending {
    id: String,
    click: AdmittedClick,
    until: Instant,
}
struct Wait {
    id: String,
    peer: ui_authority::ActivationReceiver,
    queue: Arc<Queue>,
    until: Instant,
}
/// Passed only by the service's verified-absence decision to the selected native
/// launch mechanics. It is not Clone/Deserialize and has no public constructor.
pub(in crate::desktop) struct Launch {
    open: Open,
    configuration: String,
    until: Instant,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
    preparation: Option<Arc<dyn Send + Sync>>,
}
impl Launch {
    #[cfg(test)]
    pub(super) fn fixture(open: Open, configuration: String) -> Self {
        Self {
            open,
            configuration,
            until: Instant::now() + wire::MAX_AGE,
            cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            preparation: None,
        }
    }

    pub fn open(&self) -> &Open {
        &self.open
    }
    pub fn configuration(&self) -> &str {
        &self.configuration
    }
    #[cfg(any(target_os = "linux", windows))]
    pub fn current(&self) -> bool {
        Instant::now() < self.until && !self.cancelled.load(std::sync::atomic::Ordering::Acquire)
    }
    #[cfg(any(target_os = "linux", windows))]
    pub fn bind_preparation(
        &mut self,
        resources: Arc<dyn Send + Sync>,
        cancelled: Arc<std::sync::atomic::AtomicBool>,
    ) {
        self.preparation = Some(resources);
        self.cancelled = cancelled;
    }
    #[cfg(any(target_os = "linux", windows))]
    pub fn preparation_resources(&self) -> Option<Arc<dyn Send + Sync>> {
        self.preparation.clone()
    }
    #[cfg(any(target_os = "linux", windows))]
    pub fn release_preparation(&mut self) {
        self.preparation = None;
    }
}
pub(in crate::desktop) struct Router {
    configuration: String,
    epoch: String,
    pending: VecDeque<Pending>,
    waiting: Option<Wait>,
    stopping: bool,
    overflow: u64,
    /// Foreground rights lent to and refused for receivers of an Open.
    foreground: (u64, u64),
}
impl Router {
    pub fn new(configuration: &str, epoch: &str) -> Self {
        Self {
            configuration: configuration.into(),
            epoch: epoch.into(),
            pending: VecDeque::new(),
            waiting: None,
            stopping: false,
            overflow: 0,
            foreground: (0, 0),
        }
    }
    pub fn offer(&mut self, store: &Store, outbox: &Outbox, click: AdmittedClick) -> Result<bool> {
        ensure!(!self.stopping, "activation transport stopped");
        self.expire();
        ensure!(
            outbox.session.configuration == self.configuration
                && outbox.session.service_epoch == self.epoch,
            "activation source changed"
        );
        if outbox.admitted_activation(store, &click)?.is_none() {
            return Ok(false);
        }
        if self.pending.len() == wire::QUEUE {
            self.overflow = self.overflow.saturating_add(1);
            return Ok(false);
        }
        self.pending.push_back(Pending {
            id: uuid::Uuid::new_v4().to_string(),
            click,
            until: Instant::now() + wire::MAX_AGE,
        });
        Ok(true)
    }
    pub fn wait(&mut self, authority: ui_authority::Validated, queue: Arc<Queue>) -> Result<()> {
        ensure!(
            !self.stopping && !queue.closed(),
            "activation transport stopped"
        );
        // At most one original child of the canonical frontend can wait.
        if self
            .waiting
            .as_ref()
            .is_some_and(|w| w.queue.closed() || !w.peer.current())
        {
            self.waiting.take().unwrap().queue.close();
        }
        ensure!(self.waiting.is_none(), "activation receiver busy");
        let (id, peer) = authority.activation_wait()?;
        self.waiting = Some(Wait {
            id,
            peer,
            queue,
            until: Instant::now() + wire::WAIT,
        });
        Ok(())
    }
    pub fn poll(&mut self, store: &Store, outbox: Option<&Outbox>, seq: &mut u64) {
        self.expire();
        let Some(wait) = self.waiting.as_ref() else {
            return;
        };
        if wait.queue.closed() || !wait.peer.current() {
            self.waiting.take().unwrap().queue.close();
            return;
        }
        let activation = match outbox {
            Some(outbox) => self.next(store, outbox),
            None => Ok(None),
        };
        let activation = match activation {
            Ok(value) => value.map(|(open, _)| open),
            Err(_) => {
                self.waiting.take().unwrap().queue.close();
                return;
            }
        };
        if activation.is_none() && Instant::now() < self.waiting.as_ref().unwrap().until {
            return;
        }
        let wait = self.waiting.take().unwrap();
        // Best-effort, after the handoff proof and before the reply exists.
        #[cfg(any(windows, test))]
        if activation.is_some() {
            if wait.peer.lend_foreground() {
                self.foreground.0 = self.foreground.0.saturating_add(1);
            } else {
                self.foreground.1 = self.foreground.1.saturating_add(1);
            }
        }
        // No replay after an uncertain pipe handoff. The frontend may start its
        // next wait; another native click is a distinct event with its own ID.
        let reply = Reply {
            abi: wire::ABI.into(),
            request_id: wait.id,
            configuration: self.configuration.clone(),
            service_epoch: self.epoch.clone(),
            activation,
        };
        if let Some(next) = seq.checked_add(1) {
            *seq = next;
            if let Ok(bytes) = crate::desktop::protocol::encode(&crate::desktop::frame(
                &self.epoch,
                *seq,
                "response",
                serde_json::to_value(reply).expect("closed reply"),
            )) {
                wait.queue.offer(bytes.into());
            }
        }
        wait.queue.close();
    }
    pub fn needs_frontend(&self) -> bool {
        !self.stopping
            && self.waiting.is_none()
            && self.pending.iter().any(|p| Instant::now() < p.until)
    }
    /// Absence is only a routing observation. The original durable native grant
    /// supplies mode/identity; none of the observation's JSON grants authority.
    pub fn closed_frontend(
        &mut self,
        store: &Store,
        outbox: &Outbox,
        observation: &desktop::Observation,
        grant: Option<&ownership::Grant>,
    ) -> Result<Option<Launch>> {
        self.expire();
        if self.stopping
            || self.waiting.is_some()
            || !matches!(observation, desktop::Observation::Absent)
            || self.pending.is_empty()
        {
            return Ok(None);
        }
        let grant = grant.context("authenticated notification ownership handoff required")?;
        grant.permits_launch(store, &self.configuration, &outbox.session.writer_epoch)?;
        Ok(self.next(store, outbox)?.map(|(open, until)| Launch {
            open,
            configuration: self.configuration.clone(),
            until,
            cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            preparation: None,
        }))
    }
    fn next(&mut self, store: &Store, outbox: &Outbox) -> Result<Option<(Open, Instant)>> {
        ensure!(
            outbox.session.configuration == self.configuration
                && outbox.session.service_epoch == self.epoch,
            "activation source changed"
        );
        // The whole queue is capped at 32; no journal or session scan.
        while let Some(pending) = self.pending.pop_front() {
            let Some(binding) = outbox.admitted_activation(store, &pending.click)? else {
                continue;
            };
            let raw:Option<String>=store.db.query_row("SELECT CASE WHEN length(CAST(identity AS BLOB))<=4096 THEN identity END FROM desktop_intents WHERE id=?1",[&binding.intent_id],|r|r.get(0)).optional()?.flatten();
            let identity: Identity =
                serde_json::from_str(&raw.context("activation identity unavailable")?)?;
            ensure!(
                super::ledger::intent_id(&identity)? == binding.intent_id,
                "activation identity changed"
            );
            let (computer, run, conversation, startup_question) = match identity.target {
                Target::Session {
                    computer,
                    run,
                    conversation,
                } => (computer, run, conversation, None),
                Target::StartupQuestion {
                    computer,
                    run,
                    question,
                } => (computer, run, String::new(), Some(question)),
                _ => continue,
            };
            if let Some(wait) = &self.waiting {
                // No frame was offered yet. Keep a refused click under its
                // ORIGINAL age so a newly authenticated receiver can take it.
                if wait.queue.closed() || !wait.peer.handoff_current() {
                    self.pending.push_front(pending);
                    anyhow::bail!("activation receiver changed");
                }
            }
            let open = Open {
                activation_id: pending.id,
                post_request_id: pending.click.value.request_id,
                target: wire::Target {
                    computer,
                    run,
                    conversation,
                    startup_question,
                },
                token: pending.click.value.token,
                platform_context: pending.click.value.platform_context,
            };
            ensure!(open.valid(), "activation target invalid");
            return Ok(Some((open, pending.until)));
        }
        Ok(None)
    }
    fn expire(&mut self) {
        let now = Instant::now();
        self.pending.retain(|p| now < p.until);
    }
    pub fn stop(&mut self) {
        self.stopping = true;
        self.pending.clear();
        if let Some(wait) = self.waiting.take() {
            wait.queue.close()
        }
    }
    pub fn status(&self) -> Value {
        json!({"abi":wire::ABI,"pending":self.pending.len(),"limit":wire::QUEUE,"waiting":self.waiting.is_some(),"stopping":self.stopping,"overflow":self.overflow,"foreground":{"lent":self.foreground.0,"refused":self.foreground.1},"durable":false})
    }
}
impl Drop for Router {
    fn drop(&mut self) {
        self.stop()
    }
}

#[cfg(test)]
mod tests;
