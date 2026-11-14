use super::*;

#[test]
fn m2_a_malformed_member_state_omits_only_its_teams_member_sessions() {
    let h = Harness::new();
    h.create();
    h.create_team(TEAM_B, "Docs", "zerus/team/docs");
    h.bind("lead", LEAD_SESSION);
    h.bind("m1", M1_SESSION);
    h.bind_for(TEAM_B, "lead", "run_id:docs-lead-example");
    h.bind_for(TEAM_B, "m1", "run_id:docs-member-example");
    h.store()
        .db
        .as_ref()
        .unwrap()
        .execute(
            "UPDATE members SET state='\"unknown_state\"' WHERE team=?1 AND id='m2'",
            [TEAM_B],
        )
        .unwrap();
    let (reply, exit) = h.cli(&Caller::Person, "list", "");
    assert_eq!(exit, 0, "{reply}");
    let rows = reply["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["id"], TEAM_B);
    assert!(rows[0].get("member_sessions").is_none());
    assert_eq!(
        rows[0]["lead_session"],
        json!(key("run_id:docs-lead-example"))
    );
    assert_eq!(rows[1]["id"], TEAM);
    assert_eq!(rows[1]["member_sessions"].as_array().unwrap().len(), 2);
    let (status, exit) = h.cli(&Caller::Person, &format!("status --team {TEAM_B}"), "");
    assert_eq!(exit, 1, "{status}");
    assert_eq!(code(&status), "invalid_state");
    let (page, exit) = h.cli(&Caller::Person, "list --limit 1", "");
    assert_eq!(exit, 0, "{page}");
    let cursor = page["next"].as_str().unwrap();
    let (page, exit) = h.cli(
        &Caller::Person,
        &format!("list --limit 1 --after {cursor}"),
        "",
    );
    assert_eq!(exit, 0, "{page}");
    assert_eq!(page["rows"][0]["id"], TEAM);
}

#[test]
fn a_failed_member_read_is_not_hidden_as_an_unusable_roster() {
    let h = Harness::new();
    h.create();
    h.store()
        .db
        .as_ref()
        .unwrap()
        .execute_batch("PRAGMA foreign_keys=OFF; DROP TABLE members")
        .unwrap();
    let (reply, exit) = h.cli(&Caller::Person, "list", "");
    assert_eq!(exit, 1, "{reply}");
    assert!(reply.get("rows").is_none());
}

fn fill_to(h: &Harness, team: &str, target: usize) {
    let store = h.store();
    let db = store.db.as_ref().unwrap();
    let have: usize = db
        .query_row("SELECT count(*) FROM members WHERE team=?1", [team], |r| {
            r.get(0)
        })
        .unwrap();
    let mut member = store.member(team, "m1").unwrap().unwrap();
    for index in have..target {
        member.id = format!("z{index:04}");
        member.session = Some(key(&format!("run_id:overflow-member-{index:04}")));
        db.execute(
            "INSERT INTO members(team,id,data,machine,session_id,state) VALUES(?1,?2,?3,?4,?5,?6)",
            rusqlite::params![
                team,
                member.id,
                serde_json::to_string(&member).unwrap(),
                MACHINE,
                member.session.as_ref().unwrap().session_id,
                serde_json::to_string(&member.state).unwrap()
            ],
        )
        .unwrap();
    }
}

#[test]
fn l1_an_oversized_roster_omits_only_its_memberships_and_keeps_the_page_usable() {
    let h = Harness::new();
    h.create();
    h.create_team(TEAM_B, "Docs", "zerus/team/docs");
    h.bind("lead", LEAD_SESSION);
    h.bind("m1", M1_SESSION);
    h.bind_for(TEAM_B, "lead", "run_id:docs-lead-example");
    fill_to(&h, TEAM_B, 202);
    let (reply, exit) = h.cli(&Caller::Person, "list", "");
    assert_eq!(exit, 0, "{reply}");
    let rows = reply["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["id"], TEAM_B);
    assert!(rows[0].get("member_sessions").is_none());
    assert_eq!(
        rows[0]["lead_session"],
        json!(key("run_id:docs-lead-example"))
    );
    assert_eq!(rows[1]["id"], TEAM);
    assert_eq!(
        rows[1]["member_sessions"],
        json!([
            {"member":"lead","role":"lead","machine":MACHINE,"session_id":LEAD_SESSION},
            {"member":"m1","role":"developer","machine":MACHINE,"session_id":M1_SESSION}
        ])
    );
    let (first, exit) = h.cli(&Caller::Person, "list --limit 1", "");
    assert_eq!(exit, 0, "{first}");
    assert_eq!(first["rows"][0]["id"], TEAM_B);
    let cursor = first["next"].as_str().unwrap();
    let (next, exit) = h.cli(
        &Caller::Person,
        &format!("list --limit 1 --after {cursor}"),
        "",
    );
    assert_eq!(exit, 0, "{next}");
    assert_eq!(next["rows"][0]["id"], TEAM);
    assert!(next["rows"][0].get("member_sessions").is_some());
}

#[test]
fn status_still_reports_the_oversized_roster_bound() {
    let h = Harness::new();
    h.create();
    fill_to(&h, TEAM, 202);
    let (reply, exit) = h.cli(&Caller::Person, &format!("status --team {TEAM}"), "");
    assert_eq!(exit, 1, "{reply}");
    assert_eq!(code(&reply), "over_cap");
    assert_eq!(
        h.store().member_sessions(TEAM).unwrap_err().code,
        "over_cap"
    );
}

#[test]
fn u2_an_undecodable_lead_record_does_not_refuse_the_list_page() {
    for stopped in [false, true] {
        let h = Harness::new();
        h.create();
        h.create_team(TEAM_B, "Docs", "zerus/team/docs");
        h.bind("lead", LEAD_SESSION);
        h.store().db.as_ref().unwrap().execute("UPDATE members SET state='\"unknown_state\"',data=json_set(data,'$.state','unknown_state') WHERE team=?1 AND id='lead'",[TEAM]).unwrap();
        if stopped {
            h.store()
                .db
                .as_ref()
                .unwrap()
                .execute("UPDATE teams SET status='\"stopped\"' WHERE id=?1", [TEAM])
                .unwrap();
        }
        let (reply, exit) = h.cli(&Caller::Person, "list", "");
        assert_eq!(exit, 0, "{reply}");
        let rows = reply["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        let row = rows.iter().find(|r| r["id"] == TEAM).unwrap();
        assert_eq!(row["lead_session"], json!(key(LEAD_SESSION)));
        assert!(row.get("member_sessions").is_none());
    }
}
