//! `Fleet` against a fake `hgs` (tests/support/fake_hgs.py), never the real CLI.
mod common;

use common::{catalog, eventually, hello, meta, status_of, upsert, FakeHgs, Seen};
use serde_json::json;
use std::sync::mpsc::channel;
use std::time::Duration;
use zerus_client_core::{
    ActivityUpdate, Change, ComputerId, ComputerStatus, Fleet, FollowError, SessionRef, Timing,
};

fn quick() -> Timing {
    Timing {
        stop_grace: Duration::from_millis(500),
        ..Timing::scaled(0.002)
    }
}

fn is_status(change: &Change, computer: &str, status: ComputerStatus) -> bool {
    status_of(change, &ComputerId::from(computer)) == Some(status)
}

fn upserted(change: &Change, computer: &str, key: &str) -> bool {
    matches!(change, Change::Upsert { computer: owner, key: k, .. } if owner.0 == computer && k == key)
}

#[test]
fn a_local_fleet_starts_no_peer_streams() {
    let fake = FakeHgs::new(
        "fleet_local_only",
        json!({
            "": {"runs": [[{"lines": [hello("workstation"), catalog(&[("l1", "claude/sample-project/local")]),
                meta(Some(&["alpha"])), upsert("l2", "claude/sample-project/after-meta")]}, {"serve": true}]]},
            "alpha": {"runs": [[{"serve": true}]]}
        }),
    );
    let (tx, rx) = channel();
    let mut seen = Seen::new(rx);
    let fleet = Fleet::start_local(fake.path(), tx, quick());
    seen.until("the upsert after the meta", |c| upserted(c, "", "run:l2"));
    assert_eq!(fleet.computers(), vec![ComputerId::local()]);
    // The listed peers are known even without their streams.
    assert_eq!(fleet.peers(), Some(["alpha".to_owned()].into()));
    fleet.stop();
    assert_eq!(fake.hosts_started(), vec![String::new(), String::new()]);
}

#[test]
fn peers_stay_unknown_until_a_listing_names_them() {
    let fake = FakeHgs::new(
        "fleet_peers_unknown",
        json!({
            "": {"runs": [[{"lines": [hello("workstation"), catalog(&[("l1", "claude/sample-project/local")]),
                meta(None), upsert("l2", "claude/sample-project/after-meta")]}, {"serve": true}]]}
        }),
    );
    let (tx, rx) = channel();
    let mut seen = Seen::new(rx);
    let fleet = Fleet::start_with(fake.path(), tx, quick());
    assert_eq!(fleet.peers(), None);
    seen.until("the upsert after the meta", |c| upserted(c, "", "run:l2"));
    // An older host's listing has no peers array: still unknown.
    assert_eq!(fleet.peers(), None);
    fleet.stop();
}

#[test]
fn the_fleet_follows_the_peers_this_computer_lists() {
    let fake = FakeHgs::new(
        "fleet_peers",
        json!({
            "": {"runs": [[
                {"lines": [hello("workstation"), catalog(&[("l1", "claude/sample-project/local")]),
                    meta(Some(&["alpha", "beta"]))]},
                {"await_file": "step2"},
                {"lines": [meta(Some(&["beta"]))]},
                {"await_file": "step3"},
                {"lines": [meta(None), upsert("l2", "claude/sample-project/after-meta")]},
                {"serve": true}
            ]]},
            "alpha": {"runs": [[
                {"lines": [hello("a-name-alpha-calls-itself"), catalog(&[("a1", "codex/sample-project/on-alpha")])]},
                {"serve": true}
            ]]},
            "beta": {"runs": [[
                {"lines": [hello("beta.example.com"), catalog(&[("b1", "kimi/docs-site/on-beta")]),
                    meta(Some(&["gamma"])), upsert("b2", "kimi/docs-site/after-meta")]},
                {"serve": true}
            ]]}
        }),
    );
    let (tx, rx) = channel();
    let mut seen = Seen::new(rx);
    let fleet = Fleet::start_with(fake.path(), tx, quick());
    seen.until("alpha live", |c| {
        is_status(c, "alpha", ComputerStatus::Live)
    });
    seen.until("beta live", |c| is_status(c, "beta", ComputerStatus::Live));
    // beta's own peers came before this upsert and were ignored.
    seen.until("beta after its meta", |c| upserted(c, "beta", "run:b2"));
    let listed = |aliases: &[&str]| Some(aliases.iter().map(|a| (*a).to_owned()).collect());
    assert_eq!(fleet.peers(), listed(&["alpha", "beta"]));
    let alpha_rows = seen
        .all
        .iter()
        .find_map(|change| match change {
            Change::Replace { computer, rows } if computer.0 == "alpha" => Some(rows.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(alpha_rows.len(), 1);
    assert_eq!(alpha_rows[0].1.machine, "alpha", "keyed by the alias");
    assert_eq!(
        fleet.computers(),
        vec![
            ComputerId::local(),
            ComputerId::from("alpha"),
            ComputerId::from("beta")
        ]
    );

    // A peer that leaves the list stops; its rows leave.
    fake.touch("step2");
    let ended = seen.until("alpha ended", |c| {
        is_status(c, "alpha", ComputerStatus::Ended)
    });
    assert!(seen.all[..ended].iter().any(|change| matches!(change,
        Change::Replace { computer, rows } if computer.0 == "alpha" && rows.is_empty())));
    eventually("alpha's stream EOF", || fake.eofs("alpha") == 1);
    assert_eq!(fleet.peers(), listed(&["beta"]));

    // A listing without a peers array keeps the previous peers.
    fake.touch("step3");
    seen.until("the local upsert after the older meta", |c| {
        upserted(c, "", "run:l2")
    });
    seen.drain_for(Duration::from_millis(100));
    assert!(!seen
        .all
        .iter()
        .any(|c| is_status(c, "beta", ComputerStatus::Ended)));
    assert_eq!(
        fleet.computers(),
        vec![ComputerId::local(), ComputerId::from("beta")]
    );
    assert_eq!(fleet.peers(), listed(&["beta"]));
    assert!(!fake.hosts_started().iter().any(|host| host == "gamma"));
    assert_eq!(fake.runs("alpha", &["watch", "--json"]).len(), 1);
    assert_eq!(fake.runs("beta", &["watch", "--json"]).len(), 1);

    fleet.stop();
    seen.drain_for(Duration::from_millis(50));
    for computer in ["", "beta"] {
        assert!(
            seen.all
                .iter()
                .any(|c| is_status(c, computer, ComputerStatus::Ended)),
            "{computer}"
        );
    }
}

#[test]
fn a_peer_without_the_marker_is_unsupported_and_the_others_stay_live() {
    let fake = FakeHgs::new(
        "fleet_unsupported_peer",
        json!({
            "": {"runs": [[{"lines": [hello("workstation"), catalog(&[("l1", "claude/sample-project/local")]),
                meta(Some(&["older", "newer"]))]}, {"serve": true}]]},
            "older": {"marker": false},
            "newer": {"runs": [[{"lines": [hello("newer"), catalog(&[("n1", "codex/sample-project/newer")])]},
                {"serve": true}]]}
        }),
    );
    let (tx, rx) = channel();
    let mut seen = Seen::new(rx);
    let fleet = Fleet::start_with(fake.path(), tx, quick());
    seen.until("older unsupported", |c| {
        is_status(c, "older", ComputerStatus::Unsupported)
    });
    seen.until("newer live", |c| {
        is_status(c, "newer", ComputerStatus::Live)
    });
    seen.until("local live", |c| is_status(c, "", ComputerStatus::Live));
    assert!(fake.runs("older", &["watch", "--json"]).is_empty());
    assert_eq!(fake.runs("older", &["-h"]).len(), 1);
    let session = SessionRef {
        name: "kimi/sample-project/older".into(),
        ..SessionRef::default()
    };
    // Following there waits for a stream; nothing is written anywhere.
    assert_eq!(
        fleet.follow(
            &ComputerId::from("older"),
            &session.key(),
            session,
            None,
            100
        ),
        Ok(())
    );
    fleet.stop();
    assert!(fake.stdin("older").is_empty());
    assert!(!seen
        .all
        .iter()
        .any(|c| is_status(c, "older", ComputerStatus::Live)));
}

fn session(run: &str) -> SessionRef {
    SessionRef {
        name: format!("claude/sample-project/{run}"),
        run_id: Some(run.to_owned()),
        conversation_id: Some(format!("conv-{run}")),
    }
}

#[test]
fn follow_subscribes_with_a_tail_and_unfollow_unsubscribes() {
    let fake = FakeHgs::new(
        "fleet_follow",
        json!({"": {"runs": [[{"lines": [hello("workstation"), catalog(&[("r1", "claude/sample-project/r1")]),
            meta(Some(&[]))]}, {"serve": true}]]}}),
    );
    let (tx, rx) = channel();
    let mut seen = Seen::new(rx);
    let fleet = Fleet::start_with(fake.path(), tx, quick());
    let local = ComputerId::local();
    seen.until("live", |c| is_status(c, "", ComputerStatus::Live));

    assert_eq!(
        fleet.follow(&local, "run:r1", session("r1"), None, 100),
        Ok(())
    );
    seen.until("events", |change| {
        matches!(change, Change::Activity { key, update: ActivityUpdate::Events { events, cursor: 1 }, .. }
            if key == "run:r1" && events.len() == 1)
    });
    assert_eq!(
        fleet.follow(&local, "run:other", session("r1"), None, 100),
        Err(FollowError::KeyMismatch)
    );
    assert_eq!(
        fleet.follow(
            &ComputerId::from("nowhere"),
            "run:r1",
            session("r1"),
            None,
            100
        ),
        Err(FollowError::UnknownComputer)
    );
    for n in 2..=16 {
        let run = format!("r{n}");
        assert_eq!(
            fleet.follow(&local, &format!("run:{run}"), session(&run), None, 100),
            Ok(())
        );
    }
    assert_eq!(
        fleet.follow(&local, "run:r17", session("r17"), None, 100),
        Err(FollowError::TooMany)
    );
    assert_eq!(
        fleet.follow(&local, "run:r1", session("r1"), None, 100),
        Ok(()),
        "already followed"
    );
    assert_eq!(fleet.followed(&local).len(), 16);
    assert!(fleet.unfollow(&local, "run:r1"));
    assert!(!fleet.unfollow(&local, "run:r1"));
    assert_eq!(
        fleet.follow(&local, "run:r17", session("r17"), Some(40), 100),
        Ok(())
    );
    eventually("18 control lines", || fake.stdin("").len() >= 18);
    let controls = fake.stdin("");
    assert_eq!(
        controls[0],
        json!({"op": "subscribe", "topic": "activity", "session": {"name": "claude/sample-project/r1",
            "run_id": "r1", "conversation_id": "conv-r1"}, "tail": 100})
    );
    assert_eq!(controls[16]["op"], "unsubscribe");
    assert_eq!(controls[16]["session"]["run_id"], "r1");
    assert_eq!(controls[17]["after"], 40);
    assert!(controls[17].get("tail").is_none());
    fleet.stop();
}

#[test]
fn a_reconnect_subscribes_again_after_the_last_cursor() {
    let fake = FakeHgs::new(
        "fleet_resubscribe",
        json!({"": {"runs": [
            [{"lines": [hello("workstation"), catalog(&[("r1", "claude/sample-project/r1")])]},
                {"serve": true, "until_file": "drop"}],
            [{"lines": [hello("workstation"), catalog(&[("r1", "claude/sample-project/r1")])]}, {"serve": true}]
        ]}}),
    );
    let (tx, rx) = channel();
    let mut seen = Seen::new(rx);
    let fleet = Fleet::start_with(fake.path(), tx, quick());
    let local = ComputerId::local();
    seen.until("live", |c| is_status(c, "", ComputerStatus::Live));
    fleet
        .follow(&local, "run:r1", session("r1"), None, 100)
        .unwrap();
    seen.until("events", |change| matches!(change, Change::Activity { .. }));
    fake.touch("drop");
    let failed = seen.until("reconnecting", |c| {
        is_status(c, "", ComputerStatus::Reconnecting { attempt: 1 })
    });
    seen.until_from(failed, "live again", |c| {
        is_status(c, "", ComputerStatus::Live)
    });
    eventually("the second subscription", || fake.stdin("").len() >= 2);
    let controls = fake.stdin("");
    assert_eq!(controls[0]["tail"], 100);
    assert_eq!(controls[1]["op"], "subscribe");
    assert_eq!(controls[1]["after"], 1);
    assert!(controls[1].get("tail").is_none());
    assert_eq!(fleet.followed(&local), vec!["run:r1".to_owned()]);
    fleet.stop();
}
