use super::super::ops::{self, Actor};
use super::*;
use super::support::*;

#[test]
fn a_hand_out_that_never_starts_is_warned_once() {
    let mut team = team();
    delivered(&mut team);
    let timeout = team.settings.start_timeout_seconds as f64;
    // An idle session whose last reply predates the hand-off has not started.
    let mut quiet = session("codex/x/dev", "r1", true);
    quiet.reply_at = Some(5.0);
    assert!(!steps(&team, &[quiet.clone()], 10.0 + timeout - 1.0).iter().any(|s| matches!(s, Step::StartLate { .. })));
    assert!(steps(&team, &[quiet.clone()], 10.0 + timeout).contains(&Step::StartLate { task: 1 }));
    // The warning is set once; Zerus never re-sends or presses keys.
    team.task_mut(1).unwrap().attempts[0].start_warned = true;
    let warned = steps(&team, &[quiet.clone()], 300.0);
    assert!(!warned.iter().any(|s| matches!(s, Step::StartLate { .. } | Step::Started { .. })), "{warned:?}");
    // Once the lead is free it hears about it, once.
    let lead = session("claude/x/lead", "r0", true);
    let mut heard = team.clone();
    assert!(steps(&heard, &[lead.clone(), quiet.clone()], 300.0).contains(&Step::TellStartLate { task: 1 }));
    heard.task_mut(1).unwrap().attempts[0].told_lead.push("start_late".into());
    assert!(!steps(&heard, &[lead, quiet.clone()], 300.0).iter().any(|s| matches!(s, Step::TellStartLate { .. })));
    // The message names the task, the member and the wait.
    let task = team.task(1).unwrap();
    let text = start_late_text(&team, task, &task.attempts[0], 10.0 + timeout);
    assert!(text.contains("#1") && text.contains("Backend") && text.contains("min ago"), "{text}");
    assert!(text.contains("has not started"), "{text}");
    // A custom timeout is respected.
    let mut earlier = self::team();
    delivered(&mut earlier);
    earlier.settings.start_timeout_seconds = 30;
    assert!(steps(&earlier, &[quiet], 41.0).contains(&Step::StartLate { task: 1 }));
}

#[test]
fn a_started_attempt_is_recorded_from_the_session() {
    let mut team = team();
    delivered(&mut team);
    // A busy member started.
    assert!(steps(&team, &[session("codex/x/dev", "r1", false)], 20.0).contains(&Step::Started { task: 1 }));
    // A reply newer than the hand-off started it too.
    let mut replied = session("codex/x/dev", "r1", true);
    replied.reply_at = Some(15.0);
    assert!(steps(&team, &[replied], 20.0).contains(&Step::Started { task: 1 }));
    // A turn that began after the hand-off counts even with no reply yet.
    let mut turned = session("codex/x/dev", "r1", false);
    turned.turn_started = Some(12.0);
    assert!(steps(&team, &[turned], 20.0).contains(&Step::Started { task: 1 }));
    // A stopped session is not a start.
    let mut stopped = session("codex/x/dev", "r1", false);
    stopped.live = false;
    assert!(!steps(&team, &[stopped], 200.0).iter().any(|s| matches!(s, Step::Started { .. })));
    // Recorded once, and never warned after a start.
    team.task_mut(1).unwrap().attempts[0].started_at = Some(12.0);
    let started = steps(&team, &[session("codex/x/dev", "r1", false)], 500.0);
    assert!(!started.iter().any(|s| matches!(s, Step::Started { .. } | Step::StartLate { .. })), "{started:?}");
}

#[test]
fn a_turn_end_is_recorded_only_for_a_quiet_session() {
    let mut team = team();
    delivered(&mut team);
    let mut idle = session("codex/x/dev", "r1", true);
    idle.reply_at = Some(50.0);
    idle.reply_id = "r7".into();
    // A plain finished turn is recorded with the reply's identity.
    assert!(steps(&team, &[idle.clone()], 60.0).contains(&Step::TurnEnded { task: 1, at: 50.0, reply: "r7".into() }));
    // A question, input or approval the member waits on is not a turn end.
    for phase in ["question", "input", "approval"] {
        let mut asking = idle.clone();
        asking.phase = phase.into();
        assert!(!steps(&team, &[asking], 60.0).iter().any(|s| matches!(s, Step::TurnEnded { .. })), "{phase}");
    }
    // Background work is not a finished turn either.
    let mut background = idle.clone();
    background.background = true;
    assert!(!steps(&team, &[background], 60.0).iter().any(|s| matches!(s, Step::TurnEnded { .. })));
    // Nor is a turn end recorded while the attempt has an open question.
    team.task_mut(1).unwrap().attempts[0].questions.push(Question { id: "q1".into(), text: "Which?".into(), at: 40.0, ..Default::default() });
    assert!(!steps(&team, &[idle], 60.0).iter().any(|s| matches!(s, Step::TurnEnded { .. })));
}

#[test]
fn the_report_timeout_holds_with_a_question_or_background_work() {
    let mut team = team();
    delivered(&mut team);
    let timeout = team.settings.report_timeout_seconds as f64;
    {
        let attempt = &mut team.task_mut(1).unwrap().attempts[0];
        attempt.turn_ended_at = Some(50.0);
        attempt.turn_reply = "r7".into();
        // The gate was first seen open at 50: the timeout counts from there.
        attempt.report_ready_at = Some(50.0);
    }
    let mut quiet = session("codex/x/dev", "r1", true);
    quiet.reply_at = Some(50.0);
    quiet.reply_id = "r7".into();
    // A plain finished turn is No report after its timeout.
    assert!(!steps(&team, &[quiet.clone()], 50.0 + timeout - 1.0).iter().any(|s| matches!(s, Step::NoReport { .. })));
    assert!(steps(&team, &[quiet.clone()], 50.0 + timeout).contains(&Step::NoReport { task: 1 }));
    // An open question from the member holds the timeout: it stays a question.
    for phase in ["question", "input", "approval"] {
        let mut asking = quiet.clone();
        asking.phase = phase.into();
        assert!(!steps(&team, &[asking], 50.0 + timeout).iter().any(|s| matches!(s, Step::NoReport { .. })), "{phase}");
    }
    // Background work holds it too.
    let mut background = quiet.clone();
    background.background = true;
    assert!(!steps(&team, &[background], 50.0 + timeout).iter().any(|s| matches!(s, Step::NoReport { .. })));
    // An open team question on the attempt holds it, until the lead answers.
    team.task_mut(1).unwrap().attempts[0].questions.push(Question { id: "q1".into(), text: "Which?".into(), at: 40.0, ..Default::default() });
    assert!(!steps(&team, &[quiet.clone()], 50.0 + timeout).iter().any(|s| matches!(s, Step::NoReport { .. })));
    team.task_mut(1).unwrap().attempts[0].questions[0].answer = "The old one".into();
    assert!(steps(&team, &[quiet.clone()], 50.0 + timeout).contains(&Step::NoReport { task: 1 }));
    // The member works again: its turn end is cleared, not taken as a report.
    let mut busy = quiet.clone();
    busy.idle = false;
    assert!(steps(&team, &[busy], 50.0 + timeout).contains(&Step::WorkResumed { task: 1 }));
    let mut newer = quiet;
    newer.reply_id = "r8".into();
    newer.reply_at = Some(70.0);
    assert!(steps(&team, &[newer], 50.0 + timeout).contains(&Step::WorkResumed { task: 1 }));
}

#[test]
fn a_long_background_job_does_not_become_no_report_while_it_runs() {
    let mut team = team();
    delivered(&mut team);
    let timeout = team.settings.report_timeout_seconds as f64;
    // The turn ended at 50, but a Background job still runs, so no gate.
    {
        let attempt = &mut team.task_mut(1).unwrap().attempts[0];
        attempt.turn_ended_at = Some(50.0);
        attempt.turn_reply = "r7".into();
    }
    let mut quiet = session("codex/x/dev", "r1", true);
    quiet.reply_at = Some(50.0);
    quiet.reply_id = "r7".into();
    let mut working = quiet.clone();
    working.background = true;
    // The gate was never open: the closed gate is recorded, not a timeout.
    assert!(steps(&team, &[working.clone()], 60.0).contains(&Step::GateClosed { task: 1 }));
    assert!(!steps(&team, &[working], 1000.0).iter().any(|s| matches!(s, Step::NoReport { .. })));
    // It was recorded at 60; when the job ends at 1000 the gate opens then.
    team.task_mut(1).unwrap().attempts[0].blocked_since = Some(60.0);
    let found = steps(&team, &[quiet.clone()], 1000.0);
    assert!(found.contains(&Step::GateOpen { task: 1 }), "{found:?}");
    assert!(!found.iter().any(|s| matches!(s, Step::NoReport { .. })));
    // The timeout counts from 1000, not from the reply at 50.
    team.task_mut(1).unwrap().attempts[0].report_ready_at = Some(1000.0);
    assert!(!steps(&team, &[quiet.clone()], 1000.0 + timeout - 1.0).iter().any(|s| matches!(s, Step::NoReport { .. })));
    assert!(steps(&team, &[quiet], 1000.0 + timeout).contains(&Step::NoReport { task: 1 }));
}

/// Owner, 2026-11-07: a turn that ends without `hgs team report` is No report.
/// Zerus does not store the member's last reply as the report.
#[test]
fn a_turn_that_ends_without_a_report_is_no_report_not_a_captured_report() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut team = team();
    team.status = TeamStatus::Running;
    let task = team.tasks.iter_mut().find(|task| task.assignee == "dev").unwrap();
    task.attempts.push(Attempt {
        n: 1,
        id: "A1".into(),
        member: "dev".into(),
        state: AttemptState::Delivered,
        sent_at: 10.0,
        delivered_at: Some(10.0),
        started_at: Some(11.0),
        turn_ended_at: Some(50.0),
        turn_reply: "r7".into(),
        report_ready_at: Some(50.0),
        ..Default::default()
    });
    task.status = TaskStatus::InProgress;
    let id = task.id;
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), team.clone(), vec![]).unwrap();
    inspect_override(Some(json!({"reply_id": "r7", "reply_at": 50.0, "last_message": "Done: checked"})));
    run_step(&mut store, &team, &Step::NoReport { task: id }, "").unwrap();
    inspect_override(None);
    let saved = store.load(&team.id).unwrap().unwrap();
    let attempt = saved.task(id).unwrap().attempts.last().unwrap();
    assert!(attempt.report.is_none(), "the last reply was stored as the report: {:?}", attempt.report);
    assert!(attempt.no_report);
    assert_eq!(attempt.state, AttemptState::Delivered);
    assert_eq!(saved.task(id).unwrap().status, TaskStatus::InProgress);
    // The reply is kept for the lead to read and to use explicitly.
    assert_eq!(attempt.last_reply, "Done: checked");
}

#[test]
fn the_last_reply_of_the_finished_turn_is_kept_for_the_lead() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut team = team();
    team.status = TeamStatus::Running;
    let task = team.tasks.iter_mut().find(|task| task.assignee == "dev").unwrap();
    task.attempts.push(Attempt {
        n: 1,
        id: "A1".into(),
        member: "dev".into(),
        state: AttemptState::Delivered,
        sent_at: 10.0,
        delivered_at: Some(10.0),
        started_at: Some(11.0),
        ..Default::default()
    });
    task.status = TaskStatus::InProgress;
    let id = task.id;
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), team.clone(), vec![]).unwrap();
    inspect_override(Some(json!({"reply_id": "r7", "reply_at": 50.0, "last_message": "  Done: checked.  "})));
    run_step(&mut store, &team, &Step::TurnEnded { task: id, at: 50.0, reply: "r7".into() }, "").unwrap();
    inspect_override(None);
    let saved = store.load(&team.id).unwrap().unwrap();
    let attempt = saved.task(id).unwrap().attempts.last().unwrap();
    assert_eq!(attempt.turn_ended_at, Some(50.0));
    assert_eq!(attempt.turn_reply, "r7");
    assert_eq!(attempt.last_reply, "Done: checked.");
    assert!(attempt.report.is_none() && !attempt.no_report);
}

/// A reply that only waits is a status like any other: it is kept, never
/// stored as the report; the task is No report all the same.
#[test]
fn a_waiting_reply_is_kept_and_never_stored_as_the_report() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut team = team();
    team.status = TeamStatus::Running;
    let task = team.tasks.iter_mut().find(|task| task.assignee == "dev").unwrap();
    task.attempts.push(Attempt {
        n: 1,
        id: "A1".into(),
        member: "dev".into(),
        state: AttemptState::Delivered,
        sent_at: 10.0,
        delivered_at: Some(10.0),
        started_at: Some(11.0),
        turn_ended_at: Some(50.0),
        turn_reply: "r7".into(),
        report_ready_at: Some(50.0),
        ..Default::default()
    });
    task.status = TaskStatus::InProgress;
    let id = task.id;
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), team.clone(), vec![]).unwrap();
    inspect_override(Some(json!({"reply_id": "r7", "reply_at": 50.0, "last_message": "Waiting for the build lock held by QA."})));
    run_step(&mut store, &team, &Step::NoReport { task: id }, "").unwrap();
    inspect_override(None);
    let saved = store.load(&team.id).unwrap().unwrap();
    let attempt = saved.task(id).unwrap().attempts.last().unwrap();
    assert!(attempt.report.is_none());
    assert!(attempt.no_report);
    assert_eq!(attempt.last_reply, "Waiting for the build lock held by QA.");
}

/// A member that ends its turn on its own question stays a question: no turn
/// end, no No report, and the inbox carries the question, not a report item.
#[test]
fn a_question_stays_a_question() {
    let mut team = team();
    delivered(&mut team);
    team.status = TeamStatus::Running;
    team.task_mut(1).unwrap().status = TaskStatus::InProgress;
    team.task_mut(1).unwrap().attempts[0].questions.push(Question {
        id: "q1".into(),
        text: "Which database?".into(),
        at: 40.0,
        ..Default::default()
    });
    let mut quiet = session("codex/x/dev", "r1", true);
    quiet.reply_at = Some(50.0);
    quiet.reply_id = "r7".into();
    let timeout = team.settings.report_timeout_seconds as f64;
    let found = steps(&team, &[quiet.clone()], 50.0 + timeout);
    assert!(!found.iter().any(|s| matches!(s, Step::TurnEnded { .. } | Step::NoReport { .. })), "{found:?}");
    // The question is in the lead's inbox; no report item is.
    let items = crate::state::teams::inbox::items(&team);
    assert!(items.iter().any(|item| item.kind == "question"), "{items:?}");
    assert!(!items.iter().any(|item| item.kind == "no_report"), "{items:?}");
}

/// The one action for No report is "use this reply as the report": the item
/// carries the kept reply and the `use-last-reply` command, not a capture.
#[test]
fn the_no_report_item_offers_the_last_reply_as_the_one_action() {
    let mut team = team();
    team.status = TeamStatus::Running;
    let now = crate::state::now();
    let task = team.tasks.iter_mut().find(|task| task.assignee == "dev").unwrap();
    task.status = TaskStatus::InProgress;
    task.attempts.push(Attempt {
        n: 1,
        id: "A1".into(),
        member: "dev".into(),
        state: AttemptState::Delivered,
        sent_at: now - 7200.0,
        delivered_at: Some(now - 7200.0),
        started_at: Some(now - 7100.0),
        turn_ended_at: Some(now - 3600.0),
        turn_reply: "r7".into(),
        last_reply: "Done: the parser and its tests".into(),
        no_report: true,
        ..Default::default()
    });
    let id = task.id;
    let items = crate::state::teams::inbox::items(&team);
    let item = items.iter().find(|item| item.kind == "no_report").unwrap_or_else(|| panic!("no report item: {items:?}"));
    assert!(item.command.contains("hgs team use-last-reply"), "{}", item.command);
    assert!(!item.command.contains("team report"), "{}", item.command);
    assert_eq!(item.detail, "Done: the parser and its tests");
    // The use-last-reply op is the explicit action that turns the kept reply
    // into the report.
    let mut loaded = team.clone();
    let mut events = Vec::new();
    ops::apply(
        &mut loaded,
        "use-last-reply",
        &json!({"task": id, "attempt": "A1", "text": "Done: the parser and its tests"}),
        &Actor::Human,
        &BTreeSet::new(),
        &mut events,
    )
    .unwrap();
    let report = loaded.task(id).unwrap().attempts.last().unwrap().report.as_ref().unwrap();
    assert_eq!(report.text, "Done: the parser and its tests");
    assert!(!report.captured);
}
