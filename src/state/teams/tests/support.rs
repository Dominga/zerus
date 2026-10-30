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
