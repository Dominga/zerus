//! Real-Git publication/release races, under private state and provider locks.
use super::*;
use crate::platform::lock_tests as hooks;
use std::cell::{Cell, RefCell};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::process::Command;
use std::rc::Rc;

const FIXTURE: &str = "state::worktree_lock::tests::fixture";
const STAGES: &[&str] = &[
    "reference-before-census",
    "reference-after-census",
    "reference-before-intent",
    "reference-after-intent",
    "reference-after-take",
    "reference-before-delete",
    "reference-after-delete",
    "reference-before-proof",
    "reference-after-proof",
];

#[test]
fn provider_arrivals_are_excluded_at_every_final_release_stage() {
    for kind in ["legacy", "v1", "v2"] {
        for provider in ["dsh", "opencode"] {
            for stage in STAGES {
                hooks::control(FIXTURE, &format!("reference_{kind}_{provider}_{stage}"));
            }
        }
    }
}

#[test]
fn committed_and_inflight_providers_keep_the_original_git_veto() {
    for kind in ["legacy", "v1", "v2"] {
        for provider in ["dsh", "opencode"] {
            for mode in ["existing", "busy"] {
                hooks::control(FIXTURE, &format!("reference_{kind}_{provider}_{mode}"));
            }
        }
    }
}

#[test]
fn publication_guards_cover_subfolders_aliases_launch_folders_and_ownership() {
    for provider in ["dsh", "opencode"] {
        for mode in ["subfolder", "alias", "launch", "owner", "invalid", "deep"] {
            hooks::control(FIXTURE, &format!("reference_v2_{provider}_{mode}"));
        }
    }
}

#[test]
fn changed_publication_namespace_keeps_or_restores_the_original_veto() {
    for mode in ["replace-before", "replace-taken"] {
        hooks::control(FIXTURE, &format!("reference_v2_dsh_{mode}"));
    }
}

#[test]
fn shared_handoff_still_allows_a_new_provider_reference() {
    for provider in ["dsh", "opencode"] {
        hooks::control(FIXTURE, &format!("reference_v2_{provider}_handoff"));
    }
}

fn git(main: &Path, args: &[&str], checkout: &Path) -> std::process::Output {
    Command::new("git")
        .current_dir(main)
        .args(args)
        .arg(checkout)
        .output()
        .unwrap()
}

fn provider_record(backend: &str, name: &str, folder: &Path) -> Value {
    json!({"backend":backend,"agent":backend,"name":format!("{backend}/example/{name}"),
        "session_id":uuid::Uuid::new_v4().to_string(),"run_id":"provider-run","created":100,
        "conversation_id":format!("{}{name}",if backend=="dsh" {"session-"} else {"ses_"}),"cwd":folder})
}

fn publish(backend: &str, record: &mut Value) -> Result<()> {
    let _provider = storage::lock(Some(&root().join(backend).join("bindings.lock")))?;
    provider_binding::Provider::named(string(record, "name"))
        .unwrap()
        .save(record)
}

fn binding(record: &Value) -> PathBuf {
    provider_binding::Provider::named(string(record, "name"))
        .unwrap()
        .label_path(string(record, "name"))
}

fn veto_stays(main: &Path, checkout: &Path, veto: &Path, original: (u64, u64, u32)) {
    let current = fs::metadata(veto).unwrap();
    assert_eq!((current.dev(), current.ino(), current.mode()), original);
    let removed = git(main, &["worktree", "remove"], checkout);
    assert!(!removed.status.success());
    assert!(String::from_utf8_lossy(&removed.stderr).contains("locked"));
    assert!(checkout.exists());
}

pub(super) fn run(case: &str, main: &Path, checkout: &Path, original: &Value, veto: &Path) {
    let parts: Vec<_> = case.split('_').collect();
    let (kind, backend, mode) = (parts[1], parts[2], parts[3]);
    let _writer = storage::lock(None).unwrap();
    fs::set_permissions(root(), fs::Permissions::from_mode(0o700)).unwrap();
    let mut leaving = original.clone();
    if kind == "v1" {
        drop(Protection::acquire(&leaving).unwrap().unwrap());
        leaving["worktree_protection"] = leaving["worktree_owner"]["id"].clone();
    } else {
        let mask = unsafe { libc::umask(0o022) };
        let made = git(
            main,
            &[
                "worktree",
                "lock",
                "--reason",
                &format!("zerus:{}", string(&leaving, "session_id")),
            ],
            checkout,
        );
        unsafe {
            libc::umask(mask);
        }
        assert!(made.status.success());
        if kind == "v2" {
            atomic(
                &storage::legacy_record_path(string(&leaving, "name")),
                &leaving.to_string(),
            )
            .unwrap();
            drop(Protection::for_legacy_move(&mut leaving).unwrap().unwrap());
            fs::remove_file(storage::legacy_record_path(string(&leaving, "name"))).unwrap();
        }
    }
    let meta = fs::metadata(veto).unwrap();
    let object = (meta.dev(), meta.ino(), meta.mode());
    let elsewhere = checkout.parent().unwrap().join("unrelated");
    fs::create_dir_all(&elsewhere).unwrap();
    let mut related = provider_record(backend, "related", checkout);
    match mode {
        "subfolder" => {
            let folder = checkout.join("nested");
            fs::create_dir(&folder).unwrap();
            related["cwd"] = json!(folder);
        }
        "alias" => {
            let alias = checkout.parent().unwrap().join("folder-alias");
            std::os::unix::fs::symlink(checkout, &alias).unwrap();
            related["cwd"] = json!(alias);
        }
        "launch" => {
            related["cwd"] = json!(elsewhere);
            related["launch_dir"] = json!(checkout);
        }
        "owner" => {
            related["cwd"] = json!(elsewhere);
            related["worktree_owner"] = leaving["worktree_owner"].clone();
        }
        _ => {}
    }
    if mode == "handoff" {
        let mut remaining = leaving.clone();
        remaining["session_id"] = json!(uuid::Uuid::new_v4().to_string());
        remaining["name"] = json!("claude/example/remaining");
        remaining
            .as_object_mut()
            .unwrap()
            .remove("worktree_protection");
        let path = storage::legacy_record_path(string(&remaining, "name"));
        atomic(&path, &remaining.to_string()).unwrap();
        let (backend, mut arriving) = (backend.to_owned(), related.clone());
        hooks::arm("v2-holder-marked", move || {
            publish(&backend, &mut arriving).unwrap()
        });
        assert!(
            matches!(release_locked(&leaving), Release::Kept(_, reason) if reason.contains("transferred"))
        );
        assert!(binding(&related).exists());
        veto_stays(main, checkout, veto, object);
        let remaining: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        fs::remove_file(path).unwrap();
        assert!(matches!(release_locked(&remaining), Release::Kept(..)));
        veto_stays(main, checkout, veto, object);
        return;
    }
    if mode == "invalid" {
        related["cwd"] = json!({"unreadable":true});
        assert!(
            matches!(publish(backend, &mut related), Err(error) if error.contains("folder is unreadable"))
        );
        assert!(!binding(&related).exists());
        assert_eq!(release_locked(&leaving), Release::Released(checkout.into()));
        return;
    }
    if mode == "deep" {
        let mut folder = PathBuf::from("/");
        for _ in 0..130 {
            folder.push("ancestor");
        }
        related["cwd"] = json!(folder);
        assert!(
            matches!(publish(backend, &mut related), Err(error) if error.contains("coordination bound"))
        );
        assert!(!binding(&related).exists());
        assert!(!root().join("worktree-reference-locks").exists());
        assert_eq!(release_locked(&leaving), Release::Released(checkout.into()));
        return;
    }
    if mode == "existing" || mode == "busy" {
        let publishing = if mode == "busy" {
            worktree_references::Scope::publishing(&related).unwrap()
        } else {
            publish(backend, &mut related).unwrap();
            None
        };
        let start = std::time::Instant::now();
        let release = release_locked(&leaving);
        assert!(matches!(release, Release::Kept(..)), "{release:?}");
        assert!(
            start.elapsed() < std::time::Duration::from_secs(2),
            "release waited for a provider"
        );
        veto_stays(main, checkout, veto, object);
        drop(publishing);
        if mode == "existing" {
            fs::remove_file(binding(&related)).unwrap();
        }
        assert_eq!(release_locked(&leaving), Release::Released(checkout.into()));
        return;
    }
    if mode.starts_with("replace-") {
        let stage = if mode == "replace-before" {
            "reference-after-census"
        } else {
            "reference-after-take"
        };
        let mut related = related.clone();
        let backend = backend.to_owned();
        hooks::arm(stage, move || {
            fs::rename(
                root().join("worktree-reference-locks"),
                root().join("old-reference-locks"),
            )
            .unwrap();
            publish(&backend, &mut related).unwrap();
        });
        assert!(matches!(release_locked(&leaving), Release::Kept(..)));
        veto_stays(main, checkout, veto, object);
        return;
    }
    let stage = STAGES
        .iter()
        .copied()
        .find(|stage| *stage == mode)
        .unwrap_or("reference-after-census");
    let ran = Rc::new(Cell::new(false));
    let observed = ran.clone();
    let publication = Rc::new(RefCell::new(None));
    let committed = publication.clone();
    let (backend, mut attempted, mut unrelated) = (
        backend.to_owned(),
        related.clone(),
        provider_record(backend, "unrelated", &elsewhere),
    );
    hooks::arm(stage, move || {
        observed.set(true);
        let result = publish(&backend, &mut attempted);
        if let Err(error) = &result {
            assert!(error.contains("Folder references are busy"), "{error}");
        }
        assert_eq!(binding(&attempted).exists(), result.is_ok());
        *committed.borrow_mut() = Some(result.is_ok());
        // This uses the same provider and state writer context: only the
        // exact checkout/owner is excluded, not the whole provider or Root.
        publish(&backend, &mut unrelated).unwrap();
    });
    let release = release_locked(&leaving);
    assert!(ran.get(), "the selected release stage did not run");
    if publication.borrow().unwrap() {
        assert!(
            matches!(release, Release::Kept(..)),
            "a committed provider lost its Git veto: {release:?}"
        );
        veto_stays(main, checkout, veto, object);
        fs::remove_file(binding(&related)).unwrap();
        assert_eq!(release_locked(&leaving), Release::Released(checkout.into()));
    } else {
        assert_eq!(release, Release::Released(checkout.into()));
    }
    assert!(!binding(&related).exists());
    assert!(!veto.exists());
    // A later, distinct publication is admitted after release settles. This
    // never recreates the retired Git veto or replays a native request.
    publish(parts[2], &mut related).unwrap();
    fs::remove_file(binding(&related)).unwrap();
    assert!(git(main, &["worktree", "remove"], checkout)
        .status
        .success());
}
