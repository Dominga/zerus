//! zerus-63rru.86 through the production Native and the real pass: a member
//! paused or stopped while it holds a started task keeps the task, and the
//! lead is told once per member and task at the first pass that sees the
//! member out of the active set; the member's return ends the memory, so a
//! later leave tells the lead again. No timer starts, and the person's own
//! pause is not stored, so the item is written in every case.
use super::real_rows::Production;
use super::rows::seed_row;
use crate::teams::ops::tests::*;
use crate::teams::ops::{Actor, Operation};
use crate::teams::store::rows;
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

fn items(h: &Harness) -> Vec<(String, String)> {
    h.store
        .inbox(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .map(|i| (i.kind, i.text))
        .collect()
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

/// A paused or stopped member that holds a started task is told once, in
/// every case; its return ends the memory, so a later leave tells the lead
/// again; nothing else is sent over many passes and the task stays with
/// the member.
#[test]
fn a_paused_or_stopped_member_holding_a_started_task_is_told_once() {
    for how in ["paused", "stopped"] {
        let (mut h, mut a, mut p, lead) = team();
        // The first pass hands task 1 to m1 and the child submits it: the
        // task is in progress with m1.
        a.tick(&mut h.store, &mut p, TEAM, T0, false).unwrap();
        assert_eq!(effects(&h), [("submitted".into(), "m1".into(), 1)], "{how}");
        match how {
            "paused" => {
                h.run(
                    &lead,
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
            _ => unreachable!(),
        }
        a.tick(&mut h.store, &mut p, TEAM, T0 + MINUTE, false)
            .unwrap();
        let told = items(&h);
        assert_eq!(told.len(), 1, "{how}: {told:?}");
        assert!(
            told[0].0.starts_with("member_started:1:m1:1"),
            "{how}: {:?}",
            told[0].0
        );
        assert_eq!(
            told[0].1,
            format!(
                "Member m1 was {how} while task 1 is in progress; the work stays with the member until it is resumed or the person moves the task."
            ),
            "{how}"
        );
        // Nothing more while the state lasts, and the task stays with m1.
        for minute in 2..20 {
            a.tick(&mut h.store, &mut p, TEAM, T0 + minute * MINUTE, false)
                .unwrap();
        }
        assert_eq!(items(&h).len(), 1, "{how}: {:?}", items(&h));
        assert_eq!(effects(&h), [("submitted".into(), "m1".into(), 1)], "{how}");
        // The member returns and leaves again: a second item.
        h.run(
            &person(),
            Operation::ResumeMember {
                member: "m1".into(),
            },
        )
        .unwrap();
        a.tick(&mut h.store, &mut p, TEAM, T0 + 20 * MINUTE, false)
            .unwrap();
        assert_eq!(items(&h).len(), 1, "{how}: the return tells nothing");
        let inbox = h.store.inbox(TEAM, 200, None).unwrap();
        assert_eq!(inbox.rows[0].state, "closed", "{how}");
        match how {
            "paused" => {
                h.run(
                    &lead,
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
            _ => unreachable!(),
        }
        a.tick(&mut h.store, &mut p, TEAM, T0 + 21 * MINUTE, false)
            .unwrap();
        assert_eq!(items(&h).len(), 2, "{how}: the second leave tells again");
        assert!(items(&h)[1].0.starts_with("member_started:1:m1:2"), "{how}");
    }
}

/// A member holding no started task gets no item from this path.
#[test]
fn a_member_holding_no_started_task_gets_no_item() {
    let (mut h, mut a, mut p, lead) = team();
    // m1 is paused before the first pass: nothing was handed out.
    h.run(
        &lead,
        Operation::PauseMember {
            member: "m1".into(),
        },
    )
    .unwrap();
    for minute in 0..10 {
        a.tick(&mut h.store, &mut p, TEAM, T0 + minute * MINUTE, false)
            .unwrap();
    }
    assert!(items(&h).is_empty(), "{:?}", items(&h));
    assert!(effects(&h).is_empty());
}

/// The ops' own verification: a TellPausedStarted call for a member
/// without started work on the task writes nothing.
#[test]
fn a_tell_for_a_member_without_started_work_writes_nothing() {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    h.bind("m1");
    h.bind("m2");
    h.add(&person(), 1);
    h.run(
        &lead,
        Operation::PauseMember {
            member: "m1".into(),
        },
    )
    .unwrap();
    let request = request(
        h.store.team(TEAM).unwrap().unwrap().revision,
        Operation::TellPausedStarted {
            member: "m1".into(),
            node: 1,
        },
    );
    crate::teams::ops::apply(&mut h.store, &service(), &request, T0).unwrap();
    assert!(items(&h).is_empty(), "{:?}", items(&h));
}

/// The store's own once-only: a replayed request writes nothing, and a
/// lost actor row (a crash between the apply and the row) tells the lead
/// only once.
#[test]
fn a_replayed_request_and_a_lost_row_tell_once() {
    let (mut h, mut a, mut p, lead) = team();
    a.tick(&mut h.store, &mut p, TEAM, T0, false).unwrap();
    h.run(
        &lead,
        Operation::PauseMember {
            member: "m1".into(),
        },
    )
    .unwrap();
    a.tick(&mut h.store, &mut p, TEAM, T0 + MINUTE, false)
        .unwrap();
    assert_eq!(items(&h).len(), 1);
    // The replayed request: the same op again writes nothing.
    let request = request(
        h.store.team(TEAM).unwrap().unwrap().revision,
        Operation::TellPausedStarted {
            member: "m1".into(),
            node: 1,
        },
    );
    crate::teams::ops::apply(&mut h.store, &service(), &request, T0 + MINUTE).unwrap();
    assert_eq!(items(&h).len(), 1, "the replayed request told twice");
    // The crash between the apply and the actor's row: the row is lost,
    // and the next pass still tells once.
    let rows = a
        .ledger
        .prefix_page(TEAM, "paused_started:m1:", "")
        .unwrap();
    for row in &rows {
        a.ledger.remove(row).unwrap();
    }
    a.tick(&mut h.store, &mut p, TEAM, T0 + 2 * MINUTE, false)
        .unwrap();
    assert_eq!(items(&h).len(), 1, "the lost row told twice");
}

/// Round 3: the close is exact with the delimiter — m1's return closes
/// only m1's items, never m10's or a pattern member's.
#[test]
fn the_close_is_exact_with_the_delimiter() {
    let (mut h, mut a, mut p, lead) = team();
    a.tick(&mut h.store, &mut p, TEAM, T0, false).unwrap();
    h.run(&lead, Operation::PauseMember { member: "m1".into() })
        .unwrap();
    a.tick(&mut h.store, &mut p, TEAM, T0 + MINUTE, false)
        .unwrap();
    assert_eq!(items(&h).len(), 1, "{:?}", items(&h));
    // A second member's and a pattern member's items, written through the
    // store's own row writer.
    let encoded = crate::teams::store::encode(&service()).unwrap();
    // A second member's ledger row, so the cleanup's delimiter matters.
    a.ledger
        .reserve(TEAM, "paused_started:m10:1", serde_json::json!({}))
        .unwrap();
    for kind in ["member_started:7:m10:1", "member_started:7:m_1:1"] {
        rows::hand_back_item(
            h.store.db.as_ref().unwrap(),
            TEAM,
            1,
            &encoded,
            kind,
            "the item",
            T0,
        )
        .unwrap();
    }
    // m1 returns: only m1's items close.
    h.run(&person(), Operation::ResumeMember { member: "m1".into() })
        .unwrap();
    a.tick(&mut h.store, &mut p, TEAM, T0 + 2 * MINUTE, false)
        .unwrap();
    let inbox = h.store.inbox(TEAM, 200, None).unwrap();
    let mut states: Vec<(String, String)> = inbox
        .rows
        .iter()
        .map(|i| (i.kind.clone(), i.state.clone()))
        .collect();
    states.sort();
    assert_eq!(
        states,
        [
            ("member_started:1:m1:1".into(), "closed".into()),
            ("member_started:7:m10:1".into(), "open".into()),
            ("member_started:7:m_1:1".into(), "open".into()),
        ],
        "the close touched the wrong items"
    );
    // The ledger cleanup kept m10's row: the delimiter'd prefix never
    // removes another member's rows.
    assert!(
        a.ledger.get(TEAM, "paused_started:m10:1").unwrap().is_some(),
        "m1's return removed m10's ledger row"
    );
}

/// Round 3: the crash window between the tell's apply and the actor's
/// ledger row — the item is written, the row is lost, and the member's
/// return still closes the item because the close's decision comes from
/// the store.
#[test]
fn the_crash_window_item_still_closes_on_the_return() {
    let (mut h, mut a, mut p, lead) = team();
    a.tick(&mut h.store, &mut p, TEAM, T0, false).unwrap();
    h.run(
        &lead,
        Operation::PauseMember {
            member: "m1".into(),
        },
    )
    .unwrap();
    a.tick(&mut h.store, &mut p, TEAM, T0 + MINUTE, false)
        .unwrap();
    assert_eq!(items(&h).len(), 1, "{:?}", items(&h));
    // The ledger row is lost (the crash window).
    let rows = a
        .ledger
        .prefix_page(TEAM, "paused_started:m1:", "")
        .unwrap();
    for row in &rows {
        a.ledger.remove(row).unwrap();
    }
    // The member returns: the store-driven close closes the item.
    h.run(
        &person(),
        Operation::ResumeMember {
            member: "m1".into(),
        },
    )
    .unwrap();
    a.tick(&mut h.store, &mut p, TEAM, T0 + 2 * MINUTE, false)
        .unwrap();
    let inbox = h.store.inbox(TEAM, 200, None).unwrap();
    assert!(
        inbox
            .rows
            .iter()
            .any(|i| i.kind.starts_with("member_started:") && i.state == "closed"),
        "the item was not closed: {:?}",
        inbox
            .rows
            .iter()
            .map(|i| (i.kind.as_str(), i.state.as_str()))
            .collect::<Vec<_>>()
    );
}

/// A removed member keeps the removal's path: the removal's own item, never
/// the paused-started one.
#[test]
fn a_removed_member_keeps_the_removal_path() {
    let (mut h, mut a, mut p, _lead) = team();
    a.tick(&mut h.store, &mut p, TEAM, T0, false).unwrap();
    h.run(
        &person(),
        Operation::ArchiveMember {
            member: "m1".into(),
        },
    )
    .unwrap();
    for minute in 1..10 {
        a.tick(&mut h.store, &mut p, TEAM, T0 + minute * MINUTE, false)
            .unwrap();
    }
    assert!(items(&h)
        .iter()
        .any(|(kind, _)| kind.starts_with("member_removed:")));
    assert!(items(&h)
        .iter()
        .all(|(kind, _)| !kind.starts_with("member_left:")));
}
