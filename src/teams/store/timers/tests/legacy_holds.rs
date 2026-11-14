//! The step to layout 11 on stores the old commits wrote with their own
//! operations (layout 8: 40046fa3f, layout 9: 452bec1fe, layout 10:
//! c29dc402e), kept as SQL dumps under `legacy/`. The generator ran inside
//! each commit's own ops test harness; its source and how to rerun it are
//! in `legacy/README.md`. A held node without an unsettled decision belongs
//! to the last of its recorded events in journal order; a hand-back the
//! records cannot place, an archived record, or nothing found leaves it the
//! person's. No clock is read.
use super::*;
use crate::teams::store::timers::schema::EXTENSION;

/// What a store's layout, case and dump are.
struct Dump {
    layout: u32,
    case: &'static str,
    sql: &'static str,
}
macro_rules! dump {
    ($layout:literal, $case:literal) => {
        Dump {
            layout: $layout,
            case: $case,
            sql: include_str!(concat!("legacy/layout", $layout, "-", $case, ".sql")),
        }
    };
}
const HOLDS: [Dump; 3] = [dump!(8, "holds"), dump!(9, "holds"), dump!(10, "holds")];

#[derive(Clone, Copy, Debug)]
enum Clock {
    /// The times as the old commit wrote them.
    Kept,
    /// Every time the same.
    One,
    /// Every time running backwards against the rows' order.
    Descending,
}

/// The dump as a store of its own layout, opened at this head.
fn legacy(dump: &Dump, clock: Clock, archived: Option<&str>) -> Harness {
    let mut h = Harness::empty();
    drop(h.store.db.take());
    let path = h.temp.path().join("teams/teams-v1.sqlite3");
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
    // Created private, as the store creates its file.
    drop(
        crate::platform::private_open(
            std::fs::OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true),
        )
        .open(&path)
        .unwrap(),
    );
    let db = Connection::open(&path).unwrap();
    db.execute_batch(dump.sql).unwrap();
    let layout: u32 = db
        .query_row("SELECT version FROM timers_extension WHERE id=1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(layout, dump.layout, "{}", dump.case);
    let times: Vec<(String, String)> = {
        let mut statement = db
            .prepare("SELECT m.name,c.name FROM sqlite_master m, pragma_table_info(m.name) c WHERE m.type='table' AND c.name LIKE '%\\_ms' ESCAPE '\\'")
            .unwrap();
        let rows = statement
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        rows
    };
    assert!(times.iter().any(|(t, c)| t == "journal" && c == "at_ms"));
    for (table, column) in &times {
        let value = match clock {
            Clock::Kept => continue,
            Clock::One => "1".to_owned(),
            Clock::Descending => "1000000000-rowid".to_owned(),
        };
        db.execute(&format!("UPDATE {table} SET {column}={value}"), [])
            .unwrap();
    }
    if let Some(rows) = archived {
        // As the journal's archive writer moves rows (ops/journal.rs).
        let rows = format!("team='{TEAM}' AND seq<=({rows})");
        db.execute_batch(&format!(
            "INSERT INTO journal_archive SELECT * FROM journal WHERE {rows}; DELETE FROM journal WHERE {rows};"
        ))
        .unwrap();
    }
    drop(db);
    reopen(&mut h);
    assert_eq!(marker(&h), Some(EXTENSION));
    h
}
fn marker(h: &Harness) -> Option<u32> {
    db(h)
        .query_row("SELECT version FROM timers_extension WHERE id=1", [], |r| {
            r.get(0)
        })
        .ok()
}
/// The stored hold: (flag, owner, hand-back).
type Stored = (bool, Option<String>, bool);
fn stored(h: &Harness, node: u32) -> Stored {
    db(h)
        .query_row(
            "SELECT hold,hold_owner,hand_back FROM nodes WHERE team=?1 AND id=?2",
            params![TEAM, node],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap()
}
fn lead(h: &Harness) -> Actor {
    Actor::Session {
        person: "local".into(),
        team: TEAM.into(),
        member: "lead".into(),
        session: h
            .store
            .member(TEAM, "lead")
            .unwrap()
            .unwrap()
            .session
            .unwrap(),
    }
}
fn person_hold() -> Stored {
    (true, Some("person".to_owned()), false)
}
fn lead_hold() -> Stored {
    (true, Some("lead".to_owned()), false)
}
fn hand_back() -> Stored {
    (false, None, true)
}
/// The holds store's nodes and what the records say of each.
fn expected() -> [(u32, Stored, &'static str); 8] {
    [
        (
            1,
            person_hold(),
            "a hand-back, the lead's Release, the person's Hold",
        ),
        (2, hand_back(), "the person's Hold, then a hand-back"),
        (3, lead_hold(), "the lead's Hold, then nothing"),
        (
            4,
            lead_hold(),
            "a hand-back, the lead's Release, the lead's Hold",
        ),
        (5, hand_back(), "a refused reopen, placed by its effect"),
        (
            6,
            person_hold(),
            "a reopen cutting over, the lead's Release, the person's Hold",
        ),
        (7, person_hold(), "the person's Hold, then nothing"),
        (
            8,
            person_hold(),
            "as 1, then a card of the node numbered as its item",
        ),
    ]
}

/// The reviewer's sequence and its mirrors, on the stores of each layout.
#[test]
fn each_legacy_hold_goes_to_the_last_recorded_event_that_set_it() {
    for dump in &HOLDS {
        let h = legacy(dump, Clock::Kept, None);
        for (node, want, case) in expected() {
            assert_eq!(
                stored(&h, node),
                want,
                "layout {}: node {node}, {case}",
                dump.layout
            );
            assert!(
                h.node(node).hold,
                "layout {}: node {node} stays held",
                dump.layout
            );
        }
    }
}

/// The reviewer's sequence: the lead may not release the person's Hold;
/// the person may. A lead's hold and a hand-back the lead releases.
#[test]
fn the_migrated_holds_are_released_by_their_owners_only() {
    for dump in &HOLDS {
        let mut h = legacy(dump, Clock::Kept, None);
        let lead = lead(&h);
        for node in [1, 6, 7, 8] {
            let error = h.run(&lead, Operation::Release { node }).unwrap_err();
            assert_eq!(
                error.code,
                code::NOT_ALLOWED,
                "layout {}: node {node}",
                dump.layout
            );
            h.run(&person(), Operation::Release { node }).unwrap();
            assert!(!h.node(node).hold, "layout {}: node {node}", dump.layout);
        }
        for node in [2, 3, 4, 5] {
            h.run(&lead, Operation::Release { node }).unwrap();
            assert!(!h.node(node).hold, "layout {}: node {node}", dump.layout);
        }
    }
}

/// The step reads no clock: the same stores with every time made equal, or
/// running backwards, classify the same.
#[test]
fn the_step_classifies_by_the_records_order_not_by_their_times() {
    for dump in &HOLDS {
        for clock in [Clock::One, Clock::Descending] {
            let h = legacy(dump, clock, None);
            for (node, want, case) in expected() {
                assert_eq!(
                    stored(&h, node),
                    want,
                    "layout {} {clock:?}: node {node}, {case}",
                    dump.layout
                );
            }
        }
    }
}

/// The archive is not read: with every row of the team archived, as the
/// journal's own archive writer moves them, nothing places a hold, and each
/// is the person's.
#[test]
fn a_hold_whose_records_are_archived_is_the_persons() {
    for dump in &HOLDS {
        let h = legacy(dump, Clock::Kept, Some("SELECT max(seq) FROM journal"));
        for (node, _, case) in expected() {
            assert_eq!(
                stored(&h, node),
                person_hold(),
                "layout {}: node {node}, {case}",
                dump.layout
            );
        }
    }
}

/// A hand-back that no record places (a removed member's item, a pipeline
/// active on a stage, the step to layout 10's own Return item) is the
/// person's: the layout-11 store reads it held through every reader, the
/// lead's Release is refused, the episode's own end neither fails nor frees
/// the node, and the person's Release frees it.
fn unplaced(dump: &Dump, node: u32, end: impl Fn(&mut Harness)) {
    let label = format!("layout {} {}", dump.layout, dump.case);
    let mut h = legacy(dump, Clock::Kept, None);
    assert_eq!(stored(&h, node), person_hold(), "{label}");
    assert!(h.node(node).hold, "{label}");
    assert!(
        h.store
            .queue(TEAM, 200, None)
            .unwrap()
            .rows
            .iter()
            .any(|n| n.id == node && n.hold),
        "{label}: the queue reads it held"
    );
    crate::teams::ops::readiness(&h.store, TEAM).unwrap();
    let lead = lead(&h);
    let error = h.run(&lead, Operation::Release { node }).unwrap_err();
    assert_eq!(error.code, code::NOT_ALLOWED, "{label}");
    end(&mut h);
    let (flag, owner, _) = stored(&h, node);
    assert_eq!(
        (flag, owner.as_deref()),
        (true, Some("person")),
        "{label}: the episode's end keeps it"
    );
    assert!(h.node(node).hold, "{label}");
    let mut h = legacy(dump, Clock::Kept, None);
    h.run(&person(), Operation::Release { node }).unwrap();
    assert!(!h.node(node).hold, "{label}: the person's Release frees it");
}

/// Layout 10: the lead's decision on the removed member's item.
#[test]
fn a_removed_members_hand_back_is_the_persons() {
    // Node 2 was the lead's Hold before the same hand-back: still the person's.
    for node in [1, 2] {
        unplaced(&dump!(10, "removed"), node, |h| {
            let lead = lead(h);
            h.run(
                &lead,
                Operation::Reassign {
                    node,
                    member: "m1".into(),
                    reason: "To a present member".into(),
                },
            )
            .unwrap();
        });
    }
}

/// Layout 9: the step to 10 hands the stranded Return back without a
/// journal row; the lead's decision on its item.
#[test]
fn the_step_to_layout_10s_own_hand_back_is_the_persons() {
    // Node 2 was the lead's Hold before its Return: still the person's.
    for node in [1, 2] {
        unplaced(&dump!(9, "stranded"), node, |h| {
            let lead = lead(h);
            h.run(
                &lead,
                Operation::Reassign {
                    node,
                    member: "m1".into(),
                    reason: "Decided again".into(),
                },
            )
            .unwrap();
        });
    }
}

/// Layouts 8, 9 and 10: a parent whose pipeline is active on a stage. The
/// stage's end: its task is created, handed out, and its verdict passes.
#[test]
fn a_pipeline_active_on_a_stage_is_the_persons() {
    // Parent 3 (its first sibling's stage is node 2) had a hand-back placed by its record before its stage.
    for (dump, parent) in [
        (dump!(8, "pipeline_active"), 1),
        (dump!(8, "pipeline_active"), 3),
        (dump!(9, "pipeline_active"), 1),
        (dump!(9, "pipeline_active"), 3),
        (dump!(10, "pipeline_active"), 1),
        (dump!(10, "pipeline_active"), 3),
    ] {
        unplaced(&dump, parent, |h| {
            let stage = h
                .store
                .queue(TEAM, 200, None)
                .unwrap()
                .rows
                .into_iter()
                .find(|n| n.stage_of == Some(parent))
                .unwrap()
                .id;
            let id = uuid::Uuid::new_v4().to_string();
            h.run(
                &service(),
                Operation::StageTaskIntent {
                    node: stage,
                    effect_request_id: id.clone(),
                },
            )
            .unwrap();
            h.run(
                &service(),
                Operation::StageTaskOutcome {
                    effect_request_id: id,
                    outcome: crate::teams::ops::StageTaskOutcome::Created {
                        task: task(1000 + stage),
                    },
                },
            )
            .unwrap();
            h.observe(stage, crate::teams::ops::TaskStage::Todo);
            h.submit(stage, "m3");
            let reviewer = Actor::Session {
                person: "local".into(),
                team: TEAM.into(),
                member: "m3".into(),
                session: h
                    .store
                    .member(TEAM, "m3")
                    .unwrap()
                    .unwrap()
                    .session
                    .unwrap(),
            };
            h.run(
                &reviewer,
                Operation::Verdict {
                    node: stage,
                    generation: 1,
                    pass: true,
                    text: "Checked the criteria".into(),
                },
            )
            .unwrap();
        });
    }
}

/// Layouts 8, 9 and 10: a pipeline waiting on the person is placed exactly,
/// by the card its operation's reply names, and stays a hand-back.
#[test]
fn a_pipeline_waiting_on_the_person_is_a_hand_back() {
    for dump in [
        dump!(8, "pipeline_person"),
        dump!(9, "pipeline_person"),
        dump!(10, "pipeline_person"),
    ] {
        let mut h = legacy(&dump, Clock::Kept, None);
        assert_eq!(stored(&h, 1), hand_back(), "layout {}", dump.layout);
        let lead = lead(&h);
        h.run(&lead, Operation::Release { node: 1 }).unwrap();
        assert!(!h.node(1).hold, "layout {}", dump.layout);
    }
}

/// A hand-back whose record is archived is not placed, even when a later
/// Hold is live: node 4's refused cancel is archived, its lead's Release and
/// Hold are not, and the hold is the person's. Node 5's records stay live.
#[test]
fn a_hand_back_whose_record_is_archived_is_not_outweighed_by_a_live_hold() {
    for dump in &HOLDS {
        let h = legacy(
            dump,
            Clock::Kept,
            Some("SELECT max(seq) FROM journal WHERE kind='record_cancel' AND refs LIKE '%\"node:4\"%'"),
        );
        assert_eq!(stored(&h, 4), person_hold(), "layout {}", dump.layout);
        assert_eq!(stored(&h, 5), hand_back(), "layout {}", dump.layout);
    }
}

/// A Release last with the flag still set (a stop's own hold, whose cancel
/// was refused after the lead had released a hand-back): no record says
/// who holds it, and it is the person's.
#[test]
fn a_release_last_leaves_the_hold_the_persons() {
    for dump in [
        dump!(8, "stopped"),
        dump!(9, "stopped"),
        dump!(10, "stopped"),
    ] {
        let h = legacy(&dump, Clock::Kept, None);
        assert_eq!(stored(&h, 1), person_hold(), "layout {}", dump.layout);
    }
}

/// A verdict counts only on a stage of the node: the same real store, with
/// the verdict's saved request naming another node (as a verdict on another
/// parent's stage whose reply names the same number would), leaves the
/// card unplaced and the hold the person's.
#[test]
fn a_verdict_places_only_the_card_of_its_stages_parent() {
    for dump in [
        dump!(8, "pipeline_person"),
        dump!(9, "pipeline_person"),
        dump!(10, "pipeline_person"),
    ] {
        let mut sql = dump.sql.to_owned();
        let verdict = "\"op\":\"verdict\",\"node\":2,";
        assert_eq!(sql.matches(verdict).count(), 1, "layout {}", dump.layout);
        sql = sql.replace(verdict, "\"op\":\"verdict\",\"node\":99,");
        let moved = Dump {
            layout: dump.layout,
            case: dump.case,
            sql: Box::leak(sql.into_boxed_str()),
        };
        let h = legacy(&moved, Clock::Kept, None);
        assert_eq!(stored(&h, 1), person_hold(), "layout {}", dump.layout);
    }
}

/// Layout 10: an integration conflict whose holder was removed, and a
/// failing verdict whose implementer was removed, each placed by the saved
/// reply of the operation that filed its item: hand-backs the lead ends.
#[test]
fn a_removed_holders_conflict_and_verdict_are_hand_backs() {
    for (dump, node) in [
        (dump!(10, "conflict"), 1),
        (dump!(10, "pipeline_verdict"), 1),
    ] {
        let mut h = legacy(&dump, Clock::Kept, None);
        assert_eq!(stored(&h, node), hand_back(), "{}", dump.case);
        let lead = lead(&h);
        h.run(&lead, Operation::Release { node }).unwrap();
        assert!(!h.node(node).hold, "{}", dump.case);
    }
}
