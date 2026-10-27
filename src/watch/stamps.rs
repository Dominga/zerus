//! A cheap stamp over the files the live listing reads, compared on every
//! pulse. Bindings are replaced by rename, so a stamp of name, size, mtime and
//! inode sees every change; temporary `.hgs-*` files are ignored. Each session
//! binding has a stamp of its own, so a change re-derives only its row.
use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeMap;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// Session bindings: 64 lowercase hex digits and `.json`.
fn binding(name: &str) -> bool {
    name.strip_suffix(".json")
        .is_some_and(|stem| stem.len() == 64 && stem.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
}

fn ordinary(name: &str) -> bool {
    !name.starts_with(".hgs-") && name != ".lock" && !name.ends_with(".lock")
}

/// Root files other than bindings that change listed rows. Desktop presence
/// changes only `catalog.meta`, which the probe refreshes.
const ROOT_FILES: [&str; 2] = ["read-marks.json", "notifications.json"];
/// The folder that holds routed bindings next to the root's own.
const BINDINGS: &str = "bindings";
/// Folders whose every entry counts.
const FOLDERS: [&str; 6] = [BINDINGS, "name_routes", "renames", "launches", "dsh/bindings", "opencode/bindings"];
/// Single files and folders whose own stamp counts. Archive files are added or
/// removed, which changes the folder.
const PATHS: [&str; 4] = ["coordination/waits.sqlite3", "coordination/waits.sqlite3-wal", "coordination/waits.sqlite3-journal", "archives"];

fn entry(path: &Path, metadata: &fs::Metadata) -> u64 {
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
    hasher.finish()
}

/// One hash of these files' stamps; a missing file counts as missing. The git
/// topic stats a checkout's Git metadata with it on every pulse.
pub(super) fn files(paths: &[PathBuf]) -> u64 {
    let mut hasher = DefaultHasher::new();
    for path in paths {
        match fs::metadata(path) {
            Ok(metadata) => entry(path, &metadata).hash(&mut hasher),
            Err(_) => (path, u8::MAX).hash(&mut hasher),
        }
    }
    hasher.finish()
}

/// An order-independent hash of every stamped entry, and their count.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Stamp {
    hash: u64,
    entries: usize,
}

impl Stamp {
    fn add(&mut self, path: &Path, metadata: &fs::Metadata) {
        self.hash = self.hash.wrapping_add(entry(path, metadata));
        self.entries += 1;
    }

    #[cfg(test)]
    fn entries(&self) -> usize {
        self.entries
    }
}

/// The stamps of a state folder: one for every input but session bindings,
/// and one per binding file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Scan {
    pub(super) global: Stamp,
    pub(super) bindings: BTreeMap<PathBuf, u64>,
}

impl Scan {
    fn folder(&mut self, folder: &Path, keep: impl Fn(&str) -> bool, bindings: bool) {
        let Ok(entries) = fs::read_dir(folder) else { return };
        for found in entries.flatten() {
            let name = found.file_name();
            let Some(name) = name.to_str() else { continue };
            if !keep(name) {
                continue;
            }
            let Ok(metadata) = found.metadata() else { continue };
            let path = found.path();
            if bindings && binding(name) {
                self.bindings.insert(path.clone(), entry(&path, &metadata));
            } else {
                self.global.add(&path, &metadata);
            }
        }
    }

    /// Binding files whose stamp changed or that appeared, and whether any
    /// disappeared, since `before`.
    pub(super) fn changed_bindings(&self, before: &Scan) -> (Vec<PathBuf>, bool) {
        let changed = self
            .bindings
            .iter()
            .filter(|(path, stamp)| before.bindings.get(*path) != Some(stamp))
            .map(|(path, _)| path.clone())
            .collect();
        let removed = before.bindings.keys().any(|path| !self.bindings.contains_key(path));
        (changed, removed)
    }
}

/// The stamps of a state folder.
pub(super) fn scan(root: &Path) -> Scan {
    let mut scan = Scan::default();
    scan.folder(root, |name| binding(name) || ROOT_FILES.contains(&name), true);
    for folder in FOLDERS {
        scan.folder(&root.join(folder), ordinary, folder == BINDINGS);
    }
    for path in PATHS {
        let path = root.join(path);
        if let Ok(metadata) = fs::symlink_metadata(&path) {
            scan.global.add(&path, &metadata);
        }
    }
    scan
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
        // A binding has a stamp of its own; every other input stays unchanged.
        assert_eq!((written.global, written.bindings.len()), (first.global, 1));
        assert_eq!(written.changed_bindings(&first), (vec![record.clone()], false));
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
        assert_eq!(replaced.global, written.global);
        assert_eq!(replaced.changed_bindings(&written), (vec![record.clone()], false));
        // A routed binding is a binding too.
        let routed = root.join(format!("bindings/{}.json", "b".repeat(64)));
        fs::create_dir_all(routed.parent().unwrap()).unwrap();
        fs::write(&routed, "{}").unwrap();
        let both = scan(root);
        assert_eq!((both.global, both.changed_bindings(&replaced)), (replaced.global, (vec![routed.clone()], false)));
        fs::remove_file(&record).unwrap();
        assert_eq!(scan(root).changed_bindings(&both), (vec![], true));
        for path in ["read-marks.json", "launches/1.json", "renames/r.json", "name_routes/n.json", "dsh/bindings/b.json", "bindings/notes.txt"] {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            let before = scan(root);
            fs::write(&path, "{}").unwrap();
            let after = scan(root);
            assert_ne!(after.global, before.global, "{}", path.display());
            assert_eq!(after.bindings, before.bindings, "{}", path.display());
        }
        let before = scan(root);
        fs::create_dir(root.join("archives")).unwrap();
        assert_ne!(scan(root).global, before.global);
        assert_eq!(before.global.entries(), 6);
    }
}
