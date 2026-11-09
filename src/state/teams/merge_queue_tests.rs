use super::*;
use crate::state::teams::model::{Attempt, Member, MemberFolder, MemberState, PlanMode, Report, TeamStatus};
use crate::state::teams::ops::{self, Actor};
use crate::state::teams::test_queue::run_due;
use crate::state::teams::test_runners::Runner;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

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

/// A command provider that fails when `fail.txt` exists, in `platforms`.
fn provider(name: &str, platforms: &[&str]) -> String {
    let check = if cfg!(windows) { "if exist fail.txt (exit /b 1) else (exit /b 0)" } else { "test ! -f fail.txt" };
    let platforms = if platforms.is_empty() {
        String::new()
    } else {
        format!("platforms = [{}]\n", platforms.iter().map(|platform| format!("\"{platform}\"")).collect::<Vec<_>>().join(", "))
    };
    format!("[[provider]]\nname = \"{name}\"\nkind = \"command\"\ncommand = '{check}'\nsources = [\"**\"]\naffected = \"all\"\n{platforms}")
}

fn unit_provider() -> String {
    provider("unit", &[])
}

/// A POSIX-only suite (like the real python provider) and one for this test's
/// "here" class, beside the unit suite.
fn platform_providers() -> String {
    format!("{}{}", provider("python", &["linux", "macos"]), provider("here", &["windows"]))
}

/// A lead checkout with the queue on and a suite that fails when `fail.txt`
/// exists, and a member worktree on its own branch.
struct Repo {
    _dir: tempfile::TempDir,
    lead: PathBuf,
    member: PathBuf,
}

fn repo() -> Repo {
    repo_providers(&unit_provider())
}

/// `repo` with extra `[[provider]]` tables in the project file.
fn repo_with(extra: &str) -> Repo {
    repo_providers(&format!("{}{}", unit_provider(), extra))
}

fn repo_providers(providers: &str) -> Repo {
    let dir = tempfile::tempdir().unwrap();
    crate::platform::owner_only_dir(dir.path()).unwrap();
    let lead = dir.path().join("lead");
    let member = dir.path().join("member");
    std::fs::create_dir_all(lead.join(".zerus")).unwrap();
    std::fs::write(
        lead.join(".zerus/tests.toml"),
        format!("version = 1\n[merge_queue]\nenabled = true\n{providers}"),
    )
    .unwrap();
    run_git(&lead, &["init", "-q"]);
    // The queue's own merge commits use the repository's identity: a CI runner
    // has no global one.
    run_git(&lead, &["config", "user.name", "t"]);
    run_git(&lead, &["config", "user.email", "t@example.com"]);
    run_git(&lead, &["config", "commit.gpgsign", "false"]);
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
        line_branch: "master".into(),
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

/// The queue's step for a computer whose classes are `host` and whose
/// configured runners are `runners`.
fn step_here(team: &mut Team, database: &TestStore, now: f64, events: &mut Vec<Event>) -> anyhow::Result<bool> {
    step(team, database, now, events, &["windows".to_owned()], &[])
}

/// The queue's step, the test queue's run of what it enqueued, and its step again.
fn settle(team: &mut Team, database: &TestStore, lead: &Path) -> Vec<Event> {
    let mut events = Vec::new();
    assert!(step_here(team, database, 100.0, &mut events).unwrap());
    let queued = database.queued(&team.id).unwrap();
    run_due(database, lead, &queued, 101.0, &Default::default(), team.settings.tests.as_ref()).unwrap();
    step_here(team, database, 102.0, &mut events).unwrap();
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
fn a_suite_of_another_platform_with_no_runner_is_recorded_not_run() {
    let repo = repo_with(&platform_providers());
    write(&repo.member, "b.txt", "work\n");
    run_git(&repo.member, &["add", "."]);
    run_git(&repo.member, &["commit", "-q", "-m", "work"]);
    let mut team = team(&repo);
    team.merge_tested = "abc1234".into();
    accept(&mut team);
    let database = database(&repo);
    let mut events = Vec::new();
    let host = vec!["windows".to_owned()];
    assert!(step(&mut team, &database, 100.0, &mut events, &host, &[]).unwrap());
    let entry = team.task(1).unwrap().merge.clone().unwrap();
    assert_eq!(entry.state, "testing");
    let named = entry.not_run.join(" ");
    assert!(named.contains("python runs on linux or macos") && named.contains("no runner of that class"), "{:?}", entry.not_run);
    // The lead sees it on the task's merge view and in the feed.
    assert_eq!(view(team.task(1).unwrap()).unwrap()["not_run"], json!(entry.not_run.clone()));
    assert!(events.iter().any(|event| event.kind == "merge_not_run" && event.payload["suites"] == json!(entry.not_run.clone())), "{events:?}");
    // Only the suites that run here are queued; python is not waited for.
    let queued = database.queued("t1").unwrap();
    let suites: Vec<&str> = queued.iter().map(|request| request.suite.as_str()).collect();
    assert!(suites.contains(&"unit") && suites.contains(&"here") && !suites.contains(&"python"), "{suites:?}");
    assert!(!step(&mut team, &database, 100.5, &mut events, &host, &[]).unwrap(), "it waits for the suites that ran");
    run_due(&database, &repo.lead, &queued, 101.0, &Default::default(), None).unwrap();
    step(&mut team, &database, 102.0, &mut events, &host, &[]).unwrap();
    let task = team.task(1).unwrap();
    assert_eq!(task.status, TaskStatus::Accepted);
    let mut ran = team.merge_log[0].suites.clone();
    ran.sort();
    assert_eq!(ran, vec!["here", "unit"]);
    assert_eq!(team.merge_log[0].not_run, entry.not_run);
    // The suites that ran advance the tested point; python keeps its own range
    // open and the accept reason names it.
    assert_eq!(team.merge_tested, entry.merged);
    assert!(task.decision_reason.contains("Not run here:") && task.decision_reason.contains("python"), "{}", task.decision_reason);
    let untested = untested_since(&team).expect("the not-run suite is untested");
    assert_eq!(untested.since, entry.merged);
    assert_eq!(untested.suites(), vec!["python"]);
    assert_eq!(untested.per_suite, vec![("python".to_owned(), entry.prev.clone())]);
    assert!(team.merge_not_run.contains_key("python"), "the open suite is persisted");
}

#[test]
fn a_not_run_suite_keeps_only_its_own_range_open() {
    let repo = repo_with(&platform_providers());
    write(&repo.member, "b.txt", "one\n");
    run_git(&repo.member, &["add", "."]);
    run_git(&repo.member, &["commit", "-q", "-m", "one"]);
    let mut team = team(&repo);
    accept(&mut team);
    let database = database(&repo);
    let mut events = Vec::new();
    let host = vec!["windows".to_owned()];
    assert!(step(&mut team, &database, 100.0, &mut events, &host, &[]).unwrap());
    let queued = database.queued("t1").unwrap();
    run_due(&database, &repo.lead, &queued, 101.0, &Default::default(), None).unwrap();
    step(&mut team, &database, 102.0, &mut events, &host, &[]).unwrap();
    let first = team.merge_log[0].clone();
    let first_merged = team.merge_tested.clone();
    assert_eq!(first_merged, first.merged);
    assert_eq!(team.merge_tested_suites.get("unit").map(String::as_str), Some(team.merge_tested.as_str()));
    // A second accepted merge advances the suites that ran again; python's
    // own range still starts before the first merge.
    write(&repo.member, "b.txt", "two\n");
    run_git(&repo.member, &["add", "."]);
    run_git(&repo.member, &["commit", "-q", "-m", "two"]);
    let head = run_git(&repo.member, &["rev-parse", "HEAD"]);
    team.tasks.push(Task {
        id: 2,
        title: "More".into(),
        assignee: "dev".into(),
        status: TaskStatus::Submitted,
        attempts: vec![Attempt {
            n: 1,
            id: "A2".into(),
            member: "dev".into(),
            state: AttemptState::Reported,
            folder_head: head,
            folder_branch: "task/1".into(),
            report: Some(Report { text: "more".into(), at: 2.0, ..Default::default() }),
            ..Default::default()
        }],
        ..Default::default()
    });
    ops::apply(&mut team, "accept", &json!({"task":2}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut events).unwrap();
    assert!(step(&mut team, &database, 200.0, &mut events, &host, &[]).unwrap());
    let queued = database.queued("t1").unwrap();
    run_due(&database, &repo.lead, &queued, 201.0, &Default::default(), None).unwrap();
    step(&mut team, &database, 202.0, &mut events, &host, &[]).unwrap();
    let second_merged = team.merge_tested.clone();
    assert_ne!(second_merged, first_merged);
    assert_eq!(second_merged, team.merge_log[1].merged);
    assert_eq!(team.merge_tested_suites.get("unit").map(String::as_str), Some(second_merged.as_str()));
    assert_eq!(team.merge_tested_suites.get("here").map(String::as_str), Some(second_merged.as_str()));
    let untested = untested_since(&team).expect("python is still not run");
    assert_eq!(untested.since, second_merged);
    assert_eq!(untested.per_suite, vec![("python".to_owned(), first.prev.clone())]);
    assert!(team.merge_tested_suites.len() <= 64);
}

#[test]
fn a_config_error_rolls_back_and_requeues_the_merge() {
    let repo = repo_with(&platform_providers());
    write(&repo.member, "b.txt", "work\n");
    run_git(&repo.member, &["add", "."]);
    run_git(&repo.member, &["commit", "-q", "-m", "work"]);
    let before = head(&repo);
    let mut team = team(&repo);
    accept(&mut team);
    let database = database(&repo);
    let mut events = Vec::new();
    step_with(&mut team, &database, 100.0, &mut events, || Err(anyhow::anyhow!("boom"))).unwrap();
    let task = team.task(1).unwrap();
    assert_eq!(task.status, TaskStatus::Submitted, "a runner error never accepts");
    let entry = task.merge.as_ref().expect("the merge is requeued");
    assert_eq!(entry.state, "queued");
    assert!(entry.note.contains("test runners could not be read: boom"), "{}", entry.note);
    assert!(entry.note.contains("rolled back"), "{}", entry.note);
    assert_eq!(head(&repo), before, "the merge was rolled back at once");
    assert!(entry.merged.is_empty() && entry.requests.is_empty());
    assert!(events.iter().any(|event| event.kind == "merge_requeued"), "{events:?}");
    // Neither HEAD nor the tests map changed: no retry and no second event.
    assert!(!step_with(&mut team, &database, 101.0, &mut events, || Err(anyhow::anyhow!("boom"))).unwrap());
    assert_eq!(events.iter().filter(|event| event.kind == "merge_requeued").count(), 1, "{events:?}");
    // A changed tests map lets the retry run: this time it selects and tests.
    std::fs::write(repo.lead.join(".zerus/tests.toml"), format!("{}\n# changed\n", std::fs::read_to_string(repo.lead.join(".zerus/tests.toml")).unwrap())).unwrap();
    run_git(&repo.lead, &["add", ".zerus/tests.toml"]);
    run_git(&repo.lead, &["commit", "-q", "-m", "change the tests map"]);
    let host = vec!["windows".to_owned()];
    step_with(&mut team, &database, 102.0, &mut events, || Ok((host.clone(), Vec::new()))).unwrap();
    let first = team.task(1).unwrap().merge.as_ref().unwrap();
    assert_eq!(first.state, "testing");
    assert_ne!(head(&repo), before);
    assert!(team.merge_tested.is_empty(), "the retried merge is not tested yet");
    let queued = database.queued("t1").unwrap();
    let suites: Vec<&str> = queued.iter().map(|request| request.suite.as_str()).collect();
    assert!(suites.contains(&"unit") && !suites.contains(&"python"), "{suites:?}");
}

#[test]
fn a_persistent_selection_error_returns_the_task() {
    let repo = repo();
    write(&repo.member, "b.txt", "work\n");
    run_git(&repo.member, &["add", "."]);
    run_git(&repo.member, &["commit", "-q", "-m", "work"]);
    let mut team = team(&repo);
    accept(&mut team);
    std::fs::write(repo.lead.join(".zerus/tests.toml"), "version = 1\n[merge_queue\n").unwrap();
    run_git(&repo.lead, &["add", ".zerus/tests.toml"]);
    run_git(&repo.lead, &["commit", "-q", "-m", "break the test map"]);
    let broken = head(&repo);
    let database = database(&repo);
    let mut events = Vec::new();
    assert!(step_here(&mut team, &database, 100.0, &mut events).unwrap());
    let task = team.task(1).unwrap();
    assert_eq!(task.status, TaskStatus::Submitted, "the first selection error requeues");
    let entry = task.merge.as_ref().expect("the merge is requeued");
    assert_eq!(entry.state, "queued");
    assert!(entry.note.contains("affected tests could not be selected"), "{}", entry.note);
    assert_eq!(head(&repo), broken, "the failed merge was rolled back");
    // A forced retry fails the selection again and returns the task.
    team.task_mut(1).unwrap().merge.as_mut().unwrap().retry = true;
    step_here(&mut team, &database, 101.0, &mut events).unwrap();
    let task = team.task(1).unwrap();
    assert_eq!(task.status, TaskStatus::Pending);
    assert!(task.merge.is_none());
    assert!(task.decision_reason.contains("could not be selected twice"), "{}", task.decision_reason);
    assert_eq!(head(&repo), broken);
    assert!(database.queued("t1").unwrap().is_empty());
}

#[test]
fn a_moved_head_runs_the_recorded_merge_tests_where_it_is() {
    let repo = repo();
    write(&repo.lead, "one.txt", "one\n");
    run_git(&repo.lead, &["add", "one.txt"]);
    run_git(&repo.lead, &["commit", "-q", "-m", "one"]);
    write(&repo.lead, "two.txt", "two\n");
    run_git(&repo.lead, &["add", "two.txt"]);
    run_git(&repo.lead, &["commit", "-q", "-m", "two"]);
    let head = head(&repo);
    let merged = run_git(&repo.lead, &["rev-parse", "HEAD~1"]);
    let prev = run_git(&repo.lead, &["rev-parse", "HEAD~2"]);
    let mut team = team(&repo);
    team.task_mut(1).unwrap().merge = Some(MergeEntry {
        state: "queued".into(),
        author: "dev".into(),
        commit: merged.clone(),
        prev: prev.clone(),
        merged: merged.clone(),
        note: "The test runners could not be read: boom".into(),
        wait_head: head.clone(),
        wait_map: super::tests_stamp(&team),
        ..Default::default()
    });
    let database = database(&repo);
    let mut events = Vec::new();
    step_with(&mut team, &database, 100.0, &mut events, || Ok((vec!["windows".into()], Vec::new()))).unwrap();
    let entry = team.task(1).unwrap().merge.as_ref().unwrap();
    assert_eq!(entry.state, "testing", "the recorded range is tested on the moved HEAD");
    assert_eq!((entry.prev.as_str(), entry.merged.as_str()), (prev.as_str(), merged.as_str()));
    assert_eq!(team.task(1).unwrap().status, TaskStatus::Submitted);
    assert!(team.merge_tested.is_empty());
    let queued = database.queued("t1").unwrap();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].commit, head, "the test runs on HEAD");
}

#[test]
fn a_returned_reachable_merge_records_its_untested_range() {
    let repo = repo();
    write(&repo.lead, "one.txt", "one\n");
    run_git(&repo.lead, &["add", "one.txt"]);
    run_git(&repo.lead, &["commit", "-q", "-m", "one"]);
    let head = head(&repo);
    let prev = run_git(&repo.lead, &["rev-parse", "HEAD~1"]);
    let mut team = team(&repo);
    let entry = MergeEntry {
        state: "queued".into(),
        author: "dev".into(),
        commit: head.clone(),
        prev: prev.clone(),
        merged: head.clone(),
        ..Default::default()
    };
    team.task_mut(1).unwrap().merge = Some(entry.clone());
    let mut events = Vec::new();
    super::return_to_author(&mut team, 1, "the tests failed on HEAD", &mut events);
    assert!(team.task(1).unwrap().merge.is_none());
    let open = team.task(1).unwrap().untested_merge.as_ref().expect("the untested range stays open");
    assert_eq!(open, &MergeRange { task: 1, prev: prev.clone(), merged: head.clone() });
    assert!(team.merge_not_run.is_empty(), "a merge range is not a synthetic suite");
    assert!(team.merge_tested.is_empty());
    write(&repo.lead, "next.txt", "next\n");
    run_git(&repo.lead, &["add", "."]); run_git(&repo.lead, &["commit", "-q", "-m", "next"]);
    let later = run_git(&repo.lead, &["rev-parse", "HEAD"]);
    team.task_mut(1).unwrap().merge = Some(MergeEntry { prev: head, merged: later.clone(), ..entry });
    super::return_to_author(&mut team, 1, "returned again", &mut events);
    assert_eq!(team.task(1).unwrap().untested_merge.as_ref(), Some(&MergeRange { task: 1, prev, merged: later }));
}

#[test]
fn a_git_error_is_unknown_not_gone() {
    let dir = tempfile::tempdir().unwrap();
    let mut team = Team {
        id: "t".into(),
        name: "T".into(),
        lead: "lead".into(),
        folder: dir.path().join("not-a-repository").display().to_string(),
        status: TeamStatus::Running,
        tasks: vec![Task {
            id: 1,
            title: "Work".into(),
            status: TaskStatus::Submitted,
            merge: Some(MergeEntry {
                state: "queued".into(),
                author: "dev".into(),
                commit: "abc".into(),
                prev: "def".into(),
                merged: "def".into(),
                note: "The test runners could not be read".into(),
                ..Default::default()
            }),
            ..Default::default()
        }],
        ..Default::default()
    };
    let database = TestStore::open_at(&dir.path().join("tests.sqlite3"), false).unwrap();
    let mut events = Vec::new();
    step_with(&mut team, &database, 100.0, &mut events, || Ok((vec!["windows".into()], Vec::new()))).unwrap();
    let entry = team.task(1).unwrap().merge.as_ref().expect("an unknown git answer never removes it");
    assert_eq!(entry.state, "queued");
    assert!(entry.note.contains("could not tell"), "{}", entry.note);
    assert_eq!(team.task(1).unwrap().status, TaskStatus::Submitted);
    assert!(events.iter().any(|event| event.kind == "merge_rollback_blocked"), "{events:?}");
}

#[test]
fn a_saved_waiting_entry_becomes_queued() {
    let repo = repo();
    write(&repo.lead, "one.txt", "one\n");
    run_git(&repo.lead, &["add", "one.txt"]);
    run_git(&repo.lead, &["commit", "-q", "-m", "one"]);
    let merged = head(&repo);
    let prev = run_git(&repo.lead, &["rev-parse", "HEAD~1"]);
    let mut team = team(&repo);
    team.task_mut(1).unwrap().merge = Some(MergeEntry {
        state: "waiting".into(),
        author: "dev".into(),
        commit: merged.clone(),
        prev: prev.clone(),
        merged: merged.clone(),
        ..Default::default()
    });
    let database = database(&repo);
    let mut events = Vec::new();
    step_with(&mut team, &database, 100.0, &mut events, || panic!("a saved rollback needs no tests")).unwrap();
    let entry = team.task(1).unwrap().merge.as_ref().expect("the saved entry stays queued");
    assert_eq!(entry.state, "queued");
    assert!(entry.merged.is_empty());
    assert_eq!(head(&repo), prev);
    assert_eq!(team.task(1).unwrap().status, TaskStatus::Submitted);
}

#[test]
fn a_reachable_merge_refuses_reject_and_cancel_until_it_is_gone() {
    let repo = repo();
    write(&repo.lead, "one.txt", "one\n");
    run_git(&repo.lead, &["add", "one.txt"]);
    run_git(&repo.lead, &["commit", "-q", "-m", "one"]);
    write(&repo.lead, "two.txt", "two\n");
    run_git(&repo.lead, &["add", "two.txt"]);
    run_git(&repo.lead, &["commit", "-q", "-m", "two"]);
    let merged = run_git(&repo.lead, &["rev-parse", "HEAD~1"]);
    let prev = run_git(&repo.lead, &["rev-parse", "HEAD~2"]);
    let mut team = team(&repo);
    team.task_mut(1).unwrap().merge = Some(MergeEntry {
        state: "queued".into(),
        author: "dev".into(),
        commit: merged.clone(),
        prev: prev.clone(),
        merged: merged.clone(),
        ..Default::default()
    });
    let mut events = Vec::new();
    let error = ops::apply(&mut team, "reject", &json!({"task":1,"reason":"no"}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut events).unwrap_err().to_string();
    assert!(error.contains("the merge is still in the lead branch; retry, or revert it by hand"), "{error}");
    let error = ops::apply(&mut team, "cancel", &json!({"task":1}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap_err().to_string();
    assert!(error.contains("the merge is still in the lead branch; retry, or revert it by hand"), "{error}");
    run_git(&repo.lead, &["reset", "--hard", &prev]);
    let rejected = ops::apply(&mut team, "reject", &json!({"task":1,"reason":"reverted"}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut events).unwrap();
    assert_eq!(rejected["status"], "pending");
    assert!(team.task(1).unwrap().merge.is_none());
}

#[test]
fn rejecting_or_cancelling_a_queued_merge_clears_it() {
    let repo = repo();
    write(&repo.member, "b.txt", "work\n");
    run_git(&repo.member, &["add", "."]);
    run_git(&repo.member, &["commit", "-q", "-m", "work"]);
    let mut rejected = team(&repo);
    accept(&mut rejected);
    let mut events = Vec::new();
    ops::apply(&mut rejected, "reject", &json!({"task":1,"reason":"not yet"}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut events).unwrap();
    assert!(rejected.task(1).unwrap().merge.is_none());
    let mut team = team(&repo);
    accept(&mut team);
    let mut events = Vec::new();
    ops::apply(&mut team, "cancel", &json!({"task":1}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    assert_eq!(team.task(1).unwrap().status, TaskStatus::Cancelled);
    assert!(team.task(1).unwrap().merge.is_none());
}

#[test]
fn a_lead_accept_or_retry_resumes_a_queued_retry() {
    let mut team = Team {
        id: "t".into(),
        name: "T".into(),
        lead: "lead".into(),
        status: TeamStatus::Running,
        tasks: vec![Task {
            id: 1,
            title: "Queued".into(),
            assignee: "dev".into(),
            status: TaskStatus::Submitted,
            attempts: vec![Attempt { n: 1, id: "A1".into(), member: "dev".into(), state: AttemptState::Reported, ..Default::default() }],
            merge: Some(MergeEntry { state: "queued".into(), author: "dev".into(), note: "The tests map is broken; the merge was rolled back".into(), ..Default::default() }),
            ..Default::default()
        }],
        ..Default::default()
    };
    let items = crate::state::teams::inbox::items_for(&team, "lead");
    let item = items.iter().find(|item| item.kind == "merge_waiting").expect("the lead sees the retry");
    assert!(item.command.contains("hgs team land --json"), "{}", item.command);
    let mut events = Vec::new();
    let accepted = ops::apply(&mut team, "accept", &json!({"task":1}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut events).unwrap();
    assert_eq!(accepted["status"], "merge_retry");
    assert!(team.task(1).unwrap().merge.as_ref().unwrap().retry);
    assert_eq!(team.task(1).unwrap().status, TaskStatus::Submitted);
    team.task_mut(1).unwrap().merge.as_mut().unwrap().retry = false;
    let retried = ops::apply(&mut team, "retry", &json!({"task":1}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut events).unwrap();
    assert_eq!(retried["status"], "merge_retry");
    assert!(team.task(1).unwrap().merge.as_ref().unwrap().retry);
    assert!(events.iter().any(|event| event.kind == "merge_retry_requested"));
}

#[test]
fn a_merge_waiting_item_names_its_cause() {
    let mut team = Team {
        id: "t".into(),
        name: "T".into(),
        lead: "lead".into(),
        status: TeamStatus::Running,
        tasks: vec![Task {
            id: 1,
            title: "Queued".into(),
            status: TaskStatus::Submitted,
            merge: Some(MergeEntry { state: "queued".into(), author: "dev".into(), note: "The test runners could not be read: boom; the merge was rolled back".into(), wait_reason: MergeWaitReason::Runners, ..Default::default() }),
            ..Default::default()
        }],
        ..Default::default()
    };
    let detail = |team: &Team| crate::state::teams::inbox::items_for(team, "lead").into_iter().find(|item| item.kind == "merge_waiting").unwrap().detail;
    assert!(detail(&team).contains("Fix the runner or machine config"), "{}", detail(&team));
    team.task_mut(1).unwrap().merge.as_mut().unwrap().note = "The affected tests could not be selected: broken map; the merge was rolled back".into();
    team.task_mut(1).unwrap().merge.as_mut().unwrap().wait_reason = MergeWaitReason::Selection;
    assert!(detail(&team).contains("Fix the tests map"), "{}", detail(&team));
    team.task_mut(1).unwrap().merge.as_mut().unwrap().note = "The lead branch moved and the merge could not be rolled back".into();
    team.task_mut(1).unwrap().merge.as_mut().unwrap().wait_reason = MergeWaitReason::Rollback;
    assert!(detail(&team).contains("revert it, then retry or reject"), "{}", detail(&team));
    team.task_mut(1).unwrap().merge.as_mut().unwrap().note = "the lead's folder has uncommitted changes".into();
    team.task_mut(1).unwrap().merge.as_mut().unwrap().wait_reason = MergeWaitReason::Busy;
    assert!(!crate::state::teams::inbox::items_for(&team, "lead").iter().any(|item| item.kind == "merge_waiting"), "a Busy note is not an inbox item");
}

#[test]
fn a_kept_merge_with_nothing_selected_advances_the_tested_range() {
    let repo = repo();
    write(&repo.member, "b.txt", "work\n");
    run_git(&repo.member, &["add", "."]);
    run_git(&repo.member, &["commit", "-q", "-m", "work"]);
    // The lead already has it: the merge is a no-op and nothing is selected.
    run_git(&repo.lead, &["merge", "--no-edit", "task/1"]);
    let already = head(&repo);
    let mut team = team(&repo);
    team.merge_tested = "older".into();
    accept(&mut team);
    let database = database(&repo);
    let mut events = Vec::new();
    assert!(step_here(&mut team, &database, 100.0, &mut events).unwrap());
    assert_eq!(team.task(1).unwrap().status, TaskStatus::Accepted);
    assert_eq!(team.merge_tested, already, "nothing selected still tests the current head");
    assert!(team.merge_log[0].suites.is_empty() && team.merge_log[0].not_run.is_empty());
}

#[test]
fn an_open_not_run_suite_outlives_the_bounded_merge_log() {
    let mut team = Team { merge_tested: "head60".into(), ..Default::default() };
    for i in 0..60 {
        team.merge_log.push(crate::state::teams::model::MergeLog {
            task: i + 1,
            prev: format!("m{i}"),
            merged: format!("m{}", i + 1),
            suites: vec!["unit".into()],
            ..Default::default()
        });
    }
    team.merge_not_run.insert(
        "python".into(),
        crate::state::teams::model::OpenNotRun {
            since: "base".into(),
            message: "not run here: python runs on linux or macos; no runner of that class".into(),
        },
    );
    let untested = untested_since(&team).expect("the open suite outlives the 50-entry log");
    assert_eq!(untested.suites(), vec!["python"]);
    assert_eq!(untested.per_suite, vec![("python".to_owned(), "base".to_owned())]);
}

#[test]
fn an_open_range_overflow_holds_the_tested_point_and_says_so() {
    let mut team = Team { merge_tested: "newer".into(), ..Default::default() };
    for i in 0..65 {
        team.merge_not_run.insert(
            format!("suite{i:02}"),
            crate::state::teams::model::OpenNotRun { since: format!("s{i}"), message: format!("not run here: suite{i:02}") },
        );
    }
    let mut events = Vec::new();
    super::guard_not_run(&mut team, &mut events);
    assert_eq!(team.merge_not_run.len(), 65, "no open range is silently evicted");
    assert_eq!(team.merge_tested, "s0");
    assert!(events.iter().any(|event| event.kind == "merge_not_run_overflow"), "{events:?}");
}

#[test]
fn a_merge_of_only_not_run_suites_does_not_advance_the_tested_range() {
    let repo = repo_providers(&provider("python", &["linux", "macos"]));
    write(&repo.member, "b.txt", "work\n");
    run_git(&repo.member, &["add", "."]);
    run_git(&repo.member, &["commit", "-q", "-m", "work"]);
    let mut team = team(&repo);
    team.merge_tested = "abc1234".into();
    accept(&mut team);
    let database = database(&repo);
    let mut events = Vec::new();
    let host = vec!["windows".to_owned()];
    assert!(step(&mut team, &database, 100.0, &mut events, &host, &[]).unwrap());
    let task = team.task(1).unwrap();
    assert_eq!(task.status, TaskStatus::Accepted);
    assert_eq!(team.merge_tested, "abc1234", "zero suites run is not a tested range");
    assert!(team.merge_log[0].suites.is_empty());
    assert!(team.merge_log[0].not_run.iter().any(|message| message.contains("python")), "{:?}", team.merge_log[0].not_run);
    assert!(task.decision_reason.contains("Not run here:") && task.decision_reason.contains("python"), "{}", task.decision_reason);
    let untested = untested_since(&team).expect("python was not run");
    assert_eq!(untested.since, "abc1234");
    assert_eq!(untested.suites(), vec!["python"]);
    // The phase build checks the range and names the suite the queue could not run.
    team.plan_mode = PlanMode::Free;
    let added = ops::apply(&mut team, "integrate-build", &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    let build = team.task(added["task"].as_u64().unwrap() as u32).unwrap();
    assert!(build.description.contains("hgs test affected abc1234..HEAD"), "{}", build.description);
    assert!(build.description.contains("did not run python"), "{}", build.description);
    assert!(build.description.contains(&format!("python since {}", team.merge_log[0].prev)), "{}", build.description);
}

#[test]
fn a_suite_of_another_platform_with_a_runner_is_queued_and_waited_for() {
    let repo = repo_with(&platform_providers());
    write(&repo.member, "b.txt", "work\n");
    run_git(&repo.member, &["add", "."]);
    run_git(&repo.member, &["commit", "-q", "-m", "work"]);
    let mut team = team(&repo);
    accept(&mut team);
    let database = database(&repo);
    let mut events = Vec::new();
    let host = vec!["windows".to_owned()];
    let runners = vec![Runner { alias: "box".into(), platform: "linux".into() }];
    assert!(step(&mut team, &database, 100.0, &mut events, &host, &runners).unwrap());
    let entry = team.task(1).unwrap().merge.clone().unwrap();
    assert_eq!(entry.state, "testing");
    assert!(entry.not_run.is_empty(), "a runner of the class keeps the suite");
    let queued = database.queued("t1").unwrap();
    let python = queued.iter().find(|request| request.suite == "python").expect("python is queued for its runner");
    assert!(python.platforms.contains(&"linux".to_owned()), "{:?}", python.platforms);
    // It waits for the runner's result like any queued suite.
    assert!(!step(&mut team, &database, 100.5, &mut events, &host, &runners).unwrap());
    run_due(&database, &repo.lead, &queued, 101.0, &Default::default(), None).unwrap();
    step(&mut team, &database, 102.0, &mut events, &host, &runners).unwrap();
    assert_eq!(team.task(1).unwrap().status, TaskStatus::Accepted);
    assert!(team.merge_log[0].suites.iter().any(|suite| suite == "python"));
    assert!(team.merge_log[0].not_run.is_empty());
    assert!(team.merge_not_run.is_empty(), "a suite that ran clears its open range");
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
    assert!(step_here(&mut team, &database, 100.0, &mut events).unwrap());
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
    assert!(!step_here(&mut team, &database, 100.5, &mut events).unwrap());
    run_due(&database, &repo.lead, &queued, 101.0, &Default::default(), None).unwrap();
    assert!(step_here(&mut team, &database, 102.0, &mut events).unwrap());
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
fn a_failed_merge_names_the_suites_not_run_here() {
    let repo = repo_with(&platform_providers());
    write(&repo.member, "fail.txt", "breaks the suite\n");
    run_git(&repo.member, &["add", "."]);
    run_git(&repo.member, &["commit", "-q", "-m", "breaks"]);
    let mut team = team(&repo);
    accept(&mut team);
    let database = database(&repo);
    let mut events = Vec::new();
    let host = vec!["windows".to_owned()];
    assert!(step(&mut team, &database, 100.0, &mut events, &host, &[]).unwrap());
    let queued = database.queued("t1").unwrap();
    run_due(&database, &repo.lead, &queued, 101.0, &Default::default(), None).unwrap();
    step(&mut team, &database, 102.0, &mut events, &host, &[]).unwrap();
    let task = team.task(1).unwrap();
    assert_eq!(task.status, TaskStatus::Pending);
    assert!(task.decision_reason.contains("affected tests failed"), "{}", task.decision_reason);
    assert!(task.decision_reason.contains("Not run here:") && task.decision_reason.contains("python"), "{}", task.decision_reason);
    assert!(team.merge_tested.is_empty());
}

#[test]
fn a_conflict_becomes_one_resolve_task_and_later_landings_wait() {
    let repo = repo();
    write(&repo.member, "a.txt", "member\n");
    run_git(&repo.member, &["commit", "-q", "-am", "member"]);
    write(&repo.lead, "a.txt", "lead\n");
    run_git(&repo.lead, &["commit", "-q", "-am", "lead"]);
    let before = head(&repo);
    let mut team = team(&repo);
    team.settings.integration_folder = repo.lead.to_string_lossy().into_owned();
    let queued = accept(&mut team);
    assert_eq!(queued["landing"]["state"], "queued", "{queued}");
    assert_eq!(queued["landing"]["line"], "master", "{queued}");
    // A second landing accepted behind it (its commit is this same work).
    let member_head = run_git(&repo.member, &["rev-parse", "HEAD"]);
    team.tasks.push(Task { id: 2, title: "Second".into(), assignee: "dev".into(), status: TaskStatus::Submitted,
        attempts: vec![Attempt { n: 1, id: "A2".into(), member: "dev".into(), state: AttemptState::Reported,
            folder_head: member_head, folder_branch: "task/1".into(), report: Some(Report { text: "done".into(), at: 1.0, ..Default::default() }), ..Default::default() }], ..Default::default() });
    let mut second = Vec::new();
    ops::apply(&mut team, "accept", &json!({"task":2}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut second).unwrap();
    let database = database(&repo);
    let mut events = Vec::new();
    assert!(step_here(&mut team, &database, 100.0, &mut events).unwrap());
    assert_eq!(head(&repo), before, "nothing landed");
    assert!(!repo.lead.join(".git/MERGE_HEAD").exists(), "the merge was aborted");
    let entry = team.task(1).unwrap().merge.clone().unwrap();
    assert_eq!((entry.state.as_str(), entry.conflict_files.clone()), ("conflict", vec!["a.txt".to_owned()]));
    let resolve = entry.resolve_task.expect("a resolve task");
    let task = team.task(resolve).expect("the resolve task");
    assert_eq!((task.assignee.as_str(), task.origin), ("lead", Some(1)), "no reviewer member: the lead resolves");
    assert_eq!(task.status, TaskStatus::Pending);
    assert!(task.description.contains("a.txt") && task.description.contains(&entry.commit[..9]) && task.description.contains(&before[..9]), "{}", task.description);
    assert_eq!(team.task(1).unwrap().status, TaskStatus::Submitted, "the accepted work stays undecided");
    assert!(database.queued("t1").unwrap().is_empty(), "nothing to test yet");
    // One resolve task per conflict: another step makes no second one.
    step_here(&mut team, &database, 101.0, &mut events).unwrap();
    assert_eq!(team.tasks.iter().filter(|task| task.title.starts_with("Resolve the integration conflict")).count(), 1);
    assert_eq!(events.iter().find(|event| event.kind == "merge_resolve_task").unwrap().payload["files"], json!(["a.txt"]));
    // The later landing waits behind the conflict instead of meeting it again.
    step_here(&mut team, &database, 102.0, &mut events).unwrap();
    assert_eq!(team.task(2).unwrap().merge.as_ref().unwrap().state, "queued", "the later landing waits");
    assert_eq!(head(&repo), before, "still nothing landed");
}

#[test]
fn the_landing_retries_once_its_resolve_task_is_decided() {
    let repo = repo();
    write(&repo.member, "a.txt", "member\n");
    run_git(&repo.member, &["commit", "-q", "-am", "member"]);
    write(&repo.lead, "a.txt", "lead\n");
    run_git(&repo.lead, &["commit", "-q", "-am", "lead"]);
    let mut team = team(&repo);
    team.settings.integration_folder = repo.lead.to_string_lossy().into_owned();
    accept(&mut team);
    let database = database(&repo);
    let mut events = Vec::new();
    step_here(&mut team, &database, 100.0, &mut events).unwrap();
    let resolve = team.task(1).unwrap().merge.as_ref().unwrap().resolve_task.unwrap();
    // The reviewer resolves the conflict on the integration line itself: the
    // accepted commit is merged there and the resolution committed.
    let accepted = run_git(&repo.member, &["rev-parse", "HEAD"]);
    run_git(&repo.lead, &["merge", "--no-edit", "-q", "-X", "ours", &accepted]);
    let resolved = run_git(&repo.lead, &["rev-parse", "HEAD"]);
    ops::apply(&mut team, "cancel", &json!({"task":resolve,"reason":"resolved on the integration line"}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut events).unwrap();
    // The landing is back in the queue; its commit is on the line now, so the
    // queue accepts it without merging again.
    let events = settle(&mut team, &database, &repo.lead);
    assert_eq!(team.task(1).unwrap().status, TaskStatus::Accepted);
    assert!(run_git(&repo.lead, &["merge-base", "--is-ancestor", &accepted, "HEAD"]).is_empty());
    assert_eq!(head(&repo), resolved);
    assert!(events.iter().any(|event| event.kind == "merged"), "{events:?}");
}

#[test]
fn a_clean_new_task_starts_from_the_integration_head() {
    let repo = repo();
    // The integration line moved after the member's worktree was made.
    write(&repo.lead, "later.txt", "later\n");
    run_git(&repo.lead, &["add", "."]);
    run_git(&repo.lead, &["commit", "-q", "-m", "later"]);
    let mut team = team(&repo);
    team.settings.integration_folder = repo.lead.to_string_lossy().into_owned();
    team.members[1].folder = MemberFolder::Worktree;
    team.members[1].session = Some(crate::state::teams::model::SessionRef { name: "codex/x/dev".into(), ..Default::default() });
    team.settings.branch_per_task = Some(crate::state::teams::model::BranchPerTask { template: "work/{id}".into() });
    let step = crate::state::teams::dispatcher::prepare_branch(&team, 2, "dev").expect("the branch step");
    assert_eq!(step.result, "done", "{step:?}");
    assert!(step.reason.contains("from master"), "{step:?}");
    assert_eq!(run_git(&repo.member, &["rev-parse", "HEAD"]), head(&repo), "the task branch starts from the integration head");
    assert_eq!(run_git(&repo.member, &["rev-parse", "--abbrev-ref", "HEAD"]), "work/2");
}

#[test]
fn a_dirty_folder_is_committed_before_it_starts_from_the_integration_head() {
    let repo = repo();
    write(&repo.lead, "later.txt", "later\n");
    run_git(&repo.lead, &["add", "."]);
    run_git(&repo.lead, &["commit", "-q", "-m", "later"]);
    // Unfinished work on the member's old branch, not committed.
    write(&repo.member, "a.txt", "unfinished\n");
    let mut team = team(&repo);
    team.settings.integration_folder = repo.lead.to_string_lossy().into_owned();
    let dev = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
    dev.folder = MemberFolder::Worktree;
    dev.session = Some(crate::state::teams::model::SessionRef { name: "codex/x/dev".into(), ..Default::default() });
    team.tasks[0].attempts.clear();
    team.tasks[0].status = TaskStatus::Pending;
    let step = crate::state::teams::dispatcher::prepare_integration_base(&team, 1, "dev").expect("the base step");
    assert_eq!(step.result, "done", "{step:?}");
    assert!(step.reason.contains("starts from the integration head"), "{step:?}");
    // Task #382: the unfinished work is committed on the branch the folder was
    // on, which keeps that commit; the folder moves to the task's own branch at
    // the integration head. The old branch is never reset.
    assert_eq!(run_git(&repo.member, &["rev-parse", "HEAD"]), head(&repo));
    assert_eq!(run_git(&repo.member, &["log", "-1", "--format=%s", "task/1"]), "Zerus: dev's unfinished work before task #1");
    let task_branch = format!("team/{}/dev-1", &team.id[..8.min(team.id.len())]);
    assert_eq!(run_git(&repo.member, &["rev-parse", "--abbrev-ref", "HEAD"]), task_branch);
    assert_eq!(run_git(&repo.member, &["show", "HEAD:a.txt"]), "one", "the head is the integration line's");
    // A folder already at the head is left alone.
    let step = crate::state::teams::dispatcher::prepare_integration_base(&team, 1, "dev").expect("the base step again");
    assert_eq!(step.result, "skipped", "{step:?}");
}

#[test]
fn a_dependent_waits_while_its_predecessors_work_is_not_on_the_line() {
    let repo = repo();
    write(&repo.member, "b.txt", "work\n");
    run_git(&repo.member, &["add", "."]);
    run_git(&repo.member, &["commit", "-q", "-m", "work"]);
    let mut team = team(&repo);
    team.settings.integration_folder = repo.lead.to_string_lossy().into_owned();
    // Task 1 was accepted without a landing (an old forgotten merge).
    team.task_mut(1).unwrap().status = TaskStatus::Accepted;
    team.tasks.push(Task { id: 2, title: "Dependent".into(), assignee: "dev".into(), status: TaskStatus::Pending,
        deps: vec![crate::state::teams::model::Dependency { task: 1, kind: crate::state::teams::model::DependencyKind::Hard }], ..Default::default() });
    let step = crate::state::teams::dispatcher::prepare_dependencies(&team, 2, "dev").expect("the dependencies step");
    assert_eq!(step.result, "refused", "{step:?}");
    assert!(step.reason.contains("not on the integration line yet") && step.reason.contains("#1"), "{step:?}");
    // Once the work is on the line, the step no longer merges anything.
    let commit = run_git(&repo.member, &["rev-parse", "HEAD"]);
    run_git(&repo.lead, &["merge", "--no-edit", "-q", &commit]);
    let step = crate::state::teams::dispatcher::prepare_dependencies(&team, 2, "dev").expect("the dependencies step");
    assert_eq!(step.result, "skipped", "{step:?}");
    assert!(step.reason.contains("lands on the integration line"), "{step:?}");
}

#[test]
fn status_and_the_lead_inbox_name_accepted_work_that_is_not_landed() {
    let repo = repo();
    write(&repo.member, "b.txt", "work\n");
    run_git(&repo.member, &["add", "."]);
    run_git(&repo.member, &["commit", "-q", "-m", "work"]);
    let mut team = team(&repo);
    team.settings.integration_folder = repo.lead.to_string_lossy().into_owned();
    // Accepted work the queue never landed (the old forgotten merges).
    team.task_mut(1).unwrap().status = TaskStatus::Accepted;
    team.task_mut(1).unwrap().attempts[0].state = AttemptState::Reported;
    let view = crate::state::teams::commands::view_with_cancelled(&team, &[], None, false);
    assert_eq!(view["integration"]["branch"], "master");
    assert_eq!(view["integration"]["head"], head(&repo));
    assert_eq!(view["integration"]["unlanded"][0]["task"], 1);
    assert!(view["integration"]["unlanded"][0]["why"].as_str().unwrap().contains("no landing is queued"), "{}", view["integration"]);
    assert!(view["integration"]["landed"].as_array().unwrap().is_empty());
    let inbox = crate::state::teams::inbox::items(&team);
    let item = inbox.iter().find(|item| item.kind == "unlanded").expect("the lead's item");
    assert!(item.summary.contains("#1 is accepted but not on the integration line"), "{}", item.summary);
    assert!(item.command.contains("hgs team land"));
    // Once the queue lands its commit, it is on the line and the item leaves.
    let commit = run_git(&repo.member, &["rev-parse", "HEAD"]);
    run_git(&repo.lead, &["merge", "--no-edit", "-q", &commit]);
    let view = crate::state::teams::commands::view_with_cancelled(&team, &[], None, false);
    assert_eq!(view["integration"]["landed"], json!([{"task":1,"commit":commit,"equivalent":false,"marked":null}]), "{}", view["integration"]);
    assert!(view["integration"]["unlanded"].as_array().unwrap().is_empty());
    assert!(crate::state::teams::inbox::items(&team).iter().all(|item| item.kind != "unlanded"));
}

/// Git runs merge drivers through its shell. A sandboxed member's test runner
/// (low integrity) cannot start MSYS sh, so the driver is exercised where that
/// shell works: CI and Zerus's own merge paths outside the member sandbox.
fn merge_driver_available() -> bool {
    Command::new("git")
        .args(["-c", "alias.zerus-shell-probe=!exit 0", "zerus-shell-probe"])
        .output()
        .is_ok_and(|out| out.status.success())
}

#[test]
fn the_catalog_merge_driver_joins_disjoint_keys_through_the_queue() {
    // Task 272: .gitattributes names the `i18n` driver; the queue passes it with
    // `-c` at merge time, so both sides adding different supplement keys is not
    // a text conflict.
    if !merge_driver_available() {
        eprintln!("skipped: this sandbox cannot start Git's shell, which runs merge drivers");
        return;
    }
    let repo = repo();
    let script = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/i18n-merge.py"))
        .expect("scripts/i18n-merge.py");
    let base = "{\n  \"shared key\": \"shared text\"\n}\n";
    std::fs::create_dir_all(repo.lead.join("client/src")).unwrap();
    std::fs::create_dir_all(repo.lead.join("scripts")).unwrap();
    write(&repo.lead, ".gitattributes", "client/src/i18n.next-ru.json merge=i18n\n");
    write(&repo.lead, "scripts/i18n-merge.py", &script);
    write(&repo.lead, "client/src/i18n.next-ru.json", base);
    run_git(&repo.lead, &["add", "."]);
    run_git(&repo.lead, &["commit", "-q", "-m", "catalogs and driver"]);
    // The member starts from that base (its branch has no commit yet).
    run_git(&repo.member, &["merge", "-q", "--no-edit", &head(&repo)]);
    write(&repo.member, "client/src/i18n.next-ru.json",
          "{\n  \"member key\": \"member text\",\n  \"shared key\": \"shared text\"\n}\n");
    run_git(&repo.member, &["commit", "-q", "-am", "member key"]);
    write(&repo.lead, "client/src/i18n.next-ru.json",
          "{\n  \"lead key\": \"lead text\",\n  \"shared key\": \"shared text\"\n}\n");
    run_git(&repo.lead, &["commit", "-q", "-am", "lead key"]);

    let mut team = team(&repo);
    accept(&mut team);
    let database = database(&repo);
    settle(&mut team, &database, &repo.lead);

    let task = team.task(1).unwrap();
    let merge = task.merge.as_ref().map(|entry| format!("{}: {}", entry.state, entry.note)).unwrap_or_default();
    let current = std::fs::read_to_string(repo.lead.join("client/src/i18n.next-ru.json")).unwrap_or_default();
    assert_eq!(task.status, TaskStatus::Accepted, "decision: {} / merge: {merge} / file: {current}", task.decision_reason);
    let merged = std::fs::read_to_string(repo.lead.join("client/src/i18n.next-ru.json")).unwrap();
    assert!(!merged.contains("<<<<<<<"), "{merged}");
    let catalog: serde_json::Value = serde_json::from_str(&merged).unwrap();
    assert_eq!(catalog["member key"], "member text");
    assert_eq!(catalog["lead key"], "lead text");
    assert_eq!(catalog["shared key"], "shared text");
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
    assert!(step_here(&mut team, &database, 100.0, &mut events).unwrap());
    let entry = team.task(1).unwrap().merge.clone().unwrap();
    assert_eq!((entry.state.as_str(), entry.note.as_str()), ("queued", "the lead's folder has uncommitted changes"));
    assert_eq!(head(&repo), before);
    // The same wait again changes nothing.
    assert!(!step_here(&mut team, &database, 101.0, &mut events).unwrap());
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


fn marked_range(repo: &Repo, team: &mut Team) -> MergeRange {
    let prev = head(repo);
    write(&repo.lead, "old.txt", "untested\n");
    run_git(&repo.lead, &["add", "."]);
    run_git(&repo.lead, &["commit", "-q", "-m", "old merge"]);
    let range = MergeRange { task: 1, prev, merged: head(repo) };
    team.task_mut(1).unwrap().untested_merge = Some(range.clone());
    range
}

fn queue_next(repo: &Repo, team: &mut Team, id: u32) {
    run_git(&repo.member, &["merge", "--no-edit", &head(repo)]);
    write(&repo.member, &format!("task-{id}.txt"), "next\n");
    run_git(&repo.member, &["add", "."]);
    run_git(&repo.member, &["commit", "-q", "-m", "next"]);
    team.tasks.push(Task {
        id, title: "Next".into(), assignee: "dev".into(), status: TaskStatus::Submitted,
        attempts: vec![Attempt {
            n: 1, id: format!("A{id}"), member: "dev".into(), state: AttemptState::Reported,
            folder_head: run_git(&repo.member, &["rev-parse", "HEAD"]), folder_branch: "task/1".into(),
            report: Some(Report { text: "done".into(), at: id as f64, ..Default::default() }), ..Default::default()
        }], ..Default::default()
    });
    ops::apply(team, "accept", &json!({"task":id}), &Actor::Human, &BTreeSet::new(), &mut Vec::new()).unwrap();
}

#[test]
fn an_untested_merge_is_a_range_and_a_build_closes_only_the_ranges_it_named() {
    let repo = repo();
    let mut team = team(&repo);
    let range = marked_range(&repo, &mut team);
    // A round-4 save used a synthetic suite. Loading and the ops entry point
    // must migrate it before composing the build prompt.
    team.task_mut(1).unwrap().untested_merge = None;
    team.merge_not_run.insert("merge-1".into(), OpenNotRun {
        since: range.prev.clone(),
        message: format!("not run here: merge-1 runs on all; the merge of {} was not tested on {}", range.merged, range.merged),
    });
    let mut team: Team = serde_json::from_value(serde_json::to_value(team).unwrap()).unwrap();
    team.plan_mode = PlanMode::Free;
    let mut events = Vec::new();
    let added = ops::apply(&mut team, "integrate-build", &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    let build_id = added["task"].as_u64().unwrap() as u32;
    let build = team.task(build_id).unwrap();
    assert!(build.description.contains(&format!("untested merge of task #1, range {}..{}", range.prev, range.merged)), "{}", build.description);
    assert!(!build.description.contains("did not run merge-1"));
    assert_eq!(build.build_merge_ranges, vec![range.clone()]);
    assert!(team.merge_not_run.is_empty());
    // A different range created after the build's snapshot must survive.
    let mut another = range.clone(); another.task = 3;
    team.tasks.push(Task { id: 3, untested_merge: Some(another.clone()), ..Default::default() });
    ops::apply(&mut team, "accept", &json!({"task":build_id}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    assert!(team.task(1).unwrap().untested_merge.is_none());
    assert_eq!(team.task(3).unwrap().untested_merge.as_ref(), Some(&another));
}

#[test]
fn a_later_kept_merge_closes_an_open_range_only_after_all_its_suites_ran() {
    let repo = repo_with(&provider("other", &["linux"]).replace("sources = [\"**\"]", "sources = [\"old.txt\"]"));
    let mut team = team(&repo);
    let range = marked_range(&repo, &mut team);
    let database = database(&repo);
    let mut events = Vec::new();
    queue_next(&repo, &mut team, 2);
    let host = vec!["windows".to_owned()];
    step(&mut team, &database, 100.0, &mut events, &host, &[]).unwrap();
    run_due(&database, &repo.lead, &database.queued("t1").unwrap(), 101.0, &Default::default(), None).unwrap();
    step(&mut team, &database, 102.0, &mut events, &host, &[]).unwrap();
    assert_eq!(team.task(2).unwrap().status, TaskStatus::Accepted);
    assert_eq!(team.task(1).unwrap().untested_merge.as_ref(), Some(&range));
    assert_eq!(team.merge_not_run.get("other").unwrap().since, range.prev, "a suite selected by the older range keeps that original start");
    queue_next(&repo, &mut team, 3);
    let host = vec!["windows".to_owned(), "linux".to_owned()];
    step(&mut team, &database, 200.0, &mut events, &host, &[]).unwrap();
    let queued = database.queued("t1").unwrap();
    assert!(queued.iter().any(|request| request.suite == "other"));
    assert!(queued.iter().all(|request| request.commit == head(&repo)));
    run_due(&database, &repo.lead, &queued, 201.0, &Default::default(), None).unwrap();
    step(&mut team, &database, 202.0, &mut events, &host, &[]).unwrap();
    assert_eq!(team.task(3).unwrap().status, TaskStatus::Accepted);
    assert!(team.task(1).unwrap().untested_merge.is_none());
}

#[test]
fn old_reverting_entries_are_dropped_when_returned_cancelled_or_unreachable() {
    for (status, reachable) in [(TaskStatus::Pending, true), (TaskStatus::Cancelled, true),
        (TaskStatus::Pending, false), (TaskStatus::Cancelled, false), (TaskStatus::Submitted, false)] {
        let repo = repo();
        let prev = head(&repo);
        let folder = if reachable { &repo.lead } else { &repo.member };
        write(folder, "old.txt", "old\n");
        run_git(folder, &["add", "."]); run_git(folder, &["commit", "-q", "-m", "old"]);
        let merged = run_git(folder, &["rev-parse", "HEAD"]);
        let initial = head(&repo);
        let mut team = team(&repo);
        let task = team.task_mut(1).unwrap(); task.status = status;
        task.merge = Some(MergeEntry { state: "reverting".into(), author: "dev".into(),
            commit: merged.clone(), prev, merged, folder: repo.member.to_string_lossy().into_owned(), ..Default::default() });
        let mut saved = serde_json::to_value(team).unwrap();
        saved["tasks"][0]["merge"].as_object_mut().unwrap().remove("wait_reason");
        saved["tasks"][0]["merge"].as_object_mut().unwrap().remove("tested_ranges");
        let mut team: Team = serde_json::from_value(saved).unwrap();
        assert_eq!(team.task(1).unwrap().merge.as_ref().unwrap().wait_reason, MergeWaitReason::Other);
        let database = database(&repo); let mut events = Vec::new();
        assert!(step_with(&mut team, &database, 100.0, &mut events, || panic!("a dropped entry runs no tests")).unwrap());
        assert!(team.task(1).unwrap().merge.is_none()); assert_eq!(team.task(1).unwrap().status, status);
        assert_eq!(head(&repo), initial); assert!(database.queued("t1").unwrap().is_empty());
        assert!(!step_here(&mut team, &database, 101.0, &mut events).unwrap());
    }
}

#[test]
fn rejecting_or_cancelling_a_testing_merge_names_the_running_tests_first() {
    let repo = repo(); let mut team = team(&repo);
    team.task_mut(1).unwrap().merge = Some(MergeEntry { state: "testing".into(), merged: head(&repo), ..Default::default() });
    for operation in ["reject", "cancel"] {
        let error = ops::apply(&mut team, operation, &json!({"task":1,"reason":"no"}), &Actor::Human,
            &BTreeSet::new(), &mut Vec::new()).unwrap_err().to_string();
        assert!(error.contains("its tests run; wait for the merge queue's result"), "{error}");
        assert!(!error.contains("revert it by hand"), "{error}");
        assert_eq!(team.task(1).unwrap().merge.as_ref().unwrap().state, "testing");
    }
}

#[test]
fn a_failed_rollback_waits_for_a_changed_head_or_an_explicit_retry() {
    for resume in ["retry", "head"] {
        let repo = repo(); let mut team = team(&repo);
        let prev = head(&repo);
        write(&repo.lead, "old.txt", "old\n"); run_git(&repo.lead, &["add", "."]); run_git(&repo.lead, &["commit", "-q", "-m", "old"]);
        let merged = head(&repo);
        team.task_mut(1).unwrap().merge = Some(MergeEntry { state: "queued".into(), author: "dev".into(),
            commit: merged.clone(), prev: prev.clone(), merged: merged.clone(), ..Default::default() });
        let lock = repo.lead.join(".git/index.lock"); std::fs::write(&lock, "fixture").unwrap();
        let database = database(&repo); let mut events = Vec::new();
        assert!(step_with(&mut team, &database, 100.0, &mut events, || panic!("rollback reads no runners")).unwrap());
        assert_eq!(team.task(1).unwrap().merge.as_ref().unwrap().wait_reason, MergeWaitReason::Rollback);
        std::fs::remove_file(lock).unwrap();
        // Even a map change must not retry reset while HEAD is unchanged.
        let config = repo.lead.join(".zerus/tests.toml"); let original = std::fs::read_to_string(&config).unwrap();
        std::fs::write(&config, format!("{original}\n# map changed\n")).unwrap();
        assert!(!step_with(&mut team, &database, 101.0, &mut events, || panic!("a held rollback reads no runners")).unwrap());
        assert_eq!(head(&repo), merged);
        std::fs::write(&config, original).unwrap();
        if resume == "retry" {
            ops::apply(&mut team, "retry", &json!({"task":1}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
            assert!(step_with(&mut team, &database, 102.0, &mut events, || panic!("rollback reads no runners")).unwrap());
            assert_eq!(head(&repo), prev);
        } else {
            write(&repo.lead, "next.txt", "next\n"); run_git(&repo.lead, &["add", "."]); run_git(&repo.lead, &["commit", "-q", "-m", "next"]);
            step_here(&mut team, &database, 102.0, &mut events).unwrap();
            assert_eq!(team.task(1).unwrap().merge.as_ref().unwrap().state, "testing");
        }
    }
}

#[test]
fn merge_waiting_filter_and_guidance_use_the_typed_reason_not_note_words() {
    let repo = repo(); let mut team = team(&repo); accept(&mut team);
    write(&repo.lead, "a.txt", "dirty\n");
    let database = database(&repo); let mut events = Vec::new();
    step_here(&mut team, &database, 100.0, &mut events).unwrap();
    assert_eq!(team.task(1).unwrap().merge.as_ref().unwrap().wait_reason, MergeWaitReason::Busy);
    team.task_mut(1).unwrap().merge.as_mut().unwrap().note = "test runners could not be read".into();
    let items = crate::state::teams::inbox::items_for(&team, "lead");
    assert!(!items.iter().any(|item| item.kind == "merge_waiting"));
    let entry = team.task_mut(1).unwrap().merge.as_mut().unwrap();
    entry.wait_reason = MergeWaitReason::Runners; entry.note = "the lead's folder has uncommitted changes".into();
    let detail = crate::state::teams::inbox::items_for(&team, "lead").into_iter().find(|item| item.kind == "merge_waiting").unwrap().detail;
    assert!(detail.contains("Fix the runner or machine config"), "{detail}");
    let mut saved = serde_json::to_value(&team).unwrap();
    saved["tasks"][0]["merge"]["wait_reason"] = json!("a_future_reason");
    let loaded: Team = serde_json::from_value(saved).unwrap();
    assert_eq!(loaded.task(1).unwrap().merge.as_ref().unwrap().wait_reason, MergeWaitReason::Other);
}


#[test]
fn a_git_error_while_recording_a_range_preserves_its_older_start() {
    let repo = repo();
    let mut team = team(&repo);
    let older = marked_range(&repo, &mut team);
    write(&repo.lead, "later.txt", "later\n");
    run_git(&repo.lead, &["add", "."]);
    run_git(&repo.lead, &["commit", "-q", "-m", "later merge"]);
    let latest = head(&repo);
    let task = team.task_mut(1).unwrap();
    task.status = TaskStatus::Pending;
    task.merge = Some(MergeEntry {
        state: "reverting".into(),
        author: "dev".into(),
        commit: latest.clone(),
        prev: older.merged.clone(),
        merged: latest.clone(),
        ..Default::default()
    });
    // A transient repository read failure, without changing permissions or
    // moving any repository: both ancestry reads must return Unknown.
    team.folder = repo.lead.join("temporarily-unavailable").display().to_string();
    assert_eq!(ancestry_at(&team.folder, &older.merged, &latest), Ancestry::Unknown);
    let database = database(&repo);
    let mut events = Vec::new();
    assert!(step_with(&mut team, &database, 100.0, &mut events,
        || panic!("migration of a returned task reads no runners")).unwrap());
    let task = team.task(1).unwrap();
    assert!(task.merge.is_none());
    assert_eq!(task.status, TaskStatus::Pending);
    let expected = MergeRange { task: 1, prev: older.prev, merged: latest.clone() };
    assert_eq!(task.untested_merge.as_ref(), Some(&expected));
    assert_eq!(head(&repo), latest);
    assert!(database.queued("t1").unwrap().is_empty());
    let loaded: Team = serde_json::from_value(serde_json::to_value(&team).unwrap()).unwrap();
    assert_eq!(loaded.task(1).unwrap().untested_merge.as_ref(), Some(&expected));
}

#[test]
fn a_saved_synthetic_merge_suite_migrates_with_its_original_start() {
    let repo = repo();
    let original = head(&repo);
    write(&repo.lead, "first.txt", "first\n");
    run_git(&repo.lead, &["add", "."]);
    run_git(&repo.lead, &["commit", "-q", "-m", "first"]);
    let parent = head(&repo);
    write(&repo.lead, "second.txt", "second\n");
    run_git(&repo.lead, &["add", "."]);
    run_git(&repo.lead, &["commit", "-q", "-m", "second"]);
    let merged = head(&repo);
    let mut team = team(&repo);
    team.task_mut(1).unwrap().status = TaskStatus::Pending;
    let message = format!("not run here: merge-1 runs on all; the merge of {merged} was not tested on {merged}");
    team.merge_not_run.insert("merge-1".into(), OpenNotRun { since: original.clone(), message: message.clone() });
    // The log's newer start must not replace the original persisted start.
    team.merge_log.push(MergeLog {
        task: 2, prev: parent.clone(), merged: merged.clone(), not_run: vec![message.clone()], ..Default::default()
    });
    let saved = serde_json::to_value(team).unwrap();
    assert!(!saved["tasks"][0].as_object().unwrap().contains_key("untested_merge"));
    let mut loaded: Team = serde_json::from_value(saved).unwrap();
    let database = database(&repo);
    let mut events = Vec::new();
    assert!(step_with(&mut loaded, &database, 100.0, &mut events,
        || panic!("legacy metadata migration reads no runners")).unwrap());
    let expected = MergeRange { task: 1, prev: original, merged: merged.clone() };
    assert_ne!(expected.prev, parent);
    assert_eq!(loaded.task(1).unwrap().untested_merge.as_ref(), Some(&expected));
    assert!(loaded.merge_not_run.is_empty());
    assert!(loaded.merge_log.iter().all(|log| !log.not_run.contains(&message)));
    let open = untested_since(&loaded).unwrap();
    assert!(open.suites().is_empty(), "the synthetic suite cannot reappear from the log");
    assert_eq!(open.merges, vec![expected.clone()]);
    let mut loaded: Team = serde_json::from_value(serde_json::to_value(loaded).unwrap()).unwrap();
    assert!(!step_with(&mut loaded, &database, 101.0, &mut events,
        || panic!("an already migrated range reads no runners")).unwrap());
    assert_eq!(loaded.task(1).unwrap().untested_merge.as_ref(), Some(&expected));
    assert_eq!(head(&repo), merged);
    assert!(database.queued("t1").unwrap().is_empty());
}

/// Task 174: this team's layout: the team's folder stays on its own branch,
/// the owner's integration folder (a worktree on the integration branch)
/// takes the merge and its tests, and members merge from it.
#[test]
fn the_owners_integration_folder_takes_the_merge_and_its_tests() {
    let repo = repo();
    let integration = repo.lead.parent().unwrap().join("integration");
    run_git(&repo.lead, &["worktree", "add", "-q", "-b", "phase3", &integration.to_string_lossy()]);
    // The team's folder moves on alone: the queue never merges there.
    write(&repo.lead, "lead-only.txt", "lead\n");
    run_git(&repo.lead, &["add", "."]);
    run_git(&repo.lead, &["commit", "-q", "-m", "lead only"]);
    let lead_before = head(&repo);
    write(&repo.member, "b.txt", "work\n");
    run_git(&repo.member, &["add", "."]);
    run_git(&repo.member, &["commit", "-q", "-m", "work"]);
    let mut team = team(&repo);
    team.settings.integration_folder = integration.to_string_lossy().into_owned();
    team.line_branch = "phase3".into();
    assert_eq!(team.integration_folder(), integration.to_string_lossy());
    assert_eq!(accept(&mut team)["status"], "merge_queued");
    let database = database(&repo);
    let events = settle(&mut team, &database, &integration);
    assert_eq!(head(&repo), lead_before, "the team's folder is untouched");
    assert!(integration.join("b.txt").is_file(), "the work is merged into the integration branch");
    assert!(!integration.join("lead-only.txt").exists());
    assert_eq!(run_git(&integration, &["rev-parse", "--abbrev-ref", "HEAD"]), "phase3");
    let task = team.task(1).unwrap();
    assert_eq!(task.status, TaskStatus::Accepted, "{events:?}");
    assert!(events.iter().any(|event| event.kind == "merged"), "{events:?}");
}

/// Task 174: an integration folder that cannot take a merge (missing, another
/// repository, a detached HEAD) holds the queue with that note.
#[test]
fn an_unusable_integration_folder_holds_the_queue_with_its_note() {
    let repo = repo();
    write(&repo.member, "b.txt", "work\n");
    run_git(&repo.member, &["add", "."]);
    run_git(&repo.member, &["commit", "-q", "-m", "work"]);
    let mut team = team(&repo);
    assert_eq!(accept(&mut team)["status"], "merge_queued");
    let parent = repo.lead.parent().unwrap().to_path_buf();
    let other = parent.join("other");
    std::fs::create_dir_all(&other).unwrap();
    run_git(&other, &["init", "-q"]);
    write(&other, "x.txt", "x\n");
    run_git(&other, &["add", "."]);
    run_git(&other, &["commit", "-q", "-m", "other"]);
    let detached = parent.join("detached");
    run_git(&repo.lead, &["worktree", "add", "-q", "--detach", &detached.to_string_lossy()]);
    let lead_before = head(&repo);
    let database = database(&repo);
    for (folder, said) in [(parent.join("missing"), "does not exist"), (other, "is not a checkout of the team's repository"), (detached, "detached HEAD")] {
        team.settings.integration_folder = folder.to_string_lossy().into_owned();
        let mut events = Vec::new();
        step_here(&mut team, &database, 100.0, &mut events).unwrap();
        let entry = team.task(1).unwrap().merge.clone().unwrap();
        assert_eq!(entry.state, "queued");
        assert!(entry.note.contains(said), "{said}: {}", entry.note);
        assert_eq!(entry.wait_reason, MergeWaitReason::Busy);
    }
    assert_eq!(head(&repo), lead_before, "nothing was merged anywhere");
    assert!(database.queued(&team.id).unwrap().is_empty(), "nor tested");
}







fn ctest_repo() -> Repo {
    let repo = repo_providers(r#"[[provider]]
name = "gui"
kind = "ctest"
build_dir = "build/gui"
sources = ["a.txt", "tests/**", "tests.txt"]
tests = ["tests/*.cpp"]
configure = '"{python}" .zerus/fixture.py configure "{build_dir}"'
build = '"{python}" .zerus/fixture.py build "{build_dir}" {targets}'
probe = '"{python}" .zerus/fixture.py probe "{build_dir}"'
command = '"{python}" .zerus/fixture.py test "{build_dir}" {targets}'
"#);
    write(&repo.lead, ".gitignore", "build/\nstages.log\n.zerus/tests.local.toml\n");
    std::fs::create_dir(repo.lead.join("tests")).unwrap();
    write(&repo.lead, "tests/test_old.cpp", "old test\n");
    write(&repo.lead, "tests.txt", "test_old\n");
    write(&repo.lead, ".zerus/fixture.py", r#"import os, pathlib, sys
stage, folder = sys.argv[1:3]
folder = pathlib.Path(folder)
assert os.environ.get('FIXTURE_LOCAL') == 'yes'
assert stage == 'test' or os.environ.get('EXPECT_OWNER_STAGE') != 'yes' or os.environ.get('OWNER_STAGE') == 'yes'
with open('stages.log', 'a') as log:
    log.write(stage + ' ' + ' '.join(sys.argv[3:]) + '\n')
if stage == 'probe':
    if os.environ.get('PROBE_READY') == 'no':
        print('x' * 5000)
        print('fixture compiler is unavailable')
        sys.exit(9)
elif stage == 'configure':
    folder.mkdir(parents=True, exist_ok=True)
    (folder / 'CMakeCache.txt').write_text('fixture cache')
elif stage == 'build':
    if pathlib.Path('fail-build.txt').exists():
        print('fixture source build failed')
        sys.exit(7)
    assert (folder / 'CMakeCache.txt').exists()
    (folder / 'built.txt').write_text(pathlib.Path('tests.txt').read_text())
elif stage == 'test':
    assert (folder / 'built.txt').read_text() == pathlib.Path('tests.txt').read_text(), 'stale test binary'
    print('Ran ' + (folder / 'built.txt').read_text().strip())
"#);
    write(&repo.lead, ".zerus/test-map.json", r#"{"schema":1,"providers":{"gui":{"kind":"ctest","rule":"build","targets":{"test_old":{"run":["old"],"tests":1}},"files":{"a.txt":["test_old"],"tests/test_old.cpp":["test_old"]},"test_files":{"tests/test_old.cpp":["test_old"]}}}}"#);
    run_git(&repo.lead, &["add", "."]);
    run_git(&repo.lead, &["commit", "-q", "-m", "fixture recipes"]);
    run_git(&repo.member, &["merge", "--ff-only", &head(&repo)]);
    write_local(&repo, "yes");
    crate::test_discipline::config::load(&repo.lead, None).unwrap();
    repo
}

fn write_local(repo: &Repo, ready: &str) {
    let python = if cfg!(windows) { "python" } else { "python3" };
    write(&repo.lead, ".zerus/tests.local.toml", &format!(
        "[provider.gui.vars]\npython = '{python}'\n[provider.gui.env]\nFIXTURE_LOCAL = 'yes'\nPROBE_READY = '{ready}'\n"));
}

fn ctest_change(repo: &Repo) {
    write(&repo.member, "a.txt", "changed\n");
    run_git(&repo.member, &["add", "."]);
    run_git(&repo.member, &["commit", "-q", "-m", "change"]);
}

#[test]
fn ctest_queue_configures_missing_folder_then_builds_selected_targets() {
    let repo = ctest_repo();
    ctest_change(&repo);
    let mut team = team(&repo);
    accept(&mut team);
    let database = database(&repo);
    settle(&mut team, &database, &repo.lead);
    assert_eq!(team.task(1).unwrap().status, TaskStatus::Accepted);
    let stages = std::fs::read_to_string(repo.lead.join("stages.log")).unwrap();
    assert_eq!(stages.lines().map(|line| line.split_whitespace().next().unwrap()).collect::<Vec<_>>(), ["probe", "configure", "build", "test"]);
    assert!(stages.contains("build --target test_old"), "{stages}");
}

#[test]
fn ctest_queue_uses_owner_overrides_for_every_preparation_stage() {
    let repo = ctest_repo();
    write(&repo.member, ".zerus/owner.py", "import os, runpy, sys\nassert sys.argv.pop(1) == 'owner'\nos.environ['OWNER_STAGE'] = 'yes'\nrunpy.run_path('.zerus/fixture.py', run_name='__main__')\n");
    let mut local = std::fs::read_to_string(repo.lead.join(".zerus/tests.local.toml")).unwrap();
    local.push_str("EXPECT_OWNER_STAGE = 'yes'\n");
    write(&repo.lead, ".zerus/tests.local.toml", &local);
    ctest_change(&repo);
    let mut team = team(&repo);
    team.settings.tests = Some(json!({"provider":[{
        "name":"gui", "build_dir":"build/owner",
        "probe":"\"{python}\" .zerus/owner.py owner probe \"{build_dir}\"",
        "configure":"\"{python}\" .zerus/owner.py owner configure \"{build_dir}\"",
        "build":"\"{python}\" .zerus/owner.py owner build \"{build_dir}\" {targets}"
    }]}));
    accept(&mut team);
    let database = database(&repo);
    settle(&mut team, &database, &repo.lead);
    assert_eq!(team.task(1).unwrap().status, TaskStatus::Accepted);
    assert!(repo.lead.join("build/owner/CMakeCache.txt").is_file());
    assert!(repo.lead.join("build/owner/built.txt").is_file());
    assert!(!repo.lead.join("build/gui").exists());
    let stages = std::fs::read_to_string(repo.lead.join("stages.log")).unwrap();
    assert_eq!(stages.lines().map(|line| line.split_whitespace().next().unwrap()).collect::<Vec<_>>(), ["probe", "configure", "build", "test"]);
}

#[test]
fn ctest_queue_rebuilds_stale_folder_and_runs_a_new_test_missing_from_the_map() {
    let repo = ctest_repo();
    std::fs::create_dir_all(repo.lead.join("build/gui")).unwrap();
    write(&repo.lead, "build/gui/CMakeCache.txt", "old cache");
    write(&repo.lead, "build/gui/built.txt", "test_old\n");
    write(&repo.member, "tests/test_new.cpp", "new test\n");
    write(&repo.member, "tests.txt", "test_old\ntest_new\n");
    run_git(&repo.member, &["add", "."]);
    run_git(&repo.member, &["commit", "-q", "-m", "new test"]);
    let mut team = team(&repo);
    accept(&mut team);
    let database = database(&repo);
    settle(&mut team, &database, &repo.lead);
    assert_eq!(team.task(1).unwrap().status, TaskStatus::Accepted);
    assert_eq!(std::fs::read_to_string(repo.lead.join("stages.log")).unwrap().lines().collect::<Vec<_>>(), ["probe ", "build ", "test "]);
    assert_eq!(std::fs::read_to_string(repo.lead.join("build/gui/built.txt")).unwrap().lines().collect::<Vec<_>>(), ["test_old", "test_new"]);
}

#[test]
fn ctest_queue_build_failure_fails_the_suite_and_rolls_back() {
    let repo = ctest_repo();
    write(&repo.member, "fail-build.txt", "bad source");
    ctest_change(&repo);
    let prev = head(&repo);
    let mut team = team(&repo);
    accept(&mut team);
    let database = database(&repo);
    let mut events = Vec::new();
    step_here(&mut team, &database, 100.0, &mut events).unwrap();
    let queued = database.queued(&team.id).unwrap();
    let records = run_due(&database, &repo.lead, &queued, 101.0,
        &super::super::test_results::Policy { retries: 1, ..Default::default() }, None).unwrap();
    assert_eq!(records[0].status, "failed");
    assert!(records[0].flags.iter().any(|flag| flag == "build"));
    step_here(&mut team, &database, 102.0, &mut events).unwrap();
    assert_eq!(team.task(1).unwrap().status, TaskStatus::Pending);
    assert_eq!(head(&repo), prev);
    assert!(team.task(1).unwrap().decision_reason.contains("fixture source build failed"));
    let stages = std::fs::read_to_string(repo.lead.join("stages.log")).unwrap();
    assert_eq!(stages.lines().filter(|line| line.starts_with("build ")).count(), 1);
    assert!(!stages.lines().any(|line| line.starts_with("test ")));
}

#[test]
fn ctest_queue_probe_failure_is_not_run_or_green_on_a_later_run() {
    let repo = ctest_repo();
    write_local(&repo, "no");
    ctest_change(&repo);
    let mut team = team(&repo);
    let prev = head(&repo);
    accept(&mut team);
    let database = database(&repo);
    let mut events = Vec::new();
    step_here(&mut team, &database, 100.0, &mut events).unwrap();
    let queued = database.queued(&team.id).unwrap();
    let first = run_due(&database, &repo.lead, &queued, 101.0, &Default::default(), None).unwrap();
    assert_eq!(first[0].status, "not_run");
    assert!(first[0].excerpt.contains("fixture compiler is unavailable"));
    step_here(&mut team, &database, 102.0, &mut events).unwrap();
    assert_eq!(team.task(1).unwrap().status, TaskStatus::Accepted);
    assert_ne!(head(&repo), prev);
    assert!(team.merge_log[0].suites.is_empty());
    assert!(team.merge_log[0].not_run[0].starts_with("not run here: gui runs on "));
    assert!(team.merge_tested.is_empty() && !team.merge_tested_suites.contains_key("gui"));
    assert!(team.merge_not_run.contains_key("gui"));
    assert!(database.last_green(&team.id, "gui", "").unwrap().is_none());
    let mut next = queued[0].clone();
    next.id = uuid::Uuid::new_v4().to_string();
    database.enqueue(&next).unwrap();
    let second = run_due(&database, &repo.lead, &[next], 103.0, &Default::default(), None).unwrap();
    assert_eq!(second[0].status, "not_run");
    assert!(database.last_green(&team.id, "gui", "").unwrap().is_none());
}

#[test]
fn ctest_queue_refuses_tracked_or_unknown_local_configuration() {
    for case in ["tracked", "provider", "key"] {
        let repo = ctest_repo();
        ctest_change(&repo);
        let mut team = team(&repo);
        accept(&mut team);
        match case {
            "tracked" => { run_git(&repo.lead, &["add", "-f", ".zerus/tests.local.toml"]); run_git(&repo.lead, &["commit", "-q", "-m", "tracked local"]); }
            "provider" => write(&repo.lead, ".zerus/tests.local.toml", "[provider.unknown.vars]\nx = 'y'\n"),
            _ => write(&repo.lead, ".zerus/tests.local.toml", "[provider.gui]\ncommand = 'not allowed'\n"),
        }
        let database = database(&repo);
        let mut events = Vec::new();
        step_here(&mut team, &database, 100.0, &mut events).unwrap();
        let note = &team.task(1).unwrap().merge.as_ref().unwrap().note;
        assert!(note.contains("local") && (note.contains("tracked") || note.contains("unknown") || note.contains("command")), "{case}: {note}");
        assert!(database.queued(&team.id).unwrap().is_empty());
    }
}


#[test]
fn ctest_queue_missing_probe_program_or_placeholder_is_not_run() {
    for missing_program in [false, true] {
        let repo = ctest_repo();
        if missing_program {
            write(&repo.lead, ".zerus/tests.local.toml", "[provider.gui.vars]\npython = 'zerus-fixture-missing-program'\n");
        } else {
            write(&repo.lead, ".zerus/tests.local.toml", "[provider.gui.env]\nFIXTURE_LOCAL = 'yes'\n");
        }
        ctest_change(&repo);
        let mut team = team(&repo);
        accept(&mut team);
        let database = database(&repo);
        let mut events = Vec::new();
        step_here(&mut team, &database, 100.0, &mut events).unwrap();
        let queued = database.queued(&team.id).unwrap();
        let records = run_due(&database, &repo.lead, &queued, 101.0, &Default::default(), None).unwrap();
        assert_eq!(records[0].status, "not_run");
        assert!(records[0].excerpt.starts_with("not run here: gui runs on "));
        assert!(!repo.lead.join("build/gui/CMakeCache.txt").exists());
        assert!(database.last_green(&team.id, "gui", "").unwrap().is_none());
    }
}

/// Task #393 (owner, 2026-11-08): a landing the queue recorded is on the line
/// already; the finished task is named landed from its record, not read from
/// Git again. The line here holds no such commit, so only the record can
/// decide it.
#[test]
fn a_landing_the_queue_recorded_is_not_read_from_git_again() {
    let repo = repo();
    let mut team = team(&repo);
    team.settings.integration_folder = repo.lead.to_string_lossy().into_owned();
    let merged = "0123456789abcdef0123456789abcdef01234567".to_owned();
    team.task_mut(1).unwrap().status = TaskStatus::Accepted;
    team.task_mut(1).unwrap().merge = Some(MergeEntry {
        state: "landed".into(),
        author: "dev".into(),
        commit: merged.clone(),
        merged: merged.clone(),
        ..Default::default()
    });
    let view = landing(&team);
    // Each landed entry names its task and commit; task 370 adds how the landing was recognized.
    let landed = view["landed"].as_array().unwrap();
    assert_eq!(landed.len(), 1, "{}", view);
    assert_eq!((landed[0]["task"].clone(), landed[0]["commit"].clone()), (json!(1), json!(merged)), "{}", view);
    assert!(view["unlanded"].as_array().unwrap().is_empty(), "{}", view);
}
/// Task #370: a cherry-pick of the accepted change counts as landed, and the
/// lead may mark a landing by a named line commit.
#[test]
fn cherry_picked_work_counts_as_landed_and_the_lead_can_mark_a_landing() {
    let repo = repo();
    write(&repo.member, "work.txt", "work\n");
    run_git(&repo.member, &["add", "."]);
    run_git(&repo.member, &["commit", "-q", "-m", "The accepted work"]);
    let accepted = run_git(&repo.member, &["rev-parse", "HEAD"]);
    // The merger cherry-picks the accepted change onto the line.
    run_git(&repo.lead, &["cherry-pick", &accepted]);
    // The line keeps the change with a commit of its own: the patch, not the sha, matches.
    run_git(&repo.lead, &["commit", "--amend", "-q", "-m", "Land: the accepted work"]);
    let line = head(&repo);
    let mut team = team(&repo);
    team.task_mut(1).unwrap().status = TaskStatus::Accepted;
    team.task_mut(1).unwrap().merge = None;
    let view = landing(&team);
    assert!(view["unlanded"].as_array().unwrap().is_empty(), "{view}");
    assert_eq!(view["landed"][0]["task"], 1);
    assert_eq!(view["landed"][0]["equivalent"], true, "{view}");
    // A commit the line does not carry is unlanded until the lead marks the
    // landing by a commit that is on the line.
    write(&repo.member, "more.txt", "more\n");
    run_git(&repo.member, &["add", "."]);
    run_git(&repo.member, &["commit", "-q", "-m", "More accepted work"]);
    let more = run_git(&repo.member, &["rev-parse", "HEAD"]);
    team.task_mut(1).unwrap().attempts[0].folder_head = more.clone();
    let view = landing(&team);
    assert_eq!(view["unlanded"][0]["task"], 1, "{view}");
    let mut events = Vec::new();
    let value = ops::apply(&mut team, "landed", &json!({"task":1,"commit":line}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut events).unwrap();
    assert_eq!(value["status"], "landed");
    assert!(events.iter().any(|event| event.kind == "task_marked_landed"), "{events:?}");
    let view = landing(&team);
    assert!(view["unlanded"].as_array().unwrap().is_empty(), "{view}");
    assert_eq!(view["landed"][0]["marked"], line);
    // The line commit must be on the line.
    let error = ops::apply(&mut team, "landed", &json!({"task":1,"commit":more}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut events).unwrap_err().to_string();
    assert!(error.contains("not on the integration line"), "{error}");
}

/// Task #370: a landing refuses a range carrying a rejected attempt's commit
/// and offers a cherry-pick of the task's own commit.
#[test]
fn a_landing_refuses_a_range_with_a_rejected_attempt_and_offers_a_cherry_pick() {
    let repo = repo();
    write(&repo.member, "rejected.txt", "rejected\n");
    run_git(&repo.member, &["add", "."]);
    run_git(&repo.member, &["commit", "-q", "-m", "A rejected try"]);
    let rejected = run_git(&repo.member, &["rev-parse", "HEAD"]);
    write(&repo.member, "accepted.txt", "accepted\n");
    run_git(&repo.member, &["add", "."]);
    run_git(&repo.member, &["commit", "-q", "-m", "The accepted work"]);
    let accepted = run_git(&repo.member, &["rev-parse", "HEAD"]);
    let mut team = team(&repo);
    team.settings.integration_folder = repo.lead.to_string_lossy().into_owned();
    team.task_mut(1).unwrap().attempts[0].folder_head = accepted.clone();
    team.task_mut(1).unwrap().attempts.push(Attempt { n: 2, id: "A0".into(), member: "dev".into(), state: AttemptState::Rejected, folder_head: rejected.clone(), ..Default::default() });
    accept(&mut team);
    let database = database(&repo);
    let mut events = Vec::new();
    step_here(&mut team, &database, 100.0, &mut events).unwrap();
    let entry = team.task(1).unwrap().merge.clone().unwrap();
    assert!(entry.blocked, "{entry:?}");
    assert!(entry.note.contains("rejected attempt") && entry.note.contains("cherry-pick"), "{}", entry.note);
    assert!(!repo.lead.join("accepted.txt").exists() && !repo.lead.join("rejected.txt").exists());
    // The queue's advice is the cherry-pick landing of the task's own commit.
    let inbox = crate::state::teams::inbox::items(&team);
    let item = inbox.iter().find(|item| item.kind == "merge_waiting").unwrap_or_else(|| panic!("no merge_waiting item: {inbox:?}"));
    assert!(item.command.contains("hgs team land") && item.command.contains("cherry_pick"), "{}", item.command);
    ops::apply(&mut team, "land", &json!({"task":1,"cherry_pick":true}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut events).unwrap();
    step_here(&mut team, &database, 101.0, &mut events).unwrap();
    assert!(repo.lead.join("accepted.txt").exists());
    assert!(!repo.lead.join("rejected.txt").exists());
}

/// Task #370: work that belongs to another repository is never unlanded here.
#[test]
fn an_out_of_repo_task_is_never_unlanded() {
    let repo = repo();
    let mut team = team(&repo);
    team.task_mut(1).unwrap().status = TaskStatus::Accepted;
    team.task_mut(1).unwrap().merge = None;
    team.task_mut(1).unwrap().attempts[0].folder_head = "0123456789abcdef0123456789abcdef01234567".into();
    let view = landing(&team);
    assert!(view["unlanded"].as_array().unwrap().is_empty(), "{view}");
    assert_eq!(view["landed"][0]["outside"], true, "{view}");
    assert!(crate::state::teams::inbox::items(&team).iter().all(|item| item.kind != "unlanded"));
}

// Task 377 (owner, 2026-11-08): the line is a named branch. A folder left on
// another checkout lands nothing: the task keeps its queue note, the lead gets
// a merge_wrong_checkout event, and the task is not reopened.
#[test]
fn a_checkout_that_is_not_the_line_holds_the_landing_with_a_notice() {
    let repo = repo();
    let mut team = team(&repo);
    team.settings.integration_folder = repo.lead.to_string_lossy().into_owned();
    // The line's name, while the folder's checkout is master.
    team.line_branch = "release".into();
    assert_eq!(accept(&mut team)["status"], "merge_queued");
    let database = database(&repo);
    let mut events = Vec::new();
    let landed = step_here(&mut team, &database, 100.0, &mut events).unwrap();
    assert!(!landed, "nothing lands on a wrong checkout");
    let entry = team.task(1).unwrap().merge.clone().expect("the queued landing");
    assert!(entry.note.contains("release") && entry.note.contains("master"), "{}", entry.note);
    assert!(entry.merged.is_empty(), "no merge was made");
    let kinds: Vec<String> = events.iter().map(|event| event.kind.to_string()).collect();
    assert!(kinds.iter().any(|kind| kind == "merge_wrong_checkout"), "{kinds:?}");
}

// Task 377 (owner, 2026-11-08): a merge runs the project's declared generators
// and folds their output into the merge commit, so a generated catalog keeps
// both sides' strings instead of whichever side the merge happened to keep.
#[test]
fn a_merge_regenerates_the_projects_generated_files_into_its_commit() {
    let repo = repo();
    // The project declares a generator; the line's copy of its output is stale.
    let path = repo.lead.join(".zerus/tests.toml");
    let config = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, format!("generate = [\"echo fresh > generated.txt\"]\n{config}")).unwrap();
    write(&repo.lead, "generated.txt", "stale\n");
    run_git(&repo.lead, &["add", "."]);
    run_git(&repo.lead, &["commit", "-q", "-m", "the project declares its generator"]);
    let mut team = team(&repo);
    team.settings.integration_folder = repo.lead.to_string_lossy().into_owned();
    assert_eq!(accept(&mut team)["status"], "merge_queued");
    let database = database(&repo);
    let events = settle(&mut team, &database, &repo.lead);
    assert_eq!(team.task(1).unwrap().status, TaskStatus::Accepted, "{events:?}");
    let generated = std::fs::read_to_string(repo.lead.join("generated.txt")).unwrap();
    assert_eq!(generated.trim(), "fresh", "the generator ran over the stale copy");
    // The regenerated file is part of the merge commit, not a dirty leftover.
    let head = head(&repo);
    let files = run_git(&repo.lead, &["show", "--name-only", "--pretty=format:", &head]);
    assert!(files.lines().any(|file| file == "generated.txt"), "{files}");
    assert!(run_git(&repo.lead, &["status", "--porcelain"]).is_empty(), "the merge commit carries it");
}
