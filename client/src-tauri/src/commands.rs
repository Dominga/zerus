//! The commands the frontend calls: `start`, `next_batch`, `follow`,
//! `unfollow` and `metrics`. Blocking work runs off the main thread.
//! `next_batch` hands the followed processes to the Processes hub
//! (`processes.rs`) and sends its view as `processes_view`.
//!
//! Data comes from zerus-client-core (feature `live`): `Fleet` runs
//! `hgs [@host] watch --json` per computer, or, with `ZERUS_CLIENT_REPLAY`
//! set, a synthetic fixture replays through the same `CatalogState` and
//! coalescer (see `replay.rs`). `ZERUS_CLIENT_HGS` names the `hgs` to run.

#[cfg(feature = "live")]
use std::sync::Arc;

use serde_json::Value;
use tauri::State;

#[derive(Default)]
pub struct AppState {
    #[cfg(feature = "live")]
    live: Arc<live::Live>,
}

#[cfg(feature = "live")]
impl AppState {
    /// A manual offline Refresh nudges only its existing source; never starts
    /// another fleet or changes an online source.
    pub fn reconnect(&self, computer: &str) -> bool {
        self.live.reconnect(computer)
    }

    /// The change pump, whose mirror is the screens hub's view of the fleet.
    pub fn pump(&self) -> crate::pump::Pump {
        self.live.pump.clone()
    }

    /// Starts the data source now, before any window loads: the menu bar and
    /// notifications need the fleet while the window is closed. A later
    /// `start` from the webview finds it running and resends the catalog.
    pub fn start_source(&self) -> Result<String, String> {
        self.live.start()
    }

    /// The peers this computer's listing names; None before a listing named
    /// them, and for a replay.
    pub fn peers(&self) -> Option<std::collections::BTreeSet<String>> {
        self.live.peers()
    }

    /// The organization hub: `start` gives it the snapshot source and the
    /// organization updates. Set once, before the webview starts the source.
    pub fn set_organization(&self, organization: crate::organization::Organization) {
        let _ = self.live.organization.set(organization);
    }

    /// Set once before the source starts. The host's observer never blocks
    /// the source drain or takes the pump/organization locks.
    pub fn set_creations(&self, host: crate::creations::CreationHost) {
        let _ = self.live.creations.set(host);
    }

    /// Applies the preferences the host confirmed for a Mute or Unmute to the
    /// session's row at once, for the window and the menu bar alike
    /// (`FleetState::applyNotificationPreferences`). A replay has no hosts.
    pub fn apply_notifications(
        &self,
        target: &zerus_client_core::actions::Target,
        preferences: &Value,
    ) {
        self.live.apply_notifications(target, preferences);
    }

    /// Applies the resumed run to the replay after the fake accepted Resume.
    pub fn smoke_resume(&self, key: &str, ready: Value) -> Result<(), String> {
        self.live.smoke_resume(key, ready)
    }

    /// Follows the processes of the session the Processes tab shows.
    pub fn follow_processes(&self, target: &crate::processes::ProcessTarget) -> Result<(), String> {
        self.live.follow_processes(target)
    }

    pub fn set_processes_interval(
        &self,
        target: &crate::processes::ProcessTarget,
        ms: u64,
    ) -> Result<(), String> {
        self.live.set_processes_interval(target, ms)
    }

    pub fn refresh_processes(
        &self,
        target: &crate::processes::ProcessTarget,
    ) -> Result<(), String> {
        self.live.refresh_processes(target)
    }

    pub fn unfollow_processes(&self, target: &crate::processes::ProcessTarget) {
        self.live.unfollow_processes(target);
    }
}

#[cfg(feature = "live")]
mod live {
    use std::collections::BTreeSet;
    use std::sync::{mpsc, Arc, Mutex, OnceLock, PoisonError};
    use std::time::Duration;

    use serde_json::Value;
    use zerus_client_core::{default_hgs, ComputerId, Fleet, FollowError, OrganizationSnapshot};
    use zerus_watch_protocol::SessionRef;

    use crate::organization::Organization;
    use crate::processes::ProcessTarget;
    use crate::pump::Pump;
    use crate::replay::Replay;

    enum Source {
        Fleet(Fleet),
        Replay(Replay),
    }

    #[derive(Default)]
    pub struct Live {
        pub pump: Pump,
        source: Mutex<Option<Source>>,
        pub organization: OnceLock<Organization>,
        pub creations: OnceLock<crate::creations::CreationHost>,
    }

    /// Follows this computer's organization: the catalog is shared, so its
    /// copy is the one the hub edits. A Fleet starts once, so this logs at
    /// most once.
    fn follow_organization(fleet: &Fleet) {
        if let Err(error) = fleet.follow_organization(&ComputerId::local()) {
            eprintln!("zerus-client: project sync cannot follow this computer's catalog: {error}");
        }
    }

    fn replay_interval() -> Duration {
        let ms = std::env::var("ZERUS_CLIENT_REPLAY_INTERVAL_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(900);
        Duration::from_millis(ms)
    }

    impl Live {
        pub fn reconnect(&self, computer: &str) -> bool {
            match self
                .source
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()
            {
                Some(Source::Fleet(fleet)) => fleet.reconnect(&ComputerId::from(computer)),
                Some(Source::Replay(_)) | None => false,
            }
        }

        /// Starts the data source once; a later call (a reloaded webview)
        /// queues the last-known catalog again. Returns "live" or "replay".
        /// This computer's organization goes to the hub, which reads its
        /// base and versions back through [`Live::organization_snapshot`].
        /// The hub asks without its own lock held, and its source is set
        /// here before `source` is taken, so the two locks never nest.
        pub fn start(self: &Arc<Self>) -> Result<String, String> {
            let organization = self.organization.get().cloned();
            let creations = self.creations.get().cloned();
            if let Some(organization) = &organization {
                let live = Arc::downgrade(self);
                organization.set_source(Arc::new(move || live.upgrade()?.organization_snapshot()));
            }
            let mut source = self.source.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(running) = source.as_ref() {
                self.pump.resend();
                return Ok(match running {
                    Source::Fleet(_) => "live",
                    Source::Replay(_) => "replay",
                }
                .into());
            }
            let (sink, changes) = mpsc::channel();
            let (started, mode) =
                match std::env::var_os("ZERUS_CLIENT_REPLAY").filter(|list| !list.is_empty()) {
                    Some(list) => {
                        let replay = Replay::start(&list, replay_interval(), sink)
                            .map_err(|error| format!("replay: {error}"))?;
                        (Source::Replay(replay), "replay")
                    }
                    None => {
                        let fleet = Fleet::start(default_hgs(), sink);
                        if organization.is_some() {
                            follow_organization(&fleet);
                        }
                        (Source::Fleet(fleet), "live")
                    }
                };
            let after = creations.clone();
            self.pump.drain_with_after(
                changes,
                move |change| {
                    if let Some(organization) = &organization {
                        organization.observe(change);
                    }
                    creations.is_some() && crate::creations::CreationHost::observes(change)
                },
                move || {
                    if let Some(creations) = &after {
                        creations.catalog_changed();
                    }
                },
            );
            *source = Some(started);
            Ok(mode.into())
        }

        /// The base and versions of this computer's latest organization state.
        fn organization_snapshot(&self) -> Option<OrganizationSnapshot> {
            match self
                .source
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()?
            {
                Source::Fleet(fleet) => fleet.organization_snapshot(&ComputerId::local()),
                Source::Replay(replay) => replay.organization_snapshot(),
            }
        }

        /// The peers this computer's listing names; None before a listing
        /// named them, and for a replay.
        pub fn peers(&self) -> Option<BTreeSet<String>> {
            match self
                .source
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()
            {
                Some(Source::Fleet(fleet)) => fleet.peers(),
                _ => None,
            }
        }

        pub fn smoke_resume(&self, key: &str, ready: Value) -> Result<(), String> {
            match self
                .source
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_mut()
            {
                Some(Source::Replay(replay)) => replay.resume("", key, ready),
                _ => Err("not a replay".into()),
            }
        }

        pub fn follow(
            &self,
            computer: String,
            key: String,
            session: SessionRef,
            after: Option<i64>,
            tail: u32,
        ) -> Result<(), String> {
            match self
                .source
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()
            {
                Some(Source::Fleet(fleet)) => {
                    let computer = ComputerId(computer);
                    // Following the same session again is a no-op in the core, so
                    // an older page (`after`) starts a new subscription from there.
                    if after.is_some() {
                        fleet.unfollow(&computer, &key);
                    }
                    fleet
                        .follow(&computer, &key, session, after, tail)
                        .map_err(|error| error.to_string())?
                }
                Some(Source::Replay(replay)) => replay.follow(&computer, &key, after, tail),
                None => return Err("not started".into()),
            }
            Ok(())
        }

        pub fn apply_notifications(
            &self,
            target: &zerus_client_core::actions::Target,
            preferences: &serde_json::Value,
        ) {
            if let Some(Source::Fleet(fleet)) = self
                .source
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()
            {
                fleet.apply_notifications(target, preferences.clone());
            }
        }

        /// Follows a session's processes (`Fleet::follow_processes`); the
        /// session's catalog key must be the target's. A replay serves the
        /// fixture's processes state.
        pub fn follow_processes(&self, target: &ProcessTarget) -> Result<(), String> {
            match self
                .source
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()
            {
                Some(Source::Fleet(fleet)) => {
                    let computer = ComputerId(target.computer.clone());
                    let filled = |id: &str| Some(id.to_owned()).filter(|id| !id.is_empty());
                    let session = SessionRef {
                        name: target.name.clone(),
                        run_id: filled(&target.run_id),
                        conversation_id: filled(&target.conversation_id),
                    };
                    let key = fleet
                        .follow_processes(&computer, session)
                        .map_err(|error| error.to_string())?;
                    if key != target.key {
                        fleet.unfollow_processes(&computer, &key);
                        return Err(FollowError::KeyMismatch.to_string());
                    }
                    Ok(())
                }
                Some(Source::Replay(replay)) => {
                    replay.follow_processes(&target.computer, &target.key);
                    Ok(())
                }
                None => Err("not started".into()),
            }
        }

        pub fn set_processes_interval(
            &self,
            target: &ProcessTarget,
            ms: u64,
        ) -> Result<(), String> {
            match self
                .source
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()
            {
                Some(Source::Fleet(fleet)) => fleet
                    .set_processes_interval(&ComputerId(target.computer.clone()), &target.key, ms)
                    .map_err(|error| error.to_string()),
                Some(Source::Replay(_)) => Ok(()),
                None => Err("not started".into()),
            }
        }

        pub fn refresh_processes(&self, target: &ProcessTarget) -> Result<(), String> {
            match self
                .source
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()
            {
                Some(Source::Fleet(fleet)) => fleet
                    .refresh_processes(&ComputerId(target.computer.clone()), &target.key)
                    .map_err(|error| error.to_string()),
                Some(Source::Replay(_)) => Ok(()),
                None => Err("not started".into()),
            }
        }

        /// Not followed (it ended meanwhile) is not an error.
        pub fn unfollow_processes(&self, target: &ProcessTarget) {
            match self
                .source
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()
            {
                Some(Source::Fleet(fleet)) => {
                    fleet.unfollow_processes(&ComputerId(target.computer.clone()), &target.key);
                }
                Some(Source::Replay(replay)) => {
                    replay.unfollow_processes(&target.computer, &target.key);
                }
                None => {}
            }
        }

        pub fn unfollow(&self, computer: String, key: String) -> Result<(), String> {
            match self
                .source
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()
            {
                // Not followed (it ended meanwhile) is not an error.
                Some(Source::Fleet(fleet)) => {
                    fleet.unfollow(&ComputerId(computer), &key);
                }
                Some(Source::Replay(replay)) => replay.unfollow(&computer, &key),
                None => return Err("not started".into()),
            }
            Ok(())
        }
    }
}

#[cfg(not(feature = "live"))]
const NO_LIVE: &str = "this build has no live data source (cargo feature `live` is off)";

/// Runs blocking work on the blocking pool, off the main thread.
#[cfg(feature = "live")]
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| error.to_string())?
}

/// Starts the data source; "live" (hgs watch) or "replay" (a fixture). A
/// reloaded webview gets the catalog, the composer's view and the draft keys
/// again.
#[cfg(feature = "live")]
#[tauri::command]
pub async fn start(
    state: State<'_, AppState>,
    composers: State<'_, crate::composer::Composers>,
    processes: State<'_, crate::processes::ProcessHub>,
) -> Result<String, String> {
    let live = Arc::clone(&state.live);
    composers.resend();
    processes.resend();
    // A reloaded webview shows no attachment yet.
    composers.cache().clear_pins();
    blocking(move || live.start()).await
}

#[cfg(not(feature = "live"))]
#[tauri::command]
pub async fn start(state: State<'_, AppState>) -> Result<String, String> {
    let _ = &state;
    Err(NO_LIVE.into())
}

/// Codex's question replies become `question_replies` on their events
/// (R7's `annotate_replies`), so the webview never parses them.
#[cfg(feature = "live")]
pub(crate) fn annotate(batch: &mut zerus_client_core::Batch) {
    for out in &mut batch.activity {
        zerus_client_core::questions::annotate_replies(&mut out.payload);
    }
}

/// What `next_batch` returns: the core's batch, plus the hubs' projected views
/// when they changed (latest value wins).
#[cfg(feature = "live")]
#[derive(serde::Serialize)]
struct Envelope<'a> {
    #[serde(flatten)]
    batch: &'a zerus_client_core::Batch,
    #[serde(skip_serializing_if = "Option::is_none")]
    composer: Option<crate::composer::ComposerView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    draft_keys: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    processes_view: Option<crate::processes::ProcessesView>,
    /// Notices and Terminal requests that arrived after their call (A2).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    composer_events: Vec<crate::composer::ComposerEvent>,
}

/// Everything pending since the last call; parks up to 250 ms while nothing is.
/// The frontend keeps one call in flight and applies one batch per frame.
/// The batch goes as raw JSON bytes: a first catalog batch is tens of KiB,
/// beyond what the evaluated-script path should carry.
#[cfg(feature = "live")]
#[tauri::command]
pub async fn next_batch(
    state: State<'_, AppState>,
    actions: State<'_, crate::actions::Actions>,
    composers: State<'_, crate::composer::Composers>,
    processes: State<'_, crate::processes::ProcessHub>,
) -> Result<tauri::ipc::Response, String> {
    let live = Arc::clone(&state.live);
    let actions = actions.inner().clone();
    let composers = composers.inner().clone();
    let processes = processes.inner().clone();
    let bytes = blocking(move || {
        let mut batch = live.pump.take(crate::pump::PARK);
        // Codex's question replies are parsed here, never in the webview.
        annotate(&mut batch);
        // The followed processes go to the Processes hub, not to the webview.
        processes.observe(&mut batch);
        // The host's rows settle pending read marks, whether they arrive as
        // single upserts or inside a computer's snapshot.
        actions.observe(&batch);
        // The followed session's Activity state and its row feed the
        // composer's block reason, before its view is taken.
        composers.observe(&batch);
        // Marks for computers the local listing no longer names can never be
        // written (Qt's `publishReadMarks`).
        actions.drop_orphans(|| live.peers());
        let envelope = Envelope {
            batch: &batch,
            composer: composers.take_view(),
            draft_keys: composers.take_draft_keys(),
            processes_view: processes.take_view(),
            composer_events: composers.take_events(),
        };
        serde_json::to_vec(&envelope).map_err(|error| error.to_string())
    })
    .await?;
    Ok(tauri::ipc::Response::new(bytes))
}

#[cfg(not(feature = "live"))]
#[tauri::command]
pub async fn next_batch(state: State<'_, AppState>) -> Result<Value, String> {
    let _ = &state;
    Err(NO_LIVE.into())
}

/// Follows one session's Activity: the newest `tail` events, or the events after `after`.
#[tauri::command(rename_all = "snake_case")]
#[allow(clippy::too_many_arguments)]
pub async fn follow(
    state: State<'_, AppState>,
    computer: String,
    key: String,
    name: String,
    run_id: Option<String>,
    conversation_id: Option<String>,
    after: Option<i64>,
    tail: Option<u32>,
) -> Result<(), String> {
    #[cfg(feature = "live")]
    {
        let live = Arc::clone(&state.live);
        let session = zerus_watch_protocol::SessionRef {
            name,
            run_id,
            conversation_id,
        };
        let tail = tail.unwrap_or(zerus_watch_protocol::ACTIVITY_TAIL as u32);
        blocking(move || live.follow(computer, key, session, after, tail)).await
    }
    #[cfg(not(feature = "live"))]
    {
        let _ = (
            &state,
            computer,
            key,
            name,
            run_id,
            conversation_id,
            after,
            tail,
        );
        Err(NO_LIVE.into())
    }
}

#[tauri::command(rename_all = "snake_case")]
pub async fn unfollow(
    state: State<'_, AppState>,
    computer: String,
    key: String,
) -> Result<(), String> {
    #[cfg(feature = "live")]
    {
        let live = Arc::clone(&state.live);
        blocking(move || live.unfollow(computer, key)).await
    }
    #[cfg(not(feature = "live"))]
    {
        let _ = (&state, computer, key);
        Err(NO_LIVE.into())
    }
}

/// The measurement run mode from the environment: `ZERUS_CLIENT_PERF` (`auto`
/// runs the Diagnostics scenarios, saves the report and exits),
/// `ZERUS_CLIENT_PERF_OUT` (where the report goes), `ZERUS_CLIENT_PERF_LABEL`
/// (the platform label), `ZERUS_CLIENT_PERF_REPRESENTATIVE=0` (Xvfb and other
/// non-representative runs) and `ZERUS_CLIENT_PERF_PREFERENCE` (`default`
/// leaves the macOS and Linux WebKit frame-rate preference unchanged). `replay`
/// tells the page that `ZERUS_CLIENT_REPLAY` supplies the data.
#[tauri::command]
pub fn perf_config() -> Value {
    let var = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
    serde_json::json!({
        "mode": var("ZERUS_CLIENT_PERF"),
        "out": var("ZERUS_CLIENT_PERF_OUT"),
        "label": var("ZERUS_CLIENT_PERF_LABEL"),
        // `0` marks a run whose numbers do not represent the device (Xvfb, a remote session).
        "representative": var("ZERUS_CLIENT_PERF_REPRESENTATIVE").is_none_or(|value| value != "0"),
        "preference": if crate::keep_default_preference() { "default" } else { "display-rate" },
        // `:N` is an X server (Xvfb); `wayland-N` is a desktop session.
        "display": crate::display::display_name(),
        // The actions smoke runs only on a replay with an explicit fake hgs.
        "actions_smoke": actions_smoke_allowed(),
        "smoke_file": var("ZERUS_CLIENT_SMOKE_FILE"),
        // A replay run measures a synthetic Activity (the auto run tries every session for it).
        "replay": var("ZERUS_CLIENT_REPLAY").is_some(),
        // The Linux WebKit renderer decision (renderer, reason, nvidia, wayland, marker_state).
        "renderer": crate::renderer::diagnostics(),
    })
}

#[cfg(feature = "live")]
fn actions_smoke_allowed() -> bool {
    crate::actions::smoke_env_allowed()
}

#[cfg(not(feature = "live"))]
fn actions_smoke_allowed() -> bool {
    false
}

fn perf_mode() -> bool {
    std::env::var("ZERUS_CLIENT_PERF").is_ok_and(|mode| !mode.is_empty())
}

/// Writes a measurement report; only in a measurement run (`ZERUS_CLIENT_PERF`
/// set). An empty `path` means `ZERUS_CLIENT_PERF_OUT`. Returns the path written.
#[tauri::command(async)]
pub fn save_perf(path: String, json: String) -> Result<String, String> {
    if !perf_mode() {
        return Err("not a measurement run (ZERUS_CLIENT_PERF is not set)".into());
    }
    let path = match path.is_empty() {
        true => std::env::var("ZERUS_CLIENT_PERF_OUT")
            .map_err(|_| "ZERUS_CLIENT_PERF_OUT is not set".to_owned())?,
        false => path,
    };
    let path = std::path::PathBuf::from(path);
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    std::fs::write(&path, json).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(path.display().to_string())
}

/// The refresh rate of the main window's display in hertz, None when unknown
/// (`ZERUS_CLIENT_PERF_HZ` overrides it). Synchronous: it runs on the main
/// thread, where the platform display APIs must be called.
#[tauri::command]
pub fn display_hz(window: tauri::WebviewWindow) -> Option<f64> {
    crate::display::display_hz(&window)
}

/// One progress line of a measurement run on stderr, so a stalled run shows
/// where it stopped. The frontend writes no session names into it.
#[tauri::command]
pub fn perf_log(line: String) {
    let line: String = line.chars().filter(|c| !c.is_control()).take(500).collect();
    eprintln!("zerus-client perf: {line}");
}

#[cfg(feature = "live")]
fn smoke_origin(label: &str) -> Result<(), String> {
    if label == crate::MAIN_WINDOW {
        Ok(())
    } else {
        Err("not the main webview".into())
    }
}

/// The fake `hgs` call log beside `ZERUS_CLIENT_HGS`, for the phase 4 smoke.
/// Refused outside a smoke run, so a real `hgs` folder is never read.
#[cfg(feature = "live")]
#[tauri::command]
pub async fn smoke_log(window: tauri::WebviewWindow) -> Result<Vec<serde_json::Value>, String> {
    smoke_origin(window.label())?;
    if !crate::actions::smoke_env_allowed() {
        return Err("not a smoke run".into());
    }
    let hgs =
        std::path::PathBuf::from(std::env::var("ZERUS_CLIENT_HGS").map_err(|e| e.to_string())?);
    tauri::async_runtime::spawn_blocking(move || {
        let log = hgs.with_file_name("log.ndjson");
        let text =
            std::fs::read_to_string(&log).map_err(|error| format!("{}: {error}", log.display()))?;
        Ok(text
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect())
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Advances the replay after the fake's Resume, only in the phase 4 smoke.
#[cfg(feature = "live")]
#[tauri::command]
pub async fn smoke_resume(
    state: State<'_, AppState>,
    window: tauri::WebviewWindow,
    key: String,
) -> Result<(), String> {
    smoke_origin(window.label())?;
    if !crate::actions::smoke_env_allowed() {
        return Err("not a smoke run".into());
    }
    let hgs =
        std::path::PathBuf::from(std::env::var("ZERUS_CLIENT_HGS").map_err(|e| e.to_string())?);
    let ready = tauri::async_runtime::spawn_blocking(move || {
        let path = hgs.with_file_name("config.json");
        let text = std::fs::read_to_string(&path)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        let config: Value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
        Ok::<Value, String>(config["resume_ready"].clone())
    })
    .await
    .map_err(|error| error.to_string())??;
    state.smoke_resume(&key, ready)
}

/// This build's identity, as `--build-info` prints it (Qt's
/// `UpdateController::buildInfo`): the About page shows it.
#[tauri::command]
pub fn build_info() -> Value {
    crate::cli::build_info(&crate::cli::BUILD)
}

/// Ends the app after a measurement run; refused outside one.
#[tauri::command]
pub fn exit_app(app: tauri::AppHandle, code: i32) -> Result<(), String> {
    if !perf_mode() {
        return Err("not a measurement run (ZERUS_CLIENT_PERF is not set)".into());
    }
    crate::exit_flush::quit_with_flush(&app, code);
    Ok(())
}

/// Takes the window away for a measurement: `how` is `hide` or `minimize`.
/// After a settling pause it samples the process tree over `window_ms`
/// (default 10 s), brings the window back and returns `{how, minimized,
/// sample}`; `minimized` says whether the window manager minimised it (a bare
/// X server has none). Refused outside a measurement run.
#[tauri::command(async, rename_all = "snake_case")]
pub fn perf_away(
    window: tauri::WebviewWindow,
    how: String,
    window_ms: Option<u64>,
) -> Result<Value, String> {
    if !perf_mode() {
        return Err("not a measurement run (ZERUS_CLIENT_PERF is not set)".into());
    }
    match how.as_str() {
        "hide" => window.hide(),
        "minimize" => window.minimize(),
        _ => return Err(format!("unknown way to take the window away: {how}")),
    }
    .map_err(|error| error.to_string())?;
    crate::window_visibility::sync_webview(&window);
    // The page sees the change and its last frame ends before the sample starts.
    std::thread::sleep(std::time::Duration::from_millis(1500));
    let minimized = window.is_minimized().unwrap_or(false);
    let ms = window_ms.unwrap_or(10_000).clamp(100, 60_000);
    let sample = crate::metrics::process_tree_metrics_over(
        std::process::id(),
        std::time::Duration::from_millis(ms),
    );
    let back = match how.as_str() {
        "hide" => window.show(),
        _ => window.unminimize(),
    };
    crate::window_visibility::sync_webview(&window);
    if let Err(error) = back.and_then(|()| window.set_focus()) {
        eprintln!("zerus-client perf: could not bring the window back: {error}");
    }
    Ok(serde_json::json!({ "how": how, "minimized": minimized, "sample": sample }))
}

/// RSS and CPU of the client's process tree (the app and its webview
/// processes). Sampling blocks for its 500 ms window, so it runs off the main thread.
/// `window_ms` sets the window between the two samples (default 500 ms; an idle
/// measurement uses 10 s, so CPU is the cpu_seconds delta over that wall time).
#[tauri::command(async, rename_all = "snake_case")]
pub fn metrics(window_ms: Option<u64>) -> Value {
    let pid = std::process::id();
    match window_ms {
        Some(ms) => crate::metrics::process_tree_metrics_over(
            pid,
            std::time::Duration::from_millis(ms.clamp(100, 60_000)),
        ),
        None => crate::metrics::process_tree_metrics(pid),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn build_info_answers_what_the_command_line_prints() {
        let info = super::build_info();
        assert_eq!(info, crate::cli::build_info(&crate::cli::BUILD));
        assert_eq!(info["version"], env!("ZERUS_VERSION"));
    }

    #[cfg(feature = "live")]
    #[test]
    fn smoke_commands_answer_only_the_main_webview() {
        assert_eq!(super::smoke_origin("main"), Ok(()));
        assert!(super::smoke_origin("native-ui").is_err());
    }

    #[cfg(feature = "live")]
    #[test]
    fn question_replies_are_annotated_before_the_webview() {
        use zerus_client_core::{ActivityOut, Batch, ComputerId};
        let detail = "<send_user_message_question_reply>{\"questionItemId\":\"q1\",\"question\":\"Which checks?\",\"answer\":\"Unit\"}</send_user_message_question_reply>";
        let mut batch = Batch {
            activity: vec![ActivityOut {
                computer: ComputerId::from("studio"),
                key: "run:run-1".into(),
                kind: "events".into(),
                payload: serde_json::json!({"events": [{"seq": 1, "type": "UserPromptSubmit", "detail": detail}], "cursor": 1}),
            }],
            ..Batch::default()
        };
        super::annotate(&mut batch);
        assert_eq!(
            batch.activity[0].payload["events"][0]["question_replies"],
            serde_json::json!([{"question": "Which checks?", "answer": "Unit"}])
        );
    }
}
