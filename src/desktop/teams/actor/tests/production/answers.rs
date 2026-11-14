//! The real answer adapter uses the shared timer engine and committed fleet.
use super::*;
use ports::Ports;
use std::os::unix::fs::PermissionsExt;

fn answered() -> Harness {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    let member = h.bind("m1");
    let node = h.add(&person(), 1);
    h.submit(node, "m1");
    let item = h
        .run(
            &member,
            Operation::Ask {
                node,
                generation: 1,
                text: "Which width?".into(),
            },
        )
        .unwrap()
        .item
        .unwrap();
    h.run(
        &lead,
        Operation::Reply {
            item,
            text: "Keep the width.".into(),
        },
    )
    .unwrap();
    h
}

#[test]
fn native_answer_entry_reserves_for_the_current_member_and_never_repeats_unknown() {
    let mut h = answered();
    let team = h.store.team(TEAM).unwrap().unwrap();
    let mut n = native(&h);
    let program = h.temp.path().join("receiptless-send");
    std::fs::write(&program, "#!/bin/sh\nexec /bin/cat\n").unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut state = auxiliary::State::new(h.temp.path(), "first-worker");
    // cat only drains the private input and returns no receipt. No agent runs.
    crate::desktop::teams::timers::Native::fixture_program(&program, || {
        n.answer_timers(&mut state, &mut h.store, &team, 100)
            .unwrap();
    });
    let db =
        rusqlite::Connection::open(h.temp.path().join("teams/member-launch-v1.sqlite3")).unwrap();
    let row: (String, String, String) = db
        .query_row("SELECT member,run,state FROM timer_answer_bells", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap();
    assert_eq!(row, ("m1".into(), "fixture-m1".into(), "uncertain".into()));
    drop(state);
    let mut state = auxiliary::State::new(h.temp.path(), "next-worker");
    crate::desktop::teams::timers::Native::fixture_program(&program, || {
        n.answer_timers(&mut state, &mut h.store, &team, 1_000_100)
            .unwrap();
    });
    let count: u32 = db
        .query_row("SELECT COUNT(*) FROM timer_answer_bells", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        count, 1,
        "an uncertain doorbell cannot be sent by the new worker"
    );
}
