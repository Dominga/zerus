use super::*;
use super::support::*;

#[test]
fn a_dispatcher_config_error_names_the_waiting_merge() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let team = Team {
            id: "t".into(),
            name: "T".into(),
            lead: "lead".into(),
            status: TeamStatus::Running,
            tasks: vec![Task {
                id: 1,
                title: "Work".into(),
                status: TaskStatus::Submitted,
                merge: Some(MergeEntry { state: "waiting".into(), author: "dev".into(), prev: "p".into(), merged: "m".into(), note: "old".into(), ..Default::default() }),
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
        let entry = saved.task(1).unwrap().merge.as_ref().unwrap();
        assert_eq!(entry.state, "waiting");
        assert!(entry.note.contains("test runners could not be read"), "{}", entry.note);
        let page = store.events("t", None, 20).unwrap();
        assert!(page["items"].as_array().unwrap().iter().any(|event| event["kind"] == "merge_config_failed"), "{page}");
    });
}

#[test]
fn a_waiting_merge_is_resolved_before_a_later_queued_merge() {
    let dir = tempfile::tempdir().unwrap();
    let lead = dir.path().join("lead");
    let marker = dir.path().join("marker.txt");
    assert!(diverged_repo(&lead, &marker), "git could not build the test repository");
    std::fs::create_dir_all(lead.join(".zerus")).unwrap();
    std::fs::write(
        lead.join(".zerus/tests.toml"),
        "version = 1\n[merge_queue]\nenabled = true\n[[provider]]\nname = \"unit\"\nkind = \"command\"\ncommand = \"exit 0\"\nsources = [\"**\"]\naffected = \"all\"\n",
    )
    .unwrap();
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(&lead)
            .args(["-c", "user.email=t@e", "-c", "user.name=T"])
            .args(args)
            .status()
            .is_ok_and(|status| status.success())
    };
    assert!(git(&["add", ".zerus/tests.toml"]) && git(&["commit", "-qm", "tests"]));
    let head = String::from_utf8(std::process::Command::new("git").arg("-C").arg(&lead).args(["rev-parse", "HEAD"]).output().unwrap().stdout).unwrap().trim().to_owned();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let mut team = folder_team(&lead, Member { id: "dev".into(), role: "Backend".into(), agent: "codex".into(), ..Default::default() });
        team.id = "t".into();
        team.status = TeamStatus::Running;
        team.tasks.push(Task {
            id: 1,
            title: "Waiting".into(),
            assignee: "dev".into(),
            status: TaskStatus::Submitted,
            attempts: vec![Attempt { n: 1, id: "A1".into(), member: "dev".into(), state: AttemptState::Reported, ..Default::default() }],
            merge: Some(MergeEntry { state: "waiting".into(), author: "dev".into(), by: "Lead".into(), prev: head.clone(), merged: head.clone(), note: "old".into(), ..Default::default() }),
            ..Default::default()
        });
        team.tasks.push(Task {
            id: 2,
            title: "Behind".into(),
            assignee: "dev".into(),
            status: TaskStatus::Submitted,
            merge: Some(MergeEntry { state: "queued".into(), author: "dev".into(), by: "Lead".into(), ..Default::default() }),
            ..Default::default()
        });
        store.create(&uuid::Uuid::new_v4().to_string(), &json!({}), team, vec![]).unwrap();
        let config = dir.path().join("config");
        std::fs::create_dir_all(&config).unwrap();
        let old = std::env::var("HGS_CONFIG_DIR").ok();
        let _env = crate::state::ENV_LOCK.lock().unwrap();
        std::env::set_var("HGS_CONFIG_DIR", &config);
        let result = super::merge_queue_tick(&mut store, "t", 1.0);
        match &old {
            Some(value) => std::env::set_var("HGS_CONFIG_DIR", value),
            None => std::env::remove_var("HGS_CONFIG_DIR"),
        }
        result.unwrap();
        let saved = store.load("t").unwrap().unwrap();
        assert_eq!(saved.task(1).unwrap().status, TaskStatus::Accepted, "the waiting merge retried and resolved");
        assert!(saved.task(1).unwrap().merge.is_none());
        assert_eq!(saved.merge_tested, head, "nothing selected still tests the current head");
        assert_eq!(saved.task(2).unwrap().merge.as_ref().unwrap().state, "queued", "the later merge did not go ahead");
    });
}

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
