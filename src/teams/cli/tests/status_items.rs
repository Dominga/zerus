//! zerus-63rru.40: `hgs team status` names the service's non-fatal items
//! that have no card, and the store's final refusals, as bounded pages.
use super::harness::*;
use crate::teams::cli::actor::Caller;
use crate::teams::ops::{CancelState, KeyClearState, Operation};
use serde_json::{json, Value};

fn status(harness: &Harness) -> Value {
    let (reply, exit) = harness.cli(&Caller::Person, &format!("status --team {TEAM}"), "");
    assert_eq!(exit, 0, "{reply}");
    reply
}

/// The seeded team stopped, with what a stopped team shows: one cancel the
/// task command refused, two key clears it refused (the second newer), and
/// two service items the service's own passes wrote. A third task's cancel
/// got no final answer on three passes (its child said the task's project
/// is cutting over, with a reason that needs making safe), and the terminal
/// page failed on each: one item of the team and one of the whole service.
/// The service runs these passes only on Linux and macOS.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn stopped_with_conditions() -> Harness {
    let harness = seeded();
    harness.add(3);
    harness.apply(&person(), Operation::Stop);
    let store = harness.store();
    let mut cancels = store.pending_cancels(TEAM, 200, 0).unwrap();
    let waiting = cancels.pop().expect("the third task's cancel");
    for (i, row) in cancels.iter().enumerate() {
        let refused = i == 0;
        harness.apply(
            &service(),
            Operation::RecordCancel {
                node: row.node,
                decision: row.decision,
                state: if refused {
                    CancelState::Refused
                } else {
                    CancelState::Released
                },
                reason: if refused {
                    "The task has an attempt whose delivery is unknown.".into()
                } else {
                    String::new()
                },
                attempt: row.attempt,
                attempt_id: row.attempt_id.clone(),
            },
        );
    }
    let keyed: Vec<u32> = harness
        .store()
        .keyed_tasks(TEAM, 0, 200)
        .unwrap()
        .iter()
        .map(|k| k.node)
        .collect();
    assert_eq!(keyed.len(), 2, "both answered keys wait for their clear");
    assert!(
        !keyed.contains(&waiting.node),
        "its key waits for the cancel"
    );
    for (i, node) in keyed.iter().enumerate() {
        harness.apply_at(
            &service(),
            Operation::RecordKeyClear {
                node: *node,
                state: KeyClearState::Refused,
                reason: format!("The task moved to another project ({i})."),
            },
            NOW + i as i64,
        );
    }
    let last = crate::desktop::teams::failing_service_passes(
        harness.state(),
        TEAM,
        "The project\nmoves:\u{1b}[31m storage busy\u{202e}",
        "The terminal page failed: store busy",
        3,
        NOW,
    );
    assert_eq!(last, LAST);
    assert_eq!(
        harness.store().pending_cancels(TEAM, 200, 0).unwrap(),
        [waiting],
        "the cancel still waits"
    );
    harness
}
/// The last of the three service passes, an hour apart from `NOW`.
#[cfg(any(target_os = "linux", target_os = "macos"))]
const LAST: i64 = NOW + 2 * 3_600_000;

/// A stopped team shows its service items (current only while the service
/// runs; here no service answers), its key refusals newest first and its
/// refused cancels, each a page with the count of the rest.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn a_stopped_teams_status_names_its_service_items_and_refusals() {
    let harness = stopped_with_conditions();
    let waiting = harness
        .store()
        .pending_cancels(TEAM, 1, 0)
        .unwrap()
        .remove(0);
    let reply = status(&harness);
    assert_eq!(reply["service_running"], Value::Bool(false));
    assert_eq!(
        reply["service_items"],
        json!({
            "items": [
                {"scope": "service", "kind": "terminal_page", "subject": "", "node": null,
                 "reason": "The terminal page failed: store busy", "failures": 3,
                 "since_ms": NOW, "last_ms": LAST, "next_ms": LAST + 240_000, "current": false},
                {"scope": "team", "kind": "cancel_retry", "subject": waiting.decision.to_string(),
                 "node": waiting.node, "reason": "The project moves:[31m storage busy", "failures": 3,
                 "since_ms": NOW, "last_ms": LAST, "next_ms": LAST + 240_000, "current": false},
            ],
            "more": 0,
            "unlisted": false,
        })
    );
    let keys = &reply["key_refusals"];
    assert_eq!(keys["more"], 0);
    let reasons: Vec<&str> = keys["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["reason"].as_str().unwrap())
        .collect();
    assert_eq!(
        reasons,
        [
            "The task moved to another project (1).",
            "The task moved to another project (0)."
        ],
        "newest first"
    );
    let cancels = &reply["cancel_refusals"];
    assert_eq!(cancels["more"], 0);
    assert_eq!(cancels["items"].as_array().unwrap().len(), 1);
    assert_eq!(
        cancels["items"][0]["reason"],
        "The task has an attempt whose delivery is unknown."
    );
}

/// A live team shows no refused cancels (its lead has the inbox item), and
/// a state whose service never wrote the table shows its items as null,
/// "not reported", unlike an empty page.
#[test]
fn a_live_teams_status_has_no_cancel_refusals_and_null_items_without_a_ledger() {
    let harness = seeded();
    let reply = status(&harness);
    assert_eq!(reply["service_items"], Value::Null);
    assert_eq!(reply["cancel_refusals"], Value::Null);
    assert_eq!(reply["key_refusals"], json!({"items": [], "more": 0}));
    drop(crate::desktop::teams::test_actor_ledger(harness.state()));
    let reply = status(&harness);
    assert_eq!(
        reply["service_items"],
        json!({"items": [], "more": 0, "unlisted": false})
    );
}

/// The refusal pages hold 20 rows and count the rest.
#[test]
fn the_refusal_pages_hold_twenty_and_count_the_rest() {
    let harness = seeded();
    for n in 3..=26 {
        harness.add(n);
    }
    harness.apply(&person(), Operation::Stop);
    let store = harness.store();
    for row in store.pending_cancels(TEAM, 200, 0).unwrap() {
        harness.apply(
            &service(),
            Operation::RecordCancel {
                node: row.node,
                decision: row.decision,
                state: CancelState::Refused,
                reason: format!("Refused {}", row.node),
                attempt: row.attempt,
                attempt_id: row.attempt_id.clone(),
            },
        );
    }
    let keyed: Vec<u32> = harness
        .store()
        .keyed_tasks(TEAM, 0, 200)
        .unwrap()
        .iter()
        .map(|k| k.node)
        .collect();
    for node in &keyed {
        harness.apply(
            &service(),
            Operation::RecordKeyClear {
                node: *node,
                state: KeyClearState::Refused,
                reason: format!("Key {node} refused"),
            },
        );
    }
    let reply = status(&harness);
    let total = keyed.len();
    assert!(total > 20, "{total}");
    for list in ["key_refusals", "cancel_refusals"] {
        assert_eq!(reply[list]["items"].as_array().unwrap().len(), 20, "{list}");
        assert_eq!(reply[list]["more"], total - 20, "{list}");
    }
}
