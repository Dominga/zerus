//! zerus-63rru.85 through the production Native and the real pass: a
//! hand-out reserved for a member that left the active set (paused,
//! stopped, unbound) is refused by the spawn admission on each replay, and
//! after five non-starts (about 26 minutes) the effect is refused and the
//! next pass hands the task to another member, with nobody told. The fix
//! settles the reservation at the next pass like a removal does, with one
//! item per task; whether the person paused the member is not stored, so
//! the lead is told in every case.
use super::real_rows::Production;
use super::rows::seed_row;
use crate::teams::ops::tests::*;
use crate::teams::ops::{Actor, Operation};
use serde_json::{json, Value};

const MINUTE: i64 = 60_000;
const T0: i64 = 10_000_000;

/// The member's committed fleet row, as its producer writes it.
fn row(p: &Production, h: &Harness, member: &str, changes: &[(&str, Value)]) {
    let m = h.store.member(TEAM, member).unwrap().unwrap();
    let launch = m.session.as_ref().unwrap().session_id["launch_id:".len()..].to_owned();
    let run = format!("fixture-{member}");
    let mut all = vec![
        ("name", json!(format!("{}/fixture/{member}", m.agent))),
        ("run_id", json!(run)),
        ("launch_id", json!(launch)),
        ("conversation_id", json!(format!("conversation-{member}"))),
    ];
    all.extend(changes.iter().cloned());
    let row = seed_row(&m.agent, &all).to_string();
    rusqlite::Connection::open(p.native.config.state.join("desktop/state.sqlite3"))
        .unwrap()
        .execute(
            "INSERT OR REPLACE INTO desktop_catalog_rows(generation,key,json,bytes) VALUES(1,?1,?2,?3)",
            rusqlite::params![format!("run:{run}"), row, row.len()],
        )
        .unwrap();
}

/// A running team whose task 1 can go to m1 only (m2 is busy), the lead
/// bound; the first pass has not run.
fn team() -> (Harness, super::super::Runtime, Production, Actor) {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    h.bind("m1");
    h.bind("m2");
    h.add(&person(), 1);
    let native = super::production::native(&h);
    let machine = h.store.team(TEAM).unwrap().unwrap().home;
    let a = super::super::Runtime::open(h.temp.path(), &machine).unwrap();
    let p = Production {
        native,
        deliveries: vec![],
    };
    row(&p, &h, "m2", &[("activity", json!("busy"))]);
    (h, a, p, lead)
}

/// The member's store state as the ops' own operations leave it.
fn left(h: &mut Harness, state: &std::path::Path, how: &str) {
    match how {
        "paused" => {
            h.run(
                &person(),
                Operation::PauseMember {
                    member: "m1".into(),
                },
            )
            .unwrap();
        }
        "stopped" => {
            h.run(
                &person(),
                Operation::StopMember {
                    member: "m1".into(),
                },
            )
            .unwrap();
        }
        "unbound" => {
            // The real binding observation's decisive end: the member's
            // durable handle names a session whose record is gone, so the
            // actor's own availability read resolves Gone (zerus-63rru.85
            // round 2, zerus-63rru.87). Only the session's record file is
            // removed; no member state is written by hand.
            // The handle's label is the session's record name; its record's
            // absence is the observation's Gone.
            let handle = h.store.member_handle(TEAM, "m1").unwrap().unwrap();
            eprintln!("DBG85r2 handle: {:?}", handle);
            // The session's record lives in the ID layout: removing it is
            // the observation's Gone.
            let path = state
                .join("sessions")
                .join(format!("{}.json", handle.session_id));
            std::fs::remove_file(path).unwrap();
        }
        other => panic!("{other}"),
    }
}

/// The hand-out crashes after its intent (the effect reserved, no outcome
/// recorded); then the member leaves and the service restarts: the
/// reservation exists with the member no longer in the active set.
fn reserved_then_left(
    h: &mut Harness,
    a: &mut super::super::Runtime,
    p: &mut Production,
    how: &str,
) -> (super::super::Runtime, String) {
    super::super::delivery::CRASH.with(|c| c.set(Some("intent")));
    let crashed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        a.tick(&mut h.store, p, TEAM, T0, false)
    }));
    assert!(
        crashed.is_err(),
        "{how}: the intent boundary was not exercised"
    );
    assert!(p.deliveries.is_empty(), "{how}: {:?}", p.deliveries);
    let id = effect_id(h);
    left(h, &p.native.config.state, how);
    let machine = h.store.team(TEAM).unwrap().unwrap().home;
    (
        super::super::Runtime::open(h.temp.path(), &machine).unwrap(),
        id,
    )
}

fn effects(h: &Harness) -> Vec<(String, String, u32)> {
    h.store
        .effects(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .map(|e| (e.state, e.member, e.node))
        .collect()
}

fn effect_id(h: &Harness) -> String {
    h.store
        .effects(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .find(|e| e.member == "m1")
        .unwrap()
        .request_id
}

fn items(h: &Harness) -> Vec<(String, String)> {
    h.store
        .inbox(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .map(|i| (i.kind, i.text))
        .collect()
}

/// The defect measured: the reservation is replayed through the non-start
/// tiers (0/1/3/7/15 minutes, about 26 in all), then the effect is refused
/// and the next pass hands the task to m2 — and nobody is told. The fixed
/// asserts (red until the fix): settled at the next pass with one item.
#[test]
fn a_reservation_for_a_member_that_left_the_active_set_is_settled_at_the_next_pass_with_one_item() {
    for how in ["paused", "stopped"] {
        let (mut h, mut a, mut p, lead) = team();
        let (mut a, id) = reserved_then_left(&mut h, &mut a, &mut p, how);
        assert_eq!(effects(&h), [("reserved".into(), "m1".into(), 1)], "{how}");
        // The old behavior replays through the tiers; the fix settles at
        // the next pass.
        a.tick(&mut h.store, &mut p, TEAM, T0 + MINUTE, false)
            .unwrap();
        assert_eq!(
            effects(&h),
            [("superseded".into(), "m1".into(), 1)],
            "{how}"
        );
        assert_eq!(
            items(&h),
            [(
                format!("member_left:{id}"),
                format!(
                    "Member m1 was {how} before the hand-out of task 1 reached it; the task is offered to another member."
                )
            )],
            "{how}"
        );
        // No failure row for the task, and the next pass hands the task to
        // the other free member.
        row(&p, &h, "m2", &[("activity", json!("idle"))]);
        a.tick(&mut h.store, &mut p, TEAM, T0 + 2 * MINUTE, false)
            .unwrap();
        assert!(
            effects(&h)
                .iter()
                .any(|(s, m, n)| s == "submitted" && m == "m2" && *n == 1),
            "{how}: {:?}",
            effects(&h)
        );
        assert!(
            a.ledger.get(TEAM, "gate:handout:1").unwrap().is_none(),
            "{how}: no failure gate"
        );
        let _ = lead;
    }
}

/// With no other member free, the task waits in the queue after the
/// settlement; the unstaffed rules apply.
#[test]
fn a_settled_reservation_with_no_other_member_leaves_the_task_in_the_queue() {
    let (mut h, mut a, mut p, _lead) = team();
    let (mut a, _id) = reserved_then_left(&mut h, &mut a, &mut p, "paused");
    a.tick(&mut h.store, &mut p, TEAM, T0 + MINUTE, false)
        .unwrap();
    assert_eq!(items(&h).len(), 1, "{:?}", items(&h));
    assert!(effects(&h).iter().all(|(s, _, _)| s == "superseded"));
    // m2 stays busy; over the following passes nothing is handed out and
    // nothing more is told.
    for minute in 2..6 {
        a.tick(&mut h.store, &mut p, TEAM, T0 + minute * MINUTE, false)
            .unwrap();
    }
    assert_eq!(items(&h).len(), 1);
    assert!(
        a.ledger.get(TEAM, "gate:handout:1").unwrap().is_none(),
        "no failure gate"
    );
}

/// zerus-63rru.87 through the real binding observation (round 2): a
/// member whose binding ended while it holds an open hand-out is settled
/// at the next pass like a paused one, with one member_left item naming
/// why, and the task goes to another member on the following pass.
#[test]
fn an_unbound_members_reservation_is_settled_with_one_item_and_reassigned() {
    let (mut h, mut a, mut p, _lead) = team();
    let (mut a, id) = reserved_then_left(&mut h, &mut a, &mut p, "unbound");
    a.tick(&mut h.store, &mut p, TEAM, T0 + MINUTE, false)
        .unwrap();
    assert_eq!(
        items(&h),
        [(
            format!("member_left:{id}"),
            "Member m1 was unbound before the hand-out of task 1 reached it; the task is offered to another member.".into()
        )]
    );
    assert!(
        a.ledger.get(TEAM, "gate:handout:1").unwrap().is_none(),
        "no failure gate"
    );
    // Round 3: the actor recorded the ended binding in the store before the
    // settlement, so the store — not a request's reason — shows the member
    // unbound, and the handle of the ended binding is gone.
    let m = h.store.member(TEAM, "m1").unwrap().unwrap();
    assert_eq!(m.state, zerus_team_plan::model::MemberState::Active);
    assert!(m.session.is_none(), "the store shows the member unbound");
    assert!(
        h.store.member_handle(TEAM, "m1").unwrap().is_none(),
        "the ended binding's handle is cleared"
    );
    row(&p, &h, "m2", &[("activity", json!("idle"))]);
    a.tick(&mut h.store, &mut p, TEAM, T0 + 2 * MINUTE, false)
        .unwrap();
    assert!(
        effects(&h)
            .iter()
            .any(|(s, m, n)| s == "submitted" && m == "m2" && *n == 1),
        "{:?}",
        effects(&h)
    );
}

/// zerus-63rru.94: a crash after the ledger's outcome save and before the
/// store's outcome write leaves the ledger outcome set while the store
/// effect is still reserved. Recovery records the saved outcome and
/// settles nothing: no member_left item and no duplicate delivery.
#[test]
fn a_crash_at_the_outcome_boundary_records_the_outcome_and_settles_nothing() {
    let (mut h, mut a, mut p, lead) = team();
    super::super::delivery::CRASH.with(|c| c.set(Some("outcome")));
    let crashed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        a.tick(&mut h.store, &mut p, TEAM, T0, false)
    }));
    assert!(crashed.is_err(), "the outcome boundary was not exercised");
    assert_eq!(p.deliveries.len(), 1, "one delivery");
    drop(a);
    h.run(
        &lead,
        Operation::PauseMember {
            member: "m1".into(),
        },
    )
    .unwrap();
    let home = h.store.team(TEAM).unwrap().unwrap().home;
    let mut a = super::super::Runtime::open(h.temp.path(), &home).unwrap();
    a.tick(&mut h.store, &mut p, TEAM, T0 + MINUTE, false)
        .unwrap();
    // No .85 settlement item for a recorded outcome; the .86 member_started
    // item is that rule's own, correct telling.
    assert!(
        items(&h)
            .iter()
            .all(|(kind, _)| !kind.starts_with("member_left:")),
        "no .85 item for a recorded outcome: {:?}",
        items(&h)
    );
    assert_eq!(effects(&h), [("submitted".into(), "m1".into(), 1)]);
    assert_eq!(p.deliveries.len(), 1, "no duplicate delivery");
}

/// zerus-63rru.96: a settle request that loses its race (a distinct
/// request at the same revision already settled the row) returns
/// `stale_revision`, which is not a failure gate: the next pass re-reads
/// and nothing fails.
#[test]
fn a_stale_settle_revision_is_not_a_failure_gate() {
    let (mut h, mut a, mut p, _lead) = team();
    let (mut a, _id) = reserved_then_left(&mut h, &mut a, &mut p, "paused");
    // The settle's apply loses its race: the injected stale revision must
    // not open a failure gate, and the next pass re-reads and settles.
    super::super::delivery::STALE_RACE.with(|c| c.set(true));
    a.tick(&mut h.store, &mut p, TEAM, T0 + MINUTE, false)
        .unwrap();
    assert!(
        a.ledger
            .prefix_page(TEAM, "gate:delivery:", "")
            .unwrap()
            .is_empty(),
        "a stale loser opened a failure gate"
    );
    super::super::delivery::STALE_RACE.with(|c| c.set(false));
    a.tick(&mut h.store, &mut p, TEAM, T0 + 2 * MINUTE, false)
        .unwrap();
    assert_eq!(items(&h).len(), 1, "one item after the re-read");
    assert!(
        effects(&h)
            .iter()
            .any(|(s, m, n)| s == "superseded" && m == "m1" && *n == 1),
        "{:?}",
        effects(&h)
    );
}

/// The ops' own Active guard (zerus-63rru.85 round 2, made unconditional in
/// round 3): a settle for a member that is Active with a live binding is
/// refused and writes nothing, even though the reservation is open.
#[test]
fn a_settle_for_an_active_member_writes_nothing() {
    let (mut h, mut a, mut p, _lead) = team();
    super::super::delivery::CRASH.with(|c| c.set(Some("intent")));
    let crashed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        a.tick(&mut h.store, &mut p, TEAM, T0, false)
    }));
    assert!(crashed.is_err(), "the intent boundary was not exercised");
    drop(a);
    // The reservation exists and the member is still Active with a live
    // binding: the ops' own guard refuses and writes nothing.
    let request = request(
        h.store.team(TEAM).unwrap().unwrap().revision,
        Operation::SettleLeftHandout {
            node: 1,
            reason: None,
        },
    );
    let refused = crate::teams::ops::apply(&mut h.store, &service(), &request, T0);
    assert_eq!(
        refused.unwrap_err().code,
        zerus_team_plan::problem::code::INVALID_STATE
    );
    assert!(items(&h).is_empty(), "{:?}", items(&h));
    assert_eq!(effects(&h), [("reserved".into(), "m1".into(), 1)]);
}

/// The reviewer's round-2 probe, committed (zerus-63rru.85 round 3): an
/// Active bound member's reservation is refused for every reason shape a
/// round-2 caller could send — none, empty, and the text the actor used to
/// pass — and nothing is superseded or told. The store, not the reason,
/// decides.
#[test]
fn an_active_bound_members_settle_is_refused_for_none_empty_and_text_reasons() {
    let reasons: [Option<String>; 4] = [
        None,
        Some(String::new()),
        Some("unbound".into()),
        Some("paused".into()),
    ];
    for reason in reasons {
        let (mut h, mut a, mut p, _lead) = team();
        super::super::delivery::CRASH.with(|c| c.set(Some("intent")));
        let crashed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            a.tick(&mut h.store, &mut p, TEAM, T0, false)
        }));
        assert!(crashed.is_err(), "{reason:?}: the intent boundary was not exercised");
        drop(a);
        assert_eq!(
            h.store.member(TEAM, "m1").unwrap().unwrap().state,
            zerus_team_plan::model::MemberState::Active,
            "{reason:?}"
        );
        let request = request(
            h.store.team(TEAM).unwrap().unwrap().revision,
            Operation::SettleLeftHandout {
                node: 1,
                reason: reason.clone(),
            },
        );
        let refused = crate::teams::ops::apply(&mut h.store, &service(), &request, T0);
        assert_eq!(
            refused.unwrap_err().code,
            zerus_team_plan::problem::code::INVALID_STATE,
            "{reason:?}: the settle must be refused"
        );
        assert_eq!(
            effects(&h),
            [("reserved".into(), "m1".into(), 1)],
            "{reason:?}: nothing is superseded"
        );
        assert!(items(&h).is_empty(), "{reason:?}: nothing is told");
    }
}

/// A member that comes back before the next pass keeps its reservation:
/// nothing settles and the hand-out proceeds.
#[test]
fn a_member_that_returns_before_the_next_pass_keeps_its_reservation() {
    let (mut h, mut a, mut p, lead) = team();
    let (mut a, _id) = reserved_then_left(&mut h, &mut a, &mut p, "paused");
    h.run(
        &lead,
        Operation::ResumeMember {
            member: "m1".into(),
        },
    )
    .unwrap();
    a.tick(&mut h.store, &mut p, TEAM, T0 + MINUTE, false)
        .unwrap();
    // The returned member keeps its reservation: nothing settles through
    // the .85 rule and nothing is told; the interrupted delivery's own
    // rule (the service restarted) records the uncertain outcome, which
    // is never replayed.
    assert!(effects(&h)
        .iter()
        .any(|(s, m, n)| s == "uncertain" && m == "m1" && *n == 1));
    // Nothing settles through the .85 rule: no member_left item; the
    // uncertain delivery's own notice may appear.
    assert!(items(&h)
        .iter()
        .all(|(kind, _)| !kind.starts_with("member_left:")));
}

/// A crash between the reservation and the pass: the restarted pass settles
/// the reservation all the same.
#[test]
fn a_crash_between_the_reservation_and_the_pass_still_settles_it() {
    let (mut h, mut a, mut p, _lead) = team();
    let (mut restarted, id) = reserved_then_left(&mut h, &mut a, &mut p, "stopped");
    restarted
        .tick(&mut h.store, &mut p, TEAM, T0 + MINUTE, false)
        .unwrap();
    assert_eq!(
        items(&h),
        [(
            format!("member_left:{id}"),
            "Member m1 was stopped before the hand-out of task 1 reached it; the task is offered to another member.".into()
        )]
    );
    assert!(effects(&h).iter().all(|(s, _, _)| s == "superseded"));
}
