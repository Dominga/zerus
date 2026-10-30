//! Files dropped on the window (Qt's SessionFileDrop): the webview names the
//! dropped paths, and a drop is taken only when every one is a file, as Qt's
//! `LocalFile::readable` asks (a file, or a Windows shortcut, which is one).
//! A folder or a vanished path refuses the whole drop.

/// The most paths one drop may name; a larger drop is refused.
const MAX_PATHS: usize = 4096;

/// Whether every path names a file.
fn all_files(paths: &[String]) -> bool {
    !paths.is_empty()
        && paths.len() <= MAX_PATHS
        && paths
            .iter()
            .all(|path| std::fs::metadata(path).is_ok_and(|meta| meta.is_file()))
}

/// Whether a drop of `paths` may be taken, checked off the main thread.
#[tauri::command]
pub async fn dropped_files(paths: Vec<String>) -> bool {
    tauri::async_runtime::spawn_blocking(move || all_files(&paths))
        .await
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_files_make_a_drop() {
        let dir = std::env::temp_dir().join(format!("zerus-drops-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("notes.txt");
        std::fs::write(&file, b"notes").unwrap();
        let file = file.to_string_lossy().into_owned();
        let folder = dir.to_string_lossy().into_owned();
        assert!(all_files(std::slice::from_ref(&file)));
        assert!(
            !all_files(&[file.clone(), folder]),
            "a folder refuses the whole drop"
        );
        assert!(!all_files(&[dir
            .join("gone.txt")
            .to_string_lossy()
            .into_owned()]));
        assert!(!all_files(&[]));
        assert!(!all_files(&vec![file; MAX_PATHS + 1]));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
