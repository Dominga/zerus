use super::super::ops::{self, Actor};
use super::*;

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

/// U46: a stored team for the context tests, in a state root of its own.
fn stored(store: &mut Store, team: &Team) {
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), team.clone(), vec![]).unwrap();
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
