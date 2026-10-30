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
fn the_capture_gate_holds_with_a_question_or_background_work() {
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
    // A plain finished turn is captured after its timeout.
    assert!(!steps(&team, &[quiet.clone()], 50.0 + timeout - 1.0).iter().any(|s| matches!(s, Step::NoReport { .. })));
    assert!(steps(&team, &[quiet.clone()], 50.0 + timeout).contains(&Step::NoReport { task: 1 }));
    // An open question from the member blocks the capture.
    for phase in ["question", "input", "approval"] {
        let mut asking = quiet.clone();
        asking.phase = phase.into();
        assert!(!steps(&team, &[asking], 50.0 + timeout).iter().any(|s| matches!(s, Step::NoReport { .. })), "{phase}");
    }
    // Background work blocks it.
    let mut background = quiet.clone();
    background.background = true;
    assert!(!steps(&team, &[background], 50.0 + timeout).iter().any(|s| matches!(s, Step::NoReport { .. })));
    // An open team question on the attempt blocks it, until the lead answers.
    team.task_mut(1).unwrap().attempts[0].questions.push(Question { id: "q1".into(), text: "Which?".into(), at: 40.0, ..Default::default() });
    assert!(!steps(&team, &[quiet.clone()], 50.0 + timeout).iter().any(|s| matches!(s, Step::NoReport { .. })));
    team.task_mut(1).unwrap().attempts[0].questions[0].answer = "The old one".into();
    assert!(steps(&team, &[quiet.clone()], 50.0 + timeout).contains(&Step::NoReport { task: 1 }));
    // The member works again: its turn end is cleared, not captured.
    let mut busy = quiet.clone();
    busy.idle = false;
    assert!(steps(&team, &[busy], 50.0 + timeout).contains(&Step::WorkResumed { task: 1 }));
    let mut newer = quiet;
    newer.reply_id = "r8".into();
    newer.reply_at = Some(70.0);
    assert!(steps(&team, &[newer], 50.0 + timeout).contains(&Step::WorkResumed { task: 1 }));
}

#[test]
fn a_long_background_job_does_not_capture_when_it_ends() {
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
    // The gate was never open: the closed gate is recorded, not a capture.
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

#[test]
fn a_question_asked_during_the_inspect_blocks_the_capture() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut t = team();
    t.status = TeamStatus::Running;
    let task = t.tasks.iter_mut().find(|task| task.assignee == "dev").unwrap();
    task.attempts.push(Attempt {
        n: 1,
        id: "A1".into(),
        member: "dev".into(),
        state: AttemptState::Delivered,
        sent_at: 10.0,
        delivered_at: Some(10.0),
        turn_ended_at: Some(50.0),
        turn_reply: "r7".into(),
        report_ready_at: Some(50.0),
        ..Default::default()
    });
    task.status = TaskStatus::InProgress;
    let id = task.id;
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
    // The member asks while the dispatcher reads its last reply.
    store
        .mutate(None, &t.id, None, |team, events| {
            ops::apply(team, "ask", &json!({"text":"Which database?"}), &Actor::Member("dev".into()), &BTreeSet::new(), events)
        })
        .unwrap();
    let error = capture_report(&mut store, &t.id, id, "Building and running...", "r7").unwrap_err().to_string();
    assert_eq!(error, "the attempt changed before its report was captured");
    let saved = store.load(&t.id).unwrap().unwrap();
    let attempt = saved.task(id).unwrap().attempts.last().unwrap();
    assert_eq!((attempt.state, attempt.report.is_some()), (AttemptState::Delivered, false));
    assert_eq!(saved.task(id).unwrap().status, TaskStatus::InProgress);
}

#[test]
fn a_reply_that_only_waits_is_a_status_not_a_report() {
    // A reply that only says it waits uses these phrases.
    for text in [
        "Waiting for the build lock to be released.",
        "Still queued behind QA's build; I'll wait.",
        "Жду, пока освободится сборка.",
    ] {
        assert!(waits_only(text), "{text}");
    }
    // A reply that reports work, even while it waits, is not only a wait.
    for text in [
        "Done: the API is in and its tests passed.",
        "Waiting for the build; meanwhile I fixed the parser and committed it.",
        "Tests pass, but the build lock is still queued.",
    ] {
        assert!(!waits_only(text), "{text}");
    }
    // A captured waiting reply stays a status: no report is stored.
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut t = team();
    delivered(&mut t);
    {
        let attempt = &mut t.task_mut(1).unwrap().attempts[0];
        attempt.turn_ended_at = Some(50.0);
        attempt.turn_reply = "r7".into();
        attempt.report_ready_at = Some(50.0);
    }
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
    let error = capture_report(&mut store, &t.id, 1, "Waiting for the build lock held by QA.", "r7").unwrap_err().to_string();
    assert_eq!(error, "the reply only says it waits; it is a status, not a report");
    let saved = store.load(&t.id).unwrap().unwrap();
    let attempt = saved.task(1).unwrap().attempts.last().unwrap();
    assert_eq!((attempt.state, attempt.report.is_some()), (AttemptState::Delivered, false));
    // A real report is still captured on the same attempt.
    capture_report(&mut store, &t.id, 1, "Done: the API is in and its tests passed.", "r7").unwrap();
    assert!(store.load(&t.id).unwrap().unwrap().task(1).unwrap().attempts.last().unwrap().report.is_some());
    // The quiet capture refuses a waiting reply too.
    let error = capture_quiet(&mut store, &t.id, 2, "Queued behind the build.").unwrap_err().to_string();
    assert_eq!(error, "the reply only says it waits; it is a status, not a report");
}

/// U56: a reply whose last line says it still waits is not made a report,
/// whatever work it names before; one waiting for its review is a report.
#[test]
fn a_reply_that_still_waits_is_not_made_a_report() {
    for text in [
        "Implemented the parser and committed it.\nWaiting for the editor to close.",
        "Tests pass. I'm still waiting for the build to finish.",
        "Done with the API. I'll report once the GUI suite ends.",
        "Сделал парсер и тесты.\nЖду, пока закроется редактор.",
    ] {
        assert!(not_a_report(text), "{text}");
    }
    for text in [
        "Done: the API is in and its tests passed.",
        "Waiting for the build; meanwhile I fixed the parser and committed it.",
        "Done, tests pass. Waiting for your review.",
        "Tests pass, but the build lock is still queued.",
        "Waiting for the editor to close.\nThen I committed: tests pass, done.",
    ] {
        assert!(!not_a_report(text), "{text}");
    }
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut t = team();
    delivered(&mut t);
    {
        let attempt = &mut t.task_mut(1).unwrap().attempts[0];
        attempt.turn_ended_at = Some(50.0);
        attempt.turn_reply = "r7".into();
        attempt.report_ready_at = Some(50.0);
    }
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
    let reply = "Implemented the parser, tests pass.\nCommitting now: waiting for the editor to close.";
    let error = capture_report(&mut store, &t.id, 1, reply, "r7").unwrap_err().to_string();
    assert_eq!(error, "the reply only says it waits; it is a status, not a report");
    let attempt = store.load(&t.id).unwrap().unwrap().task(1).unwrap().attempts.last().unwrap().clone();
    assert_eq!((attempt.state, attempt.report.is_some()), (AttemptState::Delivered, false));
}
