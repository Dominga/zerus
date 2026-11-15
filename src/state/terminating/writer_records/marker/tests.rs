use super::*;
use crate::state::{self, storage};
use serde_json::json;
use std::{fs, os::unix::fs::PermissionsExt, process::Command, time::Duration};

const SESSION: &str = "11111111-1111-4111-8111-111111111111";
const RUN: &str = "22222222-2222-4222-8222-222222222222";
const REQUEST: &str = "33333333-3333-4333-8333-333333333333";
const ENTRY: &str = "state::terminating::writer_records::marker::tests::private_probe";

fn fixture() -> Value {
    json!({"version":1,"name":"codex/marker/original","session_id":SESSION,"run_id":RUN,
        "conversation_id":"saved-conversation","agent":"codex","pid":4242,
        "process_start":"saved coarse root stamp","supervisor":{"pid":4343,"start":"saved supervisor stamp"},
        "pane":"%123","terminal_name":"fixture-terminal","cwd":"/fixture/work",
        "launch_dir":"/fixture/work","hgs":"/fixture/bin/hgs"})
}
fn correlation() -> Correlation {
    Correlation {
        terminal_session: "$123".into(),
        root_birth: if cfg!(target_os = "macos") {
            "123:0"
        } else {
            "123"
        }
        .into(),
        supervisor_birth: if cfg!(target_os = "macos") {
            "124:0"
        } else {
            "124"
        }
        .into(),
        source_sha256: "a".repeat(64),
        configuration_sha256: "b".repeat(64),
    }
}
fn probe(case: &str) {
    let root = tempfile::tempdir().unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            ENTRY,
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", root.path())
        .env("HGS_CONFIG_DIR", root.path().join("config"))
        .env("HGS_STATE_DIR", root.path().join("state"))
        .env("HGS_TEST_TYPED_MARKER", case);
    let (status, stdout, stderr) = crate::platform::run_bounded(
        &mut command,
        [64 << 10; 2],
        Instant::now() + Duration::from_secs(15),
        &|| false,
    )
    .unwrap();
    assert!(
        status.success(),
        "{case}: {} {}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    );
    assert!(String::from_utf8_lossy(&stdout).contains("test result: ok. 1 passed; 0 failed;"));
    if case == "drop" {
        command.env("HGS_TEST_TYPED_MARKER", "read-only");
        let (status, _, stderr) = crate::platform::run_bounded(
            &mut command,
            [64 << 10; 2],
            Instant::now() + Duration::from_secs(10),
            &|| false,
        )
        .unwrap();
        assert!(
            status.success(),
            "owner-exit observation: {}",
            String::from_utf8_lossy(&stderr)
        );
    }
}

#[test]
fn a_fresh_original_publishes_one_durable_typed_marker() {
    probe("fresh");
}

#[test]
fn existing_markers_and_changed_originals_cannot_be_adopted() {
    for case in [
        "existing-null",
        "existing-false",
        "existing-object",
        "existing-own",
        "stale-run",
        "stale-conversation",
        "stale-config",
        "completion",
        "lock-replaced",
        "config-replaced",
        "root-replaced",
        "owner",
        "request",
        "deadline",
        "consumed",
        "over-budget",
        "unsafe-record",
        "missing-ledger",
        "state-alias",
    ] {
        probe(case);
    }
}

#[test]
fn staged_source_rechecks_and_uncertain_durability_never_return_ownership() {
    for case in [
        "stage-source",
        "stage-same-record",
        "stage-deadline",
        "stage-parent",
        "sync-unknown",
        "readback-unknown",
        "drop",
        "pending",
        "loan",
        "wait-order",
        "writer-busy",
        "id-layout",
        "write-failure",
        "file-sync-failure",
        "rename-failure",
    ] {
        probe(case);
    }
}

#[test]
fn stored_data_is_strict_bounded_and_cannot_select_a_different_binding() {
    probe("codec");
}

fn load(path: &std::path::Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}
fn save(path: &std::path::Path, value: &Value) {
    storage::atomic(path, &format!("{value}\n")).unwrap();
}
fn absent(path: &std::path::Path) {
    assert!(
        load(path).get("terminating").is_none(),
        "failed admission must not publish"
    );
}
fn failed(original: Original) {
    assert!(
        reserve(original).is_err(),
        "uncertain/stale original must refuse"
    );
}

#[test]
#[ignore = "private literal-environment reexecution entry"]
fn private_probe() {
    let case = std::env::var("HGS_TEST_TYPED_MARKER").unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    let mut record = fixture();
    if case == "state-alias" {
        let selected = storage::root();
        storage::private_dir(&selected).unwrap();
        let alias = selected.with_extension("alias");
        std::os::unix::fs::symlink(&selected, &alias).unwrap();
        std::env::set_var("HGS_STATE_DIR", alias);
    }
    if case == "read-only" {
        let path = storage::legacy_record_path(record["name"].as_str().unwrap());
        let before = fs::read(&path).unwrap();
        let value = load(&path);
        let Observation::Stored(data) = observe(&value) else {
            panic!("stored marker must remain data after owner exit")
        };
        assert_ne!(data.owner_pid, std::process::id());
        assert!(state::terminating::pending(&value));
        let writer = storage::lock(None).unwrap();
        assert!(
            Original::capture(
                &writer,
                &value,
                REQUEST,
                &"c".repeat(64),
                correlation(),
                until
            )
            .is_err(),
            "saved data cannot revive original ownership"
        );
        assert_eq!(fs::read(path).unwrap(), before);
        return;
    }
    if case == "id-layout" {
        record["layout"] = "id".into();
    }
    storage::private_dir(&crate::config::config_dir(&state::home().to_string_lossy())).unwrap();
    let scope = super::super::Run::new(SESSION, RUN).unwrap();
    let writer = storage::lock(None).unwrap();
    // Normal session creation has already initialized its optional index
    // parent. The dormant reservation itself never initializes storage.
    storage::private_dir(&storage::root().join("ids")).unwrap();
    let path = if case == "id-layout" {
        storage::id_record_path(SESSION).unwrap()
    } else {
        storage::own_path(&record).unwrap()
    };
    if case == "id-layout" {
        storage::private_dir(path.parent().unwrap()).unwrap();
    }
    save(&path, &record);
    if case == "missing-ledger" {
        assert!(Original::capture(
            &writer,
            &record,
            REQUEST,
            &"c".repeat(64),
            correlation(),
            until
        )
        .is_err());
        assert!(!storage::root().join("terminating-writers").exists());
        absent(&path);
        return;
    }
    super::super::initialize(&scope, until).unwrap();
    let mut original = Original::capture(
        &writer,
        &record,
        REQUEST,
        &"c".repeat(64),
        correlation(),
        until,
    )
    .unwrap();
    drop(writer);
    match case.as_str() {
        "fresh" | "id-layout" | "drop" | "codec" | "existing-own" | "completion"
        | "state-alias" => {
            if case == "completion" {
                record["last_exit"] = 0.into();
                record["attention"] = json!({"question":"original completion"});
                save(&path, &record);
            }
            let reserved =
                reserve(original).expect("fresh original must publish a durable typed reservation");
            let stored = load(&path);
            assert!(state::terminating::pending(&stored));
            assert_eq!(stored["terminating"]["request_id"], REQUEST);
            assert_eq!(
                stored["terminating"],
                serde_json::to_value(&reserved.data).unwrap()
            );
            assert!(matches!(observe(&stored), Observation::Stored(_)));
            assert!(
                storage::writer_until(Instant::now() + Duration::from_millis(100))
                    .unwrap()
                    .is_some(),
                "reservation must release the machine-wide writer"
            );
            assert!(
                RunWriterGate::acquire(&scope, Instant::now() + Duration::from_millis(50)).is_err(),
                "same original run exclusion must remain held"
            );
            if case == "completion" {
                assert_eq!(stored["last_exit"], 0);
            }
            if case == "codec" {
                codec_controls(&stored);
            }
            drop(reserved);
            assert_eq!(
                load(&path),
                stored,
                "Drop never clears or edits persisted data"
            );
            assert!(
                RunWriterGate::acquire(&scope, until).is_ok(),
                "Drop releases only native exclusion"
            );
            if case == "existing-own" {
                let writer = storage::lock(None).unwrap();
                assert!(
                    Original::capture(
                        &writer,
                        &stored,
                        REQUEST,
                        &"c".repeat(64),
                        correlation(),
                        until
                    )
                    .is_err(),
                    "exact repeat is data, not fresh ownership"
                );
            }
        }
        "existing-null" | "existing-false" | "existing-object" => {
            record["terminating"] = match case.as_str() {
                "existing-null" => Value::Null,
                "existing-false" => false.into(),
                _ => json!({"version":999}),
            };
            save(&path, &record);
            failed(original);
            assert_eq!(load(&path), record);
            assert!(state::terminating::pending(&record));
        }
        "stale-run" | "stale-conversation" | "stale-config" => {
            match case.as_str() {
                "stale-run" => record["run_id"] = REQUEST.into(),
                "stale-conversation" => record["conversation_id"] = "another conversation".into(),
                _ => record["base"] = json!({"permission":"changed"}),
            }
            save(&path, &record);
            failed(original);
            assert_eq!(load(&path), record);
        }
        "owner" | "request" | "deadline" | "consumed" | "over-budget" => {
            match case.as_str() {
                "owner" => original.source.fixture_wrong_owner(),
                "request" => original.request_id = uuid::Uuid::nil().to_string(),
                "deadline" => original.until = Instant::now(),
                "over-budget" => original.until = original.began + Duration::from_secs(31),
                _ => original.consumed.store(true, Ordering::Release),
            }
            failed(original);
            absent(&path);
        }
        "unsafe-record" => {
            fs::set_permissions(&path, fs::Permissions::from_mode(0o666)).unwrap();
            failed(original);
            absent(&path);
        }
        "lock-replaced" => {
            let lock = storage::root().join(".lock");
            fs::rename(&lock, lock.with_extension("previous")).unwrap();
            fs::write(&lock, b"").unwrap();
            fs::set_permissions(&lock, fs::Permissions::from_mode(0o600)).unwrap();
            failed(original);
            absent(&path);
        }
        "config-replaced" | "root-replaced" => {
            let root = if case == "root-replaced" {
                storage::root()
            } else {
                crate::config::config_dir(&state::home().to_string_lossy())
            };
            fs::rename(&root, root.with_extension("previous")).unwrap();
            storage::private_dir(&root).unwrap();
            failed(original);
            if case == "root-replaced" {
                absent(
                    &root
                        .with_extension("previous")
                        .join(path.strip_prefix(&root).unwrap()),
                );
            } else {
                absent(&path);
            }
        }
        "stage-source" | "stage-same-record" | "stage-deadline" | "stage-parent" => {
            let changed = path.clone();
            let which = case.clone();
            let reached = Arc::new(AtomicBool::new(false));
            let signal = reached.clone();
            publication::AFTER_STAGE.with(|slot| {
                *slot.borrow_mut() = Some(Box::new(move || {
                    signal.store(true, Ordering::Release);
                    if which == "stage-source" || which == "stage-same-record" {
                        let mut record = load(&changed);
                        if which == "stage-source" {
                            record["conversation_id"] = "replacement".into();
                        }
                        save(&changed, &record);
                    } else if which == "stage-deadline" {
                        std::thread::sleep(
                            until.saturating_duration_since(Instant::now())
                                + Duration::from_millis(1),
                        );
                    } else {
                        let root = storage::root();
                        fs::rename(&root, root.with_extension("previous")).unwrap();
                        storage::private_dir(&root).unwrap();
                    }
                }))
            });
            failed(original);
            assert!(
                reached.load(Ordering::Acquire),
                "the controlled staging boundary must actually run"
            );
            if case != "stage-parent" {
                absent(&path);
            }
        }
        "sync-unknown" | "readback-unknown" => {
            if case == "sync-unknown" {
                crate::state::repositories::directory::fail_publication_sync(Some(
                    path.parent().unwrap().into(),
                ));
            } else {
                let changed = path.clone();
                publication::AFTER_SYNC.with(|slot| {
                    *slot.borrow_mut() = Some(Box::new(move || {
                        let mut value = load(&changed);
                        value["last_exit"] = 7.into();
                        value["terminating"]["intent_sha256"] = "d".repeat(64).into();
                        save(&changed, &value);
                    }))
                });
            }
            let consumed = original.consumed.clone();
            failed(original);
            crate::state::repositories::directory::fail_publication_sync(None);
            assert!(consumed.load(Ordering::Acquire));
            assert!(
                state::terminating::pending(&load(&path)),
                "possibly published marker must remain"
            );
        }
        "pending" | "loan" | "wait-order" => {
            let writer = storage::lock(None).unwrap();
            let mut prior = super::super::reserve(
                &scope,
                "44444444-4444-4444-8444-444444444444",
                super::super::Kind::Input,
                &"a".repeat(64),
                &"b".repeat(64),
                until,
            )
            .unwrap();
            drop(writer);
            if case == "pending" {
                drop(prior);
                failed(original);
                absent(&path);
            } else if case == "loan" {
                let loan = prior.resources().unwrap();
                assert!(prior.retire(until).is_err());
                original.until = Instant::now() + Duration::from_millis(50);
                original.source.fixture_deadline(original.until);
                failed(original);
                absent(&path);
                drop(loan);
                prior.retire(until).unwrap();
            } else {
                super::super::gate::CONTENDED.store(false, Ordering::SeqCst);
                let worker = std::thread::spawn(move || reserve(original));
                while !super::super::gate::CONTENDED.load(Ordering::SeqCst) {
                    assert!(Instant::now() < until);
                    std::thread::sleep(Duration::from_millis(2));
                }
                let writer = storage::writer_until(Instant::now() + Duration::from_millis(100))
                    .unwrap()
                    .expect("run waiter must not hold machine writer");
                prior.retire(until).unwrap();
                drop(writer);
                let reserved = worker
                    .join()
                    .unwrap()
                    .expect("settled original writer allows same binding");
                assert!(state::terminating::pending(&load(&path)));
                drop(reserved);
            }
        }
        "writer-busy" => {
            let _writer = storage::lock(None).unwrap();
            original.until = Instant::now() + Duration::from_millis(50);
            original.source.fixture_deadline(original.until);
            failed(original);
            absent(&path);
        }
        "write-failure" | "file-sync-failure" | "rename-failure" => {
            publication::FAIL_AT.with(|slot| {
                slot.set(Some(match case.as_str() {
                    "write-failure" => "write",
                    "file-sync-failure" => "file-sync",
                    _ => "rename",
                }))
            });
            failed(original);
            absent(&path);
            assert!(
                !fs::read_dir(path.parent().unwrap())
                    .unwrap()
                    .any(|entry| entry
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".hgs-marker-")),
                "only own staging file must be cleaned"
            );
        }
        _ => panic!("unknown private marker case"),
    }
}

fn codec_controls(stored: &Value) {
    let data = &stored["terminating"];
    for (key, value) in [
        ("version", json!(2)),
        ("phase", json!("complete")),
        ("request_id", json!(uuid::Uuid::nil().to_string())),
        ("owner_pid", json!(0)),
        ("intent_sha256", json!("A".repeat(64))),
        ("unknown", json!(true)),
    ] {
        let mut bad = data.clone();
        bad[key] = value;
        assert!(codec::decode(&bad).is_err(), "invalid {key}");
    }
    let mut foreign = stored.clone();
    foreign["run_id"] = REQUEST.into();
    assert!(matches!(observe(&foreign), Observation::Unrecognized));
    let mut huge = data.clone();
    huge["binding"]["conversation_id"] = "x".repeat(MARKER_BYTES).into();
    assert!(codec::decode(&huge).is_err());
    let mut outer = stored.clone();
    outer["unrelated"] = "x".repeat(RECORD_BYTES).into();
    assert!(matches!(observe(&outer), Observation::Unrecognized));
    assert!(codec::record(br#"{"run_id":"one","run_id":"two"}"#).is_err());
    assert!(codec::record(br#"{"nested":{"x":1,"x":2}}"#).is_err());
    assert!(codec::record(&vec![b' '; RECORD_BYTES + 1]).is_err());
    assert!(codec::record(b"{}{}").is_err());
    assert!(matches!(observe(&fixture()), Observation::Absent));
    assert!(state::terminating::check_mutation(stored).is_err());
}
