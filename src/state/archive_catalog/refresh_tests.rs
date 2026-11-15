use super::*;

fn probe(case: &str) {
    let private = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "state::archive_catalog::refresh_tests::isolated_catalog_probe",
            "--ignored",
            "--nocapture",
        ])
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", private.path())
        .env("HGS_STATE_DIR", private.path().join("state"))
        .env("HGS_CONFIG_DIR", private.path().join("config"))
        .env("ZERUS_ARCHIVE_REFRESH_PROBE", case)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_cold_catalog_leaves_archive_body_reads_to_background_work() {
    probe("cold");
}

#[test]
fn a_replacement_archive_is_read_once_for_search_and_reference_metadata() {
    probe("replacement");
}

macro_rules! controls {
    ($($name:ident => $case:literal),* $(,)?) => {$(#[test] fn $name() {probe($case);})*};
}
controls! {
    a_worker_slice_has_a_record_and_byte_bound => "slice",
    committed_progress_survives_a_worker_restart_without_rereading => "restart",
    additions_and_removals_never_reuse_a_stale_absence_proof => "membership",
    identical_size_and_mtime_do_not_hide_a_replaced_file => "stamp",
    unchanged_coverage_does_not_rewrite_the_catalog => "unchanged",
    corrupt_routing_rows_cannot_mint_presence_or_absence => "corrupt",
    reference_projection_limits_do_not_prune_valid_search_input => "projection",
    corrupt_native_manifests_never_mint_presence_or_absence => "manifest",
    search_omission_never_hides_a_native_archive_identity => "shadow",
    a_late_worker_never_recreates_a_removed_state_root => "removed-root",
    a_page_keeps_one_cache_revision_while_a_worker_commits => "page-snapshot",
}

fn work() -> (usize, usize) {
    archive_references::METADATA_WORK.with(std::cell::Cell::get)
}

fn reset_work() {
    archive_references::METADATA_WORK.with(|work| work.set((0, 0)));
}

fn reconciled() -> Catalog {
    assert_eq!(
        worker().unwrap(),
        0,
        "private bounded worker did not complete"
    );
    catalog().unwrap()
}

#[test]
#[ignore = "private process and state root"]
fn isolated_catalog_probe() {
    let Ok(case) = std::env::var("ZERUS_ARCHIVE_REFRESH_PROBE") else {
        return;
    };
    private_dir(&root().join("archives")).unwrap();
    let mut originals = Vec::new();
    let count = if matches!(case.as_str(), "slice" | "restart") {
        150
    } else {
        8
    };
    for index in 0..count {
        let id = uuid::Uuid::from_u128(index + 1).to_string();
        let mut value = json!({"version":1,"archive_id":id,"agent":"codex",
            "name":format!("codex/example/archive-{index}"),
            "run_id":format!("run-{index}"),"conversation_id":format!("conversation-{index}"),
            "archived_at":index+1,"created":1,"cwd":"/srv/example/archive",
            "shell_jobs":{"unknown":{"status":"unknown","output":"x".repeat(4096)}}});
        if case == "shadow" && index == 0 {
            value["rename_shadow"] = json!(true);
            value["rename_target"] = json!("codex/example/renamed");
            value["legacy_shadow"] = json!({"run_id":value["run_id"],"name":value["name"]});
        }
        if case == "stamp" {
            value["title"] = json!("initial");
        }
        if case == "projection" && index == 0 {
            value["cwd"] = json!(format!("/srv/example/{}", "x".repeat(70 << 10)));
        }
        let path = archive::path(&id).unwrap();
        let bytes = serde_json::to_vec(&value).unwrap();
        fs::write(&path, &bytes).unwrap();
        originals.push((path, bytes));
    }
    reset_work();
    if case == "cold" {
        let result = catalog();
        assert_eq!(
            work(),
            (0, 0),
            "a request rebuilt the archive catalog inline"
        );
        if let Ok(db) = result {
            let missing = json!({"name":"codex/example/missing","run_id":"missing","conversation_id":"missing"});
            assert!(
                contains(&db, &missing).is_err(),
                "partial coverage became absence"
            );
            assert!(
                find(&db, &missing).is_err(),
                "partial coverage became absence"
            );
        }
    } else if case == "replacement" {
        drop(reconciled());
        let (path, bytes) = &mut originals[0];
        let mut value: Value = serde_json::from_slice(bytes).unwrap();
        value["title"] = json!("replacement archive");
        *bytes = serde_json::to_vec(&value).unwrap();
        let replacement = path.with_extension("next");
        fs::write(&replacement, &*bytes).unwrap();
        fs::rename(replacement, &*path).unwrap();
        reset_work();
        drop(reconciled());
        assert_eq!(
            work(),
            (1, bytes.len()),
            "a replacement was read repeatedly"
        );
    } else if matches!(case.as_str(), "slice" | "restart") {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let mut builder = refresh::Builder::new(until).unwrap();
        assert!(matches!(builder.step().unwrap(), refresh::Step::Progress));
        assert!(matches!(builder.step().unwrap(), refresh::Step::Progress));
        let partial = work();
        assert!(partial.0 > 0 && partial.0 <= 64, "{partial:?}");
        assert!(partial.1 <= 32 << 20, "{partial:?}");
        assert!(
            catalog().is_err(),
            "a partial scan became complete evidence"
        );
        drop(builder);
        if case == "restart" {
            drop(reconciled());
            assert_eq!(
                work().0,
                count as usize,
                "restart reread committed native bodies"
            );
        }
    } else if case == "membership" {
        let missing =
            json!({"name":"codex/example/missing","run_id":"missing","conversation_id":"missing"});
        let held = reconciled();
        assert_eq!(find(&held, &missing).unwrap(), None);
        let mut arrived: Value = serde_json::from_slice(&originals[0].1).unwrap();
        arrived["archive_id"] = json!(uuid::Uuid::from_u128(999).to_string());
        arrived["name"] = json!("codex/example/arrived");
        arrived["run_id"] = json!("arrived-run");
        arrived["conversation_id"] = json!("arrived-conversation");
        let path = archive::path(string(&arrived, "archive_id")).unwrap();
        let bytes = serde_json::to_vec(&arrived).unwrap();
        fs::write(&path, &bytes).unwrap();
        originals.push((path, bytes));
        assert!(
            find(&held, &arrived).is_err(),
            "a changed namespace reused an old absence proof"
        );
        drop(held);
        let fresh = reconciled();
        assert!(find(&fresh, &arrived).unwrap().is_some());
        drop(fresh);
        let (path, bytes) = originals.remove(0);
        let removed: Value = serde_json::from_slice(&bytes).unwrap();
        fs::remove_file(path).unwrap();
        reset_work();
        let fresh = reconciled();
        assert_eq!(work(), (0, 0), "removal reread unrelated bodies");
        assert_eq!(find(&fresh, &removed).unwrap(), None);
    } else if case == "stamp" {
        drop(reconciled());
        let (path, bytes) = &mut originals[0];
        let modified = fs::metadata(&*path).unwrap().modified().unwrap();
        let mut value: Value = serde_json::from_slice(bytes).unwrap();
        value["title"] = json!("changed");
        let replacement = serde_json::to_vec(&value).unwrap();
        assert_eq!(replacement.len(), bytes.len());
        let next = path.with_extension("next");
        fs::write(&next, &replacement).unwrap();
        fs::File::open(&next)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(modified))
            .unwrap();
        fs::rename(next, &*path).unwrap();
        *bytes = replacement;
        assert_eq!(fs::metadata(&*path).unwrap().modified().unwrap(), modified);
        reset_work();
        drop(reconciled());
        assert_eq!(
            work(),
            (1, bytes.len()),
            "same-size/time replacement reused a stale native stamp"
        );
    } else if case == "unchanged" {
        drop(reconciled());
        let path = root().join("archive-catalog-v1.sqlite3");
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        reset_work();
        drop(reconciled());
        assert_eq!(work(), (0, 0));
        assert_eq!(
            fs::metadata(path).unwrap().modified().unwrap(),
            modified,
            "unchanged worker rewrote the index"
        );
    } else if case == "corrupt" {
        drop(reconciled());
        let expected: Value = serde_json::from_slice(&originals[0].1).unwrap();
        let mut foreign = expected.clone();
        foreign["run_id"] = json!("foreign-run");
        {
            let db = Connection::open(root().join("archive-catalog-v1.sqlite3")).unwrap();
            db.execute(
                "UPDATE archive_rows SET run='foreign-run' WHERE id=?",
                [string(&expected, "archive_id")],
            )
            .unwrap();
        }
        let stale = catalog().unwrap();
        assert!(
            find(&stale, &expected).unwrap().is_some(),
            "display corruption hid native presence"
        );
        assert_eq!(
            find(&stale, &foreign).unwrap(),
            None,
            "display corruption minted foreign presence"
        );
        drop(stale);
        reset_work();
        let repaired = reconciled();
        assert_eq!(work().0, 1, "repair reread unaffected native files");
        assert!(find(&repaired, &expected).unwrap().is_some());
    } else if case == "manifest" {
        drop(reconciled());
        let expected: Value = serde_json::from_slice(&originals[0].1).unwrap();
        let mut foreign = expected.clone();
        foreign["run_id"] = json!("foreign-run");
        {
            let db = Connection::open(root().join("archive-catalog-v1.sqlite3")).unwrap();
            db.execute(
                "UPDATE archive_identities SET run='foreign-run' WHERE id=?",
                [string(&expected, "archive_id")],
            )
            .unwrap();
        }
        let stale = catalog().unwrap();
        assert!(
            find(&stale, &expected).is_err(),
            "incomplete native manifest became absence"
        );
        assert!(
            find(&stale, &foreign).is_err(),
            "a forged native identity became presence"
        );
        drop(stale);
        reset_work();
        let repaired = reconciled();
        assert_eq!(work().0, 1);
        assert!(find(&repaired, &expected).unwrap().is_some());
    } else if case == "shadow" {
        let db = reconciled();
        let rows: u64 = db
            .query_row("SELECT count(*) FROM archive_rows", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 7, "raw-label search omission changed");
        let mut expected: Value = serde_json::from_slice(&originals[0].1).unwrap();
        super::super::rename::normalize_archive(&mut expected).unwrap();
        assert!(
            find(&db, &expected).unwrap().is_some(),
            "display omission hid a retained native identity"
        );
        let missing =
            json!({"name":"codex/example/missing","run_id":"missing","conversation_id":"missing"});
        assert_eq!(find(&db, &missing).unwrap(), None);
    } else if case == "page-snapshot" {
        drop(reconciled());
        let db = Connection::open(root().join("archive-catalog-v1.sqlite3")).unwrap();
        db.execute_batch("PRAGMA journal_mode=WAL").unwrap();
        drop(db);
        fn worker_commit() {
            let db = Connection::open(root().join("archive-catalog-v1.sqlite3")).unwrap();
            db.execute(
                "DELETE FROM archive_rows WHERE id>?",
                [uuid::Uuid::from_u128(2).to_string()],
            )
            .unwrap();
        }
        for bounded in [false, true] {
            drop(reconciled());
            AFTER_PAGE_COUNT.with(|slot| slot.set(Some(worker_commit)));
            let page = page_with(&json!({}), bounded).unwrap();
            assert_eq!(page["total"], json!(8));
            assert_eq!(
                page["sessions"].as_array().unwrap().len(),
                8,
                "one page mixed cache revisions"
            );
            let db = Connection::open(root().join("archive-catalog-v1.sqlite3")).unwrap();
            let remaining: u64 = db
                .query_row("SELECT count(*) FROM archive_rows", [], |row| row.get(0))
                .unwrap();
            assert_eq!(remaining, 2, "the independent cache writer did not commit");
        }
    } else if case == "removed-root" {
        drop(reconciled());
        let moved = root().with_extension("retained");
        fs::rename(root(), &moved).unwrap();
        let result = worker();
        let recreated = root().exists();
        fs::rename(moved, root()).unwrap();
        assert!(result.is_err());
        assert!(!recreated, "late worker recreated the removed state root");
    } else if case == "projection" {
        let db = reconciled();
        let count: u64 = db
            .query_row("SELECT count(*) FROM archive_rows", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            count, 8,
            "reference projection refusal pruned valid search input"
        );
        let missing =
            json!({"name":"codex/example/missing","run_id":"missing","conversation_id":"missing"});
        assert_eq!(find(&db, &missing).unwrap(), None);
        drop(db);
        let until = std::time::Instant::now() + std::time::Duration::from_secs(2);
        assert!(
            archive_references::visit_metadata(until, |_| Ok(())).is_err(),
            "search completeness authorized an incomplete folder proof"
        );
    } else {
        panic!("unknown private catalog probe");
    }
    for (path, bytes) in originals {
        assert_eq!(
            fs::read(path).unwrap(),
            bytes,
            "catalog readers rewrote canonical evidence"
        );
    }
}
