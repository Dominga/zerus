use super::*;
use super::support::*;

#[test]
fn a_complete_testing_merge_does_not_read_the_machine_config() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let team = Team {
            id: "t".into(),
            name: "T".into(),
            lead: "lead".into(),
            status: TeamStatus::Running,
            members: vec![Member { id: "lead".into(), role: "Lead".into(), agent: "claude".into(), state: MemberState::Active, ..Default::default() }],
            tasks: vec![Task {
                id: 1,
                title: "Work".into(),
                assignee: "dev".into(),
                status: TaskStatus::Submitted,
                attempts: vec![Attempt { n: 1, id: "A1".into(), member: "dev".into(), state: AttemptState::Reported, ..Default::default() }],
                merge: Some(MergeEntry { state: "testing".into(), author: "dev".into(), by: "Lead".into(), ..Default::default() }),
                ..Default::default()
            }],
            ..Default::default()
        };
        store.create(&uuid::Uuid::new_v4().to_string(), &json!({}), team, vec![]).unwrap();
        let broken = dir.path().join("broken-config");
        std::fs::create_dir_all(broken.join("config")).unwrap();
        let old = std::env::var("HGS_CONFIG_DIR").ok();
        let _env = crate::state::ENV_LOCK.lock().unwrap();
        std::env::set_var("HGS_CONFIG_DIR", &broken);
        let result = super::merge_queue_tick(&mut store, "t", 1.0);
        match &old {
            Some(value) => std::env::set_var("HGS_CONFIG_DIR", value),
            None => std::env::remove_var("HGS_CONFIG_DIR"),
        }
        result.unwrap();
        let saved = store.load("t").unwrap().unwrap();
        assert_eq!(saved.task(1).unwrap().status, TaskStatus::Accepted, "the completed test decides without the machine config");
        assert!(saved.task(1).unwrap().merge.is_none());
    });
}

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

/// Task #1 (team ede724c4): a scripted repository for the merge queue: the
/// lead's checkout with the queue on and one suite, and a member worktree
/// whose reported commit waits in the queue. None when the sandbox blocks git.
struct QueueRepo {
    lead: std::path::PathBuf,
    member: std::path::PathBuf,
    line: String,
}

fn queue_git(dir: &Path, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn queue_repo(dir: &Path) -> Option<QueueRepo> {
    let lead = dir.join("lead");
    let member = dir.join("member");
    std::fs::create_dir_all(lead.join(".zerus")).ok()?;
    let check = if cfg!(windows) { "if exist fail.txt (exit /b 1) else (exit /b 0)" } else { "test ! -f fail.txt" };
    std::fs::write(
        lead.join(".zerus/tests.toml"),
        format!("version = 1\n[merge_queue]\nenabled = true\n[[provider]]\nname = \"unit\"\nkind = \"command\"\ncommand = '{check}'\nsources = [\"**\"]\naffected = \"all\"\n"),
    )
    .ok()?;
    queue_git(&lead, &["init", "-q"])?;
    for (key, value) in [("user.name", "t"), ("user.email", "t@example.com"), ("commit.gpgsign", "false")] {
        queue_git(&lead, &["config", key, value])?;
    }
    std::fs::write(lead.join("a.txt"), "one\n").ok()?;
    queue_git(&lead, &["add", "."])?;
    queue_git(&lead, &["commit", "-q", "-m", "base"])?;
    let line = queue_git(&lead, &["symbolic-ref", "--short", "HEAD"])?;
    queue_git(&lead, &["worktree", "add", "-q", "-b", "task/1", &member.to_string_lossy()])?;
    std::fs::write(member.join("b.txt"), "work\n").ok()?;
    queue_git(&member, &["add", "."])?;
    queue_git(&member, &["commit", "-q", "-m", "work"])?;
    // The line moved meanwhile: the queue makes a merge commit.
    std::fs::write(lead.join("c.txt"), "lead\n").ok()?;
    queue_git(&lead, &["add", "."])?;
    queue_git(&lead, &["commit", "-q", "-m", "lead work"])?;
    Some(QueueRepo { lead, member, line })
}

/// The team whose task 1 the member reported and the lead accepted: its
/// landing is queued.
fn queued_team(repo: &QueueRepo) -> Team {
    let head = queue_git(&repo.member, &["rev-parse", "HEAD"]).unwrap();
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
    let mut team = Team {
        id: "t".into(),
        name: "T".into(),
        lead: "lead".into(),
        folder: repo.lead.to_string_lossy().into_owned(),
        line_branch: repo.line.clone(),
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
    };
    let mut events = Vec::new();
    ops::apply(&mut team, "accept", &json!({"task":1}), &ops::Actor::Member("lead".into()), &BTreeSet::new(), &mut events).unwrap();
    assert_eq!(team.task(1).unwrap().merge.as_ref().map(|entry| entry.state.as_str()), Some("queued"));
    team
}

/// Runs `test` with a state folder, a scripted queue repository and its team
/// in a store, and an empty machine config.
fn with_queue(test: impl FnOnce(&mut Store, &QueueRepo)) {
    let dir = tempfile::tempdir().unwrap();
    let Some(repo) = queue_repo(dir.path()) else {
        eprintln!("git could not build the test repository; the sandbox blocks it");
        return;
    };
    let config = dir.path().join("config");
    std::fs::create_dir_all(&config).unwrap();
    let _env = crate::state::ENV_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let old = std::env::var("HGS_CONFIG_DIR").ok();
    std::env::set_var("HGS_CONFIG_DIR", &config);
    let state = dir.path().join("state");
    std::fs::create_dir_all(&state).unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::state::with_test_state_root(&state, || {
            let mut store = Store::open_at(&state.join("teams/teams.sqlite3"), true).unwrap();
            store.create(&uuid::Uuid::new_v4().to_string(), &json!({}), queued_team(&repo), vec![]).unwrap();
            test(&mut store, &repo);
        })
    }));
    super::set_merge_between(None);
    match &old {
        Some(value) => std::env::set_var("HGS_CONFIG_DIR", value),
        None => std::env::remove_var("HGS_CONFIG_DIR"),
    }
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

fn kinds(store: &Store, kind: &str) -> usize {
    let page = store.events("t", None, 200).unwrap();
    page["items"].as_array().unwrap().iter().filter(|item| item["kind"] == kind).count()
}

fn lead_head(repo: &QueueRepo) -> String {
    queue_git(&repo.lead, &["rev-parse", "HEAD"]).unwrap()
}

fn queued_tests() -> usize {
    super::super::test_queue::TestStore::open(false).unwrap().queued("t").unwrap().len()
}

/// Task #1 (team ede724c4): the merge queue's Git (fetch, merge, merge-base,
/// the test selection) runs before the store's write lock is taken; the lock
/// is held only for the short write of the outcome. The longest hold is
/// printed for the report.
#[test]
fn a_merge_runs_no_git_under_the_store_write_lock() {
    with_queue(|store, repo| {
        let before = lead_head(repo);
        super::super::store::take_lock_lines();
        let started = Instant::now();
        super::merge_queue_tick(store, "t", 100.0).unwrap();
        let tick_ms = started.elapsed().as_millis();
        let lines = super::super::store::take_lock_lines();
        let processes: Vec<&Value> = lines.iter().filter(|line| line.get("process").is_some()).collect();
        assert!(processes.is_empty(), "a process started under the write lock: {processes:?}");
        let holds: Vec<u64> = lines.iter().filter_map(|line| line["held_ms"].as_u64()).collect();
        assert_eq!(holds.len(), 1, "one short write: {lines:?}");
        eprintln!("merge queue: tick {tick_ms} ms, longest write-lock hold {} ms", holds.iter().max().unwrap());
        let saved = store.load("t").unwrap().unwrap();
        let entry = saved.task(1).unwrap().merge.clone().unwrap();
        assert_eq!((entry.state.as_str(), entry.prev.as_str()), ("testing", before.as_str()));
        assert_eq!(entry.merged, lead_head(repo), "the merge is recorded");
        assert_eq!((kinds(store, "merged"), queued_tests()), (1, 1), "one landing, its tests queued once");
        // The next tick only waits for the tests: no write, no second landing.
        let revision = saved.revision;
        super::merge_queue_tick(store, "t", 101.0).unwrap();
        assert_eq!(store.load("t").unwrap().unwrap().revision, revision);
        assert_eq!((kinds(store, "merged"), queued_tests()), (1, 1));
    });
}

/// Task #1 (team ede724c4): the lead marked the landing landed by hand while
/// the queue merged it: the lead's record stands, the queue records nothing,
/// keeps the merge on the line and queues no tests; a later tick lands nothing.
#[test]
fn a_landing_marked_landed_meanwhile_is_recorded_once() {
    with_queue(|store, repo| {
        let commit = store.load("t").unwrap().unwrap().task(1).unwrap().merge.clone().unwrap().commit;
        super::set_merge_between(Some(Box::new(move |store: &mut Store, team: &str| {
            let commit = commit.clone();
            store
                .mutate(None, team, None, move |team, _| {
                    let task = team.task_mut(1).unwrap();
                    task.merge = None;
                    task.landed = Some(Landed { commit, by: "lead".into(), at: 100.5 });
                    Ok(Value::Null)
                })
                .unwrap();
        })));
        super::merge_queue_tick(store, "t", 100.0).unwrap();
        super::set_merge_between(None);
        let merged = lead_head(repo);
        let saved = store.load("t").unwrap().unwrap();
        let task = saved.task(1).unwrap();
        assert!(task.merge.is_none(), "{:?}", task.merge);
        assert_eq!(task.landed.as_ref().map(|landed| landed.by.as_str()), Some("lead"));
        assert_eq!((kinds(store, "merged"), queued_tests()), (0, 0), "the queue recorded no second landing");
        super::merge_queue_tick(store, "t", 102.0).unwrap();
        assert_eq!(lead_head(repo), merged, "the line keeps the landed merge");
        assert_eq!((kinds(store, "merged"), queued_tests()), (0, 0));
    });
}

/// Task #1 (team ede724c4): a landing cancelled while the queue merged it
/// leaves no merge on the line and no record.
#[test]
fn a_landing_cancelled_meanwhile_is_rolled_back() {
    with_queue(|store, repo| {
        let before = lead_head(repo);
        super::set_merge_between(Some(Box::new(|store: &mut Store, team: &str| {
            store
                .mutate(None, team, None, |team, _| {
                    team.task_mut(1).unwrap().merge = None;
                    Ok(Value::Null)
                })
                .unwrap();
        })));
        super::merge_queue_tick(store, "t", 100.0).unwrap();
        super::set_merge_between(None);
        assert_eq!(lead_head(repo), before, "the queue's merge was rolled back");
        let saved = store.load("t").unwrap().unwrap();
        assert!(saved.task(1).unwrap().merge.is_none());
        assert_eq!((kinds(store, "merged"), queued_tests()), (0, 0));
    });
}

/// Task #1 (team ede724c4): another writer's unrelated change meanwhile (a new
/// task, a renamed team) is kept beside the landing, which is recorded once.
#[test]
fn an_unrelated_change_meanwhile_keeps_both_and_lands_once() {
    with_queue(|store, repo| {
        super::set_merge_between(Some(Box::new(|store: &mut Store, team: &str| {
            store
                .mutate(None, team, None, |team, _| {
                    team.name = "Renamed".into();
                    team.tasks.push(Task { id: 2, title: "Other".into(), assignee: "dev".into(), status: TaskStatus::Pending, ..Default::default() });
                    Ok(Value::Null)
                })
                .unwrap();
        })));
        super::merge_queue_tick(store, "t", 100.0).unwrap();
        super::set_merge_between(None);
        let saved = store.load("t").unwrap().unwrap();
        assert_eq!(saved.name, "Renamed");
        assert_eq!(saved.task(2).map(|task| task.title.as_str()), Some("Other"));
        let entry = saved.task(1).unwrap().merge.clone().unwrap();
        assert_eq!((entry.state.as_str(), entry.merged.as_str()), ("testing", lead_head(repo).as_str()));
        assert_eq!((kinds(store, "merged"), queued_tests()), (1, 1));
        super::merge_queue_tick(store, "t", 101.0).unwrap();
        assert_eq!((kinds(store, "merged"), queued_tests()), (1, 1), "never landed twice");
    });
}
