use super::super::tests::{fixture, message};
use super::*;

#[test]
fn a_match_in_a_later_window_is_the_original_once_across_all_cursor_pages() {
    let (mut index, key) = fixture();
    let token = "a".repeat(64);
    for number in 0..70 {
        index
            .ingest_part(
                &key,
                &message("needle 🦀"),
                Part {
                    fingerprint: &token,
                    start: number * 11,
                    next: (number + 1) * 11,
                    total: 70 * 11,
                },
            )
            .unwrap();
    }
    index.source_progress(&key, true, false).unwrap();
    let mut older = message("needle older");
    older.id = "older-message".into();
    older.at = 0.5;
    index.ingest(&key, &[older]).unwrap();
    let first = index.query("needle", 1, None).unwrap();
    assert_eq!(first.hits.len(), 1);
    assert_eq!(first.hits[0].message.id, "original-message");
    assert_eq!(first.hits[0].message.seq, 7);
    assert_eq!(first.hits[0].part.as_ref().unwrap().offset, 69 * 11);
    let second = index
        .query("needle", 1, first.next_cursor.as_deref())
        .unwrap();
    assert_eq!(second.hits.len(), 1);
    assert_eq!(second.hits[0].message.id, "older-message");
    assert!(second.next_cursor.is_none());
}

#[test]
fn nonmatching_windows_continue_boundedly_without_changing_the_original_snapshot() {
    let (mut index, key) = fixture();
    let token = "a".repeat(64);
    for number in 0..70 {
        let text = if number == 0 {
            "ab needle......."
        } else {
            "ab noise........"
        };
        assert_eq!(text.len(), 16);
        index
            .ingest_part(
                &key,
                &message(text),
                Part {
                    fingerprint: &token,
                    start: number * 16,
                    next: (number + 1) * 16,
                    total: 70 * 16,
                },
            )
            .unwrap();
    }
    let first = index.query("ab needle", 10, None).unwrap();
    assert!(first.hits.is_empty());
    assert!(first.candidates <= CANDIDATES);
    let cursor = first.next_cursor.as_deref().unwrap();
    assert!(cursor.len() <= 1024);
    let mut late = message("ab needle inserted later");
    late.id = "late-message".into();
    index.ingest(&key, &[late]).unwrap();
    let second = index.query("ab needle", 10, Some(cursor)).unwrap();
    assert!(second.candidates <= CANDIDATES);
    assert_eq!(second.hits.len(), 1);
    assert_eq!(second.hits[0].message.id, "original-message");
    assert!(second.next_cursor.is_none());
    assert_eq!(
        index.query("ab needle", 10, None).unwrap().hits.len(),
        1,
        "a fresh first bounded prefix need not reach the older original"
    );
}

#[test]
fn a_group_finished_after_the_cursor_stays_out_of_its_insertion_snapshot() {
    for split_existing in [false, true] {
        let (mut index, key) = fixture();
        let mut existing = message("needle existing");
        existing.id = "existing".into();
        if split_existing {
            index
                .ingest_part(
                    &key,
                    &existing,
                    Part {
                        fingerprint: &"b".repeat(64),
                        start: 0,
                        next: existing.text.len(),
                        total: existing.text.len(),
                    },
                )
                .unwrap();
        } else {
            index.ingest(&key, &[existing]).unwrap();
        }
        let token = "a".repeat(64);
        index
            .ingest_part(
                &key,
                &message("needle"),
                Part {
                    fingerprint: &token,
                    start: 0,
                    next: 6,
                    total: 12,
                },
            )
            .unwrap();
        let before = index.start_cursor("needle", 10).unwrap();
        index
            .ingest_part(
                &key,
                &message("needle"),
                Part {
                    fingerprint: &token,
                    start: 6,
                    next: 12,
                    total: 12,
                },
            )
            .unwrap();
        let old = index.query("needle", 10, Some(&before)).unwrap();
        assert_eq!(old.hits.len(), 1);
        assert_eq!(old.hits[0].message.id, "existing");
        assert_eq!(index.query("needle", 10, None).unwrap().hits.len(), 2);
    }
}

#[test]
fn unfinished_parts_never_appear_in_a_query_with_an_existing_part_snapshot() {
    let (mut index, key) = fixture();
    let token = "a".repeat(64);
    index
        .ingest_part(
            &key,
            &message("needle"),
            Part {
                fingerprint: &token,
                start: 0,
                next: 6,
                total: 6,
            },
        )
        .unwrap();
    let mut pending = message("needle pending");
    pending.id = "pending".into();
    index
        .ingest_part(
            &key,
            &pending,
            Part {
                fingerprint: &token,
                start: 0,
                next: pending.text.len(),
                total: pending.text.len() + 5,
            },
        )
        .unwrap();
    let page = index.query("needle", 10, None).unwrap();
    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].message.id, "original-message");
    assert_ne!(page.coverage.state, "ready");
}
