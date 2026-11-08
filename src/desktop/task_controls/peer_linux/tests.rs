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
        if command == *b"x" {
            panic!(
                "exec failed: {}",
                Command::new("/bin/sleep").arg("20").exec()
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
    fixture.socket.write_all(b"x").unwrap();
    let deadline = until();
    while peer.current(deadline).is_ok() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    // Rejection came from the loaded-image/argv check, not a new birth/death.
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
    f.socket.write_all(b"x").unwrap();
    let deadline = until();
    while resident.current(deadline).is_ok() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(birth(&directory).unwrap(), original);
    assert!(f.child.try_wait().unwrap().is_none());
}
