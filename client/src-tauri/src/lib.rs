//! Zerus Next: the Tauri shell around the SolidJS frontend.
//!
//! The window is created here rather than in `tauri.conf.json`, so platform
//! code can hand the webview its configuration before it exists (the macOS
//! frame-rate preference, plan Task 7).

#[cfg(feature = "live")]
mod actions;
mod clipboard;
mod commands;
#[cfg(feature = "live")]
mod composer;
mod display;
mod metrics;
mod platform;
#[cfg(feature = "live")]
mod pump;
#[cfg(feature = "live")]
mod replay;
#[cfg(feature = "live")]
mod screens;
#[cfg(feature = "live")]
mod terminal;
#[cfg(feature = "media")]
mod voice;
mod window_visibility;

use tauri::{AppHandle, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

/// The app's one draft store, shared by the composers (and voice), so every
/// draft key has one writer.
#[cfg(feature = "live")]
pub struct Drafts(pub std::sync::Arc<zerus_client_core::drafts::DraftStore>);

/// The label of the one window this phase opens.
pub const MAIN_WINDOW: &str = "main";

/// The root of every private path of the client: the draft store, read marks
/// and any later file. Tauri's `app_data_dir` for now; the Mac switch build
/// replaces this body, since there Tauri reports the compile-time identifier
/// rather than the running bundle's. Nothing else asks Tauri for it.
#[cfg_attr(not(feature = "live"), allow(dead_code))]
pub(crate) fn data_dir(app: &AppHandle) -> std::path::PathBuf {
    use tauri::Manager;
    app.path()
        .app_data_dir()
        .expect("the platform has an app data directory")
}

/// The client's own draft store under its data directory: `drafts`, or
/// `drafts-replay` in a replay, so a replay never touches the real drafts.
/// Qt's store is only ever read (Saved drafts and its recovery).
#[cfg(feature = "live")]
fn drafts_root(data_dir: &std::path::Path, replay: bool) -> std::path::PathBuf {
    data_dir.join(zerus_client_core::drafts::dir(replay))
}

/// Tests that start child processes hold this shared; the tests that sample
/// this process's tree hold it alone, since a child that exits mid-sample
/// leaves the tree's memory total unknown.
#[cfg(all(test, unix))]
pub(crate) static PROCESS_TREE: std::sync::RwLock<()> = std::sync::RwLock::new(());

/// Builds the app and runs its event loop until the last window closes.
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(commands::AppState::default())
        .manage(window_visibility::Visibility::default())
        .invoke_handler(handler())
        .setup(|app| {
            #[cfg(feature = "live")]
            {
                use tauri::Manager;
                let dir = data_dir(app.handle());
                std::fs::create_dir_all(&dir)?;
                // A replay never touches the real read marks.
                let replay = std::env::var_os("ZERUS_CLIENT_REPLAY").is_some_and(|list| !list.is_empty());
                app.manage(actions::Actions::new(dir.join(actions::marks_file(replay))));
                let pump = app.state::<commands::AppState>().pump();
                app.manage(screens::Screens::tauri(app.handle().clone(), pump));
                app.manage(terminal::Terminals::new(
                    zerus_client_core::default_hgs(),
                    actions::actions_allowed(
                        std::env::var("ZERUS_CLIENT_REPLAY").ok().as_deref(),
                        std::env::var("ZERUS_CLIENT_HGS").ok().as_deref(),
                    ),
                ));
                // A replay never touches the real drafts either.
                let drafts = std::sync::Arc::new(zerus_client_core::drafts::DraftStore::open(
                    drafts_root(&dir, replay),
                ));
                #[cfg(feature = "media")]
                app.manage(voice::Voices::new(std::sync::Arc::clone(&drafts)));
                app.manage(Drafts(drafts));
                let drafts = std::sync::Arc::clone(&app.state::<Drafts>().0);
                let notify_pump = app.state::<commands::AppState>().pump();
                app.manage(composer::Composers::new(
                    drafts,
                    composer::Deps {
                        spawn: std::sync::Arc::new(|job: composer::Job| {
                            tauri::async_runtime::spawn_blocking(job);
                        }),
                        notify: std::sync::Arc::new(move || notify_pump.poke()),
                    },
                ));
                #[cfg(unix)]
                exit_on_signals(app.handle().clone());
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
        .on_window_event(|window, event| {
            if window.label() == MAIN_WINDOW
                && matches!(event, tauri::WindowEvent::Resized(_) | tauri::WindowEvent::Focused(_) | tauri::WindowEvent::Moved(_))
            {
                window_visibility::sync(window);
            }
            #[cfg(feature = "live")]
            if window.label() == MAIN_WINDOW && matches!(event, tauri::WindowEvent::Destroyed) {
                use tauri::Manager;
                window.state::<terminal::Terminals>().stop_all();
            }
            #[cfg(not(feature = "live"))]
            let _ = (window, event);
        })
        .build(tauri::generate_context!())
        .expect("Zerus Next failed to start")
        .run(|_app, event| {
            if let tauri::RunEvent::Exit = event {
                #[cfg(feature = "live")]
                exit(_app);
            }
        });
}

/// Before the app quits: recordings under way are cancelled, the hubs take
/// no more changes, and every draft save already queued reaches the disk.
#[cfg(feature = "live")]
fn exit(app: &AppHandle) {
    use tauri::Manager;
    #[cfg(feature = "media")]
    if let Some(voices) = app.try_state::<voice::Voices>() {
        voices.close_all();
    }
    let composers = app.try_state::<composer::Composers>();
    close_drafts(composers.as_deref(), || {
        if let Some(drafts) = app.try_state::<Drafts>() {
            drafts.0.flush();
        }
    });
}

/// The composers close before the store's last flush: an attach read under
/// way ends and its save is queued first, and nothing is queued after it.
#[cfg(feature = "live")]
fn close_drafts(composers: Option<&composer::Composers>, flush_store: impl FnOnce()) {
    if let Some(composers) = composers {
        composers.flush();
    }
    flush_store();
}

/// SIGTERM, SIGINT or SIGHUP: the exit hook runs here first (drafts reach the
/// disk even if the event loop is stuck), then the app exits, which runs the
/// hook again harmlessly. Should the event loop not end the process, it ends
/// five seconds later.
#[cfg(all(unix, feature = "live"))]
fn exit_on_signals(app: AppHandle) {
    use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
    let Ok(mut signals) = signal_hook::iterator::Signals::new([SIGTERM, SIGINT, SIGHUP]) else {
        return;
    };
    std::thread::Builder::new()
        .name("zerus-signals".into())
        .spawn(move || {
            if signals.forever().next().is_some() {
                exit(&app);
                app.exit(0);
                std::thread::sleep(std::time::Duration::from_secs(5));
                std::process::exit(0);
            }
        })
        .expect("the signal thread starts");
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
        actions::run_action,
        actions::inspect_session,
        actions::pending_read_marks,
        actions::session_reference,
        actions::copy_command,
        clipboard::copy_text,
        screens::screen_visible,
        screens::accounts_view,
        screens::machines_view,
        screens::machine_resolve,
        screens::machine_check,
        screens::projects_view,
        terminal::terminal_connect,
        terminal::terminal_write,
        terminal::terminal_resize,
        terminal::terminal_ack,
        terminal::terminal_disconnect,
        terminal::terminal_retarget,
        terminal::terminal_status,
        composer::composer_open,
        composer::composer_edit,
        composer::composer_pick,
        composer::composer_attach,
        composer::composer_paste_image,
        composer::composer_remove,
        composer::composer_undo_remove,
        composer::composer_allow_retry,
        #[cfg(feature = "media")]
        voice::voice_open,
        #[cfg(feature = "media")]
        voice::voice_input,
        #[cfg(feature = "media")]
        voice::voice_close,
        #[cfg(feature = "media")]
        voice::voice_microphones,
        #[cfg(feature = "media")]
        voice::voice_video,
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

#[cfg(all(test, feature = "live"))]
mod tests {
    use super::*;
    use composer::tests::{dir, drafts, session, until, KEY};
    use composer::{Composers, Deps, Job};
    use std::sync::Arc;
    use std::time::Duration;
    use zerus_client_core::attachments::Selection;
    use zerus_client_core::drafts::read_draft;

    #[test]
    fn the_exit_closes_the_composers_before_the_store_flushes() {
        let root = dir("exit-order");
        let file = root.join("notes.txt");
        std::fs::write(&file, "attached while quitting").unwrap();
        let store = drafts(&root);
        // An attach read still under way when the app quits.
        let composers = Composers::new(
            Arc::clone(&store),
            Deps {
                spawn: Arc::new(|job: Job| {
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(200));
                        job();
                    });
                }),
                notify: Arc::new(|| {}),
            },
        );
        composers.open(Some(session()));
        let view = until(&composers, |v| v.loaded);
        composers.attach_paths(view.handle, vec![file], Selection::default());
        let mut at_flush = None;
        close_drafts(Some(&composers), || {
            at_flush = composers.current();
            store.flush();
        });
        let at_flush = at_flush.unwrap();
        assert!(
            !at_flush.attaching && at_flush.attachments.len() == 1,
            "the store flushed before the composers closed: {at_flush:?}"
        );
        assert_eq!(
            read_draft(&root.join("drafts"), KEY)
                .draft
                .draft
                .attachments
                .len(),
            1
        );
    }

    #[test]
    fn the_draft_store_is_the_clients_own() {
        let data = std::path::Path::new("/home/example/.local/share/dev.zerus.next");
        assert_eq!(drafts_root(data, false), data.join("drafts"));
        assert_eq!(
            drafts_root(data, true),
            data.join("drafts-replay"),
            "a replay never touches the real drafts"
        );
        if let Some(qt) = zerus_client_core::drafts::qt_root() {
            let root = drafts_root(data, false);
            assert!(
                !root.starts_with(&qt) && !qt.starts_with(&root),
                "Qt's store is only read"
            );
        }
    }
}
