//! Every write command against a temporary state directory: who may run it,
//! what the request may say, and what the store records.
use super::harness::*;
use crate::teams::cli::actor::Caller;
use crate::teams::ops::Operation;
use serde_json::{json, Value};
use zerus_team_plan::model::MemberState;

/// One request envelope: a fresh request ID, the team, and its current
/// revision. The operation's own fields come from `extra`.
fn ask(harness: &Harness, step: u32, extra: Value) -> String {
    let mut fields = match extra {
        Value::Object(fields) => fields,
        _ => panic!("an operation's fields are an object"),
    };
    fields.insert("request_id".into(), json!(uid(0x4000 + step)));
    fields.insert("team".into(), json!(TEAM));
    fields.insert("expected_revision".into(), json!(harness.revision()));
    Value::Object(fields).to_string()
}

fn person_caller() -> Caller {
    Caller::Person
}

fn lead_caller() -> Caller {
    Caller::Session(key(LEAD_SESSION))
}

fn m1_caller() -> Caller {
    Caller::Session(key(M1_SESSION))
}

fn bind_lead_and_m1(harness: &Harness) {
    harness.create();
    harness.bind("lead", LEAD_SESSION);
    harness.bind("m1", M1_SESSION);
}

#[test]
fn create_from_a_template_records_the_team_and_launches_nothing() {
    let harness = Harness::new();
    let input = ask(
        &harness,
        1,
        json!({
            "project": "example-project",
            "name": "Fixes",
            "goal": "Keep the list usable.",
            "home": MACHINE,
            "integration_branch": "zerus/team/fixes",
            "base_commit": COMMIT,
        }),
    );
    let (reply, exit) = harness.cli(&person_caller(), "create --template bugfix", &input);
    assert_eq!(exit, 0, "{reply}");
    assert_eq!(reply["ok"], true);
    assert_eq!(reply["revision"], 1);
    assert_eq!(reply["team"], TEAM);
    assert_eq!(reply["actor"]["kind"], "person");
    assert_eq!(reply["actor"]["origin"], "cli_interim");
    assert_eq!(reply["actor"]["interim"], true);

    let team = harness.store().team(TEAM).unwrap().expect("the team");
    assert_eq!((team.name.as_str(), team.revision), ("Fixes", 1));
    let members = harness.store().members(TEAM, 200, None).unwrap().rows;
    assert_eq!(members.len(), 3, "the bugfix roster and its lead");
    assert!(members
        .iter()
        .all(|member| member.state == MemberState::NotCreated));
    // Nothing was launched: no member has a session, and the store wrote no
    // effect row.
    assert!(members.iter().all(|member| member.session.is_none()));
    let (reply, exit) = harness.cli(&person_caller(), "create --template bugfix", &input);
    assert_eq!(
        (exit, reply["revision"].as_u64()),
        (0, Some(1)),
        "the same request replays its first reply"
    );
}

#[test]
fn create_needs_a_person_and_a_known_template() {
    let harness = Harness::new();
    let input = ask(
        &harness,
        1,
        json!({
            "project": "example-project",
            "name": "Fixes",
            "home": MACHINE,
            "integration_branch": "zerus/team/fixes",
            "base_commit": COMMIT,
        }),
    );
    let (reply, exit) = harness.cli(
        &Caller::Session(key(OTHER_SESSION)),
        "create --template bugfix",
        &input,
    );
    assert_eq!((code(&reply), exit), ("not_allowed", 1), "{reply}");
    let (reply, exit) = harness.cli(&person_caller(), "create --template ghost", &input);
    assert_eq!((code(&reply), exit), ("not_found", 1), "{reply}");
    let (reply, _) = harness.cli(&person_caller(), "create", &input);
    assert_eq!(code(&reply), "invalid_state");
    assert!(harness.store().team(TEAM).unwrap().is_none());
}

#[test]
fn a_request_never_names_its_actor_or_its_operation() {
    let harness = Harness::new();
    harness.create();
    harness.bind("lead", LEAD_SESSION);
    for field in ["actor", "op"] {
        let input = ask(
            &harness,
            1,
            json!({"node": 1, "role": "developer", "pipeline": "reviewed", field: "local"}),
        );
        let (reply, exit) = harness.cli(&lead_caller(), "route", &input);
        assert_eq!((code(&reply), exit), ("unknown_field", 1), "{field}");
        assert_eq!(reply["error"]["refs"][0], field);
    }
}

#[test]
fn the_lead_routes_and_admits_and_a_member_cannot() {
    let harness = Harness::new();
    bind_lead_and_m1(&harness);
    let unadmitted = harness
        .apply(
            &member("m1", M1_SESSION),
            Operation::AddTask {
                task: task(3),
                role: None,
                pipeline: None,
            },
        )
        .node
        .expect("a member's task");
    assert!(
        !harness
            .store()
            .node(TEAM, unadmitted)
            .unwrap()
            .unwrap()
            .admitted
    );

    let input = ask(&harness, 1, json!({"node": unadmitted}));
    let (reply, exit) = harness.cli(&m1_caller(), "admit", &input);
    assert_eq!((code(&reply), exit), ("not_allowed", 1), "{reply}");
    let input = ask(&harness, 2, json!({"node": unadmitted}));
    let (reply, exit) = harness.cli(&lead_caller(), "admit", &input);
    assert_eq!(exit, 0, "{reply}");
    assert!(
        harness
            .store()
            .node(TEAM, unadmitted)
            .unwrap()
            .unwrap()
            .admitted
    );

    let input = ask(
        &harness,
        3,
        json!({"node": unadmitted, "role": "developer", "pipeline": "reviewed"}),
    );
    let (reply, exit) = harness.cli(&lead_caller(), "route", &input);
    assert_eq!(exit, 0, "{reply}");
    let node = harness.store().node(TEAM, unadmitted).unwrap().unwrap();
    assert_eq!(
        (node.role.as_str(), node.pipeline.as_str()),
        ("developer", "reviewed")
    );
}

#[test]
fn holding_releasing_cancelling_and_reassigning_are_the_leads() {
    let harness = Harness::new();
    bind_lead_and_m1(&harness);
    let pending = harness.add(4);
    let reported = harness.reported(5);

    let (reply, exit) = harness.cli(
        &lead_caller(),
        "hold",
        &ask(&harness, 1, json!({"node": pending})),
    );
    assert_eq!((exit, reply["ok"].as_bool()), (0, Some(true)), "{reply}");
    assert!(harness.store().node(TEAM, pending).unwrap().unwrap().hold);
    let (reply, exit) = harness.cli(
        &lead_caller(),
        "release",
        &ask(&harness, 2, json!({"node": pending})),
    );
    assert_eq!(exit, 0, "{reply}");
    assert!(!harness.store().node(TEAM, pending).unwrap().unwrap().hold);

    let (reply, exit) = harness.cli(
        &lead_caller(),
        "reassign",
        &ask(
            &harness,
            3,
            json!({"node": reported, "member": "m2", "reason": "m1 is out today"}),
        ),
    );
    assert_eq!(
        (code(&reply), exit),
        ("invalid_state", 1),
        "m2 is not active: {reply}"
    );

    let (reply, exit) = harness.cli(
        &lead_caller(),
        "cancel",
        &ask(
            &harness,
            4,
            json!({"node": pending, "reason": "Already fixed on main."}),
        ),
    );
    assert_eq!(exit, 0, "{reply}");
    let (reply, exit) = harness.cli(
        &m1_caller(),
        "cancel",
        &ask(&harness, 5, json!({"node": pending, "reason": "no"})),
    );
    assert_eq!((code(&reply), exit), ("not_allowed", 1), "{reply}");
}

#[test]
fn accept_and_return_record_the_leads_decision() {
    let harness = Harness::new();
    bind_lead_and_m1(&harness);
    let first = harness.reported(6);
    let second = harness.reported(7);
    let (reply, exit) = harness.cli(
        &lead_caller(),
        "accept",
        &ask(&harness, 1, json!({"node": first})),
    );
    assert_eq!(exit, 0, "{reply}");
    assert_eq!(
        harness
            .store()
            .node(TEAM, first)
            .unwrap()
            .unwrap()
            .decided_by,
        Some(zerus_team_plan::model::Decider::Lead)
    );

    let (reply, exit) = harness.cli(
        &lead_caller(),
        "return",
        &ask(
            &harness,
            2,
            json!({"node": second, "reason": "The empty case needs a test."}),
        ),
    );
    assert_eq!(exit, 0, "{reply}");
    let (reply, exit) = harness.cli(
        &m1_caller(),
        "return",
        &ask(&harness, 3, json!({"node": second, "reason": "no"})),
    );
    assert_eq!((code(&reply), exit), ("not_allowed", 1), "{reply}");
}

#[test]
fn settings_pause_resume_stop_and_archive_follow_the_authority_table() {
    let harness = Harness::new();
    bind_lead_and_m1(&harness);
    let mut settings = harness.store().team(TEAM).unwrap().unwrap().settings;
    settings.timers.remind_lead_minutes = 25;
    let changed = json!({"settings": serde_json::to_value(&settings).unwrap()});

    let (reply, exit) = harness.cli(
        &lead_caller(),
        "settings",
        &ask(&harness, 1, changed.clone()),
    );
    assert_eq!((code(&reply), exit), ("not_allowed", 1), "{reply}");
    let (reply, exit) = harness.cli(
        &person_caller(),
        "settings",
        &ask(&harness, 2, changed.clone()),
    );
    assert_eq!(exit, 0, "{reply}");
    assert_eq!(
        harness
            .store()
            .team(TEAM)
            .unwrap()
            .unwrap()
            .settings
            .timers
            .remind_lead_minutes,
        25
    );

    let (reply, exit) = harness.cli(&lead_caller(), "pause", &ask(&harness, 3, json!({})));
    assert_eq!(exit, 0, "{reply}");
    let (reply, exit) = harness.cli(&lead_caller(), "resume", &ask(&harness, 4, json!({})));
    assert_eq!(exit, 0, "{reply}");

    let (reply, exit) = harness.cli(&lead_caller(), "stop", &ask(&harness, 5, json!({})));
    assert_eq!((code(&reply), exit), ("not_allowed", 1), "{reply}");
    let (reply, exit) = harness.cli(&person_caller(), "stop", &ask(&harness, 6, json!({})));
    assert_eq!(exit, 0, "{reply}");
    let (reply, exit) = harness.cli(&person_caller(), "archive", &ask(&harness, 7, json!({})));
    assert_eq!(exit, 0, "{reply}");
    assert_eq!(
        harness.store().team(TEAM).unwrap().unwrap().status,
        zerus_team_plan::model::TeamStatus::Archived
    );
}

#[test]
fn a_member_reads_its_own_work_and_asks_and_the_lead_replies() {
    let harness = Harness::new();
    bind_lead_and_m1(&harness);
    let reported = harness.reported(8);

    let (reply, exit) = harness.cli(&m1_caller(), "current", "");
    assert_eq!(exit, 0, "{reply}");
    assert_eq!(reply["member"]["id"], "m1");
    assert_eq!(
        reply["revision"].as_u64(),
        Some(harness.revision()),
        "current names the team's revision"
    );
    let nodes = reply["nodes"].as_array().unwrap();
    let held = nodes
        .iter()
        .find(|node| node["id"] == reported)
        .expect("the held node");
    let generation = held["generation"].as_u64().expect("the node's generation");
    assert!(nodes.iter().all(|node| node["state"] != "accepted"));

    // Both values `ask` needs come from `current`, not from the test.
    let input = json!({
        "request_id": uid(0x4000 + 1),
        "team": TEAM,
        "expected_revision": reply["revision"],
        "node": held["id"],
        "generation": generation,
        "text": "Which case do you mean?",
    });
    let (reply, exit) = harness.cli(&m1_caller(), "ask", &input.to_string());
    assert_eq!(exit, 0, "{reply}");
    let item = reply["item"].as_u64().expect("the question's item");
    let (reply, exit) = harness.cli(
        &lead_caller(),
        "reply",
        &ask(
            &harness,
            2,
            json!({"item": item, "text": "The empty list, not the empty label."}),
        ),
    );
    assert_eq!(exit, 0, "{reply}");

    let (reply, exit) = harness.cli(&m1_caller(), &format!("inbox --team {TEAM}"), "");
    assert_eq!((code(&reply), exit), ("not_allowed", 1), "{reply}");
    let (reply, exit) = harness.cli(&person_caller(), "current", "");
    assert_eq!((code(&reply), exit), ("not_allowed", 1), "{reply}");
}

#[test]
fn a_cli_person_cannot_decide_an_owner_card() {
    let harness = Harness::new();
    bind_lead_and_m1(&harness);
    let card = harness
        .apply(
            &person(),
            Operation::OpenCard {
                kind: "note".into(),
                text: "The release moved.".into(),
                node: None,
                decisions: None,
                parameters: None,
            },
        )
        .item
        .expect("the card's sequence");
    let (reply, exit) = harness.cli(
        &lead_caller(),
        "decide",
        &ask(
            &harness,
            1,
            json!({"card": card, "decision": "acknowledged"}),
        ),
    );
    assert_eq!((code(&reply), exit), ("not_allowed", 1), "{reply}");
    let (reply, exit) = harness.cli(
        &person_caller(),
        "decide",
        &ask(
            &harness,
            2,
            json!({"card": card, "decision": "acknowledged"}),
        ),
    );
    assert_eq!(
        (code(&reply), exit),
        ("person_gesture_required", 1),
        "{reply}"
    );
    assert_eq!(
        harness.store().cards(TEAM, 10, None).unwrap().rows[0].state,
        "open"
    );
}

#[test]
fn every_operation_word_is_known() {
    for word in [
        "settings",
        "pause",
        "resume",
        "stop",
        "archive",
        "decide",
        "admit",
        "route",
        "accept",
        "return",
        "reassign",
        "hold",
        "release",
        "cancel",
        "reply",
        "ask",
        "verdict",
        "pause-member",
        "resume-member",
        "stop-member",
        "archive-member",
    ] {
        assert!(
            crate::teams::cli::commands::variant(word).is_some(),
            "{word} is not in the operation table"
        );
    }
    let harness = Harness::new();
    let (reply, exit) = harness.cli(&person_caller(), "frobnicate", "");
    assert_eq!((code(&reply), exit), ("not_found", 1));
    assert_eq!(reply["error"]["refs"][0], "frobnicate");
}

#[test]
fn an_unproved_session_and_an_off_preview_are_refused_before_anything_else() {
    let harness = Harness::new();
    let refused = Caller::Refused(zerus_team_plan::Problem::new(
        "not_allowed",
        "that run is not live",
    ));
    let (reply, exit) = harness.cli(&refused, &format!("status --team {TEAM}"), "");
    assert_eq!((code(&reply), exit), ("not_allowed", 1));

    harness.create();
    std::fs::remove_file(harness.state().join("teams/preview")).unwrap();
    let before = std::fs::read_dir(harness.state().join("teams"))
        .unwrap()
        .count();
    for line in [
        "status",
        "inbox",
        "queue",
        "current",
        "create --template bugfix",
        "catalog show",
    ] {
        let (reply, exit) = harness.cli(&person_caller(), line, "");
        assert_eq!((code(&reply), exit), ("teams_inactive", 1), "{line}");
    }
    let after = std::fs::read_dir(harness.state().join("teams"))
        .unwrap()
        .count();
    assert_eq!(after, before, "an off preview wrote nothing");
}

#[test]
fn a_request_is_refused_at_the_bound_before_anything_parses_it() {
    let harness = Harness::new();
    let over = "x".repeat(262_145);
    let (reply, exit) = harness.cli(&person_caller(), "admit", &over);
    assert_eq!((code(&reply), exit), ("over_cap", 1), "{reply}");
    // Exactly the bound is accepted and then read as the (empty) request it is.
    let at = " ".repeat(262_144);
    let (reply, _) = harness.cli(&person_caller(), "admit", &at);
    assert_ne!(code(&reply), "over_cap", "{reply}");
}

#[test]
fn the_queue_page_of_a_new_task_names_it_and_its_rank() {
    let harness = Harness::new();
    bind_lead_and_m1(&harness);
    let node = harness.add(9);
    let (reply, exit) = harness.cli(
        &lead_caller(),
        &format!("queue --team {TEAM} --limit 10"),
        "",
    );
    assert_eq!(exit, 0, "{reply}");
    let rows = reply["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], node);
    assert_eq!(rows[0]["state"], "pending");
    assert_eq!(rows[0]["rank"], "a01");
}

#[test]
fn pause_and_resume_accept_the_persons_and_the_leads_word() {
    let harness = Harness::new();
    bind_lead_and_m1(&harness);
    let mut lead = harness.store().team(TEAM).unwrap().unwrap();
    assert_eq!(lead.status, zerus_team_plan::model::TeamStatus::Running);
    let (reply, exit) = harness.cli(&lead_caller(), "pause", &ask(&harness, 1, json!({})));
    assert_eq!(exit, 0, "{reply}");
    lead = harness.store().team(TEAM).unwrap().unwrap();
    assert_eq!(lead.status, zerus_team_plan::model::TeamStatus::Paused);
    // A team the person paused is not resumed by the lead.
    let (reply, exit) = harness.cli(&person_caller(), "resume", &ask(&harness, 2, json!({})));
    assert_eq!(exit, 0, "{reply}");
    let (reply, exit) = harness.cli(&lead_caller(), "pause", &ask(&harness, 3, json!({})));
    assert_eq!(exit, 0, "{reply}");
    let (reply, exit) = harness.cli(&m1_caller(), "resume", &ask(&harness, 4, json!({})));
    assert_eq!((code(&reply), exit), ("not_allowed", 1), "{reply}");
}

#[test]
fn a_node_a_member_was_handed_shows_in_status_counts() {
    let harness = Harness::new();
    harness.create();
    harness.bind("lead", LEAD_SESSION);
    harness.bind("m1", M1_SESSION);
    harness.reported(10);
    let (reply, exit) = harness.cli(&lead_caller(), &format!("status --team {TEAM}"), "");
    assert_eq!(exit, 0, "{reply}");
    assert_eq!(reply["counts"]["nodes"]["reported"], 1);
    assert_eq!(reply["counts"]["members"]["active"], 2);
    assert_eq!(reply["counts"]["inbox"]["open"], 1);
    assert!(reply["team"].get("bundle").is_none());
    let (reply, _) = harness.cli(
        &lead_caller(),
        &format!("status --team {TEAM} --bundle"),
        "",
    );
    assert!(reply["team"].get("bundle").is_some());
}

#[test]
fn another_sessions_actor_may_read_but_not_operate() {
    let harness = Harness::new();
    harness.create();
    let mut store = harness.store();
    let request = harness.request(Operation::Pause);
    let problem = crate::teams::ops::apply(&mut store, &other(), &request, NOW).unwrap_err();
    assert_eq!(problem.code, "not_allowed");
}

#[test]
fn the_person_pauses_resumes_stops_and_archives_a_member() {
    let harness = Harness::new();
    bind_lead_and_m1(&harness);
    let run = |line: &str, caller: &Caller, step: u32| {
        harness.cli(caller, line, &ask(&harness, step, json!({"member": "m1"})))
    };
    let state = |harness: &Harness| harness.store().member(TEAM, "m1").unwrap().unwrap().state;
    let (reply, exit) = run("pause-member", &person_caller(), 1);
    assert_eq!(exit, 0, "{reply}");
    assert_eq!(state(&harness), MemberState::Paused);
    let (reply, exit) = run("resume-member", &lead_caller(), 2);
    assert_eq!(
        exit, 0,
        "the lead may resume a member it did not stop: {reply}"
    );
    assert_eq!(state(&harness), MemberState::Active);
    let (reply, exit) = run("stop-member", &lead_caller(), 3);
    assert_eq!((code(&reply), exit), ("not_allowed", 1), "{reply}");
    let (reply, exit) = run("stop-member", &person_caller(), 4);
    assert_eq!(exit, 0, "{reply}");
    assert_eq!(state(&harness), MemberState::Paused);
    let (reply, exit) = run("resume-member", &lead_caller(), 5);
    assert_eq!((code(&reply), exit), ("invalid_state", 1), "{reply}");
    let (reply, exit) = run("resume-member", &person_caller(), 6);
    assert_eq!(exit, 0, "{reply}");
    let (reply, exit) = run("archive-member", &lead_caller(), 7);
    assert_eq!((code(&reply), exit), ("not_allowed", 1), "{reply}");
    let (reply, exit) = run("archive-member", &person_caller(), 8);
    assert_eq!(exit, 0, "{reply}");
    assert_eq!(state(&harness), MemberState::Removed);
}

#[test]
fn advance_base_and_its_form_read_refuse_sessions_before_their_input_or_git() {
    let h = seeded();
    for session in [LEAD_SESSION, M1_SESSION, OTHER_SESSION] {
        for words in [
            "advance-base",
            "base --team 00000000-0000-4000-8000-00000000beef",
        ] {
            let (reply, exit) = h.cli(&Caller::Session(key(session)), words, "not JSON");
            assert_eq!(exit, 1);
            assert_eq!(reply["error"]["code"], "not_allowed", "{reply}");
        }
    }
}
