use super::*;
use tempfile::TempDir;
fn scope() -> Scope {
    Scope {
        machine: "example-machine".into(),
        configuration: "private-config".into(),
        boot: "private-boot".into(),
    }
}
fn process(pid: u32) -> Process {
    Process {
        pid,
        birth: Birth::Linux(u64::from(pid) + 100),
    }
}
fn source(n: u32) -> Source {
    Source {
        kind: Kind::Native,
        key: format!("source-{n:08}"),
    }
}
fn publication(n: u32, revision: u64) -> Publication {
    let source = source(n);
    let process = process(n);
    Publication {
        anchors: vec![Anchor {
            process: process.clone(),
            role: Role::Agent,
            source: source.clone(),
            revision,
            fingerprint: "a".repeat(64),
            published_at: 1,
        }],
        edges: vec![Edge {
            process,
            role: Role::Agent,
            session_id: uuid::Uuid::from_u128(u128::from(n) + 1).to_string(),
            run: format!("run-{n}"),
            conversation: None,
            source,
            revision,
            fingerprint: "a".repeat(64),
            published_at: 1,
        }],
    }
}
fn change(n: u32, before: u64) -> Change {
    Change {
        id: uuid::Uuid::new_v4().to_string(),
        source: source(n),
        before,
        locator: None,
        publication: publication(n, before + 1),
    }
}
fn fixture() -> (TempDir, Store) {
    let tmp = tempfile::tempdir().unwrap();
    let db = Connection::open(tmp.path().join("index.sqlite3")).unwrap();
    (tmp, Store::create(db, scope()).unwrap())
}
fn publish(store: &mut Store, change: &Change) -> Ticket {
    let t = store.reserve(change).unwrap();
    store.settle(&t).unwrap();
    t
}
fn seal(store: &mut Store) {
    let b = store.rebuild().unwrap();
    store.scan_done(&b).unwrap();
    let end = store.stamp().unwrap().high_water;
    store.finish(&b, end).unwrap();
}
fn lookup(store: &mut Store, chain: &[Process]) -> Result<Lookup> {
    store.lookup(chain, Instant::now() + Duration::from_secs(2))
}

#[test]
fn an_empty_partial_generation_is_unavailable_never_certified_absence() {
    let (_t, mut s) = fixture();
    assert_eq!(lookup(&mut s, &[process(1)]).unwrap(), Lookup::Unavailable);
    let b = s.rebuild().unwrap();
    assert!(s.finish(&b, 0).is_err());
    s.scan_done(&b).unwrap();
    s.finish(&b, 0).unwrap();
    assert!(matches!(
        lookup(&mut s, &[process(1)]).unwrap(),
        Lookup::NoMatch { lookups: 1, .. }
    ));
    assert!(lookup(&mut s, &[]).is_err());
    assert!(lookup(&mut s, &(1..=65).map(process).collect::<Vec<_>>()).is_err());
    assert!(s.lookup(&[process(1)], Instant::now()).is_err());
}
#[test]
fn pending_intent_fences_absence_across_reopen_and_settlement_is_idempotent() {
    let (t, mut s) = fixture();
    seal(&mut s);
    let prior = s.stamp().unwrap();
    let c = change(1, 0);
    let ticket = s.reserve(&c).unwrap();
    assert_eq!(ticket, s.reserve(&c).unwrap());
    assert!(!s.unchanged(&prior).unwrap());
    assert_eq!(lookup(&mut s, &[process(9)]).unwrap(), Lookup::Unavailable);
    drop(s);
    let mut s = Store::open(
        Connection::open(t.path().join("index.sqlite3")).unwrap(),
        scope(),
    )
    .unwrap();
    assert_eq!(lookup(&mut s, &[process(9)]).unwrap(), Lookup::Unavailable);
    s.settle(&ticket).unwrap();
    let stamp = s.stamp().unwrap();
    s.settle(&ticket).unwrap();
    assert_eq!(s.stamp().unwrap(), stamp);
    assert!(
        matches!(lookup(&mut s,&[process(1)]).unwrap(),Lookup::Candidate{anchor,..} if anchor==c.publication.anchors[0])
    );
    assert!(matches!(
        lookup(&mut s, &[process(9)]).unwrap(),
        Lookup::NoMatch { .. }
    ));
}
#[test]
fn immutable_requests_and_source_revision_cas_preserve_the_original() {
    let (_t, mut s) = fixture();
    seal(&mut s);
    let c = change(1, 0);
    let t = publish(&mut s, &c);
    let before = s.stamp().unwrap();
    let mut altered = c.clone();
    altered.publication.anchors[0].published_at += 1;
    assert!(s.reserve(&altered).is_err());
    assert!(s.reserve(&change(1, 0)).is_err());
    assert!(
        s.settle(&Ticket {
            epoch: t.epoch.clone(),
            id: t.id.clone(),
            seq: t.seq + 1
        })
        .is_err()
    );
    assert_eq!(s.stamp().unwrap(), before);
    let retire = Change {
        id: uuid::Uuid::new_v4().to_string(),
        source: source(1),
        before: 1,
        locator: None,
        publication: Publication::default(),
    };
    publish(&mut s, &retire);
    s.settle(&t).unwrap();
    assert!(matches!(
        lookup(&mut s, &[process(1)]).unwrap(),
        Lookup::NoMatch { .. }
    ));
    assert!(s.reserve(&change(1, 1)).is_err());
}
#[test]
fn precise_birth_and_scope_prevent_pid_reuse_and_cross_namespace_adoption() {
    let (t, mut s) = fixture();
    seal(&mut s);
    publish(&mut s, &change(1, 0));
    let mut reused = process(1);
    reused.birth = Birth::Linux(999);
    assert!(matches!(
        lookup(&mut s, &[reused]).unwrap(),
        Lookup::NoMatch { .. }
    ));
    for field in 0..3 {
        let mut other = scope();
        match field {
            0 => other.machine.push('x'),
            1 => other.configuration.push('x'),
            _ => other.boot.push('x'),
        };
        assert!(
            Store::open(
                Connection::open(t.path().join("index.sqlite3")).unwrap(),
                other
            )
            .is_err()
        );
    }
    for birth in [
        Birth::Linux(0),
        Birth::Mac {
            seconds: 1,
            micros: 1_000_000,
        },
        Birth::Windows(0),
    ] {
        let mut bad = change(2, 0);
        bad.publication.anchors[0].process.birth = birth;
        assert!(s.reserve(&bad).is_err());
    }
}
#[test]
fn rebuild_cuts_retain_changes_and_old_pages_cannot_overwrite_or_resurrect() {
    let (_t, mut s) = fixture();
    let b = s.rebuild().unwrap();
    let snapshot = Snapshot {
        source: source(1),
        revision: 1,
        publication: publication(1, 1),
    };
    s.seed(&b, "a", std::slice::from_ref(&snapshot)).unwrap();
    let mut newer = change(1, 1);
    newer.publication = Publication::default();
    publish(&mut s, &newer);
    s.seed(&b, "b", &[snapshot]).unwrap();
    s.scan_done(&b).unwrap();
    assert!(s.finish(&b, b.high_water).is_err());
    let end = s.stamp().unwrap().high_water;
    s.finish(&b, end).unwrap();
    assert!(matches!(
        lookup(&mut s, &[process(1)]).unwrap(),
        Lookup::NoMatch { .. }
    ));
    let next = s.rebuild().unwrap();
    assert!(s.scan_done(&b).is_err());
    assert!(s.seed(&next, "a", &[]).is_ok());
    assert!(s.seed(&next, "a", &[]).is_err());
    let pending = s.reserve(&change(2, 0)).unwrap();
    s.scan_done(&next).unwrap();
    let end = s.stamp().unwrap().high_water;
    assert!(s.finish(&next, end).is_err());
    s.settle(&pending).unwrap();
    s.finish(&next, end).unwrap();
}
fn hosted(s: &mut Store, count: u32) -> (Build, Process) {
    let b = s.rebuild().unwrap();
    let p = process(999);
    let host = Source {
        kind: Kind::DeepSeekHost,
        key: "private-host".into(),
    };
    let anchor = Anchor {
        process: p.clone(),
        role: Role::Host,
        source: host.clone(),
        revision: 1,
        fingerprint: "b".repeat(64),
        published_at: 1,
    };
    s.seed(
        &b,
        "00000000",
        &[Snapshot {
            source: host,
            revision: 1,
            publication: Publication {
                anchors: vec![anchor],
                edges: vec![],
            },
        }],
    )
    .unwrap();
    for n in 1..=count {
        let src = Source {
            kind: Kind::DeepSeek,
            key: format!("hosted-{n:08}"),
        };
        let edge = Edge {
            process: p.clone(),
            role: Role::Hosted,
            session_id: uuid::Uuid::from_u128(u128::from(n)).to_string(),
            run: format!("host-run-{n}"),
            conversation: Some("c".repeat(1800)),
            source: src.clone(),
            revision: 1,
            fingerprint: "b".repeat(64),
            published_at: 1,
        };
        s.seed(
            &b,
            &format!("{n:08}"),
            &[Snapshot {
                source: src,
                revision: 1,
                publication: Publication {
                    anchors: vec![],
                    edges: vec![edge],
                },
            }],
        )
        .unwrap();
    }
    s.scan_done(&b).unwrap();
    s.finish(&b, b.high_water).unwrap();
    (b, p)
}
#[test]
fn a_shared_host_lookup_is_one_anchor_and_edges_are_byte_bounded_keyset_pages() {
    let (_t, mut s) = fixture();
    let (_b, p) = hosted(&mut s, 70);
    assert!(
        matches!(lookup(&mut s,std::slice::from_ref(&p)).unwrap(),Lookup::Candidate{anchor,lookups:1,..} if anchor.role==Role::Host)
    );
    let mut after = String::new();
    let mut rows = vec![];
    loop {
        let page = s.bindings(&p, &after, PAGE_ROWS).unwrap();
        assert!(page.bytes <= PAGE_BYTES);
        rows.extend(page.rows);
        if let Some(next) = page.next {
            assert!(next > after);
            after = next;
        } else {
            break;
        }
    }
    assert_eq!(rows.len(), 70);
    rows.sort_by(|a, b| a.session_id.cmp(&b.session_id));
    rows.dedup_by(|a, b| a.session_id == b.session_id);
    assert_eq!(rows.len(), 70);
    assert!(s.bindings(&p, "", PAGE_ROWS + 1).is_err());
    let mut forged = change(9, 0);
    forged.publication.anchors[0].process = p;
    assert!(s.reserve(&forged).is_ok());
    let ticket = s.reserve(&forged).unwrap();
    assert!(s.settle(&ticket).is_err());
    assert!(
        matches!(lookup(&mut s,&[process(999)]).unwrap(),Lookup::Candidate{anchor,..} if anchor.role==Role::Host)
    );
}
#[test]
fn journal_pages_are_bounded_and_pending_changes_cannot_be_hidden() {
    let (_t, mut s) = fixture();
    seal(&mut s);
    for n in 1..=8 {
        let mut c = change(n, 0);
        c.publication.edges[0].conversation = Some("x".repeat(1800));
        publish(&mut s, &c);
    }
    let t = s.reserve(&change(9, 0)).unwrap();
    let first = s.changes(0, 2).unwrap();
    assert_eq!(first.rows.len(), 2);
    assert!(first.bytes <= PAGE_BYTES);
    let second = s.changes(first.rows.last().unwrap().seq, 2).unwrap();
    assert!(second.rows[0].seq > first.rows.last().unwrap().seq);
    assert!(s.changes(0, 0).is_err());
    s.settle(&t).unwrap();
}
#[test]
fn malformed_rows_and_missing_pinned_indexes_never_fall_back_to_a_scan() {
    let (_t, mut s) = fixture();
    seal(&mut s);
    publish(&mut s, &change(1, 0));
    let mut bad = publication(1, 1).anchors[0].clone();
    bad.process.pid = 2;
    s.db.execute(
        "UPDATE process_anchors SET body=?1 WHERE pid=1",
        [serde_json::to_string(&bad).unwrap()],
    )
    .unwrap();
    assert!(lookup(&mut s, &[process(1)]).is_err());
    s.db.execute(
        "UPDATE process_anchors SET body=?1 WHERE pid=1",
        ["x".repeat(ENTRY_BYTES + 1)],
    )
    .unwrap();
    assert!(lookup(&mut s, &[process(1)]).is_err());
    s.db.execute_batch("DROP INDEX process_anchors_pid")
        .unwrap();
    assert!(lookup(&mut s, &[process(7)]).is_err());
}
#[test]
fn opening_never_initializes_repairs_or_resets_an_existing_schema() {
    let (t, s) = fixture();
    let path = t.path().join("index.sqlite3");
    drop(s);
    let db = Connection::open(&path).unwrap();
    db.execute("UPDATE process_meta SET schema=99 WHERE id=1", [])
        .unwrap();
    drop(db);
    assert!(Store::open(Connection::open(&path).unwrap(), scope()).is_err());
    assert!(Store::create(Connection::open(&path).unwrap(), scope()).is_err());
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row("SELECT schema FROM process_meta WHERE id=1", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        99
    );
    assert!(Store::open(Connection::open_in_memory().unwrap(), scope()).is_err());
}
#[test]
fn oversized_or_mismatched_publications_fail_before_any_reservation() {
    let (_t, mut s) = fixture();
    seal(&mut s);
    let before = s.stamp().unwrap();
    let mut c = change(1, 0);
    c.publication.anchors[0].source = source(2);
    assert!(s.reserve(&c).is_err());
    let mut c = change(1, 0);
    c.publication.edges[0].conversation = Some("x".repeat(ENTRY_BYTES * 2));
    assert!(s.reserve(&c).is_err());
    assert_eq!(before, s.stamp().unwrap());
}
#[test]
fn a_locked_or_full_store_refuses_without_evicting_claims_or_reserving_work() {
    let (t, mut s) = fixture();
    seal(&mut s);
    publish(&mut s, &change(1, 0));
    let before = s.stamp().unwrap();
    let other = Connection::open(t.path().join("index.sqlite3")).unwrap();
    other.execute_batch("BEGIN EXCLUSIVE").unwrap();
    assert!(s.reserve(&change(2, 0)).is_err());
    other.execute_batch("ROLLBACK").unwrap();
    assert_eq!(before, s.stamp().unwrap());
    // Retained journal quota: deliberately fill only private fixture rows.
    s.db.execute_batch(&format!("WITH RECURSIVE n(x) AS (SELECT 2 UNION ALL SELECT x+1 FROM n WHERE x<{JOURNAL_ROWS}) INSERT INTO process_changes(seq,id,source,body,settled) SELECT x,'fixture-'||x,'fixture','{{}}',1 FROM n; UPDATE process_meta SET high_water={JOURNAL_ROWS} WHERE id=1;")).unwrap();
    assert!(s.reserve(&change(2, 0)).is_err());
    assert!(matches!(
        lookup(&mut s, &[process(1)]).unwrap(),
        Lookup::Candidate { .. }
    ));
}
#[test]
fn scope_changes_and_publication_during_the_final_bracket_refuse_the_read() {
    let (t, mut s) = fixture();
    seal(&mut s);
    let path = t.path().join("index.sqlite3");
    AFTER_LOOKUP.with(|c| {
        *c.borrow_mut() = Some(Box::new(move || {
            let db = Connection::open(path).unwrap();
            db.execute("UPDATE process_meta SET revision=revision+1 WHERE id=1", [])
                .unwrap();
        }))
    });
    assert_eq!(lookup(&mut s, &[process(8)]).unwrap(), Lookup::Unavailable);
    let mut other = scope();
    other.configuration.push('x');
    s.db.execute(
        "UPDATE process_meta SET scope=?1 WHERE id=1",
        [serde_json::to_string(&other).unwrap()],
    )
    .unwrap();
    assert!(lookup(&mut s, &[process(8)]).is_err());
    assert!(s.reserve(&change(8, 0)).is_err());
}
#[test]
fn scan_catchup_and_preexisting_revision_guards_each_preserve_newer_bindings() {
    let (_t, mut s) = fixture();
    let b = s.rebuild().unwrap();
    let mut c = change(1, 0);
    c.publication.anchors[0].fingerprint = "b".repeat(64);
    publish(&mut s, &c);
    s.seed(
        &b,
        "a",
        &[Snapshot {
            source: source(1),
            revision: 1,
            publication: publication(1, 1),
        }],
    )
    .unwrap();
    s.scan_done(&b).unwrap();
    let end = s.stamp().unwrap().high_water;
    s.finish(&b, end).unwrap();
    assert!(
        matches!(lookup(&mut s,&[process(1)]).unwrap(),Lookup::Candidate{anchor,..} if anchor.fingerprint=="b".repeat(64))
    );
    let retired = Change {
        id: uuid::Uuid::new_v4().to_string(),
        source: source(1),
        before: 1,
        locator: None,
        publication: Publication::default(),
    };
    publish(&mut s, &retired);
    let b = s.rebuild().unwrap();
    s.seed(
        &b,
        "a",
        &[Snapshot {
            source: source(1),
            revision: 1,
            publication: publication(1, 1),
        }],
    )
    .unwrap();
    s.scan_done(&b).unwrap();
    s.finish(&b, b.high_water).unwrap();
    assert!(matches!(
        lookup(&mut s, &[process(1)]).unwrap(),
        Lookup::NoMatch { .. }
    ));
}
#[test]
fn exact_production_queries_use_pinned_point_and_keyset_indexes() {
    let (_t, s) = fixture();
    for (sql, args, index) in [
        (
            schema::ANCHOR,
            vec![rusqlite::types::Value::Integer(1)],
            "process_anchors_pid",
        ),
        (
            schema::EDGES,
            vec![
                1.into(),
                "birth".to_owned().into(),
                String::new().into(),
                3.into(),
            ],
            "process_edges_key",
        ),
        (
            schema::CHANGES,
            vec![0.into(), 3.into()],
            "process_changes_page",
        ),
    ] {
        let mut q = s.db.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
        let plan = q
            .query_map(rusqlite::params_from_iter(args), |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert!(
            plan.iter()
                .any(|v| v.contains("SEARCH") && v.contains(index)),
            "{plan:?}"
        );
        assert!(
            !plan
                .iter()
                .any(|v| v.contains("TEMP B-TREE") || v.starts_with("SCAN")),
            "{plan:?}"
        );
    }
}
#[test]
fn progress_is_bounded_and_repeated_finish_does_not_change_the_snapshot() {
    let (_t, mut s) = fixture();
    assert!(
        s.scan_done(&Build {
            epoch: s.stamp().unwrap().epoch,
            generation: 0,
            high_water: 0
        })
        .is_err()
    );
    let b = s.rebuild().unwrap();
    s.seed(&b, "a", &[]).unwrap();
    let progress = s.progress().unwrap();
    assert_eq!(progress.build, b);
    assert_eq!(progress.after, "a");
    assert!(!progress.scanned);
    s.scan_done(&b).unwrap();
    s.finish(&b, 0).unwrap();
    let stamp = s.stamp().unwrap();
    s.finish(&b, 0).unwrap();
    assert_eq!(stamp, s.stamp().unwrap());
}
#[test]
fn identical_counters_from_another_store_never_match_the_original_stamp() {
    let (_a, mut first) = fixture();
    let (_b, mut second) = fixture();
    seal(&mut first);
    seal(&mut second);
    assert!(!first.unchanged(&second.stamp().unwrap()).unwrap());
}
#[test]
fn failed_projection_publication_rolls_back_before_settling_the_original_intent() {
    let (t, mut s) = fixture();
    seal(&mut s);
    publish(&mut s, &change(1, 0));
    let original = publication(1, 1).anchors[0].clone();
    let mut c = change(1, 1);
    c.publication.anchors[0].fingerprint = "c".repeat(64);
    let ticket = s.reserve(&c).unwrap();
    s.db.execute_batch("CREATE TRIGGER private_abort BEFORE INSERT ON process_edges BEGIN SELECT RAISE(ABORT,'private fixture cut'); END;").unwrap();
    assert!(s.settle(&ticket).is_err());
    assert!(
        matches!(lookup(&mut s,&[process(1)]).unwrap(),Lookup::Candidate{anchor,..} if anchor==original)
    );
    assert_eq!(lookup(&mut s, &[process(8)]).unwrap(), Lookup::Unavailable);
    drop(s);
    let mut s = Store::open(
        Connection::open(t.path().join("index.sqlite3")).unwrap(),
        scope(),
    )
    .unwrap();
    s.db.execute_batch("DROP TRIGGER private_abort").unwrap();
    s.settle(&ticket).unwrap();
    assert!(
        matches!(lookup(&mut s,&[process(1)]).unwrap(),Lookup::Candidate{anchor,..} if anchor.fingerprint=="c".repeat(64))
    );
}
#[test]
fn scan_page_conflicts_roll_back_rows_and_cursor_together() {
    let (_t, mut s) = fixture();
    let b = s.rebuild().unwrap();
    let first = Snapshot {
        source: source(1),
        revision: 1,
        publication: publication(1, 1),
    };
    let mut second = Snapshot {
        source: source(2),
        revision: 1,
        publication: publication(2, 1),
    };
    second.publication.anchors[0].process = process(1);
    assert!(s.seed(&b, "a", &[first.clone(), second]).is_err());
    assert_eq!(s.progress().unwrap().after, "");
    assert_eq!(lookup(&mut s, &[process(1)]).unwrap(), Lookup::Unavailable);
    s.seed(&b, "a", &[first]).unwrap();
}
#[test]
fn an_original_intent_ticket_cannot_settle_another_store_with_the_same_request() {
    let (_a, mut first) = fixture();
    let (_b, mut second) = fixture();
    let c = change(1, 0);
    let one = first.reserve(&c).unwrap();
    let two = second.reserve(&c).unwrap();
    assert!(second.settle(&one).is_err());
    second.settle(&two).unwrap();
    first.settle(&one).unwrap();
    let a = first.rebuild().unwrap();
    let b = second.rebuild().unwrap();
    assert!(first.seed(&b, "a", &[]).is_err());
    first.seed(&a, "a", &[]).unwrap();
}
#[test]
fn a_preexisting_page_size_cannot_silently_expand_the_new_store_quota() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("PRAGMA page_size=8192;CREATE TABLE discarded(x);DROP TABLE discarded;")
        .unwrap();
    assert!(Store::create(db, scope()).is_err());
}
