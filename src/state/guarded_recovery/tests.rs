use super::*;
use std::{
    fs,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Mutex,
    },
    time::Duration,
};

#[test]
fn equal_bytes_replacement_never_adopts_the_new_original() {
    let temp = tempfile::tempdir().unwrap();
    let path = crate::platform::canonical(temp.path())
        .unwrap()
        .join("intent.json");
    fs::write(&path, b"{\"version\":1}\n").unwrap();
    let held = Original::read_existing(&path, 4096).unwrap().unwrap();
    held.verify().unwrap();
    fs::rename(&path, path.with_extension("retained")).unwrap();
    fs::write(&path, held.bytes()).unwrap();
    assert_eq!(fs::read(&path).unwrap(), held.bytes());
    assert!(held.verify().is_err());
}

#[test]
fn in_place_change_with_restored_mtime_still_refuses_the_original() {
    let temp = tempfile::tempdir().unwrap();
    let path = crate::platform::canonical(temp.path())
        .unwrap()
        .join("record.json");
    fs::write(&path, b"original").unwrap();
    let held = Original::read_existing(&path, 4096).unwrap().unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    std::thread::sleep(Duration::from_millis(2));
    fs::write(&path, b"replaced").unwrap();
    File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(modified))
        .unwrap();
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
    assert!(held.verify().is_err());
}

#[test]
fn absent_source_survives_own_parent_creation_but_refuses_a_racing_file() {
    let temp = tempfile::tempdir().unwrap();
    let root = crate::platform::canonical(temp.path()).unwrap();
    let path = root.join("new-parent/entry.json");
    let held = Original::read_absent(&path).unwrap();
    assert!(!held.is_present());
    fs::create_dir(root.join("new-parent")).unwrap();
    held.verify().unwrap();
    fs::write(&path, b"another writer").unwrap();
    assert!(held.verify().is_err());
    assert_eq!(fs::read(path).unwrap(), b"another writer");
}

#[test]
fn replaced_parent_and_nonregular_sources_never_become_absence() {
    use std::os::unix::{ffi::OsStrExt, fs::symlink};
    let temp = tempfile::tempdir().unwrap();
    let root = crate::platform::canonical(temp.path()).unwrap();
    fs::create_dir(root.join("original")).unwrap();
    let absent = Original::read_absent(&root.join("original/missing")).unwrap();
    fs::rename(root.join("original"), root.join("retained")).unwrap();
    fs::create_dir(root.join("original")).unwrap();
    assert!(absent.verify().is_err());
    fs::write(root.join("target"), b"data").unwrap();
    symlink(root.join("target"), root.join("alias")).unwrap();
    assert!(Original::read_existing(&root.join("alias"), 4096).is_err());
    let fifo = root.join("fifo");
    let native = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(native.as_ptr(), 0o600) }, 0);
    assert!(Original::read_existing(&fifo, 4096).is_err());
}

type Query = Box<dyn FnOnce() + Send>;
struct Control {
    open: AtomicBool,
    until: Instant,
    query: Mutex<Option<Query>>,
    changes: AtomicUsize,
}
impl Control {
    fn new(query: Option<Query>) -> Self {
        Self {
            open: AtomicBool::new(true),
            until: Instant::now() + Duration::from_secs(5),
            query: Mutex::new(query),
            changes: AtomicUsize::new(0),
        }
    }
}
impl Effects for Control {
    fn check(&self) -> Result<()> {
        if self.open.load(Ordering::Acquire) && Instant::now() < self.until {
            Ok(())
        } else {
            Err("original fixture endpoint ended".into())
        }
    }
    fn until(&self) -> Instant {
        self.until
    }
    fn read(&self, path: &Path, limit: u64) -> Result<Option<Original>> {
        self.check()?;
        Original::read_existing(path, limit)
    }
    fn session_name(&self, _: &str) -> Result<Option<String>> {
        if let Some(query) = self.query.lock().unwrap().take() {
            query();
        }
        Ok(Some("codex/example/original".into()))
    }
    fn atomic(&self, _: &Path, _: &[u8], _: &[&Original]) -> Result<()> {
        self.changes.fetch_add(1, Ordering::SeqCst);
        Err("unexpected fixture publication".into())
    }
    fn remove(&self, path: &Path, sources: &[&Original]) -> Result<()> {
        for original in sources {
            original.verify()?;
        }
        self.check()?;
        self.changes.fetch_add(1, Ordering::SeqCst);
        fs::remove_file(path).map_err(|e| e.to_string())
    }
    fn restore(
        &self,
        _: &Path,
        _: &Path,
        _: &str,
        _: Option<&Value>,
        _: &[&Original],
    ) -> Result<()> {
        self.changes.fetch_add(1, Ordering::SeqCst);
        Err("unexpected fixture restore".into())
    }
}

#[test]
fn original_source_drift_during_native_query_refuses_before_removal() {
    let temp = tempfile::tempdir().unwrap();
    let path = crate::platform::canonical(temp.path())
        .unwrap()
        .join("transaction.json");
    fs::write(&path, b"original").unwrap();
    let changed = path.clone();
    let effects = Control::new(Some(Box::new(move || {
        fs::write(changed, b"replaced").unwrap();
    })));
    let mut view = View::new(&effects);
    assert_eq!(view.read(&path, 4096).unwrap().unwrap(), b"original");
    assert!(view.session_name("$1").is_err());
    assert_eq!(effects.changes.load(Ordering::SeqCst), 0);
    assert_eq!(fs::read(path).unwrap(), b"replaced");
}

#[test]
fn endpoint_refusal_cannot_remove_a_previously_read_original() {
    let temp = tempfile::tempdir().unwrap();
    let path = crate::platform::canonical(temp.path())
        .unwrap()
        .join("transaction.json");
    fs::write(&path, b"original").unwrap();
    let effects = Control::new(None);
    let mut view = View::new(&effects);
    view.read(&path, 4096).unwrap();
    effects.open.store(false, Ordering::Release);
    assert!(view.remove(&path).is_err());
    assert_eq!(effects.changes.load(Ordering::SeqCst), 0);
    assert_eq!(fs::read(path).unwrap(), b"original");
}

#[test]
fn a_cached_source_cannot_bypass_a_later_smaller_read_bound() {
    let temp = tempfile::tempdir().unwrap();
    let path = crate::platform::canonical(temp.path())
        .unwrap()
        .join("entry");
    fs::write(&path, b"12345678").unwrap();
    let effects = Control::new(None);
    let mut view = View::new(&effects);
    view.read(&path, 8).unwrap();
    assert!(view.read(&path, 7).is_err());
}

fn recovery_control(case: &str) {
    let temp = tempfile::tempdir().unwrap();
    let state = crate::platform::canonical(temp.path())
        .unwrap()
        .join("state");
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "state::guarded_recovery::tests::recovery_fixture",
            "--ignored",
            "--test-threads=1",
        ])
        .env("HGS_STATE_DIR", state)
        .env("ZERUS_GUARDED_RECOVERY_CONTROL", case)
        .env_remove("HGS_SESSION")
        .env_remove("HGS_SESSION_ID")
        .env_remove("HGS_RUN_ID")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{case}: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("test result: ok. 1 passed"));
}

#[test]
fn rename_recovery_known_not_renamed_retires_only_the_original_intent() {
    recovery_control("not_renamed");
}

#[test]
fn rename_recovery_same_bytes_intent_substitution_after_query_refuses() {
    recovery_control("intent_substitution");
}

#[test]
fn raw_metadata_recovery_preserves_working_children_without_external_refresh() {
    recovery_control("metadata_only");
}

#[test]
fn effort_metadata_selectors_preserve_identity_ambiguity_and_reference_fences() {
    recovery_control("selectors");
}

#[test]
fn canonical_sid_reference_uses_index_without_fleet_enumeration() {
    recovery_control("reference_index");
}

fn indexed_reference_fixture() {
    const SID: &str = "11111111-1111-4111-8111-111111111111";
    const NAME: &str = "codex/example/indexed";
    fs::create_dir_all(root().join("sessions")).unwrap();
    fs::create_dir_all(root().join("ids")).unwrap();
    let record = json!({"version":1,"layout":"id","session_id":SID,"name":NAME,"agent":"codex","run_id":"run","conversation_id":"conversation"});
    fs::write(
        storage::id_record_path(SID).unwrap(),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    fs::write(
        root().join("ids").join(format!("{SID}.json")),
        serde_json::to_vec(&json!({"version":1,"name":NAME,"kind":"id"})).unwrap(),
    )
    .unwrap();
    for index in 0..5001 {
        fs::write(
            root().join("sessions").join(format!("unrelated-{index}")),
            b"",
        )
        .unwrap();
    }
    assert!(
        session_index::metadata_records_unrepaired()
            .unwrap_err()
            .contains("bound"),
        "control must establish that a full fleet scan cannot cover the fixture"
    );
    let reference = crate::session_reference::Reference {
        machine: SID.into(),
        host: "local".into(),
        name: "codex/example/old".into(),
        conversation: "conversation".into(),
        run: "old-run".into(),
        archive: String::new(),
        agent: String::new(),
        session: SID.into(),
    };
    assert_eq!(resolve_effort_reference(&reference).unwrap(), NAME);
    assert_eq!(session_id::selected().unwrap().session_id, SID);
    assert!(session_id::selected().unwrap().id_layout);
    assert!(!root().join(".lock").exists());
    assert!(!root().join("scan-cache").exists());
}

fn selector_fixture() {
    const A: &str = "11111111-1111-4111-8111-111111111111";
    const B: &str = "22222222-2222-4222-8222-222222222222";
    const C: &str = "33333333-3333-4333-8333-333333333333";
    const NAME: &str = "codex/example/shared";
    fs::create_dir_all(root().join("sessions")).unwrap();
    let a = json!({"version":1,"layout":"id","session_id":A,"name":NAME,"agent":"codex","run_id":"run-a","conversation_id":"conversation-a","pid":41,"process_start":"birth-a","supervisor":{"pid":40,"birth":"supervisor-a"},"subagents":{"child":{"state":"working","display_state":"unknown"}}});
    let mut b = a.clone();
    b["session_id"] = json!(B);
    b["run_id"] = json!("run-b");
    b["conversation_id"] = json!("conversation-b");
    let pa = storage::id_record_path(A).unwrap();
    let pb = storage::id_record_path(B).unwrap();
    fs::write(&pa, serde_json::to_vec(&a).unwrap()).unwrap();
    fs::write(&pb, serde_json::to_vec(&b).unwrap()).unwrap();
    let refusal = resolve_effort_label(NAME, LabelGuard::None).unwrap_err();
    assert_eq!(refusal.code, session_id::AMBIGUOUS);
    let report = refusal.value.unwrap();
    assert_eq!(report["total"], 2);
    assert!(report["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row["state"] == "unknown"));
    resolve_effort_label(NAME, LabelGuard::Request).unwrap();
    assert!(session_id::selected().is_none());
    assert_eq!(session_id::take_deferred(NAME).unwrap().1["total"], 2);
    resolve_effort_label(
        NAME,
        LabelGuard::Given {
            run: "run-a".into(),
            conversation: "conversation-a".into(),
        },
    )
    .unwrap();
    assert_eq!(session_id::selected().unwrap().session_id, A);
    assert_eq!(storage::read_metadata(NAME).unwrap(), a);
    session_id::clear();
    // A deferred Request must remain DATA-only while Given selects among a
    // listed ID index and a legacy first holder. No inventory read can set
    // global REFUSED or invoke the ordinary selected-reader projection path.
    let mut first = a.clone();
    first.as_object_mut().unwrap().remove("layout");
    first["session_id"] = json!(C);
    first["run_id"] = json!("run-c");
    first["conversation_id"] = json!("conversation-c");
    let first_path = storage::legacy_record_path(NAME);
    fs::write(&first_path, serde_json::to_vec(&first).unwrap()).unwrap();
    fs::create_dir_all(root().join("labels")).unwrap();
    let key = storage::label_key(NAME);
    let index = serde_json::to_vec(&json!({"version":1,"label":NAME,"ids":[A,B]})).unwrap();
    fs::write(root().join("labels").join(format!("{key}.json")), &index).unwrap();
    use sha2::{Digest, Sha256};
    let mut manifest = BTreeMap::new();
    manifest.insert(key, format!("{:x}", Sha256::digest(&index)));
    fs::write(
        root().join("labels/.complete"),
        serde_json::to_vec(&json!({"version":1,"labels":manifest})).unwrap(),
    )
    .unwrap();
    resolve_effort_label(NAME, LabelGuard::Request).unwrap();
    resolve_effort_label(
        NAME,
        LabelGuard::Given {
            run: "run-a".into(),
            conversation: "conversation-a".into(),
        },
    )
    .unwrap();
    assert_eq!(session_id::selected().unwrap().session_id, A);
    assert!(
        session_id::refused().is_none(),
        "raw candidate reads must not create stale ambiguity refusal"
    );
    assert_eq!(storage::read_metadata(NAME).unwrap(), a);
    session_id::take_deferred(NAME);
    session_id::clear();
    fs::remove_dir_all(root().join("labels")).unwrap();
    fs::remove_file(first_path).unwrap();
    assert_eq!(resolve_effort_id(A).unwrap(), NAME);
    assert!(session_id::selected().unwrap().id_layout);
    let mut first = a.clone();
    first.as_object_mut().unwrap().remove("layout");
    first["session_id"] = json!(C);
    let legacy = storage::legacy_record_path(NAME);
    fs::write(&legacy, serde_json::to_vec(&first).unwrap()).unwrap();
    fs::remove_file(&pa).unwrap();
    assert!(
        storage::read_metadata(NAME).is_err(),
        "missing selected ID must not reach another label holder"
    );
    let missing = resolve_effort_id(A).unwrap_err();
    assert_eq!(missing.code, 1);
    assert!(missing.text.contains("No live or saved session"));
    fs::write(&pa, serde_json::to_vec(&a).unwrap()).unwrap();
    fs::remove_file(legacy).unwrap();
    session_id::clear();
    let mut reference = crate::session_reference::Reference {
        machine: C.into(),
        host: "local".into(),
        name: "codex/example/old-name".into(),
        conversation: "conversation-b".into(),
        run: "obsolete-run".into(),
        archive: String::new(),
        agent: String::new(),
        session: String::new(),
    };
    assert_eq!(resolve_effort_reference(&reference).unwrap(), NAME);
    let selected = session_id::selected().unwrap();
    assert_eq!(selected.session_id, B);
    assert!(selected.id_layout);
    assert_eq!(selected.conversation, "conversation-b");
    assert!(selected.run.is_empty());
    // Public CLI already rejects URI+--session-id. Reusable metadata helpers
    // still compose their floors conjunctively rather than dropping a prior
    // reference when invoked again by a state caller/classifier.
    assert!(resolve_effort_id(A).is_err());
    assert_eq!(session_id::selected().unwrap().session_id, B);
    assert_eq!(resolve_effort_id(B).unwrap(), NAME);
    assert_eq!(
        session_id::selected().unwrap().conversation,
        "conversation-b"
    );
    assert!(resolve_effort_label(
        NAME,
        LabelGuard::Given {
            run: "run-a".into(),
            conversation: "conversation-a".into()
        }
    )
    .is_err());
    assert_eq!(session_id::selected().unwrap().session_id, B);
    assert_eq!(
        session_id::selected().unwrap().conversation,
        "conversation-b"
    );
    session_id::clear();
    reference.archive = C.into();
    assert!(resolve_effort_reference(&reference)
        .unwrap_err()
        .contains("archived conversation"));
    reference.archive.clear();
    let mut duplicate = a.clone();
    duplicate["conversation_id"] = json!("conversation-b");
    fs::write(&pa, serde_json::to_vec(&duplicate).unwrap()).unwrap();
    assert!(resolve_effort_reference(&reference)
        .unwrap_err()
        .contains("Ambiguous session reference"));
    assert!(session_id::selected().is_none());
    let old = json!({"version":1,"name":"codex/example/legacy","agent":"codex","run_id":"legacy-run","conversation_id":"","pid":51,"process_start":"legacy-birth"});
    fs::write(
        storage::legacy_record_path("codex/example/legacy"),
        serde_json::to_vec(&old).unwrap(),
    )
    .unwrap();
    reference.conversation.clear();
    reference.run = "legacy-run".into();
    assert_eq!(
        resolve_effort_reference(&reference).unwrap(),
        "codex/example/legacy"
    );
    assert_eq!(session_id::selected().unwrap().run, "legacy-run");
    assert!(!session_id::selected().unwrap().id_layout);
    assert_eq!(storage::read_metadata("codex/example/legacy").unwrap(), old);
    for path in [".lock", "labels", "ids", "scan-cache"] {
        assert!(
            !root().join(path).exists(),
            "unexpected selector effect: {path}"
        );
    }
}

fn metadata_fixture() {
    const SID: &str = "33333333-3333-4333-8333-333333333333";
    const PARENT: &str = "11111111-1111-4111-8111-111111111111";
    const CHILD: &str = "22222222-2222-4222-8222-222222222222";
    let home = root().join("fixture-provider-home");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(root().join("sessions")).unwrap();
    let database = home.join("state_5.sqlite");
    let rollout = home.join("rollout-child.jsonl");
    {
        let db = rusqlite::Connection::open(&database).unwrap();
        db.execute_batch("CREATE TABLE threads(id TEXT PRIMARY KEY, rollout_path TEXT)")
            .unwrap();
        db.execute(
            "INSERT INTO threads VALUES(?1,?2)",
            rusqlite::params![CHILD, rollout.to_str()],
        )
        .unwrap();
    }
    let header = json!({"type":"session_meta","payload":{"id":CHILD,"parent_thread_id":PARENT,"forked_from_id":PARENT}});
    let complete = json!({"type":"event_msg","timestamp":"1970-01-01T00:00:10Z","payload":{"type":"task_complete","turn_id":"44444444-4444-4444-8444-444444444444","last_agent_message":"Complete"}});
    fs::write(&rollout, format!("{header}\n{complete}\n")).unwrap();
    let record = json!({"version":1,"layout":"id","session_id":SID,"name":"codex/example/original","agent":"codex","agent_home":home,
        "run_id":"55555555-5555-4555-8555-555555555555","conversation_id":PARENT,"pid":4242,"process_start":"original-start","supervisor":{"pid":4241,"birth":"original-supervisor"},
        "main_done":true,"phase":"working","activity":"busy","active_tools":{},"reply_id":"original-reply",
        "subagents":{CHILD:{"name":"Reviewer","state":"working","display_state":"unknown","started":1.0,"updated":2.0,"reply_id":"child-reply"}}});
    let cache = root().join("fixture-scan-cache");
    scan_cache::DIR.with(|slot| *slot.borrow_mut() = Some(cache.clone()));
    let mut projected = record.clone();
    native_subagents::reconcile(&mut projected);
    assert_eq!(
        projected["subagents"][CHILD]["state"], "finished",
        "positive control must establish real external telemetry refresh"
    );
    assert!(
        cache.is_dir(),
        "positive control must establish the advisory write"
    );
    fs::remove_dir_all(&cache).unwrap();
    let database_before = fs::read(&database).unwrap();
    let rollout_before = fs::read(&rollout).unwrap();
    let path = storage::id_record_path(SID).unwrap();
    fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
    let effects = Control::new(None);
    let mut view = View::new(&effects);
    assert_eq!(view.record(&path, string(&record, "name")).unwrap(), record);
    assert_eq!(
        storage::read_metadata("codex/example/original").unwrap(),
        record
    );
    assert_eq!(
        session_index::metadata_records_for_id_unrepaired(SID)
            .unwrap()
            .iter()
            .map(|record| string(record, "name").to_owned())
            .collect::<Vec<_>>(),
        vec!["codex/example/original"]
    );
    let candidates = labels::candidates_metadata("codex/example/original").unwrap();
    assert!(!candidates.first);
    assert_eq!(candidates.by_id, vec![record.clone()]);
    assert!(
        !root().join("ids").exists(),
        "unrepaired ID lookup must not repair cache/Root"
    );
    assert!(
        !root().join("labels").exists(),
        "metadata label lookup must not rebuild manifest"
    );
    assert!(
        !root().join(".lock").exists(),
        "metadata lookup must not acquire or initialize Root"
    );
    assert!(!cache.exists());
    assert!(!root().join("scan-cache").exists());
    assert!(!database.with_extension("sqlite-shm").exists());
    assert!(!database.with_extension("sqlite-wal").exists());
    assert_eq!(fs::read(database).unwrap(), database_before);
    assert_eq!(fs::read(rollout).unwrap(), rollout_before);
    assert_eq!(effects.changes.load(Ordering::SeqCst), 0);
    scan_cache::DIR.with(|slot| *slot.borrow_mut() = None);
}

#[test]
#[ignore = "isolated original Root data fixture"]
fn recovery_fixture() {
    let case = std::env::var("ZERUS_GUARDED_RECOVERY_CONTROL").unwrap();
    if case == "metadata_only" {
        metadata_fixture();
        return;
    }
    if case == "selectors" {
        selector_fixture();
        return;
    }
    if case == "reference_index" {
        indexed_reference_fixture();
        return;
    }
    fs::create_dir_all(root().join("renames")).unwrap();
    let intent = root().join("renames/transaction.json");
    let transaction = json!({"version":1,"old":"codex/example/original","new":"codex/example/renamed", "terminal":"codex/example/original", "session_id":"$7", "record":null});
    let bytes = serde_json::to_vec(&transaction).unwrap();
    fs::write(&intent, &bytes).unwrap();
    let original_binding = root().join("untouched-binding");
    fs::write(&original_binding, b"kept original binding").unwrap();
    let query: Option<Query> = if case == "intent_substitution" {
        let path = intent.clone();
        let same = bytes.clone();
        Some(Box::new(move || {
            fs::rename(&path, path.with_extension("retained")).unwrap();
            fs::write(&path, &same).unwrap();
        }))
    } else {
        None
    };
    let effects = Control::new(query);
    let result = rename::recover_guarded(&effects);
    match case.as_str() {
        "not_renamed" => {
            result.unwrap();
            assert!(!intent.exists());
            assert_eq!(effects.changes.load(Ordering::SeqCst), 1);
        }
        "intent_substitution" => {
            assert!(result.is_err());
            assert_eq!(fs::read(&intent).unwrap(), bytes);
            assert_eq!(fs::read(intent.with_extension("retained")).unwrap(), bytes);
            assert_eq!(effects.changes.load(Ordering::SeqCst), 0);
        }
        _ => panic!("unknown recovery fixture"),
    }
    assert_eq!(
        fs::read(original_binding).unwrap(),
        b"kept original binding"
    );
}
