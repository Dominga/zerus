//! The clipboard, written from Rust: the webview loses the user activation
//! `navigator.clipboard` needs while `hgs` runs. Built with and without the
//! `live` feature, so Copy session name works in both.

use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

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
#[tauri::command]
pub fn copy_text(app: AppHandle, text: String) -> Result<(), String> {
    copyable(&text)?;
    app.clipboard()
        .write_text(text)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::copyable;

    #[test]
    fn refuses_an_empty_text_and_a_line_break() {
        assert!(copyable("claude/sample-project/a").is_ok());
        assert!(copyable("  \t").unwrap_err().starts_with("nothing to copy"));
        assert!(copyable("a\nb").unwrap_err().contains("newline"));
        assert!(copyable("a\rb").unwrap_err().contains("newline"));
    }
}
