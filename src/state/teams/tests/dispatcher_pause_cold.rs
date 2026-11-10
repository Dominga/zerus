//! Task 254 (owner, 2026-11-08, on task 227): crash recovery and every
//! hand-out respect pauses and a cold cache. A paused team, phase or member, a
//! draining member or a held task gets no give-back until the pause ends; a
//! session whose prompt cache is cold and whose context is large is compacted
//! before it works; a warm or small one is delivered at once; a failed
//! compaction still delivers and tells the lead.
use super::support::*;
use super::*;
use std::cell::RefCell;
use std::rc::Rc;

/// The delivered attempt of #1 a crash left unconfirmed, as task 227 sees it.
fn crashed_team() -> Team {
    let mut team = team();
    delivered(&mut team);
    let attempt = team.task_mut(1).unwrap().attempts.last_mut().unwrap();
    attempt.state = AttemptState::Unconfirmed;
    attempt.error = "the session crashed at 1000; restarted".into();
    team.members.iter_mut().find(|member| member.id == "dev").unwrap().crash = Some(1_000.0);
    team
}

fn idle_seen() -> Vec<Seen> {
    vec![session("claude/x/lead", "r0", false), session("codex/x/dev", "r1", true)]
}

fn gives_back(team: &Team) -> bool {
    steps(team, &idle_seen(), 1_100.0).iter().any(|step| matches!(step, Step::RecoverCrash { task: 1, .. }))
}

fn dev(team: &mut Team) -> &mut Member {
    team.members.iter_mut().find(|member| member.id == "dev").unwrap()
}

/// A fresh context reading of `percent` of the window and the cache state.
fn reading(team: &mut Team, percent: u64, cold: bool) {
    let found = dev(team);
    found.context = Some(MemberContext::of(percent * 10_000, 1_000_000, crate::state::now()));
    found.seen_cold = cold;
}

#[test]
fn a_crashed_attempt_waits_out_every_pause_and_comes_back_after_it() {
    let base = crashed_team();
    assert!(gives_back(&base), "a running team gives it back");
    #[allow(clippy::type_complexity)]
    let paused: [(&str, Box<dyn Fn(&mut Team)>); 5] = [
        ("the team is not running", Box::new(|team: &mut Team| team.status = TeamStatus::Paused)),
        ("its phase is paused", Box::new(|team: &mut Team| {
            let phase = team.task(1).unwrap().phase;
            match team.phases.iter_mut().find(|found| found.n == phase) {
                Some(found) => found.pause = Some(PhasePause { by: "You".into(), owner: true, ..Default::default() }),
                None => team.phases.push(Phase { n: phase, pause: Some(PhasePause { by: "You".into(), owner: true, ..Default::default() }), ..Default::default() }),
            }
        })),
        ("the member is paused", Box::new(|team: &mut Team| dev(team).state = MemberState::Paused)),
        ("the member is draining", Box::new(|team: &mut Team| dev(team).draining = true)),
        ("the task is held", Box::new(|team: &mut Team| team.task_mut(1).unwrap().hold = Some(Hold { by: "You".into(), at: 1.0, reason: String::new() }))),
    ];
    for (why, pause) in paused {
        let mut team = base.clone();
        pause(&mut team);
        let task = team.task(1).unwrap().clone();
        let member = team.member("dev").unwrap().clone();
        assert_eq!(crash_held(&team, &task, &member), Some(why));
        assert!(!gives_back(&team), "{why}: nothing goes back");
        // The sweep after an update keeps it too.
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open_at(&dir.path().join("teams.sqlite3"), true).unwrap();
        stored(&mut store, &team);
        let told = Rc::new(RefCell::new(0));
        let count = told.clone();
        set_fake_hgs(Some(Box::new(move |_, _, _| {
            *count.borrow_mut() += 1;
            Ok(fake_output(0, r#"{"status":"submitted"}"#, ""))
        })));
        let done = recover_crashed(&mut store, &idle_seen(), 1_100.0).unwrap();
        set_fake_hgs(None);
        assert!(done.is_empty() && *told.borrow() == 0, "{why}: the sweep sends nothing: {done:?}");
        let saved = store.load(&team.id).unwrap().unwrap();
        assert_eq!(saved.task(1).unwrap().attempts.last().unwrap().state, AttemptState::Unconfirmed, "{why}: kept for later");
    }
    // The pause ends: the next tick gives it back.
    let mut team = base.clone();
    dev(&mut team).state = MemberState::Paused;
    assert!(!gives_back(&team));
    dev(&mut team).state = MemberState::Active;
    assert!(gives_back(&team), "resumed after the pause");
}

#[test]
fn a_cold_large_context_is_compacted_before_the_crashed_attempt_goes_back() {
    let mut team = crashed_team();
    reading(&mut team, 40, true);
    let found = steps(&team, &idle_seen(), 1_100.0);
    assert!(found.contains(&Step::CompactCold { member: "dev".into() }), "{found:?}");
    assert!(!found.iter().any(|step| matches!(step, Step::RecoverCrash { .. })), "the give-back waits for the compaction");
    // While it compacts, nothing is given back; afterwards it is.
    dev(&mut team).context_compact = Some(MemberCompact { action: "compact".into(), reason: "cold".into(), ..Default::default() });
    assert!(!gives_back(&team));
    dev(&mut team).context_compact = None;
    dev(&mut team).context = None;
    assert!(gives_back(&team), "after the compaction the attempt goes back");
    // A warm cache or a small context goes back at once.
    let mut warm = crashed_team();
    reading(&mut warm, 40, false);
    assert!(gives_back(&warm));
    let mut small = crashed_team();
    reading(&mut small, 10, true);
    assert!(gives_back(&small));
    // 0 turns the rule off.
    let mut off = crashed_team();
    reading(&mut off, 90, true);
    off.settings.cold_compact_percent = 0;
    assert!(gives_back(&off));
}

/// A hand-out of #1 to dev with the fake hgs answering `compact` with `exit`.
fn hand_out_with(team: &Team, compact_exit: i32) -> (Vec<String>, Team, Vec<String>) {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        stored(&mut store, team);
        let calls = Rc::new(RefCell::new(Vec::<String>::new()));
        let record = calls.clone();
        set_fake_hgs(Some(Box::new(move |_, args: &[&str], input: Option<&Value>| {
            let call = args.first().copied().unwrap_or_default().to_owned();
            record.borrow_mut().push(call.clone());
            match call.as_str() {
                "compact-context" if compact_exit != 0 => Ok(fake_output(compact_exit, "", "the session refused the compact")),
                "compact-context" => {
                    let id = input.unwrap()["request_id"].as_str().unwrap().to_owned();
                    Ok(fake_output(0, &json!({"request_id":id,"status":"submitted"}).to_string(), ""))
                }
                _ => Ok(fake_output(0, r#"{"status":"submitted"}"#, "")),
            }
        })));
        hand_out(&mut store, &team.id, 1, "dev", None, "").unwrap();
        set_fake_hgs(None);
        let saved = store.load(&team.id).unwrap().unwrap();
        let kinds = event_kinds(&store, &team.id);
        let calls = calls.borrow().clone();
        (calls, saved, kinds)
    })
}

fn ready_team(percent: u64, cold: bool) -> Team {
    let mut team = team();
    with_context(&mut team, "dev", "");
    reading(&mut team, percent, cold);
    team
}

#[test]
fn a_cold_session_with_a_large_context_is_compacted_before_its_hand_out() {
    let (calls, saved, _) = hand_out_with(&ready_team(40, true), 0);
    assert_eq!(calls, ["compact-context"], "compacted first, nothing sent yet");
    let wait = saved.task(1).unwrap().waits.as_ref().expect("the hand-out waits for the compaction");
    assert_eq!((wait.state.as_str(), wait.action.as_str(), wait.reason.as_str()), ("waiting", "compact", COLD_REASON));
    assert!(saved.task(1).unwrap().attempts.is_empty());
}

#[test]
fn a_warm_session_or_a_small_context_is_delivered_at_once() {
    for (percent, cold) in [(40, false), (10, true), (0, false)] {
        let (calls, saved, _) = hand_out_with(&ready_team(percent, cold), 0);
        assert!(!calls.iter().any(|call| call == "compact-context"), "{percent}% cold={cold}: {calls:?}");
        assert_eq!(saved.task(1).unwrap().attempts.last().map(|attempt| attempt.state), Some(AttemptState::Delivered), "{percent}% cold={cold}");
    }
}

#[test]
fn a_failed_cold_compaction_still_delivers_and_tells_the_lead() {
    let (calls, saved, kinds) = hand_out_with(&ready_team(40, true), 1);
    assert_eq!(calls.first().map(String::as_str), Some("compact-context"));
    assert!(calls.iter().filter(|call| *call == "send").count() >= 1, "the task is sent: {calls:?}");
    let task = saved.task(1).unwrap();
    assert_eq!(task.attempts.last().map(|attempt| attempt.state), Some(AttemptState::Delivered), "delivered anyway");
    assert!(task.waits.is_none(), "no wait is left behind");
    assert!(kinds.iter().any(|kind| kind == "context_compact_failed"), "{kinds:?}");
    // The threshold rule (task 209) keeps its own failure path: the task waits.
    let mut full = ready_team(97, false);
    full.settings.cold_compact_percent = 30;
    let (_, saved, _) = hand_out_with(&full, 1);
    let task = saved.task(1).unwrap();
    assert!(
        task.attempts.last().is_none_or(|attempt| attempt.state != AttemptState::Delivered),
        "a failed threshold compaction is not delivered: {:?}",
        task.attempts.last()
    );
    assert!(task.waits.is_some(), "it waits for the lead");
}

#[test]
fn the_listing_tells_a_cold_cache() {
    assert!(cache_cold(&json!({"session_state":"cold"}), 10.0));
    assert!(!cache_cold(&json!({"session_state":"cooling"}), 10.0));
    assert!(!cache_cold(&json!({"session_state":"working","last_active":0,"cache_ttl":{"seconds":300}}), 1_000.0), "the state wins");
    // A listing without session_state: the TTL since the last activity.
    assert!(cache_cold(&json!({"last_active":100.0,"cache_ttl":{"seconds":300}}), 500.0));
    assert!(!cache_cold(&json!({"last_active":100.0,"cache_ttl":{"seconds":300}}), 300.0));
    assert!(!cache_cold(&json!({}), 500.0), "nothing known is not cold");
    let team = team();
    assert_eq!(team.settings.cold_compact_percent, 30, "30% by default");
}
