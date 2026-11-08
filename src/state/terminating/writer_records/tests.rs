use super::*;
use serde_json::{json, Value};
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, process::Command, time::Duration};

const SESSION: &str = "11111111-1111-4111-8111-111111111111";
const RUN: &str = "22222222-2222-4222-8222-222222222222";
const REQUEST: &str = "33333333-3333-4333-8333-333333333333";
fn scope() -> Run {
    Run::new(SESSION, RUN).unwrap()
}
fn end() -> Instant {
    Instant::now() + Duration::from_secs(5)
}
fn entry() -> Entry {
    Entry {
        request: REQUEST.into(),
        kind: Kind::Input,
        binding_sha256: "a".repeat(64),
        intent_sha256: "b".repeat(64),
        owner: Owner {
            pid: 42,
            #[cfg(target_os = "linux")]
            birth: "1".into(),
            #[cfg(target_os = "macos")]
            birth: "1:1".into(),
        },
    }
}
fn data() -> Ledger {
    Ledger {
        version: VERSION,
        scope: scope(),
        pending: vec![entry()],
    }
}
fn path() -> PathBuf {
    crate::state::storage::root()
        .join("terminating-writers")
        .join(SESSION)
        .join(RUN)
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

#[test]
fn bounded_closed_pending_data_is_not_a_consent_or_clear_representation() {
    let valid = data().encode().unwrap();
    assert_eq!(
        Ledger::decode(&valid, &scope()).unwrap().pending,
        vec![entry()]
    );
    for field in ["version", "scope", "pending"] {
        let text = std::str::from_utf8(&valid).unwrap();
        let mut value: Value = serde_json::from_slice(&valid).unwrap();
        let duplicate = format!("{{\"{field}\":{},{}", value[field], &text[1..]);
        assert!(Ledger::decode(duplicate.as_bytes(), &scope()).is_err());
        value.as_object_mut().unwrap().remove(field);
        assert!(Ledger::decode(&serde_json::to_vec(&value).unwrap(), &scope()).is_err());
    }
    for bytes in [
        b"".to_vec(),
        vec![b' '; BYTES + 1],
        vec![0xff],
        b"{} trailing".to_vec(),
    ] {
        assert!(Ledger::decode(&bytes, &scope()).is_err());
    }
    for (pointer, value) in [
        ("/version", json!(2)),
        ("/scope/run", json!(SESSION)),
        ("/scope/session", json!(RUN)),
        ("/pending/0/kind", json!("terminate")),
        (
            "/pending/0/request",
            json!("00000000-0000-0000-0000-000000000000"),
        ),
        ("/pending/0/owner/pid", json!(0)),
        ("/pending/0/owner/birth", json!("")),
        ("/pending/0/intent_sha256", json!("B".repeat(64))),
        ("/pending/0/binding_sha256", json!("a".repeat(65))),
    ] {
        let mut changed: Value = serde_json::from_slice(&valid).unwrap();
        *changed.pointer_mut(pointer).unwrap() = value;
        assert!(
            Ledger::decode(&serde_json::to_vec(&changed).unwrap(), &scope()).is_err(),
            "{pointer}"
        );
    }
    let mut repeated = data();
    repeated.pending.push(entry());
    assert!(repeated.encode().is_err());
    let mut overflow = data();
    overflow.pending = (1..=ROWS + 1)
        .map(|id| Entry {
            request: uuid::Uuid::from_u128(id as u128).to_string(),
            ..entry()
        })
        .collect();
    assert!(overflow.encode().is_err());
    for extra in ["consent", "complete", "clear", "delivery", "deadline"] {
        let mut value: Value = serde_json::from_slice(&valid).unwrap();
        value[extra] = json!(true);
        assert!(Ledger::decode(&serde_json::to_vec(&value).unwrap(), &scope()).is_err());
    }
    for pointer in ["/scope", "/pending/0", "/pending/0/owner"] {
        let mut value: Value = serde_json::from_slice(&valid).unwrap();
        value.pointer_mut(pointer).unwrap()["complete"] = json!(true);
        assert!(Ledger::decode(&serde_json::to_vec(&value).unwrap(), &scope()).is_err());
    }
    for (old, duplicate) in [
        ("\"pid\":42", "\"pid\":42,\"pid\":42"),
        (
            "\"kind\":\"input\"",
            "\"kind\":\"input\",\"kind\":\"input\"",
        ),
    ] {
        let text = std::str::from_utf8(&valid).unwrap();
        assert!(text.contains(old));
        assert!(Ledger::decode(text.replace(old, duplicate).as_bytes(), &scope()).is_err());
    }
    for birth in [":::", "0", "-1", "1:1000000", "1:2:3", "01"] {
        let mut value = data();
        value.pending[0].owner.birth = birth.into();
        assert!(value.encode().is_err(), "{birth}");
    }
}

fn probe(case: &str, root: &std::path::Path) {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "state::terminating::writer_records::tests::private_probe",
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", root)
        .env("HGS_CONFIG_DIR", root.join("config"))
        .env("HGS_STATE_DIR", root.join("state"))
        .env("HGS_TEST_WRITER_RECORDS_CASE", case);
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
fn private_reservations_keep_exact_pending_rows_and_native_lease_owners() {
    for case in [
        "missing",
        "basic",
        "dropped",
        "capacity",
        "damaged",
        "private",
        "links",
        "replaced",
        "owner",
        "foreign-owner",
        "expired",
        "lease",
        "borrowed-drop",
        "changed-entry",
        "gate",
        "lock",
        "fifo",
        "write-cut",
    ] {
        let root = tempfile::tempdir().unwrap();
        probe(case, root.path());
    }
}

#[test]
fn a_crashed_original_is_not_reconstructed_from_a_free_lock_or_absent_pid() {
    let root = tempfile::tempdir().unwrap();
    probe("crash", root.path());
    probe("after-crash", root.path());
}

#[test]
#[ignore = "exact private reexec only; no real session or agent"]
fn private_probe() {
    let case = std::env::var("HGS_TEST_WRITER_RECORDS_CASE").unwrap();
    let root = crate::state::storage::root();
    crate::platform::private_dir(&root).unwrap();
    let scope = scope();
    let until = end();
    if case == "missing" {
        assert!(pending(&scope, until).is_err());
        assert!(!root.join("terminating-writers").exists());
        assert!(initialize(&scope, Instant::now()).is_err());
        assert!(!root.join("terminating-writers").exists());
        return;
    }
    if matches!(
        case.as_str(),
        "after-crash" | "lease-busy" | "lease-free" | "lease-free-pending"
    ) {
        if case == "lease-busy" {
            use fs2::FileExt;
            let file = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(path().join("gate"))
                .unwrap();
            let error = file.try_lock_exclusive().unwrap_err();
            assert!(crate::platform::lock_contended(&error));
            assert_eq!(pending(&scope, until).unwrap().len(), 1);
            return;
        }
        if matches!(case.as_str(), "lease-free" | "lease-free-pending") {
            use fs2::FileExt;
            let file = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(path().join("gate"))
                .unwrap();
            file.try_lock_exclusive().unwrap();
            assert_eq!(
                pending(&scope, until).unwrap().len(),
                usize::from(case == "lease-free-pending")
            );
            return;
        }
        assert_eq!(pending(&scope, until).unwrap().len(), 1);
        use fs2::FileExt;
        // This new handle's exclusive lock is genuinely free. It does NOT
        // settle the dead original's pending row or reconstruct a live token.
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path().join("gate"))
            .unwrap();
        file.try_lock_exclusive().unwrap();
        drop(file);
        assert_eq!(pending(&scope, until).unwrap().len(), 1);
        assert!(initialize(&scope, until).is_err());
        assert!(reserve(
            &scope,
            REQUEST,
            Kind::Input,
            &"a".repeat(64),
            &"b".repeat(64),
            until
        )
        .is_err());
        assert_eq!(pending(&scope, until).unwrap().len(), 1);
        return;
    }
    initialize(&scope, until).unwrap();
    match case.as_str() {
        "basic" => {
            assert!(pending(&scope, until).unwrap().is_empty());
            assert!(initialize(&scope, until).is_err());
            let mut owned = reserve_one(until);
            let borrowed = owned.resources().unwrap();
            assert_eq!(pending(&scope, until).unwrap().len(), 1);
            assert!(owned.retire(until).is_err());
            assert_eq!(pending(&scope, until).unwrap().len(), 1);
            drop(borrowed);
            owned.retire(until).unwrap();
            assert!(owned.resources().is_err() && owned.retire(until).is_err());
            assert!(pending(&scope, until).unwrap().is_empty());
        }
        "lease" => {
            let mut owned = reserve_one(until);
            probe("lease-busy", root.parent().unwrap());
            let borrowed = owned.resources().unwrap();
            assert!(owned.retire(until).is_err());
            drop(borrowed);
            owned.retire(until).unwrap();
            probe("lease-free", root.parent().unwrap());
        }
        "borrowed-drop" => {
            let owned = reserve_one(until);
            let borrowed = owned.resources().unwrap();
            drop(owned);
            // I/O still owns the very same lease after the local controller
            // goes away. Its final close is not authority to erase the row.
            probe("lease-busy", root.parent().unwrap());
            drop(borrowed);
            probe("lease-free-pending", root.parent().unwrap());
            assert_eq!(pending(&scope, until).unwrap().len(), 1);
        }
        "dropped" => {
            drop(reserve_one(until));
            assert_eq!(pending(&scope, until).unwrap().len(), 1);
            assert!(reserve(
                &scope,
                REQUEST,
                Kind::Settings,
                &"c".repeat(64),
                &"d".repeat(64),
                until
            )
            .is_err());
            assert_eq!(pending(&scope, until).unwrap()[0].kind, Kind::Input);
        }
        "capacity" => {
            for id in 1..=ROWS {
                drop(
                    reserve(
                        &scope,
                        &uuid::Uuid::from_u128(id as u128).to_string(),
                        Kind::Answer,
                        &"a".repeat(64),
                        &"b".repeat(64),
                        until,
                    )
                    .unwrap(),
                );
            }
            let before = fs::read(path().join("pending.json")).unwrap();
            assert!(reserve(
                &scope,
                REQUEST,
                Kind::Answer,
                &"a".repeat(64),
                &"b".repeat(64),
                until
            )
            .is_err());
            assert_eq!(fs::read(path().join("pending.json")).unwrap(), before);
        }
        "damaged" => {
            let mut owned = reserve_one(until);
            fs::write(path().join("pending.json"), b"{\"version\":1").unwrap();
            let before = fs::read(path().join("pending.json")).unwrap();
            assert!(pending(&scope, until).is_err() && owned.retire(until).is_err());
            assert!(owned.resources().is_err());
            assert_eq!(fs::read(path().join("pending.json")).unwrap(), before);
        }
        "changed-entry" => {
            let mut owned = reserve_one(until);
            let mut value: Value =
                serde_json::from_slice(&fs::read(path().join("pending.json")).unwrap()).unwrap();
            value["pending"][0]["intent_sha256"] = json!("c".repeat(64));
            fs::write(path().join("pending.json"), value.to_string()).unwrap();
            assert!(owned.resources().is_err() && owned.retire(until).is_err());
            assert_eq!(pending(&scope, until).unwrap().len(), 1);
        }
        "gate" => {
            let mut owned = reserve_one(until);
            fs::write(path().join("gate"), b"HGS-WRITER-GATE-2\n").unwrap();
            assert!(owned.resources().is_err() && owned.retire(until).is_err());
            assert!(pending(&scope, until).is_err());
        }
        "lock" => {
            let mut owned = reserve_one(until);
            fs::rename(path().join("lock"), path().join("old-lock")).unwrap();
            fs::write(path().join("lock"), b"").unwrap();
            fs::set_permissions(path().join("lock"), fs::Permissions::from_mode(0o600)).unwrap();
            assert!(owned.resources().is_err() && owned.retire(until).is_err());
            assert_eq!(pending(&scope, until).unwrap().len(), 1);
        }
        "fifo" => {
            use std::os::unix::ffi::OsStrExt;
            let mut owned = reserve_one(until);
            let target = path().join("pending.json");
            fs::rename(&target, path().join("saved-pending")).unwrap();
            let encoded = std::ffi::CString::new(target.as_os_str().as_bytes()).unwrap();
            assert_eq!(unsafe { libc::mkfifo(encoded.as_ptr(), 0o600) }, 0);
            assert!(pending(&scope, until).is_err());
            assert!(owned.resources().is_err() && owned.retire(until).is_err());
        }
        "write-cut" => {
            let mut owned = reserve_one(until);
            crate::state::storage::cut::arm(0);
            assert!(owned.retire(until).is_err());
            assert!(owned.lease.is_some());
            assert_eq!(pending(&scope, until).unwrap().len(), 1);
            crate::state::storage::cut::disarm();
            owned.retire(until).unwrap();
            assert!(pending(&scope, until).unwrap().is_empty());
        }
        "private" => {
            let mut owned = reserve_one(until);
            for mode in [0o644, 0o000, 0o400] {
                fs::set_permissions(path().join("lock"), fs::Permissions::from_mode(mode)).unwrap();
                assert!(pending(&scope, until).is_err());
                assert!(owned.retire(until).is_err());
            }
            fs::set_permissions(path().join("lock"), fs::Permissions::from_mode(0o600)).unwrap();
            fs::set_permissions(
                path().join("pending.json"),
                fs::Permissions::from_mode(0o400),
            )
            .unwrap();
            assert!(owned.retire(until).is_err());
            assert_eq!(pending(&scope, until).unwrap().len(), 1);
        }
        "links" => {
            drop(reserve_one(until));
            let original = path().join("pending.json");
            fs::hard_link(&original, path().join("copy")).unwrap();
            assert!(pending(&scope, until).is_err());
            fs::remove_file(path().join("copy")).unwrap();
            fs::rename(&original, path().join("saved")).unwrap();
            std::os::unix::fs::symlink(path().join("saved"), &original).unwrap();
            assert!(pending(&scope, until).is_err());
        }
        "replaced" => {
            let mut owned = reserve_one(until);
            let old = path().with_extension("original");
            fs::rename(path(), &old).unwrap();
            crate::platform::private_dir(&path()).unwrap();
            for file in ["gate", "lock", "pending.json"] {
                fs::copy(old.join(file), path().join(file)).unwrap();
            }
            assert!(owned.retire(until).is_err());
            assert_eq!(
                fs::read(old.join("pending.json")).unwrap(),
                fs::read(path().join("pending.json")).unwrap()
            );
        }
        "owner" => {
            let mut owned = reserve_one(until);
            owned.entry.owner.birth.push('0');
            assert!(owned.retire(until).is_err());
            assert_eq!(pending(&scope, until).unwrap().len(), 1);
        }
        "foreign-owner" => {
            let mut owned = reserve_one(until);
            // Matching stored correlation data cannot adopt another native
            // writer. The live token must still check its original owner.
            #[cfg(target_os = "linux")]
            {
                owned.entry.owner.birth = "1".into();
            }
            #[cfg(target_os = "macos")]
            {
                owned.entry.owner.birth = "1:1".into();
            }
            assert_ne!(Owner::current().unwrap(), owned.entry.owner);
            fs::write(
                path().join("pending.json"),
                Ledger {
                    version: VERSION,
                    scope: scope.clone(),
                    pending: vec![owned.entry.clone()],
                }
                .encode()
                .unwrap(),
            )
            .unwrap();
            assert!(owned.resources().is_err() && owned.retire(until).is_err());
            assert_eq!(pending(&scope, until).unwrap().len(), 1);
        }
        "expired" => {
            let mut owned = reserve_one(until);
            assert!(owned.retire(Instant::now()).is_err());
            assert_eq!(pending(&scope, until).unwrap().len(), 1);
        }
        "crash" => {
            let _original = reserve_one(until);
            std::process::exit(0); // Kernel releases fds; the original row stays.
        }
        _ => panic!("unknown private writer-record case"),
    }
}
