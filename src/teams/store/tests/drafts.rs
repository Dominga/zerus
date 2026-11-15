use super::*;

// Product writers create the team and queue. Only the missing/newer schema
// faults below are injected; no team, phase or draft row is written by hand.
fn before_drafts(h: &Harness) {
    h.store.db.as_ref().unwrap().execute_batch(
        "DROP TABLE phase_draft_nodes; DROP TABLE phase_draft_counters; DROP TABLE phase_drafts;",
    ).unwrap();
}
fn objects(db: &Connection) -> u32 {
    db.query_row("SELECT count(*) FROM sqlite_master WHERE name IN ('phase_drafts','phase_draft_nodes','phase_draft_page','phase_draft_counters')", [], |r| r.get(0)).unwrap()
}

#[test]
fn concurrent_draft_extension_opens_keep_the_old_product_rows_and_revision() {
    let h = Harness::new();
    let before = h.store.team(TEAM).unwrap();
    let queue = h.store.queue(TEAM, 200, None).unwrap();
    let journal = h.store.journal(TEAM, 200, None).unwrap();
    before_drafts(&h);
    let gate = std::sync::Arc::new(std::sync::Barrier::new(3));
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..2)
            .map(|_| {
                let gate = gate.clone();
                let path = h.temp.path();
                let before = before.clone();
                scope.spawn(move || {
                    gate.wait();
                    let store = Store::open(path).unwrap();
                    assert_eq!(objects(store.db.as_ref().unwrap()), 4);
                    assert_eq!(store.team(TEAM).unwrap(), before);
                    assert!(!store.phase_draft(TEAM, 0).unwrap().complete);
                })
            })
            .collect();
        gate.wait();
        for worker in workers {
            worker.join().unwrap();
        }
    });
    assert_eq!(h.store.team(TEAM).unwrap(), before);
    assert_eq!(h.store.queue(TEAM, 200, None).unwrap(), queue);
    assert_eq!(h.store.journal(TEAM, 200, None).unwrap(), journal);
}

#[test]
fn draft_extension_busy_is_known_no_write_and_a_newer_fence_changes_nothing() {
    let h = Harness::new();
    before_drafts(&h);
    let owner = h.store.db.as_ref().unwrap();
    let contender = Connection::open(h.temp.path().join("teams/teams-v1.sqlite3")).unwrap();
    contender.busy_timeout(Duration::from_millis(10)).unwrap();
    owner.execute_batch("BEGIN IMMEDIATE").unwrap();
    refused(
        crate::teams::store::drafts::extend(&contender),
        code::STORE_BUSY,
    );
    assert_eq!(objects(owner), 0);
    owner
        .execute_batch("UPDATE timers_extension SET version=999 WHERE id=1; COMMIT")
        .unwrap();
    assert!(crate::teams::store::drafts::extend(&contender).is_err());
    assert_eq!(objects(owner), 0);
    assert!(contender.is_autocommit());
}
