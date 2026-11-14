use super::*;
#[test]
fn a_creation_whose_effect_disappears_before_its_child_settles_without_clearing() {
    let (mut h, target, stage) = wanted();
    let machine = h.store.team(TEAM).unwrap().unwrap().home;
    let mut a = super::super::super::Runtime::open(h.temp.path(), &machine).unwrap();
    let mut p = Fake::default();
    super::super::super::tasks::CRASH.with(|s| s.set(Some("reserved")));
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| tick(
            &mut h, &mut a, &mut p, 100
        )))
        .is_err()
    );
    assert!(p.calls.is_empty());
    h.run(
        &person(),
        crate::teams::ops::Operation::Return {
            node: target,
            reason: "Retire the stage before its child".into(),
        },
    )
    .unwrap();
    drop(a);
    let mut a = super::super::super::Runtime::open(h.temp.path(), &machine).unwrap();
    let team = h.store.team(TEAM).unwrap().unwrap();
    let key = format!("task_command:stage:{stage}");
    let mut budget = 4;
    a.advance_command(&mut h.store, &mut p, &team, &key, 101, &mut budget)
        .unwrap();
    assert_eq!(budget, 4);
    assert!(p.calls.is_empty() && p.clears.is_empty());
    assert!(a.ledger.get(TEAM, &key).unwrap().is_none());
    assert_eq!(h.store.released_stages(TEAM, 16, 0).unwrap().len(), 1);
}
