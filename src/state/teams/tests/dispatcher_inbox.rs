use super::super::ops::{self, Actor};
use super::*;
use super::support::*;

/// U56: keep_assignees pins every planned assignee: a busy assignee's task
/// waits for it instead of going to a free member of its role.
#[test]
fn keep_assignees_keeps_a_task_with_its_planned_member() {
    let (mut team, seen) = team_with_two_backends();
    // dev works on #1; #2, also dev's, would go to the free dev2.
    team.task_mut(1).unwrap().attempts.push(Attempt { n: 1, member: "dev".into(), state: AttemptState::Delivered, ..Default::default() });
    team.task_mut(1).unwrap().status = TaskStatus::InProgress;
    let moved = |team: &Team| steps(team, &seen, 1.0).iter().any(|step| matches!(step, Step::Rebalance { .. }));
    // Off (the default), the free same-role member takes it, as today.
    assert!(moved(&team));
    team.settings.keep_assignees = true;
    assert!(!moved(&team), "{:?}", steps(&team, &seen, 1.0));
    assert!(team.keeps_assignee(team.task(2).unwrap()));
    // A task for a role has no planned assignee to keep.
    let role_task = Task { id: 9, role: "Backend".into(), ..Default::default() };
    assert!(!team.keeps_assignee(&role_task));
}

/// U56: the lead's notices name a member by id with its role.
#[test]
fn notices_name_members_by_id_and_role() {
    let mut team = team();
    assert_eq!((team.name_of("dev"), team.name_of("lead"), team.name_of("gone")), ("dev (Backend)".into(), "Lead".into(), "gone".into()));
    delivered(&mut team);
    let attempt = team.task(1).unwrap().attempts[0].clone();
    let task = team.task(1).unwrap().clone();
    assert!(start_late_text(&team, &task, &attempt, 1000.0).contains("delivered to dev (Backend)"));
    assert!(blocked_text(&team, &task, &attempt, 1000.0).contains("dev (Backend)'s turn"));
    team.members.iter_mut().find(|member| member.id == "dev").unwrap().starved_since = Some(10.0);
    let member = team.member("dev").unwrap().clone();
    assert!(starved_text(&team, &member, 1210.0).contains("dev (Backend) has had no ready task"));
    team.task_mut(1).unwrap().attempts[0].no_report = true;
    let items = inbox::items(&team);
    let stalled = items.iter().find(|item| item.kind == "stalled").expect("a stalled item");
    assert_eq!(stalled.member, "dev (Backend)");
    assert!(stalled.summary.starts_with("dev (Backend) ended its turn"), "{}", stalled.summary);
    assert!(inbox::text(&team, &items, 1000.0).contains("dev (Backend)"));
}

#[test]
fn a_closed_question_is_never_sent() {
    let mut team = team();
    delivered(&mut team);
    let mut events = Vec::new();
    ops::apply(&mut team, "ask", &json!({"text":"Which database?"}), &Actor::Member("dev".into()), &BTreeSet::new(), &mut events).unwrap();
    // The task is cancelled: the question closes with the attempt (N5).
    ops::apply(&mut team, "cancel", &json!({"task":1}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    let question = &team.task(1).unwrap().attempts[0].questions[0];
    assert!(!question.open() && question.answer.is_empty() && question.answered_at.is_some());
    let sent = steps(&team, &[session("claude/x/lead", "r0", true), session("codex/x/dev", "r1", true)], 100.0);
    assert!(!sent.iter().any(|step| matches!(step, Step::AnswerQuestion { .. })), "{sent:?}");
    // #759: the lead hears only that #1's cancellation released #2, never the
    // closed question.
    let items = inbox::items(&team);
    assert!(items.iter().all(|item| item.kind != "question"), "{items:?}");
    assert!(items.iter().all(|item| item.kind == "dependency_cancelled"), "{items:?}");
}

#[test]
fn a_long_blocked_turn_reaches_the_lead_once() {
    let mut team = team();
    delivered(&mut team);
    let timeout = team.settings.blocked_timeout_seconds as f64;
    {
        let attempt = &mut team.task_mut(1).unwrap().attempts[0];
        attempt.turn_ended_at = Some(50.0);
        attempt.turn_reply = "r7".into();
        attempt.blocked_since = Some(50.0);
    }
    let mut quiet = session("codex/x/dev", "r1", true);
    quiet.reply_at = Some(50.0);
    quiet.reply_id = "r7".into();
    let mut background = quiet.clone();
    background.background = true;
    let lead = session("claude/x/lead", "r0", true);
    // Background work blocks the turn; the timeout is 30 min by default.
    assert!(!steps(&team, &[lead.clone(), background.clone()], 49.0 + timeout).iter().any(|s| matches!(s, Step::BlockedLate { .. })));
    assert!(steps(&team, &[lead.clone(), background], 50.0 + timeout).contains(&Step::BlockedLate { task: 1 }));
    // Told once.
    team.task_mut(1).unwrap().attempts[0].told_lead.push("blocked".into());
    assert!(!steps(&team, &[lead.clone(), quiet.clone()], 50.0 + timeout).iter().any(|s| matches!(s, Step::BlockedLate { .. })));
    // A question is named in the message.
    team.task_mut(1).unwrap().attempts[0].questions.push(Question { id: "q1".into(), text: "Which database?".into(), at: 60.0, ..Default::default() });
    let task = team.task(1).unwrap();
    let text = blocked_text(&team, task, &task.attempts[0], 50.0 + timeout);
    assert!(text.contains("Which database?") && text.contains("hgs team reply --json"), "{text}");
    assert!(text.contains("min"), "{text}");
}

#[test]
fn a_question_before_the_turn_end_starts_the_block() {
    let mut team = team();
    delivered(&mut team);
    // The lead was already told of the question.
    team.task_mut(1).unwrap().attempts[0].questions.push(Question { id: "q1".into(), text: "Which database?".into(), at: 40.0, ..Default::default() });
    team.task_mut(1).unwrap().attempts[0].told_lead.push("question:q1".into());
    // The member asked, then ended its turn: idle, with a reply after the hand-off.
    let mut asked = session("codex/x/dev", "r1", true);
    asked.reply_at = Some(50.0);
    asked.reply_id = "r7".into();
    let lead = session("claude/x/lead", "r0", true);
    let first = steps(&team, &[lead.clone(), asked.clone()], 60.0);
    assert!(first.contains(&Step::GateClosed { task: 1 }), "{first:?}");
    assert!(!first.iter().any(|s| matches!(s, Step::TurnEnded { .. })));
    // Once the block is recorded, a long wait reaches the lead.
    let timeout = team.settings.blocked_timeout_seconds as f64;
    team.task_mut(1).unwrap().attempts[0].blocked_since = Some(60.0);
    assert!(!steps(&team, &[lead.clone(), asked.clone()], 59.0 + timeout).iter().any(|s| matches!(s, Step::BlockedLate { .. })));
    let late = steps(&team, &[lead.clone(), asked.clone()], 60.0 + timeout);
    assert!(late.contains(&Step::BlockedLate { task: 1 }), "{late:?}");
    // Working again clears it.
    let mut busy = asked.clone();
    busy.idle = false;
    busy.free = false;
    assert!(steps(&team, &[lead, busy], 70.0).contains(&Step::WorkResumed { task: 1 }));
}

#[test]
fn a_lead_with_its_own_background_work_still_gets_the_inbox_and_the_reminder() {
    // U18: a lead whose turn ended but whose own subagent or Background job
    // runs takes messages (CLAUDE.md): its inbox and the report reminder
    // must not wait for the job; a lead in its own turn gets neither.
    let mut lead = session("claude/x/lead", "r0", true);
    lead.free = false;
    lead.idle = false;
    lead.activity = "background".into();
    lead.background = true;
    // An open question waits for the lead while its own Background job runs.
    let mut question_team = team();
    {
        let task = question_team.task_mut(2).unwrap();
        task.attempts.push(Attempt {
            n: 1,
            id: "A2".into(),
            member: "dev".into(),
            state: AttemptState::Delivered,
            sent_at: 10.0,
            delivered_at: Some(10.0),
            questions: vec![Question { id: "q1".into(), text: "Which?".into(), at: 15.0, ..Default::default() }],
            ..Default::default()
        });
        task.status = TaskStatus::InProgress;
    }
    let found = steps(&question_team, &[lead.clone()], 20.0);
    assert!(found.iter().any(|step| matches!(step, Step::Inbox)), "{found:?}");
    // An old report reminds the lead even with its Background job running.
    let mut report_team = team();
    {
        let task = report_team.task_mut(1).unwrap();
        task.attempts.push(Attempt {
            n: 1,
            id: "A1".into(),
            member: "dev".into(),
            state: AttemptState::Reported,
            report: Some(Report { text: "Done".into(), at: 5.0, ..Default::default() }),
            ..Default::default()
        });
        task.status = TaskStatus::Submitted;
    }
    let late = 5.0 + report_team.settings.remind_lead_minutes as f64 * 60.0 + 60.0;
    let found = steps(&report_team, &[lead.clone()], late);
    assert!(found.iter().any(|step| matches!(step, Step::RemindReport { task: 1 })), "{found:?}");
    // A lead in its own turn gets the inbox (it takes queued input between
    // tool calls), not the reminder; one asking on its screen gets neither.
    let mut busy = lead;
    busy.background = false;
    busy.activity = "busy".into();
    busy.phase = "working".into();
    let found = steps(&report_team, &[busy.clone()], late);
    assert!(found.iter().any(|step| matches!(step, Step::Inbox)), "{found:?}");
    assert!(!found.iter().any(|step| matches!(step, Step::RemindReport { .. })), "{found:?}");
    busy.asking = true;
    let found = steps(&report_team, &[busy], late);
    assert!(!found.iter().any(|step| matches!(step, Step::Inbox | Step::RemindReport { .. })), "{found:?}");
}

#[test]
fn an_urgent_task_goes_before_the_members_other_ready_tasks() {
    // N11 (spec 8): dispatch_order keeps ready's urgent-first order. The
    // urgent phase is the one a finding's "urgent" action appends.
    let mut team = team();
    let mut events = Vec::new();
    team.findings.push(crate::state::teams::model::Finding {
        id: "f1".into(),
        severity: "critical".into(),
        title: "Lost write".into(),
        scenario: "two saves".into(),
        state: "open".into(),
        ..Default::default()
    });
    ops::apply(
        &mut team,
        "finding",
        &json!({"finding":"f1","action":"urgent","tasks":[{"id":4,"title":"Hotfix","assignee":"dev"}]}),
        &Actor::Human,
        &BTreeSet::new(),
        &mut events,
    )
    .unwrap();
    let hotfix = team.tasks.iter().find(|task| task.title == "Hotfix").unwrap().id;
    assert_eq!(plan::ready(&team)[0], hotfix, "{:?}", team.phases);
    let seen = vec![session("claude/x/lead", "r0", true), session("codex/x/dev", "r1", true)];
    let found = steps(&team, &seen, 100.0);
    assert!(found.contains(&Step::HandOut { task: hotfix, member: "dev".into() }), "{found:?}");
    assert!(!found.contains(&Step::HandOut { task: 1, member: "dev".into() }), "{found:?}");
}

#[test]
fn a_paused_teams_lead_hears_one_message_a_tick() {
    // N35 (spec 2.5): the inbox and the late-report reminder do not both
    // go out in one tick because the team is paused.
    let mut team = team();
    team.status = TeamStatus::Paused;
    {
        let task = team.task_mut(1).unwrap();
        task.attempts.push(Attempt {
            n: 1,
            id: "A1".into(),
            member: "dev".into(),
            state: AttemptState::Reported,
            notice: Notice::Pending,
            report: Some(crate::state::teams::model::Report { text: "Done".into(), at: 10.0, ..Default::default() }),
            ..Default::default()
        });
        task.status = TaskStatus::Submitted;
    }
    {
        // A turn blocked far longer than the blocked warning allows.
        let task = team.task_mut(2).unwrap();
        task.attempts.push(Attempt {
            n: 1,
            id: "A2".into(),
            member: "dev".into(),
            state: AttemptState::Delivered,
            sent_at: 10.0,
            delivered_at: Some(10.0),
            blocked_since: Some(20.0),
            ..Default::default()
        });
        task.status = TaskStatus::InProgress;
    }
    let lead = session("claude/x/lead", "r0", true);
    let found = steps(&team, &[lead, session("codex/x/dev", "r1", false)], 100_000.0);
    let lead_messages = found
        .iter()
        .filter(|step| matches!(step, Step::Inbox | Step::RemindReport { .. } | Step::BlockedLate { .. } | Step::TellStartLate { .. }))
        .count();
    assert_eq!(lead_messages, 1, "{found:?}");
}

#[test]
fn a_refused_hand_out_keeps_the_finding_for_the_next_one() {
    // N16: the reopen note goes with the hand-out that is delivered.
    let dir = tempfile::tempdir().unwrap();
    let (mut store, t) = reopen_store(dir.path());
    let sent = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    let record = sent.clone();
    let refuse = std::rc::Rc::new(std::cell::Cell::new(true));
    let refusing = refuse.clone();
    set_fake_hgs(Some(Box::new(move |_, args: &[&str], input: Option<&Value>| {
        assert_eq!(args.first().copied(), Some("send"));
        record.borrow_mut().push(input.map(|input| input["text"].as_str().unwrap_or_default().to_owned()).unwrap_or_default());
        Ok(if refusing.get() { fake_output(1, "", "hgs: the session is busy") } else { fake_output(0, r#"{"status":"submitted"}"#, "") })
    })));
    let result = hand_out(&mut store, &t.id, 1, "dev", None, "");
    let after_refusal = store.load(&t.id).unwrap().unwrap();
    refuse.set(false);
    let second = hand_out(&mut store, &t.id, 1, "dev", None, "");
    set_fake_hgs(None);
    result.unwrap();
    second.unwrap();
    let task = after_refusal.task(1).unwrap();
    assert_eq!(task.attempts.last().unwrap().state, AttemptState::Refused);
    assert!(task.reopen_note.contains("Finding f1"), "{task:?}");
    let sent = sent.borrow();
    assert_eq!(sent.len(), 2);
    assert!(sent[0].contains("Finding f1") && sent[1].contains("Finding f1"), "{sent:?}");
    // Delivered once, the finding is not sent again.
    let saved = store.load(&t.id).unwrap().unwrap();
    assert_eq!(saved.task(1).unwrap().attempts.last().unwrap().state, AttemptState::Delivered);
    assert!(saved.task(1).unwrap().reopen_note.is_empty());
}

#[test]
fn a_queued_report_repairs_a_placeholder_while_the_task_is_undecided() {
    // U42 (#512): the QA's whole report reached the worker after the queued
    // placeholder had closed its attempt. A queued report now reaches that
    // attempt while the lead has not decided.
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let mut t = team();
        {
            let task = t.task_mut(1).unwrap();
            task.attempts.push(Attempt {
                n: 1,
                id: "A1".into(),
                member: "dev".into(),
                state: AttemptState::Reported,
                ..Default::default()
            });
            task.attempts[0].report = Some(Report { text: "poll".into(), at: 1.0, unstructured: true, ..Default::default() });
            task.status = TaskStatus::Submitted;
        }
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
        let saved = store.load(&t.id).unwrap().unwrap();
        let dev = saved.member("dev").unwrap().clone();
        let request = json!({"op":"report","request_id":"44444444-4444-4444-8444-444444444444","team":t.id,"member":"dev","attempt":"A1",
            "input":{"request_id":"44444444-4444-4444-8444-444444444444","team":t.id,"task":1,"attempt":"A1",
                "text":"The whole report: 512 checks and their totals"}});
        let outcome = apply_outbox(&mut store, &saved, &dev, &request).unwrap();
        assert_eq!(outcome["status"], "submitted");
        let saved = store.load(&t.id).unwrap().unwrap();
        let report = saved.task(1).unwrap().attempts[0].report.as_ref().unwrap();
        assert_eq!(report.text, "The whole report: 512 checks and their totals");
        assert_eq!(report.replaced[0].text, "poll");
        // Once the lead decides, the queue refuses the attempt again.
        store
            .mutate(None, &t.id, None, |team, _| {
                team.task_mut(1).unwrap().status = TaskStatus::Accepted;
                Ok(Value::Null)
            })
            .unwrap();
        let saved = store.load(&t.id).unwrap().unwrap();
        let later = json!({"op":"report","request_id":"55555555-5555-4555-8555-555555555555","team":t.id,"member":"dev","attempt":"A1",
            "input":{"request_id":"55555555-5555-4555-8555-555555555555","team":t.id,"task":1,"attempt":"A1","text":"later"}});
        let error = apply_outbox(&mut store, &saved, &dev, &later).unwrap_err().to_string();
        assert!(error.contains("does not run for this member"), "{error}");
    });
}

#[test]
fn a_queued_budget_is_applied_as_the_members_own() {
    // U42: `hgs team expect` from a sandbox queues like a report. The worker
    // applies it as the member's own budget, with no attempt to name; the
    // folder still proves the member.
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let t = team();
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
        let saved = store.load(&t.id).unwrap().unwrap();
        let dev = saved.member("dev").unwrap().clone();
        let request = json!({"op":"expect","request_id":"33333333-3333-4333-8333-333333333333","team":t.id,"member":"dev","attempt":"",
            "input":{"request_id":"33333333-3333-4333-8333-333333333333","team":t.id,"label":"the suites","minutes":15}});
        let outcome = apply_outbox(&mut store, &saved, &dev, &request).unwrap();
        assert_eq!((outcome["status"].as_str(), outcome["label"].as_str()), (Some("expected"), Some("the suites")));
        let saved = store.load(&t.id).unwrap().unwrap();
        let budget = saved.member("dev").unwrap().budget.as_ref().unwrap();
        assert_eq!((budget.label.as_str(), budget.minutes, budget.red_minutes), ("the suites", 15, 30));
        let events = store.events(&t.id, None, 20).unwrap();
        assert!(
            events["items"].as_array().unwrap().iter().any(|event| event["kind"] == "budget_declared"),
            "{events}"
        );
        // The folder proves the member: another member's request is refused.
        let qa = saved.member("qa").unwrap().clone();
        let error = apply_outbox(&mut store, &saved, &qa, &request).unwrap_err().to_string();
        assert!(error.contains("the folder belongs to"), "{error}");
    });
}
