use super::*;

fn make_unique(h: &Harness) {
    h.store.db.as_ref().unwrap().execute_batch("DROP INDEX integration_card; CREATE UNIQUE INDEX integration_card ON cards(decision_seq) WHERE kind='integration'").unwrap();
}
fn unique_flag(db: &Connection) -> Option<bool> {
    db.query_row(
        "SELECT \"unique\" FROM pragma_index_list('cards') WHERE name='integration_card'",
        [],
        |r| r.get(0),
    )
    .optional()
    .unwrap()
}

#[test]
fn a_busy_first_open_keeps_the_old_index_and_defers_conversion() {
    let h = Harness::new();
    make_unique(&h);
    let file = h.temp.path().join("teams/teams-v1.sqlite3");
    let writer = Connection::open(file).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    let opened = Store::open(h.temp.path()).unwrap();
    assert!(opened.team(TEAM).unwrap().is_some());
    assert_eq!(unique_flag(opened.db.as_ref().unwrap()), Some(true));
    writer.execute_batch("ROLLBACK").unwrap();
    let converted = Store::open(h.temp.path()).unwrap();
    assert_eq!(unique_flag(converted.db.as_ref().unwrap()), Some(false));
}

#[test]
fn a_failed_index_conversion_rolls_back_the_drop_and_keeps_all_cards() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    h.settings(|s| s.delegations.merge_accepted = false);
    make_unique(&h);
    let before = h.store.cards(TEAM, 200, None).unwrap();
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute_batch("DROP INDEX cards_waiting; CREATE TABLE cards_waiting(x)")
        .unwrap();
    let error = Store::open(h.temp.path()).unwrap_err();
    assert_eq!(error.code, code::STORAGE_FAILED);
    assert_eq!(unique_flag(h.store.db.as_ref().unwrap()), Some(true));
    assert_eq!(h.store.cards(TEAM, 200, None).unwrap(), before);
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute_batch("DROP TABLE cards_waiting")
        .unwrap();
    let repaired = Store::open(h.temp.path()).unwrap();
    assert_eq!(unique_flag(repaired.db.as_ref().unwrap()), Some(false));
    assert_eq!(repaired.cards(TEAM, 200, None).unwrap(), before);
}
