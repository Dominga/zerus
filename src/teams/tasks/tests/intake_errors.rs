//! Real store errors, documents and observation, without session effects.
use super::*;
use crate::teams::tasks::intake::Intake;
use std::cell::{Cell, RefCell};

thread_local! {
    static BEFORE: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
    static LOST_REPLY: Cell<bool> = const { Cell::new(false) };
}

pub(in crate::teams::tasks) fn before_apply() {
    let hook = BEFORE.with(|hook| hook.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}
pub(in crate::teams::tasks) fn after_apply(result: Result<ops::Reply>) -> Result<ops::Reply> {
    if LOST_REPLY.with(|lost| lost.replace(false)) && result.is_ok() {
        return Err(Problem::new(
            code::STORAGE_FAILED,
            "The private fixture lost the committed answer.",
        ));
    }
    result
}

fn send_task(h: &Harness, reference: &TaskRef) -> Intake {
    send(&caller(h, &TaskActor::Person), reference, TEAM, None, None).unwrap()
}
fn one_node(h: &Harness, reference: &TaskRef, node: u32) {
    let doc = task(h, reference);
    assert_eq!(doc.team().unwrap().unwrap().team, TEAM);
    observe(h.temp.path(), reference).unwrap();
    assert_eq!(
        binding(&h.store, reference).unwrap(),
        Some((TEAM.into(), node))
    );
    let rows = h.store.queue(TEAM, 200, None).unwrap().rows;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, node);
    let identities: u32 = h
        .store
        .db
        .as_ref()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM node_identity WHERE team=?1 AND task_project=?2 AND task_uuid=?3",
            params![TEAM, reference.project, reference.uuid],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        identities, 1,
        "an error and observation must never produce a second identity"
    );
}
fn receipt_failure(h: &Harness) {
    h.store.db.as_ref().unwrap().execute_batch(
        "CREATE TRIGGER intake_receipt_failure BEFORE INSERT ON requests BEGIN SELECT RAISE(ABORT,'private receipt failure'); END;"
    ).unwrap();
}
fn remove_receipt_failure(h: &Harness) {
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute_batch("DROP TRIGGER intake_receipt_failure")
        .unwrap();
}
fn rule_refusal_after_prepare(h: &Harness) {
    let state = h.temp.path().to_owned();
    BEFORE.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move || {
            let mut store = Store::open(&state).unwrap();
            let team = store.team(TEAM).unwrap().unwrap();
            let mut settings = team.settings;
            settings.caps.max_open_tasks -= 1;
            ops::apply(
                &mut store,
                &person(),
                &ops::Request {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    team: TEAM.into(),
                    expected_revision: team.revision,
                    operation: ops::Operation::ChangeSettings { settings },
                },
                1234,
            )
            .unwrap();
        }))
    });
}
fn targeted(h: &Harness) -> (TaskRef, Target) {
    let reference = document(h);
    let target = Target::Session(person_session());
    edit(h, &reference, |doc| {
        doc.set_target(&target, &TaskActor::Person, 2).unwrap()
    });
    (reference, target)
}

#[test]
fn a_failed_receipt_repeat_keeps_the_key_and_observer_finds_one_node() {
    let h = Harness::new();
    let reference = document(&h);
    let first = send_task(&h, &reference).node.unwrap();
    let before = task(&h, &reference).revision();
    receipt_failure(&h);
    let result = send_task(&h, &reference);
    remove_receipt_failure(&h);
    assert_eq!(result.refusal.unwrap().code, code::STORAGE_FAILED);
    assert_eq!(task(&h, &reference).revision(), before);
    one_node(&h, &reference, first);
    assert_eq!(send_task(&h, &reference).node, Some(first));
    one_node(&h, &reference, first);
}

#[test]
fn a_busy_repeat_keeps_the_key_and_observer_finds_one_node() {
    let h = Harness::new();
    let reference = document(&h);
    let first = send_task(&h, &reference).node.unwrap();
    let before = task(&h, &reference).revision();
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute_batch("BEGIN IMMEDIATE")
        .unwrap();
    let result = send_task(&h, &reference);
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute_batch("ROLLBACK")
        .unwrap();
    assert_eq!(result.refusal.unwrap().code, code::STORE_BUSY);
    assert_eq!(task(&h, &reference).revision(), before);
    one_node(&h, &reference, first);
    assert_eq!(send_task(&h, &reference).node, Some(first));
    one_node(&h, &reference, first);
}

#[test]
fn a_rule_refused_repeat_keeps_the_key_it_did_not_set() {
    let h = Harness::new();
    let reference = document(&h);
    let first = send_task(&h, &reference).node.unwrap();
    rule_refusal_after_prepare(&h);
    assert_eq!(
        send_task(&h, &reference).refusal.unwrap().code,
        code::STALE_REVISION
    );
    one_node(&h, &reference, first);
    assert_eq!(send_task(&h, &reference).node, Some(first));
}

#[test]
fn a_first_rule_refusal_restores_the_absent_key_and_original_target() {
    let h = Harness::new();
    let (reference, target) = targeted(&h);
    rule_refusal_after_prepare(&h);
    assert_eq!(
        send_task(&h, &reference).refusal.unwrap().code,
        code::STALE_REVISION
    );
    let doc = task(&h, &reference);
    assert_eq!(doc.team().unwrap(), None);
    assert_eq!(doc.view().unwrap().target, target);
    observe(h.temp.path(), &reference).unwrap();
    assert!(h.store.queue(TEAM, 200, None).unwrap().rows.is_empty());
}

#[test]
fn a_first_busy_send_restores_key_and_target_then_queues_once() {
    let h = Harness::new();
    let (reference, target) = targeted(&h);
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute_batch("BEGIN IMMEDIATE")
        .unwrap();
    let result = send_task(&h, &reference);
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute_batch("ROLLBACK")
        .unwrap();
    assert_eq!(result.refusal.unwrap().code, code::STORE_BUSY);
    assert_eq!(task(&h, &reference).team().unwrap(), None);
    assert_eq!(task(&h, &reference).view().unwrap().target, target);
    observe(h.temp.path(), &reference).unwrap();
    let node = send_task(&h, &reference).node.unwrap();
    one_node(&h, &reference, node);
    assert_eq!(send_task(&h, &reference).node, Some(node));
}

#[test]
fn a_first_unknown_result_keeps_the_prepared_key_and_target_then_queues_once() {
    let h = Harness::new();
    let (reference, _) = targeted(&h);
    receipt_failure(&h);
    let result = send_task(&h, &reference);
    remove_receipt_failure(&h);
    assert_eq!(result.refusal.unwrap().code, code::STORAGE_FAILED);
    assert_eq!(task(&h, &reference).team().unwrap().unwrap().team, TEAM);
    assert_eq!(task(&h, &reference).view().unwrap().target, Target::Any);
    observe(h.temp.path(), &reference).unwrap();
    let node = send_task(&h, &reference).node.unwrap();
    one_node(&h, &reference, node);
    assert_eq!(send_task(&h, &reference).node, Some(node));
}

#[test]
fn a_lost_committed_answer_is_recovered_by_task_identity() {
    let h = Harness::new();
    let reference = document(&h);
    LOST_REPLY.with(|lost| lost.set(true));
    let result = send_task(&h, &reference);
    assert_eq!(result.refusal.unwrap().code, code::STORAGE_FAILED);
    let rows = h.store.queue(TEAM, 200, None).unwrap().rows;
    assert_eq!(rows.len(), 1);
    one_node(&h, &reference, rows[0].id);
    assert_eq!(send_task(&h, &reference).node, Some(rows[0].id));
    one_node(&h, &reference, rows[0].id);
}

#[test]
fn a_rule_refusal_restores_a_previous_teams_key_and_its_name() {
    let mut h = Harness::new();
    let other = ops::tests::TEAM_B;
    let mut create = ops::tests::create();
    if let ops::Operation::Create {
        name,
        integration_branch,
        ..
    } = &mut create
    {
        *name = "Previous team".into();
        *integration_branch = "zerus/team/previous".into();
    }
    h.run_for(other, &person(), create).unwrap();
    let (reference, target) = targeted(&h);
    edit(&h, &reference, |doc| {
        doc.set_team_named(
            Some(&zerus_replica::TaskTeam {
                team: other.into(),
                home: fixture().team.home,
            }),
            "Previous team",
            &TaskActor::Person,
            3,
        )
        .unwrap()
    });
    rule_refusal_after_prepare(&h);
    assert_eq!(
        send_task(&h, &reference).refusal.unwrap().code,
        code::STALE_REVISION
    );
    let doc = task(&h, &reference);
    assert_eq!(doc.team().unwrap().unwrap().team, other);
    let view = doc.view().unwrap();
    assert_eq!(view.target, target);
    assert!(view
        .log
        .iter()
        .any(|entry| entry.text == "Sent to team Previous team"));
    assert_eq!(
        view.log
            .iter()
            .rev()
            .find(|entry| entry.kind == "team")
            .unwrap()
            .text,
        "Sent to team Previous team"
    );
    assert!(h.store.queue(TEAM, 200, None).unwrap().rows.is_empty());
}
