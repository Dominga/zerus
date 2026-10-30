//! Raw service output reuses the real supervisor and the existing fake CLI.
mod common;
use common::{catalog, eventually, hello, FakeHgs};
use serde_json::json;
use std::{
    io::Write,
    process::{Child, Command},
    sync::Mutex,
    time::{Duration, Instant},
};
use zerus_client_core::{
    source::{self, Event, Receiver},
    ComputerId, ComputerStatus, Timing,
};
use zerus_watch_protocol::{topic, Body};

// Mirrors the hgs caller's shared spawn_in_turn hook, including on macOS.
fn spawn(command: &mut Command) -> std::io::Result<Child> {
    static LOCK: Mutex<()> = Mutex::new(());
    let _guard = LOCK.lock().unwrap();
    command.spawn()
}
fn quick() -> Timing {
    Timing {
        stop_grace: Duration::from_millis(200),
        silence: Duration::from_secs(5),
        ..Timing::scaled(0.01)
    }
}
fn until(rx: &Receiver, mut accept: impl FnMut(&Event) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let item = rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("source event before deadline");
        if accept(item.event()) {
            return;
        }
    }
}

#[test]
fn source_preserves_frames_identity_background_facts_and_single_upstream() {
    let mut row = catalog(&[("run-1", "session")]);
    row["items"][0]["row"]["process_work"] = json!({"count":7,"known":true});
    let fake = FakeHgs::new(
        "source_frames",
        json!({"":{"runs":[[{"lines":[hello("local"),row]},{"serve":true}]]}}),
    );
    let (tx, rx) = source::channel(spawn);
    let handle = tx
        .spawn(&fake.path(), &ComputerId::local(), quick())
        .unwrap();
    let identity = handle.identity().clone();
    let mut last = 0;
    until(&rx, |event| match event {
        Event::Frame {
            source,
            generation,
            frame,
            ..
        } => {
            assert_eq!(*source, identity);
            assert_eq!(*generation, 1);
            assert!(frame.seq > last);
            last = frame.seq;
            if let Body::Snapshot(page) = &frame.body {
                if page.topic == topic::CATALOG {
                    assert_eq!(
                        page.items.as_ref().unwrap()[0]["row"]["process_work"]["count"],
                        7
                    );
                    return true;
                }
            }
            false
        }
        _ => false,
    });
    assert!(tx
        .spawn(&fake.path(), &ComputerId::local(), quick())
        .is_err());
    assert_eq!(fake.runs("", &["watch", "--json"]).len(), 1);
    handle.stop();
    eventually("owned reader ends", || tx.usage().readers == 0);
    assert_eq!(tx.usage().sources, 0);
    assert!(fake.stdin("").is_empty());
}

#[test]
fn older_raw_peer_is_explicitly_unsupported_without_legacy_or_agent_calls() {
    let fake = FakeHgs::new("source_old", json!({"":{"marker":false}}));
    let (tx, rx) = source::channel(spawn);
    let handle = tx
        .spawn(&fake.path(), &ComputerId::local(), quick())
        .unwrap();
    until(&rx, |event| {
        matches!(
            event,
            Event::Status {
                status: ComputerStatus::Unsupported,
                ..
            }
        )
    });
    assert!(fake.runs("", &["watch", "--json"]).is_empty());
    assert!(fake.runs("", &["ls", "--json"]).is_empty());
    assert!(fake
        .log()
        .iter()
        .all(|entry| entry["args"] == json!(["-h"])));
    handle.stop();
    assert_eq!(tx.usage().connecting, 0);
}

#[test]
fn reconnect_reuses_registration_and_increments_stream_generation() {
    let fake = FakeHgs::new(
        "source_reconnect",
        json!({"":{"runs":[[{"lines":[hello("local"),catalog(&[("r","s")])]}]]}}),
    );
    let (tx, rx) = source::channel(spawn);
    let handle = tx
        .spawn(&fake.path(), &ComputerId::local(), quick())
        .unwrap();
    let identity = handle.identity().clone();
    let mut generations = Vec::new();
    until(&rx, |event| {
        if let Event::Frame {
            source,
            generation,
            frame,
            ..
        } = event
        {
            assert_eq!(*source, identity);
            if matches!(&frame.body, Body::Hello(_)) {
                generations.push(*generation);
            }
        }
        generations.len() == 3
    });
    assert_eq!(generations, [1, 2, 3]);
    handle.stop();
}

#[test]
fn first_hello_and_increasing_sequences_are_required_before_projection() {
    for (name, lines, accepted) in [
        ("before_hello", vec![catalog(&[("r", "s")])], 0),
        ("duplicate_hello", vec![hello("local"), hello("changed")], 1),
        (
            "fatal_organization",
            vec![
                hello("local"),
                json!({"t":"error","code":"failed","message":"fixture","topic":"organization","fatal":true}),
            ],
            2,
        ),
        (
            "old_sequence",
            vec![hello("local"), json!({"t":"heartbeat","seq":1,"at":0})],
            1,
        ),
    ] {
        let fake = FakeHgs::new(
            &format!("source_{name}"),
            json!({"":{"runs":[[{"lines":lines},{"serve":true}]]}}),
        );
        let (tx, rx) = source::channel(spawn);
        let mut timing = quick();
        timing.restart = [Duration::from_secs(10); 4];
        let handle = tx
            .spawn(&fake.path(), &ComputerId::local(), timing)
            .unwrap();
        let mut frames = 0;
        until(&rx, |event| match event {
            Event::Frame { .. } => {
                frames += 1;
                false
            }
            Event::Status {
                status: ComputerStatus::Reconnecting { attempt: 1 },
                ..
            } => true,
            _ => false,
        });
        assert_eq!(frames, accepted, "{name}");
        let at = Instant::now();
        handle.stop();
        assert!(at.elapsed() < Duration::from_secs(2));
    }
}

#[test]
fn shared_connection_gate_covers_probe_and_pre_hello_child() {
    let mut hosts = serde_json::Map::new();
    for n in 0..8 {
        hosts.insert(
            format!("peer-{n}"),
            json!({"runs":[[{"await_file":"release"},{"lines":[hello("peer")]},{"serve":true}]]}),
        );
    }
    let fake = FakeHgs::new("source_connection_gate", hosts.into());
    let (tx, _rx) = source::channel(spawn);
    let handles: Vec<_> = (0..8)
        .map(|n| {
            tx.spawn(&fake.path(), &format!("peer-{n}").as_str().into(), quick())
                .unwrap()
        })
        .collect();
    eventually("four admitted watches", || {
        fake.log()
            .iter()
            .filter(|e| e["args"] == json!(["watch", "--json"]))
            .count()
            == 4
    });
    assert_eq!(tx.usage().connecting, 4);
    std::thread::sleep(Duration::from_millis(80));
    assert_eq!(
        fake.log()
            .iter()
            .filter(|e| e["args"] == json!(["watch", "--json"]))
            .count(),
        4
    );
    fake.touch("release");
    eventually("all eight handshakes", || {
        tx.usage().readers == 8 && tx.usage().connecting == 0
    });
    for handle in handles {
        handle.stop();
    }
    eventually("all owned readers close", || tx.usage().readers == 0);
}

#[test]
fn full_shared_ingress_cancels_without_retaining_a_catalog() {
    let fake = FakeHgs::new(
        "source_full_ingress",
        json!({"":{"runs":[[{"file":"unused"},{"serve":true}]]}}),
    );
    let burst = fake.dir.join("burst.ndjson");
    let mut out = std::fs::File::create(&burst).unwrap();
    let mut greeting = hello("local");
    greeting["v"] = json!(1);
    greeting["seq"] = json!(1);
    writeln!(out, "{greeting}").unwrap();
    let payload = "x".repeat(500_000);
    for n in 0..48 {
        writeln!(out,"{}",json!({"v":1,"seq":n+2,"t":"snapshot","topic":"catalog","page":n,"last":n==47,"cursor":1,
            "items":[{"key":format!("run:{n}"),"row":{"name":format!("s-{n}"),"run_id":format!("r-{n}"),"preview":payload}}]})).unwrap();
    }
    drop(out);
    let config = fake.dir.join("config.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&config).unwrap()).unwrap();
    value["hosts"][""]["runs"][0][0]["file"] = json!(burst);
    std::fs::write(config, value.to_string()).unwrap();
    let (tx, rx) = source::channel(spawn);
    let handle = tx
        .spawn(&fake.path(), &ComputerId::local(), quick())
        .unwrap();
    eventually("shared ingress pressure", || tx.usage().bytes > 15_000_000);
    assert!(tx.usage().high_bytes <= source::INGRESS_BYTES);
    assert!(tx.usage().high_items <= source::INGRESS_ITEMS);
    let at = Instant::now();
    handle.stop();
    assert!(at.elapsed() < Duration::from_secs(2));
    drop(rx);
    eventually("ingress permits released", || {
        tx.usage().items == 0 && tx.usage().readers == 0
    });
    println!("raw source pressure: 48 pages, 24 MB offered, ingress <=16 MiB and 1024 items; cancellation <2s");
}

#[test]
fn closing_consumer_stops_owned_source_even_with_no_frames() {
    let fake = FakeHgs::new(
        "source_consumer_drop",
        json!({"":{"runs":[[{"lines":[hello("local")]},{"serve":true}]]}}),
    );
    let (tx, rx) = source::channel(spawn);
    let handle = tx
        .spawn(&fake.path(), &ComputerId::local(), quick())
        .unwrap();
    until(
        &rx,
        |e| matches!(e,Event::Frame{frame,..}if matches!(frame.body,Body::Hello(_))),
    );
    drop(rx);
    let at = Instant::now();
    handle.stop();
    assert!(at.elapsed() < Duration::from_secs(2));
    eventually("consumer drop releases readers", || tx.usage().readers == 0);
}
