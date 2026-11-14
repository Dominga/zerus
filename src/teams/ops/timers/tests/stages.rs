//! Released stages wait in an outbox for one final key record, whatever the
//! team's status; the terminal page may name the same node, and a second
//! record is refused without changing the first.
use super::*;
use crate::teams::ops::CancelState;
use crate::teams::store::timers::KeyedTask;

fn op(value: serde_json::Value) -> Operation {
    serde_json::from_value(value).unwrap()
}
fn pipeline() -> Harness {
    let mut h = Harness::empty();
    let mut create = create();
    if let Operation::Create { bundle, .. } = &mut create {
        let catalog = zerus_team_plan::builtin();
        bundle.roles = catalog.roles;
        bundle.pipelines = catalog.pipelines;
        bundle.template.default_pipeline = "reviewed".into();
    }
    h.run(&person(), create).unwrap();
    for member in ["lead", "m1", "m2", "m3"] {
        h.bind(member);
    }
    h
}
/// A reported task whose review stage is wanted, and that stage.
fn staged(h: &mut Harness, n: u32, member: &str) -> (u32, u32) {
    let parent = h.add(&person(), n);
    h.submit(parent, member);
    h.report(parent, member);
    let stage = h.store.queue(TEAM, 200, None).unwrap().rows.into_iter();
    let stage = stage
        .filter(|s| s.stage_of == Some(parent))
        .map(|s| s.id)
        .next()
        .unwrap();
    (parent, stage)
}
fn creation(h: &mut Harness, stage: u32, outcome: serde_json::Value) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    h.run(
        &service(),
        op(serde_json::json!({"op":"stage_task_intent","node":stage,"effect_request_id":id})),
    )
    .unwrap();
    h.run(
        &service(),
        op(serde_json::json!({"op":"stage_task_outcome","effect_request_id":id,"outcome":outcome})),
    )
    .unwrap();
    id
}
fn cancel(h: &mut Harness, parent: u32) {
    let mut o = h.observation(parent, TaskStage::Todo);
    o.cancelled = true;
    h.run(
        &service(),
        Operation::ObserveTask {
            node: parent,
            observation: o,
        },
    )
    .unwrap();
}
/// The answer to a retired stage's cancel for a task with nothing running:
/// released, which ends the stage and archives it into the frontier.
fn released(h: &mut Harness, stage: u32) {
    let row = h
        .store
        .pending_cancels(TEAM, 200, 0)
        .unwrap()
        .into_iter()
        .find(|r| r.node == stage)
        .unwrap();
    h.run(
        &service(),
        Operation::RecordCancel {
            node: stage,
            decision: row.decision,
            state: CancelState::Released,
            reason: String::new(),
            attempt: row.attempt,
            attempt_id: row.attempt_id,
        },
    )
    .unwrap();
}
fn clear(node: u32, state: KeyClearState, reason: &str) -> Operation {
    Operation::RecordKeyClear {
        node,
        state,
        reason: reason.into(),
    }
}
fn keyed(node: u32, uuid: &str) -> KeyedTask {
    let project = fixture().team.project;
    KeyedTask {
        node,
        task: TaskRef {
            project,
            uuid: uuid.into(),
        },
    }
}
struct Stages {
    h: Harness,
    bound: u32,
    unbound: (u32, String),
    taskless: u32,
    live: (u32, u32),
}
fn stages() -> Stages {
    let mut h = pipeline();
    let (p1, bound) = staged(&mut h, 1, "m1");
    creation(
        &mut h,
        bound,
        serde_json::json!({"status":"created","task":task(1001)}),
    );
    cancel(&mut h, p1);
    released(&mut h, bound);
    let (_, unbound) = staged(&mut h, 2, "m2");
    let id = creation(
        &mut h,
        unbound,
        serde_json::json!({"status":"refused","reason":"The writer refused"}),
    );
    let (p3, taskless) = staged(&mut h, 3, "m1");
    cancel(&mut h, p3);
    let live = staged(&mut h, 4, "m2");
    creation(
        &mut h,
        live.1,
        serde_json::json!({"status":"created","task":task(1004)}),
    );
    Stages {
        h,
        bound,
        unbound: (unbound, id),
        taskless,
        live,
    }
}

#[test]
fn released_stages_name_the_bound_task_or_the_saved_creation_and_nothing_else() {
    let s = stages();
    let all = s.h.store.released_stages(TEAM, 200, 0).unwrap();
    let expected = vec![
        keyed(s.bound, &task(1001).uuid),
        keyed(s.unbound.0, &s.unbound.1),
    ];
    assert_eq!(all, expected);
    assert!(all
        .iter()
        .all(|k| k.node != s.taskless && k.node != s.live.1));
}

#[test]
fn released_stages_page_in_node_order() {
    let s = stages();
    let first = s.h.store.released_stages(TEAM, 1, 0).unwrap();
    assert_eq!(first.iter().map(|k| k.node).collect::<Vec<_>>(), [s.bound]);
    let second = s.h.store.released_stages(TEAM, 1, s.bound).unwrap();
    assert_eq!(
        second.iter().map(|k| k.node).collect::<Vec<_>>(),
        [s.unbound.0]
    );
    assert!(s
        .h
        .store
        .released_stages(TEAM, 1, s.unbound.0)
        .unwrap()
        .is_empty());
    refused(s.h.store.released_stages(TEAM, 201, 0), code::OVER_CAP);
}

#[test]
fn a_released_stage_is_recorded_once_in_a_running_team_and_a_live_one_never() {
    let mut s = stages();
    refused(
        s.h.run(&person(), clear(s.bound, KeyClearState::Cleared, "")),
        code::NOT_ALLOWED,
    );
    s.h.run(&service(), clear(s.bound, KeyClearState::Cleared, ""))
        .unwrap();
    let left: Vec<u32> =
        s.h.store
            .released_stages(TEAM, 200, 0)
            .unwrap()
            .iter()
            .map(|k| k.node)
            .collect();
    assert_eq!(left, [s.unbound.0]);
    refused(
        s.h.run(&service(), clear(s.bound, KeyClearState::Refused, "Again")),
        code::INVALID_STATE,
    );
    for node in [s.live.1, s.live.0, s.taskless] {
        refused(
            s.h.run(&service(), clear(node, KeyClearState::Cleared, "")),
            code::INVALID_STATE,
        );
    }
}

#[test]
fn both_reads_may_name_a_stopped_teams_released_stage_and_the_second_record_changes_nothing() {
    let mut s = stages();
    s.h.run(&person(), Operation::Stop).unwrap();
    let terminal: Vec<u32> =
        s.h.store
            .keyed_tasks(TEAM, 0, 200)
            .unwrap()
            .iter()
            .map(|k| k.node)
            .collect();
    assert!(terminal.contains(&s.unbound.0) && !terminal.contains(&s.taskless));
    assert!(s
        .h
        .store
        .keyed_tasks(TEAM, 0, 200)
        .unwrap()
        .contains(&keyed(s.unbound.0, &s.unbound.1)));
    let refusal = clear(s.unbound.0, KeyClearState::Refused, "The task moved on");
    s.h.run(&service(), refusal).unwrap();
    let rev = s.h.store.team(TEAM).unwrap().unwrap().revision;
    refused(
        s.h.run(&service(), clear(s.unbound.0, KeyClearState::Cleared, "")),
        code::INVALID_STATE,
    );
    assert_eq!(s.h.store.team(TEAM).unwrap().unwrap().revision, rev);
    assert_eq!(
        s.h.store.key_refusals(TEAM, 0, 200).unwrap(),
        vec![(s.unbound.0, "The task moved on".to_owned())]
    );
    let stages: Vec<u32> =
        s.h.store
            .released_stages(TEAM, 200, 0)
            .unwrap()
            .iter()
            .map(|k| k.node)
            .collect();
    assert_eq!(stages, [s.bound]);
}

#[test]
fn the_frontier_is_filled_from_an_older_store() {
    let mut s = stages();
    let db = s.h.store.db.as_ref().unwrap();
    db.execute_batch("DROP TRIGGER key_clear_stage_archived_v2; DROP TRIGGER key_clear_stage_settled_v2; DROP TABLE key_clear_work; DROP TABLE timers_extension;").unwrap();
    s.h.store = Store::open(s.h.temp.path()).unwrap();
    assert_eq!(s.h.store.released_stages(TEAM, 200, 0).unwrap().len(), 2);
}

#[test]
fn a_stage_archived_before_its_creation_settles_enters_the_outbox_when_it_does() {
    let s = stages();
    let db = s.h.store.db.as_ref().unwrap();
    let archive = format!(
        "INSERT INTO node_archive({0}) SELECT {0} FROM nodes WHERE team=?1 AND id=?2",
        crate::teams::store::rows::NODE_COLUMNS
    );
    db.execute(&archive, rusqlite::params![TEAM, s.live.1])
        .unwrap();
    db.execute(
        "DELETE FROM nodes WHERE team=?1 AND id=?2",
        rusqlite::params![TEAM, s.live.1],
    )
    .unwrap();
    let nodes = |h: &Harness| -> Vec<u32> {
        h.store
            .released_stages(TEAM, 200, 0)
            .unwrap()
            .iter()
            .map(|k| k.node)
            .collect()
    };
    assert!(
        !nodes(&s.h).contains(&s.live.1),
        "its creation is still live"
    );
    db.execute(
        "UPDATE stage_tasks SET state='finished' WHERE team=?1 AND node=?2",
        rusqlite::params![TEAM, s.live.1],
    )
    .unwrap();
    assert!(nodes(&s.h).contains(&s.live.1));
    assert!(s
        .h
        .store
        .released_stages(TEAM, 200, 0)
        .unwrap()
        .contains(&keyed(s.live.1, &task(1004).uuid)));
}
