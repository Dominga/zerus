//! Task 243 (owner, 2026-11-07): the Compact skins draw one title bar that
//! holds the app's controls and the window buttons (Qt's WindowTitleBar,
//! "window":{"main":"frameless"}); the skin layout variant `chrome: unified`
//! turns the native decorations off for the main window and the webview's
//! bar drives the window through these commands. Other skins keep the
//! system title bar (`chrome: system`).
//!
//! Task 297: Windows 11 snap layouts appear on a maximize button that the
//! window's hit test answers with HTMAXBUTTON. Qt's frameless bar answers
//! HTCAPTION for its empty space and HTCLIENT for every control, its maximize
//! button included (tray/src/WindowTitleBar.cpp FramelessWindow::nativeEvent;
//! test_sessionswindow_settings1 framelessTitleBarHitTestsKeepTheButtonsClientArea),
//! so it offers no snap layouts there; this bar keeps that parity.
use tauri::{Runtime, WebviewWindow};

/// The window buttons the bar draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Minimize,
    Maximize,
    Close,
    /// Only reads whether the window is maximized (the bar's Restore glyph).
    State,
}

impl Control {
    pub fn parse(action: &str) -> Option<Self> {
        match action {
            "minimize" => Some(Self::Minimize),
            "maximize" => Some(Self::Maximize),
            "close" => Some(Self::Close),
            "state" => Some(Self::State),
            _ => None,
        }
    }
}

/// The main window with or without its native title bar and frame.
#[tauri::command]
pub fn window_chrome<R: Runtime>(window: WebviewWindow<R>, unified: bool) -> Result<(), String> {
    if window.label() != crate::MAIN_WINDOW {
        return Err("Only the main window draws its own title bar.".into());
    }
    if window.is_decorated().map_err(|error| error.to_string())? == unified {
        window.set_decorations(!unified).map_err(|error| error.to_string())?;
    }
    // Undecorated windows keep the system shadow (and its resize edge on Windows).
    #[cfg(any(windows, target_os = "macos"))]
    let _ = window.set_shadow(true);
    Ok(())
}

/// Minimize, maximize or restore, or close (as the system close button does:
/// the close request reaches the same handlers). Answers whether the window
/// is maximized now.
#[tauri::command]
pub fn window_control<R: Runtime>(window: WebviewWindow<R>, action: String) -> Result<bool, String> {
    let control = Control::parse(&action).ok_or_else(|| format!("Unknown window action: {action}"))?;
    let error = |error: tauri::Error| error.to_string();
    match control {
        Control::Minimize => window.minimize().map_err(error)?,
        Control::Maximize => {
            if window.is_maximized().map_err(error)? {
                window.unmaximize().map_err(error)?;
            } else {
                window.maximize().map_err(error)?;
            }
        }
        Control::Close => window.close().map_err(error)?,
        Control::State => {}
    }
    window.is_maximized().map_err(error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_bars_four_actions_are_accepted() {
        assert_eq!(Control::parse("minimize"), Some(Control::Minimize));
        assert_eq!(Control::parse("maximize"), Some(Control::Maximize));
        assert_eq!(Control::parse("close"), Some(Control::Close));
        assert_eq!(Control::parse("state"), Some(Control::State));
        for other in ["", "Close", "destroy", "fullscreen", "hide"] {
            assert_eq!(Control::parse(other), None, "{other}");
        }
    }
}
