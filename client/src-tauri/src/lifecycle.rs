//! The menu-bar app's lifecycle, as the Qt GUI behaves on macOS
//! (`tray/src/main.cpp`, `tray/src/MacAppVisibility.mm`): closing the window
//! hides it and the app stays in the menu bar; a Dock icon shows only while
//! the window is open; a Dock click restores a minimised window; quitting
//! removes the single-instance socket. Builds without a menu-bar item close
//! as before. `lib.rs` calls [`on_event`] from its run loop (Decision 15).
use tauri::{AppHandle, Manager, RunEvent, WebviewWindow, WindowEvent};

use crate::MAIN_WINDOW;

/// The close button only hides the window of an app with a menu-bar item
/// (`QApplication::setQuitOnLastWindowClosed(false)`, `main.cpp:279`).
pub fn close_hides(menu_bar: bool) -> bool {
    menu_bar
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    Regular,
    Accessory,
}

/// A normal app while its window is open, shown or minimised, else a
/// menu-bar app (`MacAppVisibility.mm:65-81`).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn policy(window_open: bool) -> Policy {
    if window_open {
        Policy::Regular
    } else {
        Policy::Accessory
    }
}

/// A Dock click restores a minimised window when none is visible
/// (`MacAppVisibility.mm:82-88`).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn reopen_restores(minimized: bool, visible: bool) -> bool {
    minimized && !visible
}

/// The window opens at start for `--sessions`, and always without a menu-bar item.
pub fn window_at_start(opens_sessions: bool, menu_bar: bool) -> bool {
    opens_sessions || !menu_bar
}

/// Starts the fleet at launch, before any window loads (menu-bar builds). A
/// source that fails to start, such as a missing `ZERUS_CLIENT_REPLAY`
/// fixture, is logged and the app runs on: the webview's own `start` tries
/// again and reports it when the window opens. Returns whether it started.
#[cfg_attr(not(feature = "live"), allow(dead_code))]
pub fn start_fleet(start: impl FnOnce() -> Result<String, String>) -> bool {
    match start() {
        Ok(_) => true,
        Err(error) => {
            eprintln!("zerus-client: the data source did not start at launch: {error}");
            false
        }
    }
}

/// Kept in the app state.
pub struct Lifecycle {
    pub menu_bar: bool,
}

fn menu_bar(app: &AppHandle) -> bool {
    app.try_state::<Lifecycle>()
        .is_some_and(|lifecycle| lifecycle.menu_bar)
}

/// The run loop's events this module handles.
pub fn on_event(app: &AppHandle, event: &RunEvent) {
    match event {
        // The last window went away: the app keeps its menu-bar item.
        RunEvent::ExitRequested {
            code: None, api, ..
        } if menu_bar(app) => api.prevent_exit(),
        RunEvent::WindowEvent {
            label,
            event: WindowEvent::CloseRequested { api, .. },
            ..
        } if label == MAIN_WINDOW && close_hides(menu_bar(app)) => {
            api.prevent_close();
            hide_main(app);
        }
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => reopen(app),
        RunEvent::Exit => exit(app),
        _ => {}
    }
}

/// Opens Zerus: Qt's `TrayAgent::showSessions` (`TrayAgent.cpp:781-783`).
pub fn show_main(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let window = match app.get_webview_window(MAIN_WINDOW) {
        Some(window) => window,
        None => crate::create_main_window(app)?,
    };
    if window.is_minimized()? {
        window.unminimize()?;
    }
    window.show()?;
    window.set_focus()?;
    sync_policy(app);
    Ok(window)
}

pub fn hide_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.hide();
    }
    sync_policy(app);
}

#[cfg(target_os = "macos")]
fn window_open(app: &AppHandle) -> bool {
    app.get_webview_window(MAIN_WINDOW).is_some_and(|window| {
        window.is_visible().unwrap_or(false) || window.is_minimized().unwrap_or(false)
    })
}

#[cfg(target_os = "macos")]
fn native(policy: Policy) -> tauri::ActivationPolicy {
    match policy {
        Policy::Regular => tauri::ActivationPolicy::Regular,
        Policy::Accessory => tauri::ActivationPolicy::Accessory,
    }
}

/// Applies [`policy`] to the app (macOS menu-bar builds only).
pub fn sync_policy(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    if menu_bar(app) {
        let _ = app.set_activation_policy(native(policy(window_open(app))));
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

/// The policy the app launches with, set before the run loop starts (macOS
/// menu-bar builds). tao makes every app a normal one when it finishes
/// launching, before `setup` runs, so a hidden start would show a Dock icon
/// first. Qt's bundle starts as a menu-bar app (`LSUIElement`,
/// `tray/packaging/Info.plist:21`).
#[cfg(target_os = "macos")]
pub fn prepare(app: &mut tauri::App, window_at_start: bool) {
    if app
        .try_state::<Lifecycle>()
        .is_some_and(|lifecycle| lifecycle.menu_bar)
    {
        app.set_activation_policy(native(policy(window_at_start)));
    }
}

#[cfg(target_os = "macos")]
fn reopen(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let minimized = window.is_minimized().unwrap_or(false);
        let visible = window.is_visible().unwrap_or(false);
        if reopen_restores(minimized, visible) {
            let _ = window.unminimize();
            let _ = window.set_focus();
        }
    }
}

/// The one run-loop exit hook (Decision 15). `RunEvent::Exit` ends every
/// quit: Quit, a signal (`signals.rs`) and the updater's quit event (tao's
/// `applicationWillTerminate`). Other work before the process ends (drafts,
/// organization) joins here, before the socket goes: Qt's `QLocalServer`
/// outlives the tray agent whose `aboutToQuit` flushes (`main.cpp:340`,
/// `:378`), so the socket is removed last, as `QLocalServer::close` does.
fn exit(app: &AppHandle) {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    if let Some(state) = app.try_state::<crate::instance::InstanceState>() {
        state.release();
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let _ = app;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dock_icon_shows_while_the_window_is_open() {
        assert_eq!(policy(true), Policy::Regular);
        assert_eq!(policy(false), Policy::Accessory);
    }

    #[test]
    fn a_dock_click_restores_only_a_minimised_window() {
        assert!(reopen_restores(true, false));
        assert!(!reopen_restores(true, true));
        assert!(
            !reopen_restores(false, false),
            "a closed window stays closed, as in Qt"
        );
    }

    #[test]
    fn the_window_opens_at_start_only_when_asked_in_the_menu_bar() {
        assert!(
            !window_at_start(false, true),
            "the LaunchAgent start stays in the menu bar"
        );
        assert!(window_at_start(true, true));
        assert!(
            window_at_start(false, false),
            "without a menu-bar item the window is the only way in"
        );
        assert!(close_hides(true));
        assert!(!close_hides(false));
    }

    #[test]
    fn a_source_that_fails_at_launch_leaves_the_app_running() {
        assert!(!start_fleet(|| Err(
            "replay: sample.ndjson: not found".to_owned()
        )));
        assert!(start_fleet(|| Ok("live".to_owned())));
    }
}
