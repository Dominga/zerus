//! Task 76: the Teams poller against a fake hgs that answers as this computer's
//! hgs does (`team list`, `waiting`, `status`, `inbox` and `events` with the
//! request on stdin), through the same process path the app uses
//! (`TeamsPoller::spawn`). Every followed view arrives within seconds; the
//! frontend then keeps it (client/src/bridge.test.ts, the pump's `teams`).

mod common;

use common::FakeHgs;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use zerus_client_core::teams::{TeamsFollow, TeamsPoller, TeamsUpdate};
use zerus_client_core::{Change, ComputerId};

/// The team of the sanitized fixtures (tests/fixtures/teams).
const TEAM: &str = "33fbafeb-8291-4eda-a00c-6e1448ec6f20";

fn fixture(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/teams").join(name);
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn every_followed_view_arrives_from_an_hgs_answering_like_this_computer() {
    let fake = FakeHgs::new("team_reads_like_this_computer", json!({"": {}}));
    for (op, file) in [("list", "team-list.json"), ("waiting", "team-waiting.json"), ("status", "team-status.json"),
                       ("inbox", "team-inbox.json"), ("events", "team-events.json")] {
        std::fs::write(fake.dir.join(format!("team-{op}-local.json")), json!({"stdout": fixture(file)}).to_string()).unwrap();
    }
    let (sender, receiver) = mpsc::channel();
    let poller = TeamsPoller::spawn(fake.path(), ComputerId::local(), move |change| {
        let _ = sender.send(change);
    })
    .unwrap();
    // As the Sessions page with the lead selected and Waiting for you shown.
    poller.follow(TeamsFollow { list: true, waiting: true, team: Some(TEAM.to_owned()), catalog: false });
    let started = Instant::now();
    let mut seen: Vec<String> = Vec::new();
    let wanted = ["list".to_owned(), "waiting".to_owned(), format!("team:{TEAM}"), format!("inbox:{TEAM}"), format!("feed:{TEAM}")];
    while !wanted.iter().all(|key| seen.contains(key)) {
        let left = Duration::from_secs(20).saturating_sub(started.elapsed());
        let Ok(change) = receiver.recv_timeout(left) else {
            panic!("only {seen:?} arrived; log: {:?}", fake.log());
        };
        if let Change::Teams { computer, key, update, .. } = change {
            assert!(computer.is_local());
            assert!(!matches!(update, TeamsUpdate::Failed { .. }), "{key}: {update:?}");
            seen.push(key);
        }
    }
    // The reads went to `hgs team OP --json` with their request on stdin.
    let ops: Vec<String> = fake.log().iter().filter_map(|entry| entry["team"].as_str().map(str::to_owned)).collect();
    for op in ["list", "waiting", "status", "inbox", "events"] {
        assert!(ops.iter().any(|found| found == op), "{op} in {ops:?}");
    }
    let waiting = fake.log().into_iter().find(|entry| entry["team"] == "waiting").unwrap();
    assert_eq!(waiting["request"]["limit"], 100);
    assert!(started.elapsed() < Duration::from_secs(20));
}
