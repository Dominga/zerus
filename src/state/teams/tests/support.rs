use super::*;
thread_local! {
    static TEST_SWITCH: std::cell::RefCell<Option<super::TestSwitch>> = const { std::cell::RefCell::new(None) };
}

pub(crate) fn set_test_switch(switch: Option<super::TestSwitch>) {
    TEST_SWITCH.with(|slot| *slot.borrow_mut() = switch);
}

pub(crate) fn test_switch() -> Option<super::TestSwitch> {
    TEST_SWITCH.with(|slot| *slot.borrow())
}

thread_local! {
    pub(super) static SWITCH_CALLS: std::cell::RefCell<Vec<(String, String)>> = const { std::cell::RefCell::new(Vec::new()) };
}

pub(super) fn switched_switch(name: &str, account: &str, _request: &str) -> anyhow::Result<Value> {
    SWITCH_CALLS.with(|calls| calls.borrow_mut().push((name.to_owned(), account.to_owned())));
    Ok(json!({"status":"switched"}))
}

pub(super) fn uncertain_switch(_name: &str, _account: &str, _request: &str) -> anyhow::Result<Value> {
    Ok(json!({"status":"uncertain"}))
}

/// The dispatcher's worker lock, held so a command that would start a
/// worker does not spawn one from the test binary.
pub(super) fn hold_worker_lock() -> crate::platform::FileLock {
    let path = crate::state::teams::dispatcher::lock_path();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = std::fs::OpenOptions::new().create(true).write(true).truncate(false).open(&path).unwrap();
    let lock = crate::platform::FileLock::exclusive(file).unwrap();
    assert!(crate::status::locked(&path), "the worker lock is held");
    lock
}

// ---- #480: the phase 35 review's findings through the real entry point
// (commands::run on a private store), as the owner, the lead and members.

/// A running team: the lead (a dsh session, so a test can act as it),
/// dev and qa; #1 handed to dev, #2 to qa; phase 2 is a draft.
pub(super) fn review_team() -> (Team, String, String) {
    use crate::state::teams::model::{Attempt, AttemptState, TaskStatus};
    let (mut team, _) = ops::create(
        "rv",
        &json!({"name":"Review","lead":{"id":"lead","agent":"dsh",
            "session":{"name":"dsh/x/lead","run_id":"r0","conversation_id":"lead-conversation"}}}),
    )
    .unwrap();
    let mut events = Vec::new();
    ops::apply(
        &mut team,
        "plan",
        &json!({"members":[{"id":"dev","role":"Backend","agent":"codex"},{"id":"qa","role":"Tester","agent":"kimi"}],
            "phases":[{"n":2,"title":"Polish"}],
            "tasks":[{"id":1,"title":"API","assignee":"dev"},{"id":2,"title":"Tests","assignee":"qa"},
                {"id":3,"title":"Docs","assignee":"dev","phase":2}]}),
        &Actor::Human,
        &BTreeSet::new(),
        &mut events,
    )
    .unwrap();
    ops::apply(&mut team, "approve", &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    let mut attempts = Vec::new();
    for id in [1u32, 2] {
        let task = team.task_mut(id).unwrap();
        let attempt = format!("00000000-0000-4000-8000-00000000010{id}");
        task.attempts.push(Attempt { n: 1, id: attempt.clone(), member: task.assignee.clone(), state: AttemptState::Delivered, ..Default::default() });
        task.status = TaskStatus::InProgress;
        attempts.push(attempt);
    }
    (team, attempts[0].clone(), attempts[1].clone())
}

pub(super) fn with_review_team<T>(team: &Team, body: impl FnOnce() -> T) -> T {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        Store::open(true).unwrap().create(&uuid::Uuid::new_v4().to_string(), &json!({}), team.clone(), vec![]).unwrap();
        body()
    })
}

/// One command, with a fresh request id, as the owner unless the input
/// names an attempt (a member) or the lead acts.
pub(super) fn call(op: &str, mut input: Value) -> anyhow::Result<Value> {
    input["request_id"] = json!(uuid::Uuid::new_v4().to_string());
    input["team"] = json!("rv");
    run(op, &input, false)
}

pub(super) fn as_lead<T>(body: impl FnOnce() -> T) -> T {
    set_test_caller_identities(Some((Some("lead-conversation".into()), None, None)));
    let result = body();
    set_test_caller_identities(Some((None, None, None)));
    result
}

pub(super) fn stored() -> Team {
    Store::open(false).unwrap().load("rv").unwrap().unwrap()
}

pub(super) fn finding(severity: &str, title: &str) -> Value {
    json!({"severity":severity,"title":title,"scenario":"two saves at once","task":1})
}

/// U32: the events fixture the lead's own session reads owner words from.
pub(super) fn owner_word_events(dir: &std::path::Path, prompts: &[(f64, &str)]) {
    use rusqlite::Connection;
    let db = Connection::open(dir.join("events.sqlite3")).unwrap();
    db.execute_batch("CREATE TABLE IF NOT EXISTS events (seq INTEGER PRIMARY KEY, name TEXT, conversation TEXT, payload TEXT);").unwrap();
    for (at, detail) in prompts {
        db.execute(
            "INSERT INTO events(name,conversation,payload) VALUES ('dsh/x/lead','c1',?1)",
            rusqlite::params![json!({"type":"UserPromptSubmit","at":at,"agent_id":"","detail":detail}).to_string()],
        ).unwrap();
    }
}

/// U32: a running team whose member cap is already full (six members).
pub(super) fn full_team() -> Team {
    use crate::state::teams::model::{FolderPolicy, Settings};
    let mut team = Team {
        id: "abcdef12-0000-4000-8000-000000000000".into(),
        name: "T".into(),
        lead: "lead".into(),
        status: TeamStatus::Running,
        settings: Settings { max_members: 6, folder_policy: FolderPolicy::Shared, ..Default::default() },
        ..Default::default()
    };
    team.members.push(Member {
        id: "lead".into(),
        role: "Lead".into(),
        agent: "dsh".into(),
        session: Some(SessionRef { name: "dsh/x/lead".into(), conversation_id: "c1".into(), ..Default::default() }),
        ..Default::default()
    });
    for index in 1..=5 {
        team.members.push(Member { id: format!("m{index}"), role: "Backend".into(), agent: "dsh".into(), ..Default::default() });
    }
    team
}

pub(super) use super::super::dispatcher::Seen;

/// A running team with an open hand-out to dev.
pub(super) fn running_team() -> Team {
    let (mut team, _) = ops::create(
        "t",
        &json!({"name":"T","lead":{"id":"lead","agent":"claude","session":{"name":"claude/x/lead","run_id":"r0"}}}),
    )
    .unwrap();
    let mut events = Vec::new();
    ops::apply(
        &mut team,
        "plan",
        &json!({"members":[{"id":"dev","role":"Backend","agent":"codex","session":{"name":"codex/x/dev","run_id":"r1"}}],
            "tasks":[{"id":1,"title":"API","assignee":"dev"}]}),
        &Actor::Human,
        &BTreeSet::new(),
        &mut events,
    )
    .unwrap();
    ops::apply(&mut team, "approve", &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    {
        let task = team.task_mut(1).unwrap();
        task.attempts.push(Attempt {
            n: 1,
            id: "00000000-0000-4000-8000-000000000001".into(),
            member: "dev".into(),
            state: AttemptState::Delivered,
            sent_at: 10.0,
            delivered_at: Some(10.0),
            started_at: Some(11.0),
            ..Default::default()
        });
        task.status = TaskStatus::InProgress;
    }
    team
}

pub(super) fn found_session(name: &str, run: &str, live: bool, cwd: &str) -> Seen {
    Seen {
        name: name.into(),
        run_id: run.into(),
        conversation_id: "conversation-1".into(),
        cwd: cwd.into(),
        live,
        ..Default::default()
    }
}

pub(super) fn existing_member() -> Member {
    Member {
        id: "dev2".into(),
        role: "Backend".into(),
        agent: "dsh".into(),
        session: Some(SessionRef { name: "dsh/x/dev2".into(), ..Default::default() }),
        ..Default::default()
    }
}

pub(super) fn live_session(session: &SessionRef) -> Seen {
    Seen {
        host: session.host.clone(),
        name: session.name.clone(),
        run_id: session.run_id.clone(),
        conversation_id: session.conversation_id.clone(),
        live: true,
        ..Default::default()
    }
}

/// What the dispatcher records for a delivered hand-off.
pub(super) fn hand_out(team: &mut Team, id: u32) -> String {
    let task = team.task_mut(id).unwrap();
    let n = task.attempts.len() as u32 + 1;
    let attempt = format!("00000000-0000-4000-8000-00000000000{n}");
    task.attempts.push(Attempt { n, id: attempt.clone(), member: task.assignee.clone(), state: AttemptState::Delivered, ..Default::default() });
    task.status = TaskStatus::InProgress;
    attempt
}

/// The running team with "qalead" as the Tester's sub-lead (spec 5.6).
pub(super) fn team_with_sub_lead() -> Team {
    let mut team = running_team();
    let none = BTreeSet::new();
    let mut events = Vec::new();
    apply(&mut team, "add-member", &json!({"member":{"id":"qalead","role":"QA lead","agent":"codex"}}), &Actor::Human, &none, &mut events)
        .unwrap();
    apply(&mut team, "sub-lead", &json!({"member":"qalead","sub":true}), &Actor::Member("lead".into()), &none, &mut events).unwrap();
    apply(&mut team, "reports-to", &json!({"member":"qa","parent":"qalead"}), &Actor::Human, &none, &mut events).unwrap();
    team
}

pub(super) fn kinds(events: &[Event]) -> Vec<String> {
    events.iter().map(|event| event.kind.to_owned()).collect()
}

use super::super::ops::{self, Actor};
fn team() -> Team {
    let (mut team, _) = ops::create(
        "t",
        &json!({"name":"Checkout","lead":{"id":"lead","agent":"claude","session":{"name":"claude/x/lead","run_id":"r0"}}}),
    )
    .unwrap();
    let mut events = Vec::new();
    ops::apply(
        &mut team,
        "plan",
        &json!({"members":[{"id":"dev","role":"Backend","agent":"codex","session":{"name":"codex/x/dev","run_id":"r1"}},
            {"id":"qa","role":"Tester","agent":"kimi"}],
            "tasks":[{"id":1,"title":"API","assignee":"dev"},{"id":2,"title":"More API","assignee":"dev"},
                {"id":3,"title":"Tests","assignee":"qa","deps":[{"task":1,"kind":"soft"}]}]}),
        &Actor::Human,
        &BTreeSet::new(),
        &mut events,
    )
    .unwrap();
    ops::apply(&mut team, "approve", &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    team
}
/// U36: dev reported #1 under the Code pipeline; rev reviews and qa tests,
/// each with a free session; every member works in `folder`.
fn staged(folder: &str) -> Team {
    let mut t = team();
    t.folder = folder.to_owned();
    t.members.push(Member { id: "rev".into(), role: "Reviewer".into(), agent: "claude".into(), state: MemberState::Active,
        session: Some(SessionRef { name: "claude/x/rev".into(), run_id: "r2".into(), ..Default::default() }), ..Default::default() });
    for member in &mut t.members {
        member.folder = MemberFolder::Shared;
        if member.id == "qa" {
            member.state = MemberState::Active;
            member.session = Some(SessionRef { name: "kimi/x/qa".into(), run_id: "r3".into(), ..Default::default() });
        }
    }
    t.task_mut(1).unwrap().pipeline = "code".into();
    super::super::pipeline::snapshot_with(&mut t, 1, &super::super::catalog::builtin());
    let task = t.task_mut(1).unwrap();
    task.attempts.push(Attempt { n: 1, id: "a1".into(), member: "dev".into(), state: AttemptState::Delivered, ..Default::default() });
    task.status = TaskStatus::InProgress;
    let mut events = Vec::new();
    ops::apply(&mut t, "report", &json!({"task":1,"attempt":"a1","text":"Done"}), &Actor::Member("dev".into()), &BTreeSet::new(), &mut events).unwrap();
    assert_eq!(t.task(1).unwrap().status, TaskStatus::InReview);
    t
}
fn free_sessions() -> Vec<Seen> {
    vec![session("claude/x/lead", "r0", true), session("codex/x/dev", "r1", true), session("claude/x/rev", "r2", true), session("kimi/x/qa", "r3", true)]
}
fn session(name: &str, run: &str, free: bool) -> Seen {
    Seen { name: name.into(), run_id: run.into(), conversation_id: "c".into(), free, idle: free, live: true, ..Default::default() }
}
/// U52: the listing row of a session that is no longer running, with the
/// state the listing reports for it.
fn stopped(name: &str, run: &str, state: &str) -> Seen {
    Seen { name: name.into(), run_id: run.into(), conversation_id: "c".into(), live: false, state: state.into(), ..Default::default() }
}
/// A hand-out delivered at 10 to `dev`, with the attempts list one open.
fn delivered(team: &mut Team) {
    let task = team.task_mut(1).unwrap();
    task.attempts.push(Attempt {
        n: 1,
        id: "00000000-0000-4000-8000-000000000001".into(),
        member: "dev".into(),
        state: AttemptState::Delivered,
        sent_at: 10.0,
        delivered_at: Some(10.0),
        ..Default::default()
    });
    task.status = TaskStatus::InProgress;
}
fn reopen_store(dir: &std::path::Path) -> (Store, Team) {
    let mut store = Store::open_at(&dir.join("teams/teams.sqlite3"), true).unwrap();
    let mut t = team();
    let task = t.task_mut(1).unwrap();
    task.reopen_note = "Finding f1 (critical): Lost write\nScenario: two saves at once".into();
    task.pinned = true;
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
    (store, t)
}
fn event_kinds(store: &Store, team: &str) -> Vec<String> {
    let page = store.events(team, None, 50).unwrap();
    let mut items: Vec<String> = page["items"].as_array().unwrap().iter().map(|event| event["kind"].as_str().unwrap_or_default().to_owned()).collect();
    items.reverse();
    items
}
/// U46: the member the hand-out prepares, with a session and a setting.
fn with_context(team: &mut Team, member: &str, setting: &str) {
    let found = team.members.iter_mut().find(|found| found.id == member).unwrap();
    found.session = Some(SessionRef { name: format!("codex/x/{member}"), run_id: "r1".into(), conversation_id: "c1".into(), ..Default::default() });
    found.state = MemberState::Active;
    found.context_before_task = setting.into();
}
/// U46: one dispatcher tick for a waiting hand-out, as the worker runs it.
fn prepare_tick(store: &mut Store, team_id: &str) -> bool {
    let team = store.load(team_id).unwrap().unwrap();
    let found = steps(&team, &[], crate::state::now());
    let step = found.iter().find(|step| matches!(step, Step::PrepareContext { .. })).cloned();
    if let Some(step) = &step {
        run_step(store, &team, step, "").unwrap();
    }
    step.is_some()
}
/// The sessions the local recovery service is asked to wait for.
fn local_overrides() -> Vec<String> {
    let path = crate::state::storage::root().join("recovery/team.json");
    let value: Value = std::fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
    value["items"].as_object().map(|items| items.keys().cloned().collect()).unwrap_or_default()
}
fn limited_dev(name: &str, run: &str) -> Seen {
    let mut dev = session(name, run, false);
    dev.phase = "error".into();
    dev.limited = Some(SeenLimit { at: 100.0, resets_at: Some(200.0), detail: "session limit".into() });
    dev
}
/// Every step of one pass, as the worker runs them.
fn pass(store: &mut Store, id: &str, seen: &[Seen]) -> Vec<Step> {
    let team = store.load(id).unwrap().unwrap();
    let found = steps(&team, seen, 300.0);
    for step in &found {
        run_step(store, &team, step, "").unwrap();
    }
    found
}
/// U21: a repository whose `teams-v2/lead` branch diverged from the member's
/// branch, with a post-merge hook that writes `marker` when it runs.
fn diverged_repo(dir: &std::path::Path, marker: &std::path::Path) -> bool {
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.email=t@e", "-c", "user.name=T"])
            .args(args)
            .status()
            .is_ok_and(|status| status.success())
    };
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("a.txt"), "a").unwrap();
    // merge_lead makes the merge commit itself: the repository carries its
    // own identity for a machine without a global one (a CI runner).
    git(&["init", "-q"])
        && git(&["config", "user.email", "t@example.com"])
        && git(&["config", "user.name", "T"])
        && git(&["add", "."])
        && git(&["commit", "-qm", "A"])
        && git(&["checkout", "-qb", "teams-v2/lead"])
        && std::fs::write(dir.join("lead.txt"), "l").is_ok()
        && git(&["add", "."])
        && git(&["commit", "-qm", "lead"])
        && git(&["checkout", "-q", "-"])
        && std::fs::write(dir.join("mine.txt"), "m").is_ok()
        && git(&["add", "."])
        && git(&["commit", "-qm", "mine"])
        && std::fs::create_dir_all(dir.join(".git").join("hooks")).is_ok()
        && std::fs::write(dir.join(".git").join("hooks").join("post-merge"), format!("#!/bin/sh\necho x > \"{}\"\n", marker.display())).is_ok()
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
fn folder_team(folder: &Path, member: Member) -> Team {
    let mut team = Team { id: "abcdef12-0000-4000-8000-000000000000".into(), name: "T".into(), lead: "lead".into(), folder: folder.display().to_string(), ..Default::default() };
    team.members.push(Member { id: "lead".into(), role: "Lead".into(), agent: "claude".into(), ..Default::default() });
    team.members.push(member);
    team
}

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
