//! Hold the first presentation until the page has applied its saved skin.
//! Readiness belongs to one created window; late pages cannot reveal a successor.
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

use tauri::{Manager, WebviewWindow};

const SHOW_DEADLINE: Duration = Duration::from_secs(2);

#[derive(Default)]
struct Gate {
    generation: String,
    revision: u64,
    ready: bool,
    requested: bool,
    deadline: Option<Instant>,
}
impl Gate {
    fn request(&mut self, now: Instant) -> (bool, Option<Instant>) {
        if self.ready {
            return (true, None);
        }
        self.requested = true;
        let start = self.deadline.is_none();
        let deadline = *self.deadline.get_or_insert(now + SHOW_DEADLINE);
        (false, start.then_some(deadline))
    }
    fn reveal(&mut self) -> bool {
        self.ready = true;
        self.deadline = None;
        std::mem::take(&mut self.requested)
    }
    fn expired(&mut self, generation: &str, now: Instant) -> bool {
        if self.generation != generation || self.ready || self.deadline.is_none_or(|at| now < at) {
            return false;
        }
        self.reveal()
    }
    fn accepts(&self, generation: &str, revision: u64) -> bool {
        self.generation == generation && revision > self.revision
    }
    fn painted(&mut self, revision: u64) -> bool {
        self.revision = revision;
        self.reveal()
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
    let app = window.app_handle();
    let state = app.state::<Theme>();
    let (show, deadline, generation) = {
        let mut gate = state.0.lock().unwrap_or_else(|e| e.into_inner());
        let (show, deadline) = gate.request(Instant::now());
        (show, deadline, gate.generation.clone())
    };
    if let Some(deadline) = deadline {
        let handle = app.clone();
        let selected = generation.clone();
        let timer = std::thread::Builder::new()
            .name("window-theme-deadline".into())
            .spawn(move || {
                std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
                let app = handle.clone();
                let _ = handle.run_on_main_thread(move || {
                    let show = app
                        .state::<Theme>()
                        .0
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .expired(&selected, Instant::now());
                    present(&app, show);
                });
            });
        if timer.is_err() {
            // An unavailable timer must fail open for the original show too.
            return state
                .0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .expired(&generation, deadline);
        }
    }
    show
}
fn present(app: &tauri::AppHandle, show: bool) {
    if show && crate::lifecycle::request_main(app).is_err() {
        crate::startup_log::record("failed", "themed_window_presentation_failed", None);
    }
}

// Reserve under the gate, perform native work without its mutex, then finish
// even if a setter failed. Visibility is more important than a paint fallback.
fn apply_theme(
    state: &Mutex<Gate>,
    generation: &str,
    revision: u64,
    paint: impl FnOnce() -> Result<(), ()>,
) -> Option<(bool, bool)> {
    {
        let mut gate = state.lock().unwrap_or_else(|e| e.into_inner());
        if !gate.accepts(generation, revision) {
            return None;
        }
        gate.revision = revision;
    }
    let failed = paint().is_err();
    let mut gate = state.lock().unwrap_or_else(|e| e.into_inner());
    if gate.generation != generation || gate.revision != revision {
        return None;
    }
    Some((gate.painted(revision), failed))
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
        let result = apply_theme(&state.0, &generation, revision, || {
            let [r, g, b] = background;
            let window_color =
                selected.set_background_color(Some(tauri::window::Color(r, g, b, 255)));
            // Do not call a Tauri dispatcher from inside with_webview: Wry
            // retains its window-id mutex during this callback.
            let webview_color = selected.with_webview(move |native| {
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
            });
            window_color.and(webview_color).map_err(|_| ())
        });
        if let Some((show, failed)) = result {
            present(app, show);
            if failed {
                crate::startup_log::record("fallback", "window_theme_setter_failed", None);
            }
        }
    })
    .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_deadline_releases_a_requested_show_without_javascript_and_is_never_renewed() {
        let mut gate = Gate {
            generation: "original".into(),
            ..Gate::default()
        };
        let now = Instant::now();
        assert_eq!(gate.request(now), (false, Some(now + SHOW_DEADLINE)));
        assert_eq!(gate.request(now + Duration::from_secs(1)), (false, None));
        assert!(!gate.expired("original", now + SHOW_DEADLINE - Duration::from_nanos(1)));
        assert!(!gate.ready);
        assert!(gate.expired("original", now + SHOW_DEADLINE));
        assert_eq!(gate.request(now + SHOW_DEADLINE), (true, None));
        assert!(
            !gate.painted(1),
            "late JavaScript must not reopen the window"
        );
    }
    #[test]
    fn deadline_keeps_background_and_cancelled_shows_hidden_and_rejects_a_replaced_window() {
        let now = Instant::now();
        let mut gate = Gate {
            generation: "original".into(),
            ..Gate::default()
        };
        assert!(!gate.expired("original", now + SHOW_DEADLINE));
        assert!(!gate.ready, "background starts have no show deadline");
        gate.request(now);
        assert!(!gate.expired("old-window", now + SHOW_DEADLINE));
        assert!(!gate.ready);
        gate.requested = false; // the same explicit hide used by the native path
        assert!(!gate.expired("original", now + SHOW_DEADLINE));
        assert!(
            gate.ready,
            "later Open remains usable even if JavaScript never starts"
        );
        assert!(!gate.requested);
    }
    #[test]
    fn failed_native_setters_still_release_the_show_without_holding_the_gate_mutex() {
        let now = Instant::now();
        for (window_ok, webview_ok) in [(false, true), (true, false), (false, false)] {
            let state = Mutex::new(Gate {
                generation: "original".into(),
                ..Gate::default()
            });
            state.lock().unwrap().request(now);
            let result = apply_theme(&state, "original", 1, || {
                assert!(
                    state.try_lock().is_ok(),
                    "native calls must not hold the theme mutex"
                );
                if window_ok && webview_ok {
                    Ok(())
                } else {
                    Err(())
                }
            });
            assert_eq!(result, Some((true, true)));
            assert!(state.lock().unwrap().ready);
            assert!(!state
                .lock()
                .unwrap()
                .expired("original", now + SHOW_DEADLINE));
        }
    }
    #[test]
    fn first_show_waits_for_theme_and_later_shows_are_immediate() {
        let mut gate = Gate::default();
        assert!(!gate.request(Instant::now()).0);
        assert!(!gate.request(Instant::now()).0);
        assert!(gate.painted(1));
        assert!(gate.request(Instant::now()).0);
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
        assert!(!gate.request(Instant::now()).0);
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
