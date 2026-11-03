//! Driven by test-service-reconnect.py against two original private service
//! children. This is the GUI's real subscriber/pump without a display window.
use super::*;

#[test]
fn reconnect_refollows_every_topic_and_keeps_projects_while_metadata_is_warming() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let fixture = Fixture::new(
            json!({"details":true,"restart_watch":true,"source_warming":true,
            "organization":{"default_project":"ungrouped","projects":[
                {"id":"alpha","name":"Alpha","sessions":["untrusted-caption\ncodex/example/row"]},
                {"id":"ungrouped","name":"Inbox","sessions":[]}]}}),
        );
        let pump = crate::pump::Pump::default();
        let organization = crate::organization::Organization::new(
            pump.clone(),
            fixture.0.join("hgs"),
            false,
            fixture.0.join("layout.json"),
            fixture.0.join("draft.json"),
            true,
        );
        let observer = organization.clone();
        let (sink, rx) = std::sync::mpsc::sync_channel(256);
        let drain = pump.drain_service(rx, move |change| observer.observe(change));
        let service = Service::start(
            fixture.0.join("hgs"),
            pump.clone(),
            sink,
            true,
            drain,
            None,
            vec![],
        );
        let weak = Arc::downgrade(&service);
        organization.set_initial_source(Arc::new(move || weak.upgrade()?.organization_snapshot()));
        for topic in ["activity", "processes", "tasks", "organization"] {
            service.follow_detail("", detail_spec(topic, None)).unwrap();
        }
        let grouped = || {
            pump.resend();
            let batch = pump.take(Duration::ZERO);
            batch
                .grouping
                .is_some_and(|g| g.loaded && g.headers.iter().any(|h| h.id == "alpha"))
                && batch
                    .groups
                    .iter()
                    .any(|g| g.key == "row" && g.group == "alpha")
        };
        eventually("organization topic is ready", || {
            service.organization_snapshot().is_some()
        })
        .await;
        eventually("source row reaches its project", grouped).await;
        eventually("same-generation metadata becomes readable", || {
            pump.machine(&ComputerId::local()).is_some_and(|m| {
                m.machine_id.as_deref() == Some("00000000-0000-4000-8000-000000000009")
            })
        })
        .await;
        assert!(
            fixture
                .calls()
                .iter()
                .filter(|call| call["value"]["op"] == "detail_source"
                    && call["value"]["part"] == "hello")
                .count()
                >= 2
        );
        fs::write(
            fixture.0.join("runtime-epoch"),
            uuid::Uuid::new_v4().to_string(),
        )
        .unwrap();
        fs::write(fixture.0.join("restart-watch"), []).unwrap();
        eventually("automatic replacement watch held before greeting", || {
            fixture.0.join("second-watch-held").exists()
        })
        .await;
        assert!(!pump.online(""));
        assert!(
            grouped(),
            "project placement survives the unavailable organization topic"
        );
        fs::write(fixture.0.join("release-second-watch"), []).unwrap();
        eventually("all four desired topics re-follow with new IDs", || {
            let calls = fixture.calls();
            ["activity", "processes", "tasks", "organization"]
                .iter()
                .all(|topic| {
                    let ids: BTreeSet<_> = calls
                        .iter()
                        .filter(|c| {
                            c["value"]["op"] == "detail_follow"
                                && c["value"]["subscription"]["topic"] == *topic
                        })
                        .filter_map(|c| c["value"]["id"].as_str())
                        .collect();
                    ids.len() == 2
                })
                && service.organization_snapshot().is_some()
                && pump.online("")
                && grouped()
        })
        .await;
        service.join().await.unwrap();
        organization.flush();
    });
}

#[test]
#[ignore = "run client/scripts/test-service-reconnect.py with private binaries"]
fn real_service_restart_retains_catalog_and_recovers_without_retry() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let root =
        PathBuf::from(std::env::var_os("ZERUS_TEST_SERVICE_RESTART").expect("private harness"));
    let mark = |name: &str| fs::write(root.join(name), []).unwrap();
    tauri::async_runtime::block_on(async {
        let pump = crate::pump::Pump::default();
        let (sink, rx) = std::sync::mpsc::sync_channel(256);
        let drain = pump.drain_service(rx, |_| {});
        let service = Service::start(
            root.join("relay"),
            pump.clone(),
            sink,
            true,
            drain,
            None,
            vec![],
        );
        eventually("private service connected", || service.snapshot().connected).await;
        let first_epoch = service.snapshot().epoch.unwrap();
        mark("client-connected");
        eventually("two fresh baseline rows", || {
            pump.row("peer", "run:run-1").is_some()
                && pump.row("peer", "run:run-2").is_some()
                && pump.online("peer")
        })
        .await;
        mark("baseline-ready");
        eventually("service socket loss makes rows stale", || {
            !service.snapshot().connected && !pump.online("peer")
        })
        .await;
        assert!(service.snapshot().reconnecting);
        assert!(pump.row("peer", "run:run-1").is_some());
        assert!(pump.row("peer", "run:run-2").is_some());
        mark("client-offline");
        eventually("new service epoch reconnects automatically", || {
            service.snapshot().connected
                && service
                    .snapshot()
                    .epoch
                    .as_ref()
                    .is_some_and(|epoch| epoch != &first_epoch)
        })
        .await;
        // Greeting/IPC readiness alone cannot declare old membership fresh.
        assert!(pump.row("peer", "run:run-1").is_some());
        assert!(pump.row("peer", "run:run-2").is_some());
        assert!(!pump.online("peer"));
        mark("client-reconnected");
        eventually("new source incomplete baseline selected", || {
            root.join("staging-started").exists()
        })
        .await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(pump.row("peer", "run:run-1").is_some());
        assert!(pump.row("peer", "run:run-2").is_some());
        assert!(!pump.online("peer"));
        mark("staging-retained");
        eventually(
            "complete new membership replaces only obsolete rows",
            || {
                pump.row("peer", "run:run-1").is_some()
                    && pump.row("peer", "run:run-3").is_some()
                    && pump.row("peer", "run:run-2").is_none()
                    && pump.online("peer")
            },
        )
        .await;
        service.join().await.unwrap();
        mark("client-finished");
    });
}
