//! Actual platform custody plus real SQLite transaction/COMMIT lifetimes.
//! Windows executes the native keeper/lease and original-handle fault path.
use super::*;
use crate::state::task_controls::Source;
use crate::{platform, state::repositories::directory::Directory};
use rusqlite::Connection;
use std::{
    cell::Cell,
    fs,
    fs::OpenOptions,
    path::PathBuf,
    time::{Duration, Instant},
};

fn fact() -> Fact {
    Fact {
        id: "00000000-0000-4000-8000-000000000001".into(),
        executor: Executor {
            computer: "00000000-0000-4000-8000-000000000002".into(),
            session_id: "00000000-0000-4000-8000-000000000003".into(),
            run: "run".into(),
            birth: "birth".into(),
            conversation: None,
            child: None,
        },
        source: Source::PersonStop,
        turn: Some("turn".into()),
    }
}

fn fixture() -> (tempfile::TempDir, Capture, PathBuf) {
    let root = tempfile::tempdir_in(platform::canonical(std::env::temp_dir()).unwrap()).unwrap();
    platform::private_dir(root.path()).unwrap();
    platform::restrict_dir(root.path()).unwrap();
    drop(
        platform::private_open(OpenOptions::new().create_new(true).read(true).write(true))
            .open(root.path().join(".lock"))
            .unwrap(),
    );
    let (capture, path) = Capture::fixture(
        root.path(),
        &fact(),
        Instant::now() + Duration::from_secs(20),
    )
    .unwrap();
    (root, capture, path)
}
fn database() -> Connection {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE retained(request TEXT PRIMARY KEY)")
        .unwrap();
    db
}
fn count(db: &Connection) -> i64 {
    db.query_row("SELECT count(*) FROM retained", [], |row| row.get(0))
        .unwrap()
}
fn changed_fact(path: &std::path::Path) {
    let before = fs::read_to_string(path).unwrap();
    let after = before.replace("\"turn\":\"turn\"", "\"turn\":\"swap\"");
    assert_ne!(before, after);
    assert_eq!(before.len(), after.len());
    fs::write(path, after).unwrap();
}

#[test]
fn same_current_scope_spans_commit_without_holding_or_reacquiring_native_writer() {
    let (root, capture, _) = fixture();
    let mut db = database();
    capture
        .with_current(&fact().executor, |executor, facts, check| {
            assert_eq!(executor, &fact().executor);
            assert_eq!(facts, &[fact()]);
            let tx = db.transaction().map_err(|error| error.to_string())?;
            // A real separate lock can be held while this SQL transaction/current
            // lease exists. The read path must not retain or reacquire its Writer.
            let file = platform::private_open(OpenOptions::new().read(true).write(true))
                .open(root.path().join(".lock"))
                .map_err(|error| error.to_string())?;
            fs2::FileExt::try_lock_exclusive(&file).map_err(|error| error.to_string())?;
            check()?;
            tx.execute("INSERT INTO retained VALUES('one')", [])
                .map_err(|error| error.to_string())?;
            check()?;
            tx.commit().map_err(|error| error.to_string())?;
            check()?;
            fs2::FileExt::unlock(&file).map_err(|error| error.to_string())?;
            Ok(())
        })
        .unwrap();
    assert_eq!(count(&db), 1);
    crate::state::storage::atomic(&root.path().join("run.json"), "{\"state\":\"live\"}").unwrap();
    capture
        .with_current(&fact().executor, |_, facts, check| {
            assert_eq!(facts, &[fact()]);
            check()
        })
        .unwrap();
}

#[test]
fn pre_commit_current_check_failure_rolls_back_every_tentative_row() {
    let (_root, capture, path) = fixture();
    let mut db = database();
    let established = Cell::new(false);
    let refused = Cell::new(false);
    let result = capture.with_current(&fact().executor, |_, _, check| {
        let tx = db.transaction().map_err(|error| error.to_string())?;
        tx.execute("INSERT INTO retained VALUES('tentative')", [])
            .map_err(|error| error.to_string())?;
        changed_fact(&path);
        established.set(true);
        match check() {
            Err(error) => {
                refused.set(true);
                Err(error)
            }
            Ok(()) => {
                tx.commit().map_err(|error| error.to_string())?;
                Ok(())
            }
        }
    });
    assert!(
        established.get(),
        "setup/panic is not a current-check refusal"
    );
    assert!(refused.get(), "the SAME scope must refuse before COMMIT");
    assert!(result.is_err());
    assert_eq!(count(&db), 0);
}

#[test]
fn after_commit_final_refusal_keeps_durable_rows_and_never_replays_callback() {
    let (_root, capture, path) = fixture();
    let mut db = database();
    let calls = Cell::new(0);
    let callback_returned = Cell::new(false);
    let result = capture.with_current(&fact().executor, |_, _, check| {
        calls.set(calls.get() + 1);
        let tx = db.transaction().map_err(|error| error.to_string())?;
        tx.execute("INSERT INTO retained VALUES('durable')", [])
            .map_err(|error| error.to_string())?;
        check()?;
        tx.commit().map_err(|error| error.to_string())?;
        changed_fact(&path);
        callback_returned.set(true);
        Ok("a committed result must not escape a failed final check")
    });
    assert!(
        callback_returned.get(),
        "COMMIT and actual mutation must finish normally"
    );
    assert!(
        result.is_err(),
        "post-callback failure must withhold the result"
    );
    assert_eq!(
        count(&db),
        1,
        "an AFTER-COMMIT refusal is not rollback proof"
    );
    assert!(capture
        .with_current(&fact().executor, |_, _, _| {
            calls.set(calls.get() + 1);
            Ok(())
        })
        .is_err());
    assert_eq!(
        calls.get(),
        1,
        "only lookup is valid; a failed capture cannot replay"
    );
    assert_eq!(count(&db), 1);
}

#[test]
fn equal_byte_fact_substitution_is_really_established_and_refuses_before_commit() {
    let (_root, capture, path) = fixture();
    let directory = Directory::open(path.parent().unwrap()).unwrap();
    let name = path.file_name().unwrap().to_str().unwrap();
    #[cfg(windows)]
    let reader = directory.read_private(name).unwrap();
    let original = fs::read(&path).unwrap();
    #[cfg(windows)]
    let mut moved = None;
    let mut db = database();
    let established = Cell::new(false);
    let refused = Cell::new(false);
    let result = capture.with_current(&fact().executor, |_, _, check| {
        let tx = db.transaction().map_err(|error| error.to_string())?;
        tx.execute("INSERT INTO retained VALUES('tentative')", [])
            .map_err(|error| error.to_string())?;
        #[cfg(windows)]
        {
            moved = Some(
                directory
                    .move_private_for_fault(&reader, name, "retained-original")
                    .unwrap(),
            );
        }
        #[cfg(not(windows))]
        fs::rename(&path, path.with_file_name("retained-original")).unwrap();
        directory.write_new(name, &original).unwrap();
        #[cfg(windows)]
        moved.as_ref().unwrap().verify_replacement().unwrap();
        established.set(true);
        match check() {
            Err(error) => {
                refused.set(true);
                Err(error)
            }
            Ok(()) => {
                tx.commit().map_err(|error| error.to_string())?;
                Ok(())
            }
        }
    });
    assert!(
        established.get(),
        "a failed plain rename is never substitution evidence"
    );
    assert!(refused.get());
    assert!(result.is_err());
    assert_eq!(count(&db), 0);
    #[cfg(windows)]
    moved.as_ref().unwrap().verify_replacement().unwrap();
    assert_eq!(
        fs::read(&path).unwrap(),
        original,
        "refusal must not repair the replacement"
    );
}
