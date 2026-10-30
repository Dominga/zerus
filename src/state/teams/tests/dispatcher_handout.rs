use super::*;
use super::support::*;

#[test]
fn the_dispatcher_hands_out_the_task_with_the_most_dependants_first() {
    let mut team = team();
    // #4 unblocks #5 and #6, more than #1 unblocks #3 (C6).
    team.tasks.push(Task { id: 4, title: "Base".into(), assignee: "dev".into(), status: TaskStatus::Pending, ..Default::default() });
    team.tasks.push(Task {
        id: 5,
        title: "Next".into(),
        assignee: "dev".into(),
        status: TaskStatus::Pending,
        deps: vec![Dependency { task: 4, kind: DependencyKind::Hard }],
        ..Default::default()
    });
    team.tasks.push(Task {
        id: 6,
        title: "Later".into(),
        assignee: "dev".into(),
        status: TaskStatus::Pending,
        deps: vec![Dependency { task: 4, kind: DependencyKind::Hard }],
        ..Default::default()
    });
    let dev = session("codex/x/dev", "r1", true);
    let found = steps(&team, &[session("claude/x/lead", "r0", false), dev.clone()], 10.0);
    assert!(found.contains(&Step::HandOut { task: 4, member: "dev".into() }), "{found:?}");
    assert!(!found.iter().any(|step| matches!(step, Step::HandOut { task: 1, .. })), "{found:?}");
}

#[test]
fn a_check_hand_out_names_the_command_and_its_exit_code() {
    let mut team = team();
    team.tasks.push(Task {
        id: 4,
        title: "Build".into(),
        assignee: "dev".into(),
        kind: TaskKind::Check,
        check: Some(Check { command: "cargo test --locked".into(), expect_exit: 0, timeout_minutes: 30 }),
        status: TaskStatus::Pending,
        ..Default::default()
    });
    let attempt = Attempt { n: 1, id: "A4".into(), member: "dev".into(), state: AttemptState::Delivered, ..Default::default() };
    let text = hand_off_text(&team, team.task(4).unwrap(), &attempt, "", "");
    assert!(text.contains("This is a check task"), "{text}");
    assert!(text.contains("cargo test --locked") && text.contains("Expected exit code: 0"), "{text}");
    // The report command carries the check result the member fills in.
    assert!(text.contains("\"check\":{\"exit\":0"), "{text}");
    // A plan task's hand-out has no check section.
    let normal = hand_off_text(&team, team.task(1).unwrap(), &attempt, "", "");
    assert!(!normal.contains("This is a check task"), "{normal}");
}

#[test]
fn a_member_with_no_ready_work_starves_the_plan_and_the_lead_hears_once() {
    let mut team = team();
    // All ready work belongs to qa; dev sits idle while tasks are open.
    for id in [1, 2] {
        team.task_mut(id).unwrap().assignee = "qa".into();
    }
    let lead = session("claude/x/lead", "r0", true);
    let dev = session("codex/x/dev", "r1", true);
    let qa = session("kimi/x/qa", "r2", true);
    let seen = [lead.clone(), dev.clone(), qa.clone()];
    // The first idle tick opens the episode.
    assert!(steps(&team, &seen, 10.0).contains(&Step::Starving { member: "dev".into() }));
    team.members.iter_mut().find(|member| member.id == "dev").unwrap().starved_since = Some(10.0);
    // Not before 20 minutes, then once.
    assert!(!steps(&team, &seen, 10.0 + 20.0 * 60.0 - 1.0).iter().any(|step| matches!(step, Step::PlanStarved { .. })));
    let late = steps(&team, &seen, 10.0 + 20.0 * 60.0);
    assert!(late.contains(&Step::PlanStarved { member: "dev".into() }), "{late:?}");
    let member = team.member("dev").unwrap().clone();
    let text = starved_text(&team, &member, 10.0 + 20.0 * 60.0);
    assert!(text.contains("starved") && text.contains("Backend") && text.contains("20 min"), "{text}");
    assert!(text.contains("#1") && text.contains("hgs guide team-lead"), "{text}");
    // Told once per episode.
    team.members.iter_mut().find(|member| member.id == "dev").unwrap().starved_told = true;
    assert!(!steps(&team, &seen, 10.0 + 30.0 * 60.0).iter().any(|step| matches!(step, Step::PlanStarved { .. })));
    // Getting work ends the episode.
    team.members.iter_mut().find(|member| member.id == "dev").unwrap().starved_told = false;
    team.task_mut(2).unwrap().assignee = "dev".into();
    assert!(steps(&team, &seen, 10.0 + 30.0 * 60.0).contains(&Step::StarvedEnded { member: "dev".into() }));
    // A busy leader is not starving: it has work.
    team.task_mut(1).unwrap().assignee = "qa".into();
    team.task_mut(2).unwrap().assignee = "qa".into();
    let busy_dev = session("codex/x/dev", "r1", false);
    assert!(!steps(&team, &[lead, busy_dev, qa], 10.0 + 30.0 * 60.0).iter().any(|step| matches!(step, Step::Starving { .. })));
}

#[test]
fn starving_stays_quiet_until_the_owner_approves_a_phase() {
    let mut team = team();
    // Every open task is in a draft phase: the owner owes an approval.
    for task in &mut team.tasks {
        task.phase = 2;
        task.status = TaskStatus::Draft;
    }
    for id in [1, 2] {
        team.task_mut(id).unwrap().assignee = "qa".into();
    }
    team.phases.push(Phase {
        n: 2,
        title: "Next".into(),
        state: PhaseState::Draft,
        tasks: team.tasks.iter().map(|task| task.id).collect(),
        ..Default::default()
    });
    team.members.iter_mut().find(|member| member.id == "dev").unwrap().starved_since = Some(10.0);
    let lead = session("claude/x/lead", "r0", true);
    let dev = session("codex/x/dev", "r1", true);
    let qa = session("kimi/x/qa", "r2", true);
    let seen = [lead.clone(), dev.clone(), qa.clone()];
    // U9: no episode starts or continues; the old one ends.
    let waiting = steps(&team, &seen, 10.0 + 30.0 * 60.0);
    assert!(
        !waiting.iter().any(|step| matches!(step, Step::Starving { .. } | Step::PlanStarved { .. })),
        "{waiting:?}"
    );
    assert!(waiting.contains(&Step::StarvedEnded { member: "dev".into() }), "{waiting:?}");
    // The phase is approved and its tasks are in work: dev starves again.
    team.phases[0].state = PhaseState::Approved;
    for task in &mut team.tasks {
        task.status = TaskStatus::Pending;
    }
    team.members.iter_mut().find(|member| member.id == "dev").unwrap().starved_since = None;
    assert!(steps(&team, &seen, 10.0).contains(&Step::Starving { member: "dev".into() }));
}

#[test]
fn a_declared_budget_ties_to_the_job_and_turns_amber_then_red() {
    let mut team = team();
    let lead = session("claude/x/lead", "r0", true);
    let mut dev = session("codex/x/dev", "r1", true);
    team.members.iter_mut().find(|member| member.id == "dev").unwrap().budget = Some(Budget {
        label: "install".into(),
        minutes: 15,
        red_minutes: 30,
        declared_at: 100.0,
        ..Default::default()
    });
    // Without Background work the budget waits for its job (up to a minute).
    let idle = steps(&team, &[lead.clone(), dev.clone()], 110.0);
    assert!(!idle.iter().any(|step| matches!(step, Step::BudgetRed { .. } | Step::BudgetAmber { .. })), "{idle:?}");
    // The job appears: the budget is tied to it and the episode starts.
    dev.background = true;
    dev.work = Some(SeenWork { command: "npm install".into(), started_at: 140.0 });
    let tied = steps(&team, &[lead.clone(), dev.clone()], 150.0);
    assert!(tied.contains(&Step::BackgroundStart { member: "dev".into() }), "{tied:?}");
    assert!(
        tied.contains(&Step::BudgetTie { member: "dev".into(), command: "npm install".into(), since: 140.0 }),
        "{tied:?}"
    );
    // Tied and running: amber at 15 minutes, red at 30, once each.
    let member = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
    member.background_since = Some(140.0);
    member.budget.as_mut().unwrap().job = Some(JobRef { command: "npm install".into(), since: 140.0 });
    let amber = steps(&team, &[lead.clone(), dev.clone()], 140.0 + 15.0 * 60.0);
    assert!(amber.contains(&Step::BudgetAmber { member: "dev".into() }), "{amber:?}");
    assert!(!amber.iter().any(|step| matches!(step, Step::BudgetRed { .. })));
    team.members.iter_mut().find(|member| member.id == "dev").unwrap().budget.as_mut().unwrap().amber_told = true;
    let red = steps(&team, &[lead.clone(), dev.clone()], 140.0 + 30.0 * 60.0);
    assert!(red.contains(&Step::BudgetRed { member: "dev".into() }), "{red:?}");
    team.members.iter_mut().find(|member| member.id == "dev").unwrap().budget.as_mut().unwrap().red_told = true;
    assert!(!steps(&team, &[lead.clone(), dev.clone()], 140.0 + 40.0 * 60.0).iter().any(|step| matches!(step, Step::BudgetRed { .. })));
    // The message names the label, the estimate, the command and the output.
    let member = team.member("dev").unwrap().clone();
    let text = budget_text(&team, &member, "npm install", 140.0, 140.0 + 34.0 * 60.0, "npm ERR! build failed");
    assert!(text.contains("install") && text.contains("expected 15 min, red after 30 min"), "{text}");
    assert!(text.contains("npm install") && text.contains("npm ERR! build failed"), "{text}");
    // Background work without a budget is red at the default 30 minutes.
    let mut plain = team.clone();
    plain.members.iter_mut().find(|member| member.id == "dev").unwrap().budget = None;
    plain.members.iter_mut().find(|member| member.id == "dev").unwrap().background_since = Some(0.0);
    let late = steps(&plain, &[lead.clone(), dev.clone()], 31.0 * 60.0);
    assert!(
        late.contains(&Step::BackgroundRed { member: "dev".into(), command: "npm install".into(), since: 0.0 }),
        "{late:?}"
    );
    // The job ends: the budget ends with it.
    dev.background = false;
    dev.work = None;
    let ended = steps(&team, &[lead, dev], 140.0 + 41.0 * 60.0);
    assert!(ended.contains(&Step::BackgroundEnd { member: "dev".into() }), "{ended:?}");
}

#[test]
fn mcp_services_are_listed_on_the_member_and_not_background_work() {
    let listing = json!([{"sessions":[
        {"name":"codex/x/dev","process_state":"running","activity":"idle","phase":"idle",
         "process_work":{"count":0,"services":[{"name":"socraticode","command":"node socraticode.js","since":5.0}]}}]}]);
    let found = seen(&listing);
    assert_eq!(
        found[0].services,
        vec![SeenService { name: "socraticode".into(), command: "node socraticode.js".into(), since: 5.0 }]
    );
    // G7: a session that only keeps its MCP server alive is not Background.
    assert!(!found[0].background && found[0].free);
    // The member carries its services for status.
    let team = team();
    let steps_now = steps(&team, &[session("claude/x/lead", "r0", true), found[0].clone()], 10.0);
    assert!(
        steps_now.iter().any(|step| matches!(step, Step::Services { member, services } if member == "dev" && services.len() == 1)),
        "{steps_now:?}"
    );
}

#[test]
fn a_ready_task_goes_to_a_free_member_with_the_same_role() {
    let (mut team, seen) = team_with_two_backends();
    // dev works on #1; #2, also dev's, goes to the free dev2.
    team.task_mut(1).unwrap().attempts.push(Attempt { n: 1, member: "dev".into(), state: AttemptState::Delivered, ..Default::default() });
    team.task_mut(1).unwrap().status = TaskStatus::InProgress;
    let now = steps(&team, &seen, 1.0);
    assert!(now.contains(&Step::Rebalance { task: 2, from: "dev".into(), to: "dev2".into() }), "{now:?}");
    // Your or the lead's choice stays with its member.
    team.task_mut(2).unwrap().pinned = true;
    assert!(!steps(&team, &seen, 1.0).iter().any(|s| matches!(s, Step::Rebalance { .. })));
    // Another role is not the same work.
    team.task_mut(2).unwrap().pinned = false;
    team.members.iter_mut().find(|m| m.id == "dev2").unwrap().role = "Frontend".into();
    assert!(!steps(&team, &seen, 1.0).iter().any(|s| matches!(s, Step::Rebalance { .. })));
}

#[test]
fn a_rejected_task_is_retried_by_a_fresh_member_when_one_is_free() {
    let (mut team, mut seen) = team_with_two_backends();
    let task = team.task_mut(1).unwrap();
    task.attempts.push(Attempt { n: 1, member: "dev".into(), state: AttemptState::Rejected, ..Default::default() });
    task.status = TaskStatus::Pending;
    // dev failed #1 and is free, yet the free dev2 takes the retry; dev takes #2.
    let now = steps(&team, &seen, 1.0);
    assert!(now.contains(&Step::Rebalance { task: 1, from: "dev".into(), to: "dev2".into() }), "{now:?}");
    assert!(now.contains(&Step::HandOut { task: 2, member: "dev".into() }), "{now:?}");
    // Without a free peer, the assignee retries rather than wait.
    seen[2].free = false;
    let now = steps(&team, &seen, 1.0);
    assert!(now.contains(&Step::HandOut { task: 1, member: "dev".into() }), "{now:?}");
}

#[test]
fn a_stage_returned_task_stays_with_its_implementer_not_a_peer() {
    // #630 C2: a return's attempt is not failed(); the implementer keeps
    // the task, while a lead's rejection still rebalances it.
    let (mut team, seen) = team_with_two_backends();
    let task = team.task_mut(1).unwrap();
    task.attempts.push(Attempt { n: 1, member: "dev".into(), state: AttemptState::Rejected, pipeline_return: true, ..Default::default() });
    task.status = TaskStatus::Pending;
    let now = steps(&team, &seen, 1.0);
    assert!(now.contains(&Step::HandOut { task: 1, member: "dev".into() }), "{now:?}");
    assert!(!now.iter().any(|step| matches!(step, Step::Rebalance { task: 1, .. })), "{now:?}");
    let mut rejected = team.clone();
    rejected.task_mut(1).unwrap().attempts[0].pipeline_return = false;
    let now = steps(&rejected, &seen, 1.0);
    assert!(now.contains(&Step::Rebalance { task: 1, from: "dev".into(), to: "dev2".into() }), "{now:?}");
}

#[test]
fn a_pinned_task_from_plan_is_never_rebalanced() {
    let (mut team, _) = ops::create(
        "t",
        &json!({"name":"Checkout","lead":{"id":"lead","agent":"claude","session":{"name":"claude/x/lead","run_id":"r0"}}}),
    )
    .unwrap();
    let mut events = Vec::new();
    ops::apply(
        &mut team,
        "plan",
        &json!({"members":[{"id":"dev","role":"Backend","agent":"codex","session":{"name":"codex/x/dev","run_id":"r1"}}],
            "tasks":[{"id":1,"title":"API","assignee":"dev"},{"id":2,"title":"More API","assignee":"dev","pinned":true}]}),
        &Actor::Member("lead".into()),
        &BTreeSet::new(),
        &mut events,
    )
    .unwrap();
    ops::apply(&mut team, "approve", &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    assert!(team.task(2).unwrap().pinned);
    let mut dev2 = team.member("dev").unwrap().clone();
    dev2.id = "dev2".into();
    dev2.session = Some(SessionRef { name: "codex/x/dev2".into(), run_id: "r3".into(), conversation_id: "c".into(), ..Default::default() });
    team.members.push(dev2);
    team.task_mut(1).unwrap().attempts.push(Attempt { n: 1, member: "dev".into(), state: AttemptState::Delivered, ..Default::default() });
    team.task_mut(1).unwrap().status = TaskStatus::InProgress;
    let seen = vec![session("claude/x/lead", "r0", false), session("codex/x/dev", "r1", true), session("codex/x/dev2", "r3", true)];
    let now = steps(&team, &seen, 1.0);
    // dev is busy with #1, but #2 stays with its member: no peer takes it.
    assert!(!now.iter().any(|s| matches!(s, Step::Rebalance { .. })), "{now:?}");
    assert!(!now.iter().any(|s| matches!(s, Step::HandOut { task: 2, .. })), "{now:?}");
}

#[test]
fn a_deepseek_member_takes_its_settings_before_its_task() {
    let mut team = team();
    let dsh = team.members.iter_mut().find(|m| m.id == "dev").unwrap();
    dsh.agent = "dsh".into(); dsh.effort = "low".into();
    let seen = [session("claude/x/lead", "r0", true), session("codex/x/dev", "r1", true)];
    let steps_now = steps(&team, &seen, 1.0);
    assert!(steps_now.contains(&Step::Settings { member: "dev".into() }), "{steps_now:?}");
    assert!(!steps_now.iter().any(|s| matches!(s, Step::HandOut { member, .. } if member == "dev")));
    team.members.iter_mut().find(|m| m.id == "dev").unwrap().settings_applied = true;
    assert!(steps(&team, &seen, 1.0).iter().any(|s| matches!(s, Step::HandOut { member, .. } if member == "dev")));
}

#[test]
fn send_outcomes_are_classified_conservatively() {
    let output = |code: i32, stdout: &str, stderr: &str| {
        #[cfg(windows)]
        let status = std::os::windows::process::ExitStatusExt::from_raw(code as u32);
        #[cfg(unix)]
        let status = std::os::unix::process::ExitStatusExt::from_raw(code << 8);
        Ok(Output { status, stdout: stdout.into(), stderr: stderr.into() })
    };
    assert_eq!(classify(&output(0, r#"{"status":"submitted"}"#, "")), Sent::Delivered);
    assert!(matches!(classify(&output(1, "", "session is busy")), Sent::Refused(_)));
    assert!(matches!(classify(&output(1, "", "delivery uncertain: x")), Sent::Uncertain(_)));
    assert!(matches!(classify(&output(255, "", "ssh: connection reset")), Sent::Uncertain(_)));
    assert!(matches!(classify(&Err(anyhow::anyhow!("timed out"))), Sent::Uncertain(_)));
}
