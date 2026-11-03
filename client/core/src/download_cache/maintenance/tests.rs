use super::*;
use std::sync::atomic::AtomicU64;

const NOW: i64 = 1_800_000_000_000;
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Folder(PathBuf);
impl Folder {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "zerus-cache-owner-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }
    fn save(&self, name: &str, body: &[u8], age: i64) {
        let part = reserve(&self.0).unwrap();
        fs::write(part.path(), body).unwrap();
        assert!(complete(part, name, None, NOW - age)
            .unwrap()
            .warning
            .is_none());
    }
    fn prepare(&self, limit: u64) -> Prepared {
        Prepared::open(&self.0, limit, NOW, &AtomicBool::new(false)).unwrap()
    }
    fn index(&self) -> Value {
        serde_json::from_slice(&fs::read(self.0.join(INDEX)).unwrap()).unwrap()
    }
}
impl Drop for Folder {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o700));
        }
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn preparation_is_read_only_and_holds_the_real_qt_exclusion() {
    let folder = Folder::new();
    folder.save("old.txt", b"old cache", 90_000);
    let original = fs::read(folder.0.join(INDEX)).unwrap();
    let prepared = folder.prepare(1);
    assert!(prepared.binding().valid());
    assert_eq!(prepared.binding().candidates.len(), 1);
    assert_eq!(fs::read(folder.0.join(INDEX)).unwrap(), original);
    assert_eq!(fs::read(folder.0.join("old.txt")).unwrap(), b"old cache");
    assert!(qt_lock::lock(&folder.0.join(LOCK), Duration::ZERO)
        .unwrap()
        .is_none());
    drop(prepared);
    assert!(qt_lock::lock(&folder.0.join(LOCK), Duration::ZERO)
        .unwrap()
        .is_some());
}

#[test]
fn a_reserved_pass_removes_only_matching_old_copies_and_preserves_opaque_index_fields() {
    let folder = Folder::new();
    folder.save("old.txt", b"old cache", 90_000);
    folder.save("recent.txt", b"new", 1000);
    fs::write(folder.0.join("unknown.txt"), b"user content").unwrap();
    let mut index = folder.index();
    index["future"] = json!({"original":"header"});
    index["files"][1]["future"] = json!({"original":"entry"});
    fs::write(folder.0.join(INDEX), serde_json::to_vec(&index).unwrap()).unwrap();
    let prepared = folder.prepare(4);
    let receipt = prepared.execute(&[true], &AtomicBool::new(false));
    assert_eq!(receipt.files, [State::Removed]);
    assert!(!folder.0.join("old.txt").exists());
    assert_eq!(fs::read(folder.0.join("recent.txt")).unwrap(), b"new");
    assert_eq!(
        fs::read(folder.0.join("unknown.txt")).unwrap(),
        b"user content"
    );
    let kept = folder.index();
    assert_eq!(kept["future"], index["future"]);
    assert_eq!(kept["files"], json!([index["files"][1]]));
}

#[test]
fn a_same_size_edit_after_freeze_is_preserved_with_its_original_index_entry() {
    let folder = Folder::new();
    folder.save("edited.txt", b"old", 90_000);
    let prepared = folder.prepare(1);
    let original_index = fs::read(folder.0.join(INDEX)).unwrap();
    fs::write(folder.0.join("edited.txt"), b"NEW").unwrap();
    let receipt = prepared.execute(&[true], &AtomicBool::new(false));
    assert_eq!(receipt.files, [State::Kept]);
    assert_eq!(fs::read(folder.0.join("edited.txt")).unwrap(), b"NEW");
    assert_eq!(fs::read(folder.0.join(INDEX)).unwrap(), original_index);
}

#[test]
fn a_changed_index_or_cancelled_or_expired_pass_never_unlinks() {
    for cause in ["index", "cancelled", "expired"] {
        let folder = Folder::new();
        folder.save("keep.txt", b"content", 90_000);
        let mut prepared = folder.prepare(1);
        let cancel = AtomicBool::new(cause == "cancelled");
        if cause == "index" {
            fs::write(folder.0.join(INDEX), b"{\"future\":2}").unwrap();
        }
        if cause == "expired" {
            prepared.at = Instant::now() - DEADLINE;
        }
        assert_eq!(prepared.execute(&[true], &cancel).files, [State::Kept]);
        assert_eq!(fs::read(folder.0.join("keep.txt")).unwrap(), b"content");
    }
}

#[test]
fn a_prior_unknown_content_fence_survives_unrelated_index_edits() {
    let folder = Folder::new();
    folder.save("keep.txt", b"content", 90_000);
    let original = folder.prepare(1).binding().clone();
    let mut index = folder.index();
    index["future"] = json!("a different generation");
    fs::write(folder.0.join(INDEX), serde_json::to_vec(&index).unwrap()).unwrap();
    let prepared = folder.prepare(1);
    assert_ne!(prepared.binding().index_sha256, original.index_sha256);
    assert_eq!(
        prepared.binding().candidates[0].content_id,
        original.candidates[0].content_id
    );
    assert_eq!(
        prepared.execute(&[false], &AtomicBool::new(false)).files,
        [State::Kept]
    );
    assert!(folder.0.join("keep.txt").is_file());
}

#[test]
fn content_and_candidate_bounds_preserve_excess_files() {
    let folder = Folder::new();
    for i in 0..CANDIDATES + 2 {
        folder.save(&format!("entry-{i}"), b"content", 90_000);
    }
    let prepared = folder.prepare(1);
    assert_eq!(prepared.binding().candidates.len(), CANDIDATES);
    let receipt = prepared.execute(&[true; CANDIDATES], &AtomicBool::new(false));
    assert_eq!(
        receipt
            .files
            .iter()
            .filter(|s| **s == State::Removed)
            .count(),
        CANDIDATES
    );
    assert_eq!(folder.index()["files"].as_array().unwrap().len(), 2);
    // A sparse file larger than the complete hash budget is not read or removed.
    let big = Folder::new();
    big.save("large", b"x", 90_000);
    OpenOptions::new()
        .write(true)
        .open(big.0.join("large"))
        .unwrap()
        .set_len(CONTENT_BYTES + 1)
        .unwrap();
    let mut index = big.index();
    index["files"][0]["bytes"] = json!(CONTENT_BYTES + 1);
    fs::write(big.0.join(INDEX), serde_json::to_vec(&index).unwrap()).unwrap();
    assert_eq!(
        big.prepare(1)
            .execute(&[true], &AtomicBool::new(false))
            .files,
        [State::Kept]
    );
    assert_eq!(
        fs::metadata(big.0.join("large")).unwrap().len(),
        CONTENT_BYTES + 1
    );
}

#[test]
fn a_future_or_oversized_index_is_never_rewritten() {
    for bytes in [
        b"{\"version\":2,\"files\":[]}".to_vec(),
        vec![b' '; INDEX_BYTES as usize + 1],
    ] {
        let folder = Folder::new();
        fs::write(folder.0.join(INDEX), &bytes).unwrap();
        assert!(Prepared::open(&folder.0, 1, NOW, &AtomicBool::new(false)).is_err());
        assert_eq!(fs::read(folder.0.join(INDEX)).unwrap(), bytes);
    }
}

#[cfg(unix)]
#[test]
fn links_and_replaced_directories_do_not_inherit_the_frozen_plan() {
    use std::os::unix::fs::symlink;
    let folder = Folder::new();
    folder.save("old.txt", b"content", 90_000);
    let prepared = folder.prepare(1);
    let replaced = folder.0.with_extension("retained");
    fs::rename(&folder.0, &replaced).unwrap();
    fs::create_dir(&folder.0).unwrap();
    fs::write(folder.0.join("old.txt"), b"new directory").unwrap();
    assert_eq!(
        prepared.execute(&[true], &AtomicBool::new(false)).files,
        [State::Kept]
    );
    assert_eq!(
        fs::read(folder.0.join("old.txt")).unwrap(),
        b"new directory"
    );
    fs::remove_dir_all(&folder.0).unwrap();
    fs::rename(replaced, &folder.0).unwrap();
    // Qt deliberately does not guess a renamed lock's new path on Drop.
    // Use a separate cache for the file-link controls.
    let folder = Folder::new();
    folder.save("old.txt", b"content", 90_000);
    fs::hard_link(folder.0.join("old.txt"), folder.0.join("hard-link")).unwrap();
    assert!(folder.prepare(1).binding().candidates.is_empty());
    fs::remove_file(folder.0.join("hard-link")).unwrap();
    let prepared = folder.prepare(1);
    fs::rename(folder.0.join("old.txt"), folder.0.join("retained.txt")).unwrap();
    symlink(folder.0.join("retained.txt"), folder.0.join("old.txt")).unwrap();
    assert_eq!(
        prepared.execute(&[true], &AtomicBool::new(false)).files,
        [State::Kept]
    );
    assert_eq!(fs::read(folder.0.join("retained.txt")).unwrap(), b"content");
}

#[cfg(unix)]
#[test]
fn real_unlink_denial_is_unknown_and_not_a_verified_refusal() {
    use std::os::unix::fs::PermissionsExt;
    if unsafe { libc::geteuid() } == 0 {
        return;
    } // root bypasses this real permission cut
    let folder = Folder::new();
    folder.save("keep.txt", b"content", 90_000);
    let prepared = folder.prepare(1);
    fs::set_permissions(&folder.0, fs::Permissions::from_mode(0o500)).unwrap();
    let result = prepared.execute(&[true], &AtomicBool::new(false));
    fs::set_permissions(&folder.0, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(result.files, [State::Unknown]);
    assert_eq!(fs::read(folder.0.join("keep.txt")).unwrap(), b"content");
}

#[test]
fn renaming_the_same_unknown_original_cannot_change_its_content_fence() {
    let folder = Folder::new();
    folder.save("old.txt", b"content", 90_000);
    let original = folder.prepare(1).binding().clone();
    fs::rename(folder.0.join("old.txt"), folder.0.join("moved.txt")).unwrap();
    let mut index = folder.index();
    index["files"][0]["name"] = json!("moved.txt");
    fs::write(folder.0.join(INDEX), serde_json::to_vec(&index).unwrap()).unwrap();
    let prepared = folder.prepare(1);
    assert_eq!(
        prepared.binding().candidates[0].content_id,
        original.candidates[0].content_id
    );
    assert_eq!(
        prepared.execute(&[false], &AtomicBool::new(false)).files,
        [State::Kept]
    );
    assert!(folder.0.join("moved.txt").is_file());
}

#[test]
fn byte_equal_index_replacement_is_a_different_frozen_generation() {
    let folder = Folder::new();
    folder.save("keep.txt", b"content", 90_000);
    let original = folder.prepare(1).binding().clone();
    let raw = fs::read(folder.0.join(INDEX)).unwrap();
    fs::write(folder.0.join("replacement"), raw).unwrap();
    fs::rename(folder.0.join("replacement"), folder.0.join(INDEX)).unwrap();
    let next = folder.prepare(1);
    assert_eq!(next.binding().index_sha256, original.index_sha256);
    assert_ne!(next.binding().index_identity, original.index_identity);
}
