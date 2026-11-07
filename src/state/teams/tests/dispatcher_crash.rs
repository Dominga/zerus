//! Task #227 (owner, 2026-11-07: "tasks 192/196 stalled"): a member session
//! that crashes mid-attempt gets the same attempt back by itself; a retry while
//! it works on the task never revokes it; the sweep after an update; the
//! unassigned item naming the task its branch shows.
use super::support::*;
use super::*;
use std::cell::RefCell;
use std::rc::Rc;

const ATTEMPT: &str = "00000000-0000-4000-8000-000000000001";

/// What a crashed session left: the delivered attempt (or a task's attempt) is
/// unconfirmed with the dispatcher's line, and the member carries the crash.
fn crashed(team: &mut Team) {
    if let Some(task) = team.task_mut(1) {
        if let Some(attempt) = task.attempts.last_mut() {
            attempt.state = AttemptState::Unconfirmed;
            attempt.error = "the session crashed at 1000; restarted".into();
        }
    }
    if let Some(member) = team.members.iter_mut().find(|member| member.id == "dev") {
        member.crash = Some(1_000.0);
    }
}

fn store_with(team: &Team) -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams.sqlite3"), true).unwrap();
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), team.clone(), vec![]).unwrap();
    (dir, store)
}

fn fake_send(told: Rc<RefCell<Vec<String>>>) {
    set_fake_hgs(Some(Box::new(move |_, args, input| {
        if args == ["--help"] {
            return Ok(fake_output(0, "", ""));
        }
        if let Some(input) = input {
            told.borrow_mut().push(crate::state::teams::doorbell::posted_text(input));
        }
        Ok(fake_output(0, r#"{"status":"submitted"}"#, ""))
    })));
}

fn idle_seen() -> Vec<Seen> {
    vec![session("claude/x/lead", "r0", false), session("codex/x/dev", "r1", true)]
}

fn busy_seen() -> Vec<Seen> {
    let mut dev = session("codex/x/dev", "r1", false);
    dev.activity = "busy".into();
    dev.turn_started = Some(1_050.0);
    vec![session("claude/x/lead", "r0", false), dev]
}

#[test]
fn an_idle_restarted_session_gets_the_same_attempt_back() {
    let mut team = team();
    delivered(&mut team);
    crashed(&mut team);
    let found = steps(&team, &idle_seen(), 1_100.0);
    assert!(found.contains(&Step::RecoverCrash { task: 1, member: "dev".into(), busy: false }), "{found:?}");
    let (_dir, mut store) = store_with(&team);
    let told = Rc::new(RefCell::new(Vec::new()));
    fake_send(told.clone());
    let step = found.iter().find(|step| matches!(step, Step::RecoverCrash { .. })).unwrap().clone();
    run_step(&mut store, &team, &step, "").unwrap();
    set_fake_hgs(None);
    let saved = store.load(&team.id).unwrap().unwrap();
    let attempt = saved.task(1).unwrap().attempts.last().unwrap();
    assert_eq!((attempt.n, attempt.state), (1, AttemptState::Delivered), "the attempt keeps its number");
    assert!(attempt.error.is_empty());
    let sent = told.borrow().join("\n");
    assert!(sent.contains("crashed at") && sent.contains("Do not start over"), "{sent}");
    assert!(sent.contains("attempt: 1"), "the hand-out names the same attempt: {sent}");
    // The lead reads one item, with no action needed.
    let items = super::inbox::items(&saved);
    let item = items.iter().find(|item| item.kind == "attempt_recovered").expect("the lead's item");
    assert!(item.summary.contains("#1 attempt 1 sent again"), "{}", item.summary);
    // A report under that attempt is still the one the lead decides.
    let mut reported = saved.clone();
    let mut events = Vec::new();
    ops::apply(&mut reported, "report", &json!({"task":1,"attempt":ATTEMPT,"text":"Done"}), &ops::Actor::Member("dev".into()), &BTreeSet::new(), &mut events)
        .unwrap();
    assert!(reported.task(1).unwrap().attempts.last().unwrap().report.is_some(), "the report is kept");
}

#[test]
fn a_busy_restarted_session_keeps_the_attempt_without_a_second_hand_out() {
    let mut team = team();
    delivered(&mut team);
    crashed(&mut team);
    let found = steps(&team, &busy_seen(), 1_100.0);
    assert!(found.contains(&Step::RecoverCrash { task: 1, member: "dev".into(), busy: true }), "{found:?}");
    let (_dir, mut store) = store_with(&team);
    let told = Rc::new(RefCell::new(Vec::new()));
    fake_send(told.clone());
    let step = found.iter().find(|step| matches!(step, Step::RecoverCrash { .. })).unwrap().clone();
    run_step(&mut store, &team, &step, "").unwrap();
    set_fake_hgs(None);
    let saved = store.load(&team.id).unwrap().unwrap();
    let attempt = saved.task(1).unwrap().attempts.last().unwrap();
    assert_eq!((attempt.n, attempt.state), (1, AttemptState::Delivered));
    assert!(attempt.started_at.is_some(), "bound to the run that works on it");
    assert!(told.borrow().is_empty(), "nothing is sent again: {:?}", told.borrow());
    let items = super::inbox::items(&saved);
    let item = items.iter().find(|item| item.kind == "attempt_recovered").expect("the lead's item");
    assert!(item.summary.contains("#1 attempt 1 continued on its own"), "{}", item.summary);
}

#[test]
fn a_retry_while_the_member_works_continues_and_never_revokes() {
    let mut team = team();
    delivered(&mut team);
    let mut events = Vec::new();
    let lead = ops::Actor::Member("lead".into());
    let answer = ops::apply(&mut team, "retry", &json!({"task":1}), &lead, &BTreeSet::new(), &mut events).unwrap();
    assert_eq!(answer["status"], "continue", "{answer}");
    let attempt = team.task(1).unwrap().attempts.last().unwrap();
    assert_eq!((attempt.n, attempt.state), (1, AttemptState::Delivered), "nothing was revoked");
    assert_eq!(team.task(1).unwrap().status, TaskStatus::InProgress);
    assert_eq!(team.continues.len(), 1);
    assert_eq!(team.continues[0].attempt, 1);
    // The notice is a team message, sent at the member's safe point.
    let found = steps(&team, &idle_seen(), 1_100.0);
    assert!(found.contains(&Step::Continues), "{found:?}");
    let (_dir, mut store) = store_with(&team);
    let told = Rc::new(RefCell::new(Vec::new()));
    fake_send(told.clone());
    run_step(&mut store, &team, &Step::Continues, "").unwrap();
    set_fake_hgs(None);
    let sent = told.borrow().join("\n");
    assert!(sent.contains("Continue task #1 (attempt 1)") && sent.contains("nothing was revoked"), "{sent}");
    let saved = store.load(&team.id).unwrap().unwrap();
    assert_eq!(saved.continues[0].notice, Notice::Sent);
    // A crashed attempt the recovery has not reached yet is not revoked either.
    let mut left = team.clone();
    {
        let attempt = left.task_mut(1).unwrap().attempts.last_mut().unwrap();
        attempt.state = AttemptState::Unconfirmed;
        attempt.error = "the session crashed at 1000; restarted".into();
    }
    let mut events = Vec::new();
    let answer = ops::apply(&mut left, "retry", &json!({"task":1}), &lead, &BTreeSet::new(), &mut events).unwrap();
    assert_eq!(answer["status"], "continue", "a crashed attempt waits for the recovery, not a revoke: {answer}");
    // "fresh": true still starts the task over, as before.
    let mut fresh = team.clone();
    let mut events = Vec::new();
    let answer = ops::apply(&mut fresh, "retry", &json!({"task":1,"fresh":true}), &lead, &BTreeSet::new(), &mut events).unwrap();
    assert_eq!(answer["status"], "pending", "{answer}");
    assert_eq!(fresh.task(1).unwrap().attempts.last().unwrap().state, AttemptState::Revoked);
}

#[test]
fn a_run_busy_on_another_task_keeps_the_crashed_attempt_for_the_lead() {
    let repo = git_repo("team/x/hgs-2");
    let mut team = team();
    team.folder = repo.path().display().to_string();
    team.members.iter_mut().find(|member| member.id == "dev").unwrap().state = MemberState::Active;
    delivered(&mut team);
    crashed(&mut team);
    let found = steps(&team, &busy_seen(), 1_100.0);
    assert!(
        !found.iter().any(|step| matches!(step, Step::RecoverCrash { .. })),
        "the branch shows task #2, so #1 is not handed to that run: {found:?}"
    );
}

#[test]
fn the_worker_start_sweep_reads_the_listing_and_gives_the_attempt_back() {
    let mut team = team();
    delivered(&mut team);
    crashed(&mut team);
    let (_dir, mut store) = store_with(&team);
    let told = Rc::new(RefCell::new(Vec::new()));
    let kept = told.clone();
    set_fake_hgs(Some(Box::new(move |_, args, input| {
        if args.first() == Some(&"ls") {
            let listing = json!([{"sessions":[
                {"name":"claude/x/lead","run_id":"r0","state":"running","activity":"idle","phase":"idle"},
                {"name":"codex/x/dev","run_id":"r1","state":"running","activity":"idle","phase":"idle"}]}]);
            return Ok(fake_output(0, &listing.to_string(), ""));
        }
        if let Some(input) = input {
            kept.borrow_mut().push(crate::state::teams::doorbell::posted_text(input));
        }
        Ok(fake_output(0, r#"{"status":"submitted"}"#, ""))
    })));
    let done = super::sweep_crashed(&mut store).unwrap();
    set_fake_hgs(None);
    assert_eq!(done.len(), 1, "{done:?}");
    let saved = store.load(&team.id).unwrap().unwrap();
    let attempt = saved.task(1).unwrap().attempts.last().unwrap();
    assert_eq!((attempt.n, attempt.state), (1, AttemptState::Delivered));
    assert!(told.borrow().iter().any(|text| text.contains("crashed at")), "{:?}", told.borrow());
}

#[test]
fn the_sweep_after_an_update_recovers_every_crashed_attempt_and_says_so_once() {
    let mut team = team();
    delivered(&mut team);
    crashed(&mut team);
    let task = team.task_mut(2).unwrap();
    task.attempts.push(Attempt {
        n: 1,
        id: "00000000-0000-4000-8000-000000000002".into(),
        member: "dev".into(),
        state: AttemptState::Unconfirmed,
        sent_at: 10.0,
        error: "the session crashed at 1000; restarted".into(),
        ..Default::default()
    });
    task.status = TaskStatus::InProgress;
    let (_dir, mut store) = store_with(&team);
    let told = Rc::new(RefCell::new(Vec::new()));
    fake_send(told.clone());
    let done = super::recover_crashed(&mut store, &idle_seen(), 1_100.0).unwrap();
    set_fake_hgs(None);
    assert_eq!(done.len(), 2, "{done:?}");
    let saved = store.load(&team.id).unwrap().unwrap();
    for id in [1, 2] {
        let attempt = saved.task(id).unwrap().attempts.last().unwrap();
        assert_eq!((attempt.n, attempt.state), (1, AttemptState::Delivered), "task #{id}");
    }
    // One item, naming both, and nothing for the lead to do.
    let items = super::inbox::items(&saved);
    let recovered: Vec<_> = items.iter().filter(|item| item.kind == "attempt_recovered").collect();
    assert_eq!(recovered.len(), 1, "{items:?}");
    assert!(recovered[0].summary.contains("#1") && recovered[0].summary.contains("#2"), "{}", recovered[0].summary);
    assert!(recovered[0].command.starts_with("no action"), "{}", recovered[0].command);
    // Nothing waits for the lead as unconfirmed any more.
    assert!(!saved.task(1).unwrap().attempts.iter().any(|attempt| attempt.state == AttemptState::Unconfirmed));
}

#[test]
fn the_unassigned_item_names_the_task_its_branch_shows_and_one_retry_binds_it() {
    let repo = git_repo("team/x/hgs-1");
    let mut team = team();
    team.folder = repo.path().display().to_string();
    let dev = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
    dev.state = MemberState::Active;
    let seen = busy_seen();
    let found = steps(&team, &seen, 1_200.0);
    let step = found
        .iter()
        .find(|step| matches!(step, Step::Unassigned { .. }))
        .expect("unassigned work")
        .clone();
    assert_eq!(step, Step::Unassigned { member: "dev".into(), what: "works (busy) since 3 min".into(), task: Some(1) }, "{found:?}");
    let (_dir, mut store) = store_with(&team);
    run_step(&mut store, &team, &step, "").unwrap();
    let saved = store.load(&team.id).unwrap().unwrap();
    let flag = saved.member("dev").unwrap().unassigned.clone().expect("flagged");
    assert_eq!(flag.task, Some(1));
    let items = super::inbox::items(&saved);
    let item = items.iter().find(|item| item.kind == "unassigned_work").expect("the lead's item");
    assert_eq!(item.task, Some(1));
    assert!(item.summary.contains("#1 API"), "{}", item.summary);
    assert!(item.command.contains("\"task\":1") && item.command.contains("retry"), "{}", item.command);
    // One retry binds the work in progress: the attempt exists, nothing is sent
    // again, and the member is told to report under it.
    let mut events = Vec::new();
    let mut bound = saved.clone();
    let answer = ops::apply(&mut bound, "retry", &json!({"task":1}), &ops::Actor::Member("lead".into()), &BTreeSet::new(), &mut events).unwrap();
    assert_eq!(answer["status"], "continue", "{answer}");
    let attempt = bound.task(1).unwrap().attempts.last().unwrap();
    assert_eq!((attempt.n, attempt.state), (1, AttemptState::Delivered), "the work is bound, not restarted");
    assert_eq!(bound.continues.len(), 1);
}

/// A folder with `branch` checked out, one commit deep.
fn git_repo(branch: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(["-c", "user.email=t@example.com", "-c", "user.name=T"])
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    };
    git(&["init", "-q"]);
    std::fs::write(dir.path().join("a.txt"), "a").unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "A"]);
    git(&["checkout", "-q", "-b", branch]);
    dir
}

