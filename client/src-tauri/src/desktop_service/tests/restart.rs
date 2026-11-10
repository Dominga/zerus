//! Driven by test-service-reconnect.py against two original private service
//! children. This is the GUI's real subscriber/pump without a display window.
use super::readiness::Readiness;
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
        let mut readiness = Readiness::new(&fixture, pump.clone());
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
        readiness
            .until("organization topic is ready", |_| {
                service.organization_snapshot().is_some()
            })
            .await;
        readiness
            .until("source row reaches its project", Readiness::grouped)
            .await;
        readiness
            .until("same-generation metadata becomes readable", |_| {
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
        // Every desired topic must be followed on the first lease before the
        // socket gap. Otherwise a topic the detail worker has not reached yet
        // is followed only once, on the new lease, and never shows two IDs.
        readiness
            .until(
                "all four desired topics followed on the first lease",
                |_| {
                    let calls = fixture.calls();
                    ["activity", "processes", "tasks", "organization"]
                        .iter()
                        .all(|topic| {
                            calls
                                .iter()
                                .filter(|c| {
                                    c["value"]["op"] == "detail_follow"
                                        && c["value"]["subscription"]["topic"] == *topic
                                })
                                .filter_map(|c| c["value"]["id"].as_str())
                                .collect::<BTreeSet<_>>()
                                .len()
                                == 1
                        })
                },
            )
            .await;
        fs::write(
            fixture.0.join("runtime-epoch"),
            uuid::Uuid::new_v4().to_string(),
        )
        .unwrap();
        let mut config: Value =
            serde_json::from_slice(&fs::read(fixture.0.join("fixture.json")).unwrap()).unwrap();
        config["produced_status"] = json!({"running":false});
        fs::write(fixture.0.join("fixture.json"), config.to_string()).unwrap();
        fs::write(fixture.0.join("restart-watch"), []).unwrap();
        readiness
            .until(
                "socket gap is transient and retains the stale project",
                |_| {
                    fixture.0.join("produced-status-read").exists()
                        && !service.snapshot().connected
                        && service.snapshot().reconnecting
                        && service.snapshot().code == "service_unavailable"
                        && !pump.online("")
                },
            )
            .await;
        assert!(readiness.checkpoint_grouped());
        assert_eq!(watch_calls(&fixture.calls()), 1);
        config.as_object_mut().unwrap().remove("produced_status");
        fs::write(fixture.0.join("fixture.json"), config.to_string()).unwrap();
        readiness
            .until("automatic replacement watch held before greeting", |_| {
                fixture.0.join("second-watch-held").exists()
            })
            .await;
        assert!(!pump.online(""));
        assert!(
            readiness.checkpoint_grouped(),
            "project placement survives the unavailable organization topic"
        );
        fs::write(fixture.0.join("release-second-watch"), []).unwrap();
        readiness
            .until("all four desired topics re-follow with new IDs", |view| {
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
                    && view.grouped()
            })
            .await;
        readiness.report();
        assert!(
            readiness.checkpoint_grouped(),
            "the final current grouping must still match the GUI snapshot"
        );
        assert_eq!(
            watch_calls(&fixture.calls()),
            2,
            "the held replacement watch must not expire and be silently replaced"
        );
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

#[test]
#[ignore = "run client/scripts/test-service-reconnect.py --scenario resume with private binaries"]
fn real_service_resume_delta_keeps_the_native_replaced_reason() {
    // zerus-uutz: the real journal's revision for the native resume frame
    // reaches the view successor first, with the stopped run `replaced`.
    real_service_replacement(vec![
        ("run:run-1".to_owned(), None),
        ("run:run-2".to_owned(), None),
        ("run:run-1".to_owned(), Some(Some("replaced".to_owned()))),
    ]);
}

#[test]
#[ignore = "run client/scripts/test-service-reconnect.py --scenario paged-fresh with private binaries"]
fn real_service_paged_start_fresh_keeps_the_journaled_replaced_reason() {
    // zerus-ic13: the removal and its new-conversation successor arrive as
    // separate native frames; only the journaled native reason says so.
    real_service_replacement(vec![
        ("run:run-1".to_owned(), None),
        ("run:run-1".to_owned(), Some(Some("replaced".to_owned()))),
        ("run:run-2".to_owned(), None),
    ]);
}

/// The view's changes for the peer while the harness replaces `run:run-1`
/// by `run:run-2` of the same session name.
fn real_service_replacement(expected: Vec<(String, Option<Option<String>>)>) {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let root =
        PathBuf::from(std::env::var_os("ZERUS_TEST_SERVICE_RESTART").expect("private harness"));
    let mark = |name: &str| fs::write(root.join(name), []).unwrap();
    tauri::async_runtime::block_on(async {
        let pump = crate::pump::Pump::default();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let observe = seen.clone();
        pump.tap(Box::new(move |change| match change {
            Change::Upsert { computer, key, .. } if computer.0 == "peer" => {
                observe.lock().unwrap().push((key.clone(), None))
            }
            Change::Remove {
                computer,
                key,
                reason,
            } if computer.0 == "peer" => observe
                .lock()
                .unwrap()
                .push((key.clone(), Some(reason.clone()))),
            _ => {}
        }));
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
        mark("client-connected");
        eventually("stopped run baseline", || {
            pump.row("peer", "run:run-1").is_some() && pump.online("peer")
        })
        .await;
        mark("baseline-ready");
        eventually("new run replaces the stopped run", || {
            pump.row("peer", "run:run-2").is_some() && pump.row("peer", "run:run-1").is_none()
        })
        .await;
        service.join().await.unwrap();
        assert_eq!(*seen.lock().unwrap(), expected);
        mark("client-finished");
    });
}

#[test]
#[ignore = "run client/scripts/test-service-reconnect.py --scenario resync with private binaries"]
fn real_service_native_resync_keeps_a_live_computer_live() {
    // zerus-bkbb: an overflow resync rebuilds an online source's view. A Live
    // computer never shows Probing or Reconnecting for it: every row of that
    // computer would repaint as offline and back (the list-wide blink).
    let _process = crate::PROCESS_TREE.read().unwrap();
    let root =
        PathBuf::from(std::env::var_os("ZERUS_TEST_SERVICE_RESTART").expect("private harness"));
    let mark = |name: &str| fs::write(root.join(name), []).unwrap();
    tauri::async_runtime::block_on(async {
        let pump = crate::pump::Pump::default();
        let statuses = Arc::new(Mutex::new(Vec::new()));
        let observe = statuses.clone();
        pump.tap(Box::new(move |change| {
            if let Change::Computer { computer, status } = change {
                if computer.0 == "peer" {
                    observe.lock().unwrap().push(*status);
                }
            }
        }));
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
        mark("client-connected");
        eventually("live baseline", || {
            pump.live("peer")
                && (1..=3).all(|n| pump.row("peer", &format!("run:run-{n}")).is_some())
        })
        .await;
        statuses.lock().unwrap().clear();
        mark("baseline-ready");
        // Walk the staging view: its hints arrive with the gap and first page.
        tokio::time::sleep(Duration::from_millis(600)).await;
        assert!(pump.live("peer"), "{:?}", statuses.lock().unwrap());
        mark("staging-walked");
        eventually("the completed snapshot's membership", || {
            pump.row("peer", "run:run-4").is_some()
                && pump.row("peer", "run:run-2").is_none()
                && pump.row("peer", "run:run-1").is_some()
        })
        .await;
        service.join().await.unwrap();
        let statuses = statuses.lock().unwrap().clone();
        assert!(
            !statuses.is_empty(),
            "the resync published no catalog status"
        );
        assert!(
            statuses
                .iter()
                .all(|status| *status == ComputerStatus::Live),
            "{statuses:?}"
        );
        mark("client-finished");
    });
}

#[test]
#[ignore = "run client/scripts/test-service-reconnect.py --scenario parity with private binaries"]
fn real_service_matches_native_delivery_of_lifecycle_frames() {
    // zerus-pc6h.1: rename, a stopped run and archive.
    assert_service_parity(
        include_str!("fixtures/lifecycle-parity.ndjson"),
        &["archived"],
    );
}

#[test]
#[ignore = "run client/scripts/test-service-reconnect.py --scenario session-parity with private binaries"]
fn real_service_matches_native_delivery_of_session_lifecycle_frames() {
    // zerus-pc6h.1: launch, pause, resume, fork, Start fresh, forget,
    // terminate and restore from Archive.
    assert_service_parity(
        include_str!("fixtures/session-lifecycle-parity.ndjson"),
        &["replaced", "forgotten", "archived"],
    );
}

/// Recorded native watch frames through native CatalogState and through a
/// real private service and this adapter must leave the same rows after every
/// catalog frame, and the same removals with their reasons (`reasons` must
/// all occur, so a recording cannot silently lose its lifecycle step).
fn assert_service_parity(frames: &str, reasons: &[&str]) {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let root =
        PathBuf::from(std::env::var_os("ZERUS_TEST_SERVICE_RESTART").expect("private harness"));
    let mark = |name: &str| fs::write(root.join(name), []).unwrap();
    let peer = ComputerId("peer".into());
    // Native delivery: the rows after each catalog frame, and all removals.
    let native = crate::pump::Pump::default();
    let mut state = zerus_client_core::CatalogState::new();
    let (mut steps, mut native_removed) = (Vec::new(), Vec::new());
    for (index, line) in frames.lines().enumerate() {
        let entry: Value = serde_json::from_str(line).unwrap();
        let frame: zerus_watch_protocol::Frame =
            serde_json::from_value(entry["frame"].clone()).unwrap();
        for change in state.apply(&peer, &frame) {
            if let Change::Remove { key, reason, .. } = &change {
                native_removed.push((key.clone(), reason.clone()));
            }
            native.push(change);
        }
        let catalog = entry["frame"]["topic"] == "catalog"
            && (entry["frame"]["t"] == "delta" || entry["frame"]["last"] == true);
        if catalog {
            let rows: BTreeMap<String, Value> = state
                .rows()
                .keys()
                .map(|key| {
                    (
                        key.clone(),
                        serde_json::to_value(native.row("peer", key).unwrap()).unwrap(),
                    )
                })
                .collect();
            steps.push((index, rows));
        }
    }
    assert!(steps.len() >= 4);
    for reason in reasons {
        assert!(
            native_removed
                .iter()
                .any(|(_, r)| r.as_deref() == Some(*reason)),
            "{reason}"
        );
    }
    tauri::async_runtime::block_on(async {
        let pump = crate::pump::Pump::default();
        let removed = Arc::new(Mutex::new(Vec::new()));
        let observe = removed.clone();
        pump.tap(Box::new(move |change| {
            if let Change::Remove {
                computer,
                key,
                reason,
            } = change
            {
                if computer.0 == "peer" {
                    observe.lock().unwrap().push((key.clone(), reason.clone()));
                }
            }
        }));
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
        mark("client-connected");
        // Every field the view reads, except the machine label: native mode
        // labels a peer by its hello host, the service by the configured alias.
        let differences = |expected: &BTreeMap<String, Value>| {
            let mut out = Vec::new();
            for (key, native_row) in expected {
                let Some(row) = pump.row("peer", key) else {
                    out.push(format!("{key}: missing"));
                    continue;
                };
                let service_row = serde_json::to_value(row).unwrap();
                for (field, value) in native_row.as_object().unwrap() {
                    if field != "machine" && service_row.get(field) != Some(value) {
                        out.push(format!(
                            "{key}.{field}: native {value} service {}",
                            service_row.get(field).unwrap_or(&Value::Null)
                        ));
                    }
                }
            }
            out
        };
        for (index, expected) in &steps {
            let deadline = Instant::now() + crate::test_wait::window(15);
            loop {
                let stale = native_removed
                    .iter()
                    .any(|(key, _)| !expected.contains_key(key) && pump.row("peer", key).is_some());
                if pump.live("peer") && !stale && differences(expected).is_empty() {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "frame {index}: {:#?}",
                    differences(expected)
                );
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            mark(&format!("parity-{index}"));
        }
        service.join().await.unwrap();
        assert_eq!(*removed.lock().unwrap(), native_removed);
        mark("client-finished");
    });
}
