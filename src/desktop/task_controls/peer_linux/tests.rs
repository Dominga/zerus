use super::*;
use std::{
    io::{Read, Write},
    os::unix::{net::UnixListener, process::CommandExt},
    process::{Child, Command, Stdio},
    time::Duration,
};

const ROLE: &str = "ZERUS_TEST_TASK_CONTROL_PEER";
const TEST:&str="desktop::task_controls::peer_linux::tests::the_original_socket_peer_proves_its_actual_native_ancestor";
fn until() -> Instant {
    Instant::now() + Duration::from_secs(2)
}
fn trusted() -> File {
    File::open("/proc/self/exe").unwrap()
}
fn own_birth() -> String {
    birth(&File::open("/proc/self").unwrap())
        .unwrap()
        .ticks
        .to_string()
}
fn own_start() -> String {
    crate::platform::process_start_time(std::process::id()).unwrap()
}
struct Fixture {
    socket: UnixStream,
    child: Child,
    _dir: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        crate::platform::restrict_dir(dir.path()).unwrap();
        let path = dir.path().join("peer.sock");
        let listener = UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        // Exec the retained original test image even if another build replaces
        // the shared Cargo target path. No agent, service or owner GUI starts.
        let mut child = Command::new("/proc/self/exe")
            .args(["--exact", TEST, "--nocapture", "--test-threads=1"])
            .env(ROLE, &path)
            .env("HGS_SESSION", "fabricated/environment/label")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = until();
        let socket = loop {
            match listener.accept() {
                Ok((s, _)) => break s,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock && Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(2))
                }
                Err(e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!("private peer did not connect: {e}");
                }
            }
        };
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut ready = [0];
        (&socket).read_exact(&mut ready).unwrap();
        assert_eq!(ready, [b'r']);
        Self {
            socket,
            child,
            _dir: dir,
        }
    }
    fn exec_and_wait(&mut self, rejected: impl Fn(Instant) -> bool, deadline: Instant) {
        // A first refusal can be the non-leader exec transition (even Z), not
        // the new image. Only the replacement cat can echo these queued bytes.
        const WITNESS: &[u8] = b"replacement-ready";
        self.socket.write_all(b"x").unwrap();
        self.socket.write_all(WITNESS).unwrap();
        self.socket
            .set_read_timeout(Some(
                deadline.checked_duration_since(Instant::now()).unwrap(),
            ))
            .unwrap();
        let mut echoed = [0; WITNESS.len()];
        let received = self.socket.read_exact(&mut echoed);
        assert!(
            received.is_ok(),
            "replacement acknowledgement: {received:?}, child={:?}",
            self.child.try_wait()
        );
        assert_eq!(echoed, WITNESS);
        assert!(Instant::now() < deadline, "original exec deadline");
        assert!(
            rejected(deadline),
            "the replacement image must lose the original proof"
        );
    }
    fn pause(&mut self) {
        self.socket.write_all(b"s").unwrap();
        let deadline = until();
        loop {
            let mut status = 0;
            // SAFETY: our unreaped private child only; observe its fixture stop.
            let pid = unsafe {
                libc::waitpid(
                    self.child.id() as i32,
                    &mut status,
                    libc::WUNTRACED | libc::WNOHANG,
                )
            };
            if pid == self.child.id() as i32 {
                assert!(libc::WIFSTOPPED(status));
                return;
            }
            assert_eq!(pid, 0);
            assert!(Instant::now() < deadline, "private child did not pause");
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    fn stop(&mut self) {
        self.socket.write_all(b"q").unwrap();
        let deadline = until();
        loop {
            if self.child.try_wait().unwrap().is_some() {
                return;
            }
            assert!(Instant::now() < deadline, "private child failed to retire");
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn the_original_socket_peer_proves_its_actual_native_ancestor() {
    if let Some(path) = std::env::var_os(ROLE) {
        let mut socket = UnixStream::connect(path).unwrap();
        socket.write_all(b"r").unwrap();
        let mut command = [0];
        socket.read_exact(&mut command).unwrap();
        if command == *b"s" {
            // SAFETY: stop this private helper, never another process.
            assert_eq!(unsafe { libc::raise(libc::SIGSTOP) }, 0);
            socket.read_exact(&mut command).unwrap();
        }
        if command == *b"x" {
            panic!(
                "exec failed: {}",
                Command::new("/bin/cat")
                    .stdin(Stdio::from(std::os::fd::OwnedFd::from(
                        socket.try_clone().unwrap()
                    )))
                    .stdout(Stdio::from(std::os::fd::OwnedFd::from(socket)))
                    .exec()
            );
        }
        return;
    }
    let mut fixture = Fixture::new();
    let mut peer = Peer::capture(&fixture.socket, trusted(), until()).unwrap();
    peer.command(
        &[
            b"--exact",
            TEST.as_bytes(),
            b"--nocapture",
            b"--test-threads=1",
        ],
        until(),
    )
    .unwrap();
    peer.executor(std::process::id(), &own_start(), until())
        .unwrap();
    peer.current(until()).unwrap();
    assert!(peer
        .command(&[b"task", b"control", b"execute", b"--json"], until())
        .is_err());
    fixture.stop();
    assert!(peer.current(until()).is_err());
}

#[test]
fn a_fabricated_ancestor_birth_or_trusted_image_never_authenticates() {
    let mut fixture = Fixture::new();
    assert!(Peer::capture(&fixture.socket, File::open("/bin/true").unwrap(), until()).is_err());
    let mut peer = Peer::capture(&fixture.socket, trusted(), until()).unwrap();
    assert!(peer.executor(std::process::id(), "1", until()).is_err());
    fixture.stop();
}

#[test]
fn a_same_birth_exec_cannot_keep_the_loaded_cli_proof() {
    let mut fixture = Fixture::new();
    let mut peer = Peer::capture(&fixture.socket, trusted(), until()).unwrap();
    peer.executor(std::process::id(), &own_start(), until())
        .unwrap();
    let original = peer.process.birth.clone();
    fixture.exec_and_wait(|deadline| peer.current(deadline).is_err(), until());
    // Rejection came from the loaded-image/argv check, not a new birth/death.
    assert_eq!(birth(&peer.process.directory).unwrap(), original);
    alive(&peer.process.handle).unwrap();
    assert!(fixture.child.try_wait().unwrap().is_none());
}

#[test]
fn an_early_refusal_does_not_complete_the_exec_fixture() {
    let mut fixture = Fixture::new();
    let peer = Peer::capture(&fixture.socket, trusted(), until()).unwrap();
    let original = peer.process.birth.clone();
    fixture.pause();
    assert!(
        peer.current(until()).is_err(),
        "a stopped original peer is refused"
    );
    alive(&peer.process.handle).unwrap();
    let deadline = until();
    std::thread::scope(|scope| {
        let (completed, receipt) = std::sync::mpsc::channel();
        let fixture = &mut fixture;
        let peer = &peer;
        let worker = scope.spawn(move || {
            fixture.exec_and_wait(|deadline| peer.current(deadline).is_err(), deadline);
            completed.send(()).unwrap();
        });
        let early = receipt.recv_timeout(Duration::from_millis(20));
        // SAFETY: the original private child's retained pidfd, still unreaped.
        let resumed = unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                peer.process.handle.as_raw_fd(),
                libc::SIGCONT,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            )
        };
        assert_eq!(resumed, 0);
        worker.join().unwrap();
        assert_eq!(
            early,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout),
            "refusal while stopped cannot stand in for the replacement image's acknowledgement"
        );
    });
    assert_eq!(birth(&peer.process.directory).unwrap(), original);
    alive(&peer.process.handle).unwrap();
    assert!(fixture.child.try_wait().unwrap().is_none());
}

#[test]
fn proof_parsing_and_ancestry_obey_the_original_deadline() {
    let mut fixture = Fixture::new();
    assert!(Peer::capture(&fixture.socket, trusted(), Instant::now()).is_err());
    let mut peer = Peer::capture(&fixture.socket, trusted(), until()).unwrap();
    assert!(peer
        .executor(std::process::id(), &own_start(), Instant::now())
        .is_err());
    fixture.stop();
}

#[test]
fn the_loaded_resident_supervisor_and_its_agent_stay_pinned_independently() {
    let mut f = Fixture::new();
    let directory = File::open(format!("/proc/{}", f.child.id())).unwrap();
    let original = birth(&directory).unwrap();
    let agent_start = crate::platform::process_start_time(f.child.id()).unwrap();
    let supervisor_start = own_start();
    let precise_birth = own_birth();
    // Actual legacy run-record format and the precise native key are separate.
    assert!(!agent_start.bytes().all(|b| b.is_ascii_digit()));
    let capture = |a: &str, s: &str, b: &str, image: File| {
        Resident::capture(f.child.id(), a, std::process::id(), s, b, image, until())
    };
    let resident = capture(&agent_start, &supervisor_start, &precise_birth, trusted()).unwrap();
    resident.current(until()).unwrap();
    assert!(capture("invalid", &supervisor_start, &precise_birth, trusted()).is_err());
    assert!(capture(&agent_start, "invalid", &precise_birth, trusted()).is_err());
    assert!(capture(&agent_start, &supervisor_start, "1", trusted()).is_err());
    assert!(capture(
        &agent_start,
        &supervisor_start,
        &supervisor_start,
        trusted()
    )
    .is_err());
    assert!(capture(
        &agent_start,
        &supervisor_start,
        &precise_birth,
        File::open("/bin/true").unwrap()
    )
    .is_err());
    assert!(Resident::capture(
        f.child.id(),
        &agent_start,
        f.child.id(),
        &agent_start,
        &original.ticks.to_string(),
        trusted(),
        until()
    )
    .is_err());
    f.exec_and_wait(|deadline| resident.current(deadline).is_err(), until());
    assert_eq!(birth(&directory).unwrap(), original);
    assert!(f.child.try_wait().unwrap().is_none());
}

#[test]
fn a_cli_image_guard_is_shared_by_fresh_invocations_but_never_survives_peer_death() {
    let mut first = Fixture::new();
    let mut second = Fixture::new();
    let peer = Peer::capture(&first.socket, trusted(), until()).unwrap();
    let other = Peer::capture(&second.socket, trusted(), until()).unwrap();
    assert_ne!(first.child.id(), second.child.id());
    assert_eq!(
        peer.image_guard(until()).unwrap(),
        other.image_guard(until()).unwrap()
    );
    first.stop();
    assert!(peer.image_guard(until()).is_err());
    assert!(other.image_guard(until()).is_ok());
    second.stop();
}
