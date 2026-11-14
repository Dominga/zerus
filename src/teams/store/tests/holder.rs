use super::*;
use crate::teams::ops::HandoutOutcome;
use rusqlite::params;

#[test]
fn attempt_holder_is_only_the_latest_current_generation_delivery() {
    let mut h = Harness::new();
    h.bind("m1");
    let node = h.add(&person(), 1);
    assert!(h.store.attempt_holder(TEAM, node).unwrap().is_none());
    let id = h.intent(node, "m1");
    assert!(h.store.attempt_holder(TEAM, node).unwrap().is_none());
    h.outcome(
        &id,
        HandoutOutcome::Refused {
            reason: "Not delivered".into(),
        },
    );
    assert!(h.store.attempt_holder(TEAM, node).unwrap().is_none());
    let delivered = h.submit(node, "m1");
    let holder = h.store.attempt_holder(TEAM, node).unwrap().unwrap();
    assert_eq!(holder.member, "m1");
    assert_eq!(holder.generation, 1);
    assert!(h
        .store
        .attempt_holder("other-team", node)
        .unwrap()
        .is_none());
    let db = h.store.db.as_ref().unwrap();
    db.execute(
        "UPDATE effects SET state='reported' WHERE team=?1 AND request_id=?2",
        params![TEAM, id],
    )
    .unwrap();
    for state in ["refused", "superseded", "reserved"] {
        db.execute(
            "UPDATE effects SET state=?3 WHERE team=?1 AND request_id=?2",
            params![TEAM, delivered, state],
        )
        .unwrap();
        assert!(
            h.store.attempt_holder(TEAM, node).unwrap().is_none(),
            "{state} fell back to the older report"
        );
    }
    let empty = Harness::empty();
    assert!(empty.store.attempt_holder(TEAM, 1).unwrap().is_none());
    std::fs::remove_file(h.temp.path().join("teams/preview")).unwrap();
    refused(h.store.attempt_holder(TEAM, node), code::TEAMS_INACTIVE);
}
#[test]
fn attempt_lookup_uses_its_index_and_an_old_version_two_file_gains_it_on_open() {
    let mut h = Harness::new();
    h.bind("m1");
    let node = h.add(&person(), 1);
    h.store.db.as_ref().unwrap().execute("WITH RECURSIVE old(n) AS (VALUES(1) UNION ALL SELECT n+1 FROM old WHERE n<5000) INSERT INTO effects(team,node,member,generation,request_id,actor,state,created_ms) SELECT ?1,?2,'m1',1,printf('00000000-0000-4000-8000-%012x',n+10000),'{}','refused',1 FROM old",params![TEAM,node]).unwrap();
    h.submit(node, "m1");
    assert_eq!(
        h.store.attempt_holder(TEAM, node).unwrap().unwrap().member,
        "m1"
    );
    let db = h.store.db.as_ref().unwrap();
    let mut statement = db
        .prepare(&format!(
            "EXPLAIN QUERY PLAN {}",
            super::super::holder::HOLDER_SQL
        ))
        .unwrap();
    let plan = statement
        .query_map(params![TEAM, node], |r| r.get::<_, String>(3))
        .unwrap()
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap()
        .join("\n");
    assert!(plan.contains("effects_attempt"), "{plan}");
    assert!(!plan.contains("SCAN e"), "{plan}");
    drop(statement);
    db.execute_batch("DROP INDEX effects_attempt;").unwrap();
    h.store = Store::open(h.temp.path()).unwrap();
    let count: u32 = h
        .store
        .db
        .as_ref()
        .unwrap()
        .query_row(
            "SELECT count(*) FROM sqlite_schema WHERE type='index' AND name='effects_attempt'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
    assert_eq!(
        h.store
            .attempt_holder(TEAM, node)
            .unwrap()
            .unwrap()
            .generation,
        1
    );
    let version: u32 = h
        .store
        .db
        .as_ref()
        .unwrap()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 2);
}
