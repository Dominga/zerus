use super::*;
use crate::tests::{delta, item, put, setup, snapshot};
use crate::Applied;
use serde_json::json;

fn approval(n: usize) -> watch::CatalogItem {
    let mut item = item(n);
    item.row["phase"] = json!("approval");
    item
}

fn visible(db: &Connection) -> i64 {
    db.query_row(
        "SELECT visible FROM desktop_catalog_sources WHERE computer='peer'",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

fn flagged(db: &Connection) -> Vec<String> {
    page(db, visible(db), 0, 100)
        .unwrap()
        .into_iter()
        .map(|row| row.key)
        .collect()
}

use zerus_watch_protocol as watch;

#[test]
fn the_flag_follows_each_stored_row_version() {
    let (mut db, source) = setup();
    let mut child = item(3);
    child.row["subagents"] = json!({"a1":{"status":"Needs approval"}});
    let snap = snapshot(2, 0, true, 1, vec![item(1), approval(2), child]);
    assert_eq!(put(&mut db, &source, &snap), Applied::Visible);
    assert_eq!(flagged(&db), ["run:run-2", "run:run-3"]);
    // A session entering and one leaving the phase, and a removal.
    let update = delta(3, 2, vec![approval(1), item(2)], &[3]);
    assert_eq!(put(&mut db, &source, &update), Applied::Visible);
    assert_eq!(flagged(&db), ["run:run-1"]);
    // A resync builds a new generation whose rows carry their own flags.
    let old = visible(&db);
    assert_eq!(
        put(
            &mut db,
            &source,
            &snapshot(4, 0, true, 3, vec![item(1), approval(4)])
        ),
        Applied::Visible
    );
    assert_ne!(visible(&db), old);
    assert_eq!(flagged(&db), ["run:run-4"]);
}

#[test]
fn input_and_every_subagent_marker_are_approval_phases() {
    for (row, expected) in [
        (json!({"phase":"approval"}), true),
        (json!({"phase":"input"}), true),
        (json!({"phase":"working"}), false),
        (json!({"phase":"question"}), false),
        (json!({}), false),
        (json!({"subagents":{"a":{"phase":"approval"}}}), true),
        (json!({"subagents":{"a":{"state":"approval"}}}), true),
        (
            json!({"subagents":{"a":{"display_state":"approval"}}}),
            true,
        ),
        (json!({"subagents":{"a":{"status":"Needs approval"}}}), true),
        (
            json!({"subagents":{"a":{"phase":"working"},"b":{"status":"Running"}}}),
            false,
        ),
        (json!({"subagents":[{"phase":"approval"}]}), false),
        (json!({"phase":["approval"]}), false),
    ] {
        assert_eq!(approval_phase(&row), expected, "{row}");
    }
}

#[test]
fn backfill_derives_old_rows_in_bounded_resumable_batches() {
    let (mut db, source) = setup();
    let rows: Vec<_> = (0..5)
        .map(|n| if n % 2 == 0 { approval(n) } else { item(n) })
        .collect();
    put(&mut db, &source, &snapshot(2, 0, true, 1, rows));
    // Rows stored before schema 22 read NULL, as does one corrupt row.
    db.execute_batch("UPDATE desktop_catalog_rows SET approval=NULL;")
        .unwrap();
    db.execute(
        "UPDATE desktop_catalog_rows SET json='not json' WHERE key='run:run-4'",
        [],
    )
    .unwrap();
    assert!(flagged(&db).is_empty());
    assert_eq!(backfill(&mut db, 2).unwrap(), 2);
    assert_eq!(flagged(&db), ["run:run-0"]);
    // A restart resumes from the database alone.
    assert_eq!(backfill(&mut db, 2).unwrap(), 2);
    assert_eq!(backfill(&mut db, 2).unwrap(), 1);
    assert_eq!(backfill(&mut db, 2).unwrap(), 0);
    assert_eq!(flagged(&db), ["run:run-0", "run:run-2"]);
    let pending: i64 = db
        .query_row(
            "SELECT count(*) FROM desktop_catalog_rows WHERE approval IS NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(pending, 0);
}

#[test]
fn the_keyset_walk_stays_stable_while_rows_arrive() {
    let (mut db, source) = setup();
    put(
        &mut db,
        &source,
        &snapshot(2, 0, true, 1, vec![approval(1), approval(2)]),
    );
    let generation = visible(&db);
    let first = page(&db, generation, 0, 1).unwrap();
    assert_eq!(first[0].key, "run:run-1");
    // An update keeps its row id; a new row gets a later one.
    put(
        &mut db,
        &source,
        &delta(3, 2, vec![approval(1), approval(3)], &[]),
    );
    let rest: Vec<_> = page(&db, generation, first[0].id, 10)
        .unwrap()
        .into_iter()
        .map(|row| row.key)
        .collect();
    assert_eq!(rest, ["run:run-2", "run:run-3"]);
    assert_eq!(page(&db, generation, 0, 10).unwrap().len(), 3);
}
