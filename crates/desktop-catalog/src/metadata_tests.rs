use super::*;
use crate::tests::{hello, setup};
use serde_json::json;

fn hello_bytes() -> i64 {
    let Body::Hello(value) = hello().body else {
        unreachable!()
    };
    serde_json::to_vec(&value).unwrap().len() as i64
}

#[test]
fn metadata_counters_track_utf8_updates_nulls_and_source_deletion() {
    let (mut db, source) = setup();
    assert_eq!(totals(&db).unwrap().stored_bytes, hello_bytes());
    let value = json!({"label":"example-λ-🦀"}).to_string();
    db.execute("UPDATE desktop_catalog_sources SET meta=?1", [&value])
        .unwrap();
    assert_eq!(
        totals(&db).unwrap().stored_bytes,
        hello_bytes() + value.len() as i64
    );
    db.execute("UPDATE desktop_catalog_sources SET meta=NULL", [])
        .unwrap();
    assert_eq!(totals(&db).unwrap().stored_bytes, hello_bytes());
    let next = Source {
        generation: 2,
        ..source
    };
    let metadata = json!({"label":"example-λ"});
    let bytes = metadata.to_string().len() as i64;
    legacy::begin(&mut db, &next, &metadata, 0, 1).unwrap();
    assert_eq!(totals(&db).unwrap().stored_bytes, bytes);
    legacy::page(&mut db, &next, 0, &[]).unwrap();
    legacy::complete(&mut db, &next).unwrap();
    assert_eq!(
        totals(&db).unwrap().stored_bytes,
        bytes,
        "completion moves without duplicating metadata"
    );
    let staged = Source {
        generation: 3,
        ..next
    };
    legacy::begin(&mut db, &staged, &metadata, 0, 1).unwrap();
    assert_eq!(totals(&db).unwrap().stored_bytes, bytes * 2);
    disconnect(&mut db, &staged).unwrap();
    assert_eq!(
        totals(&db).unwrap().stored_bytes,
        bytes,
        "abandon keeps only last-known metadata"
    );
    db.execute("DELETE FROM desktop_catalog_sources", [])
        .unwrap();
    assert_eq!(totals(&db).unwrap().stored_bytes, 0);
}

#[test]
fn schema_five_metadata_migration_is_exact_and_idempotent() {
    let (mut db, _) = setup();
    db.execute_batch(
        "DROP TRIGGER desktop_catalog_source_insert;
        DROP TRIGGER desktop_catalog_source_update;
        DROP TRIGGER desktop_catalog_source_delete;
        UPDATE desktop_catalog_totals SET bytes=0;",
    )
    .unwrap();
    let value = json!({"retained":"example-λ"}).to_string();
    db.execute("UPDATE desktop_catalog_sources SET meta=?1", [&value])
        .unwrap();
    let expected = hello_bytes() + value.len() as i64;
    for _ in 0..2 {
        let tx = db.transaction().unwrap();
        migrate_metadata(&tx).unwrap();
        tx.commit().unwrap();
        assert_eq!(totals(&db).unwrap().stored_bytes, expected);
    }
    db.execute_batch("DROP TRIGGER desktop_catalog_source_update")
        .unwrap();
    assert!(
        migrate_metadata(&db).is_err(),
        "a partial migration must fail closed"
    );
    assert_eq!(totals(&db).unwrap().stored_bytes, expected);
}

#[test]
fn metadata_migration_refuses_excess_sources_before_aggregate_read() {
    let (db, _) = setup();
    db.execute_batch(
        "DROP TRIGGER desktop_catalog_source_insert;
        DROP TRIGGER desktop_catalog_source_update;
        DROP TRIGGER desktop_catalog_source_delete;",
    )
    .unwrap();
    for n in 0..MAX_SOURCES {
        db.execute("INSERT INTO desktop_catalog_sources(computer,epoch,incarnation,generation,stream,seq,hello)
            VALUES(?1,'old',1,1,'old-stream',1,'{}')", [format!("example-{n}")]).unwrap();
    }
    let before = totals(&db).unwrap().stored_bytes;
    assert!(migrate_metadata(&db).is_err());
    assert_eq!(totals(&db).unwrap().stored_bytes, before);
}

#[test]
fn metadata_only_pressure_refuses_legacy_and_watch_admission_transactionally() {
    let (mut db, source) = setup();
    let before = hello_bytes();
    db.execute("UPDATE desktop_catalog_totals SET bytes=?1", [MAX_BYTES])
        .unwrap();
    let next = Source {
        generation: 2,
        ..source.clone()
    };
    let value = json!({"large":"x".repeat(32 << 10)});
    assert!(legacy::begin(&mut db, &next, &value, 0, 1).is_err());
    assert_eq!(totals(&db).unwrap().stored_bytes, MAX_BYTES);
    assert_eq!(totals(&db).unwrap().obsolete_generations, 0);
    let mut frame = hello();
    if let Body::Hello(h) = &mut frame.body {
        h.host.push_str("-larger");
    }
    assert!(connect(&mut db, &next, &frame).is_err());
    let stored: String = db
        .query_row("SELECT hello FROM desktop_catalog_sources", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(stored.len() as i64, before);
    // Existing watch metadata updates use the same budget, with rollback.
    let meta = Frame {
        v: 1,
        seq: 2,
        body: Body::Snapshot(watch::Snapshot {
            topic: topic::CATALOG_META.into(),
            key: None,
            items: None,
            value: Some(value),
            page: 0,
            last: true,
            cursor: 1,
        }),
    };
    assert!(apply(&mut db, &source, &meta).is_err());
    let stored: Option<String> = db
        .query_row("SELECT meta FROM desktop_catalog_sources", [], |r| r.get(0))
        .unwrap();
    assert!(stored.is_none());
    assert_eq!(totals(&db).unwrap().stored_bytes, MAX_BYTES);
}
