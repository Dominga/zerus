use super::*;
use crate::teams::store::member_sessions::{MAX_ROSTER, MEMBER_SESSIONS_SQL};
use rusqlite::params;

#[test]
fn member_session_read_is_lazy_and_preview_gated() {
    let h = Harness::empty();
    assert!(h.store.member_sessions("missing").unwrap().is_empty());
    assert!(!h.temp.path().join("teams/teams-v1.sqlite3").exists());
    std::fs::remove_file(h.temp.path().join("teams/preview")).unwrap();
    refused(h.store.member_sessions("missing"), code::TEAMS_INACTIVE);
}

#[test]
fn member_session_read_uses_the_member_primary_key_without_a_sort() {
    let h = Harness::new();
    let db = h.store.db.as_ref().unwrap();
    let mut statement = db
        .prepare(&format!("EXPLAIN QUERY PLAN {MEMBER_SESSIONS_SQL}"))
        .unwrap();
    let plan = statement
        .query_map(params![TEAM, MAX_ROSTER + 1], |r| r.get::<_, String>(3))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
        .join("\n");
    assert!(
        plan.contains("SEARCH members USING INDEX sqlite_autoindex_members_1 (team=?)"),
        "{plan}"
    );
    assert!(
        !plan.contains("SCAN ") && !plan.contains("TEMP B-TREE"),
        "{plan}"
    );
}

#[test]
fn member_session_read_includes_the_whole_roster_and_refuses_an_oversized_one() {
    let h = Harness::new();
    let db = h.store.db.as_ref().unwrap();
    let count: usize = db
        .query_row("SELECT count(*) FROM members WHERE team=?1", [TEAM], |r| {
            r.get(0)
        })
        .unwrap();
    let mut member = crate::teams::ops::tests::fixture().members.remove(1);
    member.session = None;
    for index in count..MAX_ROSTER {
        member.id = format!("z{index:04}");
        if index == MAX_ROSTER - 1 {
            member.session = Some(SessionKey {
                machine: "00000000-0000-4000-8000-0000000000bb".into(),
                session_id: "run_id:last-member-example".into(),
            });
        }
        db.execute(
            "INSERT INTO members(team,id,data,machine,session_id,state) VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                TEAM,
                member.id,
                encode(&member).unwrap(),
                member.session.as_ref().map(|s| &s.machine),
                member.session.as_ref().map(|s| &s.session_id),
                encode(&member.state).unwrap()
            ],
        )
        .unwrap();
    }
    let sessions = h.store.member_sessions(TEAM).unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].member, member.id);
    assert_eq!(sessions[0].session_id, "run_id:last-member-example");
    assert!(h.store.member_sessions("another-team").unwrap().is_empty());
    member.id = "overflow".into();
    member.session = None;
    db.execute(
        "INSERT INTO members(team,id,data,state) VALUES(?1,?2,?3,?4)",
        params![
            TEAM,
            member.id,
            encode(&member).unwrap(),
            encode(&member.state).unwrap()
        ],
    )
    .unwrap();
    refused(h.store.member_sessions(TEAM), code::OVER_CAP);
}

#[test]
fn the_single_session_read_is_lazy_gated_and_uses_the_primary_key() {
    use crate::teams::store::member_sessions::MEMBER_SESSION_SQL;
    let h = Harness::empty();
    assert_eq!(h.store.member_session(TEAM, "lead").unwrap(), None);
    std::fs::remove_file(h.temp.path().join("teams/preview")).unwrap();
    refused(h.store.member_session(TEAM, "lead"), code::TEAMS_INACTIVE);
    let mut h = Harness::new();
    h.bind("lead");
    let expected = h.store.member(TEAM, "lead").unwrap().unwrap().session;
    assert_eq!(h.store.member_session(TEAM, "lead").unwrap(), expected);
    assert_eq!(h.store.member_session(TEAM, "m1").unwrap(), None);
    assert_eq!(h.store.member_session("missing", "lead").unwrap(), None);
    let db = h.store.db.as_ref().unwrap();
    db.execute(
        "UPDATE members SET data='broken' WHERE team=?1 AND id='lead'",
        [TEAM],
    )
    .unwrap();
    assert_eq!(h.store.member_session(TEAM, "lead").unwrap(), expected);
    let mut q = db
        .prepare(&format!("EXPLAIN QUERY PLAN {MEMBER_SESSION_SQL}"))
        .unwrap();
    let plan = q
        .query_map(params![TEAM, "lead"], |r| r.get::<_, String>(3))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
        .join("\n");
    assert!(
        plan.contains("SEARCH members USING INDEX sqlite_autoindex_members_1 (team=? AND id=?)"),
        "{plan}"
    );
    assert!(!plan.contains("SCAN "), "{plan}");
}
