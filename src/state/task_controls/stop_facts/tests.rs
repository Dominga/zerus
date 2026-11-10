use super::*;
use std::{fs, fs::OpenOptions, path::PathBuf};

fn id(n: u64) -> String {
    format!("00000000-0000-4000-8000-{n:012x}")
}
fn executor() -> Executor {
    Executor {
        computer: id(1),
        session_id: id(9000),
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
    let dir = crate::host::staging::private_tempdir_in(&platform::canonical(std::env::temp_dir()).unwrap());
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
fn a_reused_run_with_a_different_session_selector_never_reuses_or_mutates_facts() {
    let dir = fixture();
    let guard = writer(dir.path());
    let publisher = Publisher::open_enabled(dir.path(), &guard, &executor(), true).unwrap();
    publisher.publish(&fact(2)).unwrap();
    let file = path(dir.path(), &executor());
    let original = fs::read(&file).unwrap();
    let mut substituted = executor();
    substituted.session_id = id(9001);
    assert_eq!(substituted.key().unwrap(), executor().key().unwrap());
    assert!(Publisher::open_enabled(dir.path(), &guard, &substituted, false).is_err());
    assert!(Publisher::open_enabled(dir.path(), &guard, &substituted, true).is_err());
    let mut changed = fact(3);
    changed.executor = substituted;
    assert!(publisher.publish(&changed).is_err());
    let mut snapshot: serde_json::Value = serde_json::from_slice(&original).unwrap();
    snapshot["facts"][0]["executor"]["session_id"] = id(9001).into();
    let damaged = serde_json::to_vec(&snapshot).unwrap();
    fs::write(&file, &damaged).unwrap();
    assert!(publisher.snapshot().is_err());
    assert_eq!(fs::read(file).unwrap(), damaged);
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

#[test]
fn captured_stop_evidence_rechecks_retained_bytes_without_acquiring_the_native_writer() {
    let dir = fixture();
    let guard = writer(dir.path());
    let publisher = Publisher::open_enabled(dir.path(), &guard, &executor(), true).unwrap();
    publisher.publish(&fact(2)).unwrap();
    let captured = publisher.capture().unwrap();
    drop(guard);
    assert_eq!(captured.facts(), &[fact(2)]);
    assert_eq!(captured.executor(), &executor());
    // Rechecking is a bounded read, not a second native exclusion. A future
    // writer holding that exclusion cannot deadlock the actor's transaction.
    let held = writer(dir.path());
    captured.verify().unwrap();
    drop(held);
    let record = dir.path().join("run.json");
    storage::atomic(&record, r#"{"state":"live"}"#).unwrap();
    captured.verify().unwrap(); // rollback/run-record rewrites cannot erase it
    let file = path(dir.path(), &executor());
    let original = fs::read(&file).unwrap();
    let mut changed = original.clone();
    let at = changed.iter().position(|b| *b == b'2').unwrap();
    changed[at] = b'3';
    fs::write(&file, &changed).unwrap(); // same-size in-place change
    assert!(captured.verify().is_err());
    assert_eq!(fs::read(file).unwrap(), changed);
}

#[test]
fn removed_oversized_replaced_or_overflowed_capture_refuses_for_review_without_repair() {
    for mode in ["removed", "oversized", "replaced", "overflow", "writer"] {
        let dir = fixture();
        let guard = writer(dir.path());
        let publisher = Publisher::open_enabled(dir.path(), &guard, &executor(), true).unwrap();
        publisher.publish(&fact(2)).unwrap();
        if mode == "overflow" {
            for n in 3..=66 {
                publisher.publish(&fact(n)).unwrap();
            }
        }
        let captured = publisher.capture().unwrap();
        drop(guard);
        let file = path(dir.path(), &executor());
        match mode {
            "removed" => fs::remove_file(&file).unwrap(),
            "oversized" => fs::write(&file, vec![b' '; BYTES + 1]).unwrap(),
            "replaced" => {
                let bytes = fs::read(&file).unwrap();
                fs::rename(&file, file.with_extension("kept")).unwrap();
                platform::private_open(OpenOptions::new().write(true).create_new(true))
                    .open(&file)
                    .unwrap();
                fs::write(&file, bytes).unwrap();
            }
            "writer" => {
                fs::rename(dir.path().join(".lock"), dir.path().join(".old-lock")).unwrap();
                drop(writer(dir.path()));
            }
            _ => (),
        }
        let bytes = fs::read(&file).ok();
        assert!(captured.verify().is_err(), "{mode}");
        assert_eq!(
            fs::read(&file).ok(),
            bytes,
            "{mode}: recheck must not repair"
        );
    }
}

#[cfg(unix)]
#[test]
fn a_captured_fact_replaced_with_a_link_is_never_followed() {
    let dir = fixture();
    let guard = writer(dir.path());
    let publisher = Publisher::open_enabled(dir.path(), &guard, &executor(), true).unwrap();
    let captured = publisher.capture().unwrap();
    drop(guard);
    let file = path(dir.path(), &executor());
    let kept = dir.path().join("keep");
    fs::write(&kept, b"untouched").unwrap();
    fs::remove_file(&file).unwrap();
    std::os::unix::fs::symlink(&kept, &file).unwrap();
    assert!(captured.verify().is_err());
    assert_eq!(fs::read(&kept).unwrap(), b"untouched");
}

#[cfg(target_os = "linux")]
#[test]
fn review_a_recreated_native_fact_object_never_passes_as_the_original_capture() {
    use std::io::Write;
    let dir = tempfile::tempdir_in(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target"))
        .unwrap();
    platform::restrict_dir(dir.path()).unwrap();
    let guard = writer(dir.path());
    let publisher = Publisher::open_enabled(dir.path(), &guard, &executor(), true).unwrap();
    publisher.publish(&fact(2)).unwrap();
    let captured = publisher.capture().unwrap();
    drop(guard);
    let file = path(dir.path(), &executor());
    let original = fs::read(&file).unwrap();
    let modified = fs::metadata(&file).unwrap().modified().unwrap();
    let mut reused = None;
    fs::remove_file(&file).unwrap();
    for n in 1..=4096 {
        let mut replacement =
            platform::private_open(OpenOptions::new().read(true).write(true).create_new(true))
                .open(&file)
                .unwrap();
        replacement.write_all(&original).unwrap();
        replacement
            .set_times(std::fs::FileTimes::new().set_modified(modified))
            .unwrap();
        if platform::FileIdentity::of(&replacement).unwrap() == captured.identity {
            reused = Some(n);
            break;
        }
        drop(replacement);
        fs::rename(&file, file.with_extension(format!("kept-{n}"))).unwrap();
    }
    eprintln!("private original-identity recycling: {reused:?}");
    assert!(
        captured.verify().is_err(),
        "a deleted and recreated native fact object passed the original captured-object check"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn review_a_recreated_writer_object_never_passes_as_the_original_capture_writer() {
    use std::io::Write;
    let dir = tempfile::tempdir_in(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target"))
        .unwrap();
    platform::restrict_dir(dir.path()).unwrap();
    let guard = writer(dir.path());
    let publisher = Publisher::open_enabled(dir.path(), &guard, &executor(), true).unwrap();
    publisher.publish(&fact(2)).unwrap();
    let captured = publisher.capture().unwrap();
    drop(guard);
    let lock = dir.path().join(".lock");
    let modified = fs::metadata(&lock).unwrap().modified().unwrap();
    fs::remove_file(&lock).unwrap();
    let mut reused = None;
    for n in 1..=4096 {
        let mut replacement =
            platform::private_open(OpenOptions::new().read(true).write(true).create_new(true))
                .open(&lock)
                .unwrap();
        replacement.write_all(b"").unwrap();
        replacement
            .set_times(std::fs::FileTimes::new().set_modified(modified))
            .unwrap();
        if platform::FileIdentity::of(&replacement).unwrap() == captured.writer {
            reused = Some(n);
            break;
        }
        drop(replacement);
        if n != 4096 {
            fs::rename(&lock, dir.path().join(format!("kept-writer-{n}"))).unwrap();
        }
    }
    eprintln!("private original-writer identity recycling: {reused:?}");
    let _new_writer = platform::FileLock::exclusive(
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(&lock)
            .unwrap(),
    )
    .unwrap();
    assert!(
        captured.verify().is_err(),
        "a different native writer file passed the original captured-writer check"
    );
}

#[test]
fn retaining_captured_objects_releases_the_native_exclusion_for_another_writer() {
    use std::sync::mpsc;
    use std::time::{Duration, Instant};
    let dir = fixture();
    let guard = writer(dir.path());
    let publisher = Publisher::open_enabled(dir.path(), &guard, &executor(), true).unwrap();
    publisher.publish(&fact(2)).unwrap();
    let captured = publisher.capture().unwrap();
    drop(guard);

    let root = dir.path().to_path_buf();
    let (acquired, ready) = mpsc::sync_channel(1);
    let (release, released) = mpsc::sync_channel(1);
    let next = std::thread::spawn(move || -> Result<()> {
        let next = crate::state::task_controls::writer::test_open(
            &root,
            Instant::now() + Duration::from_secs(5),
        )?;
        acquired.send(()).map_err(err)?;
        released.recv_timeout(Duration::from_secs(5)).map_err(err)?;
        next.verify()
    });
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    // Import's read-only recheck must succeed while a different native writer
    // holds the original exclusion. No native lock may be taken under the DB.
    captured.verify().unwrap();
    release.send(()).unwrap();
    next.join().unwrap().unwrap();
    captured.verify().unwrap();
}
