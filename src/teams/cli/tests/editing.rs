//! The editing commands, projected from rows their product writers created.
use super::harness::*;
use crate::teams::cli::actor::Caller;
use serde_json::json;
fn input(h: &Harness, fields: serde_json::Value) -> String {
    let mut fields = fields.as_object().unwrap().clone();
    fields.insert("team".into(), json!(TEAM));
    fields.insert("request_id".into(), json!(uuid::Uuid::new_v4().to_string()));
    fields.insert("expected_revision".into(), json!(h.revision()));
    serde_json::Value::Object(fields).to_string()
}
#[test]
fn editing_commands_return_member_identity_and_status_keeps_pinned_choices() {
    let h = Harness::new();
    h.create();
    let bundle = h.store().team(TEAM).unwrap().unwrap().bundle;
    let add = input(
        &h,
        json!({"role":"developer","agent":"codex","model":"seat-model","effort":"high","account":{"mode":"named","account":"example-account"}}),
    );
    let (reply, code) = h.cli(&Caller::Person, "add-member", &add);
    assert_eq!(code, 0, "{reply}");
    let member = reply["member"].as_str().unwrap();
    assert_eq!(
        h.cli(&Caller::Person, "add-member", &add).0,
        reply,
        "saved request was not replayed"
    );
    let (status, exit) = h.cli(&Caller::Person, &format!("status --team {TEAM}"), "");
    assert_eq!(exit, 0, "{status}");
    let row = status["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == member)
        .unwrap();
    assert_eq!(row["model"], "seat-model");
    assert_eq!(row["effort"], "high");
    assert_eq!(
        row["account_choice"],
        json!({"mode":"named","account":"example-account"})
    );
    assert_eq!(row["open_work"], 0);
    assert!(status["member_roles"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["id"] == "developer"));
    assert_eq!(h.store().team(TEAM).unwrap().unwrap().bundle, bundle);
    for (command, body) in [
        ("rename-team", json!({"name":"Readable team"})),
        ("set-goal", json!({"goal":"New goal"})),
    ] {
        let (reply, exit) = h.cli(&Caller::Person, command, &input(&h, body));
        assert_eq!(exit, 0, "{reply}");
    }
    let team = h.store().team(TEAM).unwrap().unwrap();
    assert_eq!(
        (team.name.as_str(), team.goal.as_str()),
        ("Readable team", "New goal")
    );
    let (reply, exit) = h.cli(
        &Caller::Person,
        "swap-member",
        &input(&h, json!({"member":member,"agent":"claude"})),
    );
    assert_eq!(exit, 1);
    assert!(reply["error"]["message"]
        .as_str()
        .unwrap()
        .contains("Stop the member first"));
}
#[test]
fn status_counts_current_work_outside_the_displayed_queue_page() {
    let h = super::harness::seeded();
    let first = h.store().member_edit_work(TEAM).unwrap();
    let (reply, exit) = h.cli(&Caller::Person, &format!("status --team {TEAM}"), "");
    assert_eq!(exit, 0, "{reply}");
    for m in reply["members"].as_array().unwrap() {
        let id = m["id"].as_str().unwrap();
        assert_eq!(m["open_work"], json!(first.get(id).copied().unwrap_or(0)));
    }
    assert_eq!(
        first.get("m1"),
        Some(&1),
        "the reported task still belongs to its holder"
    );
    assert_eq!(first.values().sum::<usize>(), 1);
    // An agent's edit request cannot gain authority from its request fields.
    let (reply, exit) = h.cli(
        &Caller::Session(key(M1_SESSION)),
        "rename-team",
        &input(&h, json!({"name":"Forged"})),
    );
    assert_eq!(exit, 1);
    assert_eq!(reply["error"]["code"], "not_allowed");
}

#[test]
fn a_long_goal_is_projected_completely_for_editing_without_widening_the_bar() {
    let h = Harness::new();
    h.create();
    let goal = "x".repeat(9000);
    let (reply, exit) = h.cli(
        &Caller::Person,
        "set-goal",
        &input(&h, json!({"goal":goal})),
    );
    assert_eq!(exit, 0, "{reply}");
    let (status, exit) = h.cli(&Caller::Person, &format!("status --team {TEAM}"), "");
    assert_eq!(exit, 0, "{status}");
    let view = zerus_client_core::teams::view::team(&status, &json!({"rows":[]}));
    assert_eq!(view.edit_goal.as_deref(), Some(goal.as_str()));
    assert_eq!(
        view.bar.goal.chars().count(),
        zerus_client_core::teams::view::TEXT_CHARS
    );
}
