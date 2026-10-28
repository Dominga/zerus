//! The core's whole-file writes: the read marks ([`crate::readmarks`]) and
//! the organization's pending edits ([`crate::swarm::draft`]).
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Replaces `path` with `bytes` as one change. They go to a temporary file
/// beside it that belongs to this call alone (named after the process and a
/// counter), are flushed to disk, and the file is renamed over the target,
/// as Qt's `QSaveFile` commits its settings. A reader sees the old file or
/// the new one, never a part of either; a failure keeps the old file and
/// removes the temporary one. A missing folder is created. The file gets the
/// permissions a new file gets, as before this was shared.
pub(crate) fn write_private_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(format!(
        ".{}-{}.tmp",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let temporary = PathBuf::from(temporary);
    if let Some(folder) = path
        .parent()
        .filter(|folder| !folder.as_os_str().is_empty())
    {
        std::fs::create_dir_all(folder)?;
    }
    let written = std::fs::File::create(&temporary)
        .and_then(|mut file| {
            file.write_all(bytes)?;
            file.sync_all()
        })
        .and_then(|()| std::fs::rename(&temporary, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zerus-fsx-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn names(dir: &Path) -> Vec<std::ffi::OsString> {
        let mut names: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_write_replaces_the_file_whole_and_creates_its_folder() {
        let dir = scratch("replace");
        let path = dir.join("nested").join("state.json");
        write_private_atomic(&path, b"first").unwrap();
        write_private_atomic(&path, b"second").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        assert_eq!(
            names(&dir.join("nested")),
            ["state.json"],
            "no temporary file is left"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_rename_keeps_the_target_and_removes_the_temporary_file() {
        let dir = scratch("failed");
        // A folder where the file should go: the rename over it fails.
        let path = dir.join("state.json");
        std::fs::create_dir_all(path.join("kept")).unwrap();
        assert!(write_private_atomic(&path, b"bytes").is_err());
        assert!(path.join("kept").is_dir());
        assert_eq!(names(&dir), ["state.json"], "no temporary file is left");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
