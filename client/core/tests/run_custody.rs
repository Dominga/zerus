//! Private synthetic subprocesses only. Descriptor-flag mutations run in an
//! isolated copy of this test executable, never beside other tests' spawns.
#![cfg(any(target_os = "linux", target_os = "macos"))]

use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Write},
    os::fd::{AsRawFd, FromRawFd},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::AtomicBool,
    thread,
    time::{Duration, Instant},
};
use zerus_client_core::{
    run::{custody::InheritedLock, run_owned, OwnedError},
    Invocation,
};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("hgs-custody-{}", std::process::id()));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }
    fn file(&self) -> PathBuf {
        self.0.join("attempt.lock")
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn file(path: &Path) -> File {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap()
}
fn held(path: &Path) -> bool {
    match InheritedLock::try_lock(File::open(path).unwrap()) {
        Ok(_) => false,
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => true,
        Err(error) => panic!("unexpected lock error: {error}"),
    }
}
fn released(path: &Path) {
    let until = Instant::now() + Duration::from_secs(5);
    while held(path) && Instant::now() < until {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(!held(path), "synthetic resource did not close");
}
fn isolated(case: &str) {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "isolated_worker", "--nocapture"])
        .env("CUSTODY_CASE", case)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let until = Instant::now() + Duration::from_secs(15);
    while child.try_wait().unwrap().is_none() && Instant::now() < until {
        thread::sleep(Duration::from_millis(10));
    }
    if child.try_wait().unwrap().is_none() {
        child.kill().unwrap();
    }
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn close_only_clones_keep_one_nonblocking_lock() {
    isolated("clones");
}
#[test]
fn only_custody_is_inherited_and_parent_flags_do_not_change() {
    isolated("whitelist");
}
#[test]
fn whitelist_covers_descriptors_above_a_reduced_soft_limit() {
    isolated("reduced");
}
#[test]
fn custody_is_never_remapped_as_standard_input() {
    isolated("stdio");
}
#[test]
fn failed_exec_still_reports_its_error_and_keeps_parent_custody() {
    isolated("failed");
}
#[test]
fn child_keeps_custody_after_all_parent_copies_close() {
    isolated("child");
}
#[test]
fn owned_output_reader_keeps_custody_after_exchange_returns() {
    isolated("resources");
}

#[test]
#[cfg(target_os = "linux")]
fn oversized_descriptor_roster_refuses_before_spawning() {
    isolated("pressure");
}

#[test]
fn service_exit_before_registration_leaves_the_synthetic_helper_charged() {
    // No durable registration is made. This proves only the inherited-lock
    // mechanism; exact-ticket journal and private-file verification are G1 work.
    let temp = Temp::new();
    let result = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "isolated_worker", "--nocapture"])
        .env("CUSTODY_CASE", "crash")
        .env("CUSTODY_ROOT", &temp.0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(held(&temp.file()));
    released(&temp.file());
}

#[test]
fn isolated_worker() {
    let Ok(case) = std::env::var("CUSTODY_CASE") else {
        return;
    };
    if case == "crash" {
        let root = PathBuf::from(std::env::var_os("CUSTODY_ROOT").unwrap());
        let lock = InheritedLock::try_lock(file(&root.join("attempt.lock"))).unwrap();
        let mut command = Command::new("python3");
        command
            .args(["-c", "import time;time.sleep(1)"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        lock.inherit(&mut command).unwrap();
        let _unregistered = command.spawn().unwrap();
        // Simulate only our private service process dying, without Rust Drop.
        std::process::exit(0);
    }
    let temp = Temp::new();
    if case == "stdio" {
        unsafe {
            libc::close(0);
        }
    }
    let lock = InheritedLock::try_lock(file(&temp.file())).unwrap();
    assert!(lock.as_raw_fd() >= 3);
    let parent_flags = unsafe { libc::fcntl(lock.as_raw_fd(), libc::F_GETFD) };
    assert_ne!(parent_flags & libc::FD_CLOEXEC, 0);
    match case.as_str() {
        "pressure" => {
            let mut limit = std::mem::MaybeUninit::<libc::rlimit>::uninit();
            assert_eq!(
                unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, limit.as_mut_ptr()) },
                0
            );
            let mut limit = unsafe { limit.assume_init() };
            assert!(limit.rlim_max >= 10000);
            limit.rlim_cur = 10000;
            assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limit) }, 0);
            let copies: Vec<_> = (0..8193)
                .map(|_| {
                    let fd = unsafe { libc::fcntl(lock.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 3) };
                    assert!(fd >= 3);
                    unsafe { File::from_raw_fd(fd) }
                })
                .collect();
            let mut command = Command::new("unused-never-spawned");
            assert_eq!(
                lock.inherit(&mut command).unwrap_err().kind(),
                std::io::ErrorKind::Unsupported
            );
            assert!(held(&temp.file()));
            drop(copies);
        }
        "clones" => {
            let copy = lock.clone();
            assert!(held(&temp.file()));
            drop(lock);
            assert!(held(&temp.file()));
            drop(copy);
            assert!(!held(&temp.file()));
        }
        "whitelist" | "reduced" | "stdio" => {
            let sentinel = file(&temp.0.join("unrelated"));
            // Deliberately inheritable and high, inside this isolated process.
            let fd = unsafe { libc::fcntl(sentinel.as_raw_fd(), libc::F_DUPFD, 256) };
            assert!(fd >= 256);
            let _sentinel_copy = unsafe { File::from_raw_fd(fd) };
            if case == "reduced" {
                let mut limit = std::mem::MaybeUninit::<libc::rlimit>::uninit();
                assert_eq!(
                    unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, limit.as_mut_ptr()) },
                    0
                );
                let mut limit = unsafe { limit.assume_init() };
                limit.rlim_cur = 64;
                assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limit) }, 0);
            }
            let mut command = Command::new("python3");
            command.args(["-c", "import os,sys,json\nfds=[]\nfor fd in range(3,1024):\n try: os.fstat(fd);fds.append(fd)\n except OSError: pass\nprint(json.dumps(fds))"]);
            lock.inherit(&mut command).unwrap();
            let result = command.output().unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(
                serde_json::from_slice::<Vec<i32>>(&result.stdout).unwrap(),
                vec![lock.as_raw_fd()]
            );
            assert_eq!(
                unsafe { libc::fcntl(lock.as_raw_fd(), libc::F_GETFD) },
                parent_flags
            );
            assert_eq!(
                unsafe { libc::fcntl(fd, libc::F_GETFD) } & libc::FD_CLOEXEC,
                0
            );
        }
        "failed" => {
            let mut command = Command::new(temp.0.join("absent-executable"));
            lock.inherit(&mut command).unwrap();
            assert_eq!(
                command.spawn().unwrap_err().kind(),
                std::io::ErrorKind::NotFound
            );
            drop(lock);
            assert!(
                held(&temp.file()),
                "hook retains its descriptor until Command drops"
            );
            drop(command);
            assert!(!held(&temp.file()));
        }
        "child" => {
            let mut command = Command::new("python3");
            command.args(["-c", "import sys,signal;signal.alarm(8);print('ready',flush=True);sys.stdin.buffer.read()"])
                .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
            lock.inherit(&mut command).unwrap();
            let mut child = command.spawn().unwrap();
            drop(command);
            drop(lock);
            let mut line = String::new();
            BufReader::new(child.stdout.take().unwrap())
                .read_line(&mut line)
                .unwrap();
            assert_eq!(line, "ready\n");
            assert!(held(&temp.file()));
            child.stdin.take().unwrap().write_all(b"partial").unwrap();
            assert!(child.wait().unwrap().success());
            released(&temp.file());
        }
        "resources" => {
            let inv = Invocation {
                args: vec!["-c".into(), "import os,sys,time\nsys.stdin.buffer.read()\nif os.fork()==0:\n time.sleep(1);os._exit(0)\nos._exit(0)".into()],
                stdin: Some(vec![]), timeout: Duration::from_millis(150), request_id: None,
            };
            let spawned = lock.clone();
            let result = run_owned(
                Path::new("python3"),
                &inv,
                &[],
                &AtomicBool::new(false),
                move |command| {
                    spawned.inherit(command)?;
                    command.spawn()
                },
                lock,
            );
            assert_eq!(result, Err(OwnedError::TimedOut));
            assert!(held(&temp.file()));
            released(&temp.file());
        }
        _ => panic!("unknown isolated case"),
    }
}
