//! Task list follows through `Fleet` against a fake `hgs`
//! (tests/support/fake_hgs.py), never the real CLI.
mod common;

use common::{catalog, eventually, hello, meta, status_of, FakeHgs, Seen};
use serde_json::{json, Value};
use std::sync::mpsc::channel;
use std::time::Duration;
use zerus_client_core::{
    Change, ComputerId, ComputerStatus, Fleet, FollowError, TasksUpdate, Timing,
};

fn quick() -> Timing {
    Timing {
        stop_grace: Duration::from_millis(500),
        ..Timing::scaled(0.002)
    }
}

/// A hello that offers the tasks topic with `limit` subscriptions.
fn hello_with_tasks(limit: usize) -> Value {
    let mut hello = hello("workstation");
    hello["topics"].as_array_mut().unwrap().push(json!("tasks"));
    hello["limits"]["tasks_subscriptions"] = json!(limit);
    hello
}

/// One snapshot page of a project's list; the last carries its summary.
fn page(project: &str, tasks: &[&str], page: u32, digest: Option<&str>) -> Value {
    let mut frame = json!({"t": "snapshot", "topic": "tasks", "key": project, "page": page,
        "last": digest.is_some(), "cursor": 0,
        "items": tasks.iter().map(|text| json!({"text": text})).collect::<Vec<_>>()});
    if let Some(digest) = digest {
        frame["value"] =
            json!({"project": {"id": project, "name": "orchard"}, "damaged": [], "digest": digest});
    }
    frame
}

fn live(change: &Change) -> bool {
    status_of(change, &ComputerId::local()) == Some(ComputerStatus::Live)
}

fn tasks<'a>(change: &'a Change, wanted: &str) -> Option<&'a TasksUpdate> {
    match change {
        Change::Tasks {
            computer,
            key,
            update,
        } if computer.is_local() && key == wanted => Some(update),
        _ => None,
    }
}

fn texts(update: &TasksUpdate) -> Vec<String> {
    match update {
        TasksUpdate::List { tasks, .. } => tasks
            .iter()
            .map(|task| task["text"].as_str().unwrap().to_owned())
            .collect(),
        other => panic!("not a list: {other:?}"),
    }
}

/// The tasks subscriptions the fake read, as `(op, project)`.
fn controls(fake: &FakeHgs) -> Vec<(String, String)> {
    fake.stdin("")
        .into_iter()
        .filter(|control| control["topic"] == "tasks")
        .map(|control| {
            (
                control["op"].as_str().unwrap().to_owned(),
                control["project"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

#[test]
fn a_followed_project_republishes_after_reconnect_and_delivers_changes() {
    let fake = FakeHgs::new(
        "tasks_follow",
        json!({"": {"runs": [
            [{"lines": [hello_with_tasks(1), catalog(&[])]}, {"await_file": "followed"},
             {"lines": [page("p1", &["a", "b"], 0, None), page("p1", &["c"], 1, Some("d1")),
                        page("p2", &["x"], 0, Some("d9"))]},
             {"await_file": "again"}],
            [{"lines": [hello_with_tasks(1), catalog(&[]), page("p1", &["a", "b", "c"], 0, Some("d1")),
                        page("p1", &["a"], 0, Some("d2")),
                        {"t": "error", "code": "tasks_failed", "message": "unknown project", "topic": "tasks", "key": "p1", "fatal": false}]},
             {"serve": true}]
        ]}}),
    );
    let (tx, rx) = channel();
    let mut seen = Seen::new(rx);
    let fleet = Fleet::start_local(fake.path(), tx, quick());
    let local = ComputerId::local();
    seen.until("live", live);
    assert_eq!(fleet.follow_tasks(&local, "p1"), Ok("p1".into()));
    assert_eq!(fleet.follow_tasks(&local, "p1"), Ok("p1".into()));
    assert_eq!(fleet.follow_tasks(&local, "p2"), Err(FollowError::TooMany));
    assert_eq!(
        fleet.follow_tasks(&local, ""),
        Err(FollowError::InvalidProject)
    );
    assert_eq!(
        fleet.follow_tasks(&local, "bad\nproject"),
        Err(FollowError::InvalidProject)
    );
    fake.touch("followed");
    let first = seen.until("the first list", |change| tasks(change, "p1").is_some());
    assert_eq!(
        texts(tasks(&seen.all[first], "p1").unwrap()),
        ["a", "b", "c"]
    );
    // A reconnect republishes d1 to clear its connection error, then d2 changes it.
    fake.touch("again");
    let second = seen.until_from(first + 1, "the changed list", |change| {
        matches!(tasks(change, "p1"), Some(TasksUpdate::List { digest, .. }) if digest == "d2")
    });
    let Some(TasksUpdate::List { digest, .. }) = tasks(&seen.all[second], "p1") else {
        panic!("{:?}", seen.all[second])
    };
    assert_eq!(digest, "d2");
    assert_eq!(texts(tasks(&seen.all[second], "p1").unwrap()), ["a"]);
    seen.until_from(second + 1, "the failure", |change| {
        matches!(tasks(change, "p1"), Some(TasksUpdate::Failed { code, .. }) if code == "tasks_failed")
    });
    assert!(seen.all.iter().all(|change| tasks(change, "p2").is_none()));
    assert!(fleet.unfollow_tasks(&local, "p1"));
    assert!(!fleet.unfollow_tasks(&local, "p1"));
    eventually("the unsubscription", || controls(&fake).len() == 2);
    assert_eq!(
        controls(&fake),
        vec![
            ("subscribe".into(), "p1".into()),
            ("unsubscribe".into(), "p1".into())
        ]
    );
    assert!(fleet.followed_tasks(&local).is_empty());
    fleet.stop();
}

#[test]
fn a_cli_without_the_topic_reports_unsupported_and_a_refusal_ends_the_follow() {
    let fake = FakeHgs::new(
        "tasks_unsupported",
        json!({"": {"runs": [
            [{"lines": [hello("workstation"), catalog(&[])]}, {"await_file": "followed"}],
            [{"lines": [hello_with_tasks(8), catalog(&[]),
                        {"t": "error", "code": "too_many_subscriptions", "message": "no", "topic": "tasks", "key": "p1", "fatal": false}]},
             {"serve": true}]
        ]}}),
    );
    let (tx, rx) = channel();
    let mut seen = Seen::new(rx);
    let fleet = Fleet::start_local(fake.path(), tx, quick());
    let local = ComputerId::local();
    seen.until("live", live);
    assert_eq!(fleet.follow_tasks(&local, "p1"), Ok("p1".into()));
    seen.until("unsupported task topic", |change| {
        matches!(tasks(change, "p1"), Some(TasksUpdate::Failed { code, .. }) if code == "unsupported_topic")
    });
    assert!(controls(&fake).is_empty());
    // The next stream offers the topic, subscribes and is refused.
    fake.touch("followed");
    seen.until("the refusal's end", |change| {
        tasks(change, "p1") == Some(&TasksUpdate::Ended)
    });
    assert!(fleet.followed_tasks(&local).is_empty());
    eventually("the subscription", || !controls(&fake).is_empty());
    assert_eq!(controls(&fake), vec![("subscribe".into(), "p1".into())]);
    fleet.stop();
}

#[test]
fn peer_session_tasks_use_the_local_replica_and_reconnect_restores_the_same_digest() {
    let fake = FakeHgs::new(
        "tasks_two_computers",
        json!({
            "": {"runs": [
                [{"lines": [hello_with_tasks(8), catalog(&[]), meta(Some(&["peer"]))]},
                 {"await_file": "follow"}, {"lines": [page("project-id", &["local replica task"], 0, Some("same"))]},
                 {"serve": true, "until_file": "disconnect"}],
                [{"await_file": "reconnect"}, {"lines": [hello_with_tasks(8), catalog(&[]),
                 page("project-id", &["local replica task"], 0, Some("same"))]}, {"serve": true}]
            ]},
            "peer": {"runs": [[{"lines": [hello_with_tasks(8), catalog(&[("peer-run", "codex/example/remote")])]}, {"serve": true}]]}
        }),
    );
    let (tx, rx) = channel();
    let mut seen = Seen::new(rx);
    let fleet = Fleet::start_with(fake.path(), tx, quick());
    seen.until("peer session catalog", |c| matches!(c, Change::Replace { computer, rows } if computer.0 == "peer" && !rows.is_empty()));
    fleet
        .follow_tasks(&ComputerId::local(), "project-id")
        .unwrap();
    fake.touch("follow");
    seen.until("local replica tasks", |c| {
        matches!(tasks(c, "project-id"), Some(TasksUpdate::List { .. }))
    });
    fake.touch("disconnect");
    let failed = seen.until("task disconnect state", |c| matches!(tasks(c, "project-id"), Some(TasksUpdate::Failed { code, .. }) if code == "disconnected"));
    fake.touch("reconnect");
    let restored = seen.until_from(failed + 1, "unchanged list clears failure", |c| matches!(tasks(c, "project-id"), Some(TasksUpdate::List { digest, .. }) if digest == "same"));
    assert_eq!(
        texts(tasks(&seen.all[restored], "project-id").unwrap()),
        ["local replica task"]
    );
    eventually("local tasks subscription", || !controls(&fake).is_empty());
    assert!(fake.stdin("peer").iter().all(|c| c["topic"] != "tasks"));
    assert!(controls(&fake)
        .iter()
        .all(|(_, project)| project == "project-id"));
    fleet.stop();
}

#[test]
fn a_polled_legacy_cli_reports_unsupported_tasks_without_starting_a_task_poller() {
    let fake = FakeHgs::new("tasks_legacy_unsupported", json!({"": {"marker": false}}));
    std::fs::write(
        fake.dir.join("ls-local.json"),
        json!({"sessions": []}).to_string(),
    )
    .unwrap();
    let (tx, rx) = channel();
    let mut seen = Seen::new(rx);
    let fleet = Fleet::start_local(fake.path(), tx, quick());
    seen.until("legacy catalog", |c| {
        status_of(c, &ComputerId::local()) == Some(ComputerStatus::Polled)
    });
    fleet
        .follow_tasks(&ComputerId::local(), "project-id")
        .unwrap();
    seen.until("unsupported legacy task follow", |c| matches!(tasks(c, "project-id"), Some(TasksUpdate::Failed { code, .. }) if code == "unsupported_topic"));
    assert!(fake.log().iter().all(|entry| entry["args"][0] != "task"));
    fleet.stop();
}
