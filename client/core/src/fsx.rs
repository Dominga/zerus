//! The core's whole-file writes: the read marks ([`crate::readmarks`]) and
//! the organization's pending edits ([`crate::swarm::draft`]).
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Creates `path` for writing, readable and writable by this user only on
/// Unix (mode 0600 from its creation). A stale file of that name, left by an
/// earlier process with the same ID, goes first, so its mode is never kept.
fn private_file(path: &Path) -> io::Result<File> {
    let _ = std::fs::remove_file(path);
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)
}

/// Replaces `path` with `bytes` as one change. They go to a temporary file
/// beside it that belongs to this call alone (named after the process and a
/// counter) and that only this user may read (0600 on Unix: read marks and
/// drafts hold session and project names), and the file is renamed over the
/// target. A reader sees the old file or the new one, never a part of
/// either; a failure keeps the old file and removes the temporary one. A
/// missing folder is created. `durable` flushes the temporary file to disk
/// before the rename, as Qt's `QSaveFile` commits its settings; the read
/// marks skip it, since they are saved under their lock on the batch path.
pub(crate) fn write_private_atomic(path: &Path, bytes: &[u8], durable: bool) -> io::Result<()> {
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
    let written = private_file(&temporary)
        .and_then(|mut file| {
            file.write_all(bytes)?;
            if durable {
                file.sync_all()?;
            }
            Ok(())
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
        write_private_atomic(&path, b"first", true).unwrap();
        write_private_atomic(&path, b"second", false).unwrap();
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
        assert!(write_private_atomic(&path, b"bytes", true).is_err());
        assert!(path.join("kept").is_dir());
        assert_eq!(names(&dir), ["state.json"], "no temporary file is left");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Controller ruling, fix round 1 (2026-10-28): read marks and drafts hold
    // session and project names, so only this user may read them.
    #[cfg(unix)]
    #[test]
    fn the_file_is_this_user_s_alone_also_when_it_replaces_a_readable_one() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("private");
        let path = dir.join("state.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, b"old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        for durable in [true, false] {
            write_private_atomic(&path, b"new", durable).unwrap();
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "durable {durable}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
