//! A refusal is proved only by the request's saved receipt. A storage
//! failure carries the same code and is retried by a later pass.
use super::*;
use crate::teams::ops::{Operation, TimerKind};

/// Hold the store's write lock from another connection, as a busy or
/// failing writer would; readers still see the store.
fn locked(w: &World) -> rusqlite::Connection {
    let db = rusqlite::Connection::open(w.h.temp.path().join("teams/teams-v1.sqlite3")).unwrap();
    db.execute_batch("BEGIN IMMEDIATE;").unwrap();
    db
}
fn team(w: &World) -> Team {
    w.h.store.team(TEAM).unwrap().unwrap()
}

#[test]
fn only_a_saved_refusal_is_a_refusal() {
    let mut w = World::new();
    let (node, _) = w.reported();
    let early = Operation::FireTimer {
        timer: TimerKind::RemindLead,
        node,
        generation: 1,
        item: 1,
        since_ms: 0,
    };
    let t = team(&w);
    assert_eq!(
        super::super::record(&mut w.h.store, &t, early.clone(), T0).unwrap(),
        Recorded::Refused
    );
    let due = |item| Operation::FireTimer {
        timer: TimerKind::RemindLead,
        node,
        generation: 1,
        item,
        since_ms: 0,
    };
    let item = w.items("report")[0].seq;
    let lock = locked(&w);
    let t = team(&w);
    assert_eq!(
        super::super::record(&mut w.h.store, &t, due(item), T0 + 20 * MINUTE).unwrap(),
        Recorded::Retry
    );
    drop(lock);
    let t = team(&w);
    assert_eq!(
        super::super::record(&mut w.h.store, &t, due(item), T0 + 20 * MINUTE).unwrap(),
        Recorded::Done
    );
    assert_eq!(w.items("report_reminder").len(), 1);
}

#[test]
fn a_storage_failure_at_the_write_is_retried_then_recorded_once() {
    let mut w = World::new();
    w.h.run(
        &person(),
        Operation::ArchiveMember {
            member: "m2".into(),
        },
    )
    .unwrap();
    super::late::row(
        &mut w,
        "m2",
        crate::desktop::teams::ledger::State::Submitted,
        T0 + 60 * MINUTE,
    );
    let session = super::late::session(&w);
    w.effects
        .launches
        .insert(super::late::LAUNCH.into(), session);
    let lock = locked(&w);
    w.terminal(T0);
    drop(lock);
    assert!(w.cards("late_session").is_empty());
    assert_eq!(
        super::late::launch_state(&mut w, "m2").0,
        crate::desktop::teams::ledger::State::Submitted
    );
    w.terminal(T0 + MINUTE);
    w.terminal(T0 + 2 * MINUTE);
    assert_eq!(w.cards("late_session").len(), 1);
    assert_eq!(
        super::late::launch_state(&mut w, "m2").0,
        crate::desktop::teams::ledger::State::Done
    );
    assert_eq!(
        w.effects.reads, 2,
        "read again only after the unproved write"
    );
}

#[test]
fn a_proved_refusal_is_not_retried() {
    let mut w = World::new();
    super::late::row(
        &mut w,
        "m1",
        crate::desktop::teams::ledger::State::Submitted,
        T0 + 60 * MINUTE,
    );
    let session = super::late::session(&w);
    w.effects
        .launches
        .insert(super::late::LAUNCH.into(), session);
    w.terminal(T0);
    w.terminal(T0 + MINUTE);
    assert!(
        w.cards("late_session").is_empty(),
        "a live member binds normally"
    );
    assert_eq!(
        super::late::launch_state(&mut w, "m1").0,
        crate::desktop::teams::ledger::State::Done
    );
    assert_eq!(w.effects.reads, 1);
}
