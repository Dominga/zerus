use super::*;
use crate::{Actor, NewTask, Stage, Target, TaskDoc};

fn fixture() -> (tempfile::TempDir, ProjectStore, String) {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::open(root.path(), "example").unwrap();
    let mut task = TaskDoc::create(NewTask {
        project: "example",
        id: "T-abcdef",
        text: "Review task",
        stage: Stage::Todo,
        target: Target::Any,
        author: Actor::Person,
        rank: "V",
        now: 1,
    })
    .unwrap();
    task.floor().unwrap();
    store.save(&mut task).unwrap();
    let uuid = task.uuid().to_owned();
    (root, store, uuid)
}

#[test]
fn explicit_review_preserves_files_and_only_accepts_the_next_copy_after_confirmation() {
    let (_root, mut store, uuid) = fixture();
    let good = store.load(&uuid).unwrap().snapshot().unwrap();
    assert!(
        store.prepare_copy_review(&uuid).is_err(),
        "healthy copy is never a review target"
    );
    fs::write(store.snapshot_path(&uuid), b"damaged").unwrap();
    let updates = fs::read(store.updates_path(&uuid)).unwrap();
    fs::write(store.dir.join("tasks").join("unrelated.txt"), b"keep").unwrap();
    let review = store.prepare_copy_review(&uuid).unwrap();
    assert_eq!(fs::read(store.snapshot_path(&uuid)).unwrap(), b"damaged");
    assert!(store.was_managed(&uuid));
    assert!(store.move_reviewed_copy(&uuid, &"0".repeat(64)).is_err());
    let result = store
        .move_reviewed_copy(&uuid, &review.fingerprint)
        .unwrap();
    assert_eq!(result.state, "moved");
    assert_eq!(
        fs::read(Path::new(&result.backup).join(format!("{uuid}.loro"))).unwrap(),
        b"damaged"
    );
    assert_eq!(
        fs::read(Path::new(&result.backup).join(format!("{uuid}.updates"))).unwrap(),
        updates
    );
    assert!(!store.was_managed(&uuid));
    assert!(!store.snapshot_path(&uuid).exists());
    assert!(!store.updates_path(&uuid).exists());
    assert_eq!(
        fs::read(store.dir.join("tasks").join("unrelated.txt")).unwrap(),
        b"keep"
    );
    assert!(
        store.prepare_copy_review(&uuid).is_err(),
        "no replay after files moved"
    );
    let received = store
        .import_updates(vec![crate::Update {
            document: crate::Document::Task(uuid.clone()),
            bytes: good,
        }])
        .unwrap();
    assert!(received.refused.is_empty());
    assert!(
        store.load(&uuid).is_ok(),
        "next incoming copy uses the existing importer"
    );
}

#[test]
fn missing_files_and_changed_state_are_not_guessed() {
    let (_root, mut store, uuid) = fixture();
    fs::remove_file(store.snapshot_path(&uuid)).unwrap();
    fs::remove_file(store.updates_path(&uuid)).unwrap();
    let review = store.prepare_copy_review(&uuid).unwrap();
    assert_eq!(review.files, ["managed"]);
    fs::write(store.managed_path(&uuid), b"changed mark").unwrap();
    assert!(store
        .move_reviewed_copy(&uuid, &review.fingerprint)
        .is_err());
    assert!(store.was_managed(&uuid));
    let review = store.prepare_copy_review(&uuid).unwrap();
    assert_eq!(
        store
            .move_reviewed_copy(&uuid, &review.fingerprint)
            .unwrap()
            .state,
        "moved"
    );
    assert!(store.prepare_copy_review(&uuid).is_err());
    for bad in ["../outside", "", "T-abcdef"] {
        assert!(store.prepare_copy_review(bad).is_err());
    }
}

#[cfg(unix)]
#[test]
fn links_at_every_reviewed_path_and_parent_are_refused() {
    use std::os::unix::fs::symlink;
    for extension in ["loro", "updates", "managed"] {
        let (root, mut store, uuid) = fixture();
        let outside = root.path().join("outside");
        fs::write(&outside, b"unchanged").unwrap();
        let path = store.dir.join("tasks").join(format!("{uuid}.{extension}"));
        fs::remove_file(&path).unwrap();
        symlink(&outside, &path).unwrap();
        assert!(store.prepare_copy_review(&uuid).is_err());
        assert_eq!(fs::read(&outside).unwrap(), b"unchanged");
        assert!(fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink());
    }
    let (root, store, _uuid) = fixture();
    let folder = store.dir.join("tasks");
    let actual = store.dir.join("saved-tasks");
    fs::rename(&folder, &actual).unwrap();
    symlink(&actual, &folder).unwrap();
    drop(store);
    assert!(ProjectStore::open_copy_review(root.path(), "example", Duration::ZERO).is_err());
}

#[test]
fn concurrent_writer_holding_the_store_lock_causes_a_bounded_refusal() {
    let (root, store, uuid) = fixture();
    let before = fs::read(store.managed_path(&uuid)).unwrap();
    let start = Instant::now();
    assert!(
        ProjectStore::open_copy_review(root.path(), "example", Duration::from_millis(30))
            .unwrap()
            .is_none()
    );
    assert!(start.elapsed() < Duration::from_secs(1));
    assert_eq!(fs::read(store.managed_path(&uuid)).unwrap(), before);
    drop(store);
    assert!(
        ProjectStore::open_copy_review(root.path(), "example", Duration::from_millis(30))
            .unwrap()
            .is_some()
    );
}

#[test]
fn a_failure_after_moving_files_is_unknown_and_keeps_the_mark_and_backups() {
    let (_root, mut store, uuid) = fixture();
    fs::write(store.snapshot_path(&uuid), b"damaged").unwrap();
    let mark = fs::read(store.managed_path(&uuid)).unwrap();
    let review = store.prepare_copy_review(&uuid).unwrap();
    let outcome = store
        .move_reviewed_copy_with(&uuid, &review.fingerprint, || {
            Err(std::io::Error::new(
                ErrorKind::PermissionDenied,
                "synthetic mark refusal",
            ))
        })
        .unwrap();
    assert_eq!(outcome.state, "unknown");
    assert!(outcome.reason.unwrap().contains("synthetic mark refusal"));
    assert_eq!(fs::read(store.managed_path(&uuid)).unwrap(), mark);
    assert_eq!(
        fs::read(Path::new(&outcome.backup).join(format!("{uuid}.loro"))).unwrap(),
        b"damaged"
    );
    assert!(store
        .move_reviewed_copy(&uuid, &review.fingerprint)
        .is_err());
}

#[test]
fn oversized_or_missing_stores_are_refused_without_creating_anything() {
    let (root, mut store, uuid) = fixture();
    OpenOptions::new()
        .write(true)
        .open(store.snapshot_path(&uuid))
        .unwrap()
        .set_len(REVIEW_BYTES + 1)
        .unwrap();
    assert!(store.prepare_copy_review(&uuid).is_err());
    assert!(store.was_managed(&uuid));
    let missing = root.path().join("missing");
    assert!(ProjectStore::open_copy_review(&missing, "example", Duration::ZERO).is_err());
    assert!(!missing.exists());
}
