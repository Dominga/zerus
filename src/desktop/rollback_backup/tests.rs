use super::*;
use crate::desktop::store::Store;
use rusqlite::Connection;
use std::{
    path::{Path, PathBuf},
    process::Command,
};

fn location(root: &Path) -> Location {
    Location {
        root: root.into(),
        configuration: "a".repeat(64),
    }
}
fn make_old(root: &Path) -> Location {
    let location = location(root);
    location.prepare().unwrap();
    let store = Store::open(&location, &uuid::Uuid::new_v4().to_string()).unwrap();
    crate::desktop::creation_worktree::rewind_slots(&store.db);
    store.db.execute_batch("PRAGMA user_version=34; CREATE TABLE rollback_sentinel(value TEXT NOT NULL); INSERT INTO rollback_sentinel VALUES('original row');").unwrap();
    drop(store);
    location
}
fn epoch() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn schema(db: &Connection) -> i64 {
    db.query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap()
}
fn backup_bytes(location: &Location) -> Vec<(PathBuf, Vec<u8>)> {
    let root = location.root.join(BACKUP);
    let mut files = fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect::<Vec<_>>();
    files.sort();
    files
        .into_iter()
        .map(|p| {
            let b = fs::read(&p).unwrap();
            (p, b)
        })
        .collect()
}
fn copy_for_restore(location: &Location, target: &Path) {
    fs::create_dir(target).unwrap();
    for name in [DATABASE, WAL] {
        let from = location.root.join(BACKUP).join(name);
        if from.exists() {
            fs::copy(from, target.join(name)).unwrap();
        }
    }
}
#[test]
fn production_open_keeps_one_complete_backup_and_later_starts_never_overwrite_it() {
    let temp = tempfile::tempdir().unwrap();
    let location = make_old(&temp.path().canonicalize().unwrap().join("state/desktop"));
    // Force the positive no-WAL case, independently of SQLite's creation of
    // an empty WAL during a read-only probe of a WAL-mode database.
    Connection::open(location.root.join(DATABASE))
        .unwrap()
        .execute_batch("PRAGMA journal_mode=DELETE")
        .unwrap();
    let owner = location.lock().unwrap();
    let store = Store::open_owned(&location, &epoch(), &owner).unwrap();
    assert_eq!(schema(&store.db), 35);
    let original = backup_bytes(&location);
    let manifest: Manifest =
        serde_json::from_slice(&fs::read(location.root.join(BACKUP).join(MANIFEST)).unwrap())
            .unwrap();
    assert_eq!(manifest.source_schema, 34);
    assert!(manifest.wal.is_none());
    store
        .db
        .execute_batch("INSERT INTO rollback_sentinel VALUES('after migration');")
        .unwrap();
    drop(store);
    let store = Store::open_owned(&location, &epoch(), &owner).unwrap();
    assert_eq!(backup_bytes(&location), original);
    drop(store);
    let restored = temp.path().join("restored");
    copy_for_restore(&location, &restored);
    let restored = Connection::open(restored.join(DATABASE)).unwrap();
    assert_eq!(schema(&restored), 34);
    assert_eq!(
        restored
            .query_row::<i64, _, _>("SELECT count(*) FROM rollback_sentinel", [], |r| r.get(0))
            .unwrap(),
        1
    );
}

#[test]
fn the_schema_34_preflight_refuses_35_without_rewriting_its_database_or_columns() {
    let temp = tempfile::tempdir().unwrap();
    let location = make_old(&temp.path().canonicalize().unwrap().join("state/desktop"));
    let owner = location.lock().unwrap();
    drop(Store::open_owned(&location, &epoch(), &owner).unwrap());
    let path = location.root.join(DATABASE);
    let before = fs::read(&path).unwrap();
    let backup = backup_bytes(&location);
    let error = super::super::schema_before_write(&path, 34).unwrap_err();
    assert!(error.to_string().contains("requires a newer hgs version"));
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(backup_bytes(&location), backup);
    let db = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert_eq!(schema(&db), 35);
    assert_eq!(db.query_row::<i64,_,_>("SELECT count(*) FROM pragma_table_info('desktop_creation_worktrees') WHERE name IN ('creation_id','creation_slot')",[],|r|r.get(0)).unwrap(),2);
}
/// Task #421 (2026-11-09): an interrupted attempt left an EMPTY
/// schema-34-rollback (on Windows its first file could not be published), and
/// every later start refused it. Now the empty folder is retained under a new
/// name, a complete backup is made again, and the store migrates.
#[test]
fn an_empty_leftover_backup_is_retained_and_the_backup_is_made_again() {
    let temp = tempfile::tempdir().unwrap();
    let location = make_old(&temp.path().canonicalize().unwrap().join("state/desktop"));
    platform::private_dir(&location.root.join(BACKUP)).unwrap();
    let owner = location.lock().unwrap();
    let store = Store::open_owned(&location, &epoch(), &owner).unwrap();
    assert_eq!(schema(&store.db), 35);
    drop(store);
    let manifest: Manifest =
        serde_json::from_slice(&fs::read(location.root.join(BACKUP).join(MANIFEST)).unwrap()).unwrap();
    assert_eq!(manifest.source_schema, 34);
    assert!(location.root.join(BACKUP).join(DATABASE).is_file());
    let retained: Vec<_> = fs::read_dir(&location.root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.file_name().unwrap().to_string_lossy().starts_with(&format!("{BACKUP}.empty-")))
        .collect();
    assert_eq!(retained.len(), 1, "the leftover is kept under a new name");
    assert_eq!(fs::read_dir(&retained[0]).unwrap().count(), 0);
}

/// Task #440: `hgs desktop store-check` tells, read-only, whether a start
/// gets past the store and its rollback backup; the window relaunches only
/// when it does.
#[test]
fn the_store_check_reads_the_backup_state_without_writing() {
    let temp = tempfile::tempdir().unwrap();
    let location = make_old(&temp.path().canonicalize().unwrap().join("state/desktop"));
    let check = || super::super::store::store_check(&location);
    assert_eq!((check()["code"].clone(), check()["opens"].clone()), (serde_json::json!("backup_needed"), serde_json::json!(true)));
    platform::private_dir(&location.root.join(BACKUP)).unwrap();
    assert_eq!((check()["code"].clone(), check()["opens"].clone()), (serde_json::json!("empty_leftover"), serde_json::json!(true)));
    fs::write(location.root.join(BACKUP).join("partial"), b"x").unwrap();
    assert_eq!((check()["code"].clone(), check()["opens"].clone()), (serde_json::json!("backup_incomplete"), serde_json::json!(false)));
    fs::remove_file(location.root.join(BACKUP).join("partial")).unwrap();
    fs::remove_dir(location.root.join(BACKUP)).unwrap();
    let owner = location.lock().unwrap();
    {
        let handoff = location.handoff_lock().unwrap();
        Original::new(&location, &owner, &handoff).unwrap().prepare(Some(34)).unwrap();
    }
    let before = backup_bytes(&location);
    assert_eq!(check()["code"], "backup_present");
    assert_eq!(backup_bytes(&location), before, "the check writes nothing");
    let db = Connection::open(location.root.join(DATABASE)).unwrap();
    assert_eq!(schema(&db), 34, "the check never migrates");
}

/// Task #421: the backup of an older store (schema 30, the owner's) completes:
/// its manifest, the database and, when there is one, the WAL.
#[test]
fn a_schema_30_store_is_backed_up_completely() {
    let temp = tempfile::tempdir().unwrap();
    let location = make_old(&temp.path().canonicalize().unwrap().join("state/desktop"));
    let owner = location.lock().unwrap();
    let handoff = location.handoff_lock().unwrap();
    let original = Original::new(&location, &owner, &handoff).unwrap();
    original.prepare(Some(30)).unwrap();
    let backup = location.root.join(BACKUP);
    let manifest: Manifest = serde_json::from_slice(&fs::read(backup.join(MANIFEST)).unwrap()).unwrap();
    assert_eq!(manifest.source_schema, 30);
    assert_eq!(fs::read(backup.join(DATABASE)).unwrap().len() as u64, manifest.database.bytes);
    assert_eq!(manifest.wal.is_some(), backup.join(WAL).is_file());
    // A later start finds it complete and never rewrites it.
    let before = backup_bytes(&location);
    original.prepare(Some(30)).unwrap();
    assert_eq!(backup_bytes(&location), before);
}

#[test]
fn partial_or_corrupt_backup_is_never_repaired_or_used_for_a_migration() {
    for corrupt in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let location = make_old(&temp.path().canonicalize().unwrap().join("state/desktop"));
        let owner = location.lock().unwrap();
        if corrupt {
            let handoff = location.handoff_lock().unwrap();
            let original = Original::new(&location, &owner, &handoff).unwrap();
            original.prepare(Some(34)).unwrap();
            fs::write(
                location.root.join(BACKUP).join(DATABASE),
                b"retain corrupt backup",
            )
            .unwrap();
        } else {
            platform::private_dir(&location.root.join(BACKUP)).unwrap();
            fs::write(
                location.root.join(BACKUP).join("incomplete"),
                b"retain partial backup",
            )
            .unwrap();
        }
        let before = backup_bytes(&location);
        assert!(Store::open_owned(&location, &epoch(), &owner).is_err());
        assert_eq!(backup_bytes(&location), before);
        let db = Connection::open(location.root.join(DATABASE)).unwrap();
        assert_eq!(schema(&db), 34);
        assert_eq!(
            db.query_row::<String, _, _>("SELECT value FROM rollback_sentinel", [], |r| r.get(0))
                .unwrap(),
            "original row"
        );
    }
}
#[test]
fn interrupted_slot_upgrade_rolls_back_the_real_transaction_and_keeps_the_backup() {
    let temp = tempfile::tempdir().unwrap();
    let location = make_old(&temp.path().canonicalize().unwrap().join("state/desktop"));
    let child = Command::new(std::env::current_exe().unwrap())
        .args([
            "desktop::store::rollback_backup::tests::interrupted_upgrade_probe",
            "--exact",
            "--nocapture",
        ])
        .env("ZERUS_PRIVATE_SLOT_INTERRUPT", &location.root)
        .output()
        .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(
            child.status.signal(),
            Some(libc::SIGKILL),
            "{}",
            String::from_utf8_lossy(&child.stderr)
        );
    }
    #[cfg(not(unix))]
    assert_eq!(
        child.status.code(),
        Some(86),
        "{}",
        String::from_utf8_lossy(&child.stderr)
    );
    let backup = backup_bytes(&location);
    let db = Connection::open(location.root.join(DATABASE)).unwrap();
    assert_eq!(schema(&db), 34);
    let columns = db
        .prepare("PRAGMA table_info(desktop_creation_worktrees)")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(1))
        .unwrap()
        .map(Result::unwrap)
        .collect::<Vec<_>>();
    assert!(columns.contains(&"creation_id".into()));
    assert!(!columns.contains(&"creation_slot".into()));
    assert_eq!(
        db.query_row::<i64, _, _>(
            "SELECT count(*) FROM sqlite_schema WHERE name='desktop_creation_worktree_original'",
            [],
            |r| r.get(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(
        db.query_row::<String, _, _>("SELECT value FROM rollback_sentinel", [], |r| r.get(0))
            .unwrap(),
        "original row"
    );
    drop(db);
    let owner = location.lock().unwrap();
    let store = Store::open_owned(&location, &epoch(), &owner).unwrap();
    assert_eq!(schema(&store.db), 35);
    assert_eq!(backup_bytes(&location), backup);
}
#[test]
fn interrupted_upgrade_probe() {
    let Some(root) = std::env::var_os("ZERUS_PRIVATE_SLOT_INTERRUPT") else {
        return;
    };
    let location = location(Path::new(&root));
    let owner = location.lock().unwrap();
    INTERRUPT.with(|v| v.set(true));
    let _ = Store::open_owned(&location, &epoch(), &owner);
    panic!("migration did not reach interruption boundary");
}
#[test]
fn committed_wal_is_copied_with_the_database_and_restores_the_original_row() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap().join("state/desktop");
    let child = Command::new(std::env::current_exe().unwrap())
        .args([
            "desktop::store::rollback_backup::tests::leave_committed_wal_probe",
            "--exact",
            "--nocapture",
        ])
        .env("ZERUS_PRIVATE_COMMITTED_WAL", &root)
        .output()
        .unwrap();
    assert_eq!(
        child.status.code(),
        Some(87),
        "{}",
        String::from_utf8_lossy(&child.stderr)
    );
    assert!(root.join(WAL).exists());
    let location = location(&root);
    let owner = location.lock().unwrap();
    let store = Store::open_owned(&location, &epoch(), &owner).unwrap();
    assert_eq!(schema(&store.db), 35);
    drop(store);
    let manifest: Manifest =
        serde_json::from_slice(&fs::read(root.join(BACKUP).join(MANIFEST)).unwrap()).unwrap();
    assert!(manifest.wal.as_ref().is_some_and(|v| v.bytes > 0));
    let restored = temp.path().join("restored");
    copy_for_restore(&location, &restored);
    let db = Connection::open(restored.join(DATABASE)).unwrap();
    assert_eq!(schema(&db), 34);
    assert_eq!(
        db.query_row::<String, _, _>(
            "SELECT value FROM rollback_sentinel WHERE value='committed WAL row'",
            [],
            |r| r.get(0)
        )
        .unwrap(),
        "committed WAL row"
    );
}
#[test]
fn leave_committed_wal_probe() {
    let Some(root) = std::env::var_os("ZERUS_PRIVATE_COMMITTED_WAL") else {
        return;
    };
    let location = make_old(Path::new(&root));
    let db = Connection::open(location.root.join(DATABASE)).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; INSERT INTO rollback_sentinel VALUES('committed WAL row');").unwrap();
    std::process::exit(87);
}

#[test]
fn an_existing_backup_requires_original_root_store_configuration_and_its_complete_roster() {
    for changed in [
        "configuration",
        "root_inode",
        "database_inode",
        "extra_file",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let location = make_old(&temp.path().canonicalize().unwrap().join("state/desktop"));
        let owner = location.lock().unwrap();
        {
            let handoff = location.handoff_lock().unwrap();
            let original = Original::new(&location, &owner, &handoff).unwrap();
            original.prepare(Some(34)).unwrap();
        }
        let backup = location.root.join(BACKUP);
        if changed == "extra_file" {
            fs::write(backup.join("unexpected"), b"preserve").unwrap();
        } else {
            let file = backup.join(MANIFEST);
            let mut value: serde_json::Value =
                serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
            if changed == "configuration" {
                value[changed] = serde_json::json!("b".repeat(64));
            } else {
                value[changed] = serde_json::json!(0);
            }
            fs::write(file, serde_json::to_vec(&value).unwrap()).unwrap();
        }
        let before = backup_bytes(&location);
        assert!(Store::open_owned(&location, &epoch(), &owner).is_err());
        assert_eq!(backup_bytes(&location), before);
        let db = Connection::open(location.root.join(DATABASE)).unwrap();
        assert_eq!(schema(&db), 34);
    }
}

#[cfg(unix)]
#[test]
fn irregular_or_replaced_inputs_and_exclusions_refuse_before_migration() {
    use std::os::unix::fs::symlink;
    for which in [
        DATABASE,
        WAL,
        "owner.lock",
        zerus_desktop_migration::HANDOFF_LOCK,
    ] {
        let temp = tempfile::tempdir().unwrap();
        let location = make_old(&temp.path().canonicalize().unwrap().join("state/desktop"));
        let owner = location.lock().unwrap();
        let handoff = location.handoff_lock().unwrap();
        let original = Original::new(&location, &owner, &handoff).unwrap();
        let path = location.root.join(which);
        let retained = location.root.join(format!("retained-{which}"));
        if path.exists() {
            fs::rename(&path, &retained).unwrap();
        }
        if which == WAL {
            use std::os::unix::ffi::OsStrExt;
            let name = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
            assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        } else if which == DATABASE {
            symlink(&retained, &path).unwrap();
        } else {
            fs::write(&path, b"replaced exclusion").unwrap();
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        }
        let start = Instant::now();
        assert!(original.prepare(Some(34)).is_err(), "{which}");
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(!location.root.join(BACKUP).join(MANIFEST).exists());
    }
}
