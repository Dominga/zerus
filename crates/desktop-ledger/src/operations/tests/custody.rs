//! SQL contract cases only. File identity and process fields are synthetic;
//! actual inherited-descriptor and private-file proof belongs to the root host.
use super::super::custody as store;
use super::*;

fn setup() -> (Connection, Reserve, store::Prepared) {
    let mut db = db();
    let tx = db.transaction().unwrap();
    store::migrate(&tx).unwrap();
    tx.commit().unwrap();
    let original = put(&mut db, 0);
    let request = request(&original, 0);
    let prepared = store::Prepared {
        version: 1,
        configuration: fixture()["binding"]["key"]["configuration"]
            .as_str()
            .unwrap()
            .into(),
        ticket: Ticket {
            operation_id: original.operation_id,
            attempt_id: request.attempt_id.clone(),
            revision: original.revision,
            admission_id: original.admission_id,
            binding_sha256: original.binding_sha256,
            writer_epoch: WRITER.into(),
            service_epoch: SERVICE.into(),
            lease: LEASE.into(),
        },
        boot: id(200),
        service: store::Birth {
            pid: 123,
            birth: "456".into(),
        },
        file: store::FileIdentity {
            device: 1,
            inode: 2,
            changed: 3,
            changed_ns: 4,
            bytes: 5,
            sha256: hash(b"synthetic custody header"),
        },
    };
    (db, request, prepared)
}

#[test]
fn reservation_and_prepared_custody_are_atomic_and_replay_is_status_only() {
    let (mut db, request, prepared) = setup();
    db.execute_batch("CREATE TRIGGER refuse_custody BEFORE INSERT ON desktop_operation_custody BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert!(store::reserve(&mut db, &request, &prepared).is_err());
    assert_eq!(totals(&db).unwrap().undrained, 0);
    assert!(store::pending(&db).unwrap().is_none());
    db.execute_batch("DROP TRIGGER refuse_custody").unwrap();
    let reserved = store::reserve(&mut db, &request, &prepared).unwrap();
    assert!(reserved.fresh);
    assert_eq!(reserved.attempt.ticket, prepared.ticket);
    assert!(!store::reserve(&mut db, &request, &prepared).unwrap().fresh);
    let mut changed = prepared.clone();
    changed.file.inode += 1;
    assert!(store::reserve(&mut db, &request, &changed).is_err());
    let row = store::pending(&db).unwrap().unwrap();
    assert_eq!(row.1.unwrap().prepared, prepared);
    assert_eq!(totals(&db).unwrap().undrained, 1);
}

#[test]
fn changed_ticket_or_configuration_cannot_leave_a_charged_half_reservation() {
    for field in 0..3 {
        let (mut db, request, mut prepared) = setup();
        match field {
            0 => prepared.ticket.lease = id(999),
            1 => prepared.configuration = "f".repeat(64),
            _ => prepared.ticket.binding_sha256 = "f".repeat(64),
        }
        assert!(store::reserve(&mut db, &request, &prepared).is_err());
        assert_eq!(totals(&db).unwrap().undrained, 0);
        assert_eq!(
            db.query_row("SELECT count(*) FROM desktop_operation_attempts", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
}

#[test]
fn helper_birth_is_once_only_and_registration_refuses_after_revoke_or_restart() {
    let (mut db, request, prepared) = setup();
    store::reserve(&mut db, &request, &prepared).unwrap();
    let helper = store::Birth {
        pid: 789,
        birth: "012".into(),
    };
    store::register(&mut db, &prepared.ticket, &helper).unwrap();
    store::register(&mut db, &prepared.ticket, &helper).unwrap();
    let mut changed = helper.clone();
    changed.birth.push('3');
    assert!(store::register(&mut db, &prepared.ticket, &changed).is_err());
    assert_eq!(
        store::record(&db, &prepared.ticket)
            .unwrap()
            .unwrap()
            .helper,
        Some(helper.clone())
    );
    let tx = db.transaction().unwrap();
    crate::revoke(&tx, "fixture").unwrap();
    tx.commit().unwrap();
    assert!(store::register(&mut db, &prepared.ticket, &helper).is_err());
    recover_batch(&mut db, &id(300)).unwrap();
    assert_eq!(store::pending(&db).unwrap().unwrap().0.state, "unknown");
    assert_eq!(totals(&db).unwrap().undrained, 1);
}

#[test]
fn missing_custody_and_unregistered_helper_never_mean_resources_ended() {
    let (mut db, request, prepared) = setup();
    reserve(&mut db, &request).unwrap();
    assert!(store::pending(&db).unwrap().unwrap().1.is_none());
    assert!(store::reserve(&mut db, &request, &prepared).is_err());
    assert_eq!(totals(&db).unwrap().undrained, 1);
    let (mut db, request, prepared) = setup();
    store::reserve(&mut db, &request, &prepared).unwrap();
    assert!(store::pending(&db)
        .unwrap()
        .unwrap()
        .1
        .unwrap()
        .helper
        .is_none());
    recover_batch(&mut db, &id(300)).unwrap();
    assert_eq!(totals(&db).unwrap().undrained, 1);
    finish(
        &mut db,
        &prepared.ticket,
        &Outcome::Unknown {
            reason: Unknown::ServiceRestart,
        },
    )
    .unwrap();
    assert_eq!(totals(&db).unwrap().undrained, 1);
}

#[test]
fn custody_preservation_and_closed_bounds_fail_without_mutating_identity() {
    let (mut db, request, prepared) = setup();
    store::reserve(&mut db, &request, &prepared).unwrap();
    for sql in [
        "DELETE FROM desktop_operation_custody",
        "UPDATE desktop_operation_custody SET prepared='{}'",
        "UPDATE desktop_operation_custody SET fingerprint='changed'",
    ] {
        assert!(db.execute(sql, []).is_err());
    }
    let mut changed = prepared.ticket.clone();
    changed.lease = id(999);
    assert!(store::record(&db, &changed).is_err());
    for field in 0..5 {
        let mut changed = prepared.clone();
        match field {
            0 => changed.file.bytes = 4097,
            1 => changed.file.changed_ns = -1,
            2 => changed.service.pid = 0,
            3 => changed.boot = "x".repeat(129),
            _ => changed.version = 2,
        }
        assert!(changed.validate().is_err());
    }
    assert_eq!(
        store::pending(&db).unwrap().unwrap().1.unwrap().prepared,
        prepared
    );
}
