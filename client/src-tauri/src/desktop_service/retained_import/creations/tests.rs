use super::super::tests::Fixture;
use super::*;
use std::fs;
fn source(f: &Fixture) {
    zerus_client_core::fsx::write_private_durable(
        &f.root.join("creations.json"),
        br#"{"schema":1,"operations":{},"opaque":"before-load"}"#,
    )
    .unwrap();
    assert!(f.report.inputs.creations.prepare().is_ok());
}
fn run(f: &Fixture, explicit: bool) -> Result<(), &'static str> {
    let charge = f.report.work.admit(Role::Operations).unwrap();
    f.report.run_creation(
        f.relay.clone(),
        "a".repeat(64),
        Arc::new(AtomicBool::new(false)),
        explicit,
        Arc::new(charge),
    )
}
#[test]
fn creation_group_and_qt_publish_separately_with_original_ids_and_historical_provenance() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("success");
    source(&f);
    let id = f.report.inputs.creations.status().capture_id.unwrap();
    assert_eq!(f.run(false), Ok(()));
    let qt = f.report.status()["import_id"].clone();
    assert_eq!(run(&f, false), Ok(()));
    let first = f.report.creation_status();
    assert_eq!(first["import_id"], id);
    assert_ne!(qt, id);
    assert_eq!(first["phase"], "published");
    assert_eq!(f.attempts(), 2);
    let record: Value =
        serde_json::from_slice(&fs::read(f.root.join(format!("record-{id}.json"))).unwrap())
            .unwrap();
    assert_eq!(record["evidence"], "staged");
    assert_eq!(record["automatic_approvals"], false);
    assert_eq!(record["sources"][0]["family"], "next-creations");
    assert_eq!(record["sources"][0]["format"], "creation-json-v1");
    assert_eq!(
        record["sources"][1]["family"],
        "next-creations-preload-provenance"
    );
    // Recovery may rewrite the live file. Both later connections inspect only
    // their original record and never adopt today's new journal bytes.
    zerus_client_core::fsx::write_private_durable(
        &f.root.join("creations.json"),
        br#"{"schema":1,"operations":{},"later":true}"#,
    )
    .unwrap();
    assert_eq!(f.run(false), Ok(()));
    assert_eq!(run(&f, false), Ok(()));
    assert_eq!(f.attempts(), 2);
    assert_eq!(
        f.report.creation_status()["import_record"],
        first["import_record"]
    );
}
#[test]
fn creation_exact_negative_retries_only_explicit_same_capture_and_receipt() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("busy");
    source(&f);
    assert_eq!(run(&f, false), Err("creations_import_handoff_busy"));
    let first = f.report.creation_status();
    f.mode("success");
    assert_eq!(run(&f, false), Err("creations_import_handoff_busy"));
    assert_eq!(f.attempts(), 1);
    assert_eq!(run(&f, true), Ok(()));
    assert_eq!(f.attempts(), 2);
    assert_eq!(f.report.creation_status()["import_id"], first["import_id"]);
    assert_eq!(
        f.report.creation_status()["import_record"],
        first["import_record"]
    );
}
#[test]
fn missing_creation_source_is_not_applicable_without_an_empty_report() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("success");
    assert!(matches!(
        f.report.inputs.creations.prepare(),
        Ok(Prepared::Missing { .. })
    ));
    assert_eq!(run(&f, false), Ok(()));
    assert_eq!(f.report.creation_status()["required"], false);
    assert_eq!(f.report.creation_status()["complete"], true);
    assert_eq!(f.attempts(), 0);
    assert!(!f.root.join("creations-retained-import-v1.json").exists());
}
#[test]
fn callback_lock_contention_is_definitely_before_record_and_explicitly_retryable_after_restart() {
    use std::os::fd::AsRawFd;
    struct Held {
        file: std::fs::File,
        data: PathBuf,
        persisted: u8,
        expected: migration::transport::StagedReceipt,
    }
    impl Drop for Held {
        fn drop(&mut self) {
            let _ = unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
            if self.persisted != 0 {
                // Model a receipt that reached disk despite failed write/readback.
                let j = CreationJournal::new(&self.data, &"a".repeat(64)).unwrap();
                let old = j.load().unwrap().unwrap();
                let mut record = old.record;
                record.expected = Some(self.expected.clone());
                let saved = j.save(record, Some(&old.sha256)).unwrap();
                if self.persisted == 2 {
                    // A different/unknown journal state must never be adopted
                    // as proof that the original attempt can be retried.
                    let mut record = saved.record;
                    record.phase = Phase::Unknown;
                    j.save(record, Some(&saved.sha256)).unwrap();
                } else if self.persisted >= 3 {
                    // Each is well-shaped metadata, but not the SAME attempt
                    // and receipt that this callback was preparing.
                    let mut record = saved.record;
                    match self.persisted {
                        3 => record.attempt += 1,
                        4 => record.created_at += 1,
                        5 => {
                            record.expected.as_mut().unwrap().import_record.sha256 = "e".repeat(64)
                        }
                        _ => unreachable!(),
                    }
                    assert!(record.valid());
                    zerus_client_core::fsx::write_private_durable(
                        &self.data.join("creations-retained-import-v1.json"),
                        &serde_json::to_vec(&record).unwrap(),
                    )
                    .unwrap();
                }
            }
        }
    }
    let _process = crate::PROCESS_TREE.read().unwrap();
    for persisted in 0..6 {
        let f = Fixture::new("success");
        source(&f);
        let id = f.report.inputs.creations.status().capture_id.unwrap();
        let data = f.root.clone();
        let lock = data.join("creations-retained-import-v1.lock");
        f.report.creation_state().before_record = Some(Box::new(move |expected| {
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(lock)
                .unwrap();
            assert_eq!(
                unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
                0
            );
            Box::new(Held {
                file,
                data,
                persisted,
                expected: expected.clone(),
            })
        }));
        assert_eq!(
            run(&f, false),
            Err(if persisted >= 2 {
                "creations_import_record_unavailable"
            } else {
                "creations_import_before_record"
            })
        );
        assert_eq!(f.attempts(), 1);
        assert!(!f.root.join("committed.json").exists());
        assert!(!f.root.join(format!("record-{id}.json")).exists());
        let saved = CreationJournal::new(&f.root, &"a".repeat(64))
            .unwrap()
            .load()
            .unwrap()
            .unwrap()
            .record;
        assert_eq!(
            saved.phase,
            if persisted == 2 {
                Phase::Unknown
            } else if persisted >= 3 {
                Phase::Attempted
            } else {
                Phase::BeforeRecord
            }
        );
        assert_eq!(saved.expected.is_some(), persisted != 0);
        assert_eq!(saved.import_id, id);
        let restarted = Report::new(f.report.inputs.clone());
        for explicit in [false, true] {
            let charge = restarted.work.admit(Role::Operations).unwrap();
            let result = restarted.run_creation(
                f.relay.clone(),
                "a".repeat(64),
                Arc::new(AtomicBool::new(false)),
                explicit,
                Arc::new(charge),
            );
            assert_eq!(
                result,
                if persisted >= 2 {
                    Err("creations_import_original_result_unconfirmed")
                } else if explicit {
                    Ok(())
                } else {
                    Err("creations_import_before_record")
                }
            );
        }
        let current = restarted.creation_state().stored.clone().unwrap().record;
        assert_eq!(
            current.phase,
            if persisted == 2 {
                Phase::Unknown
            } else if persisted >= 3 {
                Phase::Attempted
            } else {
                Phase::Published
            }
        );
        assert_eq!(
            current.attempt,
            if persisted < 2 { 2 } else { saved.attempt }
        );
        assert_eq!(current.import_id, saved.import_id);
        assert_eq!(current.source, saved.source);
        assert_eq!(current.created_at, saved.created_at);
        assert_eq!(f.attempts(), if persisted >= 2 { 1 } else { 2 });
    }
}

#[test]
fn a_source_changed_inside_the_callback_is_quarantined_without_a_commit_record() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("success");
    source(&f);
    let id = f.report.inputs.creations.status().capture_id.unwrap();
    let path = f.root.join("creation-preload-v1/original.bin");
    let changed = b"changed inside the final callback";
    let replace = path.clone();
    f.report.creation_state().before_record = Some(Box::new(move |_| {
        zerus_client_core::fsx::write_private_durable(&replace, changed).unwrap();
        Box::new(())
    }));
    assert_eq!(
        run(&f, false),
        Err("creations_import_source_kept_for_review")
    );
    assert_eq!(f.report.creation_status()["phase"], "quarantined");
    assert_eq!(f.report.creation_status()["import_id"], id);
    assert_eq!(fs::read(path).unwrap(), changed);
    assert!(!f.root.join("committed.json").exists());
    assert!(!f.root.join(format!("record-{id}.json")).exists());
    assert_eq!(
        run(&f, true),
        Err("creations_import_source_kept_for_review")
    );
    assert_eq!(f.attempts(), 1);
}

#[test]
fn callback_failure_then_unreadable_reconciliation_never_replays_after_metadata_recovers() {
    use std::os::{fd::AsRawFd, unix::fs::PermissionsExt};
    struct Held {
        file: std::fs::File,
        journal: PathBuf,
    }
    impl Drop for Held {
        fn drop(&mut self) {
            let _ = unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
            fs::set_permissions(&self.journal, fs::Permissions::from_mode(0o644)).unwrap();
        }
    }
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("success");
    source(&f);
    let id = f.report.inputs.creations.status().capture_id.unwrap();
    let lock = f.root.join("creations-retained-import-v1.lock");
    let journal = f.root.join("creations-retained-import-v1.json");
    let affect = journal.clone();
    f.report.creation_state().before_record = Some(Box::new(move |_| {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(lock)
            .unwrap();
        assert_eq!(
            unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
            0
        );
        Box::new(Held {
            file,
            journal: affect,
        })
    }));
    assert_eq!(run(&f, false), Err("creations_import_record_unavailable"));
    assert!(!f.root.join("committed.json").exists());
    assert!(!f.root.join(format!("record-{id}.json")).exists());
    let bytes = fs::read(&journal).unwrap();
    let record: CreationRecord = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(record.phase, Phase::Attempted);
    assert!(record.expected.is_none());
    assert_eq!(record.import_id, id);
    // Restoring access is not permission to turn an Attempted record into a
    // new publication. The lost durable preparation proof needs review.
    fs::set_permissions(&journal, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        run(&f, true),
        Err("creations_import_original_result_unconfirmed")
    );
    let restarted = Report::new(f.report.inputs.clone());
    let charge = restarted.work.admit(Role::Operations).unwrap();
    assert_eq!(
        restarted.run_creation(
            f.relay.clone(),
            "a".repeat(64),
            Arc::new(AtomicBool::new(false)),
            true,
            Arc::new(charge)
        ),
        Err("creations_import_original_result_unconfirmed")
    );
    assert_eq!(f.attempts(), 1);
    assert_eq!(fs::read(journal).unwrap(), bytes);
}

#[test]
fn restarted_capture_and_report_settle_the_original_unknown_without_republishing() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("commit_lost");
    source(&f);
    assert!(run(&f, false).is_err());
    let original = f.report.creation_status();
    zerus_client_core::fsx::write_private_durable(
        &f.root.join("creations.json"),
        br#"{"schema":1,"operations":{},"later-live-journal":true}"#,
    )
    .unwrap();
    let mut inputs = f.report.inputs.clone();
    inputs.creations = zerus_client_core::migration::preload::Capture::new(f.root.clone());
    assert!(inputs.creations.prepare().is_ok());
    let restarted = Report::new(inputs);
    let charge = restarted.work.admit(Role::Operations).unwrap();
    assert_eq!(
        restarted.run_creation(
            f.relay.clone(),
            "a".repeat(64),
            Arc::new(AtomicBool::new(false)),
            true,
            Arc::new(charge)
        ),
        Ok(())
    );
    assert_eq!(restarted.creation_status()["phase"], "published");
    assert_eq!(
        restarted.creation_status()["import_id"],
        original["import_id"]
    );
    assert_eq!(
        restarted.creation_status()["import_record"],
        original["import_record"]
    );
    assert_eq!(f.attempts(), 1);
}
#[test]
fn creation_lost_or_partial_result_never_resends_and_original_commit_can_settle() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    for mode in ["lost", "partial", "mismatch", "commit_lost"] {
        let f = Fixture::new(mode);
        source(&f);
        assert!(run(&f, false).is_err());
        let first = f.report.creation_status();
        assert_eq!(first["phase"], "unknown");
        f.mode("success");
        let retry = run(&f, true);
        if mode == "commit_lost" {
            assert_eq!(retry, Ok(()));
        } else {
            assert!(retry.is_err());
        }
        assert_eq!(f.attempts(), 1);
        assert_eq!(f.report.creation_status()["import_id"], first["import_id"]);
    }
}
#[test]
fn creation_config_or_snapshot_mismatch_refuses_and_unsupported_is_private_quarantine() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("busy");
    source(&f);
    assert!(run(&f, false).is_err());
    let charge = f.report.work.admit(Role::Operations).unwrap();
    assert!(f
        .report
        .run_creation(
            f.relay.clone(),
            "b".repeat(64),
            Arc::new(AtomicBool::new(false)),
            true,
            Arc::new(charge)
        )
        .is_err());
    assert_eq!(f.attempts(), 1);
    let path = f.root.join("creation-preload-v1/original.bin");
    fs::rename(&path, path.with_extension("old")).unwrap();
    zerus_client_core::fsx::write_private_durable(
        &path,
        br#"{"schema":1,"operations":{},"opaque":"before-load"}"#,
    )
    .unwrap();
    assert!(run(&f, true).is_err());
    assert_eq!(f.attempts(), 1);
    let bad = Fixture::new("success");
    zerus_client_core::fsx::write_private_durable(
        &bad.root.join("creations.json"),
        br#"{"schema":999,"operations":{}}"#,
    )
    .unwrap();
    assert!(bad.report.inputs.creations.prepare().is_err());
    assert_eq!(
        run(&bad, false),
        Err("creations_import_source_kept_for_review")
    );
    assert_eq!(bad.report.creation_status()["phase"], "quarantined");
    assert_eq!(bad.attempts(), 0);
}

#[cfg(target_os = "linux")]
#[test]
fn creation_capture_waits_for_shared_report_admission_without_gating_grant_or_catalog() {
    use crate::desktop_service::tests::eventually;
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let f = Fixture::new("success");
        let (service, _cache) = f.service();
        eventually("connected Qt report before creation capture", || {
            service.snapshot().connected && service.shared.ownership.granted() && f.attempts() == 1
        })
        .await;
        let report = service.shared.retained_import.as_ref().unwrap().clone();
        let held = report.work.admit(Role::Operations).unwrap();
        source(&f);
        assert_eq!(
            report.creation_status()["code"],
            "creations_import_source_resources_pending"
        );
        assert_eq!(f.attempts(), 1);
        assert!(service.snapshot().connected);
        let view = service.ownership_rearm().await;
        assert!(!view.pending && view.code.is_empty(), "{view:?}");
        assert!(service.snapshot().connected);
        assert!(f
            .calls()
            .iter()
            .any(|c| c["value"]["request"]["action"] == "grant"));
        drop(held);
        schedule_current(&service.shared);
        eventually("creation report after original admission releases", || {
            report.creation_status()["phase"] == "published"
        })
        .await;
        assert_eq!(f.attempts(), 2);
        service.stop();
        service.join().await.unwrap();
    });
}
#[cfg(target_os = "linux")]
#[test]
fn creation_sdk_keeps_original_shared_charge_until_reply_completion_after_close() {
    use crate::desktop_service::tests::eventually;
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let f = Fixture::new("success");
        let (service, _cache) = f.service();
        eventually("Qt report connected before held creation SDK", || {
            service.snapshot().connected && f.attempts() == 1
        })
        .await;
        f.mode("hold");
        source(&f);
        eventually(
            "actual creation SDK has the complete original input",
            || f.root.join("holding").exists(),
        )
        .await;
        let report = service.shared.retained_import.as_ref().unwrap().clone();
        let closed = report.close();
        assert_eq!(report.work.snapshot().unwrap().admitted, 1);
        assert!(closed.iter().any(|c| c.wait_until(Instant::now())
            == zerus_client_core::desktop::drain::Observation::Pending));
        assert!(service.snapshot().connected);
        assert!(f.root.join("creation-preload-v1/original.bin").exists());
        arm(
            &service.shared,
            f.relay.clone(),
            "a".repeat(64),
            Arc::new(AtomicBool::new(false)),
            true,
        );
        assert_eq!(f.attempts(), 2);
        fs::write(f.root.join("release"), b"release original only").unwrap();
        eventually(
            "original SDK completion releases the source report charge",
            || report.work.snapshot().unwrap().admitted == 0,
        )
        .await;
        assert_eq!(report.creation_status()["phase"], "published");
        assert_eq!(f.attempts(), 2);
        service.stop();
        service.join().await.unwrap();
    });
}
