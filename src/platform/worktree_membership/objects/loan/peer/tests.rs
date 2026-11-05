use super::*;
use std::{
    io::{Read, Write},
    os::unix::{net::UnixListener, process::CommandExt},
    process::{Child, Command, Stdio},
    time::Duration,
};

const FIXTURE: &str = "ZERUS_PROGRAM_LOAN_PEER_FIXTURE";
const TEST: &str =
    "platform::worktree_membership::objects::loan::peer::tests::original_helper_fixture";
fn until() -> Instant {
    Instant::now() + Duration::from_secs(3)
}

#[test]
fn original_helper_fixture() {
    let Some(path) = std::env::var_os(FIXTURE) else {
        return;
    };
    let socket = UnixStream::connect(path).unwrap();
    let mut byte = [0];
    if std::io::stdin().read(&mut byte).unwrap() == 1 {
        if byte == *b"E" {
            panic!(
                "fixture exec refused: {}",
                Command::new("/bin/sleep").arg("300").exec()
            );
        }
        assert_eq!(byte, *b"L");
        let mut size = [0; 4];
        std::io::stdin().read_exact(&mut size).unwrap();
        let size = u32::from_be_bytes(size) as usize;
        assert!(size <= zerus_client_core::worktree_programs::MAX_BYTES);
        let mut bytes = vec![0; size];
        std::io::stdin().read_exact(&mut bytes).unwrap();
        let manifest = serde_json::from_slice(&bytes).unwrap();
        let (description, fds) = super::super::transport::receive(&socket, until()).unwrap();
        let original = super::super::receive(&description, fds, &manifest, until()).unwrap();
        original.current(until()).unwrap();
        super::super::transport::send(&socket, b"original native objects received", &[], until())
            .unwrap();
        // Keep the actual received objects, not a path reconstruction, until
        // the original parent releases this synthetic read-only helper.
        std::io::stdin().read_exact(&mut byte).unwrap();
        assert_eq!(byte, *b"R");
        original.current(until()).unwrap();
    }
}
pub(crate) struct OwnedChild(Child);
impl std::ops::Deref for OwnedChild {
    type Target = Child;
    fn deref(&self) -> &Child {
        &self.0
    }
}
impl std::ops::DerefMut for OwnedChild {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.0
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
pub(crate) struct Fixture {
    pub(crate) child: OwnedChild,
    pub(crate) socket: UnixStream,
    _dir: tempfile::TempDir,
}
impl Fixture {
    pub(crate) fn start(program: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("peer");
        let listener = UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let mut command;
        if program == "original" {
            command = Command::new(std::env::current_exe().unwrap());
            command
                .args(["--exact", TEST, "--nocapture", "--test-threads=1"])
                .env(FIXTURE, &path);
        } else {
            command = Command::new("/usr/bin/python3");
            command.args(["-c", "import socket,sys; s=socket.socket(socket.AF_UNIX); s.connect(sys.argv[1]); sys.stdin.buffer.read()"])
                .arg(&path);
        }
        let mut child = OwnedChild(
            command
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap(),
        );
        let watchdog = Instant::now() + Duration::from_secs(15);
        let socket = loop {
            match listener.accept() {
                Ok((socket, _)) => break socket,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                Err(e) => panic!("fixture accept: {e}"),
            }
            assert!(
                child.try_wait().unwrap().is_none(),
                "helper exited before connecting"
            );
            if Instant::now() >= watchdog {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("fixture did not connect");
            }
            std::thread::sleep(Duration::from_millis(1));
        };
        Self {
            child,
            socket,
            _dir: dir,
        }
    }
    pub(crate) fn original(&self) -> Peer {
        let birth = crate::platform::process_identity(self.child.id())
            .unwrap()
            .birth;
        Peer::original(&self.socket, self.child.id(), &birth, until()).unwrap()
    }
    pub(crate) fn expect_loan(
        &mut self,
        manifest: &zerus_client_core::worktree_programs::Manifest,
    ) {
        let bytes = serde_json::to_vec(manifest).unwrap();
        let input = self.child.stdin.as_mut().unwrap();
        input.write_all(b"L").unwrap();
        input
            .write_all(&(bytes.len() as u32).to_be_bytes())
            .unwrap();
        input.write_all(&bytes).unwrap();
        input.flush().unwrap();
    }
    pub(crate) fn release_loan(&mut self) {
        let (bytes, fds) = super::super::transport::receive(&self.socket, until()).unwrap();
        assert_eq!(bytes, b"original native objects received");
        assert!(fds.is_empty());
        self.child.stdin.as_mut().unwrap().write_all(b"R").unwrap();
        drop(self.child.stdin.take());
        assert!(self.child.wait().unwrap().success());
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        drop(self.child.stdin.take());
    }
}

#[test]
fn the_native_socket_birth_and_exact_executing_image_are_all_required() {
    let fixture = Fixture::start("original");
    let peer = fixture.original();
    assert!(!peer.exited().unwrap());
    peer.current(&fixture.socket, until()).unwrap();
    assert!(Peer::original(
        &fixture.socket,
        fixture.child.id(),
        "another birth",
        until()
    )
    .is_err());
    assert!(Peer::original(
        &fixture.socket,
        std::process::id(),
        &peer.original.birth,
        until()
    )
    .is_err());
    assert!(peer.current(&fixture.socket, Instant::now()).is_err());
    let other = Fixture::start("other");
    let birth = crate::platform::process_identity(other.child.id())
        .unwrap()
        .birth;
    assert!(Peer::original(&other.socket, other.child.id(), &birth, until()).is_err());
    assert!(peer.current(&other.socket, until()).is_err());
}

#[test]
fn an_exec_or_closed_socket_is_not_a_retired_original_process() {
    let mut fixture = Fixture::start("original");
    let peer = fixture.original();
    fixture
        .child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"E")
        .unwrap();
    // The socket's original credentials survive exec. Current executing-object
    // evidence must change, and this still does NOT establish physical exit.
    let watchdog = Instant::now() + Duration::from_secs(5);
    while peer.current(&fixture.socket, until()).is_ok() {
        assert!(
            Instant::now() < watchdog,
            "fixture never executed its replacement"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(!peer.exited().unwrap());
    drop(std::mem::replace(
        &mut fixture.socket,
        UnixStream::pair().unwrap().0,
    ));
    assert!(!peer.exited().unwrap());
    fixture.child.kill().unwrap();
    fixture.child.wait().unwrap();
    assert!(peer.exited().unwrap());
}
