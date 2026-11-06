use super::*;

fn until() -> Instant {
    Instant::now() + Duration::from_secs(2)
}
fn record(id: &str, folder: &Path) -> Value {
    json!({"version":1,"archive_id":id,"agent":"codex","name":"codex/p/original",
        "session_id":uuid::Uuid::new_v4().to_string(),"run_id":uuid::Uuid::new_v4().to_string(),
        "conversation_id":uuid::Uuid::new_v4().to_string(),"archived_at":1,
        "cwd":folder,"launch_dir":folder,"worktree_owner":{"id":"original-owner"}})
}
fn put(record: &Value) -> PathBuf {
    private_dir(&root().join("archives")).unwrap();
    let path = super::super::archive::path(string(record, "archive_id")).unwrap();
    atomic(&path, &record.to_string()).unwrap();
    path
}
fn metadata() -> Result<Vec<Value>> {
    let mut rows = vec![];
    visit_metadata(until(), |row| {
        rows.push(row);
        Ok(())
    })?;
    Ok(rows)
}
fn related(folder: &Path) -> Result<Vec<Value>> {
    let mut rows = vec![];
    visit_related(until(), folder, "original-owner", |row| {
        rows.push(row);
        Ok(())
    })?;
    Ok(rows)
}
fn reconcile() {
    // Simulate an ordinary namespace revision; hot guards must not repair a
    // corrupt cache themselves, and warm archive pages keep their fast path.
    if let Ok(db) = Connection::open_with_flags(
        root().join("archive-catalog-v1.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_WRITE,
    ) {
        db.execute("DELETE FROM catalog_meta WHERE key='stamp'", [])
            .unwrap();
    }
    drop(super::super::archive_catalog::catalog().unwrap());
}
fn database() -> Connection {
    Connection::open(root().join("archive-catalog-v1.sqlite3")).unwrap()
}
fn run(case: &str) {
    let directory = tempfile::tempdir().unwrap();
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "state::archive_references::tests::isolated_reference_probe",
            "--exact",
            "--nocapture",
        ])
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", directory.path())
        .env("HGS_CONFIG_DIR", directory.path().join("config"))
        .env("HGS_STATE_DIR", directory.path().join("state"))
        .env("HGS_TEST_ARCHIVE_REFERENCES", case);
    if let Some(system) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", system);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{case}: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn missing_corrupt_or_incomplete_reference_coverage_never_means_empty() {
    for case in ["missing", "corrupt", "paths", "invalid", "timeout"] {
        run(case);
    }
}
#[test]
fn every_matching_archive_is_selected_with_legacy_and_label_routing_preserved() {
    run("selection");
    run("routed");
}
#[test]
fn ordinary_writes_rewrites_and_removal_keep_reference_coverage_current() {
    run("writers");
}
#[test]
fn a_changed_relevant_body_is_never_overwritten_by_a_preloaded_record() {
    run("changed");
}
#[cfg(unix)]
#[test]
fn an_in_place_rewrite_with_restored_mtime_invalidates_reference_coverage() {
    run("in-place");
}
#[cfg(unix)]
#[test]
fn a_dangling_or_directory_symlink_is_not_an_empty_archive() {
    run("symlink");
}

#[test]
fn isolated_reference_probe() {
    let Ok(case) = std::env::var("HGS_TEST_ARCHIVE_REFERENCES") else {
        return;
    };
    assert!(metadata().unwrap().is_empty());
    private_dir(&root().join("archives")).unwrap();
    assert!(metadata().unwrap().is_empty());
    let target = root().join("checkout");
    let id = "11111111-1111-4111-8111-111111111111";
    let mut original = record(id, &target);
    let path = put(&original);
    if case == "missing" {
        assert!(metadata().is_err());
        reconcile();
        assert_eq!(related(&target).unwrap(), vec![original.clone()]);
        database()
            .execute("DELETE FROM archive_references WHERE id=?", [id])
            .unwrap();
        assert!(metadata().is_err());
        // A first migration cut before its transaction commits must reconcile
        // again even when the archive directory generation has not changed.
        database()
            .execute(
                "DELETE FROM catalog_meta WHERE key='archive_references_v1'",
                [],
            )
            .unwrap();
        drop(super::super::archive_catalog::catalog().unwrap());
        assert_eq!(related(&target).unwrap(), vec![original]);
        return;
    }
    reconcile();
    assert_eq!(related(&target).unwrap(), vec![original.clone()]);
    match case.as_str() {
        "corrupt" | "paths" => {
            if case == "corrupt" {
                database()
                    .execute(
                        "UPDATE archive_references SET digest='0000000000000000000000000000000000000000000000000000000000000000' WHERE id=?",
                        [id],
                    )
                    .unwrap();
            } else {
                database()
                    .execute("DELETE FROM archive_reference_paths WHERE id=?", [id])
                    .unwrap();
            }
            assert!(metadata().is_err());
            assert!(related(&target).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), original.to_string());
            reconcile();
            assert_eq!(related(&target).unwrap(), vec![original]);
        }
        "invalid" => {
            let invalid =
                super::super::archive::path("22222222-2222-4222-8222-222222222222").unwrap();
            atomic(&invalid, "{invalid").unwrap();
            reconcile();
            assert!(metadata().is_err());
            assert!(related(&target).is_err());
        }
        "timeout" => {
            let mut calls = 0;
            assert!(
                visit_related(Instant::now(), &target, "original-owner", |_| {
                    calls += 1;
                    Ok(())
                })
                .is_err()
            );
            assert_eq!(calls, 0);
        }
        "selection" | "routed" => {
            let mut legacy = record(
                "22222222-2222-4222-8222-222222222222",
                &target.join("nested"),
            );
            legacy.as_object_mut().unwrap().remove("worktree_owner");
            put(&legacy);
            let outside = record(
                "33333333-3333-4333-8333-333333333333",
                &target.with_file_name("checkout-other"),
            );
            put(&outside);
            let mut foreign = record("44444444-4444-4444-8444-444444444444", &target);
            foreign["worktree_owner"]["id"] = json!("foreign-owner");
            put(&foreign);
            // Same mutable label, distinct durable archive/session identities.
            assert_eq!(original["name"], legacy["name"]);
            if case == "routed" {
                super::super::session_id::select(
                    string(&original, "name"),
                    string(&original, "session_id"),
                );
            }
            reconcile();
            assert_eq!(related(&target).unwrap(), vec![original, legacy]);
            assert_eq!(metadata().unwrap().len(), 4);
            if case == "routed" {
                assert_eq!(
                    database()
                        .query_row("SELECT count(*) FROM archive_rows", [], |r| r
                            .get::<_, u64>(0))
                        .unwrap(),
                    1,
                    "the global restore index must not inherit scoped search omissions"
                );
            }
        }
        "writers" => {
            original["name"] = json!("codex/p/renamed");
            original["cwd"] = json!(root().join("elsewhere"));
            original["launch_dir"] = original["cwd"].clone();
            atomic_record(&path, &mut original).unwrap();
            assert_eq!(metadata().unwrap()[0]["name"], original["name"]);
            assert!(related(&target).unwrap().is_empty());
            assert_eq!(
                related(Path::new(string(&original, "cwd"))).unwrap(),
                vec![original.clone()]
            );
            super::super::archive::remove_synced(&path).unwrap();
            assert!(metadata().unwrap().is_empty());
            assert_eq!(
                database()
                    .query_row("SELECT count(*) FROM archive_references", [], |r| r
                        .get::<_, u64>(0))
                    .unwrap(),
                0
            );
            atomic_record(&path, &mut original).unwrap();
            assert_eq!(metadata().unwrap().len(), 1);
        }
        "changed" => {
            let second = record("22222222-2222-4222-8222-222222222222", &target);
            let second_path = put(&second);
            reconcile();
            let mut calls = 0;
            assert!(visit_related(until(), &target, "original-owner", |_| {
                calls += 1;
                atomic(&second_path, "{replacement")
            })
            .is_err());
            assert_eq!(calls, 1);
            assert_eq!(fs::read_to_string(second_path).unwrap(), "{replacement");
        }
        #[cfg(unix)]
        "in-place" => {
            let prior = fs::metadata(&path).unwrap();
            original["name"] = json!("codex/p/modified");
            assert_eq!(original.to_string().len(), prior.len() as usize);
            std::thread::sleep(Duration::from_millis(2));
            fs::write(&path, original.to_string()).unwrap();
            fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_times(fs::FileTimes::new().set_modified(prior.modified().unwrap()))
                .unwrap();
            assert!(metadata().is_err());
            assert!(related(&target).is_err());
            reconcile();
            assert_eq!(related(&target).unwrap(), vec![original]);
        }
        #[cfg(unix)]
        "symlink" => {
            use std::os::unix::fs::symlink;
            fs::rename(root().join("archives"), root().join("old")).unwrap();
            symlink(root().join("missing"), root().join("archives")).unwrap();
            assert!(metadata().is_err());
            fs::remove_file(root().join("archives")).unwrap();
            symlink(root().join("old"), root().join("archives")).unwrap();
            assert!(metadata().is_err());
        }
        _ => panic!("unknown reference fixture"),
    }
}
