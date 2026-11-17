//! Timeline controls use the product operation writers, never hand-written rows.
use super::*;

#[test]
fn timeline_201_product_events_keep_the_cursor_across_new_writes_and_archive() {
    let mut h = Harness::new();
    // Create + 100 AddTask/ObserveTask pairs = exactly 201 recorded events.
    for n in 1..=100 {
        h.add(&person(), n);
    }
    let first = h.store.journal(TEAM, 200, None).unwrap();
    assert_eq!(first.rows.len(), 200);
    assert_eq!(first.rows.first().unwrap().seq, 1);
    assert_eq!(first.rows.last().unwrap().seq, 200);
    let cursor = first.next.unwrap();
    let expected_tail = h.store.journal(TEAM, 200, Some(&cursor)).unwrap();
    assert_eq!(expected_tail.rows.len(), 1);
    assert_eq!(expected_tail.rows[0].seq, 201);
    assert!(expected_tail.next.is_none());

    // New events and the actual archive writer must not change this window.
    h.add(&person(), 101);
    h.run(&person(), Operation::Stop).unwrap();
    h.run(&person(), Operation::Archive).unwrap();
    let after = h.store.journal(TEAM, 200, Some(&cursor)).unwrap();
    assert_eq!(
        serde_json::to_value(&after.rows).unwrap(),
        serde_json::to_value(&expected_tail.rows).unwrap()
    );
    assert!(after.next.is_none());
    assert_eq!(
        h.store
            .db
            .as_ref()
            .unwrap()
            .query_row("SELECT count(*) FROM journal WHERE team=?1", [TEAM], |r| {
                r.get::<_, u32>(0)
            })
            .unwrap(),
        0
    );
}
