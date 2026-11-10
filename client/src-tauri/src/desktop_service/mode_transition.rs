//! Native registration orchestration, separate from notification click reads.
//! A helper result is routing data. Only the maintained helper's original
//! caller/current Grant checks admit manager effects; no JSON grants authority.
use super::*;
use serde::Serialize;
use zerus_client_core::{
    desktop::drain::Charge,
    run::{run_owned, OwnedError},
    Invocation,
};

const ABI: &str = "desktop-service-activation-v1";
const MARKER: &str = "Desktop service activation ABI: desktop-service-activation-v1";
const CHOICE_MARKER: &str =
    "Desktop service mode choice scope ABI: desktop-service-mode-choice-scope-v1";
const BYTES: usize = 64 << 10;
pub(super) mod classification;
mod first;
pub(super) mod refresh;
mod review;
pub(super) use first::{Prepared as FirstPrepared, View as FirstView};
pub(super) use review::{Action as ReviewAction, Prepared, View as ReviewView};
type Resources = Arc<dyn Send + Sync>;

/// Read-only source selection before canonical startup. The same maintained
/// helper validates its registration; neither files nor CLI lifecycle alone
/// establish one. A later Service Grant still supplies all effect authority.
pub(super) fn registered_for_default(ordinary: &std::path::Path) -> bool {
    let owner = Controller::new();
    let Ok(route) = owner.select_route(ordinary).map(Arc::new) else {
        return false;
    };
    let Ok(charge) = owner.gate.admit(Role::Fleet) else {
        return false;
    };
    let resources: Resources = Arc::new(charge);
    let Some(help) = owner.call_bounded(
        route.clone(),
        vec![route.helper.to_string_lossy().into_owned(), "--help".into()],
        resources.clone(),
        Duration::from_secs(3),
    ) else {
        return false;
    };
    if !std::str::from_utf8(&help).is_ok_and(|text| text.lines().any(|line| line.trim() == MARKER))
    {
        return false;
    }
    let Some(raw) = owner.call_bounded(
        route.clone(),
        route.arguments(None, false, false),
        resources,
        Duration::from_secs(3),
    ) else {
        return false;
    };
    let Ok(status) = serde_json::from_slice::<Value>(&raw) else {
        return false;
    };
    let label = if cfg!(target_os = "linux") {
        "hgs-desktop.service"
    } else {
        "com.hgdev.hgs-desktop"
    };
    route.check().is_ok()
        && status["schema"] == 1
        && status["operation"] == "status"
        && status["registered"] == true
        && status["label"] == label
        && status["configuration"].as_str().is_some_and(|value| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        && matches!(
            status["selected_mode"].as_str(),
            Some("passive" | "background")
        )
}

#[derive(Clone, Default, Serialize)]
pub(super) struct View {
    pub pending: bool,
    pub running: bool,
    pub needs_resume: bool,
    pub code: String,
    /// The refused native check's own words beside `code`, e.g. why the
    /// pinned image could not be opened (task #59). Presentation only.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub reason: String,
    pub review: Option<review::View>,
    pub automatic_blocked: bool,
    pub request_id: Option<String>,
    pub active: bool,
    /// A first explicit background choice offer (Windows, zerus-yfbd.1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first: Option<first::View>,
}
/// One checked status's latch: the durable native latch stays for this
/// process; an explicit journal loss lasts only while the latest coherent
/// status reports it, so a later healthy read recovers.
fn latch(state: &mut State, status: &Value) {
    state.first_lost = !status["first_choice_lost"].is_null();
    if !state.first_lost {
        state.latched |= status["activation_automatic_blocked"] == true;
    }
    state.view.automatic_blocked = state.latched || state.first_lost;
}
struct Intent {
    id: String,
    configuration: String,
    route: Arc<Route>,
    complete: bool,
    joined: bool,
    /// A first explicit choice (zerus-yfbd.1): only a closed no-spawn fact
    /// or an exact refused receipt ends it as not started.
    first: bool,
    /// The original effect child was never spawned (pin or spawn-time
    /// refusal before the process existed). Never inferred from a join.
    no_spawn: bool,
}
#[derive(Default)]
struct State {
    view: View,
    intent: Option<Intent>,
    selection: Option<Value>,
    review: Option<review::Target>,
    review_capable: bool,
    choice_capable: bool,
    business: Option<review::Business>,
    classification_required: bool,
    observation_only: bool,
    first: Option<first::Target>,
    first_capable: bool,
    /// The durable native latch, sticky for this process. It never includes
    /// a journal loss, which lasts only while the latest read reports it.
    latched: bool,
    first_lost: bool,
    refresh: refresh::State,
}
pub(super) struct Controller {
    state: Mutex<State>,
    gate: Gate,
    stop: AtomicBool,
    #[cfg(all(test, target_os = "linux"))]
    helper_directory: Mutex<Option<PathBuf>>,
    #[cfg(all(test, target_os = "linux"))]
    windows_fixture: Mutex<Option<PathBuf>>,
}
impl Controller {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State::default()),
            gate: Gate::new(1).unwrap(),
            stop: AtomicBool::new(false),
            #[cfg(all(test, target_os = "linux"))]
            helper_directory: Mutex::new(None),
            #[cfg(all(test, target_os = "linux"))]
            windows_fixture: Mutex::new(None),
        })
    }
    #[cfg(all(test, target_os = "linux"))]
    pub(super) fn fixture_helpers(&self, directory: Option<PathBuf>) {
        *self.helper_directory.lock().unwrap() = directory;
    }
    /// A Windows-shaped route over a fake registration CLI, so the
    /// first-choice controller runs on Linux CI. Test-only.
    #[cfg(all(test, target_os = "linux"))]
    pub(super) fn fixture_windows(&self, cli: Option<PathBuf>) {
        *self.windows_fixture.lock().unwrap() = cli;
    }
    fn select_route(&self, ordinary: &std::path::Path) -> Result<Route, Refusal> {
        #[cfg(all(test, target_os = "linux"))]
        if let Some(cli) = self.windows_fixture.lock().unwrap().clone() {
            let relay = Arc::new(zerus_client_core::desktop::relay::Connection::select(&cli)?);
            return Ok(Route {
                relay,
                helper: cli.clone(),
                cli,
                windows: true,
                files: Vec::new(),
            });
        }
        #[cfg(all(test, target_os = "linux"))]
        if let Some(directory) = self.helper_directory.lock().unwrap().clone() {
            let relay = Arc::new(zerus_client_core::desktop::relay::Connection::select(
                ordinary,
            )?);
            let cli = relay.executable()?.to_owned();
            return Route::open(relay, cli, directory);
        }
        Route::select(ordinary)
    }
    pub fn view(&self) -> View {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let mut view = state.view.clone();
        // Classification may recover eligibility, but the deferred connection
        // still needs the existing explicit Resume before scheduling work.
        if state.observation_only
            && !view.pending
            && !view.automatic_blocked
            && view.review.is_none()
            && state.selection.as_ref().is_some_and(|s| {
                s["selected_mode"] == "background" && s["running_mode"] == "background"
            })
        {
            view.needs_resume = true;
        }
        view
    }
    pub fn close(&self) -> zerus_client_core::desktop::drain::Closed {
        self.stop.store(true, Ordering::Release);
        self.gate.close()
    }
    #[cfg(all(test, target_os = "linux"))]
    pub fn pending(&self) -> bool {
        self.view().pending
    }
    fn automatic(&self) -> bool {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        !state.view.pending
            && !state.view.automatic_blocked
            && state.view.review.is_none()
            && state.selection.as_ref().is_some_and(|s| {
                s["selected_mode"] == "passive"
                    && s["running_mode"] == "passive"
                    && s["mode_selection"] == "default"
                    && s["ipc_ready"] == true
                    && s["pending_restart"] == false
                    && s["transition"].is_null()
            })
    }
    pub async fn observe(self: &Arc<Self>, ordinary: PathBuf, configuration: String) {
        self.exchange(ordinary, configuration, false, false).await;
    }
    pub async fn activate(self: &Arc<Self>, ordinary: PathBuf, configuration: String) {
        self.exchange(ordinary, configuration, true, false).await;
    }
    async fn activate_default(self: &Arc<Self>, ordinary: PathBuf, configuration: String) {
        self.exchange(ordinary, configuration, true, true).await;
    }
    async fn exchange(
        self: &Arc<Self>,
        ordinary: PathBuf,
        configuration: String,
        activate: bool,
        default_only: bool,
    ) -> bool {
        let Ok(charge) = self.gate.admit(Role::Fleet) else {
            return false;
        };
        if self.stop.load(Ordering::Acquire) {
            return false;
        }
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .view
            .running = true;
        let owner = self.clone();
        let joined = tauri::async_runtime::spawn_blocking(move || {
            owner.run(ordinary, configuration, activate, default_only, charge)
        })
        .await
        .is_ok();
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.view.running = false;
        joined && state.selection.is_some()
    }
    fn run(
        self: &Arc<Self>,
        ordinary: PathBuf,
        configuration: String,
        activate: bool,
        default_only: bool,
        charge: Charge,
    ) {
        let resources = Arc::new(charge);
        let existing = {
            let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state
                .intent
                .as_ref()
                .filter(|i| !i.complete)
                .map(|i| (i.id.clone(), i.configuration.clone(), i.route.clone()))
        };
        if let Some((id, scope, route)) = existing {
            if scope != configuration {
                self.failed("activation_scope_changed");
                return;
            }
            if self
                .status(route.clone(), &scope, resources.clone())
                .is_some()
            {
                self.lookup(id, scope, route, resources);
            }
            return;
        }
        let route = match self.select_route(&ordinary) {
            Ok(route) => Arc::new(route),
            Err(refusal) => {
                self.refused(refusal);
                return;
            }
        };
        let Some(help) = self.call_bounded(
            route.clone(),
            route.help_arguments(),
            resources.clone(),
            Duration::from_secs(3),
        ) else {
            return;
        };
        if !std::str::from_utf8(&help)
            .is_ok_and(|s| s.lines().any(|line| line.trim() == route.marker()))
        {
            self.failed("activation_helper_update_required");
            return;
        }
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .review_capable = std::str::from_utf8(&help).is_ok_and(|s| {
            s.contains(if route.windows {
                "desktop-registration-mode-review-v1"
            } else {
                "review-activation"
            })
        });
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .choice_capable =
            std::str::from_utf8(&help).is_ok_and(|s| s.lines().any(|l| l.trim() == CHOICE_MARKER));
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .first_capable = route.windows
            && std::str::from_utf8(&help)
                .is_ok_and(|s| s.lines().any(|l| l.trim() == first::MARKER));
        let Some(status) = self.status(route.clone(), &configuration, resources.clone()) else {
            return;
        };
        // A restarted consumer adopts only the stored original ID for lookup,
        // never an execution/completion object or the old Grant's authority.
        if let Some(id) = status["activation_pending"].as_str() {
            if !uuid::Uuid::parse_str(id)
                .is_ok_and(|value| !value.is_nil() && value.to_string() == id)
            {
                self.failed("activation_status_unavailable");
                return;
            }
            self.reserve(id.into(), configuration.clone(), route.clone());
            self.lookup(id.into(), configuration, route, resources);
            return;
        }
        if self.blocks_resume() {
            return;
        }
        if !activate
            || (status["selected_mode"] == "background"
                && status["running_mode"] == "background"
                && status["ipc_ready"] == true
                && status["pending_restart"] == false)
        {
            return;
        }
        if default_only
            && !(status["selected_mode"] == "passive"
                && status["running_mode"] == "passive"
                && status["mode_selection"] == "default"
                && status["ipc_ready"] == true
                && status["pending_restart"] == false
                && status["transition"].is_null())
        {
            return;
        }
        // Windows ordinary activation remains the maintained manual route.
        // This slice adds only the distinct post-review Settings choice, whose
        // native helper supports a caller-retained UUID and after-review fence.
        if route.windows {
            self.failed("activation_platform_manual");
            return;
        }
        let id = uuid::Uuid::new_v4().to_string();
        self.reserve(id.clone(), configuration.clone(), route.clone());
        let Some(bytes) = self.call_effect(
            route.clone(),
            route.arguments(Some(&id), true, default_only),
            resources,
            &id,
        ) else {
            return;
        };
        self.result(&bytes, &id, &configuration);
    }
    fn reserve(&self, id: String, configuration: String, route: Arc<Route>) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.intent = Some(Intent {
            id,
            configuration,
            route,
            complete: false,
            joined: false,
            first: false,
            no_spawn: false,
        });
        state.view.pending = true;
        state.view.request_id = state.intent.as_ref().map(|i| i.id.clone());
        state.view.code = "activation_unconfirmed".into();
    }
    fn status(
        self: &Arc<Self>,
        route: Arc<Route>,
        configuration: &str,
        resources: Resources,
    ) -> Option<Value> {
        let raw = self.call_bounded(
            route.clone(),
            route.arguments(None, false, false),
            resources,
            Duration::from_secs(3),
        )?;
        let status = match serde_json::from_slice::<Value>(&raw) {
            Ok(value) => route.status(value),
            Err(_) => {
                self.failed("activation_status_unavailable");
                return None;
            }
        };
        if !valid_status(&status, configuration) {
            self.failed("activation_status_unavailable");
            return None;
        }
        if route.windows && !status["first_choice_lost"].is_null() {
            // The native journal lost an anchored first choice: explicitly
            // unavailable. Never an offer, a Resume or an inferred NotStarted;
            // an own unsettled intent keeps its pending original ID.
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state.first = None;
            state.view.first = None;
            latch(&mut state, &status);
            state.view.needs_resume = false;
            state.selection = Some(status.clone());
            if state.intent.as_ref().is_none_or(|i| i.complete) {
                state.view.code = "activation_first_choice_lost".into();
            }
            return Some(status);
        }
        let capable = self
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .review_capable;
        if capable {
            if let Err(code) =
                self.record_review(route.clone(), configuration, &status, route.windows)
            {
                self.failed(code);
                return None;
            }
        }
        self.project(&status, route.windows);
        if route.windows {
            self.record_first(route, configuration, &status);
        }
        Some(status)
    }
    /// The view one checked registration status leaves. Every path that reads
    /// it (observe, Check result, reconnect, before an activation) projects the
    /// same state, so a later read never undoes an earlier one.
    fn project(&self, status: &Value, windows: bool) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        latch(&mut state, status);
        state.selection = Some(status.clone());
        state.view.active = status["selected_mode"] == "background"
            && status["running_mode"] == "background"
            && status["ipc_ready"] == true
            && status["pending_restart"] == false;
        state.view.needs_resume = !windows
            && state.view.review.is_none()
            && !state.view.automatic_blocked
            && status["selected_mode"] == "passive";
        if state.intent.is_none() {
            // Windows ordinary activation is the maintained manual route: an
            // activation attempt reaches the same code before any effect. Only
            // a plain passive selection says so; an interrupted record, a latch
            // or any transition keeps its own presentation.
            let manual = windows
                && state.view.review.is_none()
                && !state.view.automatic_blocked
                && status["selected_mode"] == "passive"
                && status["running_mode"] == "passive"
                && status["pending_restart"] == false
                && status["transition"].is_null()
                && status["activation_pending"].is_null();
            state.view.code = if state.view.needs_resume {
                "activation_required"
            } else if manual {
                "activation_platform_manual"
            } else {
                ""
            }
            .into();
        }
    }
    /// One checked status, as `status()` projects it after its native read.
    #[cfg(test)]
    pub(super) fn project_for_test(&self, status: &Value, windows: bool) {
        self.project(status, windows);
    }
    fn lookup(
        self: &Arc<Self>,
        id: String,
        configuration: String,
        route: Arc<Route>,
        resources: Resources,
    ) {
        let Some(bytes) = self.call(
            route.clone(),
            route.arguments(Some(&id), false, false),
            resources,
        ) else {
            return;
        };
        let bytes = route.result(&bytes, &id, &configuration);
        if let Some(bytes) = bytes {
            self.result(&bytes, &id, &configuration);
        } else {
            self.failed("activation_unconfirmed");
        }
    }
    fn result(&self, bytes: &[u8], id: &str, configuration: &str) {
        let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
            self.failed("activation_unconfirmed");
            return;
        };
        if value["abi"] != ABI
            || value["request_id"] != id
            || value["configuration"] != configuration
            || value["read_only"] != true
        {
            self.failed("activation_unconfirmed");
            return;
        }
        let settled =
            value["phase"] == "settled" && value["result"] == json!({"ok":true,"code":"ready"});
        let refused = value["phase"] == "refused"
            && value["result"] == json!({"ok":false,"code":"activation_not_started"});
        // Missing after this original exchange physically joined proves no
        // helper reservation, not success. It offers another explicit action.
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if value["phase"] == "lost" {
            // The native journal lost an anchored first choice: this original
            // stays pending (Unknown). Never NotStarted, an offer or a Resume.
            if state
                .intent
                .as_ref()
                .is_some_and(|i| i.id == id && i.configuration == configuration && !i.complete)
            {
                state.first = None;
                state.view.first = None;
                state.first_lost = true;
                state.view.automatic_blocked = true;
                state.view.pending = true;
                state.view.request_id = Some(id.into());
                state.view.needs_resume = false;
                state.view.code = "activation_first_choice_lost".into();
            }
            return;
        }
        let Some(intent) = state
            .intent
            .as_mut()
            .filter(|i| i.id == id && i.configuration == configuration)
        else {
            return;
        };
        // A first choice may have run Stop/Update/RunEx before its journal
        // went missing: a join settles resources, it does not prove no start.
        let closed = if intent.first {
            intent.no_spawn
        } else {
            intent.joined
        };
        let absent = closed && value["phase"] == "missing" && value["result"].is_null();
        intent.complete = settled || refused || absent;
        state.view.pending = !(settled || refused || absent);
        state.view.needs_resume = !settled && state.view.review.is_none();
        state.view.code = if settled {
            ""
        } else if refused || absent {
            "activation_not_started"
        } else {
            "activation_unconfirmed"
        }
        .into();
    }
    /// The code a Windows activation attempt leaves before any effect.
    #[cfg(test)]
    pub(super) fn platform_manual_for_test(&self) {
        self.failed("activation_platform_manual");
    }
    /// A refused route keeps its code and the refused check's own words
    /// (task #59), in the view and in the log.
    fn refused(&self, refusal: Refusal) {
        let code = refusal.logged();
        self.failed(code);
        if code != "activation_platform_manual" {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state.view.reason = refusal.reason;
        }
    }
    fn failed(&self, code: &str) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.view.reason.clear();
        // Any failed read closes a first-choice offer until a checked status.
        state.first = None;
        if code == "activation_platform_manual" {
            // No native activation route on this platform: nothing to resume
            // here. The code stays visible so the client can say where the
            // mode is chosen (Settings); it is presentation, never authority.
            state.view = View {
                code: code.into(),
                ..View::default()
            };
            return;
        }
        state.view.first = None;
        state.business = None;
        state.selection = None;
        state.observation_only |= state.classification_required;
        state.view.code = code.into();
        state.view.needs_resume = true;
    }
    fn call(
        self: &Arc<Self>,
        route: Arc<Route>,
        args: Vec<String>,
        resources: Resources,
    ) -> Option<Vec<u8>> {
        self.call_bounded(route, args, resources, Duration::from_secs(30))
    }
    fn call_effect(
        self: &Arc<Self>,
        route: Arc<Route>,
        args: Vec<String>,
        resources: Resources,
        id: &str,
    ) -> Option<Vec<u8>> {
        self.call_effect_checked(route, args, resources, id, None)
    }
    /// `veto` runs at the mutating child's spawn, after every earlier check.
    fn call_effect_checked(
        self: &Arc<Self>,
        route: Arc<Route>,
        args: Vec<String>,
        resources: Resources,
        id: &str,
        veto: Option<Arc<dyn Fn() -> Result<(), &'static str> + Send + Sync>>,
    ) -> Option<Vec<u8>> {
        let (result, joined) =
            self.exchange_bytes(route, args, resources, Duration::from_secs(30), veto);
        if let Some(intent) = self
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .intent
            .as_mut()
            .filter(|i| i.id == id)
        {
            intent.joined = joined;
            intent.no_spawn = matches!(result, Err(OwnedError::StartFailed));
        }
        match result {
            Ok(bytes) => Some(bytes),
            Err(_) => {
                self.failed("activation_unconfirmed");
                None
            }
        }
    }
    fn call_bounded(
        self: &Arc<Self>,
        route: Arc<Route>,
        args: Vec<String>,
        resources: Resources,
        timeout: Duration,
    ) -> Option<Vec<u8>> {
        match self.exchange_bytes(route, args, resources, timeout, None).0 {
            Ok(bytes) => Some(bytes),
            Err(_) => {
                self.failed("activation_unconfirmed");
                None
            }
        }
    }
    fn exchange_bytes(
        self: &Arc<Self>,
        route: Arc<Route>,
        args: Vec<String>,
        resources: Resources,
        timeout: Duration,
        veto: Option<Arc<dyn Fn() -> Result<(), &'static str> + Send + Sync>>,
    ) -> (Result<Vec<u8>, OwnedError>, bool) {
        let inv = Invocation {
            args,
            stdin: Some(Vec::new()),
            timeout,
            request_id: None,
        };
        let selected = match RoutePin::new(route.clone()) {
            Ok(pin) => pin,
            Err(_) => return (Err(OwnedError::StartFailed), true),
        };
        let executable = route.executable();
        let checked = route.clone();
        // The charge, checked route and controller remain with the original
        // child and every pipe, even if this bounded call returns early.
        let held: Arc<dyn Send + Sync> =
            Arc::new((resources, route, self.clone(), selected.clone()));
        let result = run_owned(
            &executable,
            &inv,
            &[],
            &self.stop,
            move |command| {
                selected.check().map_err(std::io::Error::other)?;
                if let Some(veto) = &veto {
                    veto().map_err(std::io::Error::other)?;
                }
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
                }
                command.spawn()
            },
            held,
        );
        // Only a complete original exchange proves its real pipe EOFs. A
        // restarted consumer's lookup child is never that effect original.
        let joined = result.is_ok()
            || matches!(
                result,
                Err(OwnedError::ChildFailed | OwnedError::StartFailed)
            );
        let result = result.and_then(|bytes| {
            if bytes.len() > BYTES {
                Err(OwnedError::OutputTooLarge)
            } else if checked.check().is_err() {
                Err(OwnedError::Incomplete)
            } else {
                Ok(bytes)
            }
        });
        (result, joined)
    }
}

/// Why a route was refused: its stable code for the view and, where a native
/// check refused it, that check's own words. Task #59 (ZERUS/T-8r0u7v): a
/// bare "activation_helper_unavailable" hid why PinnedImage::open refused.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Refusal {
    pub code: &'static str,
    pub reason: String,
}
impl Refusal {
    fn new(code: &'static str, reason: impl std::fmt::Display) -> Self {
        Self {
            code,
            reason: reason.to_string(),
        }
    }
    /// Logs the reason and returns the code, for callers that keep only it.
    pub(super) fn logged(&self) -> &'static str {
        if !self.reason.is_empty() {
            eprintln!("zerus-client: mode transition route refused: {self}");
        }
        self.code
    }
}
impl From<&'static str> for Refusal {
    fn from(code: &'static str) -> Self {
        Self {
            code,
            reason: String::new(),
        }
    }
}
impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.reason.is_empty() {
            f.write_str(self.code)
        } else {
            write!(f, "{}: {}", self.code, self.reason)
        }
    }
}

/// Selected only from the native connection and its maintained installation.
/// Renderer arguments and paths never enter this route.
struct Route {
    relay: Arc<zerus_client_core::desktop::relay::Connection>,
    helper: PathBuf,
    cli: PathBuf,
    windows: bool,
    #[cfg(windows)]
    image_id: zerus_client_core::desktop::windows::image::FileId,
    #[cfg(unix)]
    files: Vec<(std::fs::File, PathBuf, std::fs::Metadata, bool)>,
}
/// The supported registration status shape for this configuration. Every
/// read that can admit or veto an effect checks it, including the final one.
fn valid_status(status: &Value, configuration: &str) -> bool {
    status["schema"] == 1
        && status["configuration"] == configuration
        && matches!(
            status["selected_mode"].as_str(),
            Some("passive" | "background")
        )
        && matches!(
            status["mode_selection"].as_str(),
            Some("default" | "explicit" | "retained")
        )
}

impl Route {
    fn select(ordinary: &std::path::Path) -> Result<Self, Refusal> {
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        {
            let _ = ordinary;
            Err("activation_platform_manual".into())
        }
        #[cfg(windows)]
        {
            let relay = Arc::new(zerus_client_core::desktop::relay::Connection::select(
                ordinary,
            )?);
            let executable = relay.executable()?;
            let image = zerus_client_core::desktop::windows::image::PinnedImage::open(executable)
                .map_err(|error| {
                Refusal::new(
                    "activation_helper_unavailable",
                    format_args!("{} could not be pinned: {error}", executable.display()),
                )
            })?;
            Ok(Self {
                cli: image.path().into(),
                helper: image.path().into(),
                image_id: image.id(),
                relay,
                windows: true,
            })
        }
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            let relay = Arc::new(zerus_client_core::desktop::relay::Connection::select(
                ordinary,
            )?);
            let cli = relay.executable()?.to_owned();
            #[cfg(target_os = "linux")]
            let directory = cli
                .parent()
                .and_then(std::path::Path::parent)
                .ok_or("activation_helper_unavailable")?
                .join("lib/zerus");
            #[cfg(target_os = "macos")]
            let directory = cli
                .parent()
                .and_then(std::path::Path::parent)
                .ok_or("activation_helper_unavailable")?
                .join("Resources/setup");
            Self::open(relay, cli, directory)
        }
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn open(
        relay: Arc<zerus_client_core::desktop::relay::Connection>,
        cli: PathBuf,
        directory: PathBuf,
    ) -> Result<Self, Refusal> {
        #[cfg(target_os = "linux")]
        let name = "install-linux-desktop-service.py";
        #[cfg(target_os = "macos")]
        let name = "install-macos-desktop-service.py";
        let helper = directory.join(name);
        let mut files = Vec::new();
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
        for path in [
            &helper,
            &directory.join("desktop-revoke-hold.py"),
            &directory.join("desktop-service-activation.py"),
            &cli,
        ] {
            let follow = path == &cli; // Maintained CLI/cask links retain their exact target object.
            let no_follow = if follow { 0 } else { libc::O_NOFOLLOW };
            let unavailable = |why: &dyn std::fmt::Display| {
                Refusal::new(
                    "activation_helper_unavailable",
                    format_args!("{}: {why}", path.display()),
                )
            };
            let file = std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(no_follow | libc::O_NONBLOCK | libc::O_CLOEXEC)
                .open(path)
                .map_err(|error| unavailable(&error))?;
            let meta = file.metadata().map_err(|error| unavailable(&error))?;
            let limit = if path == &cli { 512 << 20 } else { 1 << 20 };
            if !meta.is_file()
                || (!follow && meta.nlink() != 1)
                || meta.mode() & 0o022 != 0
                || meta.len() > limit
                || (meta.uid() != unsafe { libc::geteuid() } && meta.uid() != 0)
            {
                return Err(unavailable(&format_args!(
                    "not a private plain file (mode {:o}, links {}, owner {}, {} bytes)",
                    meta.mode() & 0o7777,
                    meta.nlink(),
                    meta.uid(),
                    meta.len()
                )));
            }
            files.push((file, path.to_owned(), meta, follow));
        }
        let route = Self {
            relay,
            helper,
            cli,
            files,
            windows: false,
        };
        route.check()?;
        Ok(route)
    }
    fn check(&self) -> Result<(), &'static str> {
        #[cfg(windows)]
        {
            let image = zerus_client_core::desktop::windows::image::PinnedImage::open(
                self.relay.executable()?,
            )
            .map_err(|_| "activation_helper_changed")?;
            if image.id() != self.image_id || image.path() != self.cli {
                return Err("activation_helper_changed");
            }
            image
                .revalidate()
                .map_err(|_| "activation_helper_changed")?;
        }
        #[cfg(not(windows))]
        if self.relay.executable()? != self.cli {
            return Err("activation_helper_changed");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let same = |a: &std::fs::Metadata, b: &std::fs::Metadata| {
                (
                    a.dev(),
                    a.ino(),
                    a.len(),
                    a.mtime(),
                    a.mtime_nsec(),
                    a.ctime(),
                    a.ctime_nsec(),
                ) == (
                    b.dev(),
                    b.ino(),
                    b.len(),
                    b.mtime(),
                    b.mtime_nsec(),
                    b.ctime(),
                    b.ctime_nsec(),
                )
            };
            for (file, path, original, follow) in &self.files {
                let held = file.metadata().map_err(|_| "activation_helper_changed")?;
                let named = (if *follow {
                    std::fs::metadata(path)
                } else {
                    std::fs::symlink_metadata(path)
                })
                .map_err(|_| "activation_helper_changed")?;
                if !same(original, &held) || !same(original, &named) {
                    return Err("activation_helper_changed");
                }
            }
        }
        Ok(())
    }
    fn marker(&self) -> &'static str {
        if self.windows {
            "Desktop registration mode review ABI: desktop-registration-mode-review-v1"
        } else {
            MARKER
        }
    }
    fn executable(&self) -> PathBuf {
        if self.windows {
            self.cli.clone()
        } else {
            PathBuf::from("python3")
        }
    }
    fn help_arguments(&self) -> Vec<String> {
        if self.windows {
            vec!["--help".into()]
        } else {
            vec![self.helper.to_string_lossy().into_owned(), "--help".into()]
        }
    }
    fn status(&self, mut value: Value) -> Value {
        if self.windows {
            value["activation_pending"] = value["pending_transition"]["request_id"].clone();
            value["mode_selection"] = "explicit".into();
            value["ipc_ready"] = (!value["running_mode"].is_null()).into();
            value["transition"] = value["pending_transition"].clone();
        }
        value
    }
    fn result(&self, bytes: &[u8], id: &str, configuration: &str) -> Option<Vec<u8>> {
        if !self.windows {
            return Some(bytes.to_vec());
        }
        let value: Value = serde_json::from_slice(bytes).ok()?;
        if value["abi"] != "desktop-registration-mode-v1"
            || value["configuration"] != configuration
            || value["request_id"] != id
            || value["read_only"] != true
        {
            return None;
        }
        let receipt = &value["receipt"];
        let phase = if value["code"] == "first_choice_lost" {
            // An explicit journal loss, never an ordinary missing receipt.
            let lost = value["first_choice_lost"].as_str()?;
            if !receipt.is_null()
                || value["outcome"] != "unknown"
                || !uuid::Uuid::parse_str(lost).is_ok_and(|u| !u.is_nil() && u.to_string() == lost)
            {
                return None;
            }
            "lost"
        } else if receipt.is_null() {
            "missing"
        } else {
            if receipt["request"] != id {
                return None;
            }
            match receipt["phase"].as_str()? {
                "settled" => "settled",
                "refused" => "refused",
                "unknown" => "unknown",
                _ => return None,
            }
        };
        serde_json::to_vec(
            &json!({"abi":ABI,"configuration":configuration,"request_id":id,"read_only":true,
            "phase":phase,"result":match phase { "settled"=>json!({"ok":true,"code":"ready"}),
            "refused"=>json!({"ok":false,"code":"activation_not_started"}), _=>Value::Null }}),
        )
        .ok()
    }
    fn arguments(&self, id: Option<&str>, activate: bool, default_only: bool) -> Vec<String> {
        if self.windows {
            let mut args = vec!["desktop".into(), "registration".into()];
            if let Some(id) = id {
                args.extend([
                    "mode-result".into(),
                    "--json".into(),
                    "--request-id".into(),
                    id.into(),
                ]);
            } else {
                args.extend(["status".into(), "--json".into()]);
            }
            return args; // Ordinary Windows activation is deliberately not added here.
        }
        let mut args = vec![self.helper.to_string_lossy().into_owned()];
        #[cfg(target_os = "linux")]
        {
            args.push(
                if activate {
                    "install"
                } else if id.is_some() {
                    "activation-status"
                } else {
                    "status"
                }
                .into(),
            );
            args.extend(["--hgs".into(), self.cli.to_string_lossy().into_owned()]);
        }
        #[cfg(target_os = "macos")]
        {
            args.push(self.cli.to_string_lossy().into_owned());
            if !activate {
                args.push(
                    if id.is_some() {
                        "--activation-status"
                    } else {
                        "--status"
                    }
                    .into(),
                );
            }
        }
        if activate {
            args.extend(["--mode".into(), "background".into()]);
        }
        if let Some(id) = id {
            args.extend(["--activation-id".into(), id.into()]);
        }
        if default_only {
            args.push("--default-only".into());
        }
        args.push("--json".into());
        args
    }
    fn review_arguments(&self, id: &str) -> Vec<String> {
        if self.windows {
            return [
                "desktop",
                "registration",
                "review-mode",
                "--json",
                "--request-id",
                id,
            ]
            .into_iter()
            .map(str::to_owned)
            .collect();
        }
        let mut args = vec![self.helper.to_string_lossy().into_owned()];
        #[cfg(target_os = "linux")]
        args.extend([
            "review-activation".into(),
            "--hgs".into(),
            self.cli.to_string_lossy().into_owned(),
        ]);
        #[cfg(target_os = "macos")]
        args.extend([
            self.cli.to_string_lossy().into_owned(),
            "--review-activation".into(),
        ]);
        args.extend(["--activation-id".into(), id.into(), "--json".into()]);
        args
    }
    /// The first-choice form; offered only on the Windows route.
    fn first_arguments(&self, id: &str) -> Vec<String> {
        [
            "desktop",
            "registration",
            "install",
            "--mode",
            "background",
            "--json",
            "--request-id",
            id,
        ]
        .into_iter()
        .map(str::to_owned)
        .collect()
    }
    fn choice_arguments(&self, reviewed: &str, id: &str, action: ReviewAction) -> Vec<String> {
        if self.windows {
            return [
                "desktop",
                "registration",
                "install",
                "--mode",
                if action == ReviewAction::Passive {
                    "passive"
                } else {
                    "background"
                },
                "--json",
                "--after-review",
                reviewed,
                "--request-id",
                id,
            ]
            .into_iter()
            .map(str::to_owned)
            .collect();
        }
        let mut args = self.arguments(Some(id), true, false);
        args.extend(["--after-review".into(), reviewed.into()]);
        args
    }
}

/// Strong native image/ancestor pins live only through a consuming exchange,
/// including uncertain child/pipe custody. An idle display snapshot retains an
/// identity, so it cannot block a later maintained CLI update indefinitely.
struct RoutePin {
    route: Arc<Route>,
    #[cfg(windows)]
    image: zerus_client_core::desktop::windows::image::PinnedImage,
}
impl RoutePin {
    fn new(route: Arc<Route>) -> Result<Arc<Self>, &'static str> {
        #[cfg(windows)]
        let image = zerus_client_core::desktop::windows::image::PinnedImage::open(
            route.relay.executable()?,
        )
        .map_err(|_| "activation_helper_changed")?;
        #[cfg(windows)]
        if image.id() != route.image_id || image.path() != route.cli {
            return Err("activation_helper_changed");
        }
        let held = Arc::new(Self {
            route,
            #[cfg(windows)]
            image,
        });
        held.check()?;
        Ok(held)
    }
    fn check(&self) -> Result<(), &'static str> {
        self.route.check()?;
        #[cfg(windows)]
        self.image
            .revalidate()
            .map_err(|_| "activation_helper_changed")?;
        Ok(())
    }
}

pub(super) async fn startup(shared: &Arc<Shared>, automatic: bool, configuration: &str) {
    if !automatic
        && !startup_recovery()
        && shared.ownership.granted()
        && shared.mode_transition.automatic()
    {
        shared
            .mode_transition
            .activate_default(shared.hgs.clone(), configuration.into())
            .await;
    }
}

#[cfg(all(test, target_os = "linux"))]
mod first_tests;
#[cfg(test)]
mod projection_tests;
#[cfg(all(test, target_os = "linux"))]
mod tests;

#[cfg(all(test, windows))]
mod windows_pins {
    use super::*;
    #[test]
    fn a_refused_pin_names_why_the_image_could_not_be_opened() {
        // Task #59 (ZERUS/T-8r0u7v): the code stays, PinnedImage::open's own
        // words come with it, in the refusal and in the published view.
        let relative = Route::select(std::path::Path::new("hgs.exe"))
            .err()
            .unwrap();
        assert_eq!(relative.code, "activation_helper_unavailable");
        assert!(
            relative.reason.contains("image selection must be absolute"),
            "{relative}"
        );
        let root = std::env::temp_dir().join(format!("zerus-mode-pin-{}", uuid::Uuid::new_v4()));
        let missing = Route::select(&root.join("hgs.exe")).err().unwrap();
        assert_eq!(missing.code, "activation_helper_unavailable");
        assert!(
            missing.reason.contains("hgs.exe could not be pinned"),
            "{missing}"
        );
        assert!(
            missing
                .to_string()
                .starts_with("activation_helper_unavailable: "),
            "{missing}"
        );
        let owner = Controller::new();
        owner.refused(missing);
        let view = owner.view();
        assert_eq!(view.code, "activation_helper_unavailable");
        assert!(
            view.reason.contains("could not be pinned"),
            "{}",
            view.reason
        );
        assert_eq!(
            serde_json::to_value(&view).unwrap()["reason"],
            view.reason.as_str()
        );
        // A later failure without words never keeps the earlier ones.
        owner.failed("activation_helper_changed");
        assert!(owner.view().reason.is_empty());
        assert!(serde_json::to_value(owner.view())
            .unwrap()
            .get("reason")
            .is_none());
    }
    #[test]
    fn only_consuming_loans_pin_the_selected_image_and_its_ancestors() {
        // Native file-sharing control only: this is no caller, manager or
        // installed-GUI positive. The plain fixture need not be a PE image.
        let root = std::env::temp_dir().join(format!("zerus-mode-pin-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let cli = root.join("hgs.exe");
        let old = root.join("held.exe");
        std::fs::write(&cli, b"original selection").unwrap();
        let route = Arc::new(Route::select(&cli).unwrap_or_else(|refusal| panic!("{refusal}")));
        std::fs::rename(&cli, &old).unwrap(); // An idle snapshot holds no pin.
        std::fs::rename(&old, &cli).unwrap();
        let held = RoutePin::new(route.clone()).unwrap();
        let loan = held.clone();
        drop(held);
        assert!(
            std::fs::rename(&cli, &old).is_err(),
            "the last original consuming loan must pin its image"
        );
        assert!(
            std::fs::rename(&root, root.with_extension("moved")).is_err(),
            "the same loan pins the ancestors"
        );
        loan.check().unwrap();
        drop(loan);
        std::fs::rename(&cli, &old).unwrap();
        std::fs::write(&cli, b"replacement selection").unwrap();
        assert!(route.check().is_err(), "a replacement is never adopted");
        drop(route);
        std::fs::remove_dir_all(root).unwrap();
    }
}
