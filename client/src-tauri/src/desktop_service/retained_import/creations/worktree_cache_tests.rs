use super::super::tests::Fixture;
use super::*;
use std::fs;
const GROUP: Group = Group::WorktreeCache;
fn raw() -> Vec<u8> {
    serde_json::to_vec_pretty(&json!({"a".repeat(64):{"state":"ok","sampled_at":1,"worktrees":[],"private":{"original":true}}})).unwrap()
}
fn source(f: &Fixture) {
    zerus_client_core::fsx::write_private_durable(&f.root.join("worktree-snapshots.json"), &raw())
        .unwrap();
    assert!(f.report.inputs.worktree_cache.prepare().is_ok());
}
fn run_on(f: &Fixture, report: &Arc<Report>, explicit: bool) -> Result<(), &'static str> {
    let charge = report.work.admit(Role::Operations).unwrap();
    report.run_group(
        GROUP,
        f.relay.clone(),
        "a".repeat(64),
        Arc::new(AtomicBool::new(false)),
        explicit,
        Arc::new(charge),
    )
}
fn run(f: &Fixture, explicit: bool) -> Result<(), &'static str> {
    run_on(f, &f.report, explicit)
}
fn journal(f: &Fixture) -> CreationJournal {
    CreationJournal::for_group(GROUP, &f.root, &"a".repeat(64)).unwrap()
}
#[test]
fn original_cache_publishes_separately_after_live_writes_and_restart_only_looks_up() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("success");
    source(&f);
    let capture = f.report.inputs.worktree_cache.clone();
    let id = capture.status().capture_id.unwrap();
    let (_, mut live) = capture.worktree_owner().unwrap();
    live.write(&json!({})).unwrap();
    let live_bytes = fs::read(f.root.join("worktree-snapshots.json")).unwrap();
    assert_ne!(live_bytes, raw());
    assert_eq!(f.run(false), Ok(()));
    super::tests::source(&f);
    assert_eq!(super::tests::run(&f, false), Ok(()));
    assert_eq!(run(&f, false), Ok(()));
    let first = f.report.group_status(GROUP);
    assert_eq!(first["import_id"], id);
    assert_ne!(f.report.creation_status()["import_id"], id);
    assert_ne!(f.report.status()["import_id"], id);
    assert_eq!(first["scope"], GROUP.scope());
    assert_eq!(first["phase"], "published");
    let record: Value =
        serde_json::from_slice(&fs::read(f.root.join(format!("record-{id}.json"))).unwrap())
            .unwrap();
    assert_eq!(record["evidence"], "staged");
    assert_eq!(record["automatic_approvals"], false);
    assert_eq!(record["sources"][0]["family"], "next-worktree-snapshots");
    assert_eq!(record["sources"][0]["format"], "worktree-snapshots-json-v1");
    assert_eq!(record["sources"][0]["sha256"], {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(raw()))
    });
    assert_eq!(
        record["sources"][1]["family"],
        "next-worktree-cache-preload-provenance"
    );
    assert_eq!(
        fs::read(f.root.join("worktree-cache-preload-v1/original.bin")).unwrap(),
        raw()
    );
    assert_eq!(
        fs::read(f.root.join("worktree-snapshots.json")).unwrap(),
        live_bytes
    );
    let restarted = Report::new(f.report.inputs.clone());
    assert_eq!(run_on(&f, &restarted, true), Ok(()));
    assert_eq!(f.attempts(), 3);
    assert_eq!(
        restarted.group_status(GROUP)["import_record"],
        first["import_record"]
    );
    assert_eq!(
        restarted.group_status(GROUP)["import_id"],
        first["import_id"]
    );
}
#[test]
fn worktree_cache_lost_partial_and_mismatched_results_never_resend() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    for mode in ["lost", "partial", "mismatch", "commit_lost"] {
        let f = Fixture::new(mode);
        source(&f);
        assert!(run(&f, false).is_err());
        let first = f.report.group_status(GROUP);
        assert_eq!(first["phase"], "unknown");
        f.mode("success");
        let restarted = Report::new(f.report.inputs.clone());
        let result = run_on(&f, &restarted, true);
        assert_eq!(result.is_ok(), mode == "commit_lost");
        assert_eq!(f.attempts(), 1);
        assert_eq!(
            restarted.group_status(GROUP)["import_id"],
            first["import_id"]
        );
    }
}
#[test]
fn worktree_cache_exact_negative_needs_explicit_retry_and_preserves_original_record() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("busy");
    source(&f);
    assert_eq!(run(&f, false), Err("worktree_cache_import_handoff_busy"));
    let first = f.report.group_status(GROUP);
    assert_eq!(first["phase"], "not_committed");
    f.mode("success");
    assert_eq!(run(&f, false), Err("worktree_cache_import_handoff_busy"));
    assert_eq!(f.attempts(), 1);
    assert_eq!(run(&f, true), Ok(()));
    assert_eq!(f.attempts(), 2);
    assert_eq!(
        f.report.group_status(GROUP)["import_id"],
        first["import_id"]
    );
    assert_eq!(
        f.report.group_status(GROUP)["import_record"],
        first["import_record"]
    );
}
#[test]
fn worktree_cache_preframe_recovery_is_explicit_and_recorded_attempt_never_replays() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    for gated in [false, true] {
        let f = Fixture::new("success");
        source(&f);
        let Prepared::Snapshot(snapshot) = f.report.inputs.worktree_cache.retained().unwrap()
        else {
            panic!("missing")
        };
        let j = journal(&f);
        let r = CreationRecord::new_group(GROUP, "a".repeat(64), snapshot.reference().unwrap(), 1)
            .unwrap();
        let first = j.save(r, None).unwrap();
        let mut r = first.record;
        r.phase = Phase::Attempted;
        r.attempt = 1;
        let attempt = j.save(r, Some(&first.sha256)).unwrap();
        if gated {
            let receipt = super::tests::receipt_for(&attempt.record);
            j.save(
                attempt.record.with_expected(&receipt).unwrap(),
                Some(&attempt.sha256),
            )
            .unwrap();
        }
        assert_eq!(
            run(&f, false),
            Err("worktree_cache_import_original_result_unconfirmed")
        );
        assert_eq!(f.attempts(), 0);
        let result = run(&f, true);
        assert_eq!(
            result,
            if gated {
                Err("worktree_cache_import_original_result_unconfirmed")
            } else {
                Ok(())
            }
        );
        assert_eq!(f.attempts(), usize::from(!gated));
        assert_eq!(
            f.report.group_status(GROUP)["import_id"],
            snapshot.reference().unwrap().capture_id()
        );
        assert_eq!(
            j.load().unwrap().unwrap().record.attempt,
            if gated { 1 } else { 2 }
        );
    }
}
#[test]
fn worktree_cache_missing_unsupported_and_over_cap_do_not_become_empty_success() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("success");
    assert!(matches!(
        f.report.inputs.worktree_cache.prepare(),
        Ok(Prepared::Missing { .. })
    ));
    assert_eq!(run(&f, false), Ok(()));
    assert_eq!(f.report.group_status(GROUP)["required"], false);
    assert_eq!(f.report.group_status(GROUP)["complete"], true);
    assert!(!f
        .root
        .join("worktree-cache-retained-import-v1.json")
        .exists());
    assert_eq!(f.attempts(), 0);
    let over = serde_json::to_vec(
        &(0..65)
            .map(|i| {
                (
                    format!("{i:064x}"),
                    json!({"state":"ok","sampled_at":1,"worktrees":[]}),
                )
            })
            .collect::<serde_json::Map<_, _>>(),
    )
    .unwrap();
    for bytes in [b"{bad".to_vec(), b"[]".to_vec(), over] {
        let f = Fixture::new("success");
        zerus_client_core::fsx::write_private_durable(
            &f.root.join("worktree-snapshots.json"),
            &bytes,
        )
        .unwrap();
        assert!(f.report.inputs.worktree_cache.prepare().is_err());
        assert_eq!(
            run(&f, false),
            Err("worktree_cache_import_source_kept_for_review")
        );
        assert_eq!(f.report.group_status(GROUP)["phase"], "quarantined");
        assert_eq!(f.report.group_status(GROUP)["complete"], false);
        assert_eq!(
            fs::read(f.root.join("worktree-snapshots.json")).unwrap(),
            bytes
        );
        assert_eq!(
            fs::read(f.root.join("worktree-cache-preload-v1/original.bin")).unwrap(),
            bytes
        );
        assert_eq!(f.attempts(), 0);
    }
}
#[test]
fn worktree_cache_configuration_and_same_byte_snapshot_replacement_refuse_without_new_attempt() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("busy");
    source(&f);
    assert!(run(&f, false).is_err());
    let before = fs::read(f.root.join("worktree-cache-retained-import-v1.json")).unwrap();
    let charge = f.report.work.admit(Role::Operations).unwrap();
    assert_eq!(
        f.report.run_group(
            GROUP,
            f.relay.clone(),
            "b".repeat(64),
            Arc::new(AtomicBool::new(false)),
            true,
            Arc::new(charge)
        ),
        Err("worktree_cache_import_record_unavailable")
    );
    assert_eq!(
        fs::read(f.root.join("worktree-cache-retained-import-v1.json")).unwrap(),
        before
    );
    let path = f.root.join("worktree-cache-preload-v1/original.bin");
    fs::rename(&path, path.with_extension("old")).unwrap();
    zerus_client_core::fsx::write_private_durable(&path, &raw()).unwrap();
    assert!(run(&f, true).is_err());
    assert_eq!(f.attempts(), 1);
    assert_eq!(
        fs::read(f.root.join("worktree-cache-retained-import-v1.json")).unwrap(),
        before
    );
}
#[cfg(target_os = "linux")]
#[test]
fn worktree_cache_group_waits_for_the_shared_slot_and_never_gates_grant_or_catalog() {
    use crate::desktop_service::tests::eventually;
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let f = Fixture::new("success");
        let (service, _cache) = f.service();
        let report = service.shared.retained_import.as_ref().unwrap().clone();
        eventually("Qt completion before cache capture", || {
            service.snapshot().connected
                && service.shared.ownership.granted()
                && report.status()["complete"] == true
                && report.status()["running"] == false
                && f.attempts() == 1
        })
        .await;
        let held = report.work.admit(Role::Operations).unwrap();
        source(&f);
        assert_eq!(
            report.group_status(GROUP)["code"],
            "worktree_cache_import_source_resources_pending"
        );
        assert!(service.snapshot().connected && service.shared.ownership.granted());
        let view = service.ownership_rearm().await;
        assert!(!view.pending && view.code.is_empty(), "{view:?}");
        assert_eq!(f.attempts(), 1);
        // Later live-file failure pauses cache persistence only. Publication
        // must still read/recheck the historical archive, not today's file.
        zerus_client_core::fsx::write_private_durable(
            &f.root.join("worktree-snapshots.json"),
            b"[]",
        )
        .unwrap();
        report
            .inputs
            .worktree_cache
            .worktree_paused(SourceError::SourceChanged);
        drop(held);
        schedule_current(&service.shared);
        eventually(
            "cache historical report after the same slot releases",
            || report.group_status(GROUP)["phase"] == "published",
        )
        .await;
        assert_eq!(f.attempts(), 2);
        assert_eq!(
            service.snapshot().retained_source_import.unwrap()["worktree_cache_report"]["phase"],
            "published"
        );
        service.stop();
        service.join().await.unwrap();
    });
}

#[test]
fn cache_archive_changed_inside_the_final_callback_is_quarantined_without_a_commit_record() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("success");
    source(&f);
    let original = f.report.inputs.worktree_cache.status().capture_id;
    let path = f.root.join("worktree-cache-preload-v1/original.bin");
    f.report.group_state(GROUP).before_record = Some(Box::new(move |_| {
        zerus_client_core::fsx::write_private_durable(&path, b"{}").unwrap();
        Box::new(())
    }));
    assert_eq!(
        run(&f, false),
        Err("worktree_cache_import_source_kept_for_review")
    );
    assert!(!f.root.join("committed.json").exists());
    let id = original.as_ref().unwrap();
    assert!(!f.root.join(format!("record-{id}.json")).exists());
    let stored = journal(&f).load().unwrap().unwrap();
    assert!(stored.record.expected.is_none());
    assert_eq!(stored.record.recorded_attempt, Some(0));
    assert_eq!(f.report.group_status(GROUP)["phase"], "quarantined");
    assert_eq!(
        run(&f, true),
        Err("worktree_cache_import_source_kept_for_review")
    );
    assert_eq!(journal(&f).load().unwrap().unwrap().record.attempt, 1);
    assert_eq!(f.report.inputs.worktree_cache.status().capture_id, original);
    assert_eq!(
        fs::read(f.root.join("worktree-snapshots.json")).unwrap(),
        raw()
    );
}
