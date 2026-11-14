//! The reads: their shape, their bounded pages and who may run them.
use super::harness::*;
use crate::teams::cli::actor::Caller;
use crate::teams::ops::ReportNote;
use serde_json::Value;

#[test]
fn inbox_carries_the_service_notes_apart_from_the_member_report() {
    let harness = Harness::new();
    harness.create();
    harness.bind("lead", LEAD_SESSION);
    harness.bind("m1", M1_SESSION);
    let node = harness.reported_notes(
        1,
        vec![
            ReportNote::new("text_shortened"),
            ReportNote::new("worktree_dirty"),
        ],
    );
    let (reply, exit) = harness.cli(&lead_caller(), &format!("inbox --team {TEAM}"), "");
    assert_eq!(exit, 0, "{reply}");
    let item = reply["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["kind"] == "report" && row["node"] == node)
        .unwrap();
    assert!(
        item["text"]
            .as_str()
            .unwrap()
            .contains("> Trimmed the label and added a case for an empty one."),
        "the member's block is quoted: {item}"
    );
    assert_eq!(
        item["notes"],
        serde_json::json!([
            {"code": "text_shortened"},
            {"code": "worktree_dirty"}
        ])
    );
}

fn person_caller() -> Caller {
    Caller::Person
}

fn lead_caller() -> Caller {
    Caller::Session(key(LEAD_SESSION))
}

fn unbound_caller() -> Caller {
    Caller::Session(key(OTHER_SESSION))
}

/// A person's actor view names every limit of the caller proofs in the one
/// composed text; a session's view carries no proof kind (zerus-63rru.28).
#[test]
fn a_persons_view_states_every_limit_and_no_view_names_a_proof_kind() {
    let view = super::super::actor::view(&person());
    assert_eq!(view["note"], crate::teams::caller::limits());
    assert!(view["note"]
        .as_str()
        .unwrap()
        .contains(crate::state::session_graph::HOSTED_LIMIT));
    for actor in [member("m1", M1_SESSION), other()] {
        let view = super::super::actor::view(&actor);
        assert!(
            view.get("note").is_none() && !view.to_string().contains("proof"),
            "{view}"
        );
    }
}

fn status(harness: &Harness, caller: &Caller) -> Value {
    let (reply, exit) = harness.cli(caller, &format!("status --team {TEAM}"), "");
    assert_eq!(exit, 0, "{reply}");
    reply
}

#[test]
fn status_carries_the_team_its_members_and_counts_but_not_the_bundle() {
    let harness = seeded();
    let reply = status(&harness, &person_caller());
    assert_eq!(reply["ok"], true);
    assert_eq!(reply["team"]["id"], TEAM);
    assert_eq!(reply["team"]["name"], "Fixes");
    assert!(reply["team"].get("bundle").is_none());
    assert_eq!(reply["members"].as_array().unwrap().len(), 4);
    assert_eq!(reply["counts"]["members"]["active"], 2);
    assert_eq!(reply["counts"]["members"]["not_created"], 2);
    assert_eq!(reply["counts"]["nodes"]["pending"], 1);
    assert_eq!(reply["counts"]["nodes"]["reported"], 1);
    assert_eq!(reply["counts"]["inbox"]["open"], 1);
    assert_eq!(reply["counts"]["cards"]["open"], 1);
    assert_eq!(reply["actor"]["origin"], "cli_interim");
    let (reply, _) = harness.cli(
        &person_caller(),
        &format!("status --team {TEAM} --bundle"),
        "",
    );
    assert_eq!(reply["team"]["bundle"]["template"]["id"], "bugfix");
}

#[test]
fn every_lead_list_holds_one_page_and_names_the_next_cursor() {
    let harness = seeded();
    for (line, rows) in [
        (format!("queue --team {TEAM} --limit 1"), 1usize),
        (format!("inbox --team {TEAM}"), 2),
        (format!("cards --team {TEAM}"), 1),
        (format!("journal --team {TEAM} --limit 3"), 3),
    ] {
        let (reply, exit) = harness.cli(&lead_caller(), &line, "");
        assert_eq!(exit, 0, "{line}: {reply}");
        let list = reply["rows"].as_array().unwrap();
        assert_eq!(list.len(), rows, "{line}");
        if line.contains("--limit 1") || line.contains("--limit 3") {
            assert!(reply["next"].is_string(), "{line} pages on");
        }
    }
}

#[test]
fn a_page_stays_stable_while_rows_are_added() {
    let harness = Harness::new();
    harness.create();
    harness.bind("lead", LEAD_SESSION);
    for n in 1..=3 {
        harness.add(n);
    }
    let (first, exit) = harness.cli(
        &lead_caller(),
        &format!("queue --team {TEAM} --limit 2"),
        "",
    );
    assert_eq!(exit, 0, "{first}");
    let cursor = first["next"].as_str().expect("a next page").to_owned();
    let first_ids: Vec<u64> = first["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["id"].as_u64().unwrap())
        .collect();
    assert_eq!(first_ids, [1, 2]);
    harness.add(4);
    let (second, exit) = harness.cli(
        &lead_caller(),
        &format!("queue --team {TEAM} --limit 2 --after {cursor}"),
        "",
    );
    assert_eq!(exit, 0, "{second}");
    let second_ids: Vec<u64> = second["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["id"].as_u64().unwrap())
        .collect();
    assert_eq!(second_ids, [3], "the first page's ceiling stays");

    let (reply, exit) = harness.cli(
        &lead_caller(),
        &format!("inbox --team {TEAM} --limit 2 --after {cursor}"),
        "",
    );
    assert_eq!((code(&reply), exit), ("invalid_state", 1), "{reply}");
    let (reply, _) = harness.cli(
        &lead_caller(),
        &format!("queue --team {TEAM} --limit 201"),
        "",
    );
    assert_eq!(code(&reply), "over_cap");
}

#[test]
fn a_member_and_an_unbound_session_do_not_read_the_leads_lists() {
    let harness = seeded();
    for line in ["inbox", "queue", "cards", "journal"] {
        let (reply, exit) = harness.cli(
            &Caller::Session(key(M1_SESSION)),
            &format!("{line} --team {TEAM}"),
            "",
        );
        assert_eq!((code(&reply), exit), ("not_allowed", 1), "{line} as m1");
        let (reply, exit) = harness.cli(&unbound_caller(), &format!("{line} --team {TEAM}"), "");
        assert_eq!(
            (code(&reply), exit),
            ("not_allowed", 1),
            "{line} as another session"
        );
    }
    // Any other session may read status; naming the team is required.
    let reply = status(&harness, &unbound_caller());
    assert_eq!(reply["team"]["id"], TEAM);
    let (reply, _) = harness.cli(&unbound_caller(), "status", "");
    assert_eq!(code(&reply), "not_found");
    let (reply, _) = harness.cli(&person_caller(), "status", "");
    assert_eq!(code(&reply), "not_found", "the person names the team");
}

#[test]
fn the_inbox_and_the_journal_name_the_report_and_the_answer() {
    let harness = seeded();
    let (reply, _) = harness.cli(&lead_caller(), &format!("inbox --team {TEAM}"), "");
    let rows = reply["rows"].as_array().unwrap();
    assert_eq!(rows[0]["kind"], "report");
    assert_eq!(rows[0]["state"], "open");
    assert_eq!(rows[0]["node"], 2);
    assert_eq!(rows[1]["state"], "closed");
    assert_eq!(
        rows[1]["reply"],
        "Keep it, and say so in the commit message."
    );

    let (reply, _) = harness.cli(
        &lead_caller(),
        &format!("journal --team {TEAM} --limit 200"),
        "",
    );
    let kinds: Vec<&str> = reply["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["kind"].as_str().unwrap())
        .collect();
    for kind in [
        "create",
        "set_member_session",
        "add_task",
        "observe_task",
        "record_report",
    ] {
        assert!(kinds.contains(&kind), "{kind} is missing from {kinds:?}");
    }
}

#[test]
fn a_members_current_work_shows_its_own_questions_with_the_answer() {
    let harness = seeded();
    let (reply, exit) = harness.cli(&Caller::Session(key(M1_SESSION)), "current", "");
    assert_eq!(exit, 0, "{reply}");
    let node = &reply["nodes"][0];
    let question = &node["questions"][0];
    assert_eq!(
        question["question"],
        "Should the empty case keep its old width?"
    );
    assert_eq!(question["state"], "answered");
    assert_eq!(
        question["answer"],
        "Keep it, and say so in the commit message."
    );
    assert_eq!(node["questions_more"], false);
    let (lead, _) = harness.cli(&Caller::Session(key(LEAD_SESSION)), "current", "");
    assert!(
        lead["nodes"].as_array().unwrap().is_empty(),
        "nobody else's"
    );
}

#[test]
fn only_a_session_reads_the_current_work() {
    let harness = seeded();
    let (reply, exit) = harness.cli(&Caller::Session(key(LEAD_SESSION)), "current", "");
    assert_eq!(exit, 0, "{reply}");
    assert_eq!(reply["member"]["id"], "lead");
    assert!(reply["nodes"].as_array().unwrap().is_empty());
    let (reply, exit) = harness.cli(&Caller::Session(key(M1_SESSION)), "current", "");
    assert_eq!(exit, 0, "{reply}");
    let nodes = reply["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0]["implementer"], "m1");
    assert_eq!(nodes[0]["state"], "reported");
    for caller in [person_caller(), unbound_caller()] {
        let (reply, exit) = harness.cli(&caller, "current", "");
        assert_eq!((code(&reply), exit), ("not_allowed", 1), "{reply}");
    }
}

#[test]
fn a_read_refuses_words_it_does_not_take() {
    let harness = seeded();
    for (line, expected) in [
        (format!("status --team {TEAM} now"), "invalid_state"),
        (format!("status --team {TEAM} --limit 5"), "invalid_state"),
        ("status --team nope".into(), "invalid_state"),
        ("status --colour red".into(), "invalid_state"),
        (
            "queue --team 00000000-0000-4000-8000-00000000beee".to_owned(),
            "not_found",
        ),
    ] {
        let (reply, exit) = harness.cli(&lead_caller(), &line, "");
        assert_eq!((code(&reply), exit), (expected, 1), "{line}: {reply}");
    }
}

const LEAD_B_SESSION: &str = "launch_id:00000000-0000-4000-8000-000000000065";

#[test]
fn another_teams_lead_does_not_pass_the_lead_reads() {
    let harness = Harness::new();
    harness.create();
    harness.create_team(TEAM_B, "Docs", "zerus/team/docs");
    harness.bind("lead", LEAD_SESSION);
    harness.bind_for(TEAM_B, "lead", LEAD_B_SESSION);
    // Both teams call their lead "lead"; only the session's own team counts.
    let b_lead = Caller::Session(key(LEAD_B_SESSION));
    for line in ["inbox", "queue", "cards", "journal"] {
        let (reply, exit) = harness.cli(&b_lead, &format!("{line} --team {TEAM}"), "");
        assert_eq!((code(&reply), exit), ("not_allowed", 1), "{line}");
        let (reply, exit) = harness.cli(&b_lead, &format!("{line} --team {TEAM_B}"), "");
        assert_eq!(exit, 0, "its own team: {line}: {reply}");
    }
    let (reply, exit) = harness.cli(&lead_caller(), &format!("inbox --team {TEAM_B}"), "");
    assert_eq!(
        (code(&reply), exit),
        ("not_allowed", 1),
        "the same the other way"
    );
}

#[test]
fn list_reads_the_teams_of_this_computer_in_id_order() {
    let harness = Harness::new();
    harness.create();
    harness.create_team(TEAM_B, "Docs", "zerus/team/docs");
    harness.bind("lead", LEAD_SESSION);
    // Any proved caller may read the list, including another team's session.
    let (reply, exit) = harness.cli(&Caller::Session(key(OTHER_SESSION)), "list", "");
    assert_eq!(exit, 0, "{reply}");
    assert_eq!(reply["ok"], true);
    let rows = reply["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    // ID order: beee sorts before beef.
    assert_eq!(rows[0]["id"], TEAM_B);
    assert_eq!(rows[1]["id"], TEAM);
    for (id, name) in [(TEAM, "Fixes"), (TEAM_B, "Docs")] {
        let row = rows.iter().find(|row| row["id"] == id).unwrap();
        assert_eq!(row["name"], name);
        assert_eq!(row["project"], "example-project");
        assert_eq!(row["status"], "running");
        assert_eq!(row["lead"], "lead");
        assert_eq!(row["home"], MACHINE);
        assert!(row["revision"].as_u64().is_some());
        assert_eq!(
            row.as_object().unwrap().len(),
            if id == TEAM { 9 } else { 8 }
        );
    }
    let bound = rows.iter().find(|row| row["id"] == TEAM).unwrap();
    assert_eq!(
        bound["lead_session"],
        serde_json::json!({"machine": MACHINE, "session_id": LEAD_SESSION})
    );
    let unbound = rows.iter().find(|row| row["id"] == TEAM_B).unwrap();
    assert!(
        unbound["lead_session"].is_null(),
        "the unbound lead is null"
    );
    let (reply, _) = harness.cli(&person_caller(), "list --limit 1", "");
    assert_eq!(reply["rows"].as_array().unwrap().len(), 1);
    assert!(reply["next"].is_string());
    let cursor = reply["next"].as_str().unwrap().to_owned();
    let (reply, _) = harness.cli(
        &person_caller(),
        &format!("list --limit 1 --after {cursor}"),
        "",
    );
    assert_eq!(reply["rows"][0]["id"], TEAM);
    let (reply, _) = harness.cli(&person_caller(), "list --team x", "");
    assert_eq!(code(&reply), "invalid_state", "list takes no team");
}

#[test]
fn waiting_lists_every_open_owner_card_oldest_first_and_only_for_the_person() {
    let harness = seeded();
    harness.create_team(TEAM_B, "Docs", "zerus/team/docs");
    harness.apply_for(
        TEAM_B,
        &person(),
        crate::teams::ops::Operation::OpenCard {
            kind: "note".into(),
            text: "Decide the release date.".into(),
            node: None,
            decisions: None,
                parameters: None,
        },
    );
    let (reply, exit) = harness.cli(&person_caller(), "waiting", "");
    assert_eq!(exit, 0, "{reply}");
    let rows = reply["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert!(
        rows[0]["seq"].as_u64() < rows[1]["seq"].as_u64(),
        "oldest first"
    );
    assert_eq!(rows[0]["team"], TEAM);
    assert_eq!(rows[0]["kind"], "note");
    assert_eq!(rows[1]["team"], TEAM_B);
    for row in rows {
        for field in [
            "seq",
            "kind",
            "node",
            "actor",
            "text",
            "state",
            "decision",
            "decided_actor",
            "created_ms",
            "team",
            "notes",
        ] {
            assert!(row.get(field).is_some(), "{field} is missing from {row}");
        }
    }
    let (reply, exit) = harness.cli(&lead_caller(), "waiting", "");
    assert_eq!((code(&reply), exit), ("not_allowed", 1), "{reply}");
    let (reply, _) = harness.cli(&Caller::Session(key(M1_SESSION)), "waiting", "");
    assert_eq!(code(&reply), "not_allowed");
}
