//! Whether the app window is shown, as the Terminal needs it.
//!
//! Qt disconnects the Terminal in `hideEvent` (hide or minimize) and only
//! then. A webview's own `visibilitychange` also fires when macOS occludes the
//! window behind another one, which Qt ignores, so the Rust side reports
//! `visible = shown and not minimized` as the `window-visibility` event, once
//! per change.

use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow, Window};

pub const EVENT: &str = "window-visibility";

#[derive(Default)]
pub struct Visibility(Mutex<Option<bool>>);

impl Visibility {
    /// Records `visible`; true when it differs from the last one reported.
    pub fn changed(&self, visible: bool) -> bool {
        let mut last = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let changed = *last != Some(visible);
        *last = Some(visible);
        changed
    }
}

/// Reads the window's state and emits `{visible}` if it changed.
pub fn sync(window: &Window) {
    report(
        window.app_handle(),
        window.is_visible().unwrap_or(true) && !window.is_minimized().unwrap_or(false),
    );
}

/// `sync` for a command's `WebviewWindow` (the measurement's hide and show).
pub fn sync_webview(window: &WebviewWindow) {
    report(
        window.app_handle(),
        window.is_visible().unwrap_or(true) && !window.is_minimized().unwrap_or(false),
    );
}

fn report(app: &AppHandle, visible: bool) {
    if app.state::<Visibility>().changed(visible) {
        if let Err(error) = app.emit(EVENT, serde_json::json!({ "visible": visible })) {
            eprintln!("zerus-client: could not report the window state: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_change_is_reported() {
        let v = Visibility::default();
        assert!(v.changed(true), "the first state is reported");
        assert!(!v.changed(true));
        assert!(v.changed(false));
        assert!(!v.changed(false));
        assert!(v.changed(true));
    }
}
