use super::*;
use crate::desktop_service::{
    frontend::tests::native::{responder, CacheServer},
    tests::{eventually, Fixture},
};
use std::{
    fs,
    process::{Child, Command, Stdio},
};
use zerus_client_core::{operation_recovery::task_start as policy, tasks_write::outbox};
fn private_case(case: &'static str) -> bool {
    const MARKER: &str = "ZERUS_TASK_START_CHECK_CASE";
    if std::env::var(MARKER).as_deref() == Ok(case) {
        return true;
    }
    let _tree = crate::PROCESS_TREE.read().unwrap();
    let invocation = Invocation {
        args: ["--exact", case, "--nocapture", "--test-threads=1"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        stdin: None,
        timeout: Duration::from_secs(40),
        request_id: None,
    };
    match run::run_without(
        &std::env::current_exe().unwrap(),
        &invocation,
        &[
            (MARKER, case),
            (zerus_client_core::desktop::service::MODE_ENV, "1"),
        ],
        &["HGS_SESSION", "HGS_RUN_ID"],
        64 << 10,
    )
    .unwrap()
    {
        zerus_client_core::Exit::Code {
            code,
            stdout,
            stderr,
        } => assert_eq!(code, 0, "{}\n{stderr}", String::from_utf8_lossy(&stdout)),
        other => panic!("{other:?}"),
    }
    false
}
fn fixture(config: Value) -> Fixture {
    let f = Fixture::new(config);
    crate::write_executable(
        &f.0.join("hgs"),
        format!(
            "#!/usr/bin/env python3\n{}\n{}",
            include_str!("fixture.py"),
            include_str!("../fake_hgs.py")
        ),
    );
    f
}
async fn connected(f: &Fixture) -> (Arc<Service>, CacheServer) {
    let r = responder();
    assert!(r.configure_ownership_mode_once(zerus_client_core::desktop::ownership::Mode::Service));
    let cache = CacheServer::start(&f.0, r.clone());
    let (service, _) = f.start_with_responder(true, Some(r));
    eventually("task service connected", || service.snapshot().connected).await;
    (service, cache)
}
fn source(f: &Fixture) -> (std::path::PathBuf, task_start::View) {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../fixtures/desktop-operation-port-v1.json"
    ))
    .unwrap();
    let mut record: Value = serde_json::from_str(fixture["raw_record"].as_str().unwrap()).unwrap();
    record["intent"]["retained_input"] = fixture["retained_input"].clone();
    let binding: policy::Binding = serde_json::from_value(fixture["binding"].clone()).unwrap();
    let root = f.0.join("drafts");
    let p = outbox::path(
        &root.join("task-start"),
        &binding.key.project,
        &binding.key.task,
    );
    use std::os::unix::fs::PermissionsExt;
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    for dir in [&root, &root.join("task-start"), p.parent().unwrap()] {
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).unwrap();
    }
    zerus_client_core::fsx::write_private_atomic(&p, record.to_string().as_bytes(), true).unwrap();
    zerus_client_core::fsx::write_private_atomic(
        &p.with_extension("marker"),
        binding.key.request_id.as_bytes(),
        true,
    )
    .unwrap();
    let view = task_start::open(&root, &binding.key.project, &binding.key.task, None).unwrap();
    (root, view)
}
fn calls(f: &Fixture) -> Vec<Value> {
    fs::read_to_string(f.0.join("task-calls"))
        .unwrap_or_default()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect()
}
async fn invoke(
    service: Arc<Service>,
    root: std::path::PathBuf,
    v: task_start::View,
    held: Arc<dyn Send + Sync>,
) -> Result<task_start::View, String> {
    tauri::async_runtime::spawn_blocking(move || {
        check(
            Some(service),
            &root,
            &v.project,
            &v.task,
            &v.id,
            &v.snapshot,
            held,
        )
    })
    .await
    .unwrap()
}
#[test]
fn lost_offer_reply_uses_native_original_child_then_queries_and_cas_settles() {
    if !private_case("desktop_service::task_start_reconcile::tests::lost_offer_reply_uses_native_original_child_then_queries_and_cas_settles"){return;}
    tauri::async_runtime::block_on(async {
        let f = fixture(json!({"task_lost":true}));
        let (service, _cache) = connected(&f).await;
        let (root, view) = source(&f);
        let answer = invoke(service.clone(), root.clone(), view, Arc::new(()))
            .await
            .unwrap();
        assert_eq!(answer.outcome.as_deref(), Some("submitted"));
        assert!(!answer.pending);
        assert_eq!(answer.phase, "started");
        let log = calls(&f);
        assert_eq!(
            log.iter()
                .filter(|v| v["op"] == "task_start_reconcile")
                .count(),
            1
        );
        assert!(log.iter().any(|v| v["op"] == "task_start_reconcile_result"));
        service.stop();
        service.join().await.unwrap();
    });
}
#[test]
fn unknown_or_missing_reopened_result_never_offers_a_replacement() {
    if !private_case("desktop_service::task_start_reconcile::tests::unknown_or_missing_reopened_result_never_offers_a_replacement"){return;}
    tauri::async_runtime::block_on(async {
        for missing in [false, true] {
            let f = fixture(json!({"task_lost":true,"task_unknown":true,"task_missing":missing}));
            let (service, _cache) = connected(&f).await;
            let (root, view) = source(&f);
            assert!(
                invoke(service.clone(), root.clone(), view.clone(), Arc::new(()))
                    .await
                    .is_err()
            );
            let original = Saved::read(&root, &view.project, &view.task)
                .unwrap()
                .unwrap()
                .reconcile_id()
                .to_owned();
            assert!(
                invoke(service.clone(), root.clone(), view.clone(), Arc::new(()))
                    .await
                    .is_err()
            );
            let current = task_start::open(&root, &view.project, &view.task, None).unwrap();
            assert_ne!(
                crate::task_start::present(&root, current.clone())["unsent_check"],
                true
            );
            assert!(crate::task_start::discard_check(
                &root,
                &current.project,
                &current.task,
                &current.id,
                &current.snapshot,
                &original
            )
            .is_err());
            let log = calls(&f);
            assert_eq!(
                log.iter()
                    .filter(|v| v["op"] == "task_start_reconcile")
                    .count(),
                1
            );
            assert!(log.iter().all(|v| v["reconcile_id"] == original));
            service.stop();
            service.join().await.unwrap();
        }
    });
}
#[test]
fn refused_original_registration_withholds_input_and_keeps_owned_child_cleanup() {
    if !private_case("desktop_service::task_start_reconcile::tests::refused_original_registration_withholds_input_and_keeps_owned_child_cleanup"){return;}
    struct Owned(Child);
    impl Drop for Owned {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    tauri::async_runtime::block_on(async {
        let f = fixture(json!({}));
        let (service, _cache) = connected(&f).await;
        let (root, view) = source(&f);
        let read = Read::new(Some(service.clone())).unwrap();
        let source = Captured::read(
            &root,
            &view.project,
            &view.task,
            &view.id,
            &view.snapshot,
            read.scope.clone(),
        )
        .unwrap();
        let registration = read
            .prepare(source.intent(&uuid::Uuid::new_v4().to_string()).unwrap())
            .unwrap();
        let other = Owned(
            Command::new("/bin/sleep")
                .arg("8")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        assert!(registration.bind_original(&other.0));
        let result = tauri::async_runtime::spawn_blocking(move || {
            read.exchange(Some(registration), Value::Null, false, Arc::new(()))
        })
        .await
        .unwrap();
        assert!(result.is_err());
        assert!(calls(&f).is_empty(), "no private packet was released");
        drop(other);
        service.stop();
        service.join().await.unwrap();
    });
}
#[test]
fn original_pipe_lifetime_retains_the_native_task_write_permit_after_failure() {
    if !private_case("desktop_service::task_start_reconcile::tests::original_pipe_lifetime_retains_the_native_task_write_permit_after_failure"){return;}
    tauri::async_runtime::block_on(async {
        let f = fixture(json!({"task_hold":true,"task_unknown":true}));
        let (service, _cache) = connected(&f).await;
        let (root, view) = source(&f);
        let writes = crate::task_details::TaskWrites::default();
        let held: Arc<dyn Send + Sync> = Arc::new(writes.begin().unwrap());
        assert!(invoke(service.clone(), root, view, held).await.is_err());
        assert!(
            writes.begin().is_err(),
            "logical failure cannot drop the actual pipe charge"
        );
        fs::write(f.0.join("task-release"), b"release").unwrap();
        eventually("task pipe resources joined", || writes.begin().is_ok()).await;
        service.stop();
        service.join().await.unwrap();
    });
}

#[test]
fn older_helper_or_changed_original_image_stays_unoffered() {
    if !private_case("desktop_service::task_start_reconcile::tests::older_helper_or_changed_original_image_stays_unoffered"){return;}
    tauri::async_runtime::block_on(async {
        let f = fixture(json!({"task_no_abi":true}));
        let (service, _cache) = connected(&f).await;
        let (root, view) = source(&f);
        assert!(
            invoke(service.clone(), root.clone(), view.clone(), Arc::new(()))
                .await
                .is_err()
        );
        assert!(calls(&f).is_empty());
        assert!(Saved::read(&root, &view.project, &view.task)
            .unwrap()
            .is_none());
        let read = Read::new(Some(service.clone())).unwrap();
        let bytes = fs::read(f.0.join("hgs")).unwrap();
        zerus_client_core::fsx::write_private_atomic(&f.0.join("hgs"), &bytes, true).unwrap();
        assert!(!read.current());
        assert!(read
            .exchange(None, Value::Null, true, Arc::new(()))
            .is_err());
        assert!(calls(&f).is_empty());
        service.stop();
        service.join().await.unwrap();
    });
}

#[test]
fn larger_native_original_and_oversized_control_input_stay_unoffered() {
    if !private_case("desktop_service::task_start_reconcile::tests::larger_native_original_and_oversized_control_input_stay_unoffered"){return;}
    tauri::async_runtime::block_on(async {
        let f = fixture(json!({}));
        let (service, _cache) = connected(&f).await;
        let (root, view) = source(&f);
        let file = outbox::path(&root.join("task-start"), &view.project, &view.task);
        let mut value: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
        value["opaque_future"] = json!("x".repeat(wire::SOURCE_BYTES));
        zerus_client_core::fsx::write_private_atomic(
            &file,
            &serde_json::to_vec(&value).unwrap(),
            true,
        )
        .unwrap();
        let view = task_start::open(&root, &view.project, &view.task, None).unwrap();
        assert!(
            invoke(service.clone(), root.clone(), view.clone(), Arc::new(()))
                .await
                .is_err()
        );
        assert!(Saved::read(&root, &view.project, &view.task)
            .unwrap()
            .is_none());
        let read = Read::new(Some(service.clone())).unwrap();
        let result = read.exchange(None, json!({"op":"task_start_reconcile_result", "padding":"x".repeat(zerus_watch_protocol::CONTROL_LINE_BYTES)}), false, Arc::new(()));
        assert!(result.is_err());
        assert!(
            calls(&f).is_empty(),
            "oversized input is refused before a helper receives it"
        );
        service.stop();
        service.join().await.unwrap();
    });
}

fn prepare_check(service: Arc<Service>, root: &Path, view: &task_start::View) -> String {
    let read = Read::new(Some(service)).unwrap();
    let source = Captured::read(
        root,
        &view.project,
        &view.task,
        &view.id,
        &view.snapshot,
        read.scope.clone(),
    )
    .unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let journal = Journal::prepare(source, &id).unwrap();
    assert_eq!(journal.saved().phase(), Phase::Prepared);
    id
}
fn later_start(root: &Path, view: &task_start::View) -> task_start::View {
    let file = outbox::path(&root.join("task-start"), &view.project, &view.task);
    let record: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    let released = task_start::release(root, &view.project, &view.task, &view.id, &view.snapshot, |call| {
        assert!(call.args.iter().any(|s| s == "--recover-only"));
        Ok(json!({"schema":1,"id":view.id,"project":view.project,"uuid":view.task,"state":"uncertain","task":{"uuid":view.task,"project":view.project}}).as_object().unwrap().clone())
    }).unwrap();
    assert_eq!(released.phase, "released");
    let mut plan = record["intent"]["plan"].clone();
    let id = uuid::Uuid::new_v4().to_string();
    plan["id"] = json!(id);
    plan["request"]["request_id"] = json!(id);
    let selection = serde_json::from_value(plan["selection"].clone()).unwrap();
    task_start::start(root, &view.project, &view.task, 123, &task_start::Assignment {
        kind:"any".into(), machine:String::new(), session_id:String::new(), name:String::new(),
    }, &selection, &id, |call| match call.args[0].as_str() {
        "get" => Ok(json!({"project":{"id":view.project},"task":{"project":view.project,"uuid":view.task,"stage":"todo","stage_at":123,"writable":true,"target":{"kind":"any"},"history_schema":1,"log":[],"history_next":null}}).as_object().unwrap().clone()),
        "start-plan" => Ok(json!({"plan":plan}).as_object().unwrap().clone()),
        "start-deliver" => Err("fixture: delivery result unavailable".into()),
        other => panic!("unexpected task call: {other}"),
    }).unwrap()
}
#[test]
fn earlier_prepared_check_can_be_confirmed_discarded_before_checking_a_later_start() {
    if !private_case("desktop_service::task_start_reconcile::tests::earlier_prepared_check_can_be_confirmed_discarded_before_checking_a_later_start"){return;}
    tauri::async_runtime::block_on(async {
        let f = fixture(json!({}));
        let (service, _cache) = connected(&f).await;
        let (root, first) = source(&f);
        let check_id = prepare_check(service.clone(), &root, &first);
        // Actual native release and Start paths; no helper offer happened for A.
        let later = later_start(&root, &first);
        assert_ne!(later.id, first.id);
        assert!(later.pending);
        let file = outbox::path(&root.join("task-start"), &later.project, &later.task);
        let bytes = fs::read(&file).unwrap();
        for _ in 0..2 {
            assert_eq!(
                invoke(service.clone(), root.clone(), later.clone(), Arc::new(()))
                    .await
                    .err()
                    .as_deref(),
                Some(STALE_CHECK)
            );
        }
        let displayed = crate::task_start::present(&root, later.clone());
        assert_eq!(displayed["unsent_check"], true);
        assert_eq!(displayed["unsent_check_id"], check_id);
        assert_eq!(displayed["check_available"], false);
        assert_eq!(displayed["check_problem"], STALE_CHECK);
        assert!(crate::task_start::discard_check(
            &root,
            &later.project,
            &later.task,
            &later.id,
            &later.snapshot,
            &uuid::Uuid::new_v4().to_string()
        )
        .is_err());
        assert!(crate::task_start::discard_check(
            &root,
            &later.project,
            &later.task,
            &later.id,
            "stale",
            &check_id
        )
        .is_err());
        crate::task_start::discard_check(
            &root,
            &later.project,
            &later.task,
            &later.id,
            &later.snapshot,
            &check_id,
        )
        .unwrap();
        let saved = Saved::read(&root, &later.project, &later.task)
            .unwrap()
            .unwrap();
        assert_eq!(saved.phase(), Phase::CancelledBeforeOffer);
        assert_eq!(saved.original_id(), first.id);
        assert_eq!(saved.reconcile_id(), check_id);
        assert_eq!(fs::read(&file).unwrap(), bytes);
        assert!(calls(&f).is_empty());
        assert_eq!(
            crate::task_start::present(&root, later.clone())["check_available"],
            true
        );
        let answer = invoke(service.clone(), root.clone(), later, Arc::new(()))
            .await
            .unwrap();
        assert_eq!(answer.outcome.as_deref(), Some("submitted"));
        let log = calls(&f);
        assert_eq!(
            log.iter()
                .filter(|v| v["op"] == "task_start_reconcile")
                .count(),
            1
        );
        assert!(log.iter().all(|v| v["reconcile_id"] != check_id));
        service.stop();
        service.join().await.unwrap();
    });
}
#[test]
fn prepared_check_reuses_its_same_original_reconcile_id() {
    if !private_case("desktop_service::task_start_reconcile::tests::prepared_check_reuses_its_same_original_reconcile_id"){return;}
    tauri::async_runtime::block_on(async {
        let f = fixture(json!({}));
        let (service, _cache) = connected(&f).await;
        let (root, view) = source(&f);
        let id = prepare_check(service.clone(), &root, &view);
        assert!(invoke(service.clone(), root, view, Arc::new(()))
            .await
            .is_ok());
        let log = calls(&f);
        assert_eq!(
            log.iter()
                .filter(|v| v["op"] == "task_start_reconcile")
                .count(),
            1
        );
        assert!(log.iter().all(|v| v["reconcile_id"] == id));
        service.stop();
        service.join().await.unwrap();
    });
}
fn original_lookup(
    read: &Arc<Read>,
    root: &Path,
    view: &task_start::View,
) -> (Saved, wire::Query, wire::Lookup) {
    let source = Captured::read(
        root,
        &view.project,
        &view.task,
        &view.id,
        &view.snapshot,
        read.scope.clone(),
    )
    .unwrap();
    let intent = source.intent(&uuid::Uuid::new_v4().to_string()).unwrap();
    let registration = read.prepare(intent.clone()).unwrap();
    let mut journal = Journal::prepare(source, &intent.reconcile_id).unwrap();
    journal.offer().unwrap();
    let held: Arc<dyn Send + Sync> = Arc::new(Mutex::new(journal));
    read.exchange(Some(registration), Value::Null, false, held)
        .unwrap();
    let saved = Saved::read(root, &view.project, &view.task)
        .unwrap()
        .unwrap();
    let q = saved
        .query(read.scope.clone(), &uuid::Uuid::new_v4().to_string())
        .unwrap();
    let bytes = read
        .exchange(
            None,
            json!({"op":"task_start_reconcile_result","query":q}),
            false,
            Arc::new(()),
        )
        .unwrap();
    let r: wire::Lookup = serde_json::from_slice(&bytes).unwrap();
    assert!(r.matches(&q));
    (saved, q, r)
}
#[test]
fn native_connection_loss_before_settlement_cannot_change_the_source() {
    if !private_case("desktop_service::task_start_reconcile::tests::native_connection_loss_before_settlement_cannot_change_the_source"){return;}
    tauri::async_runtime::block_on(async {
        let f = fixture(json!({}));
        let (service, _cache) = connected(&f).await;
        let (root, view) = source(&f);
        let read = Read::new(Some(service.clone())).unwrap();
        let (mut saved, q, r) = original_lookup(&read, &root, &view);
        let file = outbox::path(&root.join("task-start"), &view.project, &view.task);
        let bytes = fs::read(&file).unwrap();
        read.cancel.store(true, Ordering::Release);
        assert!(!read.current());
        assert!(read.settle(&mut saved, &q, &r).is_err());
        assert_eq!(fs::read(file).unwrap(), bytes);
        assert_eq!(saved.phase(), Phase::OfferedUnknown);
        service.stop();
        service.join().await.unwrap();
    });
}
#[test]
fn completed_view_still_requires_the_original_native_connection() {
    if !private_case("desktop_service::task_start_reconcile::tests::completed_view_still_requires_the_original_native_connection"){return;}
    tauri::async_runtime::block_on(async {
        let f = fixture(json!({}));
        let (service, _cache) = connected(&f).await;
        let (root, view) = source(&f);
        let read = Read::new(Some(service.clone())).unwrap();
        assert!(
            invoke(service.clone(), root.clone(), view.clone(), Arc::new(()))
                .await
                .is_ok()
        );
        read.cancel.store(true, Ordering::Release);
        assert!(!read.current());
        assert!(read
            .completed_view(&root, &view.project, &view.task, &view.id)
            .is_err());
        service.stop();
        service.join().await.unwrap();
    });
}
#[test]
fn submitted_but_undrained_result_is_queried_until_actual_drain() {
    if !private_case("desktop_service::task_start_reconcile::tests::submitted_but_undrained_result_is_queried_until_actual_drain"){return;}
    tauri::async_runtime::block_on(async {
        let f = fixture(json!({"task_undrained_reads":1}));
        let (service, _cache) = connected(&f).await;
        let (root, view) = source(&f);
        assert!(invoke(service.clone(), root, view, Arc::new(()))
            .await
            .is_ok());
        assert_eq!(
            calls(&f)
                .iter()
                .filter(|v| v["op"] == "task_start_reconcile_result")
                .count(),
            2
        );
        service.stop();
        service.join().await.unwrap();
    });
}

#[test]
fn publication_review_and_oversize_are_visible_without_offering_or_discarding() {
    if !private_case("desktop_service::task_start_reconcile::tests::publication_review_and_oversize_are_visible_without_offering_or_discarding") { return; }
    tauri::async_runtime::block_on(async {
        for (wal, problem) in [
            (
                b"{\"stage\":\"unrecognized\"}\n".to_vec(),
                Some(PUBLICATION_REVIEW),
            ),
            (vec![0; 4097], Some(PUBLICATION_OVERSIZE)),
            (b"garbage\0\xff".to_vec(), None),
        ] {
            let f = fixture(json!({}));
            let (service, _cache) = connected(&f).await;
            let (root, view) = source(&f);
            let id = prepare_check(service.clone(), &root, &view);
            let file = outbox::path(&root.join("task-start"), &view.project, &view.task);
            let lock = file.with_extension("reconcile-lock");
            fs::write(&lock, &wal).unwrap();
            let source_bytes = fs::read(&file).unwrap();
            let marker = fs::read(file.with_extension("marker")).unwrap();
            let journal = fs::read(file.with_extension("reconcile")).unwrap();
            let presented = crate::task_start::present(&root, view.clone());
            if let Some(problem) = problem {
                assert_eq!(presented["check_available"], false);
                assert_eq!(presented["check_problem"], problem);
                assert_eq!(
                    invoke(service.clone(), root.clone(), view.clone(), Arc::new(()))
                        .await
                        .err()
                        .unwrap(),
                    problem
                );
                assert_eq!(
                    crate::task_start::discard_check(
                        &root,
                        &view.project,
                        &view.task,
                        &view.id,
                        &view.snapshot,
                        &id
                    )
                    .err()
                    .unwrap(),
                    problem
                );
                assert!(calls(&f).is_empty());
                assert_eq!(fs::read(&lock).unwrap(), wal);
                assert_eq!(fs::read(file.with_extension("reconcile")).unwrap(), journal);
                assert_eq!(fs::read(&file).unwrap(), source_bytes);
                assert_eq!(fs::read(file.with_extension("marker")).unwrap(), marker);
                let saved = Saved::read(&root, &view.project, &view.task)
                    .unwrap()
                    .unwrap();
                assert_eq!(saved.phase(), Phase::Prepared);
                assert_eq!(saved.reconcile_id(), id);
            } else {
                assert_eq!(presented["check_available"], true);
                assert!(presented["check_problem"].is_null());
                let settled = invoke(service.clone(), root.clone(), view, Arc::new(()))
                    .await
                    .unwrap();
                assert_eq!(settled.outcome.as_deref(), Some("submitted"));
                assert_eq!(
                    calls(&f)
                        .iter()
                        .filter(|c| c["op"] == "task_start_reconcile")
                        .count(),
                    1
                );
                assert!(calls(&f).iter().all(|c| c["reconcile_id"] == id));
            }
            service.stop();
            service.join().await.unwrap();
        }
        assert_eq!(
            journal_problem(journal::Error::PublicationNeedsReview),
            PUBLICATION_REVIEW
        );
        assert_eq!(
            journal_problem(journal::Error::PublicationOversize),
            PUBLICATION_OVERSIZE
        );
    });
}
