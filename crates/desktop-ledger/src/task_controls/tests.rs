use super::*;
use rusqlite::Connection;

fn id(n: u64) -> String {
    format!("00000000-0000-4000-8000-{n:012x}")
}
fn executor() -> Executor {
    Executor {
        computer: id(1),
        run: "original-run".into(),
        conversation: "original-conversation".into(),
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
