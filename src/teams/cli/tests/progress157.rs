//! Count controls driven by the same operations as the live queue.
use crate::teams::ops::tests::{self, person, TEAM};
use crate::teams::ops::{Operation, TaskStage};
use crate::teams::store::{Store, TeamProgress};

fn progress(store: &Store) -> TeamProgress {
    let rows = store.teams(1, None).unwrap().rows;
    store.team_progress(&rows).unwrap().remove(TEAM).unwrap()
}

fn queue() -> tests::Harness {
    let mut h = tests::Harness::empty();
    let mut create = tests::create();
    if let Operation::Create { bundle, .. } = &mut create {
        bundle.pipelines[0].stages.clear();
        bundle.template.settings.integration = zerus_team_plan::model::Integration::None;
    }
    h.run(&person(), create).unwrap();
    h
}

#[test]
fn cancelled_tasks_are_neither_accepted_nor_in_the_total() {
    let mut h = queue();
    h.bind("lead");
    let accepted = h.add(&person(), 1);
    h.observe(accepted, TaskStage::Done);
    let cancelled = h.add(&person(), 2);
    h.cancel_fact(cancelled);
    h.add(&person(), 3);
    assert_eq!(
        h.store.node(TEAM, cancelled).unwrap().unwrap().state,
        zerus_team_plan::model::NodeState::Cancelled
    );
    let p = progress(&h.store);
    assert_eq!((p.accepted, p.tasks), (1, 2));
    assert!(!p.tasks_more);
}

#[test]
fn the_lead_counts_before_launch_and_paused_members_still_count() {
    let mut h = queue();
    assert_eq!(progress(&h.store).members, 1, "the unlaunched lead counts");
    h.bind("lead");
    assert_eq!(progress(&h.store).members, 1, "the bound lead counts once");
    h.bind("m1");
    h.bind("m2");
    assert_eq!(progress(&h.store).members, 3);
    h.run(
        &person(),
        Operation::PauseMember {
            member: "m1".into(),
        },
    )
    .unwrap();
    assert_eq!(progress(&h.store).members, 3, "a paused member stays live");
    h.run(
        &person(),
        Operation::PauseMember {
            member: "lead".into(),
        },
    )
    .unwrap();
    assert_eq!(progress(&h.store).members, 3, "a paused lead counts once");
}

#[test]
fn answering_a_member_question_removes_it_from_the_count() {
    let mut h = queue();
    let lead = h.bind("lead");
    let member = h.bind("m1");
    let node = h.add(&person(), 1);
    h.submit(node, "m1");
    h.observe(node, TaskStage::InProgress);
    let asked = h
        .run(
            &member,
            Operation::Ask {
                node,
                generation: 1,
                text: "Which case?".into(),
            },
        )
        .unwrap();
    assert_eq!(progress(&h.store).questions, 1);
    h.run(
        &lead,
        Operation::Reply {
            item: asked.item.unwrap(),
            text: "The first.".into(),
        },
    )
    .unwrap();
    assert_eq!(progress(&h.store).questions, 0);
}

#[test]
fn cancelled_archive_growth_does_not_grow_the_count_read() {
    let mut h = queue();
    let measure = |store: &Store| {
        use std::sync::{
            atomic::{AtomicUsize, Ordering::Relaxed},
            Arc,
        };
        let rows = store.teams(1, None).unwrap().rows;
        let db = store.db.as_ref().unwrap();
        let steps = Arc::new(AtomicUsize::new(0));
        let counted = steps.clone();
        db.progress_handler(
            1,
            Some(move || {
                counted.fetch_add(1, Relaxed);
                false
            }),
        );
        let p = store.team_progress(&rows).unwrap().remove(TEAM).unwrap();
        db.progress_handler(0, None::<fn() -> bool>);
        assert_eq!((p.accepted, p.tasks), (0, 0));
        steps.load(Relaxed)
    };
    for n in 1..=100 {
        let node = h.add(&person(), n);
        h.cancel_fact(node);
    }
    let before = measure(&h.store);
    for n in 101..=200 {
        let node = h.add(&person(), n);
        h.cancel_fact(node);
    }
    let after = measure(&h.store);
    assert!(
        after <= before + 20,
        "cancelled archive scan grew: {before} -> {after}"
    );
}
