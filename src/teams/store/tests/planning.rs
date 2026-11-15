use super::*;
use crate::teams::ops::{
    tests::{create, person, Harness as WriterHarness},
    Operation,
};
use rusqlite::params;

#[test]
fn planning_and_queue_teams_share_a_bounded_live_index() {
    let mut h = WriterHarness::new();
    let plan = uuid::Uuid::new_v4().to_string();
    let mut input = serde_json::to_value(create()).unwrap();
    input["op"] = "start_planning".into();
    input["goal"] = "Prepare the indexed plan.".into();
    input["name"] = "Planning index control".into();
    input["integration_branch"] = "teams/planning-index-control".into();
    h.run_for(&plan, &person(), serde_json::from_value(input).unwrap())
        .unwrap();
    let rows = h.store.live_teams(200, None).unwrap().rows;
    assert_eq!(rows.len(), 2);
    assert!(rows
        .iter()
        .any(|r| r.id == plan && r.status == TeamStatus::Planning));
    assert!(rows
        .iter()
        .any(|r| r.id == TEAM && r.status == TeamStatus::Running));
    let sql = super::super::cross::teams_sql(true);
    let db = h.store.db.as_ref().unwrap();
    let mut statement = db.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
    let steps = statement
        .query_map(params!["", i64::MAX, 1], |r| r.get::<_, String>(3))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert!(
        steps.iter().any(|s| s.contains("teams_planning_live")),
        "{steps:?}"
    );
    assert!(
        steps.iter().all(|s| !s.contains("TEMP B-TREE")),
        "{steps:?}"
    );
    drop(statement);
    h.run_for(&plan, &person(), Operation::Stop).unwrap();
    h.run_for(&plan, &person(), Operation::Archive).unwrap();
    let rows = h.store.live_teams(200, None).unwrap().rows;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, TEAM);
}
