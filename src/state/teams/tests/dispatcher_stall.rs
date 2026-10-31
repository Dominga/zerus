use super::*;
use super::support::*;

#[test]
fn a_started_attempt_whose_session_is_gone_is_captured_after_the_timeout() {
    // The session left the listing entirely (stopped or renamed): the attempt
    // still ends, with the last reply read from its own record.
    let mut t = team();
    delivered(&mut t);
    t.task_mut(1).unwrap().attempts[0].started_at = Some(20.0);
    let timeout = t.settings.report_timeout_seconds as f64;
    let seen = vec![session("claude/x/lead", "r0", true)];
    let late = steps(&t, &seen, 10.0 + timeout + 1.0);
    assert!(late.contains(&Step::NoReport { task: 1 }), "{late:?}");
}

#[test]
fn a_question_the_session_stopped_asking_closes_in_the_team() {
    // The owner, 2026-10-28: the reviewer's question was answered on the
    // desktop and stayed open in team status for an hour.
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut t = team();
    t.status = TeamStatus::Running;
    let pending = PendingQuestion { question_id: "q1".into(), question_hash: "h1".into(), approval: true, at: 1.0, card: json!({}) };
    {
        let dev = t.members.iter_mut().find(|m| m.id == "dev").unwrap();
        dev.permission = Some(permission_record(&pending));
        dev.pending = Some(pending);
    }
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
    run_step(&mut store, &t, &Step::ClearPermission { member: "dev".into() }, "").unwrap();
    let after = store.load(&t.id).unwrap().unwrap();
    let dev = after.members.iter().find(|m| m.id == "dev").unwrap();
    assert!(dev.pending.is_none());
    assert!(dev.permission.is_none(), "the closed question still shows");
    assert_eq!(dev.permission_answers.last().unwrap()["decision"], "unknown");
}

#[test]
fn a_member_silent_in_its_turn_is_told_to_the_lead_once() {
    // The owner, 2026-10-28: the reviewer sat 74 minutes in a turn with no
    // event and no question Zerus could see; nobody heard of it.
    let mut t = team();
    t.status = TeamStatus::Running;
    {
        let task = t.tasks.iter_mut().find(|task| task.assignee == "dev").unwrap();
        task.attempts.push(Attempt { n: 1, id: "A1".into(), member: "dev".into(), state: AttemptState::Delivered,
            sent_at: 10.0, delivered_at: Some(10.0), started_at: Some(11.0), ..Default::default() });
        task.status = TaskStatus::InProgress;
    }
    let id = t.tasks.iter().find(|task| task.assignee == "dev").unwrap().id;
    let lead = session("claude/x/lead", "r0", true);
    let at = 10_000.0;
    let mut dev = session("codex/x/dev", "r1", false);
    dev.activity = "busy".into();
    dev.phase = "tool".into();
    dev.last_event_at = Some(at - 25.0 * 60.0);
    let found = steps(&t, &[lead.clone(), dev.clone()], at);
    assert!(found.iter().any(|step| matches!(step, Step::Silent { task, minutes: 25 } if *task == id)), "{found:?}");
    // A question on its screen, or recent events, is not silence.
    let mut asking = dev.clone();
    asking.asking = true;
    assert!(!steps(&t, &[lead.clone(), asking], at).iter().any(|step| matches!(step, Step::Silent { .. })));
    let mut talking = dev.clone();
    talking.last_event_at = Some(at - 60.0);
    assert!(!steps(&t, &[lead.clone(), talking], at).iter().any(|step| matches!(step, Step::Silent { .. })));
    // Once told, the lead is not told again for the attempt.
    t.tasks.iter_mut().find(|task| task.id == id).unwrap().attempts[0].told_lead.push("silent".into());
    assert!(!steps(&t, &[lead, dev], at).iter().any(|step| matches!(step, Step::Silent { .. })));
}

/// #628: a DeepSeek session's listing `last_event_at` does not advance with
/// its dsh_native tool events; the newest event in the session decides,
/// and a working member is not called silent.
/// U56: a busy member with no session event for stuck_minutes is stuck with
/// its reason; a question, Background work or a fresh event is not stuck.
#[test]
fn a_stuck_member_is_marked_when_busy_without_events() {
    let at = 10_000.0;
    let mut t = team();
    let lead = session("claude/x/lead", "r0", true);
    let mut dev = session("codex/x/dev", "r1", true);
    dev.activity = "busy".into();
    dev.phase = "tool".into();
    dev.last_event_at = Some(at - 11.0 * 60.0);
    let found = steps(&t, &[lead.clone(), dev.clone()], at);
    assert!(
        found.iter().any(|step| matches!(step, Step::MarkStuck { member, reason, .. } if member == "dev" && reason == "no_progress")),
        "{found:?}"
    );
    // A question, Background work, a Zerus build or a fresh event is not stuck.
    for cleared in [
        { let mut seen = dev.clone(); seen.asking = true; seen },
        { let mut seen = dev.clone(); seen.background = true; seen },
        { let mut seen = dev.clone(); seen.building = true; seen },
        { let mut seen = dev.clone(); seen.last_event_at = Some(at - 60.0); seen },
    ] {
        assert!(
            !steps(&t, &[lead.clone(), cleared.clone()], at).iter().any(|step| matches!(step, Step::MarkStuck { .. })),
            "{:?}",
            cleared
        );
    }
    // Compaction is named by its own reason.
    let mut compacting = dev.clone();
    compacting.phase = "compacting".into();
    let found = steps(&t, &[lead.clone(), compacting], at);
    assert!(
        found.iter().any(|step| matches!(step, Step::MarkStuck { reason, .. } if reason == "compaction_timeout")),
        "{found:?}"
    );
    // stuck_minutes moves the limit: 5 minutes marks the 6-minute silence.
    t.settings.stuck_minutes = 5;
    let mut five = dev.clone();
    five.last_event_at = Some(at - 6.0 * 60.0);
    assert!(steps(&t, &[lead.clone(), five], at).iter().any(|step| matches!(step, Step::MarkStuck { .. })));
    // Marked once: the same state is not marked again, and a member that
    // reports again is unmarked.
    t.members.iter_mut().find(|member| member.id == "dev").unwrap().stuck =
        Some(MemberStuck { at, reason: "no_progress".into(), phase: "tool".into(), ..Default::default() });
    assert!(!steps(&t, &[lead.clone(), dev.clone()], at).iter().any(|step| matches!(step, Step::MarkStuck { .. })));
    let mut fresh = dev.clone();
    fresh.last_event_at = Some(at - 5.0);
    assert!(steps(&t, &[lead, fresh], at).iter().any(|step| matches!(step, Step::Unstuck { member } if member == "dev")));
}

/// U56: a pending task pinned to a stuck or stopped member says so in its
/// waits, and the wait goes once the member is back.
#[test]
fn a_pinned_pending_task_waits_for_a_stuck_or_stopped_member() {
    let at = 10_000.0;
    let mut t = team();
    let lead = session("claude/x/lead", "r0", true);
    {
        let task = t.tasks.iter_mut().find(|task| task.assignee == "dev").unwrap();
        task.status = TaskStatus::Pending;
        task.pinned = true;
    }
    let mut dev = session("codex/x/dev", "r1", true);
    // Stuck: the wait says so with the member id, role and cause.
    t.members.iter_mut().find(|member| member.id == "dev").unwrap().stuck =
        Some(MemberStuck { at: at - 15.0 * 60.0, reason: "no_progress".into(), phase: "tool".into(), ..Default::default() });
    let found = steps(&t, &[lead.clone(), dev.clone()], at);
    let reason = found
        .iter()
        .find_map(|step| match step {
            Step::WaitOnMember { member, reason, .. } if member == "dev" => Some(reason.clone()),
            _ => None,
        })
        .expect("the pinned task waits");
    assert!(reason.contains("dev (Backend)") && reason.contains("stuck for 15 min"), "{reason}");
    // Not live: the wait names a stopped session, and a live member clears it.
    t.members.iter_mut().find(|member| member.id == "dev").unwrap().stuck = None;
    dev.live = false;
    let found = steps(&t, &[lead.clone(), dev.clone()], at);
    let reason = found
        .iter()
        .find_map(|step| match step {
            Step::WaitOnMember { reason, .. } => Some(reason.clone()),
            _ => None,
        })
        .expect("the pinned task waits");
    assert!(reason.contains("stopped or offline"), "{reason}");
    // The wait is recorded on the task, then cleared when the member is back.
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
    let task_id = t.tasks.iter().find(|task| task.assignee == "dev").unwrap().id;
    run_step(
        &mut store,
        &t,
        &Step::WaitOnMember { task: task_id, member: "dev".into(), reason: "pinned to dev (Backend), whose session is stopped or offline".into() },
        "",
    )
    .unwrap();
    let saved = store.load(&t.id).unwrap().unwrap();
    let wait = saved.task(task_id).unwrap().waits.clone().unwrap();
    assert_eq!((wait.waits_for.as_str(), wait.on.clone()), ("member", vec!["dev".to_owned()]));
    let items = super::inbox::items(&saved);
    let item = items.iter().find(|item| item.kind == "task_waits").expect("a waits item");
    // N3: the stopped session's command is its relaunch, in a whole sentence.
    assert!(item.detail.contains("Relaunch"), "{}", item.detail);
    assert!(item.command.contains("relaunch"), "{}", item.command);
    // A task pin to an available member frees the wait.
    run_step(&mut store, &saved, &Step::ReleaseWait { task: task_id }, "").unwrap();
    assert!(store.load(&t.id).unwrap().unwrap().task(task_id).unwrap().waits.is_none());
    assert!(!steps(&t, &[lead, session("codex/x/dev", "r1", true)], at).iter().any(|step| matches!(step, Step::WaitOnMember { .. })));
    // N4: with keep_assignees a planned (not pinned) assignee is kept too.
    let mut kept = team();
    kept.settings.keep_assignees = true;
    kept.members.iter_mut().find(|member| member.id == "dev").unwrap().stuck =
        Some(MemberStuck { at: at - 15.0 * 60.0, reason: "no_progress".into(), phase: "tool".into(), ..Default::default() });
    {
        let task = kept.tasks.iter_mut().find(|task| task.assignee == "dev").unwrap();
        task.status = TaskStatus::Pending;
        task.pinned = false;
    }
    assert!(steps(&kept, &[session("claude/x/lead", "r0", true), session("codex/x/dev", "r1", true)], at)
        .iter()
        .any(|step| matches!(step, Step::WaitOnMember { .. })));
}

/// N1: a dsh member's stale listing with a fresh native event in its open
/// attempt is not stuck; a stale attempt is read once, then marked.
#[test]
fn a_dsh_member_with_fresh_native_activity_is_not_stuck() {
    let at = 10_000.0;
    let mut t = team();
    t.members.iter_mut().find(|member| member.id == "dev").unwrap().agent = "dsh".into();
    let task = t.tasks.iter_mut().find(|task| task.assignee == "dev").unwrap();
    task.status = TaskStatus::InProgress;
    task.attempts.push(Attempt {
        n: 1,
        id: "a1".into(),
        member: "dev".into(),
        state: AttemptState::Delivered,
        sent_at: 1.0,
        delivered_at: Some(1.0),
        activity_at: Some(at - 30.0),
        ..Default::default()
    });
    let mut dev = session("codex/x/dev", "r1", true);
    dev.agent = "dsh".into();
    dev.activity = "busy".into();
    dev.phase = "tool".into();
    dev.last_event_at = Some(at - 20.0 * 60.0);
    let lead = session("claude/x/lead", "r0", true);
    let found = steps(&t, &[lead.clone(), dev.clone()], at);
    assert!(!found.iter().any(|step| matches!(step, Step::MarkStuck { .. })), "a fresh native event is progress: {found:?}");
    // The native event is stale too: the member is read once before the mark.
    t.tasks.iter_mut().find(|task| task.assignee == "dev").unwrap().attempts[0].activity_at = Some(at - 20.0 * 60.0);
    let found = steps(&t, &[lead.clone(), dev.clone()], at);
    assert!(found.iter().any(|step| matches!(step, Step::CheckSilent { .. })), "one read first: {found:?}");
    assert!(!found.iter().any(|step| matches!(step, Step::MarkStuck { .. })), "not before the read: {found:?}");
    // After the read found nothing new, the mark is made.
    t.tasks.iter_mut().find(|task| task.assignee == "dev").unwrap().attempts[0].told_lead.push("silent".into());
    let found = steps(&t, &[lead, dev], at);
    assert!(found.iter().any(|step| matches!(step, Step::MarkStuck { .. })), "{found:?}");
}

/// #728: the requested restart interrupts the session and resumes it in its
/// conversation; the member is no longer stuck and the request is cleared.
#[test]
fn a_restart_interrupts_the_session_and_resumes_it_in_its_conversation() {
    use std::cell::RefCell;
    use std::rc::Rc;
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut t = team();
    {
        let dev = t.members.iter_mut().find(|member| member.id == "dev").unwrap();
        dev.stuck = Some(MemberStuck { at: 1.0, reason: "no_progress".into(), phase: "tool".into(), ..Default::default() });
        dev.restart_pending = true;
    }
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let recorded = calls.clone();
    set_fake_hgs(Some(Box::new(move |_host, args: &[&str], _input| {
        recorded.borrow_mut().push(args.first().copied().unwrap_or_default().to_owned());
        Ok(fake_output(0, "{}", ""))
    })));
    let seen = [session("claude/x/lead", "r0", true), session("codex/x/dev", "r1", true)];
    let found = steps(&t, &seen, 100.0);
    assert!(found.contains(&Step::RestartStuck { member: "dev".into() }), "{found:?}");
    run_step(&mut store, &t, &Step::RestartStuck { member: "dev".into() }, "").unwrap();
    set_fake_hgs(None);
    let saved = store.load(&t.id).unwrap().unwrap();
    let dev = saved.member("dev").unwrap();
    assert!(dev.stuck.is_none() && !dev.restart_pending);
    assert_eq!(calls.borrow().as_slice(), ["interrupt", "resume"]);
    let feed = store.events(&t.id, None, 20).unwrap().to_string();
    assert!(feed.contains("member_relaunched"), "{feed}");
}

/// #728 L2: between the stuck and the silent bounds a quiet dsh member's
/// session is read at most once a minute.
#[test]
fn a_quiet_dsh_member_is_read_at_most_once_a_minute() {
    let at = crate::state::now();
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut t = team();
    t.members.iter_mut().find(|member| member.id == "dev").unwrap().agent = "dsh".into();
    let task = t.tasks.iter_mut().find(|task| task.assignee == "dev").unwrap();
    task.status = TaskStatus::InProgress;
    task.attempts.push(Attempt {
        n: 1,
        id: "a1".into(),
        member: "dev".into(),
        state: AttemptState::Delivered,
        sent_at: 1.0,
        delivered_at: Some(1.0),
        activity_at: Some(at - 15.0 * 60.0),
        ..Default::default()
    });
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
    let mut dev = session("codex/x/dev", "r1", true);
    dev.agent = "dsh".into();
    dev.activity = "busy".into();
    dev.phase = "tool".into();
    dev.last_event_at = Some(at - 15.0 * 60.0);
    let lead = session("claude/x/lead", "r0", true);
    let old = at - 15.0 * 60.0;
    set_fake_hgs(Some(Box::new(move |_, args: &[&str], _| match args.first().copied() {
        Some("inspect") => Ok(fake_output(0, &json!({"run_id":"r1","conversation_id":"c","events":[{"at":old}]}).to_string(), "")),
        other => panic!("only an inspect is sent: {other:?}"),
    })));
    let first = steps(&t, &[lead.clone(), dev.clone()], at);
    assert!(first.iter().any(|step| matches!(step, Step::CheckSilent { .. })), "{first:?}");
    run_step(&mut store, &t, &Step::CheckSilent { task: 1, last: old }, "").unwrap();
    let saved = store.load(&t.id).unwrap().unwrap();
    let attempt = saved.task(1).unwrap().attempts.last().unwrap();
    // Counted from the read itself: a slow machine may take seconds to get there.
    let read = attempt.silent_read_at.expect("the read is recorded");
    // A few seconds later there is no second read; a minute later there is.
    assert!(!steps(&saved, &[lead.clone(), dev.clone()], read + 3.0).iter().any(|step| matches!(step, Step::CheckSilent { .. })));
    assert!(steps(&saved, &[lead, dev], read + 61.0).iter().any(|step| matches!(step, Step::CheckSilent { .. })));
    set_fake_hgs(None);
}

/// U56: marking a member stuck records it, the feed says so, and the lead's
/// inbox has one item naming the id, phase, duration and waiting tasks.
#[test]
fn a_stuck_member_gets_one_item_and_a_feed_event() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let t = team();
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
    run_step(
        &mut store,
        &t,
        &Step::MarkStuck { member: "dev".into(), reason: "no_progress".into(), phase: "tool".into(), since: crate::state::now() - 12.0 * 60.0 },
        "",
    )
    .unwrap();
    let saved = store.load(&t.id).unwrap().unwrap();
    assert!(saved.member("dev").unwrap().stuck.is_some());
    let items = super::inbox::items(&saved);
    let item = items.iter().find(|item| item.kind == "member_stuck").expect("a member_stuck item");
    assert!(item.summary.contains("dev (Backend)") && item.summary.contains("phase tool"), "{}", item.summary);
    assert!(item.command.contains("settings"), "{}", item.command);
    let feed = store.events(&t.id, None, 20).unwrap().to_string();
    assert!(feed.contains("member_stuck"), "{feed}");
    // #728 (live, 2026-10-31): the item says the real minutes since the last
    // progress, never "0 min" from the time of the mark.
    assert!(item.summary.contains("for 12 min"), "{}", item.summary);
    assert!(feed.contains("\"minutes\":12"), "{feed}");
    // A mark whose quiet time is under the stuck bound sends no item.
    let mut early = saved.clone();
    early.members.iter_mut().find(|member| member.id == "dev").unwrap().stuck.as_mut().unwrap().since = Some(crate::state::now() - 60.0);
    assert!(!super::inbox::items(&early).iter().any(|item| item.kind == "member_stuck"));
}

/// #628: a DeepSeek session's listing `last_event_at` does not advance with
/// its dsh_native tool events; the newest event in the session decides,
/// and a working member is not called silent.
#[test]
fn a_dsh_member_with_fresh_native_tool_events_is_not_silent() {
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
    let lead = session("claude/x/lead", "r0", true);
    let mut dev = session("dsh/x/dev", "r1", false);
    dev.agent = "dsh".into();
    dev.activity = "busy".into();
    dev.phase = "tool".into();
    dev.last_event_at = Some(at - 25.0 * 60.0);
    let found = steps(&t, &[lead.clone(), dev.clone()], at);
    assert!(found.iter().any(|step| matches!(step, Step::CheckSilent { task, .. } if *task == id)), "{found:?}");
    assert!(!found.iter().any(|step| matches!(step, Step::Silent { .. })), "a dsh member is read before it is called silent");
    // The native tool events are seconds old: the member is working.
    let newest = at - 5.0;
    set_fake_hgs(Some(Box::new(move |_, args: &[&str], _| match args.first().copied() {
        Some("inspect") => Ok(fake_output(0, &json!({"run_id":"r1","conversation_id":"c","activity":"busy","phase":"tool",
            "events":[{"type":"PreToolUse","at":newest - 25.0},{"type":"PostToolUse","at":newest}]}).to_string(), "")),
        other => panic!("a working dsh member sends nothing: {other:?}"),
    })));
    run_step(&mut store, &t, &Step::CheckSilent { task: id, last: at - 25.0 * 60.0 }, "").unwrap();
    set_fake_hgs(None);
    let saved = store.load(&t.id).unwrap().unwrap();
    assert_eq!(saved.task(id).unwrap().attempts.last().unwrap().activity_at, Some(newest));
    assert!(saved.task(id).unwrap().attempts.last().unwrap().told_lead.is_empty());
    let after = steps(&saved, &[lead, dev], at);
    assert!(!after.iter().any(|step| matches!(step, Step::Silent { .. } | Step::CheckSilent { .. })), "{after:?}");
}
