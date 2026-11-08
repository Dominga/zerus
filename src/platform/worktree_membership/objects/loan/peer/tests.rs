//! Native custody fixtures only. Self-reported Mac test tokens exercise kernel
//! correlation, not paired live-code proof or any stop/Force authority.
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
    #[cfg(target_os = "macos")]
    super::super::transport::send(
        &socket,
        &serde_json::to_vec(&own_audit().unwrap()).unwrap(),
        &[],
        until(),
    )
    .unwrap();
    let mut byte = [0];
    if std::io::stdin().read(&mut byte).unwrap() == 1 {
        if byte == *b"E" {
            panic!(
                "fixture exec refused: {}",
                Command::new("/bin/sleep").arg("300").exec()
            );
        }
        if byte == *b"B" {
            let mut size = [0; 4];
            std::io::stdin().read_exact(&mut size).unwrap();
            let size = u32::from_be_bytes(size) as usize;
            assert!(size <= zerus_client_core::worktree_programs::custody::FRAME_BYTES);
            let mut bytes = vec![0; size];
            std::io::stdin().read_exact(&mut bytes).unwrap();
            let intent: zerus_client_core::worktree_programs::invocation::Intent =
                serde_json::from_slice(&bytes).unwrap();
            let bound = until();
            let borrowed = super::super::borrower::Borrowed::acquire(&intent, bound).unwrap();
            assert!(borrowed.deadline() <= bound);
            assert_eq!(borrowed.scope().identity(), &intent.input.manifest.checkout);
            borrowed.current(&intent).unwrap();
            // The census lists the holder this helper borrows from (its
            // parent, the test process), not every process of this user on
            // the host (zerus-89pkd). The holder's exact root pin is then
            // still examined and excluded, never vacuously absent.
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            let roster = crate::platform::worktree_membership::test_roster::owned(&[
                std::os::unix::process::parent_id(),
            ]);
            assert!(
                borrowed.programs(&intent).unwrap().is_empty(),
                "the verified holder root pin is not an occupant"
            );
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            roster.assert_completed();
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            drop(roster);
            let mut changed = intent.clone();
            changed.pipeline = uuid::Uuid::new_v4().to_string();
            assert!(borrowed.current(&changed).is_err());
            super::super::transport::send(
                &socket,
                b"borrowed original native objects",
                &[],
                until(),
            )
            .unwrap();
            std::io::stdin().read_exact(&mut byte).unwrap();
            assert_eq!(byte, *b"R");
            borrowed.current(&intent).unwrap();
            return;
        }
        assert_eq!(byte, *b"L");
        let mut size = [0; 4];
        std::io::stdin().read_exact(&mut size).unwrap();
        let size = u32::from_be_bytes(size) as usize;
        assert!(size <= zerus_client_core::worktree_programs::MAX_BYTES);
        let mut bytes = vec![0; size];
        std::io::stdin().read_exact(&mut bytes).unwrap();
        let (manifest, binding) = serde_json::from_slice(&bytes).unwrap();
        let (description, fds) = super::super::transport::receive(&socket, until()).unwrap();
        let (original, deadline, _) =
            super::super::receive_transfer(&description, fds, &binding, &manifest, until())
                .unwrap();
        original.current(deadline).unwrap();
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
    pub(crate) audit: Option<[u32; 8]>,
    _dir: tempfile::TempDir,
    pub(crate) listener: UnixListener,
    pub(crate) path: std::path::PathBuf,
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
        #[cfg(target_os = "macos")]
        let audit = if program == "original" {
            let (bytes, fds) = super::super::transport::receive(&socket, until()).unwrap();
            assert!(fds.is_empty());
            serde_json::from_slice(&bytes).unwrap()
        } else {
            None
        };
        #[cfg(target_os = "linux")]
        let audit = None;
        Self {
            child,
            socket,
            audit,
            _dir: dir,
            listener,
            path,
        }
    }
    pub(crate) fn original(&self) -> Peer {
        let birth = crate::platform::process_identity(self.child.id())
            .unwrap()
            .birth;
        Peer::original(
            &self.socket,
            self.child.id(),
            &birth,
            self.audit.as_ref(),
            until(),
        )
        .unwrap()
    }
    pub(crate) fn expect_loan(
        &mut self,
        manifest: &zerus_client_core::worktree_programs::Manifest,
        binding: &zerus_client_core::worktree_programs::custody::Binding,
    ) {
        let bytes = serde_json::to_vec(&(manifest, binding)).unwrap();
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
#[cfg(target_os = "linux")]
fn the_native_socket_birth_and_exact_executing_image_are_all_required() {
    let fixture = Fixture::start("original");
    let peer = fixture.original();
    assert!(!peer.exited().unwrap());
    peer.current(&fixture.socket, until()).unwrap();
    assert!(Peer::pin(
        fixture.child.id(),
        &peer.original.birth,
        std::process::id() + 1,
        None,
        until()
    )
    .is_err());
    assert!(Peer::original(
        &fixture.socket,
        fixture.child.id(),
        "another birth",
        None,
        until()
    )
    .is_err());
    assert!(Peer::original(
        &fixture.socket,
        std::process::id(),
        &peer.original.birth,
        None,
        until()
    )
    .is_err());
    assert!(peer.current(&fixture.socket, Instant::now()).is_err());
    let other = Fixture::start("other");
    let birth = crate::platform::process_identity(other.child.id())
        .unwrap()
        .birth;
    assert!(Peer::original(&other.socket, other.child.id(), &birth, None, until()).is_err());
    assert!(peer.current(&other.socket, until()).is_err());
}

#[test]
#[cfg(target_os = "macos")]
fn a_missing_foreign_or_different_exec_generation_never_binds_the_original_socket() {
    let fixture = Fixture::start("original");
    let peer = fixture.original();
    peer.current(&fixture.socket, until()).unwrap();
    let birth = &peer.original.birth;
    assert!(Peer::pin(
        fixture.child.id(),
        birth,
        std::process::id() + 1,
        fixture.audit.as_ref(),
        until()
    )
    .is_err());
    assert!(Peer::pin(fixture.child.id(), birth, std::process::id(), None, until()).is_err());
    let other = Fixture::start("original");
    assert!(peer.current(&other.socket, until()).is_err());
    assert!(Peer::pin(
        fixture.child.id(),
        birth,
        std::process::id(),
        other.audit.as_ref(),
        until()
    )
    .is_err());
    let mut wrong = fixture.audit.unwrap();
    wrong[7] ^= 1; // public audit_token_t pidversion word; native BSM validates it
    assert!(Peer::pin(
        fixture.child.id(),
        birth,
        std::process::id(),
        Some(&wrong),
        until()
    )
    .is_err());
    assert!(!peer.exited().unwrap());
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
