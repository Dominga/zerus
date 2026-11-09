use super::super::Store;
use super::*;

fn epoch() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn fixture(temp: &tempfile::TempDir) -> Location {
    let location = Location {
        root: temp.path().canonicalize().unwrap().join("state/desktop"),
        configuration: "a".repeat(64),
    };
    location.prepare().unwrap();
    location
}
fn current(location: &Location) {
    drop(Store::open(location, &epoch()).unwrap());
}
fn schema(path: &std::path::Path) -> i64 {
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    db.query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn held_handoff_allows_only_passive_missing_empty_and_current_startup() {
    for shape in ["missing", "empty", "current"] {
        let temp = tempfile::tempdir().unwrap();
        let location = fixture(&temp);
        let owner = location.lock().unwrap();
        let path = location.root.join(DATABASE);
        match shape {
            "empty" => {
                crate::platform::private_open(fs::OpenOptions::new().write(true).create_new(true))
                    .open(&path)
                    .unwrap();
            }
            "current" => current(&location),
            _ => (),
        }
        let hold = location.handoff_lock().unwrap();
        let store = Store::open_owned(&location, &epoch(), &owner).unwrap();
        assert_eq!(schema(&path), SCHEMA);
        assert!(
            location.try_handoff_lock().unwrap().is_none(),
            "original {shape} hold stays held"
        );
        assert_eq!(store.effect_status()["revoked"], true);
        assert!(!location.root.join("schema-34-rollback").exists());
        drop(store);
        assert!(location.try_handoff_lock().unwrap().is_none());
        drop(hold);
        assert!(location.try_handoff_lock().unwrap().is_some());
    }
}

#[cfg(windows)]
#[test]
fn a_live_store_keeps_its_database_handle_but_not_startup_only_parent_pins() {
    use std::io::Write;
    let temp = tempfile::tempdir().unwrap();
    let location = fixture(&temp);
    let owner = location.lock().unwrap();
    let store = Store::open_owned(&location, &epoch(), &owner).unwrap();
    // The ordinary handoff writer uses this same pathname publication lane.
    // Startup's parent guard must not become a runtime-wide deny-write pin.
    for bytes in [b"first".as_slice(), b"replacement".as_slice()] {
        let mut publication = tempfile::NamedTempFile::new_in(&location.root).unwrap();
        publication.write_all(bytes).unwrap();
        publication.as_file().sync_all().unwrap();
        platform::persist(publication, location.root.join("runtime-publication")).unwrap();
        assert_eq!(
            fs::read(location.root.join("runtime-publication")).unwrap(),
            bytes
        );
    }
    assert_eq!(store.effect_status()["revoked"], true);
}

#[test]
fn zero_schema_never_adopts_user_objects_including_sqlite_lookalikes() {
    for sql in [
        "CREATE TABLE original(value); INSERT INTO original VALUES('kept')",
        "CREATE TABLE sqliteLookalike(value); INSERT INTO sqliteLookalike VALUES('kept')",
        "CREATE VIEW original AS SELECT 'kept' AS value",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let location = fixture(&temp);
        let path = location.root.join(DATABASE);
        drop(
            crate::platform::private_open(fs::OpenOptions::new().write(true).create_new(true))
                .open(&path)
                .unwrap(),
        );
        let db = Connection::open(&path).unwrap();
        db.execute_batch(sql).unwrap();
        drop(db);
        let before = fs::read(&path).unwrap();
        let owner = location.lock().unwrap();
        assert!(Store::open_owned(&location, &epoch(), &owner).is_err());
        assert_eq!(schema(&path), 0);
        assert_eq!(fs::read(&path).unwrap(), before);
        assert!(!location.root.join("schema-34-rollback").exists());
    }
}

#[test]
fn missing_or_zero_with_a_sidecar_refuses_without_claiming_empty() {
    for existing in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let location = fixture(&temp);
        let path = location.root.join(DATABASE);
        if existing {
            drop(
                crate::platform::private_open(fs::OpenOptions::new().write(true).create_new(true))
                    .open(&path)
                    .unwrap(),
            );
        }
        fs::write(
            location.root.join("state.sqlite3-journal"),
            b"retained original",
        )
        .unwrap();
        let owner = location.lock().unwrap();
        assert!(Store::open_owned(&location, &epoch(), &owner).is_err());
        assert_eq!(path.exists(), existing);
        assert_eq!(
            fs::read(location.root.join("state.sqlite3-journal")).unwrap(),
            b"retained original"
        );
    }
}

#[test]
fn newer_schema_and_configuration_mismatch_are_not_migrated_or_reclassified() {
    for query in [
        "PRAGMA user_version=99",
        "UPDATE service SET configuration='other'",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let location = fixture(&temp);
        current(&location);
        let path = location.root.join(DATABASE);
        let db = Connection::open(&path).unwrap();
        db.execute_batch(query).unwrap();
        drop(db);
        let before = fs::read(&path).unwrap();
        let owner = location.lock().unwrap();
        let _hold = location.handoff_lock().unwrap();
        assert!(Store::open_owned(&location, &epoch(), &owner).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        assert!(!location.root.join("schema-34-rollback").exists());
    }
}

#[test]
fn an_appearing_file_is_not_adopted_after_positive_absence() {
    let temp = tempfile::tempdir().unwrap();
    let location = fixture(&temp);
    let owner = location.lock().unwrap();
    BEFORE_CREATE.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(|root| {
            let mut file =
                crate::platform::private_open(fs::OpenOptions::new().write(true).create_new(true))
                    .open(root.path.join(DATABASE))
                    .unwrap();
            std::io::Write::write_all(&mut file, b"original appearing file").unwrap();
        }))
    });
    let result = Store::open_owned(&location, &epoch(), &owner);
    BEFORE_CREATE.with(|hook| hook.borrow_mut().take());
    assert!(result.is_err());
    assert_eq!(
        fs::read(location.root.join(DATABASE)).unwrap(),
        b"original appearing file"
    );
}

#[test]
fn first_probe_drift_refuses_without_acquiring_migration_authority() {
    let temp = tempfile::tempdir().unwrap();
    let location = fixture(&temp);
    current(&location);
    let owner = location.lock().unwrap();
    let _hold = location.handoff_lock().unwrap();
    AFTER_OBSERVATION.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(|pin| {
            Connection::open(pin.root.path.join(DATABASE))
                .unwrap()
                .execute_batch("PRAGMA user_version=34")
                .unwrap();
        }))
    });
    let result = Store::open_owned(&location, &epoch(), &owner);
    AFTER_OBSERVATION.with(|hook| hook.borrow_mut().take());
    assert!(format!("{:#}", result.err().unwrap()).contains("changed"));
    assert_eq!(schema(&location.root.join(DATABASE)), 34);
    assert!(!location.root.join("schema-34-rollback").exists());
}

#[cfg(unix)]
#[test]
fn aliases_hardlinks_and_fifo_never_become_database_pins() {
    use std::os::unix::fs::symlink;
    for shape in ["symlink", "hardlink", "fifo"] {
        let temp = tempfile::tempdir().unwrap();
        let location = fixture(&temp);
        let owner = location.lock().unwrap();
        let foreign = temp.path().join("foreign");
        fs::write(&foreign, b"retained").unwrap();
        let path = location.root.join(DATABASE);
        match shape {
            "symlink" => symlink(&foreign, &path).unwrap(),
            "hardlink" => fs::hard_link(&foreign, &path).unwrap(),
            "fifo" => {
                use std::os::unix::ffi::OsStrExt;
                let path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
                assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
            }
            _ => unreachable!(),
        }
        assert!(Store::open_owned(&location, &epoch(), &owner).is_err());
        assert_eq!(fs::read(&foreign).unwrap(), b"retained");
    }
}

#[test]
fn the_real_schema_one_migration_waits_for_handoff_and_keeps_its_backup() {
    let temp = tempfile::tempdir().unwrap();
    let location = fixture(&temp);
    let path = location.root.join(DATABASE);
    drop(
        crate::platform::private_open(fs::OpenOptions::new().write(true).create_new(true))
            .open(&path)
            .unwrap(),
    );
    let db = Connection::open(&path).unwrap();
    db.execute_batch("CREATE TABLE service(id INTEGER PRIMARY KEY CHECK(id=1), configuration TEXT NOT NULL, generation INTEGER NOT NULL, epoch TEXT NOT NULL); PRAGMA user_version=1").unwrap();
    db.execute(
        "INSERT INTO service VALUES(1,?1,0,'')",
        [&location.configuration],
    )
    .unwrap();
    drop(db);
    let owner = location.lock().unwrap();
    let hold = location.handoff_lock().unwrap();
    let before = fs::read(&path).unwrap();
    assert!(Store::open_owned(&location, &epoch(), &owner).is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(!location.root.join("schema-34-rollback").exists());
    drop(hold);
    drop(Store::open_owned(&location, &epoch(), &owner).unwrap());
    assert_eq!(schema(&path), SCHEMA);
    let manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(location.root.join("schema-34-rollback/manifest.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest["source_schema"], 1);
}

#[cfg(unix)]
fn read_transaction(db: &Connection) {
    db.execute_batch("BEGIN").unwrap();
    db.query_row::<i64, _, _>("SELECT count(*) FROM sqlite_schema", [], |r| r.get(0))
        .unwrap();
}

#[cfg(unix)]
#[test]
fn actual_owned_multiopen_success_failure_and_drop_preserve_every_local_reader() {
    use crate::sqlite_file::foreign_lock_refused;
    let temp = tempfile::tempdir().unwrap();
    let location = fixture(&temp);
    let owner = location.lock().unwrap();
    let path = location.root.join(DATABASE);
    let first = Store::open_owned(&location, &epoch(), &owner).unwrap();
    read_transaction(&first.db);
    assert!(foreign_lock_refused(&path));
    let second = Store::open_owned(&location, &epoch(), &owner).unwrap();
    drop(second);
    assert!(foreign_lock_refused(&path));
    let wrong = Location {
        root: location.root.clone(),
        configuration: "b".repeat(64),
    };
    assert!(Store::open_owned(&wrong, &epoch(), &owner).is_err());
    assert!(foreign_lock_refused(&path));
    let external = Connection::open(&path).unwrap();
    read_transaction(&external);
    first.db.execute_batch("ROLLBACK").unwrap();
    drop(first);
    assert!(
        foreign_lock_refused(&path),
        "independent SQLite user outlives Store"
    );
    assert!(Store::open_owned(&wrong, &epoch(), &owner).is_err());
    assert!(foreign_lock_refused(&path));
    external.execute_batch("ROLLBACK").unwrap();
    drop(external);
    assert!(
        !foreign_lock_refused(&path),
        "SQLite last-close releases its own locks"
    );
}

#[cfg(unix)]
#[test]
fn a_replaced_owner_name_cannot_close_an_alias_of_an_independent_sqlite_reader() {
    use crate::sqlite_file::foreign_lock_refused;
    let temp = tempfile::tempdir().unwrap();
    let location = fixture(&temp);
    current(&location);
    let owner = location.lock().unwrap();
    let path = location.root.join(DATABASE);
    let external = Connection::open(&path).unwrap();
    read_transaction(&external);
    let owner_path = location.root.join("owner.lock");
    let saved = location.root.join("original-owner");
    fs::rename(&owner_path, &saved).unwrap();
    fs::hard_link(&path, &owner_path).unwrap();
    assert!(Store::open_owned(&location, &epoch(), &owner).is_err());
    assert!(
        foreign_lock_refused(&path),
        "checking the invalid exclusion name never opens/closes its DB alias"
    );
    fs::remove_file(&owner_path).unwrap();
    fs::rename(saved, &owner_path).unwrap();
    external.execute_batch("ROLLBACK").unwrap();
    drop(external);
    assert!(!foreign_lock_refused(&path));
}

#[cfg(unix)]
#[test]
fn migration_and_backup_reuse_the_original_pin_while_an_external_reader_is_live() {
    use crate::sqlite_file::foreign_lock_refused;
    let temp = tempfile::tempdir().unwrap();
    let location = fixture(&temp);
    let store = Store::open(&location, &epoch()).unwrap();
    crate::desktop::creation_worktree::rewind_slots(&store.db);
    store.db.execute_batch("PRAGMA user_version=34").unwrap();
    drop(store);
    let path = location.root.join(DATABASE);
    let external = Connection::open(&path).unwrap();
    read_transaction(&external);
    assert!(foreign_lock_refused(&path));
    let owner = location.lock().unwrap();
    let hold = location.handoff_lock().unwrap();
    assert!(Store::open_owned(&location, &epoch(), &owner).is_err());
    assert_eq!(schema(&path), 34);
    assert!(!location.root.join("schema-34-rollback").exists());
    assert!(foreign_lock_refused(&path));
    drop(hold);
    let migrated = Store::open_owned(&location, &epoch(), &owner).unwrap();
    assert_eq!(schema(&path), SCHEMA);
    assert!(location
        .root
        .join("schema-34-rollback/manifest.json")
        .is_file());
    assert!(
        foreign_lock_refused(&path),
        "backup copy/hash/name checks preserve the OTHER SQLite user's locks"
    );
    drop(migrated);
    assert!(foreign_lock_refused(&path));
    external.execute_batch("ROLLBACK").unwrap();
    drop(external);
    assert!(!foreign_lock_refused(&path));
}

#[cfg(unix)]
#[test]
fn production_rw_binding_rejects_a_b_a_and_schema_drift() {
    for swap in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let location = fixture(&temp);
        current(&location);
        let owner = location.lock().unwrap();
        super::super::AROUND_OPEN.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move |path, opened| {
                let a = path.with_extension("original");
                let b = path.with_extension("other");
                if swap {
                    if !opened {
                        fs::rename(path, &a).unwrap();
                        crate::sqlite_file::create(path).unwrap();
                    } else {
                        fs::rename(path, b).unwrap();
                        fs::rename(a, path).unwrap();
                    }
                } else if !opened {
                    Connection::open(path)
                        .unwrap()
                        .execute_batch("PRAGMA user_version=34")
                        .unwrap();
                }
            }))
        });
        let result = Store::open_owned(&location, &epoch(), &owner);
        super::super::AROUND_OPEN.with(|hook| hook.borrow_mut().take());
        assert!(result.is_err());
        assert_eq!(
            schema(&location.root.join(DATABASE)),
            if swap { SCHEMA } else { 34 }
        );
        assert!(!location.root.join("schema-34-rollback").exists());
    }
}

// Run this one in its OWN process: intentionally filling the immutable
// registry must not consume the parallel unit binary's real fixed ceiling.
#[cfg(unix)]
#[test]
fn registry_cap_and_failed_validation_never_close_a_live_sqlite_descriptor() {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "desktop::store::startup::tests::registry_cap_child",
            "--exact",
            "--nocapture",
        ])
        .env("ZERUS_PRIVATE_PIN_CAP", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
fn registry_cap_child() {
    if std::env::var_os("ZERUS_PRIVATE_PIN_CAP").as_deref() != Some(std::ffi::OsStr::new("1")) {
        return;
    }
    use crate::sqlite_file::foreign_lock_refused;
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(PIN_LIMIT, 64);
    let temp = tempfile::tempdir().unwrap();
    let location = fixture(&temp);
    current(&location);
    let owner = location.lock().unwrap();
    let path = location.root.join(DATABASE);
    let external = Connection::open(&path).unwrap();
    read_transaction(&external);
    AFTER_PIN.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(|pin| {
            fs::set_permissions(
                pin.root.path.join(DATABASE),
                fs::Permissions::from_mode(0o644),
            )
            .unwrap();
        }))
    });
    assert!(Store::open_owned(&location, &epoch(), &owner).is_err());
    AFTER_PIN.with(|hook| hook.borrow_mut().take());
    assert_eq!(
        PINS.lock().unwrap().len(),
        1,
        "failed validation retained its original"
    );
    assert!(foreign_lock_refused(&path));
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let mut roots = Vec::new();
    while PINS.lock().unwrap().len() < PIN_LIMIT {
        let temp = tempfile::tempdir().unwrap();
        let next = fixture(&temp);
        let owner = next.lock().unwrap();
        drop(Store::open_owned(&next, &epoch(), &owner).unwrap());
        roots.push(temp);
    }
    let next_temp = tempfile::tempdir().unwrap();
    let next = fixture(&next_temp);
    let next_owner = next.lock().unwrap();
    assert!(format!(
        "{:#}",
        Store::open_owned(&next, &epoch(), &next_owner)
            .err()
            .unwrap()
    )
    .contains("pin limit"));
    assert!(
        !next.root.join(DATABASE).exists(),
        "cap refuses BEFORE CREATE_NEW/open"
    );
    let auxiliary = next.root.join("manifest.json");
    fs::write(&auxiliary, b"private backup metadata").unwrap();
    fs::set_permissions(&auxiliary, fs::Permissions::from_mode(0o600)).unwrap();
    let root = Directory::open(&next.root).unwrap();
    BEFORE_RAW_OPEN.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(|_, _| {
            panic!("auxiliary cap refusal must precede the raw open");
        }))
    });
    let error = Pin::input(&root, "manifest.json").err().unwrap();
    let unopened = BEFORE_RAW_OPEN.with(|slot| slot.borrow_mut().take().is_some());
    assert!(error.to_string().contains("pin limit"), "{error:#}");
    assert!(unopened);
    assert_eq!(retained_count(), PIN_LIMIT);
    assert!(foreign_lock_refused(&path));
    external.execute_batch("ROLLBACK").unwrap();
    drop(external);
    assert!(!foreign_lock_refused(&path));
}
