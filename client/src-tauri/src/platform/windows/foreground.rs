//! Bring the main window to the front without Tao's focus fallback. Windows
//! decides from its foreground rules (a toast click lets the service pass its
//! right on); when it refuses, the taskbar button flashes a few times.
use crate::windows_shell::{bring_to_front, Foreground, Front};
use windows_sys::Win32::{Foundation::HWND, UI::WindowsAndMessaging::*};

struct Window(HWND);
impl Foreground for Window {
    fn minimized(&self) -> bool {
        unsafe { IsIconic(self.0) != 0 }
    }
    fn restore(&self) {
        unsafe {
            ShowWindow(self.0, SW_RESTORE);
        }
    }
    fn in_front(&self) -> bool {
        unsafe { GetForegroundWindow() == self.0 }
    }
    fn take_front(&self) -> bool {
        unsafe { SetForegroundWindow(self.0) != 0 }
    }
    fn flash(&self) {
        let info = FLASHWINFO {
            cbSize: std::mem::size_of::<FLASHWINFO>() as u32,
            hwnd: self.0,
            dwFlags: FLASHW_TRAY,
            uCount: 3,
            dwTimeout: 0,
        };
        unsafe {
            FlashWindowEx(&info);
        }
    }
}

/// Call on the main thread after the window is shown.
pub fn present(window: &tauri::WebviewWindow) -> Option<Front> {
    let hwnd = window.hwnd().ok()?;
    Some(bring_to_front(&Window(hwnd.0)))
}

/// Another process's already visible window, raised with this process's own
/// foreground right: a second start that could not hand its activation over
/// (task 110). No right is passed on to that process.
pub fn present_handle(hwnd: HWND) -> Front {
    bring_to_front(&Window(hwnd))
}
