//! Hold the first presentation until the page has applied its saved skin.
//! Readiness belongs to one created window; late pages cannot reveal a successor.
use std::sync::Mutex;
use tauri::{Manager, WebviewWindow};

#[derive(Default)]
struct Gate {
    generation: String,
    revision: u64,
    ready: bool,
    requested: bool,
}
impl Gate {
    fn request(&mut self) -> bool {
        if self.ready {
            true
        } else {
            self.requested = true;
            false
        }
    }
    fn accepts(&self, generation: &str, revision: u64) -> bool {
        self.generation == generation && revision > self.revision
    }
    fn painted(&mut self, revision: u64) -> bool {
        self.revision = revision;
        self.ready = true;
        std::mem::take(&mut self.requested)
    }
}
#[derive(Default)]
pub struct Theme(Mutex<Gate>);

pub fn register(app: &tauri::AppHandle) -> String {
    let generation = uuid::Uuid::new_v4().to_string();
    *app.state::<Theme>()
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Gate {
        generation: generation.clone(),
        ..Gate::default()
    };
    generation
}

/// Every production show path checks this on the main thread immediately before
/// mapping. A hidden startup is not a request to show when theme loading ends.
pub fn can_show(window: &WebviewWindow) -> bool {
    window
        .app_handle()
        .state::<Theme>()
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .request()
}
pub fn hidden(app: &tauri::AppHandle) {
    app.state::<Theme>()
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .requested = false;
}

/// CSS resolves the active skin, including imported colors and system theme.
/// The native/webview surfaces receive that opaque RGB BEFORE a queued show.
#[tauri::command]
pub fn window_theme(
    window: WebviewWindow,
    generation: String,
    revision: u64,
    background: [u8; 3],
) -> Result<(), String> {
    if window.label() != crate::MAIN_WINDOW || generation.len() != 36 || revision == 0 {
        return Err("Invalid window theme".into());
    }
    let app = window.app_handle().clone();
    app.run_on_main_thread(move || {
        let selected = window;
        let app = selected.app_handle();
        let state = app.state::<Theme>();
        let mut gate = state.0.lock().unwrap_or_else(|e| e.into_inner());
        if !gate.accepts(&generation, revision) {
            return;
        }
        let [r, g, b] = background;
        // The pinned Tauri/Wry API sets the native window and the WKWebView /
        // WebView2 background as well. Keep this outside with_webview.
        if let Err(error) = selected.set_background_color(Some(tauri::window::Color(r, g, b, 255)))
        {
            eprintln!("zerus-client: could not set window theme background: {error}");
            return;
        }
        // Do not call a Tauri dispatcher from inside with_webview: Wry
        // retains its window-id mutex during this callback.
        if let Err(error) = selected.with_webview(move |native| {
            #[cfg(target_os = "linux")]
            {
                use webkit2gtk::WebViewExt;
                native.inner().set_background_color(&gtk::gdk::RGBA::new(
                    r as f64 / 255.,
                    g as f64 / 255.,
                    b as f64 / 255.,
                    1.,
                ));
            }
            #[cfg(not(target_os = "linux"))]
            let _ = native;
        }) {
            eprintln!("zerus-client: could not set webview theme background: {error}");
            return;
        }
        let show = gate.painted(revision);
        drop(gate);
        if show {
            if let Err(error) = crate::lifecycle::request_main(app) {
                eprintln!("zerus-client: could not present themed window: {error}");
            }
        }
    })
    .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn first_show_waits_for_theme_and_later_shows_are_immediate() {
        let mut gate = Gate::default();
        assert!(!gate.request());
        assert!(!gate.request());
        assert!(gate.painted(1));
        assert!(gate.request());
        assert!(
            !gate.painted(2),
            "theme changes cannot reopen a hidden window"
        );
    }
    #[test]
    fn background_start_or_cancelled_show_cannot_be_reopened_by_theme_loading() {
        let mut gate = Gate::default();
        assert!(!gate.painted(1));
        let mut gate = Gate::default();
        assert!(!gate.request());
        gate.requested = false;
        assert!(!gate.painted(1));
    }
    #[test]
    fn late_page_and_out_of_order_theme_updates_cannot_repaint_a_newer_window() {
        let mut gate = Gate {
            generation: "original".into(),
            ..Gate::default()
        };
        assert!(gate.accepts("original", 1));
        gate.painted(2);
        assert!(!gate.accepts("original", 1));
        assert!(!gate.accepts("original", 2));
        assert!(!gate.accepts("replaced", 3));
        assert!(gate.accepts("original", 3));
    }
}
