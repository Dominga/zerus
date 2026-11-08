use super::*;
fn imports() -> Vec<(Request, Hold)> {
    (1..=2)
        .map(|n| {
            let request = request(n);
            let mut hold = hold(n, HoldKind::Stop, &request);
            hold.id = request.id.clone();
            (request, hold)
        })
        .collect()
}
fn yielding() -> Request {
    let mut value = request(50);
    value.action = Action::Yield;
    value.attempt = Some(attempt(50));
    value.slot = Some(SlotGuard {
        id: id(900),
        revision: 1,
    });
    value.task_version = Some("original-task-version".into());
    value
}
#[test]
fn existing_only_status_never_creates_or_repairs_a_ledger() {
    let mut db = connection();
    assert!(store::existing_enabled(&mut db).unwrap().is_none());
    let count: usize = db
        .query_row("SELECT count(*) FROM sqlite_schema", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
    db.execute_batch("CREATE TABLE desktop_task_control_extra(id)")
        .unwrap();
    assert!(store::existing_enabled(&mut db).is_err());
    assert_eq!(
        db.query_row::<usize, _, _>("SELECT count(*) FROM sqlite_schema", [], |r| r.get(0))
            .unwrap(),
        1
    );
    db.execute_batch("DROP TABLE desktop_task_control_extra")
        .unwrap();
    store::open_enabled(&mut db).unwrap();
    assert!(store::existing_enabled(&mut db).unwrap().is_some());
    db.execute_batch("DROP INDEX desktop_task_control_attempt")
        .unwrap();
    assert!(store::existing_enabled(&mut db).is_err());
    assert_eq!(
        db.query_row::<usize, _, _>(
            "SELECT count(*) FROM sqlite_schema WHERE name='desktop_task_control_attempt'",
            [],
            |r| r.get(0)
        )
        .unwrap(),
        0
    );
}
#[test]
fn lazy_creation_rechecks_custody_on_the_original_connection_and_rolls_back() {
    let mut db = connection();
    let original = unsafe { db.handle() } as usize;
    let mut calls = 0;
    assert!(store::open_enabled_checked(&mut db, |current| {
        calls += 1;
        assert_eq!(unsafe { current.handle() } as usize, original);
        anyhow::bail!("original native peer/custody changed")
    })
    .is_err());
    assert_eq!(calls, 1);
    assert!(store::existing_enabled(&mut db).unwrap().is_none());
    // The public false gate does not even call the verifier or initialize SQL.
    assert!(open_checked(&mut db, |_| panic!("off must return first"))
        .unwrap()
        .is_none());
    assert!(existing(&mut db).unwrap().is_none());
}
#[test]
fn a_negative_admission_and_every_stop_commit_or_rollback_together() {
    let mut db = connection();
    let address = unsafe { db.handle() } as usize;
    let mut store = store::open_enabled(&mut db).unwrap();
    let request = yielding();
    let imports = imports();
    assert!(store
        .refuse_with_stops(&request, &imports, "before effect", |current| {
            assert_eq!(unsafe { current.handle() } as usize, address);
            anyhow::bail!("fact changed after capture / lost custody / overflow")
        })
        .is_err());
    assert!(store
        .controls(&executor(), None, 128)
        .unwrap()
        .rows
        .is_empty());
    assert!(store.holds(&executor(), None, 128).unwrap().rows.is_empty());
    let result = store
        .refuse_with_stops(&request, &imports, "before effect", |_| Ok(()))
        .unwrap();
    assert_eq!(result.outcome, Outcome::Refused);
    assert_eq!(
        store.controls(&executor(), None, 128).unwrap().rows.len(),
        3
    );
    assert_eq!(store.holds(&executor(), None, 128).unwrap().rows.len(), 2);
    // Exact repeat is lookup-only: even an invalid new import is not applied.
    let mut changed = imports;
    changed[0].0.admission = "b".repeat(64);
    assert_eq!(
        store
            .refuse_with_stops(&request, &changed, "different refusal", |_| Ok(()))
            .unwrap(),
        result
    );
    let mut substituted = request.clone();
    substituted.admission = "a".repeat(64);
    assert!(store
        .refuse_with_stops(&substituted, &[], "before effect", |_| Ok(()))
        .is_err());
    assert_eq!(store.control(&request.id).unwrap().unwrap(), result);
}
#[test]
fn durable_reserved_or_unknown_is_lookup_only_and_never_restarts_admission() {
    for outcome in [Outcome::Reserved, Outcome::Unknown] {
        let mut db = connection();
        let request = yielding();
        let original = {
            let mut store = store::open_enabled(&mut db).unwrap();
            store.reserve(&request).unwrap();
            if outcome == Outcome::Unknown {
                store
                    .settle(&request.id, outcome, Some("original unknown"))
                    .unwrap();
            }
            store.control(&request.id).unwrap().unwrap()
        };
        let mut reopened = store::existing_enabled(&mut db).unwrap().unwrap();
        assert_eq!(
            reopened
                .refuse_with_stops(&request, &imports(), "must not replace", |_| Ok(()))
                .unwrap(),
            original
        );
        assert!(reopened
            .holds(&executor(), None, 128)
            .unwrap()
            .rows
            .is_empty());
        assert_eq!(
            reopened
                .controls(&executor(), None, 128)
                .unwrap()
                .rows
                .len(),
            1
        );
    }
}
