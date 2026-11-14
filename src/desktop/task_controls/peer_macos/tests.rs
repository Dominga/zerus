use super::*;
use std::{
    io::{Read, Write},
    os::unix::{net::UnixListener, process::CommandExt},
    process::{Child, Command, Stdio},
    time::Duration,
};

const ROLE: &str = "ZERUS_TEST_MAC_TASK_PEER";
const HELPER: &str = "desktop::task_controls::peer_macos::tests::private_peer_helper";
const ARGUMENTS: [&[u8]; 5] = [
    b"--exact",
    HELPER.as_bytes(),
    b"--ignored",
    b"--nocapture",
    b"--test-threads=1",
];

fn until() -> Instant {
    Instant::now() + Duration::from_secs(10)
}
fn program() -> PathBuf {
    std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap()
}
fn start(pid: u32) -> String {
    crate::platform::process_start_time(pid).unwrap()
}
fn status(token: Audit) -> u32 {
    let mut value = 0_u32;
    assert_eq!(
        unsafe {
            csops_audittoken(
                validate_token(token).unwrap() as i32,
                0,
                (&mut value as *mut u32).cast(),
                std::mem::size_of_val(&value),
                &token,
            )
        },
        0
    );
    value
}
fn hash_text(hash: [u8; 20]) -> String {
    hash.iter().map(|b| format!("{b:02x}")).collect()
}

struct Fixture {
    socket: UnixStream,
    child: Child,
    _dir: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        Self::image(&program())
    }
    fn image(image: &Path) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("peer.sock");
        let listener = UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let mut command = Command::new(image);
        command
            .args([
                "--exact",
                HELPER,
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(ROLE, &path)
            .env("HGS_SESSION", "fabricated/environment/label")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        let child = command.spawn().unwrap();
        let mut fixture = Self {
            socket: UnixStream::pair().unwrap().0,
            child,
            _dir: dir,
        };
        let deadline = until();
        fixture.socket = loop {
            match listener.accept() {
                Ok((socket, _)) => break socket,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    assert!(
                        fixture.child.try_wait().unwrap().is_none(),
                        "private peer exited"
                    );
                    assert!(Instant::now() < deadline, "private peer did not connect");
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("private peer accept: {error}"),
            }
        };
        // Darwin accept can retain the listener's nonblocking flag. The
        // fixture's acknowledgement reads deliberately use blocking timeouts.
        // SAFETY: read-only flags query on this owned connected socket.
        let flags = unsafe { libc::fcntl(fixture.socket.as_raw_fd(), libc::F_GETFL) };
        assert!(flags >= 0);
        eprintln!(
            "TASK_PEER_SOCKET accepted_nonblocking={}",
            flags & libc::O_NONBLOCK != 0
        );
        fixture.socket.set_nonblocking(false).unwrap();
        fixture
            .socket
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        fixture
            .socket
            .set_write_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut ready = [0];
        fixture.socket.read_exact(&mut ready).unwrap();
        assert_eq!(ready, [b'r']);
        fixture
    }
    fn capture(&self) -> Peer {
        Peer::capture(&self.socket, &program(), until()).unwrap()
    }
    fn agent(&mut self) -> u32 {
        self.socket.write_all(b"g").unwrap();
        let mut pid = [0; 4];
        self.socket.read_exact(&mut pid).unwrap();
        let pid = u32::from_be_bytes(pid);
        assert_eq!(
            Process::query(pid, until()).unwrap().parent,
            self.child.id()
        );
        pid
    }
    fn retire(&mut self) {
        self.socket.write_all(b"q").unwrap();
        let deadline = until();
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success());
                return;
            }
            assert!(Instant::now() < deadline, "private child did not exit");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    fn exec_cat(&mut self) {
        const WITNESS: &[u8] = b"replacement-cat-ready";
        self.socket.write_all(b"x").unwrap();
        self.socket.write_all(WITNESS).unwrap();
        let mut echoed = [0; WITNESS.len()];
        self.socket.read_exact(&mut echoed).unwrap();
        assert_eq!(echoed, WITNESS);
        assert_eq!(
            image_path(self.child.id()).unwrap(),
            std::fs::canonicalize("/bin/cat").unwrap()
        );
        assert!(self.child.try_wait().unwrap().is_none());
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // Only this original, unreaped private child is signalled.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
#[ignore = "owned socket fixture entry point, never a qualification skip"]
fn private_peer_helper() {
    let path = std::env::var_os(ROLE).expect("private fixture socket");
    let mut socket = UnixStream::connect(path).unwrap();
    socket.write_all(b"r").unwrap();
    let mut command = [0];
    socket.read_exact(&mut command).unwrap();
    if command == *b"g" {
        let mut agent = Command::new("/bin/cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        socket.write_all(&agent.id().to_be_bytes()).unwrap();
        socket.read_exact(&mut command).unwrap();
        let _ = agent.kill();
        agent.wait().unwrap();
        assert_eq!(command, *b"q");
        return;
    }
    if command == *b"x" {
        panic!(
            "private cat exec failed: {}",
            Command::new("/bin/cat")
                .stdin(Stdio::from(std::os::fd::OwnedFd::from(
                    socket.try_clone().unwrap()
                )))
                .stdout(Stdio::from(std::os::fd::OwnedFd::from(socket)))
                .exec()
        );
    }
    assert_eq!(command, *b"q");
}

#[test]
fn the_kernel_socket_token_loaded_hash_command_and_ancestor_are_required() {
    let mut f = Fixture::new();
    let token = audit(&f.socket).unwrap();
    assert_eq!(validate_token(token).unwrap(), f.child.id());
    let own = own_audit().unwrap();
    assert_eq!(loaded(token).unwrap(), loaded(own).unwrap());
    assert_ne!(status(token) & 1, 0);
    eprintln!(
        "TASK_PEER_NATIVE status={:#x} cdhash={}",
        status(own),
        hash_text(loaded(own).unwrap())
    );
    let mut peer = f.capture();
    peer.command(&ARGUMENTS, until()).unwrap();
    peer.executor(std::process::id(), &start(std::process::id()), until())
        .unwrap();
    peer.arguments(until()).unwrap();
    peer.current(until()).unwrap();
    assert!(peer
        .command(&[b"task", b"control", b"execute", b"--json"], until())
        .is_err());
    assert!(Peer::capture(&f.socket, Path::new("/bin/sh"), until()).is_err());
    f.retire();
    assert!(peer.current(until()).is_err());
}

#[test]
fn another_real_peer_and_a_stale_audit_pidversion_cannot_substitute() {
    let mut first = Fixture::new();
    let mut second = Fixture::new();
    let mut peer = first.capture();
    assert_ne!(peer.process.pid, second.child.id());
    assert!(peer
        .executor(second.child.id(), &start(second.child.id()), until())
        .is_err());
    let mut stale = audit(&first.socket).unwrap();
    stale.words[7] = stale.words[7].wrapping_add(1).max(1);
    assert_ne!(stale.words[7], audit(&first.socket).unwrap().words[7]);
    assert!(
        loaded(stale).is_err(),
        "kernel must reject a stale process/exec token"
    );
    first.retire();
    second.retire();
}

#[test]
fn a_reused_pid_with_a_different_native_birth_cannot_keep_the_proof() {
    let mut f = Fixture::new();
    let mut peer = f.capture();
    // Keep the genuine live PID/token and change only the saved generation:
    // deterministic stale-pin controls without forcing system-wide PID churn.
    peer.process.start.1 ^= 1;
    assert!(peer.current(until()).is_err());
    peer.process = Process::query(f.child.id(), until()).unwrap();
    peer.process.unique ^= 1;
    assert!(peer.current(until()).is_err());
    let mut peer = f.capture();
    assert!(peer
        .executor(std::process::id(), "wrong-birth", until())
        .is_err());
    f.retire();
}

#[test]
fn a_same_birth_exec_loses_the_original_peer_and_resident_proofs() {
    let mut f = Fixture::new();
    let peer = f.capture();
    let supervisor = Process::query(std::process::id(), until()).unwrap();
    let resident = Resident::capture(
        f.child.id(),
        &start(f.child.id()),
        supervisor.pid,
        &start(supervisor.pid),
        &supervisor.birth(),
        &program(),
        until(),
    )
    .unwrap();
    resident.current(until()).unwrap();
    resident.arguments(until()).unwrap();
    let original = peer.process.start;
    f.exec_cat();
    assert_eq!(
        Process::query(f.child.id(), until()).unwrap().start,
        original
    );
    assert!(peer.current(until()).is_err());
    assert!(resident.current(until()).is_err());
}

#[test]
fn a_different_adhoc_code_directory_is_not_the_services_loaded_image() {
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("other-hgs");
    std::fs::copy(program(), &image).unwrap();
    let signed = Command::new("/usr/bin/codesign")
        .args([
            "--force",
            "--sign",
            "-",
            "--identifier",
            "org.example.zerus.task-peer-other",
        ])
        .arg(&image)
        .output()
        .unwrap();
    assert!(
        signed.status.success(),
        "{}",
        String::from_utf8_lossy(&signed.stderr)
    );
    let identity = Command::new("/usr/bin/codesign")
        .args(["--display", "--verbose=4"])
        .arg(&image)
        .output()
        .unwrap();
    assert!(identity.status.success());
    let identity = String::from_utf8(identity.stderr).unwrap();
    assert!(identity.lines().any(|line| line == "Signature=adhoc"));
    assert!(identity
        .lines()
        .any(|line| line == "Identifier=org.example.zerus.task-peer-other"));
    let mut f = Fixture::image(&image);
    let token = audit(&f.socket).unwrap();
    assert_ne!(
        status(token) & 1,
        0,
        "the negative helper really executes valid ad-hoc code"
    );
    // Signing scheme is fixture metadata, not a runtime CS_OPS_STATUS bit or
    // authority fallback. The production proof still reads kernel status/hash.
    eprintln!(
        "TASK_PEER_OTHER signature=adhoc status={:#x} cdhash={}",
        status(token),
        hash_text(loaded(token).unwrap())
    );
    assert_ne!(
        loaded(token).unwrap(),
        loaded(own_audit().unwrap()).unwrap()
    );
    assert_eq!(
        image_path(f.child.id()).unwrap(),
        std::fs::canonicalize(&image).unwrap()
    );
    assert!(
        Peer::capture(&f.socket, &image, until()).is_err(),
        "a matching path cannot replace the loaded CodeDirectory binding"
    );
    let agent = f.agent();
    let supervisor = Process::query(f.child.id(), until()).unwrap();
    assert!(
        Resident::capture(
            agent,
            &start(agent),
            supervisor.pid,
            &start(supervisor.pid),
            &supervisor.birth(),
            &image,
            until(),
        )
        .is_err(),
        "a real parent-child relationship cannot replace trusted supervisor code"
    );
    f.retire();
}

#[test]
fn a_separate_resident_supervisor_and_its_actual_child_have_native_proofs() {
    let mut f = Fixture::new();
    let agent = f.agent();
    let supervisor = Process::query(f.child.id(), until()).unwrap();
    let resident = Resident::capture(
        agent,
        &start(agent),
        supervisor.pid,
        &start(supervisor.pid),
        &supervisor.birth(),
        &program(),
        until(),
    )
    .unwrap();
    resident.current(until()).unwrap();
    assert!(Resident::capture(
        agent,
        &start(agent),
        supervisor.pid,
        &start(supervisor.pid),
        "wrong-birth",
        &program(),
        until(),
    )
    .is_err());
    f.retire();
    assert!(resident.current(until()).is_err());
}

#[test]
fn an_unsigned_private_helper_never_authenticates() {
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("unsigned-hgs");
    std::fs::copy(program(), &image).unwrap();
    let removed = Command::new("/usr/bin/codesign")
        .arg("--remove-signature")
        .arg(&image)
        .output()
        .unwrap();
    assert!(removed.status.success());
    assert!(!Command::new("/usr/bin/codesign")
        .args(["--verify", "--strict"])
        .arg(&image)
        .output()
        .unwrap()
        .status
        .success());
    let path = dir.path().join("peer.sock");
    let listener = UnixListener::bind(&path).unwrap();
    listener.set_nonblocking(true).unwrap();
    let child = Command::new(&image)
        .args([
            "--exact",
            HELPER,
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(ROLE, &path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let child = match child {
        Ok(child) => child,
        Err(error) => {
            assert!(matches!(
                error.raw_os_error(),
                Some(libc::EPERM | libc::EACCES | libc::ENOEXEC)
            ));
            eprintln!(
                "TASK_PEER_UNSIGNED kernel refused exec: {:?}",
                error.raw_os_error()
            );
            return;
        }
    };
    let mut f = Fixture {
        socket: UnixStream::pair().unwrap().0,
        child,
        _dir: dir,
    };
    let deadline = until();
    loop {
        match listener.accept() {
            Ok((socket, _)) => {
                assert!(Peer::capture(&socket, &image, until()).is_err());
                eprintln!("TASK_PEER_UNSIGNED native proof refused executed unsigned helper");
                return;
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if let Some(status) = f.child.try_wait().unwrap() {
                    assert!(!status.success());
                    eprintln!("TASK_PEER_UNSIGNED kernel stopped unsigned helper: {status}");
                    return;
                }
                assert!(Instant::now() < deadline, "unsigned helper never settled");
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("unsigned helper socket: {error}"),
        }
    }
}

#[test]
fn expired_native_proofs_and_wrong_resident_births_refuse() {
    let mut f = Fixture::new();
    assert!(Peer::capture(&f.socket, &program(), Instant::now()).is_err());
    let peer = f.capture();
    assert!(peer.current(Instant::now()).is_err());
    let p = Process::query(std::process::id(), until()).unwrap();
    let agent_start = start(f.child.id());
    for (agent, supervisor, birth) in [
        ("wrong", start(p.pid), p.birth()),
        (agent_start.as_str(), "wrong".into(), p.birth()),
        (agent_start.as_str(), start(p.pid), "wrong".into()),
    ] {
        assert!(Resident::capture(
            f.child.id(),
            agent,
            p.pid,
            &supervisor,
            &birth,
            &program(),
            until()
        )
        .is_err());
    }
    f.retire();
}
