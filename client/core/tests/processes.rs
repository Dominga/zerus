//! Processes follows through `Fleet` against a fake `hgs`
//! (tests/support/fake_hgs.py), never the real CLI.
mod common;

use common::{catalog, eventually, hello, status_of, FakeHgs, Seen};
use serde_json::{json, Value};
use std::sync::mpsc::channel;
use std::time::Duration;
use zerus_client_core::{
    Change, ComputerId, ComputerStatus, Fleet, FollowError, ProcessesUpdate, SessionRef, Timing,
};

fn quick() -> Timing {
    Timing {
        stop_grace: Duration::from_millis(500),
        ..Timing::scaled(0.002)
    }
}

/// A hello that offers the processes topic with `limit` subscriptions.
fn hello_with_processes(limit: usize) -> Value {
    let mut hello = hello("workstation");
    hello["topics"]
        .as_array_mut()
        .unwrap()
        .push(json!("processes"));
    hello["limits"]["processes_subscriptions"] = json!(limit);
    hello
}

fn session(run: &str, name: &str) -> SessionRef {
    SessionRef {
        name: name.into(),
        run_id: Some(run.into()),
        conversation_id: None,
    }
}

fn live(change: &Change) -> bool {
    status_of(change, &ComputerId::local()) == Some(ComputerStatus::Live)
}

fn processes<'a>(change: &'a Change, wanted: &str) -> Option<&'a ProcessesUpdate> {
    match change {
        Change::Processes {
            computer,
            key,
            update,
        } if computer.is_local() && key == wanted => Some(update),
        _ => None,
    }
}

/// The processes subscriptions the fake read, as `(op, run_id)`.
fn controls(fake: &FakeHgs) -> Vec<(String, String)> {
    fake.stdin("")
        .into_iter()
        .filter(|control| control["topic"] == "processes")
        .map(|control| {
            (
                control["op"].as_str().unwrap().to_owned(),
                control["session"]["run_id"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

#[test]
fn followed_processes_stream_until_unfollowed_or_removed_within_the_limit() {
    let value = json!({"processes": {"items": [], "active_count": 1, "sampled_at": 1.0}, "process_work": null});
    let fake = FakeHgs::new(
        "processes_follow",
        json!({"": {"runs": [[
            {"lines": [hello_with_processes(2), catalog(&[("r1", "claude/sample-project/review"), ("r2", "codex/sample-project/tests")])]},
            {"await_file": "followed"},
            {"lines": [
                {"t": "state", "topic": "processes", "key": "run:r1", "value": value, "rev": 1},
                {"t": "state", "topic": "processes", "key": "run:r3", "value": value, "rev": 1},
                {"t": "error", "code": "inspect_failed", "message": "ps failed", "topic": "processes", "key": "run:r1", "fatal": false},
                {"t": "delta", "topic": "catalog", "upsert": [], "remove": [{"key": "run:r2", "reason": "ended"}], "cursor": 3}
            ]},
            {"serve": true}
        ]]}}),
    );
    let (tx, rx) = channel();
    let mut seen = Seen::new(rx);
    let fleet = Fleet::start_local(fake.path(), tx, quick());
    let local = ComputerId::local();
    seen.until("live", live);
    // A session the live catalog lacks ends at once and frees its place.
    assert_eq!(
        fleet.follow_processes(&local, session("r9", "claude/sample-project/gone")),
        Ok("run:r9".into())
    );
    seen.until("the missing session's end", |change| {
        processes(change, "run:r9") == Some(&ProcessesUpdate::Ended)
    });
    eventually("the end frees the place", || {
        fleet.followed_processes(&local).is_empty()
    });
    for (run, name) in [
        ("r1", "claude/sample-project/review"),
        ("r2", "codex/sample-project/tests"),
    ] {
        assert_eq!(
            fleet.follow_processes(&local, session(run, name)),
            Ok(format!("run:{run}"))
        );
    }
    // Following again is idempotent; the stream's limit of two refuses a third.
    assert_eq!(
        fleet.follow_processes(&local, session("r1", "claude/sample-project/review")),
        Ok("run:r1".into())
    );
    assert_eq!(
        fleet.follow_processes(&local, session("r3", "claude/sample-project/third")),
        Err(FollowError::TooMany)
    );
    assert_eq!(
        fleet.follow_processes(&ComputerId::from("nowhere"), session("r1", "x")),
        Err(FollowError::UnknownComputer)
    );
    fake.touch("followed");
    let index = seen.until("the state", |change| {
        matches!(
            processes(change, "run:r1"),
            Some(ProcessesUpdate::State { rev: 1, .. })
        )
    });
    let Some(ProcessesUpdate::State { value: got, .. }) = processes(&seen.all[index], "run:r1")
    else {
        unreachable!()
    };
    assert_eq!(*got, value);
    seen.until("the failure", |change| {
        matches!(processes(change, "run:r1"), Some(ProcessesUpdate::Failed { code, .. }) if code == "inspect_failed")
    });
    // The removed session's follow ends; an unfollowed key's frames change nothing.
    seen.until("the removal's end", |change| {
        processes(change, "run:r2") == Some(&ProcessesUpdate::Ended)
    });
    assert!(seen
        .all
        .iter()
        .all(|change| processes(change, "run:r3").is_none()));
    assert_eq!(fleet.followed_processes(&local), vec!["run:r1".to_owned()]);
    assert!(fleet.unfollow_processes(&local, "run:r1"));
    assert!(!fleet.unfollow_processes(&local, "run:r1"));
    eventually("the unsubscriptions", || controls(&fake).len() == 4);
    assert_eq!(
        controls(&fake),
        vec![
            ("subscribe".into(), "r1".into()),
            ("subscribe".into(), "r2".into()),
            ("unsubscribe".into(), "r2".into()),
            ("unsubscribe".into(), "r1".into()),
        ]
    );
    // The Activity follow is separate.
    assert!(fleet.followed(&local).is_empty());
    fleet.stop();
}

#[test]
fn a_cli_without_the_topic_stays_quiet() {
    let fake = FakeHgs::new(
        "processes_unsupported",
        json!({"": {"runs": [[
            {"lines": [hello("workstation"), catalog(&[("r1", "claude/sample-project/review")])]},
            {"serve": true}
        ]]}}),
    );
    let (tx, rx) = channel();
    let mut seen = Seen::new(rx);
    let fleet = Fleet::start_local(fake.path(), tx, quick());
    let local = ComputerId::local();
    seen.until("live", live);
    assert_eq!(
        fleet.follow_processes(&local, session("r1", "claude/sample-project/review")),
        Ok("run:r1".into())
    );
    fleet
        .set_processes_interval(&local, "run:r1", 2_500)
        .unwrap();
    fleet.refresh_processes(&local, "run:r1").unwrap();
    // A ping written after the follow shows the stream read past it.
    assert!(fleet.send(
        &local,
        zerus_watch_protocol::Control::Ping(zerus_watch_protocol::Ping { id: Some(json!(1)) })
    ));
    eventually("the ping", || !fake.stdin("").is_empty());
    seen.drain_for(Duration::from_millis(200));
    assert!(controls(&fake).is_empty(), "{:?}", fake.stdin(""));
    assert!(seen
        .all
        .iter()
        .all(|change| !matches!(change, Change::Processes { .. })));
    assert_eq!(fleet.followed_processes(&local), vec!["run:r1".to_owned()]);
    assert!(fleet.unfollow_processes(&local, "run:r1"));
    fleet.stop();
    assert!(controls(&fake).is_empty());
}

#[test]
fn a_new_stream_with_a_lower_limit_ends_only_the_follows_beyond_it() {
    let rows = [
        ("r1", "claude/sample-project/one"),
        ("r2", "claude/sample-project/two"),
        ("r3", "claude/sample-project/three"),
    ];
    let fake = FakeHgs::new(
        "processes_lower_limit",
        json!({"": {"runs": [
            [{"lines": [hello_with_processes(16), catalog(&rows)]}, {"await_file": "followed"}],
            [{"lines": [hello_with_processes(2), catalog(&rows)]}, {"serve": true}]
        ]}}),
    );
    let (tx, rx) = channel();
    let mut seen = Seen::new(rx);
    let fleet = Fleet::start_local(fake.path(), tx, quick());
    let local = ComputerId::local();
    seen.until("live", live);
    for (run, name) in rows {
        assert!(fleet.follow_processes(&local, session(run, name)).is_ok());
    }
    fake.touch("followed");
    seen.until("the end beyond the new limit", |change| {
        processes(change, "run:r3") == Some(&ProcessesUpdate::Ended)
    });
    assert_eq!(
        fleet.followed_processes(&local),
        vec!["run:r1".to_owned(), "run:r2".to_owned()]
    );
    eventually("the subscriptions of the new stream", || {
        controls(&fake).len() == 2
    });
    assert_eq!(
        controls(&fake),
        vec![
            ("subscribe".into(), "r1".into()),
            ("subscribe".into(), "r2".into())
        ]
    );
    assert!(seen.all.iter().all(|change| {
        processes(change, "run:r1") != Some(&ProcessesUpdate::Ended)
            && processes(change, "run:r2") != Some(&ProcessesUpdate::Ended)
    }));
    fleet.stop();
}

#[test]
fn interval_and_refresh_controls_preserve_the_follow_and_reconnect_cadence() {
    let rows = [("r1", "codex/sample-project/one")];
    let value = json!({"processes": {"items": [], "active_count": 1}, "process_work": null});
    let fake = FakeHgs::new(
        "processes_interval",
        json!({"": {"runs": [
            [
                {"lines": [hello_with_processes(16), catalog(&rows)]},
                {"await_file": "followed"},
                {"lines": [{"t": "state", "topic": "processes", "key": "run:r1", "value": value, "rev": 7}]},
                {"serve": true, "until_file": "reconnect"}
            ],
            [{"lines": [hello_with_processes(16), catalog(&rows)]}, {"serve": true}]
        ]}}),
    );
    let (tx, rx) = channel();
    let mut seen = Seen::new(rx);
    let fleet = Fleet::start_local(fake.path(), tx, quick());
    let local = ComputerId::local();
    seen.until("live", live);
    fleet
        .follow_processes(&local, session("r1", "codex/sample-project/one"))
        .unwrap();
    fake.touch("followed");
    eventually("the original follow", || controls(&fake).len() == 1);
    seen.until("the current inventory", |change| {
        matches!(
            processes(change, "run:r1"),
            Some(ProcessesUpdate::State { rev: 7, .. })
        )
    });
    fleet
        .set_processes_interval(&local, "run:r1", 2_500)
        .unwrap();
    eventually("the active cadence", || {
        fake.stdin("")
            .iter()
            .any(|line| line["interval_ms"] == 2_500)
    });
    fleet.refresh_processes(&local, "run:r1").unwrap();
    eventually("one refresh", || {
        fake.stdin("").iter().any(|line| {
            line["topic"] == "processes" && line["refresh"] == true && line["interval_ms"] == 2_500
        })
    });
    fleet
        .set_processes_interval(&local, "run:r1", u64::MAX)
        .unwrap();
    eventually("the bounded cadence", || {
        fake.stdin("")
            .iter()
            .any(|line| line["interval_ms"] == 600_000)
    });
    fleet
        .follow_processes(&local, session("r1", "codex/sample-project/renamed"))
        .unwrap();
    eventually("the renamed reference keeps its cadence", || {
        fake.stdin("").iter().any(|line| {
            line["session"]["name"] == "codex/sample-project/renamed"
                && line["interval_ms"] == 600_000
        })
    });
    assert_eq!(fleet.followed_processes(&local), ["run:r1"]);
    seen.drain_for(Duration::from_millis(50));
    assert!(!seen
        .all
        .iter()
        .any(|change| processes(change, "run:r1") == Some(&ProcessesUpdate::Ended)));
    assert!(controls(&fake).iter().all(|(op, _)| op == "subscribe"));
    fake.touch("reconnect");
    eventually("the cadence after reconnect", || {
        fake.stdin("")
            .iter()
            .filter(|line| line["interval_ms"] == 600_000)
            .count()
            >= 2
    });
    assert_eq!(
        fleet.set_processes_interval(&local, "missing", 1_000),
        Err(FollowError::KeyMismatch)
    );
    assert_eq!(
        fleet.refresh_processes(&local, "missing"),
        Err(FollowError::KeyMismatch)
    );
    assert_eq!(
        fleet.refresh_processes(&ComputerId::from("missing"), "run:r1"),
        Err(FollowError::UnknownComputer)
    );
    fleet.stop();
}
