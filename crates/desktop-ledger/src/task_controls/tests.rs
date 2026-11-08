use super::*;
use rusqlite::Connection;

fn id(n: u64) -> String {
    format!("00000000-0000-4000-8000-{n:012x}")
}
fn executor() -> Executor {
    Executor {
        computer: id(1),
        session_id: id(9000),
        run: "original-run".into(),
        birth: "original-birth".into(),
        conversation: Some("original-conversation".into()),
        child: None,
    }
}
fn attempt(n: u64) -> Attempt {
    Attempt {
        project: "project-one".into(),
        task: id(n),
        attempt: id(n + 100),
        generation: 1,
        fingerprint: "a".repeat(64),
    }
}
fn slot(n: u64) -> Slot {
    Slot {
        id: id(n + 200),
        revision: 1,
        executor: executor(),
        attempt: attempt(n),
        provenance: Provenance::Dispatch {
            request_id: id(n + 100),
            digest: "b".repeat(64),
        },
        binding: None,
    }
}
fn request(n: u64) -> Request {
    Request {
        id: id(n + 300),
        admission: "f".repeat(64),
        action: Action::Stop,
        executor: executor(),
        attempt: None,
        slot: None,
        task_version: None,
        turn: Some("turn-one".into()),
        observed_pauses: vec![],
        observed_stops: vec![],
    }
}
fn hold(n: u64, kind: HoldKind, request: &Request) -> Hold {
    Hold {
        id: id(n + 400),
        executor: request.executor.clone(),
        kind,
        attempt: request.attempt.clone(),
        request_id: request.id.clone(),
        reason: "explicit fixture hold".into(),
    }
}
fn connection() -> Connection {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA user_version=34;")
        .unwrap();
    db
}

#[test]
fn changed_plan_guard_bindings_refuse_the_original_uuid_without_overwrite_or_replay() {
    let mut db = connection();
    let mut store = store::open_enabled(&mut db).unwrap();
    let original = request(1);
    assert!(matches!(
        store.reserve(&original).unwrap(),
        store::Reservation::New
    ));
    let retained = store.control(&original.id).unwrap().unwrap();
    for guard in [
        "caller",
        "configuration",
        "store_identity",
        "cli_image",
        "loaded_adapter",
    ] {
        let mut changed = original.clone();
        changed.admission = hash(guard.as_bytes());
        assert_ne!(changed.admission, original.admission);
        assert!(store.reserve(&changed).is_err(), "{guard}");
        assert_eq!(store.control(&original.id).unwrap(), Some(retained.clone()));
        assert_eq!(store.controls(&executor(), None, 10).unwrap().rows.len(), 1);
    }
    assert_eq!(
        store.reserve(&original).unwrap(),
        store::Reservation::Existing(Box::new(retained))
    );
}

#[test]
fn executor_keys_include_birth_and_child_but_never_conversation() {
    let mut value = executor();
    let original = value.key().unwrap();
    value.session_id = id(9001);
    assert_eq!(value.key().unwrap(), original);
    value.conversation = None;
    assert_eq!(value.key().unwrap(), original);
    value.conversation = Some("a-later-conversation".into());
    assert_eq!(value.key().unwrap(), original);
    for part in ["run", "birth", "child", "computer"] {
        let mut changed = value.clone();
        match part {
            "run" => changed.run = "resumed-run".into(),
            "birth" => changed.birth = "another-birth".into(),
            "child" => changed.child = Some("addressable-child".into()),
            _ => changed.computer = id(2),
        }
        assert_ne!(changed.key().unwrap(), original, "{part}");
    }
    let mut unconfirmed = value.clone();
    unconfirmed.session_id = uuid::Uuid::nil().to_string();
    assert!(unconfirmed.validate().is_err());
    value.birth.clear();
    assert!(value.key().is_err());
}

#[test]
fn a_different_session_selector_cannot_cross_or_clear_original_stop_holds() {
    let mut db = connection();
    let mut store = store::open_enabled(&mut db).unwrap();
    let stop = request(1);
    store.reserve(&stop).unwrap();
    let held = hold(1, HoldKind::Stop, &stop);
    store.hold(&held).unwrap();
    let mut changed = executor();
    changed.session_id = id(9001);
    assert_eq!(changed.key().unwrap(), stop.executor.key().unwrap());
    assert!(matches!(
        store.dispatch(&changed, Dispatch::PersonSend, &[]).unwrap(),
        Admission::Held(_)
    ));
    assert!(store
        .dispatch(
            &changed,
            Dispatch::PersonSend,
            std::slice::from_ref(&held.id)
        )
        .is_err());
    let mut continuation = request(2);
    continuation.executor = changed;
    continuation.action = Action::Send;
    continuation.attempt = Some(attempt(1));
    continuation.task_version = Some("original-version".into());
    continuation.observed_stops = vec![held.id.clone()];
    store.reserve(&continuation).unwrap();
    store
        .settle(&continuation.id, Outcome::Submitted, None)
        .unwrap();
    assert!(store.settle_observed(&continuation.id).is_err());
    assert_eq!(
        store.holds(&executor(), None, 10).unwrap().rows[0].value,
        held
    );
}

#[test]
fn startup_binds_once_without_retargeting_its_slot_or_stop_holds() {
    let mut db = connection();
    let mut store = store::open_enabled(&mut db).unwrap();
    let mut original = slot(1);
    original.executor.conversation = None;
    store.claim(&original).unwrap();
    let mut stop = request(1);
    stop.executor = original.executor.clone();
    store.reserve(&stop).unwrap();
    let held = hold(1, HoldKind::Stop, &stop);
    store.hold(&held).unwrap();
    let confirmed = executor();
    let binding = Binding {
        acknowledgement: id(1),
        turn: "exact-turn".into(),
    };
    let bound = store
        .bind_startup(
            &original.guard(),
            &original.attempt,
            &original.provenance,
            &confirmed,
            &binding,
        )
        .unwrap();
    assert_eq!(bound.executor, confirmed);
    assert_eq!(bound.revision, original.revision + 1);
    assert_eq!(store.slot(&original.executor).unwrap(), Some(bound.clone()));
    assert_eq!(
        store.holds(&confirmed, None, 10).unwrap().rows[0].value,
        held
    );
    let mut later = confirmed.clone();
    later.conversation = Some("later-conversation".into());
    assert_eq!(
        store.dispatch(&later, Dispatch::Automatic, &[]).unwrap(),
        Admission::Occupied(bound.guard())
    );
    assert!(store
        .bind_startup(
            &bound.guard(),
            &original.attempt,
            &original.provenance,
            &later,
            &binding
        )
        .is_err());
    let mut changed = bound.clone();
    changed.revision += 1;
    changed.executor = later;
    assert!(store.replace(&bound.guard(), &changed).is_err());
    assert_eq!(store.slot(&confirmed).unwrap(), Some(bound));
}

#[test]
fn startup_cas_requires_the_exact_original_request_attempt_and_run_birth() {
    let mut db = connection();
    let mut store = store::open_enabled(&mut db).unwrap();
    let mut original = slot(1);
    original.executor.conversation = None;
    store.claim(&original).unwrap();
    let binding = Binding {
        acknowledgement: id(1),
        turn: "exact-turn".into(),
    };
    for part in [
        "slot",
        "request",
        "digest",
        "generation",
        "run",
        "birth",
        "child",
        "unconfirmed",
        "session_id",
    ] {
        let mut guard = original.guard();
        let mut attempt = original.attempt.clone();
        let mut provenance = original.provenance.clone();
        let mut executor = executor();
        match part {
            "slot" => guard.revision += 1,
            "generation" => attempt.generation += 1,
            "run" => executor.run = "other-run".into(),
            "birth" => executor.birth = "other-birth".into(),
            "child" => executor.child = Some("other-child".into()),
            "unconfirmed" => executor.conversation = None,
            "session_id" => executor.session_id = id(9001),
            _ => {
                if let Provenance::Dispatch { request_id, digest } = &mut provenance {
                    if part == "request" {
                        *request_id = id(99);
                    } else {
                        *digest = "c".repeat(64);
                    }
                }
            }
        }
        assert!(
            store
                .bind_startup(&guard, &attempt, &provenance, &executor, &binding)
                .is_err(),
            "{part}"
        );
        assert_eq!(
            store.slot(&original.executor).unwrap(),
            Some(original.clone()),
            "{part}"
        );
    }
}

#[test]
fn a_resumed_run_cannot_inherit_satisfy_or_clear_the_old_runs_slot_and_holds() {
    let mut db = connection();
    let mut store = store::open_enabled(&mut db).unwrap();
    let original = slot(1);
    store.claim(&original).unwrap();
    let stop = request(1);
    store.reserve(&stop).unwrap();
    let held = hold(1, HoldKind::Stop, &stop);
    store.hold(&held).unwrap();
    let mut resumed = executor();
    resumed.run = "new-resume-run".into();
    resumed.birth = "new-resume-birth".into();
    assert_eq!(resumed.session_id, original.executor.session_id);
    assert!(store.slot(&resumed).unwrap().is_none());
    assert!(store.holds(&resumed, None, 10).unwrap().rows.is_empty());
    assert!(matches!(
        store.dispatch(&resumed, Dispatch::Automatic, &[]).unwrap(),
        Admission::Allowed { .. }
    ));
    let mut release = request(2);
    release.action = Action::ReleaseStop;
    release.executor = resumed;
    release.turn = None;
    release.observed_stops = vec![held.id.clone()];
    store.reserve(&release).unwrap();
    store.settle(&release.id, Outcome::Confirmed, None).unwrap();
    assert!(store.settle_observed(&release.id).is_err());
    let mut retarget = original.clone();
    retarget.revision += 1;
    retarget.executor = release.executor.clone();
    assert!(store.replace(&original.guard(), &retarget).is_err());
    assert_eq!(
        store.slot(&original.executor).unwrap(),
        Some(original.clone())
    );
    assert_eq!(
        store.holds(&original.executor, None, 10).unwrap().rows[0].value,
        held
    );
    let mut fresh = slot(2);
    fresh.attempt.project = "another-project".into();
    fresh.executor = release.executor;
    store.claim(&fresh).unwrap();
    assert_ne!(fresh.attempt.attempt, original.attempt.attempt);
    assert_eq!(store.slot(&fresh.executor).unwrap(), Some(fresh));
}

#[test]
fn a_version_one_prototype_refuses_without_migration_or_reset() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("prototype.sqlite3");
    let mut db = Connection::open(&path).unwrap();
    let _ = store::open_enabled(&mut db).unwrap();
    db.execute(
        "UPDATE desktop_task_control_meta SET version=1 WHERE id=1",
        [],
    )
    .unwrap();
    drop(db);
    let original = std::fs::read(&path).unwrap();
    let mut db = Connection::open(&path).unwrap();
    assert!(store::open_enabled(&mut db).is_err());
    drop(db);
    assert_eq!(std::fs::read(path).unwrap(), original);
}

#[test]
fn gate_off_is_byte_identical_and_does_not_even_access_sqlite() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.sqlite3");
    let db = Connection::open(&path).unwrap();
    db.execute_batch("CREATE TABLE sentinel(payload); INSERT INTO sentinel VALUES('unchanged'); PRAGMA user_version=34;").unwrap();
    drop(db);
    let original = std::fs::read(&path).unwrap();
    let mut db =
        Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    // Gate-off admission is a strict no-op even with an existing slot/hold
    // ledger or a database that could not admit any new write.
    for _ in 0..3 {
        assert!(open(&mut db).unwrap().is_none());
    }
    drop(db);
    assert_eq!(std::fs::read(&path).unwrap(), original);
    let mut db = connection();
    {
        let mut records = store::open_enabled(&mut db).unwrap();
        records.claim(&slot(1)).unwrap();
        let r = request(1);
        records.reserve(&r).unwrap();
        records.hold(&hold(1, HoldKind::Stop, &r)).unwrap();
    }
    let before = db.total_changes();
    db.execute_batch("PRAGMA query_only=ON").unwrap();
    assert!(open(&mut db).unwrap().is_none());
    assert_eq!(db.total_changes(), before);
}

#[test]
fn lazy_records_do_not_change_desktop_schema_or_reset_unknown_state() {
    let mut db = connection();
    assert!(open(&mut db).unwrap().is_none());
    assert_eq!(
        db.query_row("SELECT count(*) FROM sqlite_schema", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    let _ = store::open_enabled(&mut db).unwrap();
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        34
    );
    db.execute("UPDATE desktop_task_control_meta SET version=99", [])
        .unwrap();
    let before = db.total_changes();
    assert!(store::open_enabled(&mut db).is_err());
    assert_eq!(db.total_changes(), before);
    let mut partial = connection();
    partial
        .execute_batch("CREATE TABLE desktop_task_control_slots(value)")
        .unwrap();
    assert!(store::open_enabled(&mut partial).is_err());
    assert_eq!(
        partial
            .query_row("SELECT count(*) FROM sqlite_schema", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn one_executor_across_projects_and_exact_slot_cas() {
    let mut db = connection();
    let mut records = store::open_enabled(&mut db).unwrap();
    let first = slot(1);
    records.claim(&first).unwrap();
    let mut second = slot(2);
    second.attempt.project = "another-project".into();
    assert!(records.claim(&second).is_err());
    let mut changed = first.clone();
    changed.revision += 1;
    changed.binding = Some(Binding {
        acknowledgement: id(900),
        turn: "turn-one".into(),
    });
    let mut forged = changed.clone();
    forged.attempt.generation += 1;
    assert!(records.replace(&first.guard(), &forged).is_err());
    forged = changed.clone();
    forged.executor.run = "replacement-run".into();
    assert!(records.replace(&first.guard(), &forged).is_err());
    records.replace(&first.guard(), &changed).unwrap();
    assert!(records.release(&first.guard()).is_err());
    assert_eq!(records.slot(&executor()).unwrap(), Some(changed.clone()));
    records.release(&changed.guard()).unwrap();
    assert!(records.claim(&first).is_err()); // Retired UUID remains a tombstone.
    records.claim(&second).unwrap();
}

#[test]
fn a_confirmed_child_has_a_separate_key_and_cannot_steal_the_parent_slot() {
    let mut db = connection();
    let mut records = store::open_enabled(&mut db).unwrap();
    let parent = slot(1);
    records.claim(&parent).unwrap();
    let mut child = slot(2);
    child.executor.child = Some("verified-native-child".into());
    records.claim(&child).unwrap();
    assert_eq!(records.slot(&parent.executor).unwrap(), Some(parent));
    assert_eq!(records.slot(&child.executor).unwrap(), Some(child));
}

#[test]
fn concurrent_connections_cannot_claim_two_projects_for_one_executor() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite3");
    let mut db = Connection::open(&path).unwrap();
    let _ = store::open_enabled(&mut db).unwrap();
    drop(db);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let threads = (1..=2)
        .map(|n| {
            let path = path.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut db = Connection::open(path).unwrap();
                db.busy_timeout(std::time::Duration::from_secs(5)).unwrap();
                let mut records = store::open_enabled(&mut db).unwrap();
                let mut candidate = slot(n);
                candidate.attempt.project = format!("project-{n}");
                barrier.wait();
                records.claim(&candidate).is_ok()
            })
        })
        .collect::<Vec<_>>();
    let admitted = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .filter(|ok| *ok)
        .count();
    assert_eq!(admitted, 1);
    let mut db = Connection::open(path).unwrap();
    assert_eq!(
        store::open_enabled(&mut db)
            .unwrap()
            .slots(None, 128)
            .unwrap()
            .rows
            .len(),
        1
    );
}

#[test]
fn same_request_is_lookup_only_and_payload_substitution_refuses() {
    let mut db = connection();
    let mut records = store::open_enabled(&mut db).unwrap();
    let r = request(1);
    assert_eq!(records.reserve(&r).unwrap(), Reservation::New);
    assert!(matches!(
        records.reserve(&r).unwrap(),
        Reservation::Existing(value) if value.outcome == Outcome::Reserved
    ));
    let mut changed = r.clone();
    changed.executor.run = "replacement".into();
    assert!(records.reserve(&changed).is_err());
    records
        .settle(
            &r.id,
            Outcome::Unknown,
            Some("original helper outcome unknown"),
        )
        .unwrap();
    assert!(matches!(
        records.reserve(&r).unwrap(),
        Reservation::Existing(value) if value.outcome == Outcome::Unknown
    ));
    assert!(records.settle(&r.id, Outcome::Submitted, None).is_err());
    records
        .settle(
            &r.id,
            Outcome::Unknown,
            Some("original helper outcome unknown"),
        )
        .unwrap();
}

#[test]
fn a_person_can_cross_observed_stops_but_new_holds_and_agent_handoff_refuse() {
    let mut db = connection();
    let mut records = store::open_enabled(&mut db).unwrap();
    let r = request(1);
    records.reserve(&r).unwrap();
    let stopped = hold(1, HoldKind::Stop, &r);
    records.hold(&stopped).unwrap();
    for kind in [
        Dispatch::PersonSend,
        Dispatch::PersonReopen,
        Dispatch::PersonDirectStart,
    ] {
        assert_eq!(
            records
                .dispatch(&executor(), kind, std::slice::from_ref(&stopped.id))
                .unwrap(),
            Admission::Allowed {
                crossed_stops: vec![stopped.id.clone()]
            }
        );
        assert!(matches!(
            records.dispatch(&executor(), kind, &[]).unwrap(),
            Admission::Held(_)
        ));
    }
    for kind in [Dispatch::Automatic, Dispatch::AgentHandoff] {
        assert!(matches!(
            records
                .dispatch(&executor(), kind, std::slice::from_ref(&stopped.id))
                .unwrap(),
            Admission::Held(_)
        ));
    }
    records.hold(&hold(2, HoldKind::Stop, &r)).unwrap();
    assert!(matches!(
        records
            .dispatch(&executor(), Dispatch::PersonSend, &[stopped.id])
            .unwrap(),
        Admission::Held(_)
    ));
    // Crossing is not clearing: both hold records remain active.
    assert_eq!(records.holds(&executor(), None, 64).unwrap().rows.len(), 2);
}

#[test]
fn permissions_unknown_and_custody_cannot_be_crossed() {
    for kind in [
        HoldKind::Permission,
        HoldKind::UnknownInput,
        HoldKind::Transfer,
        HoldKind::UnknownDelivery,
        HoldKind::Custody,
    ] {
        let mut db = connection();
        let mut records = store::open_enabled(&mut db).unwrap();
        let r = request(1);
        records.reserve(&r).unwrap();
        let stop = hold(1, HoldKind::Stop, &r);
        records.hold(&stop).unwrap();
        records.hold(&hold(2, kind, &r)).unwrap();
        assert!(matches!(
            records
                .dispatch(&executor(), Dispatch::PersonSend, &[stop.id])
                .unwrap(),
            Admission::Held(_)
        ));
    }
}

#[test]
fn settlement_requires_the_original_success_and_never_clears_a_new_pause_or_stop() {
    let mut db = connection();
    let mut records = store::open_enabled(&mut db).unwrap();
    let mut paused = request(1);
    paused.action = Action::Pause;
    paused.attempt = Some(attempt(1));
    paused.task_version = Some("original-version".into());
    records.reserve(&paused).unwrap();
    let p1 = hold(1, HoldKind::Pause, &paused);
    let s1 = hold(2, HoldKind::Stop, &paused);
    records.hold(&p1).unwrap();
    records.hold(&s1).unwrap();
    let mut resume = paused.clone();
    resume.id = id(999);
    resume.action = Action::Resume;
    resume.observed_pauses = vec![p1.id.clone()];
    resume.observed_stops = vec![s1.id.clone()];
    records.reserve(&resume).unwrap();
    assert!(records.settle_observed(&resume.id).is_err());
    let p2 = hold(3, HoldKind::Pause, &paused);
    let s2 = hold(4, HoldKind::Stop, &paused);
    records.hold(&p2).unwrap();
    records.hold(&s2).unwrap();
    records
        .settle(&resume.id, Outcome::Submitted, None)
        .unwrap();
    records.settle_observed(&resume.id).unwrap();
    let ids = records
        .holds(&executor(), None, 64)
        .unwrap()
        .rows
        .into_iter()
        .map(|r| r.value.id)
        .collect::<Vec<_>>();
    assert_eq!(ids, vec![p2.id, s2.id]);
    records.hold(&p1).unwrap(); // Same ID is not a new pause.
    assert_eq!(records.holds(&executor(), None, 64).unwrap().rows.len(), 2);
}

#[test]
fn an_unknown_resume_preserves_every_hold_and_the_paused_slot() {
    let mut db = connection();
    let mut records = store::open_enabled(&mut db).unwrap();
    records.claim(&slot(1)).unwrap();
    let mut r = request(1);
    r.action = Action::Resume;
    r.attempt = Some(attempt(1));
    r.task_version = Some("version".into());
    records.reserve(&r).unwrap();
    let h = hold(1, HoldKind::Pause, &r);
    records.hold(&h).unwrap();
    records
        .settle(&r.id, Outcome::Unknown, Some("no receipt"))
        .unwrap();
    assert!(records.settle_observed(&r.id).is_err());
    assert_eq!(
        records.holds(&executor(), None, 64).unwrap().rows[0].value,
        h
    );
    assert!(matches!(
        records
            .dispatch(&executor(), Dispatch::PersonSend, &[])
            .unwrap(),
        Admission::Occupied(_)
    ));
}

#[test]
fn bounded_pages_remain_stable_across_new_records_and_holds_are_all_examined() {
    let mut db = connection();
    let mut records = store::open_enabled(&mut db).unwrap();
    for n in 1..=5 {
        records.reserve(&request(n)).unwrap();
    }
    let first = records.controls(&executor(), None, 2).unwrap();
    let after = first.next_after.unwrap();
    records.reserve(&request(6)).unwrap();
    let rest = records.controls(&executor(), Some(after), 128).unwrap();
    assert_eq!(first.rows.len(), 2);
    assert_eq!(rest.rows.len(), 4);
    assert!(rest.rows.iter().all(|r| r.sequence > after));
    assert!(records.controls(&executor(), None, 0).is_err());
    assert!(records.controls(&executor(), None, 129).is_err());
    assert!(records.controls(&executor(), Some(-1), 1).is_err());
    let r = request(1);
    let mut observed = Vec::new();
    for n in 1..=64 {
        let mut h = hold(n, HoldKind::Stop, &r);
        h.reason = "long reason ".repeat(80);
        observed.push(h.id.clone());
        records.hold(&h).unwrap();
    }
    assert!(records
        .holds(&executor(), None, 64)
        .unwrap()
        .next_after
        .is_some());
    assert_eq!(
        records
            .dispatch(&executor(), Dispatch::PersonSend, &observed)
            .unwrap(),
        Admission::Allowed {
            crossed_stops: observed[..].to_vec()
        }
    );
    assert!(matches!(
        records
            .dispatch(&executor(), Dispatch::PersonSend, &observed[..63])
            .unwrap(),
        Admission::Held(_)
    ));
    assert!(records.hold(&hold(65, HoldKind::Stop, &r)).is_err());
}

#[test]
fn restart_retains_slot_holds_and_unknown_no_replay_records() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite3");
    let r = request(1);
    let stopped = hold(1, HoldKind::Stop, &r);
    {
        let mut db = Connection::open(&path).unwrap();
        let mut records = store::open_enabled(&mut db).unwrap();
        records.claim(&slot(1)).unwrap();
        records.reserve(&r).unwrap();
        records.hold(&stopped).unwrap();
        records
            .settle(&r.id, Outcome::Unknown, Some("native result unknown"))
            .unwrap();
    }
    let mut db = Connection::open(&path).unwrap();
    let mut records = store::open_enabled(&mut db).unwrap();
    assert_eq!(records.slot(&executor()).unwrap(), Some(slot(1)));
    assert_eq!(
        records.holds(&executor(), None, 64).unwrap().rows[0].value,
        stopped
    );
    assert!(matches!(
        records.reserve(&r).unwrap(),
        Reservation::Existing(value) if value.outcome == Outcome::Unknown
    ));
}

#[test]
fn capacity_failures_roll_back_without_pruning_or_dropping_unknowns() {
    let mut db = connection();
    {
        let mut records = store::open_enabled(&mut db).unwrap();
        records.reserve(&request(1)).unwrap();
        records
            .settle(&request(1).id, Outcome::Unknown, Some("retained"))
            .unwrap();
    }
    db.execute(
        "UPDATE desktop_task_control_budget SET rows=?1",
        [STORE_ROWS],
    )
    .unwrap();
    {
        let mut records = store::open_enabled(&mut db).unwrap();
        assert!(records.reserve(&request(2)).is_err());
        assert!(records.control(&request(2).id).unwrap().is_none());
        assert_eq!(
            records.control(&request(1).id).unwrap().unwrap().outcome,
            Outcome::Unknown
        );
    }
    db.execute(
        "UPDATE desktop_task_control_budget SET rows=1,bytes=?1",
        [STORE_BYTES],
    )
    .unwrap();
    let mut records = store::open_enabled(&mut db).unwrap();
    assert!(records.claim(&slot(1)).is_err());
    assert!(records.slot(&executor()).unwrap().is_none());
}

#[test]
fn malformed_or_oversized_records_refuse_without_resetting_them() {
    let mut db = connection();
    {
        let mut records = store::open_enabled(&mut db).unwrap();
        records.reserve(&request(1)).unwrap();
        let mut changed = request(2);
        changed.observed_stops = vec![id(4), id(4)];
        assert!(records.reserve(&changed).is_err());
        changed = request(2);
        changed.executor.run = "bad\nidentity".into();
        assert!(records.reserve(&changed).is_err());
        let mut value = serde_json::to_value(request(2)).unwrap();
        value["actor"] = "person".into();
        assert!(serde_json::from_value::<Request>(value).is_err());
    }
    let damaged = "x".repeat(RECORD_BYTES + 1);
    db.execute(
        "UPDATE desktop_task_control_requests SET payload=?1",
        [&damaged],
    )
    .unwrap();
    let records = store::open_enabled(&mut db).unwrap();
    assert!(records.control(&request(1).id).is_err());
    assert!(records.controls(&executor(), None, 128).is_err());
}

#[test]
fn review_1823_partial_ledger_without_executor_index_refuses() {
    let mut db = connection();
    let _ = store::open_enabled(&mut db).unwrap();
    db.execute_batch("DROP INDEX desktop_task_control_executor")
        .unwrap();
    let Ok(mut records) = store::open_enabled(&mut db) else {
        return;
    };
    records.claim(&slot(1)).unwrap();
    let mut other = slot(2);
    other.attempt.project = "other-project".into();
    assert!(
        records.claim(&other).is_err(),
        "partial ledger admitted two active tasks for the same exact executor"
    );
}

#[test]
fn review_1823_partial_ledger_without_budget_row_refuses() {
    let mut db = connection();
    let _ = store::open_enabled(&mut db).unwrap();
    db.execute_batch("DELETE FROM desktop_task_control_budget")
        .unwrap();
    let Ok(mut records) = store::open_enabled(&mut db) else {
        return;
    };
    assert!(
        records.reserve(&request(1)).is_err(),
        "missing budget singleton was accepted and its trigger updated zero rows"
    );
}

#[test]
fn review_1823_partial_ledger_without_budget_trigger_refuses() {
    let mut db = connection();
    let _ = store::open_enabled(&mut db).unwrap();
    db.execute(
        "UPDATE desktop_task_control_budget SET rows=?1",
        [STORE_ROWS],
    )
    .unwrap();
    db.execute_batch("DROP TRIGGER desktop_task_control_slots_insert")
        .unwrap();
    let Ok(mut records) = store::open_enabled(&mut db) else {
        return;
    };
    assert!(
        records.claim(&slot(1)).is_err(),
        "partial ledger bypassed the configured full-record budget"
    );
}

#[test]
fn review_1823_late_wrong_pause_rolls_back_every_observed_settlement() {
    let mut db = connection();
    let mut records = store::open_enabled(&mut db).unwrap();
    let stopped = request(1);
    records.reserve(&stopped).unwrap();
    let stop = hold(1, HoldKind::Stop, &stopped);
    records.hold(&stop).unwrap();
    let mut paused = request(2);
    paused.action = Action::Pause;
    paused.attempt = Some(attempt(1));
    paused.task_version = Some("v-one".into());
    records.reserve(&paused).unwrap();
    let pause = hold(2, HoldKind::Pause, &paused);
    records.hold(&pause).unwrap();
    let mut other = paused.clone();
    other.id = id(997);
    other.attempt = Some(attempt(2));
    records.reserve(&other).unwrap();
    let other_pause = hold(3, HoldKind::Pause, &other);
    records.hold(&other_pause).unwrap();
    let mut wrong = paused.clone();
    wrong.id = id(998);
    wrong.action = Action::Resume;
    wrong.observed_stops = vec![stop.id.clone()];
    wrong.observed_pauses = vec![pause.id.clone(), other_pause.id.clone()];
    records.reserve(&wrong).unwrap();
    records.settle(&wrong.id, Outcome::Submitted, None).unwrap();
    assert!(records.settle_observed(&wrong.id).is_err());
    assert_eq!(records.holds(&executor(), None, 64).unwrap().rows.len(), 3);
    assert!(matches!(
        records
            .dispatch(&executor(), Dispatch::PersonSend, &[stop.id])
            .unwrap(),
        Admission::Held(_)
    ));
}

#[test]
fn every_required_named_index_and_trigger_is_checked_without_repair() {
    let mut db = connection();
    let _ = store::open_enabled(&mut db).unwrap();
    let objects = db.prepare("SELECT type,name FROM sqlite_schema WHERE substr(name,1,21)='desktop_task_control_' AND type IN('index','trigger') ORDER BY name LIMIT 33").unwrap()
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).unwrap()
        .collect::<rusqlite::Result<Vec<_>>>().unwrap();
    assert_eq!(objects.len(), 12);
    for (kind, name) in objects {
        let mut db = connection();
        let _ = store::open_enabled(&mut db).unwrap();
        // Names come from this fixture's fixed schema, never external input.
        db.execute_batch(&format!("DROP {kind} {name}")).unwrap();
        let before = db.total_changes();
        assert!(store::open_enabled(&mut db).is_err(), "missing {name}");
        assert_eq!(
            db.total_changes(),
            before,
            "validation wrote while refusing {name}"
        );
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name=?1",
                [&name],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        db.execute_batch("PRAGMA query_only=ON").unwrap();
        assert!(open(&mut db).unwrap().is_none()); // Gate-off still ignores it.
    }
}

#[test]
fn correct_names_do_not_hide_changed_constraints_or_trigger_bodies() {
    for mutation in [
        "DROP INDEX desktop_task_control_executor; CREATE INDEX desktop_task_control_executor ON desktop_task_control_slots(executor) WHERE active=1;",
        "DROP TRIGGER desktop_task_control_slots_insert; CREATE TRIGGER desktop_task_control_slots_insert AFTER INSERT ON desktop_task_control_slots BEGIN UPDATE desktop_task_control_budget SET rows=rows WHERE id=1; END;",
        "DROP TABLE desktop_task_control_budget; CREATE TABLE desktop_task_control_budget(id INTEGER PRIMARY KEY,rows INTEGER NOT NULL,bytes INTEGER NOT NULL); INSERT INTO desktop_task_control_budget VALUES(1,0,0);",
    ] {
        let mut db = connection();
        let _ = store::open_enabled(&mut db).unwrap();
        db.execute_batch(mutation).unwrap();
        let before = db.total_changes();
        assert!(store::open_enabled(&mut db).is_err());
        assert_eq!(db.total_changes(), before);
    }
}

#[test]
fn disk_schema_refusal_keeps_original_database_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite3");
    {
        let mut db = Connection::open(&path).unwrap();
        let mut records = store::open_enabled(&mut db).unwrap();
        records.reserve(&request(1)).unwrap();
        records
            .settle(
                &request(1).id,
                Outcome::Unknown,
                Some("retained original outcome"),
            )
            .unwrap();
        db.execute_batch("DROP INDEX desktop_task_control_executor")
            .unwrap();
    }
    let before = std::fs::read(&path).unwrap();
    {
        let mut db = Connection::open(&path).unwrap();
        assert!(store::open_enabled(&mut db).is_err());
    }
    assert_eq!(std::fs::read(path).unwrap(), before);
}

#[test]
fn executor_stop_release_requires_observed_facts_and_does_not_resume_a_task() {
    let mut db = connection();
    let mut records = store::open_enabled(&mut db).unwrap();
    let stopped = request(1);
    records.reserve(&stopped).unwrap();
    let old = hold(1, HoldKind::Stop, &stopped);
    records.hold(&old).unwrap();
    let mut release = request(2);
    release.action = Action::ReleaseStop;
    release.turn = None;
    assert!(release.validate().is_err());
    release.observed_stops = vec![old.id.clone()];
    records.reserve(&release).unwrap();
    let new = hold(2, HoldKind::Stop, &stopped);
    records.hold(&new).unwrap();
    let permission = hold(3, HoldKind::Permission, &stopped);
    records.hold(&permission).unwrap();
    records
        .settle(&release.id, Outcome::Confirmed, None)
        .unwrap();
    records.settle_observed(&release.id).unwrap();
    let active = records
        .holds(&executor(), None, 64)
        .unwrap()
        .rows
        .into_iter()
        .map(|r| r.value.id)
        .collect::<Vec<_>>();
    assert_eq!(active, vec![new.id, permission.id]);
    assert!(records.slot(&executor()).unwrap().is_none());
    assert!(matches!(
        records
            .dispatch(&executor(), Dispatch::Automatic, &[])
            .unwrap(),
        Admission::Held(_)
    ));
}

#[test]
fn exact_attempt_lookup_uses_its_index_and_never_selects_a_newer_or_ambiguous_slot() {
    let mut db = connection();
    let mut store = store::open_enabled(&mut db).unwrap();
    let first = slot(1);
    store.claim(&first).unwrap();
    assert_eq!(
        store.slot_for_attempt(&first.attempt.attempt).unwrap(),
        Some(first.clone())
    );
    assert!(store.slot_for_attempt(&id(999)).unwrap().is_none());
    let mut other = slot(2);
    other.executor.run = "other-run".into();
    store.claim(&other).unwrap();
    assert_eq!(
        store.slot_for_attempt(&first.attempt.attempt).unwrap(),
        Some(first.clone())
    );
    store.release(&first.guard()).unwrap();
    assert!(store
        .slot_for_attempt(&first.attempt.attempt)
        .unwrap()
        .is_none());
    let mut collision = slot(3);
    collision.executor.run = "third-run".into();
    collision.attempt.project = "other-project".into();
    collision.attempt.attempt = other.attempt.attempt.clone();
    store.claim(&collision).unwrap();
    assert!(store.slot_for_attempt(&other.attempt.attempt).is_err());
    assert!(store.slot(&other.executor).unwrap().is_some());
    assert!(store.slot(&collision.executor).unwrap().is_some());
    let plan:String = db.query_row("EXPLAIN QUERY PLAN SELECT payload FROM desktop_task_control_slots INDEXED BY desktop_task_control_attempt WHERE json_extract(payload,'$.attempt.attempt')=?1 AND active=1 LIMIT 2",[&other.attempt.attempt],|r|r.get(3)).unwrap();
    assert!(
        plan.contains("SEARCH") && plan.contains("desktop_task_control_attempt"),
        "{plan}"
    );
}

#[test]
fn a_removed_attempt_index_refuses_at_lookup_without_a_scan_or_schema_repair() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite");
    let mut db = Connection::open(&path).unwrap();
    let mut store = store::open_enabled(&mut db).unwrap();
    let original = slot(1);
    store.claim(&original).unwrap();
    let other = Connection::open(path).unwrap();
    other
        .execute_batch("DROP INDEX desktop_task_control_attempt")
        .unwrap();
    assert!(store.slot_for_attempt(&original.attempt.attempt).is_err());
    assert_eq!(store.slot(&original.executor).unwrap(), Some(original));
    assert_eq!(
        other
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name='desktop_task_control_attempt'",
                [],
                |row| row.get::<_, usize>(0)
            )
            .unwrap(),
        0
    );
}

mod stop_import;

mod service;
