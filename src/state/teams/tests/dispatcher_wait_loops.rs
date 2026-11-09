//! Task 161: waiting is always a park. The lead's wait instruction is
//! refused with the park, a blocked member's question is answered with one, a
//! turn that ends waiting is offered one, and an attempt found waiting (its
//! own wait job, a repeated read-only check) is parked by Zerus with the lead
//! told once.
use super::*;
use super::support::*;
use crate::state::teams::ops::{self, Actor};

const ATTEMPT: &str = "00000000-0000-4000-8000-000000000001";

fn running() -> Team {
    let mut team = team();
    team.status = TeamStatus::Running;
    delivered(&mut team);
    team
}

fn busy() -> Vec<Seen> {
    let mut seen = vec![session("claude/x/lead", "r0", true), session("codex/x/dev", "r1", false)];
    seen[1].idle = false;
    seen
}

fn asked(team: &mut Team, text: &str) {
    team.task_mut(1).unwrap().attempts[0].questions.push(Question { id: "q1".into(), text: text.into(), at: 40.0, ..Default::default() });
}

/// Runs one step with a fake hgs: `inspect` answers `events`, every `send`
/// is kept as (session, text).
fn run_with(store: &mut Store, team: &Team, step: &Step, events: Value) -> Vec<(String, String)> {
    let sent = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(String, String)>::new()));
    let keep = sent.clone();
    set_fake_hgs(Some(Box::new(move |_, args, input| {
        if args[0] == "inspect" {
            return Ok(fake_output(0, &json!({"events":events}).to_string(), ""));
        }
        if args[0] == "send" {
            keep.lock().unwrap().push((args[1].to_string(), input.map(crate::state::teams::doorbell::posted_text).unwrap_or_default()));
        }
        Ok(fake_output(0, r#"{"status":"submitted"}"#, ""))
    })));
    let result = run_step(store, team, step, "");
    set_fake_hgs(None);
    result.unwrap();
    let out = sent.lock().unwrap().clone();
    out
}

fn stored(team: &Team) -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams.sqlite3"), true).unwrap();
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), team.clone(), vec![]).unwrap();
    (dir, store)
}

#[test]
fn the_lead_cannot_answer_a_member_with_a_wait_and_parks_instead() {
    let mut team = running();
    asked(&mut team, "Blocked: the fix needs hgs 1.58.0 installed before I can test it. Waiting for the install?");
    let lead = Actor::Member("lead".into());
    let error = ops::apply(&mut team.clone(), "reply", &json!({"question":"q1","text":"Keep 138 and wait for the install, check every 2 minutes"}), &lead, &BTreeSet::new(), &mut Vec::new())
        .unwrap_err()
        .to_string();
    assert!(error.starts_with("Waiting is a park, never an instruction"), "{error}");
    assert!(error.contains("\"task\":1") && error.contains(ATTEMPT) && error.contains("hgs team park --json"), "{error}");
    // An ordinary answer passes.
    ops::apply(&mut team.clone(), "reply", &json!({"question":"q1","text":"Test against the source build instead"}), &lead, &BTreeSet::new(), &mut Vec::new()).unwrap();
    // The card offers the park as the answer, with a guessed condition and an ETA.
    let item = super::super::inbox::items(&team).into_iter().find(|item| item.kind == "question").unwrap();
    assert!(item.detail.starts_with("Blocked on an outside event? Answer with a park until hgs 1.58.0 installed, ETA"), "{}", item.detail);
    assert!(item.detail.contains(r#""park":{"condition":{"kind":"build","version":"1.58.0"},"eta_minutes":60"#), "{}", item.detail);
    // That answer closes the question and parks the task at once.
    let mut events = Vec::new();
    let answer = ops::apply(&mut team, "reply", &json!({"question":"q1","text":"","park":{"condition":{"kind":"build","version":"1.58.0"},"eta_minutes":60}}),
        &lead, &BTreeSet::new(), &mut events).unwrap();
    assert_eq!(answer["status"], "answered");
    assert!(answer["parked"].as_str().unwrap().starts_with("parked until hgs version 1.58.0 installed"), "{answer}");
    let task = team.task(1).unwrap();
    assert_eq!(task.attempts[0].state, AttemptState::Parked);
    assert!(!task.attempts[0].questions[0].open());
    assert!(events.iter().any(|event| event.kind == "parked"));
}

#[test]
fn a_member_repeating_a_check_is_parked_and_the_lead_told_once() {
    let team = running();
    assert!(!steps(&team, &busy(), 500.0).iter().any(|step| matches!(step, Step::DetectWait { .. })), "not before ten minutes");
    assert!(steps(&team, &busy(), 700.0).contains(&Step::DetectWait { task: 1 }));
    let free = vec![session("claude/x/lead", "r0", true), session("codex/x/dev", "r1", true)];
    assert!(!steps(&team, &free, 700.0).iter().any(|step| matches!(step, Step::DetectWait { .. })), "an idle session loops nothing");
    let (_dir, mut store) = stored(&team);
    let events = json!([
        {"type":"PreToolUse","tool":"Bash","detail":"hgs --version","at":"100"},
        {"type":"PreToolUse","tool":"Bash","detail":"hgs --version","at":"220"},
        {"type":"PreToolUse","tool":"Bash","detail":"hgs --version","at":"345"},
    ]);
    let sent = run_with(&mut store, &team, &Step::DetectWait { task: 1 }, events);
    let saved = store.load(&team.id).unwrap().unwrap();
    let park = saved.task(1).unwrap().park.clone().expect("parked");
    assert_eq!((park.by.as_str(), park.member.as_str(), park.attempt.as_str()), ("Zerus (a wait loop)", "dev", ATTEMPT));
    assert!(park.note.contains("it ran `hgs --version` 3 times at intervals"), "{}", park.note);
    let member: Vec<&String> = sent.iter().filter(|(to, _)| to == "codex/x/dev").map(|(_, text)| text).collect();
    assert!(member.len() == 1 && member[0].contains("Zerus parked it until") && member[0].contains("Stop the wait now"), "{sent:?}");
    let lead: Vec<&String> = sent.iter().filter(|(to, _)| to == "claude/x/lead").map(|(_, text)| text).collect();
    assert!(lead.len() == 1 && lead[0].contains("was a wait loop") && lead[0].contains("a guess, ETA 60 min"), "{sent:?}");
    // Parked, the task is looked at no more; the member is free.
    assert!(!steps(&saved, &busy(), 2000.0).iter().any(|step| matches!(step, Step::DetectWait { .. })));
}

/// Task #413: a repeated check is a wait loop only while the state it reads
/// stands. A first observation never parks; a fast-forward that moves HEAD
/// between checks is work; an unchanged HEAD is the loop.
#[test]
fn a_fast_forward_that_moves_head_is_never_parked() {
    let repo = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        let output = Command::new("git").args(["-c", "user.name=t", "-c", "user.email=t@example.test"]).args(args).current_dir(repo.path()).output().unwrap();
        assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    };
    git(&["init", "-q", "-b", "main"]);
    std::fs::write(repo.path().join("a.txt"), "one").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-qm", "one"]);
    let mut team = running();
    {
        let dev = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
        dev.directory = repo.path().display().to_string();
    }
    let events = json!([
        {"type":"PreToolUse","tool":"Bash","detail":"git log -1 --format=%H","at":"100"},
        {"type":"PreToolUse","tool":"Bash","detail":"git log -1 --format=%H","at":"220"},
        {"type":"PreToolUse","tool":"Bash","detail":"git log -1 --format=%H","at":"345"},
    ]);
    let (_dir, mut store) = stored(&team);
    // The first look records the state and never parks.
    run_with(&mut store, &team, &Step::DetectWait { task: 1 }, events.clone());
    let saved = store.load(&team.id).unwrap().unwrap();
    assert!(saved.task(1).unwrap().park.is_none(), "a first observation is not a loop");
    let first = saved.task(1).unwrap().attempts[0].wait_head.clone();
    assert!(!first.is_empty(), "the folder's HEAD is recorded");
    // The next gate fast-forwards the line: the same command, a moved HEAD.
    std::fs::write(repo.path().join("b.txt"), "two").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-qm", "two"]);
    let moved = saved.clone();
    run_with(&mut store, &moved, &Step::DetectWait { task: 1 }, events.clone());
    let saved = store.load(&team.id).unwrap().unwrap();
    assert!(saved.task(1).unwrap().park.is_none(), "a moved HEAD is work, not a wait");
    let second = saved.task(1).unwrap().attempts[0].wait_head.clone();
    assert_ne!(first, second, "the new state is recorded instead");
    // Once the state stands, the same repeats are the loop.
    run_with(&mut store, &saved, &Step::DetectWait { task: 1 }, events);
    let saved = store.load(&team.id).unwrap().unwrap();
    let park = saved.task(1).unwrap().park.clone().expect("an unchanged HEAD parks");
    assert!(park.note.contains("with HEAD") && park.note.contains("unchanged"), "{}", park.note);
}

#[test]
fn a_member_with_a_background_job_is_never_parked_on_a_repeated_command() {
    let mut team = running();
    team.members.iter_mut().find(|member| member.id == "dev").unwrap().jobs =
        vec![crate::state::teams::model::MemberJob { what: "the gate monitor".into(), since: 1.0, ..Default::default() }];
    let (_dir, mut store) = stored(&team);
    let events = json!([
        {"type":"PreToolUse","tool":"Bash","detail":"hgs --version","at":"100"},
        {"type":"PreToolUse","tool":"Bash","detail":"hgs --version","at":"220"},
        {"type":"PreToolUse","tool":"Bash","detail":"hgs --version","at":"345"},
    ]);
    run_with(&mut store, &team, &Step::DetectWait { task: 1 }, events);
    let saved = store.load(&team.id).unwrap().unwrap();
    assert!(saved.task(1).unwrap().park.is_none(), "its own background job is the wait");
}

#[test]
fn an_attempt_doing_real_work_is_left_alone_and_looked_at_again_later() {
    let team = running();
    let (_dir, mut store) = stored(&team);
    let events = json!([
        {"type":"PreToolUse","tool":"Bash","detail":"cargo build","at":"100"},
        {"type":"PreToolUse","tool":"Bash","detail":"git status","at":"220"},
        {"type":"PreToolUse","tool":"Edit","detail":"src/lib.rs","at":"345"},
    ]);
    let sent = run_with(&mut store, &team, &Step::DetectWait { task: 1 }, events);
    assert!(sent.is_empty(), "{sent:?}");
    let saved = store.load(&team.id).unwrap().unwrap();
    assert!(saved.task(1).unwrap().park.is_none());
    let checked = saved.task(1).unwrap().attempts[0].wait_checked_at;
    assert!(checked > 0.0);
    assert!(!steps(&saved, &busy(), checked + 100.0).iter().any(|step| matches!(step, Step::DetectWait { .. })), "five minutes apart");
    assert!(steps(&saved, &busy(), checked + 301.0).contains(&Step::DetectWait { task: 1 }));
}

#[test]
fn a_session_building_only_for_its_own_wait_job_is_parked() {
    let mut team = running();
    // A session name of its own: the claims folder is shared by the tests.
    let id = uuid::Uuid::new_v4().to_string();
    let name = format!("codex/x/{id}");
    team.members.iter_mut().find(|member| member.id == "dev").unwrap().session.as_mut().unwrap().name = name.clone();
    let (_dir, mut store) = stored(&team);
    let claim = crate::state::builds::test_running_job(&id, "wait for 1.58.0 install", "hgs --version",
        json!({"name":name,"run_id":"r1"}), crate::state::now());
    let sent = run_with(&mut store, &team, &Step::DetectWait { task: 1 }, json!([]));
    let _ = std::fs::remove_file(claim);
    let saved = store.load(&team.id).unwrap().unwrap();
    let park = saved.task(1).unwrap().park.clone().expect("parked");
    assert!(park.note.contains("its run \"wait for 1.58.0 install\""), "{}", park.note);
    assert_eq!(serde_json::to_value(&park.condition).unwrap()["version"], "1.58.0", "the guess reads the job's own version");
    assert!(sent.iter().any(|(to, text)| to == "claude/x/lead" && text.contains("hgs 1.58.0 installed")), "{sent:?}");
}

#[test]
fn a_turn_that_ends_waiting_is_offered_the_park() {
    let mut team = running();
    {
        let attempt = &mut team.task_mut(1).unwrap().attempts[0];
        attempt.turn_ended_at = Some(50.0);
        attempt.turn_reply = "r7".into();
        attempt.report_ready_at = Some(50.0);
    }
    let (_dir, mut store) = stored(&team);
    inspect_override(Some(json!({"reply_id":"r7","reply_at":50.0,"last_message":"I'll wait for the CI run 4242 to finish before merging."})));
    let sent = run_with(&mut store, &team, &Step::NoReport { task: 1 }, json!([]));
    inspect_override(None);
    let saved = store.load(&team.id).unwrap().unwrap();
    let attempt = &saved.task(1).unwrap().attempts[0];
    assert!(attempt.no_report && attempt.report.is_none(), "never kept as a report");
    let offer = sent.iter().find(|(to, _)| to == "codex/x/dev").map(|(_, text)| text.clone()).expect("the park is offered");
    assert!(offer.contains("ended waiting") && offer.contains("hgs team park --json") && offer.contains(ATTEMPT), "{offer}");
    // A finished turn's ordinary reply is offered nothing.
    let mut plain = running();
    {
        let attempt = &mut plain.task_mut(1).unwrap().attempts[0];
        attempt.turn_ended_at = Some(50.0);
        attempt.turn_reply = "r8".into();
    }
    let (_dir2, mut store2) = stored(&plain);
    inspect_override(Some(json!({"reply_id":"r8","reply_at":50.0,"last_message":"Fixed the parser; tests pass."})));
    let sent = run_with(&mut store2, &plain, &Step::NoReport { task: 1 }, json!([]));
    inspect_override(None);
    assert!(sent.is_empty(), "{sent:?}");
}
