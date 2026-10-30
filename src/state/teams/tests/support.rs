use super::*;

/// N6: one NoReport step with a given `hgs inspect` record.
fn capture_with(reply: Value) -> (tempfile::TempDir, Store, String, u32) {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut t = team();
    t.status = TeamStatus::Running;
    let task = t.tasks.iter_mut().find(|t| t.assignee == "dev").unwrap();
    task.attempts.push(Attempt {
        n: 1,
        id: "A1".into(),
        member: "dev".into(),
        state: AttemptState::Delivered,
        sent_at: 10.0,
        delivered_at: Some(10.0),
        started_at: Some(11.0),
        ..Default::default()
    });
    task.status = TaskStatus::InProgress;
    let id = task.id;
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
    inspect_override(Some(reply));
    run_step(&mut store, &t, &Step::NoReport { task: id }, "").unwrap();
    inspect_override(None);
    (dir, store, t.id, id)
}

/// The team with a second Backend member, dev2, free and started.
fn team_with_two_backends() -> (Team, Vec<Seen>) {
    let mut team = team();
    let mut dev2 = team.member("dev").unwrap().clone();
    dev2.id = "dev2".into();
    dev2.session = Some(SessionRef { name: "codex/x/dev2".into(), run_id: "r3".into(), conversation_id: "c".into(), ..Default::default() });
    team.members.push(dev2);
    let seen = vec![session("claude/x/lead", "r0", false), session("codex/x/dev", "r1", true), session("codex/x/dev2", "r3", true)];
    (team, seen)
}

/// A7: `member`'s report on `task`, Submitted and undecided; `captured`
/// when Zerus took it from the member's last reply (A2).
fn submit(team: &mut Team, task: u32, member: &str, captured: bool) {
    let task = team.task_mut(task).unwrap();
    task.status = TaskStatus::Submitted;
    task.attempts.push(Attempt {
        n: task.attempts.len() as u32 + 1,
        id: format!("A{}", task.id),
        member: member.into(),
        state: AttemptState::Reported,
        report: Some(Report {
            text: "(From Backend's last reply; it ran no hgs team report.)\n\nQueued, after FU-trees".into(),
            at: 1.0,
            captured,
            ..Default::default()
        }),
        ..Default::default()
    });
}

fn attempt_with_report(member: &str, text: &str, state: AttemptState) -> Attempt {
    Attempt { n: 1, member: member.into(), state, report: Some(Report { text: text.into(), at: 0.0, ..Default::default() }), ..Default::default() }
}

/// A Git repository with one commit in `dir/name`, for the folder tests.
fn folder_repo(dir: &Path, name: &str) -> PathBuf {
    let repo = dir.join(name);
    std::fs::create_dir(&repo).unwrap();
    let git = |args: &[&str]| std::process::Command::new("git").arg("-C").arg(&repo).args(args).status().unwrap();
    assert!(git(&["init", "-q"]).success());
    assert!(git(&["config", "user.email", "t@example.com"]).success());
    assert!(git(&["config", "user.name", "T"]).success());
    std::fs::write(repo.join(format!("{name}.txt")), name).unwrap();
    assert!(git(&["add", "."]).success());
    assert!(git(&["commit", "-qm", name]).success());
    repo
}

/// What `hgs worktrees create` does on this computer, for `member_folder_with`.
fn local_create(args: &[&str]) -> anyhow::Result<Output> {
    let value = |key: &str| args.iter().position(|arg| *arg == key).map(|at| args[at + 1].to_owned()).unwrap();
    let (source, branch, destination) = (value("--path"), value("--branch"), value("--destination"));
    if args.contains(&"--clone") {
        let cloned = std::process::Command::new("git").args(["clone", "--local", "--no-hardlinks", "--quiet", &source, &destination]).output()?;
        anyhow::ensure!(cloned.status.success(), "{}", String::from_utf8_lossy(&cloned.stderr));
        Ok(std::process::Command::new("git").arg("-C").arg(&destination).args(["switch", "--quiet", "-c", &branch]).output()?)
    } else {
        Ok(std::process::Command::new("git").arg("-C").arg(&source).args(["worktree", "add", "--quiet", "-b", &branch, &destination, "HEAD"]).output()?)
    }
}

fn folder_team(folder: &Path, member: Member) -> Team {
    let mut team = Team { id: "abcdef12-0000-4000-8000-000000000000".into(), name: "T".into(), lead: "lead".into(), folder: folder.display().to_string(), ..Default::default() };
    team.members.push(Member { id: "lead".into(), role: "Lead".into(), agent: "claude".into(), ..Default::default() });
    team.members.push(member);
    team
}

/// U47/U48: one git command in a test repository, which must succeed.
fn git_in(folder: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git").arg("-C").arg(folder).args(["-c", "commit.gpgsign=false"]).args(args).output().unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn git_fails(folder: &Path, args: &[&str]) -> bool {
    !std::process::Command::new("git").arg("-C").arg(folder).args(args).output().unwrap().status.success()
}

const STAGE_TEAM: &str = "77777777-7777-4777-8777-777777777777";

/// A real repository: the lead's folder on main and dev's worktree on its
/// team branch, as Zerus makes them.
fn stage_repo(root: &Path) -> (PathBuf, PathBuf) {
    let lead = root.join("lead");
    std::fs::create_dir_all(&lead).unwrap();
    git_in(&lead, &["init", "-q"]);
    git_in(&lead, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    git_in(&lead, &["config", "user.name", "Zerus Test"]);
    git_in(&lead, &["config", "user.email", "test@example.com"]);
    std::fs::write(lead.join("gate.txt"), "fail\n").unwrap();
    git_in(&lead, &["add", "-A"]);
    git_in(&lead, &["commit", "-q", "-m", "init"]);
    let dev = root.join("dev");
    git_in(&lead, &["worktree", "add", "-q", "-b", &format!("team/{}/dev", &STAGE_TEAM[..8]), &dev.display().to_string()]);
    (lead, dev)
}

/// A running team in that repository: dev (Backend) works in its own
/// worktree, rev reviews; tasks #1 and #2 are dev's, each with the preset
/// `catalog(n)` gives it.
fn stage_team(lead: &Path, dev: &Path, pipeline: &str, catalog: &dyn Fn(u32) -> super::super::catalog::Catalog) -> Team {
    let (mut team, _) = ops::create(
        "t",
        &json!({"name":"Stages","lead":{"id":"lead","agent":"claude","session":{"name":"claude/x/lead","run_id":"r0"}}}),
    )
    .unwrap();
    let mut events = Vec::new();
    ops::apply(
        &mut team,
        "plan",
        &json!({"members":[{"id":"dev","role":"Backend","agent":"codex","folder":"worktree","directory":dev.display().to_string(),
                "session":{"name":"codex/x/dev","run_id":"r1"}},
            {"id":"rev","role":"Reviewer","agent":"claude","session":{"name":"claude/x/rev","run_id":"r2"}}],
            "tasks":[{"id":1,"title":"Cards API","assignee":"dev","pinned":true},{"id":2,"title":"Cards paging","assignee":"dev","pinned":true}]}),
        &Actor::Human,
        &BTreeSet::new(),
        &mut events,
    )
    .unwrap();
    ops::apply(&mut team, "approve", &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    team.id = STAGE_TEAM.into();
    team.folder = lead.display().to_string();
    for id in [1, 2] {
        team.task_mut(id).unwrap().pipeline = pipeline.into();
        super::super::pipeline::snapshot_with(&mut team, id, &catalog(id));
    }
    team
}

/// The owner's preset of one Zerus stage of `kind` running `command`.
fn command_preset(kind: &str, command: String, exits: &[(&str, &str)]) -> super::super::catalog::Catalog {
    let mut catalog = super::super::catalog::builtin();
    catalog.pipelines.push(super::super::catalog::Pipeline {
        id: "gate".into(),
        name: format!("Gate: {kind}"),
        stages: vec![super::super::catalog::Stage {
            kind: kind.into(),
            command: Some(Check { command, expect_exit: 0, timeout_minutes: 5 }),
            exits: exits.iter().map(|(code, outcome)| (code.to_string(), outcome.to_string())).collect(),
            ..Default::default()
        }],
        ..Default::default()
    });
    catalog
}

/// #680: the same preset with the checkout rule.
fn checkout_command_preset(kind: &str, command: String) -> super::super::catalog::Catalog {
    let mut catalog = command_preset(kind, command, &[("0", "accepted")]);
    catalog.pipelines.last_mut().unwrap().folder = "checkout".into();
    catalog
}

/// #683: Deliver - review, then the command, then land - under the hold rule.
fn deliver_preset() -> super::super::catalog::Catalog {
    let mut catalog = command_preset("command", "git status --porcelain".into(), &[]);
    let pipeline = catalog.pipelines.last_mut().unwrap();
    pipeline.id = "deliver".into();
    pipeline.folder = "hold".into();
    pipeline.stages.insert(0, super::super::catalog::Stage { kind: "review".into(), role: "reviewer".into(), ..Default::default() });
    pipeline.stages.push(super::super::catalog::Stage {
        kind: "land".into(),
        command: Some(Check { command: "git status --porcelain".into(), expect_exit: 0, timeout_minutes: 5 }),
        exits: [("0".to_owned(), "accepted".to_owned())].into_iter().collect(),
        ..Default::default()
    });
    catalog
}

/// #680: a stage command that writes the environment it got into `env.txt`.
fn env_command() -> String {
    if cfg!(windows) {
        "> env.txt echo %HGS_TASK_ID%^|%HGS_TASK_BRANCH%^|%HGS_TASK_COMMIT%".to_owned()
    } else {
        "echo \"$HGS_TASK_ID|$HGS_TASK_BRANCH|$HGS_TASK_COMMIT\" > env.txt".to_owned()
    }
}

/// The member reports its open attempt of `task`, as `hgs team report` does.
fn stage_report(store: &mut Store, task: u32) {
    let team = store.load(STAGE_TEAM).unwrap().unwrap();
    let attempt = team.task(task).unwrap().open_attempt().unwrap().id.clone();
    store
        .mutate(None, STAGE_TEAM, None, |team, events| {
            ops::apply(team, "report", &json!({"task":task,"attempt":attempt,"text":"Done"}), &Actor::Member("dev".into()), &BTreeSet::new(), events)
        })
        .unwrap();
}

/// The dispatcher's two passes over a Zerus stage: start it (the runner
/// runs inline in a test), then read its record and decide.
fn run_stage(store: &mut Store) -> String {
    let team = store.load(STAGE_TEAM).unwrap().unwrap();
    let found = steps(&team, &[], now());
    let Some(Step::StageCommand { task, run }) = found.iter().find(|step| matches!(step, Step::StageCommand { .. })).cloned() else {
        panic!("no stage command in {found:?}")
    };
    start_stage_command(store, &team, task, &run).unwrap();
    let team = store.load(STAGE_TEAM).unwrap().unwrap();
    let found = steps(&team, &[], now());
    assert!(!found.iter().any(|step| matches!(step, Step::StageCommand { .. })), "a started stage never starts twice");
    // A stage refused before it ran (land_allowed) has nothing to read.
    if found.contains(&Step::StageCommandEnded { task, run: run.clone() }) {
        settle_stage_command(store, &team, task, &run).unwrap();
    }
    run
}

fn feed_kinds(store: &Store) -> Vec<String> {
    let page = store.events(STAGE_TEAM, None, 200).unwrap();
    let mut kinds: Vec<String> = page["items"].as_array().unwrap().iter().map(|item| item["kind"].as_str().unwrap().to_owned()).collect();
    kinds.reverse();
    kinds
}

fn deliver_all() {
    set_fake_hgs(Some(Box::new(|_, args: &[&str], _| {
        assert_eq!(args.first().copied(), Some("send"));
        Ok(fake_output(0, r#"{"status":"submitted"}"#, ""))
    })));
}

/// A team whose running phase has no ready work, and a draft phase 2 with
/// #4 (dev), #5 (qa) that depends on #4, and #6 (dev) that depends on the
/// running #1.
fn work_ahead_team() -> Team {
    let mut t = team();
    for task in &mut t.tasks {
        if task.phase == 1 {
            task.status = TaskStatus::Submitted;
            task.attempts.push(Attempt { n: 1, state: AttemptState::Reported, member: "dev".into(), ..Default::default() });
        }
    }
    let mut events = Vec::new();
    ops::apply(&mut t, "extend", &json!({"phase_title":"Next","tasks":[
        {"id":4,"title":"Independent docs","assignee":"dev"},
        {"id":5,"title":"Tests of the docs","assignee":"qa","deps":[{"task":4,"kind":"hard"}]},
        {"id":6,"title":"After the API","assignee":"dev","deps":[{"task":1,"kind":"soft"}]}]}),
        &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    t.settings.work_ahead = true;
    t.members.iter_mut().find(|member| member.id == "qa").unwrap().session =
        Some(SessionRef { name: "kimi/x/qa".into(), run_id: "r3".into(), ..Default::default() });
    t
}
