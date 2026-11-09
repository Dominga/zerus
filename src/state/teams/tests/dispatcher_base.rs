// Task #382 (owner, 2026-11-08): a hand-out never moves a branch that holds
// unlanded commits. The base step creates the task's own branch from the
// integration head and leaves the member's branch (and the commit its report
// named) where it is; before, `checkout -B <current> <line>` reset that branch
// to the line and orphaned the commit.
use super::*;

fn git(folder: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new("git").arg("-C").arg(folder).args(args).output().unwrap();
    assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

#[test]
fn a_hand_out_keeps_the_branch_that_holds_the_reported_commit() {
    let dir = tempfile::tempdir().unwrap();
    let line = dir.path().join("line");
    fs::create_dir(&line).unwrap();
    git(&line, &["init", "-q", "-b", "main"]);
    git(&line, &["config", "user.email", "team@example.test"]);
    git(&line, &["config", "user.name", "Team"]);
    fs::write(line.join("line.txt"), "the line").unwrap();
    git(&line, &["add", "-A"]);
    git(&line, &["commit", "-qm", "the line"]);
    let line_head = git(&line, &["rev-parse", "HEAD"]);
    let folder = dir.path().join("dev-folder");
    git(&line, &["worktree", "add", "-q", "-b", "team/33fbafeb/dev", folder.to_str().unwrap(), "main"]);
    fs::write(folder.join("work.txt"), "the member's reported work").unwrap();
    git(&folder, &["add", "-A"]);
    git(&folder, &["commit", "-qm", "the reported work"]);
    let reported = git(&folder, &["rev-parse", "HEAD"]);
    let project = dir.path().join("project");
    fs::create_dir(&project).unwrap();
    let mut team = super::support::team();
    team.folder = project.display().to_string();
    team.settings.integration_folder = line.display().to_string();
    team.line_branch = "main".into();
    {
        let dev = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
        dev.folder = MemberFolder::Worktree;
        dev.directory = folder.display().to_string();
    }
    let branch = format!("team/{}/{}-2", &team.id[..8.min(team.id.len())], tag("dev"));
    let step = prepare_integration_base(&team, 2, "dev").expect("a base step");
    assert_eq!((step.result.as_str(), step.to.as_str()), ("done", branch.as_str()), "{step:?}");
    assert_eq!(step.from, "team/33fbafeb/dev");
    assert_eq!(git(&folder, &["rev-parse", "--abbrev-ref", "HEAD"]), branch);
    assert_eq!(git(&folder, &["rev-parse", "HEAD"]), line_head, "the task starts from the line");
    assert_eq!(git(&folder, &["rev-parse", "team/33fbafeb/dev"]), reported, "the old branch is not moved");
    assert_eq!(git(&folder, &["rev-parse", &branch]), line_head);
    assert!(git(&folder, &["log", "--format=%H", "team/33fbafeb/dev"]).contains(&reported), "the reported commit stays reachable");
    assert!(git(&folder, &["log", "--format=%H", &branch]).contains(&line_head));
}

#[test]
fn a_reused_task_branch_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let line = dir.path().join("line");
    fs::create_dir(&line).unwrap();
    git(&line, &["init", "-q", "-b", "main"]);
    git(&line, &["config", "user.email", "team@example.test"]);
    git(&line, &["config", "user.name", "Team"]);
    fs::write(line.join("line.txt"), "the line").unwrap();
    git(&line, &["add", "-A"]);
    git(&line, &["commit", "-qm", "the line"]);
    let mut team = super::support::team();
    let project = dir.path().join("project");
    fs::create_dir(&project).unwrap();
    team.folder = project.display().to_string();
    team.settings.integration_folder = line.display().to_string();
    team.line_branch = "main".into();
    let branch = format!("team/{}/{}-2", &team.id[..8.min(team.id.len())], tag("dev"));
    let folder = dir.path().join("dev-folder");
    git(&line, &["worktree", "add", "-q", "-b", &branch, folder.to_str().unwrap(), "main"]);
    fs::write(folder.join("work.txt"), "work on the task branch").unwrap();
    git(&folder, &["add", "-A"]);
    git(&folder, &["commit", "-qm", "work on the task branch"]);
    let task_head = git(&folder, &["rev-parse", "HEAD"]);
    {
        let dev = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
        dev.folder = MemberFolder::Worktree;
        dev.directory = folder.display().to_string();
    }
    let step = prepare_integration_base(&team, 2, "dev").expect("a base step");
    assert_eq!(step.result, "skipped", "{step:?}");
    assert_eq!(git(&folder, &["rev-parse", "HEAD"]), task_head, "the task branch is not reset");
}