// Task #393 (owner, 2026-11-08): a git read is answered from memory while the
// repository's HEAD stands, and a blank folder never starts `git -C ""`.
use super::*;

fn git(dir: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

#[test]
fn a_blank_folder_never_starts_git() {
    // The status and inbox reads once started `git -C "" rev-parse
    // --show-toplevel` in the process's own folder.
    assert_eq!(git_at("", &["rev-parse", "--show-toplevel"]), None);
    assert_eq!(git_at("   ", &["rev-parse", "HEAD"]), None);
    assert!(git_bounded("", &["rev-parse", "--show-toplevel"]).is_err());
}

#[test]
fn a_git_answer_stays_while_the_head_stands() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    git(repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("a.txt"), "one").unwrap();
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-qm", "one"]);
    let path = repo.to_str().unwrap();
    let first = git_at(path, &["rev-parse", "HEAD"]).expect("the head");
    assert_eq!(first.len(), 40);
    assert!(head_key(path).contains(&first), "{}", head_key(path));
    // The same read again is the stored answer, not a new Git.
    assert_eq!(git_at(path, &["rev-parse", "HEAD"]).as_deref(), Some(first.as_str()));
    // A new commit moves the ref: the old answer is not reused.
    std::fs::write(repo.join("a.txt"), "two").unwrap();
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-qm", "two"]);
    let second = git_at(path, &["rev-parse", "HEAD"]).expect("the new head");
    assert_ne!(first, second);
    assert!(head_key(path).contains(&second), "{}", head_key(path));
    // A read that fails is not remembered as a success.
    assert_eq!(git_at(path, &["rev-parse", "no-such-branch"]), None);
}
