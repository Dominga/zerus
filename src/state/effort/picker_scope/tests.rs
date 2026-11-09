use super::*;
use crate::state::storage::{atomic, id_record_path, lock, owner_transfer, publish_new_id};
use fs2::FileExt as _;
use std::{
    cell::Cell,
    os::unix::fs::symlink,
    process::{Command, Stdio},
};

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod helper;

fn control(case: &str) {
    let private = tempfile::tempdir().unwrap();
    let output = tempfile::tempfile().unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", private.path())
        .env("HGS_CONFIG_DIR", private.path().join("config"))
        .env("HGS_STATE_DIR", private.path().join("state"))
        .env("TMPDIR", private.path())
        .env("ZERUS_PICKER_SCOPE_CASE", case)
        .args([
            "--exact",
            "state::effort::picker_scope::tests::fixture",
            "--ignored",
            "--test-threads=1",
        ])
        .stdin(Stdio::null())
        .stdout(output.try_clone().unwrap())
        .stderr(output.try_clone().unwrap());
    let mut child = command.spawn().unwrap();
    let until = Instant::now() + Duration::from_secs(20);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= until {
            let _ = child.kill();
            let _ = child.wait();
            panic!("original private picker fixture blocked: {case}");
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let mut bytes = Vec::new();
    use std::io::{Read, Seek};
    let mut output = output;
    output.rewind().unwrap();
    output.read_to_end(&mut bytes).unwrap();
    assert!(
        status.success(),
        "{case}: {}",
        String::from_utf8_lossy(&bytes)
    );
}

fn each(case: &str) {
    for kind in ["fresh", "moved"] {
        control(&format!("{kind}:{case}"));
    }
}

#[test]
fn both_kinds_hold_the_original_kernel_exclusion_through_all_effects() {
    each("effects");
}
#[test]
fn exact_empty_conversation_does_not_establish_native_readiness() {
    each("empty");
}
#[test]
fn a_possible_send_never_allows_a_cleanup_key_or_final_write_after_refusal() {
    each("sent-error");
    each("sent-change");
}
#[test]
fn the_original_deadline_is_not_renewed_by_nested_keys_or_commits() {
    control("fresh:clock");
}
#[test]
fn native_refusal_before_delivery_is_sticky_without_releasing_exclusion() {
    each("native-refused");
}
#[test]
fn every_native_target_field_is_checked_again_after_the_native_observation() {
    for key in [
        "session_id",
        "layout",
        "name",
        "run_id",
        "conversation_id",
        "terminal_name",
        "pane",
        "pid",
        "process_start",
        "supervisor",
        "layout_transition",
    ] {
        each(&format!("field:{key}"));
    }
}
#[test]
fn changed_originals_refuse_every_later_effect_while_the_kernel_lock_stays_held() {
    for name in [
        "proof", "writer", "inputs", "receipts", "sessions", "root", "record",
    ] {
        each(&format!("held:{name}"));
    }
}
#[test]
fn both_final_commits_have_their_own_fresh_native_barrier() {
    for name in ["record", "receipt"] {
        each(&format!("final:{name}"));
    }
}
#[test]
fn missing_or_irregular_evidence_never_creates_a_fallback() {
    for name in [
        "missing-proof",
        "future-proof",
        "proof-alias",
        "missing-input",
        "input-alias",
        "missing-receipts",
        "legacy",
        "stale-record",
        "wrong-writer",
    ] {
        each(&format!("open:{name}"));
    }
    control("absent");
}

fn record() -> Value {
    json!({"version":1,"agent":"claude","name":"claude/example/shared",
        "session_id":uuid::Uuid::new_v4().to_string(),"run_id":uuid::Uuid::new_v4().to_string(),
        "conversation_id":"original-conversation","terminal_name":"private-terminal",
        "pane":"%7","pid":123456789,"process_start":"synthetic-birth",
        "future":{"opaque":[1,2,3]},"model":"before"})
}

fn setup(kind: &str) -> (Guard, Value) {
    let writer = lock(None).unwrap();
    for name in ["sessions", "effort_receipts", "settings_receipts"] {
        private_dir(&root().join(name)).unwrap();
    }
    let mut record = record();
    if kind == "fresh" {
        record["layout"] = json!("id");
        publish_new_id(&writer, &mut record).unwrap();
    } else {
        atomic(
            &crate::state::storage::legacy_record_path(string(&record, "name")),
            &record.to_string(),
        )
        .unwrap();
        record = owner_transfer::begin(&writer, &record, Instant::now() + Duration::from_secs(5))
            .unwrap();
    }
    (writer, record)
}

fn input(record: &Value) -> File {
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(
            root()
                .join("native-input")
                .join(format!("{}.lock", string(record, "session_id"))),
        )
        .unwrap()
}
fn save(record: &Value) {
    atomic(
        &id_record_path(string(record, "session_id")).unwrap(),
        &record.to_string(),
    )
    .unwrap();
}
fn proof(record: &Value) -> PathBuf {
    root()
        .join("native-input-reservations")
        .join(format!("sid-{}.json", string(record, "session_id")))
}
fn refused(scope: &mut Scope<'_>, original: &File) {
    let calls = Cell::new(0);
    assert!(scope
        .key(
            Key::Escape,
            |_| {
                calls.set(calls.get() + 1);
                Ok(())
            },
            |_, _| {
                calls.set(calls.get() + 1);
                Ok(())
            }
        )
        .is_err());
    assert!(scope
        .record_commit(
            |_| {
                calls.set(calls.get() + 1);
                Ok(())
            },
            |_, _, _| {
                calls.set(calls.get() + 1);
                Ok(())
            }
        )
        .is_err());
    assert!(scope
        .receipt_commit(
            |_| {
                calls.set(calls.get() + 1);
                Ok(())
            },
            |_, _| {
                calls.set(calls.get() + 1);
                Ok(())
            }
        )
        .is_err());
    assert_eq!(calls.get(), 0, "a refused scope invokes no later callback");
    assert!(
        original.try_lock_exclusive().is_err(),
        "refusal retains the original kernel exclusion"
    );
}

// Synthetic fixture tree only. Native acquisition itself never enumerates.
fn snapshot() -> Vec<(PathBuf, Vec<u8>)> {
    fn visit(path: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            let m = fs::symlink_metadata(&path).unwrap();
            if m.is_dir() {
                visit(&path, out)
            } else if m.is_file() {
                let bytes = fs::read(&path).unwrap();
                out.push((path, bytes))
            } else {
                out.push((path, b"irregular".to_vec()))
            }
        }
    }
    let mut out = Vec::new();
    visit(&root(), &mut out);
    out.sort();
    out
}

#[test]
#[ignore = "exact bounded private re-exec fixture only"]
fn fixture() {
    let Ok(case) = std::env::var("ZERUS_PICKER_SCOPE_CASE") else {
        return;
    };
    if case == "absent" {
        let unrelated = tempfile::tempfile().unwrap();
        let writer = Guard::held(unrelated);
        assert!(!root().exists());
        assert!(Scope::open(
            &writer,
            &record(),
            Receipts::Settings,
            Instant::now() + Duration::from_secs(3)
        )
        .is_err());
        assert!(!root().exists());
        return;
    }
    let (kind, case) = case.split_once(':').unwrap();
    let (writer, mut current) = setup(kind);
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    if let Some(case) = case.strip_prefix("helper:") {
        helper::fixture(writer, current, case);
        return;
    }
    if case == "empty" {
        current["conversation_id"] = json!("");
        save(&current);
    }
    let original = input(&current);
    let until = Instant::now() + Duration::from_secs(3);
    if let Some(case) = case.strip_prefix("open:") {
        if case == "wrong-writer" {
            let wrong = Guard::held(tempfile::tempfile().unwrap());
            let before = snapshot();
            assert!(Scope::open(&wrong, &current, Receipts::Settings, until).is_err());
            assert_eq!(snapshot(), before);
            return;
        }
        match case {
            "missing-proof" => fs::remove_file(proof(&current)).unwrap(),
            "future-proof" => {
                let mut value: Value =
                    serde_json::from_slice(&fs::read(proof(&current)).unwrap()).unwrap();
                value["version"] = json!(99);
                atomic(&proof(&current), &value.to_string()).unwrap();
            }
            "proof-alias" => {
                let p = proof(&current);
                let kept = p.with_extension("original");
                fs::rename(&p, &kept).unwrap();
                symlink(kept, p).unwrap();
            }
            "missing-input" => fs::remove_file(
                root()
                    .join("native-input")
                    .join(format!("{}.lock", string(&current, "session_id"))),
            )
            .unwrap(),
            "input-alias" => {
                let p = root().join("native-input");
                fs::rename(&p, p.with_extension("original")).unwrap();
                symlink(p.with_extension("original"), p).unwrap();
            }
            "missing-receipts" => fs::remove_dir(root().join("settings_receipts")).unwrap(),
            "legacy" => {
                current.as_object_mut().unwrap().remove("layout");
            }
            "stale-record" => {
                current["run_id"] = json!(uuid::Uuid::new_v4().to_string());
            }
            _ => panic!("unknown fixture"),
        }
        let before = snapshot();
        assert!(Scope::open(&writer, &current, Receipts::Settings, until).is_err());
        assert_eq!(
            snapshot(),
            before,
            "no fallback, namespace creation or evidence repair"
        );
        return;
    }
    let receipt_kind = if case == "effects" {
        Receipts::Effort
    } else {
        Receipts::Settings
    };
    let mut scope = Scope::open(&writer, &current, receipt_kind, until).unwrap();
    assert!(original.try_lock_exclusive().is_err());
    if case == "effects" {
        let mut trace = Vec::new();
        for key in [
            Key::Literal("/effort"),
            Key::Literal("/model"),
            Key::Named("Down"),
            Key::Named("Enter"),
            Key::Escape,
        ] {
            scope
                .key(
                    key,
                    |r| {
                        assert_eq!(r["pane"], "%7");
                        Ok(())
                    },
                    |_, key| {
                        assert!(original.try_lock_exclusive().is_err());
                        trace.push(format!("{key:?}"));
                        Ok(())
                    },
                )
                .unwrap();
        }
        // A nested operation borrows the SAME scope and its original clock.
        fn nested(scope: &mut Scope<'_>, until: Instant) {
            assert_eq!(scope.until, until);
            scope
                .key(Key::Named("s"), |_| Ok(()), |_, _| Ok(()))
                .unwrap();
            assert_eq!(scope.until, until);
        }
        nested(&mut scope, until);
        let opaque = current["future"].clone();
        scope
            .record_commit(
                |_| Ok(()),
                |directory, name, fresh| {
                    assert!(original.try_lock_exclusive().is_err());
                    let mut next = fresh.clone();
                    next["model"] = json!("after");
                    directory.replace_file(name, next.to_string().as_bytes())
                },
            )
            .unwrap();
        scope
            .receipt_commit(
                |_| Ok(()),
                |directory, fresh| {
                    assert!(original.try_lock_exclusive().is_err());
                    assert_eq!(fresh["model"], "after");
                    assert_eq!(fresh["future"], opaque);
                    directory.write_new("original.json", b"{\"status\":\"confirmed\"}")
                },
            )
            .unwrap();
        assert_eq!(trace.len(), 5);
        assert!(scope.may_have_sent());
        drop(scope);
        original.try_lock_exclusive().unwrap();
        return;
    }
    if case == "empty" {
        scope
            .key(
                Key::Named("Enter"),
                |r| {
                    assert_eq!(r["conversation_id"], "");
                    Err("native first-message readiness refused".into())
                },
                |_, _| panic!("exclusion is not readiness"),
            )
            .unwrap_err();
        assert!(!scope.may_have_sent());
        refused(&mut scope, &original);
        return;
    }
    if case == "clock" {
        scope
            .key(Key::Literal("/model"), |_| Ok(()), |_, _| Ok(()))
            .unwrap();
        std::thread::sleep(
            until.saturating_duration_since(Instant::now()) + Duration::from_millis(5),
        );
        refused(&mut scope, &original);
        assert!(scope.may_have_sent());
        return;
    }
    if case == "native-refused" {
        scope
            .key(
                Key::Named("Enter"),
                |_| Err("native panel changed".into()),
                |_, _| panic!("must not send"),
            )
            .unwrap_err();
        refused(&mut scope, &original);
        assert!(!scope.may_have_sent());
        return;
    }
    if case == "sent-error" || case == "sent-change" {
        scope
            .key(
                Key::Named("Enter"),
                |_| Ok(()),
                |_, _| {
                    if case == "sent-change" {
                        current["pane"] = json!("%8");
                        save(&current);
                        Ok(())
                    } else {
                        Err("delivery uncertain".into())
                    }
                },
            )
            .unwrap_err();
        assert!(scope.may_have_sent());
        refused(&mut scope, &original);
        return;
    }
    if let Some(key) = case.strip_prefix("field:") {
        let active = id_record_path(string(&current, "session_id")).unwrap();
        scope
            .key(
                Key::Named("Enter"),
                |_| {
                    if key == "layout_transition" {
                        current[key] = Value::Null
                    } else {
                        current[key] = json!("changed")
                    };
                    atomic(&active, &current.to_string()).unwrap();
                    Ok(())
                },
                |_, _| panic!("changed record cannot receive a key"),
            )
            .unwrap_err();
        assert!(!scope.may_have_sent());
        refused(&mut scope, &original);
        return;
    }
    scope
        .key(Key::Literal("/model"), |_| Ok(()), |_, _| Ok(()))
        .unwrap();
    if let Some(name) = case.strip_prefix("final:") {
        let native = |_: &Value| {
            current["run_id"] = json!(uuid::Uuid::new_v4().to_string());
            save(&current);
            Ok(())
        };
        if name == "record" {
            scope
                .record_commit(native, |_, _, _| panic!("stale final record commit"))
                .unwrap_err();
        } else {
            scope
                .receipt_commit(native, |_, _| panic!("stale final receipt commit"))
                .unwrap_err();
        }
        refused(&mut scope, &original);
        assert!(scope.may_have_sent());
        return;
    }
    if let Some(name) = case.strip_prefix("held:") {
        match name {
            "proof" => {
                let p = proof(&current);
                let bytes = fs::read(&p).unwrap();
                atomic(&p, std::str::from_utf8(&bytes).unwrap()).unwrap();
            }
            "writer" => atomic(&root().join(".lock"), "").unwrap(),
            "record" => save(&current),
            "inputs" | "receipts" | "sessions" => {
                let p = root().join(match name {
                    "inputs" => "native-input",
                    "receipts" => "settings_receipts",
                    _ => "sessions",
                });
                fs::rename(&p, p.with_extension("retained")).unwrap();
                private_dir(&p).unwrap();
            }
            "root" => {
                let p = root();
                fs::rename(&p, p.with_extension("retained")).unwrap();
                private_dir(&p).unwrap();
            }
            _ => panic!("unknown held case"),
        }
        let before = snapshot();
        refused(&mut scope, &original);
        assert_eq!(snapshot(), before);
        assert!(scope.may_have_sent());
        return;
    }
    panic!("unknown fixture: {case}");
}
