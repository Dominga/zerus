use super::*;
use crate::state::teams::model::{Attempt, Member, MemberState, PlanMode, Report, TeamStatus};
use crate::state::teams::ops::{self, Actor};
use crate::state::teams::test_queue::run_due;
use std::collections::BTreeSet;
use std::path::PathBuf;

fn run_git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn write(dir: &Path, file: &str, text: &str) {
    std::fs::write(dir.join(file), text).unwrap();
}

/// A lead checkout with the queue on and a suite that fails when `fail.txt`
/// exists, and a member worktree on its own branch.
struct Repo {
    _dir: tempfile::TempDir,
    lead: PathBuf,
    member: PathBuf,
}

fn repo() -> Repo {
    let dir = tempfile::tempdir().unwrap();
    let lead = dir.path().join("lead");
    let member = dir.path().join("member");
    std::fs::create_dir_all(lead.join(".zerus")).unwrap();
    let check = if cfg!(windows) { "if exist fail.txt (exit /b 1) else (exit /b 0)" } else { "test ! -f fail.txt" };
    std::fs::write(
        lead.join(".zerus/tests.toml"),
        format!("version = 1\n[merge_queue]\nenabled = true\n[[provider]]\nname = \"unit\"\nkind = \"command\"\ncommand = '{check}'\nsources = [\"**\"]\naffected = \"all\"\n"),
    )
    .unwrap();
    run_git(&lead, &["init", "-q"]);
    write(&lead, "a.txt", "one\n");
    run_git(&lead, &["add", "."]);
    run_git(&lead, &["commit", "-q", "-m", "base"]);
    run_git(&lead, &["worktree", "add", "-q", "-b", "task/1", &member.to_string_lossy()]);
    Repo { _dir: dir, lead, member }
}

/// The member's reported commit, and a team whose task 1 it reported.
fn team(repo: &Repo) -> Team {
    let head = run_git(&repo.member, &["rev-parse", "HEAD"]);
    let attempt = Attempt {
        n: 1,
        id: "A1".into(),
        member: "dev".into(),
        state: AttemptState::Reported,
        folder_head: head,
        folder_branch: "task/1".into(),
        report: Some(Report { text: "done".into(), at: 1.0, ..Default::default() }),
        ..Default::default()
    };
    Team {
        id: "t1".into(),
        name: "T".into(),
        lead: "lead".into(),
        folder: repo.lead.to_string_lossy().into_owned(),
        status: TeamStatus::Running,
        members: vec![
            Member { id: "lead".into(), role: "Lead".into(), agent: "claude".into(), state: MemberState::Active, ..Default::default() },
            Member {
                id: "dev".into(),
                role: "Backend".into(),
                agent: "codex".into(),
                state: MemberState::Active,
                directory: repo.member.to_string_lossy().into_owned(),
                ..Default::default()
            },
        ],
        tasks: vec![Task { id: 1, title: "Work".into(), assignee: "dev".into(), status: TaskStatus::Submitted, attempts: vec![attempt], ..Default::default() }],
        ..Default::default()
    }
}

fn accept(team: &mut Team) -> Value {
    let mut events = Vec::new();
    ops::apply(team, "accept", &json!({"task":1}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut events).unwrap()
}

fn head(repo: &Repo) -> String {
    run_git(&repo.lead, &["rev-parse", "HEAD"])
}

fn database(repo: &Repo) -> TestStore {
    TestStore::open_at(&repo.lead.parent().unwrap().join("tests.sqlite3"), false).unwrap()
}

/// The queue's step, the test queue's run of what it enqueued, and its step again.
fn settle(team: &mut Team, database: &TestStore, lead: &Path) -> Vec<Event> {
    let mut events = Vec::new();
    assert!(step(team, database, 100.0, &mut events).unwrap());
    let queued = database.queued(&team.id).unwrap();
    run_due(database, lead, &queued, 101.0, &Default::default()).unwrap();
    step(team, database, 102.0, &mut events).unwrap();
    events
}

#[test]
fn an_accept_queues_the_reported_commit_and_leaves_the_task_undecided() {
    let repo = repo();
    write(&repo.member, "b.txt", "work\n");
    run_git(&repo.member, &["add", "."]);
    run_git(&repo.member, &["commit", "-q", "-m", "work"]);
    let mut team = team(&repo);
    assert_eq!(accept(&mut team)["status"], "merge_queued");
    let task = team.task(1).unwrap();
    assert_eq!(task.status, TaskStatus::Submitted, "undecided while queued");
    let entry = task.merge.as_ref().unwrap();
    assert_eq!((entry.state.as_str(), entry.author.as_str(), entry.branch.as_str()), ("queued", "dev", "task/1"));
    assert_eq!(entry.commit, run_git(&repo.member, &["rev-parse", "HEAD"]));
    // Accepting again changes nothing; a reject takes it out of the queue.
    assert_eq!(accept(&mut team)["status"], "merge_queued");
    let mut events = Vec::new();
    ops::apply(&mut team, "reject", &json!({"task":1,"reason":"not yet"}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut events).unwrap();
    assert!(team.task(1).unwrap().merge.is_none());
}

#[test]
fn a_merged_report_whose_affected_tests_pass_is_kept_and_accepted() {
    let repo = repo();
    write(&repo.member, "b.txt", "work\n");
    run_git(&repo.member, &["add", "."]);
    run_git(&repo.member, &["commit", "-q", "-m", "work"]);
    // The lead moved meanwhile: a merge commit, not a fast-forward.
    write(&repo.lead, "c.txt", "lead\n");
    run_git(&repo.lead, &["add", "."]);
    run_git(&repo.lead, &["commit", "-q", "-m", "lead work"]);
    let before = head(&repo);
    let mut team = team(&repo);
    accept(&mut team);
    let database = database(&repo);
    let mut events = Vec::new();
    assert!(step(&mut team, &database, 100.0, &mut events).unwrap());
    let entry = team.task(1).unwrap().merge.clone().unwrap();
    assert_eq!(entry.state, "testing");
    assert_eq!(entry.prev, before);
    assert_eq!(entry.merged, head(&repo));
    assert_eq!(run_git(&repo.lead, &["rev-list", "--parents", "-n", "1", "HEAD"]).split_whitespace().count(), 3, "a merge commit");
    assert!(run_git(&repo.lead, &["log", "-1", "--format=%s"]).starts_with("Merge accepted work from dev (Backend)"));
    // Its affected set runs on the merged commit, as the queue's forced request.
    let queued = database.queued("t1").unwrap();
    assert_eq!(queued.len(), 1);
    assert_eq!((queued[0].suite.as_str(), queued[0].commit.as_str(), queued[0].task, queued[0].forced), ("unit", entry.merged.as_str(), Some(1), true));
    // Nothing decides before the result.
    assert!(!step(&mut team, &database, 100.5, &mut events).unwrap());
    run_due(&database, &repo.lead, &queued, 101.0, &Default::default()).unwrap();
    assert!(step(&mut team, &database, 102.0, &mut events).unwrap());
    let task = team.task(1).unwrap();
    assert_eq!(task.status, TaskStatus::Accepted);
    assert!(task.merge.is_none());
    assert_eq!(head(&repo), entry.merged, "the merge stays");
    assert_eq!(team.merge_tested, entry.merged);
    assert_eq!(team.merge_log[0].suites, vec!["unit"]);
    assert!(events.iter().any(|event| event.kind == "accepted"));
}

#[test]
fn a_fast_forward_merge_with_failing_tests_is_rolled_back_and_returned() {
    let repo = repo();
    write(&repo.member, "fail.txt", "breaks the suite\n");
    run_git(&repo.member, &["add", "."]);
    run_git(&repo.member, &["commit", "-q", "-m", "breaks"]);
    let before = head(&repo);
    let mut team = team(&repo);
    accept(&mut team);
    let database = database(&repo);
    let events = settle(&mut team, &database, &repo.lead);
    assert_eq!(head(&repo), before, "rolled back");
    assert!(!repo.lead.join("fail.txt").exists());
    let task = team.task(1).unwrap();
    assert_eq!((task.status, task.assignee.as_str(), task.pinned), (TaskStatus::Pending, "dev", true));
    assert!(task.decision_reason.contains("affected tests failed on the merged commit: unit failed") && task.decision_reason.contains("rolled back"), "{}", task.decision_reason);
    assert_eq!(task.attempts[0].state, AttemptState::Rejected);
    assert!(events.iter().any(|event| event.kind == "merge_returned"));
    assert!(team.merge_tested.is_empty());
}

#[test]
fn a_conflict_goes_back_to_its_author_with_the_files() {
    let repo = repo();
    write(&repo.member, "a.txt", "member\n");
    run_git(&repo.member, &["commit", "-q", "-am", "member"]);
    write(&repo.lead, "a.txt", "lead\n");
    run_git(&repo.lead, &["commit", "-q", "-am", "lead"]);
    let before = head(&repo);
    let mut team = team(&repo);
    accept(&mut team);
    let database = database(&repo);
    let mut events = Vec::new();
    assert!(step(&mut team, &database, 100.0, &mut events).unwrap());
    assert_eq!(head(&repo), before);
    assert!(!repo.lead.join(".git/MERGE_HEAD").exists(), "the merge was aborted");
    let task = team.task(1).unwrap();
    assert_eq!(task.status, TaskStatus::Pending);
    assert!(task.decision_reason.contains("conflicts in: a.txt") && task.decision_reason.contains("git-merge-lead"), "{}", task.decision_reason);
    assert_eq!(events.iter().find(|event| event.kind == "merge_conflict").unwrap().payload["files"], json!(["a.txt"]));
    assert!(database.queued("t1").unwrap().is_empty(), "nothing to test");
}

#[test]
fn a_dirty_lead_folder_makes_the_merge_wait() {
    let repo = repo();
    write(&repo.member, "b.txt", "work\n");
    run_git(&repo.member, &["add", "."]);
    run_git(&repo.member, &["commit", "-q", "-m", "work"]);
    write(&repo.lead, "a.txt", "uncommitted\n");
    let before = head(&repo);
    let mut team = team(&repo);
    accept(&mut team);
    let database = database(&repo);
    let mut events = Vec::new();
    assert!(step(&mut team, &database, 100.0, &mut events).unwrap());
    let entry = team.task(1).unwrap().merge.clone().unwrap();
    assert_eq!((entry.state.as_str(), entry.note.as_str()), ("queued", "the lead's folder has uncommitted changes"));
    assert_eq!(head(&repo), before);
    // The same wait again changes nothing.
    assert!(!step(&mut team, &database, 101.0, &mut events).unwrap());
}

#[test]
fn members_merge_the_lead_branch_first_and_a_phase_build_checks_only_what_is_untested() {
    let repo = repo();
    let mut team = team(&repo);
    let task = team.task(1).unwrap().clone();
    let text = super::super::dispatcher::hand_off_text(&team, &task, &task.attempts[0], "", "");
    assert!(text.contains("merge the lead branch into your folder before you start (the git-merge-lead template)"), "{text}");
    // A phase build after tested merges names only the range after them.
    team.merge_tested = "abc1234".into();
    team.plan_mode = PlanMode::Free;
    let mut events = Vec::new();
    let added = ops::apply(&mut team, "integrate-build", &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    let build = team.task(added["task"].as_u64().unwrap() as u32).unwrap();
    assert!(build.description.contains("hgs test affected abc1234..HEAD"), "{}", build.description);
    // Without the queue nothing changes in the hand-out.
    std::fs::write(repo.lead.join(".zerus/tests.toml"), "version = 1\n").unwrap();
    assert!(!super::super::dispatcher::hand_off_text(&team, &task, &task.attempts[0], "", "").contains("merge queue"));
}
