//! Real configured state selection in a fresh process. Resolving a trusted
//! root is data routing, never native confirmation or permission to execute.
use super::*;
use std::{os::unix::fs::PermissionsExt, process::Command, time::Duration};

fn isolated(leaf: &str) -> bool {
    let exact = format!("state::worktree_programs::config_tests::{leaf}");
    const KEY: &str = "ZERUS_TEST_PROGRAM_STATE_CONFIG";
    if std::env::var(KEY).ok().as_deref() == Some(&exact) {
        return false;
    }
    let mut child = Command::new(std::env::current_exe().unwrap());
    child
        .args([&exact, "--exact", "--test-threads=1"])
        .env(KEY, &exact);
    let (status, out, err) = crate::platform::run_bounded(
        &mut child,
        [64 << 10, 64 << 10],
        Instant::now() + Duration::from_secs(10),
        &|| false,
    )
    .unwrap();
    assert!(
        status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&out),
        String::from_utf8_lossy(&err)
    );
    assert!(String::from_utf8_lossy(&out).contains("test result: ok. 1 passed"));
    true
}

fn private(path: &Path) {
    std::fs::create_dir_all(path).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
}
fn environment(root: &Path) {
    for (key, _) in std::env::vars_os().filter(|(key, _)| key.to_string_lossy().starts_with("HGS_"))
    {
        std::env::remove_var(key);
    }
    std::env::remove_var("XDG_STATE_HOME");
    private(&root.join("home"));
    private(&root.join("config"));
    std::env::set_var("HOME", root.join("home"));
    std::env::set_var("HGS_CONFIG_DIR", root.join("config"));
}

#[test]
fn configured_state_sources_resolve_without_changing_identity_or_creating_a_profile() {
    if isolated("configured_state_sources_resolve_without_changing_identity_or_creating_a_profile")
    {
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    for source in ["explicit", "xdg", "home", "local"] {
        let case = root.join(source);
        environment(&case);
        let (selected, expected) = match source {
            "explicit" => {
                let state = case.join("real-state");
                private(&state);
                let alias = case.join("state-alias");
                std::os::unix::fs::symlink(&state, &alias).unwrap();
                std::env::set_var("HGS_STATE_DIR", &alias);
                (alias, state)
            }
            "xdg" => {
                let actual = case.join("xdg-physical");
                private(&actual.join("hgs/sessions"));
                let alias = case.join("xdg-alias");
                std::os::unix::fs::symlink(&actual, &alias).unwrap();
                std::env::set_var("XDG_STATE_HOME", &alias);
                (alias.join("hgs/sessions"), actual.join("hgs/sessions"))
            }
            "home" => {
                let home = case.join("home");
                private(&home.join(".local/state/hgs/sessions"));
                let alias = case.join("home-alias");
                std::os::unix::fs::symlink(&home, &alias).unwrap();
                std::env::set_var("HOME", &alias);
                (
                    alias.join(".local/state/hgs/sessions"),
                    home.join(".local/state/hgs/sessions"),
                )
            }
            _ => {
                let actual = case.join("local-physical");
                private(&actual.join("state/hgs/sessions"));
                let alias = case.join("home/.local");
                std::os::unix::fs::symlink(&actual, &alias).unwrap();
                (
                    alias.join("state/hgs/sessions"),
                    actual.join("state/hgs/sessions"),
                )
            }
        };
        let identity =
            zerus_desktop_migration::configuration(&selected, &case.join("config")).unwrap();
        let config = selected_config().unwrap();
        assert_eq!(config.state, expected);
        assert_eq!(
            std::env::var_os("HGS_STATE_DIR").unwrap(),
            expected.as_os_str()
        );
        assert_eq!(
            zerus_desktop_migration::configuration(&config.state, &config.dir).unwrap(),
            identity
        );
        assert!(!expected.join("worktree-program-stops").exists());
    }
    environment(&root);
    for name in ["missing-state", "file-state", "dangling-state"] {
        let selected = root.join(name);
        if name == "file-state" {
            std::fs::write(&selected, "retained").unwrap();
        } else if name == "dangling-state" {
            std::os::unix::fs::symlink(root.join("absent-target"), &selected).unwrap();
        }
        std::env::set_var("HGS_STATE_DIR", &selected);
        let error = selected_config().err().expect("unavailable selected root");
        assert!(error.contains(selected.to_string_lossy().as_ref()));
        assert!(error.contains("configured session state root"));
    }
    assert!(!root.join("missing-state").exists());
    assert!(!root.join("absent-target").exists());
    assert_eq!(
        std::fs::read_to_string(root.join("file-state")).unwrap(),
        "retained"
    );
    if unsafe { libc::geteuid() } != 0 {
        let denied = root.join("denied-state");
        private(&denied);
        std::fs::set_permissions(&denied, std::fs::Permissions::from_mode(0o0)).unwrap();
        std::env::set_var("HGS_STATE_DIR", &denied);
        let result = selected_config();
        std::fs::set_permissions(&denied, std::fs::Permissions::from_mode(0o700)).unwrap();
        let error = result
            .err()
            .expect("unreadable root refuses without creation");
        assert!(error.contains(denied.to_string_lossy().as_ref()));
        assert!(!denied.join("worktree-program-stops").exists());
    }
    // APFS refuses non-UTF-8 filenames itself. Linux permits the physical
    // name, so also pin the legacy UTF-8 state-reader refusal there.
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::ffi::OsStringExt;
        let physical = root.join(std::ffi::OsString::from_vec(b"state-\xff".to_vec()));
        private(&physical);
        let alias = root.join("utf8-state-alias");
        std::os::unix::fs::symlink(&physical, &alias).unwrap();
        std::env::set_var("HGS_STATE_DIR", &alias);
        assert!(selected_config().err().unwrap().contains("UTF-8"));
        assert!(!physical.join("worktree-program-stops").exists());
    }
}

#[test]
fn an_alias_redirect_cannot_retarget_the_captured_root_or_allow_a_journal_link() {
    if isolated("an_alias_redirect_cannot_retarget_the_captured_root_or_allow_a_journal_link") {
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    environment(&root);
    let original = root.join("original-state");
    let foreign = root.join("foreign-state");
    private(&original);
    private(&foreign);
    std::fs::write(foreign.join("retained"), "foreign data").unwrap();
    let alias = root.join("state-alias");
    std::os::unix::fs::symlink(&original, &alias).unwrap();
    std::env::set_var("HGS_STATE_DIR", &alias);
    let config = selected_config().unwrap();
    let store = journal::Store::open(&config.state, true).unwrap().unwrap();
    std::fs::remove_file(&alias).unwrap();
    std::os::unix::fs::symlink(&foreign, &alias).unwrap();
    assert_eq!(selected_config().unwrap().state, original);
    assert_eq!(super::super::root(), original);
    store.current().unwrap();
    assert!(!foreign.join("worktree-program-stops").exists());
    std::fs::rename(
        original.join("worktree-program-stops"),
        root.join("held-journal"),
    )
    .unwrap();
    std::os::unix::fs::symlink(&foreign, original.join("worktree-program-stops")).unwrap();
    assert!(store.current().is_err());
    assert!(journal::Store::open(&config.state, true).is_err());
    assert_eq!(
        std::fs::read_to_string(foreign.join("retained")).unwrap(),
        "foreign data"
    );
}
