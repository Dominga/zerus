use super::*;
use crate::tests::{heartbeat, hello, item, put, setup, snapshot, visible};
use serde_json::json;

fn start() -> (Connection, Source) {
    let (mut db, source) = setup();
    put(&mut db, &source, &snapshot(2, 0, true, 1, vec![item(999)]));
    let source = Source {
        generation: 2,
        ..source
    };
    (db, source)
}
fn metadata() -> Value {
    json!({"machine_id":"example-machine","future":{"retained":true}})
}
fn meta(db: &Connection) -> Option<String> {
    db.query_row(
        "SELECT meta FROM desktop_catalog_sources WHERE computer='peer'",
        [],
        |r| r.get(0),
    )
    .unwrap()
}
fn replace(db: &mut Connection, source: &Source, row: usize) {
    begin(db, source, &metadata(), 1, 1).unwrap();
    page(db, source, 0, &[item(row)]).unwrap();
    assert_eq!(complete(db, source).unwrap(), Applied::Visible);
}

#[test]
fn rows_and_metadata_publish_together_only_after_explicit_completion() {
    let (mut db, source) = start();
    db.execute("UPDATE desktop_catalog_sources SET meta='old-meta'", [])
        .unwrap();
    begin(&mut db, &source, &metadata(), 130, 2).unwrap();
    assert_eq!(visible(&db), vec![item(999).row]);
    assert_eq!(meta(&db).as_deref(), Some("old-meta"));
    assert_eq!(
        page(&mut db, &source, 0, &(0..128).map(item).collect::<Vec<_>>()).unwrap(),
        Applied::Staging
    );
    page(&mut db, &source, 1, &[item(128), item(129)]).unwrap();
    assert_eq!(
        visible(&db),
        vec![item(999).row],
        "a last data page stays provisional"
    );
    assert!(!status(&db, "peer").unwrap().complete);
    complete(&mut db, &source).unwrap();
    let first = crate::page(&db, "peer", None).unwrap();
    assert_eq!(first.rows.len(), 128);
    assert!(first.complete);
    let last = crate::page(&db, "peer", first.next.as_ref()).unwrap();
    assert_eq!(last.rows.len(), 2);
    assert!(last.next.is_none());
    assert_eq!(
        serde_json::from_str::<Value>(&first.rows[0].json).unwrap(),
        item(0).row
    );
    assert_eq!(
        serde_json::from_str::<Value>(&meta(&db).unwrap()).unwrap(),
        metadata()
    );
    assert_eq!(collect(&mut db).unwrap(), Some(1));
    assert_eq!(status(&db, "peer").unwrap().stored_rows, 130);
}

#[test]
fn invalid_pages_and_incomplete_counts_retire_staging_without_replacing_display() {
    for case in 0..9 {
        let (mut db, source) = start();
        begin(&mut db, &source, &metadata(), 130, 2).unwrap();
        page(&mut db, &source, 0, &(0..128).map(item).collect::<Vec<_>>()).unwrap();
        let bad = match case {
            0 => complete(&mut db, &source),
            1 => page(&mut db, &source, 2, &[item(128), item(129)]),
            2 => page(&mut db, &source, 0, &[item(128), item(129)]),
            3 => page(&mut db, &source, 1, &[item(0), item(129)]),
            4 => page(&mut db, &source, 1, &[item(128), item(129), item(130)]),
            5 => {
                let mut bad = item(128);
                bad.key = "run:wrong".into();
                page(&mut db, &source, 1, &[bad])
            }
            6 => page(&mut db, &source, 1, &[]),
            7 => page(
                &mut db,
                &source,
                1,
                &(128..257).map(item).collect::<Vec<_>>(),
            ),
            _ => {
                let mut bad = item(128);
                bad.row["padding"] = Value::String("x".repeat(watch::MAX_FRAME_BYTES));
                page(&mut db, &source, 1, &[bad])
            }
        };
        assert!(bad.is_err(), "case {case}");
        assert_eq!(visible(&db), vec![item(999).row]);
        assert!(!status(&db, "peer").unwrap().complete);
        assert!(complete(&mut db, &source).is_err());
        assert_eq!(collect(&mut db).unwrap(), Some(128));
        assert_eq!(status(&db, "peer").unwrap().stored_rows, 1);
    }
}

#[test]
fn a_short_final_page_cannot_settle_a_larger_declared_count() {
    let (mut db, source) = start();
    begin(&mut db, &source, &metadata(), 2, 1).unwrap();
    page(&mut db, &source, 0, &[item(0)]).unwrap();
    assert!(complete(&mut db, &source).is_err());
    assert_eq!(visible(&db), vec![item(999).row]);
}

#[test]
fn an_empty_replacement_requires_its_one_empty_page() {
    let (mut db, source) = start();
    begin(&mut db, &source, &metadata(), 0, 1).unwrap();
    page(&mut db, &source, 0, &[]).unwrap();
    complete(&mut db, &source).unwrap();
    assert!(visible(&db).is_empty());
    assert!(status(&db, "peer").unwrap().complete);
    assert_eq!(collect(&mut db).unwrap(), Some(1));
}

#[test]
fn invalid_metadata_counts_and_generations_do_not_admit_a_source() {
    for (rows, pages) in [
        (0, 0),
        (0, 2),
        (1, 2),
        (129, 1),
        (MAX_ROWS + 1, 2000),
        (usize::MAX, usize::MAX),
    ] {
        let (mut db, source) = start();
        assert!(begin(&mut db, &source, &metadata(), rows, pages).is_err());
        assert!(status(&db, "peer").unwrap().complete);
        assert_eq!(totals(&db).unwrap().stored_rows, 1);
    }
    for metadata in [
        json!([]),
        json!({"sessions":[]}),
        json!({"large":"x".repeat(METADATA_BYTES)}),
    ] {
        let (mut db, source) = start();
        assert!(begin(&mut db, &source, &metadata, 1, 1).is_err());
        assert_eq!(totals(&db).unwrap().obsolete_generations, 0);
    }
}

#[test]
fn stale_legacy_callbacks_cannot_retire_a_newer_source_or_actor() {
    let (mut db, source) = start();
    replace(&mut db, &source, 1);
    let newer = Source {
        generation: 3,
        ..source.clone()
    };
    replace(&mut db, &newer, 2);
    assert!(begin(&mut db, &source, &metadata(), 1, 1).is_err());
    assert!(page(&mut db, &source, 0, &[item(3)]).is_err());
    assert!(complete(&mut db, &source).is_err());
    assert!(status(&db, "peer").unwrap().complete);
    assert_eq!(visible(&db), vec![item(2).row]);
    begin_service(&mut db, "actor-2").unwrap();
    let restarted = Source {
        service_epoch: "actor-2".into(),
        generation: 1,
        ..source
    };
    replace(&mut db, &restarted, 4);
    assert!(complete(&mut db, &newer).is_err());
    assert!(status(&db, "peer").unwrap().complete);
    assert_eq!(visible(&db), vec![item(4).row]);
}

#[test]
fn watch_and_legacy_callbacks_cannot_mix_and_upgrade_requires_a_new_generation() {
    let (mut db, source) = start();
    assert!(page(
        &mut db,
        &Source {
            generation: 1,
            ..source.clone()
        },
        0,
        &[item(0)]
    )
    .is_err());
    replace(&mut db, &source, 1);
    assert!(apply(&mut db, &source, &heartbeat(2)).is_err());
    assert!(!status(&db, "peer").unwrap().complete);
    let upgraded = Source {
        generation: 3,
        ..source.clone()
    };
    connect(&mut db, &upgraded, &hello()).unwrap();
    put(&mut db, &upgraded, &snapshot(2, 0, true, 1, vec![item(2)]));
    assert!(complete(&mut db, &source).is_err());
    assert!(status(&db, "peer").unwrap().complete);
    assert_eq!(visible(&db), vec![item(2).row]);
}

#[test]
fn restart_and_disconnect_abandon_partial_replacements_and_keep_visible_rows() {
    let (mut db, source) = start();
    begin(&mut db, &source, &metadata(), 2, 2).unwrap();
    page(&mut db, &source, 0, &[item(1)]).unwrap();
    begin_service(&mut db, "actor-2").unwrap();
    assert!(complete(&mut db, &source).is_err());
    assert_eq!(visible(&db), vec![item(999).row]);
    assert_eq!(collect(&mut db).unwrap(), Some(1));
    let source = Source {
        service_epoch: "actor-2".into(),
        generation: 1,
        ..source
    };
    begin(&mut db, &source, &metadata(), 1, 1).unwrap();
    page(&mut db, &source, 0, &[item(2)]).unwrap();
    disconnect(&mut db, &source).unwrap();
    assert!(complete(&mut db, &source).is_err());
    assert_eq!(visible(&db), vec![item(999).row]);
}

#[test]
fn legacy_rows_use_the_existing_storage_budget_and_transactional_counters() {
    let (mut db, source) = start();
    begin(&mut db, &source, &metadata(), 1, 1).unwrap();
    db.execute(
        "UPDATE desktop_catalog_totals SET bytes=?1",
        [super::super::MAX_BYTES],
    )
    .unwrap();
    assert!(page(&mut db, &source, 0, &[item(1)]).is_err());
    assert_eq!(visible(&db), vec![item(999).row]);
    let rows: i64 = db
        .query_row("SELECT rows FROM desktop_catalog_totals", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 1);
}

#[test]
fn a_large_legacy_replacement_has_bounded_pages_and_indexed_reads() {
    let (mut db, source) = start();
    begin(&mut db, &source, &metadata(), 100_000, 782).unwrap();
    for (index, offset) in (0..100_000).step_by(128).enumerate() {
        page(
            &mut db,
            &source,
            index,
            &(offset..(offset + 128).min(100_000))
                .map(item)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    }
    assert_eq!(status(&db, "peer").unwrap().visible_rows, 1);
    complete(&mut db, &source).unwrap();
    let mut cursor = None;
    let (mut rows, mut pages) = (0, 0);
    loop {
        let p = crate::page(&db, "peer", cursor.as_ref()).unwrap();
        assert!(p.rows.len() <= PAGE_ROWS);
        assert!(
            p.rows
                .iter()
                .map(|r| charge(&r.key, &r.json) as usize)
                .sum::<usize>()
                <= PAGE_BYTES
        );
        rows += p.rows.len();
        pages += 1;
        cursor = p.next;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!((rows, pages), (100_000, 782));
    let mut statement = db.prepare("EXPLAIN QUERY PLAN SELECT id,key,json,bytes FROM desktop_catalog_rows WHERE generation=1 AND id>0 AND id<=100000 ORDER BY id LIMIT 129").unwrap();
    let plan = statement
        .query_map([], |r| r.get::<_, String>(3))
        .unwrap()
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap()
        .join(" ");
    assert!(
        plan.contains("SEARCH") && !plan.contains("SCAN") && !plan.contains("TEMP"),
        "{plan}"
    );
    println!("indexed legacy rows={rows} pages={pages}; {plan}");
}
