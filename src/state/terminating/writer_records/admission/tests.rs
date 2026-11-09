use super::*;
use serde_json::json;
use std::{
    fs,
    process::{Command, Stdio},
};

const ENTRY: &str = "state::terminating::writer_records::admission::tests::private_probe";
const SESSION: &str = "11111111-1111-4111-8111-111111111111";
const RUN: &str = "22222222-2222-4222-8222-222222222222";
const REQUEST: &str = "33333333-3333-4333-8333-333333333333";
fn end() -> Instant {
    Instant::now() + Duration::from_secs(5)
}
fn scope() -> Run {
    Run::new(SESSION, RUN).unwrap()
}
fn request() -> Request {
    Request::new(REQUEST, Kind::Input, &"a".repeat(64), &"b".repeat(64)).unwrap()
}
fn fixture(id_layout: bool) -> Value {
    let mut value = json!({"version":1,"name":"codex/fixture/original","session_id":SESSION,"run_id":RUN,"conversation_id":"saved-conversation","agent":"codex","pid":4242,"process_start":"saved coarse root stamp","supervisor":{"pid":4343,"start":"saved coarse supervisor stamp"},"pane":"%123","terminal_name":"fixture-terminal","cwd":"/fixture/work","launch_dir":"/fixture/work","hgs":"/fixture/bin/hgs","worktree_owner":{"version":1,"id":"fixture-owner","path":"/fixture/work"}});
    if id_layout {
        value["layout"] = json!("id");
    }
    value
}
fn path(record: &Value) -> PathBuf {
    if record["layout"] == "id" {
        storage::id_record_path(SESSION).unwrap()
    } else {
        storage::label_record_path(record["name"].as_str().unwrap())
    }
}
fn save(path: &Path, record: &Value) {
    storage::atomic(path, &serde_json::to_string(record).unwrap()).unwrap();
}
fn probe(case: &str) {
    let home = tempfile::tempdir().unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap());
    child
        .args([
            ENTRY,
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home.path())
        .env("HGS_CONFIG_DIR", home.path().join("config"))
        .env("HGS_STATE_DIR", home.path().join("state"))
        .env("HGS_TEST_ORIGINAL_ADMISSION", case);
    let (status, out, err) = platform::run_bounded(
        &mut child,
        [64 << 10; 2],
        Instant::now() + Duration::from_secs(15),
        &|| false,
    )
    .unwrap();
    assert!(
        status.success(),
        "{case}: {} {}",
        String::from_utf8_lossy(&out),
        String::from_utf8_lossy(&err)
    );
    assert!(String::from_utf8_lossy(&out).contains("test result: ok. 1 passed; 0 failed;"));
}
#[test]
fn original_request_fields_are_bounded_correlation_not_recoverable_authority() {
    for (id, source, intent) in [
        ("", "a".repeat(64), "b".repeat(64)),
        (
            "00000000-0000-0000-0000-000000000000",
            "a".repeat(64),
            "b".repeat(64),
        ),
        (REQUEST, "A".repeat(64), "b".repeat(64)),
        (REQUEST, "a".repeat(65), "b".repeat(64)),
        (REQUEST, "a".repeat(64), "".into()),
    ] {
        assert!(Request::new(id, Kind::Input, &source, &intent).is_err());
    }
    let record = fixture(false);
    assert!(Binding::from_record(&record).is_ok());
    for (key, value) in [
        ("name", json!(false)),
        ("session_id", json!("not-uuid")),
        ("run_id", json!(null)),
        ("conversation_id", json!(12)),
        ("pid", json!(0)),
        ("supervisor", json!({"pid":4343,"start":false})),
        ("pane", json!("x\n")),
        ("hgs", json!("relative")),
        ("layout", json!(false)),
        ("cwd", json!("x".repeat(4097))),
    ] {
        let mut invalid = record.clone();
        invalid[key] = value;
        assert!(Binding::from_record(&invalid).is_err(), "{key}");
    }
    for field in [
        "session_id",
        "run_id",
        "name",
        "supervisor",
        "pid",
        "pane",
        "conversation_id",
        "terminal_name",
        "hgs",
    ] {
        let mut invalid = record.clone();
        invalid.as_object_mut().unwrap().remove(field);
        assert!(Binding::from_record(&invalid).is_err(), "missing {field}");
    }
    let mut invalid = record.clone();
    invalid["worktree_owner"] = Value::Array(vec![Value::Null; 65]);
    assert!(Binding::from_record(&invalid).is_err());
    let mut deep = Value::Null;
    for _ in 0..10 {
        deep = json!({"nested":deep});
    }
    invalid["worktree_owner"] = deep;
    assert!(Binding::from_record(&invalid).is_err());
    invalid["worktree_owner"] = json!({"large":"x".repeat(8193)});
    assert!(Binding::from_record(&invalid).is_err());
    let mut absent = record.clone();
    absent.as_object_mut().unwrap().remove("worktree_owner");
    let mut null = absent.clone();
    null["worktree_owner"] = Value::Null;
    assert_ne!(
        hash(&Binding::from_record(&absent).unwrap().0, BINDING_BYTES).unwrap(),
        hash(&Binding::from_record(&null).unwrap().0, BINDING_BYTES).unwrap()
    );
}
#[test]
fn original_native_record_and_writer_bind_one_exact_request_and_io_loan() {
    probe("basic");
    probe("id");
}
#[test]
fn metadata_hooks_can_advance_but_original_binding_changes_refuse() {
    probe("fields");
}
#[test]
fn a_record_replaced_during_read_is_not_the_current_native_object() {
    probe("read-race");
}

#[test]
fn a_late_source_change_keeps_the_original_row_without_a_clear_or_retry() {
    probe("late-change");
}

#[test]
fn every_marker_shape_refuses_without_initialization_or_effects() {
    probe("marker");
}
#[test]
fn foreign_or_replaced_writers_owner_and_configuration_never_select_a_new_scope() {
    for case in [
        "foreign-writer",
        "lock-replaced",
        "parent-replaced",
        "owner",
        "configuration",
    ] {
        probe(case);
    }
}
#[test]
fn malformed_missing_oversized_and_linked_records_never_repair_or_fall_back() {
    for case in [
        "missing-record",
        "malformed",
        "large",
        "large-tail",
        "symlink",
        "hardlink",
        "fifo",
        "missing-store",
    ] {
        probe(case);
    }
}
#[test]
fn original_deadline_shared_gate_and_duplicate_uuid_cannot_be_renewed_or_replayed() {
    for case in ["deadline", "gate", "duplicate"] {
        probe(case);
    }
}

#[test]
#[ignore = "exact bounded private reexec; no native agent or installed source"]
fn private_probe() {
    let case = std::env::var("HGS_TEST_ORIGINAL_ADMISSION").unwrap();
    let root = storage::root();
    drop(storage::lock(None).unwrap());
    let id_layout = matches!(case.as_str(), "id" | "parent-replaced");
    let record = fixture(id_layout);
    let at = path(&record);
    if id_layout {
        platform::private_dir(at.parent().unwrap()).unwrap();
    }
    save(&at, &record);
    if case == "missing-store" {
        let guard = storage::writer_until(end()).unwrap().unwrap();
        assert!(OriginalRequest::capture(&guard, &record, request(), end()).is_err());
        assert!(!root.join("terminating-writers").exists());
        return;
    }
    initialize(&scope(), end()).unwrap();
    let guard = storage::writer_until(end()).unwrap().unwrap();
    let until = if case == "deadline" {
        Instant::now() + Duration::from_millis(50)
    } else {
        end()
    };
    let original = OriginalRequest::capture(&guard, &record, request(), until).unwrap();
    match case.as_str() {
        "basic" | "id" => {
            let captured = original.reserve(&guard).unwrap();
            let (source, reservation) = captured.into_parts();
            assert_eq!(source.until, until);
            source.recheck(&guard).unwrap();
            assert_eq!(reservation.entry.request, REQUEST);
            assert_eq!(reservation.entry.intent_sha256, "b".repeat(64));
            assert_eq!(reservation.entry.kind, Kind::Input);
            assert!(digest(&reservation.entry.binding_sha256));
            let original_binding = reservation.entry.binding_sha256.clone();
            let admitted = super::super::io::IoAdmission::reserve(reservation)
                .unwrap_or_else(|_| panic!("same original row"));
            let mut child = Command::new("/bin/cat");
            child
                .env_clear()
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let mut exchange = admitted.attach(child.spawn().unwrap());
            exchange
                .send(b"one original".to_vec(), |_, deadline| {
                    deadline == until && source.recheck(&guard).is_ok()
                })
                .unwrap();
            let mut settled = None;
            let end = end();
            while settled.is_none() {
                assert!(Instant::now() < end);
                settled = exchange.poll();
                std::thread::sleep(Duration::from_millis(2));
            }
            let mut settled = settled.unwrap();
            assert_eq!(settled.report().stdout, b"one original");
            settled.retire(end).unwrap();
            assert!(pending(&scope(), end).unwrap().is_empty());
            let changed_request = Request::new(
                "44444444-4444-4444-8444-444444444444",
                Kind::Input,
                &"c".repeat(64),
                &"b".repeat(64),
            )
            .unwrap();
            let (_, mut changed) = OriginalRequest::capture(&guard, &record, changed_request, end)
                .unwrap()
                .reserve(&guard)
                .unwrap()
                .into_parts();
            assert_ne!(
                changed.entry.binding_sha256, original_binding,
                "the original host source fingerprint must be bound"
            );
            changed.retire(end).unwrap();
        }
        "fields" => {
            let mut metadata = record.clone();
            metadata["phase"] = json!("ended");
            metadata["exit_code"] = json!(0);
            metadata["transcript"] = json!("private-not-selected");
            save(&at, &metadata);
            original.source.recheck(&guard).unwrap();
            for (key, value) in [
                ("name", json!("codex/fixture/stranger")),
                ("session_id", json!(REQUEST)),
                ("run_id", json!(REQUEST)),
                ("conversation_id", json!("replacement")),
                ("agent", json!("claude")),
                ("pid", json!(4444)),
                ("process_start", json!("new stamp")),
                ("supervisor", json!({"pid":4344,"start":"replacement"})),
                ("pane", json!("%124")),
                ("terminal_name", json!("another")),
                ("hgs", json!("/other/hgs")),
                ("launch_dir", json!("/other/work")),
                ("worktree_owner", json!({"id":"replacement"})),
            ] {
                let mut changed = metadata.clone();
                changed[key] = value;
                save(&at, &changed);
                assert!(original.source.recheck(&guard).is_err(), "{key}");
            }
            save(&at, &metadata);
            let (_, mut reservation) = original.reserve(&guard).unwrap().into_parts();
            reservation.retire(end()).unwrap();
        }
        "read-race" => {
            let location = at.clone();
            let mut changed = record.clone();
            changed["phase"] = json!("completed-metadata");
            AFTER_RECORD_READ.with(|slot| {
                *slot.borrow_mut() = Some(Box::new(move || save(&location, &changed)))
            });
            assert!(original.source.recheck(&guard).is_err());
            // A subsequent clean read sees the same logical binding, while no
            // record-race failure is mistaken for permission to replay input.
            original.source.recheck(&guard).unwrap();
            let (_, mut held) = original.reserve(&guard).unwrap().into_parts();
            held.retire(end()).unwrap();
        }
        "late-change" => {
            let location = at.clone();
            let mut changed = record.clone();
            changed["conversation_id"] = json!("late-replacement");
            AFTER_RESERVE.with(|slot| {
                *slot.borrow_mut() = Some(Box::new(move || save(&location, &changed)))
            });
            assert!(original.reserve(&guard).is_err());
            assert_eq!(pending(&scope(), end()).unwrap().len(), 1);
            assert!(super::super::gate::RunWriterGate::acquire(&scope(), end()).is_err());
        }
        "marker" => {
            for value in [Value::Null, json!(false), json!({}), json!("damaged")] {
                let mut changed = record.clone();
                changed["terminating"] = value;
                save(&at, &changed);
                assert!(original.source.recheck(&guard).is_err());
                assert!(OriginalRequest::capture(&guard, &changed, request(), end()).is_err());
            }
            assert!(original.reserve(&guard).is_err());
            assert!(pending(&scope(), end()).unwrap().is_empty());
        }
        "foreign-writer" => {
            let aux = storage::lock(Some(&root.join("aux.lock"))).unwrap();
            assert!(original.source.recheck(&aux).is_err());
            assert!(OriginalRequest::capture(&aux, &record, request(), end()).is_err());
            assert!(original.reserve(&aux).is_err());
            assert!(pending(&scope(), end()).unwrap().is_empty());
        }
        "lock-replaced" => {
            fs::rename(root.join(".lock"), root.join("original-lock")).unwrap();
            storage::atomic(&root.join(".lock"), "").unwrap();
            assert!(original.reserve(&guard).is_err());
            assert!(pending(&scope(), end()).unwrap().is_empty());
        }
        "parent-replaced" => {
            fs::rename(root.join("sessions"), root.join("original-sessions")).unwrap();
            platform::private_dir(&root.join("sessions")).unwrap();
            save(&at, &record);
            assert!(original.reserve(&guard).is_err());
            assert!(pending(&scope(), end()).unwrap().is_empty());
        }
        "owner" => {
            let mut original = original;
            original.source.owner.birth.push('0');
            assert!(original.reserve(&guard).is_err());
            assert!(pending(&scope(), end()).unwrap().is_empty());
        }
        "configuration" => {
            let new = root.with_extension("changed-root");
            std::env::set_var("HGS_STATE_DIR", &new);
            assert!(original.reserve(&guard).is_err());
            assert!(!new.exists());
            std::env::set_var("HGS_STATE_DIR", &root);
            assert!(pending(&scope(), end()).unwrap().is_empty());
        }
        "missing-record" | "malformed" | "large" | "large-tail" | "symlink" | "hardlink"
        | "fifo" => {
            match case.as_str() {
                "missing-record" => fs::remove_file(&at).unwrap(),
                "malformed" => storage::atomic(&at, "{\"version\":").unwrap(),
                "large" => {
                    let mut large = record.clone();
                    large["extra"] = json!("x".repeat(RECORD_BYTES));
                    save(&at, &large);
                }
                "large-tail" => {
                    let mut body = serde_json::to_vec(&record).unwrap();
                    body.extend(std::iter::repeat_n(b' ', RECORD_BYTES));
                    fs::write(&at, body).unwrap();
                }
                "symlink" => {
                    fs::rename(&at, root.join("saved-record")).unwrap();
                    std::os::unix::fs::symlink(root.join("saved-record"), &at).unwrap();
                }
                "hardlink" => fs::hard_link(&at, root.join("extra-link")).unwrap(),
                "fifo" => {
                    use std::os::unix::ffi::OsStrExt;
                    fs::remove_file(&at).unwrap();
                    let p = std::ffi::CString::new(at.as_os_str().as_bytes()).unwrap();
                    assert_eq!(unsafe { libc::mkfifo(p.as_ptr(), 0o600) }, 0);
                }
                _ => unreachable!(),
            }
            assert!(original.reserve(&guard).is_err());
            assert!(pending(&scope(), end()).unwrap().is_empty());
        }
        "deadline" => {
            assert!(OriginalRequest::capture(
                &guard,
                &record,
                request(),
                Instant::now() + Duration::from_secs(31)
            )
            .is_err());
            while Instant::now() < until {
                std::thread::sleep(Duration::from_millis(2));
            }
            assert!(original.reserve(&guard).is_err());
            assert!(pending(&scope(), end()).unwrap().is_empty());
        }
        "gate" => {
            let store = Store::open(&scope(), end()).unwrap();
            let _exclusive = store.exclusive(end()).unwrap();
            let started = Instant::now();
            assert!(original.reserve(&guard).is_err());
            assert!(started.elapsed() < Duration::from_secs(1));
            assert!(pending(&scope(), end()).unwrap().is_empty());
        }
        "duplicate" => {
            let duplicate = OriginalRequest::capture(&guard, &record, request(), end()).unwrap();
            let (_, mut held) = original.reserve(&guard).unwrap().into_parts();
            assert!(duplicate.reserve(&guard).is_err());
            assert_eq!(pending(&scope(), end()).unwrap().len(), 1);
            held.retire(end()).unwrap();
        }
        _ => panic!("unknown private admission fixture"),
    }
}
