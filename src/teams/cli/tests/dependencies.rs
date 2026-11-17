//! Person-positive setup uses an explicit admitted-writer model; reads and
//! Session/negative caller checks still use the production CLI unchanged.
//! The graph page comes from the real operations and CLI queue writer.
use super::harness::*;
use crate::teams::cli::actor::Caller;
use crate::teams::ops::Operation;
use serde_json::json;
use zerus_team_plan::model::{Dep, DepKind};

#[test]
fn set_deps_cli_is_person_only_and_queue_carries_both_kinds_and_off_page_edges() {
    let h = Harness::new();
    h.create();
    h.bind("lead", LEAD_SESSION);
    h.bind("m1", M1_SESSION);
    for id in 1..=4 {
        h.add(id);
    }
    let ask = || {
        json!({"request_id":uuid::Uuid::new_v4().to_string(),"team":TEAM,"expected_revision":h.revision(),"node":2,"deps":[{"node":1,"kind":"soft"}]}).to_string()
    };
    for caller in [
        Caller::Session(key(LEAD_SESSION)),
        Caller::Session(key(M1_SESSION)),
    ] {
        let (reply, exit) = h.cli(&caller, "set-deps", &ask());
        assert_eq!((exit, code(&reply)), (1, "not_allowed"));
    }
    let input = ask();
    let (reply, exit) = h.native_command("set-deps", &input);
    assert_eq!(exit, 0, "{reply}");
    assert_eq!(h.native_command("set-deps", &input), (reply, 0));
    h.apply(
        &person(),
        Operation::SetDeps {
            node: 3,
            deps: vec![
                Dep {
                    node: 2,
                    kind: DepKind::Hard,
                },
                Dep {
                    node: 4,
                    kind: DepKind::Hard,
                },
            ],
        },
    );
    let (queue, exit) = h.cli(
        &Caller::Person,
        &format!("queue --team {TEAM} --limit 3"),
        "",
    );
    assert_eq!(exit, 0, "{queue}");
    assert_eq!(queue["rows"][1]["deps"], json!([{"node":1,"kind":"soft"}]));
    assert_eq!(queue["rows"][2]["deps"], json!([{"node":2},{"node":4}]));
    assert!(queue["next"].is_string());
    println!("GRAPH_PAGE {}", serde_json::to_string(&queue).unwrap());
    let pinned: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../client/core/src/upstream_teams/fixtures/graph-page.json"
    ))
    .unwrap();
    assert_eq!(queue, pinned);
    let (status, exit) = h.cli(&Caller::Person, &format!("status --team {TEAM}"), "");
    assert_eq!(exit, 0, "{status}");
    // Keep only the core's consumed fields, from the same store snapshot as
    // the queue; runtime service health is not a fixture input.
    let status =
        json!({"team":status["team"],"members":status["members"],"counts":status["counts"]});
    println!("GRAPH_STATUS {}", serde_json::to_string(&status).unwrap());
    let pinned: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../client/core/src/upstream_teams/fixtures/graph-status.json"
    ))
    .unwrap();
    assert_eq!(status, pinned);
}

#[test]
fn queue_edit_eligibility_rejects_reserved_and_historical_effects_on_pending_nodes() {
    let h = Harness::new();
    h.create();
    h.bind("m1", M1_SESSION);
    let a = h.add(1);
    let b = h.add(2);
    let effect = uuid::Uuid::new_v4().to_string();
    h.apply(
        &service(),
        Operation::HandoutIntent {
            node: b,
            member: "m1".into(),
            effect_request_id: effect.clone(),
        },
    );
    let read = || {
        let (queue, exit) = h.cli(&Caller::Person, &format!("queue --team {TEAM}"), "");
        assert_eq!(exit, 0, "{queue}");
        assert_eq!(queue["rows"][0]["deps_editable"], true);
        assert_eq!(queue["rows"][1]["state"], "pending");
        assert_eq!(queue["rows"][1]["deps_editable"], false);
    };
    read();
    h.apply(
        &service(),
        Operation::HandoutOutcome {
            effect_request_id: effect,
            outcome: crate::teams::ops::HandoutOutcome::Refused {
                reason: "Fixture refusal.".into(),
            },
        },
    );
    read();
    let input = json!({"request_id":uuid::Uuid::new_v4().to_string(), "team":TEAM, "expected_revision":h.revision(), "node":b, "deps":[{"node":a}]}).to_string();
    let (reply, exit) = h.native_command("set-deps", &input);
    assert_eq!((exit, code(&reply)), (1, "invalid_state"));
    assert_eq!(
        reply["error"]["message"],
        "Dependencies can only be edited before this task starts or has an attempt or hand-out recorded."
    );
    assert_eq!(h.native_command("set-deps", &input), (reply, 1));
}
