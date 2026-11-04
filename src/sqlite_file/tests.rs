use super::*;
use std::os::unix::fs::PermissionsExt;

fn names(directory: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn create_makes_an_owner_only_file_and_never_replaces_one() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("a.sqlite3");
    create(&path).unwrap();
    let metadata = fs::symlink_metadata(&path).unwrap();
    assert!(metadata.is_file() && metadata.len() == 0 && metadata.nlink() == 1);
    assert_eq!(metadata.mode() & 0o777, 0o600);
    fs::write(&path, b"kept").unwrap();
    create(&path).unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"kept", "never replaced");
    assert_eq!(names(temp.path()), ["a.sqlite3"], "no staging name is left");
    assert!(private(&path).is_ok());
}

#[test]
fn private_refuses_links_shared_modes_and_a_second_name() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("a.sqlite3");
    create(&path).unwrap();
    let found = private(&path).unwrap();
    let metadata = fs::metadata(&path).unwrap();
    let expected = Identity {
        device: metadata.dev(),
        inode: metadata.ino(),
    };
    assert_eq!(found, expected);
    assert_eq!(existing(&path).unwrap(), found);
    let link = temp.path().join("link.sqlite3");
    std::os::unix::fs::symlink(&path, &link).unwrap();
    assert!(private(&link).is_err(), "a symbolic link");
    assert_eq!(existing(&link).unwrap(), found, "existing follows links");
    let second = temp.path().join("second.sqlite3");
    fs::hard_link(&path, &second).unwrap();
    assert!(private(&path).is_err(), "a second name");
    fs::remove_file(&second).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    assert!(private(&path).is_err(), "group access");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(private(&path).unwrap(), found);
}

#[test]
fn bind_accepts_the_checked_file_and_refuses_another() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("a.sqlite3");
    create(&path).unwrap();
    let identity = private(&path).unwrap();
    let db = Connection::open(&path).unwrap();
    bind(&db, identity).unwrap();
    let other = temp.path().join("b.sqlite3");
    create(&other).unwrap();
    assert!(bind(&db, private(&other).unwrap()).is_err(), "another file");
    fs::rename(&other, &path).unwrap();
    assert!(bind(&db, identity).is_err(), "SQLite's open file has moved");
}

/// The failure this module avoids, and the open sequence that avoids it.
/// While one connection holds a read transaction, another process cannot
/// lock the database for writing. Opening it again through `create`,
/// `private` and `bind` keeps that so; closing any raw descriptor of the
/// file does not.
#[test]
fn opening_again_keeps_a_live_connection_s_locks() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("a.sqlite3");
    let open = || {
        create(&path).unwrap();
        let identity = private(&path).unwrap();
        let db = Connection::open(&path).unwrap();
        bind(&db, identity).unwrap();
        db
    };
    let live = open();
    live.execute_batch("CREATE TABLE t(x); INSERT INTO t VALUES(1); BEGIN;")
        .unwrap();
    let _: i64 = live
        .query_row("SELECT count(*) FROM t", [], |r| r.get(0))
        .unwrap();
    assert!(foreign_lock_refused(&path), "the reader holds its lock");
    drop(open());
    assert!(foreign_lock_refused(&path), "opening again kept the lock");
    drop(fs::File::open(&path).unwrap());
    assert!(
        !foreign_lock_refused(&path),
        "the control: a closed raw descriptor dropped the lock"
    );
    live.execute_batch("COMMIT").unwrap();
}
