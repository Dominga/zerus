use super::*;
use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command, time::Duration};

const SESSION: &str = "11111111-1111-4111-8111-111111111111";
const RUN: &str = "22222222-2222-4222-8222-222222222222";
const REQUEST: &str = "33333333-3333-4333-8333-333333333333";
fn scope() -> Run {
    Run::new(SESSION, RUN).unwrap()
}
fn end() -> Instant {
    Instant::now() + Duration::from_secs(5)
}
fn reserve_one(until: Instant) -> Reservation {
    reserve(
        &scope(),
        REQUEST,
        Kind::Input,
        &"a".repeat(64),
        &"b".repeat(64),
        until,
    )
    .unwrap()
}
fn probe(case: &str, home: &Path) {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "state::terminating::writer_records::gate::tests::private_probe",
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home)
        .env("HGS_CONFIG_DIR", home.join("config"))
        .env("HGS_STATE_DIR", home.join("state"))
        .env("HGS_TEST_WRITER_GATE_CASE", case);
    let (status, output, error) = crate::platform::run_bounded(
        &mut command,
        [64 << 10; 2],
        Instant::now() + Duration::from_secs(10),
        &|| false,
    )
    .unwrap();
    assert!(
        status.success(),
        "{case}: {} {}",
        String::from_utf8_lossy(&output),
        String::from_utf8_lossy(&error)
    );
}

#[test]
fn original_run_exclusion_and_state_writer_are_native_resources_not_status_permission() {
    for case in [
        "missing",
        "wait-order",
        "pending",
        "writer-busy",
        "exclusion",
        "expired",
        "owner",
        "owner-birth",
        "configuration",
        "state-replaced",
        "root-replaced",
        "recovery-wrong-writer",
        "recovery-replaced-writer",
        "pending-changed",
    ] {
        let home = tempfile::tempdir().unwrap();
        probe(case, home.path());
    }
}

#[test]
#[ignore = "bounded private reexec; no real agent or installed state"]
fn private_probe() {
    let case = std::env::var("HGS_TEST_WRITER_GATE_CASE").unwrap();
    let root = storage::root();
    crate::platform::private_dir(&root).unwrap();
    let scope = scope();
    let until = end();
    if case == "missing" {
        assert!(RunWriterGate::acquire(&scope, until).is_err());
        assert!(!root.join(".lock").exists());
        assert!(!root.join("terminating-writers").exists());
        return;
    }
    if case == "state-busy" {
        assert!(
            storage::writer_until(Instant::now() + Duration::from_millis(80))
                .unwrap()
                .is_none()
        );
        return;
    }
    if matches!(case.as_str(), "shared-blocked" | "shared-free") {
        let _writer = storage::writer_until(until).unwrap().unwrap();
        let attempt = reserve(
            &scope,
            REQUEST,
            Kind::Input,
            &"a".repeat(64),
            &"b".repeat(64),
            until,
        );
        assert_eq!(attempt.is_ok(), case == "shared-free");
        assert_eq!(
            pending(&scope, until).unwrap().len(),
            usize::from(case == "shared-free")
        );
        return;
    }
    // Normal storage initialization is confined to this private fixture.
    drop(storage::lock(None).unwrap());
    initialize(&scope, until).unwrap();
    match case.as_str() {
        "wait-order" => {
            let _writer = reserve_one(until);
            let copy = scope.clone();
            let worker = std::thread::spawn(move || {
                RunWriterGate::acquire(&copy, Instant::now() + Duration::from_millis(300))
            });
            let stop = end();
            while !CONTENDED.load(std::sync::atomic::Ordering::SeqCst) {
                assert!(Instant::now() < stop);
                std::thread::sleep(Duration::from_millis(2));
            }
            // The native run lease is genuinely contended. Its waiter must
            // leave the normal writer available to original completion hooks.
            let normal = storage::writer_until(Instant::now() + Duration::from_millis(80)).unwrap();
            assert!(normal.is_some());
            drop(normal);
            assert!(worker.join().unwrap().is_err());
            assert_eq!(pending(&scope, until).unwrap().len(), 1);
        }
        "pending" => {
            drop(reserve_one(until));
            let before = pending(&scope, until).unwrap();
            assert!(RunWriterGate::acquire(&scope, until).is_err());
            assert_eq!(pending(&scope, until).unwrap(), before);
        }
        "writer-busy" => {
            let _normal = storage::lock(None).unwrap();
            assert!(
                RunWriterGate::acquire(&scope, Instant::now() + Duration::from_millis(80)).is_err()
            );
            let store = Store::open(&scope, until).unwrap();
            let _exclusive = store.exclusive(until).unwrap();
            assert!(store.pending(until).unwrap().is_empty());
        }
        "exclusion" => {
            let owned = RunWriterGate::acquire(&scope, until).unwrap();
            owned.recheck().unwrap();
            probe("state-busy", root.parent().unwrap());
            let run = owned.release_writer();
            run.recheck().unwrap();
            probe("shared-blocked", root.parent().unwrap());
            drop(run);
            probe("shared-free", root.parent().unwrap());
            // That new writer's Drop retains its row; a free gate cannot
            // reconstruct or erase its original custody.
            assert!(RunWriterGate::acquire(&scope, until).is_err());
        }
        "expired" => {
            let stop = Instant::now() + Duration::from_secs(1);
            let owned = RunWriterGate::acquire(&scope, stop).unwrap();
            assert_eq!(owned.run.until, stop);
            while Instant::now() < owned.run.until {
                std::thread::sleep(Duration::from_millis(2));
            }
            assert!(owned.recheck().is_err());
        }
        "owner" => {
            let mut owned = RunWriterGate::acquire(&scope, until).unwrap();
            owned.run.owner.pid = 0;
            assert!(owned.recheck().is_err());
        }
        "owner-birth" => {
            let mut owned = RunWriterGate::acquire(&scope, until).unwrap();
            owned.run.owner.birth.push('0');
            assert!(owned.recheck().is_err());
        }
        "configuration" => {
            let owned = RunWriterGate::acquire(&scope, until).unwrap();
            let other = tempfile::tempdir().unwrap();
            std::env::set_var("HGS_STATE_DIR", other.path());
            assert!(owned.recheck().is_err());
            assert!(!other.path().join(".lock").exists());
        }
        "state-replaced" => {
            let owned = RunWriterGate::acquire(&scope, until).unwrap();
            fs::rename(root.join(".lock"), root.join("old-lock")).unwrap();
            fs::write(root.join(".lock"), b"").unwrap();
            fs::set_permissions(root.join(".lock"), fs::Permissions::from_mode(0o600)).unwrap();
            assert!(owned.recheck().is_err());
            assert!(owned.release_writer().recheck().is_err());
        }
        "root-replaced" => {
            let owned = RunWriterGate::acquire(&scope, until).unwrap();
            let original = root.with_extension("original");
            fs::rename(&root, &original).unwrap();
            crate::platform::private_dir(&root).unwrap();
            // Preserve the native run directory, gate and state-lock objects:
            // only the original ROOT object changes, despite the same paths.
            for name in ["terminating-writers", ".lock"] {
                fs::rename(original.join(name), root.join(name)).unwrap();
            }
            assert!(owned.recheck().is_err());
        }
        "recovery-wrong-writer" => {
            let original = Store::open(&scope, until).unwrap().state_lock().unwrap();
            let expected = platform::FileIdentity::of(&original).unwrap();
            let wrong = storage::lock(Some(&root.join("other-lock"))).unwrap();
            assert!(storage::recover_run_writer(&wrong, expected, until).is_err());
        }
        "recovery-replaced-writer" => {
            let original = Store::open(&scope, until).unwrap().state_lock().unwrap();
            let expected = platform::FileIdentity::of(&original).unwrap();
            original.try_lock_exclusive().unwrap();
            let writer = storage::Guard::held(original);
            fs::rename(root.join(".lock"), root.join("old-lock")).unwrap();
            fs::write(root.join(".lock"), b"").unwrap();
            fs::set_permissions(root.join(".lock"), fs::Permissions::from_mode(0o600)).unwrap();
            assert!(storage::recover_run_writer(&writer, expected, until).is_err());
        }
        "pending-changed" => {
            let owned = RunWriterGate::acquire(&scope, until).unwrap();
            let ledger = Ledger {
                version: VERSION,
                scope: scope.clone(),
                pending: vec![Entry {
                    request: REQUEST.into(),
                    kind: Kind::Answer,
                    binding_sha256: "a".repeat(64),
                    intent_sha256: "b".repeat(64),
                    owner: Owner::current().unwrap(),
                }],
            };
            let path = root
                .join("terminating-writers")
                .join(SESSION)
                .join(RUN)
                .join("pending.json");
            fs::write(path, ledger.encode().unwrap()).unwrap();
            assert!(owned.recheck().is_err());
        }
        _ => panic!("unknown private gate case"),
    }
}
