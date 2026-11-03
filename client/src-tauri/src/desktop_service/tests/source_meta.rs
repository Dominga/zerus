//! zerus-yw3f: after the initial `detail_source` reads, the service pushes
//! each native Hello and complete `catalog.meta` snapshot as a `source` frame
//! (src/desktop/details/frames.rs). The GUI applies it through the same
//! CatalogState as native mode, so the machine card and the ended launches a
//! creation waits for follow it as native mode's do.
use super::*;

/// The details lease fake_hgs announces in its Hello.
const LEASE: &str = "00000000-0000-4000-8000-000000000003";

/// Queue service frames for the fake's watch stream (its `pending-*` hook).
fn push(fixture: &Fixture, name: &str, frames: Value) {
    let path = fixture.0.join(format!("pending-{name}"));
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, frames.to_string()).unwrap();
    fs::rename(tmp, path).unwrap();
}

fn card(machine: Option<zerus_client_core::MachineMeta>) -> Value {
    serde_json::to_value(machine).unwrap()
}

#[test]
fn a_pushed_catalog_meta_source_frame_updates_the_machine_card_like_native_mode() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let fixture = Fixture::new(json!({"details": true}));
        let (service, pump) = fixture.start(true);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let observe = seen.clone();
        pump.tap(Box::new(move |change| {
            if let Change::Machine { machine, .. } = change {
                observe.lock().unwrap().push(machine.clone());
            }
        }));
        let local = ComputerId::local();
        eventually("the initial Hello and metadata reads", || {
            pump.machine(&local).is_some_and(|m| {
                m.ok == Some(true)
                    && m.machine_id.as_deref() == Some("00000000-0000-4000-8000-000000000009")
            })
        })
        .await;
        let initial = card(pump.machine(&local));
        let reads = |calls: &[Value]| {
            calls
                .iter()
                .filter(|c| c["value"]["op"] == "detail_source")
                .count()
        };
        let before = reads(&fixture.calls());

        // A later native catalog.meta: a launch ended, the host updated hgs.
        let ended = json!({"launch_id": "00000000-0000-4000-8000-0000000000e1",
            "name": "codex/example/new", "code": 1, "output": "launch failed"});
        let meta = json!({"v": 1, "seq": 9, "t": "snapshot", "topic": "catalog.meta", "page": 0,
            "last": true, "cursor": 4, "value": {"host": "metadata-caption", "ok": true,
            "hgs_version": "fixture-2", "peers": [], "metrics": {"platform": "linux"},
            "ended_launches": [ended.clone()]}});
        push(
            &fixture,
            "source-meta",
            json!([["source", {"lease": LEASE, "computer": "", "generation": 1, "frame": meta}]]),
        );
        eventually("the pushed metadata reaches the machine card", || {
            pump.machine(&local).is_some_and(|m| {
                m.hgs_version.as_deref() == Some("fixture-2")
                    && m.ended_launches.as_ref().is_some_and(|e| e.len() == 1)
            })
        })
        .await;
        let pushed = pump.machine(&local).unwrap();
        let launch = &pushed.ended_launches.as_ref().unwrap()[0];
        assert_eq!(
            (
                launch.launch_id.as_str(),
                launch.name.as_str(),
                launch.code,
                launch.output.as_str()
            ),
            (
                "00000000-0000-4000-8000-0000000000e1",
                "codex/example/new",
                Some(1),
                "launch failed"
            )
        );
        // The original Hello's physical identity still wins over metadata.
        assert_eq!(
            pushed.machine_id.as_deref(),
            Some("00000000-0000-4000-8000-000000000009")
        );
        assert_ne!(card(Some(pushed.clone())), initial);
        // Creations follow the Machine change the pump publishes (Creations::observes).
        assert!(seen
            .lock()
            .unwrap()
            .iter()
            .any(|m| m.ended_launches.as_ref().is_some_and(|e| e.len() == 1)));
        // Pushed, not polled: no further detail_source read was needed.
        assert_eq!(reads(&fixture.calls()), before);

        // Native mode: the same Hello and both metadata snapshots, in order.
        let mut native = zerus_client_core::CatalogState::new();
        assert!(
            fixture
                .calls()
                .iter()
                .any(|c| c["value"]["op"] == "detail_source" && c["value"]["part"] == "hello"),
            "the GUI read the source Hello"
        );
        let mut machine = None;
        for frame in [
            json!({"v": 1, "seq": 1, "t": "hello", "machine_id": "00000000-0000-4000-8000-000000000009",
                "host": "untrusted-caption", "hgs_version": "fixture", "stream": "native-stream",
                "heartbeat_ms": 10000, "max_frame_bytes": 1048576,
                "topics": ["activity", "processes", "tasks", "organization"],
                "limits": {"activity_subscriptions": 16, "page_bytes": 1048576, "queue_bytes": 1048576,
                    "activity_tail": 100, "activity_read": 100, "control_line_bytes": 8192}}),
            json!({"v": 1, "seq": 2, "t": "snapshot", "topic": "catalog.meta", "page": 0, "last": true,
                "cursor": 1, "value": {"host": "metadata-caption", "ok": true,
                "machine_id": "00000000-0000-4000-8000-000000000099", "peers": [],
                "metrics": {"platform": "linux"}}}),
            meta,
        ] {
            let frame: zerus_watch_protocol::Frame = serde_json::from_value(frame).unwrap();
            for change in native.apply(&local, &frame) {
                if let Change::Machine { machine: m, .. } = change {
                    machine = Some(m);
                }
            }
        }
        assert_eq!(card(Some(pushed)), card(machine));
        service.join().await.unwrap();
    });
}
