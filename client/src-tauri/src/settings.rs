//! Settings: the client's local preferences (the core's `settings` store in
//! `<data_dir>/settings.json`), read and written for the webview. See
//! `docs/superpowers/plans/2026-10-28-tauri-client-phase3b-settings.md`.
//!
//! Rust code reads a setting from the managed [`Store`], for example
//! `app.state::<settings::Store>().0.number("processes/activeSeconds")`.

use serde_json::{json, Map, Value};
use std::path::Path;
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};
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

/// Opens the store in `dir`; every change is announced as
/// `settings-changed` with its key and new value.
pub fn open(app: &AppHandle, dir: &Path) -> Store {
    let settings = Settings::open(
        dir.join("settings.json"),
        seed_source(&{
            use tauri::Manager;
            app.state::<crate::identity::Identity>().bundle_id.clone()
        }),
    );
    let handle = app.clone();
    settings.subscribe(Box::new(move |key, value| {
        let _ = handle.emit("settings-changed", json!({ "key": key, "value": value }));
    }));
    Store(settings)
}

/// Every setting, as Qt would read it.
#[tauri::command]
pub async fn settings_get(store: State<'_, Store>) -> Result<Map<String, Value>, String> {
    Ok(store.0.all())
}

/// Stores one setting and returns the value now in effect (clamped as Qt
/// clamps it); `null` removes a shortcut override.
#[tauri::command]
pub async fn settings_set(
    store: State<'_, Store>,
    key: String,
    value: Value,
) -> Result<Value, String> {
    store.0.set(&key, value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_switch_identity_seeds_from_qt() {
        assert!(seed_source("dev.zerus.next").is_none());
        assert!(seed_source("dev.zerus.next.phase2").is_none());
        if std::env::var_os("HOME").is_some() || std::env::var_os("APPDATA").is_some() {
            assert!(seed_source(SWITCH_IDENTITY).is_some());
        }
    }
}
