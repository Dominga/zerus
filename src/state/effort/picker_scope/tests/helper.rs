use super::*;
use crate::state::effort::picker_scope::helper::Prepared;
use std::{
    io::{Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    panic::{catch_unwind, AssertUnwindSafe},
};
use zerus_client_core::run::{writer_custody::Failure, GuardedChild};

#[test]
fn owned_picker_scope_returns_only_after_original_helper_and_all_pipe_joins() {
    each("helper:success");
}

#[test]
fn cancellation_drop_and_deadline_keep_both_original_locks_until_descendant_eof() {
    for case in ["cancel", "drop", "expired"] {
        each(&format!("helper:{case}"));
    }
}

#[test]
fn post_spawn_native_refusal_and_unwind_retain_the_original_scope() {
    for case in ["changed", "panic", "withheld"] {
        each(&format!("helper:{case}"));
    }
}

#[test]
fn lost_original_wait_evidence_never_releases_or_reconstructs_custody() {
    each("helper:lost-wait");
}

#[test]
fn pre_spawn_refusal_and_admission_failure_never_start_a_helper() {
    each("helper:before-refused");
    each("helper:expired-admission");
    each("helper:unwaitable-admission");
}

fn until() -> Instant {
    Instant::now() + Duration::from_secs(5)
}

fn await_peer(listener: &UnixListener, deadline: Instant) -> UnixStream {
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut ready = [0; 5];
                stream.read_exact(&mut ready).unwrap();
                assert_eq!(&ready, b"ready");
                return stream;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => panic!("original fixture peer: {error}"),
        }
        assert!(Instant::now() < deadline, "original helper never connected");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn child(mode: &str, socket: &Path, effect: &Path) -> std::process::Child {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("ZERUS_PICKER_HELPER_MODE", mode)
        .env("ZERUS_PICKER_HELPER_SOCKET", socket)
        .env("ZERUS_PICKER_HELPER_EFFECT", effect)
        .args([
            "--exact",
            "state::effort::picker_scope::tests::helper::original_helper",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command.spawn().unwrap()
}

fn writer_probe() -> File {
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(root().join(".lock"))
        .unwrap()
}

fn held(writer: &File, input: &File) {
    assert!(
        writer.try_lock_exclusive().is_err(),
        "original Root writer released early"
    );
    assert!(
        input.try_lock_exclusive().is_err(),
        "original SID input exclusion released early"
    );
}

fn released(writer: &File, input: &File) {
    let deadline = until();
    loop {
        let a = writer.try_lock_exclusive();
        if a.is_ok() {
            fs2::FileExt::unlock(writer).unwrap();
        }
        let b = input.try_lock_exclusive();
        if b.is_ok() {
            fs2::FileExt::unlock(input).unwrap();
        }
        if a.is_ok() && b.is_ok() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "physical EOF/joins never released original custody"
        );
        std::thread::sleep(Duration::from_millis(3));
    }
}

pub(super) fn fixture(writer: Guard, mut record: Value, case: &str) {
    let original_input = input(&record);
    let original_writer = writer_probe();
    if case == "expired-admission" {
        assert!(Prepared::new(writer, &record, Receipts::Settings, Instant::now()).is_err());
        released(&original_writer, &original_input);
        return;
    }
    if case == "unwaitable-admission" {
        let mut prior: libc::sigaction = unsafe { std::mem::zeroed() };
        let mut ignored: libc::sigaction = unsafe { std::mem::zeroed() };
        ignored.sa_sigaction = libc::SIG_IGN;
        assert_eq!(
            unsafe { libc::sigaction(libc::SIGCHLD, &ignored, &mut prior) },
            0
        );
        let result = Prepared::new(writer, &record, Receipts::Settings, until());
        assert_eq!(
            unsafe { libc::sigaction(libc::SIGCHLD, &prior, std::ptr::null_mut()) },
            0
        );
        assert!(
            result.is_err(),
            "unsupported wait policy refuses before any spawn"
        );
        released(&original_writer, &original_input);
        return;
    }
    let deadline = Instant::now() + Duration::from_secs(if case == "expired" { 1 } else { 5 });
    let prepared = Prepared::new(writer, &record, Receipts::Settings, deadline).unwrap();
    held(&original_writer, &original_input);
    if case == "before-refused" {
        assert!(prepared
            .spawn(
                |_| Err("original native guard refused".into()),
                |_, _| panic!("must not spawn")
            )
            .is_err());
        released(&original_writer, &original_input);
        return;
    }
    let private = tempfile::tempdir().unwrap();
    let socket = private.path().join("original.sock");
    let effect = private.path().join("argv-effect");
    let listener = UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    if case == "withheld" {
        let mut helper = prepared
            .spawn(
                |_| Ok(()),
                |_, supplied| {
                    assert_eq!(supplied, deadline);
                    Ok(GuardedChild::Withheld(child("exit", &socket, &effect)))
                },
            )
            .unwrap();
        let finish = until();
        let mut drained = loop {
            if let Some(drained) = helper.poll() {
                break drained;
            }
            assert!(Instant::now() < finish);
            std::thread::sleep(Duration::from_millis(2));
        };
        assert_eq!(drained.report.failure, Some(Failure::SourceChanged));
        assert!(
            drained.scope.may_have_sent(),
            "withheld stdin does not prove no argv effect"
        );
        held(&original_writer, &original_input);
        refused(&mut drained.scope, &original_input);
        drop(drained);
        released(&original_writer, &original_input);
        return;
    }
    if case == "lost-wait" {
        let mut helper = prepared
            .spawn(
                |_| Ok(()),
                |_, supplied| {
                    assert_eq!(supplied, deadline);
                    let child = child("exit", &socket, &effect);
                    let mut status = 0;
                    loop {
                        // Deliberately steal this SAME child's wait, without updating
                        // Child's cached status: the custody layer must keep Unknown.
                        let result =
                            unsafe { libc::waitpid(child.id() as i32, &mut status, libc::WNOHANG) };
                        if result == child.id() as i32 {
                            break;
                        }
                        assert_eq!(result, 0);
                        assert!(Instant::now() < supplied);
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    Ok(GuardedChild::Ready(child))
                },
            )
            .unwrap();
        assert!(helper.poll().is_none());
        assert_eq!(helper.failure(), Some(Failure::Unavailable));
        held(&original_writer, &original_input);
        helper.cancel();
        assert!(helper.poll().is_none());
        drop(helper);
        held(&original_writer, &original_input);
        // This process is a bounded re-exec fixture. Retained Unknown remains
        // in its retirement registry until process exit, never a fresh retry.
        return;
    }
    let mut peer = None;
    let mut checks = 0;
    let started = catch_unwind(AssertUnwindSafe(|| {
        prepared.spawn(
            |native| {
                checks += 1;
                assert_eq!(native["session_id"], record["session_id"]);
                if checks == 2 {
                    peer = Some(await_peer(&listener, deadline));
                    held(&original_writer, &original_input);
                    assert!(
                        effect.exists(),
                        "argv effect already possible before stdin EOF"
                    );
                    if case == "changed" {
                        record["pane"] = json!("%changed");
                        save(&record);
                    } else if case == "panic" {
                        panic!("private post-spawn native-check cancellation");
                    }
                }
                Ok(())
            },
            |_, supplied| {
                assert_eq!(supplied, deadline);
                Ok(GuardedChild::Ready(child("hold", &socket, &effect)))
            },
        )
    }));
    if case == "panic" {
        assert!(started.is_err());
        held(&original_writer, &original_input);
        drop(peer);
        released(&original_writer, &original_input);
        return;
    }
    let mut helper = started.unwrap().unwrap();
    assert_eq!(checks, 2);
    assert!(
        helper.poll().is_none(),
        "parent exit cannot stand in for descendant EOF"
    );
    held(&original_writer, &original_input);
    if case == "cancel" {
        helper.cancel();
        assert_eq!(helper.failure(), Some(Failure::Cancelled));
    } else if case == "expired" {
        while Instant::now() <= deadline {
            assert!(helper.poll().is_none());
            std::thread::sleep(Duration::from_millis(3));
        }
        assert!(helper.poll().is_none());
        assert_eq!(helper.failure(), Some(Failure::Expired));
    }
    if matches!(case, "drop" | "cancel" | "expired") {
        drop(helper);
        held(&original_writer, &original_input);
        drop(peer);
        // No new admission or call is used to make the poller release custody.
        released(&original_writer, &original_input);
        return;
    }
    drop(peer);
    let finish = until();
    let mut drained = loop {
        if let Some(drained) = helper.poll() {
            break drained;
        }
        assert!(Instant::now() < finish);
        std::thread::sleep(Duration::from_millis(2));
    };
    held(&original_writer, &original_input);
    assert!(drained.scope.may_have_sent());
    if case == "changed" {
        assert!(drained.report.failure.is_some());
        refused(&mut drained.scope, &original_input);
    } else {
        assert_eq!(case, "success");
        assert!(drained.report.exit.success());
        assert_eq!(drained.report.failure, None);
        assert!(drained.report.input_written);
        assert_eq!(drained.scope.until, deadline);
        drained
            .scope
            .receipt_commit(
                |_| Ok(()),
                |directory, _| directory.write_new("after-original-drain", b"retained"),
            )
            .unwrap();
    }
    drop(drained);
    released(&original_writer, &original_input);
}

#[test]
#[ignore = "original private helper or descendant, invoked only by bounded fixtures"]
// The original helper deliberately exits while its descendant retains the two
// output pipes. The bounded fixture owns its private release endpoint; reaping
// that descendant here would invalidate the after-parent-exit custody control.
#[allow(clippy::zombie_processes)]
fn original_helper() {
    let Ok(mode) = std::env::var("ZERUS_PICKER_HELPER_MODE") else {
        return;
    };
    let socket = PathBuf::from(std::env::var_os("ZERUS_PICKER_HELPER_SOCKET").unwrap());
    if mode == "descendant" {
        let mut peer = UnixStream::connect(socket).unwrap();
        peer.write_all(b"ready").unwrap();
        let mut buffer = [0; 1];
        assert_eq!(peer.read(&mut buffer).unwrap(), 0);
        std::io::stdout()
            .write_all(b"original output EOF follows this descendant")
            .unwrap();
        std::io::stderr()
            .write_all(b"original error EOF follows this descendant")
            .unwrap();
        return;
    }
    fs::write(
        std::env::var_os("ZERUS_PICKER_HELPER_EFFECT").unwrap(),
        b"possible argv effect",
    )
    .unwrap();
    if mode == "exit" {
        return;
    }
    assert_eq!(mode, "hold");
    let mut descendant = Command::new(std::env::current_exe().unwrap());
    descendant
        .env("ZERUS_PICKER_HELPER_MODE", "descendant")
        .args([
            "--exact",
            "state::effort::picker_scope::tests::helper::original_helper",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    let _original_descendant = descendant.spawn().unwrap();
    let mut bytes = Vec::new();
    std::io::stdin().read_to_end(&mut bytes).unwrap();
    assert!(
        bytes.is_empty(),
        "adapter offers only the guarded original empty EOF"
    );
}
