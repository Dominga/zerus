//! Whether Zerus is the active app on Windows (Qt's `Qt::ApplicationActive`,
//! `AttentionTracker.cpp:45-49,122-125`), for the attention tracker: an
//! active Zerus consumes attention edges without banners
//! (zerus-5wmh.13.14.1). Linux polls GTK's toplevels every 250 ms
//! (`platform/linux_notify.rs`) and macOS follows NSApplication's activity;
//! Windows polls whether the foreground window belongs to this process,
//! which, as in Qt, counts every window of the app (its dialogs and file
//! pickers too), not just the main one.
use std::time::Duration;

/// How often the foreground window is read, as Linux reads GTK's.
pub const POLL: Duration = Duration::from_millis(250);

/// Whether the foreground window's process is this one.
pub fn owned_by(foreground: Option<u32>, own: u32) -> bool {
    foreground == Some(own)
}

/// Reports `active()` every [`POLL`] until `stopped()`. Every poll reports,
/// not only changes, so a desk that starts later still learns the state.
pub fn watch(
    active: impl Fn() -> bool,
    report: impl Fn(bool),
    stopped: impl Fn() -> bool,
    pause: impl Fn(Duration),
) {
    while !stopped() {
        report(active());
        pause(POLL);
    }
}

#[cfg(windows)]
static STOPPED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Starts the watch for this app; [`stop`] ends it.
#[cfg(windows)]
pub fn start(app: &tauri::AppHandle) {
    use std::sync::atomic::Ordering;
    let app = app.clone();
    let started = std::thread::Builder::new()
        .name("active-app".into())
        .spawn(move || {
            watch(
                || owned_by(foreground_process(), std::process::id()),
                |active| crate::desk::set_focused(&app, active),
                || STOPPED.load(Ordering::Relaxed),
                std::thread::sleep,
            )
        });
    if let Err(error) = started {
        eprintln!("zerus-client: active-app watch: {error}");
    }
}

#[cfg(windows)]
pub fn stop() {
    STOPPED.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// The process that owns the foreground window, if there is one.
#[cfg(windows)]
fn foreground_process() -> Option<u32> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId,
    };
    // SAFETY: both calls only read the current foreground window and its owner.
    let window = unsafe { GetForegroundWindow() };
    if window.is_null() {
        return None;
    }
    let mut process = 0_u32;
    unsafe { GetWindowThreadProcessId(window, &mut process) };
    (process != 0).then_some(process)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[test]
    fn only_a_foreground_window_of_this_process_makes_zerus_active() {
        assert!(owned_by(Some(42), 42));
        assert!(!owned_by(Some(7), 42), "another app's window");
        assert!(!owned_by(None, 42), "no foreground window (a locked desk)");
    }

    #[test]
    fn the_watch_reports_every_poll_until_it_is_stopped() {
        let states = RefCell::new(vec![true, true, false, true].into_iter());
        let reported = RefCell::new(Vec::new());
        let pauses = Cell::new(0);
        watch(
            || states.borrow_mut().next().unwrap_or(false),
            |active| reported.borrow_mut().push(active),
            || pauses.get() == 4,
            |pause| {
                assert_eq!(pause, POLL);
                pauses.set(pauses.get() + 1);
            },
        );
        assert_eq!(*reported.borrow(), [true, true, false, true]);
    }
}
