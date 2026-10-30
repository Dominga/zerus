//! The refresh rate of the display the main window is on, so a measurement
//! judges frame pacing against the real T = 1000 / Hz instead of an estimate.
//!
//! `ZERUS_CLIENT_PERF_HZ` overrides it. macOS reads
//! `NSScreen.maximumFramesPerSecond`; Linux reads the refresh rate of the
//! window's GdkMonitor. Anything unknown is None and the frontend estimates.

use tauri::WebviewWindow;

/// `ZERUS_CLIENT_PERF_HZ` as a rate: a positive, finite number of hertz.
pub fn parse_override(value: Option<&str>) -> Option<f64> {
    let hz: f64 = value?.trim().parse().ok()?;
    (hz.is_finite() && hz > 0.0 && hz <= 1000.0).then_some(hz)
}

/// A GdkMonitor refresh rate (millihertz, 0 when unknown) in hertz.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn from_millihertz(millihertz: i32) -> Option<f64> {
    (millihertz > 0).then(|| f64::from(millihertz) / 1000.0)
}

/// The window's display rate. Called on the main thread (a synchronous command).
pub fn display_hz(window: &WebviewWindow) -> Option<f64> {
    if let Some(hz) = parse_override(std::env::var("ZERUS_CLIENT_PERF_HZ").ok().as_deref()) {
        return Some(hz);
    }
    native(window)
}

#[cfg(target_os = "macos")]
fn native(_window: &WebviewWindow) -> Option<f64> {
    let mtm = objc2::MainThreadMarker::new()?;
    crate::platform::macos::screen_max_fps(mtm)
        .filter(|fps| *fps > 0)
        .map(|fps| fps as f64)
}

#[cfg(target_os = "linux")]
fn native(window: &WebviewWindow) -> Option<f64> {
    use gtk::prelude::*;
    let gtk_window = window.gtk_window().ok()?;
    let gdk_window = gtk_window.window()?;
    let monitor = gdk_window.display().monitor_at_window(&gdk_window)?;
    from_millihertz(monitor.refresh_rate())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn native(_window: &WebviewWindow) -> Option<f64> {
    None
}

/// The display GDK opened: `:N` for an X server (Xvfb), `wayland-N` for a
/// Wayland session. GTK falls back to `wayland-0` when WAYLAND_DISPLAY is unset,
/// so unsetting it does not keep a window off a desktop; GDK_BACKEND=x11 does.
#[cfg(target_os = "linux")]
pub fn display_name() -> Option<String> {
    gtk::gdk::Display::default().map(|display| display.name().to_string())
}

#[cfg(not(target_os = "linux"))]
pub fn display_name() -> Option<String> {
    None
}

/// With `ZERUS_CLIENT_HEADLESS=1` a window may open only on an X server
/// display (`:N`), never on a desktop session.
pub fn headless_allowed(headless: bool, display: Option<&str>) -> bool {
    !headless || display.is_some_and(|name| name.starts_with(':'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_override_takes_positive_rates_only() {
        assert_eq!(parse_override(Some("120")), Some(120.0));
        assert_eq!(parse_override(Some(" 143.856 ")), Some(143.856));
        assert_eq!(parse_override(Some("0")), None);
        assert_eq!(parse_override(Some("-60")), None);
        assert_eq!(parse_override(Some("fast")), None);
        assert_eq!(parse_override(Some("inf")), None);
        assert_eq!(parse_override(Some("")), None);
        assert_eq!(parse_override(None), None);
    }

    #[test]
    fn a_headless_run_opens_windows_on_an_x_server_only() {
        assert!(headless_allowed(false, Some("wayland-0")));
        assert!(headless_allowed(true, Some(":101")));
        assert!(!headless_allowed(true, Some("wayland-0")));
        assert!(!headless_allowed(true, None));
    }

    #[test]
    fn gdk_millihertz_become_hertz() {
        assert_eq!(from_millihertz(143_856), Some(143.856));
        assert_eq!(from_millihertz(60_000), Some(60.0));
        assert_eq!(from_millihertz(0), None);
    }
}
