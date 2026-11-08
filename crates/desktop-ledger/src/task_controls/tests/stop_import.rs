use super::*;

fn import(n: u64) -> (Request, Hold) {
    let request = request(n);
    let mut hold = hold(n, HoldKind::Stop, &request);
    hold.id = request.id.clone();
    (request, hold)
}

#[test]
fn stop_import_recovers_a_reserved_request_after_restart_and_is_idempotent() {
    let mut db = connection();
    let original = import(1);
    {
        let mut store = store::open_enabled(&mut db).unwrap();
        store.reserve(&original.0).unwrap(); // crash before the hold
    }
    let mut store = store::open_enabled(&mut db).unwrap();
    for _ in 0..2 {
        store
            .import_stops(&executor(), std::slice::from_ref(&original), || Ok(()))
            .unwrap();
        assert_eq!(
            store.control(&original.0.id).unwrap().unwrap().outcome,
            Outcome::Confirmed
        );
        assert_eq!(store.holds(&executor(), None, 128).unwrap().rows.len(), 1);
        assert!(matches!(
            store
                .dispatch(&executor(), Dispatch::Automatic, &[])
                .unwrap(),
            Admission::Held(_)
        ));
    }
    assert_eq!(
        store.controls(&executor(), None, 128).unwrap().rows.len(),
        1
    );
}

#[test]
fn stop_evidence_is_rechecked_inside_the_actor_transaction_before_any_commit() {
    let disk = tempfile::tempdir().unwrap();
    let path = disk.path().join("ledger.sqlite");
    let mut db = Connection::open(&path).unwrap();
    let mut store = store::open_enabled(&mut db).unwrap();
    let original = import(1);
    let second = Connection::open(path).unwrap();
    second.busy_timeout(std::time::Duration::ZERO).unwrap();
    let called = std::cell::Cell::new(false);
    assert!(store.import_stops(&executor(), std::slice::from_ref(&original), || {
        called.set(true);
        assert!(matches!(second.execute_batch("BEGIN IMMEDIATE").unwrap_err(), rusqlite::Error::SqliteFailure(e, _) if e.code == rusqlite::ErrorCode::DatabaseBusy));
        anyhow::bail!("retained fact changed after capture")
    }).is_err());
    assert!(called.get());
    assert!(store.control(&original.0.id).unwrap().is_none());
    assert!(store.holds(&executor(), None, 128).unwrap().rows.is_empty());
}

#[test]
fn failed_recheck_retains_an_old_reservation_and_rolls_back_the_whole_page() {
    let mut db = connection();
    let mut store = store::open_enabled(&mut db).unwrap();
    let first = import(1);
    let second = import(2);
    store.reserve(&first.0).unwrap();
    let reserved = store.control(&first.0.id).unwrap();
    assert!(store
        .import_stops(
            &executor(),
            &[first.clone(), second.clone()],
            || anyhow::bail!("overflow, missing or changed retained evidence")
        )
        .is_err());
    assert_eq!(store.control(&first.0.id).unwrap(), reserved);
    assert!(store.control(&second.0.id).unwrap().is_none());
    assert!(store.holds(&executor(), None, 128).unwrap().rows.is_empty());
    store
        .import_stops(&executor(), &[first, second], || Ok(()))
        .unwrap();
    assert_eq!(store.holds(&executor(), None, 128).unwrap().rows.len(), 2);
}

#[test]
fn changed_stop_fact_configuration_or_store_guard_cannot_replace_the_identity() {
    let mut db = connection();
    let mut store = store::open_enabled(&mut db).unwrap();
    let first = import(1);
    store
        .import_stops(&executor(), std::slice::from_ref(&first), || Ok(()))
        .unwrap();
    let retained = store.control(&first.0.id).unwrap();
    for changed in ["fact", "configuration", "store_identity"] {
        let mut other = first.clone();
        other.0.admission = hash(changed.as_bytes());
        let new = import(2);
        assert!(store
            .import_stops(&executor(), &[new.clone(), other], || Ok(()))
            .is_err());
        assert_eq!(store.control(&first.0.id).unwrap(), retained);
        assert!(store.control(&new.0.id).unwrap().is_none());
        assert_eq!(store.holds(&executor(), None, 128).unwrap().rows.len(), 1);
    }
}

#[test]
fn settled_stop_holds_are_never_reactivated_by_import_or_restart() {
    let mut db = connection();
    let original = import(1);
    {
        let mut store = store::open_enabled(&mut db).unwrap();
        store
            .import_stops(&executor(), std::slice::from_ref(&original), || Ok(()))
            .unwrap();
        let mut release = request(2);
        release.action = Action::ReleaseStop;
        release.turn = None;
        release.observed_stops = vec![original.1.id.clone()];
        store.reserve(&release).unwrap();
        store.settle(&release.id, Outcome::Confirmed, None).unwrap();
        store.settle_observed(&release.id).unwrap();
        assert!(store.holds(&executor(), None, 128).unwrap().rows.is_empty());
    }
    let mut store = store::open_enabled(&mut db).unwrap();
    store
        .import_stops(&executor(), &[original], || Ok(()))
        .unwrap();
    assert!(store.holds(&executor(), None, 128).unwrap().rows.is_empty());
    assert!(matches!(
        store
            .dispatch(&executor(), Dispatch::Automatic, &[])
            .unwrap(),
        Admission::Allowed { .. }
    ));
}

#[test]
fn stop_import_obeys_page_hold_and_global_budgets_without_partial_records() {
    let mut db = connection();
    let mut store = store::open_enabled(&mut db).unwrap();
    let too_many = (1..=65).map(import).collect::<Vec<_>>();
    assert!(store
        .import_stops(&executor(), &too_many, || panic!(
            "over-bound page verified"
        ))
        .is_err());
    let first = import(1);
    let mut invalid = first.clone();
    invalid.0.action = Action::ReleaseStop;
    assert!(store
        .import_stops(&executor(), &[invalid], || Ok(()))
        .is_err());
    assert!(store
        .controls(&executor(), None, 128)
        .unwrap()
        .rows
        .is_empty());
    store
        .import_stops(&executor(), &too_many[..64], || Ok(()))
        .unwrap();
    assert!(store
        .import_stops(&executor(), &too_many[64..], || Ok(()))
        .is_err());
    assert!(store.control(&too_many[64].0.id).unwrap().is_none());
    assert_eq!(store.holds(&executor(), None, 128).unwrap().rows.len(), 64);
    db.execute(
        "UPDATE desktop_task_control_budget SET rows=?1 WHERE id=1",
        [STORE_ROWS as i64],
    )
    .unwrap();
    let mut store = store::open_enabled(&mut db).unwrap();
    let mut foreign = import(100);
    foreign.0.executor.run = "other-run".into();
    foreign.1.executor = foreign.0.executor.clone();
    assert!(store
        .import_stops(&foreign.0.executor, std::slice::from_ref(&foreign), || Ok(
            ()
        ))
        .is_err());
    assert!(store.control(&foreign.0.id).unwrap().is_none());
}
