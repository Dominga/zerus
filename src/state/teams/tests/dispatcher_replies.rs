use super::*;

/// #628: a DeepSeek member whose newest native event is 25 minutes old is
/// still silent, and the lead hears it once.
#[test]
fn a_dsh_member_with_no_native_event_for_twenty_minutes_is_silent() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut t = team();
    t.status = TeamStatus::Running;
    {
        let dev = t.members.iter_mut().find(|member| member.id == "dev").unwrap();
        dev.agent = "dsh".into();
        dev.session = Some(SessionRef { name: "dsh/x/dev".into(), run_id: "r1".into(), conversation_id: "c".into(), ..Default::default() });
    }
    {
        let task = t.tasks.iter_mut().find(|task| task.assignee == "dev").unwrap();
        task.attempts.push(Attempt { n: 1, id: "A1".into(), member: "dev".into(), state: AttemptState::Delivered,
            sent_at: 10.0, delivered_at: Some(10.0), started_at: Some(11.0), ..Default::default() });
        task.status = TaskStatus::InProgress;
    }
    let id = t.tasks.iter().find(|task| task.assignee == "dev").unwrap().id;
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
    let at = crate::state::now();
    let sent = std::rc::Rc::new(std::cell::RefCell::new(String::new()));
    let record = sent.clone();
    let quiet_at = at - 25.0 * 60.0;
    set_fake_hgs(Some(Box::new(move |_, args: &[&str], input: Option<&Value>| match args.first().copied() {
        Some("inspect") => Ok(fake_output(0, &json!({"run_id":"r1","conversation_id":"c","activity":"busy","phase":"tool",
            "events":[{"type":"PreToolUse","at":quiet_at - 100.0},{"type":"PostToolUse","at":quiet_at}]}).to_string(), "")),
        Some("send") => {
            *record.borrow_mut() = input.unwrap()["text"].as_str().unwrap_or_default().to_owned();
            Ok(fake_output(0, r#"{"status":"submitted"}"#, ""))
        }
        other => panic!("unexpected hgs {other:?}"),
    })));
    run_step(&mut store, &t, &Step::CheckSilent { task: id, last: quiet_at }, "").unwrap();
    set_fake_hgs(None);
    let saved = store.load(&t.id).unwrap().unwrap();
    let attempt = saved.task(id).unwrap().attempts.last().unwrap();
    assert!(attempt.told_lead.iter().any(|kind| kind == "silent"), "{attempt:?}");
    assert!(sent.borrow().contains("silent 25 min"), "{}", sent.borrow());
    let page = store.events(&t.id, None, 50).unwrap();
    assert!(page["items"].as_array().unwrap().iter().any(|event| event["kind"] == "member_silent"), "{page}");
}

/// #595 L3: an inspect that fails (here: a record from another run) does not
/// become a plain "silent": the lead hears that Zerus could not read it.
#[test]
fn a_dsh_member_whose_events_cannot_be_read_is_told_so() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut t = team();
    t.status = TeamStatus::Running;
    {
        let dev = t.members.iter_mut().find(|member| member.id == "dev").unwrap();
        dev.agent = "dsh".into();
        dev.session = Some(SessionRef { name: "dsh/x/dev".into(), run_id: "r1".into(), conversation_id: "c".into(), ..Default::default() });
    }
    {
        let task = t.tasks.iter_mut().find(|task| task.assignee == "dev").unwrap();
        task.attempts.push(Attempt { n: 1, id: "A1".into(), member: "dev".into(), state: AttemptState::Delivered,
            sent_at: 10.0, delivered_at: Some(10.0), started_at: Some(11.0), ..Default::default() });
        task.status = TaskStatus::InProgress;
    }
    let id = t.tasks.iter().find(|task| task.assignee == "dev").unwrap().id;
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
    let at = crate::state::now();
    let sent = std::rc::Rc::new(std::cell::RefCell::new(String::new()));
    let record = sent.clone();
    inspect_override(Some(json!({"run_id":"another-run","conversation_id":"c","activity":"busy","phase":"tool","events":[]})));
    set_fake_hgs(Some(Box::new(move |_, args: &[&str], input: Option<&Value>| match args.first().copied() {
        Some("send") => {
            *record.borrow_mut() = input.unwrap()["text"].as_str().unwrap_or_default().to_owned();
            Ok(fake_output(0, r#"{"status":"submitted"}"#, ""))
        }
        other => panic!("unexpected hgs {other:?}"),
    })));
    run_step(&mut store, &t, &Step::CheckSilent { task: id, last: at - 25.0 * 60.0 }, "").unwrap();
    set_fake_hgs(None);
    inspect_override(None);
    let text = sent.borrow().clone();
    assert!(text.contains("could not read its events"), "{text}");
    assert!(text.contains("silent 25 min"), "{text}");
}

#[test]
fn a_quiet_capture_takes_only_a_reply_newer_than_the_delivery() {
    // N6: after a rejection the session's last message is still attempt 1's
    // report (or another task's); without a turn identity only a reply
    // written after this attempt was delivered is its own.
    let (_dir, store, team_id, task) = capture_with(json!({"reply_id":"old","reply_at":5.0,"last_message":"attempt 1's report"}));
    let saved = store.load(&team_id).unwrap().unwrap();
    let attempt = saved.task(task).unwrap().attempts.last().unwrap();
    assert!(attempt.report.is_none(), "an older reply was captured: {:?}", attempt.report);
    assert!(attempt.no_report);
    assert_eq!(attempt.state, AttemptState::Delivered);
    // A reply written after the delivery is this attempt's report.
    let (_dir, store, team_id, task) = capture_with(json!({"reply_id":"new","reply_at":20.0,"last_message":"Done: checked"}));
    let saved = store.load(&team_id).unwrap().unwrap();
    let attempt = saved.task(task).unwrap().attempts.last().unwrap();
    let report = attempt.report.as_ref().unwrap();
    assert!(report.captured && report.text.contains("Done: checked"), "{report:?}");
    assert_eq!(attempt.state, AttemptState::Reported);
    // No time at all: not proven to be this attempt's.
    let (_dir, store, team_id, task) = capture_with(json!({"reply_id":"none","last_message":"who knows"}));
    let saved = store.load(&team_id).unwrap().unwrap();
    assert!(saved.task(task).unwrap().attempts.last().unwrap().report.is_none());
}

#[test]
fn a_no_report_attempt_is_not_decided_again() {
    // N7: after a failed capture the guard stops a repeated `hgs inspect` on
    // every tick, in both NoReport branches.
    let mut t = team();
    t.status = TeamStatus::Running;
    let task = t.tasks.iter_mut().find(|t| t.assignee == "dev").unwrap();
    task.attempts.push(Attempt {
        n: 1,
        id: "A1".into(),
        member: "dev".into(),
        state: AttemptState::Delivered,
        sent_at: 10.0,
        delivered_at: Some(10.0),
        started_at: Some(11.0),
        no_report: true,
        ..Default::default()
    });
    task.status = TaskStatus::InProgress;
    let id = task.id;
    let timeout = t.settings.report_timeout_seconds as f64;
    let at = 10.0 + timeout + 1.0;
    // No reply mark in the listing: the (None, Some) branch.
    assert!(!steps(&t, &[session("codex/x/dev", "r1", true)], at).iter().any(|step| matches!(step, Step::NoReport { .. })));
    // No session in the listing: the (None, None) branch.
    assert!(!steps(&t, &[], at).iter().any(|step| matches!(step, Step::NoReport { .. })));
    // Without the flag the same states decide it once.
    t.task_mut(id).unwrap().attempts.last_mut().unwrap().no_report = false;
    assert!(steps(&t, &[session("codex/x/dev", "r1", true)], at).iter().any(|step| matches!(step, Step::NoReport { .. })));
    assert!(steps(&t, &[], at).iter().any(|step| matches!(step, Step::NoReport { .. })));
}

#[test]
fn a_computer_that_did_not_list_is_not_a_stopped_member() {
    // N7: a remote host whose listing failed decides nothing; its session is
    // not "gone" and the lead is not told it stopped.
    let mut t = team();
    t.status = TeamStatus::Running;
    let task = t.tasks.iter_mut().find(|t| t.assignee == "dev").unwrap();
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
    t.members.iter_mut().find(|member| member.id == "dev").unwrap().session.as_mut().unwrap().host = "msi".into();
    let timeout = t.settings.report_timeout_seconds as f64;
    let at = 10.0 + timeout + 1.0;
    let local_only: BTreeSet<String> = [String::new()].into_iter().collect();
    let found = steps_with(&t, &[], at, Some(&local_only));
    assert!(!found.iter().any(|step| matches!(step, Step::NoReport { .. } | Step::StartLate { .. })), "{found:?}");
    // Its host answers and the session is really not there: decided.
    let both: BTreeSet<String> = [String::new(), "msi".to_owned()].into_iter().collect();
    assert!(steps_with(&t, &[], at, Some(&both)).iter().any(|step| matches!(step, Step::NoReport { .. })));
    // A local member whose session is gone is decided as before.
    t.members.iter_mut().find(|member| member.id == "dev").unwrap().session.as_mut().unwrap().host.clear();
    assert!(steps_with(&t, &[], at, Some(&local_only)).iter().any(|step| matches!(step, Step::NoReport { .. })));
}

#[test]
fn a_quiet_turn_end_that_never_reached_the_listing_is_captured_as_the_report() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut t = team();
    t.status = TeamStatus::Running;
    let task = t.tasks.iter_mut().find(|t| t.assignee == "dev").unwrap();
    task.attempts.push(Attempt {
        n: 1,
        id: "A1".into(),
        member: "dev".into(),
        state: AttemptState::Delivered,
        sent_at: 10.0,
        delivered_at: Some(10.0),
        started_at: Some(20.0),
        ..Default::default()
    });
    task.status = TaskStatus::InProgress;
    let id = task.id;
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
    capture_quiet(&mut store, &t.id, id, "Done: everything checked").unwrap();
    let saved = store.load(&t.id).unwrap().unwrap();
    let attempt = saved.task(id).unwrap().attempts.last().unwrap();
    let report = attempt.report.as_ref().unwrap();
    assert!(report.captured);
    assert!(report.text.contains("Done: everything checked"));
    assert_eq!((attempt.state, attempt.turn_ended_at.is_some()), (AttemptState::Reported, true));
}

#[test]
fn a_captured_reply_is_marked_as_the_report() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut t = team();
    t.status = TeamStatus::Running;
    let task = t.tasks.iter_mut().find(|t| t.assignee == "dev").unwrap();
    task.attempts.push(Attempt {
        n: 1,
        id: "A1".into(),
        member: "dev".into(),
        state: AttemptState::Delivered,
        sent_at: 10.0,
        delivered_at: Some(10.0),
        turn_ended_at: Some(50.0),
        turn_reply: "r7".into(),
        ..Default::default()
    });
    task.status = TaskStatus::InProgress;
    let id = task.id;
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
    capture_report(&mut store, &t.id, id, "Done and checked", "r7").unwrap();
    let saved = store.load(&t.id).unwrap().unwrap();
    let attempt = saved.task(id).unwrap().attempts.last().unwrap();
    let report = attempt.report.as_ref().unwrap();
    assert!(report.captured);
    assert_eq!(report.text, "(From the last reply of dev (Backend); it ran no hgs team report.)\n\nDone and checked");
    assert_eq!((attempt.state, saved.task(id).unwrap().status), (AttemptState::Reported, TaskStatus::Submitted));
    assert_eq!(attempt.notice, Notice::Pending);
    // The member's real report replaces the captured one (A6) and the lead
    // hears again.
    let mut loaded = saved.clone();
    let mut events = Vec::new();
    let replaced = ops::apply(
        &mut loaded,
        "report",
        &json!({"task":id,"attempt":"A1","text":"Done: 42 tests pass."}),
        &Actor::Member("dev".into()),
        &BTreeSet::new(),
        &mut events,
    )
    .unwrap();
    assert_eq!(replaced["status"], "submitted");
    assert_eq!(events.last().unwrap().kind, "report_replaced");
    let attempt = loaded.task(id).unwrap().attempts.last().unwrap();
    let report = attempt.report.as_ref().unwrap();
    assert!(!report.captured);
    assert_eq!(report.text, "Done: 42 tests pass.");
    assert_eq!(report.replaced[0].text, "(From the last reply of dev (Backend); it ran no hgs team report.)\n\nDone and checked");
}

#[test]
fn the_captured_reply_text_comes_from_the_inspect_record() {
    // A terminal session's last message, trimmed.
    assert_eq!(reply_text(&json!({"last_message":"  Done.  "})), "Done.");
    // DeepSeek reports its replies as the main agent's AgentMessage events;
    // a subagent's message is not the member's reply.
    let record = json!({"events":[
        {"type":"AgentMessage","agent_id":"child","detail":"Not mine"},
        {"type":"AgentMessage","agent_id":"","detail":"  The answer.  "},
        {"type":"Tool","detail":"run"}]});
    assert_eq!(reply_text(&record), "The answer.");
    assert_eq!(reply_text(&json!({})), "");
}

#[test]
fn a_members_liveness_is_recorded_only_when_it_changes() {
    let mut team = team();
    let live = session("codex/x/dev", "r1", true);
    // The first listing records the member as live; it is not rewritten
    // every tick once recorded (L3).
    assert!(steps(&team, std::slice::from_ref(&live), 20.0).contains(&Step::Liveness { member: "dev".into(), live: true }));
    team.members.iter_mut().find(|member| member.id == "dev").unwrap().seen = Some(MemberSeen { at: 20.0, live: true });
    assert!(!steps(&team, &[live], 20.0).iter().any(|step| matches!(step, Step::Liveness { .. })));
    // A session that stopped changes it once.
    let mut stopped = session("codex/x/dev", "r1", true);
    stopped.live = false;
    assert!(steps(&team, &[stopped], 20.0).contains(&Step::Liveness { member: "dev".into(), live: false }));
    team.members.iter_mut().find(|member| member.id == "dev").unwrap().seen = Some(MemberSeen { at: 20.0, live: false });
    assert!(steps(&team, &[session("codex/x/dev", "r1", true)], 20.0).contains(&Step::Liveness { member: "dev".into(), live: true }));
}
