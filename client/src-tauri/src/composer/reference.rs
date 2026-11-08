//! The composer's Reference file on this computer (Qt
//! `MessageComposer::referenceFile`): only the path goes into the draft; the
//! file is never read or uploaded. A remote session's path is checked in the
//! webview only, since its existence cannot be checked from here.
use std::path::{Path, PathBuf};

use tauri::AppHandle;

/// The picker's title (Qt's `QFileDialog` "Reference an existing file").
pub const PICKER_TITLE: &str = "Reference an existing file";

/// The path to insert for `path` on this computer: `~/` names the home
/// folder, and the result must be an absolute path to an existing file
/// (Qt `QFileInfo::isAbsolute` and `isFile`); None otherwise.
pub fn local_file(path: &str, home: Option<&Path>) -> Option<String> {
    let expanded: PathBuf = match path.strip_prefix("~/") {
        Some(rest) => home?.join(rest),
        None => PathBuf::from(path),
    };
    (expanded.is_absolute() && expanded.is_file()).then(|| expanded.to_string_lossy().into_owned())
}

/// [`local_file`] on the blocking pool: a stat can wait on a network drive.
#[tauri::command]
pub async fn reference_local_file(app: AppHandle, path: String) -> Option<String> {
    use tauri::Manager;
    let home = app.path().home_dir().ok();
    tauri::async_runtime::spawn_blocking(move || local_file(&path, home.as_deref()))
        .await
        .ok()
        .flatten()
}

/// Browse… beside the path (local sessions only): the system file dialog,
/// modal to the main window, opened from Rust as the composer's attach
/// picker is. The chosen file, None when cancelled.
#[tauri::command]
pub async fn pick_reference_file(app: AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    // The window handle is asked of the main thread: never block it here.
    let window = crate::main_window(&app);
    let picked = tauri::async_runtime::spawn_blocking(move || {
        let mut dialog = app.dialog().file().set_title(PICKER_TITLE);
        if let Some(window) = &window {
            dialog = dialog.set_parent(window);
        }
        dialog.blocking_pick_file()
    })
    .await
    .ok()
    .flatten()?;
    let path = picked.into_path().ok()?;
    Some(path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_existing_absolute_file_is_referenced() {
        let root =
            std::env::temp_dir().join(format!("zerus-reference-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(root.join("docs")).unwrap();
        let file = root.join("docs").join("notes and spaces.txt");
        std::fs::write(&file, b"kept local").unwrap();
        let shown = file.to_string_lossy().into_owned();
        assert_eq!(local_file(&shown, None), Some(shown.clone()));
        // A folder, a missing file and a relative path are not files of this computer.
        assert_eq!(local_file(&root.join("docs").to_string_lossy(), None), None);
        assert_eq!(
            local_file(&root.join("missing.txt").to_string_lossy(), None),
            None
        );
        assert_eq!(local_file("docs/notes.txt", Some(&root)), None);
        assert_eq!(local_file("", Some(&root)), None);
        // ~/ is the home folder; without one it names nothing.
        let home = root.join("docs/notes and spaces.txt");
        assert_eq!(
            local_file("~/docs/notes and spaces.txt", Some(&root)),
            Some(home.to_string_lossy().into_owned())
        );
        assert_eq!(local_file("~/docs/notes and spaces.txt", None), None);
        assert_eq!(local_file("~/missing.txt", Some(&root)), None);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
