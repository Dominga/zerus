use super::*;
use crate::task_start::capture::tests::Fixture;
use std::{fs, os::unix::fs::symlink};

fn prepared(f: &Fixture) -> Journal {
    Journal::prepare(f.capture().unwrap(), &f.intent.reconcile_id).unwrap()
}
fn saved(f: &Fixture) -> Saved {
    Saved::read(
        &f.store,
        &f.intent.binding.key.project,
        &f.intent.binding.key.task,
    )
    .unwrap()
    .unwrap()
}
fn names(f: &Fixture) -> Vec<OsString> {
    let mut names: Vec<_> = fs::read_dir(f.file.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    names.sort();
    names
}
fn stage(journal: &Journal, cut: &str) -> File {
    let slot = &journal.saved.slot;
    let mut next = journal.saved.disk.record.clone();
    next.phase = Phase::OfferedUnknown;
    let bytes = encode(&next).unwrap();
    let mut file = slot.create(&name(slot), true).unwrap();
    file.write_all(&bytes).unwrap();
    file.sync_all().unwrap();
    slot.folder.sync_all().unwrap();
    if cut == "unrecorded" {
        return file;
    }
    track(
        slot,
        &journal.saved.disk,
        &journal.lock,
        &file,
        &next,
        &bytes,
        &mut |f, _| f.sync_all(),
    )
    .unwrap();
    if cut == "renamed" {
        let from = CString::new(name(slot).as_bytes()).unwrap();
        let to = CString::new(slot.name.as_bytes()).unwrap();
        // SAFETY: the exact staged original in its retained private directory.
        assert_eq!(
            unsafe {
                libc::renameat(
                    slot.folder.as_raw_fd(),
                    from.as_ptr(),
                    slot.folder.as_raw_fd(),
                    to.as_ptr(),
                )
            },
            0
        );
        slot.folder.sync_all().unwrap();
    } else {
        assert_eq!(cut, "recorded");
    }
    file
}

#[test]
fn recorded_original_is_the_only_candidate_removed_and_reading_never_mutates() {
    let f = Fixture::new();
    let journal = prepared(&f);
    let original = journal.saved.disk.bytes.clone();
    let outbox = fs::read(&f.file).unwrap();
    let marker = fs::read(&f.marker).unwrap();
    let held = stage(&journal, "recorded");
    let tracked = fs::read(f.file.with_extension("reconcile-lock")).unwrap();
    let before_names = names(&f);
    assert_eq!(saved(&f).publication_status(), Status::Recoverable);
    assert_eq!(names(&f), before_names);
    assert_eq!(
        fs::read(f.file.with_extension("reconcile-lock")).unwrap(),
        tracked
    );
    assert_eq!(
        Journal::prepare(f.capture().unwrap(), &f.intent.reconcile_id).err(),
        Some(Error::Busy)
    );
    drop(journal);
    let observed = saved(&f);
    let lock = observed.slot.lock(false).unwrap();
    recover(&observed.slot, &observed.disk, &lock).unwrap();
    assert_eq!(observed.publication_status(), Status::Clear);
    assert_eq!(
        fs::read(f.file.with_extension("reconcile")).unwrap(),
        original
    );
    assert_eq!(fs::read(&f.file).unwrap(), outbox);
    assert_eq!(fs::read(&f.marker).unwrap(), marker);
    assert_eq!(held.metadata().unwrap().nlink(), 0);
    assert!(read(&lock, BYTES).unwrap().0.is_empty());
    drop(lock);
    let mut explicit = prepared(&f);
    assert_eq!(
        explicit.offer().unwrap().reconcile_id,
        f.intent.reconcile_id
    );
    assert_eq!(explicit.saved().phase(), Phase::OfferedUnknown);
}

#[test]
fn renamed_original_retires_tracking_without_replacing_journal_or_offering_again() {
    let f = Fixture::new();
    let journal = prepared(&f);
    let held = stage(&journal, "renamed");
    drop(journal);
    let observed = saved(&f);
    let before = observed.disk.version.clone();
    let bytes = observed.disk.bytes.clone();
    assert_eq!(observed.phase(), Phase::OfferedUnknown);
    assert_eq!(observed.publication_status(), Status::Recoverable);
    let lock = observed.slot.lock(false).unwrap();
    recover(&observed.slot, &observed.disk, &lock).unwrap();
    assert_eq!(observed.publication_status(), Status::Clear);
    assert_eq!(file_version(&held, super::super::BYTES).unwrap(), before);
    assert_eq!(observed.disk.bytes, bytes);
    drop(lock);
    let mut explicit = prepared(&f);
    assert_eq!(explicit.offer().err(), Some(Error::OfferedUnknown));
    assert_eq!(explicit.saved().reconcile_id(), f.intent.reconcile_id);
}

#[test]
fn unknown_slot_is_bounded_across_repeated_explicit_checks_and_legacy_files_are_kept() {
    let f = Fixture::new();
    let journal = prepared(&f);
    let _original = stage(&journal, "unrecorded");
    drop(journal);
    let before = names(&f);
    let bytes = fs::read(f.file.with_extension("reconcile.tmp")).unwrap();
    for _ in 0..32 {
        let mut explicit = prepared(&f);
        assert_eq!(explicit.saved().publication_status(), Status::NeedsReview);
        assert_eq!(explicit.offer().err(), Some(Error::PublicationUnknown));
        assert_eq!(names(&f), before);
        assert_eq!(
            fs::read(f.file.with_extension("reconcile.tmp")).unwrap(),
            bytes
        );
        assert_eq!(explicit.saved().phase(), Phase::Prepared);
        assert_eq!(explicit.saved().reconcile_id(), f.intent.reconcile_id);
    }
    let f = Fixture::new();
    let legacy = f
        .file
        .with_extension("reconcile.00000009-1111-4111-8111-111111111111.tmp");
    crate::fsx::write_private_durable(&legacy, b"unrecorded legacy object").unwrap();
    let mut journal = prepared(&f);
    journal.offer().unwrap();
    assert_eq!(fs::read(legacy).unwrap(), b"unrecorded legacy object");
    assert_eq!(journal.saved().publication_status(), Status::Clear);
}

#[test]
fn replaced_changed_and_unsafe_candidates_remain_for_review_even_with_identical_bytes() {
    for change in [
        "replace",
        "body",
        "restore",
        "hardlink",
        "symlink",
        "mode",
        "oversize",
        "fifo",
        "before-id",
        "after-id",
        "before-hash",
        "after-hash",
        "directories",
        "duplicate",
        "tracking-oversize",
        "tracking-partial",
        "missing",
    ] {
        let f = Fixture::new();
        let journal = prepared(&f);
        let original = stage(&journal, "recorded");
        let path = f.file.with_extension("reconcile.tmp");
        let bytes = fs::read(&path).unwrap();
        let lock_path = f.file.with_extension("reconcile-lock");
        match change {
            "replace" | "symlink" | "fifo" => {
                fs::rename(&path, f.base.join("kept-original")).unwrap();
                match change {
                    "replace" => crate::fsx::write_private_durable(&path, &bytes).unwrap(),
                    "symlink" => symlink(f.base.join("kept-original"), &path).unwrap(),
                    "fifo" => {
                        let p = CString::new(path.as_os_str().as_bytes()).unwrap();
                        // SAFETY: new private fixture FIFO, never an owner path.
                        assert_eq!(unsafe { libc::mkfifo(p.as_ptr(), 0o600) }, 0);
                    }
                    _ => unreachable!(),
                }
            }
            "body" | "restore" => {
                fs::write(&path, b"changed in place").unwrap();
                if change == "restore" {
                    fs::write(&path, &bytes).unwrap();
                    original
                        .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1))
                        .unwrap();
                }
            }
            "hardlink" => fs::hard_link(&path, f.base.join("kept-link")).unwrap(),
            "mode" => {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
            }
            "oversize" => original.set_len(super::super::BYTES as u64 + 1).unwrap(),
            "missing" => fs::remove_file(&path).unwrap(),
            _ => {
                let mut header: serde_json::Value =
                    serde_json::from_slice(&fs::read(&lock_path).unwrap()).unwrap();
                match change {
                    "before-id" => {
                        header["before_id"] =
                            serde_json::json!("00000009-1111-4111-8111-111111111111")
                    }
                    "after-id" => {
                        header["after_id"] =
                            serde_json::json!("00000009-1111-4111-8111-111111111111")
                    }
                    "before-hash" => header["before_sha256"] = serde_json::json!("0".repeat(64)),
                    "after-hash" => header["after_sha256"] = serde_json::json!("0".repeat(64)),
                    "directories" => header["directories"][0]["object"][1] = serde_json::json!(0),
                    "duplicate" | "tracking-oversize" | "tracking-partial" => (),
                    _ => unreachable!(),
                }
                let bytes = if change == "duplicate" {
                    format!("{{\"abi\":\"{ABI}\",{}", &header.to_string()[1..]).into_bytes()
                } else if change == "tracking-oversize" {
                    let mut bytes = header.to_string().into_bytes();
                    bytes.resize(BYTES + 1, b' ');
                    bytes
                } else if change == "tracking-partial" {
                    let mut bytes = header.to_string().into_bytes();
                    bytes.truncate(bytes.len() / 2);
                    bytes
                } else {
                    header.to_string().into_bytes()
                };
                fs::write(&lock_path, bytes).unwrap();
            }
        }
        let tracking = fs::read(&lock_path).unwrap();
        let journal_bytes = fs::read(f.file.with_extension("reconcile")).unwrap();
        assert_eq!(
            journal.saved().publication_status(),
            Status::NeedsReview,
            "{change}"
        );
        assert!(
            recover(&journal.saved.slot, &journal.saved.disk, &journal.lock).is_err(),
            "{change}"
        );
        assert_eq!(fs::read(&lock_path).unwrap(), tracking, "{change}");
        assert_eq!(
            fs::read(f.file.with_extension("reconcile")).unwrap(),
            journal_bytes,
            "{change}"
        );
        assert_eq!(
            path.symlink_metadata().is_ok(),
            change != "missing",
            "{change}"
        );
    }
}

#[test]
fn copied_before_or_published_journal_cannot_authorize_orphan_cleanup() {
    for cut in ["recorded", "renamed"] {
        let f = Fixture::new();
        let journal = prepared(&f);
        let held = stage(&journal, cut);
        drop(journal);
        let path = f.file.with_extension("reconcile");
        let bytes = fs::read(&path).unwrap();
        crate::fsx::write_private_atomic(&path, &bytes, true).unwrap();
        let observed = saved(&f);
        let lock = observed.slot.lock(false).unwrap();
        let tracking = read(&lock, BYTES).unwrap().0;
        assert_eq!(observed.publication_status(), Status::NeedsReview, "{cut}");
        assert!(recover(&observed.slot, &observed.disk, &lock).is_err());
        assert_eq!(read(&lock, BYTES).unwrap().0, tracking);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(
            held.metadata().unwrap().nlink(),
            u64::from(cut == "recorded")
        );
    }
}

#[test]
fn real_writer_death_keeps_one_slot_and_recovers_only_the_recorded_original() {
    use std::{
        io::Read,
        process::{Child, Command, Stdio},
        time::{Duration, Instant},
    };
    const LEAF: &str = "ZERUS_TEST_TASK_PUBLICATION_LEAF";
    if let Some(root) = std::env::var_os(LEAF) {
        let cut = std::env::var("ZERUS_TEST_TASK_PUBLICATION_CUT").unwrap();
        assert!(["unrecorded", "recorded", "renamed"].contains(&cut.as_str()));
        let intent = wire::test_intent();
        let source = Captured::read(
            Path::new(&root),
            &intent.binding.key.project,
            &intent.binding.key.task,
            &intent.binding.key.request_id,
            &intent.binding.ui_snapshot_sha256,
            intent.scope,
        )
        .unwrap();
        let journal = Journal::prepare(source, &intent.reconcile_id).unwrap();
        let _held = stage(&journal, &cut);
        crate::fsx::write_private_durable(
            &Path::new(&root).join("publication-ready"),
            cut.as_bytes(),
        )
        .unwrap();
        // Exactly this original parent's pipe; this leaf cannot spawn again.
        let _ = std::io::stdin().read(&mut [0; 1]);
        return;
    }
    struct Owned(Child);
    impl Drop for Owned {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    for cut in ["unrecorded", "recorded", "renamed"] {
        let f = Fixture::new();
        drop(prepared(&f));
        let outbox = fs::read(&f.file).unwrap();
        let marker = fs::read(&f.marker).unwrap();
        let mut child = Owned(Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "task_start::capture::journal::publication::tests::real_writer_death_keeps_one_slot_and_recovers_only_the_recorded_original", "--nocapture"])
            .env(LEAF, &f.store).env("ZERUS_TEST_TASK_PUBLICATION_CUT", cut)
            .stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().unwrap());
        let until = Instant::now() + Duration::from_secs(8);
        while !f.store.join("publication-ready").exists() {
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "original leaf exited before the durable cut"
            );
            assert!(
                Instant::now() < until,
                "original leaf did not reach the bounded durable cut"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(
            fs::read(f.store.join("publication-ready")).unwrap(),
            cut.as_bytes()
        );
        child.0.kill().unwrap();
        assert!(!child.0.wait().unwrap().success());
        drop(child.0.stdin.take());
        let observed = saved(&f);
        let before = observed.disk.bytes.clone();
        let lock = observed.slot.lock(false).unwrap();
        assert_eq!(observed.reconcile_id(), f.intent.reconcile_id);
        if cut == "unrecorded" {
            assert_eq!(observed.publication_status(), Status::NeedsReview);
            assert!(recover(&observed.slot, &observed.disk, &lock).is_err());
            assert!(f.file.with_extension("reconcile.tmp").exists());
        } else {
            assert_eq!(observed.publication_status(), Status::Recoverable);
            recover(&observed.slot, &observed.disk, &lock).unwrap();
            assert_eq!(observed.publication_status(), Status::Clear);
            assert!(!f.file.with_extension("reconcile.tmp").exists());
        }
        assert_eq!(observed.disk.bytes, before);
        assert_eq!(fs::read(&f.file).unwrap(), outbox);
        assert_eq!(fs::read(&f.marker).unwrap(), marker);
        if cut == "renamed" {
            assert_eq!(observed.phase(), Phase::OfferedUnknown);
        } else {
            assert_eq!(observed.phase(), Phase::Prepared);
        }
    }
}

#[test]
fn original_tracking_and_candidate_snapshots_cannot_be_rebound_during_recovery() {
    for change in ["tracking", "candidate"] {
        let f = Fixture::new();
        let journal = prepared(&f);
        let _held = stage(&journal, "recorded");
        let observed = inspect(&journal.saved.slot, &journal.saved.disk, &journal.lock).unwrap();
        let lock_path = f.file.with_extension("reconcile-lock");
        let candidate_path = f.file.with_extension("reconcile.tmp");
        let bytes = fs::read(&candidate_path).unwrap();
        let tracking = fs::read(&lock_path).unwrap();
        if change == "tracking" {
            // Restore identical bytes on the same inode, but not its native
            // version. An already captured recovery must not adopt that edit.
            fs::write(&lock_path, b"changed").unwrap();
            fs::write(&lock_path, &tracking).unwrap();
            journal
                .lock
                .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1))
                .unwrap();
        } else {
            fs::rename(&candidate_path, f.base.join("original-candidate")).unwrap();
            crate::fsx::write_private_durable(&candidate_path, &bytes).unwrap();
        }
        assert!(
            reconcile(
                &journal.saved.slot,
                &journal.saved.disk,
                &journal.lock,
                observed
            )
            .is_err(),
            "{change}"
        );
        assert_eq!(fs::read(&candidate_path).unwrap(), bytes);
        assert_eq!(fs::read(&lock_path).unwrap(), tracking);
        assert_eq!(journal.saved().phase(), Phase::Prepared);
    }
}

#[test]
fn publication_sync_order_precedes_return_and_final_clear_rechecks_its_snapshot() {
    let f = Fixture::new();
    let mut journal = prepared(&f);
    let mut steps = Vec::new();
    journal
        .offer_with_sync(|file, directory| {
            if directory {
                steps.push("directory");
                assert_eq!(saved(&f).phase(), Phase::OfferedUnknown);
            } else {
                let bytes = read(file, super::super::BYTES)?.0;
                if bytes.is_empty() {
                    steps.push("clear");
                    assert_eq!(saved(&f).phase(), Phase::OfferedUnknown);
                    assert!(!f.file.with_extension("reconcile.tmp").exists());
                } else {
                    let value: serde_json::Value =
                        serde_json::from_slice(&bytes).map_err(|_| refused())?;
                    if value.get("abi").is_some() {
                        steps.push("tracking");
                        assert_eq!(saved(&f).phase(), Phase::Prepared);
                    } else {
                        steps.push("candidate");
                        assert_eq!(saved(&f).phase(), Phase::Prepared);
                    }
                }
            }
            file.sync_all()
        })
        .unwrap();
    assert_eq!(steps, ["candidate", "tracking", "directory", "clear"]);
    assert_eq!(journal.saved().publication_status(), Status::Clear);

    let f = Fixture::new();
    let journal = prepared(&f);
    let _held = stage(&journal, "renamed");
    let pending = tracking(&journal.saved.slot, &journal.lock)
        .unwrap()
        .unwrap();
    let path = f.file.with_extension("reconcile-lock");
    let bytes = fs::read(&path).unwrap();
    fs::write(&path, b"intervening edit").unwrap();
    fs::write(&path, &bytes).unwrap();
    journal
        .lock
        .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1))
        .unwrap();
    assert!(clear(
        &journal.saved.slot,
        &journal.lock,
        &pending,
        &mut |file, _| file.sync_all()
    )
    .is_err());
    assert_eq!(fs::read(path).unwrap(), bytes);
    assert_eq!(saved(&f).phase(), Phase::OfferedUnknown);
}
