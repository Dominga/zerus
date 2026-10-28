//! The commands the frontend calls: `start`, `next_batch`, `follow`,
//! `unfollow` and `metrics`. Blocking work runs off the main thread.
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
    /// The change pump, whose mirror is the screens hub's view of the fleet.
    pub fn pump(&self) -> crate::pump::Pump {
        self.live.pump.clone()
    }
}

#[cfg(feature = "live")]
mod live {
    use std::collections::BTreeSet;
    use std::sync::{mpsc, Mutex, PoisonError};
    use std::time::Duration;

    use zerus_client_core::{default_hgs, ComputerId, Fleet};
    use zerus_watch_protocol::SessionRef;

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
    }

    fn replay_interval() -> Duration {
        let ms = std::env::var("ZERUS_CLIENT_REPLAY_INTERVAL_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(900);
        Duration::from_millis(ms)
    }

    impl Live {
        /// Starts the data source once; a later call (a reloaded webview)
        /// queues the last-known catalog again. Returns "live" or "replay".
        pub fn start(&self) -> Result<String, String> {
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
                    None => (Source::Fleet(Fleet::start(default_hgs(), sink)), "live"),
                };
            self.pump.drain(changes);
            *source = Some(started);
            Ok(mode.into())
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

/// Starts the data source; "live" (hgs watch) or "replay" (a fixture).
#[tauri::command]
pub async fn start(state: State<'_, AppState>) -> Result<String, String> {
    #[cfg(feature = "live")]
    {
        let live = Arc::clone(&state.live);
        blocking(move || live.start()).await
    }
    #[cfg(not(feature = "live"))]
    {
        let _ = &state;
        Err(NO_LIVE.into())
    }
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
) -> Result<tauri::ipc::Response, String> {
    let live = Arc::clone(&state.live);
    let actions = actions.inner().clone();
    let bytes = blocking(move || {
        let batch = live.pump.take(crate::pump::PARK);
        // The host's rows settle pending read marks, whether they arrive as
        // single upserts or inside a computer's snapshot.
        actions.observe(&batch);
        // Marks for computers the local listing no longer names can never be
        // written (Qt's `publishReadMarks`).
        actions.drop_orphans(|| live.peers());
        serde_json::to_vec(&batch).map_err(|error| error.to_string())
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
/// leaves the macOS and Linux WebKit frame-rate preference unchanged).
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

/// Ends the app after a measurement run; refused outside one.
#[tauri::command]
pub fn exit_app(app: tauri::AppHandle, code: i32) -> Result<(), String> {
    if !perf_mode() {
        return Err("not a measurement run (ZERUS_CLIENT_PERF is not set)".into());
    }
    app.exit(code);
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
