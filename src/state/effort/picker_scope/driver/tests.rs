//! Faults exercise the actual retained-object publication/recovery primitives.
//! Synthetic observation completion is NOT native-controller qualification;
//! that is separately exercised through the non-test CLI/PTY fixture.
use super::*;
use crate::state::guarded_recovery::{Effects, Original as Source};
use fs2::FileExt as _;
use std::{
    cell::RefCell,
    os::unix::net::UnixStream,
    process::{Command, Stdio},
};

thread_local! {static SYNC_FAULT:RefCell<Option<Box<dyn FnOnce()>>>=RefCell::new(None);}
pub(super) fn restore_sync_fault() {
    SYNC_FAULT.with(|fault| {
        if let Some(fault) = fault.borrow_mut().take() {
            fault();
        }
    });
}

fn run(case: &str) {
    for kind in ["fresh", "moved"] {
        let private = tempfile::tempdir().unwrap();
        let output = tempfile::tempfile().unwrap();
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", private.path())
            .env("HGS_CONFIG_DIR", private.path().join("config"))
            .env("HGS_STATE_DIR", private.path().join("state"))
            .env("ZERUS_EFFORT_DRIVER_CASE", case)
            .env("ZERUS_EFFORT_DRIVER_KIND", kind)
            .args([
                "--exact",
                "state::effort::picker_scope::driver::tests::fixture",
                "--ignored",
                "--test-threads=1",
            ])
            .stdin(Stdio::null())
            .stdout(output.try_clone().unwrap())
            .stderr(output.try_clone().unwrap());
        let mut child = command.spawn().unwrap();
        let end = Instant::now() + Duration::from_secs(12);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= end {
                let _ = child.kill();
                let _ = child.wait();
                panic!("private Effort primitive fixture timed out: {kind}/{case}");
            }
            std::thread::sleep(Duration::from_millis(3));
        };
        use std::io::{Read, Seek};
        let mut output = output;
        output.rewind().unwrap();
        let mut text = String::new();
        output.read_to_string(&mut text).unwrap();
        assert!(status.success(), "{kind}/{case}: {text}");
    }
}

#[test]
fn publication_rechecks_sources_and_names_after_staging_and_observation() {
    for case in [
        "commit",
        "stage-replaced",
        "destination-replaced",
        "proof-replaced",
        "eof",
    ] {
        run(case);
    }
}
#[test]
fn sticky_context_never_falls_back_after_foreign_missing_nested_or_ended_use() {
    run("context");
}
#[test]
fn guarded_restore_checks_both_commits_and_only_rebases_its_own_link() {
    for case in [
        "restore-linked",
        "restore-existing",
        "restore-newlines",
        "restore-existing-change",
        "restore-linked-change",
        "restore-eof",
    ] {
        run(case);
    }
}
#[test]
fn recovery_atomic_creates_only_checked_children_and_refuses_changed_absence() {
    run("atomic");
    run("atomic-absence");
}

fn contended(file: &File) -> bool {
    match file.try_lock_exclusive() {
        Ok(()) => {
            fs2::FileExt::unlock(file).unwrap();
            false
        }
        Err(error) => {
            assert!(crate::platform::lock_contended(&error));
            true
        }
    }
}

#[test]
#[ignore = "private exact-object fixture, invoked by bounded driver controls"]
fn fixture() {
    let case = std::env::var("ZERUS_EFFORT_DRIVER_CASE").unwrap();
    let kind = std::env::var("ZERUS_EFFORT_DRIVER_KIND").unwrap();
    let (writer, record) = super::super::tests::setup(&kind);
    let raw=serde_json::to_vec(&json!({"request_id":uuid::Uuid::new_v4().to_string(),
        "expected_run_id":record["run_id"],"expected_conversation_id":record["conversation_id"],"effort":"high"})).unwrap();
    let frame = host::frame::Frame::new(
        host::frame::Operation::Effort,
        raw,
        &record,
        host::frame::Deadline::after(Duration::from_secs(8)).unwrap(),
    )
    .unwrap();
    let (peer, stream) = UnixStream::pair().unwrap();
    let control = host::Control::fixture(frame, stream).unwrap();
    let (owner, done) = owner::Owner::new(writer, control).unwrap();
    let entered = context::Entered::install(owner.clone()).unwrap();
    let root_file = File::options()
        .read(true)
        .write(true)
        .open(root().join(".lock"))
        .unwrap();
    assert!(contended(&root_file));
    if case.starts_with("restore-") || case.starts_with("atomic") {
        let effects = effects::Recovery(owner.clone());
        if case.starts_with("atomic") {
            let path = root().join("missing/child/value.json");
            let absent = Source::read_absent(&path).unwrap();
            if case == "atomic-absence" {
                private_dir(path.parent().unwrap()).unwrap();
                fs::write(&path, b"other").unwrap();
                assert!(effects.atomic(&path, b"ours", &[&absent]).is_err());
                assert_eq!(fs::read(&path).unwrap(), b"other");
                assert!(owner.actor().is_err());
            } else {
                effects.atomic(&path, b"ours", &[&absent]).unwrap();
                assert_eq!(fs::read(path).unwrap(), b"ours");
            }
        } else {
            let parent = root().join("git-control");
            private_dir(&parent).unwrap();
            let lock = parent.join("locked");
            let taken = parent.join("locked.zerus-release-example");
            let sid = string(&record, "session_id");
            let text = if case == "restore-newlines" {
                format!("zerus:{sid}\n\n")
            } else {
                format!("zerus:{sid}\n")
            };
            fs::write(&taken, text).unwrap();
            let existing = case.contains("existing") || case == "restore-newlines";
            if existing {
                fs::write(&lock, b"other protective lock").unwrap();
            }
            let original = Source::read_existing(&taken, 4096).unwrap().unwrap();
            let locked = Source::read_existing(&lock, 4096)
                .unwrap()
                .unwrap_or_else(|| Source::read_absent(&lock).unwrap());
            if case.ends_with("change") {
                let changing = taken.clone();
                SYNC_FAULT.with(|fault| {
                    *fault.borrow_mut() = Some(Box::new(move || {
                        fs::write(changing, b"changed during namespace sync").unwrap()
                    }))
                });
            }
            let peer = if case == "restore-eof" {
                SYNC_FAULT.with(|fault| *fault.borrow_mut() = Some(Box::new(move || drop(peer))));
                None
            } else {
                Some(peer)
            };
            let result = effects.restore(&lock, &taken, sid, None, &[&original, &locked]);
            if case.ends_with("change") || case == "restore-newlines" || case == "restore-eof" {
                assert!(result.is_err());
                assert!(taken.exists());
                assert!(owner.actor().is_err());
                if existing {
                    assert_eq!(fs::read(&lock).unwrap(), b"other protective lock");
                }
            } else {
                result.unwrap();
                assert!(!taken.exists());
                if existing {
                    assert_eq!(fs::read(&lock).unwrap(), b"other protective lock");
                } else {
                    assert_eq!(fs::read(&lock).unwrap(), original.bytes());
                }
            }
            drop(peer);
        }
        assert!(contended(&root_file));
        drop(effects);
        drop(entered);
        drop(owner);
        done.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(!contended(&root_file));
        return;
    }
    owner.open_scope(&record).unwrap();
    let input = super::super::tests::input(&record);
    assert!(contended(&input));
    if case == "context" {
        let foreign = std::thread::spawn(|| read::route(read::Call::Keys, &[]).unwrap().is_err())
            .join()
            .unwrap();
        assert!(foreign);
        assert!(context::Entered::install(owner.clone()).is_err());
        assert!(read::route(read::Call::Keys, &[]).unwrap().is_err());
        drop(entered);
        assert!(read::route(read::Call::Panes, &[]).unwrap().is_err());
    } else {
        let original = owner.snapshot().unwrap().0;
        let mut next = original.clone();
        next["effort"] = json!("high");
        let path = crate::state::storage::id_record_path(string(&record, "session_id")).unwrap();
        let staged = owner
            .with_scope(|scope| {
                publication::Staged::prepare(
                    &scope.sessions,
                    &scope.record.name,
                    next.to_string().as_bytes(),
                )
            })
            .unwrap();
        let observation = context::Observation::begin().unwrap().unwrap();
        observation.complete(&original).unwrap();
        let fence = publication::CommitFence::native(&owner, &staged).unwrap();
        match case.as_str() {
            "stage-replaced" => {
                let temp = fs::read_dir(path.parent().unwrap())
                    .unwrap()
                    .map(|entry| entry.unwrap().path())
                    .find(|path| {
                        path.file_name()
                            .unwrap()
                            .to_string_lossy()
                            .starts_with(".hgs-native-stage-")
                    })
                    .unwrap();
                fs::rename(&temp, temp.with_extension("kept")).unwrap();
                fs::write(temp, b"foreign stage").unwrap();
            }
            "destination-replaced" => {
                crate::state::storage::atomic(&path, &original.to_string()).unwrap();
            }
            "proof-replaced" => {
                let proof = root()
                    .join("native-input-reservations")
                    .join(format!("sid-{}.json", string(&record, "session_id")));
                let bytes = fs::read(&proof).unwrap();
                fs::rename(&proof, proof.with_extension("kept")).unwrap();
                fs::write(proof, bytes).unwrap();
            }
            "eof" => drop(peer),
            "commit" => {}
            _ => panic!("unknown private driver case"),
        }
        let result = staged.commit(&fence);
        if case == "commit" {
            owner.adopt_record(result.unwrap()).unwrap();
            assert_eq!(owner.snapshot().unwrap().0, next);
        } else {
            assert!(result.is_err());
            assert!(owner.actor().is_err());
            assert_eq!(
                serde_json::from_slice::<Value>(&fs::read(path).unwrap()).unwrap(),
                original
            );
        }
        assert!(contended(&root_file) && contended(&input));
        drop(entered);
    }
    drop(owner);
    done.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(!contended(&root_file) && !contended(&input));
}
