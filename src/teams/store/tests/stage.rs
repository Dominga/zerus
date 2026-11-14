use super::*;
use rusqlite::params;

#[test]
fn stage_pages_exclude_additions_and_use_the_pending_index_without_history() {
    let mut h = Harness::empty();
    h.run(&person(), create()).unwrap();
    h.bind("m1");
    h.bind("m3");
    let db = h.store.db.as_ref().unwrap();
    db.execute("WITH RECURSIVE history(n) AS (VALUES(1) UNION ALL SELECT n+1 FROM history WHERE n<5000) INSERT INTO node_identity(team,id,task_project,task_uuid) SELECT ?1,n,'example-project','' FROM history",[TEAM]).unwrap();
    db.execute("INSERT INTO stage_tasks(team,node,target,generation,state,created_ms) SELECT team,id,1,1,'finished',1 FROM node_identity WHERE team=?1",[TEAM]).unwrap();
    db.execute("UPDATE teams SET next_node=5001 WHERE id=?1", [TEAM])
        .unwrap();
    let parent = h.add(&person(), 1);
    h.submit(parent, "m1");
    h.report(parent, "m1");
    let first = h.store.stage_tasks(TEAM, 1, None).unwrap();
    assert_eq!(first.rows[0].node, 5002);
    let n = h.add(&person(), 2);
    h.submit(n, "m1");
    h.report(n, "m1");
    assert!(h
        .store
        .stage_tasks(TEAM, 1, first.next.as_ref())
        .unwrap()
        .rows
        .is_empty());
    assert_eq!(h.store.stage_tasks(TEAM, 200, None).unwrap().rows.len(), 2);
    let db = h.store.db.as_ref().unwrap();
    let mut s = db
        .prepare(&format!(
            "EXPLAIN QUERY PLAN {}",
            super::super::stage::PENDING_SQL
        ))
        .unwrap();
    let plan = s
        .query_map(params![TEAM, 0, 20000, 1], |r| r.get::<_, String>(3))
        .unwrap()
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap()
        .join("\n");
    assert!(plan.contains("stage_tasks_pending"), "{plan}");
}

#[test]
fn a_version_two_store_gets_empty_pipeline_storage_on_open_without_a_migration() {
    let mut h = Harness::empty();
    h.run(&person(), create()).unwrap();
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute_batch("DROP TABLE verdicts; DROP TABLE stage_tasks; DROP TABLE pipeline_runs;")
        .unwrap();
    h.store = Store::open(h.temp.path()).unwrap();
    let version: u32 = h
        .store
        .db
        .as_ref()
        .unwrap()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 2);
    h.bind("m1");
    let parent = h.add(&person(), 1);
    h.submit(parent, "m1");
    h.report(parent, "m1");
    assert_eq!(h.store.stage_tasks(TEAM, 200, None).unwrap().rows.len(), 1);
}
