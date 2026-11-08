use super::*;
use std::{fs, fs::OpenOptions, path::PathBuf};

fn id(n: u64) -> String {
    format!("00000000-0000-4000-8000-{n:012x}")
}
fn executor() -> Executor {
    Executor {
        computer: id(1),
        run: "original-run".into(),
        birth: "original-birth".into(),
        conversation: Some("original-conversation".into()),
        child: None,
    }
}
fn fact(n: u64) -> Fact {
    Fact {
        id: id(n),
        executor: executor(),
        source: Source::VerifiedExternalInterrupt,
        turn: Some("original-turn".into()),
    }
}
fn writer(root: &Path) -> storage::Guard {
    let file = platform::private_open(
        OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true),
    )
    .open(root.join(".lock"))
    .unwrap();
    platform::FileLock::exclusive(file).unwrap()
}
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    platform::private_dir(dir.path()).unwrap();
    platform::restrict_dir(dir.path()).unwrap();
    dir
}
fn path(root: &Path, executor: &Executor) -> PathBuf {
    root.join("task-stop-facts")
        .join(format!("{}.json", executor.key().unwrap()))
}

#[test]
fn gate_off_never_opens_even_a_missing_root_or_an_invalid_executor() {
    let dir = fixture();
    let guard = writer(dir.path());
    let mut executor = executor();
    executor.birth.clear();
    let missing = dir.path().join("absent");
    assert!(open(&missing, &guard, &executor, true).unwrap().is_none());
    assert!(!missing.exists());
    assert!(!dir.path().join("task-stop-facts").exists());
}

#[test]
fn a_known_runs_missing_or_damaged_facts_are_never_reset() {
    let dir = fixture();
    let guard = writer(dir.path());
    assert!(Publisher::open_enabled(dir.path(), &guard, &executor(), false).is_err());
    assert!(!dir.path().join("task-stop-facts").exists());
    let publisher = Publisher::open_enabled(dir.path(), &guard, &executor(), true).unwrap();
    publisher.publish(&fact(2)).unwrap();
    let file = path(dir.path(), &executor());
    for bytes in [b"{broken".to_vec(), vec![b' '; BYTES + 1]] {
        fs::write(&file, &bytes).unwrap();
        assert!(Publisher::open_enabled(dir.path(), &guard, &executor(), true).is_err());
        assert_eq!(fs::read(&file).unwrap(), bytes);
    }
    fs::remove_file(&file).unwrap();
    assert!(Publisher::open_enabled(dir.path(), &guard, &executor(), false).is_err());
    assert!(!file.exists());
}

#[test]
fn publication_is_idempotent_and_substitution_refuses_without_a_write() {
    let dir = fixture();
    let guard = writer(dir.path());
    let publisher = Publisher::open_enabled(dir.path(), &guard, &executor(), true).unwrap();
    let original = fact(2);
    let published = publisher.publish(&original).unwrap();
    assert_eq!(published.facts, vec![original.clone()]);
    let file = path(dir.path(), &executor());
    let bytes = fs::read(&file).unwrap();
    assert_eq!(publisher.publish(&original).unwrap(), published);
    let mut changed = original;
    changed.turn = Some("other-turn".into());
    assert!(publisher.publish(&changed).is_err());
    changed = fact(3);
    changed.executor.run = "other-run".into();
    assert!(publisher.publish(&changed).is_err());
    assert_eq!(fs::read(file).unwrap(), bytes);
}

#[test]
fn a_run_record_rewrite_between_publication_and_import_cannot_drop_stop_facts() {
    let dir = fixture();
    let guard = writer(dir.path());
    let publisher = Publisher::open_enabled(dir.path(), &guard, &executor(), true).unwrap();
    let published = publisher.publish(&fact(2)).unwrap();
    let original_bytes = fs::read(path(dir.path(), &executor())).unwrap();
    drop(publisher);
    drop(guard); // No desktop import under the native writer.
                 // Rollback writer's selected-field record: the new field does not survive.
    let record = dir.path().join("run.json");
    storage::atomic(
        &record,
        r#"{"run_id":"original-run","conversation_id":"original-conversation"}"#,
    )
    .unwrap();
    let guard = writer(dir.path());
    let publisher = Publisher::open_enabled(dir.path(), &guard, &executor(), false).unwrap();
    assert_eq!(publisher.snapshot().unwrap(), published);
    assert_eq!(
        fs::read(path(dir.path(), &executor())).unwrap(),
        original_bytes
    );
    assert!(!fs::read_to_string(record).unwrap().contains("stop"));
}

#[test]
fn overflow_is_durably_unknown_and_never_drops_the_retained_exact_ids() {
    let dir = fixture();
    let guard = writer(dir.path());
    let publisher = Publisher::open_enabled(dir.path(), &guard, &executor(), true).unwrap();
    for n in 1..=EXECUTOR_HOLDS as u64 {
        assert!(!publisher.publish(&fact(n)).unwrap().held_unknown());
    }
    let overflow = publisher.publish(&fact(65)).unwrap();
    assert_eq!(overflow.facts.len(), 64);
    assert!(overflow.held_unknown());
    assert_eq!(publisher.publish(&fact(66)).unwrap(), overflow);
    let bytes = fs::read(path(dir.path(), &executor())).unwrap();
    drop(publisher);
    drop(guard);
    let guard = writer(dir.path());
    let publisher = Publisher::open_enabled(dir.path(), &guard, &executor(), false).unwrap();
    assert_eq!(publisher.snapshot().unwrap(), overflow);
    assert_eq!(fs::read(path(dir.path(), &executor())).unwrap(), bytes);
}

#[test]
fn conversation_binding_keeps_facts_but_a_resumed_run_does_not_inherit_them() {
    let dir = fixture();
    let guard = writer(dir.path());
    let mut unbound = executor();
    unbound.conversation = None;
    let publisher = Publisher::open_enabled(dir.path(), &guard, &unbound, true).unwrap();
    let mut stop = fact(2);
    stop.executor = unbound;
    publisher.publish(&stop).unwrap();
    let bound = Publisher::open_enabled(dir.path(), &guard, &executor(), false).unwrap();
    assert_eq!(bound.snapshot().unwrap().facts, vec![stop.clone()]);
    let mut resumed = executor();
    resumed.run = "resumed-run".into();
    resumed.birth = "resumed-birth".into();
    assert!(Publisher::open_enabled(dir.path(), &guard, &resumed, false).is_err());
    let new = Publisher::open_enabled(dir.path(), &guard, &resumed, true).unwrap();
    assert!(new.snapshot().unwrap().facts.is_empty());
    assert_eq!(publisher.snapshot().unwrap().facts, vec![stop]);
}

#[cfg(unix)]
#[test]
fn unexpected_links_special_files_and_replaced_writer_refuse() {
    use std::os::unix::fs::symlink;
    let dir = fixture();
    let guard = writer(dir.path());
    let publisher = Publisher::open_enabled(dir.path(), &guard, &executor(), true).unwrap();
    let file = path(dir.path(), &executor());
    let kept = dir.path().join("kept");
    fs::write(&kept, b"keep").unwrap();
    fs::remove_file(&file).unwrap();
    symlink(&kept, &file).unwrap();
    assert!(publisher.publish(&fact(2)).is_err());
    assert_eq!(fs::read(&kept).unwrap(), b"keep");
    fs::remove_file(&file).unwrap();
    let name = std::ffi::CString::new(file.to_str().unwrap()).unwrap();
    // SAFETY: private fixture path; the maintained reader must not wait on it.
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    assert!(publisher.snapshot().is_err());
    fs::remove_file(&file).unwrap();
    fs::rename(dir.path().join(".lock"), dir.path().join(".old-lock")).unwrap();
    let _replacement = writer(dir.path());
    assert!(Publisher::open_enabled(dir.path(), &guard, &executor(), true).is_err());
}
