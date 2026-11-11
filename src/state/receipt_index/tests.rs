use super::*;

fn value(n: u128, conversation: &str, run: &str, agent: &str, at: f64) -> Value {
    json!({"request_id":uuid::Uuid::from_u128(n).to_string(),"name":"codex/fixture/receipt","run_id":run,"conversation_id":conversation,"agent_id":agent,
        "digest":format!("fixture-digest-{n}"),"status":"submitted","first_message":false,"submitted_at":at,"text":"fixture","submitted_text":"fixture",
        "attachments":[{"name":"image.png","mime":"image/png","bytes":1,"reference":"[Image #1]","path":"/private/fixture/image.png"}]})
}
fn scope(conversation: &str, run: &str, agent: &str) -> Scope {
    Scope::new(
        &json!({"agent":"codex","conversation_id":conversation,"run_id":run}),
        agent,
    )
}
fn put(catalog: &Catalog, source: Source, value: &Value) -> PathBuf {
    let directory = catalog.state.join(source.directory());
    fs::create_dir_all(&directory).unwrap();
    let filename = format!("{}.json", string(value, "request_id"));
    let path = directory.join(&filename);
    fs::write(&path, value.to_string()).unwrap();
    catalog.remember(source, &path, value).unwrap();
    path
}
fn names(page: &Page) -> Vec<String> {
    page.locators.iter().map(|l| l.id.clone()).collect()
}

#[test]
fn twelve_thousand_foreign_receipts_do_not_fill_the_target_window() {
    let temp = tempfile::tempdir().unwrap();
    let catalog = Catalog::open(temp.path(), true).unwrap();
    let transaction = catalog.db.unchecked_transaction().unwrap();
    for n in 1..=12_000 {
        let record = value(
            n,
            if n <= 2 { "wanted" } else { "foreign" },
            "old-run",
            "",
            n as f64,
        );
        if n <= 2 {
            put(&catalog, Source::Terminal, &record);
        } else {
            catalog
                .remember(
                    Source::Terminal,
                    &catalog
                        .state
                        .join(Source::Terminal.directory())
                        .join(format!("{}.json", record["request_id"].as_str().unwrap())),
                    &record,
                )
                .unwrap();
        }
    }
    transaction.commit().unwrap();
    let page = catalog
        .page(&scope("wanted", "new-run", ""), "", 100, true)
        .unwrap();
    assert_eq!(
        names(&page),
        vec![
            uuid::Uuid::from_u128(2).to_string(),
            uuid::Uuid::from_u128(1).to_string()
        ]
    );
    assert_eq!(page.metadata["examined"], 2);
    assert_eq!(page.metadata["absence_confirmed"], false);
    for locator in &page.locators {
        assert_eq!(
            catalog.original(locator).unwrap()["conversation_id"],
            "wanted"
        );
    }
    assert!(catalog
        .page(&scope("other", "new-run", ""), "", 100, true)
        .unwrap()
        .locators
        .is_empty());
    assert!(catalog
        .page(&scope("wanted", "new-run", "other-child"), "", 100, true)
        .unwrap()
        .locators
        .is_empty());
}

#[test]
fn pages_keep_the_original_snapshot_while_new_receipts_arrive() {
    let temp = tempfile::tempdir().unwrap();
    let catalog = Catalog::open(temp.path(), true).unwrap();
    for n in 1..=10 {
        put(
            &catalog,
            Source::Terminal,
            &value(n, "wanted", "run", "", 1000.0 + n as f64),
        );
    }
    let scope = scope("wanted", "run", "");
    let first = catalog.page(&scope, "", 3, true).unwrap();
    let mut seen = names(&first);
    // These backdated inserts occupy a whole bounded scan, but cannot enter
    // the frozen snapshot or keep its cursor from making progress.
    let tx = catalog.db.unchecked_transaction().unwrap();
    for n in 11..=410 {
        let value = value(n, "wanted", "run", "", 1007.5);
        catalog
            .remember(
                Source::Terminal,
                &catalog
                    .state
                    .join(Source::Terminal.directory())
                    .join(format!("{}.json", value["request_id"].as_str().unwrap())),
                &value,
            )
            .unwrap();
    }
    tx.commit().unwrap();
    let mut cursor = first.metadata["next_cursor"].as_str().unwrap().to_owned();
    let mut finished = false;
    for _ in 0..20 {
        let page = catalog.page(&scope, &cursor, 3, true).unwrap();
        assert!(page.metadata["examined"].as_u64().unwrap() <= EXAMINE as u64);
        seen.extend(names(&page));
        if page.metadata["has_more"] != true {
            finished = true;
            break;
        }
        let next = page.metadata["next_cursor"].as_str().unwrap().to_owned();
        assert_ne!(cursor, next);
        cursor = next;
    }
    assert!(finished);
    assert_eq!(
        seen,
        (1..=10)
            .rev()
            .map(|n| uuid::Uuid::from_u128(n).to_string())
            .collect::<Vec<_>>()
    );
    assert!(catalog
        .page(&self::scope("foreign", "run", ""), &cursor, 3, true)
        .is_err());
    assert!(catalog.page(&scope, "invalid cursor", 3, true).is_err());
    assert!(catalog.page(&scope, "", 0, true).is_err());
    assert!(catalog.page(&scope, "", 101, true).is_err());
}

#[test]
fn namespaces_children_and_initial_runs_stay_separate() {
    let temp = tempfile::tempdir().unwrap();
    let catalog = Catalog::open(temp.path(), true).unwrap();
    for source in Source::ALL {
        put(&catalog, source, &value(1, "wanted", "run", "", 1.0));
    }
    put(
        &catalog,
        Source::Terminal,
        &value(2, "wanted", "run", "child", 2.0),
    );
    let mut initial = value(3, "", "initial-run", "", 3.0);
    initial["first_message"] = json!(true);
    put(&catalog, Source::Terminal, &initial);
    assert_eq!(
        catalog
            .page(&scope("wanted", "run", ""), "", 100, true)
            .unwrap()
            .locators
            .len(),
        1
    );
    assert_eq!(
        catalog
            .page(&scope("wanted", "run", "child"), "", 100, true)
            .unwrap()
            .locators
            .len(),
        1
    );
    assert_eq!(
        catalog
            .page(&scope("new-conversation", "initial-run", ""), "", 100, true)
            .unwrap()
            .locators
            .len(),
        1
    );
    assert!(catalog
        .page(&scope("new-conversation", "other-run", ""), "", 100, true)
        .unwrap()
        .locators
        .is_empty());
    initial["resolved_conversation_id"] = json!("new-conversation");
    put(&catalog, Source::Terminal, &initial);
    assert_eq!(
        catalog
            .page(&scope("new-conversation", "resumed-run", ""), "", 100, true)
            .unwrap()
            .locators
            .len(),
        1
    );
    assert!(catalog
        .page(
            &scope("wrong-conversation", "initial-run", ""),
            "",
            100,
            true
        )
        .unwrap()
        .locators
        .is_empty());
}

#[test]
fn missing_corrupt_and_rebound_originals_are_explicit_errors() {
    let temp = tempfile::tempdir().unwrap();
    let catalog = Catalog::open(temp.path(), true).unwrap();
    let original = value(1, "wanted", "run", "", 1.0);
    let path = put(&catalog, Source::Terminal, &original);
    let locator = catalog
        .page(&scope("wanted", "run", ""), "", 1, true)
        .unwrap()
        .locators
        .remove(0);
    fs::remove_file(&path).unwrap();
    assert_eq!(catalog.original(&locator).unwrap_err(), "missing");
    fs::write(&path, b"not json").unwrap();
    assert_eq!(catalog.original(&locator).unwrap_err(), "corrupt");
    let mut foreign = original.clone();
    foreign["conversation_id"] = json!("foreign");
    foreign["digest"] = json!("changed");
    fs::write(&path, foreign.to_string()).unwrap();
    catalog.remember(Source::Terminal, &path, &foreign).unwrap();
    assert_eq!(catalog.original(&locator).unwrap_err(), "binding_changed");
    assert!(catalog
        .page(&scope("foreign", "run", ""), "", 100, true)
        .unwrap()
        .locators
        .is_empty());
    let current = catalog
        .page(&scope("wanted", "run", ""), "", 100, true)
        .unwrap();
    assert_eq!(current.locators[0].sequence, locator.sequence);
    assert_eq!(current.metadata["unattributed_errors"], 1);
    fs::write(&path, vec![b'x'; ORIGINAL_BYTES as usize + 1]).unwrap();
    assert_eq!(catalog.original(&locator).unwrap_err(), "oversized");
}

#[test]
fn bounded_migration_resumes_without_rewriting_history_or_sequences() {
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join(Source::Terminal.directory());
    fs::create_dir(&directory).unwrap();
    let mut originals = Vec::new();
    for n in 1..=73 {
        let value = value(n, "wanted", "run", "", n as f64);
        let path = directory.join(format!("{}.json", value["request_id"].as_str().unwrap()));
        let bytes = value.to_string().into_bytes();
        fs::write(&path, &bytes).unwrap();
        originals.push((path, bytes));
    }
    let corrupt = directory.join("corrupt.json");
    fs::write(&corrupt, b"{").unwrap();
    let mut migration = migration::Migration::new(temp.path()).unwrap();
    assert!(!migration.step().unwrap());
    let catalog = Catalog::open(temp.path(), false).unwrap();
    let before: Vec<(String, i64)> = catalog
        .db
        .prepare("SELECT request,seq FROM receipt_rows ORDER BY seq")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert!(!before.is_empty() && before.len() <= 64);
    drop(migration);
    let writer = Catalog::open(temp.path(), true).unwrap();
    writer
        .error(Source::Terminal, "@enumeration", "enumeration_incomplete")
        .unwrap();
    drop(writer);
    let mut resumed = migration::Migration::new(temp.path()).unwrap();
    let mut done = false;
    for _ in 0..1000 {
        if resumed.step().unwrap() {
            done = true;
            break;
        }
    }
    assert!(done);
    drop(resumed);
    for (id, sequence) in before {
        assert_eq!(
            catalog
                .db
                .query_row("SELECT seq FROM receipt_rows WHERE request=?", [id], |r| {
                    r.get::<_, i64>(0)
                })
                .unwrap(),
            sequence
        );
    }
    let page = catalog
        .page(&scope("wanted", "run", ""), "", 100, true)
        .unwrap();
    assert_eq!(page.locators.len(), 73);
    assert_eq!(page.metadata["indexed"], 73);
    assert_eq!(page.metadata["state"], "partial");
    assert_eq!(page.metadata["unattributed_errors"], 1);
    for (path, bytes) in originals {
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
    assert_eq!(fs::read(corrupt).unwrap(), b"{");
}

#[test]
fn unavailable_index_does_not_change_authoritative_receipt_persistence() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join(DATABASE), b"not sqlite").unwrap();
    let value = value(1, "wanted", "run", "", 1.0);
    let path = temp
        .path()
        .join("input_receipts")
        .join(format!("{}.json", value["request_id"].as_str().unwrap()));
    persist(&path, &value).unwrap();
    assert_eq!(read_original(&path).unwrap(), value);
    if let Ok(catalog) = Catalog::open(temp.path(), false) {
        assert!(catalog
            .page(&scope("wanted", "run", ""), "", 100, true)
            .is_err());
    }
}


#[test]
fn a_lost_row_invalidates_cursors_and_is_rediscovered_without_original_changes() {
    let temp = tempfile::tempdir().unwrap();
    let catalog = Catalog::open(temp.path(), true).unwrap();
    let one = put(
        &catalog,
        Source::Terminal,
        &value(1, "wanted", "run", "", 1.0),
    );
    put(
        &catalog,
        Source::Terminal,
        &value(2, "wanted", "run", "", 2.0),
    );
    let original = fs::read(&one).unwrap();
    let stamp = migration::stamp(&temp.path().join(Source::Terminal.directory())).unwrap();
    catalog
        .db
        .execute(
            "UPDATE receipt_sources SET stamp=?,complete=1 WHERE source='terminal'",
            [stamp],
        )
        .unwrap();
    let scope = scope("wanted", "run", "");
    let first = catalog.page(&scope, "", 1, true).unwrap();
    let cursor = first.metadata["next_cursor"].as_str().unwrap();
    catalog
        .db
        .execute(
            "DELETE FROM receipt_rows WHERE request=?",
            [uuid::Uuid::from_u128(1).to_string()],
        )
        .unwrap();
    assert!(catalog.page(&scope, cursor, 1, true).is_err());
    let missing = catalog.page(&scope, "", 100, true).unwrap();
    assert_eq!(missing.metadata["migration_complete"], false);
    assert_eq!(missing.metadata["unattributed_errors"], 1);
    let mut migration = migration::Migration::new(temp.path()).unwrap();
    while !migration.step().unwrap() {}
    let restored = catalog.page(&scope, "", 100, true).unwrap();
    assert_eq!(restored.locators.len(), 2);
    assert_eq!(restored.metadata["state"], "indexed");
    assert_eq!(restored.metadata["indexed"], 2);
    assert_eq!(restored.metadata["unattributed_errors"], 0);
    assert_eq!(fs::read(one).unwrap(), original);
    assert!(catalog.page(&scope, cursor, 1, true).is_err());

fn change_count(catalog: &Catalog) -> i64 {
    catalog
        .db
        .query_row(
            "SELECT CAST(value AS INTEGER) FROM receipt_meta WHERE key='change_seq'",
            [],
            |r| r.get(0),
        )
        .unwrap()
}

#[test]
fn changed_pages_advance_a_bounded_scope_without_foreign_receipts() {
    let temp = tempfile::tempdir().unwrap();
    let catalog = Catalog::open(temp.path(), true).unwrap();
    put(&catalog, Source::Terminal, &value(1, "wanted", "run", "", 10.0));
    put(&catalog, Source::Terminal, &value(2, "wanted", "run", "", 11.0));
    let transaction = catalog.db.unchecked_transaction().unwrap();
    for n in 3..=40 {
        let record = value(n, "foreign", "run", "", n as f64);
        catalog
            .remember(
                Source::Terminal,
                &catalog
                    .state
                    .join(Source::Terminal.directory())
                    .join(format!("{}.json", record["request_id"].as_str().unwrap())),
                &record,
            )
            .unwrap();
    }
    transaction.commit().unwrap();
    let scope = scope("wanted", "run", "");
    let first = catalog.changed_page(&scope, "", 1, true).unwrap();
    assert_eq!(first.rows.len(), 1);
    assert_eq!(
        first.rows[0].locator.id,
        uuid::Uuid::from_u128(1).to_string()
    );
    assert!(!first.drained);
    assert_eq!(first.rows[0].change, 1);
    let second = catalog.changed_page(&scope, &first.cursor, 10, true).unwrap();
    assert_eq!(second.rows.len(), 1);
    assert_eq!(
        second.rows[0].locator.id,
        uuid::Uuid::from_u128(2).to_string()
    );
    assert!(second.drained);
    let third = catalog.changed_page(&scope, &second.cursor, 10, true).unwrap();
    assert!(third.rows.is_empty());
    assert!(third.drained);
}

#[test]
fn an_update_beyond_the_frozen_upper_waits_for_the_next_chain() {
    let temp = tempfile::tempdir().unwrap();
    let catalog = Catalog::open(temp.path(), true).unwrap();
    put(&catalog, Source::Terminal, &value(1, "wanted", "run", "", 10.0));
    let second = value(2, "wanted", "run", "", 11.0);
    let path = put(&catalog, Source::Terminal, &second);
    assert_eq!(change_count(&catalog), 2);
    let scope = scope("wanted", "run", "");
    let first = catalog.changed_page(&scope, "", 1, true).unwrap();
    assert_eq!(first.rows.len(), 1);
    // The second original changes while the first chain is live. The binding
    // is immutable; the content stamp and change move beyond the frozen upper.
    let mut updated = second.clone();
    updated["status"] = json!("completed");
    fs::write(&path, updated.to_string()).unwrap();
    catalog.remember(Source::Terminal, &path, &updated).unwrap();
    assert_eq!(change_count(&catalog), 3);
    let frozen = catalog.changed_page(&scope, &first.cursor, 10, true).unwrap();
    assert!(
        frozen
            .rows
            .iter()
            .all(|row| row.locator.id != uuid::Uuid::from_u128(2).to_string()),
        "a status update leaked into the frozen chain"
    );
    assert!(frozen.drained);
    let next = catalog.changed_page(&scope, &frozen.cursor, 10, true).unwrap();
    assert_eq!(next.rows.len(), 1);
    assert_eq!(
        next.rows[0].locator.id,
        uuid::Uuid::from_u128(2).to_string()
    );
    assert_eq!(next.rows[0].change, 3);
    assert_eq!(
        catalog.original(&next.rows[0].locator).unwrap()["status"],
        "completed"
    );
}

#[test]
fn change_cursors_are_bound_to_the_index_epoch_and_scope() {
    let temp = tempfile::tempdir().unwrap();
    let catalog = Catalog::open(temp.path(), true).unwrap();
    put(&catalog, Source::Terminal, &value(1, "wanted", "run", "", 10.0));
    let page = catalog
        .changed_page(&scope("wanted", "run", ""), "", 10, true)
        .unwrap();
    assert_eq!(
        catalog
            .changed_page(&scope("other", "run", ""), &page.cursor, 10, true)
            .unwrap_err(),
        CURSOR_REJECTED
    );
    catalog
        .db
        .execute("UPDATE receipt_meta SET value='other' WHERE key='epoch'", [])
        .unwrap();
    assert_eq!(
        catalog
            .changed_page(&scope("wanted", "run", ""), &page.cursor, 10, true)
            .unwrap_err(),
        CURSOR_REJECTED
    );
}

#[test]
fn twelve_thousand_foreign_receipts_stay_out_of_a_changed_scope() {
    let temp = tempfile::tempdir().unwrap();
    let catalog = Catalog::open(temp.path(), true).unwrap();
    let transaction = catalog.db.unchecked_transaction().unwrap();
    for n in 1..=12_000 {
        let record = value(
            n,
            if n <= 2 { "wanted" } else { "foreign" },
            "run",
            "",
            n as f64,
        );
        catalog
            .remember(
                Source::Terminal,
                &catalog
                    .state
                    .join(Source::Terminal.directory())
                    .join(format!("{}.json", record["request_id"].as_str().unwrap())),
                &record,
            )
            .unwrap();
    }
    transaction.commit().unwrap();
    let page = catalog
        .changed_page(&scope("wanted", "run", ""), "", 100, true)
        .unwrap();
    assert_eq!(
        page.rows
            .iter()
            .map(|row| row.locator.id.clone())
            .collect::<Vec<_>>(),
        vec![
            uuid::Uuid::from_u128(1).to_string(),
            uuid::Uuid::from_u128(2).to_string()
        ]
    );
    assert!(page.drained);
 97301104d (Use the bounded receipt locator for the canonical history import (zerus-1naad))
}
