use super::*;
use super::support::*;

#[test]
fn a_sandboxed_agent_gets_a_clone_folder() {
    assert_eq!(default_folder("dsh"), MemberFolder::Clone);
    assert_eq!(default_folder("claude"), MemberFolder::Worktree);
    let mut team = team();
    {
        let member = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
        member.folder = MemberFolder::Clone;
        member.directory = "C:/work/project".into();
        // A clone is cut for a member that has not started yet; an adopted
        // session works in its own folder instead (N13).
        member.session = None;
    }
    let member = team.member("dev").unwrap();
    let path = member_folder_path(&team, member).unwrap();
    assert!(path.ends_with("project-team-t-dev"), "{path}");
}

#[test]
fn a_commit_records_its_sha_and_refuses_another_branch() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    fs::create_dir(&repo).unwrap();
    let run = |args: &[&str]| {
        let output = Command::new("git").arg("-C").arg(&repo).args(args).output().unwrap();
        assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
    };
    let init = Command::new("git").args(["init", "-q"]).arg(&repo).output().unwrap();
    assert!(init.status.success());
    run(&["config", "user.email", "team@example.test"]);
    run(&["config", "user.name", "Team"]);
    fs::write(repo.join("a.txt"), "one").unwrap();
    run(&["add", "-A"]);
    run(&["commit", "-qm", "init"]);
    run(&["switch", "-qc", "team/t-dev"]);
    fs::write(repo.join("a.txt"), "two").unwrap();
    let folder = repo.to_str().unwrap();
    let committed = commit_in(folder, "team/t-dev", "Fork: change", &[]).unwrap();
    assert_eq!(committed["status"], "committed");
    assert!(committed["sha"].as_str().is_some_and(|sha| sha.len() >= 7));
    assert_eq!(committed["files"], json!(1));
    assert_eq!(commit_in(folder, "team/t-dev", "again", &[]).unwrap()["status"], "clean");
    run(&["switch", "-qc", "other"]);
    let error = commit_in(folder, "team/t-dev", "x", &[]).unwrap_err().to_string();
    assert!(error.contains("not team/t-dev"), "{error}");
    // C2: a repository-local hooks path or filter is refused.
    run(&["switch", "-q", "team/t-dev"]);
    run(&["config", "--local", "core.hooksPath", ".githooks"]);
    let error = commit_in(folder, "team/t-dev", "x", &[]).unwrap_err().to_string();
    assert!(error.contains("core.hookspath"), "{error}");
    run(&["config", "--local", "--unset", "core.hooksPath"]);
    run(&["config", "--local", "filter.slow.clean", "cat"]);
    let error = commit_in(folder, "team/t-dev", "x", &[]).unwrap_err().to_string();
    assert!(error.contains("Git filter"), "{error}");
    run(&["config", "--local", "--remove-section", "filter.slow"]);
    // C2: a repository gpg program or an included config is refused.
    run(&["config", "--local", "gpg.program", "false"]);
    let error = commit_in(folder, "team/t-dev", "x", &[]).unwrap_err().to_string();
    assert!(error.contains("gpg.program"), "{error}");
    run(&["config", "--local", "--unset", "gpg.program"]);
    let included = repo.join(".git").join("included.config");
    fs::write(&included, "[filter \"slow\"]\n\tclean = false\n").unwrap();
    run(&["config", "--local", "include.path", "included.config"]);
    let error = commit_in(folder, "team/t-dev", "x", &[]).unwrap_err().to_string();
    assert!(error.contains("includes another Git config"), "{error}");
    run(&["config", "--local", "--unset", "include.path"]);
    // Git stores a conditional include in lowercase; it is refused too.
    run(&["config", "--local", "includeIf.onbranch:team/**.path", "included.config"]);
    let error = commit_in(folder, "team/t-dev", "x", &[]).unwrap_err().to_string();
    assert!(error.contains("includes another Git config"), "{error}");
    run(&["config", "--local", "--remove-section", "includeIf.onbranch:team/**"]);
    // The worktree's own config (extensions.worktreeConfig) is the
    // repository's too: a filter or include there is refused.
    run(&["config", "--local", "extensions.worktreeConfig", "true"]);
    run(&["config", "--worktree", "filter.ev2.clean", "false"]);
    let error = commit_in(folder, "team/t-dev", "x", &[]).unwrap_err().to_string();
    assert!(error.contains("Git filter"), "{error}");
    run(&["config", "--worktree", "--remove-section", "filter.ev2"]);
    run(&["config", "--worktree", "includeIf.onbranch:team/**.path", "included.config"]);
    let error = commit_in(folder, "team/t-dev", "x", &[]).unwrap_err().to_string();
    assert!(error.contains("includes another Git config"), "{error}");
    run(&["config", "--worktree", "--remove-section", "includeIf.onbranch:team/**"]);
    run(&["config", "--local", "--unset", "extensions.worktreeConfig"]);
    // Scopes other than the repository's own are not its to refuse.
    assert_eq!(refused_repository_config("global\tfilter.lfs.clean\nlocal\tcore.bare\n"), None);
    assert!(refused_repository_config("worktree\tINCLUDEIF.onbranch:x.path\n").is_some());
    // U21: a merge driver is a command git runs during the merge.
    assert!(refused_repository_config("local\tmerge.ours.driver\n").is_some());
    assert_eq!(refused_repository_config("local\tmerge.ff\n"), None);
    // C2: a call with more than 1 MB of output finishes without blocking:
    // git show --name-only prints every file of this commit.
    for index in 0..6000 {
        fs::write(repo.join(format!("{index:04}-{}", "x".repeat(150))), "x").unwrap();
    }
    let many = commit_in(folder, "team/t-dev", "many files", &[]).unwrap();
    assert_eq!(many["status"], "committed");
    assert_eq!(many["files"], json!(6000));
    // C3: a profile maps to the agent's mode, or is advisory.
    assert_eq!(profile_mode("claude", "build"), Some("acceptEdits"));
    assert_eq!(profile_mode("claude", "read"), Some("plan"));
    assert_eq!(profile_mode("codex", "read"), None);
    assert_eq!(profile_mode("claude", "network"), None);
}

#[test]
fn a_reopened_task_hands_out_its_finding_first() {
    use crate::state::teams::model::{Attempt, Member, Task, Team};
    let mut team = Team { name: "T".into(), ..Default::default() };
    team.members.push(Member { id: "dev".into(), role: "Backend".into(), ..Default::default() });
    let mut task = Task { id: 1, title: "A".into(), assignee: "dev".into(), ..Default::default() };
    task.reopen_note = "Finding f1 (critical): Lost write\nsrc/save.rs:42\nScenario: two saves at once\nLead's note: fix today".into();
    let attempt = Attempt { n: 1, member: "dev".into(), ..Default::default() };
    let text = hand_off_text(&team, &task, &attempt, "", "");
    assert!(text.starts_with("[Zerus team \u{b7} finding]"), "{text}");
    assert!(text.contains("Lost write") && text.contains("src/save.rs:42") && text.contains("Lead's note: fix today"), "{text}");
    assert!(text.contains("Task #1"), "{text}");
}

#[test]
fn an_adopted_session_is_filed_into_its_folders_project() {
    // F1.1/N36: the filing project is the team folder's catalog entry. A
    // team's `project` is the GUI's group name ("Checkout", "billing"), not
    // a project id, and is never sent as one.
    let mut team = Team {
        name: "T".into(),
        project: "Checkout".into(),
        folder: "D:/projects/checkout".into(),
        ..Default::default()
    };
    let known = json!({"schema":1,"swarm_id":"swarm-1","project_id":"checkout"});
    assert_eq!(filed_project(&known).as_deref(), Some("checkout"));
    assert_eq!(filed_project(&json!({"schema":1,"swarm_id":"swarm-1","project_id":""})), None);
    let session = SessionRef {
        host: "msi".into(),
        name: "claude/x/dev".into(),
        run_id: "run-1".into(),
        conversation_id: "conv-1".into(),
    };
    let payload = filing_payload(&team, &known, &session).unwrap();
    assert_eq!(payload["name"], "claude/x/dev");
    assert_eq!(payload["expected_run_id"], "run-1");
    assert_eq!(payload["expected_conversation_id"], "conv-1");
    assert_eq!((payload["swarm_id"].as_str(), payload["project_id"].as_str()), (Some("swarm-1"), Some("checkout")));
    assert_eq!(payload["directory"], "D:/projects/checkout");
    assert!(payload["request_id"].as_str().is_some_and(|id| id.len() == 36));
    // The label never becomes the project, whatever it says.
    team.project = "billing".into();
    assert_eq!(filing_payload(&team, &known, &session).unwrap()["project_id"], "checkout");
    // A team without a folder, or a catalog without a swarm or a project,
    // files nothing.
    assert!(filing_payload(&Team { ..Default::default() }, &known, &session).is_none());
    assert!(filing_payload(&team, &json!({"swarm_id":"","project_id":"checkout"}), &session).is_none());
    assert!(filing_payload(&team, &json!({"swarm_id":"swarm-1","project_id":""}), &session).is_none());
}

/// N9: the generic `worktrees create` grants nothing; only a DeepSeek
/// member's folder asks for the sandbox grant, so a share without ACL
/// support cannot turn another agent's created worktree into a failure.
#[test]
fn only_a_dsh_members_worktree_asks_for_the_sandbox_grant() {
    let dir = tempfile::tempdir().unwrap();
    set_git_ceiling(&dir.path().display().to_string());
    let repo = folder_repo(dir.path(), "project");
    for (agent, sandbox) in [("codex", false), ("dsh", true)] {
        let member = Member { id: format!("dev-{agent}"), role: "Backend".into(), agent: agent.into(), folder: MemberFolder::Worktree, ..Default::default() };
        let team = folder_team(&repo, member.clone());
        let calls = std::cell::RefCell::new(Vec::<Vec<String>>::new());
        let run = |_: &str, args: &[&str]| {
            calls.borrow_mut().push(args.iter().map(|arg| arg.to_string()).collect());
            local_create(args)
        };
        member_folder_with(&team, &member, &run).unwrap();
        let args = calls.borrow()[0].clone();
        assert_eq!(args.iter().any(|arg| arg == "--sandbox"), sandbox, "{agent}: {args:?}");
    }
}

/// F8/N13: an adopted worktree member works in its session's own folder; a
/// report queued in the folder computed beside it would stay queued forever.
#[test]
fn an_adopted_sessions_outbox_is_the_sessions_own_folder() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut t = team();
    t.status = TeamStatus::Running;
    let folder = dir.path().join("work");
    std::fs::create_dir_all(&folder).unwrap();
    t.folder = dir.path().join("repo").display().to_string();
    let task = t.tasks.iter_mut().find(|task| task.assignee == "dev").unwrap();
    task.attempts.push(Attempt { n: 1, id: "A1".into(), member: "dev".into(), state: AttemptState::Delivered, ..Default::default() });
    task.status = TaskStatus::InProgress;
    let task_id = task.id;
    {
        let member = t.members.iter_mut().find(|member| member.id == "dev").unwrap();
        member.folder = MemberFolder::Worktree;
        member.directory = folder.display().to_string();
        member.session = Some(SessionRef { name: "codex/x/dev".into(), run_id: "r1".into(), ..Default::default() });
    }
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
    let current = store.load(&t.id).unwrap().unwrap();
    assert_eq!(
        member_folder_path(&current, current.member("dev").unwrap()).unwrap(),
        folder.display().to_string(),
        "the adopted session's own folder is the member's"
    );
    let request_id = "44444444-4444-4444-8444-444444444444";
    // #522: a sandboxed member's ask queues in its folder like a report and
    // reaches the lead as the member's own question.
    let request = json!({"op":"ask","request_id":request_id,"team":t.id,"member":"dev","attempt":"A1",
        "input":{"request_id":request_id,"team":t.id,"attempt":"A1","text":"Must I keep the cache?"}});
    super::super::outbox::push(&folder, &request).unwrap();
    let mut dev = session("codex/x/dev", "r1", true);
    dev.cwd = folder.display().to_string();
    take_outbox(&mut store, &current, &[dev]).unwrap();
    let saved = store.load(&t.id).unwrap().unwrap();
    let questions = &saved.task(task_id).unwrap().attempts.last().unwrap().questions;
    assert_eq!(questions.first().map(|question| question.text.as_str()), Some("Must I keep the cache?"), "the queued ask reached the lead");
}

/// N11: two runs queued together in one folder (a build and a test) are
/// serialized; the second is claimed only after the first settles.
#[test]
fn a_second_run_in_one_folder_waits_for_the_first() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut t = team();
    t.status = TeamStatus::Running;
    let folder = dir.path().join("work");
    std::fs::create_dir_all(&folder).unwrap();
    t.folder = folder.display().to_string();
    let task = t.tasks.iter_mut().find(|task| task.assignee == "dev").unwrap();
    task.attempts.push(Attempt { n: 1, id: "A1".into(), member: "dev".into(), state: AttemptState::Delivered, ..Default::default() });
    task.status = TaskStatus::InProgress;
    let task_id = task.id;
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
    let settings = dir.path().join("templates.json");
    crate::state::teams::templates::settings_override(Some(settings));
    crate::state::teams::templates::set_additions(&[json!({"id":"say","program":"cmd","args":["/c","echo run-ok"]})]).unwrap();
    let run = |id: &str| {
        json!({"op":"run","request_id":id,"team":t.id,"member":"dev","attempt":"A1","template":"say","args":{},
            "input":{"request_id":id,"team":t.id,"attempt":"A1","template":"say","args":{}}})
    };
    let first = "77777777-7777-4777-8777-777777777771";
    let second = "77777777-7777-4777-8777-777777777772";
    super::super::outbox::push(&folder, &run(first)).unwrap();
    super::super::outbox::push(&folder, &run(second)).unwrap();
    let mut dev = session("codex/x/dev", "r1", true);
    dev.cwd = folder.display().to_string();
    let started = std::cell::RefCell::new(Vec::<String>::new());
    let spawn = |_: &Path, request: &Value| {
        started.borrow_mut().push(request["request_id"].as_str().unwrap_or_default().to_owned());
        Ok(())
    };
    let current = store.load(&t.id).unwrap().unwrap();
    take_outbox_with(&mut store, &current, &[dev.clone()], &spawn).unwrap();
    assert_eq!(started.borrow().as_slice(), [first.to_owned()], "a second run waits");
    assert!(super::super::outbox::request_path(&folder, second).exists(), "the second run is still queued");
    // The first run ends; the next tick claims the second.
    run_request(&mut store, &folder, &run(first));
    let current = store.load(&t.id).unwrap().unwrap();
    take_outbox_with(&mut store, &current, &[dev], &spawn).unwrap();
    assert_eq!(started.borrow().as_slice(), [first.to_owned(), second.to_owned()]);
    let saved = store.load(&t.id).unwrap().unwrap();
    assert_eq!(saved.task(task_id).unwrap().attempts.last().unwrap().runs.len(), 1);
    crate::state::teams::templates::settings_override(None);
}

/// N16: members that may not have worktrees share one folder and one outbox;
/// a request there belongs to the member that queued it and is applied on
/// their own pass, never refused as the folder's first member's.
#[test]
fn a_shared_folders_request_belongs_to_the_member_that_queued_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut t = team();
    t.status = TeamStatus::Running;
    t.settings.worktrees_forbidden = true;
    let folder = dir.path().join("work");
    std::fs::create_dir_all(&folder).unwrap();
    t.folder = folder.display().to_string();
    for member in t.members.iter_mut().filter(|member| member.id != "lead") {
        member.folder = MemberFolder::Shared;
        member.directory.clear();
    }
    let qa_task = t.tasks.iter_mut().find(|task| task.assignee == "qa").unwrap();
    qa_task.attempts.push(Attempt { n: 1, id: "A3".into(), member: "qa".into(), state: AttemptState::Delivered, ..Default::default() });
    qa_task.status = TaskStatus::InProgress;
    let qa_id = qa_task.id;
    {
        let qa = t.members.iter_mut().find(|member| member.id == "qa").unwrap();
        qa.session = Some(SessionRef { name: "kimi/x/qa".into(), run_id: "r2".into(), ..Default::default() });
    }
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
    let request_id = "44444444-4444-4444-8444-444444444445";
    let request = json!({"op":"report","request_id":request_id,"team":t.id,"member":"qa","attempt":"A3",
        "input":{"request_id":request_id,"team":t.id,"task":qa_id,"attempt":"A3","text":"qa done"}});
    super::super::outbox::push(&folder, &request).unwrap();
    let mut dev = session("codex/x/dev", "r1", true);
    dev.cwd = folder.display().to_string();
    let mut qa = session("kimi/x/qa", "r2", true);
    qa.cwd = folder.display().to_string();
    let current = store.load(&t.id).unwrap().unwrap();
    take_outbox(&mut store, &current, &[dev, qa]).unwrap();
    let saved = store.load(&t.id).unwrap().unwrap();
    assert_eq!(
        saved.task(qa_id).unwrap().attempts.last().unwrap().report.as_ref().map(|report| report.text.as_str()),
        Some("qa done"),
        "qa's report was applied for qa"
    );
}

/// N12: a clone carries its own copy of the lead's branch from the day it
/// was made. The merge fetches the branch from the team's folder first, so
/// it brings the lead's current work instead of saying "Already up to date".
#[test]
fn a_clone_merges_the_leads_current_branch_not_its_old_copy() {
    let dir = tempfile::tempdir().unwrap();
    set_git_ceiling(&dir.path().display().to_string());
    let repo = folder_repo(dir.path(), "project");
    let clone = dir.path().join("clone");
    let run_git = |folder: &Path, args: &[&str]| {
        std::process::Command::new("git").arg("-C").arg(folder).args(["-c", "user.email=t@e", "-c", "user.name=T"]).args(args).status().is_ok_and(|status| status.success())
    };
    let cloned = std::process::Command::new("git")
        .args(["clone", "--local", "--no-hardlinks", "--quiet", &repo.display().to_string(), &clone.display().to_string()])
        .status()
        .is_ok_and(|status| status.success());
    if !cloned {
        eprintln!("git could not build the test clone; the sandbox blocks it");
        return;
    }
    // The lead moves on after the clone was made.
    std::fs::write(repo.join("lead.txt"), "l").unwrap();
    assert!(run_git(&repo, &["add", "."]) && run_git(&repo, &["commit", "-qm", "lead"]));
    let member = Member {
        id: "dev".into(),
        role: "Backend".into(),
        agent: "dsh".into(),
        folder: MemberFolder::Clone,
        directory: clone.display().to_string(),
        session: Some(SessionRef { name: "dsh/x/dev".into(), run_id: "r1".into(), ..Default::default() }),
        ..Default::default()
    };
    let team = folder_team(&repo, member.clone());
    let request = json!({"template":"git-merge-lead","args":{}});
    let outcome = run_outcome(&team, &member, &clone, &request).unwrap();
    assert_eq!(outcome.exit, Some(0), "{outcome:?}");
    assert!(clone.join("lead.txt").exists(), "the lead's current branch was not merged: {}", outcome.tail);
}

/// N29: the new session of a swap is limited too; the old request must not
/// stay on the session that is no longer the member's.
#[test]
fn a_limit_request_follows_a_swap_to_another_limited_session() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let mut t = team();
        t.status = TeamStatus::Running;
        t.settings.resume_after_limits = true;
        {
            let dev = t.members.iter_mut().find(|member| member.id == "dev").unwrap();
            dev.limited = Some(MemberLimit { at: 100.0, resets_at: Some(5_000.0), detail: "session limit".into() });
            dev.limit_resume = true;
            dev.limit_session = dev.session.clone();
        }
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
        // A swap binds another session; the old limit episode stays recorded.
        store
            .mutate(Some(("22222222-2222-4222-8222-222222222222", &json!({}))), &t.id, None, |team, _| {
                let dev = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
                dev.session = Some(SessionRef { name: "codex/x/dev2".into(), run_id: "r2".into(), ..Default::default() });
                Ok(json!({}))
            })
            .unwrap();
        let mut second = session("codex/x/dev2", "r2", false);
        second.phase = "error".into();
        second.limited = Some(SeenLimit { at: 100.0, resets_at: Some(5_000.0), detail: "session limit".into() });
        let found = pass(&mut store, &t.id, &[session("claude/x/lead", "r0", true), second]);
        assert!(found.contains(&Step::StartLimitResume { member: "dev".into() }), "{found:?}");
        assert_eq!(local_overrides(), vec!["codex/x/dev2".to_owned()], "the request follows the new session");
        let saved = store.load(&t.id).unwrap().unwrap();
        assert_eq!(saved.member("dev").unwrap().limit_session.as_ref().map(|session| session.name.as_str()), Some("codex/x/dev2"));
    });
}

/// N30: a refresh on another computer costs an `hgs` call with a 60 s bound;
/// it is asked once a minute, never on every 3 s tick.
#[test]
fn a_remote_limit_refresh_is_asked_once_a_minute() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let mut t = team();
        t.settings.resume_after_limits = true;
        {
            let dev = t.members.iter_mut().find(|member| member.id == "dev").unwrap();
            dev.computer = "msi".into();
            dev.session.as_mut().unwrap().host = "msi".into();
        }
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
        let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::<Vec<String>>::new()));
        let counted = calls.clone();
        set_fake_hgs(Some(Box::new(move |_, args: &[&str], _| {
            counted.borrow_mut().push(args.iter().map(|arg| arg.to_string()).collect());
            Ok(fake_output(0, r#"{"status":"ok"}"#, ""))
        })));
        let recovery = || calls.borrow().iter().filter(|args| args.first().map(String::as_str) == Some("recovery")).count();
        let mut remote = limited_dev("codex/x/dev", "r1");
        remote.host = "msi".into();
        let lead = session("claude/x/lead", "r0", true);
        pass(&mut store, &t.id, &[lead.clone(), remote.clone()]);
        assert_eq!(recovery(), 1, "the first pass asks for the wait");
        pass(&mut store, &t.id, &[lead.clone(), remote.clone()]);
        assert_eq!(recovery(), 2, "a later pass refreshes it");
        pass(&mut store, &t.id, &[lead, remote]);
        assert_eq!(recovery(), 2, "a second tick inside the minute asks nothing");
        set_fake_hgs(None);
    });
}

#[test]
fn a_member_folder_left_on_a_merged_task_branch_returns_to_the_member_branch() {
    use crate::state::teams::model::{Attempt, BranchPerTask, Member, MemberFolder, Task};
    let dir = tempfile::tempdir().unwrap();
    set_git_ceiling(&dir.path().display().to_string());
    let repo = folder_repo(dir.path(), "project");
    let worktree = dir.path().join("dev");
    let git = |at: &Path, args: &[&str]| {
        let status = std::process::Command::new("git").arg("-C").arg(at).args(args).status().unwrap();
        assert!(status.success(), "{args:?}");
    };
    let branch = |at: &Path| {
        String::from_utf8(std::process::Command::new("git").arg("-C").arg(at).args(["rev-parse", "--abbrev-ref", "HEAD"]).output().unwrap().stdout)
            .unwrap()
            .trim()
            .to_owned()
    };
    git(&repo, &["worktree", "add", "-q", "-b", "team/abcdef12/dev", &worktree.display().to_string(), "HEAD"]);
    // The task branch is merged into the member branch, then left checked out.
    git(&worktree, &["switch", "-q", "-c", "team/abcdef12/dev-1"]);
    std::fs::write(worktree.join("task.txt"), "task").unwrap();
    git(&worktree, &["add", "."]);
    git(&worktree, &["commit", "-qm", "task"]);
    git(&worktree, &["switch", "-q", "team/abcdef12/dev"]);
    git(&worktree, &["merge", "-q", "--no-ff", "-m", "merge", "team/abcdef12/dev-1"]);
    git(&worktree, &["switch", "-q", "team/abcdef12/dev-1"]);
    let mut team = folder_team(
        &repo,
        Member {
            id: "dev".into(),
            role: "Backend".into(),
            agent: "codex".into(),
            folder: MemberFolder::Worktree,
            directory: worktree.display().to_string(),
            ..Default::default()
        },
    );
    team.settings.branch_per_task = Some(BranchPerTask { template: "team/{team8}/{member}-{id}".into() });
    team.tasks.push(Task {
        id: 1,
        title: "A".into(),
        assignee: "dev".into(),
        attempts: vec![Attempt { n: 1, id: "a1".into(), member: "dev".into(), ..Default::default() }],
        ..Default::default()
    });
    let note = check_folder_branch(&team, team.member("dev").unwrap(), &worktree.display().to_string()).unwrap();
    assert!(note.unwrap().contains("back on team/abcdef12/dev"), "the note names the checkout");
    assert_eq!(branch(&worktree), "team/abcdef12/dev");
    // The task branch is kept.
    assert!(std::process::Command::new("git")
        .arg("-C")
        .arg(&worktree)
        .args(["rev-parse", "--verify", "team/abcdef12/dev-1"])
        .output()
        .unwrap()
        .status
        .success());
    // A dirty folder is never switched.
    git(&worktree, &["switch", "-q", "team/abcdef12/dev-1"]);
    std::fs::write(worktree.join("task.txt"), "dirty").unwrap();
    let error = check_folder_branch(&team, team.member("dev").unwrap(), &worktree.display().to_string()).unwrap_err().to_string();
    assert!(error.contains("uncommitted changes"), "{error}");
    assert_eq!(branch(&worktree), "team/abcdef12/dev-1");
}

#[test]
fn a_swapped_leads_brief_names_the_previous_conversation_and_transcript() {
    let mut team = Team { id: "abcdef12-0000-4000-8000-000000000000".into(), name: "T".into(), lead: "lead".into(), status: TeamStatus::Running, ..Default::default() };
    team.members.push(Member { id: "lead".into(), role: "Lead".into(), agent: "claude".into(), ..Default::default() });
    team.swapped.push(json!({"member":"Lead","old":"claude/x/old","at":1.0,"by":"You",
        "conversation":"c-old","transcript":"C:/state/old.jsonl"}));
    let brief = brief_text(&team);
    assert!(brief.contains("claude/x/old") && brief.contains("c-old") && brief.contains("old.jsonl"), "{brief}");
    // Another member's swap is not the lead's history.
    team.swapped.clear();
    team.swapped.push(json!({"member":"Backend","old":"claude/x/dev","conversation":"c-dev"}));
    assert!(!brief_text(&team).contains("c-dev"));
}

#[test]
fn a_member_worktree_is_cut_only_from_the_teams_repository() {
    // N38/F1.4: the member's own `directory` names where its worktree or
    // clone is cut from; another repository there must not become the
    // member's folder, and nothing is created from it.
    let dir = tempfile::tempdir().unwrap();
    set_git_ceiling(&dir.path().display().to_string());
    let repo = folder_repo(dir.path(), "project");
    let other = folder_repo(dir.path(), "other");
    for kind in [MemberFolder::Worktree, MemberFolder::Clone] {
        let member = Member {
            id: "dev".into(),
            role: "Backend".into(),
            agent: "codex".into(),
            folder: kind,
            directory: other.display().to_string(),
            ..Default::default()
        };
        let team = folder_team(&repo, member.clone());
        let calls = std::cell::Cell::new(0);
        let run = |_: &str, args: &[&str]| {
            calls.set(calls.get() + 1);
            local_create(args)
        };
        let error = member_folder_with(&team, &member, &run).unwrap_err().to_string();
        assert!(error.contains("is not the team's repository"), "{kind:?}: {error}");
        assert!(error.contains("belongs to another repository"), "{kind:?}: {error}");
        assert_eq!(calls.get(), 0, "{kind:?}: nothing is created");
        assert!(!Path::new(&member_folder_path(&team, &member).unwrap()).exists(), "{kind:?}");
    }
    // A folder inside the team's repository is a fine source.
    let nested = repo.join("sub");
    std::fs::create_dir(&nested).unwrap();
    let member = Member {
        id: "qa".into(),
        role: "QA".into(),
        agent: "codex".into(),
        folder: MemberFolder::Worktree,
        directory: nested.display().to_string(),
        ..Default::default()
    };
    let team = folder_team(&repo, member.clone());
    let folder = member_folder_with(&team, &member, &|_, args| local_create(args)).unwrap();
    assert_eq!(folder_facts(&folder).1, "team/abcdef12/qa");
}

#[test]
fn a_worktree_outside_the_teams_repository_is_flagged() {
    // F1.6/N36: "outside" compares the folders, not the team's project
    // label: a worktree of the team's repository is inside, another
    // repository is not.
    let dir = tempfile::tempdir().unwrap();
    set_git_ceiling(&dir.path().display().to_string());
    let repo = folder_repo(dir.path(), "project");
    let other = folder_repo(dir.path(), "other");
    let source = repo.display().to_string();
    let team = source.clone();
    // Unknown folders are not flagged.
    assert!(!outside_project(&team, "", "", &source));
    // A shared folder, a nested folder and a member worktree are inside.
    let nested = repo.join("sub");
    std::fs::create_dir(&nested).unwrap();
    for folder in [repo.clone(), nested.clone()] {
        let folder = folder.display().to_string();
        let facts = folder_facts(&folder);
        assert!(!outside_project(&team, &folder, &facts.0, &source), "{folder}");
    }
    let member = Member { id: "dev".into(), role: "Backend".into(), agent: "codex".into(), folder: MemberFolder::Worktree, ..Default::default() };
    let team_record = folder_team(&repo, member.clone());
    let worktree = member_folder_with(&team_record, &member, &|_, args| local_create(args)).unwrap();
    let facts = folder_facts(&worktree);
    assert!(!outside_project(&team, &worktree, &facts.0, &source), "{worktree}");
    // Another repository is.
    let foreign = other.display().to_string();
    let facts = folder_facts(&foreign);
    assert!(outside_project(&team, &foreign, &facts.0, &source));
}

#[test]
fn a_clone_member_of_the_teams_repository_starts() {
    // C10: `worktrees create --clone` makes a clone with its own .git; it
    // is the team's repository, so the member starts in it.
    let dir = tempfile::tempdir().unwrap();
    set_git_ceiling(&dir.path().display().to_string());
    let repo = folder_repo(dir.path(), "project");
    let member = Member { id: "dev".into(), role: "Backend".into(), agent: "dsh".into(), folder: MemberFolder::Clone, ..Default::default() };
    let team = folder_team(&repo, member.clone());
    let calls = std::cell::Cell::new(0);
    let run = |_: &str, args: &[&str]| { calls.set(calls.get() + 1); local_create(args) };
    let folder = member_folder_with(&team, &member, &run).unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(folder_facts(&folder).1, "team/abcdef12/dev");
    // A different repository is still refused.
    let other = folder_repo(dir.path(), "other");
    let problem = folder_problem(&other.display().to_string(), &repo.display().to_string(), None).unwrap();
    assert!(problem.contains("belongs to another repository"), "{problem}");
}

#[test]
fn an_existing_member_folder_is_reused_and_a_foreign_one_refused() {
    // C12: a relaunch or a swap starts the member again in its own worktree
    // or clone; a foreign folder at that path is refused, an empty
    // reservation left by a failed create goes.
    let dir = tempfile::tempdir().unwrap();
    set_git_ceiling(&dir.path().display().to_string());
    let repo = folder_repo(dir.path(), "project");
    for kind in [MemberFolder::Worktree, MemberFolder::Clone] {
        let member = Member { id: format!("m{}", kind == MemberFolder::Clone), role: "Dev".into(), agent: "codex".into(), folder: kind, ..Default::default() };
        let team = folder_team(&repo, member.clone());
        let calls = std::cell::Cell::new(0);
        let run = |_: &str, args: &[&str]| { calls.set(calls.get() + 1); local_create(args) };
        let first = member_folder_with(&team, &member, &run).unwrap();
        let again = member_folder_with(&team, &member, &run).unwrap();
        assert_eq!((first.as_str(), calls.get()), (again.as_str(), 1), "{kind:?}");
    }
    // A foreign repository where the member's folder belongs.
    let member = Member { id: "qa".into(), role: "QA".into(), agent: "codex".into(), folder: MemberFolder::Worktree, ..Default::default() };
    let team = folder_team(&repo, member.clone());
    let destination = member_folder_path(&team, &member).unwrap();
    let foreign = folder_repo(dir.path(), Path::new(&destination).file_name().unwrap().to_str().unwrap());
    assert_eq!(foreign.display().to_string(), destination);
    let error = member_folder_with(&team, &member, &|_, args| local_create(args)).unwrap_err().to_string();
    assert!(error.contains("already exists and is not this member's folder"), "{error}");
    // An empty reservation from a failed create is replaced.
    std::fs::remove_dir_all(&foreign).unwrap();
    std::fs::create_dir(&destination).unwrap();
    let folder = member_folder_with(&team, &member, &|_, args| local_create(args)).unwrap();
    assert_eq!(folder_facts(&folder).1, "team/abcdef12/qa");
}

#[test]
fn a_member_on_another_computer_is_checked_there() {
    // C11: the folder of a member on another computer is not looked for
    // here; what is there is read there (`hgs @computer worktrees`).
    let ok = std::process::Command::new("git").arg("--version").output().unwrap();
    let reply = |stdout: Value| Output { status: ok.status, stdout: stdout.to_string().into_bytes(), stderr: Vec::new() };
    let folder = if cfg!(windows) { r"D:\work\shop" } else { "/work/shop" };
    let member = Member { id: "dev".into(), role: "Backend".into(), agent: "codex".into(), folder: MemberFolder::Worktree, computer: "devbox".into(), ..Default::default() };
    let team = folder_team(Path::new(folder), member.clone());
    let destination = member_folder_path(&team, &member).unwrap();
    // Nothing there yet: it is created there, with no local check.
    let seen = std::cell::RefCell::new(Vec::<String>::new());
    let run = |computer: &str, args: &[&str]| -> anyhow::Result<Output> {
        seen.borrow_mut().push(format!("{computer} {}", args[..2].join(" ")));
        Ok(if args[1] == "create" { reply(json!({"status":"created"})) } else { reply(json!({"state":"folder_unavailable"})) })
    };
    assert_eq!(member_folder_with(&team, &member, &run).unwrap(), destination);
    assert_eq!(*seen.borrow(), vec!["devbox worktrees --path".to_owned(), "devbox worktrees create".to_owned()]);
    // Its own checkout there is used again; another branch is refused.
    let catalog = |branch: &str| json!({"state":"ok","selected_root":destination,"worktrees":[{"path":folder,"branch":"main"},{"path":destination,"branch":branch}]});
    let created = std::cell::Cell::new(false);
    let mine = |_: &str, args: &[&str]| -> anyhow::Result<Output> { created.set(created.get() || args[1] == "create"); Ok(reply(catalog("team/abcdef12/dev"))) };
    assert_eq!(member_folder_with(&team, &member, &mine).unwrap(), destination);
    assert!(!created.get());
    let error = member_folder_with(&team, &member, &|_, _| Ok(reply(catalog("main")))).unwrap_err().to_string();
    assert!(error.contains("on devbox and is not this member's folder (on main"), "{error}");
    // A shared folder there is not looked for on this computer.
    let shared = Member { folder: MemberFolder::Shared, ..member.clone() };
    let team = folder_team(Path::new(folder), shared.clone());
    assert_eq!(member_folder_with(&team, &shared, &|_, _| anyhow::bail!("no call")).unwrap(), folder);
}

#[test]
fn the_continuation_reads_no_repository_that_would_run_code() {
    // C13: the git part of a swap's hand-off goes through the bounded,
    // hardened path, and a folder whose own config names a filter, hook path
    // or fsmonitor is not read at all.
    let dir = tempfile::tempdir().unwrap();
    set_git_ceiling(&dir.path().display().to_string());
    let repo = folder_repo(dir.path(), "project");
    let folder = repo.display().to_string();
    let text = git_continuation(&folder, 0.0);
    assert!(text.contains("Commits since the attempt began") && text.contains("project"), "{text}");
    let marker = dir.path().join("ran");
    let config = |args: &[&str]| assert!(std::process::Command::new("git").arg("-C").arg(&repo).args(args).status().unwrap().success());
    config(&["config", "--local", "core.fsmonitor", &format!("echo x > {}", marker.display())]);
    let refused = git_continuation(&folder, 0.0);
    assert!(refused.contains("not read here, this repository configures core.fsmonitor"), "{refused}");
    assert!(!marker.exists());
    config(&["config", "--local", "--unset", "core.fsmonitor"]);
    config(&["config", "--local", "filter.evil.clean", "cat"]);
    assert!(git_continuation(&folder, 0.0).contains("configures a Git filter"));
}

#[test]
fn a_folder_that_is_not_the_teams_repository_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    // Hermetic: a TEMP inside another repository must not make the sibling
    // look like a repository of its own.
    set_git_ceiling(&dir.path().display().to_string());
    let repo = dir.path().join("project");
    std::fs::create_dir(&repo).unwrap();
    let git = |args: &[&str]| std::process::Command::new("git").arg("-C").arg(&repo).args(args).status().unwrap();
    assert!(git(&["init", "-q"]).success());
    assert!(git(&["config", "user.email", "t@example.com"]).success());
    assert!(git(&["config", "user.name", "T"]).success());
    std::fs::write(repo.join("a.txt"), "a").unwrap();
    assert!(git(&["add", "a.txt"]).success());
    assert!(git(&["commit", "-qm", "one"]).success());
    assert!(git(&["switch", "-qc", "team/t1/dev"]).success());
    let source = repo.display().to_string();
    // The team folder on the member's branch passes.
    assert_eq!(folder_problem(&source, &source, Some("team/t1/dev")), None);
    // A sibling folder without a repository is refused, never General.
    let sibling = dir.path().join("sibling");
    std::fs::create_dir(&sibling).unwrap();
    let problem = folder_problem(&sibling.display().to_string(), &source, None).unwrap();
    assert!(problem.contains("is not a repository"), "{problem}");
    // A wrong branch and a missing folder are named too.
    let wrong = folder_problem(&source, &source, Some("main")).unwrap();
    assert!(wrong.contains("is on team/t1/dev, not main"), "{wrong}");
    let missing = folder_problem(&dir.path().join("missing").display().to_string(), &source, None).unwrap();
    assert!(missing.contains("does not exist"), "{missing}");
    // Facts: the repository and branch of a folder (F1.6).
    let (project, branch) = folder_facts(&source);
    assert_eq!(branch, "team/t1/dev");
    assert_eq!(Path::new(&project).file_name().and_then(|name| name.to_str()), Some("project"));
    assert_eq!(folder_facts(""), (String::new(), String::new()));
}
