//! Original-child ownership intents. Display state cannot release native writers.
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, PoisonError,
    },
    time::{Duration, Instant},
};
use zerus_client_core::{
    desktop::{
        drain::{Closed, Gate, Observation, Role},
        ownership::{self as wire, Action, Mode, Outcome, Query, Receipt, Status},
        relay,
        ui_requests::{self, Request},
        Responder,
    },
    run::{run_owned_observed, OwnedError},
    Invocation,
};

const WAIT: Duration = Duration::from_secs(5);
const GRANT_RETRIES: usize = 2;
const GRANT_BACKOFF: Duration = Duration::from_millis(250);

mod diagnostic;
pub(crate) use diagnostic::Phase;
mod observation;
pub(crate) use observation::NativeCurrent;

#[derive(Clone, Default, Debug, Serialize)]
pub(crate) struct View {
    pub status: Option<Status>,
    pub request_id: Option<String>,
    pub pending: bool,
    pub running: bool,
    pub code: String,
}
struct Intent {
    request: Request,
    configuration: String,
    route: Arc<relay::Connection>,
    exit_confirmed: bool,
    complete: bool,
    receipt: Option<Receipt>,
    // A lookup after the verified original exit found no receipt at the
    // original's unchanged journal revision. It never implies success.
    not_applied: bool,
}
/// The exact status that justified a definitive not-applied lookup. A fresh
/// recovery Grant is reserved only while the authority still reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Basis {
    configuration: String,
    journal: Option<String>,
    revision: u64,
}
impl Basis {
    fn holds(&self, status: &Status) -> bool {
        status.matches(&self.configuration)
            && status.journal == self.journal
            && status.revision == self.revision
            && !status.rearm_required
    }
}
/// Why recovery may send a fresh Grant, captured under the admission lock.
#[derive(Debug, PartialEq, Eq)]
enum Reason {
    Capacity,
    Refused(Basis),
    NotApplied(Basis),
}
#[derive(Default)]
struct State {
    status: Option<Status>,
    intent: Option<Intent>,
    // One uncertain initial native intent may be superseded by one fresh,
    // authenticated authority reduction; it is never called refused/applied.
    retired_native: Option<Intent>,
    legacy: Option<Arc<relay::Connection>>,
    running: bool,
    code: String,
    // First failing step wins over later read-only lookup failures. Only closed
    // local codes are kept, never child output, IDs, paths or configuration.
    diagnostic: Option<(&'static str, &'static str)>,
}
/// A retained veto only; this cannot grant native ownership authority.
pub(crate) type EffectCheck = Arc<dyn Fn() -> Result<(), &'static str> + Send + Sync>;
/// Captured only after a fresh actor read matches this controller's OWN
/// native Grant receipt. Read-only scope, never constructible from JSON.
pub(crate) struct CurrentGrant(Status);
impl CurrentGrant {
    pub(crate) fn status(&self) -> &Status {
        &self.0
    }
}

pub(crate) struct Controller {
    hgs: PathBuf,
    responder: Option<Arc<Responder>>,
    mode: Mode,
    gate: Gate,
    stop: Arc<AtomicBool>,
    state: Mutex<State>,
    diagnostic_publication: Mutex<()>,
}
impl Controller {
    pub(crate) fn new(hgs: PathBuf, responder: Option<Arc<Responder>>, mode: Mode) -> Arc<Self> {
        Arc::new(Self {
            hgs,
            responder,
            mode,
            gate: Gate::new(2).unwrap(),
            stop: Arc::new(AtomicBool::new(false)),
            state: Mutex::new(State::default()),
            diagnostic_publication: Mutex::new(()),
        })
    }
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
    pub(crate) fn view(&self) -> View {
        let state = self.state();
        Self::state_view(&state)
    }
    fn state_view(state: &State) -> View {
        View {
            status: state.status.clone(),
            request_id: state.intent.as_ref().map(|i| i.request.id().into()),
            pending: state.intent.as_ref().is_some_and(|i| !i.complete),
            running: state.running,
            code: state.code.clone(),
        }
    }
    /// Called only by the existing blocking ownership worker/publication task.
    /// This private record is an observation and cannot admit any operation.
    pub(crate) fn record_view(&self, phase: Phase) -> View {
        // Serialize capture plus replacement so a delayed older publication
        // cannot overwrite a newer observation from this controller.
        let _publication = self
            .diagnostic_publication
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let state = self.state();
        let view = Self::state_view(&state);
        let first = state.diagnostic;
        drop(state);
        diagnostic::record(phase, &view, first);
        view
    }
    pub(crate) fn stop(&self) {
        self.stop.store(true, Ordering::Release);
        self.gate.close();
    }
    pub(crate) fn close(&self) -> Closed {
        self.gate.close()
    }
    pub(crate) fn unsupported(&self) {
        self.state().code = "incompatible_ownership_cli".into();
    }
    /// Admission precedes spawn_blocking; unknown original pipes keep their charge.
    pub(crate) async fn request(
        self: &Arc<Self>,
        action: Option<Action>,
        configuration: Option<String>,
        cancel: Option<Arc<AtomicBool>>,
    ) -> View {
        let Some(job) = self.admit(action, configuration, cancel) else {
            return self.view();
        };
        let owner = self.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || job.run()).await;
        owner.view()
    }
    /// Additional original-selection/cancellation fence. It can only refuse;
    /// native caller/current journal admission remains in the original actor.
    pub(crate) async fn request_checked(
        self: &Arc<Self>,
        action: Action,
        configuration: String,
        check: EffectCheck,
    ) -> Result<View, &'static str> {
        let mut job = self
            .admit(Some(action), Some(configuration), None)
            .ok_or("ownership_transport_busy")?;
        job.effect_check = Some(check);
        let owner = self.clone();
        tauri::async_runtime::spawn_blocking(move || {
            if owner.mode == Mode::Service {
                owner.record_view(Phase::WorkerStarted);
            }
            let result = job.execute();
            if let Err(code) = result {
                owner.state().code = code.into();
            }
            result
        })
        .await
        .map_err(|_| "ownership_unconfirmed")??;
        Ok(self.view())
    }
    /// Image replacement can need another bounded drain after the actor's
    /// 750 ms attempt. Only a matched, definitive refusal admits a NEW Grant;
    /// uncertain originals remain on the read-only result path.
    pub(crate) async fn startup_grant(
        self: &Arc<Self>,
        configuration: String,
        cancel: Arc<AtomicBool>,
    ) -> View {
        for attempt in 0..=GRANT_RETRIES {
            // Later attempts are retries: the same bound admission as recovery.
            let view = if attempt == 0 {
                self.request(
                    Some(Action::Grant),
                    Some(configuration.clone()),
                    Some(cancel.clone()),
                )
                .await
            } else {
                self.recovery_grant(configuration.clone(), cancel.clone())
                    .await
            };
            let retryable = self.retryable_grant();
            if !retryable || attempt == GRANT_RETRIES {
                return view;
            }
            let until = tokio::time::Instant::now() + GRANT_BACKOFF;
            while tokio::time::Instant::now() < until {
                if self.stop.load(Ordering::Acquire) || cancel.load(Ordering::Acquire) {
                    return self.view();
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        }
        unreachable!("the final bounded attempt returns its view")
    }
    /// Only a complete original refusal, a definitive not-applied lookup of an
    /// original after its verified exit, or capacity before any original
    /// exists, can admit another fresh startup Grant.
    pub(crate) fn retryable_grant(&self) -> bool {
        Self::grant_reason(&self.state()).is_some()
    }
    /// The whole retry decision table, evaluated under the admission lock:
    /// - a retained not-applied basis decides alone, whatever the last local
    ///   diagnostic code (a transport-busy Check cannot mask it): a Grant
    ///   bound to that basis while no newer status contradicts it, else none;
    /// - a matched transient refusal of the current original;
    /// - capacity only at a cold start, before any original exists.
    ///   Nothing else, and never an unbound Grant that replaces a basis.
    fn grant_reason(state: &State) -> Option<Reason> {
        if let Some(basis) = Self::not_applied_basis(state) {
            let contradicted = state
                .status
                .as_ref()
                .is_some_and(|status| !basis.holds(status));
            return (!contradicted).then_some(Reason::NotApplied(basis));
        }
        // A transient refusal is bound to its own receipt's authority scope:
        // the authority refuses drain/resources before it moves a revision.
        let refused = state.intent.as_ref().and_then(|intent| {
            let receipt = intent.receipt.as_ref()?;
            let transient = intent.complete
                && intent.exit_confirmed
                && matches!(intent.request, Request::DesktopOwnership { action: Action::Grant, .. })
                && !receipt.accepted
                && !receipt.durable
                && state.status.as_ref() == Some(&receipt.current)
                && matches!(&receipt.outcome, Outcome::Refused { code }
                    if (state.code.is_empty() || state.code == *code
                        || state.code == "ownership_transport_busy")
                        && matches!(code.as_str(), "ownership_drain_pending" | "ownership_resources_pending" | "ownership_transport_busy"));
            transient.then(|| Basis {
                configuration: receipt.current.configuration.clone(),
                journal: receipt.current.journal.clone(),
                revision: receipt.current.revision,
            })
        });
        if let Some(basis) = refused {
            return Some(Reason::Refused(basis));
        }
        (!state.running && state.code == "ownership_transport_busy" && state.intent.is_none())
            .then_some(Reason::Capacity)
    }
    /// A complete Grant original found definitively not applied after its
    /// verified exit. It is a durable property of that original, independent
    /// of later diagnostic codes; only a new intent replaces it.
    fn not_applied_basis(state: &State) -> Option<Basis> {
        let intent = state.intent.as_ref()?;
        let Request::DesktopOwnership {
            action: Action::Grant,
            journal: Some(journal),
            revision,
            ..
        } = &intent.request
        else {
            return None;
        };
        (intent.complete && intent.exit_confirmed && intent.not_applied && intent.receipt.is_none())
            .then(|| Basis {
                configuration: intent.configuration.clone(),
                journal: Some(journal.clone()),
                revision: *revision,
            })
    }
    /// Recovery's next Grant. After a definitive not-applied lookup it stays
    /// bound to that original status: the job compares its fresh authority
    /// read before reserving any new intent, and a change is fenced.
    pub(crate) async fn recovery_grant(
        self: &Arc<Self>,
        configuration: String,
        cancel: Arc<AtomicBool>,
    ) -> View {
        // Revalidate the reason and capture its basis under the same state
        // lock that marks this job running. A vanished allowance refuses.
        let Some(job) = self.admit_with(
            Some(Action::Grant),
            Some(configuration),
            Some(cancel),
            |state| match Self::grant_reason(state)? {
                Reason::NotApplied(basis) | Reason::Refused(basis) => Some(Some(basis)),
                Reason::Capacity => Some(None),
            },
        ) else {
            return self.view();
        };
        let owner = self.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || job.run()).await;
        owner.view()
    }
    /// The current Grant original is unknown after its child's verified exit.
    /// Only a read-only lookup of that exact original may follow; an
    /// unverified exit or a known pending receipt is never looked up here.
    pub(crate) fn uncertain_grant(&self) -> Option<String> {
        let state = self.state();
        state
            .intent
            .as_ref()
            .filter(|intent| {
                !intent.complete
                    && intent.exit_confirmed
                    && intent.receipt.is_none()
                    && matches!(
                        intent.request,
                        Request::DesktopOwnership {
                            action: Action::Grant,
                            ..
                        }
                    )
            })
            .map(|intent| intent.request.id().to_owned())
    }
    fn admit(
        self: &Arc<Self>,
        action: Option<Action>,
        configuration: Option<String>,
        cancel: Option<Arc<AtomicBool>>,
    ) -> Option<Job> {
        self.admit_with(action, configuration, cancel, |_| Some(None))
    }
    /// `allowed` runs under the state lock before `running` is set; `None`
    /// refuses, `Some(basis)` binds the job to that basis.
    fn admit_with(
        self: &Arc<Self>,
        action: Option<Action>,
        configuration: Option<String>,
        cancel: Option<Arc<AtomicBool>>,
        allowed: impl FnOnce(&State) -> Option<Option<Basis>>,
    ) -> Option<Job> {
        if self.stop.load(Ordering::Acquire) {
            return None;
        }
        if self.responder.is_none() {
            self.state().code = "ownership_identity_unavailable".into();
            return None;
        }
        let charge = match self.gate.admit(Role::Fleet) {
            Ok(charge) => charge,
            Err(_) => {
                self.state().code = "ownership_transport_busy".into();
                return None;
            }
        };
        let mut state = self.state();
        if state.running {
            return None;
        }
        let basis = allowed(&state)?;
        state.running = true;
        drop(state);
        Some(Job {
            owner: self.clone(),
            action,
            configuration,
            cancel: cancel.unwrap_or_else(|| self.stop.clone()),
            resources: Arc::new(charge),
            deadline: None,
            refresh_absent_native: false,
            basis,
            keep_epoch: None,
            effect_check: None,
        })
    }
    fn granted_state(state: &State) -> bool {
        state
            .intent
            .as_ref()
            .and_then(|i| i.receipt.as_ref())
            .is_some_and(|r| r.service_granted() && state.status.as_ref() == Some(&r.current))
    }
    pub(crate) fn granted(&self) -> bool {
        Self::granted_state(&self.state())
    }

    /// A busy, failed or original-result-only read cannot return a cached token.
    pub(crate) async fn current_grant(
        self: &Arc<Self>,
        configuration: String,
        cancel: Arc<AtomicBool>,
    ) -> Option<CurrentGrant> {
        let job = self.admit_with(None, Some(configuration), Some(cancel), |s| {
            (!s.intent.as_ref().is_some_and(|i| !i.complete) && s.retired_native.is_none())
                .then_some(None)
        })?;
        let owner = self.clone();
        tauri::async_runtime::spawn_blocking(move || {
            job.execute().ok()?;
            let state = owner.state();
            Self::granted_state(&state)
                .then(|| CurrentGrant(state.status.clone().expect("Grant status")))
        })
        .await
        .ok()
        .flatten()
    }
    /// Recheck exactly this already granted origin/revision before reserving a
    /// single ordinary Grant. The actor still compares journal/revision under
    /// its lock and authenticates the original frontend. No recovery/new origin.
    pub(crate) async fn keep_epoch_refresh(
        self: &Arc<Self>,
        grant: CurrentGrant,
        cancel: Arc<AtomicBool>,
    ) -> View {
        let Some(mut job) = self.admit_with(
            Some(Action::Grant),
            Some(grant.0.configuration.clone()),
            Some(cancel),
            |s| (Self::granted_state(s) && s.status.as_ref() == Some(&grant.0)).then_some(None),
        ) else {
            return self.view();
        };
        job.keep_epoch = Some(grant);
        let _ = tauri::async_runtime::spawn_blocking(move || job.run()).await;
        self.view()
    }
}

struct Job {
    owner: Arc<Controller>,
    action: Option<Action>,
    configuration: Option<String>,
    cancel: Arc<AtomicBool>,
    resources: Arc<dyn Send + Sync>,
    deadline: Option<Instant>,
    refresh_absent_native: bool,
    basis: Option<Basis>,
    keep_epoch: Option<CurrentGrant>,
    effect_check: Option<EffectCheck>,
}
impl Drop for Job {
    fn drop(&mut self) {
        self.owner.state().running = false;
    }
}
fn invocation(args: &[&str], input: Vec<u8>) -> Invocation {
    Invocation {
        args: args.iter().map(|s| (*s).into()).collect(),
        stdin: Some(input),
        timeout: WAIT,
        request_id: None,
    }
}
impl Job {
    fn failure(&self, step: &'static str, error: &OwnedError, stderr: Option<&str>) {
        self.owner
            .state()
            .diagnostic
            .get_or_insert((step, failure_code(error, stderr)));
    }
    fn stopped(&self) -> bool {
        self.owner.stop.load(Ordering::Acquire) || self.cancel.load(Ordering::Acquire)
    }
    fn bound(&self, call: &mut Invocation) -> Result<(), &'static str> {
        if let Some(until) = self.deadline {
            let remaining = until
                .checked_duration_since(Instant::now())
                .filter(|time| !time.is_zero())
                .ok_or("ownership_takeover_unconfirmed")?;
            call.timeout = call.timeout.min(remaining);
        }
        Ok(())
    }
    fn call(
        &self,
        route: &Arc<relay::Connection>,
        args: &[&str],
        input: Vec<u8>,
    ) -> Result<Vec<u8>, &'static str> {
        if self.stopped() {
            return Err("ownership_stopped");
        }
        let held: Arc<dyn Send + Sync> = Arc::new((
            self.resources.clone(),
            self.owner.clone(),
            route.clone(),
            self.effect_check.clone(),
        ));
        let mut call = invocation(args, input);
        self.bound(&mut call)?;
        let step = match args {
            ["--help"] => "native_takeover.help",
            ["desktop", "ownership-status", "--json"] => "native_takeover.status",
            _ => "native_takeover.lookup",
        };
        let result = run_owned_observed(
            route.executable()?,
            &call,
            &[],
            &self.cancel,
            |c| c.spawn(),
            held,
            |error, stderr| self.failure(step, error, stderr),
        )
        .map_err(|_| "ownership_transport_unavailable")?;
        route.executable()?;
        if self.stopped() || (args != ["--help"] && result.len() > wire::BYTES) {
            return Err("invalid_ownership_response");
        }
        Ok(result)
    }
    fn run(&self) {
        if self.owner.mode == Mode::Service {
            self.owner.record_view(Phase::WorkerStarted);
        }
        if let Err(code) = self.execute() {
            self.owner.state().code = code.into();
        }
    }
    fn execute(&self) -> Result<(), &'static str> {
        if self.stopped() {
            return Err("ownership_stopped");
        }
        if self.action.is_some_and(|a| a.mode() != self.owner.mode) {
            return Err("ownership_mode_mismatch");
        }
        let pending = {
            let state = self.owner.state();
            state.intent.as_ref().filter(|i| !i.complete).map(|i| {
                (
                    i.request.clone(),
                    i.configuration.clone(),
                    i.exit_confirmed,
                    i.route.clone(),
                )
            })
        };
        let route = match &pending {
            Some((_, _, _, route)) => route.clone(),
            None => Arc::new(relay::Connection::select(&self.owner.hgs)?),
        };
        let help = self.call(&route, &["--help"], vec![])?;
        let text = std::str::from_utf8(&help).map_err(|_| "incompatible_ownership_cli")?;
        let has = |marker: &str| text.lines().any(|line| line.trim() == marker);
        if !has(wire::HELP_MARKER)
            && pending.is_none()
            && self.owner.mode == Mode::Native
            && self.action == Some(Action::NativeTakeover)
            && self.owner.state().retired_native.is_none()
        {
            // Existing Unsupported compatibility, only before any ownership
            // intent. A disappeared marker cannot erase an uncertain original.
            self.owner.state().legacy = Some(route);
            return Err("ownership_unsupported");
        }
        if ![wire::HELP_MARKER, ui_requests::HELP_MARKER]
            .iter()
            .all(|m| has(m))
        {
            return Err("incompatible_ownership_cli");
        }
        if let Some((request, configuration, exit_confirmed, _)) = pending {
            // A fresh query is made AFTER observing the original child's exit.
            // Never reuse a negative lookup received before that observation.
            let query = Query::for_request(
                uuid::Uuid::new_v4().to_string(),
                configuration.clone(),
                &request,
            )
            .ok_or("invalid_ownership_query")?;
            let bytes = self.call(
                &route,
                &["desktop", "ownership-result", "--json"],
                serde_json::to_vec(&query).map_err(|_| "invalid_ownership_query")?,
            )?;
            let lookup: wire::Lookup =
                serde_json::from_slice(&bytes).map_err(|_| "invalid_ownership_response")?;
            if !lookup.matches(&query) {
                return Err("invalid_ownership_response");
            }
            if let Some(receipt) = lookup.receipt {
                return self.settle(receipt, &request, &configuration);
            }
            let may_refresh = {
                let state = self.owner.state();
                state.retired_native.is_none()
                    && state
                        .intent
                        .as_ref()
                        .is_some_and(|intent| intent.receipt.is_none())
            };
            if self.refresh_absent_native
                && may_refresh
                && exit_confirmed
                && self.owner.mode == Mode::Native
                && matches!(
                    &request,
                    Request::DesktopOwnership {
                        action: Action::NativeTakeover,
                        journal: None,
                        revision: 0,
                        ..
                    }
                )
            {
                let bytes =
                    self.call(&route, &["desktop", "ownership-status", "--json"], vec![])?;
                let status: Status =
                    serde_json::from_slice(&bytes).map_err(|_| "invalid_ownership_status")?;
                if status.matches(&configuration)
                    && status.journal.is_none()
                    && status.revision == 0
                    && status.state == wire::State::Passive
                    && !status.rearm_required
                    && status.request_id.is_none()
                    && status.owner_epoch.is_none()
                    && !status.identity_current
                {
                    // This observation releases NO writer. It admits at most
                    // one new Root-authenticated NativeTakeover, whose exact
                    // original result and actual resource drain must settle.
                    let mut state = self.owner.state();
                    state.retired_native = state.intent.take();
                    state.status = Some(status);
                    state.code = "ownership_fresh_native_takeover".into();
                    return Ok(());
                }
            }
            let mut state = self.owner.state();
            if lookup.definitive
                && exit_confirmed
                && matches!(
                    &request,
                    Request::DesktopOwnership {
                        journal: Some(_),
                        ..
                    }
                )
            {
                if let Some(intent) = &mut state.intent {
                    intent.complete = true;
                    intent.not_applied = true;
                }
                state.code = "ownership_not_applied".into();
            } else {
                state.code = "ownership_unconfirmed".into();
            }
            return Ok(());
        }
        // This explicit mutation only tightens the owned legacy state directory;
        // passive status/lookup never changes permissions. It precedes the first
        // native intent and retains the same selected helper/resource bounds.
        if self.owner.mode == Mode::Native
            && self.action == Some(Action::NativeTakeover)
            && has(wire::PREPARE_STATE_HELP_MARKER)
        {
            self.call(&route, &["desktop", "prepare-state", "--json"], vec![])?;
        }
        let bytes = self.call(&route, &["desktop", "ownership-status", "--json"], vec![])?;
        let status: Status =
            serde_json::from_slice(&bytes).map_err(|_| "invalid_ownership_status")?;
        let expected = self
            .configuration
            .as_deref()
            .unwrap_or(&status.configuration);
        if !status.matches(expected) {
            return Err("ownership_configuration_changed");
        }
        let responder = self
            .owner
            .responder
            .as_ref()
            .ok_or("ownership_identity_unavailable")?;
        match responder.cached_configuration() {
            Some(current) if current != status.configuration => {
                return Err("ownership_configuration_changed")
            }
            None if !responder.configure_once(status.configuration.clone()) => {
                return Err("ownership_configuration_changed");
            }
            _ => (),
        }
        self.owner.state().status = Some(status.clone());
        if self
            .basis
            .as_ref()
            .is_some_and(|basis| !basis.holds(&status))
        {
            // The definitive negative no longer describes the authority.
            self.owner.state().code = "ownership_changed".into();
            return Ok(());
        }
        if self.keep_epoch.as_ref().is_some_and(|g| g.0 != status) {
            self.owner.state().code = "ownership_changed".into();
            return Ok(());
        }
        let Some(action) = self.action else {
            self.owner.state().code.clear();
            return Ok(());
        };
        if action == Action::Grant && status.rearm_required {
            self.owner.state().code = "ownership_rearm_required".into();
            return Ok(());
        }
        // All slow help/status work has finished. Check the SAME confirmed
        // target before reserving any ownership original, then again at spawn.
        if let Some(check) = &self.effect_check {
            check()?;
        }
        if self.stopped() {
            return Err("ownership_stopped");
        }
        let request = Request::DesktopOwnership {
            request_id: uuid::Uuid::new_v4().to_string(),
            action,
            journal: status.journal.clone(),
            revision: status.revision,
        };
        // Reserve the exact logical intent before any original mutating child.
        let mut state = self.owner.state();
        // A fresh logical request gets its own first diagnostic. Read-only
        // lookups of an uncertain original keep that original's first failure.
        if self.owner.mode == Mode::Service {
            state.diagnostic = None;
        }
        state.intent = Some(Intent {
            request: request.clone(),
            configuration: status.configuration.clone(),
            route: route.clone(),
            exit_confirmed: false,
            complete: false,
            receipt: None,
            not_applied: false,
        });
        drop(state);
        if self.owner.mode == Mode::Service {
            self.owner.record_view(Phase::FrontendRequestPrepared);
        }
        let registration =
            match responder.prepare_ui_request(uuid::Uuid::new_v4().to_string(), request.clone()) {
                Ok(registration) => registration,
                Err(_) => {
                    self.owner.state().intent.as_mut().unwrap().complete = true;
                    return Err("ownership_identity_unavailable");
                }
            };
        let input = match serde_json::to_vec(registration.envelope()) {
            Ok(input) => input,
            Err(_) => {
                self.owner.state().intent.as_mut().unwrap().complete = true;
                return Err("invalid_ownership_request");
            }
        };
        let helper = match route.executable() {
            Ok(helper) => helper,
            Err(code) => {
                self.owner.state().intent.as_mut().unwrap().complete = true;
                return Err(code);
            }
        };
        let held: Arc<dyn Send + Sync> = Arc::new((
            self.resources.clone(),
            self.owner.clone(),
            route.clone(),
            registration.resources(),
            self.effect_check.clone(),
        ));
        let spawned = Arc::new(AtomicBool::new(false));
        let original = spawned.clone();
        let mut call = invocation(&["desktop", "frontend-request", "--json"], input);
        if let Err(code) = self.bound(&mut call) {
            self.owner.state().intent.as_mut().unwrap().complete = true;
            return Err(code);
        }
        let check = self.effect_check.clone();
        let check_error = Arc::new(Mutex::new(None));
        let original_error = check_error.clone();
        let result = run_owned_observed(
            helper,
            &call,
            &[],
            &self.cancel,
            move |command| {
                // The target/cancellation fence survives registration and any
                // later ownership-side wait. No mutating child on refusal.
                if let Some(check) = check {
                    if let Err(code) = check() {
                        *original_error
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner) = Some(code);
                        return Err(std::io::Error::other(code));
                    }
                }
                let child = command.spawn()?;
                original.store(true, Ordering::Release);
                let _ = registration.bind_original(&child);
                Ok(child) // Always return the original, including a refused binding.
            },
            held,
            |error, stderr| self.failure("native_takeover.frontend_request", error, stderr),
        );
        {
            let mut state = self.owner.state();
            let intent = state.intent.as_mut().unwrap();
            if spawned.load(Ordering::Acquire) {
                intent.exit_confirmed = !matches!(result, Err(OwnedError::ExitUnverified));
            } else {
                intent.complete = true;
            }
        }
        if let Some(code) = *check_error.lock().unwrap_or_else(PoisonError::into_inner) {
            return Err(code);
        }
        let bytes = result.map_err(|_| "ownership_unconfirmed")?;
        route.executable()?;
        if self.stopped() || bytes.len() > wire::BYTES {
            return Err("ownership_unconfirmed");
        }
        let receipt: Receipt = serde_json::from_slice(&bytes).map_err(|_| {
            if let Some(code) = response_refusal(&bytes, request.id()) {
                self.owner
                    .state()
                    .diagnostic
                    .get_or_insert(("native_takeover.frontend_response", code));
            }
            "ownership_unconfirmed"
        })?;
        self.settle(receipt, &request, &status.configuration)
    }
    fn settle(
        &self,
        receipt: Receipt,
        request: &Request,
        configuration: &str,
    ) -> Result<(), &'static str> {
        if !receipt.matches(request, configuration) {
            return Err("ownership_receipt_mismatch");
        }
        let mut state = self.owner.state();
        state.code = match &receipt.outcome {
            Outcome::Pending => "ownership_unconfirmed".into(),
            Outcome::Refused { code } => code.clone(),
            Outcome::Granted { .. } if receipt.service_granted() => String::new(),
            _ if receipt.native_ready() => String::new(),
            _ => "ownership_changed".into(),
        };
        state.status = Some(receipt.current.clone());
        let intent = state.intent.as_mut().ok_or("ownership_intent_missing")?;
        intent.complete = !matches!(receipt.outcome, Outcome::Pending);
        intent.receipt = Some(receipt);
        Ok(())
    }
}

/// Not serializable or constructible from a webview/status snapshot.
pub(crate) struct NativeReady {
    helper: Arc<relay::Connection>,
    // None only for a selected CLI's successful marker-absent legacy probe.
    _receipt: Option<Receipt>,
    responder: Option<Arc<Responder>>,
}
impl NativeReady {
    pub(crate) fn helper(&self) -> Result<&Path, &'static str> {
        self.helper.executable()
    }

    /// Native-only source selection for explicit program cleanup. A successful
    /// legacy path check is not the current native ownership origin required
    /// by the new pipeline. This selection supplies no stop permission.
    #[allow(dead_code)] // The new native pipeline consumes this opaque source.
    pub(crate) fn program_source(&self) -> Result<NativeSource, &'static str> {
        let original = self
            ._receipt
            .as_ref()
            .filter(|receipt| receipt.native_ready())
            .ok_or("worktree_programs_native_origin_unavailable")?;
        if crate::desktop_service::mode() {
            return Err("worktree_programs_native_origin_changed");
        }
        self.helper.executable()?;
        let environment = zerus_client_core::run::FrozenEnvironment::capture()
            .map_err(|_| "worktree_programs_native_environment_unavailable")?;
        Ok(NativeSource {
            helper: self.helper.clone(),
            original: original.clone(),
            environment,
            responder: self
                .responder
                .clone()
                .ok_or("worktree_programs_native_responder_unavailable")?,
        })
    }
}

/// Constructed only from the actual guarded Native startup/takeover result.
/// Not serialized, not a cached mode flag and not a stop/Force permit.
#[allow(dead_code)] // Consumed by the live program pipeline, never legacy dispatch.
pub(crate) struct NativeSource {
    helper: Arc<relay::Connection>,
    original: Receipt,
    environment: zerus_client_core::run::FrozenEnvironment,
    responder: Arc<Responder>,
}
#[allow(dead_code)]
impl NativeSource {
    pub(crate) fn prepare_program_request(
        &self,
        request: zerus_client_core::desktop::ui_requests::Request,
    ) -> Result<zerus_client_core::desktop::ui_requests::Registration, &'static str> {
        use zerus_client_core::desktop::ui_requests::Request;
        let input = match &request {
            Request::WorktreeProgramStop { intent } => &intent.input,
            Request::WorktreeProgramCleanup { intent } => &intent.input.stop,
            _ => return Err("worktree_programs_native_request_invalid"),
        };
        if !self.route_current()
            || input.manifest.configuration != self.original.configuration
            || input.source
                != (zerus_client_core::worktree_programs::stop::Source::Native {
                    request_id: self.original.request_id.clone(),
                    journal: self.original.journal.clone(),
                    revision: self.original.revision,
                })
        {
            return Err("worktree_programs_native_origin_changed");
        }
        self.responder
            .prepare_ui_request(uuid::Uuid::new_v4().to_string(), request)
            .map_err(|_| "worktree_programs_native_registration_refused")
    }
    pub(crate) fn route_current(&self) -> bool {
        !crate::desktop_service::mode()
            && self.original.native_ready()
            && self.environment.current()
            && self.helper.executable().is_ok()
    }
    pub(crate) fn configuration(&self) -> &str {
        &self.original.configuration
    }
    pub(crate) fn connection(&self) -> Result<Arc<relay::Connection>, &'static str> {
        self.route_current()
            .then(|| self.helper.clone())
            .ok_or("worktree_programs_native_origin_changed")
    }
    pub(crate) fn environment(&self) -> &zerus_client_core::run::FrozenEnvironment {
        &self.environment
    }
    pub(crate) fn original_ownership(&self) -> &Receipt {
        &self.original
    }
}
/// Called after canonical claim, before Bootstrap or notifier construction.
pub(crate) struct NativeRecovery {
    // Keep the exact uncertain intent, original child/pipe charges and route.
    // A new consumer controller cannot replay or replace this original.
    _owner: Arc<Controller>,
    pub code: &'static str,
    /// Task 180: the first failure's code (the check that refused), which the
    /// app shows with its next step; `code` stays the takeover's own result.
    pub reason: &'static str,
}

impl NativeRecovery {
    pub(crate) fn close(&self) -> Closed {
        self._owner.close()
    }
}

pub(crate) fn native_takeover_recoverable(
    hgs: PathBuf,
    responder: Arc<Responder>,
) -> Result<NativeReady, NativeRecovery> {
    native_takeover_until(
        Controller::new(hgs, Some(responder), Mode::Native),
        Instant::now() + Duration::from_secs(15),
    )
}

fn native_takeover_until(
    owner: Arc<Controller>,
    deadline: Instant,
) -> Result<NativeReady, NativeRecovery> {
    match native_startup(owner.clone(), deadline) {
        Ok(ready) => Ok(ready),
        Err(code) => {
            let diagnostic = owner
                .state()
                .diagnostic
                .unwrap_or(("native_takeover", code));
            crate::startup_error::record(diagnostic.0, diagnostic.1);
            owner.stop();
            Err(NativeRecovery {
                _owner: owner,
                code,
                reason: diagnostic.1,
            })
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
fn native_takeover(hgs: PathBuf, responder: Arc<Responder>) -> Result<NativeReady, &'static str> {
    native_takeover_recoverable(hgs, responder).map_err(|failure| failure.code)
}

// A matching closed negative explains a refusal; it cannot complete, replay or
// grant an uncertain ownership intent. Child-selected text is never published.
fn response_refusal(bytes: &[u8], original: &str) -> Option<&'static str> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Refusal {
        request_id: String,
        accepted: bool,
        durable: bool,
        code: String,
    }
    let refusal: Refusal = serde_json::from_slice(bytes).ok()?;
    if refusal.request_id != original || refusal.accepted || refusal.durable {
        return None;
    }
    match refusal.code.as_str() {
        "frontend_authentication_refused" => Some("frontend_authentication_refused"),
        "frontend_identity_expired" => Some("frontend_identity_expired"),
        "ownership_not_settled" => Some("ownership_not_settled"),
        _ => None,
    }
}

fn failure_code(error: &OwnedError, stderr: Option<&str>) -> &'static str {
    // Child text has no authority and is never written to the diagnostic.
    // Exact known messages select codes; arbitrary/partial text stays generic.
    if *error == OwnedError::ChildFailed {
        match stderr.map(str::trim) {
            Some("hgs: frontend process image unavailable") => {
                return "frontend_process_image_unavailable"
            }
            Some("hgs: frontend process identity unavailable") => {
                return "frontend_process_identity_unavailable"
            }
            Some("hgs: agent-descended frontend request refused") => {
                return "agent_descended_frontend_refused"
            }
            Some("hgs: canonical frontend evidence unavailable") => {
                return "canonical_frontend_evidence_unavailable"
            }
            // Task 180: the portable binding's checks, by their exact lines.
            Some(line) => {
                if let Some(code) = crate::takeover_refusal::code(line) {
                    return code;
                }
            }
            None => (),
        }
    }
    match error {
        OwnedError::InvalidInput => "invalid_input",
        OwnedError::Cancelled => "cancelled",
        OwnedError::TimedOut => "timed_out",
        OwnedError::StartFailed => "start_failed",
        OwnedError::WriteFailed => "write_failed",
        OwnedError::ChildFailed => "child_failed",
        OwnedError::OutputTooLarge => "output_too_large",
        OwnedError::Incomplete => "incomplete",
        OwnedError::ExitUnverified => "exit_unverified",
    }
}
fn native_startup(owner: Arc<Controller>, deadline: Instant) -> Result<NativeReady, &'static str> {
    let mut action = Some(Action::NativeTakeover);
    loop {
        let mut job = owner
            .admit(action, None, None)
            .ok_or("ownership_transport_busy")?;
        job.deadline = Some(deadline);
        job.refresh_absent_native = true;
        job.run();
        drop(job);
        if Instant::now() >= deadline {
            return Err("ownership_takeover_unconfirmed");
        }
        let view = owner.view();
        let ready = {
            let state = owner.state();
            state
                .legacy
                .as_ref()
                .map(|route| (None, route.clone()))
                .or_else(|| {
                    state
                        .intent
                        .as_ref()
                        .and_then(|i| i.receipt.clone().map(|r| (r, i.route.clone())))
                        .filter(|(r, _)| {
                            r.native_ready() && state.status.as_ref() == Some(&r.current)
                        })
                        .map(|(receipt, route)| (Some(receipt), route))
                })
        };
        if let Some((receipt, helper)) = ready {
            if !matches!(owner.gate.close().wait_until(deadline), Observation::Idle) {
                return Err("ownership_resources_pending");
            }
            helper.executable()?;
            return Ok(NativeReady {
                helper,
                _receipt: receipt,
                responder: owner.responder.clone(),
            });
        }
        let refresh = {
            let state = owner.state();
            state.intent.is_none() && state.retired_native.is_some()
        };
        if refresh {
            action = Some(Action::NativeTakeover);
            continue;
        }
        if !view.pending || Instant::now() >= deadline {
            return Err("ownership_takeover_unconfirmed");
        }
        action = None; // A pending original is queried, never resent.
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests;
