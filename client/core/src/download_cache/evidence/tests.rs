// G3 download-index evidence on synthetic folders under the system
// temporary folder, removed even when a test fails.
use super::super::maintenance::{Prepared, State};
use super::super::{complete, lookup, reserve, trim, usage, Source, Writer};
use super::*;
use serde_json::{json, Value};
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
        let (canonical, object, birth) = folder(&self.0).unwrap();
        let slot = slot(&canonical, &object, birth.as_deref());
        Snapshot::open(&slot, &object, birth.as_deref())
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

/// fixes-u P1 on #1237: the commit may be slow, so the worker's final
/// checks follow it. A replacement or a cancellation during the commit
/// keeps every file.
fn during_commit(hook: impl FnMut() + 'static) {
    DURING_COMMIT.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
}

#[test]
fn service_execute_never_unlinks_a_file_replaced_during_the_commit() {
    let folder = Folder::new();
    folder.plain("old.txt", b"old cache", 90_000);
    let prepared = Prepared::open(&folder.0, 1, NOW, &AtomicBool::new(false), true).unwrap();
    let root = folder.0.clone();
    during_commit(move || {
        fs::rename(root.join("old.txt"), root.join("moved.txt")).unwrap();
        fs::write(root.join("old.txt"), b"replacement").unwrap();
    });
    let receipt = prepared.execute(&[true], &AtomicBool::new(false));
    during_commit(|| {});
    assert_eq!(receipt.files, vec![State::Kept]);
    assert_eq!(fs::read(folder.0.join("old.txt")).unwrap(), b"replacement");
    assert!(folder.0.join("moved.txt").exists());
}

#[test]
fn service_execute_never_unlinks_after_a_cancellation_during_the_commit() {
    let folder = Folder::new();
    folder.plain("old.txt", b"old cache", 90_000);
    let prepared = Prepared::open(&folder.0, 1, NOW, &AtomicBool::new(false), true).unwrap();
    let cancel = std::sync::Arc::new(AtomicBool::new(false));
    let set = cancel.clone();
    during_commit(move || set.store(true, Ordering::Release));
    let receipt = prepared.execute(&[true], &cancel);
    during_commit(|| {});
    assert_eq!(receipt.files, vec![State::Kept]);
    assert_eq!(fs::read(folder.0.join("old.txt")).unwrap(), b"old cache");
}

/// fixes-u P2 on #1237: a committed receipt counts only if it is exactly a
/// receipt for its slot. A corrupt one with the same capture ID refuses
/// every later change and is never overwritten.
#[test]
fn a_corrupt_receipt_with_the_same_id_refuses_changes_and_is_kept() {
    let corruptions: [fn(&mut Value); 5] = [
        |r| r["format"] = json!("other-format"),
        |r| r["boundary"] = json!("other-boundary"),
        |r| r["directory"] = json!("0".repeat(64)),
        |r| r["version"] = json!(2),
        |r| r["index"] = json!("absent"),
    ];
    for corrupt in corruptions {
        let folder = Folder::new();
        folder.plain("a.txt", b"a", 90_000);
        assert_eq!(ON.trim(&folder.0, 0, NOW).error, None);
        let slot = fs::read_dir(folder.0.join(DIR))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let mut receipt: Value =
            serde_json::from_slice(&fs::read(slot.join("receipt.json")).unwrap()).unwrap();
        corrupt(&mut receipt);
        let bytes = serde_json::to_vec(&receipt).unwrap();
        fs::write(slot.join("receipt.json"), &bytes).unwrap();
        let before = folder.index();
        folder.plain("b.txt", b"b", 90_000);
        let qt = folder.index();
        let usage = ON.trim(&folder.0, 1, NOW);
        assert_eq!(usage.error.as_deref(), Some(NOT_RECORDED), "{receipt}");
        assert_eq!(folder.index(), qt, "no change after a corrupt receipt");
        assert_ne!(qt, before);
        assert_eq!(
            fs::read(slot.join("receipt.json")).unwrap(),
            bytes,
            "kept as is"
        );
        assert_eq!(evidence(&folder.0), Evidence::Unreadable);
    }
}

#[test]
fn a_receipt_whose_original_is_missing_or_resized_refuses_changes() {
    for damage in [0, 1] {
        let folder = Folder::new();
        folder.plain("a.txt", b"a", 90_000);
        assert_eq!(ON.trim(&folder.0, 0, NOW).error, None);
        let slot = fs::read_dir(folder.0.join(DIR))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        if damage == 0 {
            fs::remove_file(slot.join("original.bin")).unwrap();
        } else {
            fs::write(slot.join("original.bin"), b"{}").unwrap();
        }
        assert_eq!(
            ON.trim(&folder.0, 0, NOW).error.as_deref(),
            Some(NOT_RECORDED)
        );
    }
}

fn private_dir(path: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut fs::DirBuilder::new(), 0o700)
        .create(path)
        .unwrap();
    #[cfg(not(unix))]
    fs::create_dir(path).unwrap();
}

/// review-n P2 on #1237: the folder hash only locates a slot. A slot moved
/// under another folder's key is never that folder's evidence.
#[test]
fn a_slot_moved_under_another_folders_key_is_never_reported_captured() {
    let a = Folder::new();
    a.plain("a.txt", b"a", 90_000);
    assert_eq!(ON.trim(&a.0, 0, NOW).error, None);
    let (ca, oa, ba) = folder(&a.0).unwrap();
    let from = slot(&ca, &oa, ba.as_deref());
    let b = Folder::new();
    b.plain("b.txt", b"b", 90_000);
    let (cb, ob, bb) = folder(&b.0).unwrap();
    let to = slot(&cb, &ob, bb.as_deref());
    // Moved whole, so original.bin keeps its object and stamp.
    private_dir(&b.0.join(DIR));
    fs::rename(&from, &to).unwrap();
    assert_eq!(evidence(&b.0), Evidence::Unreadable);
    assert_eq!(evidence(&a.0), Evidence::None, "A's slot is gone");
    let before = b.index();
    assert_eq!(ON.trim(&b.0, 0, NOW).error.as_deref(), Some(NOT_RECORDED));
    assert_eq!(b.index(), before);
}

#[test]
fn seeds_of_another_version_boundary_or_id_are_never_reported_captured() {
    let corruptions: [fn(&mut Value, &mut Value); 3] = [
        |seed, _| seed["version"] = json!(2),
        |seed, _| seed["boundary"] = json!("other-boundary"),
        |seed, receipt| {
            seed["capture_id"] = json!("not-a-uuid");
            receipt["capture_id"] = json!("not-a-uuid");
        },
    ];
    for corrupt in corruptions {
        let folder = Folder::new();
        folder.plain("a.txt", b"a", 90_000);
        assert_eq!(ON.trim(&folder.0, 0, NOW).error, None);
        let (canonical, object, birth) = super::folder(&folder.0).unwrap();
        let slot = slot(&canonical, &object, birth.as_deref());
        let read = |name: &str| -> Value {
            serde_json::from_slice(&fs::read(slot.join(name)).unwrap()).unwrap()
        };
        let (mut seed, mut receipt) = (read("identity.json"), read("receipt.json"));
        corrupt(&mut seed, &mut receipt);
        fs::write(
            slot.join("identity.json"),
            serde_json::to_vec(&seed).unwrap(),
        )
        .unwrap();
        fs::write(
            slot.join("receipt.json"),
            serde_json::to_vec(&receipt).unwrap(),
        )
        .unwrap();
        assert_eq!(evidence(&folder.0), Evidence::Unreadable, "{seed}");
        assert_eq!(
            ON.trim(&folder.0, 0, NOW).error.as_deref(),
            Some(NOT_RECORDED)
        );
    }
}

/// review-n P2 on #1237: raw entries of any kind count toward the slot cap,
/// so unrelated files can never hide existing slots.
#[test]
fn raw_entries_never_hide_the_slot_cap() {
    let folder = Folder::new();
    folder.plain("a.txt", b"a", 90_000);
    private_dir(&folder.0.join(DIR));
    for n in 0..MAX_SLOTS {
        private_dir(&folder.0.join(DIR).join(format!("{n:064x}")));
    }
    for n in 0..64 {
        fs::write(folder.0.join(DIR).join(format!("junk-{n}")), b"").unwrap();
    }
    assert_eq!(
        ON.trim(&folder.0, 0, NOW).error.as_deref(),
        Some(NOT_RECORDED)
    );
    let dirs = fs::read_dir(folder.0.join(DIR))
        .unwrap()
        .filter(|e| e.as_ref().unwrap().path().is_dir())
        .count();
    assert_eq!(dirs, MAX_SLOTS, "no 17th slot");
    // Below the cap a new slot is admitted.
    let other = Folder::new();
    other.plain("a.txt", b"a", 90_000);
    private_dir(&other.0.join(DIR));
    for n in 0..MAX_SLOTS - 1 {
        private_dir(&other.0.join(DIR).join(format!("{n:064x}")));
    }
    assert_eq!(ON.trim(&other.0, 0, NOW).error, None);
}

/// The Windows gate classification, as portable controls.
#[test]
fn only_verified_unsupported_locations_are_not_capturable() {
    assert_eq!(classify(Volume::Drive, Some(2)), Place::Local);
    assert_eq!(classify(Volume::Drive, Some(3)), Place::Local);
    for remote in [4, 5, 6] {
        assert_eq!(classify(Volume::Drive, Some(remote)), Place::Unsupported);
    }
    assert_eq!(classify(Volume::Share, None), Place::Unsupported);
    for unknown in [Some(0), Some(1), Some(7), None] {
        assert_eq!(classify(Volume::Drive, unknown), Place::Unknown);
    }
    assert_eq!(classify(Volume::Other, None), Place::Unknown);
}

#[test]
fn an_unsupported_location_records_nothing_and_an_unknown_one_refuses() {
    let folder = Folder::new();
    folder.plain("a.txt", b"a", 90_000);
    let index = folder.index();
    let mut pending = prepare_at(&folder.0, Some(&index), Place::Unsupported).unwrap();
    assert_eq!(pending.commit(), Ok(()));
    assert!(
        !folder.0.join(DIR).exists(),
        "never captured, nothing written"
    );
    assert_eq!(
        prepare_at(&folder.0, Some(&index), Place::Unknown)
            .err()
            .as_deref(),
        Some(NOT_RECORDED)
    );
}

/// review-n P2 on #1237 R4: a committed original counts only with each
/// part's recorded stamp, by a private no-follow stat (no hashing). Damage
/// that keeps the length, a replacement and a second link all refuse, and
/// no cached copy is removed.
#[test]
fn a_damaged_replaced_or_linked_original_refuses_and_unlinks_nothing() {
    for damage in ["in place", "replaced", "linked"] {
        let folder = Folder::new();
        folder.plain("first.txt", b"first", 90_000);
        assert_eq!(ON.trim(&folder.0, 0, NOW).error, None);
        let (canonical, object, birth) = super::folder(&folder.0).unwrap();
        let slot = slot(&canonical, &object, birth.as_deref());
        let original = slot.join("original.bin");
        let length = fs::metadata(&original).unwrap().len() as usize;
        std::thread::sleep(std::time::Duration::from_millis(20));
        match damage {
            "in place" => fs::write(&original, vec![b' '; length]).unwrap(),
            "replaced" => {
                let other = slot.join("other.bin");
                fs::copy(&original, &other).unwrap();
                fs::rename(&other, &original).unwrap();
            }
            _ => fs::hard_link(&original, slot.join("second-link")).unwrap(),
        }
        // Nine cached bytes over a one-byte limit: a trim would remove them.
        folder.plain("old.txt", b"old cache", 90_000);
        let usage = ON.trim(&folder.0, 1, NOW);
        assert_eq!(usage.error.as_deref(), Some(NOT_RECORDED), "{damage}");
        assert_eq!(
            fs::read(folder.0.join("old.txt")).unwrap(),
            b"old cache",
            "{damage}"
        );
        assert_eq!(evidence(&folder.0), Evidence::Unreadable, "{damage}");
    }
}

/// review-n on #1237 R4: an original over 16 MiB (the Windows reader's
/// largest read) is kept in parts, each created and read with its size.
#[test]
fn an_original_over_one_part_is_kept_in_exact_parts() {
    let folder = Folder::new();
    let files: Vec<Value> = (0..200_000)
        .map(|n| json!({"name": format!("f{n:06}.txt"), "bytes": 1, "saved": 0, "sha256": "0".repeat(64)}))
        .collect();
    let index = serde_json::to_vec(&json!({"version": 1, "files": files})).unwrap();
    assert!(
        index.len() > PART && index.len() <= ORIGINAL,
        "{}",
        index.len()
    );
    fs::write(folder.0.join(INDEX), &index).unwrap();
    assert_eq!(ON.trim(&folder.0, 0, NOW).error, None);
    let receipt = folder.receipt();
    assert_eq!(receipt.bytes, index.len() as u64);
    assert_eq!(receipt.more.len(), 1);
    let (canonical, object, birth) = super::folder(&folder.0).unwrap();
    let slot = slot(&canonical, &object, birth.as_deref());
    assert_eq!(
        fs::metadata(slot.join("original.bin")).unwrap().len() as usize,
        PART
    );
    assert_eq!(folder.original(), Some(index));
}
