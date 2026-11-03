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
fn rerun(f: &Fixture, report: &Arc<Report>, explicit: bool) -> Result<(), &'static str> {
    run_on(report, f.relay.clone(), explicit)
}
fn run_on(
    report: &Arc<Report>,
    relay: Arc<zerus_client_core::desktop::relay::Connection>,
    explicit: bool,
) -> Result<(), &'static str> {
    let charge = report.work.admit(Role::Operations).unwrap();
    report.run_creation(
        relay,
        "a".repeat(64),
        Arc::new(AtomicBool::new(false)),
        explicit,
        Arc::new(charge),
    )
}
fn journal(f: &Fixture) -> CreationJournal {
    CreationJournal::new(&f.root, &"a".repeat(64)).unwrap()
}
fn journal_bytes(f: &Fixture) -> Vec<u8> {
    fs::read(f.root.join("creations-retained-import-v1.json")).unwrap()
}
/// A durable Attempted record at attempt 1 without a receipt, as left by a
/// callback that failed before its gate (v2) or by a version 1 writer.
fn attempted(f: &Fixture, v2: bool) -> CreationStored {
    let Prepared::Snapshot(snapshot) = f.report.inputs.creations.retained().unwrap() else {
        panic!("missing");
    };
    let reference = snapshot.reference().unwrap();
    drop(snapshot);
    let record = if v2 {
        CreationRecord::new_v2("a".repeat(64), reference, 1)
    } else {
        CreationRecord::new("a".repeat(64), reference, 1)
    }
    .unwrap();
    let reserved = journal(f).save(record, None).unwrap();
    advance(f, &reserved, |r| {
        r.attempt = 1;
        r.phase = Phase::Attempted;
    })
}
fn advance(
    f: &Fixture,
    stored: &CreationStored,
    change: impl FnOnce(&mut CreationRecord),
) -> CreationStored {
    let mut record = stored.record.clone();
    change(&mut record);
    journal(f).save(record, Some(&stored.sha256)).unwrap()
}
fn receipt_for(record: &CreationRecord) -> migration::transport::StagedReceipt {
    serde_json::from_value(
        json!({"abi":"desktop-import-v1","published":true,"evidence":"staged",
        "configuration":record.configuration,"import_id":record.import_id,
        "import_record":{"version":1,"sha256":"f".repeat(64)},"bytes":100,"snapshots":2,"pages":1}),
    )
    .unwrap()
}
fn wait_for(label: &str, ready: impl Fn() -> bool) {
    let until = Instant::now() + std::time::Duration::from_secs(30);
    while !ready() {
        assert!(Instant::now() < until, "{label}");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
/// Pause the original callback before its gate until the returned sender fires.
fn pause_before_gate(f: &Fixture) -> (std::sync::mpsc::Receiver<()>, std::sync::mpsc::Sender<()>) {
    let (paused, wait_paused) = std::sync::mpsc::channel();
    let (release, wait_release) = std::sync::mpsc::channel::<()>();
    f.report.creation_state().before_record = Some(Box::new(move |_| {
        paused.send(()).unwrap();
        wait_release.recv().unwrap();
        Box::new(())
    }));
    (wait_paused, release)
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
fn new_records_are_v2_and_their_gate_is_durable_before_the_final_frame() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("hold");
    source(&f);
    let (report, relay) = (f.report.clone(), f.relay.clone());
    let publication = std::thread::spawn(move || run_on(&report, relay, false));
    // The fake child holds only after it has read the final record frame.
    wait_for("the child read the final frame", || {
        f.root.join("holding").exists()
    });
    let gated = journal(&f).load().unwrap().unwrap().record;
    assert_eq!(gated.version, 2);
    assert_eq!(gated.phase, Phase::Attempted);
    assert_eq!((gated.attempt, gated.recorded_attempt), (1, Some(1)));
    assert!(gated.expected.is_some());
    fs::write(f.root.join("release"), b"release").unwrap();
    assert_eq!(publication.join().unwrap(), Ok(()));
    let published = journal(&f).load().unwrap().unwrap().record;
    assert_eq!(published.phase, Phase::Published);
    assert_eq!(
        (published.attempt, published.recorded_attempt),
        (1, Some(1))
    );
    assert_eq!(published.expected, gated.expected);
    assert_eq!(f.attempts(), 1);
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
                let gated = old.record.with_expected(&self.expected).unwrap();
                let saved = j.save(gated, Some(&old.sha256)).unwrap();
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
            // persisted == 3 leaves attempt 2 with recorded_attempt 1: this
            // run refused to adopt it, but that durable counter gap is exactly
            // the proof an explicit Retry may revalidate after the lookup.
            assert_eq!(
                result,
                if persisted == 2 || persisted >= 4 || persisted == 3 && !explicit {
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
            } else if persisted >= 4 {
                Phase::Attempted
            } else {
                Phase::Published
            }
        );
        assert_eq!(
            current.attempt,
            match persisted {
                0 | 1 => 2,
                3 => saved.attempt + 1,
                _ => saved.attempt,
            }
        );
        assert_eq!(current.import_id, saved.import_id);
        assert_eq!(current.source, saved.source);
        assert_eq!(current.created_at, saved.created_at);
        assert_eq!(
            f.attempts(),
            if persisted == 2 || persisted >= 4 {
                1
            } else {
                2
            }
        );
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

// Controls (a) and (f): the callback fails before its gate, then reconciliation
// cannot read the journal. Without a recorded gate the durable counter gap lets
// one explicit Retry recover, also after restart; a gate that reached disk
// stays lookup-only, because its commit gate may have passed.
#[test]
fn a_double_failure_recovers_once_on_explicit_retry_unless_its_gate_was_recorded() {
    use std::os::{fd::AsRawFd, unix::fs::PermissionsExt};
    struct Held {
        file: std::fs::File,
        journal: PathBuf,
        gate: Option<migration::transport::StagedReceipt>,
    }
    impl Drop for Held {
        fn drop(&mut self) {
            let _ = unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
            if let Some(expected) = &self.gate {
                // The gate write reached disk, but its outcome was lost.
                let j =
                    CreationJournal::new(self.journal.parent().unwrap(), &"a".repeat(64)).unwrap();
                let old = j.load().unwrap().unwrap();
                j.save(
                    old.record.with_expected(expected).unwrap(),
                    Some(&old.sha256),
                )
                .unwrap();
            }
            fs::set_permissions(&self.journal, fs::Permissions::from_mode(0o644)).unwrap();
        }
    }
    let _process = crate::PROCESS_TREE.read().unwrap();
    for recorded_gate in [false, true] {
        let f = Fixture::new("success");
        source(&f);
        let id = f.report.inputs.creations.status().capture_id.unwrap();
        let lock = f.root.join("creations-retained-import-v1.lock");
        let path = f.root.join("creations-retained-import-v1.json");
        let affect = path.clone();
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
                journal: affect,
                gate: recorded_gate.then(|| expected.clone()),
            })
        }));
        assert_eq!(run(&f, false), Err("creations_import_record_unavailable"));
        assert!(!f.root.join("committed.json").exists());
        assert!(!f.root.join(format!("record-{id}.json")).exists());
        let bytes = fs::read(&path).unwrap();
        let record: CreationRecord = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(record.version, 2);
        assert_eq!(record.phase, Phase::Attempted);
        assert_eq!(record.attempt, 1);
        assert_eq!(record.recorded_attempt, Some(u32::from(recorded_gate)));
        assert_eq!(record.expected.is_some(), recorded_gate);
        assert_eq!(record.import_id, id);
        // Restored access is not permission: ordinary runs stay lookup-only.
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(
            run(&f, false),
            Err("creations_import_original_result_unconfirmed")
        );
        let restarted = Report::new(f.report.inputs.clone());
        assert_eq!(
            rerun(&f, &restarted, false),
            Err("creations_import_original_result_unconfirmed")
        );
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(f.attempts(), 1);
        let explicit = rerun(&f, &restarted, true);
        if recorded_gate {
            assert_eq!(
                explicit,
                Err("creations_import_original_result_unconfirmed")
            );
            assert_eq!(fs::read(&path).unwrap(), bytes);
            assert_eq!(f.attempts(), 1);
            continue;
        }
        assert_eq!(explicit, Ok(()));
        let recovered = journal(&f).load().unwrap().unwrap().record;
        assert_eq!(recovered.phase, Phase::Published);
        assert_eq!(
            (recovered.attempt, recovered.recorded_attempt),
            (2, Some(2))
        );
        assert_eq!(recovered.import_id, id);
        assert_eq!(recovered.source, record.source);
        assert_eq!(recovered.created_at, record.created_at);
        assert!(f.root.join(format!("record-{id}.json")).exists());
        assert_eq!(f.attempts(), 2);
        // Once published, a later explicit Retry only looks the result up.
        assert_eq!(rerun(&f, &restarted, true), Ok(()));
        assert_eq!(f.attempts(), 2);
    }
}

// Control (b): the original callback is paused before its gate. An explicit
// Retry fences it and publishes once; the released callback cannot record a
// receipt or reach its final frame.
#[test]
fn a_paused_original_callback_cannot_record_after_a_recovery_fence() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("success");
    source(&f);
    let id = f.report.inputs.creations.status().capture_id.unwrap();
    let (paused, release) = pause_before_gate(&f);
    let (report, relay) = (f.report.clone(), f.relay.clone());
    let original = std::thread::spawn(move || run_on(&report, relay, false));
    paused
        .recv_timeout(std::time::Duration::from_secs(30))
        .unwrap();
    let pending = journal(&f).load().unwrap().unwrap().record;
    assert_eq!(pending.phase, Phase::Attempted);
    assert_eq!((pending.attempt, pending.recorded_attempt), (1, Some(0)));
    let restarted = Report::new(f.report.inputs.clone());
    assert_eq!(
        rerun(&f, &restarted, false),
        Err("creations_import_original_result_unconfirmed")
    );
    assert_eq!(rerun(&f, &restarted, true), Ok(()));
    let recovered = journal(&f).load().unwrap().unwrap();
    assert_eq!(recovered.record.phase, Phase::Published);
    assert_eq!(
        (recovered.record.attempt, recovered.record.recorded_attempt),
        (2, Some(2))
    );
    assert_eq!(f.attempts(), 2);
    release.send(()).unwrap();
    assert_eq!(
        original.join().unwrap(),
        Err("creations_import_record_unavailable")
    );
    assert_eq!(
        journal(&f).load().unwrap().unwrap().sha256,
        recovered.sha256
    );
    assert_eq!(f.attempts(), 2);
    let committed: Value =
        serde_json::from_slice(&fs::read(f.root.join(format!("committed-{id}.json"))).unwrap())
            .unwrap();
    assert_eq!(
        committed["import_record"],
        serde_json::to_value(&recovered.record.expected.unwrap().import_record).unwrap()
    );
}

// Control (c): the original gate lands after the recovery's lookup but before
// its fence. The fence compare-and-swap refuses; the original publishes alone.
#[test]
fn a_gate_recorded_before_the_fence_makes_recovery_refuse() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("success");
    source(&f);
    let (paused, release) = pause_before_gate(&f);
    let (report, relay) = (f.report.clone(), f.relay.clone());
    let original = std::thread::spawn(move || run_on(&report, relay, false));
    paused
        .recv_timeout(std::time::Duration::from_secs(30))
        .unwrap();
    let restarted = Report::new(f.report.inputs.clone());
    let root = f.root.clone();
    restarted.creation_state().before_fence = Some(Box::new(move || {
        release.send(()).unwrap();
        let journal = CreationJournal::new(&root, &"a".repeat(64)).unwrap();
        wait_for("the original gate is durable", || {
            journal
                .load()
                .ok()
                .flatten()
                .is_some_and(|s| s.record.recorded_attempt == Some(1))
        });
    }));
    assert_eq!(
        rerun(&f, &restarted, true),
        Err("creations_import_record_unavailable")
    );
    assert_eq!(original.join().unwrap(), Ok(()));
    let published = journal(&f).load().unwrap().unwrap().record;
    assert_eq!(published.phase, Phase::Published);
    assert_eq!(
        (published.attempt, published.recorded_attempt),
        (1, Some(1))
    );
    assert_eq!(f.attempts(), 1);
}

// Control (d): only a version 1 Attempted record without a receipt upgrades,
// on explicit Retry. A legacy receipt or Unknown stays lookup-only, and a
// legacy known negative keeps its version 1 explicit retry.
#[test]
fn only_a_legacy_attempt_without_a_receipt_upgrades_on_explicit_retry() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    for case in [
        "none",
        "receipt",
        "unknown",
        "unknown_receipt",
        "before_record",
    ] {
        let f = Fixture::new("success");
        source(&f);
        let mut stored = attempted(&f, false);
        if case.ends_with("receipt") {
            let expected = receipt_for(&stored.record);
            stored = advance(&f, &stored, |r| *r = r.with_expected(&expected).unwrap());
        }
        if case.starts_with("unknown") {
            stored = advance(&f, &stored, |r| r.phase = Phase::Unknown);
        }
        if case == "before_record" {
            stored = advance(&f, &stored, |r| r.phase = Phase::BeforeRecord);
        }
        assert_eq!(stored.record.version, 1, "{case}");
        let bytes = journal_bytes(&f);
        assert_eq!(
            run(&f, false),
            Err(if case == "before_record" {
                "creations_import_before_record"
            } else {
                "creations_import_original_result_unconfirmed"
            }),
            "{case}"
        );
        assert_eq!(journal_bytes(&f), bytes, "{case}");
        let explicit = run(&f, true);
        let current = journal(&f).load().unwrap().unwrap().record;
        match case {
            "none" | "before_record" => {
                assert_eq!(explicit, Ok(()), "{case}");
                assert_eq!(current.phase, Phase::Published, "{case}");
                assert_eq!(current.attempt, 2, "{case}");
                if case == "none" {
                    assert_eq!((current.version, current.recorded_attempt), (2, Some(2)));
                } else {
                    assert_eq!((current.version, current.recorded_attempt), (1, None));
                }
                assert_eq!(f.attempts(), 1, "{case}");
            }
            _ => {
                assert_eq!(
                    explicit,
                    Err("creations_import_original_result_unconfirmed"),
                    "{case}"
                );
                assert_eq!(journal_bytes(&f), bytes, "{case}");
                assert_eq!(f.attempts(), 0, "{case}");
            }
        }
    }
}

// Control (e): a recoverable attempt is never revalidated against a changed
// source, configuration, attempt cap or malformed metadata.
#[test]
fn recovery_refuses_a_changed_source_configuration_cap_or_metadata() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    // A rewritten retained file cannot be prepared: refused without a write.
    let f = Fixture::new("success");
    source(&f);
    attempted(&f, true);
    let bytes = journal_bytes(&f);
    zerus_client_core::fsx::write_private_durable(
        &f.root.join("creation-preload-v1/original.bin"),
        b"changed retained creation snapshot",
    )
    .unwrap();
    assert_eq!(run(&f, true), Err("creations_import_capture_unavailable"));
    assert_eq!(journal_bytes(&f), bytes);
    assert_eq!(f.attempts(), 0);
    // A valid retained snapshot that differs from the recorded source is
    // quarantined, which also fences the recoverable attempt.
    let f = Fixture::new("success");
    source(&f);
    let Prepared::Snapshot(snapshot) = f.report.inputs.creations.retained().unwrap() else {
        panic!("missing");
    };
    let mut recorded = serde_json::to_value(snapshot.reference().unwrap()).unwrap();
    drop(snapshot);
    recorded["source_sha256"] = json!("e".repeat(64));
    let recorded: zerus_client_core::migration::preload::Reference =
        serde_json::from_value(recorded).unwrap();
    let reserved = journal(&f)
        .save(
            CreationRecord::new_v2("a".repeat(64), recorded, 1).unwrap(),
            None,
        )
        .unwrap();
    let stored = advance(&f, &reserved, |r| {
        r.attempt = 1;
        r.phase = Phase::Attempted;
    });
    assert!(stored.record.preparation_recoverable());
    assert_eq!(run(&f, true), Err("creations_import_source_changed"));
    let quarantined = journal(&f).load().unwrap().unwrap().record;
    assert_eq!(quarantined.phase, Phase::Quarantined);
    assert_eq!(quarantined.attempt, stored.record.attempt);
    assert_eq!(f.attempts(), 0);
    // A different configuration refuses without any write.
    let f = Fixture::new("success");
    source(&f);
    attempted(&f, true);
    let bytes = journal_bytes(&f);
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
    assert_eq!(journal_bytes(&f), bytes);
    assert_eq!(f.attempts(), 0);
    // An exhausted attempt cap and malformed metadata refuse before any fence.
    for case in ["cap", "malformed"] {
        let f = Fixture::new("success");
        source(&f);
        let mut record = attempted(&f, true).record;
        match case {
            "cap" => record.attempt = MAX_ATTEMPTS,
            _ => record.recorded_attempt = Some(record.attempt + 1),
        }
        assert_eq!(record.valid(), case == "cap");
        zerus_client_core::fsx::write_private_durable(
            &f.root.join("creations-retained-import-v1.json"),
            &serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        let bytes = journal_bytes(&f);
        assert_eq!(
            run(&f, true),
            Err("creations_import_record_unavailable"),
            "{case}"
        );
        assert_eq!(journal_bytes(&f), bytes, "{case}");
        assert_eq!(f.attempts(), 0, "{case}");
    }
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
        let report = service.shared.retained_import.as_ref().unwrap().clone();
        // The fake CLI logs its publication before it reads the mode. Hold only
        // after the Qt report has finished, or that report would be held instead.
        eventually("Qt report published before held creation SDK", || {
            let qt = report.status();
            service.snapshot().connected
                && qt["complete"] == true
                && qt["running"] == false
                && f.attempts() == 1
        })
        .await;
        f.mode("hold");
        source(&f);
        eventually(
            "actual creation SDK has the complete original input",
            || f.root.join("holding").exists(),
        )
        .await;
        assert_eq!(
            fs::read_to_string(f.root.join("holding")).unwrap(),
            report.inputs.creations.status().capture_id.unwrap()
        );
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

// Recovery always looks up the original ID first: the import ID is shared by
// attempts. A matching Present resolves Published without a fence or a new
// publication; any other answer stays lookup-only.
#[test]
fn recovery_looks_up_the_original_id_before_any_fence() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    for matching in [true, false] {
        let f = Fixture::new("success");
        source(&f);
        assert_eq!(run(&f, false), Ok(()));
        let mut record = journal(&f).load().unwrap().unwrap().record;
        // A later attempt reserved after this one recorded its gate.
        record.phase = Phase::Attempted;
        record.attempt = 2;
        if !matching {
            record.expected.as_mut().unwrap().import_record.sha256 = "e".repeat(64);
        }
        assert!(record.preparation_recoverable());
        zerus_client_core::fsx::write_private_durable(
            &f.root.join("creations-retained-import-v1.json"),
            &serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        let bytes = journal_bytes(&f);
        let result = run(&f, true);
        if matching {
            assert_eq!(result, Ok(()));
            let current = journal(&f).load().unwrap().unwrap().record;
            assert_eq!(current.phase, Phase::Published);
            assert_eq!((current.attempt, current.recorded_attempt), (2, Some(1)));
        } else {
            assert_eq!(result, Err("creations_import_original_result_unconfirmed"));
            assert_eq!(journal_bytes(&f), bytes);
        }
        assert_eq!(f.attempts(), 1);
    }
}

// The gate is bound to the exact stored bytes its attempt reserved. Any rewrite
// of that record, even an equal one, fences the paused callback: it records
// nothing and sends no final frame. The counter gap then stays recoverable.
#[test]
fn the_gate_is_bound_to_the_exact_bytes_its_attempt_reserved() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("success");
    source(&f);
    let id = f.report.inputs.creations.status().capture_id.unwrap();
    let (paused, release) = pause_before_gate(&f);
    let (report, relay) = (f.report.clone(), f.relay.clone());
    let original = std::thread::spawn(move || run_on(&report, relay, false));
    paused
        .recv_timeout(std::time::Duration::from_secs(30))
        .unwrap();
    let pending = journal(&f).load().unwrap().unwrap();
    zerus_client_core::fsx::write_private_durable(
        &f.root.join("creations-retained-import-v1.json"),
        &serde_json::to_vec_pretty(&pending.record).unwrap(),
    )
    .unwrap();
    let rewritten = journal(&f).load().unwrap().unwrap();
    assert_eq!(rewritten.record, pending.record);
    assert_ne!(rewritten.sha256, pending.sha256);
    release.send(()).unwrap();
    assert_eq!(
        original.join().unwrap(),
        Err("creations_import_record_unavailable")
    );
    assert_eq!(
        journal(&f).load().unwrap().unwrap().sha256,
        rewritten.sha256
    );
    assert!(!f.root.join(format!("record-{id}.json")).exists());
    assert_eq!(f.attempts(), 1);
    assert_eq!(run(&f, true), Ok(()));
    let recovered = journal(&f).load().unwrap().unwrap().record;
    assert_eq!(recovered.phase, Phase::Published);
    assert_eq!(
        (recovered.attempt, recovered.recorded_attempt),
        (2, Some(2))
    );
    assert_eq!(f.attempts(), 2);
}
