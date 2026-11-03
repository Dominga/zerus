use super::*;

pub(super) fn fixture() -> (Index, String) {
    let index = Index::new(Connection::open_in_memory().unwrap(), DEFAULT_FILE_BYTES).unwrap();
    let key = index
        .register(&Target {
            name: "codex/example/search".into(),
            run_id: "run".into(),
            conversation_id: "conversation".into(),
            archive_id: String::new(),
            agent: "codex".into(),
            cwd: "/fixture".into(),
            archived_at: 0.0,
        })
        .unwrap();
    index.catalog_ready(true).unwrap();
    (index, key)
}

pub(super) fn message(text: &str) -> Message {
    Message {
        id: "original-message".into(),
        at: 1.0,
        role: Role::Assistant,
        kind: "AgentMessage".into(),
        source: "fixture".into(),
        seq: 7,
        text: text.into(),
    }
}

#[test]
fn exact_parts_resume_idempotently_and_cannot_report_ready_before_the_last_part() {
    let (mut index, key) = fixture();
    let first = "public 🦀 ";
    let last = "tail needle";
    let total = first.len() + last.len();
    let token = hash(format!("{first}{last}").as_bytes());
    index.source_progress(&key, true, false).unwrap();
    assert_eq!(index.status().unwrap().state, "ready");
    assert_eq!(
        index
            .ingest_part(
                &key,
                &message(first),
                Part {
                    fingerprint: &token,
                    start: 0,
                    next: first.len(),
                    total,
                }
            )
            .unwrap(),
        1
    );
    assert_eq!(
        index
            .part_offset(&key, "original-message", &token, total)
            .unwrap(),
        Some(first.len())
    );
    index.source_progress(&key, true, false).unwrap();
    assert_ne!(index.status().unwrap().state, "ready");
    // A still-running legacy writer does not know about staged parts.
    assert_eq!(
        index
            .db
            .execute("UPDATE sources SET complete=1 WHERE id=?", [&key])
            .unwrap(),
        0
    );
    assert_ne!(index.status().unwrap().state, "ready");
    assert_eq!(index.status().unwrap().messages, 0);
    assert_eq!(
        index
            .ingest_part(
                &key,
                &message(first),
                Part {
                    fingerprint: &token,
                    start: 0,
                    next: first.len(),
                    total,
                }
            )
            .unwrap(),
        0
    );
    assert!(index
        .ingest_part(
            &key,
            &message("different 🦀"),
            Part {
                fingerprint: &token,
                start: 0,
                next: first.len(),
                total,
            }
        )
        .is_err());
    assert!(index
        .ingest_part(
            &key,
            &message(&last[1..]),
            Part {
                fingerprint: &token,
                start: first.len() + 1,
                next: total,
                total,
            }
        )
        .is_err());
    assert_eq!(
        index
            .part_offset(&key, "original-message", &token, total)
            .unwrap(),
        Some(first.len())
    );
    index
        .ingest_part(
            &key,
            &message(last),
            Part {
                fingerprint: &token,
                start: first.len(),
                next: total,
                total,
            },
        )
        .unwrap();
    assert_eq!(
        index
            .part_offset(&key, "original-message", &token, total)
            .unwrap(),
        Some(total)
    );
    index.source_progress(&key, true, false).unwrap();
    assert_eq!(index.status().unwrap().state, "ready");
    assert_eq!(index.status().unwrap().messages, 1);
    let physical: usize = index
        .db
        .query_row("SELECT COUNT(*) FROM message_parts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(physical, 2);
}

#[test]
fn failed_part_rolls_back_its_generation_even_when_the_caller_commits_the_refusal() {
    let (mut index, key) = fixture();
    let original = "a".repeat(64);
    index
        .ingest_part(
            &key,
            &message("first"),
            Part {
                fingerprint: &original,
                start: 0,
                next: 5,
                total: 10,
            },
        )
        .unwrap();
    index
        .db
        .execute_batch(
            "CREATE TRIGGER refuse_part BEFORE INSERT ON message_parts
        BEGIN SELECT RAISE(FAIL,'fixture storage failure'); END; BEGIN IMMEDIATE",
        )
        .unwrap();
    let replacement = "b".repeat(64);
    assert!(index
        .ingest_part(
            &key,
            &message("other"),
            Part {
                fingerprint: &replacement,
                start: 0,
                next: 5,
                total: 10,
            }
        )
        .is_err());
    index.db.execute_batch("COMMIT").unwrap();
    assert_eq!(
        index
            .part_offset(&key, "original-message", &original, 10)
            .unwrap(),
        Some(5)
    );
    assert_eq!(
        index
            .part_offset(&key, "original-message", &replacement, 10)
            .unwrap(),
        None
    );
    assert_eq!(index.status().unwrap().messages, 0);
}

#[test]
fn changing_between_small_and_split_messages_never_keeps_two_current_originals() {
    let (mut index, key) = fixture();
    index.ingest(&key, &[message("small needle")]).unwrap();
    assert_eq!(index.query("needle", 10, None).unwrap().hits.len(), 1);
    let token = "a".repeat(64);
    index
        .ingest_part(
            &key,
            &message("public needle"),
            Part {
                fingerprint: &token,
                start: 0,
                next: 13,
                total: 13,
            },
        )
        .unwrap();
    assert_eq!(index.status().unwrap().messages, 1);
    let legacy: usize = index
        .db
        .query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0))
        .unwrap();
    assert_eq!(legacy, 0);
    index.ingest(&key, &[message("small again")]).unwrap();
    assert_eq!(index.status().unwrap().messages, 1);
    assert_eq!(
        index
            .part_offset(&key, "original-message", &token, 13)
            .unwrap(),
        None
    );
    index.source_progress(&key, true, false).unwrap();
    assert_eq!(index.status().unwrap().state, "ready");
    assert_eq!(index.query("again", 10, None).unwrap().hits.len(), 1);
}

#[test]
fn retirement_refuses_parts_and_purges_only_one_existing_document_budget() {
    let (mut index, key) = fixture();
    let token = "a".repeat(64);
    for number in 0..40 {
        index
            .ingest_part(
                &key,
                &message("xxxxxxxxxxxxxxxx"),
                Part {
                    fingerprint: &token,
                    start: number * 16,
                    next: (number + 1) * 16,
                    total: 40 * 16,
                },
            )
            .unwrap();
    }
    assert_eq!(index.status().unwrap().messages, 1);
    index.retire(&key).unwrap();
    assert!(index
        .ingest_part(
            &key,
            &message("xxxxxxxxxxxxxxxx"),
            Part {
                fingerprint: &token,
                start: 0,
                next: 16,
                total: 40 * 16,
            }
        )
        .is_err());
    assert_eq!(index.purge(&key).unwrap(), PURGE_ROWS);
    let remaining: usize = index
        .db
        .query_row("SELECT COUNT(*) FROM message_parts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(remaining, 40 - PURGE_ROWS);
    assert_eq!(index.purge(&key).unwrap(), 40 - PURGE_ROWS);
    assert_eq!(index.status().unwrap().messages, 0);
    let postings: usize = index
        .db
        .query_row("SELECT COUNT(*) FROM part_grams", [], |r| r.get(0))
        .unwrap();
    assert_eq!(postings, 0);
}

#[test]
fn invalid_parts_do_not_create_a_schema_or_advance_an_original() {
    let (mut index, key) = fixture();
    let token = "a".repeat(64);
    assert!(!index.has_parts().unwrap());
    assert!(index
        .ingest_part(
            &key,
            &message(&"x".repeat(DOCUMENT_BYTES + 1)),
            Part {
                fingerprint: &token,
                start: 0,
                next: DOCUMENT_BYTES + 1,
                total: DOCUMENT_BYTES + 1,
            }
        )
        .is_err());
    assert!(index
        .ingest_part(
            &key,
            &message("body"),
            Part {
                fingerprint: &token,
                start: 0,
                next: 3,
                total: 3,
            }
        )
        .is_err());
    assert!(!index.has_parts().unwrap());
}

#[test]
fn a_legacy_insert_fences_the_prior_split_original_without_unbounded_cleanup() {
    let (mut index, key) = fixture();
    let token = "a".repeat(64);
    index
        .ingest_part(
            &key,
            &message("part"),
            Part {
                fingerprint: &token,
                start: 0,
                next: 4,
                total: 8,
            },
        )
        .unwrap();
    let body = serde_json::to_string(&message("replacement")).unwrap();
    // The old ingest path inserts this row, then its bounded gram set.
    index
        .db
        .execute(
            "INSERT INTO messages(source,message,body) VALUES(?,?,?)",
            params![key, "original-message", body],
        )
        .unwrap();
    assert_eq!(
        index
            .part_offset(&key, "original-message", &token, 8)
            .unwrap(),
        None
    );
    index.source_progress(&key, true, false).unwrap();
    assert_eq!(index.status().unwrap().state, "ready");
    assert_eq!(index.status().unwrap().messages, 1);
    let physical: usize = index
        .db
        .query_row("SELECT COUNT(*) FROM message_parts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(physical, 1, "old payload cleanup stays separately bounded");
}

#[test]
fn eviction_shares_the_existing_physical_document_limit_and_marks_coverage_incomplete() {
    let (mut index, key) = fixture();
    let token = "a".repeat(64);
    for number in 0..40 {
        index
            .ingest_part(
                &key,
                &message("xxxxxxxxxxxxxxxx"),
                Part {
                    fingerprint: &token,
                    start: number * 16,
                    next: (number + 1) * 16,
                    total: 40 * 16,
                },
            )
            .unwrap();
    }
    for number in 0..8 {
        let mut plain = message("plain");
        plain.id = format!("plain-{number}");
        index.ingest(&key, &[plain]).unwrap();
    }
    index.source_progress(&key, true, false).unwrap();
    assert_eq!(index.evict().unwrap(), PURGE_ROWS);
    let remaining: usize = index
        .db
        .query_row("SELECT COUNT(*) FROM message_parts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(remaining, 16);
    assert_eq!(index.status().unwrap().state, "partial");
    assert_eq!(index.evict().unwrap(), 16);
    assert_eq!(index.status().unwrap().messages, 0);
}

#[test]
fn eviction_fences_the_old_offset_instead_of_skipping_missing_windows() {
    let (mut index, key) = fixture();
    let token = "a".repeat(64);
    for number in 0..40 {
        index
            .ingest_part(
                &key,
                &message("xxxxxxxxxxxxxxxx"),
                Part {
                    fingerprint: &token,
                    start: number * 16,
                    next: (number + 1) * 16,
                    total: 41 * 16,
                },
            )
            .unwrap();
    }
    assert_eq!(
        index
            .part_offset(&key, "original-message", &token, 41 * 16)
            .unwrap(),
        Some(40 * 16)
    );
    assert_eq!(index.evict().unwrap(), PURGE_ROWS);
    assert_eq!(
        index
            .part_offset(&key, "original-message", &token, 41 * 16)
            .unwrap(),
        None
    );
    assert!(index
        .ingest_part(
            &key,
            &message("xxxxxxxxxxxxxxxx"),
            Part {
                fingerprint: &token,
                start: 40 * 16,
                next: 41 * 16,
                total: 41 * 16,
            }
        )
        .is_err());
    assert_eq!(index.status().unwrap().messages, 0);
    index
        .ingest_part(
            &key,
            &message("xxxxxxxxxxxxxxxx"),
            Part {
                fingerprint: &token,
                start: 0,
                next: 16,
                total: 41 * 16,
            },
        )
        .unwrap();
    assert_eq!(
        index
            .part_offset(&key, "original-message", &token, 41 * 16)
            .unwrap(),
        Some(16)
    );
    assert_ne!(index.status().unwrap().state, "ready");
}
