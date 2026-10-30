use super::super::ops::{self, Actor};
use super::*;
use super::support::*;

#[test]
fn a_merge_never_runs_the_repository_own_hook() {
    // U21, attempt 2: a clone member owns its .git and can leave a hook
    // there; the hardened route must never run it. (A hook only runs when
    // git enables it; the assertion below is about git never doing that.)
    let dir = tempfile::tempdir().unwrap();
    let hardened = dir.path().join("hardened");
    let marker = dir.path().join("marker.txt");
    if !diverged_repo(&hardened, &marker) {
        eprintln!("git could not build the test repository; the sandbox blocks it");
        return;
    }
    assert!(hardened.join(".git").join("hooks").join("post-merge").exists());
    let outcome = merge_lead(&hardened.display().to_string(), &hardened.display().to_string(), "teams-v2/lead", Duration::from_secs(60));
    assert_eq!(outcome.exit, Some(0), "{outcome:?}");
    assert!(!marker.exists(), "the repository's post-merge hook ran");
}

#[test]
fn a_repository_config_that_names_fsmonitor_is_refused() {
    // U21, attempt 2: the repository's own config must not name a program.
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    let marker = dir.path().join("marker.txt");
    std::fs::create_dir_all(&repo).unwrap();
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["-c", "user.email=t@e", "-c", "user.name=T"])
            .args(args)
            .status()
            .is_ok_and(|status| status.success())
    };
    std::fs::write(repo.join("a.txt"), "a").unwrap();
    if !(git(&["init", "-q"]) && git(&["add", "."]) && git(&["commit", "-qm", "A"])) {
        eprintln!("git could not build the test repository; the sandbox blocks it");
        return;
    }
    let program = format!("cmd /c echo x > \"{}\"", marker.display());
    assert!(git(&["config", "core.fsmonitor", &program]));
    let outcome = merge_lead(&repo.display().to_string(), &repo.display().to_string(), "main", Duration::from_secs(60));
    assert_eq!(outcome.exit, Some(1), "{outcome:?}");
    assert!(outcome.tail.contains("configures core.fsmonitor"), "{}", outcome.tail);
    assert!(!marker.exists());
}

#[test]
fn a_merge_uses_the_leads_own_branch_and_never_a_shared_folder() {
    // N1: the branch comes from the lead's folder (never a name built into
    // Zerus), and the merge runs in the member's own worktree - a member in
    // the team's shared folder never merges into the owner's checkout.
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("project");
    std::fs::create_dir_all(&repo).unwrap();
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["-c", "user.email=t@e", "-c", "user.name=T"])
            .args(args)
            .status()
            .is_ok_and(|status| status.success())
    };
    std::fs::write(repo.join("a.txt"), "a").unwrap();
    if !(git(&["init", "-q"]) && git(&["add", "."]) && git(&["commit", "-qm", "A"]) && git(&["switch", "-qc", "feature/lead"])) {
        eprintln!("git could not build the test repository; the sandbox blocks it");
        return;
    }
    let member = Member { id: "dev".into(), role: "Backend".into(), agent: "codex".into(), folder: MemberFolder::Worktree, ..Default::default() };
    let t = folder_team(&repo, member.clone());
    let mine = member_folder_path(&t, &member).unwrap();
    if !git(&["worktree", "add", "--quiet", "-b", "team/abcdef12/dev", &mine, "HEAD"]) {
        eprintln!("git could not create the member worktree; the sandbox blocks it");
        return;
    }
    // The lead's branch moves after the member's folder was cut from it.
    std::fs::write(repo.join("lead.txt"), "l").unwrap();
    assert!(git(&["add", "."]) && git(&["commit", "-qm", "lead"]));
    let request = json!({"template":"git-merge-lead","args":{}});
    let outcome = run_outcome(&t, &member, Path::new(&mine), &request).unwrap();
    assert_eq!(outcome.exit, Some(0), "{outcome:?}");
    assert!(Path::new(&mine).join("lead.txt").exists(), "the lead's branch was not merged: {}", outcome.tail);
    // A shared member works in the team's own checkout: Zerus merges nothing there.
    let shared = Member { folder: MemberFolder::Shared, ..member.clone() };
    let shared_team = folder_team(&repo, shared.clone());
    let error = run_outcome(&shared_team, &shared, &repo, &request).unwrap_err().to_string();
    assert!(error.contains("shared folder"), "{error}");
}
