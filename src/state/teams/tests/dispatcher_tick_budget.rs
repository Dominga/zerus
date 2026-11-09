//! Task #406 (owner, 2026-11-09): the team worker never stalls. Hand-outs and
//! answers run first in every tick, heavy phases wait for a tick with time
//! left (never starved), a stop ends the worker within its grace, and a phase
//! that holds the worker while tasks are ready is one alarm for the lead.
use super::super::ops::{self, Actor};
use super::*;
use super::support::*;

#[test]
fn hand_outs_and_answers_run_before_every_other_step() {
    let steps = vec![
        Step::Inbox,
        Step::ScanReport { task: 7 },
        Step::HandOut { task: 1, member: "dev".into() },
        Step::SpecDecisions,
        Step::TellDecision { task: 2, decision: "accepted".into() },
        Step::RingMessages { member: "qa".into() },
    ];
    let urgent = pass_steps(steps.clone(), Pass::Urgent);
    assert_eq!(urgent, vec![
        Step::HandOut { task: 1, member: "dev".into() },
        Step::TellDecision { task: 2, decision: "accepted".into() },
        Step::RingMessages { member: "qa".into() },
    ]);
    let rest = pass_steps(steps, Pass::Rest);
    assert_eq!(rest, vec![Step::Inbox, Step::ScanReport { task: 7 }, Step::SpecDecisions]);
}

#[test]
fn a_heavy_phase_waits_for_a_tick_with_time_left_but_is_never_starved() {
    // Outside a tick the elapsed time is zero: the phase runs.
    assert!(within_budget("outbox", "budget-team", Duration::from_secs(4)));
    // In a tick past its budget it is skipped, at most MAX_SKIPS ticks in a row.
    super::super::tick_clock::begin();
    std::thread::sleep(Duration::from_millis(20));
    let budget = Duration::from_millis(1);
    let runs: Vec<bool> = (0..=MAX_SKIPS).map(|_| within_budget("merge", "busy-team", budget)).collect();
    assert_eq!(runs.iter().filter(|ran| !**ran).count(), MAX_SKIPS as usize, "{runs:?}");
    assert_eq!(runs.last(), Some(&true), "the sixth tick runs it anyway");
    // Another team's phase keeps its own count.
    assert!(!within_budget("merge", "other-team", budget));
    super::super::tick_clock::finish();
}

#[test]
fn a_stop_request_ends_the_worker_after_its_grace() {
    let now = Instant::now();
    assert!(!stop_overdue(None, now));
    assert!(!stop_overdue(Some(now), now + Duration::from_secs(1)));
    assert!(stop_overdue(Some(now), now + STOP_GRACE));
    // The grace and the watchdog's poll stay within the owner's 5 s.
    assert!(STOP_GRACE + Duration::from_millis(200) <= Duration::from_secs(5));
}

#[test]
fn a_worker_held_for_five_minutes_while_tasks_are_ready_is_one_alarm() {
    let dir = tempfile::tempdir().unwrap();
    super::super::tick_clock::set_test_dir(Some(dir.path().to_path_buf()));
    let team = team();
    assert!(!super::super::plan::ready(&team).is_empty());
    // A tick that has run report git for six minutes.
    super::super::tick_clock::begin();
    super::super::tick_clock::phase("outbox: Checkout", || {
        let now = crate::state::now() + 360.0;
        let item = super::super::inbox::worker_stall(&team, now).expect("the alarm");
        assert_eq!((item.kind, item.key.as_str(), item.member.as_str()), ("stalled", "worker_stall", "lead"));
        assert!(item.summary.contains("'outbox: Checkout'") && item.summary.contains("6 min"), "{}", item.summary);
        assert!(item.command.contains("hgs team worker --stop"));
        // Under five minutes nothing is raised.
        assert!(super::super::inbox::worker_stall(&team, crate::state::now() + 60.0).is_none());
    });
    super::super::tick_clock::finish();
    // A finished tick holds nothing.
    assert!(super::super::inbox::worker_stall(&team, crate::state::now() + 3600.0).is_none());
    super::super::tick_clock::set_test_dir(None);
}

/// Task #406: a team of 200 tasks (a plan holds at most 200) and 6 members: deciding a tick's steps
/// stays far under the 2 s a tick may take, and the very first tick hands a
/// ready task to every idle member.
#[test]
fn a_large_team_decides_its_tick_quickly_and_hands_out_on_the_first_tick() {
    let (mut team, _) = ops::create(
        "large",
        &json!({"name":"Large","lead":{"id":"lead","agent":"claude","session":{"name":"claude/x/lead","run_id":"r0"}}}),
    )
    .unwrap();
    let members: Vec<Value> = (0..6)
        .map(|n| json!({"id":format!("m{n}"),"role":format!("Role {n}"),"agent":"codex","session":{"name":format!("codex/x/m{n}"),"run_id":format!("r{}", n + 1)}}))
        .collect();
    let tasks: Vec<Value> = (1..=200u32)
        .map(|id| {
            let deps = if id > 6 { json!([{"task":id - 6,"kind":"hard"}]) } else { json!([]) };
            json!({"id":id,"title":format!("Task {id}"),"assignee":format!("m{}", (id - 1) % 6),"deps":deps})
        })
        .collect();
    let mut events = Vec::new();
    ops::apply(&mut team, "plan", &json!({"members":members,"tasks":tasks}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    ops::apply(&mut team, "approve", &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    let mut seen = vec![session("claude/x/lead", "r0", false)];
    seen.extend((0..6).map(|n| session(&format!("codex/x/m{n}"), &format!("r{}", n + 1), true)));
    let started = Instant::now();
    let found = steps(&team, &seen, 10.0);
    let decided = started.elapsed();
    assert!(decided < Duration::from_secs(2), "deciding took {decided:?}");
    let urgent = pass_steps(found, Pass::Urgent);
    let handed: BTreeSet<String> = urgent
        .iter()
        .filter_map(|step| match step {
            Step::HandOut { member, .. } => Some(member.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(handed.len(), 6, "every idle member gets a ready task on the first tick: {urgent:?}");
}
