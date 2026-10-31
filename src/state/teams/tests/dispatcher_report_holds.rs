use super::*;

#[test]
fn a_captured_report_holds_its_member_from_new_work() {
    let (mut team, seen) = team_with_two_backends();
    // dev's acknowledgement on #1 was captured as its report: #2, also
    // dev's, goes to the free dev2 and never to dev.
    submit(&mut team, 1, "dev", true);
    assert_eq!(team.captured_hold("dev"), Some(1));
    let now = steps(&team, &seen, 1.0);
    assert!(now.contains(&Step::Rebalance { task: 2, from: "dev".into(), to: "dev2".into() }), "{now:?}");
    assert!(!now.iter().any(|s| matches!(s, Step::HandOut { member, .. } if member == "dev")), "{now:?}");
    // A pinned task waits for its held member.
    team.task_mut(2).unwrap().pinned = true;
    let now = steps(&team, &seen, 1.0);
    assert!(!now.iter().any(|s| matches!(s, Step::HandOut { task: 2, .. } | Step::Rebalance { task: 2, .. })), "{now:?}");
    // An explicit report keeps today's behaviour: dev takes #2.
    team.task_mut(1).unwrap().attempts.last_mut().unwrap().report.as_mut().unwrap().captured = false;
    assert_eq!(team.captured_hold("dev"), None);
    assert!(steps(&team, &seen, 1.0).contains(&Step::HandOut { task: 2, member: "dev".into() }));
}

#[test]
fn a_held_member_is_no_rebalance_target() {
    let (mut team, mut seen) = team_with_two_backends();
    // dev2's report on #1 was captured; dev is in a turn of its own.
    submit(&mut team, 1, "dev2", true);
    seen[1].free = false;
    let now = steps(&team, &seen, 1.0);
    assert!(!now.iter().any(|s| matches!(s, Step::Rebalance { .. } | Step::HandOut { .. })), "{now:?}");
    // Its explicit report (A6) releases it.
    let mut events = Vec::new();
    let replaced = ops::apply(
        &mut team,
        "report",
        &json!({"task":1,"attempt":"A1","text":"Done: the API is paged."}),
        &Actor::Member("dev2".into()),
        &BTreeSet::new(),
        &mut events,
    )
    .unwrap();
    assert_eq!(replaced["status"], "submitted");
    let now = steps(&team, &seen, 1.0);
    assert!(now.contains(&Step::Rebalance { task: 2, from: "dev".into(), to: "dev2".into() }), "{now:?}");
}

#[test]
fn deciding_a_captured_report_releases_its_member() {
    for decision in ["accept", "reject", "retry", "reassign", "cancel"] {
        let mut team = team();
        submit(&mut team, 1, "dev", true);
        let seen = [session("claude/x/lead", "r0", false), session("codex/x/dev", "r1", true)];
        let held = steps(&team, &seen, 1.0);
        assert!(!held.iter().any(|s| matches!(s, Step::HandOut { member, .. } if member == "dev")), "{decision}: {held:?}");
        let input = match decision {
            "reject" => json!({"task":1,"reason":"Only an acknowledgement"}),
            "reassign" => json!({"task":1,"member":"qa"}),
            _ => json!({"task":1}),
        };
        ops::apply(&mut team, decision, &input, &Actor::Member("lead".into()), &BTreeSet::new(), &mut Vec::new()).unwrap();
        assert_eq!(team.captured_hold("dev"), None, "{decision}");
        let now = steps(&team, &seen, 1.0);
        assert!(now.iter().any(|s| matches!(s, Step::HandOut { member, .. } if member == "dev")), "{decision}: {now:?}");
    }
}

#[test]
fn a_captured_report_holds_work_ahead_and_the_lead_is_never_held() {
    let mut t = team();
    t.settings.work_ahead = true;
    for task in &mut t.tasks {
        if task.phase == 1 {
            task.status = TaskStatus::Submitted;
            task.attempts.push(Attempt { n: 1, state: AttemptState::Reported, member: "dev".into(), ..Default::default() });
        }
    }
    let mut events = Vec::new();
    ops::apply(&mut t, "extend", &json!({"phase_title":"Next","tasks":[{"id":4,"title":"Independent docs","assignee":"dev"}]}),
        &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    let seen = vec![session("claude/x/lead", "r0", true), session("codex/x/dev", "r1", true)];
    assert!(steps(&t, &seen, 100.0).contains(&Step::WorkAhead { task: 4, member: "dev".into() }));
    // A captured report on #2 holds dev back from working ahead.
    t.task_mut(2).unwrap().attempts.last_mut().unwrap().report =
        Some(Report { text: "Queued".into(), at: 1.0, captured: true, ..Default::default() });
    let found = steps(&t, &seen, 100.0);
    assert!(!found.iter().any(|step| matches!(step, Step::WorkAhead { .. })), "{found:?}");
    // C1: the lead's own tasks never hold it.
    let lead = t.lead.clone();
    submit(&mut t, 3, &lead, true);
    assert_eq!(t.captured_hold(&lead), None);
}

#[test]
fn a_held_member_does_not_starve_the_plan() {
    let mut team = team();
    // dev's only work, #1, waits on a captured report; #2 belongs to qa.
    team.task_mut(2).unwrap().assignee = "qa".into();
    submit(&mut team, 1, "dev", true);
    let seen = [session("claude/x/lead", "r0", true), session("codex/x/dev", "r1", true), session("kimi/x/qa", "r2", true)];
    let now = steps(&team, &seen, 10.0);
    assert!(!now.iter().any(|step| matches!(step, Step::Starving { member } if member == "dev")), "{now:?}");
    // An episode that was open ends, and no plan_starved is told.
    let dev = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
    dev.starved_since = Some(10.0);
    let late = steps(&team, &seen, 10.0 + 30.0 * 60.0);
    assert!(late.contains(&Step::StarvedEnded { member: "dev".into() }), "{late:?}");
    assert!(!late.iter().any(|step| matches!(step, Step::PlanStarved { .. })), "{late:?}");
    // Once the report is decided, the idle member starves as before.
    team.members.iter_mut().find(|member| member.id == "dev").unwrap().starved_since = None;
    ops::apply(&mut team, "accept", &json!({"task":1}), &Actor::Human, &BTreeSet::new(), &mut Vec::new()).unwrap();
    assert!(steps(&team, &seen, 10.0).contains(&Step::Starving { member: "dev".into() }));
}

#[test]
fn the_lead_hears_that_a_captured_report_holds_its_member() {
    let mut team = team();
    submit(&mut team, 1, "dev", true);
    let item = inbox::items(&team).into_iter().find(|item| item.kind == "report").unwrap();
    assert!(item.detail.contains("dev (Backend) gets no new work until you accept, reject, retry, reassign or cancel"), "{}", item.detail);
    // An explicit report carries no such note.
    team.task_mut(1).unwrap().attempts.last_mut().unwrap().report.as_mut().unwrap().captured = false;
    let item = inbox::items(&team).into_iter().find(|item| item.kind == "report").unwrap();
    assert!(item.detail.is_empty(), "{}", item.detail);
}

#[test]
fn a_pinned_task_from_extend_is_never_rebalanced() {
    let (mut team, seen) = team_with_two_backends();
    let mut events = Vec::new();
    team.settings.phase_approval = Some(Approval::Auto);
    team.task_mut(1).unwrap().attempts.push(Attempt { n: 1, member: "dev".into(), state: AttemptState::Delivered, ..Default::default() });
    team.task_mut(1).unwrap().status = TaskStatus::InProgress;
    ops::apply(
        &mut team,
        "extend",
        &json!({"tasks":[{"id":4,"title":"Later","assignee":"dev","pinned":true}]}),
        &Actor::Human,
        &BTreeSet::new(),
        &mut events,
    )
    .unwrap();
    assert_eq!((team.task(4).unwrap().pinned, team.task(4).unwrap().status), (true, TaskStatus::Pending));
    let now = steps(&team, &seen, 1.0);
    assert!(
        !now.iter().any(|s| matches!(s, Step::Rebalance { task: 4, .. } | Step::HandOut { task: 4, .. })),
        "{now:?}"
    );
}

#[test]
fn a_task_pinned_by_reassign_is_never_rebalanced() {
    let (mut team, seen) = team_with_two_backends();
    let mut events = Vec::new();
    team.task_mut(1).unwrap().attempts.push(Attempt { n: 1, member: "dev".into(), state: AttemptState::Delivered, ..Default::default() });
    team.task_mut(1).unwrap().status = TaskStatus::InProgress;
    ops::apply(&mut team, "reassign", &json!({"task":2,"member":"dev"}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    assert!(team.task(2).unwrap().pinned);
    let now = steps(&team, &seen, 1.0);
    assert!(!now.iter().any(|s| matches!(s, Step::Rebalance { .. })), "{now:?}");
}

#[test]
fn a_pinned_task_is_retried_by_its_own_member() {
    let (mut team, seen) = team_with_two_backends();
    let mut events = Vec::new();
    ops::apply(&mut team, "reassign", &json!({"task":1,"member":"dev"}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    let task = team.task_mut(1).unwrap();
    task.attempts.push(Attempt { n: 1, member: "dev".into(), state: AttemptState::Rejected, ..Default::default() });
    task.status = TaskStatus::Pending;
    let now = steps(&team, &seen, 1.0);
    // The retry does not go to the free peer that never failed it.
    assert!(!now.iter().any(|s| matches!(s, Step::Rebalance { task: 1, .. })), "{now:?}");
    assert!(now.contains(&Step::HandOut { task: 1, member: "dev".into() }), "{now:?}");
}

#[test]
fn the_lead_hears_of_moved_tasks_in_one_message() {
    let (mut team, mut seen) = team_with_two_backends();
    for id in [1, 2] {
        let task = team.task_mut(id).unwrap();
        task.assignee = "dev2".into();
        task.attempts.push(Attempt { n: 1, member: "dev2".into(), state: AttemptState::Delivered, rebalanced_from: "dev".into(), ..Default::default() });
        task.status = TaskStatus::InProgress;
    }
    assert!(!steps(&team, &seen, 1.0).iter().any(|s| matches!(s, Step::Rebalanced { .. })));
    seen[0].free = true;
    assert!(steps(&team, &seen, 1.0).contains(&Step::Rebalanced { tasks: vec![1, 2] }));
    let text = rebalanced_text(&team, &[1, 2]);
    assert!(text.contains("#1 API: given to dev2 (Backend) instead of dev") && text.contains("#2 More API"), "{text}");
    for id in [1, 2] {
        team.task_mut(id).unwrap().attempts[0].told_lead.push("rebalanced".into());
    }
    assert!(!steps(&team, &seen, 1.0).iter().any(|s| matches!(s, Step::Rebalanced { .. })));
}

#[test]
fn the_lead_hears_once_that_every_task_is_decided() {
    let mut team = team();
    for task in &mut team.tasks {
        task.status = TaskStatus::Accepted;
    }
    team.idle = Notice::Pending;
    let lead = session("claude/x/lead", "r0", true);
    let seen = vec![lead, session("codex/x/dev", "r1", true)];
    assert!(steps(&team, &seen, 1.0).contains(&Step::AllDecided));
    // A busy lead hears it once it is free.
    let busy = vec![session("claude/x/lead", "r0", false), session("codex/x/dev", "r1", true)];
    assert!(!steps(&team, &busy, 1.0).iter().any(|s| matches!(s, Step::AllDecided)));
    // Once sent, no second message for the same state.
    team.idle = Notice::Sent;
    assert!(!steps(&team, &seen, 1.0).iter().any(|s| matches!(s, Step::AllDecided)));
    // New work clears it; deciding the new work arms one message again.
    team.idle = Notice::None;
    assert!(!steps(&team, &seen, 1.0).iter().any(|s| matches!(s, Step::AllDecided)));
    team.idle = Notice::Pending;
    team.status = TeamStatus::Paused;
    assert!(!steps(&team, &seen, 1.0).iter().any(|s| matches!(s, Step::AllDecided)));
    team.status = TeamStatus::Running;
    assert!(steps(&team, &seen, 1.0).contains(&Step::AllDecided));
    let text = all_decided_text(&team);
    assert!(text.contains("All 3 tasks are decided (3 accepted, 0 rejected, 0 cancelled)"), "{text}");
    assert!(text.contains("hgs team extend") && text.contains("hgs team finish"), "{text}");
}
