use super::*;

struct Fixture {
    _temp: tempfile::TempDir,
    reader: Reader,
}

#[cfg(unix)]
#[test]
fn reader_drop_keeps_the_live_writer_lock_on_fresh_open_and_restart() {
    let mut f = Fixture::new();
    drop(f.rows(1));
    let path = f.reader.root.join("journal.sqlite3");
    for _ in 0..2 {
        let writer = Connection::open(&path).unwrap();
        writer
            .query_row("SELECT count(*) FROM attempts", [], |r| r.get::<_, i64>(0))
            .unwrap();
        crate::approval_log::lock_tests::assert_lock(&path, true);
        assert_eq!(f.send(&f.query())["accepted"], true);
        crate::approval_log::lock_tests::assert_lock(&path, true);
        drop(f.reader.journal.take());
        crate::approval_log::lock_tests::assert_lock(&path, true);
        drop(writer);
        crate::approval_log::lock_tests::assert_lock(&path, false);
    }
}

#[cfg(unix)]
#[test]
fn a_failed_reader_check_keeps_the_live_writer_lock_after_restart() {
    let mut f = Fixture::new();
    drop(f.rows(1));
    let path = f.reader.root.join("journal.sqlite3");
    for _ in 0..2 {
        let writer = Connection::open(&path).unwrap();
        let first = f.send(&f.query());
        assert_eq!(first["accepted"], true);
        writer.pragma_update(None, "user_version", 0).unwrap();
        let mut stale = f.query();
        stale["journal"] = first["journal"].clone();
        stale["through"] = first["result"]["through"].clone();
        assert!(serde_json::from_value::<Request>(stale.clone())
            .unwrap()
            .valid());
        let refused = f.send(&stale);
        assert_eq!(refused["accepted"], false);
        assert_eq!(refused["code"], "approval_audit_unavailable");
        assert!(
            f.reader.journal.is_none(),
            "failed check must drop the original reader"
        );
        crate::approval_log::lock_tests::assert_lock(&path, true);
        writer.pragma_update(None, "user_version", 1).unwrap();
        drop(writer);
        crate::approval_log::lock_tests::assert_lock(&path, false);
    }
}

#[cfg(unix)]
#[test]
fn opening_the_writer_preserves_an_already_cached_reader_lock() {
    let mut f = Fixture::new();
    drop(f.rows(1));
    let path = f.reader.root.join("journal.sqlite3");
    for _ in 0..2 {
        assert_eq!(f.send(&f.query())["accepted"], true);
        crate::approval_log::lock_tests::assert_lock(&path, true);
        let writer = crate::approval_log::open(&f.reader.root).unwrap();
        crate::approval_log::lock_tests::assert_lock(&path, true);
        drop(writer);
        crate::approval_log::lock_tests::assert_lock(&path, true);
        drop(f.reader.journal.take());
        crate::approval_log::lock_tests::assert_lock(&path, false);
    }
}
impl Fixture {
    fn new() -> Self {
        let temp = crate::host::staging::private_tempdir();
        Self {
            reader: Reader {
                root: temp.path().join("approval-audit"),
                configuration: "a".repeat(64),
                epoch: uuid::Uuid::new_v4().to_string(),
                journal: None,
            },
            _temp: temp,
        }
    }
    fn query(&self) -> Value {
        json!({"op":"approval_audit_list","request_id":uuid::Uuid::new_v4().to_string(),
            "configuration":self.reader.configuration,"epoch":self.reader.epoch})
    }
    fn rows(&self, count: usize) -> Connection {
        let db = crate::approval_log::open(&self.reader.root).unwrap();
        for n in 1..=count {
            db.execute(
                "INSERT INTO attempts(identity,request_id,at,host,session,provider,prompt,question)
                VALUES(?1,?2,?3,?4,'agent/project/fixture','claude','Fixture question',?5)",
                params![
                    format!("identity-{n}"),
                    uuid::Uuid::new_v4().to_string(),
                    n as i64,
                    if n % 2 == 0 { "a.example" } else { "b.example" },
                    json!({"question":{"question_id":format!("fixture-{n}")},"answers":[]})
                        .to_string()
                ],
            )
            .unwrap();
        }
        db
    }
    fn send(&mut self, request: &Value) -> Value {
        let value = self.reader.request(request);
        let typed: Request = serde_json::from_value(request.clone()).unwrap();
        assert!(typed.matches(&value), "bounded selected-service envelope");
        value
    }
}
#[test]
fn absent_journal_is_empty_without_creating_or_authorizing_anything() {
    let mut f = Fixture::new();
    let reply = f.send(&f.query());
    assert_eq!(reply["accepted"], true);
    assert_eq!(reply["result"]["entries"], json!([]));
    assert_eq!(reply["journal"], Value::Null);
    assert!(!f.reader.root.exists());
}
#[test]
fn indexed_pages_hold_their_high_water_and_detail_uses_the_same_journal() {
    let mut f = Fixture::new();
    let db = f.rows(123);
    db.execute("UPDATE attempts SET question=json_set(question,'$.categories',json('[\"shell\",\"paid\"]')) WHERE id=123", []).unwrap();
    let mut request = f.query();
    let first = f.send(&request);
    assert_eq!(first["result"]["entries"].as_array().unwrap().len(), 50);
    assert_eq!(first["result"]["entries"][0]["id"], 123);
    assert_eq!(first["result"]["entries"][0]["categories"], json!(["shell", "paid"]));
    assert!(first["result"]["entries"][1]["categories"].is_null());
    request["journal"] = first["journal"].clone();
    request["through"] = first["result"]["through"].clone();
    db.execute(
        "INSERT INTO attempts(identity,request_id,at,host,session,provider,prompt,question)
        VALUES('later','later',999,'a.example','later','claude','later','{}')",
        [],
    )
    .unwrap();
    let mut ids = first["result"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_i64().unwrap())
        .collect::<Vec<_>>();
    let mut page = first.clone();
    while !page["result"]["next"].is_null() {
        request["before"] = page["result"]["next"].clone();
        page = f.send(&request);
        ids.extend(
            page["result"]["entries"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r["id"].as_i64().unwrap()),
        );
    }
    assert_eq!(ids, (1..=123).rev().collect::<Vec<_>>());
    let detail = json!({"op":"approval_audit_detail","request_id":uuid::Uuid::new_v4().to_string(),
        "configuration":f.reader.configuration,"epoch":f.reader.epoch,"journal":first["journal"],"id":123});
    let reply = f.send(&detail);
    assert_eq!(reply["result"]["question"]["question_id"], "fixture-123");
    assert_eq!(reply["result"]["categories"], json!(["shell", "paid"]));
    let mut filtered = f.query();
    filtered["host"] = json!("a.example");
    let reply = f.send(&filtered);
    assert_eq!(reply["accepted"], true);
    assert!(reply["result"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["host"] == "a.example"));
    let journal = f.reader.journal.as_ref().unwrap();
    assert!(journal.db.execute("DELETE FROM attempts", []).is_err());
    assert!(journal
        .db
        .execute_batch("CREATE TABLE forbidden(value)")
        .is_err());
    assert_eq!(
        db.query_row("SELECT count(*) FROM attempts", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        124
    );
}
#[test]
fn stale_scope_and_unknown_or_mutating_shapes_never_read_the_journal() {
    let mut f = Fixture::new();
    let mut request = f.query();
    request["epoch"] = json!(uuid::Uuid::new_v4().to_string());
    assert_eq!(
        f.reader.request(&request)["code"],
        "approval_audit_scope_changed"
    );
    request = f.query();
    request["path"] = json!("/not/selected");
    assert_eq!(f.reader.request(&request)["accepted"], false);
    request = f.query();
    request["op"] = json!("approval_audit_begin");
    assert_eq!(f.reader.request(&request)["accepted"], false);
    assert!(f.reader.journal.is_none());
    assert!(!f.reader.root.exists());
}
#[test]
fn oversized_records_or_missing_indexes_refuse_without_truncation_or_repair() {
    let mut f = Fixture::new();
    let db = f.rows(1);
    let first = f.send(&f.query());
    db.execute(
        "UPDATE attempts SET question=?1",
        ["x".repeat(wire::REPLY_BYTES)],
    )
    .unwrap();
    let detail = json!({"op":"approval_audit_detail","request_id":uuid::Uuid::new_v4().to_string(),
        "configuration":f.reader.configuration,"epoch":f.reader.epoch,"journal":first["journal"],"id":1});
    assert_eq!(f.send(&detail)["accepted"], false);
    db.execute("DROP INDEX attempts_host", []).unwrap();
    let mut request = f.query();
    request["host"] = json!("a.example");
    assert_eq!(f.send(&request)["accepted"], false);
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='attempts_host'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}
#[test]
fn an_unknown_schema_is_not_migrated_by_an_audit_read() {
    let mut f = Fixture::new();
    let db = f.rows(1);
    db.execute_batch("PRAGMA user_version=0").unwrap();
    assert_eq!(f.send(&f.query())["accepted"], false);
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn sqlite_connection_must_match_the_original_guard_even_if_its_path_is_unchanged() {
    let mut f = Fixture::new();
    let _original = f.rows(1);
    let first = f.send(&f.query());
    let other = Fixture::new();
    let _other = other.rows(2);
    // Deterministic open-race control: the guard still owns A, while SQLite
    // opened B. A path-only check of the separate guard would miss this.
    f.reader.journal.as_mut().unwrap().db = Connection::open_with_flags(
        other.reader.root.join("journal.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let detail = json!({"op":"approval_audit_detail","request_id":uuid::Uuid::new_v4().to_string(),
        "configuration":f.reader.configuration,"epoch":f.reader.epoch,"journal":first["journal"],"id":1});
    assert_eq!(f.send(&detail)["accepted"], false);
}
#[cfg(unix)]
#[test]
fn reader_allows_symlinked_ancestors_but_not_the_journal_leaf() {
    use std::os::unix::fs::symlink;
    let mut f = Fixture::new();
    let real = f._temp.path().join("real");
    f.reader.root = real.join("approval-audit");
    drop(f.rows(1));
    let link = f._temp.path().join("link");
    symlink(&real, &link).unwrap();
    f.reader.root = link.join("approval-audit");
    let path = f.reader.root.join("journal.sqlite3");
    for _ in 0..2 {
        assert_eq!(f.send(&f.query())["accepted"], true);
        crate::approval_log::lock_tests::assert_lock(&path, true);
        drop(f.reader.journal.take());
        crate::approval_log::lock_tests::assert_lock(&path, false);
    }
    let target = real.join("retained.sqlite3");
    fs::rename(&path, &target).unwrap();
    let original = fs::read(&target).unwrap();
    symlink(&target, &path).unwrap();
    let refused = f.send(&f.query());
    assert_eq!(refused["accepted"], false);
    assert_eq!(refused["code"], "approval_audit_unavailable");
    assert!(f.reader.journal.is_none());
    assert_eq!(fs::read(&target).unwrap(), original);
}

#[cfg(unix)]
#[test]
fn restoring_the_checked_path_cannot_hide_a_different_sqlite_file() {
    let f = Fixture::new();
    let other = Fixture::new();
    for (fixture, count) in [(&f, 1), (&other, 2)] {
        let db = fixture.rows(count);
        db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode=DELETE;")
            .unwrap();
    }
    let root = platform::canonical(&f.reader.root).unwrap();
    let path = root.join("journal.sqlite3");
    let original = root.join("original.sqlite3");
    let replacement = platform::canonical(&other.reader.root)
        .unwrap()
        .join("journal.sqlite3");
    let checked = platform::file_identity(&path).unwrap();
    // A is checked, B is opened at the SAME filename, then A is restored.
    // Both stat checks see A; only SQLite can identify its retained B.
    fs::rename(&path, &original).unwrap();
    fs::rename(&replacement, &path).unwrap();
    let db = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM attempts", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
    fs::rename(&path, &replacement).unwrap();
    fs::rename(&original, &path).unwrap();
    let restored = platform::file_identity(&path).unwrap();
    assert_eq!(
        (restored.device, restored.inode),
        (checked.device, checked.inode)
    );
    let journal = Journal {
        identity: (checked.device, checked.inode),
        token: uuid::Uuid::new_v4().to_string(),
        db,
    };
    assert_eq!(
        journal.check(&f.reader.root).unwrap_err().to_string(),
        "audit SQLite file changed"
    );
}

#[cfg(unix)]
#[test]
fn replacement_cannot_rebind_an_old_detail_id_or_cursor() {
    let mut f = Fixture::new();
    let db = f.rows(1);
    db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode=DELETE;")
        .unwrap();
    drop(db);
    let first = f.send(&f.query());
    fs::rename(
        f.reader.root.join("journal.sqlite3"),
        f.reader.root.join("retained.sqlite3"),
    )
    .unwrap();
    let db = f.rows(2);
    let detail = json!({"op":"approval_audit_detail","request_id":uuid::Uuid::new_v4().to_string(),
        "configuration":f.reader.configuration,"epoch":f.reader.epoch,"journal":first["journal"],"id":1});
    assert_eq!(f.send(&detail)["accepted"], false);
    let next = f.send(&f.query());
    assert_eq!(next["accepted"], true);
    assert_ne!(next["journal"], first["journal"]);
    assert_eq!(
        db.query_row("SELECT count(*) FROM attempts", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
}

/// Task #23 (team ede724c4): Task #349 raised the journal to schema 2 by adding
/// columns; this view reads that journal and one an older hgs left at 1, never
/// an unknown schema (0 or a later one).
#[test]
fn the_journal_reads_at_schema_one_or_two_only() {
    let mut f = Fixture::new();
    let db = f.rows(1);
    let version = |db: &Connection| db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0)).unwrap();
    assert_eq!(version(&db), 2, "the writer's current schema");
    assert_eq!(f.send(&f.query())["accepted"], true);
    for (schema, accepted) in [(1, true), (3, false)] {
        db.execute_batch(&format!("PRAGMA user_version={schema}")).unwrap();
        drop(f.reader.journal.take());
        assert_eq!(f.send(&f.query())["accepted"], accepted, "schema {schema}");
    }
}