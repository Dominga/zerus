//! A small note while a Windows start picks its hgs before the tray icon
//! exists (Qt's `StartupNotice`, `tray/src/StartupNotice.{h,cpp}`,
//! `main.cpp:102-140`; zerus-5wmh.13.14.4): `hgs session-host status` may take
//! 15 s and the refresh `session-host install` 60 s. It appears only once the
//! checks have run for [`DELAY`], so a quick start shows nothing; it never
//! takes the focus, follows the action that runs, and goes when they end.
//! The timing is portable so its tests run everywhere; the window is Windows'.
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Qt's `StartupNotice(int delayMs = 500)`.
pub const DELAY: Duration = Duration::from_millis(500);
/// How often a shown note handles its window's messages and the next text.
const PUMP: Duration = Duration::from_millis(30);

/// Qt's text for the `hgs session-host` action that runs now.
pub fn text(action: &str) -> zerus_client_core::text::Text {
    if action == "install" {
        zerus_client_core::text!(
            "Starting Zerus…\nUpdating the hgs command line for local sessions"
        )
    } else {
        zerus_client_core::text!("Starting Zerus…")
    }
}

/// The window a note shows, made and used on the note's own thread.
pub trait NoteWindow {
    fn open(&mut self, text: &str);
    fn retext(&mut self, text: &str);
    /// Handles the window's pending messages, then waits up to `wait`.
    fn pump(&mut self, wait: Duration);
    fn close(&mut self);
}

#[derive(Default)]
struct Shared {
    text: Option<String>,
    changed: bool,
    done: bool,
}

/// A running note: [`StartupNote::step`] names the action that starts, and
/// dropping it ends the note.
pub struct StartupNote {
    shared: Arc<(Mutex<Shared>, Condvar)>,
    thread: Option<JoinHandle<()>>,
}

impl StartupNote {
    /// Starts the note's thread with the window `make` builds there.
    pub fn start<W: NoteWindow>(
        delay: Duration,
        make: impl FnOnce() -> Option<W> + Send + 'static,
    ) -> Self {
        let shared = Arc::new((Mutex::new(Shared::default()), Condvar::new()));
        let worker = Arc::clone(&shared);
        let thread = std::thread::Builder::new()
            .name("startup-note".into())
            .spawn(move || run(&worker, delay, make))
            .ok();
        Self { shared, thread }
    }

    pub fn step(&self, text: String) {
        let (state, wake) = &*self.shared;
        let mut state = state.lock().unwrap_or_else(PoisonError::into_inner);
        state.text = Some(text);
        state.changed = true;
        wake.notify_all();
    }
}

impl Drop for StartupNote {
    fn drop(&mut self) {
        let (state, wake) = &*self.shared;
        state.lock().unwrap_or_else(PoisonError::into_inner).done = true;
        wake.notify_all();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The note's thread: nothing until `delay` has passed with a text and the
/// checks still running; then the window, following each new text, until done.
fn run<W: NoteWindow>(
    shared: &(Mutex<Shared>, Condvar),
    delay: Duration,
    make: impl FnOnce() -> Option<W>,
) {
    let (state, wake) = shared;
    let deadline = Instant::now() + delay;
    {
        let mut guard = state.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if guard.done {
                return;
            }
            let now = Instant::now();
            if now >= deadline && guard.text.is_some() {
                break;
            }
            let wait = deadline.saturating_duration_since(now).max(PUMP);
            guard = wake
                .wait_timeout(guard, wait)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
    let Some(mut window) = make() else { return };
    // Read after the window exists: the checks may have ended or moved on
    // while it was made (zerus-ogdgc).
    let first = {
        let mut guard = state.lock().unwrap_or_else(PoisonError::into_inner);
        guard.changed = false;
        (!guard.done).then(|| guard.text.clone().unwrap_or_default())
    };
    let Some(first) = first else {
        // Never shown; destroyed on this thread, which made it.
        window.close();
        return;
    };
    window.open(&first);
    loop {
        let next = {
            let mut guard = state.lock().unwrap_or_else(PoisonError::into_inner);
            if guard.done {
                break;
            }
            std::mem::take(&mut guard.changed)
                .then(|| guard.text.clone())
                .flatten()
        };
        if let Some(text) = next {
            window.retext(&text);
        }
        window.pump(PUMP);
    }
    window.close();
}

#[cfg(windows)]
pub use native::Window;

#[cfg(windows)]
mod native {
    use super::NoteWindow;
    use std::time::Duration;
    use windows_sys::Win32::{
        Foundation::{HWND, RECT},
        Graphics::Gdi::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::*,
    };

    /// Qt's `setMargin(16)`.
    const MARGIN: i32 = 16;
    /// winuser.h's `SS_CENTER` (windows-sys keeps it behind another feature).
    const SS_CENTER: u32 = 0x0001;

    fn wide(text: &str) -> Vec<u16> {
        // A static control breaks lines at CR LF.
        text.replace('\n', "\r\n")
            .encode_utf16()
            .chain(Some(0))
            .collect()
    }

    /// A borderless, topmost tool window that never activates, with the text
    /// centred in the system message font, centred on the work area.
    pub struct Window {
        frame: HWND,
        label: HWND,
        font: HFONT,
    }

    impl Window {
        pub fn new() -> Option<Self> {
            let class = wide("ZerusStartupNote");
            let module = unsafe { GetModuleHandleW(std::ptr::null()) };
            let descriptor = WNDCLASSW {
                lpfnWndProc: Some(DefWindowProcW),
                hInstance: module,
                lpszClassName: class.as_ptr(),
                hbrBackground: (COLOR_WINDOW + 1) as usize as HBRUSH,
                ..unsafe { std::mem::zeroed() }
            };
            // A second registration in this process fails harmlessly.
            unsafe { RegisterClassW(&descriptor) };
            let frame = unsafe {
                CreateWindowExW(
                    WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                    class.as_ptr(),
                    wide("Zerus").as_ptr(),
                    WS_POPUP | WS_BORDER,
                    0,
                    0,
                    0,
                    0,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    module,
                    std::ptr::null(),
                )
            };
            if frame.is_null() {
                return None;
            }
            let label = unsafe {
                CreateWindowExW(
                    0,
                    wide("STATIC").as_ptr(),
                    wide("").as_ptr(),
                    WS_CHILD | WS_VISIBLE | SS_CENTER,
                    MARGIN,
                    MARGIN,
                    0,
                    0,
                    frame,
                    std::ptr::null_mut(),
                    module,
                    std::ptr::null(),
                )
            };
            let mut metrics: NONCLIENTMETRICSW = unsafe { std::mem::zeroed() };
            metrics.cbSize = std::mem::size_of::<NONCLIENTMETRICSW>() as u32;
            let read = unsafe {
                SystemParametersInfoW(
                    SPI_GETNONCLIENTMETRICS,
                    metrics.cbSize,
                    (&mut metrics as *mut NONCLIENTMETRICSW).cast(),
                    0,
                )
            };
            let font = if read != 0 {
                unsafe { CreateFontIndirectW(&metrics.lfMessageFont) }
            } else {
                std::ptr::null_mut()
            };
            let shown = if font.is_null() {
                unsafe { GetStockObject(DEFAULT_GUI_FONT) }
            } else {
                font
            };
            unsafe { SendMessageW(label, WM_SETFONT, shown as usize, 0) };
            Some(Self { frame, label, font })
        }

        /// Sizes the window to its text and centres it on the work area.
        fn place(&self, text: &[u16]) {
            let mut bounds = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            unsafe {
                let context = GetDC(self.label);
                let font = SendMessageW(self.label, WM_GETFONT, 0, 0) as HGDIOBJ;
                let previous = SelectObject(context, font);
                DrawTextW(
                    context,
                    text.as_ptr(),
                    -1,
                    &mut bounds,
                    DT_CALCRECT | DT_CENTER,
                );
                SelectObject(context, previous);
                ReleaseDC(self.label, context);
            }
            let (width, height) = (bounds.right - bounds.left, bounds.bottom - bounds.top);
            let mut area = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            unsafe {
                SystemParametersInfoW(SPI_GETWORKAREA, 0, (&mut area as *mut RECT).cast(), 0)
            };
            let (outer_w, outer_h) = (width + 2 * MARGIN + 2, height + 2 * MARGIN + 2);
            let x = area.left + (area.right - area.left - outer_w) / 2;
            let y = area.top + (area.bottom - area.top - outer_h) / 2;
            unsafe {
                SetWindowPos(
                    self.label,
                    std::ptr::null_mut(),
                    MARGIN,
                    MARGIN,
                    width,
                    height,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                SetWindowPos(
                    self.frame,
                    HWND_TOPMOST,
                    x,
                    y,
                    outer_w,
                    outer_h,
                    SWP_NOACTIVATE,
                );
            }
        }
    }

    impl NoteWindow for Window {
        fn open(&mut self, text: &str) {
            self.retext(text);
            unsafe { ShowWindow(self.frame, SW_SHOWNOACTIVATE) };
        }
        fn retext(&mut self, text: &str) {
            let text = wide(text);
            unsafe { SetWindowTextW(self.label, text.as_ptr()) };
            self.place(&text);
        }
        fn pump(&mut self, wait: Duration) {
            let mut message: MSG = unsafe { std::mem::zeroed() };
            while unsafe { PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) } != 0
            {
                unsafe {
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
            std::thread::sleep(wait);
        }
        fn close(&mut self) {
            unsafe { DestroyWindow(self.frame) };
            if !self.font.is_null() {
                unsafe { DeleteObject(self.font) };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A window that records what the note did with it.
    struct Fake(Arc<Mutex<Vec<String>>>);
    impl NoteWindow for Fake {
        fn open(&mut self, text: &str) {
            self.0.lock().unwrap().push(format!("open {text}"));
        }
        fn retext(&mut self, text: &str) {
            self.0.lock().unwrap().push(format!("text {text}"));
        }
        fn pump(&mut self, wait: Duration) {
            std::thread::sleep(wait);
        }
        fn close(&mut self) {
            self.0.lock().unwrap().push("close".into());
        }
    }

    fn note(delay: Duration) -> (StartupNote, Arc<Mutex<Vec<String>>>) {
        let log = Arc::new(Mutex::new(Vec::new()));
        let made = Arc::clone(&log);
        (StartupNote::start(delay, move || Some(Fake(made))), log)
    }

    fn until(log: &Arc<Mutex<Vec<String>>>, entry: &str) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !log.lock().unwrap().iter().any(|line| line == entry) {
            assert!(
                Instant::now() < deadline,
                "no {entry:?}: {:?}",
                log.lock().unwrap()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn a_quick_start_shows_nothing() {
        let (note, log) = note(Duration::from_millis(300));
        note.step("Starting Zerus…".into());
        drop(note);
        assert!(log.lock().unwrap().is_empty());
    }

    #[test]
    fn a_slow_check_shows_the_note_follows_its_action_and_closes_with_it() {
        let (note, log) = note(Duration::from_millis(20));
        note.step(crate::native_text::render(&text("status")));
        until(&log, "open Starting Zerus…");
        note.step(crate::native_text::render(&text("install")));
        until(
            &log,
            "text Starting Zerus…\nUpdating the hgs command line for local sessions",
        );
        drop(note);
        assert_eq!(
            log.lock().unwrap().last().map(String::as_str),
            Some("close")
        );
    }

    #[test]
    fn no_window_opens_before_an_action_or_when_none_can_be_made() {
        let (note, log) = note(Duration::ZERO);
        std::thread::sleep(Duration::from_millis(60));
        assert!(log.lock().unwrap().is_empty(), "nothing runs yet");
        drop(note);
        let none = StartupNote::start::<Fake>(Duration::ZERO, || None);
        none.step("Starting Zerus…".into());
        std::thread::sleep(Duration::from_millis(60));
        drop(none);
    }

    /// A note whose window is made only once `release` sends, after
    /// `making` has told the test that making it began.
    struct Gated {
        note: StartupNote,
        log: Arc<Mutex<Vec<String>>>,
        making: std::sync::mpsc::Receiver<()>,
        release: std::sync::mpsc::Sender<()>,
    }

    fn gated_note() -> Gated {
        let log = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&log);
        let (made, making) = std::sync::mpsc::channel();
        let (release, gate) = std::sync::mpsc::channel();
        let note = StartupNote::start(Duration::ZERO, move || {
            made.send(()).unwrap();
            gate.recv().unwrap();
            Some(Fake(recorded))
        });
        Gated {
            note,
            log,
            making,
            release,
        }
    }

    #[test]
    fn checks_that_end_while_the_window_is_made_never_show_it() {
        let Gated {
            note,
            log,
            making,
            release,
        } = gated_note();
        note.step("Starting Zerus…".into());
        making.recv_timeout(Duration::from_secs(5)).unwrap();
        let state = Arc::clone(&note.shared);
        let ending = std::thread::spawn(move || drop(note));
        let deadline = Instant::now() + Duration::from_secs(5);
        while !state.0.lock().unwrap().done {
            assert!(Instant::now() < deadline, "the note was not ended");
            std::thread::sleep(Duration::from_millis(1));
        }
        release.send(()).unwrap();
        ending.join().unwrap();
        assert_eq!(*log.lock().unwrap(), ["close"], "made, never shown");
    }

    #[test]
    fn an_action_started_while_the_window_is_made_is_its_first_text() {
        let Gated {
            note,
            log,
            making,
            release,
        } = gated_note();
        note.step("status".into());
        making.recv_timeout(Duration::from_secs(5)).unwrap();
        note.step("install".into());
        release.send(()).unwrap();
        until(&log, "open install");
        drop(note);
        assert_eq!(*log.lock().unwrap(), ["open install", "close"]);
    }

    #[test]
    fn the_note_waits_qts_half_second() {
        assert_eq!(DELAY, Duration::from_millis(500));
    }

    #[test]
    fn the_text_names_the_session_host_action() {
        assert_eq!(
            crate::native_text::render(&text("status")),
            "Starting Zerus…"
        );
        assert_eq!(
            crate::native_text::render(&text("install")),
            "Starting Zerus…\nUpdating the hgs command line for local sessions"
        );
    }
}
