//! Zerus Next: the Tauri shell around the SolidJS frontend.
//!
//! The window is created here rather than in `tauri.conf.json`, so platform
//! code can hand the webview its configuration before it exists (the macOS
//! frame-rate preference, plan Task 7).

#[cfg(feature = "live")]
mod actions;
mod clipboard;
mod commands;
mod display;
mod metrics;
mod platform;
#[cfg(feature = "live")]
mod pump;
pub mod renderer;
#[cfg(feature = "live")]
mod readsync;
#[cfg(feature = "live")]
mod replay;
#[cfg(feature = "live")]
mod screens;

use tauri::{AppHandle, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

/// The label of the one window this phase opens.
pub const MAIN_WINDOW: &str = "main";

/// Tests that start child processes hold this shared; the tests that sample
/// this process's tree hold it alone, since a child that exits mid-sample
/// leaves the tree's memory total unknown.
#[cfg(all(test, unix))]
pub(crate) static PROCESS_TREE: std::sync::RwLock<()> = std::sync::RwLock::new(());

/// Builds the app and runs its event loop until the last window closes.
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(commands::AppState::default())
        .invoke_handler(handler())
        .setup(|app| {
            #[cfg(feature = "live")]
            {
                use tauri::Manager;
                let dir = app.path().app_data_dir()?;
                std::fs::create_dir_all(&dir)?;
                // A replay never touches the real read or attention marks.
                let replay = std::env::var_os("ZERUS_CLIENT_REPLAY").is_some_and(|list| !list.is_empty());
                let actions = actions::Actions::new(dir.join(actions::marks_file(replay)));
                readsync::start(actions.clone());
                app.manage(actions);
                app.manage(actions::ArchiveReads::default());
                app.manage(actions::Attention::new(dir.join(actions::attention_file(replay))));
                let pump = app.state::<commands::AppState>().pump();
                app.manage(screens::Screens::tauri(app.handle().clone(), pump));
            }
            let display = display::display_name();
            if !display::headless_allowed(std::env::var("ZERUS_CLIENT_HEADLESS").is_ok_and(|v| v == "1"), display.as_deref()) {
                eprintln!("zerus-client: ZERUS_CLIENT_HEADLESS=1 but the display is {display:?}, not an X server; no window opens (set GDK_BACKEND=x11)");
                std::process::exit(3);
            }
            let window = create_main_window(app.handle())?;
            // An unattended measurement needs frames: keep its window in front
            // and focused for the run, which ends by exiting the app.
            if std::env::var("ZERUS_CLIENT_PERF").is_ok_and(|mode| mode == "auto") {
                if let Err(error) = window.set_always_on_top(true).and_then(|()| window.set_focus()) {
                    eprintln!("zerus-client perf: could not bring the window to the front: {error}");
                }
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Zerus Next failed to start");
}

/// The commands the frontend may call. The session actions need the core's
/// `hgs` runner, so they exist only with the `live` feature.
#[cfg(feature = "live")]
fn handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        commands::start,
        commands::next_batch,
        commands::follow,
        commands::unfollow,
        commands::metrics,
        commands::perf_config,
        commands::save_perf,
        commands::perf_log,
        commands::display_hz,
        commands::exit_app,
        commands::perf_away,
        renderer::renderer_frame,
        actions::run_action,
        actions::set_notifications,
        actions::inspect_session,
        actions::pending_read_marks,
        actions::queue_read_marks,
        actions::attention_marks,
        actions::update_attention_marks,
        actions::session_reference,
        actions::copy_command,
        actions::archive_page,
        actions::cancel_archive_pages,
        clipboard::copy_text,
        screens::screen_visible,
        screens::accounts_view,
        screens::machines_view,
        screens::machine_resolve,
        screens::machine_check,
        screens::projects_view,
    ]
}

#[cfg(not(feature = "live"))]
fn handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        commands::start,
        commands::next_batch,
        commands::follow,
        commands::unfollow,
        commands::metrics,
        commands::perf_config,
        commands::save_perf,
        commands::perf_log,
        commands::display_hz,
        commands::exit_app,
        commands::perf_away,
        renderer::renderer_frame,
        clipboard::copy_text,
    ]
}

/// Creates the main window and applies the platform display-rate preference:
/// macOS before the webview is built, Linux through its public settings API.
pub fn create_main_window(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    // ZERUS_CLIENT_QUERY opens a given state (`select=<session name>`) for
    // headless screenshots and scripted measurements, as the mockup's URL
    // parameters do; ZERUS_CLIENT_PERF=auto adds `perf=auto`.
    let mut query: Vec<String> = std::env::var("ZERUS_CLIENT_QUERY")
        .ok()
        .filter(|q| !q.is_empty())
        .into_iter()
        .collect();
    if let Ok(mode) = std::env::var("ZERUS_CLIENT_PERF") {
        if !mode.is_empty() {
            query.push(format!("perf={mode}"));
        }
    }
    let page = match query.is_empty() {
        true => "index.html".to_owned(),
        false => format!("index.html?{}", query.join("&")),
    };
    #[allow(unused_mut)]
    let mut builder = WebviewWindowBuilder::new(app, MAIN_WINDOW, WebviewUrl::App(page.into()))
        .title("Zerus Next")
        .inner_size(1440.0, 900.0)
        .min_inner_size(900.0, 560.0);
    // Page rendering at the display rate, not near 60 fps: the preference is set
    // on the configuration before the webview exists (toggling a live page is
    // unreliable). The measurement's control run keeps WebKit's default.
    #[cfg(target_os = "macos")]
    if !keep_default_preference() {
        if let Some(mtm) = objc2::MainThreadMarker::new() {
            builder =
                builder.with_webview_configuration(platform::macos::webview_configuration(mtm));
        }
    }
    let window = builder.build()?;
    #[cfg(target_os = "linux")]
    if !keep_default_preference() {
        window.with_webview(|webview| {
            if let Err(error) = platform::linux::disable_near_60fps(&webview.inner()) {
                eprintln!("zerus-client: could not disable the near-60 fps preference: {error}");
            }
        })?;
    }
    Ok(window)
}

/// `ZERUS_CLIENT_PERF_PREFERENCE=default` leaves the macOS and Linux WebKit
/// frame-rate preference at its default: the must-fail control run of the measurement.
pub fn keep_default_preference() -> bool {
    std::env::var("ZERUS_CLIENT_PERF_PREFERENCE").is_ok_and(|value| value == "default")
}
