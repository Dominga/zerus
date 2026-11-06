use super::*;
use crate::desktop_service::tests::Fixture;
use std::sync::atomic::AtomicUsize;

fn lease() -> Lease {
    Lease {
        abi: protocol::ABI.into(),
        epoch: uuid::Uuid::new_v4().to_string(),
        lease: uuid::Uuid::new_v4().to_string(),
    }
}
fn spec(n: usize) -> Subscription {
    Subscription {
        topic: topic::ACTIVITY.into(),
        session: Some(zerus_watch_protocol::SessionRef {
            name: format!("codex/project/session-{n}"),
            run_id: Some(format!("run-{n}")),
            conversation_id: None,
        }),
        ..Default::default()
    }
}

#[test]
fn same_generation_readiness_preserves_pending_metadata_and_bounds_retries() {
    let mut hub = Hub {
        lease: Some(lease()),
        ..Hub::default()
    };
    hub.source_state("", 1, false).unwrap();
    assert!(
        hub.next().is_none(),
        "offline sources must not poll metadata"
    );
    hub.source_state("", 1, true).unwrap();
    let (
        _,
        Work::Source {
            computer,
            generation,
            serial,
            part,
        },
    ) = hub.next().unwrap()
    else {
        panic!("source read");
    };
    assert_eq!(part, "hello");
    hub.retry_source(&computer, generation, serial, &part);
    assert!(hub.next().is_none());
    assert!(hub
        .retry_at()
        .is_some_and(|at| at > tokio::time::Instant::now()));
    hub.source_state("", 1, false).unwrap();
    assert!(
        hub.retry_at().is_none(),
        "offline retry timers must not spin"
    );
    hub.source_state("", 2, true).unwrap();
    let (_, Work::Source { generation, .. }) = hub.next().unwrap() else {
        panic!("fresh source read");
    };
    assert_eq!(generation, 2, "new source discards the old retry delay");
}
#[test]
fn source_identity_reads_precede_potentially_slow_detail_follows() {
    let mut hub = Hub {
        lease: Some(lease()),
        ..Hub::default()
    };
    hub.follow("", spec(0)).unwrap();
    hub.source("", 1).unwrap();
    assert!(matches!(hub.next(), Some((_, Work::Source { part, .. })) if part == "hello"));
    assert!(matches!(hub.next(), Some((_, Work::Source { part, .. })) if part == "meta"));
    assert!(matches!(hub.next(), Some((_, Work::Follow { .. }))));
}

#[test]
fn desired_and_retiring_routes_share_a_fixed_bound_and_eof_retires_every_old_id() {
    let mut hub = Hub {
        lease: Some(lease()),
        ..Default::default()
    };
    for n in 0..protocol::DEMANDS {
        hub.follow("", spec(n)).unwrap();
    }
    assert!(hub.follow("", spec(protocol::DEMANDS)).is_err());
    assert!(matches!(hub.next(), Some((_, Work::Follow { .. }))));
    let key = hub.routes.keys().next().unwrap().clone();
    let old = hub.routes[&key].route.id.clone();
    hub.unfollow(&key);
    assert!(
        hub.follow("", spec(protocol::DEMANDS)).is_err(),
        "retirement still owns admission"
    );
    assert!(matches!(hub.next(),Some((_,Work::Unfollow(id))) if id==old));
    assert!(
        hub.follow("", spec(protocol::DEMANDS)).is_err(),
        "in-flight retirement is still charged"
    );
    hub.retired(&old).unwrap();
    hub.follow("", spec(protocol::DEMANDS)).unwrap();
    let before: Vec<_> = hub
        .routes
        .values()
        .map(|entry| entry.route.id.clone())
        .collect();
    hub.disconnect();
    assert!(hub.lease.is_none() && hub.retiring.is_empty());
    assert!(hub
        .routes
        .values()
        .all(|entry| !entry.sent && entry.pending && !before.contains(&entry.route.id)));
}
#[test]
fn queued_detail_and_metadata_cannot_reappear_after_retirement_or_replacement() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let fixture = Fixture::new(json!({}));
        let (service, pump) = fixture.start(false);
        let until = tokio::time::Instant::now() + Duration::from_secs(5);
        while service.snapshot().code != "replay_transport_unavailable" {
            assert!(tokio::time::Instant::now() < until);
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        let seen = Arc::new(AtomicUsize::new(0));
        let observe = seen.clone();
        pump.tap(Box::new(move |_| {
            observe.fetch_add(1, Ordering::Relaxed);
        }));
        let stop = Arc::new(AtomicBool::new(false));
        for replacement in ["view", "status", "lease", "source"] {
            let queued = {
                let mut hub = service.shared.details.lock().unwrap();
                hub.disconnect();
                hub.lease = Some(lease());
                hub.follow("", spec(0)).unwrap();
                hub.source("", 1).unwrap();
                let current = hub.lease.clone().unwrap();
                let key = hub.routes.keys().next().unwrap().clone();
                let entry = &hub.routes[&key];
                let gate = if replacement == "source" {
                    Target::Source {
                        computer: String::new(),
                        generation: 1,
                        serial: hub.sources[""].serial,
                    }
                } else {
                    Target::Route {
                        key: key.clone(),
                        id: entry.route.id.clone(),
                        version: entry.route.version(),
                        stamp: entry.route.stamp(),
                    }
                };
                let queued = Delivery {
                    shared: Arc::downgrade(&service.shared),
                    stopped: stop.clone(),
                    lease: current,
                    gate,
                    changes: vec![Change::Activity {
                        computer: ComputerId::local(),
                        key: key.key.clone(),
                        update: zerus_client_core::ActivityUpdate::State {
                            value: json!({"stale":true}),
                            rev: 9,
                        },
                    }],
                };
                match replacement {
                    "view" => {
                        hub.unfollow(&key);
                        hub.follow("", spec(0)).unwrap();
                    }
                    "status" => {
                        let entry = hub.routes.get_mut(&key).unwrap();
                        let status = json!({"id":entry.route.id,"computer":"","generation":1,"revision":1,"detail_seq":0,
                            "state":"incomplete","code":"source_gap","complete":false});
                        entry.route.apply("detail_status", &status).unwrap();
                    }
                    "lease" => {
                        hub.lease = Some(lease());
                    }
                    "source" => {
                        hub.sources.remove("");
                        hub.source("", 1).unwrap();
                    }
                    _ => unreachable!(),
                }
                queued
            };
            queued.apply(&pump, &|_| {});
            assert_eq!(seen.load(Ordering::Relaxed), 0, "{replacement}");
        }
        service.join().await.unwrap();
    });
}

#[test]
fn recovery_at_capacity_retires_before_rearming_and_has_a_finite_budget() {
    let mut hub = Hub {
        lease: Some(lease()),
        ..Default::default()
    };
    for n in 0..protocol::DEMANDS {
        hub.follow("", spec(n)).unwrap();
    }
    let key = hub.routes.keys().next().unwrap().clone();
    for attempt in 1..=3 {
        let id = hub.routes[&key].route.id.clone();
        hub.recover(&key, &id, "detail_sequence_gap", true).unwrap();
        assert_eq!(hub.capacity(), protocol::DEMANDS);
        assert!(hub.routes[&key].halted);
        assert!(matches!(hub.next(),Some((_,Work::Unfollow(old))) if old==id));
        hub.retired(&id).unwrap();
        assert_ne!(hub.routes[&key].route.id, id);
        assert_eq!(hub.routes[&key].pending, attempt <= 2);
        assert_eq!(hub.routes[&key].halted, attempt > 2);
    }
    let old_id = hub.routes[&key].route.id.clone();
    hub.follow("", spec(0)).unwrap();
    assert!(!hub.routes[&key].halted && hub.routes[&key].pending);
    assert_eq!(hub.routes[&key].recoveries, 0);
    assert_ne!(hub.routes[&key].route.id, old_id);
}

#[test]
fn pushed_hello_resumes_only_the_current_generations_halted_catalog() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let fixture = Fixture::new(json!({}));
        let (service, _) = fixture.start(false);
        crate::desktop_service::tests::eventually("replay transport refused", || {
            service.snapshot().code == "replay_transport_unavailable"
        })
        .await;
        let selected = lease();
        {
            let mut hub = service.shared.details.lock().unwrap();
            hub.lease = Some(selected.clone());
            hub.source("", 2).unwrap();
        }
        {
            let mut dirty = service.shared.dirty.lock().unwrap();
            dirty.sources.insert(
                String::new(),
                SourceVersion {
                    generation: 2,
                    ..SourceVersion::default()
                },
            );
            dirty.halted.insert(String::new());
        }
        let hello: Value = serde_json::from_str(
            include_str!("../../../../core/tests/fixtures/basic.ndjson")
                .lines()
                .next()
                .unwrap(),
        )
        .unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        for generation in [1, 2] {
            frame(&service.shared, &json!({"t":"source","epoch":selected.epoch,
                "value":{"lease":selected.lease,"computer":"","generation":generation,"frame":hello}}), &stop).await.unwrap();
            let dirty = service.shared.dirty.lock().unwrap();
            assert_eq!(dirty.halted.contains(""), generation == 1);
            assert_eq!(dirty.pending.contains(""), generation == 2);
        }
        assert!(service
            .shared
            .details
            .lock()
            .unwrap()
            .local_identity_ready(2));
        service.join().await.unwrap();
    });
}
