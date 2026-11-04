use super::*;
use crate::task_start::capture::tests::Fixture;
use serde_json::json;
use std::{
    fs,
    os::unix::fs::{symlink, PermissionsExt},
};

const NEXT: &str = "00000009-1111-4111-8111-111111111111";
const READ: &str = "0000000a-1111-4111-8111-111111111111";
fn names(f: &Fixture) -> Vec<OsString> {
    let mut names: Vec<_> = fs::read_dir(f.file.parent().unwrap())
        .unwrap()
        .map(|v| v.unwrap().file_name())
        .collect();
    names.sort();
    names
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
fn prepared(f: &Fixture) -> Journal {
    Journal::prepare(f.capture().unwrap(), &f.intent.reconcile_id).unwrap()
}

#[test]
fn missing_read_is_read_only_and_offered_is_durable_before_data_leaves_the_writer() {
    let f = Fixture::new();
    let before = names(&f);
    assert!(Saved::read(
        &f.store,
        &f.intent.binding.key.project,
        &f.intent.binding.key.task
    )
    .unwrap()
    .is_none());
    assert_eq!(names(&f), before);
    let original = fs::read(&f.file).unwrap();
    let mut journal = prepared(&f);
    assert_eq!(saved(&f).phase(), Phase::Prepared);
    assert_eq!(saved(&f).reconcile_id(), f.intent.reconcile_id);
    assert_eq!(
        names(&f).len(),
        before.len() + 2,
        "one bounded record and lock"
    );
    let offered = journal.offer().unwrap();
    let observed = saved(&f);
    assert_eq!(observed.phase(), Phase::OfferedUnknown);
    assert_eq!(observed.disk.record.intent, offered);
    assert_eq!(offered.original.as_bytes(), original);
    assert_eq!(journal.offer().err(), Some(Error::OfferedUnknown));
    let query = observed.query(f.intent.scope.clone(), READ).unwrap();
    let missing = wire::Lookup {
        abi: wire::RESULT_ABI.into(),
        request_id: query.request_id.clone(),
        scope: query.scope.clone(),
        read_only: true,
        found: false,
        attempt: None,
    };
    assert!(missing.matches(&query));
    assert_eq!(journal.offer().err(), Some(Error::OfferedUnknown));
    assert_eq!(fs::read(&f.file).unwrap(), original);
}

#[test]
fn ui_exit_and_absent_outbox_only_reopen_the_same_original_read_identity() {
    let f = Fixture::new();
    let offered = {
        let mut journal = prepared(&f);
        journal.offer().unwrap()
    };
    fs::remove_file(&f.file).unwrap();
    fs::remove_file(&f.marker).unwrap();
    let read = saved(&f);
    assert_eq!(read.phase(), Phase::OfferedUnknown);
    assert_eq!(read.reconcile_id(), offered.reconcile_id);
    let mut scope = f.intent.scope.clone();
    scope.service_epoch = NEXT.into();
    let query = read.query(scope.clone(), READ).unwrap();
    assert_eq!(query.scope, scope);
    assert_eq!(query.reconcile_id, offered.reconcile_id);
    assert_eq!(query.binding, offered.binding);
    assert_eq!(
        read.disk.record.intent.scope, offered.scope,
        "historical data stays original"
    );
    assert!(
        f.capture().is_err(),
        "metadata reading creates no new current source"
    );
    f.write();
    assert_eq!(
        Journal::prepare(f.capture().unwrap(), NEXT).err(),
        Some(Error::Unavailable)
    );
    assert_eq!(
        saved(&f).reconcile_id(),
        offered.reconcile_id,
        "replacement bytes cannot reset it"
    );
}

#[test]
fn prepared_reopen_reuses_the_original_id_with_only_a_fresh_service_scope() {
    let mut f = Fixture::new();
    drop(prepared(&f));
    let bytes = fs::read(&f.file).unwrap();
    f.intent.scope.service_epoch = NEXT.into();
    let mut journal = Journal::prepare(f.capture().unwrap(), READ).unwrap();
    assert_eq!(journal.saved().phase(), Phase::Prepared);
    assert_eq!(journal.saved().reconcile_id(), f.intent.reconcile_id);
    let offered = journal.offer().unwrap();
    assert_eq!(offered.reconcile_id, f.intent.reconcile_id);
    assert_eq!(offered.scope, f.intent.scope);
    assert_eq!(offered.original.as_bytes(), bytes);
    drop(journal);
    let mut reloaded = Journal::prepare(f.capture().unwrap(), READ).unwrap();
    assert_eq!(reloaded.offer().err(), Some(Error::OfferedUnknown));
    assert_eq!(reloaded.saved().reconcile_id(), f.intent.reconcile_id);
}

#[test]
fn another_writer_or_changed_source_cannot_rotate_or_replace_a_saved_original() {
    let mut f = Fixture::new();
    let first = prepared(&f);
    assert_eq!(
        Journal::prepare(f.capture().unwrap(), NEXT).err(),
        Some(Error::Busy)
    );
    let original = fs::read(f.file.with_extension("reconcile")).unwrap();
    drop(first);
    f.change(|v| v["opaque_future"] = json!("changed source"));
    assert_eq!(
        Journal::prepare(f.capture().unwrap(), NEXT).err(),
        Some(Error::Unavailable)
    );
    assert_eq!(
        fs::read(f.file.with_extension("reconcile")).unwrap(),
        original
    );
    assert_eq!(saved(&f).phase(), Phase::Prepared);
}

#[test]
fn sync_failure_returns_no_offer_and_keeps_the_exact_before_or_after_phase() {
    for fail_directory in [false, true] {
        let f = Fixture::new();
        let mut journal = prepared(&f);
        let original = fs::read(&f.file).unwrap();
        let mut steps = Vec::new();
        assert_eq!(
            journal
                .offer_with_sync(|file, directory| {
                    steps.push(directory);
                    if directory == fail_directory {
                        Err(io::Error::other("fixture sync failure"))
                    } else {
                        file.sync_all()
                    }
                })
                .err(),
            Some(Error::PublicationUnknown)
        );
        assert_eq!(
            steps,
            if fail_directory {
                vec![false, true]
            } else {
                vec![false]
            }
        );
        assert_eq!(journal.offer().err(), Some(Error::Unavailable));
        let observed = saved(&f);
        assert_eq!(observed.reconcile_id(), f.intent.reconcile_id);
        assert_eq!(
            observed.phase(),
            if fail_directory {
                Phase::OfferedUnknown
            } else {
                Phase::Prepared
            }
        );
        assert_eq!(fs::read(&f.file).unwrap(), original);
        assert!(names(&f)
            .iter()
            .all(|n| !n.to_string_lossy().ends_with(".tmp")));
        drop(journal);
        let mut reopened = prepared(&f);
        if fail_directory {
            assert_eq!(reopened.offer().err(), Some(Error::OfferedUnknown));
        } else {
            assert_eq!(
                reopened.offer().unwrap().reconcile_id,
                f.intent.reconcile_id
            );
        }
    }
}

#[test]
fn unsafe_malformed_and_replaced_journals_are_never_repaired() {
    for change in [
        "duplicate",
        "field",
        "body",
        "size",
        "symlink",
        "hardlink",
        "mode",
    ] {
        let f = Fixture::new();
        drop(prepared(&f));
        let path = f.file.with_extension("reconcile");
        let bytes = fs::read(&path).unwrap();
        match change {
            "duplicate" => crate::fsx::write_private_atomic(
                &path,
                [b"{\"schema\":1,".as_slice(), &bytes[1..]]
                    .concat()
                    .as_slice(),
                true,
            )
            .unwrap(),
            "field" | "body" => {
                let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                if change == "field" {
                    value["execution_consent"] = json!(true);
                } else {
                    value["intent"]["original"] = json!("{}");
                }
                crate::fsx::write_private_atomic(&path, value.to_string().as_bytes(), true)
                    .unwrap();
            }
            "size" => {
                // Valid JSON: the bound must apply before parsing, rather
                // than be accidentally covered by a JSON syntax refusal.
                let mut oversized = vec![b' '; 96 * 1024 + 1 - bytes.len()];
                oversized.extend_from_slice(&bytes);
                crate::fsx::write_private_atomic(&path, &oversized, true).unwrap();
            }
            "symlink" => {
                let target = f.base.join("journal-target");
                fs::rename(&path, &target).unwrap();
                symlink(target, &path).unwrap();
            }
            "hardlink" => fs::hard_link(&path, f.base.join("journal-alias")).unwrap(),
            "mode" => fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap(),
            _ => unreachable!(),
        }
        let before = fs::read(&path).unwrap();
        assert!(
            Saved::read(
                &f.store,
                &f.intent.binding.key.project,
                &f.intent.binding.key.task
            )
            .is_err(),
            "{change}"
        );
        assert_eq!(fs::read(&path).unwrap(), before);
    }
    let f = Fixture::new();
    let journal = prepared(&f);
    let path = f.file.with_extension("reconcile");
    let bytes = fs::read(&path).unwrap();
    crate::fsx::write_private_atomic(&path, &bytes, true).unwrap();
    assert!(
        !journal.saved().current(),
        "identical bytes do not replace the saved journal object"
    );
}

#[test]
fn journal_and_lock_descriptors_never_pass_to_an_executor() {
    let f = Fixture::new();
    let journal = prepared(&f);
    for file in [&journal.lock, &journal.saved.disk.file] {
        // SAFETY: only queries the retained originals, never a reopened PID.
        let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFD) };
        assert!(flags >= 0 && flags & libc::FD_CLOEXEC != 0);
    }
}

#[test]
fn invalid_or_changed_preparation_creates_no_journal_or_metadata_lock() {
    for bad_id in ["invalid", "00000000-0000-0000-0000-000000000000"] {
        let f = Fixture::new();
        let before = names(&f);
        assert_eq!(
            Journal::prepare(f.capture().unwrap(), bad_id).err(),
            Some(Error::Unavailable)
        );
        assert_eq!(names(&f), before);
    }
    let mut f = Fixture::new();
    let source = f.capture().unwrap();
    f.change(|v| v["opaque_future"] = json!("changed before preparation"));
    let before = names(&f);
    assert_eq!(
        Journal::prepare(source, NEXT).err(),
        Some(Error::Unavailable)
    );
    assert_eq!(names(&f), before);
}

#[test]
fn explicit_discard_only_retires_the_exact_never_offered_metadata() {
    let f = Fixture::new();
    drop(prepared(&f));
    let original = fs::read(&f.file).unwrap();
    assert_eq!(saved(&f).discard_prepared(NEXT), Err(Error::Unavailable));
    assert_eq!(saved(&f).phase(), Phase::Prepared);
    // The original outbox may have changed or gone away. This API discards
    // only confirmed never-offered journal metadata, not that outbox.
    fs::remove_file(&f.file).unwrap();
    saved(&f).discard_prepared(&f.intent.reconcile_id).unwrap();
    assert_eq!(saved(&f).phase(), Phase::CancelledBeforeOffer);
    assert_eq!(saved(&f).reconcile_id(), f.intent.reconcile_id);
    saved(&f).discard_prepared(&f.intent.reconcile_id).unwrap();
    assert!(!f.file.exists());
    f.write();
    assert_eq!(fs::read(&f.file).unwrap(), original);
    assert_eq!(
        Journal::prepare(f.capture().unwrap(), NEXT).err(),
        Some(Error::Cancelled)
    );
    assert_eq!(
        Journal::prepare_after_discard(f.capture().unwrap(), NEXT, READ).err(),
        Some(Error::Unavailable)
    );
    let mut next =
        Journal::prepare_after_discard(f.capture().unwrap(), NEXT, &f.intent.reconcile_id).unwrap();
    assert_eq!(next.saved().reconcile_id(), NEXT);
    assert_eq!(
        next.saved.disk.record.previous_cancelled.as_deref(),
        Some(f.intent.reconcile_id.as_str())
    );
    next.offer().unwrap();
    drop(next);
    let bytes = fs::read(f.file.with_extension("reconcile")).unwrap();
    assert_eq!(saved(&f).discard_prepared(NEXT), Err(Error::OfferedUnknown));
    assert_eq!(fs::read(f.file.with_extension("reconcile")).unwrap(), bytes);
    assert_eq!(fs::read(&f.file).unwrap(), original);
}

#[test]
fn stale_prepared_snapshot_and_missing_lock_never_discard_an_offered_or_unknown_record() {
    let f = Fixture::new();
    let mut journal = prepared(&f);
    let before = saved(&f);
    journal.offer().unwrap();
    drop(journal);
    assert_eq!(
        before.discard_prepared(&f.intent.reconcile_id),
        Err(Error::Unavailable)
    );
    assert_eq!(saved(&f).phase(), Phase::OfferedUnknown);
    let f = Fixture::new();
    drop(prepared(&f));
    let before = names(&f);
    fs::remove_file(f.file.with_extension("reconcile-lock")).unwrap();
    assert_eq!(
        saved(&f).discard_prepared(&f.intent.reconcile_id),
        Err(Error::Unavailable)
    );
    assert_eq!(
        names(&f).len(),
        before.len() - 1,
        "no lock is manufactured to clear uncertainty"
    );
    assert_eq!(saved(&f).phase(), Phase::Prepared);
    assert_eq!(
        Journal::prepare(f.capture().unwrap(), NEXT).err(),
        Some(Error::Unavailable)
    );
}

#[test]
fn missing_journal_beside_its_original_lock_never_creates_another_reconciliation() {
    let f = Fixture::new();
    let mut journal = prepared(&f);
    journal.offer().unwrap();
    drop(journal);
    fs::remove_file(f.file.with_extension("reconcile")).unwrap();
    let before = names(&f);
    assert!(Saved::read(
        &f.store,
        &f.intent.binding.key.project,
        &f.intent.binding.key.task
    )
    .unwrap()
    .is_none());
    assert_eq!(
        Journal::prepare(f.capture().unwrap(), NEXT).err(),
        Some(Error::Unavailable)
    );
    assert_eq!(
        Journal::prepare_after_discard(f.capture().unwrap(), NEXT, &f.intent.reconcile_id).err(),
        Some(Error::Unavailable)
    );
    assert_eq!(names(&f), before);
}

#[test]
fn killing_the_original_private_frontend_process_preserves_offered_unknown() {
    use std::{
        io::Read,
        process::{Child, Command, Stdio},
        time::{Duration, Instant},
    };
    const LEAF: &str = "ZERUS_TEST_TASK_START_JOURNAL_LEAF";
    if let Some(root) = std::env::var_os(LEAF) {
        let intent = wire::test_intent();
        let source = Captured::read(
            Path::new(&root),
            &intent.binding.key.project,
            &intent.binding.key.task,
            &intent.binding.key.request_id,
            &intent.binding.ui_snapshot_sha256,
            intent.scope.clone(),
        )
        .unwrap();
        let mut journal = Journal::prepare(source, &intent.reconcile_id).unwrap();
        journal.offer().unwrap();
        crate::fsx::write_private_durable(&Path::new(&root).join("ready"), b"offered").unwrap();
        // Only this marked leaf waits on its ORIGINAL parent's stdin pipe.
        // It cannot reach the parent spawn branch or start another process.
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
    let f = Fixture::new();
    let mut child = Owned(Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "task_start::capture::journal::tests::killing_the_original_private_frontend_process_preserves_offered_unknown", "--nocapture"])
        .env(LEAF, &f.store).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped())
        .spawn().unwrap());
    let until = Instant::now() + Duration::from_secs(3);
    while !f.store.join("ready").exists() && Instant::now() < until {
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "private leaf exited before durable OfferedUnknown"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(fs::read(f.store.join("ready")).unwrap(), b"offered");
    child.0.kill().unwrap();
    assert!(!child.0.wait().unwrap().success());
    drop(child.0.stdin.take());
    let read = saved(&f);
    assert_eq!(read.phase(), Phase::OfferedUnknown);
    assert_eq!(read.reconcile_id(), f.intent.reconcile_id);
    assert_eq!(
        read.query(f.intent.scope.clone(), READ).unwrap().binding,
        f.intent.binding
    );
    let mut reopened = Journal::prepare(f.capture().unwrap(), NEXT).unwrap();
    assert_eq!(reopened.offer().err(), Some(Error::OfferedUnknown));
    assert_eq!(reopened.saved().reconcile_id(), f.intent.reconcile_id);
}
