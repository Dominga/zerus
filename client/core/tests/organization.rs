//! Organization follows through `Fleet` against a fake `hgs`
//! (tests/support/fake_hgs.py), never the real CLI.
mod common;

use common::{catalog, eventually, hello, status_of, FakeHgs, Seen};
use serde_json::{json, Value};
use std::sync::mpsc::channel;
use std::time::Duration;
use zerus_client_core::{Change, ComputerId, ComputerStatus, Fleet, OrganizationUpdate, Timing};

fn quick() -> Timing {
    Timing {
        stop_grace: Duration::from_millis(500),
        ..Timing::scaled(0.002)
    }
}

/// `swarm get`'s object with one project and `sessions` members.
fn swarm(sessions: &[&str], digest: &str, current: bool) -> Value {
    json!({
        "schema": 1, "node_id": "node-1", "swarm_id": "swarm-1", "digest": digest, "initialized": true,
        "conflicts": [], "preferences": {}, "machines": [], "peers": {},
        "worker": {"running": true, "current": current},
        "organization": {"version": 2, "default_project": "ungrouped",
            "projects": [{"id": "alpha", "name": "Alpha", "color": "#336699", "accessible": true,
                          "folders": [], "sessions": sessions}]},
        "versions": {"project:alpha:name": [format!("op-{digest}")]}
    })
}

/// The rows the server sends for one `swarm get` object (see
/// src/watch/organization.rs), as `{key, row}` items.
fn rows(full: &Value) -> Vec<Value> {
    let mut summary = full.clone();
    summary.as_object_mut().unwrap().remove("versions");
    summary["organization"]["projects"] = json!(["alpha"]);
    let mut project = full["organization"]["projects"][0].clone();
    let sessions = project.as_object_mut().unwrap().remove("sessions").unwrap();
    project["session_chunks"] = json!(1);
    let mut items = vec![
        json!({"key": "organization", "row": summary}),
        json!({"key": "project:alpha", "row": project}),
        json!({"key": "members:alpha:0", "row": {"sessions": sessions}}),
    ];
    for (token, operations) in full["versions"].as_object().unwrap() {
        items.push(json!({"key": format!("version:{token}"), "row": operations}));
    }
    items
}

fn hello_with_organization() -> Value {
    let mut hello = hello("workstation");
    hello["topics"]
        .as_array_mut()
        .unwrap()
        .push(json!("organization"));
    hello
}

fn live(change: &Change) -> bool {
    status_of(change, &ComputerId::local()) == Some(ComputerStatus::Live)
}

fn state(change: &Change) -> Option<(&Value, &str)> {
    match change {
        Change::Organization {
            update: OrganizationUpdate::State { value, digest, .. },
            ..
        } => Some((value.as_ref(), digest.as_str())),
        _ => None,
    }
}

fn controls(fake: &FakeHgs) -> Vec<Value> {
    fake.stdin("")
        .into_iter()
        .filter(|control| control["topic"] == "organization")
        .collect()
}

fn swarm_gets(fake: &FakeHgs) -> usize {
    fake.runs("", &["swarm", "get"]).len()
}

#[test]
fn a_stream_with_the_topic_delivers_rows_as_states_and_an_edit_snapshot() {
    let first = swarm(&["workstation\ncodex/alpha/one"], "d1", true);
    let second = swarm(
        &[
            "workstation\ncodex/alpha/one",
            "workstation\ncodex/alpha/two",
        ],
        "d2",
        false,
    );
    let changed: Vec<Value> = rows(&second)
        .into_iter()
        .filter(|item| item["key"] != "project:alpha")
        .collect();
    let fake = FakeHgs::new(
        "organization_stream",
        json!({"": {"runs": [[
            {"lines": [hello_with_organization(), catalog(&[])]},
            {"await_file": "followed"},
            {"lines": [{"t": "snapshot", "topic": "organization", "items": rows(&first), "page": 0, "last": true, "cursor": 1}]},
            {"await_file": "changed"},
            {"lines": [{"t": "delta", "topic": "organization", "upsert": changed,
                        "remove": [{"key": "version:project:alpha:name"}], "cursor": 2}]},
            {"serve": true}
        ]]}}),
    );
    std::fs::write(fake.dir.join("swarm-get.json"), second.to_string()).unwrap();
    let (tx, rx) = channel();
    let mut seen = Seen::new(rx);
    let fleet = Fleet::start_local(fake.path(), tx, quick());
    let local = ComputerId::local();
    seen.until("live", live);
    assert!(fleet.organization_snapshot(&local).is_none());
    fleet.follow_organization(&local).unwrap();
    fake.touch("followed");
    let index = seen.until("the first state", |change| state(change).is_some());
    let (value, digest) = state(&seen.all[index]).unwrap();
    assert_eq!(digest, "d1");
    assert!(value.get("versions").is_none(), "versions stay in the core");
    let snapshot = fleet.organization_snapshot(&local).unwrap();
    assert_eq!(
        (snapshot.digest.as_str(), &snapshot.versions),
        ("d1", &json!({"project:alpha:name": ["op-d1"]}))
    );
    assert_eq!(
        snapshot.base["projects"][0]["sessions"],
        json!(["workstation\ncodex/alpha/one"])
    );
    // A delta changes members and versions; its worker is outdated, so the
    // core gives it one `swarm get`.
    fake.touch("changed");
    let index = seen.until_from(index + 1, "the changed state", |change| {
        state(change).is_some_and(|(_, digest)| digest == "d2")
    });
    let Change::Organization {
        update: OrganizationUpdate::State { membership, .. },
        ..
    } = &seen.all[index]
    else {
        unreachable!()
    };
    assert_eq!(
        membership
            .get("workstation\ncodex/alpha/two")
            .map(String::as_str),
        Some("alpha")
    );
    let snapshot = fleet.organization_snapshot(&local).unwrap();
    assert_eq!(snapshot.digest, "d2");
    assert_eq!(snapshot.versions, json!({"project:alpha:name": ["op-d2"]}));
    eventually("the worker's swarm get", || swarm_gets(&fake) == 1);
    eventually("the subscription", || !controls(&fake).is_empty());
    assert_eq!(controls(&fake)[0]["op"], "subscribe");
    assert!(fleet.unfollow_organization(&local));
    assert!(!fleet.unfollow_organization(&local));
    assert!(fleet.organization_snapshot(&local).is_none());
    eventually("the unsubscription", || controls(&fake).len() == 2);
    assert_eq!(controls(&fake)[1]["op"], "unsubscribe");
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        swarm_gets(&fake),
        1,
        "no poll while the stream offers the topic"
    );
    fleet.stop();
}

#[test]
fn a_stream_without_the_topic_is_polled_and_unchanged_polls_send_nothing() {
    let fake = FakeHgs::new(
        "organization_poll",
        json!({"": {"runs": [[{"lines": [hello("workstation"), catalog(&[])]}, {"serve": true}]]}}),
    );
    let mut reply = swarm(&["workstation\ncodex/alpha/one"], "d1", true);
    reply["written"] = json!({});
    std::fs::write(fake.dir.join("swarm-get.json"), reply.to_string()).unwrap();
    let (tx, rx) = channel();
    let mut seen = Seen::new(rx);
    let fleet = Fleet::start_local(fake.path(), tx, Timing::scaled(0.002));
    let local = ComputerId::local();
    seen.until("live", live);
    fleet.follow_organization(&local).unwrap();
    let index = seen.until("the polled state", |change| state(change).is_some());
    let (value, digest) = state(&seen.all[index]).unwrap();
    assert_eq!(digest, "d1");
    assert!(value.get("written").is_none() && value.get("versions").is_none());
    assert_eq!(fleet.organization_snapshot(&local).unwrap().digest, "d1");
    // Unchanged polls send nothing; a changed catalog does.
    reply["digest"] = json!("d2");
    std::fs::write(fake.dir.join("swarm-get.json"), reply.to_string()).unwrap();
    let next = seen.until_from(index + 1, "the next polled state", |change| {
        state(change).is_some()
    });
    assert_eq!(state(&seen.all[next]).unwrap().1, "d2");
    assert!(swarm_gets(&fake) >= 2);
    assert!(
        controls(&fake).is_empty(),
        "nothing subscribed without the topic"
    );
    // Unfollowed, the poll stops.
    assert!(fleet.unfollow_organization(&local));
    std::thread::sleep(Duration::from_millis(200));
    let polls = swarm_gets(&fake);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        swarm_gets(&fake),
        polls,
        "an unfollowed organization is not polled"
    );
    fleet.stop();
}
