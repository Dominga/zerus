use super::*;
use selected_source::Snapshot;

#[test]
fn complete_source_and_canonical_equivalence() {
    fixture("changes");
}
#[test]
fn exact_id_slot_never_resolves_its_labels_holder() {
    fixture("slot");
}
#[test]
fn invalid_irregular_relative_and_oversized_slots_refuse() {
    fixture("bounds");
}
#[test]
fn observations_and_capture_never_mutate_stored_metadata() {
    fixture("observations");
}

#[test]
fn retained_fence_keeps_the_original_near_bound_source_without_refresh() {
    fixture("retained");
}

fn stored() -> (Value, PathBuf) {
    let mut record = raw();
    record["agent_home"] = json!(home().join(".claude"));
    let path = label_record_path(string(&record, "name"));
    fs::write(&path, record.to_string()).unwrap();
    (record, path)
}

pub(super) fn changes() {
    let (original, path) = stored();
    let name = string(&original, "name");
    let frame = Snapshot::capture(&path, name).unwrap();
    for (key, value) in [
        ("session_id", json!("22222222-2222-4222-8222-222222222222")),
        ("run_id", json!("another run")),
        ("conversation_id", json!("another conversation")),
        ("base", json!(["another-executable"])),
        ("launch_dir", json!("another folder")),
        ("agent_home", json!("another account")),
        ("future", json!({"opaque":[null,false,"different"]})),
        ("layout_transition", json!({"unknown":"retain"})),
        ("layout_transition", json!(false)),
        ("layout_transition", json!(["malformed"])),
        ("future_field", Value::Null),
    ] {
        let mut changed = original.clone();
        changed[key] = value;
        fs::write(&path, changed.to_string()).unwrap();
        assert!(frame.recheck().is_err(), "changed {key} must refuse");
        assert_eq!(
            frame.record(),
            &original,
            "recheck must never adopt a new source"
        );
        assert_eq!(fs::read(&path).unwrap(), changed.to_string().as_bytes());
    }
    let mut removed = original.clone();
    removed.as_object_mut().unwrap().remove("layout_transition");
    fs::write(&path, removed.to_string()).unwrap();
    assert!(frame.recheck().is_err(), "missing differs from null");
    fs::write(&path, serde_json::to_string_pretty(&original).unwrap()).unwrap();
    frame.recheck().unwrap();
    let reordered = original
        .as_object()
        .unwrap()
        .iter()
        .rev()
        .map(|(key, value)| format!("{}:{value}", serde_json::to_string(key).unwrap()))
        .collect::<Vec<_>>()
        .join(",");
    fs::write(&path, format!("{{{reordered}}}")).unwrap();
    frame.recheck().unwrap();
    for marker in [Value::Null, json!(false), json!([]), json!({"opaque":3})] {
        let mut record = original.clone();
        record["layout_transition"] = marker;
        fs::write(&path, record.to_string()).unwrap();
        let captured = Snapshot::capture(&path, name).unwrap();
        assert_eq!(
            captured.record(),
            &record,
            "negative evidence is data, never repaired"
        );
        captured.recheck().unwrap();
    }
}

pub(super) fn slot() {
    let (legacy, first) = stored();
    let name = string(&legacy, "name");
    let id = "22222222-2222-4222-8222-222222222222";
    let mut twin = legacy.clone();
    twin["session_id"] = json!(id);
    twin["layout"] = json!("id");
    twin["base"] = json!(["twin executable"]);
    let own = id_record_path(id).unwrap();
    fs::create_dir(own.parent().unwrap()).unwrap();
    fs::write(&own, twin.to_string()).unwrap();
    let captured = Snapshot::capture(&own, name).unwrap();
    assert_eq!(
        captured.record(),
        &twin,
        "explicit slot must not read the first label holder"
    );
    let mut changed_first = legacy.clone();
    changed_first["base"] = json!(["changed first holder"]);
    fs::write(&first, changed_first.to_string()).unwrap();
    captured.recheck().unwrap();
    fs::remove_file(&own).unwrap();
    assert!(
        captured.recheck().is_err(),
        "missing ID slot cannot fall back to its label holder"
    );
    fs::write(&own, legacy.to_string()).unwrap();
    assert!(
        captured.recheck().is_err(),
        "layout/path/SID mismatch must refuse"
    );
    fs::write(&own, twin.to_string()).unwrap();
    captured.recheck().unwrap();
    session_id::select(name, string(&legacy, "session_id"));
    assert!(
        Snapshot::capture(&own, name).is_err(),
        "ambient selection mismatch cannot select another record"
    );
}

pub(super) fn bounds() {
    let (original, path) = stored();
    let name = string(&original, "name");
    let captured = Snapshot::capture(&path, name).unwrap();
    for bytes in [b"{invalid".as_slice(), b"[]", b"null", b"\xff"] {
        fs::write(&path, bytes).unwrap();
        assert!(Snapshot::capture(&path, name).is_err());
        assert!(captured.recheck().is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
    for (key, value) in [
        ("version", json!(2)),
        ("name", json!("claude/example/other")),
        ("agent", json!("unknown")),
        ("layout", json!("id")),
    ] {
        let mut record = original.clone();
        record[key] = value;
        fs::write(&path, record.to_string()).unwrap();
        assert!(Snapshot::capture(&path, name).is_err());
    }
    let mut big = original.clone();
    big["padding"] = json!("x".repeat(ID_RECORD_BYTES as usize));
    fs::write(&path, big.to_string()).unwrap();
    assert!(
        Snapshot::capture(&path, name).is_err(),
        "one byte beyond the bound must refuse"
    );
    assert!(captured.recheck().is_err());
    fs::remove_file(&path).unwrap();
    assert!(captured.recheck().is_err());
    fs::create_dir(&path).unwrap();
    assert!(Snapshot::capture(&path, name).is_err());
    fs::remove_dir(&path).unwrap();
    fs::write(&path, original.to_string()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let link = root().join("linked.json");
        symlink(&path, &link).unwrap();
        assert!(Snapshot::capture(&link, name).is_err());
        let fifo = root().join("fifo.json");
        let encoded = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(encoded.as_ptr(), 0o600) }, 0);
        assert!(
            Snapshot::capture(&fifo, name).is_err(),
            "FIFO must refuse without waiting for a writer"
        );
    }
    std::env::set_current_dir(root()).unwrap();
    fs::write("relative.json", original.to_string()).unwrap();
    assert!(Snapshot::capture(Path::new("relative.json"), name).is_err());
}

struct HeldChild(std::process::Child);
impl Drop for HeldChild {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

pub(super) fn observations() {
    let (mut original, path) = stored();
    let child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "state::storage::selected_source_tests::fixture_entry",
            "--test-threads=1",
        ])
        .env("ZERUS_SELECTED_SOURCE_CASE", "hold")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let mut child = HeldChild(child);
    original["pid"] = json!(std::process::id());
    original["process_start"] = json!(process_start(std::process::id()));
    original["subagents"]["child"]["original_process"] =
        subagent_lifecycle::capture(&original, child.0.id()).unwrap();
    fs::write(&path, original.to_string()).unwrap();
    let name = string(&original, "name");
    let before = fs::read(&path).unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    let entries = || {
        let mut entries: Vec<_> = fs::read_dir(root())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        entries.sort();
        entries
    };
    let tree = entries();
    let captured = Snapshot::capture(&path, name).unwrap();
    assert_eq!(captured.record(), &original);
    captured.recheck().unwrap();
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
    assert_eq!(
        entries(),
        tree,
        "capture/recheck never publish a lock, repair or side file"
    );
    let live = read(name).unwrap();
    assert_eq!(live["subagents"]["child"]["process_state"], "live");
    let retained = captured.into_fence();
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    let ended = read(name).unwrap();
    assert_eq!(ended["subagents"]["child"]["process_state"], "ended");
    assert_ne!(
        ended, live,
        "ordinary read projection changes without a stored-record change"
    );
    retained.recheck().unwrap();
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
}

pub(super) fn retained() {
    let (mut original, path) = stored();
    original["padding"] = json!("x".repeat(ID_RECORD_BYTES as usize - 2048));
    let bytes = original.to_string();
    assert!(bytes.len() < ID_RECORD_BYTES as usize);
    assert!(bytes.len() > ID_RECORD_BYTES as usize - 2048);
    fs::write(&path, &bytes).unwrap();
    let name = string(&original, "name");
    let frame = Snapshot::capture(&path, name).unwrap();
    assert_eq!(frame.record(), &original);
    // Conversion must not consult the source, even when it no longer exists.
    fs::remove_file(&path).unwrap();
    let retained = frame.into_fence();
    assert!(!path.exists());
    assert!(retained.recheck().is_err());
    fs::write(&path, &bytes).unwrap();
    retained.recheck().unwrap();
    let mut fences = Vec::new();
    for _ in 0..3 {
        let frame = Snapshot::capture(&path, name).unwrap();
        assert_eq!(frame.record()["padding"], original["padding"]);
        fences.push(frame.into_fence());
    }
    let frame = Snapshot::capture(&path, name).unwrap();
    let mut changed = original.clone();
    changed["future"]["opaque"] = json!("changed before conversion");
    fs::write(&path, changed.to_string()).unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    let original_proof = frame.into_fence();
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
    assert!(
        original_proof.recheck().is_err(),
        "conversion must not re-hash the new source"
    );
    for fence in &fences {
        assert!(fence.recheck().is_err());
    }
    fs::write(&path, &bytes).unwrap();
    original_proof.recheck().unwrap();
    for fence in fences {
        fence.recheck().unwrap();
    }
}
