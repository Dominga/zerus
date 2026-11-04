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
        &[(MARKER, case)],
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
    fs::remove_file(f.0.join("hgs")).unwrap();
    fs::write(
        f.0.join("hgs"),
        format!(
            "#!/usr/bin/env python3\n{}\n{}",
            include_str!("fixture.py"),
            include_str!("../fake_hgs.py")
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(f.0.join("hgs"), fs::Permissions::from_mode(0o700)).unwrap();
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
            assert!(invoke(service.clone(), root.clone(), view, Arc::new(()))
                .await
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
