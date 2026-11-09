//! Private libtest stand-in; Apple platform executables can hide their environment.
use std::{
    io::{Read, Write},
    os::fd::AsRawFd,
    process::{Child, Command},
    time::{Duration, Instant},
};

const READY: &[u8] = b"\nzerus-fixture-echo-ready\n";

pub(crate) fn echo_command() -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "test_process::echo_child",
            "--nocapture",
            "--quiet",
        ])
        .env("HGS_TEST_PROCESS_ECHO", "1");
    command
}

/// Wait for actual exec and I/O, allowing only the bounded libtest preamble.
/// Readiness has a setup bound; the child then lives until its owner closes stdin.
pub(crate) fn echo_ready(child: &mut Child) {
    child.stdin.as_mut().unwrap().write_all(READY).unwrap();
    let output = child.stdout.as_mut().unwrap();
    let until = Instant::now() + Duration::from_secs(30);
    let mut prefix = Vec::new();
    while prefix.len() < 4096 {
        assert!(Instant::now() < until, "stand-in exec/readiness timed out");
        let mut descriptor = libc::pollfd {
            fd: output.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let remaining = until.saturating_duration_since(Instant::now());
        // SAFETY: one live stdout descriptor and one initialized pollfd.
        let result = unsafe {
            libc::poll(
                &mut descriptor,
                1,
                remaining.as_millis().clamp(1, i32::MAX as u128) as i32,
            )
        };
        if result < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        assert!(result > 0, "stand-in exec/readiness unavailable");
        let mut byte = [0];
        output.read_exact(&mut byte).unwrap();
        prefix.push(byte[0]);
        if prefix.ends_with(READY) {
            return;
        }
    }
    panic!("stand-in libtest preamble exceeds its bound");
}

#[test]
fn echo_child() {
    if std::env::var("HGS_TEST_PROCESS_ECHO").as_deref() != Ok("1") {
        return;
    }
    std::io::copy(&mut std::io::stdin().lock(), &mut std::io::stdout().lock()).unwrap();
}
