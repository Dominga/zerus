#![cfg(unix)]
use super::*;
use crate::desktop::{
    counts,
    fanout::{Fanout, Queue},
    store::Store,
};
use std::{fs, time::Instant};

struct Fake {
    temp: tempfile::TempDir,
    path: PathBuf,
}
impl Fake {
    fn new(hosts: Value) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("hgs");
        std::os::unix::fs::symlink(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("client/core/tests/support/fake_hgs.py"),
            &path,
        )
        .unwrap();
        fs::write(
            temp.path().join("config.json"),
            json!({"fixtures":temp.path(),"hosts":hosts}).to_string(),
        )
        .unwrap();
        fs::write(temp.path().join("log.ndjson"), "").unwrap();
        Self { temp, path }
    }
    fn start(&self, epoch: &str) -> Runtime {
        let root = self.temp.path().to_owned();
        let factory: source::SpoolFactory =
            Arc::new(move || Ok(Box::new(tempfile::NamedTempFile::new_in(&root)?)));
        Runtime::start_with(
            self.path.clone(),
            epoch,
            factory,
            Timing {
                silence: Duration::from_secs(30),
                stop_grace: Duration::from_millis(100),
                legacy: zerus_client_core::legacy::Intervals::default(),
                ..Timing::scaled(0.001)
            },
        )
        .unwrap()
    }
    fn store(&self, epoch: &str) -> Store {
        let location = Location {
            root: self
                .temp
                .path()
                .canonicalize()
                .unwrap()
                .join("state/desktop"),
            configuration: "a".repeat(64),
        };
        location.prepare().unwrap();
        Store::open(&location, epoch).unwrap()
    }
    fn log(&self) -> Vec<Value> {
        fs::read_to_string(self.temp.path().join("log.ndjson"))
            .unwrap()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }
}
fn hello() -> Value {
    json!({"t":"hello","machine_id":null,"host":"observed-host","hgs_version":"1.56.0","stream":"stream-test",
        "heartbeat_ms":10000,"max_frame_bytes":1048576,"topics":["catalog","catalog.meta"],
        "limits":{"activity_subscriptions":16,"page_bytes":524288,"queue_bytes":4194304,"activity_tail":100,"activity_read":200,"control_line_bytes":65536}})
}
fn row(n: usize) -> Value {
    json!({"key":format!("run:r-{n}"),"row":{"name":format!("codex/example/s-{n}"),
        "run_id":format!("r-{n}"),"conversation_id":format!("c-{n}"),"tracked":true,"phase":"input",
        "activity":"busy","attention_signature":"s"}})
}
fn snapshot(page: usize, last: bool, rows: Vec<Value>) -> Value {
    json!({"t":"snapshot","topic":"catalog","page":page,"last":last,"cursor":1,"items":rows})
}
fn meta(peers: Value) -> Value {
    json!({"t":"snapshot","topic":"catalog.meta","page":0,"last":true,"cursor":1,"value":{"peers":peers}})
}
fn next(runtime: &mut Runtime) -> FleetEvent {
    let until = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(event) = runtime.poll() {
            return event;
        }
        assert!(
            Instant::now() < until,
            "fleet event timeout: {}",
            runtime.status()
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn stop(runtime: &mut Runtime) {
    // The same bound as an event: a loaded macOS runner can take over 5 s.
    let until = Instant::now() + Duration::from_secs(15);
    while !runtime.stop() {
        assert!(
            Instant::now() < until,
            "incomplete shutdown: {}",
            runtime.status()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn pump_until(
    runtime: &mut Runtime,
    store: &mut Store,
    broker: &mut feed::Broker,
    producer: &feed::Producer,
    accept: impl Fn(&feed::Packet) -> bool,
) {
    loop {
        let event = next(runtime);
        broker.apply(store, producer, &event.packet).unwrap();
        if accept(&event.packet) {
            break;
        }
    }
}

#[test]
fn many_sessions_use_one_upstream_and_paged_projection_and_slow_clients_are_isolated() {
    let mut frames = vec![hello()];
    for page in 0..160 {
        frames.push(snapshot(
            page,
            page == 159,
            (page * 128..(page + 1) * 128).map(row).collect(),
        ));
    }
    let fake = Fake::new(json!({"":{"runs":[[{"lines":frames},{"serve":true}]]}}));
    let mut runtime = fake.start("11111111-1111-4111-8111-111111111111");
    let mut store = fake.store("11111111-1111-4111-8111-111111111111");
    let mut broker = feed::Broker::new("11111111-1111-4111-8111-111111111111");
    let producer = broker.claim_internal().unwrap();
    let mut fanout = Fanout::default();
    let (slow, fast) = (Arc::new(Queue::default()), Arc::new(Queue::default()));
    fanout.subscribe(slow.clone());
    fanout.subscribe(fast.clone());
    loop {
        let event = next(&mut runtime);
        if let Some(change) = broker.apply(&mut store, &producer, &event.packet).unwrap() {
            let bytes = super::super::protocol::encode(&change).unwrap();
            fanout.publish(bytes.into());
            assert!(fast.next().is_some());
        }
        if matches!(&event.packet,feed::Packet::Watch{frame,..}if matches!(&frame.body,Body::Snapshot(s)if s.topic==topic::CATALOG && s.last))
        {
            break;
        }
    }
    let count = counts::projection(
        &store.db,
        "11111111-1111-4111-8111-111111111111",
        |c| broker.available(c),
        broker.expected(),
    )
    .unwrap();
    assert_eq!(count["aggregate"]["attention"], 20_480);
    let mut cursor = None;
    let mut total = 0;
    loop {
        let page = zerus_desktop_catalog::page(&store.db, "", cursor.as_ref()).unwrap();
        assert!(page.rows.len() <= 128);
        total += page.rows.len();
        cursor = page.next;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(total, 20_480);
    // An overflow disconnects only its subscriber; no upstream is duplicated.
    for _ in 0..1025 {
        fanout.publish(Arc::from(&b"x"[..]));
        fast.next().unwrap();
    }
    assert!(slow.closed());
    assert_eq!(fanout.len(), 1);
    assert_eq!(runtime.sender.usage().sources, 1);
    assert_eq!(runtime.sender.usage().workers, 1);
    assert_eq!(
        fake.log()
            .iter()
            .filter(|v| v["args"] == json!(["watch", "--json"]))
            .count(),
        1
    );
    stop(&mut runtime);
    broker.stop(&mut store).unwrap();
}

#[test]
fn reconnect_retires_counts_rejects_old_generations_and_replaces_service_epoch() {
    let fake = Fake::new(json!({"":{"runs":[
        [{"lines":[hello(),snapshot(0,true,vec![row(1)])]},{"serve":true,"until_file":"disconnect"}],
        [{"lines":[hello(),snapshot(0,true,vec![row(2)])]},{"serve":true}]
    ]}}));
    let mut runtime = fake.start("11111111-1111-4111-8111-111111111111");
    let mut store = fake.store("11111111-1111-4111-8111-111111111111");
    let mut broker = feed::Broker::new("11111111-1111-4111-8111-111111111111");
    let producer = broker.claim_internal().unwrap();
    pump_until(
        &mut runtime,
        &mut store,
        &mut broker,
        &producer,
        |p| matches!(p,feed::Packet::Watch{frame,..}if matches!(&frame.body,Body::Snapshot(_))),
    );
    let old_identity = runtime.computers[&ComputerId::local()]
        .handle
        .identity()
        .clone();
    let old_wire = runtime.computers[&ComputerId::local()].wire_generation;
    fs::write(fake.temp.path().join("disconnect"), "").unwrap();
    pump_until(&mut runtime, &mut store, &mut broker, &producer, |p| {
        matches!(p, feed::Packet::Offline { .. })
    });
    assert_eq!(
        counts::projection(
            &store.db,
            "11111111-1111-4111-8111-111111111111",
            |c| { broker.available(c) },
            broker.expected()
        )
        .unwrap()["aggregate"]["attention"],
        Value::Null
    );
    pump_until(
        &mut runtime,
        &mut store,
        &mut broker,
        &producer,
        |p| matches!(p,feed::Packet::Watch{frame,..}if matches!(&frame.body,Body::Snapshot(_))),
    );
    let old = source::Event::Frame {
        source: old_identity,
        generation: old_wire,
        frame: Box::new({
            let mut h = hello();
            h["v"] = json!(1);
            h["seq"] = json!(1);
            serde_json::from_value(h).unwrap()
        }),
        received_at: Instant::now(),
    };
    assert!(runtime.convert(old).is_none());
    let page = zerus_desktop_catalog::page(&store.db, "", None).unwrap();
    assert_eq!(page.rows[0].key, "run:r-2");
    stop(&mut runtime);
    broker.stop(&mut store).unwrap();
    drop(store);
    let mut runtime = fake.start("22222222-2222-4222-8222-222222222222");
    let mut store = fake.store("22222222-2222-4222-8222-222222222222");
    let mut broker = feed::Broker::new("22222222-2222-4222-8222-222222222222");
    let producer = broker.claim_internal().unwrap();
    assert_eq!(
        counts::projection(
            &store.db,
            "22222222-2222-4222-8222-222222222222",
            |c| { broker.available(c) },
            broker.expected()
        )
        .unwrap()["aggregate"]["attention"],
        Value::Null
    );
    pump_until(
        &mut runtime,
        &mut store,
        &mut broker,
        &producer,
        |p| matches!(p,feed::Packet::Watch{frame,..}if matches!(&frame.body,Body::Snapshot(_))),
    );
    assert_eq!(
        counts::projection(
            &store.db,
            "22222222-2222-4222-8222-222222222222",
            |c| { broker.available(c) },
            broker.expected()
        )
        .unwrap()["aggregate"]["attention"],
        1
    );
    stop(&mut runtime);
}

#[test]
fn local_metadata_admits_aliases_but_peer_metadata_cannot_and_capacity_is_explicit() {
    let fake = Fake::new(json!({
        "":{"runs":[[{"lines":[hello(),snapshot(0,true,vec![]),meta(json!(["peer"]))]},{"serve":true}]]},
        "peer":{"runs":[[{"lines":[hello(),snapshot(0,true,vec![row(1)]),meta(json!(["foreign"]))]},{"serve":true}]]}
    }));
    let mut runtime = fake.start("epoch");
    loop {
        let event = next(&mut runtime);
        if matches!(&event.packet,feed::Packet::Watch{computer,frame,..}if computer=="peer" && matches!(&frame.body,Body::Snapshot(s)if s.topic==topic::CATALOG_META))
        {
            break;
        }
    }
    assert_eq!(runtime.computers.len(), 2);
    assert!(!runtime
        .computers
        .contains_key(&ComputerId("foreign".into())));
    let too_many: Vec<_> = (0..64).map(|n| json!(format!("peer-{n}"))).collect();
    runtime.set_peers(&too_many);
    assert_eq!(runtime.status()["error"], "fleet_source_capacity");
    assert_eq!(runtime.computers.len(), 2);
    runtime.set_peers(&[]);
    let retirement = next(&mut runtime);
    assert!(matches!(&retirement.packet,feed::Packet::Remove{computer,..}if computer=="peer"));
    assert_eq!(runtime.computers.len(), 1);
    stop(&mut runtime);
}

#[test]
fn bounded_ingress_retains_consumer_permit_and_shutdown_cancels_backpressure() {
    let mut frames = vec![hello()];
    for page in 0..80 {
        let rows = (page * 128..(page + 1) * 128)
            .map(|n| {
                let mut r = row(n);
                r["row"]["summary"] = json!("x".repeat(5000));
                r
            })
            .collect();
        frames.push(snapshot(page, page == 79, rows));
    }
    let fake = Fake::new(json!({"":{"runs":[[{"lines":frames},{"serve":true}]]}}));
    let mut runtime = fake.start("epoch");
    let event = next(&mut runtime);
    let until = Instant::now() + Duration::from_secs(10);
    while runtime.sender.usage().bytes < 12 << 20 {
        assert!(
            Instant::now() < until,
            "source never filled bounded ingress: {}",
            runtime.status()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let u = runtime.sender.usage();
    assert!(u.bytes <= source::INGRESS_BYTES && u.items <= source::INGRESS_ITEMS);
    let began = Instant::now();
    assert!(!runtime.stop());
    assert!(began.elapsed() < Duration::from_millis(100));
    assert!(
        runtime.sender.usage().items >= 1,
        "consumer permit survives receiver close"
    );
    drop(event);
    stop(&mut runtime);
    assert_eq!(runtime.sender.usage().bytes, 0);
}

#[test]
fn rejected_apply_suppresses_queued_frames_until_a_new_generation() {
    let fake = Fake::new(
        json!({"":{"runs":[[{"lines":[hello(),snapshot(0,true,vec![row(1)])]},{"serve":true}]]}}),
    );
    let mut runtime = fake.start("epoch");
    let event = next(&mut runtime);
    runtime.reject(&event);
    assert!(matches!(
        next(&mut runtime).packet,
        feed::Packet::Offline { .. }
    ));
    let first = event.packet.scope().unwrap().1;
    drop(event);
    loop {
        let event = next(&mut runtime);
        assert!(event.packet.scope().unwrap().1 > first);
        if matches!(&event.packet,feed::Packet::Watch{frame,..}if matches!(&frame.body,Body::Snapshot(_)))
        {
            break;
        }
    }
    stop(&mut runtime);
}

#[test]
fn upstream_gap_or_sequence_hole_retires_projection_and_resnapshots_once() {
    for gap in [
        json!({"t":"gap","topic":"catalog","reason":"overflow"}),
        json!({"t":"heartbeat","seq":50,"at":1.0}),
    ] {
        let fake = Fake::new(json!({"":{"runs":[
            [{"lines":[hello(),snapshot(0,true,vec![row(1)]),gap,snapshot(0,true,vec![row(99)])]},{"serve":true}],
            [{"lines":[hello(),snapshot(0,true,vec![row(2)])]},{"serve":true}]
        ]}}));
        let epoch = "11111111-1111-4111-8111-111111111111";
        let mut runtime = fake.start(epoch);
        let mut store = fake.store(epoch);
        let mut broker = feed::Broker::new(epoch);
        let producer = broker.claim_internal().unwrap();
        pump_until(&mut runtime, &mut store, &mut broker, &producer, |p| {
            matches!(p, feed::Packet::Offline { .. })
        });
        assert_eq!(
            counts::projection(&store.db, epoch, |c| broker.available(c), broker.expected())
                .unwrap()["aggregate"]["attention"],
            Value::Null
        );
        pump_until(
            &mut runtime,
            &mut store,
            &mut broker,
            &producer,
            |p| matches!(p,feed::Packet::Watch{frame,..}if matches!(&frame.body,Body::Snapshot(_))),
        );
        assert_eq!(
            zerus_desktop_catalog::page(&store.db, "", None)
                .unwrap()
                .rows[0]
                .key,
            "run:r-2"
        );
        assert_eq!(
            fake.log()
                .iter()
                .filter(|v| v["args"] == json!(["watch", "--json"]))
                .count(),
            2
        );
        stop(&mut runtime);
    }
}

#[test]
fn older_peer_listing_is_staged_in_bounded_pages_on_the_same_scheduler() {
    let fake = Fake::new(json!({
        "":{"runs":[[{"lines":[hello(),snapshot(0,true,vec![]),meta(json!(["older"]))]},{"serve":true}]]},
        "older":{"marker":false}
    }));
    fs::write(
        fake.temp.path().join("ls-older.json"),
        json!({
            "sessions":(0..1000).map(|n|row(n)["row"].clone()).collect::<Vec<_>>(),
            "machine_id":"observed-only"
        })
        .to_string(),
    )
    .unwrap();
    let epoch = "11111111-1111-4111-8111-111111111111";
    let mut runtime = fake.start(epoch);
    let mut store = fake.store(epoch);
    let mut broker = feed::Broker::new(epoch);
    let producer = broker.claim_internal().unwrap();
    let (mut rows, mut pages) = (0, 0);
    loop {
        let event = next(&mut runtime);
        if let feed::Packet::LegacyPage { items, .. } = &event.packet {
            assert!(items.len() <= 128);
            rows += items.len();
            pages += 1;
        }
        broker.apply(&mut store, &producer, &event.packet).unwrap();
        if matches!(event.packet, feed::Packet::LegacyComplete { .. }) {
            break;
        }
    }
    assert_eq!((rows, pages), (1000, 8));
    assert_eq!(runtime.computers.len(), 2);
    assert_eq!(
        counts::projection(&store.db, epoch, |c| broker.available(c), broker.expected()).unwrap()
            ["aggregate"]["attention"],
        1000
    );
    assert!(fake
        .log()
        .iter()
        .all(|v| v["host"] != "older" || v["args"] != json!(["watch", "--json"])));
    stop(&mut runtime);
}

#[test]
fn aliases_rotate_past_lifetime_capacity_and_late_callbacks_cannot_restore_rows() {
    let fake = Fake::new(json!({"":{"runs":[[{"lines":[hello()]},{"serve":true}]]}}));
    let epoch = "11111111-1111-4111-8111-111111111111";
    let mut runtime = fake.start(epoch);
    let mut store = fake.store(epoch);
    let mut broker = feed::Broker::new(epoch);
    let producer = broker.claim_internal().unwrap();
    let event = next(&mut runtime);
    broker.apply(&mut store, &producer, &event.packet).unwrap();
    drop(event);
    let mut old = None;
    let mut old_cursor = None;
    for n in 0..70 {
        // Reuse peer-0 at the end to exercise a new registration for that alias.
        let alias = if n == 69 {
            "peer-0".to_owned()
        } else {
            format!("peer-{n}")
        };
        runtime.set_peers(&[json!(alias)]);
        while let Some(packet) = runtime.pending.pop_front() {
            broker.apply(&mut store, &producer, &packet).unwrap();
        }
        runtime.reap();
        runtime.admit_peers();
        let id = runtime.computers[&ComputerId(alias.clone())]
            .handle
            .identity()
            .clone();
        let mut h = hello();
        h["v"] = json!(1);
        h["seq"] = json!(1);
        let packet = runtime
            .convert(source::Event::Frame {
                source: id.clone(),
                generation: 1,
                frame: Box::new(serde_json::from_value(h.clone()).unwrap()),
                received_at: Instant::now(),
            })
            .unwrap();
        broker.apply(&mut store, &producer, &packet).unwrap();
        if n == 0 {
            old = Some((id.clone(), packet.scope().unwrap().1));
        }
        let mut s = snapshot(
            0,
            true,
            if n == 0 {
                (0..129).map(row).collect()
            } else {
                vec![row(n)]
            },
        );
        s["v"] = json!(1);
        s["seq"] = json!(2);
        let packet = runtime
            .convert(source::Event::Frame {
                source: id,
                generation: 1,
                frame: Box::new(serde_json::from_value(s).unwrap()),
                received_at: Instant::now(),
            })
            .unwrap();
        broker.apply(&mut store, &producer, &packet).unwrap();
        if n == 0 {
            old_cursor = zerus_desktop_catalog::page(&store.db, &alias, None)
                .unwrap()
                .next;
            assert!(old_cursor.is_some());
        }
        if n == 69 {
            let (old_id, old_generation) = old.clone().unwrap();
            assert!(packet.scope().unwrap().1 > old_generation);
            assert!(runtime
                .convert(source::Event::Frame {
                    source: old_id.clone(),
                    generation: 1,
                    frame: Box::new(serde_json::from_value(h).unwrap()),
                    received_at: Instant::now()
                })
                .is_none());
            let mut data = snapshot(0, true, vec![row(999)]);
            data["v"] = json!(1);
            data["seq"] = json!(2);
            assert!(runtime
                .convert(source::Event::Frame {
                    source: old_id.clone(),
                    generation: 1,
                    frame: Box::new(serde_json::from_value(data).unwrap()),
                    received_at: Instant::now()
                })
                .is_none());
            assert!(runtime
                .convert(source::Event::Status {
                    source: old_id,
                    generation: 1,
                    status: ComputerStatus::Reconnecting { attempt: 1 },
                    observed_at: Instant::now()
                })
                .is_none());
            assert!(broker
                .apply(
                    &mut store,
                    &producer,
                    &feed::Packet::Remove {
                        computer: alias.clone(),
                        generation: old_generation
                    }
                )
                .is_err());
            assert!(
                broker.coverage(&alias),
                "stale Remove must not invalidate the replacement"
            );
            assert!(zerus_desktop_catalog::page(&store.db, &alias, old_cursor.as_ref()).is_err());
            assert_eq!(
                zerus_desktop_catalog::page(&store.db, &alias, None)
                    .unwrap()
                    .rows[0]
                    .key,
                "run:r-69"
            );
        }
        while zerus_desktop_catalog::collect(&mut store.db)
            .unwrap()
            .is_some()
        {}
        assert!(runtime.pending.len() <= source::SOURCES);
        assert!(runtime.sender.usage().workers <= source::SOURCES);
        assert!(
            zerus_desktop_catalog::totals(&store.db)
                .unwrap()
                .retained_sources
                <= 2
        );
    }
    stop(&mut runtime);
}

#[test]
fn failed_alias_removal_is_visible_and_keeps_counts_incomplete() {
    let fake = Fake::new(
        json!({"":{"runs":[[{"lines":[hello(),snapshot(0,true,vec![row(1)])]},{"serve":true}]]}}),
    );
    let epoch = "11111111-1111-4111-8111-111111111111";
    let mut runtime = fake.start(epoch);
    let mut store = fake.store(epoch);
    let mut broker = feed::Broker::new(epoch);
    let producer = broker.claim_internal().unwrap();
    pump_until(
        &mut runtime,
        &mut store,
        &mut broker,
        &producer,
        |p| matches!(p,feed::Packet::Watch{frame,..}if matches!(&frame.body,Body::Snapshot(_))),
    );
    let generation = runtime.computers[&ComputerId::local()].generation;
    store.db.execute_batch("CREATE TRIGGER fixture_refuse_removal BEFORE DELETE ON desktop_catalog_sources BEGIN SELECT RAISE(ABORT,'fixture refusal'); END;").unwrap();
    let event = FleetEvent {
        packet: feed::Packet::Remove {
            computer: String::new(),
            generation,
        },
        _permit: None,
    };
    assert!(broker.apply(&mut store, &producer, &event.packet).is_err());
    runtime.reject(&event);
    assert_eq!(runtime.status()["error"], "fleet_removal_refused");
    assert_eq!(
        counts::projection(&store.db, epoch, |c| broker.available(c), broker.expected()).unwrap()
            ["aggregate"]["attention"],
        Value::Null
    );
    let offline = next(&mut runtime);
    assert!(matches!(offline.packet, feed::Packet::Offline { .. }));
    broker
        .apply(&mut store, &producer, &offline.packet)
        .unwrap();
    drop(offline);
    assert!(
        !zerus_desktop_catalog::page(&store.db, "", None)
            .unwrap()
            .complete
    );
    stop(&mut runtime);
}

#[test]
fn shared_detail_fence_uses_one_original_handle_and_exact_projection_generation() {
    use crate::desktop::{details, fanout::Queue};
    use zerus_watch_protocol::{Control, Ping};
    let epoch = uuid::Uuid::new_v4().to_string();
    let mut greeting = hello();
    greeting["topics"] = json!(["catalog", "catalog.meta", "activity"]);
    let fake = Fake::new(
        json!({"":{"runs":[[{"lines":[greeting,snapshot(0,true,vec![])]},{"serve":true}]]}}),
    );
    let mut runtime = fake.start(&epoch);
    let mut store = fake.store(&epoch);
    let mut broker = feed::Broker::new(&epoch);
    let producer = broker.claim_internal().unwrap();
    let mut details = details::Adapter::new(&epoch);
    let queue = Arc::new(Queue::default());
    let lease = details.open(&queue).unwrap()["lease"].clone();
    let mut seq = 0;
    let generation = loop {
        let event = next(&mut runtime);
        broker.apply(&mut store, &producer, &event.packet).unwrap();
        details.observe(&event, &mut |c, g, v| runtime.controls(c, g, v), &mut seq);
        if let feed::Packet::Watch {
            generation: g,
            frame,
            ..
        } = &event.packet
        {
            if matches!(&frame.body,Body::Snapshot(s) if s.topic==topic::CATALOG && s.last) {
                break *g;
            }
        }
    };
    assert!(generation > 0);
    assert!(!runtime.controls("", generation + 99, &[Control::Ping(Ping::default())]));
    let reply=details.request(&store,&json!({"op":"detail_follow","request_id":uuid::Uuid::new_v4().to_string(),"epoch":epoch,"lease":lease,
        "id":uuid::Uuid::new_v4().to_string(),"computer":"","subscription":{"topic":"activity","session":{"name":"claude/example/a","run_id":"r","conversation_id":"c"},"tail":100}}),
        &mut|c,g,v|runtime.controls(c,g,v),&mut seq);
    assert_eq!(reply["accepted"], true);
    let until = Instant::now() + Duration::from_secs(5);
    let mut delivered = false;
    while !delivered {
        assert!(Instant::now() < until);
        let event = next(&mut runtime);
        broker.apply(&mut store, &producer, &event.packet).unwrap();
        details.observe(&event, &mut |c, g, v| runtime.controls(c, g, v), &mut seq);
        while let Some(bytes) = queue.take() {
            let frame: Value = serde_json::from_slice(&bytes).unwrap();
            if frame["t"] == "detail" {
                assert_eq!(frame["value"]["generation"], generation);
                assert_eq!(frame["value"]["detail_seq"], 1);
                delivered = true;
            }
        }
    }
    assert_eq!(
        fake.log()
            .iter()
            .filter(|e| e["args"] == json!(["watch", "--json"]))
            .count(),
        1
    );
    let controls: Vec<_> = fake
        .log()
        .into_iter()
        .filter_map(|e| e.get("stdin").cloned())
        .collect();
    assert!(matches!(
        controls
            .iter()
            .map(|v| v["op"].as_str().unwrap())
            .collect::<Vec<_>>()
            .as_slice(),
        ["unsubscribe", "ping", "subscribe"]
    ));
    details.stop();
    stop(&mut runtime);
    assert_eq!(runtime.status()["control_batches"], 0);
}

#[test]
fn an_ungreeted_bad_peer_does_not_degrade_a_live_computer_or_become_a_source() {
    let fake = Fake::new(json!({
        "":{"runs":[[{"lines":[hello(),snapshot(0,true,vec![row(1)]),meta(json!(["broken.example"]))]},{"serve":true}]]},
        "broken.example":{"runs":[[{"exit":1}]]}
    }));
    let mut runtime = fake.start("epoch");
    let until = Instant::now() + Duration::from_secs(15);
    loop {
        drop(runtime.poll());
        let status = runtime.status();
        let failed = status["connections"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == "broken.example" && c["status"] == "reconnecting");
        if failed
            && runtime.computers[&ComputerId::local()].accepting
            && runtime.computers[&ComputerId::local()].status == ComputerStatus::Live
        {
            break;
        }
        assert!(Instant::now() < until, "fake fleet did not settle");
        std::thread::sleep(Duration::from_millis(2));
    }
    let status = runtime.status();
    let generation = runtime.computers[&ComputerId("broken.example".into())].generation;
    let admitted_controls = runtime.controls("broken.example", 0, &[]);
    stop(&mut runtime);
    assert_eq!(
        status["transport_ready"], true,
        "healthy computer remains available"
    );
    assert_eq!(
        status["all_sources_ready"], false,
        "coverage is still incomplete"
    );
    assert_eq!(status["connections"].as_array().unwrap().len(), 2);
    assert_eq!(generation, 0);
    assert!(!admitted_controls, "a display alias has no source permit");
    assert_eq!(runtime.status()["transport_ready"], false);
}

#[test]
fn a_live_peer_stays_available_when_the_local_watch_reconnects() {
    let fake = Fake::new(json!({
        "":{"runs":[
            [{"lines":[hello(),snapshot(0,true,vec![]),meta(json!(["healthy.example"]))]},
             {"serve":true,"until_file":"local-disconnect"}],
            [{"exit":1}]]},
        "healthy.example":{"runs":[[{"lines":[hello(),snapshot(0,true,vec![row(1)])]},{"serve":true}]]}
    }));
    let mut runtime = fake.start("epoch");
    let peer = ComputerId("healthy.example".into());
    let until = Instant::now() + Duration::from_secs(15);
    loop {
        drop(runtime.poll());
        if runtime
            .computers
            .get(&peer)
            .is_some_and(|c| c.accepting && c.status == ComputerStatus::Live)
        {
            break;
        }
        assert!(Instant::now() < until, "peer did not become live");
        std::thread::sleep(Duration::from_millis(2));
    }
    fs::write(fake.temp.path().join("local-disconnect"), "").unwrap();
    let until = Instant::now() + Duration::from_secs(15);
    loop {
        drop(runtime.poll());
        if !runtime.computers[&ComputerId::local()].accepting {
            break;
        }
        assert!(Instant::now() < until, "local did not disconnect");
        std::thread::sleep(Duration::from_millis(2));
    }
    let status = runtime.status();
    let peer_accepting = runtime.computers[&peer].accepting;
    stop(&mut runtime);
    assert_eq!(status["transport_ready"], true);
    assert_eq!(status["all_sources_ready"], false);
    assert!(peer_accepting);
}

impl Fake {
    fn start_waiting(&self, epoch: &str, restart: Duration) -> Runtime {
        let mut runtime = self.start(epoch);
        runtime.set_restart_delays([restart; 4]);
        runtime
    }
    fn probes(&self, host: &str) -> usize {
        self.log()
            .iter()
            .filter(|entry| entry["host"] == host && entry["args"] == json!(["-h"]))
            .count()
    }
}

fn connection(runtime: &Runtime, id: &str) -> Value {
    runtime.status()["connections"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == id)
        .cloned()
        .unwrap_or(Value::Null)
}

fn poll_until(runtime: &mut Runtime, what: &str, done: impl Fn(&Runtime) -> bool) {
    let until = Instant::now() + Duration::from_secs(15);
    while !done(runtime) {
        drop(runtime.poll());
        assert!(Instant::now() < until, "{what}: {}", runtime.status());
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn a_peer_wait_is_shown_with_its_source_and_reconnect_ends_only_that_wait() {
    let fake = Fake::new(json!({
        "":{"runs":[[{"lines":[hello(),snapshot(0,true,vec![]),meta(json!(["slow.example"]))]},{"serve":true}]]},
        "slow.example":{"runs":[[{"exit":1}]]}
    }));
    let mut runtime = fake.start_waiting("epoch", Duration::from_secs(3600));
    let before = runtime.connections_revision();
    poll_until(&mut runtime, "the peer's first wait", |runtime| {
        connection(runtime, "slow.example")["retry"]["serial"] == 1
            && runtime.computers[&ComputerId::local()].status == ComputerStatus::Live
    });
    assert!(runtime.connections_revision() > before);
    let row = connection(&runtime, "slow.example");
    assert_eq!(row["status"], "reconnecting");
    assert_eq!(row["retry"]["phase"], "waiting");
    assert_eq!(row["retry"]["delay_ms"], 3_600_000);
    assert!(row["retry"]["remaining_ms"].as_u64().unwrap() <= 3_600_000);
    assert_eq!(connection(&runtime, "")["retry"]["phase"], "idle");
    let incarnation = row["incarnation"].as_u64().unwrap();
    // Control: the hour-long wait starts nothing by itself.
    std::thread::sleep(Duration::from_millis(200));
    drop(runtime.poll());
    assert_eq!(fake.probes("slow.example"), 1);

    assert_eq!(
        runtime.reconnect("slow.example", Some(incarnation + 1)),
        "source_changed"
    );
    assert_eq!(runtime.reconnect("unknown.example", None), "not_configured");
    assert_eq!(runtime.reconnect("", None), "connected");
    assert_eq!(
        runtime.reconnect("slow.example", Some(incarnation)),
        "requested"
    );
    assert_eq!(
        runtime.reconnect("slow.example", Some(incarnation)),
        "pending"
    );
    poll_until(&mut runtime, "the peer's next wait", |runtime| {
        connection(runtime, "slow.example")["retry"]["serial"] == 2
    });
    std::thread::sleep(Duration::from_millis(200));
    drop(runtime.poll());
    assert_eq!(fake.probes("slow.example"), 2, "exactly one extra attempt");
    // The same supervisor kept its source; no other was started.
    assert_eq!(
        connection(&runtime, "slow.example")["incarnation"],
        incarnation
    );

    let reply = request(
        &mut runtime,
        &json!({"op":"fleet_reconnect","request_id":"r-1",
        "computers":[{"computer":"slow.example","incarnation":incarnation},{"computer":""}]}),
    );
    assert_eq!(reply["accepted"], true);
    assert_eq!(
        reply["results"][0],
        json!({"computer":"slow.example","code":"requested"})
    );
    assert_eq!(reply["results"][1]["code"], "connected");
    assert_eq!(reply["connections"]["display_only"], true);
    assert_eq!(reply["connections"]["authority"], false);
    for bad in [
        json!({"op":"fleet_reconnect","computers":[{"computer":"slow.example"}]}),
        json!({"op":"fleet_reconnect","request_id":"r","computers":[]}),
        json!({"op":"fleet_reconnect","request_id":"r","computers":[{"computer":"-oProxyCommand=x"}]}),
        json!({"op":"fleet_reconnect","request_id":"r","computers":[{"computer":"a"},{"computer":"a"}]}),
        json!({"op":"fleet_reconnect","request_id":"r","computers":[{"computer":"a","incarnation":"7"}]}),
        json!({"op":"fleet_reconnect","request_id":"r","computers":
            (0..65).map(|n| json!({"computer":format!("p{n}")})).collect::<Vec<_>>()}),
    ] {
        assert_eq!(
            request(&mut runtime, &bad)["code"],
            "invalid_request",
            "{bad}"
        );
    }
    assert_eq!(
        request(
            &mut runtime,
            &json!({"op":"fleet_restart","request_id":"r"})
        )["code"],
        "unknown_request"
    );
    stop(&mut runtime);
    assert_eq!(runtime.reconnect("slow.example", None), "stopping");
}

#[test]
fn a_stale_source_cannot_publish_a_schedule() {
    let fake = Fake::new(json!({"":{"runs":[[{"exit":1}]]}}));
    let mut runtime = fake.start_waiting("epoch", Duration::from_secs(3600));
    poll_until(&mut runtime, "the local wait", |runtime| {
        connection(runtime, "")["retry"]["phase"] == "waiting"
    });
    let current = runtime.computers[&ComputerId::local()].retry;
    let mut stale = runtime.computers[&ComputerId::local()]
        .handle
        .identity()
        .clone();
    stale.incarnation += 100;
    let revision = runtime.connections_revision();
    assert!(runtime
        .convert(source::Event::Retry {
            source: stale,
            generation: u64::MAX,
            retry: Retry::Idle,
        })
        .is_none());
    assert_eq!(runtime.computers[&ComputerId::local()].retry, current);
    assert_eq!(runtime.connections_revision(), revision);
    stop(&mut runtime);
}

#[test]
fn saved_retry_delays_apply_and_an_invalid_file_keeps_the_last_valid_schedule() {
    use crate::machines::retry_delays::FILE;
    let fake = Fake::new(
        json!({"":{"runs":[[{"lines":[hello(),snapshot(0,true,vec![])]},{"serve":true}]]}}),
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILE);
    let seconds = |runtime: &Runtime| runtime.restart_delays().map(|d| d.as_secs());
    let mut runtime = fake.start("epoch");

    // The cold start step of `Runtime::start`: no saved file, the defaults.
    let missing = runtime.configure_retry_delays(dir.path().to_owned());
    assert_eq!(missing["source"], "default");
    assert!(missing["diagnostic"].is_null());
    assert_eq!(seconds(&runtime), [1, 2, 5, 10]);

    fs::write(&path, r#"{"version":1,"delays_seconds":[2,4,8,30]}"#).unwrap();
    assert_eq!(runtime.reload_retry_delays()["source"], "file");
    assert_eq!(seconds(&runtime), [2, 4, 8, 30]);

    let invalid = r#"{"version":1,"delays_seconds":[30,4,8,2]}"#;
    fs::write(&path, invalid).unwrap();
    let revision = runtime.connections_revision();
    let kept = request(
        &mut runtime,
        &json!({"op":"fleet_retry_delays_reload","request_id":"r"}),
    );
    assert_eq!(kept["accepted"], true);
    assert_eq!(kept["retry_delays"]["source"], "kept");
    assert!(kept["retry_delays"]["diagnostic"]
        .as_str()
        .unwrap()
        .contains(FILE));
    assert_eq!(
        seconds(&runtime),
        [2, 4, 8, 30],
        "the last valid schedule stays"
    );
    assert!(runtime.connections_revision() > revision);
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        invalid,
        "the file is never changed"
    );
    assert_eq!(runtime.status()["retry_delays"]["source"], "kept");
    assert_eq!(
        runtime.connections()["retry_delays"]["delays_seconds"],
        json!([2, 4, 8, 30])
    );

    fs::remove_file(&path).unwrap();
    assert_eq!(runtime.reload_retry_delays()["source"], "default");
    assert_eq!(seconds(&runtime), [1, 2, 5, 10]);
    stop(&mut runtime);

    // A cold start with an invalid file keeps serving with the defaults and
    // says why. It applies them itself: this fixture began with the scaled
    // millisecond delays, which a cold start must not inherit.
    fs::write(&path, "not json").unwrap();
    let mut cold = fake.start("epoch-2");
    assert_eq!(
        seconds(&cold),
        [0, 0, 0, 0],
        "control: the fixture's own delays"
    );
    let view = cold.configure_retry_delays(dir.path().to_owned());
    assert_eq!(view["source"], "default");
    assert!(view["diagnostic"]
        .as_str()
        .is_some_and(|diagnostic| diagnostic.contains(FILE)));
    assert_eq!(seconds(&cold), [1, 2, 5, 10]);
    assert_eq!(cold.status()["state"], "running");
    assert_eq!(fs::read_to_string(&path).unwrap(), "not json");
    // Once a usable file was read, a later failed reload keeps that one.
    fs::write(&path, r#"{"version":1,"delays_seconds":[3,3,3,60]}"#).unwrap();
    assert_eq!(cold.reload_retry_delays()["source"], "file");
    fs::write(&path, "not json").unwrap();
    assert_eq!(cold.reload_retry_delays()["source"], "kept");
    assert_eq!(seconds(&cold), [3, 3, 3, 60]);
    stop(&mut cold);
}

#[test]
fn early_connection_frames_follow_changes_at_a_bounded_rate() {
    let mut publication = Publication::default();
    let at = Instant::now();
    assert!(!publication.due(0, at), "nothing changed yet");
    assert!(publication.due(1, at));
    assert!(
        !publication.due(1, at + Duration::from_secs(5)),
        "unchanged"
    );
    assert!(
        !publication.due(2, at + Duration::from_millis(100)),
        "too soon"
    );
    assert!(publication.due(2, at + CONNECTIONS_GAP));
    assert!(!publication.due(3, at + CONNECTIONS_GAP + Duration::from_millis(10)));
}
