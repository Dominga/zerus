use super::*;
use crate::state::storage::owner_transfer::input_domain::Input;
use std::os::unix::fs::symlink;

#[test]
fn completed_input_acquisition_holds_the_same_pre_move_inode_pair() {
    control("input:complete");
}

#[test]
fn an_unrelated_damaged_journal_never_establishes_a_global_input_fence() {
    control("input:unrelated");
}

#[test]
fn same_sid_rename_and_fresh_keep_exclusion_but_changed_targets_refuse() {
    control("input:currentness");
}

#[test]
fn another_sid_with_the_same_label_never_uses_the_original_pair() {
    control("input:twin");
}

#[test]
fn absent_or_malformed_evidence_never_creates_a_namespace() {
    control("input:no-evidence");
}

#[test]
fn missing_or_irregular_evidence_and_original_names_never_get_repaired() {
    for case in [
        "missing-before",
        "missing-complete",
        "bad-before",
        "bad-complete",
        "large-before",
        "large-complete",
        "missing-active",
        "stale-run",
        "foreign-sid",
        "wrong-marker",
        "missing-sid",
        "missing-old",
        "replaced-sid",
        "fifo-sid",
        "symlink-sid",
        "extra-link",
        "changed-writer",
        "alias-input",
        "alias-transaction",
        "expired",
    ] {
        control(&format!("input:{case}"));
    }
}

#[test]
fn held_evidence_replacement_refuses_without_releasing_the_original_exclusion() {
    for name in [
        "before.json",
        "complete.json",
        "writer",
        "sid",
        "active",
        "deadline",
        "transaction-dir",
        "sessions-dir",
        "input-dir",
        "root-dir",
    ] {
        control(&format!("input:held-{name}"));
    }
}

fn names(directory: &Path) -> Vec<PathBuf> {
    let mut result = Vec::new();
    for entry in fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        result.push(entry.path());
        if entry.file_type().unwrap().is_dir() {
            result.extend(names(&entry.path()));
        }
    }
    result.sort();
    result
}

pub(super) fn fixture(data: &Fixture, writer: &Guard, until: Instant, case: &str) {
    if case == "no-evidence" {
        let original = names(&root());
        for marker in [
            None,
            Some(Value::Null),
            Some(json!([])),
            Some(json!({"version":2})),
        ] {
            let mut current = data.original.clone();
            current["layout"] = json!("id");
            if let Some(marker) = marker {
                current[MARKER] = marker;
            }
            assert!(Input::acquire(&current, until).is_err());
            assert_eq!(names(&root()), original);
        }
        let retained = root().with_extension("original-retained");
        fs::rename(root(), &retained).unwrap();
        assert!(Input::acquire(&data.original, until).is_err());
        assert!(!root().exists(), "no-marker refusal must not create a root");
        fs::rename(retained, root()).unwrap();
        return;
    }
    let mut current = begin(writer, &data.original, until).unwrap();
    data.check(&current);
    let state = root();
    let active = id_record_path(&data.sid).unwrap();
    let input = state.join("native-input");
    let old = input.join(format!("{}.lock", data.old));
    let sid = input.join(format!("{}.lock", data.sid));
    let transaction = state.join(DIRECTORY).join(&data.sid);
    let original = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&old)
        .unwrap();
    let original_id = crate::platform::FileIdentity::of(&original).unwrap();

    if case == "currentness" {
        current["name"] = json!("claude/example/renamed");
        current["run_id"] = json!(uuid::Uuid::new_v4().to_string());
        current["conversation_id"] = json!("fresh-conversation");
        current["terminal_name"] = json!("claude/example/renamed~original");
        atomic(&active, &current.to_string()).unwrap();
        let held = Input::acquire(&current, until).unwrap();
        current["activity"] = json!("working");
        current["future_display"] = json!({"changed":true});
        atomic(&active, &current.to_string()).unwrap();
        held.verify().unwrap();
        current["run_id"] = json!(uuid::Uuid::new_v4().to_string());
        atomic(&active, &current.to_string()).unwrap();
        assert!(held.verify().is_err());
        assert!(
            original.try_lock_exclusive().is_err(),
            "failed verification is not physical release"
        );
        drop(held);
        original.try_lock_exclusive().unwrap();
        return;
    }
    if case == "twin" {
        let held = Input::acquire(&current, until).unwrap();
        let mut twin = current.clone();
        twin["session_id"] = json!(uuid::Uuid::new_v4().to_string());
        atomic(
            &id_record_path(string(&twin, "session_id")).unwrap(),
            &twin.to_string(),
        )
        .unwrap();
        assert!(Input::acquire(&twin, until).is_err());
        twin.as_object_mut().unwrap().remove(MARKER);
        let other = startup_choice::driving(&twin).unwrap();
        assert!(original.try_lock_exclusive().is_err());
        held.verify().unwrap();
        drop(other);
        drop(held);
        original.try_lock_exclusive().unwrap();
        return;
    }
    if case == "complete" || case == "unrelated" {
        if case == "unrelated" {
            atomic(
                &state
                    .join(DIRECTORY)
                    .join(&data.other)
                    .join("complete.json"),
                "invalid",
            )
            .unwrap();
        }
        let snapshot = names(&state);
        let held = Input::acquire(&current, until).unwrap();
        held.verify().unwrap();
        assert!(Input::acquire(&current, until).is_err());
        assert!(original.try_lock_exclusive().is_err());
        let other = fs::OpenOptions::new().write(true).open(&sid).unwrap();
        assert!(other.try_lock_exclusive().is_err());
        assert_eq!(
            crate::platform::FileIdentity::of(&other).unwrap(),
            original_id
        );
        assert_eq!(names(&state), snapshot);
        drop(held);
        original.try_lock_exclusive().unwrap();
        assert!(other.try_lock_exclusive().is_err());
        fs2::FileExt::unlock(&original).unwrap();
        other.try_lock_exclusive().unwrap();
        assert_eq!(names(&state), snapshot);
        data.check(&current);
        return;
    }
    if let Some(name) = case.strip_prefix("held-") {
        let budget = if name == "deadline" {
            Instant::now() + Duration::from_secs(1)
        } else {
            until
        };
        let held = Input::acquire(&current, budget).unwrap();
        if name == "deadline" {
            std::thread::sleep(budget.saturating_duration_since(Instant::now()));
        } else if name == "active" {
            current.as_object_mut().unwrap().remove(MARKER);
            atomic(&active, &current.to_string()).unwrap();
        } else if name.ends_with("-dir") {
            let target = match name {
                "transaction-dir" => transaction.clone(),
                "sessions-dir" => state.join("sessions"),
                "input-dir" => input.clone(),
                "root-dir" => state.clone(),
                _ => unreachable!(),
            };
            fs::rename(&target, target.with_extension("held-original")).unwrap();
            fs::create_dir(&target).unwrap();
            fs::set_permissions(&target, fs::Permissions::from_mode(0o700)).unwrap();
        } else {
            let target = match name {
                "writer" => state.join(".lock"),
                "sid" => sid.clone(),
                _ => transaction.join(name),
            };
            let bytes = fs::read(&target).unwrap();
            fs::rename(&target, target.with_extension("held-original")).unwrap();
            atomic(&target, std::str::from_utf8(&bytes).unwrap()).unwrap();
        }
        let snapshot = names(&state);
        assert!(held.verify().is_err());
        assert!(original.try_lock_exclusive().is_err());
        assert_eq!(names(&state), snapshot);
        drop(held);
        original.try_lock_exclusive().unwrap();
        return;
    }
    match case {
        "missing-before" => fs::remove_file(transaction.join("before.json")).unwrap(),
        "missing-complete" => fs::remove_file(transaction.join("complete.json")).unwrap(),
        "bad-before" => atomic(&transaction.join("before.json"), "{}").unwrap(),
        "bad-complete" => atomic(&transaction.join("complete.json"), "{}").unwrap(),
        "large-before" => atomic(
            &transaction.join("before.json"),
            &"x".repeat(JOURNAL_BYTES + 1),
        )
        .unwrap(),
        "large-complete" => atomic(&transaction.join("complete.json"), &"x".repeat(2049)).unwrap(),
        "missing-active" => fs::remove_file(&active).unwrap(),
        "stale-run" => current["run_id"] = json!("another-run"),
        "foreign-sid" => current["session_id"] = json!(uuid::Uuid::new_v4().to_string()),
        "wrong-marker" => current[MARKER]["before_sha256"] = json!("f".repeat(64)),
        "missing-sid" => fs::remove_file(&sid).unwrap(),
        "missing-old" => fs::remove_file(&old).unwrap(),
        "replaced-sid" | "fifo-sid" | "symlink-sid" => {
            fs::rename(&sid, sid.with_extension("original")).unwrap();
            match case {
                "replaced-sid" => atomic(&sid, "").unwrap(),
                "symlink-sid" => symlink(&old, &sid).unwrap(),
                _ => {
                    use std::os::unix::ffi::OsStrExt;
                    let path = std::ffi::CString::new(sid.as_os_str().as_bytes()).unwrap();
                    // SAFETY: private fixture pathname, no existing destination.
                    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
                }
            }
        }
        "extra-link" => fs::hard_link(&old, input.join("unexpected.lock")).unwrap(),
        "changed-writer" => {
            let path = state.join(".lock");
            fs::rename(&path, state.join("original-writer")).unwrap();
            atomic(&path, "").unwrap();
        }
        "alias-input" | "alias-transaction" => {
            let target = if case == "alias-input" {
                input
            } else {
                transaction
            };
            let retained = target.with_extension("original");
            fs::rename(&target, &retained).unwrap();
            symlink(retained, target).unwrap();
        }
        "expired" => {}
        _ => panic!("unknown input fixture: {case}"),
    }
    let snapshot = names(&state);
    let start = Instant::now();
    assert!(Input::acquire(&current, if case == "expired" { start } else { until }).is_err());
    assert!(start.elapsed() < Duration::from_secs(2));
    assert_eq!(
        names(&state),
        snapshot,
        "refusal must not repair/create anything"
    );
    assert_eq!(
        crate::platform::FileIdentity::of(&original).unwrap(),
        original_id
    );
}
