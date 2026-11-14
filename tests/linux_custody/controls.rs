#[path = "cleanup.rs"]
mod cleanup;
#[path = "fixtures.rs"]
mod fixtures;
use super::custody::{Custody, Failure, Snapshot};
use std::{
    io::{Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(10)
}
fn tick() {
    std::thread::sleep(Duration::from_millis(2));
}
fn command(mode: &str, case: &str, dir: &Path) -> Command {
    let mut c = Command::new(std::env::current_exe().unwrap());
    c.args([mode, case])
        .arg(dir)
        .env_clear()
        .env("PATH", "/usr/bin:/bin");
    c
}
fn frame(stream: &mut UnixStream, text: &str) {
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream.write_all(format!("{text}\n").as_bytes()).unwrap();
}
fn read_frame(stream: &mut UnixStream) -> String {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut text = Vec::new();
    loop {
        let mut byte = [0];
        stream.read_exact(&mut byte).unwrap();
        if byte[0] == b'\n' {
            break;
        }
        assert!(text.len() < 128);
        text.push(byte[0]);
    }
    String::from_utf8(text).unwrap()
}
fn until(c: &mut Custody, accept: impl Fn(Snapshot) -> bool) -> Snapshot {
    let end = deadline();
    loop {
        let s = c.poll();
        if accept(s) {
            return s;
        }
        assert!(
            Instant::now() < end,
            "no expected custody transition: {s:?}"
        );
        tick();
    }
}
fn accept(listener: &UnixListener) -> UnixStream {
    let end = deadline();
    loop {
        match listener.accept() {
            Ok((stream, _)) => return stream,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < end);
                tick();
            }
            Err(e) => panic!("accept: {e}"),
        }
    }
}

pub fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args.len() == 4 {
        match args[1].as_str() {
            "--owner" => owner(&args[2], Path::new(&args[3])),
            "--root" => fixtures::root(&args[2], Path::new(&args[3])),
            "--cleanup" => cleanup::probe(&args[2], Path::new(&args[3])),
            _ => panic!("unknown private mode"),
        }
        return;
    }
    cleanup::run(None, false, suite);
}

fn suite(domain: &mut cleanup::Domain) {
    let mut unrelated = command("--root", "external", Path::new("."))
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    for case in [
        "root",
        "daemon",
        "clone",
        "clone-direct",
        "event-cap",
        "policy",
        "no-child-wait",
        "attestation",
        "waiter",
        "deadline",
        "second",
        "cap",
        "owner-pin",
        "root-pin",
        "drop",
        "owner-death",
        "bad-policy",
        "already-child",
        "already-subreaper",
        "threads",
        "expired",
        "long-budget",
    ] {
        let home = domain.namespace();
        let after_owner = UnixListener::bind(home.join("after-owner.sock")).unwrap();
        after_owner.set_nonblocking(true).unwrap();
        let mut child = command("--owner", case, &home)
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        let end = deadline() + Duration::from_secs(3);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= end {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("private owner exceeded outer bound: {case}");
            }
            tick();
        };
        assert!(status.success(), "kernel control failed: {case}");
        if case == "owner-death" {
            let mut survivor = accept(&after_owner);
            let data = read_frame(&mut survivor);
            let mut parts = data.split_ascii_whitespace();
            let pid: i32 = parts.next().unwrap().parse().unwrap();
            assert_eq!(parts.next().unwrap(), std::process::id().to_string());
            frame(&mut survivor, "exit");
            let mut exit = 0;
            assert_eq!(unsafe { libc::waitpid(pid, &mut exit, 0) }, pid);
            assert!(libc::WIFEXITED(exit));
            assert_eq!(libc::WEXITSTATUS(exit), 17);
        }
        assert!(
            unrelated.try_wait().unwrap().is_none(),
            "unrelated external child affected"
        );
        println!("PASS {case}");
    }
    drop(unrelated.stdin.take());
    assert_eq!(unrelated.wait().unwrap().code(), Some(17));
    for case in ["normal", "unwind", "expiry"] {
        let home = domain.namespace();
        let mut child = command("--cleanup", case, &home).spawn().unwrap();
        let until = Instant::now() + Duration::from_secs(20);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            assert!(
                Instant::now() < until,
                "cleanup probe not physically settled: {case}"
            );
            tick();
        };
        assert_eq!(status.success(), case == "normal");
        let report: serde_json::Value =
            serde_json::from_slice(&std::fs::read(home.join("cleanup.json")).unwrap()).unwrap();
        assert_eq!(report["settled"], true);
        assert_eq!(report["unwound"], case == "unwind");
        assert_eq!(report["deadline_failed"], case == "expiry");
        assert_eq!(report["reaped"], 1);
        println!("PASS cleanup-{case}");
    }
}

fn owner(case: &str, dir: &Path) {
    if matches!(
        case,
        "bad-policy"
            | "already-child"
            | "already-subreaper"
            | "threads"
            | "expired"
            | "long-budget"
    ) {
        admission(case, dir);
        return;
    }
    let listener = UnixListener::bind(dir.join("root.sock")).unwrap();
    listener.set_nonblocking(true).unwrap();
    let descendants = UnixListener::bind(dir.join("child.sock")).unwrap();
    descendants.set_nonblocking(true).unwrap();
    let end = if case == "deadline" {
        Instant::now() + Duration::from_secs(1)
    } else {
        deadline()
    };
    let mut c = Custody::spawn(&mut command("--root", case, dir), end).unwrap();
    let mut stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < end);
                tick();
            }
            Err(e) => panic!("accept: {e}"),
        }
    };
    assert_eq!(read_frame(&mut stream), "ready");
    assert_eq!(c.poll(), Snapshot::Pending);
    match case {
        "root" => {
            frame(&mut stream, "exit");
            assert_eq!(
                until(&mut c, |s| s != Snapshot::Pending),
                Snapshot::Complete {
                    root: 23,
                    adopted: 0
                }
            );
        }
        "daemon" | "clone" => {
            frame(&mut stream, "fork");
            assert_eq!(read_frame(&mut stream), "forked");
            let mut child = accept(&descendants);
            let pid: i32 = read_frame(&mut child).parse().unwrap();
            assert!(pid > 0);
            frame(&mut stream, "root-exit");
            // The daemon itself proves its new native parent is THIS owner.
            assert_eq!(
                read_frame(&mut child),
                format!("adopted {}", std::process::id())
            );
            let end = deadline();
            while c.root_exit().is_none() {
                assert_eq!(c.poll(), Snapshot::Pending);
                assert!(Instant::now() < end);
                tick();
            }
            for _ in 0..3 {
                assert_eq!(c.poll(), Snapshot::Pending);
            }
            frame(&mut child, "child-exit");
            assert!(matches!(
                until(&mut c, |s| s != Snapshot::Pending),
                Snapshot::Complete {
                    root: 23,
                    adopted: 1..
                }
            ));
        }
        "event-cap" => {
            // Fixture-only direct owned children exercise the fairness seam
            // without creating thousands of processes on a shared machine.
            let end = deadline();
            for _ in 0..70 {
                let pid = unsafe { libc::fork() };
                assert!(pid >= 0);
                if pid == 0 {
                    unsafe {
                        libc::_exit(17);
                    }
                }
                loop {
                    let mut info = unsafe { std::mem::zeroed::<libc::siginfo_t>() };
                    assert_eq!(
                        unsafe {
                            libc::waitid(
                                libc::P_PID,
                                pid as u32,
                                &mut info,
                                libc::WEXITED | libc::WNOWAIT | libc::WNOHANG,
                            )
                        },
                        0
                    );
                    if unsafe { info.si_pid() } == pid {
                        break;
                    }
                    assert!(Instant::now() < end);
                    tick();
                }
            }
            assert_eq!(c.poll(), Snapshot::Pending);
            assert_eq!(c.test_exit_count(), 64);
            assert_eq!(c.poll(), Snapshot::Pending);
            assert_eq!(c.test_exit_count(), 70);
            frame(&mut stream, "exit");
            assert_eq!(
                until(&mut c, |s| s != Snapshot::Pending),
                Snapshot::Complete {
                    root: 23,
                    adopted: 70
                }
            );
        }
        "clone-direct" => {
            // Low-level wait-policy control: inject one direct kernel-owned
            // no-SIGCHLD child. Adoption can change exit-signal semantics, so
            // the orphan-clone control alone does not prove __WALL coverage.
            // This fixture-only spawn is not a production admission path.
            let pid =
                unsafe { libc::syscall(libc::SYS_clone, 0usize, 0usize, 0usize, 0usize, 0usize) };
            assert!(pid >= 0);
            if pid == 0 {
                let mut child = UnixStream::connect(dir.join("child.sock")).unwrap();
                frame(&mut child, "ready");
                assert_eq!(read_frame(&mut child), "exit");
                unsafe {
                    libc::_exit(17);
                }
            }
            let mut child = accept(&descendants);
            assert_eq!(read_frame(&mut child), "ready");
            frame(&mut stream, "exit");
            let end = deadline();
            while c.root_exit().is_none() {
                assert_eq!(c.poll(), Snapshot::Pending);
                assert!(Instant::now() < end);
                tick();
            }
            assert_eq!(c.poll(), Snapshot::Pending);
            frame(&mut child, "exit");
            assert_eq!(
                until(&mut c, |s| s != Snapshot::Pending),
                Snapshot::Complete {
                    root: 23,
                    adopted: 1
                }
            );
        }
        "policy" | "no-child-wait" => {
            let mut changed = unsafe { std::mem::zeroed::<libc::sigaction>() };
            if case == "policy" {
                changed.sa_sigaction = libc::SIG_IGN;
            } else {
                changed.sa_flags = libc::SA_NOCLDWAIT;
            }
            assert_eq!(
                unsafe { libc::sigaction(libc::SIGCHLD, &changed, std::ptr::null_mut()) },
                0
            );
            assert_eq!(c.poll(), Snapshot::Unresolved(Failure::Policy));
            unsafe {
                libc::signal(libc::SIGCHLD, libc::SIG_DFL);
            }
            frame(&mut stream, "exit");
            assert_eq!(c.poll(), Snapshot::Unresolved(Failure::Policy));
        }
        "attestation" => {
            assert_eq!(
                unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 0, 0, 0, 0) },
                0
            );
            assert_eq!(c.poll(), Snapshot::Unresolved(Failure::Policy));
            frame(&mut stream, "exit");
        }
        "waiter" => {
            frame(&mut stream, "exit");
            let mut status = 0;
            assert_eq!(
                unsafe { libc::waitpid(c.root_id() as i32, &mut status, 0) },
                c.root_id() as i32
            );
            assert_eq!(c.poll(), Snapshot::Unresolved(Failure::Wait));
            assert_eq!(c.poll(), Snapshot::Unresolved(Failure::Wait));
        }
        "deadline" => {
            while Instant::now() < end {
                tick();
            }
            assert_eq!(c.poll(), Snapshot::Unresolved(Failure::Deadline));
            frame(&mut stream, "exit");
            assert_eq!(c.poll(), Snapshot::Unresolved(Failure::Deadline));
        }
        "second" => {
            assert!(Custody::spawn(&mut command("--root", "root", dir), deadline()).is_err());
            frame(&mut stream, "exit");
        }
        "cap" => {
            c.exhaust_test_capacity();
            frame(&mut stream, "fork");
            assert_eq!(read_frame(&mut stream), "forked");
            let mut child = accept(&descendants);
            let _: i32 = read_frame(&mut child).parse().unwrap();
            assert_eq!(
                read_frame(&mut child),
                format!("adopted {}", std::process::id())
            );
            assert_eq!(c.poll(), Snapshot::Unresolved(Failure::Limit));
            frame(&mut stream, "root-exit");
            frame(&mut child, "child-exit");
            assert_eq!(c.poll(), Snapshot::Unresolved(Failure::Limit));
        }
        "owner-pin" | "root-pin" => {
            if case == "owner-pin" {
                c.lose_test_owner();
            } else {
                c.lose_test_root();
            }
            let failure = if case == "owner-pin" {
                Failure::Identity
            } else {
                Failure::Wait
            };
            assert_eq!(c.poll(), Snapshot::Unresolved(failure));
            frame(&mut stream, "exit");
            assert_eq!(c.poll(), Snapshot::Unresolved(failure));
        }
        "drop" => {
            // Drop is not a stop or completion claim. The original remains
            // alive and can still answer its private framed channel.
            let root = c.root_id() as i32;
            drop(c);
            frame(&mut stream, "alive");
            assert_eq!(read_frame(&mut stream), "alive");
            frame(&mut stream, "exit");
            let mut status = 0;
            assert_eq!(unsafe { libc::waitpid(root, &mut status, 0) }, root);
            assert_eq!(
                unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 0, 0, 0, 0) },
                0
            );
            assert!(Custody::spawn(&mut command("--root", "root", dir), deadline()).is_err());
        }
        "owner-death" => std::process::exit(0),
        _ => unreachable!(),
    }
}

fn admission(case: &str, dir: &Path) {
    let mut until = deadline();
    let mut external = None;
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let mut thread = None;
    match case {
        "bad-policy" => unsafe {
            libc::signal(libc::SIGCHLD, libc::SIG_IGN);
        },
        "already-subreaper" => {
            assert_eq!(
                unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) },
                0
            );
        }
        "already-child" => {
            external = Some(
                command("--root", "external", dir)
                    .stdin(Stdio::piped())
                    .spawn()
                    .unwrap(),
            );
        }
        "threads" => {
            thread = Some(std::thread::spawn(move || {
                rx.recv().unwrap();
            }));
        }
        "expired" => until = Instant::now(),
        "long-budget" => until = Instant::now() + Duration::from_secs(31),
        _ => unreachable!(),
    }
    assert!(Custody::spawn(&mut command("--root", "root", dir), until).is_err());
    if let Some(thread) = thread {
        tx.send(()).unwrap();
        thread.join().unwrap();
    }
    if let Some(mut external) = external {
        drop(external.stdin.take());
        assert_eq!(external.wait().unwrap().code(), Some(17));
    }
}
