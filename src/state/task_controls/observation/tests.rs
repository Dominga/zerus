use super::*;
use crate::platform;
use serde_json::json;
use std::{fs, fs::OpenOptions, io::Write, path::PathBuf};

fn id(n: u64) -> String {
    format!("00000000-0000-4000-8000-{n:012x}")
}
fn executor() -> Executor {
    Executor {
        computer: id(1),
        session_id: id(2),
        run: "original-run".into(),
        birth: "supervisor-birth".into(),
        conversation: Some("original-conversation".into()),
        child: None,
    }
}
fn write(path: &Path, value: &Value) {
    platform::private_open(OpenOptions::new().create(true).write(true).truncate(true))
        .open(path)
        .unwrap()
        .write_all(&serde_json::to_vec(value).unwrap())
        .unwrap();
}
struct Fixture {
    dir: tempfile::TempDir,
    writer: storage::Guard,
    record: Value,
    index: Value,
    path: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let dir = crate::host::staging::private_tempdir_in(&platform::canonical(std::env::temp_dir()).unwrap());
        platform::restrict_dir(dir.path()).unwrap();
        for child in ["ids", "sessions"] {
            platform::private_dir(&dir.path().join(child)).unwrap();
        }
        let file = platform::private_open(
            OpenOptions::new()
                .create(true)
                .read(true)
                .write(true)
                .truncate(false),
        )
        .open(dir.path().join(".lock"))
        .unwrap();
        let writer = platform::FileLock::exclusive(file).unwrap();
        let record = json!({"version":1,"layout":"id","session_id":id(2),"name":"claude/example/original",
            "run_id":"original-run","conversation_id":"original-conversation","agent":"claude",
            "pid":42,"process_start":"agent-birth","supervisor":{"pid":41,"start":"supervisor-birth"}});
        let index = json!({"version":1,"kind":"id","name":"claude/example/original"});
        let path = dir.path().join("sessions").join(format!("{}.json", id(2)));
        let fixture = Self {
            dir,
            writer,
            record,
            index,
            path,
        };
        fixture.save();
        fixture
    }
    fn save(&self) {
        write(&self.path, &self.record);
        write(&self.index_path(), &self.index);
    }
    fn index_path(&self) -> PathBuf {
        self.dir.path().join("ids").join(format!("{}.json", id(2)))
    }
    fn read(&self, expected: &Executor, mode: Conversation) -> Result<Observed> {
        read_enabled(self.dir.path(), &self.writer, &id(1), expected, mode)
    }
}

#[test]
fn gate_off_is_no_access_even_with_a_missing_index_and_invalid_executor() {
    let fixture = Fixture::new();
    let mut bad = executor();
    bad.session_id.clear();
    let missing = fixture.dir.path().join("absent");
    assert!(read(
        &missing,
        &fixture.writer,
        "wrong-computer",
        &bad,
        Conversation::Exact
    )
    .unwrap()
    .is_none());
    assert!(!missing.exists());
}

#[test]
fn rename_preserves_the_uuid_guard_while_resume_never_retargets_the_old_key() {
    let mut f = Fixture::new();
    let original = executor();
    let first = f.read(&original, Conversation::Exact).unwrap();
    assert_eq!(first.executor, original);
    assert_eq!((first.process.pid, first.supervisor.pid), (42, 41));
    assert_eq!(first.agent, "claude");
    f.record["name"] = "claude/example/renamed".into();
    f.index["name"] = f.record["name"].clone();
    f.save();
    assert_eq!(
        f.read(&original, Conversation::Exact).unwrap().executor,
        original
    );
    f.record["run_id"] = "resume-run".into();
    f.record["supervisor"]["start"] = "resume-birth".into();
    f.save();
    let before = fs::read(&f.path).unwrap();
    assert!(f.read(&original, Conversation::Exact).is_err());
    let mut resumed = original.clone();
    resumed.run = "resume-run".into();
    resumed.birth = "resume-birth".into();
    let current = f.read(&resumed, Conversation::Exact).unwrap();
    assert_eq!(current.executor.session_id, original.session_id);
    assert_ne!(current.executor.key().unwrap(), original.key().unwrap());
    assert_eq!(fs::read(&f.path).unwrap(), before);
}

#[test]
fn spoofed_stale_reused_archived_and_unpublished_records_refuse_without_repair() {
    let mut f = Fixture::new();
    let original = f.record.clone();
    for (field, value) in [
        ("session_id", json!(id(3))),
        ("run_id", json!("reused-run")),
        ("conversation_id", json!("other-conversation")),
        ("name", json!("reused-label")),
        ("version", json!(2)),
        ("layout", json!("legacy")),
        ("archive_id", json!(id(4))),
        ("archived_at", json!(1)),
        ("state", json!("saved")),
        ("paused", json!(true)),
        ("pausing", json!(true)),
        ("terminating", json!(true)),
        ("rename_shadow", json!(true)),
        ("pid", json!(41)),
        ("pid", json!(0)),
        ("process_start", json!("")),
        ("agent", json!("opencode")),
    ] {
        f.record = original.clone();
        f.record[field] = value;
        f.save();
        let before = fs::read(&f.path).unwrap();
        assert!(f.read(&executor(), Conversation::Exact).is_err(), "{field}");
        assert_eq!(fs::read(&f.path).unwrap(), before, "{field}");
    }
    f.record = original;
    f.record["supervisor"]["start"] = "other-birth".into();
    f.save();
    // The bounded reader observes record text; only the held native proof
    // verifies that text against the original kernel process and exact key.
    assert_eq!(
        f.read(&executor(), Conversation::Exact)
            .unwrap()
            .supervisor
            .recorded_start,
        "other-birth"
    );
    f.record["supervisor"]["start"] = "supervisor-birth".into();
    f.save();
    let mut remote = executor();
    remote.computer = id(3);
    assert!(f.read(&remote, Conversation::Exact).is_err());
    let mut child = executor();
    child.child = Some("child-one".into());
    assert!(f.read(&child, Conversation::Exact).is_err());
    fs::remove_file(f.index_path()).unwrap();
    // The correct record remains present. A miss still never scans or repairs.
    assert!(f.read(&executor(), Conversation::Exact).is_err());
    assert!(!f.index_path().exists());
}

#[test]
fn only_original_startup_binding_may_observe_the_first_confirmed_conversation() {
    let mut f = Fixture::new();
    let mut startup = executor();
    startup.conversation = None;
    assert!(f.read(&startup, Conversation::Exact).is_err());
    assert_eq!(
        f.read(&startup, Conversation::FirstBinding)
            .unwrap()
            .executor,
        executor()
    );
    assert!(f.read(&executor(), Conversation::FirstBinding).is_err());
    f.record["conversation_id"] = Value::Null;
    f.save();
    assert!(f
        .read(&startup, Conversation::Exact)
        .unwrap()
        .executor
        .conversation
        .is_none());
    assert!(f.read(&startup, Conversation::FirstBinding).is_err());
}

#[test]
fn legacy_and_routed_label_holders_are_selected_only_by_the_index_hit() {
    let mut f = Fixture::new();
    f.record.as_object_mut().unwrap().remove("layout");
    f.index["kind"] = "record".into();
    let filename = format!(
        "{:x}.json",
        Sha256::digest("claude/example/original".as_bytes())
    );
    f.path = f.dir.path().join(&filename);
    f.save();
    assert!(f.read(&executor(), Conversation::Exact).is_ok());
    platform::private_dir(&f.dir.path().join("name_routes")).unwrap();
    platform::private_dir(&f.dir.path().join("bindings")).unwrap();
    write(
        &f.dir.path().join("name_routes").join(&filename),
        &json!({"version":1}),
    );
    // The pre-route record is still there and correct: there is no fallback.
    assert!(f.read(&executor(), Conversation::Exact).is_err());
    f.path = f.dir.path().join("bindings").join(&filename);
    f.save();
    assert!(f.read(&executor(), Conversation::Exact).is_ok());
}

#[test]
fn bounded_private_no_follow_reads_refuse_unexpected_objects() {
    let mut f = Fixture::new();
    f.index["kind"] = "dsh-id".into();
    f.save();
    assert!(f.read(&executor(), Conversation::Exact).is_err());
    f.index["kind"] = "id".into();
    f.save();
    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(f.index_path())
        .unwrap();
    file.write_all(&vec![b' '; INDEX_BYTES + 1]).unwrap();
    assert!(f.read(&executor(), Conversation::Exact).is_err());
    f.save();
    let file = OpenOptions::new().write(true).open(&f.path).unwrap();
    file.set_len(storage::ID_RECORD_BYTES + 1).unwrap();
    assert!(f.read(&executor(), Conversation::Exact).is_err());
    f.save();
    #[cfg(unix)]
    {
        fs::rename(&f.path, f.path.with_extension("kept")).unwrap();
        std::os::unix::fs::symlink(f.path.with_extension("kept"), &f.path).unwrap();
        assert!(f.read(&executor(), Conversation::Exact).is_err());
        fs::remove_file(&f.path).unwrap();
        let name = std::ffi::CString::new(f.path.to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        assert!(f.read(&executor(), Conversation::Exact).is_err());
        fs::remove_file(&f.path).unwrap();
        f.save();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&f.path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(f.read(&executor(), Conversation::Exact).is_err());
    }
    fs::rename(f.dir.path().join(".lock"), f.dir.path().join(".old-lock")).unwrap();
    write(&f.dir.path().join(".lock"), &json!({}));
    assert!(f.read(&executor(), Conversation::Exact).is_err());
}

#[test]
fn review_malformed_liveness_is_unknown_and_never_observed_as_live() {
    let mut f = Fixture::new();
    for state in [json!(false), json!(42), json!([]), json!({"live":true})] {
        f.record["state"] = state.clone();
        f.save();
        let original = fs::read(&f.path).unwrap();
        let refused = f.read(&executor(), Conversation::Exact).is_err();
        assert_eq!(fs::read(&f.path).unwrap(), original);
        assert!(
            refused,
            "malformed state {state} was accepted as live evidence"
        );
    }
}

#[test]
fn legacy_liveness_absence_null_and_live_strings_remain_compatible() {
    let mut f = Fixture::new();
    for state in [Value::Null, json!("live"), json!("active")] {
        f.record["state"] = state;
        f.save();
        assert!(f.read(&executor(), Conversation::Exact).is_ok());
    }
    f.record.as_object_mut().unwrap().remove("state");
    f.save();
    assert!(f.read(&executor(), Conversation::Exact).is_ok());
}
