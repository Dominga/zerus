//! Polling a computer whose `hgs` lacks the stream, against a fake `hgs`
//! (tests/support/fake_hgs.py), never the real CLI.
mod common;

use common::{change_of, status_of, FakeHgs, Seen};
use serde_json::{json, Value};
use std::sync::mpsc::channel;
use std::time::Duration;
use zerus_client_core::stream;
use zerus_client_core::{
    ActivityUpdate, Change, ComputerId, ComputerStatus, ProcessesUpdate, SessionRef, StreamEvent,
    Timing,
};

fn quick() -> Timing {
    Timing {
        stop_grace: Duration::from_millis(500),
        ..Timing::scaled(0.002)
    }
}

fn listing(runs: &[&str]) -> Value {
    json!({
        "host": "studio-host", "ok": true, "hgs_version": "1.40.0",
        "metrics": {"platform": "linux", "cpu_percent": 12.5, "sampled_at": 1787840000.0},
        "sessions": runs.iter().map(|run| json!({"name": format!("codex/p/{run}"), "run_id": run,
            "agent": "codex", "tracked": true, "phase": "idle", "activity": "idle"})).collect::<Vec<_>>()
    })
}

fn inspection(events: std::ops::RangeInclusive<i64>, activity: &str) -> Value {
    json!({
        "name": "codex/p/r1", "run_id": "r1", "activity": activity, "phase": "idle",
        "events": events.map(|seq| json!({"seq": seq, "type": "Stop", "detail": format!("reply {seq}")})).collect::<Vec<_>>(),
        "history_truncated": false,
        "processes": {"items": [], "active_count": 0, "sampled_at": 1.0},
        "process_work": null
    })
}

fn write(fake: &FakeHgs, name: &str, value: &Value) {
    let path = fake.dir.join(name);
    let staged = fake.dir.join(format!("{name}.tmp"));
    std::fs::write(&staged, value.to_string()).unwrap();
    std::fs::rename(staged, path).unwrap();
}

fn studio() -> ComputerId {
    ComputerId::from("studio")
}

fn status(event: &StreamEvent, wanted: ComputerStatus) -> bool {
    change_of(event).and_then(|change| status_of(change, &studio())) == Some(wanted)
}

fn activity(event: &StreamEvent) -> Option<&ActivityUpdate> {
    match change_of(event) {
        Some(Change::Activity { key, update, .. }) if key == "run:r1" => Some(update),
        _ => None,
    }
}

#[test]
fn a_peer_without_the_stream_is_polled_like_the_qt_client() {
    let fake = FakeHgs::new("legacy_poll", json!({"studio": {"marker": false}}));
    write(&fake, "ls-studio.json", &listing(&["r1", "r2"]));
    write(&fake, "inspect-studio.json", &inspection(1..=3, "idle"));
    let (tx, rx) = channel();
    let mut seen = Seen::new(rx);
    let handle = stream::spawn(&fake.path(), &studio(), quick(), tx);
    seen.until("unsupported", |event| {
        status(event, ComputerStatus::Unsupported)
    });
    // The first listing gives the rows, the card and the polled status.
    let index = seen.until(
        "the rows",
        |event| matches!(change_of(event), Some(Change::Replace { rows, .. }) if rows.len() == 2),
    );
    let Some(Change::Replace { rows, .. }) = change_of(&seen.all[index]) else {
        unreachable!()
    };
    assert_eq!(rows[0].0, "run:r1");
    assert_eq!(rows[0].1.machine, "studio");
    seen.until("the card", |event| {
        matches!(change_of(event), Some(Change::Machine { machine, .. }) if machine.host.as_deref() == Some("studio-host"))
    });
    seen.until("polled", |event| status(event, ComputerStatus::Polled));
    // A followed session: its newest window, its summary, then new events.
    let session = SessionRef {
        name: "codex/p/r1".into(),
        run_id: Some("r1".into()),
        conversation_id: None,
    };
    handle.follow(session.clone(), None, 100).unwrap();
    handle.follow_processes(session).unwrap();
    let window = seen.until("the window", |event| {
        matches!(activity(event), Some(ActivityUpdate::Replace { .. }))
    });
    let Some(ActivityUpdate::Replace { events, cursor }) = activity(&seen.all[window]) else {
        unreachable!()
    };
    assert_eq!((events.len(), *cursor), (3, 3));
    seen.until("the summary", |event| {
        matches!(activity(event), Some(ActivityUpdate::State { value, .. })
            if value["activity"] == "idle" && value.get("events").is_none() && value.get("processes").is_none())
    });
    seen.until("the processes", |event| {
        matches!(
            change_of(event),
            Some(Change::Processes {
                update: ProcessesUpdate::State { .. },
                ..
            })
        )
    });
    write(&fake, "inspect-studio.json", &inspection(1..=5, "busy"));
    let more = seen.until_from(window + 1, "new events", |event| {
        matches!(activity(event), Some(ActivityUpdate::Events { .. }))
    });
    let Some(ActivityUpdate::Events { events, cursor }) = activity(&seen.all[more]) else {
        unreachable!()
    };
    assert_eq!(
        events
            .iter()
            .map(|event| event["seq"].as_i64().unwrap())
            .collect::<Vec<_>>(),
        [4, 5]
    );
    assert_eq!(*cursor, 5);
    seen.until_from(more, "the changed summary", |event| {
        matches!(activity(event), Some(ActivityUpdate::State { value, .. }) if value["activity"] == "busy")
    });
    // A failed listing reconnects with the rows kept; the next one is back.
    std::fs::remove_file(fake.dir.join("ls-studio.json")).unwrap();
    let failed = seen.until("reconnecting", |event| {
        status(event, ComputerStatus::Reconnecting { attempt: 1 })
    });
    assert!(seen.all[failed..]
        .iter()
        .all(|event| !matches!(change_of(event), Some(Change::Replace { .. }))));
    // The session leaves: its follows end.
    write(&fake, "ls-studio.json", &listing(&["r2"]));
    seen.until_from(failed, "polled again", |event| {
        status(event, ComputerStatus::Polled)
    });
    seen.until_from(failed, "the activity end", |event| {
        matches!(activity(event), Some(ActivityUpdate::Ended))
    });
    seen.until_from(failed, "the processes end", |event| {
        matches!(
            change_of(event),
            Some(Change::Processes {
                update: ProcessesUpdate::Ended,
                ..
            })
        )
    });
    assert!(handle.followed().is_empty() && handle.followed_processes().is_empty());
    // Re-probes keep the status: no unsupported again while polling works.
    let polled = seen.all.len();
    seen.drain_for(Duration::from_millis(1500));
    assert!(seen.all[polled..]
        .iter()
        .all(|event| !status(event, ComputerStatus::Unsupported)));
    assert!(
        fake.runs("studio", &["-h"]).len() >= 2,
        "it re-probes for the stream"
    );
    handle.stop();
}

#[test]
fn fake_hgs_log_ignores_an_unterminated_last_line() {
    let fake = FakeHgs::new("log_partial", json!({}));
    std::fs::write(
        fake.dir.join("log.ndjson"),
        "{\"host\":\"studio\"}\n{\"host\":",
    )
    .unwrap();
    assert_eq!(fake.log(), vec![json!({"host": "studio"})]);
}

#[test]
fn this_computer_without_the_stream_names_its_peers_from_its_listing() {
    let fake = FakeHgs::new("legacy_local", json!({"": {"marker": false}}));
    let mut local = listing(&["r1"]);
    local["host"] = json!("workstation");
    local["peers"] = json!(["studio"]);
    write(&fake, "ls-local.json", &local);
    let (tx, rx) = channel();
    let mut seen = Seen::new(rx);
    let handle = stream::spawn(&fake.path(), &ComputerId::local(), quick(), tx);
    let index = seen.until("the peers", |event| {
        matches!(event, StreamEvent::Peers { .. })
    });
    let StreamEvent::Peers { computer, peers } = &seen.all[index] else {
        unreachable!()
    };
    assert_eq!(
        (computer.is_local(), peers.as_slice()),
        (true, ["studio".to_owned()].as_slice())
    );
    seen.until("rows named by the host", |event| {
        matches!(change_of(event), Some(Change::Replace { rows, .. }) if rows[0].1.machine == "workstation")
    });
    handle.stop();
}
