use super::*;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    rc::Rc,
    time::{Duration, Instant},
};

const CHILD_ID: &str = "22222222-2222-4222-8222-222222222222";
const OTHER_ID: &str = "33333333-3333-4333-8333-333333333333";

fn until() -> Instant {
    Instant::now() + Duration::from_secs(2)
}

fn stamp(path: &Path) -> Value {
    let Ok((identity, metadata)) = crate::platform::FileIdentity::at(path) else {
        return Value::Null;
    };
    let bytes = fs::read(path).unwrap();
    json!({
        "device": identity.device,
        "inode": identity.inode,
        "bytes": metadata.len(),
        "modified": identity.modified,
        "modified_nanos": identity.modified_nanos,
        "sha256": format!("{:x}", Sha256::digest(&bytes)),
    })
}

fn stamps(home: &Path) -> Value {
    json!({
        "main": stamp(&home.join("state_5.sqlite")),
        "wal": stamp(&home.join("state_5.sqlite-wal")),
        "shm": stamp(&home.join("state_5.sqlite-shm")),
    })
}

struct Writer {
    _directory: tempfile::TempDir,
    child: Child,
    home: PathBuf,
    ready: Value,
    stopped: bool,
}
impl Writer {
    fn finish(&mut self) {
        if self.stopped {
            return;
        }
        fs::write(self.home.join("stop"), "").unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success(), "writer failed");
                self.stopped = true;
                return;
            }
            assert!(Instant::now() < deadline, "writer did not settle");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for Writer {
    fn drop(&mut self) {
        if self.stopped {
            return;
        }
        let _ = fs::write(self.home.join("stop"), "");
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if self.child.try_wait().ok().flatten().is_some() {
                break;
            }
            if Instant::now() >= deadline {
                let _ = self.child.kill();
                let _ = self.child.wait();
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn start(case: &str) -> Writer {
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path().to_path_buf();
    let stdout = fs::File::create(home.join("stdout")).unwrap();
    let stderr = fs::File::create(home.join("stderr")).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "state::codex_sqlite::tests::writer_process",
            "--ignored",
            "--test-threads=1",
        ])
        .env("ZERUS_CODEX_WRITER", case)
        .env("CODEX_WRITER_DIR", &home)
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    let ready = loop {
        let path = home.join("ready.json");
        if path.exists() {
            break serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        }
        assert!(
            child.try_wait().unwrap().is_none(),
            "writer exited before ready: {}",
            fs::read_to_string(home.join("stderr")).unwrap_or_default()
        );
        assert!(Instant::now() < deadline, "writer was not ready");
        std::thread::sleep(Duration::from_millis(10));
    };
    Writer {
        _directory: directory,
        child,
        home,
        ready,
        stopped: false,
    }
}

#[test]
#[ignore = "private writer process"]
fn writer_process() {
    let Ok(case) = std::env::var("ZERUS_CODEX_WRITER") else {
        return;
    };
    let home = PathBuf::from(std::env::var_os("CODEX_WRITER_DIR").unwrap());
    let database = home.join("state_5.sqlite");
    let rollout = home.join("rollout-child.jsonl");
    let db = Connection::open(&database).unwrap();
    db.execute_batch(
        "PRAGMA journal_mode=WAL;
        CREATE TABLE threads(id TEXT PRIMARY KEY, rollout_path TEXT);
        PRAGMA wal_checkpoint(TRUNCATE);",
    )
    .unwrap();
    let main_before = fs::read(&database).unwrap();
    let main_contains_child = String::from_utf8_lossy(&main_before).contains(CHILD_ID);
    db.execute(
        "INSERT INTO threads VALUES(?1,?2)",
        rusqlite::params![CHILD_ID, rollout.to_str().unwrap()],
    )
    .unwrap();
    let ready = json!({
        "main_contains_child": main_contains_child,
        "stamps": stamps(&home),
    });
    fs::write(
        home.join("ready.json"),
        serde_json::to_string(&ready).unwrap(),
    )
    .unwrap();
    if case == "busy" {
        let mut actions = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(home.join("actions.jsonl"))
            .unwrap();
        for index in 0..200 {
            if home.join("stop").exists() {
                break;
            }
            db.execute(
                "INSERT OR REPLACE INTO threads VALUES(?1,?2)",
                rusqlite::params![format!("other-{index}"), rollout.to_str().unwrap()],
            )
            .unwrap();
            if index % 3 == 0 {
                db.execute_batch("PRAGMA wal_checkpoint(PASSIVE)").unwrap();
            }
            writeln!(
                actions,
                "{}",
                json!({"action": index, "stamps": stamps(&home)})
            )
            .unwrap();
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while !home.join("stop").exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(db);
}

fn isolated(fixture: &str) {
    let state = tempfile::tempdir().unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &format!("state::codex_sqlite::tests::{fixture}"),
            "--ignored",
            "--test-threads=1",
        ])
        .env("HGS_STATE_DIR", state.path())
        .env("HGS_CONFIG_DIR", state.path().join("config"))
        .env("ZERUS_CODEX_SQLITE_FIXTURE", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn directories() -> Rc<RefCell<Vec<PathBuf>>> {
    let directories = Rc::new(RefCell::new(Vec::new()));
    let captured = Rc::clone(&directories);
    AFTER_DIRECTORY.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move |path| {
            captured.borrow_mut().push(path.to_path_buf());
        }));
    });
    directories
}

fn assert_removed(directories: &Rc<RefCell<Vec<PathBuf>>>) {
    assert!(!directories.borrow().is_empty());
    for path in directories.borrow().iter() {
        assert!(!path.exists(), "snapshot retained {path:?}");
    }
    AFTER_DIRECTORY.with(|hook| {
        hook.borrow_mut().take();
    });
}

#[test]
fn a_native_connection_refuses_before_any_raw_open() {
    isolated("native_connection_exclusion");
}

#[test]
#[ignore = "process-isolated private state fixture"]
fn native_connection_exclusion() {
    if std::env::var("ZERUS_CODEX_SQLITE_FIXTURE").as_deref() != Ok("1") {
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path().to_path_buf();
    let database = home.join("state_5.sqlite");
    let db = Connection::open(&database).unwrap();
    db.execute_batch("CREATE TABLE threads(id TEXT PRIMARY KEY, rollout_path TEXT)")
        .unwrap();
    drop(db);
    let native = NativeConnection::open(database).unwrap();
    RAW_OPENS.store(0, std::sync::atomic::Ordering::Relaxed);
    let error = rollout_paths(&home, &[CHILD_ID.into()], until()).unwrap_err();
    assert!(error.contains("already open"), "{error}");
    assert_eq!(RAW_OPENS.load(std::sync::atomic::Ordering::Relaxed), 0);
    drop(native);
}

#[test]
fn a_current_wal_child_is_read_and_the_copy_is_removed() {
    isolated("current_wal_child");
}

#[test]
#[ignore = "process-isolated private state fixture"]
fn current_wal_child() {
    if std::env::var("ZERUS_CODEX_SQLITE_FIXTURE").as_deref() != Ok("1") {
        return;
    }
    let mut writer = start("positive");
    let directories = directories();
    let paths = rollout_paths(&writer.home, &[CHILD_ID.into()], until()).unwrap();
    assert_eq!(
        paths[CHILD_ID],
        writer.home.join("rollout-child.jsonl").to_string_lossy()
    );
    assert_eq!(writer.ready["main_contains_child"], false);
    assert_eq!(stamps(&writer.home), writer.ready["stamps"]);
    assert_removed(&directories);
    writer.finish();
}

#[test]
fn a_busy_writer_can_be_read_without_native_side_effects() {
    isolated("busy_writer");
}

#[test]
#[ignore = "process-isolated private state fixture"]
fn busy_writer() {
    if std::env::var("ZERUS_CODEX_SQLITE_FIXTURE").as_deref() != Ok("1") {
        return;
    }
    let mut writer = start("busy");
    let directories = directories();
    let paths = rollout_paths(&writer.home, &[CHILD_ID.into()], until()).unwrap();
    assert_eq!(
        paths[CHILD_ID],
        writer.home.join("rollout-child.jsonl").to_string_lossy()
    );
    let after = stamps(&writer.home);
    let actions = fs::read_to_string(writer.home.join("actions.jsonl")).unwrap();
    let actions: Vec<Value> = actions
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(!actions.is_empty());
    for name in ["main", "wal", "shm"] {
        assert!(
            after[name] == writer.ready["stamps"][name]
                || actions
                    .iter()
                    .any(|action| action["stamps"][name] == after[name]),
            "unattributed {name} delta: {}",
            after[name]
        );
    }
    println!("before={}", writer.ready["stamps"]);
    println!("after={after}");
    println!("writer_actions={}", actions.len());
    assert_removed(&directories);
    writer.finish();
}

fn refuse_with_hook(case: &str, mut mutate: impl FnMut(&Path) + 'static) {
    let mut writer = start("positive");
    let home = writer.home.clone();
    let directories = directories();
    AFTER_READ.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move || mutate(&home)));
    });
    let error = rollout_paths(&writer.home, &[CHILD_ID.into()], until()).unwrap_err();
    AFTER_READ.with(|hook| {
        hook.borrow_mut().take();
    });
    assert!(!error.is_empty(), "{case}");
    assert_removed(&directories);
    writer.finish();
}

#[test]
fn replaced_stale_and_eof_sources_refuse() {
    isolated("replaced_source");
    isolated("stale_source");
    isolated("eof_source");
}

#[test]
#[ignore = "process-isolated private state fixture"]
fn replaced_source() {
    if std::env::var("ZERUS_CODEX_SQLITE_FIXTURE").as_deref() != Ok("1") {
        return;
    }
    refuse_with_hook("replaced", |home| {
        let database = home.join("state_5.sqlite");
        let old = home.join("old.sqlite");
        fs::rename(&database, &old).unwrap();
        fs::copy(&old, &database).unwrap();
    });
}

#[test]
#[ignore = "process-isolated private state fixture"]
fn stale_source() {
    if std::env::var("ZERUS_CODEX_SQLITE_FIXTURE").as_deref() != Ok("1") {
        return;
    }
    refuse_with_hook("stale", |home| {
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(home.join("state_5.sqlite-wal"))
            .unwrap();
        file.write_all(&[0]).unwrap();
    });
}

#[test]
#[ignore = "process-isolated private state fixture"]
fn eof_source() {
    if std::env::var("ZERUS_CODEX_SQLITE_FIXTURE").as_deref() != Ok("1") {
        return;
    }
    refuse_with_hook("eof", |home| {
        let wal = home.join("state_5.sqlite-wal");
        let length = wal.metadata().unwrap().len();
        fs::OpenOptions::new()
            .write(true)
            .open(&wal)
            .unwrap()
            .set_len(length / 2)
            .unwrap();
    });
}

#[test]
fn deadline_and_over_bound_refuse_before_copying() {
    isolated("deadline_over_bound");
}

#[test]
#[ignore = "process-isolated private state fixture"]
fn deadline_over_bound() {
    if std::env::var("ZERUS_CODEX_SQLITE_FIXTURE").as_deref() != Ok("1") {
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    RAW_OPENS.store(0, std::sync::atomic::Ordering::Relaxed);
    let error = rollout_paths(directory.path(), &[CHILD_ID.into()], Instant::now()).unwrap_err();
    assert!(error.contains("deadline"), "{error}");
    assert_eq!(RAW_OPENS.load(std::sync::atomic::Ordering::Relaxed), 0);
    let database = directory.path().join("state_5.sqlite");
    fs::write(&database, b"SQLite format 3\0").unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&database)
        .unwrap()
        .set_len(MAX_SOURCE + 1)
        .unwrap();
    let error = rollout_paths(directory.path(), &[CHILD_ID.into()], until()).unwrap_err();
    assert!(error.contains("byte bound"), "{error}");
}

#[test]
fn a_panic_removes_the_private_copy() {
    isolated("panic_cleanup");
}

#[test]
#[ignore = "process-isolated private state fixture"]
fn panic_cleanup() {
    if std::env::var("ZERUS_CODEX_SQLITE_FIXTURE").as_deref() != Ok("1") {
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("state_5.sqlite");
    let db = Connection::open(&database).unwrap();
    db.execute_batch("CREATE TABLE threads(id TEXT PRIMARY KEY, rollout_path TEXT)")
        .unwrap();
    drop(db);
    let directories = Rc::new(RefCell::new(Vec::new()));
    let captured = Rc::clone(&directories);
    AFTER_DIRECTORY.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move |path| {
            captured.borrow_mut().push(path.to_path_buf());
            panic!("fixture panic");
        }));
    });
    let result = std::panic::catch_unwind(|| {
        let _ = rollout_paths(directory.path(), &[CHILD_ID.into()], until());
    });
    assert!(result.is_err());
    assert_removed(&directories);
}

#[test]
fn a_corrupt_wal_frame_is_refused() {
    isolated("corrupt_wal_frame");
}

#[test]
#[ignore = "process-isolated private state fixture"]
fn corrupt_wal_frame() {
    if std::env::var("ZERUS_CODEX_SQLITE_FIXTURE").as_deref() != Ok("1") {
        return;
    }
    let mut writer = start("positive");
    let wal_path = writer.home.join("state_5.sqlite-wal");
    let mut wal = fs::read(&wal_path).unwrap();
    assert!(wal.len() > 56);
    wal[56] ^= 1;
    fs::write(&wal_path, wal).unwrap();
    let directories = directories();
    let error = rollout_paths(&writer.home, &[CHILD_ID.into()], until()).unwrap_err();
    assert!(error.contains("checksum"), "{error}");
    assert!(directories.borrow().is_empty());
    AFTER_DIRECTORY.with(|hook| {
        hook.borrow_mut().take();
    });
    writer.finish();
}

#[test]
fn a_missing_child_is_not_evidence() {
    isolated("missing_child");
}

#[test]
#[ignore = "process-isolated private state fixture"]
fn missing_child() {
    if std::env::var("ZERUS_CODEX_SQLITE_FIXTURE").as_deref() != Ok("1") {
        return;
    }
    let mut writer = start("positive");
    let directories = directories();
    let error = rollout_paths(&writer.home, &[OTHER_ID.into()], until()).unwrap_err();
    assert!(error.contains("absent"), "{error}");
    assert_removed(&directories);
    writer.finish();
}
