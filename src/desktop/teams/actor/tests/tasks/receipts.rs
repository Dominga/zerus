//! A lost success reply replays the exact receipt on both command paths.
use super::super::super::tasks::settle::AFTER_APPLY;
use super::*;
use std::{cell::RefCell, rc::Rc};
struct Reset;
impl Drop for Reset {
    fn drop(&mut self) {
        AFTER_APPLY.with(|s| s.borrow_mut().take());
    }
}
fn lost(ids: Rc<RefCell<Vec<String>>>) {
    AFTER_APPLY.with(|s| {
        *s.borrow_mut() = Some(Box::new(move |request, result| {
            assert!(result.is_ok());
            ids.borrow_mut().push(request.request_id.clone());
            Err(zerus_team_plan::Problem::new(
                zerus_team_plan::problem::code::STORAGE_FAILED,
                "fixture lost a committed task fact reply",
            ))
        }))
    });
}
fn replay(ids: Rc<RefCell<Vec<String>>>) {
    AFTER_APPLY.with(|s| {
        *s.borrow_mut() = Some(Box::new(move |request, result| {
            assert_eq!(
                request.request_id,
                ids.borrow()[0],
                "the receipt changed after restart"
            );
            assert!(result.is_ok(), "{result:?}");
            ids.borrow_mut().push(request.request_id.clone());
            result
        }))
    });
}
fn check(creation: bool) {
    let _reset = Reset;
    let (mut h, target, stage) = wanted();
    let machine = h.store.team(TEAM).unwrap().unwrap().home;
    let mut a = super::super::super::Runtime::open(h.temp.path(), &machine).unwrap();
    let mut p = Fake::default();
    if !creation {
        tick(&mut h, &mut a, &mut p, 100);
        h.run(
            &person(),
            crate::teams::ops::Operation::Return {
                node: target,
                reason: "Revise".into(),
            },
        )
        .unwrap();
        h.release_retired(stage);
    }
    let key = format!(
        "task_command:{}:{stage}",
        if creation { "stage" } else { "clear" }
    );
    let ids = Rc::new(RefCell::new(Vec::new()));
    lost(ids.clone());
    tick(&mut h, &mut a, &mut p, 101);
    assert_eq!(ids.borrow().len(), 1);
    assert!(a.ledger.get(TEAM, &key).unwrap().is_some());
    drop(a);
    let mut a = super::super::super::Runtime::open(h.temp.path(), &machine).unwrap();
    replay(ids.clone());
    tick(&mut h, &mut a, &mut p, 60_101);
    assert_eq!(ids.borrow().len(), 2);
    assert!(a.ledger.get(TEAM, &key).unwrap().is_none());
    assert_eq!(p.calls.len(), 1);
    assert_eq!(p.clears.len(), usize::from(!creation));
}
#[test]
fn a_lost_stage_fact_reply_replays_its_original_receipt_after_restart() {
    check(true);
}
#[test]
fn a_lost_clear_fact_reply_replays_its_original_receipt_after_restart() {
    check(false);
}
