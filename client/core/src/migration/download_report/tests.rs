// The download folder's committed original as a report source, on
// synthetic folders under the system temporary folder.
use super::*;
use crate::download_cache::{complete, reserve, Writer};
use crate::migration::creation_report::{Group, Record, Source};
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
const ON: Writer = Writer { evidence: true };

struct Folder(PathBuf);
impl Folder {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "zerus-download-report-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }
    fn plain(&self, name: &str, body: &[u8]) {
        let part = reserve(&self.0).unwrap();
        fs::write(part.path(), body).unwrap();
        complete(part, name, None, 1).unwrap();
    }
    fn index(&self) -> Vec<u8> {
        fs::read(self.0.join(".zerus-files.json")).unwrap()
    }
}
impl Drop for Folder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn the_committed_original_is_the_source_and_the_live_index_is_not() {
    let folder = Folder::new();
    folder.plain("a.txt", b"a");
    let original = folder.index();
    assert!(retained(&folder.0).unwrap().is_none(), "no receipt yet");
    assert_eq!(ON.trim(&folder.0, 0, 2).error, None);
    folder.plain("b.txt", b"b");
    let snapshot = retained(&folder.0).unwrap().unwrap();
    assert_eq!(snapshot.index, original, "never today's live index");
    let binding = snapshot.reference().unwrap();
    assert!(binding.valid());
    assert_eq!(binding.index_sha256, sha256(&original));
    let sources = snapshot.report_sources().unwrap();
    assert_eq!(sources.len(), 1);
}

#[test]
fn a_changed_receipt_is_another_source() {
    let folder = Folder::new();
    folder.plain("a.txt", b"a");
    assert_eq!(ON.trim(&folder.0, 0, 2).error, None);
    let snapshot = retained(&folder.0).unwrap().unwrap();
    // The folder is replaced by another object at the same path.
    let moved = folder.0.with_extension("moved");
    fs::rename(&folder.0, &moved).unwrap();
    fs::create_dir(&folder.0).unwrap();
    assert_eq!(snapshot.reference(), Err(SourceError::SourceChanged));
    let _ = fs::remove_dir_all(&moved);
}

#[test]
fn a_folder_without_an_original_index_publishes_nothing() {
    let folder = Folder::new();
    let part = reserve(&folder.0).unwrap();
    fs::write(part.path(), b"first").unwrap();
    ON.complete(part, "first.txt", None, 1).unwrap();
    assert!(retained(&folder.0).unwrap().is_none());
    let missing = folder.0.join("never-created");
    assert!(retained(&missing).unwrap().is_none());
}

#[test]
fn download_records_keep_their_own_scope_and_preload_bytes_are_unchanged() {
    let folder = Folder::new();
    folder.plain("a.txt", b"a");
    assert_eq!(ON.trim(&folder.0, 0, 2).error, None);
    let binding = retained(&folder.0).unwrap().unwrap().reference().unwrap();
    let configuration = "c".repeat(64);
    let record =
        Record::new_group(Group::Downloads, configuration.clone(), binding.clone(), 1).unwrap();
    assert_eq!(record.import_id, binding.capture_id);
    // Round trip through its serialized form keeps the download binding.
    let bytes = serde_json::to_vec(&record).unwrap();
    let again: Record = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(again.source, Source::Downloads(binding.clone()));
    // A download binding never fits another group's scope.
    assert!(Record::new_group(Group::Creations, configuration, binding, 1).is_err());
}
