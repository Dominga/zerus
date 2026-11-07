use super::*;
use crate::state::teams::model::{Attempt, TaskStatus};
use crate::state::teams::store::Store;

fn task() -> Task {
    Task { id: 12, title: "Render the black frame fix".into(), ..Default::default() }
}

fn event(kind: &str, tool: &str, detail: &str, at: f64) -> Value {
    json!({"type": kind, "tool": tool, "detail": detail, "at": at.to_string(), "seq": "1"})
}

/// Task 133: an automatic pulse names the task, the files changed and the
/// last tool line; it is never a generic "worked on the task".
#[test]
fn an_automatic_pulse_names_the_task_files_and_last_tool() {
    let events = vec![
        event("PreToolUse", "Edit", r"D:\work\dev\src\render\post.rs", 10.0),
        event("PostToolUse", "Edit", r"D:\work\dev\src\render\post.rs", 11.0),
        event("PostToolUse", "Write", "D:/work/dev/tests/post_test.rs", 12.0),
        event("PostToolUse", "Bash", "hgs team run --json < run.json\nsecond line", 13.0),
        event("UserPromptSubmit", "", "the whole chat prompt text that must never be stored", 14.0),
    ];
    let (done, next, blocked) = auto(&task(), &events, 5.0, r"D:\work\dev").unwrap();
    assert_eq!(done, "#12 Render the black frame fix; changed src/render/post.rs, tests/post_test.rs");
    assert_eq!(next, "last: Bash: hgs team run --json < run.json");
    assert_eq!(blocked, "");
    for field in [&done, &next, &blocked] {
        assert!(!field.contains("chat prompt"), "no chat text is stored");
    }
    // Only the turn's events count.
    assert_eq!(auto(&task(), &events, 13.5, r"D:\work\dev"), None, "nothing concrete: no pulse");
    assert_eq!(auto(&task(), &[], 0.0, ""), None);
}

#[test]
fn a_failed_last_tool_is_the_blocked_line() {
    let events = vec![
        event("PostToolUse", "Bash", "cargo build", 1.0),
        event("PostToolUseFailure", "Bash", "error[E0425]: cannot find value `frame` in this scope\n  --> src/post.rs:3", 2.0),
    ];
    let (done, next, blocked) = auto(&task(), &events, 0.0, "").unwrap();
    assert_eq!(done, "#12 Render the black frame fix");
    assert_eq!(next, "last: Bash: cargo build");
    assert_eq!(blocked, "Bash failed: error[E0425]: cannot find value `frame` in this scope");
    // A later tool that succeeds clears it.
    let mut later = events.clone();
    later.push(event("PostToolUse", "Bash", "cargo build", 3.0));
    assert_eq!(auto(&task(), &later, 0.0, "").unwrap().2, "");
}

#[test]
fn a_pulse_is_bounded() {
    let many: Vec<Value> = (0..9).map(|n| event("PostToolUse", "Edit", &format!("src/file{n}.rs"), f64::from(n) + 1.0)).collect();
    let (done, _, _) = auto(&task(), &many, 0.0, "").unwrap();
    assert!(done.ends_with("and 4 more"), "{done}");
    let long = event("PostToolUse", "Bash", &"x".repeat(2000), 1.0);
    let (_, next, _) = auto(&task(), &[long], 0.0, "").unwrap();
    assert!(next.chars().count() <= "last: Bash: ".len() + 160, "one line of at most 160 characters: {}", next.chars().count());
    assert!(check_note(&"y".repeat(501), "", "").unwrap_err().to_string().contains("at most 500"));
    assert!(check_note("", "", " ").is_err(), "a note says something");
    assert!(check_note("built the shader cache", "", "").is_ok());
}

#[test]
fn the_store_keeps_the_latest_pulse_per_member_bounded() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let pulse = |member: &str, task: u32, done: &str, at: f64| Pulse {
            team: "t".into(), member: member.into(), task, done: done.into(), at, source: "auto".into(), ..Pulse::default()
        };
        let first = store.put_pulse(&pulse("dev", 1, "started", 1_000.0)).unwrap();
        // Within 5 minutes the same member and task replace in place.
        let replaced = store.put_pulse(&pulse("dev", 1, "halfway", 1_200.0)).unwrap();
        assert_eq!(replaced.id, first.id);
        store.put_pulse(&pulse("dev", 1, "done", 2_000.0)).unwrap();
        store.put_pulse(&pulse("qa", 2, "tested", 1_500.0)).unwrap();
        let latest = store.latest_pulses("t", 3_000.0);
        assert_eq!(latest.iter().map(|p| (p.member.as_str(), p.done.as_str())).collect::<Vec<_>>(), [("dev", "done"), ("qa", "tested")]);
        assert_eq!(store.pulses("t", Some("dev"), 3_000.0, 10).len(), 2);
        // A closed task's pulses expire 14 days after its close.
        let mut team = crate::state::teams::model::Team { id: "t".into(), ..Default::default() };
        let mut closed = Task { id: 1, status: TaskStatus::Accepted, ..Default::default() };
        closed.attempts.push(Attempt { decided_at: Some(2_500.0), ..Default::default() });
        team.tasks.push(closed);
        team.tasks.push(Task { id: 2, ..Default::default() });
        assert_eq!(store.prune_pulses(&team, 2_500.0 + KEEP_SECONDS - 1.0).unwrap(), 0);
        assert_eq!(store.prune_pulses(&team, 2_500.0 + KEEP_SECONDS).unwrap(), 2);
        assert_eq!(store.latest_pulses("t", 2_500.0 + KEEP_SECONDS).len(), 1, "the open task's pulse stays");
    });
}

#[test]
fn the_digest_lists_each_member_once() {
    let mut team = crate::state::teams::model::Team { id: "t".into(), lead: "lead".into(), ..Default::default() };
    team.members.push(crate::state::teams::model::Member { id: "dev".into(), role: "Backend".into(), ..Default::default() });
    let pulses = vec![Pulse { member: "dev".into(), task: 12, done: "#12 changed src/post.rs".into(), blocked: "cargo failed".into(), at: 100.0, ..Pulse::default() }];
    let text = digest(&team, &pulses, 700.0);
    assert_eq!(text, "What each member is doing (latest pulse):\n- dev (Backend) #12, 10 min ago; done: #12 changed src/post.rs; blocked: cargo failed");
    assert_eq!(digest(&team, &[], 0.0), "");
}

/// Task 187: a reopened task's pulses lose their expiry and stay; a task
/// closed without a decision (cancelled before a hand-out) and an open task of
/// a finished team expire from the first pass that saw them closed, a time
/// that never moves (team.updated does), so they do expire.
#[test]
fn pulse_expiry_follows_reopens_undecided_closes_and_a_finished_team() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let pulse = |member: &str, task: u32, at: f64| Pulse {
            team: "t".into(), member: member.into(), task, done: "worked".into(), at, source: "auto".into(), ..Pulse::default()
        };
        for (member, task) in [("dev", 1), ("qa", 2), ("ops", 3)] {
            store.put_pulse(&pulse(member, task, 1_000.0)).unwrap();
        }
        let mut team = crate::state::teams::model::Team { id: "t".into(), updated: 5_000.0, ..Default::default() };
        // #1 accepted at 2,000; #2 cancelled with no attempt; #3 open.
        let mut accepted = Task { id: 1, status: TaskStatus::Accepted, ..Default::default() };
        accepted.attempts.push(Attempt { decided_at: Some(2_000.0), ..Default::default() });
        team.tasks.push(accepted);
        team.tasks.push(Task { id: 2, status: TaskStatus::Cancelled, ..Default::default() });
        team.tasks.push(Task { id: 3, ..Default::default() });
        assert_eq!(store.prune_pulses(&team, 3_000.0).unwrap(), 0);
        // #1 is reopened before its expiry: its pulse stays past it.
        team.tasks[0].status = TaskStatus::InProgress;
        team.tasks[0].attempts.clear();
        assert_eq!(store.prune_pulses(&team, 3_000.0 + KEEP_SECONDS + 10.0).unwrap(), 1, "only the cancelled task's pulse goes");
        assert_eq!(store.pulses("t", Some("dev"), 9e9, 10).len(), 1, "the reopened task's pulse stays");
        assert!(store.pulses("t", Some("qa"), 9e9, 10).is_empty(), "the undecided cancel expired 14 days after it was first seen");
        // The team finishes; its open tasks expire from the first pass that
        // sees it finished, whatever team.updated does afterwards.
        team.status = crate::state::teams::model::TeamStatus::Done;
        let seen = 3_000.0 + KEEP_SECONDS + 20.0;
        assert_eq!(store.prune_pulses(&team, seen).unwrap(), 0);
        team.updated = seen + KEEP_SECONDS * 2.0;
        assert_eq!(store.prune_pulses(&team, seen + KEEP_SECONDS - 1.0).unwrap(), 0);
        assert_eq!(store.prune_pulses(&team, seen + KEEP_SECONDS).unwrap(), 2, "both open tasks' pulses expire");
    });
}
