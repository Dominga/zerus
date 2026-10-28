//! Settings: the client's local preferences (the core's `settings` store in
//! `<data_dir>/settings.json`), read and written for the webview. See
//! `docs/superpowers/plans/2026-10-28-tauri-client-phase3b-settings.md`.
//!
//! Rust code reads a setting from the managed [`Store`], for example
//! `app.state::<settings::Store>().0.number("processes/activeSeconds")`.

use serde_json::{json, Value};
use std::path::Path;
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};
use zerus_client_core::settings::{QtStore, Settings};

/// The identity that replaces Qt at the switch: only it seeds from Qt's
/// store, once. The dev identity never does.
pub const SWITCH_IDENTITY: &str = "com.hgdev.hgs-tray";

/// How long quitting waits for the last change to reach the disk.
pub const EXIT_FLUSH: Duration = Duration::from_millis(750);

/// The app's one settings store.
pub struct Store(pub Settings);

/// Qt's store to seed from, for this app identity.
pub fn seed_source(identifier: &str) -> Option<QtStore> {
    (identifier == SWITCH_IDENTITY)
        .then(QtStore::here)
        .flatten()
}

/// Settings > Appearance > Keep Zerus above other windows (WindowLayer.cpp).
pub const ALWAYS_ON_TOP: &str = "workspace/alwaysOnTop";

/// Opens the store in `dir`; every change is announced as
/// `settings-changed` with its key and new value, and Keep Zerus above other
/// windows applies to the main window at once.
pub fn open(app: &AppHandle, dir: &Path) -> Store {
    let settings = Settings::open(
        dir.join("settings.json"),
        seed_source(&{
            use tauri::Manager;
            app.state::<crate::identity::Identity>().bundle_id.clone()
        }),
    );
    let handle = app.clone();
    settings.subscribe(Box::new(move |key, value, revision| {
        if key == ALWAYS_ON_TOP && newer_layer(revision) {
            if let Some(window) = handle.get_webview_window(crate::MAIN_WINDOW) {
                apply_window_layer(&window, value.as_bool().unwrap_or(false));
            }
        }
        let _ = handle.emit(
            "settings-changed",
            json!({ "key": key, "value": value, "revision": revision }),
        );
    }));
    Store(settings)
}

/// Whether this session can keep the window above others, decided once.
static KEEP_ABOVE: OnceLock<bool> = OnceLock::new();
/// The store revision of the last Keep above applied (0: none yet). Listeners
/// run after the store's lock is released, so changes can arrive out of
/// order: only a newer one applies. Never held while calling Tauri.
static LAYER: Mutex<u64> = Mutex::new(0);

/// Records `revision` as applied when it is newer than the last one.
fn newer_layer(revision: u64) -> bool {
    let mut last = LAYER.lock().unwrap_or_else(PoisonError::into_inner);
    if revision <= *last {
        return false;
    }
    *last = revision;
    true
}

/// Whether a window on `display` can be kept above others. GTK has no
/// keep-above on Wayland, where Qt needs KWin scripting (WindowLayer.cpp);
/// X11, macOS and Windows keep it.
pub fn supports_keep_above(display: Option<&str>) -> bool {
    !(cfg!(target_os = "linux") && display.is_some_and(|name| name.starts_with("wayland")))
}

/// Records, from the display GTK opened (display::display_name), whether
/// Keep Zerus above other windows can work.
pub fn init_window_layer(display: Option<&str>) {
    let _ = KEEP_ABOVE.set(supports_keep_above(display));
}

pub fn keep_above_supported() -> bool {
    KEEP_ABOVE.get().copied().unwrap_or(true)
}

/// Keeps `window` above other windows, or not, where the platform can.
pub fn apply_window_layer(window: &WebviewWindow, on_top: bool) {
    if !keep_above_supported() {
        return;
    }
    if let Err(error) = window.set_always_on_top(on_top) {
        eprintln!("zerus-client: Keep Zerus above other windows: {error}");
    }
}

/// Applies the stored Keep Zerus above other windows once the store has
/// loaded (on the switch identity's first start, after the Qt seed), on a
/// thread of its own: setup never waits for the store.
pub fn apply_stored_window_layer(store: &Store, window: WebviewWindow) {
    let settings = store.0.clone();
    let started = std::thread::Builder::new()
        .name("window-layer".into())
        .spawn(move || {
            // Waits for the load without the lock; a change applied since
            // then is newer than the stored value.
            let on = settings.flag(ALWAYS_ON_TOP);
            if *LAYER.lock().unwrap_or_else(PoisonError::into_inner) == 0 {
                apply_window_layer(&window, on);
            }
        });
    if let Err(error) = started {
        eprintln!("zerus-client: Keep Zerus above other windows: {error}");
    }
}

/// Whether Keep Zerus above other windows can work in this session.
#[tauri::command]
pub fn window_layer_supported() -> bool {
    keep_above_supported()
}

/// Every setting, as Qt would read it, and the store's revision.
#[tauri::command]
pub async fn settings_get(store: State<'_, Store>) -> Result<Value, String> {
    let (values, revision) = store.0.all_tracked();
    Ok(json!({ "values": values, "revision": revision }))
}

/// Stores one setting and answers with the value now in effect (clamped as
/// Qt clamps it; `null` for a removed shortcut override) and the store's
/// revision after the change.
#[tauri::command]
pub async fn settings_set(
    store: State<'_, Store>,
    key: String,
    value: Value,
) -> Result<Value, String> {
    let (value, revision) = store.0.set_tracked(&key, value)?;
    Ok(json!({ "value": value, "revision": revision }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keep_above_is_unavailable_only_on_a_wayland_display() {
        assert!(supports_keep_above(Some(":0")));
        assert!(supports_keep_above(None));
        assert_eq!(
            supports_keep_above(Some("wayland-0")),
            !cfg!(target_os = "linux")
        );
    }

    #[test]
    fn only_the_switch_identity_seeds_from_qt() {
        assert!(seed_source("dev.zerus.next").is_none());
        assert!(seed_source("dev.zerus.next.phase2").is_none());
        if std::env::var_os("HOME").is_some() || std::env::var_os("APPDATA").is_some() {
            assert!(seed_source(SWITCH_IDENTITY).is_some());
        }
    }
}
