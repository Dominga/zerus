//! Real fence refusal and native input through the private CLI fixture.
use super::*;

pub(crate) fn fence_delivery_cycle(read: impl Fn(&Value, &str) -> ops::HandoutOutcome) -> Value {
    let mut f = Fixture::new();
    let old = f.body.id.clone();
    // The incident concerns an execution-floored task. Create that floor with
    // the product writer before closing its ordinary attempt-admission fence.
    let mut doc = f.document();
    doc.floor().unwrap();
    save(&f, &mut doc);
    let hold = |on| {
        let tasks = crate::tasks::store(&f.config, &f.task.project).unwrap();
        let mut project = tasks.project().unwrap();
        project.set_cutover_hold(on, &Actor::Person, now()).unwrap();
        tasks.save_project(&project).unwrap();
    };
    hold(true);
    let refused = f.run("submitted", false);
    assert_eq!(refused["reply"]["state"], "cutting_over", "{refused}");
    assert_eq!(f.count(), 0, "the fence never offered input");
    let outcome = read(&refused["reply"], &old);
    assert!(matches!(outcome, ops::HandoutOutcome::CuttingOver { .. }));
    f.h.outcome(&old, outcome);
    assert!(f
        .h
        .store
        .open_effects(TEAM, 200, None)
        .unwrap()
        .rows
        .is_empty());
    hold(false);
    let new = f.h.intent(f.body.node, "m1");
    f.body.id = new.clone();
    let delivered = f.run("submitted", false);
    assert_eq!(delivered["reply"]["state"], "submitted", "{delivered}");
    f.h.outcome(&new, read(&delivered["reply"], &new));
    assert_eq!(f.count(), 1);
    f.body.id = old.clone();
    let replay = f.run("submitted", false);
    assert_eq!(replay["reply"]["state"], "refused", "{replay}");
    assert_eq!(f.count(), 1, "the old effect cannot offer input again");
    json!({"old":old,"new":new,"sent":f.count()})
}
