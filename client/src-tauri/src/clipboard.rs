//! The clipboard, written from Rust: the webview loses the user activation
//! `navigator.clipboard` needs while `hgs` runs. Built with and without the
//! `live` feature, so Copy session name works in both.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;
use tauri::AppHandle;
#[cfg(not(target_os = "linux"))]
use tauri_plugin_clipboard_manager::ClipboardExt;

#[cfg(target_os = "linux")]
mod linux;

/// Linux fallbacks can wait for a peer/helper; callers run this off GTK's
/// thread. The plugin also forbids read_text on GTK's thread.
fn write(app: &AppHandle, text: String) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    return linux::write(app, &text);
    #[cfg(not(target_os = "linux"))]
    app.clipboard()
        .write_text(text)
        .map_err(|error| error.to_string())
}

/// Whether `text` may go to the clipboard (Qt's `ClipboardBackend::isCopyable`):
/// a newline would make the shell run the command the moment it is pasted.
fn copyable(text: &str) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("nothing to copy: the command is empty".to_owned());
    }
    if text.contains(['\n', '\r']) {
        return Err("the command has a newline in it -- copying that is dangerous: the shell would run it the moment you paste".to_owned());
    }
    Ok(())
}

/// Puts `text` on the clipboard, refusing an empty text or one with a line break.
#[tauri::command(async)]
pub fn copy_text(app: AppHandle, text: String) -> Result<(), String> {
    copyable(&text)?;
    write(&app, text)
}

/// Whether `text` may go to the clipboard as it is: anything but an empty
/// text. A process's command is copied as the agent ran it, as Qt's
/// ProcessView copies it; the newline rule guards commands Zerus builds.
fn verbatim(text: &str) -> Result<(), String> {
    if text.is_empty() {
        return Err("nothing to copy: the command is empty".to_owned());
    }
    Ok(())
}

/// Puts `text` on the clipboard exactly as it is, refusing only an empty text.
#[tauri::command(async)]
pub fn copy_verbatim(app: AppHandle, text: String) -> Result<(), String> {
    verbatim(&text)?;
    write(&app, text)
}

/// The clipboard's text for Paste in Terminal's right-click menu (Qt's
/// `TerminalScreen::pasteClipboard`, which read `QClipboard::text`); the paste
/// keys arrive as the webview's own paste event. Async, so the read never runs
/// on GTK's thread, which the plugin forbids.
#[tauri::command(async)]
pub fn paste_text(app: AppHandle) -> Result<String, String> {
    tauri_plugin_clipboard_manager::ClipboardExt::clipboard(&app)
        .read_text()
        .map_err(|error| error.to_string())
}

/// How long Paste in the composer waits for the clipboard's file list.
const FILES_BUDGET: Duration = Duration::from_secs(2);
/// A file-list read still blocked: the next paste does not start another.
static READING_FILES: AtomicBool = AtomicBool::new(false);

/// Every copied entry as a path for the composer's attach: absolute and
/// UTF-8 (the webview passes paths as text), or none at all.
fn attachable(files: Vec<PathBuf>) -> Option<Vec<String>> {
    if files.is_empty() {
        return None;
    }
    files
        .into_iter()
        .map(|path| {
            path.is_absolute()
                .then(|| path.into_os_string().into_string().ok())
                .flatten()
        })
        .collect()
}

/// The read, with the clipboard call and the budget given (tests block it).
fn copied_files_with(
    budget: Duration,
    read: impl FnOnce() -> Option<Vec<PathBuf>> + Send + 'static,
) -> Option<Vec<String>> {
    if READING_FILES.swap(true, Ordering::AcqRel) {
        return None;
    }
    let (sender, receiver) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("clipboard-files".into())
        .spawn(move || {
            let files = read();
            READING_FILES.store(false, Ordering::Release);
            let _ = sender.send(files);
        });
    if spawned.is_err() {
        READING_FILES.store(false, Ordering::Release);
        return None;
    }
    attachable(receiver.recv_timeout(budget).ok()??)
}

/// Paste in the composer (Qt's `insertFromMimeData`): the files copied in a
/// file manager, when the clipboard holds a list of local files. None when it
/// holds none, cannot answer within [`FILES_BUDGET`] or an earlier read is
/// still blocked; the webview then pastes what its own event carries. Async,
/// so the read never runs on the UI thread.
#[tauri::command(async)]
pub fn clipboard_files() -> Option<Vec<String>> {
    copied_files_with(FILES_BUDGET, || {
        arboard::Clipboard::new().ok()?.get().file_list().ok()
    })
}

#[cfg(test)]
mod tests {
    use super::{attachable, copied_files_with, copyable, verbatim, FILES_BUDGET};
    use std::path::PathBuf;
    use std::sync::{mpsc, Mutex};
    use std::time::Duration;

    /// The blocked-read flag is process-wide: these tests take turns.
    static SERIAL: Mutex<()> = Mutex::new(());

    fn absolute(name: &str) -> PathBuf {
        PathBuf::from(if cfg!(windows) {
            format!(r"C:\work\{name}")
        } else {
            format!("/work/{name}")
        })
    }

    #[test]
    fn only_a_list_of_absolute_files_is_attachable() {
        let both = vec![absolute("a.txt"), absolute("b.png")];
        assert_eq!(
            attachable(both.clone()),
            Some(
                both.iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect()
            )
        );
        assert_eq!(attachable(Vec::new()), None, "no copied files");
        assert_eq!(
            attachable(vec![absolute("a.txt"), PathBuf::from("relative.txt")]),
            None,
            "one relative entry refuses the whole list"
        );
    }

    #[test]
    fn a_read_answers_within_its_budget() {
        let _turn = SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let file = absolute("notes.txt");
        let copied = file.clone();
        assert_eq!(
            copied_files_with(FILES_BUDGET, move || Some(vec![copied])),
            Some(vec![file.to_string_lossy().into_owned()])
        );
        assert_eq!(
            copied_files_with(FILES_BUDGET, || None),
            None,
            "no file list"
        );
    }

    #[test]
    fn a_blocked_read_answers_none_and_starts_no_second_read_until_it_returns() {
        let _turn = SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (release, blocked) = mpsc::channel::<()>();
        let (started, running) = mpsc::channel::<()>();
        let file = absolute("late.txt");
        let late = file.clone();
        let read = move || {
            let _ = started.send(());
            let _ = blocked.recv();
            Some(vec![late])
        };
        assert_eq!(copied_files_with(Duration::from_millis(20), read), None);
        running.recv().unwrap();
        // Still blocked: a second paste starts nothing.
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = calls.clone();
        assert_eq!(
            copied_files_with(FILES_BUDGET, move || {
                counted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                None
            }),
            None
        );
        assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 0);
        release.send(()).unwrap();
        // Once the blocked call returned, a new paste reads again.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let again = loop {
            let copied = file.clone();
            if let Some(files) = copied_files_with(FILES_BUDGET, move || Some(vec![copied])) {
                break files;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the blocked read never released"
            );
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(again, vec![file.to_string_lossy().into_owned()]);
    }

    #[test]
    fn a_verbatim_copy_refuses_only_an_empty_text() {
        assert!(verbatim("cat <<'EOF'\nline\nEOF").is_ok());
        assert!(verbatim("  \t").is_ok());
        assert!(verbatim("").unwrap_err().starts_with("nothing to copy"));
    }

    #[test]
    fn refuses_an_empty_text_and_a_line_break() {
        assert!(copyable("claude/sample-project/a").is_ok());
        assert!(copyable("  \t").unwrap_err().starts_with("nothing to copy"));
        assert!(copyable("a\nb").unwrap_err().contains("newline"));
        assert!(copyable("a\rb").unwrap_err().contains("newline"));
    }
}
