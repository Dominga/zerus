use super::*;

fn manual(service: &Service) -> bool {
    let snapshot = service.snapshot();
    !snapshot.connected
        && snapshot.code == "manual_refresh_required"
        && snapshot.executable_upgrade == Some(service::ExecutableUpgrade::ManualRefreshRequired)
}

#[test]
fn manual_preflight_is_visible_and_explicit_retry_remains_read_only() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let fixture = Fixture::new(json!({"manual_upgrade":true}));
        let (service, _) = fixture.start(true);
        eventually("manual refresh retained", || manual(&service)).await;
        let original = service.snapshot();
        assert!(service.shared.read_only_reconnect.load(Ordering::Acquire));
        assert_eq!(
            original.epoch.as_deref(),
            Some("00000000-0000-4000-8000-000000000001")
        );
        assert_eq!(watch_calls(&fixture.calls()), 0);
        let before = fixture.calls().len();
        assert!(service
            .check_status("00000000-0000-4000-8000-000000000002")
            .is_err());
        assert_eq!(
            fixture.calls().len(),
            before,
            "changed scope starts no check"
        );
        fs::write(
            fixture.0.join("fixture.json"),
            json!({"manual_upgrade":true,"status_generation":2}).to_string(),
        )
        .unwrap();
        service
            .check_status(original.epoch.as_deref().unwrap())
            .unwrap();
        eventually("explicit status read completed", || {
            fixture
                .calls()
                .iter()
                .filter(|c| c["args"] == json!(["desktop", "status", "--json"]))
                .count()
                == 2
                && manual(&service)
                && service.snapshot().generation == Some(2)
        })
        .await;
        assert_eq!(service.snapshot().epoch, original.epoch);
        fs::write(
            fixture.0.join("runtime-epoch"),
            "00000000-0000-4000-8000-000000000003",
        )
        .unwrap();
        service
            .check_status(original.epoch.as_deref().unwrap())
            .unwrap();
        eventually("changed original is refused", || {
            service.snapshot().code == "original_service_changed"
        })
        .await;
        assert_eq!(service.snapshot().epoch, original.epoch);
        assert_eq!(
            service.snapshot().executable_upgrade,
            Some(service::ExecutableUpgrade::ManualRefreshRequired)
        );
        assert!(service
            .check_status("00000000-0000-4000-8000-000000000003")
            .is_err());
        service.join().await.unwrap();
        assert!(fixture
            .calls()
            .iter()
            .all(|c| c["args"] == json!(["--help"])
                || c["args"] == json!(["desktop", "status", "--json"])));
    });
}

#[test]
fn failed_manual_check_and_automatic_backoff_never_adopt_a_healthy_replacement() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let fixture = Fixture::new(json!({"manual_upgrade":true}));
        let (service, _) = fixture.start(true);
        eventually("original manual service", || manual(&service)).await;
        let original = service.snapshot();
        fs::write(
            fixture.0.join("fixture.json"),
            json!({"produced_status":{"running":false}}).to_string(),
        )
        .unwrap();
        service
            .check_status(original.epoch.as_deref().unwrap())
            .unwrap();
        eventually("original status check failed", || {
            service.snapshot().code == "service_unavailable"
        })
        .await;
        let failed = service.snapshot();

        // Only automatic backoff drives the next read. F is a healthy service
        // at the same configuration; it must never replace the checked E.
        let replacement = "00000000-0000-4000-8000-000000000003";
        fs::write(fixture.0.join("runtime-epoch"), replacement).unwrap();
        fs::write(
            fixture.0.join("fixture.json"),
            json!({"ready":true}).to_string(),
        )
        .unwrap();
        eventually("backoff refuses replacement", || {
            service.snapshot().code == "original_service_changed"
                || watch_calls(&fixture.calls()) != 0
        })
        .await;
        let settled = service.snapshot();
        service.join().await.unwrap();
        let calls = fixture.calls();
        assert_eq!(
            settled.code,
            "original_service_changed",
            "replacement was adopted after failed original check: epoch={:?}, watches={}",
            settled.epoch,
            watch_calls(&calls)
        );
        for snapshot in [failed, settled] {
            assert_eq!(snapshot.epoch, original.epoch);
            assert_eq!(
                snapshot.executable_upgrade,
                Some(service::ExecutableUpgrade::ManualRefreshRequired)
            );
            assert!(!snapshot.connected);
        }
        assert_eq!(watch_calls(&calls), 0);
        assert!(calls.iter().all(|c| c["args"] == json!(["--help"])
            || c["args"] == json!(["desktop", "status", "--json"])));
    });
}

#[test]
fn read_only_preflight_with_a_missing_binding_refuses_before_any_child() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let fixture = Fixture::new(json!({}));
        let (service, _) = fixture.start(false);
        eventually("disabled fixture source", || {
            service.snapshot().code == "replay_transport_unavailable"
        })
        .await;
        service
            .shared
            .read_only_reconnect
            .store(true, Ordering::Release);
        assert_eq!(
            preflight(&service.shared, Arc::new(AtomicBool::new(false))).await,
            Err("original_service_changed")
        );
        service.join().await.unwrap();
        assert!(fixture.calls().is_empty());
    });
}

#[test]
fn watch_refusal_retains_the_original_manual_notice_and_stale_catalog() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    for mode in ["error", "status"] {
        tauri::async_runtime::block_on(async {
            let fixture = Fixture::new(json!({"ready":true,"manual_upgrade_watch":mode}));
            let (service, pump) = fixture.start(true);
            eventually("original catalog published", || {
                pump.row("", "row").is_some()
            })
            .await;
            let original_epoch = service.snapshot().epoch;
            fs::write(fixture.0.join("manual-upgrade"), []).unwrap();
            eventually("manual state after refused watch", || manual(&service)).await;
            assert_eq!(service.snapshot().epoch, original_epoch);
            assert!(pump.row("", "row").is_some());
            assert!(!pump.online(""), "the retained catalog is stale, not live");
            assert_eq!(watch_calls(&fixture.calls()), 1);
            assert!(service.shared.read_only_reconnect.load(Ordering::Acquire));
            let before = fixture.calls().len();
            fs::write(
                fixture.0.join("fixture.json"),
                json!({"ready":true,"manual_upgrade_watch":mode,"status_generation":2}).to_string(),
            )
            .unwrap();
            assert!(service.reconnect(""));
            eventually("read-only refresh of original notice", || {
                fixture.calls().len() >= before + 2
                    && manual(&service)
                    && service.snapshot().generation == Some(2)
            })
            .await;
            assert_eq!(service.snapshot().epoch, original_epoch);
            assert!(pump.row("", "row").is_some());
            service.join().await.unwrap();
            assert_eq!(watch_calls(&fixture.calls()), 1);
            assert!(fixture.calls()[before..]
                .iter()
                .all(|c| c["args"] == json!(["--help"])
                    || c["args"] == json!(["desktop", "status", "--json"])));
        });
    }
}
