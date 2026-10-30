use super::*;
use super::support::*;

#[test]
fn only_you_approve_and_errors_block_it() {
    let (mut team, _) = create("t", &json!({"name":"T","lead":{"id":"lead","agent":"claude"}})).unwrap();
    let mut events = Vec::new();
    plan(&mut team, &json!({"tasks":[{"id":1,"title":"A","assignee":"ghost"}]}), &Actor::Human, &mut events).unwrap();
    assert!(approve(&mut team, &json!({}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut events).is_err());
    let error = approve(&mut team, &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap_err().to_string();
    assert!(error.contains("no assignee"), "{error}");
    assert!(plan(&mut team, &json!({"tasks":[{"id":1,"title":"A","assignee":"lead","deps":[{"task":1}]}]}), &Actor::Human, &mut events).is_err());
}

#[test]
fn a_report_needs_the_current_attempt_and_a_stale_one_is_recorded() {
    let mut team = running_team();
    let first = hand_out(&mut team, 1);
    let mut events = Vec::new();
    reassign(&mut team, &json!({"task":1,"member":"dev"}), &Actor::Human, &mut events).unwrap();
    assert!(team.task(1).unwrap().pinned);   // your choice: Zerus does not move it to another Backend
    let second = hand_out(&mut team, 1);
    let late = report(&mut team, &json!({"task":1,"attempt":first,"text":"done"}), &Actor::Member("dev".into()), &mut events).unwrap();
    assert_eq!((late["status"].as_str(), events.last().unwrap().kind), (Some("stale"), "stale_report"));
    // Another member cannot report this attempt either.
    let foreign = report(&mut team, &json!({"task":1,"attempt":second,"text":"done"}), &Actor::Member("qa".into()), &mut events).unwrap();
    assert_eq!(foreign["status"], "stale");
    report(&mut team, &json!({"task":1,"attempt":second,"text":"done"}), &Actor::Member("dev".into()), &mut events).unwrap();
    assert_eq!(team.task(1).unwrap().status, TaskStatus::Submitted);
}

#[test]
fn a_member_repairs_a_placeholder_report_before_the_lead_decides() {
    // U42: a queued report that reached the store before its whole text did
    // closed #512's attempt with the placeholder "poll". The member replaces
    // it while the task is undecided; a decision closes that door.
    let mut team = running_team();
    let attempt = hand_out(&mut team, 1);
    let mut events = Vec::new();
    let first = report(&mut team, &json!({"task":1,"attempt":attempt,"text":"poll"}), &Actor::Member("dev".into()), &mut events).unwrap();
    assert_eq!((first["status"].as_str(), team.task(1).unwrap().status), (Some("submitted"), TaskStatus::Submitted));
    let whole = report(
        &mut team,
        &json!({"task":1,"attempt":attempt,"text":"The whole report: 512 checks and their totals"}),
        &Actor::Member("dev".into()),
        &mut events,
    )
    .unwrap();
    assert_eq!(whole["status"], "submitted");
    let task = team.task(1).unwrap();
    let stored = task.attempts[0].report.as_ref().unwrap();
    assert_eq!(stored.text, "The whole report: 512 checks and their totals");
    assert_eq!(stored.replaced.last().map(|replaced| replaced.text.as_str()), Some("poll"));
    assert_eq!(task.attempts[0].notice, Notice::Pending);
    assert_eq!(events.last().unwrap().kind, "report_replaced");
    // Another member's attempt, and an older report, are not this one's.
    let stolen = report(&mut team, &json!({"task":1,"attempt":attempt,"text":"mine"}), &Actor::Member("qa".into()), &mut events).unwrap();
    assert_eq!(stolen["status"], "stale");
    // The lead's decision ends the replacement window.
    apply(&mut team, "accept", &json!({"task":1}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut events).unwrap();
    let late = report(&mut team, &json!({"task":1,"attempt":attempt,"text":"later"}), &Actor::Member("dev".into()), &mut events).unwrap();
    assert_eq!(late["status"], "stale");
}

#[test]
fn the_lead_reports_and_accepts_its_own_task_in_one_step() {
    let (mut team, _) = create("t", &json!({"name":"T","lead":{"id":"lead","agent":"claude","session":{"name":"claude/x/lead"}}})).unwrap();
    let mut events = Vec::new();
    plan(
        &mut team,
        &json!({"members":[{"id":"dev","role":"Backend","agent":"codex"}],
            "tasks":[{"id":1,"title":"Review the plan","assignee":"lead"}]}),
        &Actor::Member("lead".into()),
        &mut events,
    )
    .unwrap();
    approve(&mut team, &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    // C1: the lead's task is handed out without a message; its report and
    // its acceptance are one step.
    let attempt = hand_out(&mut team, 1);
    events.clear();
    let receipt = report(
        &mut team,
        &json!({"task":1,"attempt":attempt,"text":"Reviewed: the plan holds","accept":true}),
        &Actor::Member("lead".into()),
        &mut events,
    )
    .unwrap();
    assert_eq!((receipt["status"].as_str(), receipt["attempt"].as_u64()), (Some("accepted"), Some(1)));
    let task = team.task(1).unwrap();
    assert_eq!((task.status, task.decided_by), (TaskStatus::Accepted, Some(Decider::Lead)));
    // U37: the decision's time is on the attempt it decided.
    assert!(task.attempts[0].decided_at.is_some());
    assert_eq!(task.attempts[0].report.as_ref().unwrap().text, "Reviewed: the plan holds");
    assert_eq!(task.attempts[0].notice, Notice::None);
    let kinds: Vec<&str> = events.iter().map(|event| event.kind).collect();
    assert_eq!(kinds, vec!["reported", "accepted"]);
    assert_eq!(events[1].payload["by"], "Lead");
    // Only the lead accepts on a report, and only its own task.
    let mut other = running_team();
    let attempt = hand_out(&mut other, 1);
    let error = report(
        &mut other,
        &json!({"task":1,"attempt":attempt,"text":"Mine","accept":true}),
        &Actor::Member("dev".into()),
        &mut events,
    )
    .unwrap_err()
    .to_string();
    assert_eq!(error, "Only the lead accepts its own task on its report");
    let error = report(&mut other, &json!({"task":1,"attempt":attempt,"text":"Mine","accept":true}), &Actor::Human, &mut events)
        .unwrap_err()
        .to_string();
    assert_eq!(error, "Only the lead accepts its own task on its report");
    // Without accept the report waits for a decision, as any other.
    let receipt = report(&mut other, &json!({"task":1,"attempt":attempt,"text":"Done"}), &Actor::Member("dev".into()), &mut events).unwrap();
    assert_eq!(receipt["status"], "submitted");
}

#[test]
fn a_check_task_is_decided_by_its_exit_code() {
    let check_team = |command: &str, expect_exit: i32| -> Team {
        let (mut team, _) = create("t", &json!({"name":"T","lead":{"id":"lead","agent":"claude"}})).unwrap();
        let mut events = Vec::new();
        plan(
            &mut team,
            &json!({"members":[{"id":"dev","role":"Backend","agent":"codex"}],
                "tasks":[{"id":1,"title":"Build","assignee":"dev","kind":"check",
                    "check":{"command":command,"expect_exit":expect_exit,"timeout_minutes":5}}]}),
            &Actor::Human,
            &mut events,
        )
        .unwrap();
        approve(&mut team, &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
        team
    };
    let mut events = Vec::new();
    // A check task carries its command, expected exit code and time limit.
    let mut team = check_team("cargo test --locked", 0);
    let attempt = hand_out(&mut team, 1);
    let passed = report(
        &mut team,
        &json!({"task":1,"attempt":attempt,"check":{"exit":0,"tail":"test result: ok. 42 passed"}}),
        &Actor::Member("dev".into()),
        &mut events,
    )
    .unwrap();
    assert_eq!((passed["status"].as_str(), passed["check"].as_str()), (Some("accepted"), Some("passed")));
    let task = team.task(1).unwrap();
    assert_eq!(task.status, TaskStatus::Accepted);
    assert_eq!(task.decided_by, None);
    assert_eq!(task.decision_reason, "check passed: exit 0");
    let check = task.attempts.last().unwrap().check.as_ref().unwrap();
    assert_eq!((check.exit, check.tail.as_str()), (0, "test result: ok. 42 passed"));
    assert!(task.attempts.last().unwrap().report.as_ref().unwrap().text.contains("cargo test --locked"));
    assert_eq!(events.iter().rev().take(2).map(|event| event.kind).collect::<Vec<_>>(), vec!["check_passed", "reported"]);
    // A wrong exit code rejects the task finally, without the lead.
    let mut failed_team = check_team("cargo test", 0);
    let attempt = hand_out(&mut failed_team, 1);
    let failed = report(
        &mut failed_team,
        &json!({"task":1,"attempt":attempt,"check":{"exit":101,"tail":"error: test failed"}}),
        &Actor::Member("dev".into()),
        &mut events,
    )
    .unwrap();
    assert_eq!((failed["status"].as_str(), failed["check"].as_str(), failed["final"].as_bool()), (Some("rejected"), Some("failed"), Some(false)));
    let task = failed_team.task(1).unwrap();
    // N6: a failed check is a non-final reject - another attempt - and
    // only the lead's decision ends it; the check result stays.
    assert_eq!(task.status, TaskStatus::Pending);
    assert_eq!(task.decision_reason, "check failed: exit 101, expected 0");
    assert_eq!(task.attempts.last().unwrap().check.as_ref().unwrap().exit, 101);
    assert_eq!(events.last().unwrap().kind, "check_failed");
    // The failed check waits for the lead: ready never hands it out again.
    assert!(!plan::ready(&failed_team).contains(&1));
    let held = failed_team.task(1).unwrap().hold.as_ref().unwrap();
    assert_eq!((held.by.as_str(), held.reason.as_str()), ("Zerus", "check failed: exit 101, expected 0"));
    // A release gives it another attempt; cancel still ends it.
    apply(&mut failed_team, "release", &json!({"task":1}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    assert!(plan::ready(&failed_team).contains(&1));
    let again = hand_out(&mut failed_team, 1);
    assert_ne!(again, attempt);
    apply(&mut failed_team, "cancel", &json!({"task":1}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    assert_eq!(failed_team.task(1).unwrap().status, TaskStatus::Cancelled);
    // A non-zero expected exit accepts a failing command (a lint check).
    let mut nonzero = check_team("cargo fmt --check", 1);
    let attempt = hand_out(&mut nonzero, 1);
    let passed = report(
        &mut nonzero,
        &json!({"task":1,"attempt":attempt,"check":{"exit":1,"tail":"Diff in src/lib.rs"}}),
        &Actor::Member("dev".into()),
        &mut events,
    )
    .unwrap();
    assert_eq!(passed["check"], "passed");
}

#[test]
fn a_check_task_needs_a_command_and_kind() {
    let (mut team, _) = create("t", &json!({"name":"T","lead":{"id":"lead","agent":"claude"}})).unwrap();
    let mut events = Vec::new();
    let mut plan_with = |team: &mut Team, task: Value| {
        plan(team, &json!({"tasks":[task]}), &Actor::Human, &mut events).map(|_| ())
    };
    let missing = plan_with(&mut team, json!({"id":1,"title":"Build","assignee":"dev","kind":"check"})).unwrap_err().to_string();
    assert!(missing.contains("add \"check\":"), "{missing}");
    let empty = plan_with(
        &mut team,
        json!({"id":1,"title":"Build","assignee":"dev","kind":"check","check":{"command":"  "}}),
    )
    .unwrap_err()
    .to_string();
    assert_eq!(empty, "Task #1 is a check: its command is empty");
    let timeout = plan_with(
        &mut team,
        json!({"id":1,"title":"Build","assignee":"dev","kind":"check","check":{"command":"cargo test","timeout_minutes":0}}),
    )
    .unwrap_err()
    .to_string();
    assert_eq!(timeout, "Task #1: a check waits 1 to 1440 minutes");
    let no_kind = plan_with(
        &mut team,
        json!({"id":1,"title":"Build","assignee":"dev","check":{"command":"cargo test"}}),
    )
    .unwrap_err()
    .to_string();
    assert_eq!(no_kind, "Task #1 has a check but no \"kind\":\"check\"");
}

#[test]
fn a_member_declares_a_budget_for_its_background_job() {
    let mut team = running_team();
    let mut events = Vec::new();
    let none = BTreeSet::new();
    let receipt = apply(
        &mut team,
        "expect",
        &json!({"label":"install","minutes":15}),
        &Actor::Member("dev".into()),
        &none,
        &mut events,
    )
    .unwrap();
    assert_eq!((receipt["status"].as_str(), receipt["red_minutes"].as_u64()), (Some("expected"), Some(30)));
    let budget = team.member("dev").unwrap().budget.clone().unwrap();
    assert_eq!((budget.label.as_str(), budget.minutes, budget.red_minutes), ("install", 15, 30));
    assert_eq!(budget.job, None);
    assert_eq!(events.last().unwrap().kind, "budget_declared");
    // C12: ok, then amber past the estimate, then red past the red time.
    assert_eq!(budget.state(budget.declared_at + 14.0 * 60.0), "ok");
    assert_eq!(budget.state(budget.declared_at + 15.0 * 60.0), "amber");
    assert_eq!(budget.state(budget.declared_at + 29.0 * 60.0), "amber");
    assert_eq!(budget.state(budget.declared_at + 30.0 * 60.0), "red");
    // Only the member declares; bounds are named.
    let human = apply(&mut team, "expect", &json!({"label":"x","minutes":15}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
    assert_eq!(human, "Only a team member declares a budget for its own work");
    let zero = apply(&mut team, "expect", &json!({"label":"x","minutes":0}), &Actor::Member("dev".into()), &none, &mut events)
        .unwrap_err()
        .to_string();
    assert_eq!(zero, "A budget is 1 to 1440 minutes");
    let red_early = apply(
        &mut team,
        "expect",
        &json!({"label":"x","minutes":15,"red_minutes":10}),
        &Actor::Member("dev".into()),
        &none,
        &mut events,
    )
    .unwrap_err()
    .to_string();
    assert_eq!(red_early, "The red time cannot come before the estimate");
    let empty = apply(&mut team, "expect", &json!({"minutes":15}), &Actor::Member("dev".into()), &none, &mut events)
        .unwrap_err()
        .to_string();
    assert_eq!(empty, "A budget needs a label");
}

#[test]
fn a_human_decision_is_final_and_overrides_the_lead() {
    let mut team = running_team();
    let attempt = hand_out(&mut team, 1);
    let mut events = Vec::new();
    report(&mut team, &json!({"task":1,"attempt":attempt,"text":"done"}), &Actor::Member("dev".into()), &mut events).unwrap();
    assert!(decide(&mut team, &json!({"task":1}), &Actor::Member("qa".into()), true, &mut events).is_err());
    decide(&mut team, &json!({"task":1}), &Actor::Member("lead".into()), true, &mut events).unwrap();
    // You override the lead's acceptance; the lead cannot undo yours.
    decide(&mut team, &json!({"task":1,"reason":"404 for foreign cards","final":true}), &Actor::Human, false, &mut events).unwrap();
    assert_eq!(team.task(1).unwrap().status, TaskStatus::Rejected);
    assert!(decide(&mut team, &json!({"task":1}), &Actor::Member("lead".into()), true, &mut events).is_err());
}

#[test]
fn rejecting_returns_the_task_for_another_attempt() {
    let mut team = running_team();
    let attempt = hand_out(&mut team, 1);
    let mut events = Vec::new();
    report(&mut team, &json!({"task":1,"attempt":attempt,"text":"done"}), &Actor::Member("dev".into()), &mut events).unwrap();
    assert!(decide(&mut team, &json!({"task":1}), &Actor::Member("lead".into()), false, &mut events).is_err());
    decide(&mut team, &json!({"task":1,"reason":"paginate"}), &Actor::Member("lead".into()), false, &mut events).unwrap();
    let task = team.task(1).unwrap();
    assert_eq!((task.status, task.attempts[0].state), (TaskStatus::Pending, AttemptState::Rejected));
    assert!(task.attempts[0].decided_at.is_some());
}

#[test]
fn a_decided_team_waits_for_the_lead_instead_of_closing() {
    let mut team = running_team();
    let mut events = Vec::new();
    let none = BTreeSet::new();
    for id in [1, 2] {
        let attempt = hand_out(&mut team, id);
        let member = team.task(id).unwrap().assignee.clone();
        report(&mut team, &json!({"task":id,"attempt":attempt,"text":"ok"}), &Actor::Member(member), &mut events).unwrap();
    }
    apply(&mut team, "accept", &json!({"task":1}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!((team.status, team.idle), (TeamStatus::Running, Notice::None));
    apply(&mut team, "cancel", &json!({"task":2}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!((team.status, team.idle), (TeamStatus::Running, Notice::Pending));
    assert_eq!(events.last().unwrap().kind, "all_decided");
    assert_eq!(events.last().unwrap().payload["accepted"], 1);
    assert_eq!(events.last().unwrap().payload["cancelled"], 1);
    assert_eq!(events.last().unwrap().payload["tasks"], 2);
    assert!(!events.iter().any(|event| event.kind == "done"));
    // New work clears the waiting state; deciding it arms one message again.
    team.settings.phase_approval = Some(Approval::Auto);
    apply(&mut team, "extend", &json!({"tasks":[{"id":3,"title":"More","assignee":"dev"}]}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!(team.idle, Notice::None);
    apply(&mut team, "cancel", &json!({"task":3}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!(team.idle, Notice::Pending);
    assert_eq!(events.iter().filter(|event| event.kind == "all_decided").count(), 2);
}

#[test]
fn a_member_asks_about_its_open_attempt() {
    let mut team = running_team();
    let attempt = hand_out(&mut team, 1);
    let mut events = Vec::new();
    let none = BTreeSet::new();
    let error = apply(&mut team, "ask", &json!({"text":"Which?"}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
    assert_eq!(error, "Only a team member asks the lead");
    let error = apply(&mut team, "ask", &json!({"text":"Which?"}), &Actor::Member("lead".into()), &none, &mut events)
        .unwrap_err()
        .to_string();
    assert_eq!(error, "The lead has no task to ask about");
    let asked = apply(
        &mut team,
        "ask",
        &json!({"text":"  Must I keep the old field?  "}),
        &Actor::Member("dev".into()),
        &none,
        &mut events,
    )
    .unwrap();
    assert_eq!((asked["status"].as_str(), asked["question"].as_str()), (Some("asked"), Some("q1")));
    assert_eq!((asked["task"].as_u64(), asked["attempt"].as_u64()), (Some(1), Some(1)));
    assert_eq!(events.last().unwrap().kind, "question_asked");
    assert_eq!(events.last().unwrap().payload["by"], "Backend");
    let task = team.task(1).unwrap();
    assert_eq!(task.attempts[0].id, attempt);
    assert_eq!(task.attempts[0].questions[0].text, "Must I keep the old field?");
    assert_eq!(task.attempts[0].open_question().map(|question| question.id.as_str()), Some("q1"));
    assert_eq!(task.status, TaskStatus::InProgress);
    // A task without the caller's open attempt, and a member without one.
    let error = apply(&mut team, "ask", &json!({"task":2,"text":"Which?"}), &Actor::Member("dev".into()), &none, &mut events)
        .unwrap_err()
        .to_string();
    assert_eq!(error, "Task #2 has no open attempt or stage of yours");
    let error = apply(&mut team, "ask", &json!({"text":"Which?"}), &Actor::Member("qa".into()), &none, &mut events)
        .unwrap_err()
        .to_string();
    assert_eq!(error, "You have no open attempt or stage to ask about");
    // Two open attempts need a task number.
    team.task_mut(2).unwrap().assignee = "dev".into();
    hand_out(&mut team, 2);
    let error = apply(&mut team, "ask", &json!({"text":"Which?"}), &Actor::Member("dev".into()), &none, &mut events).unwrap_err().to_string();
    assert_eq!(error, "You have 2 open pieces of work: #1, #2; name one with \"task\"");
    let asked = apply(&mut team, "ask", &json!({"task":2,"text":"Which tests?"}), &Actor::Member("dev".into()), &none, &mut events).unwrap();
    assert_eq!((asked["question"].as_str(), asked["task"].as_u64()), (Some("q2"), Some(2)));
    // Only a running or paused team takes questions.
    team.status = TeamStatus::Done;
    let error = apply(&mut team, "ask", &json!({"text":"Which?"}), &Actor::Member("dev".into()), &none, &mut events)
        .unwrap_err()
        .to_string();
    assert_eq!(error, "The team is not running");
}

#[test]
fn question_bounds_are_refused_clearly() {
    let mut team = running_team();
    hand_out(&mut team, 1);
    let mut events = Vec::new();
    let none = BTreeSet::new();
    let ask = |team: &mut Team, events: &mut Vec<Event>, input: Value| {
        apply(team, "ask", &input, &Actor::Member("dev".into()), &none, events)
    };
    let reply = |team: &mut Team, events: &mut Vec<Event>, question: &str| {
        apply(team, "reply", &json!({"question":question,"text":"Keep it"}), &Actor::Member("lead".into()), &none, events)
    };
    let error = ask(&mut team, &mut events, json!({"text":"  "})).unwrap_err().to_string();
    assert_eq!(error, "A question needs text");
    let error = ask(&mut team, &mut events, json!({"text":"x".repeat(4001)})).unwrap_err().to_string();
    assert_eq!(error, "A question is too long");
    for _ in 0..3 {
        ask(&mut team, &mut events, json!({"text":"Which?"})).unwrap();
    }
    let error = ask(&mut team, &mut events, json!({"text":"Again?"})).unwrap_err().to_string();
    assert_eq!(error, "This attempt has 3 open questions; wait for the lead's answer");
    for id in 1..=3 {
        reply(&mut team, &mut events, &format!("q{id}")).unwrap();
    }
    // Questions are never removed: an attempt takes at most 20.
    for _ in 0..17 {
        ask(&mut team, &mut events, json!({"text":"More?"})).unwrap();
        let question = team.task(1).unwrap().attempts[0].questions.last().unwrap().id.clone();
        reply(&mut team, &mut events, &question).unwrap();
    }
    assert_eq!(team.task(1).unwrap().attempts[0].questions.len(), 20);
    let error = ask(&mut team, &mut events, json!({"text":"Too many?"})).unwrap_err().to_string();
    assert_eq!(error, "This attempt has 20 questions already (the most allowed)");
}

#[test]
fn the_lead_answers_a_question_once() {
    let mut team = running_team();
    hand_out(&mut team, 1);
    let mut events = Vec::new();
    let none = BTreeSet::new();
    apply(&mut team, "ask", &json!({"text":"Must I keep it?"}), &Actor::Member("dev".into()), &none, &mut events).unwrap();
    let error = apply(&mut team, "reply", &json!({"question":"q1","text":"Yes"}), &Actor::Member("dev".into()), &none, &mut events)
        .unwrap_err()
        .to_string();
    assert_eq!(error, "Only the lead or you answer a member's question");
    let error = apply(&mut team, "reply", &json!({"question":"q9","text":"Yes"}), &Actor::Human, &none, &mut events)
        .unwrap_err()
        .to_string();
    assert_eq!(error, "Unknown question q9");
    let error = apply(&mut team, "reply", &json!({"text":"Yes"}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
    assert_eq!(error, "A question id is required");
    let error = apply(&mut team, "reply", &json!({"question":"q1","text":"  "}), &Actor::Member("lead".into()), &none, &mut events)
        .unwrap_err()
        .to_string();
    assert_eq!(error, "An answer needs text");
    let answered = apply(
        &mut team,
        "reply",
        &json!({"question":"q1","text":"  Yes, keep it.  "}),
        &Actor::Human,
        &none,
        &mut events,
    )
    .unwrap();
    assert_eq!((answered["status"].as_str(), answered["question"].as_str()), (Some("answered"), Some("q1")));
    assert_eq!(events.last().unwrap().kind, "question_answered");
    assert_eq!(events.last().unwrap().payload["by"], "You");
    let question = &team.task(1).unwrap().attempts[0].questions[0];
    assert_eq!((question.answer.as_str(), question.by.as_str()), ("Yes, keep it.", "You"));
    assert!(question.answered_at.is_some());
    assert!(team.task(1).unwrap().attempts[0].open_question().is_none());
    let error = apply(&mut team, "reply", &json!({"question":"q1","text":"Again"}), &Actor::Human, &none, &mut events)
        .unwrap_err()
        .to_string();
    assert_eq!(error, "Question q1 was already answered");
}

#[test]
fn rejecting_with_hold_keeps_the_task_out_of_reach() {
    let mut team = running_team();
    let mut events = Vec::new();
    let none = BTreeSet::new();
    // A rejection without a hold returns the task as before.
    let attempt = hand_out(&mut team, 1);
    report(&mut team, &json!({"task":1,"attempt":attempt,"text":"Done"}), &Actor::Member("dev".into()), &mut events).unwrap();
    let rejected = apply(&mut team, "reject", &json!({"task":1,"reason":"Needs #2"}), &Actor::Member("lead".into()), &none, &mut events).unwrap();
    assert_eq!(rejected["status"], "pending");
    assert!(team.task(1).unwrap().hold.is_none());
    // Rejected again, held this time.
    let attempt = hand_out(&mut team, 1);
    report(&mut team, &json!({"task":1,"attempt":attempt,"text":"Done"}), &Actor::Member("dev".into()), &mut events).unwrap();
    let error = apply(
        &mut team,
        "reject",
        &json!({"task":1,"reason":"Later","hold":true,"final":true}),
        &Actor::Member("lead".into()),
        &none,
        &mut events,
    )
    .unwrap_err()
    .to_string();
    assert_eq!(error, "A final rejection cannot be held");
    let held = apply(&mut team, "reject", &json!({"task":1,"reason":"Wait for #2","hold":true}), &Actor::Member("lead".into()), &none, &mut events).unwrap();
    assert_eq!(held["status"], "pending");
    let task = team.task(1).unwrap();
    assert_eq!((task.status, task.attempts[1].state), (TaskStatus::Pending, AttemptState::Rejected));
    let hold = task.hold.as_ref().unwrap();
    assert_eq!((hold.by.as_str(), hold.reason.as_str()), ("Lead", "Wait for #2"));
    assert_eq!(events.iter().rev().take(2).map(|event| event.kind).collect::<Vec<_>>(), vec!["held", "rejected"]);
    assert_eq!(attempt_phase(task, false), Some("held"));
    // A held task is never ready.
    assert!(!plan::ready(&team).contains(&1));
    let released = apply(&mut team, "release", &json!({"task":1}), &Actor::Member("lead".into()), &none, &mut events).unwrap();
    assert_eq!(released["status"], "released");
    assert!(team.task(1).unwrap().hold.is_none());
    assert!(plan::ready(&team).contains(&1));
    assert_eq!(events.last().unwrap().kind, "released");
    let error = apply(&mut team, "release", &json!({"task":1}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
    assert_eq!(error, "Task #1 is not held");
    // The lead may hold a pending task so a busy member is not handed it.
    let held = apply(&mut team, "hold", &json!({"task":1,"reason":"Busy"}), &Actor::Member("lead".into()), &none, &mut events).unwrap();
    assert_eq!(held["status"], "held");
    assert_eq!(team.task(1).unwrap().hold.as_ref().unwrap().reason, "Busy");
    assert!(!plan::ready(&team).contains(&1));
    let error = apply(&mut team, "hold", &json!({"task":1}), &Actor::Member("lead".into()), &none, &mut events)
        .unwrap_err()
        .to_string();
    assert_eq!(error, "Task #1 is already held");
    // Only a pending task is held, and only by the lead or you.
    hand_out(&mut team, 2);
    let error = apply(&mut team, "hold", &json!({"task":2}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
    assert_eq!(error, "Only a pending task can be held");
    let error = apply(&mut team, "hold", &json!({"task":1}), &Actor::Member("dev".into()), &none, &mut events)
        .unwrap_err()
        .to_string();
    assert_eq!(error, "Only the lead or you hold a task");
    cancel(&mut team, &json!({"task":2}), &Actor::Human, &mut events).unwrap();
    let error = apply(&mut team, "hold", &json!({"task":2}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
    assert_eq!(error, "Task #2 is already decided");
}

#[test]
fn the_attempt_phase_follows_the_spec() {
    let mut team = running_team();
    let mut events = Vec::new();
    let none = BTreeSet::new();
    // No attempt, and a decided task, have no phase.
    assert_eq!(attempt_phase(team.task(2).unwrap(), false), None);
    hand_out(&mut team, 1);
    let phase = |team: &Team, stopped: bool| attempt_phase(team.task(1).unwrap(), stopped);
    assert_eq!(phase(&team, false), Some("delivered"));
    team.task_mut(1).unwrap().attempts[0].start_warned = true;
    assert_eq!(phase(&team, false), Some("late"));
    team.task_mut(1).unwrap().attempts[0].started_at = Some(5.0);
    assert_eq!(phase(&team, false), Some("working"));
    assert_eq!(phase(&team, true), Some("stopped"));
    team.task_mut(1).unwrap().attempts[0].no_report = true;
    assert_eq!(phase(&team, false), Some("no_report"));
    // An open question wins over working and late; the attempt stays open.
    apply(&mut team, "ask", &json!({"text":"Which JSON?"}), &Actor::Member("dev".into()), &none, &mut events).unwrap();
    assert_eq!(phase(&team, false), Some("asking"));
    apply(&mut team, "reply", &json!({"question":"q1","text":"The old one"}), &Actor::Member("lead".into()), &none, &mut events).unwrap();
    assert_eq!(phase(&team, false), Some("no_report"));
    let attempt = team.task(1).unwrap().attempts[0].id.clone();
    report(&mut team, &json!({"task":1,"attempt":attempt,"text":"Done"}), &Actor::Member("dev".into()), &mut events).unwrap();
    assert_eq!(phase(&team, false), Some("reported"));
    apply(&mut team, "accept", &json!({"task":1}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!(phase(&team, false), None);
    // The earlier states are the stored attempt states, not new enums.
    let mut sent = running_team();
    hand_out(&mut sent, 1);
    for (state, expected) in [
        (AttemptState::Delivering, Some("sent")),
        (AttemptState::Unconfirmed, Some("unconfirmed")),
        (AttemptState::Refused, None),
        (AttemptState::Revoked, None),
    ] {
        sent.task_mut(1).unwrap().attempts[0].state = state;
        assert_eq!(attempt_phase(sent.task(1).unwrap(), false), expected, "{state:?}");
    }
    // Rejected shows the rejection; a hold wins over it.
    let mut rejected = running_team();
    let attempt = hand_out(&mut rejected, 1);
    report(&mut rejected, &json!({"task":1,"attempt":attempt,"text":"Done"}), &Actor::Member("dev".into()), &mut events).unwrap();
    decide(&mut rejected, &json!({"task":1,"reason":"No"}), &Actor::Member("lead".into()), false, &mut events).unwrap();
    assert_eq!(attempt_phase(rejected.task(1).unwrap(), false), Some("rejected"));
    apply(&mut rejected, "hold", &json!({"task":1,"reason":"Wait"}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!(attempt_phase(rejected.task(1).unwrap(), false), Some("held"));
}

#[test]
fn a_captured_report_is_replaced_by_the_members_report() {
    let mut team = running_team();
    let attempt = hand_out(&mut team, 1);
    // What the dispatcher captures today: a progress line from a turn end.
    {
        let attempt = &mut team.task_mut(1).unwrap().attempts[0];
        attempt.state = AttemptState::Reported;
        attempt.report = Some(Report { text: "Building and running the tests...".into(), at: 20.0, captured: true, ..Default::default() });
        attempt.notice = Notice::Sent;
    }
    team.task_mut(1).unwrap().status = TaskStatus::Submitted;
    let mut events = Vec::new();
    let replaced = report(
        &mut team,
        &json!({"task":1,"attempt":attempt,"text":"  Done: the tests pass.  "}),
        &Actor::Member("dev".into()),
        &mut events,
    )
    .unwrap();
    assert_eq!((replaced["status"].as_str(), replaced["attempt"].as_u64()), (Some("submitted"), Some(1)));
    let value = &team.task(1).unwrap().attempts[0];
    let captured = value.report.as_ref().unwrap();
    assert_eq!(captured.text, "Done: the tests pass.");
    assert!(!captured.captured);
    assert_eq!(captured.replaced, vec![ReplacedReport { text: "Building and running the tests...".into(), at: 20.0 }]);
    assert_eq!(value.notice, Notice::Pending);
    assert_eq!(value.state, AttemptState::Reported);
    assert_eq!(team.task(1).unwrap().status, TaskStatus::Submitted);
    assert_eq!(events.last().unwrap().kind, "report_replaced");
    assert_eq!(events.last().unwrap().payload["by"], "Backend");
    // U42: while the task is undecided the member may replace it again, and
    // the text it replaces is kept for the lead.
    let again = report(&mut team, &json!({"task":1,"attempt":attempt,"text":"Again"}), &Actor::Member("dev".into()), &mut events).unwrap();
    assert_eq!(again["status"], "submitted");
    assert_eq!(team.task(1).unwrap().attempts[0].report.as_ref().unwrap().text, "Again");
    assert_eq!(events.last().unwrap().kind, "report_replaced");
    // Another member's report, and a decided task's, are stale.
    let foreign = report(&mut team, &json!({"task":1,"attempt":attempt,"text":"Mine"}), &Actor::Member("qa".into()), &mut events).unwrap();
    assert_eq!(foreign["status"], "stale");
    assert_eq!(team.task(1).unwrap().attempts[0].report.as_ref().unwrap().text, "Again");
    apply(&mut team, "accept", &json!({"task":1}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    let decided = report(&mut team, &json!({"task":1,"attempt":attempt,"text":"Late"}), &Actor::Member("dev".into()), &mut events).unwrap();
    assert_eq!(decided["status"], "stale");
    // At most the newest three replaced texts are kept.
    let mut team = running_team();
    let attempt = hand_out(&mut team, 1);
    {
        let attempt = &mut team.task_mut(1).unwrap().attempts[0];
        attempt.state = AttemptState::Reported;
        attempt.report = Some(Report { text: "Building".into(), at: 20.0, captured: true, ..Default::default() });
    }
    team.task_mut(1).unwrap().status = TaskStatus::Submitted;
    let mut events = Vec::new();
    report(&mut team, &json!({"task":1,"attempt":attempt,"text":"First"}), &Actor::Member("dev".into()), &mut events).unwrap();
    for n in 0..4 {
        team.task_mut(1).unwrap().attempts[0].report.as_mut().unwrap().captured = true;
        report(&mut team, &json!({"task":1,"attempt":attempt,"text":format!("Real {n}")}), &Actor::Member("dev".into()), &mut events).unwrap();
    }
    let captured = team.task(1).unwrap().attempts[0].report.as_ref().unwrap();
    assert_eq!(captured.text, "Real 3");
    let texts: Vec<&str> = captured.replaced.iter().map(|replaced| replaced.text.as_str()).collect();
    assert_eq!(texts, vec!["Real 0", "Real 1", "Real 2"]);
}

#[test]
fn findings_are_filed_reopened_and_decided() {
    let (mut team, _) = create("t", &json!({"name":"T","lead":{"id":"lead","agent":"claude"}})).unwrap();
    let mut events = Vec::new();
    plan(
        &mut team,
        &json!({"members":[{"id":"dev","role":"Backend","agent":"claude"},{"id":"qa","role":"Tester","agent":"claude"}],
            "tasks":[{"id":1,"title":"A","assignee":"dev"},{"id":2,"title":"B","assignee":"qa","deps":[{"task":1,"kind":"hard"}]}]}),
        &Actor::Human,
        &mut events,
    )
    .unwrap();
    approve(&mut team, &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    let attempt = hand_out(&mut team, 1);
    report(
        &mut team,
        &json!({"task":1,"attempt":attempt,"report":{"summary":"Done","findings":[
            {"severity":"critical","title":"Lost write","scenario":"two saves at once","task":1},
            {"severity":"normal","title":"Slow","scenario":"big log"},
            {"severity":"later","title":"Polish","scenario":"later"}]}}),
        &Actor::Member("dev".into()),
        &mut events,
    )
    .unwrap();
    // D2: nothing is filed until the report is accepted.
    assert!(team.findings.is_empty());
    apply(&mut team, "accept", &json!({"task":1}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    assert_eq!(team.findings.len(), 3);
    assert_eq!(
        (team.findings[0].id.as_str(), team.findings[0].severity.as_str(), team.findings[0].state.as_str()),
        ("f1", "critical", "open")
    );
    assert_eq!(team.findings[2].state, "deferred");
    assert!(events.iter().any(|event| event.kind == "findings_deferred"));
    // A critical finding is deferred only by the owner; fixed closes it.
    let error = apply(&mut team, "finding", &json!({"finding":"f1","action":"defer"}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut events)
        .unwrap_err()
        .to_string();
    assert!(error.contains("Only you defer a critical finding"), "{error}");
    apply(&mut team, "finding", &json!({"finding":"f1","action":"fixed","note":"fixed"}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut events).unwrap();
    assert_eq!(team.findings[0].state, "fixed");
    // Reopen: the accepted task returns, pinned, with the finding as its note.
    apply(&mut team, "finding", &json!({"finding":"f1","action":"reopen"}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    let task = team.task(1).unwrap();
    assert_eq!(task.status, TaskStatus::Pending);
    assert!(task.pinned);
    assert!(task.reopen_note.contains("Lost write"), "{}", task.reopen_note);
    assert_eq!((task.history.len(), task.decided_by), (1, None));
    // Urgent: a new approved phase with no barrier, handed out first.
    apply(
        &mut team,
        "finding",
        &json!({"finding":"f2","action":"urgent","phase_title":"Fix","tasks":[{"id":3,"title":"Fix fast","assignee":"qa"}]}),
        &Actor::Human,
        &BTreeSet::new(),
        &mut events,
    )
    .unwrap();
    let phase = team.phases.last().unwrap();
    assert_eq!((phase.state, phase.urgent, phase.barrier), (PhaseState::Approved, true, false));
    assert_eq!(plan::ready(&team)[0], 3);
    // Replan pauses the team with the owner's proposal.
    apply(&mut team, "finding", &json!({"finding":"f2","action":"replan","proposal":"Split the phase"}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    assert_eq!(team.status, TeamStatus::Paused);
    assert!(events.iter().any(|event| event.kind == "replan_proposed"));
}

#[test]
fn a_critical_finding_blocks_the_finish_unless_forced() {
    let mut team = running_team();
    let mut events = Vec::new();
    for id in [1, 2] {
        apply(&mut team, "accept", &json!({"task":id}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    }
    team.findings.push(Finding {
        id: "f1".into(),
        from_task: 1,
        severity: "critical".into(),
        title: "Lost write".into(),
        scenario: "two saves at once".into(),
        state: "open".into(),
        ..Default::default()
    });
    let error = apply(&mut team, "finish", &json!({"summary":"Done"}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap_err().to_string();
    assert!(error.contains("critical finding"), "{error}");
    let finished = apply(&mut team, "finish", &json!({"summary":"Done","force":true}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    assert_eq!(team.status, TeamStatus::Done);
    // D2: the result lists the open findings and how many were cut.
    assert_eq!(finished["findings"][0]["id"], "f1");
    assert_eq!(finished["findings"][0]["state"], "open");
    assert_eq!(finished["findings_more"], 0);
}

#[test]
fn a_structured_report_is_validated_and_kept() {
    let (mut team, _) = create("t", &json!({"name":"T","lead":{"id":"lead","agent":"claude"}})).unwrap();
    let mut events = Vec::new();
    plan(
        &mut team,
        &json!({"members":[{"id":"dev","role":"Backend","agent":"claude"}],
            "tasks":[{"id":1,"title":"A","assignee":"dev","criteria":["one","two"]}]}),
        &Actor::Human,
        &mut events,
    )
    .unwrap();
    approve(&mut team, &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    let attempt = hand_out(&mut team, 1);
    // D1: text becomes the summary; the object is kept with its evidence.
    report(
        &mut team,
        &json!({"task":1,"attempt":attempt,"report":{
            "summary":"Done: the guard is in.",
            "criteria":[{"n":1,"result":"pass","evidence":[0]}],
            "evidence":[{"kind":"command","caption":"cargo test teams","command":"cargo test --locked --bin hgs teams","exit":0,"tail":"ok"},
                {"kind":"text","caption":"note","text":"reads well"},
                {"kind":"url","caption":"log","url":"https://example.com/log"}],
            "limits":["the GUI was not rebuilt"]}}),
        &Actor::Member("dev".into()),
        &mut events,
    )
    .unwrap();
    let stored = team.task(1).unwrap().attempts[0].report.as_ref().unwrap();
    assert_eq!(stored.text, "Done: the guard is in.");
    assert!(!stored.unstructured);
    assert_eq!(stored.structured.as_ref().unwrap()["evidence"].as_array().unwrap().len(), 3);
    // Refusals: an unknown criterion, evidence kind, result or missing path.
    let attempt = hand_out(&mut team, 1);
    for (object, expected) in [
        (json!({"summary":"x","criteria":[{"n":9,"result":"pass"}]}), "has no criterion 9"),
        (json!({"summary":"x","evidence":[{"kind":"video","path":"a"}]}), "Unknown evidence kind video"),
        (json!({"summary":"x","criteria":[{"n":1,"result":"maybe"}]}), "A criterion result is pass, fail, partial or n/a"),
        (json!({"summary":"x","evidence":[{"kind":"file","caption":"no path"}]}), "Evidence 0 needs its path"),
        (json!({"summary":"x","criteria":[{"n":1,"result":"pass","evidence":[4]}]}), "cites evidence 4"),
    ] {
        let error = report(&mut team, &json!({"task":1,"attempt":attempt,"report":object}), &Actor::Member("dev".into()), &mut events)
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{error}");
    }
    // An unstructured report is still accepted and flagged as such.
    report(&mut team, &json!({"task":1,"attempt":attempt,"text":"plain"}), &Actor::Member("dev".into()), &mut events).unwrap();
    let stored = team.task(1).unwrap().attempts.last().unwrap().report.as_ref().unwrap();
    assert!(stored.unstructured);
    assert!(stored.structured.is_none());
}

#[test]
fn questions_close_with_their_attempt_or_task() {
    let mut team = running_team();
    let mut events = Vec::new();
    let none = BTreeSet::new();
    hand_out(&mut team, 1);
    apply(&mut team, "ask", &json!({"text":"Which database?"}), &Actor::Member("dev".into()), &none, &mut events).unwrap();
    assert!(team.task(1).unwrap().attempts[0].open_question().is_some());
    // The attempt ends with a rejection: its question closes and is never
    // answered or sent (N5).
    let attempt = team.task(1).unwrap().attempts[0].id.clone();
    report(&mut team, &json!({"task":1,"attempt":attempt,"text":"Done"}), &Actor::Member("dev".into()), &mut events).unwrap();
    decide(&mut team, &json!({"task":1,"reason":"No"}), &Actor::Member("lead".into()), false, &mut events).unwrap();
    let question = &team.task(1).unwrap().attempts[0].questions[0];
    assert!(!question.open() && question.answer.is_empty() && question.answered_at.is_some());
    assert_eq!((question.by.as_str(), question.notice), ("Zerus", Notice::None));
    let error = apply(&mut team, "reply", &json!({"question":"q1","text":"Yes"}), &Actor::Human, &none, &mut events)
        .unwrap_err()
        .to_string();
    assert!(error.contains("closed with its attempt"), "{error}");
    // A finished task closes its questions too, and reply names the task.
    let mut cancelled = running_team();
    hand_out(&mut cancelled, 1);
    apply(&mut cancelled, "ask", &json!({"text":"Which?"}), &Actor::Member("dev".into()), &none, &mut events).unwrap();
    apply(&mut cancelled, "cancel", &json!({"task":1}), &Actor::Human, &none, &mut events).unwrap();
    assert!(!cancelled.task(1).unwrap().attempts[0].questions[0].open());
    let error = apply(&mut cancelled, "reply", &json!({"question":"q1","text":"Yes"}), &Actor::Human, &none, &mut events)
        .unwrap_err()
        .to_string();
    assert_eq!(error, "Task #1 is decided; its question q1 is closed");
}

#[test]
fn an_escalated_question_takes_its_parents_answer() {
    let mut child = Team { id: "child".into(), name: "Child".into(), lead: "dev".into(), ..Default::default() };
    child.tasks.push(Task { id: 1, title: "Work".into(), assignee: "dev".into(), status: TaskStatus::InProgress, ..Default::default() });
    let mut attempt = Attempt { n: 1, id: "a1".into(), member: "dev".into(), state: AttemptState::Delivered, ..Default::default() };
    let mut question = Question { id: "q1".into(), text: "Which DB?".into(), at: 1.0, ..Default::default() };
    question.forwarded = forward_question(&child, &question);
    attempt.questions.push(question);
    child.tasks[0].attempts.push(attempt);
    let forwarded = child.tasks[0].attempts[0].questions[0].forwarded.clone();
    assert_eq!(forwarded, "sub:child:q1");
    assert!(answer_forwarded(&mut child, &forwarded, "Postgres").is_some());
    let question = &child.tasks[0].attempts[0].questions[0];
    assert_eq!((question.answer.as_str(), question.by.as_str()), ("Postgres", "Lead"));
    assert!(question.answered_at.is_some());
    // N21: the answer is pending for the member's dispatcher.
    assert_eq!(question.notice, Notice::Pending);
    assert!(answer_forwarded(&mut child, &forwarded, "again").is_none());
    // Section 9.7: a task with a sub-team asks how to treat it.
    let mut task = Task { id: 1, title: "Work".into(), assignee: "dev".into(), ..Default::default() };
    assert!(!subteam_stop_choice(&json!({}), &task).unwrap());
    task.subteam = Some("child".into());
    assert!(subteam_stop_choice(&json!({}), &task).is_err());
    assert!(subteam_stop_choice(&json!({"subteam":"stop"}), &task).unwrap());
    assert!(!subteam_stop_choice(&json!({"subteam":"keep"}), &task).unwrap());
}

#[test]
fn a_cancel_of_a_subteam_task_is_checked_before_anything_stops() {
    // C5: the choice and the caller are checked by the cancel itself; the
    // sub-team stops only after it is stored.
    let mut team = running_team();
    team.task_mut(1).unwrap().subteam = Some("child".into());
    let none = BTreeSet::new();
    let mut events = Vec::new();
    let stop = json!({"task":1,"subteam":"stop"});
    let error = apply(&mut team, "cancel", &stop, &Actor::Member("dev".into()), &none, &mut events).unwrap_err().to_string();
    assert!(error.contains("Only the lead or you cancel"), "{error}");
    let error = apply(&mut team, "cancel", &json!({"task":1}), &Actor::Member("lead".into()), &none, &mut events).unwrap_err().to_string();
    assert!(error.contains("has a sub-team"), "{error}");
    let error = apply(&mut team, "reassign", &json!({"task":1,"member":"qa"}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
    assert!(error.contains("has a sub-team"), "{error}");
    apply(&mut team, "cancel", &stop, &Actor::Member("lead".into()), &none, &mut events).unwrap();
}

#[test]
fn the_parent_lead_answers_a_question_forwarded_from_a_real_team_id() {
    // C7: with a UUID team id the forwarded id is longer than 40 characters.
    let mut team = running_team();
    let child = Team { id: "0b6f3f0e-5d6c-4b1e-9b8a-2f4c6d8e0a11".into(), ..Default::default() };
    let id = forward_question(&child, &Question { id: "q12".into(), ..Default::default() });
    assert!(id.len() > 40);
    let mut attempt = Attempt { n: 1, id: "a1".into(), member: "dev".into(), state: AttemptState::Delivered, ..Default::default() };
    attempt.questions.push(Question { id: id.clone(), text: "Which DB?".into(), at: 1.0, ..Default::default() });
    team.task_mut(1).unwrap().attempts.push(attempt);
    let mut events = Vec::new();
    apply(&mut team, "reply", &json!({"question":id,"text":"Postgres"}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut events).unwrap();
    assert_eq!(team.task(1).unwrap().attempts[0].questions[0].answer, "Postgres");
}

#[test]
fn only_a_finding_makes_an_urgent_phase_and_only_the_owner_forces_a_finish() {
    // C15: under review approval a phase the lead adds stays a draft, even
    // when it says "urgent".
    let mut team = running_team();
    let none = BTreeSet::new();
    let mut events = Vec::new();
    assert_eq!(phase_approval(&team), Approval::Review);
    apply(
        &mut team,
        "extend",
        &json!({"urgent":true,"phase_title":"Now","tasks":[{"id":3,"title":"Skip the owner","assignee":"dev"}]}),
        &Actor::Member("lead".into()),
        &none,
        &mut events,
    )
    .unwrap();
    let phase = team.phases.last().unwrap();
    assert_eq!((phase.state, phase.urgent), (PhaseState::Draft, false));
    // C16: an open critical finding blocks the lead's finish, forced or not.
    let mut team = running_team();
    for id in [1, 2] {
        apply(&mut team, "accept", &json!({"task":id}), &Actor::Human, &none, &mut events).unwrap();
    }
    team.findings.push(Finding { id: "f1".into(), severity: "critical".into(), title: "Lost write".into(), scenario: "s".into(), state: "open".into(), ..Default::default() });
    let error = apply(&mut team, "finish", &json!({"summary":"Done","force":true}), &Actor::Member("lead".into()), &none, &mut events)
        .unwrap_err()
        .to_string();
    assert!(error.contains("critical finding"), "{error}");
    apply(&mut team, "finish", &json!({"summary":"Done","force":true}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!(team.status, TeamStatus::Done);
}
