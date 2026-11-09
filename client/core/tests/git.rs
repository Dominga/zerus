//! Original native fleet/supervisor and pipes, synthetic watch processes only.
mod common;
use common::{catalog, eventually, hello, meta, status_of, FakeHgs, Seen};
use serde_json::{json, Value};
use std::{sync::mpsc::channel, time::Duration};
use zerus_client_core::{
    git::RowInterest, Change, ComputerId, ComputerStatus, Fleet, GitUpdate, Timing,
};

fn greeting(cap: usize) -> Value {
    let mut h = hello("fixture");
    h["topics"].as_array_mut().unwrap().push(json!("git"));
    h["limits"]["git_subscriptions"] = json!(cap);
    h
}
fn rows() -> Value {
    let mut rows = catalog(&[("a", "codex/fixture/a"), ("b", "codex/fixture/b")]);
    for item in rows["items"].as_array_mut().unwrap() {
        item["row"]["cwd"] = json!("/fixture/project");
    }
    rows
}
#[test]
fn visible_rows_flow_through_the_original_local_and_remote_watches() {
    let fake = FakeHgs::new(
        "git_original_fleet",
        json!({
            "": {"runs":[[{"lines":[greeting(1), rows(), meta(Some(&["peer"]))]},{"serve":true}]]},
            "peer": {"runs":[[{"lines":[greeting(2), rows()]},{"serve":true}]]}
        }),
    );
    let (tx, rx) = channel();
    let mut seen = Seen::new(rx);
    let fleet = Fleet::start_with(
        fake.path(),
        tx,
        Timing {
            silence: Duration::from_secs(30),
            ..Timing::scaled(0.01)
        },
    );
    for alias in ["", "peer"] {
        seen.until("catalog ready", |c| {
            status_of(c, &ComputerId::from(alias)) == Some(ComputerStatus::Live)
        });
    }
    fleet
        .git_rows(&[
            RowInterest {
                computer: "".into(),
                key: "run:a".into(),
            },
            RowInterest {
                computer: "".into(),
                key: "run:b".into(),
            }, // Same path coalesces.
            RowInterest {
                computer: "peer".into(),
                key: "run:a".into(),
            },
        ])
        .unwrap();
    for alias in ["", "peer"] {
        seen.until("Git state", |c| {
            matches!(c, Change::Git {computer, key, update:GitUpdate::State {value,..}}
            if computer.0 == alias && key == "/fixture/project" && value.changed_files == Some(3))
        });
        let controls = fake.stdin(alias);
        assert_eq!(
            controls
                .iter()
                .filter(|c| c["op"] == "subscribe" && c["topic"] == "git")
                .count(),
            1
        );
        let subscribe = controls
            .iter()
            .position(|c| c["op"] == "subscribe" && c["topic"] == "git")
            .unwrap();
        assert_eq!(controls[subscribe - 1]["op"], "ping");
        assert_eq!(controls[subscribe - 2]["op"], "unsubscribe");
        assert!(
            controls[subscribe]["session"].is_null(),
            "native path subscription, no renderer path/session substitution"
        );
    }
    fleet.git_rows(&[]).unwrap();
    for alias in ["", "peer"] {
        eventually("old path unsubscribed", || {
            fake.stdin(alias)
                .iter()
                .filter(|c| c["op"] == "unsubscribe" && c["topic"] == "git")
                .count()
                == 2
        });
        assert_eq!(
            fake.runs(alias, &["watch", "--json"]).len(),
            1,
            "no extra watch"
        );
        assert!(!fake.log().iter().any(|entry| entry["args"]
            .as_array()
            .is_some_and(|args| args.iter().any(|v| v == "git-status"))));
    }
    fleet.stop();
}
