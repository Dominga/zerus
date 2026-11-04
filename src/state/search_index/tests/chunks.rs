use super::*;

fn complete(fixture: &Fixture, store: &Store) -> bool {
    store
        .index
        .connection()
        .query_row(
            "SELECT complete FROM sources WHERE id=?",
            [fixture.identity.target().key().unwrap()],
            |row| row.get(0),
        )
        .optional()
        .unwrap()
        .unwrap_or(false)
}

fn finish(fixture: &Fixture, store: &mut Store, records: usize) -> usize {
    for count in 1..=256 {
        let progress = fixture.step(store, records);
        assert!(progress.errors.is_empty(), "{:?}", progress.errors);
        assert!(progress.records <= records && progress.bytes <= 4 << 20);
        assert!(progress.files <= 32);
        if complete(fixture, store) {
            return count;
        }
        assert_ne!(store.index.status().unwrap().state, "ready");
    }
    panic!("chunk ingestion did not complete within 256 bounded slices");
}

fn hits(store: &Store, query: &str) -> Vec<zerus_content_search::Hit> {
    let mut cursor = None;
    let mut hits = Vec::new();
    for _ in 0..256 {
        let page = store.index.query(query, 1, cursor.as_deref()).unwrap();
        assert!(page.candidates <= zerus_content_search::CANDIDATES);
        hits.extend(page.hits);
        cursor = page.next_cursor;
        if cursor.is_none() {
            return hits;
        }
    }
    panic!("query did not complete within bounded cursor pages");
}

#[test]
fn all_large_snapshot_shapes_complete_in_single_record_slices_live_and_archived() {
    for archive in [false, true] {
        for (prompt, reply) in [
            (RECORD_BYTES + 1, 0),
            (0, RECORD_BYTES + 1),
            (600 << 10, 600 << 10),
        ] {
            let mut fixture = Fixture::new();
            if archive {
                fixture.identity.archive_id = uuid::Uuid::new_v4().to_string();
                fixture.record["archive_id"] = json!(fixture.identity.archive_id);
                fixture.record["archived_at"] = json!(42);
                fs::create_dir(fixture.state.join("archives")).unwrap();
                fixture.binding = fixture
                    .state
                    .join("archives")
                    .join(format!("{}.json", fixture.identity.archive_id));
            }
            for (field, size, fill, marker) in [
                ("prompt", prompt, 'x', "prompt tailneedle"),
                ("last_message", reply, 'y', "reply tailneedle"),
            ] {
                if size > 0 {
                    fixture.record[field] =
                        json!(fill.to_string().repeat(size - marker.len()) + marker);
                }
            }
            fs::write(&fixture.binding, fixture.record.to_string()).unwrap();
            let mut store = fixture.store();
            fixture.consume_exactly(&mut store, 1);
            assert!(!complete(&fixture, &store));
            assert!(
                hits(&store, "tailneedle").is_empty(),
                "unfinished original must not publish"
            );
            let slices = finish(&fixture, &mut store, 1);
            assert!(slices > 32);
            assert!(!fixture.cursor(&store).incomplete);
            let found = hits(&store, "tailneedle");
            assert_eq!(
                found.len(),
                usize::from(prompt > 0) + usize::from(reply > 0)
            );
            assert!(found
                .iter()
                .all(|hit| hit.part.as_ref().unwrap().offset > PART_BYTES));
            for hit in found {
                let id = hit.message.id.clone();
                let query = if hit.message.role == Role::User {
                    "prompt tailneedle"
                } else {
                    "reply tailneedle"
                };
                let formatted =
                    search::indexed_hit(regex::Regex::new(query).unwrap(), hit, &fixture.record)
                        .unwrap();
                assert_eq!(formatted["message_id"], id);
                assert_eq!(formatted["content_truncated"], true);
            }
            // Sweep and publication still reverify the large binding.
            let page = fixture.publish_pages(&mut store, "tailneedle", || fixture.budget(128));
            assert_eq!(
                page["results"].as_array().unwrap().len(),
                usize::from(prompt > 0) + usize::from(reply > 0),
                "{page}"
            );
            assert_eq!(page["unavailable_sessions"], 0);
        }
    }
}

#[test]
fn transcript_cursor_stays_before_partial_message_and_preserves_provider_id() {
    let fixture = Fixture::new();
    let text = "headneedle ".to_owned() + &"x".repeat(200_000) + " tailneedle";
    let line = Fixture::event(1, &text, "final");
    fixture.append(&line);
    let mut store = fixture.store();
    fixture.consume_exactly(&mut store, 1);
    assert_eq!(fixture.cursor(&store).offset, 0);
    assert!(hits(&store, "headneedle").is_empty());
    assert!(finish(&fixture, &mut store, 1) > 1);
    assert_eq!(fixture.cursor(&store).offset, line.len() as u64);
    let head = hits(&store, "headneedle");
    let tail = hits(&store, "tailneedle");
    assert_eq!(head.len(), 1);
    assert_eq!(tail.len(), 1);
    assert_eq!(head[0].message.id, "m1");
    assert_eq!(tail[0].message.id, "m1");
    assert!(!fixture.cursor(&store).incomplete);
}

#[test]
fn journal_sequence_stays_before_a_partial_message() {
    let fixture = Fixture::new();
    let db = Connection::open(fixture.state.join("events.sqlite3")).unwrap();
    db.execute_batch("CREATE TABLE events(seq INTEGER PRIMARY KEY,name TEXT,conversation TEXT,payload TEXT);CREATE INDEX events_session ON events(name,conversation,seq)").unwrap();
    let event = json!({"type":"AgentMessage","message_id":"journal-original","at":1,"detail":"x".repeat(200_000)+" journalneedle"});
    db.execute(
        "INSERT INTO events VALUES(1,?,?,?)",
        params![
            fixture.identity.name,
            fixture.identity.conversation_id,
            event.to_string()
        ],
    )
    .unwrap();
    let mut store = fixture.store();
    fixture.consume_exactly(&mut store, 1);
    assert_eq!(fixture.cursor(&store).seq, 0);
    finish(&fixture, &mut store, 1);
    assert_eq!(fixture.cursor(&store).seq, 1);
    let found = hits(&store, "journalneedle");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].message.id, "journal-original");
    assert_eq!(found[0].message.seq, 1);
}

#[test]
fn utf8_windows_preserve_maximum_queries_across_their_boundaries() {
    for character in ['é', '界', '🦀'] {
        let query = "boundary".to_owned() + &character.to_string().repeat(248);
        assert_eq!(query.chars().count(), 256);
        let text = "x".repeat(PART_BYTES - 101 + usize::from(character.len_utf8() == 3))
            + &query
            + &"y".repeat(DOCUMENT_BYTES);
        let mut start = 0;
        while start < text.len() {
            let (end, next) = part_window(&text, start).unwrap();
            assert!(text.is_char_boundary(end) && text.is_char_boundary(next));
            assert!(end - start <= PART_BYTES && next > start);
            start = next;
        }
        let fixture = Fixture::new();
        fixture.append(&Fixture::event(1, &text, "final"));
        let mut store = fixture.store();
        finish(&fixture, &mut store, 1);
        let found = hits(&store, &query.to_uppercase());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].message.id, "m1");
    }
}

#[test]
fn replacing_a_partially_indexed_binding_retires_and_purges_every_window() {
    let mut fixture = Fixture::new();
    fixture.record["prompt"] = json!("x".repeat(RECORD_BYTES) + " tailneedle");
    fs::write(&fixture.binding, fixture.record.to_string()).unwrap();
    let mut store = fixture.store();
    // zerus-0dmh: under parallel load a slice can meet its deadline before
    // its 40 records; the preparation continues over bounded slices.
    fixture.expired_slices.set(1);
    fixture.consume_exactly(&mut store, 40);
    assert!(!complete(&fixture, &store));
    fixture.record["conversation_id"] = json!(uuid::Uuid::new_v4().to_string());
    fs::write(&fixture.binding, fixture.record.to_string()).unwrap();
    let key = fixture.identity.target().key().unwrap();
    for _ in 0..4 {
        fixture.step(&mut store, 128);
    }
    assert!(hits(&store, "tailneedle").is_empty());
    let count: usize = store
        .index
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM message_parts WHERE source=?",
            [&key],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
    let parents: usize = store
        .index
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM message_groups WHERE source=?",
            [&key],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(parents, 0);
    let active: bool = store
        .index
        .connection()
        .query_row("SELECT active FROM sources WHERE id=?", [&key], |row| {
            row.get(0)
        })
        .unwrap();
    assert!(!active);
}

#[test]
fn old_incomplete_cursors_rewind_only_after_the_saved_transcript_is_verified() {
    for replaced in [false, true] {
        let fixture = Fixture::new();
        let line = Fixture::event(1, &("x".repeat(200_000) + " recoveredneedle"), "final");
        fixture.append(&line);
        let mut store = fixture.store();
        finish(&fixture, &mut store, 128);
        let mut cursor = fixture.cursor(&store);
        cursor.chunk_version = 0;
        cursor.incomplete = true;
        let db = store.index.connection();
        db.execute("DELETE FROM message_parts", []).unwrap();
        db.execute("DELETE FROM message_groups", []).unwrap();
        db.execute("UPDATE sources SET complete=0,unavailable=1", [])
            .unwrap();
        let mut legacy = serde_json::to_value(&cursor).unwrap();
        legacy.as_object_mut().unwrap().remove("chunk_version");
        db.execute("UPDATE input_cursors SET cursor=?", [legacy.to_string()])
            .unwrap();
        if replaced {
            let replacement = fixture.path.with_extension("new");
            fs::write(&replacement, &line).unwrap();
            fs::rename(replacement, &fixture.path).unwrap();
            fixture.step(&mut store, 128);
            assert!(hits(&store, "recoveredneedle").is_empty());
            let active: bool = store
                .index
                .connection()
                .query_row("SELECT active FROM sources", [], |row| row.get(0))
                .unwrap();
            assert!(!active);
        } else {
            finish(&fixture, &mut store, 1);
            assert_eq!(hits(&store, "recoveredneedle").len(), 1);
            assert!(!fixture.cursor(&store).incomplete);
            assert_eq!(fixture.cursor(&store).chunk_version, 1);
        }
    }
}
