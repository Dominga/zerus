use super::*;
use std::{
    ffi::CString,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
};

const ENTRY: &str = "platform::unix::helper_group::tests::spawn_turn::private_spawn_probe";

fn probe(case: &str) {
    let root = tempfile::tempdir().unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            ENTRY,
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", root.path())
        .env("HGS_CONFIG_DIR", root.path().join("config"))
        .env("HGS_STATE_DIR", root.path().join("state"))
        .env("HGS_TEST_SPAWN_TURN", case);
    let result = super::super::super::run_bounded(
        &mut command,
        [32 << 10; 2],
        Instant::now() + Duration::from_secs(10),
        &|| false,
    )
    .unwrap();
    assert!(
        result.0.success(),
        "{case}: {} {}",
        String::from_utf8_lossy(&result.1),
        String::from_utf8_lossy(&result.2)
    );
    assert!(
        String::from_utf8_lossy(&result.1).contains("test result: ok. 1 passed; 0 failed;"),
        "the exact private admission control must run once"
    );
}

#[test]
fn an_expired_spawn_turn_returns_without_admitting_a_child() {
    probe("expired");
}

#[test]
fn cancellation_returns_while_the_spawn_turn_is_still_held() {
    probe("cancelled");
}

#[test]
fn releasing_the_turn_does_not_renew_the_original_deadline() {
    probe("late-release");
}

#[test]
fn cancellation_is_checked_again_inside_the_acquired_turn() {
    probe("final-cancel");
}

#[test]
fn the_deadline_is_checked_after_the_final_cancellation_callback() {
    probe("final-deadline");
}

#[test]
fn an_on_time_turn_still_serializes_and_starts_exactly_one_child() {
    probe("on-time");
}

fn marked_command(marker: &Path) -> Command {
    let name = CString::new(marker.as_os_str().as_encoded_bytes()).unwrap();
    let mut command = Command::new("/usr/bin/true");
    command.env_clear().env("PATH", "/usr/bin:/bin");
    unsafe {
        command.pre_exec(move || {
            // Count actual admission, even if a later stop kills the child
            // before its executable has had a chance to run.
            let fd = libc::open(
                name.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_APPEND | libc::O_CLOEXEC,
                0o600,
            );
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            let written = libc::write(fd, b"x".as_ptr().cast(), 1);
            libc::close(fd);
            if written != 1 {
                return Err(io::Error::other("private admission marker"));
            }
            Ok(())
        });
    }
    command
}

fn contended(case: &str, marker: &Path) {
    let turn = super::super::super::spawn_turn();
    let deadline = Instant::now()
        + if matches!(case, "expired" | "late-release") {
            Duration::from_millis(150)
        } else {
            Duration::from_secs(3)
        };
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_cancelled = cancelled.clone();
    let mut command = marked_command(marker);
    let (entered, started) = mpsc::channel();
    let (send, receive) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let announced = Cell::new(false);
        let outcome = super::super::super::run_bounded(&mut command, [16; 2], deadline, &|| {
            if !announced.replace(true) {
                let _ = entered.send(());
            }
            worker_cancelled.load(Ordering::SeqCst)
        });
        let _ = send.send(matches!(
            outcome,
            Err(super::super::super::RunError::Stopped)
        ));
        outcome
    });
    let entered = started.recv_timeout(Duration::from_secs(1));
    let before_release = match case {
        "expired" => receive.recv_timeout(Duration::from_millis(500)).ok(),
        "cancelled" => {
            cancelled.store(true, Ordering::SeqCst);
            receive.recv_timeout(Duration::from_millis(500)).ok()
        }
        "late-release" => {
            std::thread::sleep(
                deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(75),
            );
            None
        }
        "on-time" => {
            // No helper may start while a different spawn owns this turn.
            receive.recv_timeout(Duration::from_millis(30)).ok()
        }
        _ => unreachable!(),
    };
    let early_marker = marker.exists();
    // Always release and join before asserting, including negative controls.
    drop(turn);
    let result = worker.join().unwrap();
    assert!(entered.is_ok());
    assert!(!early_marker);
    if case == "on-time" {
        assert_eq!(before_release, None);
        assert!(result.unwrap().0.success());
        assert_eq!(fs::read(marker).unwrap(), b"x");
    } else {
        assert!(matches!(
            result,
            Err(super::super::super::RunError::Stopped)
        ));
        if case != "late-release" {
            assert_eq!(
                before_release,
                Some(true),
                "must return while the turn is held"
            );
        }
        assert!(
            !marker.exists(),
            "must not admit even a briefly running child"
        );
    }
}

#[test]
#[ignore = "exact private reexec: isolates the process-wide spawn mutex"]
fn private_spawn_probe() {
    let case = std::env::var("HGS_TEST_SPAWN_TURN").unwrap();
    assert!(matches!(
        case.as_str(),
        "expired" | "cancelled" | "late-release" | "on-time" | "final-cancel" | "final-deadline"
    ));
    let marker = Path::new(&std::env::var("HOME").unwrap()).join("admitted");
    if matches!(case.as_str(), "final-cancel" | "final-deadline") {
        let calls = Cell::new(0);
        let deadline = Instant::now() + Duration::from_millis(150);
        let result = super::super::super::run_bounded(
            &mut marked_command(&marker),
            [16; 2],
            deadline,
            &|| {
                let count = calls.get() + 1;
                calls.set(count);
                if count == 2 && case == "final-deadline" {
                    std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
                }
                count >= 2 && case == "final-cancel"
            },
        );
        assert!(matches!(
            result,
            Err(super::super::super::RunError::Stopped)
        ));
        assert!(calls.get() >= 2);
        assert!(!marker.exists());
    } else {
        contended(&case, &marker);
    }
}
