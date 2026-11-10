//! Task 108: fake build folders, busy folders and unknown folders. Only a
//! registered build folder that is Zerus's is ever removed; one in use, one
//! that is not Zerus's, one that is no build folder or one waiting for review
//! is kept.
use super::*;

/// A cargo target dir with a profile and a few files.
fn cargo_target(path: &Path) {
    fs::create_dir_all(path.join("debug").join("deps")).unwrap();
    fs::write(path.join("CACHEDIR.TAG"), "Signature: 8a477f597d28d172789f06886806bc55\n").unwrap();
    fs::write(path.join("debug").join("deps").join("libx.rlib"), vec![0u8; 4096]).unwrap();
}

fn cmake_build(path: &Path) {
    fs::create_dir_all(path).unwrap();
    fs::write(path.join("CMakeCache.txt"), "CMAKE_BUILD_TYPE:STRING=Release\n").unwrap();
}

/// A time well past the files' writes: they no longer count as recent.
fn later() -> f64 {
    now() + 2.0 * RECENT_SECONDS
}

fn entry_of(path: &Path) -> Entry {
    read_registry().entries.into_iter().find(|entry| same(Path::new(&entry.path), path)).unwrap()
}

#[test]
fn an_hgs_run_command_names_its_build_folders() {
    let cwd = Path::new(if cfg!(windows) { r"D:\work\repo" } else { "/work/repo" });
    // Task #61: an absolute target on every system (D:/... is relative on Linux).
    let absolute = if cfg!(windows) { "D:/Tools/zerus-build/core" } else { "/tools/zerus-build/core" };
    let found = targets_of(
        &format!(r#"set CARGO_TARGET_DIR={absolute}&& cargo test --target-dir "../scratch target" && cmake -S tray -B .ci-build/gui && cmake --build .ci-build/gui"#),
        cwd,
    );
    let kinds: Vec<(String, &str)> = found.iter().map(|(path, kind)| (path.display().to_string().replace('\\', "/"), *kind)).collect();
    let root = if cfg!(windows) { "D:/work/repo" } else { "/work/repo" };
    assert_eq!(kinds, [(absolute.to_owned(), "cargo_target"), (format!("{root}/../scratch target"), "cargo_target"),
        (format!("{root}/.ci-build/gui"), "cmake_build")]);
    // -B means a build folder only for cmake; variables are never guessed.
    assert!(targets_of("git log -B", cwd).is_empty());
    assert!(targets_of("cargo build --target-dir $TARGET", cwd).is_empty());
    assert_eq!(template_folders("cargo-test", &json!({"manifest":["client/core/Cargo.toml"]}), cwd)[0].0, cwd.join("client/core").join("target"));
    assert_eq!(template_folders("cmake-build", &json!({"build_dir":".ci-build/gui"}), cwd)[0].0, cwd.join(".ci-build").join("gui"));
    assert!(template_folders("git-commit", &json!({}), cwd).is_empty());
}

#[test]
fn ownership_is_recorded_and_unknown_or_foreign_folders_are_never_removed() {
    let state = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(state.path(), || {
        // A run creates it: Zerus's.
        let created = work.path().join("zerus-build").join("member-a");
        let entry = note(Use { path: created.clone(), kind: "cargo_target", origin: "hgs_run", ..Default::default() }).unwrap();
        assert!(entry.owned, "{}", entry.owned_reason);
        cargo_target(&created);
        // A folder inside a repository that existed before: the person's.
        let repo = work.path().join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        cargo_target(&repo.join("target"));
        let foreign = note(Use { path: repo.join("target"), kind: "cargo_target", origin: "hgs_run", ..Default::default() }).unwrap();
        assert!(!foreign.owned);
        // Inside a member folder Zerus created: Zerus's.
        cargo_target(&repo.join("client").join("target"));
        assert!(note(Use { path: repo.join("client").join("target"), kind: "cargo_target", origin: "team_run", zerus_folder: true, ..Default::default() }).unwrap().owned);
        // A drive root or the state folder is never recorded.
        let top = if cfg!(windows) { PathBuf::from(r"C:\target") } else { PathBuf::from("/target") };
        assert!(note(Use { path: top, kind: "cargo_target", origin: "hgs_run", ..Default::default() }).is_none());
        assert!(note(Use { path: root(), kind: "cargo_target", origin: "hgs_run", ..Default::default() }).is_none());
        // An unknown build folder beside them, never registered.
        let unknown = work.path().join("zerus-build").join("unknown");
        cargo_target(&unknown);
        let decisions = clean("all", false, &[], later()).unwrap();
        assert!(!created.exists(), "Zerus's own is removed: {decisions:?}");
        assert!(unknown.exists(), "an unknown folder is never touched");
        assert!(repo.join("target").exists(), "a foreign folder is never removed");
        assert!(!decisions.iter().any(|decision| decision["path"].as_str().unwrap().contains("unknown")));
        assert_eq!(entry_of(&created).state, "removed");
        // Removing a foreign folder is refused even when it is asked for.
        assert!(matches!(remove(&entry_of(&repo.join("target")), later(), false), Removal::Kept(why) if why.starts_with("not Zerus's")));
    });
}

#[test]
fn a_folder_that_is_no_build_output_is_kept() {
    let state = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(state.path(), || {
        let path = work.path().join("scratch");
        note(Use { path: path.clone(), kind: "cargo_target", origin: "hgs_run", ..Default::default() }).unwrap();
        // Sources appeared where the build folder was: never removed.
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("Cargo.toml"), "[package]").unwrap();
        fs::write(path.join("CACHEDIR.TAG"), "Signature: 8a477f597d28d172789f06886806bc55\n").unwrap();
        assert!(matches!(remove(&entry_of(&path), later(), false), Removal::Kept(why) if why.contains("Cargo.toml")));
        // No CACHEDIR.TAG: not a cargo target.
        let bare = work.path().join("bare");
        note(Use { path: bare.clone(), kind: "cargo_target", origin: "hgs_run", ..Default::default() }).unwrap();
        fs::create_dir_all(&bare).unwrap();
        assert!(matches!(remove(&entry_of(&bare), later(), false), Removal::Kept(why) if why.contains("CACHEDIR.TAG")));
        let cmake = work.path().join("gui");
        note(Use { path: cmake.clone(), kind: "cmake_build", origin: "hgs_run", ..Default::default() }).unwrap();
        cmake_build(&cmake);
        assert_eq!(shape(&cmake, "cmake_build"), Ok(()));
        assert!(path.exists() && bare.exists());
    });
}

#[test]
fn a_busy_folder_is_kept() {
    let state = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(state.path(), || {
        let path = work.path().join("target-busy");
        note(Use { path: path.clone(), kind: "cargo_target", origin: "hgs_run", ..Default::default() }).unwrap();
        cargo_target(&path);
        // Just written: a build may be running in it.
        assert!(matches!(remove(&entry_of(&path), now(), false), Removal::Kept(why) if why.contains("written")));
        // Cargo holds its build lock.
        let lock = path.join("debug").join(".cargo-lock");
        fs::write(&lock, b"").unwrap();
        let held = fs::OpenOptions::new().read(true).write(true).open(&lock).unwrap();
        fs2::FileExt::lock_exclusive(&held).unwrap();
        assert!(matches!(remove(&entry_of(&path), later(), false), Removal::Kept(why) if why.contains("cargo is building")));
        fs2::FileExt::unlock(&held).unwrap();
        drop(held);
        // An hgs run that names it.
        let claim = crate::state::builds_test_support::running_claim("6e337ab4-7843-4ee1-ab22-8d664827421a", work.path(),
            &format!("cargo build --target-dir {}", path.display()));
        assert!(matches!(remove(&entry_of(&path), later(), false), Removal::Kept(why) if why.contains("hgs run")));
        fs::remove_file(claim).unwrap();
        // Free now: removed.
        assert!(matches!(remove(&entry_of(&path), later(), false), Removal::Removed(_)));
        assert!(!path.exists());
    });
}

#[test]
fn the_policy_removes_finished_teams_and_unused_folders_and_a_dry_run_removes_nothing() {
    use crate::state::teams::{model::{Team, TeamStatus}, store::Store};
    let state = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(state.path(), || {
        let mut store = Store::open(true).unwrap();
        store.create("11111111-1111-4111-8111-000000000001", &json!({}), Team { id: "done".into(), name: "Done".into(), status: TeamStatus::Done, ..Default::default() }, vec![]).unwrap();
        store.create("11111111-1111-4111-8111-000000000002", &json!({}), Team { id: "live".into(), name: "Live".into(), status: TeamStatus::Running, ..Default::default() }, vec![]).unwrap();
        drop(store);
        // This computer's drives may be low: only the team and age rules here.
        command(&json!({"action":"policy","policy":{"enabled":true,"unused_days":7,"free_gb":0,"free_percent":0},"by":"human"})).unwrap();
        let folder = |name: &str, team: &str| {
            let path = work.path().join(name);
            note(Use { path: path.clone(), kind: "cargo_target", origin: "team_run", team: team.into(), ..Default::default() }).unwrap();
            cargo_target(&path);
            path
        };
        let finished = folder("finished", "done");
        let running = folder("running", "live");
        // A dry run decides and removes nothing.
        let dry = clean("policy", true, &[], later()).unwrap();
        assert_eq!(dry.len(), 1, "{dry:?}");
        assert_eq!((dry[0]["action"].as_str(), dry[0]["reason"].as_str()), (Some("remove"), Some("its team is finished")));
        assert!(finished.exists());
        clean("policy", false, &[], later()).unwrap();
        assert!(!finished.exists() && running.exists());
        // Unused for a week (its files and its last use).
        let week = later() + 8.0 * 86400.0;
        let decided = clean("policy", true, &[], week).unwrap();
        assert!(decided.iter().any(|decision| decision["reason"].as_str().is_some_and(|reason| reason.starts_with("unused for"))), "{decided:?}");
        // The owner turns the policy's age off.
        command(&json!({"action":"policy","policy":{"enabled":true,"unused_days":0,"free_gb":0,"free_percent":0},"by":"human"})).unwrap();
        assert!(clean("policy", true, &[], week).unwrap().is_empty());
        // Only the owner changes the policy or removes for real.
        assert!(command(&json!({"action":"policy","policy":{}})).is_err());
        assert!(command(&json!({"action":"clean","scope":"all","dry_run":false})).is_err());
        let listing = command(&json!({"action":"list"})).unwrap();
        assert_eq!(listing["policy"]["unused_days"], 0);
        assert!(listing["teams"].as_array().unwrap().iter().any(|team| team["team_name"] == "Live"));
    });
}

#[test]
fn a_removal_waiting_for_review_is_never_tried_again_and_the_owner_forgets_it() {
    let state = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(state.path(), || {
        let path = work.path().join("half");
        note(Use { path: path.clone(), kind: "cargo_target", origin: "hgs_run", ..Default::default() }).unwrap();
        cargo_target(&path);
        locked(|registry| {
            registry.entries[0].state = "review".into();
            registry.entries[0].note = "removing failed after it was moved aside".into();
            Ok(())
        })
        .unwrap();
        assert!(matches!(remove(&entry_of(&path), later(), false), Removal::Kept(why) if why.contains("review")));
        assert!(clean("all", false, &[], later()).unwrap().is_empty());
        assert!(path.exists());
        command(&json!({"action":"forget","path":path.display().to_string(),"by":"human"})).unwrap();
        assert_eq!(entry_of(&path).state, "removed");
    });
}

#[test]
fn a_drive_under_its_threshold_is_an_alert_with_what_zerus_holds() {
    let state = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(state.path(), || {
        let path = work.path().join("big");
        note(Use { path: path.clone(), kind: "cargo_target", origin: "hgs_run", ..Default::default() }).unwrap();
        cargo_target(&path);
        command(&json!({"action":"policy","policy":{"enabled":true,"unused_days":7,"free_gb":0,"free_percent":0},"by":"human"})).unwrap();
        assert!(waiting_items().is_empty(), "no drive is under a threshold of 0");
        // A threshold no drive meets.
        command(&json!({"action":"policy","policy":{"enabled":true,"unused_days":7,"free_gb":10000,"free_percent":90},"by":"human"})).unwrap();
        let items = waiting_items();
        assert!(!items.is_empty());
        assert_eq!((items[0]["kind"].as_str(), items[0]["action"].as_str()), (Some("disk_low"), Some("open_disk")));
        assert!(items[0]["id"].as_str().unwrap().starts_with("disk/"));
        // Low disk: the oldest owned folder is chosen first.
        let decided = clean("policy", true, &[], later()).unwrap();
        assert!(decided.iter().any(|decision| decision["reason"] == "its drive is low on space"), "{decided:?}");
    });
}

/// Task 109: the Disk view says which drive each folder is on, what Zerus holds
/// on each drive and whether a removable folder is in use now; its live read
/// measures without discovering.
#[test]
fn the_disk_view_names_each_folders_drive_and_whether_it_is_busy() {
    let state = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(state.path(), || {
        let path = work.path().join("target-live");
        note(Use { path: path.clone(), kind: "cargo_target", origin: "hgs_run", ..Default::default() }).unwrap();
        cargo_target(&path);
        // Just written: busy; its size is measured by the live read.
        let listing = command(&json!({"action":"list","measure":true})).unwrap();
        let folder = &listing["teams"][0]["folders"][0];
        let (key, _, _) = drive(&path).unwrap();
        assert_eq!(folder["drive"].as_str(), Some(key.as_str()), "{listing}");
        assert!(folder["busy"].as_str().is_some_and(|why| why.contains("written")), "{listing}");
        assert!(folder["size"].as_u64().is_some_and(|size| size >= 4096), "{listing}");
        let on = listing["drives"].as_array().unwrap().iter().find(|known| known["drive"] == key.as_str()).unwrap().clone();
        assert_eq!(on["held"], folder["size"], "{listing}");
        // Not Zerus's: never checked, so never shown busy.
        locked(|registry| {
            registry.entries[0].owned = false;
            Ok(())
        })
        .unwrap();
        let listing = command(&json!({"action":"list"})).unwrap();
        assert!(listing["teams"][0]["folders"][0].get("busy").is_none(), "{listing}");
    });
}

/// Task #364: a report's note lists a leftover worktree and an extra target the
/// member made during the attempt.
#[test]
fn a_report_note_lists_a_leftover_worktree_and_an_extra_target() {
    let state = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(state.path(), || {
        let since = now() - 60.0;
        let repo = work.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        let git = |args: &[&str], at: &Path| {
            let status = std::process::Command::new("git")
                .args(["-c", "user.name=Zerus Test", "-c", "user.email=test@example.com"])
                .args(args)
                .current_dir(at)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?}");
        };
        git(&["init", "-q"], &repo);
        git(&["commit", "-q", "--allow-empty", "-m", "start"], &repo);
        let wt = work.path().join("wt");
        git(&["worktree", "add", "-q", wt.to_str().unwrap(), "-b", "team/x/dev"], &repo);
        let target = work.path().join("extra-target");
        cargo_target(&target);
        note(Use { path: target.clone(), kind: "cargo_target", origin: "hgs_run", team: "t1".into(), team_name: "T".into(),
            member: "dev".into(), ..Default::default() }).unwrap();
        let found = report_leftovers("t1", "dev", since, repo.to_str().unwrap());
        let paths: Vec<&str> = found["folders"].as_array().unwrap().iter().filter_map(|folder| folder["path"].as_str()).collect();
        assert!(paths.iter().any(|path| path.ends_with("wt")), "{found}");
        assert!(paths.iter().any(|path| path.ends_with("extra-target")), "{found}");
        let text = found["text"].as_str().unwrap();
        assert!(text.contains("Remove them") && text.contains("worktree you added") && text.contains("extra build folder"), "{text}");
    });
}

/// Task #364: one team's total passes the owner's limit once, not per folder.
#[test]
fn a_team_over_the_size_limit_is_told_once() {
    let state = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(state.path(), || {
        let target = work.path().join("target");
        cargo_target(&target);
        note(Use { path: target.clone(), kind: "cargo_target", origin: "team_run", team: "t1".into(), ..Default::default() }).unwrap();
        assert_eq!(team_over("t1"), None);
        test_measure(&target, 200 * GB, 100).unwrap();
        assert_eq!(team_over("t1"), Some((200 * GB, 100 * GB)));
        // The same answer while nothing changes: the caller makes one item.
        assert_eq!(team_over("t1"), Some((200 * GB, 100 * GB)));
        // Another team is unaffected.
        assert_eq!(team_over("t2"), None);
    });
}
