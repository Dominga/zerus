use super::*;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::process::{Command, Stdio};
use std::time::Duration;

fn until() -> Instant {
    Instant::now() + Duration::from_secs(3)
}

fn record() -> Value {
    json!({"version":1,"agent":"claude","name":"claude/example/shared","layout":"id",
        "session_id":uuid::Uuid::new_v4().to_string(),"run_id":uuid::Uuid::new_v4().to_string(),
        "conversation_id":"","terminal_name":"private-terminal"})
}

fn scope() -> Guard {
    private_dir(&root()).unwrap();
    private_dir(&root().join("sessions")).unwrap();
    lock(None).unwrap()
}

fn sid_path(record: &Value) -> PathBuf {
    root()
        .join("native-input-reservations")
        .join(format!("sid-{}.json", string(record, "session_id")))
}
fn input_path(record: &Value) -> PathBuf {
    root()
        .join("native-input")
        .join(format!("{}.lock", string(record, "session_id")))
}
fn json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

// Test-only bounded synthetic tree snapshot. Production reads exact names only.
fn snapshot(path: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn visit(base: &Path, path: &Path, rows: &mut Vec<(PathBuf, Vec<u8>)>) {
        let mut entries: Vec<_> = fs::read_dir(path)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        entries.sort();
        for path in entries {
            let m = fs::symlink_metadata(&path).unwrap();
            let rel = path.strip_prefix(base).unwrap().to_owned();
            rows.push((
                rel.clone(),
                format!("{}:{}:{}:{}", m.dev(), m.ino(), m.mode(), m.len()).into_bytes(),
            ));
            if m.is_dir() {
                visit(base, &path, rows);
            } else if m.is_file() {
                rows.push((rel, fs::read(path).unwrap()));
            }
        }
    }
    let mut rows = Vec::new();
    visit(path, path, &mut rows);
    rows
}

fn control(case: &str) {
    let temp = tempfile::tempdir().unwrap();
    let output = tempfile::tempfile().unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    for (key, _) in std::env::vars_os() {
        let key_text = key.to_string_lossy();
        if ["HGS_", "ZERUS_", "CODEX_", "CLAUDE_", "KIMI_", "XDG_"]
            .iter()
            .any(|prefix| key_text.starts_with(prefix))
        {
            command.env_remove(key);
        }
    }
    for key in [
        "TMUX",
        "TMUX_PANE",
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "DBUS_SESSION_BUS_ADDRESS",
    ] {
        command.env_remove(key);
    }
    let mut child = command
        .args([
            "--exact",
            "state::storage::native_domain::kind_tests::fixture",
            "--ignored",
            "--test-threads=1",
        ])
        .env("HGS_STATE_DIR", temp.path().join("state"))
        .env("ZERUS_INPUT_KIND_CONTROL", case)
        .env("HOME", temp.path())
        .stdin(Stdio::null())
        .stdout(output.try_clone().unwrap())
        .stderr(output.try_clone().unwrap())
        .spawn()
        .unwrap();
    let until = Instant::now() + Duration::from_secs(15);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= until {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("{case}: private fixture blocked");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    use std::io::{Read, Seek};
    let mut output = output;
    output.rewind().unwrap();
    let mut text = String::new();
    output.read_to_string(&mut text).unwrap();
    assert!(
        status.success() && text.contains("test result: ok. 1 passed"),
        "{case}: {text}"
    );
}

#[test]
fn fresh_publication_holds_one_proven_sid_lock_before_the_record() {
    control("fresh");
}
#[test]
fn a_lost_moved_marker_or_alias_never_becomes_fresh() {
    control("moved");
}
#[test]
fn original_v1_bytes_still_load_but_cannot_substitute_for_a_sid_kind() {
    control("v1");
}
#[test]
fn sid_kind_mutations_and_irregular_objects_refuse_without_writes() {
    control("evidence");
}
#[test]
fn every_fresh_crash_cut_retains_its_original_orphan_without_reuse() {
    control("cuts");
}
#[test]
fn a_wrong_writer_or_aliased_scope_cannot_mint_an_input_kind() {
    control("scope");
}
#[test]
fn held_proof_identity_and_deadline_refusals_never_release_exclusion() {
    control("held");
}

#[test]
#[ignore = "run only by the bounded private controls"]
fn fixture() {
    let Ok(case) = std::env::var("ZERUS_INPUT_KIND_CONTROL") else {
        return;
    };
    let writer = scope();
    let mut current = record();
    match case.as_str() {
        "fresh" => {
            let id = string(&current, "session_id").to_owned();
            let path = id_record_path(&id).unwrap();
            let proof = sid_path(&current);
            let lock = input_path(&current);
            BEFORE_NEW_ID.with(|slot| {
                *slot.borrow_mut() = Some(Box::new(move || {
                    assert!(!path.exists());
                    assert!(proof.exists());
                    assert!(fs::metadata(&proof).unwrap().len() <= 4096);
                    assert_eq!(json(&proof)["origin"], json!({"kind":"fresh"}));
                    assert!(
                        lock_until(&lock, Instant::now()).unwrap().is_none(),
                        "publication retains physical input exclusion"
                    );
                }))
            });
            publish_new_id(&writer, &mut current).unwrap();
            let initial_proof = fs::read(sid_path(&current)).unwrap();
            assert!(!root()
                .join("native-input")
                .join(format!("{}.lock", label_key(string(&current, "name"))))
                .exists());
            assert!(!root()
                .join("native-input-reservations")
                .join(format!("{}.json", label_key(string(&current, "name"))))
                .exists());
            let held = Fresh::acquire(&current, until()).unwrap();
            held.verify().unwrap();
            assert!(Fresh::acquire(&current, until()).is_err());
            drop(held);
            current["name"] = json!("claude/example/renamed");
            current["run_id"] = json!(uuid::Uuid::new_v4().to_string());
            write(&mut current).unwrap();
            Fresh::acquire(&current, until()).unwrap().verify().unwrap();
            assert_eq!(fs::read(sid_path(&current)).unwrap(), initial_proof);
            remove_record(&current).unwrap();
            let before = snapshot(&root());
            assert!(Fresh::acquire(&current, until()).is_err());
            assert!(publish_new_id(&writer, &mut current).is_err());
            assert_eq!(
                snapshot(&root()),
                before,
                "an unpublished/orphan SID is never reused"
            );
        }
        "moved" | "v1" => {
            current.as_object_mut().unwrap().remove("layout");
            let path = legacy_record_path(string(&current, "name"));
            atomic(&path, &current.to_string()).unwrap();
            let domain = Domain::prepare(&current, until()).unwrap();
            let old_path = root()
                .join("native-input-reservations")
                .join(format!("{}.json", owner_key(&current)));
            let old = json(&old_path);
            let sid = json(&sid_path(&current));
            assert_eq!(sid["version"], 2);
            for key in ["session_id", "state", "inputs", "reservations", "lock"] {
                assert_eq!(sid[key], old[key]);
            }
            assert_eq!(
                sid["origin"],
                json!({"kind":"legacy_moved","old_key":old["old_key"],"before_digest":old["before_digest"]})
            );
            let state = Directory::open(&absolute_root().unwrap()).unwrap();
            let inputs = state.child("native-input").unwrap();
            // Exact prior struct/field-order serialization, with no new field.
            let expected=format!("{{\"version\":1,\"session_id\":{},\"old_key\":{},\"before_digest\":{},\"state\":{},\"inputs\":{},\"reservations\":{},\"lock\":{}}}",old["session_id"],old["old_key"],old["before_digest"],old["state"],old["inputs"],old["reservations"],old["lock"]);
            assert_eq!(fs::read(&old_path).unwrap(), expected.as_bytes());
            drop(domain);
            if case == "v1" {
                fs::remove_file(sid_path(&current)).unwrap();
                reservation::Held::load(&state, &inputs, &current)
                    .unwrap()
                    .verify(&state, &inputs, &current)
                    .unwrap();
                let before = snapshot(&root());
                assert!(Domain::recover(&current, until()).is_err());
                assert_eq!(snapshot(&root()), before);
                return;
            }
            let sid_original = fs::read(sid_path(&current)).unwrap();
            for key in ["session_id", "lock", "old_key", "before_digest"] {
                let mut wrong = sid.clone();
                match key {
                    "session_id" => wrong[key] = json!(uuid::Uuid::new_v4().to_string()),
                    "lock" => wrong[key] = json!([0, 0, 0, 0]),
                    _ => wrong["origin"][key] = json!("different"),
                }
                atomic(&sid_path(&current), &wrong.to_string()).unwrap();
                let before = snapshot(&root());
                assert!(Domain::recover(&current, until()).is_err());
                assert_eq!(snapshot(&root()), before);
            }
            atomic(
                &sid_path(&current),
                std::str::from_utf8(&sid_original).unwrap(),
            )
            .unwrap();
            Domain::recover(&current, until())
                .unwrap()
                .verify()
                .unwrap();
            current["layout"] = json!("id");
            state
                .child("sessions")
                .unwrap()
                .write_new(
                    &format!("{}.json", string(&current, "session_id")),
                    current.to_string().as_bytes(),
                )
                .unwrap();
            fs::remove_file(&path).unwrap();
            let before = snapshot(&root());
            assert!(Fresh::acquire(&current, until()).is_err());
            assert_eq!(snapshot(&root()), before);
            // Remove the known original legacy name, leaving a singly-linked
            // SID file. A link count cannot turn LegacyMoved into Fresh.
            fs::remove_file(
                root()
                    .join("native-input")
                    .join(format!("{}.lock", old["old_key"].as_str().unwrap())),
            )
            .unwrap();
            assert_eq!(fs::metadata(input_path(&current)).unwrap().nlink(), 1);
            let before = snapshot(&root());
            assert!(Fresh::acquire(&current, until()).is_err());
            assert_eq!(snapshot(&root()), before);
        }
        "evidence" => {
            publish_new_id(&writer, &mut current).unwrap();
            let path = sid_path(&current);
            let original = fs::read(&path).unwrap();
            let value = json(&path);
            let mut mutations = Vec::new();
            for (key, bad) in [
                ("version", json!(3)),
                ("session_id", json!(uuid::Uuid::new_v4().to_string())),
                ("state", json!([0, 0])),
                ("inputs", json!([0, 0])),
                ("reservations", json!([0, 0])),
                ("lock", json!([0, 0, 0, 0])),
                ("origin", json!({"kind":"future"})),
                ("origin", json!({"kind":"fresh","extra":true})),
                ("extra", json!(true)),
            ] {
                let mut v = value.clone();
                v[key] = bad;
                mutations.push(v.to_string().into_bytes());
            }
            mutations.extend([b"null".to_vec(), b"{".to_vec(), vec![b' '; 4097]]);
            for bytes in mutations {
                atomic(&path, std::str::from_utf8(&bytes).unwrap()).unwrap();
                let before = snapshot(&root());
                assert!(Fresh::acquire(&current, until()).is_err());
                assert_eq!(snapshot(&root()), before);
            }
            fs::remove_file(&path).unwrap();
            let foreign = root().join("foreign-kind");
            atomic(&foreign, std::str::from_utf8(&original).unwrap()).unwrap();
            symlink(&foreign, &path).unwrap();
            assert!(Fresh::acquire(&current, until()).is_err());
            fs::remove_file(&path).unwrap();
            fs::hard_link(&foreign, &path).unwrap();
            assert!(Fresh::acquire(&current, until()).is_err());
            fs::remove_file(&path).unwrap();
            let name = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
            assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
            assert!(Fresh::acquire(&current, until()).is_err());
            fs::remove_file(&path).unwrap();
            fs::create_dir(&path).unwrap();
            assert!(Fresh::acquire(&current, until()).is_err());
            fs::remove_dir(&path).unwrap();
            atomic(&path, std::str::from_utf8(&original).unwrap()).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
            assert!(Fresh::acquire(&current, until()).is_err());
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
            Fresh::acquire(&current, until()).unwrap();
            let input = input_path(&current);
            let retained = input.with_extension("original");
            fs::rename(&input, &retained).unwrap();
            for variant in ["missing", "symlink", "fifo", "replacement", "hardlink"] {
                match variant {
                    "symlink" => symlink(&retained, &input).unwrap(),
                    "fifo" => {
                        let name = std::ffi::CString::new(input.to_str().unwrap()).unwrap();
                        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
                    }
                    "replacement" => atomic(&input, "").unwrap(),
                    "hardlink" => {
                        fs::hard_link(&retained, &input).unwrap();
                    }
                    _ => (),
                }
                let before = snapshot(&root());
                assert!(Fresh::acquire(&current, until()).is_err());
                assert_eq!(snapshot(&root()), before);
                if variant != "missing" {
                    fs::remove_file(&input).unwrap();
                }
            }
            // Even after the fresh lock is lost, its authoritative SID kind
            // forbids creating an old-name domain from a legacy-shaped record.
            let mut legacy = current.clone();
            legacy.as_object_mut().unwrap().remove("layout");
            atomic(
                &legacy_record_path(string(&legacy, "name")),
                &legacy.to_string(),
            )
            .unwrap();
            let before = snapshot(&root());
            assert!(Original::acquire_under(&writer, &legacy, until()).is_err());
            assert_eq!(snapshot(&root()), before);
            fs::rename(&retained, &input).unwrap();
            Fresh::acquire(&current, until()).unwrap();
            fs::remove_file(&path).unwrap();
            let before = snapshot(&root());
            assert!(Fresh::acquire(&current, until()).is_err());
            assert!(publish_new_id(&writer, &mut current).is_err());
            assert_eq!(snapshot(&root()), before);
        }
        "cuts" => {
            drop(writer);
            let selected = root();
            let mut completed = 0;
            let mut orphans = 0;
            for budget in 0..=8 {
                let sub = selected.join(format!("cut-{budget}"));
                unsafe { std::env::set_var("HGS_STATE_DIR", &sub) };
                let writer = scope();
                let mut current = record();
                cut::arm(budget);
                let result = publish_new_id(&writer, &mut current);
                cut::disarm();
                if id_record_path(string(&current, "session_id"))
                    .unwrap()
                    .exists()
                {
                    result.unwrap();
                    Fresh::acquire(&current, until()).unwrap();
                    completed += 1;
                } else {
                    assert!(result.is_err());
                    assert!(Fresh::acquire(&current, until()).is_err());
                    if input_path(&current).exists() || sid_path(&current).exists() {
                        orphans += 1;
                        let before = snapshot(&root());
                        assert!(publish_new_id(&writer, &mut current).is_err());
                        assert_eq!(snapshot(&root()), before);
                    }
                }
            }
            unsafe { std::env::set_var("HGS_STATE_DIR", selected) };
            assert!(orphans >= 2);
            assert!(completed >= 1);
        }
        "scope" => {
            let foreign = root().join("foreign");
            private_dir(&foreign).unwrap();
            let file = lock_file(&foreign.join("writer")).unwrap();
            let wrong = Guard::exclusive(file).unwrap();
            let before = snapshot(&root());
            assert!(Fresh::create_under(&wrong, &current, until()).is_err());
            assert_eq!(snapshot(&root()), before);
            for name in ["native-input", "native-input-reservations"] {
                let path = root().join(name);
                symlink(&foreign, &path).unwrap();
                let before = snapshot(&root());
                assert!(Fresh::create_under(&writer, &current, until()).is_err());
                assert_eq!(snapshot(&root()), before);
                fs::remove_file(path).unwrap();
            }
            publish_new_id(&writer, &mut current).unwrap();
            let original = root().join("old-reservations");
            fs::rename(root().join("native-input-reservations"), &original).unwrap();
            private_dir(&root().join("native-input-reservations")).unwrap();
            let before = snapshot(&root());
            assert!(Fresh::acquire(&current, until()).is_err());
            assert_eq!(snapshot(&root()), before);
        }
        "held" => {
            publish_new_id(&writer, &mut current).unwrap();
            let held = Fresh::acquire(&current, until()).unwrap();
            let path = sid_path(&current);
            let saved = root().join("saved-kind");
            fs::rename(&path, &saved).unwrap();
            fs::copy(&saved, &path).unwrap();
            assert!(held.verify().is_err());
            assert!(lock_until(&input_path(&current), Instant::now())
                .unwrap()
                .is_none());
            drop(held);
            fs::remove_file(&path).unwrap();
            fs::rename(&saved, &path).unwrap();
            let held =
                Fresh::acquire(&current, Instant::now() + Duration::from_millis(50)).unwrap();
            std::thread::sleep(Duration::from_millis(60));
            assert!(held.verify().is_err());
            assert!(lock_until(&input_path(&current), Instant::now())
                .unwrap()
                .is_none());
            drop(held);
            let held = Fresh::acquire(&current, until()).unwrap();
            let mut replaced = current.clone();
            replaced["run_id"] = json!("other");
            write(&mut replaced).unwrap();
            assert!(held.verify().is_err());
            assert!(lock_until(&input_path(&current), Instant::now())
                .unwrap()
                .is_none());
        }
        _ => panic!("unknown private kind control"),
    }
}
