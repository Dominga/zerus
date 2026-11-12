use super::super::peer::tests::Fixture;
use super::*;
use zerus_client_core::{
    termination_cleanup::Capture,
    worktree_programs::{stop::Kind, DirectoryId, ABI},
};

fn until() -> Instant {
    Instant::now() + Duration::from_secs(3)
}
fn uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn input(root: &std::path::Path, checkout: DirectoryId) -> Input {
    let run = uuid();
    let conversation = uuid();
    let termination = uuid();
    Input { request_id: uuid(), confirmation_id: uuid(), archive_id: uuid(), stop: Kind::Normal,
        source:zerus_client_core::worktree_programs::stop::Source::Service {epoch:uuid()},
        original: serde_json::json!({"request_id":termination,"action":"terminate","expected_run_id":run,"expected_conversation_id":conversation}).to_string(),
        manifest: Manifest { abi: ABI.into(), id: uuid(), capture: Capture {
            target: zerus_client_core::Target { computer: String::new(), name: "codex/example/original".into(), run_id: run, conversation_id: conversation },
            machine: uuid(), root: root.to_str().unwrap().into(), common_dir: root.join("common").to_str().unwrap().into(), owner_id: uuid() },
            configuration: "a".repeat(64), boot: uuid(), checkout, issued: 1000, expires: 1300, programs: vec![] }
    }
}
fn holder() -> (tempfile::TempDir, Holder, Input, Vec<u8>) {
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let scope = Scope::open(&root, until()).unwrap();
    let input = input(&root, scope.identity().clone());
    assert!(input.valid());
    let pipeline = uuid();
    let request = Request::for_input(&pipeline, &input).bytes().unwrap();
    let holder = Holder::new(scope, input.manifest.clone(), pipeline, 1000).unwrap();
    (dir, holder, input, request)
}

#[test]
fn the_readonly_holder_lives_across_a_physically_retired_phase_but_never_reloans_it() {
    let (_dir, mut holder, input, request) = holder();
    let mut fixture = Fixture::start("original");
    holder
        .arm(&input, fixture.original(), 1000, until())
        .unwrap();
    assert!(!holder.settled());
    assert!(
        holder.observe(1001, until()).is_err(),
        "an active original mutator still owns its phase"
    );
    assert!(
        holder.joined(&request).is_err(),
        "an owner ACK cannot retire a live helper"
    );
    fixture.expect_loan(&input.manifest, &Request::decode(&request).unwrap());
    holder.lend(&fixture.socket, &request).unwrap();
    assert!(holder.lend(&fixture.socket, &request).is_err());
    assert!(holder.joined(&request).is_err());
    fixture.release_loan();
    holder.joined(&request).unwrap();
    assert!(holder.settled());
    // The census lists this test's own helper, not every process of this
    // user on the host, any of which can be caught while it exits
    // (zerus-89pkd). The assertions concern only the manifest's custody.
    let next = Fixture::start("original");
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let roster = crate::platform::worktree_membership::test_roster::owned(&[next.child.id()]);
    let observed = holder.observe(1001, until()).unwrap();
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    roster.assert_completed();
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    drop(roster);
    assert_eq!(observed.capture, input.manifest.capture);
    assert_eq!(observed.checkout, input.manifest.checkout);
    assert_eq!(observed.expires, input.manifest.expires);
    assert_ne!(observed.id, input.manifest.id);
    assert!(
        holder.arm(&input, next.original(), 1000, until()).is_err(),
        "retired request never borrows again"
    );
    // A separately confirmed follow-up has a different original request and
    // immutable manifest binding. This is only a loan, never Force consent.
    let mut fresh = input.clone();
    fresh.request_id = uuid();
    fresh.confirmation_id = uuid();
    fresh.manifest.id = uuid();
    fresh.stop = Kind::Force {
        previous_request_id: input.request_id.clone(),
        previous_binding: input.binding(),
    };
    let mut changed = fresh.clone();
    changed.source = zerus_client_core::worktree_programs::stop::Source::Service { epoch: uuid() };
    assert!(
        holder
            .arm(&changed, next.original(), 1000, until())
            .is_err(),
        "a fresh request cannot replace the original source"
    );
    holder.arm(&fresh, next.original(), 1000, until()).unwrap();
    assert!(!holder.settled());
    assert!(holder.lend(&next.socket, &request).is_err());
    let fresh_request = Request::for_input(&holder.pipeline, &fresh)
        .bytes()
        .unwrap();
    holder.lend(&next.socket, &fresh_request).unwrap();
    assert!(holder.lend(&next.socket, &fresh_request).is_err());
}

#[test]
fn every_binding_the_native_peer_and_the_original_root_are_independent() {
    let (_dir, mut holder, input, request) = holder();
    let original = Fixture::start("original");
    let impostor = Fixture::start("original"); // SAME image/user, different original child
    holder
        .arm(&input, original.original(), 1000, until())
        .unwrap();
    assert!(holder.lend(&impostor.socket, &request).is_err());
    for field in ["pipeline", "manifest", "request", "input"] {
        let mut changed: serde_json::Value = serde_json::from_slice(&request).unwrap();
        changed[field] = if matches!(field, "pipeline" | "request") {
            uuid()
        } else {
            "f".repeat(64)
        }
        .into();
        assert!(
            Request::decode(&serde_json::to_vec(&changed).unwrap()).is_ok(),
            "{field}: this is a valid foreign binding, not a shape refusal"
        );
        assert!(
            holder
                .lend(&original.socket, &serde_json::to_vec(&changed).unwrap())
                .is_err(),
            "{field}"
        );
    }
    let mut unknown: serde_json::Value = serde_json::from_slice(&request).unwrap();
    unknown["force"] = true.into();
    assert!(holder
        .lend(&original.socket, &serde_json::to_vec(&unknown).unwrap())
        .is_err());
    assert!(holder.lend(&original.socket, &vec![0; 4096 + 1]).is_err());
    // None of these failed admissions authorizes an effect or drops the live
    // original's custody. The exact original can still take its FIRST loan.
    holder.lend(&original.socket, &request).unwrap();
    assert!(!holder.settled());
    assert!(holder
        .arm(&input, impostor.original(), 1000, until())
        .is_err());
}

#[test]
fn final_cleanup_loans_require_the_exact_retired_stop_phase_and_a_new_binding() {
    let (_dir, mut holder, input, request) = holder();
    let cleanup = zerus_client_core::worktree_programs::cleanup::Input {
        request_id: uuid(),
        stop: Box::new(input.clone()),
    };
    let mut original = Fixture::start("original");
    let mut final_child = Fixture::start("original");
    assert!(
        holder
            .arm_cleanup(&cleanup, final_child.original(), 1000, until())
            .is_err(),
        "a new holder never adopts a stop phase from a stored input"
    );
    holder
        .arm(&input, original.original(), 1000, until())
        .unwrap();
    assert!(
        holder
            .arm_cleanup(&cleanup, final_child.original(), 1000, until())
            .is_err(),
        "neither an active helper nor its unretired loan is omitted"
    );
    original.expect_loan(&input.manifest, &Request::decode(&request).unwrap());
    holder.lend(&original.socket, &request).unwrap();
    assert!(holder.joined(&request).is_err());
    original.release_loan();
    holder.joined(&request).unwrap();
    let binding = Request::for_cleanup(&holder.pipeline, &cleanup);
    holder
        .arm_cleanup(&cleanup, final_child.original(), 1000, until())
        .unwrap();
    assert!(
        holder.lend(&final_child.socket, &request).is_err(),
        "retired stop binding cannot borrow the removal phase"
    );
    final_child.expect_loan(&input.manifest, &binding);
    holder
        .lend(&final_child.socket, &binding.bytes().unwrap())
        .unwrap();
    assert!(holder
        .lend(&final_child.socket, &binding.bytes().unwrap())
        .is_err());
    assert!(holder.joined(&binding.bytes().unwrap()).is_err());
    final_child.release_loan();
    holder.joined(&binding.bytes().unwrap()).unwrap();
    assert!(
        holder
            .arm_cleanup(
                &cleanup,
                Fixture::start("original").original(),
                1000,
                until()
            )
            .is_err(),
        "a retired removal phase can never borrow again"
    );
    let mut another = cleanup.clone();
    another.request_id = uuid();
    assert!(
        holder
            .arm_cleanup(
                &another,
                Fixture::start("original").original(),
                1000,
                until()
            )
            .is_err(),
        "a NEW UUID does not permit another removal loan"
    );
    assert!(
        holder.observe(1001, until()).is_err(),
        "status must not reopen a terminal removal phase"
    );
    // These are native custody controls only. No Complete stop receipt,
    // Remove continuation, cleanup proof or deletion is created by this test.
}

#[test]
fn cancellation_expiry_and_failed_transfer_keep_the_old_mutation_charged() {
    for expired in [false, true] {
        let (_dir, mut holder, input, request) = holder();
        let mut fixture = Fixture::start("original");
        holder
            .arm(&input, fixture.original(), 1000, until())
            .unwrap();
        if expired {
            holder.until = Instant::now();
        } else {
            holder.close();
        }
        assert!(holder.lend(&fixture.socket, &request).is_err());
        assert!(holder.observe(1001, until()).is_err());
        assert!(!holder.settled());
        assert!(holder.joined(&request).is_err());
        fixture.child.kill().unwrap();
        fixture.child.wait().unwrap();
        holder.joined(&request).unwrap();
        assert!(holder.settled());
        let next = Fixture::start("original");
        let mut fresh = input.clone();
        fresh.request_id = uuid();
        assert!(holder.arm(&fresh, next.original(), 1000, until()).is_err());
    }
    let (_dir, mut holder, input, request) = holder();
    let mut fixture = Fixture::start("original");
    holder
        .arm(&input, fixture.original(), 1000, until())
        .unwrap();
    // Shut down only our send half: peer credentials and the real original
    // remain live, but sendmsg fails AFTER the one-way spend.
    fixture.socket.shutdown(std::net::Shutdown::Write).unwrap();
    assert!(holder.lend(&fixture.socket, &request).is_err());
    assert!(holder.phase.as_ref().unwrap().spent);
    assert!(!holder.settled());
    assert!(holder.joined(&request).is_err());
    fixture.child.kill().unwrap();
    fixture.child.wait().unwrap();
    holder.joined(&request).unwrap();
}

#[test]
fn a_fresh_readonly_list_preserves_original_custody_but_never_expands_the_old_consent() {
    struct Owned(std::process::Child);
    impl Drop for Owned {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let (_dir, mut holder, input, _request) = holder();
    let child = Owned(
        std::process::Command::new("/bin/sleep")
            .arg("300")
            .current_dir(&input.manifest.capture.root)
            .spawn()
            .unwrap(),
    );
    // The census lists this test's own occupant (zerus-89pkd); the
    // assertions concern only it and the original custody.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let roster = crate::platform::worktree_membership::test_roster::owned(&[child.0.id()]);
    let fresh = holder.observe(1001, until()).unwrap();
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    roster.assert_completed();
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    drop(roster);
    assert!(fresh.programs.iter().any(|p| p.pid == child.0.id()));
    assert_eq!(fresh.capture, input.manifest.capture);
    assert_eq!(fresh.checkout, input.manifest.checkout);
    assert_eq!(
        (fresh.issued, fresh.expires),
        (input.manifest.issued, input.manifest.expires)
    );
    assert_ne!(fresh.id, input.manifest.id);
    assert!(!holder.original.contains_originals(&fresh.programs));
    let mut changed = input;
    changed.manifest = fresh;
    let borrower = Fixture::start("original");
    assert!(
        holder
            .arm(&changed, borrower.original(), 1001, until())
            .is_err(),
        "a confirmed old empty list cannot stop a newly observed program"
    );
    assert!(
        crate::platform::process_identity(child.0.id()).is_some(),
        "observation never signals the new program"
    );
}

#[test]
fn an_observation_cannot_adopt_a_replaced_root_or_extend_the_original_bound() {
    let (dir, mut original, _input, _request) = holder();
    let moved = dir.path().with_extension("original");
    std::fs::rename(dir.path(), &moved).unwrap();
    std::fs::create_dir(dir.path()).unwrap();
    assert!(original.observe(1001, until()).is_err());
    std::fs::remove_dir(moved).unwrap();
    let (_dir, mut original, _input, _request) = holder();
    original.until = Instant::now();
    assert!(original.observe(1001, until()).is_err());
}

#[test]
fn a_changed_checkout_name_cannot_be_reopened_as_the_original_holder() {
    let (dir, mut holder, input, request) = holder();
    let fixture = Fixture::start("original");
    holder
        .arm(&input, fixture.original(), 1000, until())
        .unwrap();
    let moved = dir.path().with_extension("old");
    std::fs::rename(dir.path(), &moved).unwrap();
    std::fs::create_dir(dir.path()).unwrap();
    assert!(holder.lend(&fixture.socket, &request).is_err());
    assert!(!holder.phase.as_ref().unwrap().spent);
    assert!(!holder.settled());
    std::fs::remove_dir(&moved).unwrap();
}

/// zerus-89pkd: an owned roster never hides uncertainty from an observation.
/// An entry that still exists without a cwd, as a process caught while it
/// exits, fails the observation; it is never an empty list. Since
/// zerus-lo8o1.1 that read leaves each observation unsettled, so the bounded
/// re-reads end in limit.
#[cfg(target_os = "linux")]
#[test]
fn an_observation_keeps_a_missing_read_in_an_existing_entry_uncertain() {
    let (_dir, mut holder, _input, _request) = holder();
    let roster = crate::platform::worktree_membership::test_roster::owned(&[]);
    let entry = roster.path().join(u32::MAX.to_string());
    std::fs::create_dir(&entry).unwrap();
    std::fs::create_dir(entry.join("task")).unwrap();
    for name in ["status", "stat"] {
        std::fs::write(
            entry.join(name),
            std::fs::read(format!("/proc/self/{name}")).unwrap(),
        )
        .unwrap();
    }
    let error = holder.observe(1001, until()).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData, "{error}");
}

#[cfg(target_os = "linux")]
fn native_descriptors(holder: &Holder) -> Vec<(i32, u64, u64)> {
    use std::os::fd::AsRawFd;
    holder
        .scope
        .native_descriptors()
        .iter()
        .map(|fd| {
            let mut stat: libc::stat = unsafe { std::mem::zeroed() };
            assert_eq!(unsafe { libc::fstat(fd.as_raw_fd(), &mut stat) }, 0);
            (fd.as_raw_fd(), stat.st_dev, stat.st_ino)
        })
        .collect()
}

#[test]
#[cfg(target_os = "linux")]
fn admission_still_refuses_a_previously_pinned_peer_whose_leader_exited() {
    use std::io::Write;
    let (_dir, mut holder, input, _request) = holder();
    let mut fixture = Fixture::start("original");
    let peer = fixture.original(); // Genuine live SAME-image admission, BEFORE leader exit.
    fixture.exit_leader();
    assert!(!peer.exited().unwrap());
    assert!(peer.current(&fixture.socket, until()).is_err());
    // Actual production attachment path: a physical-only admission mutant
    // would accept this original Peer, despite the unchanged old refusal.
    assert!(holder.arm(&input, peer, 1000, until()).is_err());
    assert!(holder.settled());
    fixture
        .child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"R")
        .unwrap();
    assert!(fixture.child.wait().unwrap().success());
}

#[test]
#[cfg(target_os = "linux")]
fn leader_exit_release_eof_and_closed_or_expired_admission_never_clear_a_live_phase() {
    use std::io::{Read, Write};
    for ending in ["open", "closed", "expired"] {
        let (_dir, mut holder, input, request) = holder();
        let mut fixture = Fixture::start("original");
        holder
            .arm(&input, fixture.original(), 1000, until())
            .unwrap();
        fixture.expect_loan(&input.manifest, &Request::decode(&request).unwrap());
        holder.lend(&fixture.socket, &request).unwrap();
        fixture.loan_received();
        let original_objects = native_descriptors(&holder);
        fixture.exit_leader(); // Worker retains ACTUAL received Scope objects.
        assert!(!holder.phase.as_ref().unwrap().borrower.exited().unwrap());
        assert!(holder
            .phase
            .as_ref()
            .unwrap()
            .borrower
            .current(&fixture.socket, until())
            .is_err());
        assert!(holder.joined(&request).is_err());
        let mut wrong = Request::decode(&request).unwrap();
        wrong.request = uuid();
        let wrong = wrong.bytes().unwrap();
        assert!(holder.joined(&wrong).is_err());
        fixture
            .child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(b"A")
            .unwrap();
        let mut released = [0; 8];
        fixture.socket.read_exact(&mut released).unwrap();
        assert_eq!(&released, b"released");
        assert!(
            holder.joined(&request).is_err(),
            "release ACK never retires a live process"
        );
        fixture
            .child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(b"C")
            .unwrap();
        assert_eq!(fixture.socket.read(&mut [0]).unwrap(), 0);
        assert!(
            holder.joined(&request).is_err(),
            "original loan socket EOF is not exit"
        );
        if ending == "closed" {
            holder.close();
        }
        if ending == "expired" {
            holder.until = Instant::now();
            holder.phase.as_mut().unwrap().until = Instant::now();
        }
        assert!(!holder.settled());
        assert!(holder.observe(1001, until()).is_err());
        assert!(holder.joined(&request).is_err());
        assert_eq!(native_descriptors(&holder), original_objects);
        let other = Fixture::start("original");
        let mut fresh = input.clone();
        fresh.request_id = uuid();
        assert!(holder.arm(&fresh, other.original(), 1000, until()).is_err());
        assert!(holder.lend(&fixture.socket, &request).is_err());
        assert!(!holder.settled());
        fixture
            .child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(b"R")
            .unwrap();
        let end = Instant::now() + Duration::from_secs(5);
        while !holder.phase.as_ref().unwrap().borrower.exited().unwrap() {
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(
            !holder.settled(),
            "exit fact alone cannot mint the owner's joined message"
        );
        assert!(holder.joined(&wrong).is_err());
        assert!(
            fixture.child.wait().unwrap().success(),
            "observer must not consume owner wait"
        );
        assert!(holder.phase.as_ref().unwrap().borrower.exited().unwrap());
        holder.joined(&request).unwrap();
        assert!(holder.settled());
        assert_eq!(native_descriptors(&holder), original_objects);
        if ending != "open" {
            assert!(holder.observe(1001, until()).is_err());
        }
    }
}

#[test]
#[cfg(target_os = "linux")]
fn a_real_nonleader_exec_keeps_the_old_phase_until_actual_owner_join() {
    use std::io::Write;
    let (_dir, mut holder, input, request) = holder();
    let mut fixture = Fixture::start("original");
    holder
        .arm(&input, fixture.original(), 1000, until())
        .unwrap();
    let original_objects = native_descriptors(&holder);
    fixture
        .child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"E")
        .unwrap();
    fixture.wait_replacement(&holder.phase.as_ref().unwrap().borrower);
    assert!(holder.joined(&request).is_err());
    assert!(!holder.settled());
    assert!(holder.observe(1001, until()).is_err());
    fixture.socket.shutdown(std::net::Shutdown::Both).unwrap();
    assert!(holder.joined(&request).is_err());
    assert_eq!(native_descriptors(&holder), original_objects);
    fixture.child.kill().unwrap(); // Sole original owner cleanup, never the observer.
    fixture.child.wait().unwrap();
    assert!(holder.phase.as_ref().unwrap().borrower.exited().unwrap());
    holder.joined(&request).unwrap();
    assert!(holder.settled());
}

#[test]
#[cfg(target_os = "linux")]
fn physical_peer_exit_does_not_supply_the_owners_original_output_eof_or_join() {
    use std::{
        fs::File,
        io::Read,
        os::fd::FromRawFd,
        process::{Command, Stdio},
    };
    fn pipe() -> (File, File) {
        let mut fds = [-1; 2];
        assert_eq!(unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) }, 0);
        // SAFETY: these are the two fresh owned pipe descriptors from pipe2.
        unsafe { (File::from_raw_fd(fds[0]), File::from_raw_fd(fds[1])) }
    }
    struct OutputOwner(std::process::Child);
    impl Drop for OutputOwner {
        fn drop(&mut self) {
            drop(self.0.stdin.take());
            if matches!(self.0.try_wait(), Ok(None)) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
    }
    let (_dir, mut holder, input, request) = holder();
    let (mut output, output_write) = pipe();
    let (mut error, error_write) = pipe();
    let mut fixture = Fixture::with_output(
        Stdio::from(output_write.try_clone().unwrap()),
        Stdio::from(error_write.try_clone().unwrap()),
    );
    let mut peer = OutputOwner(
        Command::new("/bin/cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::from(output_write))
            .stderr(Stdio::from(error_write))
            .spawn()
            .unwrap(),
    );
    let out = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        output.read_to_end(&mut bytes).unwrap();
        bytes
    });
    let err = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        error.read_to_end(&mut bytes).unwrap();
        bytes
    });
    holder
        .arm(&input, fixture.original(), 1000, until())
        .unwrap();
    drop(fixture.child.stdin.take()); // Native borrower exits; the two ORIGINAL streams stay held.
    assert!(fixture.child.wait().unwrap().success());
    assert!(holder.phase.as_ref().unwrap().borrower.exited().unwrap());
    assert!(peer.0.try_wait().unwrap().is_none());
    assert!(!out.is_finished() && !err.is_finished());
    assert!(!holder.settled());
    assert!(holder.observe(1001, until()).is_err());
    // Do not lie to Holder::joined: its private owner precondition requires
    // these ACTUAL original I/O workers and loan/resource owners to settle.
    drop(peer.0.stdin.take());
    assert!(peer.0.wait().unwrap().success());
    let end = Instant::now() + Duration::from_secs(5);
    while !out.is_finished() || !err.is_finished() {
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(1));
    }
    out.join().unwrap();
    err.join().unwrap();
    holder.joined(&request).unwrap();
    assert!(holder.settled());
}
