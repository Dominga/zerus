use super::*;
use crate::task_start::capture::tests::Fixture;
use std::{fs, os::unix::fs::symlink};

#[track_caller]
fn prepared(f: &Fixture) -> Journal {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(8);
    loop {
        match Journal::prepare(
            f.capture().unwrap(),
            &CheckId::fixture_initial(&f.intent.reconcile_id),
        ) {
            Ok(journal) => return journal,
            // Another parallel fixture's fork may briefly retain the same
            // CLOEXEC description until exec. Busy is a no-write refusal;
            // wait for the actual flock, never retry an offer/publication.
            Err(Error::Busy) if std::time::Instant::now() < until => {
                std::thread::sleep(std::time::Duration::from_millis(1))
            }
            Err(error) => panic!("original metadata turn unavailable: {error:?}"),
        }
    }
}
#[track_caller]
fn metadata_lock(slot: &Slot) -> File {
    metadata_lock_after_busy(slot, || {})
}
fn metadata_lock_after_busy(slot: &Slot, mut busy: impl FnMut()) -> File {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(8);
    loop {
        match slot.lock(false) {
            Ok(file) => return file,
            Err(Error::Busy) if std::time::Instant::now() < until => {
                busy();
                std::thread::sleep(std::time::Duration::from_millis(1))
            }
            Err(error) => panic!("original metadata flock unavailable: {error:?}"),
        }
    }
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
    if cut == "unrecorded" {
        // Adverse external/legacy scratch: deliberately NOT a writer cut.
        let file = slot.create(&name(slot), true).unwrap();
        file.write_all_at(&bytes, 0).unwrap();
        file.sync_all().unwrap();
        return file;
    }
    let allocated = allocate(
        slot,
        &journal.saved.disk,
        &journal.lock,
        &next,
        &bytes,
        &mut |f, _| f.sync_all(),
    )
    .unwrap();
    let file = slot.create(&name(slot), true).unwrap();
    let object = created(slot, &journal.lock, &allocated, &file, &mut |f, _| {
        f.sync_all()
    })
    .unwrap();
    file.write_all_at(&bytes, 0).unwrap();
    file.sync_all().unwrap();
    ready(slot, &journal.lock, &object, &file, &mut |f, _| {
        f.sync_all()
    })
    .unwrap();
    if cut == "renamed" {
        rename_candidate(slot);
        slot.folder.sync_all().unwrap();
    } else {
        assert_eq!(cut, "recorded");
    }
    file
}
fn rename_candidate(slot: &Slot) {
    let from = CString::new(name(slot).as_bytes()).unwrap();
    let to = CString::new(slot.name.as_bytes()).unwrap();
    // SAFETY: the original staged file in the retained private directory.
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
}
fn frames(bytes: &[u8]) -> Vec<serde_json::Value> {
    bytes
        .split(|b| *b == b'\n')
        .filter(|v| !v.is_empty())
        .map(|v| serde_json::from_slice(v).unwrap())
        .collect()
}
fn encode_frames(values: &[serde_json::Value]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|v| {
            let mut b = v.to_string().into_bytes();
            b.push(b'\n');
            b
        })
        .collect()
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
        Journal::prepare(
            f.capture().unwrap(),
            &CheckId::fixture_initial(&f.intent.reconcile_id)
        )
        .err(),
        Some(Error::Busy)
    );
    drop(journal);
    let observed = saved(&f);
    let lock = metadata_lock(&observed.slot);
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
    let lock = metadata_lock(&observed.slot);
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
        assert_eq!(explicit.offer().err(), Some(Error::PublicationNeedsReview));
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
            _ => {
                let mut log = frames(&fs::read(&lock_path).unwrap());
                let header = &mut log[0]["allocation"];
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
                    let mut bytes =
                        format!("{{\"stage\":\"allocated\",{}\n", &log[0].to_string()[1..])
                            .into_bytes();
                    bytes.extend(encode_frames(&log[1..]));
                    bytes
                } else if change == "tracking-oversize" {
                    let mut bytes = encode_frames(&log);
                    bytes.resize(BYTES + 1, b' ');
                    bytes
                } else if change == "tracking-partial" {
                    let mut bytes = encode_frames(&log);
                    bytes.truncate(bytes.len() / 2);
                    bytes
                } else {
                    encode_frames(&log)
                };
                fs::write(&lock_path, bytes).unwrap();
            }
        }
        let tracking = fs::read(&lock_path).unwrap();
        let journal_bytes = fs::read(f.file.with_extension("reconcile")).unwrap();
        assert_eq!(
            journal.saved().publication_status(),
            if change == "tracking-oversize" {
                Status::Oversize
            } else {
                Status::NeedsReview
            },
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
        assert!(path.symlink_metadata().is_ok(), "{change}");
    }
}

#[test]
fn copied_before_or_published_journal_cannot_authorize_orphan_cleanup() {
    for (cut, birth_available) in [
        ("recorded", true),
        ("recorded", false),
        ("renamed", true),
        ("renamed", false),
    ] {
        let f = Fixture::new();
        let journal = prepared(&f);
        let held = stage(&journal, cut);
        if !birth_available {
            // Some filesystems expose no creation time. The recorded native
            // object must independently reject an identical-byte replacement.
            let path = f.file.with_extension("reconcile-lock");
            let mut log = frames(&fs::read(&path).unwrap());
            log[1]["object"]["born"] = serde_json::Value::Null;
            fs::write(path, encode_frames(&log)).unwrap();
        }
        drop(journal);
        let path = f.file.with_extension("reconcile");
        let bytes = fs::read(&path).unwrap();
        crate::fsx::write_private_atomic(&path, &bytes, true).unwrap();
        let observed = saved(&f);
        let lock = metadata_lock(&observed.slot);
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

const CUTS: &[&str] = &[
    "allocation-write",
    "allocated",
    "created-empty",
    "created-write",
    "created",
    "body-partial",
    "body-synced",
    "ready-write",
    "ready",
    "renamed-unsynced",
    "renamed",
    "unlinked-unsynced",
    "unlinked",
    "clear-unsynced",
];
fn reached(root: &Path, cut: &str) -> ! {
    use std::io::Read;
    crate::fsx::write_private_durable(&root.join("publication-ready"), cut.as_bytes()).unwrap();
    // Retry interrupted reads instead of completing the crash-cut leaf early.
    let _ = std::io::stdin().read_exact(&mut [0; 1]);
    // Only the marked one-shot leaf owns this pipe. EOF cannot enter a parent
    // branch or continue another filesystem step after the original dies.
    std::process::exit(0)
}
fn crash_cut(journal: &Journal, root: &Path, cut: &str) {
    let slot = &journal.saved.slot;
    let mut next = journal.saved.disk.record.clone();
    next.phase = Phase::OfferedUnknown;
    let bytes = encode(&next).unwrap();
    let allocated = allocate(
        slot,
        &journal.saved.disk,
        &journal.lock,
        &next,
        &bytes,
        &mut |f, _| {
            if cut == "allocation-write" {
                reached(root, cut);
            }
            f.sync_all()
        },
    )
    .unwrap();
    if cut == "allocated" {
        reached(root, cut);
    }
    let file = slot.create(&name(slot), true).unwrap();
    if cut == "created-empty" {
        reached(root, cut);
    }
    let object = created(slot, &journal.lock, &allocated, &file, &mut |f, _| {
        if cut == "created-write" {
            reached(root, cut);
        }
        f.sync_all()
    })
    .unwrap();
    if cut == "created" {
        reached(root, cut);
    }
    file.write_all_at(&bytes[..bytes.len() / 2], 0).unwrap();
    if cut == "body-partial" {
        file.sync_all().unwrap();
        reached(root, cut);
    }
    file.write_all_at(&bytes, 0).unwrap();
    file.sync_all().unwrap();
    if cut == "body-synced" {
        reached(root, cut);
    }
    ready(slot, &journal.lock, &object, &file, &mut |f, _| {
        if cut == "ready-write" {
            reached(root, cut);
        }
        f.sync_all()
    })
    .unwrap();
    if cut == "ready" {
        reached(root, cut);
    }
    if ["unlinked-unsynced", "unlinked", "clear-unsynced"].contains(&cut) {
        let observed = inspect(slot, &journal.saved.disk, &journal.lock).unwrap();
        reconcile_with_sync(
            slot,
            &journal.saved.disk,
            &journal.lock,
            observed,
            &mut |f, directory| {
                if directory && cut == "unlinked-unsynced" || !directory && cut == "clear-unsynced"
                {
                    reached(root, cut);
                }
                f.sync_all()?;
                if directory && cut == "unlinked" {
                    reached(root, cut);
                }
                Ok(())
            },
        )
        .unwrap();
        unreachable!();
    }
    rename_candidate(slot);
    if cut == "renamed-unsynced" {
        reached(root, cut);
    }
    slot.folder.sync_all().unwrap();
    assert_eq!(cut, "renamed");
    reached(root, cut);
}
fn abort_original_at(f: &Fixture, cut: &str) {
    use std::{
        process::{Child, Command, Stdio},
        time::{Duration, Instant},
    };
    struct Owned(Child);
    impl Drop for Owned {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut child = Owned(Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "task_start::capture::journal::publication::tests::real_writer_death_keeps_one_slot_and_recovers_only_the_recorded_original", "--nocapture"])
        .env("ZERUS_TEST_TASK_PUBLICATION_LEAF", &f.store).env("ZERUS_TEST_TASK_PUBLICATION_CUT", cut)
        .stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().unwrap());
    let until = Instant::now() + Duration::from_secs(8);
    while !f.store.join("publication-ready").exists() {
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "original leaf exited before {cut}"
        );
        assert!(
            Instant::now() < until,
            "original leaf did not reach bounded {cut}"
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
}
#[test]
fn real_writer_death_keeps_one_slot_and_recovers_only_the_recorded_original() {
    if let Some(root) = std::env::var_os("ZERUS_TEST_TASK_PUBLICATION_LEAF") {
        let cut = std::env::var("ZERUS_TEST_TASK_PUBLICATION_CUT").unwrap();
        assert!(CUTS.contains(&cut.as_str()));
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
        let journal =
            Journal::prepare(source, &CheckId::fixture_initial(&intent.reconcile_id)).unwrap();
        crash_cut(&journal, Path::new(&root), &cut);
        unreachable!();
    }
    for cut in CUTS {
        let f = Fixture::new();
        drop(prepared(&f));
        let outbox = fs::read(&f.file).unwrap();
        let marker = fs::read(&f.marker).unwrap();
        abort_original_at(&f, cut);
        let observed = saved(&f);
        let before = observed.disk.bytes.clone();
        let lock = metadata_lock(&observed.slot);
        assert_eq!(observed.reconcile_id(), f.intent.reconcile_id);
        assert_ne!(observed.publication_status(), Status::NeedsReview, "{cut}");
        recover(&observed.slot, &observed.disk, &lock).unwrap();
        assert_eq!(observed.publication_status(), Status::Clear, "{cut}");
        assert!(!f.file.with_extension("reconcile.tmp").exists());
        assert_eq!(observed.disk.bytes, before);
        assert_eq!(fs::read(&f.file).unwrap(), outbox);
        assert_eq!(fs::read(&f.marker).unwrap(), marker);
        drop(lock);
        let mut explicit = prepared(&f);
        if cut.starts_with("renamed") {
            assert_eq!(explicit.saved().phase(), Phase::OfferedUnknown);
            assert_eq!(explicit.offer().err(), Some(Error::OfferedUnknown));
        } else {
            assert_eq!(explicit.saved().phase(), Phase::Prepared);
            assert_eq!(
                explicit.offer().unwrap().reconcile_id,
                f.intent.reconcile_id,
                "{cut}"
            );
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
                    if bytes.last() == Some(&b'\n') {
                        let values = frames(&bytes);
                        steps.push(match values.last().unwrap()["stage"].as_str().unwrap() {
                            "allocated" => {
                                assert!(!f.file.with_extension("reconcile.tmp").exists());
                                "allocated"
                            }
                            "created" => {
                                assert!(
                                    fs::read(f.file.with_extension("reconcile.tmp"))?.is_empty()
                                );
                                "created"
                            }
                            "ready" => {
                                let bytes = fs::read(f.file.with_extension("reconcile.tmp"))?;
                                assert!(!bytes.is_empty());
                                assert_eq!(values.last().unwrap()["file"]["length"], bytes.len());
                                "ready"
                            }
                            _ => unreachable!(),
                        });
                    } else {
                        steps.push("candidate");
                    }
                    assert_eq!(saved(&f).phase(), Phase::Prepared);
                }
            }
            file.sync_all()
        })
        .unwrap();
    assert_eq!(
        steps,
        [
            "allocated",
            "created",
            "candidate",
            "ready",
            "directory",
            "clear"
        ]
    );
    assert_eq!(journal.saved().publication_status(), Status::Clear);

    let f = Fixture::new();
    let journal = prepared(&f);
    let _held = stage(&journal, "renamed");
    let pending = tracking(&journal.saved.slot, &journal.lock).unwrap();
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

#[test]
fn abort_after_candidate_fsync_no_longer_blocks_any_metadata_publisher() {
    for action in ["offer", "discard", "complete"] {
        let f = Fixture::new();
        drop(prepared(&f));
        abort_original_at(&f, "body-synced");
        assert_eq!(saved(&f).publication_status(), Status::Recoverable);
        if action == "discard" {
            saved(&f).discard_prepared(&f.intent.reconcile_id).unwrap();
            let proposal = saved(&f).next_check_id().unwrap();
            let mut explicit = Journal::prepare_after_discard(
                f.capture().unwrap(),
                &proposal,
                &f.intent.reconcile_id,
            )
            .unwrap();
            assert_eq!(explicit.saved().phase(), Phase::Prepared);
            explicit.offer().unwrap();
        } else {
            let mut explicit = prepared(&f);
            assert_eq!(
                explicit.offer().unwrap().reconcile_id,
                f.intent.reconcile_id
            );
            drop(explicit);
            if action == "complete" {
                // Existing exact Submitted/drained DTO; no task call is made.
                let mut observed = saved(&f);
                let (query, lookup) = super::super::completion::tests::submitted(&f, &observed);
                assert_eq!(
                    observed.settle_submitted(&query, &lookup, || true).unwrap(),
                    Settlement::Applied
                );
                assert_eq!(observed.phase(), Phase::Completed);
            }
        }
        assert_eq!(saved(&f).publication_status(), Status::Clear);
    }
}

#[test]
fn recovery_syncs_the_namespace_before_retiring_published_tracking() {
    let f = Fixture::new();
    let journal = prepared(&f);
    let _held = stage(&journal, "recorded");
    rename_candidate(&journal.saved.slot); // Deliberately NO directory sync.
    drop(journal);
    let observed = saved(&f);
    let lock = metadata_lock(&observed.slot);
    let inspection = inspect(&observed.slot, &observed.disk, &lock).unwrap();
    let mut order = Vec::new();
    reconcile_with_sync(
        &observed.slot,
        &observed.disk,
        &lock,
        inspection,
        &mut |file, directory| {
            if directory {
                assert!(
                    !read(&lock, BYTES)?.0.is_empty(),
                    "tracking retired before namespace sync"
                );
                order.push("namespace");
            } else {
                assert_eq!(order, ["namespace"]);
                assert!(read(file, BYTES)?.0.is_empty());
                order.push("tracking");
            }
            file.sync_all()
        },
    )
    .unwrap();
    assert_eq!(order, ["namespace", "tracking"]);
    assert_eq!(observed.phase(), Phase::OfferedUnknown);
    assert_eq!(observed.publication_status(), Status::Clear);
}

#[test]
fn any_unterminated_wal_tail_is_uncommitted_at_every_stage() {
    for count in 0..=3 {
        for tail in [
            b"{\"stage\":\"alloc".as_slice(),
            b"\0\0\0\0\0",
            b"garbage\xff\0",
            b"{\"stage\":\"unrecognized\"}",
        ] {
            let f = Fixture::new();
            let journal = prepared(&f);
            let _held = stage(&journal, "recorded");
            let path = f.file.with_extension("reconcile-lock");
            let log = frames(&fs::read(&path).unwrap());
            let mut bytes = encode_frames(&log[..count]);
            bytes.extend_from_slice(tail);
            if count < 2 {
                fs::remove_file(f.file.with_extension("reconcile.tmp")).unwrap();
                if count == 1 {
                    journal
                        .saved
                        .slot
                        .create(&name(&journal.saved.slot), true)
                        .unwrap();
                }
            }
            fs::write(&path, bytes).unwrap();
            assert_eq!(
                journal.saved().publication_status(),
                Status::Recoverable,
                "{count}/{tail:?}"
            );
            recover(&journal.saved.slot, &journal.saved.disk, &journal.lock).unwrap();
            assert_eq!(journal.saved().publication_status(), Status::Clear);
            assert_eq!(journal.saved().phase(), Phase::Prepared);
            assert_eq!(journal.saved().reconcile_id(), f.intent.reconcile_id);
            assert!(!f.file.with_extension("reconcile.tmp").exists());
        }
    }
}

#[test]
fn allocation_without_created_never_adopts_a_nonempty_candidate() {
    let f = Fixture::new();
    let journal = prepared(&f);
    let held = stage(&journal, "recorded");
    let path = f.file.with_extension("reconcile-lock");
    let log = frames(&fs::read(&path).unwrap());
    let allocated = encode_frames(&log[..1]);
    fs::write(&path, &allocated).unwrap();
    let candidate = fs::read(f.file.with_extension("reconcile.tmp")).unwrap();
    assert!(!candidate.is_empty());
    assert_eq!(journal.saved().publication_status(), Status::NeedsReview);
    assert_eq!(
        recover(&journal.saved.slot, &journal.saved.disk, &journal.lock).err(),
        Some(Error::PublicationNeedsReview)
    );
    assert_eq!(fs::read(&path).unwrap(), allocated);
    assert_eq!(
        fs::read(f.file.with_extension("reconcile.tmp")).unwrap(),
        candidate
    );
    assert_eq!(held.metadata().unwrap().nlink(), 1);
    assert_eq!(journal.saved().phase(), Phase::Prepared);
}

#[test]
fn complete_unknown_and_oversize_wal_have_distinct_refusals_and_are_kept() {
    for (bytes, status, error) in [
        (
            b"{\"stage\":\"unrecognized\"}\n".to_vec(),
            Status::NeedsReview,
            Error::PublicationNeedsReview,
        ),
        (
            vec![0; BYTES + 1],
            Status::Oversize,
            Error::PublicationOversize,
        ),
    ] {
        let f = Fixture::new();
        let mut journal = prepared(&f);
        let path = f.file.with_extension("reconcile-lock");
        let original = journal.saved.disk.bytes.clone();
        fs::write(&path, &bytes).unwrap();
        assert_eq!(journal.saved().publication_status(), status);
        // An oversized lock refuses even before acquiring its flock or reading it.
        if status == Status::Oversize {
            assert_eq!(journal.saved.slot.lock(false).err(), Some(error));
        } else {
            assert_eq!(journal.offer().err(), Some(error));
        }
        drop(journal);
        assert_eq!(
            saved(&f).discard_prepared(&f.intent.reconcile_id).err(),
            Some(error)
        );
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(
            fs::read(f.file.with_extension("reconcile")).unwrap(),
            original
        );
        assert!(!f.file.with_extension("reconcile.tmp").exists());
    }
}

#[test]
fn fixture_metadata_lock_waits_for_the_actual_original_flock() {
    let f = Fixture::new();
    let Journal { saved, lock, .. } = prepared(&f);
    let (busy, observed) = std::sync::mpsc::sync_channel(1);
    std::thread::scope(|threads| {
        let waiter = threads.spawn(|| {
            metadata_lock_after_busy(&saved.slot, || {
                let _ = busy.try_send(());
            })
        });
        observed
            .recv_timeout(std::time::Duration::from_secs(8))
            .unwrap();
        assert_eq!(saved.slot.lock(false).err(), Some(Error::Busy));
        drop(lock);
        let acquired = waiter.join().unwrap();
        lock_current(&saved.slot, &acquired).unwrap();
        assert!(read(&acquired, BYTES).unwrap().0.is_empty());
    });
}
