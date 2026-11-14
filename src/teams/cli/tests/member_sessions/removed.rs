use super::*;
use zerus_team_plan::model::MemberState;

#[test]
fn l2_removed_members_keep_the_binding_but_leave_the_teams_session_list() {
    let h = Harness::new();
    h.create();
    h.bind("lead", LEAD_SESSION);
    h.bind("m1", M1_SESSION);
    h.apply(
        &person(),
        Operation::ArchiveMember {
            member: "m1".into(),
        },
    );
    let removed = h.store().member(TEAM, "m1").unwrap().unwrap();
    assert_eq!(removed.state, MemberState::Removed);
    assert_eq!(removed.session, Some(key(M1_SESSION)));
    for caller in [Caller::Person, Caller::Session(key(OTHER_SESSION))] {
        let (reply, exit) = h.cli(&caller, "list", "");
        assert_eq!(exit, 0, "{reply}");
        let row = &reply["rows"][0];
        assert_eq!(
            row["member_sessions"],
            json!([
                {"member":"lead","role":"lead","machine":MACHINE,"session_id":LEAD_SESSION}
            ])
        );
        assert_eq!(row["lead_session"], json!(key(LEAD_SESSION)));
    }
}

#[test]
fn a_roster_with_only_a_removed_binding_omits_member_sessions() {
    let h = Harness::new();
    h.create();
    h.bind("m1", M1_SESSION);
    h.apply(
        &person(),
        Operation::ArchiveMember {
            member: "m1".into(),
        },
    );
    let (reply, exit) = h.cli(&Caller::Person, "list", "");
    assert_eq!(exit, 0, "{reply}");
    assert!(reply["rows"][0].get("member_sessions").is_none());
    assert!(reply["rows"][0]["lead_session"].is_null());
}

#[test]
fn a_paused_or_stopped_member_keeps_its_team_session() {
    for operation in [
        Operation::PauseMember {
            member: "m1".into(),
        },
        Operation::StopMember {
            member: "m1".into(),
        },
    ] {
        let h = Harness::new();
        h.create();
        h.bind("m1", M1_SESSION);
        h.apply(&person(), operation);
        let (reply, exit) = h.cli(&Caller::Person, "list", "");
        assert_eq!(exit, 0, "{reply}");
        assert_eq!(
            reply["rows"][0]["member_sessions"],
            json!([
                {"member":"m1","role":"developer","machine":MACHINE,"session_id":M1_SESSION}
            ])
        );
    }
}
