//! Native registration orchestration, separate from notification click reads.
//! A helper result is routing data. Only the maintained helper's original
//! caller/current Grant checks admit manager effects; no JSON grants authority.
use super::*;
use serde::Serialize;
use zerus_client_core::{desktop::drain::Charge, run::run_owned, Invocation};

const ABI: &str = "desktop-service-activation-v1";
const MARKER: &str = "Desktop service activation ABI: desktop-service-activation-v1";
const BYTES: usize = 64 << 10;

#[derive(Clone, Default, Serialize)]
pub(super) struct View {
    pub pending: bool,
    pub running: bool,
    pub needs_resume: bool,
    pub code: String,
}
struct Intent {
    id: String,
    configuration: String,
    route: Arc<Route>,
    complete: bool,
}
#[derive(Default)]
struct State {
    view: View,
    intent: Option<Intent>,
    selection: Option<Value>,
}
pub(super) struct Controller {
    state: Mutex<State>,
    gate: Gate,
    stop: AtomicBool,
    #[cfg(all(test, target_os = "linux"))]
    helper_directory: Mutex<Option<PathBuf>>,
}
impl Controller {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State::default()),
            gate: Gate::new(1).unwrap(),
            stop: AtomicBool::new(false),
            #[cfg(all(test, target_os = "linux"))]
            helper_directory: Mutex::new(None),
        })
    }
    #[cfg(all(test, target_os = "linux"))]
    pub(super) fn fixture_helpers(&self, directory: Option<PathBuf>) {
        *self.helper_directory.lock().unwrap() = directory;
    }
    fn select_route(&self, ordinary: &std::path::Path) -> Result<Route, &'static str> {
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
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .view
            .clone()
    }
    pub fn close(&self) -> zerus_client_core::desktop::drain::Closed {
        self.stop.store(true, Ordering::Release);
        self.gate.close()
    }
    pub fn pending(&self) -> bool {
        self.view().pending
    }
    fn automatic(&self) -> bool {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        !state.view.pending
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
    ) {
        let Ok(charge) = self.gate.admit(Role::Fleet) else {
            return;
        };
        if self.stop.load(Ordering::Acquire) {
            return;
        }
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .view
            .running = true;
        let owner = self.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || {
            owner.run(ordinary, configuration, activate, default_only, charge)
        })
        .await;
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .view
            .running = false;
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
            self.lookup(id, scope, route, resources);
            return;
        }
        let route = match self.select_route(&ordinary) {
            Ok(route) => Arc::new(route),
            Err(code) => {
                self.failed(code);
                return;
            }
        };
        let Some(help) = self.call(
            route.clone(),
            vec![route.helper.to_string_lossy().into_owned(), "--help".into()],
            resources.clone(),
        ) else {
            return;
        };
        if !std::str::from_utf8(&help).is_ok_and(|s| s.lines().any(|line| line.trim() == MARKER)) {
            self.failed("activation_helper_update_required");
            return;
        }
        let Some(raw) = self.call(
            route.clone(),
            route.arguments(None, false, false),
            resources.clone(),
        ) else {
            return;
        };
        let Ok(status) = serde_json::from_slice::<Value>(&raw) else {
            self.failed("activation_status_unavailable");
            return;
        };
        if status["schema"] != 1
            || status["configuration"] != configuration
            || !matches!(
                status["selected_mode"].as_str(),
                Some("passive" | "background")
            )
            || !matches!(
                status["mode_selection"].as_str(),
                Some("default" | "explicit" | "retained")
            )
        {
            self.failed("activation_status_unavailable");
            return;
        }
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
        {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state.selection = Some(status.clone());
            state.view.needs_resume = status["selected_mode"] == "passive";
            state.view.code = if state.view.needs_resume {
                "activation_required"
            } else {
                ""
            }
            .into();
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
        let id = uuid::Uuid::new_v4().to_string();
        self.reserve(id.clone(), configuration.clone(), route.clone());
        let Some(bytes) = self.call(
            route.clone(),
            route.arguments(Some(&id), true, default_only),
            resources,
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
        });
        state.view.pending = true;
        state.view.code = "activation_unconfirmed".into();
    }
    fn lookup(
        self: &Arc<Self>,
        id: String,
        configuration: String,
        route: Arc<Route>,
        resources: Arc<Charge>,
    ) {
        let Some(bytes) = self.call(
            route.clone(),
            route.arguments(Some(&id), false, false),
            resources,
        ) else {
            return;
        };
        self.result(&bytes, &id, &configuration);
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
        let absent = value["phase"] == "missing" && value["result"].is_null();
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(intent) = state
            .intent
            .as_mut()
            .filter(|i| i.id == id && i.configuration == configuration)
        else {
            return;
        };
        intent.complete = settled || refused || absent;
        state.view.pending = !(settled || refused || absent);
        state.view.needs_resume = !settled;
        state.view.code = if settled {
            ""
        } else if refused || absent {
            "activation_not_started"
        } else {
            "activation_unconfirmed"
        }
        .into();
    }
    /// The view the platform-manual route leaves (Windows), for projection
    /// tests on other platforms.
    #[cfg(all(test, not(windows)))]
    pub(super) fn platform_manual_for_test(&self) {
        self.failed("activation_platform_manual");
    }
    fn failed(&self, code: &str) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
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
        state.view.code = code.into();
        state.view.needs_resume = true;
    }
    fn call(
        self: &Arc<Self>,
        route: Arc<Route>,
        args: Vec<String>,
        resources: Arc<Charge>,
    ) -> Option<Vec<u8>> {
        let inv = Invocation {
            args,
            stdin: Some(Vec::new()),
            timeout: Duration::from_secs(30),
            request_id: None,
        };
        let selected = route.clone();
        // The charge, checked route and controller remain with the original
        // child and every pipe, even if this bounded call returns early.
        let held: Arc<dyn Send + Sync> = Arc::new((resources, route, self.clone()));
        let result = run_owned(
            std::path::Path::new("python3"),
            &inv,
            &[],
            &self.stop,
            move |command| {
                selected.check().map_err(std::io::Error::other)?;
                command.spawn()
            },
            held,
        );
        match result {
            Ok(bytes) if bytes.len() <= BYTES => Some(bytes),
            _ => {
                self.failed("activation_unconfirmed");
                None
            }
        }
    }
}

/// Selected only from the native connection and its maintained installation.
/// Renderer arguments and paths never enter this route.
struct Route {
    relay: Arc<zerus_client_core::desktop::relay::Connection>,
    helper: PathBuf,
    cli: PathBuf,
    #[cfg(unix)]
    files: Vec<(std::fs::File, PathBuf, std::fs::Metadata, bool)>,
}
impl Route {
    fn select(ordinary: &std::path::Path) -> Result<Self, &'static str> {
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = ordinary;
            Err("activation_platform_manual")
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
    ) -> Result<Self, &'static str> {
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
            let file = std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(no_follow | libc::O_NONBLOCK | libc::O_CLOEXEC)
                .open(path)
                .map_err(|_| "activation_helper_unavailable")?;
            let meta = file
                .metadata()
                .map_err(|_| "activation_helper_unavailable")?;
            let limit = if path == &cli { 512 << 20 } else { 1 << 20 };
            if !meta.is_file()
                || (!follow && meta.nlink() != 1)
                || meta.mode() & 0o022 != 0
                || meta.len() > limit
                || (meta.uid() != unsafe { libc::geteuid() } && meta.uid() != 0)
            {
                return Err("activation_helper_unavailable");
            }
            files.push((file, path.to_owned(), meta, follow));
        }
        let route = Self {
            relay,
            helper,
            cli,
            files,
        };
        route.check()?;
        Ok(route)
    }
    fn check(&self) -> Result<(), &'static str> {
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
    fn arguments(&self, id: Option<&str>, activate: bool, default_only: bool) -> Vec<String> {
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
mod tests;
