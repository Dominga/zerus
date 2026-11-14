use super::*;
use crate::teams::ops::tests::Harness;

const TEAM: &str = crate::teams::ops::tests::TEAM;

fn failure<'a>(team: &'a str, subject: &'a str, reason: &'a str, now: i64) -> Failure<'a> {
    Failure {
        team,
        kind: "cancel_retry",
        subject,
        node: Some(3),
        reason,
        failures: 1,
        now,
        next_ms: Some(now + 60_000),
    }
}
fn ledger(h: &Harness) -> Connection {
    super::super::actor::test_ledger(h.temp.path())
}

/// A reason reaches a person without line breaks, control characters or
/// invisible formatting, at most 512 bytes and cut on a character boundary.
#[test]
fn a_reason_is_made_safe_where_it_is_written() {
    assert_eq!(
        reason("  first\nline\r\n\tsecond \u{1b}[31mred\u{7}  "),
        "first line second [31mred"
    );
    assert_eq!(reason("a\u{202e}b\u{200b}c\u{feff}d\u{2066}e"), "abcde");
    assert_eq!(reason("\n\t "), "");
    let long = "é".repeat(400);
    let cut = reason(&long);
    assert!(cut.len() <= REASON_BYTES && cut.len() >= REASON_BYTES - 1);
    assert!(cut.chars().all(|c| c == 'é'));
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(SCHEMA).unwrap();
    put(&db, &failure(TEAM, "7", &"x\u{1}".repeat(600), 10)).unwrap();
    let kept: String = db
        .query_row("SELECT reason FROM service_items", [], |r| r.get(0))
        .unwrap();
    assert_eq!(kept, "x".repeat(512));
}

/// An item is refreshed in place, keeps its first time, and is removed by
/// its clear; a team's items go with the team.
#[test]
fn an_item_is_refreshed_cleared_and_removed_with_its_team() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(SCHEMA).unwrap();
    put(&db, &failure(TEAM, "7", "First", 10)).unwrap();
    let mut second = failure(TEAM, "7", "Second", 20);
    second.failures = 2;
    put(&db, &second).unwrap();
    put(&db, &failure(TEAM, "8", "Other", 30)).unwrap();
    put(&db, &failure("other-team", "9", "Elsewhere", 30)).unwrap();
    let items = page(&db, TEAM, Some(true), 25).unwrap();
    assert_eq!(items.items.len(), 2);
    let first = &items.items[0];
    assert_eq!(
        (
            first.subject.as_str(),
            first.reason.as_str(),
            first.failures,
            first.since_ms,
            first.last_ms
        ),
        ("7", "Second", 2, 10, 20)
    );
    clear(&db, TEAM, "cancel_retry", "7").unwrap();
    assert_eq!(page(&db, TEAM, Some(true), 25).unwrap().items.len(), 1);
    clear_team(&db, TEAM).unwrap();
    assert!(page(&db, TEAM, Some(true), 25).unwrap().items.is_empty());
    assert_eq!(
        page(&db, "other-team", Some(true), 25).unwrap().items.len(),
        1
    );
    let budget: i64 = db
        .query_row("SELECT rows FROM service_items_budget", [], |r| r.get(0))
        .unwrap();
    assert_eq!(budget, 1);
}

/// One page of 20, the team's and the service's, and a count of the rest
/// that stops at 1000.
#[test]
fn a_page_of_twenty_and_a_capped_count_of_the_rest() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(SCHEMA).unwrap();
    let mut service = failure("", "", "The terminal page failed", 5);
    service.kind = "terminal_page";
    service.node = None;
    put(&db, &service).unwrap();
    for i in 0..30 {
        put(&db, &failure(TEAM, &format!("{i:04}"), "Failing", 10)).unwrap();
    }
    let items = page(&db, TEAM, Some(true), 10).unwrap();
    assert_eq!(items.items.len(), PAGE);
    assert_eq!(items.items[0].scope, "service");
    assert_eq!(items.items[0].kind, "terminal_page");
    assert!(items.items[1..].iter().all(|i| i.scope == "team"));
    assert_eq!(items.more, 11);
    assert!(!items.unlisted);
    for i in 30..1500 {
        put(&db, &failure(TEAM, &format!("{i:04}"), "Failing", 10)).unwrap();
    }
    assert_eq!(page(&db, TEAM, Some(true), 10).unwrap().more, COUNT_CAP);
}

/// An item is current only while the service runs and its next try is not
/// overdue; otherwise it is as of its last try.
#[test]
fn an_item_is_current_only_while_the_service_runs_and_is_not_overdue() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(SCHEMA).unwrap();
    put(&db, &failure(TEAM, "7", "Failing", 1_000)).unwrap();
    let next = 1_000 + 60_000;
    let current = |running, now| page(&db, TEAM, running, now).unwrap().items[0].current;
    assert!(current(Some(true), next));
    assert!(current(Some(true), next + GRACE_MS));
    assert!(!current(Some(true), next + GRACE_MS + 1), "overdue");
    assert!(!current(Some(false), next), "the service is not running");
    assert!(!current(None, next), "the service's state is unknown");
}

/// When the table is full an item is not listed: the team's marker says so,
/// the writer's transaction still commits, and the marker is never an item.
#[test]
fn a_full_table_marks_its_team_instead_of_refusing_the_writer() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(SCHEMA).unwrap();
    put(&db, &failure(TEAM, "1", "Listed", 10)).unwrap();
    db.execute("UPDATE service_items_budget SET rows=16384", [])
        .unwrap();
    put(&db, &failure(TEAM, "2", "Not listed", 20)).unwrap();
    put(&db, &failure(TEAM, "1", "Still refreshed", 30)).unwrap();
    let items = page(&db, TEAM, Some(true), 25).unwrap();
    assert_eq!(items.items.len(), 1);
    assert_eq!(items.items[0].reason, "Still refreshed");
    assert!(items.unlisted);
    assert_eq!(items.more, 0);
}

/// Status reads the ledger read-only: nothing from a state without Teams or
/// without a ledger, and None from a service whose ledger has no table, so
/// a reader tells "not reported" from "no items".
#[test]
fn status_reads_items_only_from_a_ledger_that_has_them() {
    let h = Harness::new();
    let state = h.temp.path();
    assert_eq!(read(state, TEAM, Some(true), 0), None, "no ledger");
    assert!(
        !state.join("teams/actor-v1.sqlite3").exists(),
        "not created"
    );
    let db = ledger(&h);
    assert_eq!(
        read(state, TEAM, Some(true), 0),
        Some(Items {
            items: vec![],
            more: 0,
            unlisted: false
        })
    );
    put(&db, &failure(TEAM, "7", "Failing", 10)).unwrap();
    assert_eq!(read(state, TEAM, Some(true), 10).unwrap().items.len(), 1);
    db.execute_batch("DROP TABLE service_items").unwrap();
    assert_eq!(read(state, TEAM, Some(true), 10), None, "an older service");
}

/// The reads search the primary key: no scan, no sort.
#[test]
fn the_status_reads_of_items_search_their_key() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(SCHEMA).unwrap();
    for (name, sql, args) in [
        ("PAGE", PAGE_SQL, 2),
        ("COUNT", COUNT_SQL, 2),
        ("UNLISTED", UNLISTED_SQL, 1),
    ] {
        let mut plan = db.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
        let params: Vec<&dyn rusqlite::ToSql> = if args == 2 {
            vec![&TEAM, &21]
        } else {
            vec![&TEAM]
        };
        let lines: Vec<String> = plan
            .query_map(params.as_slice(), |r| r.get::<_, String>(3))
            .unwrap()
            .map(|line| line.unwrap())
            .collect();
        eprintln!("{name}: {lines:?}");
        assert!(
            lines
                .iter()
                .any(|l| l.contains("sqlite_autoindex_service_items_1")),
            "{name}: {lines:?}"
        );
        assert!(
            lines
                .iter()
                .all(|l| !l.contains("TEMP B-TREE") && !l.starts_with("SCAN service_items")),
            "{name}: {lines:?}"
        );
    }
}
