//! Actual operation and retry ledgers; native task effects remain unused
//! until approval has saved a publication.
use super::*;
use crate::teams::ops::{self, Operation};

#[test]
fn an_auto_approval_refusal_uses_the_retry_gate_and_rechecks_changed_settings() {
    let mut h = Harness::new();
    h.bind("lead");
    let mut settings = h.store.team(TEAM).unwrap().unwrap().settings;
    settings.plan_approval = Approval::Auto;
    h.run(
        &person(),
        Operation::ChangeSettings {
            settings: settings.clone(),
        },
    )
    .unwrap();
    h.run(
        &person(),
        Operation::SubmitDraftPhase {
            phase: 1,
            draft: ops::publication::tests::input(&[1, 2]),
        },
    )
    .unwrap();
    settings.caps.max_phase_tasks = 1;
    h.run(
        &person(),
        Operation::ChangeSettings {
            settings: settings.clone(),
        },
    )
    .unwrap();
    let mut runtime =
        super::super::Runtime::open(h.temp.path(), &h.store.team(TEAM).unwrap().unwrap().home)
            .unwrap();
    let mut ports = super::core::Fake::new();
    for now in 0..50 {
        let team = h.store.team(TEAM).unwrap().unwrap();
        let _ = runtime.publications(&mut h.store, &mut ports, &team, now);
    }
    let attempts:u32 = h.store.db.as_ref().unwrap().query_row("SELECT count(*) FROM requests WHERE team=?1 AND json_extract(content,'$.op')='approve_phase'",[TEAM],|r|r.get(0)).unwrap();
    assert_eq!(
        attempts, 1,
        "an unchanged invalid draft must not append a refusal on every pass"
    );
    assert!(h.store.phase_draft(TEAM, 1).unwrap().publication.is_none());
    settings.caps.max_phase_tasks = 2;
    h.run(&person(), Operation::ChangeSettings { settings })
        .unwrap();
    let team = h.store.team(TEAM).unwrap().unwrap();
    runtime
        .publications(&mut h.store, &mut ports, &team, 100)
        .unwrap();
    assert!(h.store.phase_draft(TEAM, 1).unwrap().publication.is_some());
    assert!(h.store.node(TEAM, 1).unwrap().is_none());
}
