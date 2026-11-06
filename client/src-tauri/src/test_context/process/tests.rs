//! Native Linux controls in a private original test process. Subreaping never
//! changes the suite host and reaps/signals only the exact fixture descendant.
use super::*;
use std::{
    fs,
    io::Write,
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
};
const DRIVER: &str = "ZERUS_TEST_GROUP_DRIVER";
const ROOT: &str = "ZERUS_TEST_GROUP_ROOT";
const BODY: &str = "test_context::process::tests::hung_fixture_body";
const MESSAGE: &str = "controlled watchdog assertion near the stderr tail";

struct Owned(Child);
impl Drop for Owned {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}
struct Adopted(Option<libc::pid_t>);
impl Adopted {
    fn reaped(&mut self) -> Option<i32> {
        let pid = self.0?;
        let mut status = 0;
        // Only the exact known grandchild; never waitpid(-1) or a process scan.
        let result = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
        if result == pid {
            self.0 = None;
            Some(status)
        } else {
            None
        }
    }
}
impl Drop for Adopted {
    fn drop(&mut self) {
        let Some(pid) = self.0 else { return };
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        // Cleanup on a failed assertion is permitted only after the fixture
        // leader has exited and the kernel proves this exact child is ours.
        if unsafe {
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        } != 0
        {
            return;
        }
        let _ = unsafe { libc::kill(pid, libc::SIGKILL) };
        let mut status = 0;
        let _ = unsafe { libc::waitpid(pid, &mut status, 0) };
    }
}
struct Running {
    thread: Option<thread::JoinHandle<Report>>,
    watchdog: Arc<Watchdog>,
}
impl Running {
    fn finish(&mut self) -> Report {
        self.thread.take().unwrap().join().unwrap()
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        if self.thread.is_some() {
            self.watchdog.cancel();
            let _ = self.thread.take().unwrap().join();
        }
    }
}
fn command(root: &Path) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            BODY,
            "--ignored",
            "--test-threads=1",
            "--nocapture",
        ])
        .env(ROOT, root);
    command
}
fn ready(root: &Path, running: &Running) -> serde_json::Value {
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(bytes) = fs::read(root.join("ready.json")) {
            assert!(bytes.len() <= 512);
            return serde_json::from_slice(&bytes).unwrap();
        }
        assert!(
            !running.thread.as_ref().unwrap().is_finished(),
            "fixture exited before readiness"
        );
        assert!(Instant::now() < end, "fixture startup never became ready");
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn timeout_and_cancel_stop_the_owned_grandchild_and_keep_failure_output() {
    if std::env::var_os(DRIVER).is_none() {
        let thread = thread::current();
        super::super::run_case(thread.name().unwrap(), Duration::from_secs(90), |command| {
            command.env(DRIVER, "1");
        });
        return;
    }
    // This is a fresh owned driver, not the full parallel host. Adopt only its
    // fixture's orphan after the group leader is killed, so it can be reaped.
    assert_eq!(
        unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) },
        0
    );
    let mut sibling = Owned(
        Command::new("/bin/sleep")
            .arg("300")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    for cancel in [false, true] {
        let root = std::env::temp_dir().join(format!(
            "zerus-fixture-group-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        // On failure drop the runner first, then reap the adopted descendant.
        let mut descendant = Adopted(None);
        let watchdog = Watchdog::new(Instant::now() + Duration::from_secs(30));
        let (clock, program) = (watchdog.clone(), command(&root));
        let mut running = Running {
            watchdog: watchdog.clone(),
            thread: Some(thread::spawn(move || run(program, clock, Arc::new(())))),
        };
        let receipt = ready(&root, &running);
        let child = receipt["sleep"].as_i64().unwrap() as libc::pid_t;
        assert_ne!(child as u32, sibling.0.id());
        descendant.0 = Some(child);
        assert_eq!(receipt["body"], receipt["group"]);
        assert_ne!(
            receipt["group"].as_i64().unwrap(),
            i64::from(unsafe { libc::getpgrp() })
        );
        // Start the stop only after native child readiness, rather than making
        // a loaded runner's startup time consume the test's stop interval.
        if cancel {
            watchdog.cancel();
        } else {
            watchdog.expire();
        }
        let report = running.finish();
        assert_eq!(u64::from(report.pid), receipt["body"].as_u64().unwrap());
        assert_eq!(
            report.stopped,
            Some(if cancel {
                Stopped::Cancelled
            } else {
                Stopped::TimedOut
            })
        );
        assert!(report.complete, "{}", report.diagnostic());
        let diagnostic = report.diagnostic();
        assert!(diagnostic.contains(if cancel { "cancelled" } else { "timed out" }));
        assert!(diagnostic.contains("fixture stdout before hanging"));
        assert!(diagnostic.contains(MESSAGE), "{diagnostic}");
        let end = Instant::now() + Duration::from_secs(5);
        let status = loop {
            if let Some(status) = descendant.reaped() {
                break status;
            }
            assert!(
                Instant::now() < end,
                "owned sleep grandchild is still alive"
            );
            thread::sleep(Duration::from_millis(5));
        };
        assert!(libc::WIFSIGNALED(status));
        assert_eq!(libc::WTERMSIG(status), libc::SIGKILL);
        assert!(
            sibling.0.try_wait().unwrap().is_none(),
            "the other group remains untouched"
        );
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
#[ignore = "private subprocess body, started only by its own group controller"]
// The private driver adopts and reaps this descendant after stopping our group.
#[allow(clippy::zombie_processes)]
fn hung_fixture_body() {
    let root =
        PathBuf::from(std::env::var_os(ROOT).expect("private body needs its own controller"));
    let sleep = Command::new("/bin/sleep").arg("300").spawn().unwrap();
    let receipt = serde_json::json!({"body":std::process::id(), "group":unsafe{libc::getpgrp()}, "sleep":sleep.id()});
    println!("fixture stdout before hanging");
    std::io::stdout().flush().unwrap();
    std::io::stderr().write_all(&vec![b'x'; 70_000]).unwrap();
    std::panic::set_hook(Box::new(move |info| {
        eprintln!("{info}");
        std::io::stderr().flush().unwrap();
        let pending = root.join("ready.pending");
        fs::write(&pending, receipt.to_string()).unwrap();
        fs::rename(pending, root.join("ready.json")).unwrap();
        thread::sleep(Duration::from_secs(300));
    }));
    panic!("{MESSAGE}");
}
