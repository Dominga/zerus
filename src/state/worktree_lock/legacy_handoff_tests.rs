//! Real Git vetoes, exact holder snapshots, publication cuts and rollback.
use super::*;
use crate::platform::lock_tests as probe;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::process::Command;

const FIXTURE: &str = "state::worktree_lock::tests::fixture";

#[test]
fn v2_shared_handoff_keeps_the_original_git_veto_until_the_last_holder_leaves() {
    probe::control(FIXTURE, "v2_shared");
}

#[test]
fn v2_shared_handoff_binds_native_provider_and_id_holders() {
    for case in [
        "v2_shared_dsh",
        "v2_shared_opencode",
        "v2_shared_marked",
        "v2_shared_id",
    ] {
        probe::control(FIXTURE, case);
    }
}

#[test]
fn v2_shared_handoff_cuts_keep_an_unfinished_proof_review_only() {
    for cut in 0..=6 {
        probe::control(FIXTURE, &format!("v2_shared_cut_{cut}"));
    }
}

#[test]
fn v2_shared_handoff_refuses_changed_originals_without_rewriting_them() {
    for case in [
        "nil",
        "relock",
        "relink",
        "mode",
        "holder",
        "holder_replace",
        "veto",
        "unlock",
        "proof",
        "fence",
        "intent_sync",
        "nonce",
        "checkout",
        "dsh_busy",
        "opencode_busy",
        "dsh_alias",
        "opencode_alias",
        "reason_veto",
        "reason_holder",
        "ready_holder",
        "ready_holder_no_cut",
        "mark_sync",
        "sync",
    ] {
        probe::control(FIXTURE, &format!("v2_shared_refuse_{case}"));
    }
}

fn git(main: &Path, args: &[&str], checkout: &Path) -> std::process::Output {
    Command::new("git")
        .args(args)
        .arg(checkout)
        .current_dir(main)
        .output()
        .unwrap()
}

fn assert_git_veto(main: &Path, checkout: &Path, veto: &Path, object: (u64, u64, u32)) {
    let now = fs::metadata(veto).unwrap();
    assert_eq!(object, (now.dev(), now.ino(), now.mode()));
    let removed = git(main, &["worktree", "remove"], checkout);
    assert!(
        !removed.status.success(),
        "an unlocked gap let Git remove the private checkout"
    );
    assert!(
        String::from_utf8_lossy(&removed.stderr).contains("locked"),
        "{}",
        String::from_utf8_lossy(&removed.stderr)
    );
    assert!(checkout.is_dir());
}

pub(super) fn run(case: &str, main: &Path, checkout: &Path, record: &Value, veto: &Path) {
    fs::set_permissions(root(), fs::Permissions::from_mode(0o700)).unwrap();
    let mut writer = Some(storage::lock(None).unwrap());
    let mut leaving = record.clone();
    let old_path = storage::legacy_record_path(string(record, "name"));
    atomic(&old_path, &record.to_string()).unwrap();
    // This single-case private process lets real Git create the 0644 object.
    // SAFETY: only this private, single-case child changes its process mask.
    let mask = unsafe { libc::umask(0o022) };
    let made = git(
        main,
        &[
            "worktree",
            "lock",
            "--reason",
            &format!("zerus:{}", string(record, "session_id")),
        ],
        checkout,
    );
    unsafe {
        libc::umask(mask);
    }
    assert!(
        made.status.success(),
        "{}",
        String::from_utf8_lossy(&made.stderr)
    );
    let original = fs::metadata(veto).unwrap();
    let object = (original.dev(), original.ino(), original.mode());
    assert_eq!(original.mode() & 0o7777, 0o644);
    drop(Protection::for_legacy_move(&mut leaving).unwrap().unwrap());
    let _pinned_original = fs::File::open(veto).unwrap();
    let mut holder = record.clone();
    let holder_id = if case == "v2_shared_refuse_nil" {
        uuid::Uuid::nil()
    } else {
        uuid::Uuid::new_v4()
    }
    .to_string();
    holder["session_id"] = json!(holder_id);
    holder["run_id"] = json!("holder-run");
    holder["conversation_id"] = json!("holder-conversation");
    let provider = if case.contains("dsh") {
        "dsh"
    } else if case.contains("opencode") {
        "opencode"
    } else {
        "claude"
    };
    holder["name"] = json!(format!("{provider}/example/shared"));
    let id_layout = case == "v2_shared_id" || case == "v2_shared_old";
    let directory = if id_layout {
        holder["layout"] = json!("id");
        holder["worktree_protection"] = record["worktree_owner"]["id"].clone();
        root().join("sessions")
    } else if provider != "claude" {
        holder["backend"] = json!(provider);
        holder["conversation_id"] = json!(if provider == "dsh" {
            "session-native"
        } else {
            "ses_native"
        });
        root().join(provider).join("bindings")
    } else {
        root()
    };
    if case == "v2_shared_marked" {
        holder["worktree_protection"] = record["worktree_owner"]["id"].clone();
    }
    let binding = directory.join(format!(
        "{}.json",
        if id_layout {
            holder_id.clone()
        } else {
            storage::label_key(string(&holder, "name"))
        }
    ));
    atomic(&binding, &holder.to_string()).unwrap();
    fs::remove_file(old_path).unwrap();
    let owner = &record["worktree_owner"];
    let common = PathBuf::from(string(owner, "common_dir"));
    let proof = root()
        .join("worktree-protections")
        .join(format!("{}.json", storage::label_key(string(owner, "id"))));
    let proof_before = fs::read(&proof).unwrap();
    let old_reason = fs::read(veto).unwrap();
    let provider_path = root().join(provider).join("bindings.lock");
    let blocked = if case.ends_with("_busy") {
        Some(storage::lock(Some(&provider_path)).unwrap())
    } else {
        None
    };
    let refusal = case.strip_prefix("v2_shared_refuse_");
    match refusal {
        Some("intent_sync") => {
            repositories::directory::fail_publication_sync(Some(proof.parent().unwrap().into()))
        }
        Some("mark_sync") => {
            repositories::directory::fail_publication_sync(Some(binding.parent().unwrap().into()))
        }
        Some("relock") => {
            assert!(git(main, &["worktree", "unlock"], checkout)
                .status
                .success());
            assert!(git(
                main,
                &[
                    "worktree",
                    "lock",
                    "--reason",
                    &format!("zerus:{}", string(record, "session_id"))
                ],
                checkout
            )
            .status
            .success());
            let now = fs::metadata(veto).unwrap();
            assert_ne!((object.0, object.1), (now.dev(), now.ino()));
        }
        Some("relink") => {
            let alias = veto.with_extension("alias");
            fs::hard_link(veto, &alias).unwrap();
            fs::remove_file(alias).unwrap();
        }
        Some("mode") => {
            fs::set_permissions(veto, fs::Permissions::from_mode(0o600)).unwrap();
            fs::set_permissions(veto, fs::Permissions::from_mode(0o644)).unwrap();
        }
        Some(
            "holder"
            | "holder_replace"
            | "veto"
            | "unlock"
            | "proof"
            | "fence"
            | "nonce"
            | "checkout"
            | "dsh_alias"
            | "opencode_alias"
            | "reason_veto"
            | "reason_holder"
            | "ready_holder"
            | "ready_holder_no_cut"
            | "sync",
        ) => {
            let (binding, veto, proof, checkout, provider_path, common) = (
                binding.clone(),
                veto.to_owned(),
                proof.clone(),
                checkout.to_owned(),
                provider_path.clone(),
                common.clone(),
            );
            let action = refusal.unwrap().to_owned();
            let label = if action.starts_with("ready_holder") {
                "v2-ready-written"
            } else if action.starts_with("reason_") || action == "sync" {
                "v2-reason-written"
            } else {
                "v2-holder-marked"
            };
            probe::arm(label, move || match action.as_str() {
                "holder" | "reason_holder" | "ready_holder" | "ready_holder_no_cut" => {
                    let mut value: Value =
                        serde_json::from_slice(&fs::read(&binding).unwrap()).unwrap();
                    value["run_id"] = json!("changed-run");
                    fs::write(binding, value.to_string()).unwrap();
                    if action == "ready_holder" {
                        storage::cut::arm(0);
                    }
                }
                "holder_replace" => {
                    let bytes = fs::read_to_string(&binding).unwrap();
                    atomic(&binding, &bytes).unwrap();
                }
                "veto" | "reason_veto" => {
                    let bytes = fs::read_to_string(&veto).unwrap();
                    atomic(&veto, &bytes).unwrap();
                }
                "unlock" => fs::remove_file(veto).unwrap(),
                "fence" => {
                    let path = PathBuf::from(format!("{}.handoff", proof.display()));
                    let bytes = fs::read_to_string(&path).unwrap();
                    atomic(&path, &bytes).unwrap();
                }
                "proof" => {
                    let mut value: Value =
                        serde_json::from_slice(&fs::read(&proof).unwrap()).unwrap();
                    value["pending_holder"] = json!("changed");
                    atomic(&proof, &value.to_string()).unwrap();
                }
                "nonce" => {
                    let path = worktree_lifecycle::receipt_path(&common, &checkout);
                    let mut value: Value =
                        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                    value["id"] = json!(uuid::Uuid::new_v4().to_string());
                    atomic(&path, &value.to_string()).unwrap();
                }
                "checkout" => {
                    fs::rename(&checkout, checkout.with_extension("retained")).unwrap();
                    fs::create_dir(checkout).unwrap();
                }
                "dsh_alias" | "opencode_alias" => atomic(&provider_path, "").unwrap(),
                "sync" => repositories::directory::fail_publication_sync(Some(
                    proof.parent().unwrap().into(),
                )),
                _ => unreachable!(),
            });
        }
        _ => (),
    }
    if refusal.is_none() && !case.starts_with("v2_shared_cut_") {
        for label in ["v2-holder-marked", "v2-reason-written"] {
            let (main, checkout, veto) = (main.to_owned(), checkout.to_owned(), veto.to_owned());
            probe::arm(label, move || {
                assert_git_veto(&main, &checkout, &veto, object)
            });
        }
    }
    let cut = case
        .strip_prefix("v2_shared_cut_")
        .map(|n| n.parse::<usize>().unwrap());
    if let Some(cut) = cut {
        storage::cut::arm(cut);
    }
    let result = release_locked(&leaving);
    storage::cut::disarm();
    repositories::directory::fail_publication_sync(None);
    drop(blocked);
    if refusal.is_some() || cut.is_some_and(|n| n < 6) {
        assert!(
            matches!(&result, Release::Kept(_, reason) if !reason.contains("transferred")),
            "{case}: {result:?}"
        );
        if refusal == Some("unlock") {
            assert!(!veto.exists(), "a manual unlock must never be rebuilt");
        } else {
            let expected = if refusal.is_some_and(|r| {
                r.starts_with("reason_") || r == "sync" || r.starts_with("ready_holder")
            }) || cut.is_some_and(|n| n >= 4)
            {
                format!("zerus:{holder_id}\n").into_bytes()
            } else {
                old_reason
            };
            assert_eq!(
                fs::read(veto).unwrap(),
                expected,
                "{case}: refusal must not rewrite a changed veto"
            );
        }
        let after: Value = serde_json::from_slice(&fs::read(&proof).unwrap()).unwrap();
        if refusal.is_some_and(|r| {
            matches!(
                r,
                "nil" | "relock" | "relink" | "mode" | "dsh_busy" | "opencode_busy" | "intent_sync"
            )
        }) || cut.is_some_and(|n| n <= 1)
        {
            assert_eq!(
                fs::read(&proof).unwrap(),
                proof_before,
                "pre-effect refusal leaves proof unchanged"
            );
        } else if refusal.is_some_and(|r| r.starts_with("ready_holder") || r == "sync")
            || cut == Some(5)
        {
            assert_eq!(
                after["status"], "ready",
                "the separately retained fence vetoes this visible Ready file"
            );
        } else {
            assert_eq!(
                after["status"], "handoff",
                "unknown handoff stays review-only: {case}"
            );
        }
        let pending = PathBuf::from(format!("{}.handoff", proof.display()));
        if !(refusal.is_some_and(|r| {
            matches!(
                r,
                "nil" | "relock" | "relink" | "mode" | "dsh_busy" | "opencode_busy"
            )
        }) || cut == Some(0))
        {
            assert!(
                pending.exists(),
                "{case}: an unfinished handoff retains its independent review fence"
            );
        }
        let marked: Value = serde_json::from_slice(&fs::read(&binding).unwrap()).unwrap();
        assert!(
            protection::existing(&marked).is_err(),
            "{case}: no release authority from an interrupted handoff"
        );
        return;
    }
    assert!(
        matches!(&result, Release::Kept(_, reason) if reason.contains("transferred")),
        "{case}: {result:?}"
    );
    assert_git_veto(main, checkout, veto, object);
    assert!(!PathBuf::from(format!("{}.handoff", proof.display())).exists());
    assert_eq!(
        fs::read_to_string(veto).unwrap(),
        format!("zerus:{holder_id}\n")
    );
    let marked: Value = serde_json::from_slice(&fs::read(&binding).unwrap()).unwrap();
    holder["worktree_protection"] = owner["id"].clone();
    assert_eq!(marked, holder, "only the negative marker may change");
    protection::existing(&marked)
        .unwrap()
        .unwrap()
        .verify()
        .unwrap();
    if case == "v2_shared_old" {
        use sha2::{Digest, Sha256};
        let old = PathBuf::from(
            std::env::var_os("ZERUS_TEST_ROLLBACK_CLI").expect("provide verified rollback CLI"),
        );
        assert_eq!(
            format!("{:x}", Sha256::digest(fs::read(&old).unwrap())),
            "146731d47b4aa7c9787096bb12cf3d9054e0b043194fca245fa5a27f2988bbf5"
        );
        drop(writer.take());
        let reviewed = Command::new(&old)
            .args(["worktrees", "review", "--path"])
            .arg(main)
            .arg("--worktree")
            .arg(checkout)
            .arg("--json")
            .output()
            .unwrap();
        assert!(
            reviewed.status.success(),
            "{}",
            String::from_utf8_lossy(&reviewed.stderr)
        );
        let value: Value = serde_json::from_slice(&reviewed.stdout).unwrap();
        let row = value["worktrees"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["path"] == json!(checkout))
            .unwrap();
        assert_eq!(row["sessions"], json!([]));
        assert_eq!(row["verdict"], "blocked");
        assert!(row["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["code"] == "locked"));
        let removed = Command::new(old)
            .args(["worktrees", "remove", "--path"])
            .arg(checkout)
            .arg("--common-dir")
            .arg(&common)
            .arg("--fingerprint")
            .arg(row["fingerprint"].as_str().unwrap())
            .args(["--automatic", "--json"])
            .output()
            .unwrap();
        assert!(!removed.status.success());
        assert!(String::from_utf8_lossy(&removed.stderr).contains("Locked by Git"));
        assert_git_veto(main, checkout, veto, object);
        writer = Some(storage::lock(None).unwrap());
    }
    fs::remove_file(binding).unwrap();
    assert_eq!(release_locked(&marked), Release::Released(checkout.into()));
    assert!(!veto.exists());
    drop(writer);
}
