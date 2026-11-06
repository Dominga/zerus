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
        holder.joined(&request).is_err(),
        "an owner ACK cannot retire a live helper"
    );
    fixture.expect_loan(&input.manifest);
    holder.lend(&fixture.socket, &request).unwrap();
    assert!(holder.lend(&fixture.socket, &request).is_err());
    assert!(holder.joined(&request).is_err());
    fixture.release_loan();
    holder.joined(&request).unwrap();
    assert!(holder.settled());
    let next = Fixture::start("original");
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
