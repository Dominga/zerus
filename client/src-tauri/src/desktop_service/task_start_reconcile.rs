//! Explicit native Task Start checking. The renderer supplies only expected
//! task/view identity; native files and the original Service supply the rest.
use super::{audit::help::Image, *};
use std::{path::Path, time::Instant};
use zerus_client_core::{
    desktop::{
        relay, task_start_reconcile as wire,
        ui_requests::{Registration, Request},
    },
    run,
    task_start::{
        self,
        capture::{
            journal::{self, CheckId, Journal, Phase, PublicationStatus, Saved, Settlement},
            Captured,
        },
    },
    Invocation,
};

pub(crate) const UNAVAILABLE: &str = zerus_client_core::source!(
    "The original delivery receipt could not be verified. Recovery data was kept."
);
pub(crate) const UNSUPPORTED: &str = zerus_client_core::source!(
    "Update hgs to check the original delivery receipt from this desktop."
);
pub(crate) const STALE_CHECK: &str = zerus_client_core::source!(
    "An unsent receipt check belongs to an earlier start of this task. Discard it before checking this start."
);
pub(crate) const CHANGED: &str = zerus_client_core::source!(
    "The saved task start changed. Refresh the task before reviewing it."
);

pub(crate) const PUBLICATION_REVIEW: &str = zerus_client_core::source!(
    "Receipt-check recovery data needs manual review. Keep the files and follow the Task Start recovery runbook."
);
pub(crate) const PUBLICATION_OVERSIZE: &str = zerus_client_core::source!(
    "Receipt-check recovery tracking exceeds its size limit. Keep the files and follow the Task Start recovery runbook."
);
pub(crate) fn publication_problem(saved: &Saved) -> Option<&'static str> {
    match saved.publication_status() {
        PublicationStatus::NeedsReview => Some(PUBLICATION_REVIEW),
        PublicationStatus::Oversize => Some(PUBLICATION_OVERSIZE),
        PublicationStatus::Clear | PublicationStatus::Recoverable => None,
    }
}
pub(crate) fn journal_problem(error: journal::Error) -> &'static str {
    match error {
        journal::Error::PublicationNeedsReview => PUBLICATION_REVIEW,
        journal::Error::PublicationOversize => PUBLICATION_OVERSIZE,
        _ => UNAVAILABLE,
    }
}

struct Read {
    service: Arc<Service>,
    scope: wire::Scope,
    generation: u64,
    cancel: Arc<AtomicBool>,
    routing: Arc<relay::Connection>,
    image: Arc<Image>,
}
impl Read {
    fn new(service: Option<Arc<Service>>) -> Result<Arc<Self>, String> {
        let service = service.ok_or(UNAVAILABLE)?;
        let shared = &service.shared;
        let snapshot = service.snapshot();
        if !shared.allowed || !snapshot.connected || shared.stopped.load(Ordering::Acquire) {
            return Err(UNAVAILABLE.into());
        }
        let configuration = shared
            .configuration
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or(UNAVAILABLE)?;
        let cancel = shared
            .cancellation
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or(UNAVAILABLE)?;
        let routing = shared
            .relay
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or(UNAVAILABLE)?;
        let image = Arc::new(Image::capture(routing.executable()?).ok_or(UNAVAILABLE)?);
        let value = Arc::new(Self {
            service,
            scope: wire::Scope {
                configuration,
                service_epoch: snapshot.epoch.ok_or(UNAVAILABLE)?,
            },
            generation: snapshot.generation.ok_or(UNAVAILABLE)?,
            cancel,
            routing,
            image,
        });
        if !value.current() {
            return Err(UNAVAILABLE.into());
        }
        Ok(value)
    }
    fn current(&self) -> bool {
        let shared = &self.service.shared;
        let snapshot = self.service.snapshot();
        !self.cancel.load(Ordering::Acquire)
            && !shared.stopped.load(Ordering::Acquire)
            && snapshot.connected
            && snapshot.epoch.as_deref() == Some(self.scope.service_epoch.as_str())
            && snapshot.generation == Some(self.generation)
            && shared
                .configuration
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_deref()
                == Some(self.scope.configuration.as_str())
            && shared
                .cancellation
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()
                .is_some_and(|v| Arc::ptr_eq(v, &self.cancel))
            && shared
                .relay
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()
                .is_some_and(|v| Arc::ptr_eq(v, &self.routing))
            && self
                .routing
                .executable()
                .is_ok_and(|p| self.image.current(p))
    }
    fn prepare(&self, intent: wire::Intent) -> Result<Registration, String> {
        if !self.current() {
            return Err(UNAVAILABLE.into());
        }
        self.service
            .shared
            .responder
            .as_ref()
            .ok_or(UNAVAILABLE)?
            .prepare_ui_request(
                uuid::Uuid::new_v4().to_string(),
                Request::TaskStartReconcile {
                    intent: Box::new(intent),
                },
            )
            .map_err(|_| UNAVAILABLE.into())
    }
    fn exchange(
        self: &Arc<Self>,
        registration: Option<Registration>,
        input: Value,
        help: bool,
        extra: Arc<dyn Send + Sync>,
    ) -> Result<Vec<u8>, String> {
        if !self.current() {
            return Err(UNAVAILABLE.into());
        }
        let rpc = self
            .service
            .shared
            .rpc
            .admit(Role::Fleet)
            .map_err(|_| UNAVAILABLE)?;
        let input = if let Some(registration) = &registration {
            serde_json::to_vec(registration.envelope())
        } else if help {
            Ok(Vec::new())
        } else {
            serde_json::to_vec(&input)
        }
        .map_err(|_| UNAVAILABLE)?;
        if input.len() > zerus_watch_protocol::CONTROL_LINE_BYTES {
            return Err(UNAVAILABLE.into());
        }
        let helper = self.routing.executable()?;
        let args = if help {
            vec!["--help".into()]
        } else {
            vec![
                "desktop".into(),
                if registration.is_some() {
                    "frontend-request"
                } else {
                    "request"
                }
                .into(),
                "--json".into(),
            ]
        };
        let invocation = Invocation {
            args,
            stdin: Some(input),
            timeout: RPC_WAIT,
            request_id: None,
        };
        let held: Arc<dyn Send + Sync> = Arc::new((
            rpc,
            extra,
            self.clone(),
            registration.as_ref().map(Registration::resources),
        ));
        let result = run::run_owned(
            helper,
            &invocation,
            &[],
            &self.cancel,
            move |command| {
                let mut child = command.spawn()?;
                if registration
                    .as_ref()
                    .is_some_and(|r| !r.bind_original(&child))
                {
                    // Keep THIS child in run_owned's stop/reap/pipe custody, but
                    // withhold its input. An error after spawn must not lose it.
                    drop(child.stdin.take());
                }
                Ok(child)
            },
            held,
        )
        .map_err(|_| UNAVAILABLE)?;
        if !self.current() || result.len() > if help { 64 << 10 } else { wire::RESULT_BYTES } {
            return Err(UNAVAILABLE.into());
        }
        Ok(result)
    }
    fn settle(
        &self,
        saved: &mut Saved,
        query: &wire::Query,
        lookup: &wire::Lookup,
    ) -> Result<Settlement, String> {
        saved
            .settle_submitted(query, lookup, || self.current())
            .map_err(|error| journal_problem(error).into())
    }
    fn completed_view(
        &self,
        store: &Path,
        project: &str,
        task: &str,
        id: &str,
    ) -> Result<task_start::View, String> {
        let mut view = task_start::open(store, project, task, None)?;
        if view.id != id || view.phase != "started" || !self.current() {
            return Err(CHANGED.into());
        }
        view.outcome = Some("submitted".into());
        Ok(view)
    }
    fn supported(self: &Arc<Self>, held: Arc<dyn Send + Sync>) -> Result<bool, String> {
        let bytes = self.exchange(None, Value::Null, true, held)?;
        let text = std::str::from_utf8(&bytes).map_err(|_| UNAVAILABLE)?;
        Ok([
            wire::HELP_MARKER,
            zerus_client_core::desktop::ui_requests::HELP_MARKER,
        ]
        .iter()
        .all(|m| text.lines().any(|line| line.trim() == *m)))
    }
}

pub(crate) fn check(
    service: Option<Arc<Service>>,
    store: &Path,
    project: &str,
    task: &str,
    id: &str,
    snapshot: &str,
    resources: Arc<dyn Send + Sync>,
) -> Result<task_start::View, String> {
    let mut read = Read::new(service.clone())?;
    if !read.supported(resources.clone())? {
        return Err(UNSUPPORTED.into());
    }
    let loaded = Saved::read(store, project, task).map_err(|_| UNAVAILABLE)?;
    let original = task_start::open(store, project, task, None)?;
    let finishing = loaded.as_ref().is_some_and(|s| {
        s.original_id() == id && matches!(s.phase(), Phase::SettlementPrepared | Phase::Completed)
    });
    if original.id != id || original.snapshot != snapshot || !original.pending && !finishing {
        return Err(CHANGED.into());
    }
    if loaded
        .as_ref()
        .is_some_and(|s| s.phase() == Phase::Prepared && s.original_id() != id)
    {
        return Err(STALE_CHECK.into());
    }
    if let Some(problem) = loaded.as_ref().and_then(publication_problem) {
        return Err(problem.into());
    }
    let new_original = loaded
        .as_ref()
        .is_some_and(|s| s.phase() == Phase::Completed && s.original_id() != id);
    let offered = !new_original
        && loaded.as_ref().is_some_and(|s| {
            matches!(
                s.phase(),
                Phase::OfferedUnknown | Phase::SettlementPrepared | Phase::Completed
            )
        });
    if !offered {
        let source = Captured::read(store, project, task, id, snapshot, read.scope.clone())
            .map_err(|_| UNAVAILABLE)?;
        let proposal = match loaded.as_ref() {
            Some(saved) if saved.phase() == Phase::CancelledBeforeOffer || new_original => {
                saved.next_check_id().map_err(journal_problem)?
            }
            _ => CheckId::fresh(),
        };
        let reconcile = loaded
            .as_ref()
            .filter(|s| s.phase() == Phase::Prepared && s.original_id() == id)
            .map(|s| s.reconcile_id().to_owned())
            .unwrap_or_else(|| proposal.as_str().to_owned());
        let intent = source.intent(&reconcile).map_err(|_| UNAVAILABLE)?;
        let registration = read.prepare(intent.clone())?;
        // Defence in depth: today's bounded original cannot fill this envelope
        // (reviewed quote-filled case: 64,938 bytes). Keep the complete frame
        // check if fields grow; oversized input must remain unoffered.
        if serde_json::to_vec(registration.envelope())
            .map_err(|_| UNAVAILABLE)?
            .len()
            > zerus_watch_protocol::CONTROL_LINE_BYTES
        {
            return Err(UNAVAILABLE.into());
        }
        let journal = match loaded {
            Some(saved) if saved.phase() == Phase::CancelledBeforeOffer => {
                Journal::prepare_after_discard(source, &proposal, saved.reconcile_id())
            }
            Some(saved) if new_original => saved.prepare_new_original(source, &proposal),
            _ => Journal::prepare(source, &proposal),
        }
        .map_err(journal_problem)?;
        let journal = Arc::new(Mutex::new(journal));
        {
            let mut value = journal.lock().unwrap_or_else(PoisonError::into_inner);
            if !read.current() || value.saved().reconcile_id() != reconcile {
                return Err(UNAVAILABLE.into());
            }
            let exact = value.offer().map_err(journal_problem)?;
            if exact != intent {
                return Err(UNAVAILABLE.into());
            }
        }
        let held: Arc<dyn Send + Sync> = Arc::new((journal, resources.clone()));
        // Lost reply is followed only by a lookup of this saved identity.
        let _ = read.exchange(Some(registration), Value::Null, false, held);
    }
    let until = Instant::now() + Duration::from_secs(32);
    let mut delay = Duration::from_millis(100);
    loop {
        if !read.current() {
            read = Read::new(service.clone())?;
        }
        let saved = Saved::read(store, project, task)
            .map_err(|_| UNAVAILABLE)?
            .ok_or(UNAVAILABLE)?;
        let saved = Arc::new(Mutex::new(saved));
        let query = saved
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .query(read.scope.clone(), &uuid::Uuid::new_v4().to_string())
            .map_err(|_| UNAVAILABLE)?;
        let held: Arc<dyn Send + Sync> = Arc::new((saved.clone(), resources.clone()));
        let bytes = read.exchange(
            None,
            json!({"op":"task_start_reconcile_result","query":query}),
            false,
            held,
        )?;
        let lookup: wire::Lookup = serde_json::from_slice(&bytes).map_err(|_| UNAVAILABLE)?;
        if !lookup.matches(&query) {
            return Err(UNAVAILABLE.into());
        }
        let attempt = lookup.attempt.as_ref().ok_or(UNAVAILABLE)?; // Missing NEVER offers again.
        if attempt.submitted(&query).is_some() {
            let state = read.settle(
                &mut saved.lock().unwrap_or_else(PoisonError::into_inner),
                &query,
                &lookup,
            )?;
            if state == Settlement::ChangedOriginal {
                return Err(CHANGED.into());
            }
            return read.completed_view(store, project, task, id);
        }
        if attempt.phase != wire::Phase::Reserved && attempt.phase != wire::Phase::Submitted
            || Instant::now() >= until
        {
            return Err(UNAVAILABLE.into());
        }
        let next = Instant::now() + delay;
        while Instant::now() < next {
            if !read.current() {
                return Err(UNAVAILABLE.into());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        delay = (delay * 2).min(Duration::from_secs(1));
    }
}

#[cfg(all(test, unix))]
mod tests;
