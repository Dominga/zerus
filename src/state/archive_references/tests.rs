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
        .current_dir(directory.path())
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", directory.path())
        .env("HGS_CONFIG_DIR", directory.path().join("config"))
        .env("HGS_STATE_DIR", directory.path().join("state"))
        .env("HGS_TEST_ARCHIVE_REFERENCES", case);
    if let Some(system) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", system);
    }
    let output = if case.starts_with("native-") {
        use std::process::Stdio;
        let marker = directory.path().join("entered");
        command
            .env("HGS_TEST_ARCHIVE_READY", &marker)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().unwrap();
        let setup = Instant::now() + Duration::from_secs(30);
        let mut entered = None;
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            if entered.is_none() && marker.exists() {
                entered = Some(Instant::now());
            }
            if entered.is_some_and(|at: Instant| at.elapsed() > Duration::from_secs(3))
                || Instant::now() > setup
            {
                child.kill().unwrap();
                let output = child.wait_with_output().unwrap();
                panic!(
                    "{case}: private reader did not settle: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        child.wait_with_output().unwrap()
    } else {
        command.output().unwrap()
    };
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
#[cfg(windows)]
#[test]
fn native_receipt_publication_keeps_directory_pins_and_honors_a_live_reader() {
    run("native-receipt-publication");
}

#[test]
fn a_changed_relevant_body_is_never_overwritten_by_a_preloaded_record() {
    run("changed");
}
#[test]
fn the_regular_reader_releases_original_files_and_its_private_vfs() {
    run("resources");
}
#[test]
fn an_indexed_row_with_a_valid_digest_cannot_replace_its_authoritative_body() {
    run("body-row");
}
#[test]
fn ordinary_listing_repairs_reference_coverage_after_an_older_catalog_writer() {
    run("old-catalog");
}
#[cfg(unix)]
#[test]
fn native_reference_opens_refuse_a_final_symlink_and_a_fifo() {
    run("native-open");
}
#[cfg(unix)]
#[test]
fn replacing_the_archive_directory_after_its_type_check_cannot_block() {
    for case in [
        "native-directory-fifo",
        "native-directory-file",
        "native-directory-link",
    ] {
        run(case);
    }
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

#[cfg(unix)]
#[test]
fn irregular_catalog_and_sidecars_refuse_before_a_blocking_open() {
    for case in [
        "native-catalog",
        "native-journal",
        "native-wal",
        "native-shm",
        "native-writer",
        "native-swap-catalog",
        "native-swap-journal",
        "native-swap-wal",
        "native-swap-shm",
    ] {
        run(case);
    }
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
    #[cfg(unix)]
    if let Some(which) = case.strip_prefix("native-") {
        use std::os::unix::ffi::OsStrExt;
        if which.starts_with("directory-") {
            AFTER_DIRECTORY_TYPE.with(|slot| slot.set(Some(replace_after_directory_type)));
            fs::write(
                std::env::var_os("HGS_TEST_ARCHIVE_READY").unwrap(),
                b"ready",
            )
            .unwrap();
            let begin = Instant::now();
            assert!(visit_metadata(begin + Duration::from_millis(100), |_| Ok(())).is_err());
            assert!(begin.elapsed() < Duration::from_secs(1));
            return;
        }
        if which == "open" {
            let regular = root().join("regular-input");
            fs::write(&regular, b"bounded native data").unwrap();
            assert!(open(&regular).is_ok());
            let link = root().join("input-link");
            std::os::unix::fs::symlink(&regular, &link).unwrap();
            assert!(
                open(&link).is_err(),
                "a final link is never regular native input"
            );
            let fifo = root().join("input-fifo");
            let name = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
            assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
            fs::write(
                std::env::var_os("HGS_TEST_ARCHIVE_READY").unwrap(),
                b"ready",
            )
            .unwrap();
            assert!(open(&fifo).is_err());
            return;
        }
        let swap = which.starts_with("swap-");
        let which = which.strip_prefix("swap-").unwrap_or(which);
        let suffix = if which == "catalog" || which == "writer" {
            "".to_owned()
        } else {
            format!("-{which}")
        };
        let input = root().join(format!("archive-catalog-v1.sqlite3{suffix}"));
        if swap {
            native_reader::AFTER_PIN.with(|hook| hook.set(Some(replace_after_reader_pin)));
        } else {
            if input.exists() {
                fs::remove_file(&input).unwrap();
            }
            let raw = std::ffi::CString::new(input.as_os_str().as_bytes()).unwrap();
            assert_eq!(
                unsafe {
                    libc::mkfifo(raw.as_ptr(), if which == "writer" { 0o400 } else { 0o600 })
                },
                0
            );
        }
        fs::write(
            std::env::var_os("HGS_TEST_ARCHIVE_READY").unwrap(),
            b"ready",
        )
        .unwrap();
        let begin = Instant::now();
        if which == "writer" {
            atomic_record(&path, &mut original).unwrap();
            assert!(begin.elapsed() < Duration::from_secs(1));
            return;
        }
        assert!(visit_metadata(begin + Duration::from_millis(100), |_| Ok(())).is_err());
        assert!(begin.elapsed() < Duration::from_secs(1));
        return;
    }
    match case.as_str() {
        "resources" => native_reader::fixture_settles(),
        "body-row" => {
            let (expected, mut indexed, _) = entry(&database(), id).unwrap().unwrap();
            indexed.cwd = target.join("indexed-only").to_string_lossy().into_owned();
            // Keep the exact native version and recompute both digest and path
            // rows. Only the authoritative body comparison can reject this.
            store(&database(), &indexed, &expected).unwrap();
            assert!(metadata().is_ok());
            let mut calls = 0;
            assert!(visit_related(until(), &target, "original-owner", |_| {
                calls += 1;
                Ok(())
            })
            .is_err());
            assert_eq!(calls, 0);
            assert_eq!(fs::read_to_string(&path).unwrap(), original.to_string());
        }
        "old-catalog" => {
            let catalog = || drop(super::super::archive_catalog::catalog().unwrap());
            // An upgraded catalog without the new reference stamp must make
            // one ordinary reconciliation even if its search stamp is warm.
            database()
                .execute(
                    "DELETE FROM catalog_meta WHERE key='archive_references_stamp'",
                    [],
                )
                .unwrap();
            database()
                .execute("DELETE FROM archive_references", [])
                .unwrap();
            assert!(metadata().is_err());
            catalog();
            assert_eq!(related(&target).unwrap(), vec![original.clone()]);

            let legacy_id = "22222222-2222-4222-8222-222222222222";
            let mut legacy = record(legacy_id, &target);
            legacy.as_object_mut().unwrap().remove("worktree_owner");
            put(&legacy); // raw old writer: no new per-ID update
            original["cwd"] = json!(target.join("changed"));
            put(&original);
            // The old executable warms only its search catalog. It knows
            // nothing of references or their separate reconciliation stamp.
            let stamp = || {
                let m = fs::metadata(root().join("archives")).unwrap();
                format!("{}:{:?}", m.len(), m.modified().unwrap())
            };
            let current = stamp();
            database()
                .execute(
                    "INSERT OR REPLACE INTO catalog_meta VALUES('stamp',?)",
                    [&current],
                )
                .unwrap();
            assert!(metadata().is_err());
            let directory = directory_stamp(&root().join("archives")).unwrap();
            catalog();
            assert_eq!(
                directory_stamp(&root().join("archives")).unwrap(),
                directory
            );
            assert_eq!(related(&target).unwrap(), vec![original.clone(), legacy]);
            fs::remove_file(super::super::archive::path(legacy_id).unwrap()).unwrap();
            let current = stamp();
            database()
                .execute(
                    "UPDATE catalog_meta SET value=? WHERE key='stamp'",
                    [&current],
                )
                .unwrap();
            catalog();
            assert_eq!(related(&target).unwrap(), vec![original]);
            assert!(entry(&database(), legacy_id).unwrap().is_none());
            let index = root().join("archive-catalog-v1.sqlite3");
            let before = file_version(&index).unwrap();
            catalog();
            assert_eq!(
                file_version(&index).unwrap(),
                before,
                "warm pages do not rewrite the index"
            );
        }
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
            // Preserve every legacy UUID spelling accepted by archive::path,
            // including the braced form (longer than a canonical UUID).
            let braced = record("{55555555-5555-4555-8555-555555555555}", &target);
            put(&braced);
            // Same mutable label, distinct durable archive/session identities.
            assert_eq!(original["name"], legacy["name"]);
            if case == "routed" {
                super::super::session_id::select(
                    string(&original, "name"),
                    string(&original, "session_id"),
                );
            }
            reconcile();
            assert_eq!(related(&target).unwrap(), vec![original, legacy, braced]);
            assert_eq!(metadata().unwrap().len(), 5);
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
        #[cfg(windows)]
        "native-receipt-publication" => {
            updates::test_native_publication(&root());
        }
        "writers" => {
            original["name"] = json!("codex/p/renamed");
            original["cwd"] = json!(root().join("elsewhere"));
            original["launch_dir"] = original["cwd"].clone();
            atomic_record(&path, &mut original).unwrap();
            assert_eq!(metadata().unwrap()[0]["name"], original["name"]);
            let files = fs::read_dir(root().join("archive-reference-writes-v1"))
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .collect::<Vec<_>>();
            assert_eq!(files.len(), 1);
            let update = &files[0];
            let bytes = fs::read(update).unwrap();
            fs::write(update, b"{invalid writer receipt").unwrap();
            assert!(
                metadata().is_err(),
                "a bad writer receipt cannot adopt a stale base row"
            );
            fs::write(update, &bytes).unwrap();
            assert_eq!(metadata().unwrap()[0]["name"], original["name"]);
            fs::remove_file(update).unwrap();
            assert!(
                metadata().is_err(),
                "a missing writer receipt cannot adopt a stale base row"
            );
            fs::write(update, &bytes).unwrap();
            assert!(related(&target).unwrap().is_empty());
            assert_eq!(
                related(Path::new(string(&original, "cwd"))).unwrap(),
                vec![original.clone()]
            );
            super::super::archive::remove_synced(&path).unwrap();
            assert!(metadata().unwrap().is_empty());
            // Orphan base rows are harmless; ordinary reconciliation retires
            // them without a fresh SQLite open in the removal write hook.
            reconcile();
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

#[cfg(unix)]
fn replace_after_directory_type(path: &Path) {
    use std::os::unix::ffi::OsStrExt;
    assert_eq!(path, root().join("archives"));
    let original = root().join("retained-archives");
    fs::rename(path, &original).unwrap();
    match std::env::var("HGS_TEST_ARCHIVE_REFERENCES")
        .unwrap()
        .as_str()
    {
        "native-directory-fifo" => {
            let name = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
            assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        }
        "native-directory-file" => fs::write(path, b"not a directory").unwrap(),
        "native-directory-link" => std::os::unix::fs::symlink(original, path).unwrap(),
        _ => panic!("unexpected directory replacement control"),
    }
}

#[cfg(unix)]
fn replace_after_reader_pin() {
    use std::os::unix::ffi::OsStrExt;
    let case = std::env::var("HGS_TEST_ARCHIVE_REFERENCES").unwrap();
    let which = case.strip_prefix("native-swap-").unwrap();
    let suffix = if which == "catalog" {
        String::new()
    } else {
        format!("-{which}")
    };
    let path = root().join(format!("archive-catalog-v1.sqlite3{suffix}"));
    if path.exists() {
        fs::rename(&path, root().join("retained-original-catalog")).unwrap();
    }
    let name = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
}
