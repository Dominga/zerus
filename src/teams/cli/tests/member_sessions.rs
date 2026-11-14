use super::harness::*;
mod removed;
mod robustness;
use crate::teams::cli::actor::Caller;
use crate::teams::ops::Operation;
use serde_json::json;
use zerus_team_plan::model::TeamStatus;

#[test]
fn bound_member_sessions_are_ordered_metadata_for_every_proved_caller() {
    let h = Harness::new();
    h.create();
    h.bind("m3", "run_id:review-example");
    // Bind out of order, leaving m2 unbound. Member order is the store's ID order.
    h.bind("m1", M1_SESSION);
    h.bind("lead", LEAD_SESSION);
    let expected = json!([
        {"member":"lead","role":"lead","machine":MACHINE,"session_id":LEAD_SESSION},
        {"member":"m1","role":"developer","machine":MACHINE,"session_id":M1_SESSION},
        {"member":"m3","role":"reviewer","machine":MACHINE,"session_id":"run_id:review-example"}
    ]);
    for caller in [
        Caller::Person,
        Caller::Session(key(LEAD_SESSION)),
        Caller::Session(key(M1_SESSION)),
        Caller::Session(key(OTHER_SESSION)),
    ] {
        let (reply, exit) = h.cli(&caller, "list", "");
        assert_eq!(exit, 0, "{reply}");
        let row = &reply["rows"][0];
        assert_eq!(row["member_sessions"], expected);
        assert_eq!(row["lead_session"], json!(key(LEAD_SESSION)));
    }
}

#[test]
fn an_unbound_roster_has_no_member_sessions_field() {
    let h = Harness::new();
    h.create();
    let (reply, exit) = h.cli(&Caller::Person, "list", "");
    assert_eq!(exit, 0, "{reply}");
    let row = &reply["rows"][0];
    assert!(row.get("member_sessions").is_none());
    assert!(row["lead_session"].is_null());
}

#[test]
fn stopped_and_archived_teams_omit_member_sessions_and_keep_lead_session() {
    for archived in [false, true] {
        let h = Harness::new();
        h.create();
        h.bind("lead", LEAD_SESSION);
        h.bind("m1", M1_SESSION);
        h.apply(&person(), Operation::Stop);
        if archived {
            h.apply(&person(), Operation::Archive);
        }
        let (reply, exit) = h.cli(&Caller::Person, "list", "");
        assert_eq!(exit, 0, "{reply}");
        let row = &reply["rows"][0];
        assert!(row.get("member_sessions").is_none(), "{row}");
        assert_eq!(row["lead_session"], json!(key(LEAD_SESSION)));
    }
}

#[test]
fn only_planning_running_and_paused_rows_expose_member_sessions() {
    let h = Harness::new();
    h.create();
    h.bind("m1", M1_SESSION);
    for (status, visible) in [
        (TeamStatus::Planning, true),
        (TeamStatus::Running, true),
        (TeamStatus::Paused, true),
        (TeamStatus::Draft, false),
        (TeamStatus::Finished, false),
    ] {
        // Planning, Draft and Finished have no public transition in M1.
        h.store()
            .db
            .as_ref()
            .unwrap()
            .execute(
                "UPDATE teams SET status=?2 WHERE id=?1",
                rusqlite::params![TEAM, serde_json::to_string(&status).unwrap()],
            )
            .unwrap();
        let (reply, exit) = h.cli(&Caller::Person, "list", "");
        assert_eq!(exit, 0, "{reply}");
        let row = &reply["rows"][0];
        assert_eq!(row.get("member_sessions").is_some(), visible, "{status:?}");
        if visible {
            assert_eq!(
                row["member_sessions"],
                json!([
                    {"member":"m1","role":"developer","machine":MACHINE,"session_id":M1_SESSION}
                ])
            );
        }
    }
}

#[test]
fn member_sessions_follow_the_existing_team_page_and_cursor() {
    let h = Harness::new();
    h.create();
    h.create_team(TEAM_B, "Docs", "zerus/team/docs");
    h.bind("m1", M1_SESSION);
    h.bind_for(TEAM_B, "m3", "run_id:docs-review-example");
    let (first, exit) = h.cli(&Caller::Person, "list --limit 1", "");
    assert_eq!(exit, 0, "{first}");
    assert_eq!(first["rows"].as_array().unwrap().len(), 1);
    assert_eq!(first["rows"][0]["id"], TEAM_B);
    assert_eq!(
        first["rows"][0]["member_sessions"],
        json!([
            {"member":"m3","role":"reviewer","machine":MACHINE,"session_id":"run_id:docs-review-example"}
        ])
    );
    let cursor = first["next"].as_str().unwrap();
    let added = uid(0x2000);
    h.create_team(&added, "Added later", "zerus/team/added-later");
    let (second, exit) = h.cli(
        &Caller::Person,
        &format!("list --limit 1 --after {cursor}"),
        "",
    );
    assert_eq!(exit, 0, "{second}");
    assert_eq!(second["rows"].as_array().unwrap().len(), 1);
    assert_eq!(second["rows"][0]["id"], TEAM);
    assert_eq!(
        second["rows"][0]["member_sessions"],
        json!([
            {"member":"m1","role":"developer","machine":MACHINE,"session_id":M1_SESSION}
        ])
    );
    let next = second["next"].as_str().unwrap();
    let (end, exit) = h.cli(
        &Caller::Person,
        &format!("list --limit 1 --after {next}"),
        "",
    );
    assert_eq!(exit, 0, "{end}");
    assert!(end["rows"].as_array().unwrap().is_empty());
}
