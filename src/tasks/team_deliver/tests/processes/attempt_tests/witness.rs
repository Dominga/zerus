//! The live team's end has one writer: the observation. A child answer is
//! tested against that production rule with real replica execution facts.
use super::*;
use zerus_team_plan::model::NodeState;

fn cancel(f: &mut Fixture) -> (u64, Value) {
    delivered(f);
    decide(f);
    let decision = f.h.store.pending_cancels(TEAM, 1, 0).unwrap()[0].decision;
    let out = call(f, "team-cancel");
    assert_eq!(out["reply"]["state"], "cancelled", "{out}");
    assert_eq!(out["reply"]["attempt"], 1);
    assert_eq!(out["reply"]["attempt_id"], f.body.id);
    assert!(out["reply"].get("attempt_conflict").is_none());
    assert!(f.document().view().unwrap().cancelled_at.is_some());
    (decision, out["reply"].clone())
}

fn record(
    f: &mut Fixture,
    decision: u64,
    reply: &Value,
) -> std::result::Result<ops::Reply, zerus_team_plan::Problem> {
    f.h.run(
        &ops::tests::service(),
        ops::Operation::RecordCancel {
            node: f.body.node,
            decision,
            state: match reply["state"].as_str().unwrap() {
                "cancelled" => ops::CancelState::Cancelled,
                "released" => ops::CancelState::Released,
                other => panic!("not a successful cancel answer: {other}"),
            },
            reason: String::new(),
            attempt: reply["attempt"].as_u64().unwrap(),
            attempt_id: reply["attempt_id"].as_str().unwrap().into(),
        },
    )
}

// A semantic snapshot of every domain row in the small, private fixture
// database, including journals. The common operation receipt is checked apart.
fn stored(f: &Fixture) -> BTreeMap<String, Vec<Vec<String>>> {
    let db = f.h.store.db.as_ref().unwrap();
    let tables: Vec<String> = db
        .prepare(
            "SELECT name FROM sqlite_schema WHERE type='table' AND name<>'requests' ORDER BY name",
        )
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    tables
        .into_iter()
        .map(|table| {
            let mut query = db
                .prepare(&format!(
                    "SELECT * FROM \"{}\" ORDER BY rowid",
                    table.replace('"', "\"\"")
                ))
                .unwrap();
            let columns = query.column_count();
            let rows = query
                .query_map([], |row| {
                    (0..columns)
                        .map(|column| {
                            row.get_ref(column).map(|value| match value {
                                rusqlite::types::ValueRef::Text(text) => {
                                    format!("text:{:?}", String::from_utf8_lossy(text))
                                }
                                other => format!("{other:?}"),
                            })
                        })
                        .collect::<rusqlite::Result<Vec<_>>>()
                })
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap();
            (table, rows)
        })
        .collect()
}

fn refused_without_a_write(f: &mut Fixture, decision: u64, reply: &Value) {
    let before = stored(f);
    let task = bytes(f);
    let receipts =
        f.h.store
            .db
            .as_ref()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM requests", [], |row| {
                row.get::<_, u64>(0)
            })
            .unwrap();
    let error = record(f, decision, reply).unwrap_err();
    assert_eq!(error.code, zerus_team_plan::problem::code::INVALID_STATE);
    let after = stored(f);
    assert_eq!(
        after.keys().collect::<Vec<_>>(),
        before.keys().collect::<Vec<_>>()
    );
    for (table, rows) in &before {
        assert_eq!(after.get(table), Some(rows), "domain table {table} changed");
    }
    assert_eq!(bytes(f), task);
    let db = f.h.store.db.as_ref().unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM requests", [], |row| row
            .get::<_, u64>(0))
            .unwrap(),
        receipts + 1
    );
    let receipt: String = db
        .query_row(
            "SELECT result FROM requests ORDER BY rowid DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(serde_json::from_str::<Value>(&receipt)
        .unwrap()
        .get("Refused")
        .is_some());
}

fn observe(f: &Fixture) {
    crate::teams::tasks::observe(&f.config.state, &f.task).unwrap();
}

fn end(f: &Fixture) -> (u32, bool, u64, String, NodeState) {
    let node = f.h.store.node(TEAM, f.body.node).unwrap().unwrap();
    assert_eq!(node.task.as_ref(), Some(&f.task));
    let bound: bool = f.h.store.db.as_ref().unwrap().query_row(
        "SELECT EXISTS(SELECT 1 FROM live_tasks WHERE team=?1 AND node=?2 AND task_project=?3 AND task_uuid=?4)",
        params![TEAM, f.body.node, f.task.project, f.task.uuid], |row| row.get(0),
    ).unwrap();
    let pending: u64 =
        f.h.store
            .db
            .as_ref()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM decisions WHERE team=?1 AND node=?2 AND state<>'settled'",
                params![TEAM, f.body.node],
                |row| row.get(0),
            )
            .unwrap();
    assert!(f.h.store.pending_cancels(TEAM, 200, 0).unwrap().is_empty());
    assert!(f
        .h
        .store
        .effects(TEAM, 200, None)
        .unwrap()
        .rows
        .iter()
        .all(|e| !matches!(e.state.as_str(), "reserved" | "submitted" | "uncertain")));
    assert!(f
        .h
        .store
        .inbox(TEAM, 200, None)
        .unwrap()
        .rows
        .iter()
        .all(|i| i.state != "open"));
    let journal = f.h.store.journal(TEAM, 200, None).unwrap();
    let state = (
        f.h.store.generation(TEAM, f.body.node).unwrap().unwrap(),
        bound,
        pending,
        journal.rows.last().unwrap().kind.clone(),
        node.state,
    );
    assert_eq!(
        state,
        (2, true, 0, "person_override".into(), NodeState::InProgress)
    );
    assert_eq!(f.document().stage().unwrap(), Stage::Active);
    assert!(f.document().team().unwrap().is_some());
    state
}

#[test]
fn the_live_cancel_is_settled_only_by_observing_the_cancelled_or_revived_task() {
    for paused in [false, true] {
        for revived in [false, true] {
            let mut f = Fixture::new();
            let (decision, reply) = cancel(&mut f);
            if paused {
                f.h.run(&person(), ops::Operation::Pause).unwrap();
            }
            // A successful child answer is never a second end writer for
            // this bound live node. The service observes instead.
            refused_without_a_write(&mut f, decision, &reply);
            if revived {
                person_send(&f, false);
                let before = bytes(&f);
                let repeat = call(&f, "team-cancel");
                assert_eq!(repeat["reply"]["state"], "stale", "{repeat}");
                assert_eq!(bytes(&f), before);
                observe(&f);
                end(&f);
            } else {
                let before = bytes(&f);
                observe(&f);
                assert!(!bound(&f));
                assert!(f.h.store.pending_cancels(TEAM, 200, 0).unwrap().is_empty());
                assert_eq!(
                    f.h.store.node(TEAM, f.body.node).unwrap().unwrap().state,
                    NodeState::Cancelled
                );
                assert_eq!(f.h.store.generation(TEAM, f.body.node).unwrap(), Some(1));
                assert_eq!(bytes(&f), before);
                // Once observed cancelled the task has left the team.
                // A later ordinary revival cannot restore the old node.
                person_send(&f, false);
                let before = stored(&f);
                observe(&f);
                assert_eq!(stored(&f), before);
                assert!(!bound(&f));
            }
        }
    }
}

#[test]
fn the_real_cancel_answer_cannot_end_a_conflicting_new_execution() {
    let mut f = Fixture::new();
    let (decision, reply) = cancel(&mut f);
    let mut doc = f.document();
    doc.floor().unwrap();
    doc.set_stage(Stage::Todo, &Actor::Person, now()).unwrap();
    let mut peer = TaskDoc::from_bytes(&doc.snapshot().unwrap()).unwrap();
    let to = recipient(&f, "m1");
    for fork in [&mut doc, &mut peer] {
        let id = reserve(fork, &to, Origin::Send, true);
        receipt(&f, fork, &id);
    }
    doc.import(&peer.snapshot().unwrap()).unwrap();
    assert!(doc.view().unwrap().execution.unwrap().conflict);
    save(&f, &mut doc);
    observe(&f);
    let observed =
        f.h.store
            .task_observation(TEAM, f.body.node)
            .unwrap()
            .unwrap();
    assert_eq!(
        (
            observed.attempt,
            observed.attempt_id,
            observed.attempt_conflict
        ),
        (2, String::new(), true)
    );
    refused_without_a_write(&mut f, decision, &reply);
    assert_eq!(f.h.store.generation(TEAM, f.body.node).unwrap(), Some(1));
    assert_eq!(f.h.store.pending_cancels(TEAM, 200, 0).unwrap().len(), 1);
    // The conflict freezes the node. Once the person cancels and sends a
    // fresh unambiguous execution, its observation takes the same override.
    person_send(&f, false);
    observe(&f);
    end(&f);
}

fn bound(f: &Fixture) -> bool {
    f.h.store.db.as_ref().unwrap().query_row(
        "SELECT EXISTS(SELECT 1 FROM live_tasks WHERE team=?1 AND node=?2 AND task_project=?3 AND task_uuid=?4)",
        params![TEAM, f.body.node, f.task.project, f.task.uuid], |row| row.get(0),
    ).unwrap()
}

#[test]
fn the_real_released_answer_releases_the_binding_immediately() {
    let mut f = Fixture::new();
    // The first hand-out was never admitted: number one, no current
    // execution, no conflict. The lead's cancel releases the queued task.
    super::conflict::refused_first(&mut f);
    decide(&mut f);
    let decision = f.h.store.pending_cancels(TEAM, 1, 0).unwrap()[0].decision;
    let out = call(&f, "team-cancel");
    assert_eq!(out["reply"]["state"], "released", "{out}");
    assert_eq!(out["reply"]["attempt"], 1);
    assert_eq!(out["reply"]["attempt_id"], "");
    assert!(out["reply"].get("attempt_conflict").is_none());
    assert!(bound(&f), "the store has not yet received the answer");
    record(&mut f, decision, &out["reply"]).unwrap();
    assert!(!bound(&f), "a released answer needs no later observation");
    assert!(f.h.store.pending_cancels(TEAM, 200, 0).unwrap().is_empty());
    assert_eq!(f.document().stage().unwrap(), Stage::Todo);
    assert!(f.document().team().unwrap().is_none());
}

#[test]
fn the_real_cancelled_task_releases_the_binding_only_at_observation() {
    let mut f = Fixture::new();
    let (decision, reply) = cancel(&mut f);
    let task = bytes(&f);
    refused_without_a_write(&mut f, decision, &reply);
    assert!(bound(&f), "a refused answer cannot release the binding");
    observe(&f);
    assert!(!bound(&f), "observing the still cancelled task releases it");
    assert!(f.h.store.pending_cancels(TEAM, 200, 0).unwrap().is_empty());
    assert_eq!(
        f.h.store.node(TEAM, f.body.node).unwrap().unwrap().state,
        NodeState::Cancelled
    );
    assert_eq!(bytes(&f), task, "the observation never writes the task");
}
