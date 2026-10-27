//! A cheap stamp over the files the live listing reads, compared on every
//! pulse. Bindings are replaced by rename, so a stamp of name, size, mtime and
//! inode sees every change; temporary `.hgs-*` files are ignored.
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;

/// Session bindings: 64 lowercase hex digits and `.json`.
fn binding(name: &str) -> bool {
    name.strip_suffix(".json")
        .is_some_and(|stem| stem.len() == 64 && stem.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
}

fn ordinary(name: &str) -> bool {
    !name.starts_with(".hgs-") && name != ".lock" && !name.ends_with(".lock")
}

/// Root files other than bindings that change listed rows. Desktop presence
/// changes only `catalog.meta`, which the safety listing refreshes.
const ROOT_FILES: [&str; 2] = ["read-marks.json", "notifications.json"];
/// Folders whose every entry counts.
const FOLDERS: [&str; 6] = ["bindings", "name_routes", "renames", "launches", "dsh/bindings", "opencode/bindings"];
/// Single files and folders whose own stamp counts. Archive files are added or
/// removed, which changes the folder.
const PATHS: [&str; 4] = ["coordination/waits.sqlite3", "coordination/waits.sqlite3-wal", "coordination/waits.sqlite3-journal", "archives"];

/// An order-independent hash of every stamped entry, and their count.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Stamp {
    hash: u64,
    entries: usize,
}

impl Stamp {
    fn add(&mut self, path: &Path, metadata: &fs::Metadata) {
        let mut hasher = DefaultHasher::new();
        path.hash(&mut hasher);
        metadata.len().hash(&mut hasher);
        metadata.modified().ok().hash(&mut hasher);
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            metadata.ino().hash(&mut hasher);
            metadata.ctime().hash(&mut hasher);
            metadata.ctime_nsec().hash(&mut hasher);
        }
        self.hash = self.hash.wrapping_add(hasher.finish());
        self.entries += 1;
    }

    fn folder(&mut self, folder: &Path, keep: impl Fn(&str) -> bool) {
        let Ok(entries) = fs::read_dir(folder) else { return };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if !keep(name) {
                continue;
            }
            if let Ok(metadata) = entry.metadata() {
                self.add(&entry.path(), &metadata);
            }
        }
    }

    #[cfg(test)]
    fn entries(&self) -> usize {
        self.entries
    }
}

/// The stamp of a state folder.
pub(super) fn scan(root: &Path) -> Stamp {
    let mut stamp = Stamp::default();
    stamp.folder(root, |name| binding(name) || ROOT_FILES.contains(&name));
    for folder in FOLDERS {
        stamp.folder(&root.join(folder), ordinary);
    }
    for path in PATHS {
        let path = root.join(path);
        if let Ok(metadata) = fs::symlink_metadata(&path) {
            stamp.add(&path, &metadata);
        }
    }
    stamp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bindings_and_inputs_change_the_stamp_and_temporary_files_do_not() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let first = scan(root);
        let record = root.join(format!("{}.json", "a".repeat(64)));
        fs::write(&record, "{}").unwrap();
        let written = scan(root);
        assert_ne!(written, first);
        assert_eq!(written.entries(), 1);
        // Ignored: temporary files, locks, presence and unrelated files.
        fs::write(root.join(".hgs-temp"), "x").unwrap();
        fs::write(root.join("desktop-presence.json"), "{}").unwrap();
        fs::write(root.join(".lock"), "").unwrap();
        fs::write(root.join("notes.json"), "{}").unwrap();
        fs::create_dir_all(root.join("launches")).unwrap();
        fs::write(root.join("launches/.hgs-partial"), "x").unwrap();
        assert_eq!(scan(root), written);
        // An atomic replacement (new inode, same size) is seen.
        let temp = root.join(".hgs-next");
        fs::write(&temp, "[]").unwrap();
        fs::rename(&temp, &record).unwrap();
        let replaced = scan(root);
        assert_ne!(replaced, written);
        for path in ["read-marks.json", "launches/1.json", "renames/r.json", "name_routes/n.json", "dsh/bindings/b.json"] {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            let before = scan(root);
            fs::write(&path, "{}").unwrap();
            assert_ne!(scan(root), before, "{}", path.display());
        }
        let before = scan(root);
        fs::create_dir(root.join("archives")).unwrap();
        assert_ne!(scan(root), before);
    }
}
