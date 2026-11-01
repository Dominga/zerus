use super::*;
use crate::state::teams::model::{Attempt, Member, MemberState, PlanMode, Report, TeamStatus};
use crate::state::teams::ops::{self, Actor};
use crate::state::teams::test_queue::run_due;
use crate::state::teams::test_runners::Runner;
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
    run_due(database, lead, &queued, 101.0, &Default::default()).unwrap();
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
    run_due(&database, &repo.lead, &queued, 101.0, &Default::default()).unwrap();
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
    run_due(&database, &repo.lead, &queued, 101.0, &Default::default()).unwrap();
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
    run_due(&database, &repo.lead, &queued, 201.0, &Default::default()).unwrap();
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
fn a_config_error_after_the_merge_keeps_it_waiting_with_the_reason() {
    let repo = repo();
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
    let entry = task.merge.as_ref().expect("the merge waits");
    assert_eq!(entry.state, "waiting");
    assert!(entry.note.contains("test runners could not be read: boom"), "{}", entry.note);
    assert_ne!(head(&repo), before, "the merge is in the lead branch");
    assert!(events.iter().any(|event| event.kind == "merge_config_failed"), "{events:?}");
    // The same runner error on a later tick changes nothing and adds no event.
    assert!(!step_with(&mut team, &database, 101.0, &mut events, || Err(anyhow::anyhow!("boom"))).unwrap());
    assert_eq!(events.iter().filter(|event| event.kind == "merge_config_failed").count(), 1, "{events:?}");
}

#[test]
fn a_selection_error_rolls_the_merge_back_and_returns_the_task() {
    let repo = repo();
    write(&repo.member, "b.txt", "work\n");
    run_git(&repo.member, &["add", "."]);
    run_git(&repo.member, &["commit", "-q", "-m", "work"]);
    let before = head(&repo);
    let mut team = team(&repo);
    accept(&mut team);
    // The merge itself is fine; selecting its tests is not.
    std::fs::write(repo.lead.join(".zerus/tests.toml"), "version = 1\n[merge_queue\n").unwrap();
    run_git(&repo.lead, &["add", ".zerus/tests.toml"]);
    run_git(&repo.lead, &["commit", "-q", "-m", "break the test map"]);
    let broken = head(&repo);
    let database = database(&repo);
    let mut events = Vec::new();
    assert!(step_here(&mut team, &database, 100.0, &mut events).unwrap());
    let task = team.task(1).unwrap();
    assert_eq!(task.status, TaskStatus::Pending, "a broken selection rolls back and returns");
    assert!(task.merge.is_none());
    assert!(task.decision_reason.contains("affected tests could not be selected"), "{}", task.decision_reason);
    assert_eq!(head(&repo), broken, "the merge is rolled back to its prev");
    assert_ne!(head(&repo), before);
    assert!(events.iter().any(|event| event.kind == "merge_affected_failed"), "{events:?}");
    assert!(events.iter().any(|event| event.kind == "merge_returned"), "{events:?}");
    assert!(database.queued("t1").unwrap().is_empty());
}

#[test]
fn a_waiting_merge_retries_without_merging_again_and_blocks_later_merges() {
    let repo = repo_with(&platform_providers());
    write(&repo.member, "b.txt", "one\n");
    run_git(&repo.member, &["add", "."]);
    run_git(&repo.member, &["commit", "-q", "-m", "one"]);
    let mut team = team(&repo);
    accept(&mut team);
    let database = database(&repo);
    let mut events = Vec::new();
    step_with(&mut team, &database, 100.0, &mut events, || Err(anyhow::anyhow!("boom"))).unwrap();
    let merged = head(&repo);
    assert_eq!(team.task(1).unwrap().merge.as_ref().unwrap().state, "waiting");
    // A second accepted task queues behind the waiting one.
    write(&repo.member, "b.txt", "two\n");
    run_git(&repo.member, &["add", "."]);
    run_git(&repo.member, &["commit", "-q", "-m", "two"]);
    let second = run_git(&repo.member, &["rev-parse", "HEAD"]);
    team.tasks.push(Task {
        id: 2,
        title: "Behind".into(),
        assignee: "dev".into(),
        status: TaskStatus::Submitted,
        attempts: vec![Attempt {
            n: 1,
            id: "A2".into(),
            member: "dev".into(),
            state: AttemptState::Reported,
            folder_head: second,
            folder_branch: "task/1".into(),
            report: Some(Report { text: "two".into(), at: 2.0, ..Default::default() }),
            ..Default::default()
        }],
        ..Default::default()
    });
    ops::apply(&mut team, "accept", &json!({"task":2}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut events).unwrap();
    assert_eq!(team.task(2).unwrap().merge.as_ref().unwrap().state, "queued");
    // The retry selects the first merge's tests without merging it again and
    // does not let the second merge pass it.
    let host = vec!["windows".to_owned()];
    step_with(&mut team, &database, 101.0, &mut events, || Ok((host.clone(), Vec::new()))).unwrap();
    let first = team.task(1).unwrap().merge.as_ref().unwrap();
    assert_eq!(first.state, "testing");
    assert_eq!(team.task(2).unwrap().merge.as_ref().unwrap().state, "queued");
    assert_eq!(head(&repo), merged, "the first merge was not merged again");
    assert!(team.merge_tested.is_empty(), "a later merge must not mark the waiting range tested");
    assert!(first.not_run.iter().any(|message| message.contains("python")), "{:?}", first.not_run);
    let queued = database.queued("t1").unwrap();
    let suites: Vec<&str> = queued.iter().map(|request| request.suite.as_str()).collect();
    assert!(suites.contains(&"unit") && !suites.contains(&"python"), "{suites:?}");
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
    let mut team = Team::default();
    team.merge_tested = "head60".into();
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
    run_due(&database, &repo.lead, &queued, 101.0, &Default::default()).unwrap();
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
    run_due(&database, &repo.lead, &queued, 101.0, &Default::default()).unwrap();
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
    run_due(&database, &repo.lead, &queued, 101.0, &Default::default()).unwrap();
    step(&mut team, &database, 102.0, &mut events, &host, &[]).unwrap();
    let task = team.task(1).unwrap();
    assert_eq!(task.status, TaskStatus::Pending);
    assert!(task.decision_reason.contains("affected tests failed"), "{}", task.decision_reason);
    assert!(task.decision_reason.contains("Not run here:") && task.decision_reason.contains("python"), "{}", task.decision_reason);
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
    assert!(step_here(&mut team, &database, 100.0, &mut events).unwrap());
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
