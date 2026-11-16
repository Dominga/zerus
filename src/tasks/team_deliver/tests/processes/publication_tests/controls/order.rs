//! Actual published tasks establish both serialized writer orders. The
//! rendezvous covers a hand-out winning after fresh project custody.
use super::*;

#[test]
fn handout_intent_winning_after_move_custody_makes_that_exact_move_stale() {
    use std::time::Duration;
    let (mut f, _, tasks) = published();
    for r in &tasks {
        crate::teams::tasks::observe(&f.config.state, &r.task).unwrap();
    }
    another_published(&mut f);
    let home = f.lead.machine.clone();
    f.bind("m2", &home);
    let request = ops::tests::request(
        f.h.store.team(TEAM).unwrap().unwrap().revision,
        Operation::MovePhase {
            scope: scope(&f, 1),
            relative_to: scope(&f, 2),
            placement: PhasePlacement::After,
        },
    );
    let before = facts::phase(f.h.store.db.as_ref().unwrap(), TEAM, 1).unwrap();
    let mut writer = crate::teams::store::Store::open(&f.config.state).unwrap();
    let (arrived, acquired) = std::sync::mpsc::channel();
    let (release, resumed) = std::sync::mpsc::channel();
    std::thread::scope(|s| {
        let request = &request;
        let join = s.spawn(move || {
            facts::task_custody::fixture::once(move || {
                arrived.send(()).unwrap();
                resumed.recv_timeout(Duration::from_secs(5)).unwrap();
            });
            ops::tests::apply_fixture(&mut writer, &person(), request, now())
        });
        acquired.recv_timeout(Duration::from_secs(5)).unwrap();
        f.h.intent(tasks[0].node.id, "m2");
        release.send(()).unwrap();
        ops::tests::refused(
            join.join().unwrap(),
            zerus_team_plan::problem::code::STALE_REVISION,
        );
    });
    assert_eq!(
        facts::phase(f.h.store.db.as_ref().unwrap(), TEAM, 1).unwrap(),
        before
    );
    let team = f.h.store.team(TEAM).unwrap().unwrap();
    let saved = ops::tests::apply_fixture(&mut f.h.store, &person(), &request, now()).unwrap_err();
    assert_eq!(saved.code, zerus_team_plan::problem::code::STALE_REVISION);
    assert_eq!(
        f.h.store.team(TEAM).unwrap().unwrap().revision,
        team.revision
    );
}

#[test]
fn a_winning_move_rechecks_the_gate_order_before_any_new_handout_intent() {
    let (mut f, _, tasks) = published();
    for r in &tasks {
        crate::teams::tasks::observe(&f.config.state, &r.task).unwrap();
    }
    another_published(&mut f);
    let home = f.lead.machine.clone();
    f.bind("m2", &home);
    let current = scope(&f, 1);
    f.h.run(
        &person(),
        Operation::SetPhaseBarrier {
            scope: current,
            barrier: true,
        },
    )
    .unwrap();
    let old_intent = ops::tests::request(
        f.h.store.team(TEAM).unwrap().unwrap().revision,
        Operation::HandoutIntent {
            node: tasks[0].node.id,
            member: "m2".into(),
            effect_request_id: uuid::Uuid::new_v4().to_string(),
        },
    );
    let (current, relative_to) = (scope(&f, 1), scope(&f, 2));
    f.h.run(
        &person(),
        Operation::MovePhase {
            scope: current,
            relative_to,
            placement: PhasePlacement::After,
        },
    )
    .unwrap();
    assert_eq!(
        facts::phase(f.h.store.db.as_ref().unwrap(), TEAM, 1)
            .unwrap()
            .order,
        2
    );
    ops::tests::refused(
        ops::tests::apply_fixture(&mut f.h.store, &ops::tests::service(), &old_intent, now()),
        zerus_team_plan::problem::code::STALE_REVISION,
    );
    ops::tests::refused(
        f.h.run(&ops::tests::service(), old_intent.operation),
        zerus_team_plan::problem::code::INVALID_STATE,
    );
    let effects: u32 =
        f.h.store
            .db
            .as_ref()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM effects WHERE team=?1 AND node=?2",
                rusqlite::params![TEAM, tasks[0].node.id],
                |r| r.get(0),
            )
            .unwrap();
    assert_eq!(effects, 0);
}
