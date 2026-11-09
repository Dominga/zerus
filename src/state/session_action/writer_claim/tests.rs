use super::*;
use serde_json::{json, Value};
use std::{
    fs, io,
    os::fd::OwnedFd,
    path::Path,
    process::{Command, ExitStatus, Stdio},
};
use zerus_client_core::run::writer_custody::Admission;

const ENTRY: &str = "state::session_action::writer_claim::tests::private_probe";
const UUID: &str = "33333333-3333-4333-8333-333333333333";
const NAME: &str = "codex/fixture/action-claim";
fn end() -> Instant {
    Instant::now() + Duration::from_secs(5)
}
fn request(action: &str) -> Request {
    let mut request = json!({"request_id":UUID,"action":action,"expected_run_id":"22222222-2222-4222-8222-222222222222","expected_conversation_id":"original-conversation"});
    if action == "rename" {
        request["new_name"] = json!("codex/fixture/renamed");
    }
    if action == "fork" {
        request["tag"] = json!("fixture-child");
    }
    serde_json::from_value(request).unwrap()
}

// The ONLY construction is cfg(test), never a product mint or a recovery API.
fn fresh(action: &str, until: Instant) -> FreshExecution {
    let configured_root = storage::root();
    let root = Arc::new(Directory::open(&platform::canonical(&configured_root).unwrap()).unwrap());
    let directory = Arc::new(root.child("action_receipts").unwrap());
    let request = request(action);
    FreshExecution {
        bound: Bound {
            root,
            directory,
            configured_root,
            owner: Owner::current().unwrap(),
            hash: request_hash(&request, NAME),
            request,
            name: NAME.into(),
            began: Instant::now(),
            until,
        },
        consumed: Arc::new(AtomicBool::new(false)),
    }
}
// A fault-model alias of ONE origin, not another fresh execution. Production
// cannot construct either witness; an aliased/reused origin remains consumed.
fn alias(original: &FreshExecution) -> FreshExecution {
    FreshExecution {
        bound: Bound {
            root: original.bound.root.clone(),
            directory: original.bound.directory.clone(),
            configured_root: original.bound.configured_root.clone(),
            owner: Owner::current().unwrap(),
            request: serde_json::from_value(serde_json::to_value(&original.bound.request).unwrap())
                .unwrap(),
            name: original.bound.name.clone(),
            hash: original.bound.hash.clone(),
            began: original.bound.began,
            until: original.bound.until,
        },
        consumed: original.consumed.clone(),
    }
}
fn run(case: &str, home: &Path) -> (ExitStatus, Vec<u8>, Vec<u8>) {
    // macOS temporary prefixes can contain aliases. Keep the common fixture
    // home canonical; the root-alias controls deliberately add their own.
    let home = fs::canonicalize(home).unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    let row = case == "physical-row";
    command
        .args([
            if row {
                "state::terminating::writer_records::io::tests::private_probe"
            } else {
                ENTRY
            },
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &home)
        .env("HGS_CONFIG_DIR", home.join("config"))
        .env("HGS_STATE_DIR", home.join("state"))
        .env("HGS_TEST_ACTION_CLAIM", case);
    if row {
        command.env("HGS_TEST_WRITER_IO", "echo");
    }
    if case.starts_with("gate-original-") {
        // Independent test observer of the original known RootA lock only.
        // The tested parent's configured alias remains retargeted/refused.
        command.env("HGS_STATE_DIR", home.join("state.original-root"));
    }
    platform::run_bounded(
        &mut command,
        [64 << 10; 2],
        Instant::now() + Duration::from_secs(15),
        &|| false,
    )
    .unwrap()
}
fn reexec(case: &str, home: &Path) {
    let (status, out, err) = run(case, home);
    assert!(
        status.success(),
        "{case}: {} {}",
        String::from_utf8_lossy(&out),
        String::from_utf8_lossy(&err)
    );
    assert!(String::from_utf8_lossy(&out).contains("test result: ok. 1 passed; 0 failed;"));
}
fn probe(case: &str) {
    let home = tempfile::tempdir().unwrap();
    reexec(case, home.path());
}
fn gate(held: bool) {
    reexec(
        if held { "gate-held" } else { "gate-free" },
        storage::root().parent().unwrap(),
    );
}
fn gate_original(held: bool) {
    reexec(
        if held {
            "gate-original-held"
        } else {
            "gate-original-free"
        },
        storage::root().parent().unwrap(),
    );
}
fn path() -> PathBuf {
    storage::root()
        .join("action_receipts")
        .join(format!("{UUID}.json"))
}
fn lock_path() -> PathBuf {
    path().with_extension("lock")
}
fn wait(mut done: impl FnMut() -> bool) {
    let until = end();
    while !done() {
        assert!(Instant::now() < until, "owned claim fixture did not settle");
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn setup() {
    drop(storage::lock(None).unwrap());
    platform::private_dir(&storage::root().join("action_receipts")).unwrap();
}
fn claim_bytes() -> Vec<u8> {
    fs::read(path()).unwrap()
}
fn body() -> Value {
    serde_json::from_slice(&claim_bytes()).unwrap()
}

#[test]
fn only_original_supported_actions_reuse_the_exact_canonical_claim_format() {
    for case in ["pause", "resume", "rename", "fork"] {
        probe(case);
    }
}
#[test]
fn a_consumed_origin_cannot_mint_again_even_when_its_claim_is_missing() {
    probe("consumed");
    probe("missing-after-start");
    probe("invalid-origin");
}
#[test]
fn every_existing_or_ambiguous_receipt_refuses_without_rewriting_or_replay() {
    for case in [
        "old-claimed",
        "old-completed",
        "old-failed",
        "old-uncertain",
        "malformed",
        "oversize",
        "claim-link",
        "claim-hardlink",
        "claim-fifo",
    ] {
        probe(case);
    }
}
#[test]
fn original_validation_hash_kind_owner_and_store_are_not_replaceable_data() {
    for case in [
        "wrong-hash",
        "wrong-owner",
        "configuration",
        "unsupported",
        "oversize-request",
        "invalid-uuid",
        "missing-store",
    ] {
        probe(case);
    }
}
#[test]
fn original_exclusion_refuses_replaced_or_special_lock_evidence() {
    for case in [
        "lock-link",
        "lock-hardlink",
        "lock-fifo",
        "lock-content",
        "lock-mode",
    ] {
        probe(case);
    }
}
#[test]
fn failed_or_late_publication_never_returns_a_live_claim_or_no_start_proof() {
    probe("sync-error");
    probe("late-publication");
    probe("publication-race");
}
#[test]
fn interrupted_publication_keeps_the_original_claim_and_cannot_resume_it() {
    let home = tempfile::tempdir().unwrap();
    let (status, _, _) = run("interrupted-publication", home.path());
    assert_eq!(status.code(), Some(94));
    let at = home
        .path()
        .join("state/action_receipts")
        .join(format!("{UUID}.json"));
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(&at).unwrap()).unwrap()["status"],
        "claimed"
    );
    reexec("after-interruption", home.path());
}
#[test]
fn a_single_original_deadline_bounds_lock_contention_and_is_never_renewed() {
    probe("contended");
    probe("expired");
    probe("long-budget");
}
#[test]
fn every_loan_checks_the_same_native_claim_lock_root_and_owner() {
    for case in [
        "root-replaced",
        "store-replaced",
        "lock-replaced",
        "claim-replaced",
        "claim-changed",
        "claim-removed",
        "claim-mode",
        "loan-owner",
        "loan-config",
    ] {
        probe(case);
    }
}
#[test]
fn actual_helper_exit_and_drop_keep_the_same_native_lock_until_eof_and_all_loans_end() {
    probe("helper-loan");
    probe("extra-loan");
}
#[test]
fn actual_physical_writer_row_retirement_does_not_finalize_the_action_claim() {
    probe("physical-retirement");
}

#[test]
fn configured_native_root_binding_keeps_a_stable_original_alias_usable() {
    probe("alias-stable");
}

#[test]
fn missing_unavailable_and_racing_root_referents_never_initialize_or_reselect() {
    for case in [
        "alias-missing",
        "alias-file",
        "alias-nonprivate",
        "alias-race",
    ] {
        probe(case);
    }
}

#[test]
fn refused_root_rebinding_keeps_the_original_claim_and_existing_lock_loan() {
    probe("alias-loan-kept");
}

#[test]
#[ignore = "bounded private claim-resource reexec; no installed producer or agent"]
fn private_probe() {
    let case = std::env::var("HGS_TEST_ACTION_CLAIM").unwrap();
    if matches!(
        case.as_str(),
        "gate-held" | "gate-free" | "gate-original-held" | "gate-original-free"
    ) {
        let directory =
            Directory::open(&platform::canonical(storage::root().join("action_receipts")).unwrap())
                .unwrap();
        let file = directory
            .existing_private_lock(&format!("{UUID}.lock"))
            .unwrap();
        let result = file.try_lock_exclusive();
        assert_eq!(result.is_err(), case.ends_with("held"));
        if let Err(error) = result {
            assert!(platform::lock_contended(&error));
        }
        return;
    }
    setup();

    if matches!(
        case.as_str(),
        "review-root-alias-before" | "review-root-alias-after"
    ) {
        let selected = storage::root();
        let original_path = selected.with_extension("original-root");
        let foreign_path = selected.with_extension("other-root");
        fs::rename(&selected, &original_path).unwrap();
        platform::private_dir(&foreign_path).unwrap();
        std::os::unix::fs::symlink(&original_path, &selected).unwrap();
        let origin = fresh("pause", end());
        assert_eq!(origin.bound.root.path, original_path);
        let held = if case == "review-root-alias-after" {
            Some(claim(origin).unwrap())
        } else {
            fs::remove_file(&selected).unwrap();
            std::os::unix::fs::symlink(&foreign_path, &selected).unwrap();
            assert!(claim(origin).is_err(), "claim admitted after the SAME configured-root path selected a different native root");
            return;
        };
        fs::remove_file(&selected).unwrap();
        std::os::unix::fs::symlink(&foreign_path, &selected).unwrap();
        assert!(
            held.unwrap().resources().is_err(),
            "loan admitted after the SAME configured-root path selected a different native root"
        );
        return;
    }
    if matches!(
        case.as_str(),
        "alias-stable"
            | "alias-missing"
            | "alias-file"
            | "alias-nonprivate"
            | "alias-race"
            | "alias-loan-kept"
    ) {
        let selected = storage::root();
        let original_path = selected.with_extension("original-root");
        let foreign_path = selected.with_extension("other-root");
        fs::rename(&selected, &original_path).unwrap();
        std::os::unix::fs::symlink(&original_path, &selected).unwrap();
        let origin = fresh("pause", end());
        let original_receipt = original_path
            .join("action_receipts")
            .join(format!("{UUID}.json"));
        if case == "alias-stable" {
            let held = claim(origin).unwrap();
            let before = fs::read(&original_receipt).unwrap();
            let loan = held.resources().unwrap();
            held.recheck().unwrap();
            drop(held);
            gate_original(true);
            drop(loan);
            gate_original(false);
            assert_eq!(fs::read(original_receipt).unwrap(), before);
            return;
        }
        if case == "alias-loan-kept" {
            let held = claim(origin).unwrap();
            let before = fs::read(&original_receipt).unwrap();
            let loan = held.resources().unwrap();
            platform::private_dir(&foreign_path).unwrap();
            fs::remove_file(&selected).unwrap();
            std::os::unix::fs::symlink(&foreign_path, &selected).unwrap();
            assert!(held.recheck().is_err() && held.resources().is_err());
            drop(held);
            gate_original(true);
            assert_eq!(fs::read(&original_receipt).unwrap(), before);
            assert_eq!(fs::read_dir(&foreign_path).unwrap().count(), 0);
            drop(loan);
            gate_original(false);
            assert_eq!(fs::read(original_receipt).unwrap(), before);
            return;
        }
        if case == "alias-file" {
            storage::atomic(&foreign_path, "unavailable-directory").unwrap();
        } else if matches!(case.as_str(), "alias-race" | "alias-nonprivate") {
            platform::private_dir(&foreign_path).unwrap();
            if case == "alias-nonprivate" {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&foreign_path, fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        if case == "alias-race" {
            let selected = selected.clone();
            let foreign = foreign_path.clone();
            AFTER_ROOT_REFERENT.with(|slot| {
                *slot.borrow_mut() = Some(Box::new(move || {
                    fs::remove_file(&selected).unwrap();
                    std::os::unix::fs::symlink(&foreign, &selected).unwrap();
                }))
            });
        } else {
            fs::remove_file(&selected).unwrap();
            std::os::unix::fs::symlink(&foreign_path, &selected).unwrap();
        }
        assert!(claim(origin).is_err());
        assert!(!original_receipt.exists());
        if case == "alias-missing" {
            assert!(!foreign_path.exists());
        } else if case == "alias-file" {
            assert_eq!(fs::read(&foreign_path).unwrap(), b"unavailable-directory");
        } else {
            assert_eq!(fs::read_dir(&foreign_path).unwrap().count(), 0);
        }
        assert!(fs::read_dir(original_path.join("action_receipts"))
            .unwrap()
            .next()
            .is_none());
        return;
    }
    let action = if ["pause", "resume", "rename", "fork"].contains(&case.as_str()) {
        case.as_str()
    } else {
        "pause"
    };
    let until = if matches!(case.as_str(), "contended" | "expired" | "late-publication") {
        Instant::now() + Duration::from_millis(120)
    } else {
        end()
    };
    let mut original = fresh(action, until);
    let shadow = alias(&original);
    if matches!(case.as_str(), "pause" | "resume" | "rename" | "fork") {
        let expected = serde_json::to_value(&original.bound.request).unwrap();
        let hash = original.bound.hash.clone();
        let held = claim(original).unwrap();
        assert_eq!(held.request_id(), UUID);
        assert_eq!(held.request_hash(), hash);
        assert_eq!(held.deadline(), until);
        assert_eq!(
            held.kind().unwrap(),
            match case.as_str() {
                "pause" => ActionKind::Pause,
                "resume" => ActionKind::Resume,
                "rename" => ActionKind::Rename,
                _ => ActionKind::Fork,
            }
        );
        let value = body();
        let mut canonical = receipt(
            &serde_json::from_value(expected).unwrap(),
            NAME,
            &hash,
            "claimed",
        );
        canonical["at"] = value["at"].clone();
        assert_eq!(value, canonical);
        assert!(value["at"].as_f64().unwrap().is_finite());
        gate(true);
        assert!(claim(shadow).is_err());
        drop(held);
        gate(false);
        assert_eq!(body(), value);
        return;
    }
    if matches!(
        case.as_str(),
        "consumed" | "missing-after-start" | "invalid-origin"
    ) {
        if case == "invalid-origin" {
            original.bound.hash = "0".repeat(64);
            assert!(claim(original).is_err());
            assert!(!path().exists());
        } else {
            let held = claim(original).unwrap();
            drop(held);
            if case == "missing-after-start" {
                fs::remove_file(path()).unwrap();
            }
        }
        assert!(
            claim(shadow).is_err(),
            "absence cannot reconstruct a consumed origin"
        );
        assert_eq!(path().exists(), case == "consumed");
        return;
    }
    if case.starts_with("old-")
        || matches!(
            case.as_str(),
            "malformed" | "oversize" | "after-interruption"
        )
    {
        if case != "after-interruption" {
            let mut old = receipt(
                &original.bound.request,
                NAME,
                &original.bound.hash,
                case.strip_prefix("old-").unwrap_or("claimed"),
            );
            if case == "oversize" {
                old["error"] = json!("x".repeat(RECEIPT_BYTES));
            }
            let bytes = if case == "malformed" {
                "{broken".into()
            } else {
                old.to_string()
            };
            storage::atomic(&path(), &bytes).unwrap();
        }
        let old = claim_bytes();
        assert!(claim(original).is_err());
        assert_eq!(claim_bytes(), old);
        return;
    }
    if matches!(
        case.as_str(),
        "claim-link"
            | "claim-hardlink"
            | "claim-fifo"
            | "lock-link"
            | "lock-hardlink"
            | "lock-fifo"
            | "lock-content"
            | "lock-mode"
    ) {
        let at = if case.starts_with("lock-") {
            lock_path()
        } else {
            path()
        };
        let extra = at.with_extension("fixture");
        storage::atomic(&extra, "").unwrap();
        if case.ends_with("link") && !case.ends_with("hardlink") {
            std::os::unix::fs::symlink(&extra, &at).unwrap();
        } else if case.ends_with("hardlink") {
            fs::hard_link(&extra, &at).unwrap();
        } else if case.ends_with("fifo") {
            use std::os::unix::ffi::OsStrExt;
            let name = std::ffi::CString::new(at.as_os_str().as_bytes()).unwrap();
            assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        } else {
            storage::atomic(
                &at,
                if case == "lock-content" {
                    "unexpected"
                } else {
                    ""
                },
            )
            .unwrap();
            if case == "lock-mode" {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&at, fs::Permissions::from_mode(0o400)).unwrap();
            }
        }
        assert!(claim(original).is_err());
        assert!(!path().exists() || !case.starts_with("lock-"));
        return;
    }
    match case.as_str() {
        "wrong-hash" => original.bound.hash = "0".repeat(64),
        "wrong-owner" => original.bound.owner.birth.push('0'),
        "configuration" => {
            std::env::set_var("HGS_STATE_DIR", storage::root().with_extension("changed"))
        }
        "unsupported" => {
            original.bound.request.action = "terminate".into();
            original.bound.hash = request_hash(&original.bound.request, NAME);
        }
        "oversize-request" => {
            original.bound.request.expected_conversation_id = "x".repeat(INPUT_LIMIT);
            original.bound.hash = request_hash(&original.bound.request, NAME);
        }
        "invalid-uuid" => {
            original.bound.request.request_id = "invalid".into();
            original.bound.hash = request_hash(&original.bound.request, NAME);
        }
        "missing-store" => fs::rename(
            storage::root().join("action_receipts"),
            storage::root().join("saved-receipts"),
        )
        .unwrap(),
        "expired" => wait(|| Instant::now() >= until),
        "long-budget" => original.bound.until = original.bound.began + Duration::from_secs(31),
        _ => {}
    }
    if [
        "wrong-hash",
        "wrong-owner",
        "configuration",
        "unsupported",
        "oversize-request",
        "invalid-uuid",
        "missing-store",
        "expired",
        "long-budget",
    ]
    .contains(&case.as_str())
    {
        assert!(claim(original).is_err());
        assert!(!path().exists());
        return;
    }
    if case == "contended" {
        let file = original
            .bound
            .directory
            .open_private_lock(&original.bound.lock_leaf())
            .unwrap();
        file.try_lock_exclusive().unwrap();
        let began = Instant::now();
        assert!(claim(original).is_err());
        assert!(began.elapsed() < Duration::from_secs(2));
        assert!(!path().exists());
        drop(file);
        assert!(claim(shadow).is_err());
        return;
    }
    if [
        "sync-error",
        "late-publication",
        "interrupted-publication",
        "publication-race",
    ]
    .contains(&case.as_str())
    {
        if case == "sync-error" {
            crate::state::repositories::directory::fail_publication_sync(Some(
                original.bound.directory.path.clone(),
            ));
        }
        if case == "publication-race" {
            AFTER_ABSENCE.with(|slot| {
                *slot.borrow_mut() = Some(Box::new(|| {
                    storage::atomic(&path(), "foreign-before-publish").unwrap()
                }))
            });
        } else if case != "sync-error" {
            let crash = case == "interrupted-publication";
            AFTER_PUBLICATION.with(|slot| {
                *slot.borrow_mut() = Some(Box::new(move || {
                    if crash {
                        std::process::exit(94);
                    }
                    wait(|| Instant::now() >= until);
                }))
            });
        }
        assert!(claim(original).is_err());
        crate::state::repositories::directory::fail_publication_sync(None);
        assert!(claim(shadow).is_err());
        if case == "publication-race" {
            assert_eq!(claim_bytes(), b"foreign-before-publish");
        } else {
            assert_eq!(body()["status"], "claimed");
        }
        return;
    }
    let mut held = claim(original).unwrap();
    if [
        "root-replaced",
        "store-replaced",
        "lock-replaced",
        "claim-replaced",
        "claim-changed",
        "claim-removed",
        "claim-mode",
        "loan-owner",
        "loan-config",
    ]
    .contains(&case.as_str())
    {
        let old = claim_bytes();
        let root = storage::root();
        match case.as_str() {
            "root-replaced" => {
                fs::rename(&root, root.with_extension("original")).unwrap();
                platform::private_dir(&root).unwrap();
            }
            "store-replaced" => {
                fs::rename(root.join("action_receipts"), root.join("original-receipts")).unwrap();
                platform::private_dir(&root.join("action_receipts")).unwrap();
            }
            "lock-replaced" => {
                fs::rename(lock_path(), lock_path().with_extension("original")).unwrap();
                storage::atomic(&lock_path(), "").unwrap();
            }
            "claim-replaced" => {
                storage::atomic(&path(), std::str::from_utf8(&old).unwrap()).unwrap()
            }
            "claim-changed" => fs::write(path(), b"changed in place").unwrap(),
            "claim-removed" => fs::remove_file(path()).unwrap(),
            "claim-mode" => {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(path(), fs::Permissions::from_mode(0o644)).unwrap();
            }
            "loan-owner" => Arc::get_mut(&mut held.original)
                .unwrap()
                .lock
                .bound
                .owner
                .birth
                .push('0'),
            "loan-config" => std::env::set_var("HGS_STATE_DIR", root.with_extension("other")),
            _ => unreachable!(),
        }
        assert!(held.recheck().is_err());
        assert!(held.resources().is_err());
        return;
    }
    if case == "physical-retirement" {
        let old = claim_bytes();
        reexec("physical-row", storage::root().parent().unwrap());
        let row=storage::root().join("terminating-writers/11111111-1111-4111-8111-111111111111/22222222-2222-4222-8222-222222222222/pending.json");
        assert_eq!(
            serde_json::from_slice::<Value>(&fs::read(row).unwrap()).unwrap()["pending"],
            json!([])
        );
        assert_eq!(claim_bytes(), old);
        assert_eq!(body()["status"], "claimed");
        held.recheck().unwrap();
        gate(true);
        return;
    }
    if matches!(case.as_str(), "helper-loan" | "extra-loan") {
        let old = claim_bytes();
        let weak = Arc::downgrade(&held.original);
        let extra = if case == "extra-loan" {
            Some(held.resources().unwrap())
        } else {
            None
        };
        let admission = Admission::reserve(held.resources().unwrap(), until).unwrap();
        let (read, write) = io::pipe().unwrap();
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "read line"])
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::from(write.try_clone().unwrap()))
            .stderr(Stdio::piped());
        let mut child = command.spawn().unwrap();
        let pid = child.id();
        let birth = platform::process_identity(pid).unwrap().birth;
        child.stdout = Some(OwnedFd::from(read).into());
        drop(command);
        let mut exchange = admission.attach(child);
        exchange
            .send(b"finish\n".to_vec(), |original, deadline| {
                original.id() == pid && deadline == until && held.recheck().is_ok()
            })
            .unwrap();
        drop(held);
        wait(|| {
            assert!(exchange.poll().is_none());
            platform::process_identity(pid).is_none_or(|now| now.birth != birth)
        });
        gate(true);
        drop(exchange);
        gate(true);
        assert!(weak.strong_count() > 0);
        drop(write);
        wait(|| weak.strong_count() == usize::from(extra.is_some()));
        if extra.is_some() {
            gate(true);
        }
        drop(extra);
        wait(|| weak.strong_count() == 0);
        gate(false);
        assert_eq!(claim_bytes(), old);
        assert_eq!(body()["status"], "claimed");
        return;
    }
    panic!("unknown private claim case");
}

#[test]
fn review1929_configured_root_alias_cannot_be_retargeted_before_claim() {
    let home = tempfile::tempdir().unwrap();
    reexec("review-root-alias-before", home.path());
}
#[test]
fn review1929_configured_root_alias_cannot_be_retargeted_before_loan() {
    let home = tempfile::tempdir().unwrap();
    reexec("review-root-alias-after", home.path());
}
