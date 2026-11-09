use super::*;
use crate::state::repositories::directory::fail_publication_sync;
use serde_json::json;
use std::{
    fs,
    fs::OpenOptions,
    path::PathBuf,
    time::{Duration, SystemTime},
};

const ID: &str = "00000000-0000-4000-8000-000000000001";
fn request() -> Value {
    json!({"request_id":ID,"text":"Original task prompt","attachments":[],
        "expected_run_id":"original-run","expected_conversation_id":"original-conversation"})
}
fn scope() -> Scope {
    Scope::new("codex/example/worker", &request(), &"b".repeat(64)).unwrap()
}
fn refused(same_plan: bool) -> Observed {
    Observed::Refused {
        same_plan,
        proof: same_plan.then(|| Proof {
            schema: 1,
            request_id: ID.into(),
            plan_digest: scope().plan_digest,
            reason: Reason::AcknowledgementModeChanged,
        }),
    }
}
struct Fixture {
    root: tempfile::TempDir,
    writer: writer::Writer,
}
impl Fixture {
    fn new() -> Self {
        let root =
            tempfile::tempdir_in(platform::canonical(std::env::temp_dir()).unwrap()).unwrap();
        platform::restrict_dir(root.path()).unwrap();
        platform::private_open(OpenOptions::new().create_new(true).write(true))
            .open(root.path().join(".lock"))
            .unwrap();
        let writer =
            writer::test_open(root.path(), Instant::now() + Duration::from_secs(2)).unwrap();
        Self { root, writer }
    }
    fn file(&self) -> PathBuf {
        self.root.path().join(FOLDER).join(format!("{ID}.json"))
    }
    fn write(&self, bytes: &[u8]) {
        platform::private_dir(&self.root.path().join(FOLDER)).unwrap();
        use std::io::Write;
        platform::private_open(OpenOptions::new().create(true).write(true).truncate(true))
            .open(self.file())
            .unwrap()
            .write_all(bytes)
            .unwrap();
    }
    fn publish(&self) -> Result<Observed> {
        remember_locked(&self.writer, &scope(), Mode::Managed, Mode::Legacy)
    }
}

#[test]
fn gate_off_does_not_acquire_a_writer_or_touch_the_selected_store() {
    // No test changes HGS_STATE_DIR: both entries must return before examining
    // it, even with an expired budget. All active tests pass explicit fixtures.
    assert_eq!(
        lookup(&scope(), Instant::now()).unwrap(),
        Observed::Inactive
    );
    assert_eq!(
        remember(&scope(), Mode::Managed, Mode::Legacy, Instant::now()).unwrap(),
        Observed::Inactive
    );
}

#[test]
fn the_existing_native_input_writer_excludes_negative_publication() {
    let f = Fixture::new();
    let root = f.root;
    drop(f.writer);
    // The maintained native input writer, not another task-control actor.
    // Passing an explicit fixture path avoids ambient state and recovery.
    let input = storage::lock(Some(&root.path().join(".lock"))).unwrap();
    assert!(writer::test_open(root.path(), Instant::now() + Duration::from_millis(10)).is_err());
    assert!(!root.path().join(FOLDER).exists());
    drop(input);
    let writer = writer::test_open(root.path(), Instant::now() + Duration::from_secs(2)).unwrap();
    assert_eq!(
        remember_locked(&writer, &scope(), Mode::Managed, Mode::Legacy).unwrap(),
        refused(true)
    );
}

#[test]
fn a_complete_synced_negative_permanently_reserves_the_uuid_and_never_rewrites() {
    let f = Fixture::new();
    let original_scope = scope();
    assert_eq!(
        lookup_locked(&f.writer, &original_scope).unwrap(),
        Observed::Absent
    );
    assert_eq!(f.publish().unwrap(), refused(true));
    let bytes = fs::read(f.file()).unwrap();
    let Observed::Refused {
        proof: Some(proof), ..
    } = lookup_locked(&f.writer, &original_scope).unwrap()
    else {
        panic!("A complete synced read-back must return its own proof")
    };
    assert_eq!(
        serde_json::to_value(proof).unwrap(),
        json!({
            "schema":1,"request_id":ID,"plan_digest":scope().plan_digest,
            "reason":"acknowledgement_mode_changed",
        })
    );
    assert_eq!(
        lookup_locked(&f.writer, &original_scope).unwrap(),
        refused(true)
    );
    assert_eq!(f.publish().unwrap(), refused(true));
    let other_plan = Scope::new(&original_scope.name, &request(), &"c".repeat(64)).unwrap();
    let mut different = request();
    different["text"] = "Different body".into();
    let other_body = Scope::new(
        &original_scope.name,
        &different,
        &original_scope.plan_digest,
    )
    .unwrap();
    for changed in [other_plan, other_body] {
        assert_eq!(lookup_locked(&f.writer, &changed).unwrap(), refused(false));
        assert_eq!(
            remember_locked(&f.writer, &changed, Mode::Legacy, Mode::Managed).unwrap(),
            refused(false)
        );
        assert_eq!(fs::read(f.file()).unwrap(), bytes);
        assert!(matches!(
            super::super::existing(&f.file(), &changed.digest, &changed.name),
            super::super::Existing::Unknown(_)
        ));
    }
    assert!(remember_locked(&f.writer, &original_scope, Mode::Managed, Mode::Managed).is_err());
    assert_eq!(fs::read(f.file()).unwrap(), bytes);
}

#[test]
fn pending_submitted_different_damaged_and_partial_receipts_are_never_overwritten() {
    let f = Fixture::new();
    f.publish().unwrap();
    let original: Value = serde_json::from_slice(&fs::read(f.file()).unwrap()).unwrap();
    let mut cases = vec![
        b"{partial".to_vec(),
        b"null".to_vec(),
        b"{}".to_vec(),
        vec![b' '; BYTES + 1],
    ];
    for (field, value) in [
        ("status", json!("pending")),
        ("status", json!("submitted")),
        ("status", json!("in_progress")),
        ("status", json!("future_status")),
        ("digest", json!("x")),
        ("refusal_schema", json!(2)),
        ("first_message", json!("false")),
        ("request_id", json!("different")),
        ("extra", json!(true)),
    ] {
        let mut v = original.clone();
        v[field] = value;
        cases.push(serde_json::to_vec(&v).unwrap());
    }
    let mut missing = original;
    let duplicate = serde_json::to_string(&missing).unwrap().replacen(
        "{",
        &format!("{{\"request_id\":\"{ID}\","),
        1,
    );
    cases.push(duplicate.into_bytes());
    missing.as_object_mut().unwrap().remove("first_message");
    cases.push(serde_json::to_vec(&missing).unwrap());
    for bytes in cases {
        f.write(&bytes);
        assert!(matches!(f.publish(), Ok(Observed::Existing) | Err(_)));
        assert_eq!(fs::read(f.file()).unwrap(), bytes);
    }
}

#[test]
fn large_normal_receipts_remain_lookup_only_and_oversized_files_are_unknown() {
    let f = Fixture::new();
    let normal =
        serde_json::to_vec(&json!({"status":"submitted", "text":"x".repeat(65536)})).unwrap();
    assert!(normal.len() > BYTES);
    f.write(&normal);
    assert_eq!(f.publish().unwrap(), Observed::Existing);
    assert_eq!(fs::read(f.file()).unwrap(), normal);
    // A sparse unexpected file is rejected by metadata, without allocating
    // or reading its body, falling through to an unbounded old reader, or
    // mistaking it for absence. Its retained bytes/length stay untouched.
    let file = OpenOptions::new().write(true).open(f.file()).unwrap();
    file.set_len(READ_BYTES as u64 + 1).unwrap();
    assert!(f.publish().is_err());
    assert_eq!(file.metadata().unwrap().len(), READ_BYTES as u64 + 1);
}

#[test]
fn an_error_after_noreplace_publication_is_unknown_and_lookup_never_retries_it() {
    let f = Fixture::new();
    platform::private_dir(&f.root.path().join(FOLDER)).unwrap();
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            fail_publication_sync(None);
        }
    }
    {
        let _reset = Reset;
        fail_publication_sync(Some(f.root.path().join(FOLDER)));
        assert!(f.publish().is_err());
        assert!(f.file().is_file());
        assert!(lookup_locked(&f.writer, &scope()).is_err());
    }
    let bytes = fs::read(f.file()).unwrap();
    // A later read syncs and validates the retained object; it neither repairs
    // its bytes nor reconstructs or repeats a submission.
    assert_eq!(lookup_locked(&f.writer, &scope()).unwrap(), refused(true));
    assert_eq!(f.publish().unwrap(), refused(true));
    assert_eq!(fs::read(f.file()).unwrap(), bytes);
}

#[test]
fn restart_and_old_mtime_never_expire_a_tombstone_or_scan_other_receipts() {
    let f = Fixture::new();
    f.publish().unwrap();
    let path = f.file();
    fs::File::open(&path)
        .unwrap()
        .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(86400))
        .unwrap();
    let bytes = fs::read(&path).unwrap();
    let other = f.root.path().join(FOLDER).join("unrelated.json");
    fs::write(&other, b"keep exactly").unwrap();
    let fifo = std::ffi::CString::new(
        f.root
            .path()
            .join(FOLDER)
            .join("must-not-read")
            .to_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    let root = f.root;
    drop(f.writer);
    let writer = writer::test_open(root.path(), Instant::now() + Duration::from_secs(2)).unwrap();
    assert_eq!(lookup_locked(&writer, &scope()).unwrap(), refused(true));
    assert!(matches!(
        super::super::existing(&path, &scope().digest, &scope().name),
        super::super::Existing::Unknown(_)
    ));
    assert_eq!(fs::read(path).unwrap(), bytes);
    assert_eq!(fs::read(other).unwrap(), b"keep exactly");
}

#[test]
fn selected_symlinks_and_fifos_are_unknown_not_missing_or_replaceable() {
    let f = Fixture::new();
    f.publish().unwrap();
    let path = f.file();
    let kept = path.with_extension("kept");
    fs::rename(&path, &kept).unwrap();
    let bytes = fs::read(&kept).unwrap();
    std::os::unix::fs::symlink(&kept, &path).unwrap();
    assert!(f.publish().is_err());
    assert_eq!(fs::read(&kept).unwrap(), bytes);
    fs::remove_file(&path).unwrap();
    let name = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    assert!(f.publish().is_err());
    assert_eq!(fs::read(&kept).unwrap(), bytes);
}
