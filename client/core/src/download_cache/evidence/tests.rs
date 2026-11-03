// G3 download-index evidence on synthetic folders under the system
// temporary folder, removed even when a test fails.
use super::super::maintenance::{Prepared, State};
use super::super::{complete, lookup, reserve, trim, usage, Source, Writer};
use super::*;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

const NOW: i64 = 1_800_000_000_000;
static NEXT: AtomicU64 = AtomicU64::new(0);
const ON: Writer = Writer { evidence: true };

struct Folder(PathBuf);
impl Folder {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "zerus-cache-evidence-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }
    /// A copy saved by a build that records no evidence (Qt, dev, older Next).
    fn plain(&self, name: &str, body: &[u8], age: i64) {
        let part = reserve(&self.0).unwrap();
        fs::write(part.path(), body).unwrap();
        complete(part, name, None, NOW - age).unwrap();
    }
    fn index(&self) -> Vec<u8> {
        fs::read(self.0.join(INDEX)).unwrap()
    }
    fn receipt(&self) -> Original {
        match evidence(&self.0) {
            Evidence::Captured(receipt) => *receipt,
            other => panic!("no receipt: {other:?}"),
        }
    }
    fn original(&self) -> Option<Vec<u8>> {
        let slots: Vec<_> = fs::read_dir(self.0.join(DIR)).unwrap().collect();
        assert_eq!(slots.len(), 1);
        Snapshot::open(&slots[0].as_ref().unwrap().path())
            .unwrap()
            .index
    }
}
impl Drop for Folder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn the_first_change_records_the_exact_index_it_replaces() {
    let folder = Folder::new();
    folder.plain("old.txt", b"from qt", 90_000);
    let before = folder.index();
    assert_eq!(evidence(&folder.0), Evidence::None);
    let part = reserve(&folder.0).unwrap();
    fs::write(part.path(), b"new").unwrap();
    ON.complete(part, "new.txt", None, NOW).unwrap();
    assert_ne!(folder.index(), before, "the download changed the index");
    let receipt = folder.receipt();
    assert_eq!(receipt.index, Index::Present);
    assert_eq!(receipt.bytes, before.len() as u64);
    assert_eq!(folder.original(), Some(before));
    // The evidence folder is one of Zerus's own names, never an index entry.
    assert!(!String::from_utf8(folder.index()).unwrap().contains(DIR));
}

#[test]
fn a_folder_without_an_index_is_recorded_absent() {
    let folder = Folder::new();
    let part = reserve(&folder.0).unwrap();
    fs::write(part.path(), b"first").unwrap();
    ON.complete(part, "first.txt", None, NOW).unwrap();
    assert_eq!(folder.receipt().index, Index::Absent);
    assert_eq!(folder.original(), None);
}

#[test]
fn a_committed_receipt_is_never_replaced_by_later_versions() {
    let folder = Folder::new();
    folder.plain("a.txt", b"a", 90_000);
    let original = folder.index();
    let part = reserve(&folder.0).unwrap();
    fs::write(part.path(), b"b").unwrap();
    ON.complete(part, "b.txt", None, NOW).unwrap();
    let receipt = folder.receipt();
    // Qt and Next keep changing the folder afterwards, unhindered.
    folder.plain("c.txt", b"c", 0);
    assert_eq!(ON.trim(&folder.0, 0, NOW).error, None);
    let part = reserve(&folder.0).unwrap();
    fs::write(part.path(), b"d").unwrap();
    ON.complete(part, "d.txt", None, NOW).unwrap();
    assert_eq!(folder.receipt(), receipt);
    assert_eq!(folder.original(), Some(original));
}

#[test]
fn a_load_without_a_change_writes_nothing_and_the_change_records_the_later_version() {
    let folder = Folder::new();
    folder.plain("a.txt", b"a", 90_000);
    // A lookup of an unknown source loads under the lock but changes nothing.
    let source = Source {
        host: "mac".into(),
        path: "/srv/example/none".into(),
    };
    assert_eq!(ON.lookup(&folder.0, &source, NOW).unwrap(), None);
    assert!(
        !folder.0.join(DIR).exists(),
        "a load alone writes no evidence"
    );
    // Qt writes again; Next's first change then replaces that version.
    folder.plain("qt.txt", b"qt", 90_000);
    let replaced = folder.index();
    ON.trim(&folder.0, 0, NOW);
    assert_eq!(folder.original(), Some(replaced));
}

#[test]
fn builds_without_evidence_never_write_it() {
    let folder = Folder::new();
    folder.plain("a.txt", b"a", 90_000);
    let source = Source {
        host: "mac".into(),
        path: "/srv/example/a".into(),
    };
    lookup(&folder.0, &source, NOW).unwrap();
    trim(&folder.0, 1, NOW);
    usage(&folder.0);
    assert!(!folder.0.join(DIR).exists());
}

#[test]
fn a_change_that_cannot_record_the_original_is_refused_before_any_effect() {
    let folder = Folder::new();
    let part = reserve(&folder.0).unwrap();
    fs::write(part.path(), b"old copy").unwrap();
    let old = Source {
        host: "mac".into(),
        path: "/srv/example/old".into(),
    };
    complete(part, "old.txt", Some(&old), NOW - 90_000).unwrap();
    let before = folder.index();
    // No slot can exist: the evidence name holds a plain file.
    fs::write(folder.0.join(DIR), b"").unwrap();
    let part = reserve(&folder.0).unwrap();
    let path = part.path().to_owned();
    fs::write(&path, b"new").unwrap();
    assert_eq!(
        ON.complete(part, "new.txt", None, NOW).unwrap_err(),
        NOT_RECORDED
    );
    assert!(!folder.0.join("new.txt").exists(), "no rename");
    assert!(!path.exists(), "the part file is removed");
    let usage = ON.trim(&folder.0, 1, NOW);
    assert_eq!(usage.error.as_deref(), Some(NOT_RECORDED));
    assert!(folder.0.join("old.txt").exists(), "no unlink");
    // A lookup of a source the index holds would renew it: refused first.
    let source = Source {
        host: "mac".into(),
        path: "/srv/example/old".into(),
    };
    assert_eq!(
        ON.lookup(&folder.0, &source, NOW).unwrap_err(),
        NOT_RECORDED
    );
    assert_eq!(folder.index(), before, "no index write");
    // Fixed, the next change captures: nothing was recorded as failed.
    fs::remove_file(folder.0.join(DIR)).unwrap();
    ON.trim(&folder.0, 0, NOW);
    assert_eq!(folder.original(), Some(before));
}

#[test]
fn a_copied_folder_never_inherits_the_slot() {
    let folder = Folder::new();
    folder.plain("a.txt", b"a", 90_000);
    ON.trim(&folder.0, 0, NOW);
    let first = folder.receipt();
    let copy = Folder::new();
    for name in [INDEX, "a.txt"] {
        fs::copy(folder.0.join(name), copy.0.join(name)).unwrap();
    }
    let slot = fs::read_dir(folder.0.join(DIR))
        .unwrap()
        .next()
        .unwrap()
        .unwrap();
    let copied = copy.0.join(DIR).join(slot.file_name());
    for dir in [copy.0.join(DIR), copied.clone()] {
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut fs::DirBuilder::new(), 0o700)
            .create(&dir)
            .unwrap();
        #[cfg(not(unix))]
        fs::create_dir(&dir).unwrap();
    }
    for leaf in fs::read_dir(slot.path()).unwrap() {
        let leaf = leaf.unwrap();
        fs::copy(leaf.path(), copied.join(leaf.file_name())).unwrap();
    }
    assert_eq!(
        evidence(&copy.0),
        Evidence::None,
        "another directory object"
    );
    ON.trim(&copy.0, 0, NOW);
    let mine = match evidence(&copy.0) {
        Evidence::Captured(receipt) => *receipt,
        other => panic!("{other:?}"),
    };
    assert_ne!(mine.capture_id, first.capture_id);
    assert_eq!(fs::read_dir(copy.0.join(DIR)).unwrap().count(), 2);
}

#[test]
fn service_preparation_writes_nothing_and_execute_records_before_its_first_unlink() {
    let folder = Folder::new();
    folder.plain("old.txt", b"old cache", 90_000);
    let before = folder.index();
    let prepared = Prepared::open(&folder.0, 1, NOW, &AtomicBool::new(false), true).unwrap();
    assert!(!folder.0.join(DIR).exists(), "preparation stays read-only");
    let receipt = prepared.execute(&[true], &AtomicBool::new(false));
    assert_eq!(receipt.files, vec![State::Removed]);
    assert_eq!(folder.original(), Some(before));
}

#[test]
fn service_execute_keeps_every_file_when_the_original_cannot_be_recorded() {
    let folder = Folder::new();
    folder.plain("old.txt", b"old cache", 90_000);
    let before = folder.index();
    let prepared = Prepared::open(&folder.0, 1, NOW, &AtomicBool::new(false), true).unwrap();
    fs::write(folder.0.join(DIR), b"").unwrap();
    let receipt = prepared.execute(&[true], &AtomicBool::new(false));
    assert_eq!(receipt.files, vec![State::Kept]);
    assert!(folder.0.join("old.txt").exists());
    assert_eq!(folder.index(), before);
}

#[test]
fn service_preparation_without_evidence_never_records() {
    let folder = Folder::new();
    folder.plain("old.txt", b"old cache", 90_000);
    let prepared = Prepared::open(&folder.0, 1, NOW, &AtomicBool::new(false), false).unwrap();
    assert_eq!(
        prepared.execute(&[true], &AtomicBool::new(false)).files,
        vec![State::Removed]
    );
    assert!(!folder.0.join(DIR).exists());
}
