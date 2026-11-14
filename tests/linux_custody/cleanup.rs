//! The harness is a fresh private reaper. Its fixtures, including adopted
//! members after a deliberately failed owner, stay charged until kernel drain.
//! No process outside this creation domain is inspected or signalled.
use super::*;
use std::{
    io,
    panic::{catch_unwind, AssertUnwindSafe},
    path::PathBuf,
};

pub(super) struct Domain {
    namespaces: Vec<tempfile::TempDir>,
    reaped: usize,
}
impl Domain {
    fn new() -> Self {
        assert!(empty(), "fixture reaper must start without any children");
        let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
        assert_eq!(
            unsafe { libc::sigaction(libc::SIGCHLD, std::ptr::null(), &mut action) },
            0
        );
        assert_eq!(action.sa_sigaction, libc::SIG_DFL);
        assert_eq!(action.sa_flags & libc::SA_NOCLDWAIT, 0);
        assert_eq!(
            unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) },
            0
        );
        Self {
            namespaces: Vec::new(),
            reaped: 0,
        }
    }
    pub(super) fn namespace(&mut self) -> PathBuf {
        assert!(self.namespaces.len() < 64);
        let dir = tempfile::Builder::new()
            .prefix("zerus-kernel-fixture-")
            .tempdir()
            .unwrap();
        let path = dir.path().to_owned();
        self.namespaces.push(dir);
        path
    }
    fn poll(&mut self) -> io::Result<bool> {
        for _ in 0..64 {
            let mut info = unsafe { std::mem::zeroed::<libc::siginfo_t>() };
            let result = unsafe {
                libc::waitid(
                    libc::P_ALL,
                    0,
                    &mut info,
                    libc::WEXITED | libc::WNOHANG | libc::__WALL,
                )
            };
            if result != 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(libc::ECHILD) {
                    return Ok(true);
                }
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(error);
            }
            if unsafe { info.si_pid() } == 0 {
                return Ok(false);
            }
            self.reaped = self.reaped.saturating_add(1);
        }
        Ok(false)
    }
    fn drain(&mut self, until: Instant) -> io::Result<()> {
        loop {
            if Instant::now() >= until {
                return Err(io::ErrorKind::TimedOut.into());
            }
            if self.poll()? {
                return Ok(());
            }
            tick();
        }
    }
}

fn empty() -> bool {
    let mut info = unsafe { std::mem::zeroed::<libc::siginfo_t>() };
    let result = unsafe {
        libc::waitid(
            libc::P_ALL,
            0,
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT | libc::__WALL,
        )
    };
    result == -1 && io::Error::last_os_error().raw_os_error() == Some(libc::ECHILD)
}

/// One deadline for final/unwind settlement. Existing fixtures self-bound
/// adoption to ten seconds and framed I/O to five; no timeout is renewed.
pub(super) fn run(report: Option<&Path>, expire: bool, body: impl FnOnce(&mut Domain)) {
    let mut domain = Domain::new();
    let result = catch_unwind(AssertUnwindSafe(|| body(&mut domain)));
    let until = Instant::now()
        + if expire {
            Duration::ZERO
        } else {
            Duration::from_secs(15)
        };
    let cleanup = domain.drain(until);
    // A separate postcondition catches an omitted-drain mutant. Rescue keeps
    // its owned members here even when the test has already failed logically.
    let clean = empty();
    if cleanup.is_err() || !clean {
        eprintln!(
            "UNRESOLVED fixture cleanup; retaining reaper {} and namespaces {:?}",
            std::process::id(),
            domain
                .namespaces
                .iter()
                .map(|d| d.path())
                .collect::<Vec<_>>()
        );
        if let Some(path) = report {
            std::fs::write(path.join("cleanup.json"), r#"{"settled":false}"#).unwrap();
        }
        // Physical retirement, not a renewed test budget or a success. Keep
        // this exact outer reaper and all namespace handles until ECHILD.
        loop {
            match domain.poll() {
                Ok(true) => break,
                Ok(false) => tick(),
                Err(error) => {
                    eprintln!("fixture custody retained: {error}");
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
    }
    assert!(empty());
    if let Some(path) = report {
        std::fs::write(
            path.join("cleanup.json"),
            format!(
                r#"{{"settled":true,"unwound":{},"deadline_failed":{},"reaped":{}}}"#,
                result.is_err(),
                cleanup.is_err(),
                domain.reaped
            ),
        )
        .unwrap();
    }
    drop(domain); // Files disappear only after every fixture member settled.
    if let Err(payload) = result {
        std::panic::resume_unwind(payload);
    }
    assert!(
        cleanup.is_ok() && clean,
        "fixture cleanup failed its original bound/postcondition"
    );
}

pub(super) fn probe(case: &str, dir: &Path) {
    run(Some(dir), case == "expiry", |_| {
        let mut child = command(
            "--root",
            if case == "expiry" {
                "cleanup-late"
            } else {
                "external"
            },
            dir,
        )
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
        if case == "unwind" {
            panic!("expected private cleanup unwind");
        }
        drop(child.stdin.take());
        drop(child); // The fixture reaper, not PID1, owns its eventual wait.
    });
}
