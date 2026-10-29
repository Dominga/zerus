//! `stream::spawn` against a fake `hgs` (tests/support/fake_hgs.py).
mod common;

use common::{catalog, change_of, hello, status_of, FakeHgs, Seen};
use serde_json::json;
use std::sync::mpsc::channel;
use std::time::Duration;
use zerus_client_core::stream::{self, Probe};
use zerus_client_core::{
    ActivityUpdate, Change, ComputerId, ComputerStatus, SessionRef, StreamEvent, Timing,
};
use zerus_watch_protocol::{Control, Ping};

fn quick() -> Timing {
    Timing {
        stop_grace: Duration::from_millis(500),
        ..Timing::scaled(0.002)
    }
}

fn reconnecting(event: &StreamEvent, attempt: u32) -> bool {
    change_of(event).and_then(|change| status_of(change, &ComputerId::local()))
        == Some(ComputerStatus::Reconnecting { attempt })
}

fn live(event: &StreamEvent) -> bool {
    change_of(event).and_then(|change| status_of(change, &ComputerId::local()))
        == Some(ComputerStatus::Live)
}

#[test]
fn a_stream_delivers_frames_and_writes_controls() {
    let fake = FakeHgs::new(
        "stream_delivers",
        json!({"": {"runs": [[{"file": "basic.ndjson"}, {"serve": true}]]}}),
    );
    let (tx, rx) = channel();
    let mut seen = Seen::new(rx);
    let handle = stream::spawn(&fake.path(), &ComputerId::local(), quick(), tx);
    seen.until("probing", |event| {
        change_of(event).and_then(|change| status_of(change, &ComputerId::local()))
            == Some(ComputerStatus::Probing)
    });
    seen.until("live", live);
    // Its catalog.meta gives this computer's card data.
    seen.until("the machine card", |event| {
        matches!(change_of(event), Some(Change::Machine { computer, machine })
            if computer.is_local() && machine.host.as_deref() == Some("workstation"))
    });
    // The basic fixture's last heartbeat follows its unknown frame type and
    // every other frame kind: the stream stays up through all of them.
    seen.until("peers", |event| matches!(event, StreamEvent::Peers { .. }));
    let peers = seen
        .all
        .iter()
        .find_map(|event| match event {
            StreamEvent::Peers { computer, peers } => Some((computer.clone(), peers.clone())),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        peers,
        (
            ComputerId::local(),
            vec!["studio".into(), "build-01".into()]
        )
    );
    seen.until("the delta", |event| {
        matches!(change_of(event), Some(Change::Remove { key, .. }) if key == "run:run-lint-1")
    });

    let session = SessionRef {
        name: "claude/docs-site/draft".into(),
        run_id: Some("run-draft-1".into()),
        conversation_id: Some("conv-draft".into()),
    };
    assert_eq!(
        handle.follow(session, None, 100).unwrap(),
        "run:run-draft-1"
    );
    seen.until("activity", |event| {
        matches!(change_of(event), Some(Change::Activity { key, update: ActivityUpdate::Events { cursor: 1, .. }, .. })
            if key == "run:run-draft-1")
    });
    assert!(handle.send(Control::Ping(Ping { id: Some(json!(7)) })));
    common::eventually("the ping", || fake.stdin("").len() >= 2);
    assert!(handle.unfollow("run:run-draft-1"));
    common::eventually("the unsubscribe", || fake.stdin("").len() >= 3);
    let controls = fake.stdin("");
    assert_eq!(
        controls[0],
        json!({"op": "subscribe", "topic": "activity", "session": {"name": "claude/docs-site/draft",
            "run_id": "run-draft-1", "conversation_id": "conv-draft"}, "tail": 100})
    );
    assert_eq!(controls[1], json!({"op": "ping", "id": 7}));
    assert_eq!(controls[2]["op"], "unsubscribe");
    assert_eq!(controls[2]["session"]["run_id"], "run-draft-1");

    handle.stop();
    // Stopping closes stdin: the server sees EOF and exits.
    common::eventually("EOF", || fake.eofs("") == 1);
    seen.drain_for(Duration::from_millis(50));
    let tail: Vec<_> = seen.all.iter().rev().take(2).collect();
    assert!(matches!(
        change_of(tail[0]),
        Some(Change::Computer {
            status: ComputerStatus::Ended,
            ..
        })
    ));
    assert!(matches!(change_of(tail[1]), Some(Change::Replace { rows, .. }) if rows.is_empty()));
    assert!(
        !seen.all.iter().any(|event| reconnecting(event, 1)),
        "no frame of the fixture ended the stream"
    );
    assert_eq!(fake.runs("", &["watch", "--json"]).len(), 1);
}

#[test]
fn a_stream_that_ends_reconnects_after_growing_delays_and_keeps_its_rows() {
    let fake = FakeHgs::new(
        "stream_reconnects",
        json!({"": {"runs": [[{"lines": [hello("workstation"), catalog(&[("r1", "claude/sample-project/review")])]}]]}}),
    );
    let (tx, rx) = channel();
    let mut seen = Seen::new(rx);
    let timing = quick();
    let handle = stream::spawn(&fake.path(), &ComputerId::local(), timing.clone(), tx);
    seen.until("the fourth failure", |event| reconnecting(event, 4));
    handle.stop();

    let attempts: Vec<u32> = seen
        .all
        .iter()
        .filter_map(|event| {
            match change_of(event).and_then(|c| status_of(c, &ComputerId::local())) {
                Some(ComputerStatus::Reconnecting { attempt }) => Some(attempt),
                _ => None,
            }
        })
        .collect();
    assert_eq!(&attempts[..4], &[1, 2, 3, 4]);
    // Rows stay as last known: every replacement before the stop has the row.
    let stop = seen
        .all
        .iter()
        .position(|event| reconnecting(event, 4))
        .unwrap();
    for event in &seen.all[..stop] {
        match change_of(event) {
            Some(Change::Replace { rows, .. }) => assert_eq!(rows.len(), 1),
            Some(Change::Remove { .. }) => panic!("a removal while reconnecting"),
            _ => {}
        }
    }
    let starts = fake.runs("", &["watch", "--json"]);
    assert!(starts.len() >= 4, "{starts:?}");
    for attempt in 1..4u32 {
        let gap = starts[attempt as usize] - starts[attempt as usize - 1];
        let delay = timing.restart_delay(attempt).as_secs_f64();
        assert!(
            gap >= delay,
            "restart {attempt} after {gap} s, before {delay} s"
        );
    }
    assert_eq!(timing.restart_delay(4), Duration::from_millis(600));
    assert_eq!(timing.restart_delay(9), Duration::from_millis(600));
    // Each start probed the CLI again.
    assert!(fake.runs("", &["-h"]).len() >= 4);
}

#[test]
fn a_fatal_error_ends_the_stream_and_the_next_one_goes_live() {
    let fake = FakeHgs::new(
        "stream_fatal",
        json!({"": {"runs": [
            [{"file": "fatal.ndjson"}, {"serve": true}],
            [{"lines": [hello("workstation"), catalog(&[("r2", "codex/sample-project/tests")])]}, {"serve": true}]
        ]}}),
    );
    let (tx, rx) = channel();
    let mut seen = Seen::new(rx);
    let handle = stream::spawn(&fake.path(), &ComputerId::local(), quick(), tx);
    let failed = seen.until("reconnecting", |event| reconnecting(event, 1));
    // The client closed the failed stream itself.
    common::eventually("EOF of the failed stream", || fake.eofs("") >= 1);
    let back = seen.until_from(failed, "live again", live);
    assert!(back > failed);
    let replaced = seen.all[failed..].iter().any(|event| {
        matches!(change_of(event), Some(Change::Replace { rows, .. })
            if rows.len() == 1 && rows[0].0 == "run:r2")
    });
    assert!(replaced);
    handle.stop();
}

#[test]
fn a_silent_stream_restarts() {
    let fake = FakeHgs::new(
        "stream_silent",
        json!({"": {"runs": [[{"lines": [hello("workstation")]}, {"await_file": "never"}]]}}),
    );
    let (tx, rx) = channel();
    let mut seen = Seen::new(rx);
    let timing = Timing {
        silence: Duration::from_millis(300),
        ..quick()
    };
    let handle = stream::spawn(&fake.path(), &ComputerId::local(), timing, tx);
    seen.until("reconnecting after silence", |event| reconnecting(event, 1));
    common::eventually("a second stream", || {
        fake.runs("", &["watch", "--json"]).len() >= 2
    });
    handle.stop();
}

#[test]
fn a_computer_without_the_marker_is_unsupported_and_never_watched() {
    let fake = FakeHgs::new(
        "stream_unsupported",
        json!({"": {"marker": false, "runs": [[{"serve": true}]]}}),
    );
    let (tx, rx) = channel();
    let mut seen = Seen::new(rx);
    let timing = Timing {
        reprobe: Duration::from_millis(50),
        ..quick()
    };
    let handle = stream::spawn(&fake.path(), &ComputerId::local(), timing, tx);
    seen.until("unsupported", |event| {
        change_of(event).and_then(|change| status_of(change, &ComputerId::local()))
            == Some(ComputerStatus::Unsupported)
    });
    common::eventually("a second probe", || fake.runs("", &["-h"]).len() >= 2);
    let session = SessionRef {
        name: "claude/sample-project/review".into(),
        ..SessionRef::default()
    };
    // A follow waits for a stream that never starts.
    assert!(handle.follow(session, None, 100).is_ok());
    handle.stop();
    assert!(fake.runs("", &["watch", "--json"]).is_empty());
    assert!(fake.stdin("").is_empty());
    assert!(!seen.all.iter().any(live));
}

#[test]
fn a_failing_probe_backs_off_without_a_stream() {
    let fake = FakeHgs::new("stream_probe_fails", json!({"": {"probe_exit": 3}}));
    let (tx, rx) = channel();
    let mut seen = Seen::new(rx);
    let handle = stream::spawn(&fake.path(), &ComputerId::local(), quick(), tx);
    seen.until("the second failure", |event| reconnecting(event, 2));
    handle.stop();
    assert!(fake.runs("", &["watch", "--json"]).is_empty());
}

#[test]
fn probes_read_the_marker_of_the_selected_computer() {
    let fake = FakeHgs::new(
        "stream_probe",
        json!({"": {}, "studio": {"marker": false}, "build-01": {"probe_exit": 1}}),
    );
    let wait = Duration::from_secs(10);
    assert_eq!(
        stream::probe(&fake.path(), &ComputerId::local(), wait),
        Probe::Supported
    );
    assert_eq!(
        stream::probe(&fake.path(), &ComputerId::from("studio"), wait),
        Probe::Unsupported
    );
    assert!(matches!(
        stream::probe(&fake.path(), &ComputerId::from("build-01"), wait),
        Probe::Failed(_)
    ));
    assert!(matches!(
        stream::probe(&fake.dir.join("missing"), &ComputerId::local(), wait),
        Probe::Failed(_)
    ));
    assert_eq!(fake.runs("studio", &["-h"]).len(), 1);
}
