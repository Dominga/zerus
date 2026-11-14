use super::*;
use std::cell::RefCell;

fn fixture() -> (tempfile::TempDir, Store, Change) {
    let root = tempfile::tempdir().unwrap();
    let db = Connection::open(root.path().join("index.sqlite3")).unwrap();
    let scope = Scope {
        machine: "private-machine".into(),
        configuration: "private-state".into(),
        boot: "private-boot".into(),
    };
    let store = Store::create(db, scope).unwrap();
    let source = Source {
        kind: Kind::Native,
        key: "original-session".into(),
    };
    let process = Process {
        pid: 42,
        birth: Birth::Linux(100),
    };
    let publication = Publication {
        anchors: vec![Anchor {
            process: process.clone(),
            role: Role::Agent,
            source: source.clone(),
            revision: 1,
            fingerprint: "a".repeat(64),
            published_at: 1,
        }],
        edges: vec![Edge {
            process,
            role: Role::Agent,
            session_id: uuid::Uuid::new_v4().to_string(),
            run: "original-run".into(),
            conversation: None,
            source: source.clone(),
            revision: 1,
            fingerprint: "a".repeat(64),
            published_at: 1,
        }],
    };
    let change = Change {
        id: uuid::Uuid::new_v4().to_string(),
        source,
        before: 0,
        locator: None,
        publication,
    };
    change.validate().unwrap();
    (root, store, change)
}

#[test]
fn intent_is_durable_before_the_source_can_be_published() {
    let (root, mut store, change) = fixture();
    let inspector = Connection::open(root.path().join("index.sqlite3")).unwrap();
    publish(
        Some(&mut store),
        &change,
        |witness| {
            let pending: u64 = inspector.query_row(
                "SELECT count(*) FROM process_changes WHERE settled=0",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(
                pending, 1,
                "the source must never precede its durable intent"
            );
            assert!(witness.epoch.is_some());
            assert_eq!(witness.intent.as_deref(), Some(change.id.as_str()));
            std::fs::write(
                root.path().join("record.json"),
                serde_json::to_vec(&witness)?,
            )?;
            Ok(())
        },
        |_| {},
    )
    .unwrap();
    let mut reopened = Store::open(
        Connection::open(root.path().join("index.sqlite3")).unwrap(),
        store.scope.clone(),
    )
    .unwrap();
    let result = reopened
        .lookup(
            &[change.publication.anchors[0].process.clone()],
            Instant::now() + Duration::from_secs(1),
        )
        .unwrap();
    assert!(
        matches!(result, Lookup::Candidate { .. }),
        "the original published binding must be projected"
    );
    assert!(reopened.changes(0, 10).unwrap().rows[0].settled);
}

#[test]
fn source_failure_is_preserved_and_leaves_a_pending_intent() {
    let (root, mut store, change) = fixture();
    let file = root.path().join("record.json");
    std::fs::write(&file, b"original source").unwrap();
    let result = publish::<()>(
        Some(&mut store),
        &change,
        |_| anyhow::bail!("original source refusal"),
        |_| {},
    );
    assert_eq!(result.unwrap_err().to_string(), "original source refusal");
    assert_eq!(std::fs::read(&file).unwrap(), b"original source");
    let rows = store.changes(0, 10).unwrap().rows;
    assert_eq!(
        rows.len(),
        1,
        "the abandoned intent survives reopening/reconciliation"
    );
    assert!(!rows[0].settled);
}

#[test]
fn full_index_never_blocks_the_original_write_and_reports_coverage_loss() {
    let (root, mut store, change) = fixture();
    store
        .db
        .execute(
            "UPDATE process_meta SET high_water=?1 WHERE id=1",
            [JOURNAL_ROWS],
        )
        .unwrap();
    let losses = RefCell::new(Vec::new());
    let returned = publish(
        Some(&mut store),
        &change,
        |witness| {
            assert_eq!(witness, Witness::lost());
            std::fs::write(root.path().join("record.json"), b"original success")?;
            Ok(73)
        },
        |reason| losses.borrow_mut().push(reason.to_owned()),
    )
    .unwrap();
    assert_eq!(returned, 73);
    assert_eq!(
        std::fs::read(root.path().join("record.json")).unwrap(),
        b"original success"
    );
    assert_eq!(
        losses.borrow().len(),
        1,
        "an unreserved write must leave an explicit gap"
    );
}

#[test]
fn unavailable_index_and_failed_gap_marker_cannot_change_source_success() {
    let (root, _store, change) = fixture();
    let mut attempted = 0;
    let value = publish(
        None,
        &change,
        |witness| {
            assert_eq!(witness, Witness::lost());
            std::fs::write(
                root.path().join("record.json"),
                b"source remains authoritative",
            )?;
            Ok("original receipt")
        },
        |_| {
            attempted += 1;
            assert!(std::fs::write(root.path().join("missing-parent/lost"), b"gap").is_err());
        },
    )
    .unwrap();
    assert_eq!(attempted, 1);
    assert_eq!(value, "original receipt");
}

#[test]
fn settlement_failure_keeps_the_published_source_and_pending_evidence() {
    let (root, mut store, change) = fixture();
    // Refuse only the projection transaction, after the intent is durable.
    store.db.execute_batch("CREATE TRIGGER refuse_projection BEFORE INSERT ON process_anchors BEGIN SELECT RAISE(ABORT,'private settlement cut'); END;").unwrap();
    let losses = RefCell::new(Vec::new());
    let value = publish(
        Some(&mut store),
        &change,
        |witness| {
            std::fs::write(
                root.path().join("record.json"),
                serde_json::to_vec(&witness)?,
            )?;
            Ok("source committed")
        },
        |reason| losses.borrow_mut().push(reason.to_owned()),
    )
    .unwrap();
    assert_eq!(value, "source committed");
    let witness: Witness =
        serde_json::from_slice(&std::fs::read(root.path().join("record.json")).unwrap()).unwrap();
    assert_eq!(witness.intent.as_deref(), Some(change.id.as_str()));
    assert_eq!(losses.borrow().len(), 1);
    let reopened = Store::open(
        Connection::open(root.path().join("index.sqlite3")).unwrap(),
        store.scope.clone(),
    )
    .unwrap();
    assert!(!reopened.changes(0, 10).unwrap().rows[0].settled);
    let rows: u64 = reopened
        .db
        .query_row("SELECT count(*) FROM process_anchors", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 0, "projection and settlement must roll back together");
}

#[test]
fn writers_read_only_their_exact_source_and_pending_intent() {
    let (_root, mut store, change) = fixture();
    let ticket = store.reserve(&change).unwrap();
    assert_eq!(
        store
            .pending_source(&change.source)
            .unwrap()
            .unwrap()
            .change,
        change
    );
    let other = Source {
        kind: Kind::Native,
        key: "different-session".into(),
    };
    assert!(store.pending_source(&other).unwrap().is_none());
    store.settle(&ticket).unwrap();
    assert!(store.pending_source(&change.source).unwrap().is_none());
    let snapshot = store.current_source(&change.source).unwrap().unwrap();
    assert_eq!(snapshot.source, change.source);
    assert_eq!(snapshot.revision, 1);
    assert_eq!(snapshot.publication, change.publication);
    assert!(store.current_source(&other).unwrap().is_none());
}

#[test]
fn advisory_owner_explicitly_downgrades_any_declared_data_completion() {
    let (_root, mut store, _change) = fixture();
    let build = store.rebuild().unwrap();
    store.scan_done(&build).unwrap();
    store.finish(&build, 0).unwrap();
    assert!(store.stamp().unwrap().complete);
    store.incomplete().unwrap();
    assert!(
        !store.stamp().unwrap().complete,
        "a partial writer owner cannot retain a complete declaration"
    );
}

#[test]
fn a_changed_namespace_after_source_success_cannot_settle_the_projection() {
    let (root, mut store, change) = fixture();
    let losses = RefCell::new(Vec::new());
    let outcome = publish_checked(
        Some(&mut store),
        &change,
        |_| {
            std::fs::write(root.path().join("record.json"), b"original success")?;
            Ok(91)
        },
        || anyhow::bail!("original directory changed"),
        |reason| losses.borrow_mut().push(reason.to_owned()),
    )
    .unwrap();
    assert_eq!(outcome, 91);
    assert_eq!(
        std::fs::read(root.path().join("record.json")).unwrap(),
        b"original success"
    );
    assert_eq!(losses.borrow().len(), 1);
    assert!(!store.changes(0, 10).unwrap().rows[0].settled);
}
