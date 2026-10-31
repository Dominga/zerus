use super::*;
use super::support::*;

#[test]
fn a_restart_launches_the_same_conversation_and_returns_the_attempt_unconfirmed() {
    use crate::state::teams::model::{Attempt, AttemptState, TaskStatus};
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let mut t = team();
        t.settings.restart_crashed = true;
        t.members.iter_mut().find(|member| member.id == "dev").unwrap().seen = Some(MemberSeen { at: 1.0, live: false });
        let task = t.tasks.iter_mut().find(|task| task.assignee == "dev").unwrap();
        let task_id = task.id;
        task.attempts.push(Attempt { n: 1, id: "a1".into(), member: "dev".into(), state: AttemptState::Delivered, ..Default::default() });
        task.status = TaskStatus::InProgress;
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
        let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::<Vec<String>>::new()));
        let record = calls.clone();
        set_fake_hgs(Some(Box::new(move |_, args: &[&str], _: Option<&Value>| {
            record.borrow_mut().push(args.iter().map(|arg| arg.to_string()).collect());
            Ok(fake_output(0, "{}", ""))
        })));
        run_step(&mut store, &t, &Step::Restart { member: "dev".into() }, "").unwrap();
        run_step(&mut store, &t, &Step::CrashStopped { member: "dev".into() }, "").unwrap();
        set_fake_hgs(None);
        let after = store.load(&t.id).unwrap().unwrap();
        let dev = after.members.iter().find(|member| member.id == "dev").unwrap();
        assert_eq!(dev.restarts.len(), 1);
        assert!(dev.crash.is_some() && dev.crash_told);
        let attempt = after.task(task_id).unwrap().open_attempt().unwrap();
        assert_eq!(attempt.state, AttemptState::Unconfirmed);
        assert!(attempt.error.contains("crashed"), "{}", attempt.error);
        assert_eq!(
            calls.borrow().first().map(Vec::as_slice),
            Some(["resume".to_owned(), "codex/x/dev".to_owned(), "-d".to_owned()].as_slice())
        );
        let feed = store.events(&t.id, None, 50).unwrap().to_string();
        assert!(feed.contains("member_restarted") && feed.contains("member_crashed"), "{feed}");
    });
}

#[test]
fn a_crashed_member_is_restarted_only_when_the_team_allows_it() {
    let mut t = team();
    t.members.iter_mut().find(|member| member.id == "dev").unwrap().seen = Some(MemberSeen { at: 1.0, live: true });
    let crashed = stopped("codex/x/dev", "r1", "stopped");
    // Default off: only the liveness record changes, as today.
    let found = steps(&t, std::slice::from_ref(&crashed), 20.0);
    assert!(found.contains(&Step::Liveness { member: "dev".into(), live: false }), "{found:?}");
    assert!(!found.iter().any(|step| matches!(step, Step::Restart { .. } | Step::CrashStopped { .. })), "{found:?}");
    // On: the same tick asks for the restart.
    t.settings.restart_crashed = true;
    let found = steps(&t, std::slice::from_ref(&crashed), 20.0);
    assert!(found.contains(&Step::Restart { member: "dev".into() }), "{found:?}");
    // Three restarts in the hour: the session stays stopped, once.
    t.members.iter_mut().find(|member| member.id == "dev").unwrap().restarts = vec![10.0, 15.0, 18.0];
    let found = steps(&t, std::slice::from_ref(&crashed), 20.0);
    assert!(found.contains(&Step::CrashStopped { member: "dev".into() }), "{found:?}");
    assert!(!found.iter().any(|step| matches!(step, Step::Restart { .. })));
    t.members.iter_mut().find(|member| member.id == "dev").unwrap().crash_told = true;
    assert!(!steps(&t, std::slice::from_ref(&crashed), 20.0).iter().any(|step| matches!(step, Step::CrashStopped { .. })));
    // An hour later the old restarts no longer count.
    let found = steps(&t, std::slice::from_ref(&crashed), 4000.0);
    assert!(found.contains(&Step::Restart { member: "dev".into() }), "{found:?}");
}

#[test]
fn a_paused_archived_limited_or_unseen_session_is_never_restarted() {
    let mut t = team();
    t.settings.restart_crashed = true;
    for (state, limited) in [("paused", false), ("archived", false), ("stopped", true)] {
        {
            let dev = t.members.iter_mut().find(|member| member.id == "dev").unwrap();
            dev.seen = Some(MemberSeen { at: 1.0, live: true });
            dev.limited = None;
            dev.state = MemberState::Active;
            dev.restarts.clear();
            dev.crash_told = false;
        }
        let mut crashed = stopped("codex/x/dev", "r1", state);
        if limited {
            crashed.limited = Some(SeenLimit { at: 1.0, resets_at: Some(99.0), detail: "usage limit".into() });
        }
        let found = steps(&t, &[crashed], 20.0);
        assert!(!found.iter().any(|step| matches!(step, Step::Restart { .. } | Step::CrashStopped { .. })), "{state}: {found:?}");
    }
    // A member the team paused, or one a limit stopped, is not restarted.
    for (state, limited) in [(MemberState::Paused, None), (MemberState::Active, Some(MemberLimit { at: 1.0, resets_at: Some(99.0), detail: "limit".into() }))] {
        {
            let dev = t.members.iter_mut().find(|member| member.id == "dev").unwrap();
            dev.seen = Some(MemberSeen { at: 1.0, live: true });
            dev.state = state;
            dev.limited = limited;
        }
        let found = steps(&t, &[stopped("codex/x/dev", "r1", "stopped")], 20.0);
        assert!(!found.iter().any(|step| matches!(step, Step::Restart { .. })), "{state:?}: {found:?}");
    }
    // A session not seen live since the last restart does not loop.
    {
        let dev = t.members.iter_mut().find(|member| member.id == "dev").unwrap();
        dev.state = MemberState::Active;
        dev.limited = None;
        dev.seen = Some(MemberSeen { at: 1.0, live: false });
    }
    assert!(!steps(&t, &[stopped("codex/x/dev", "r1", "stopped")], 20.0).iter().any(|step| matches!(step, Step::Restart { .. })));
}

#[test]
fn a_member_prompt_snapshot_wins_over_the_live_catalog() {
    let mut t = team();
    t.members.iter_mut().find(|member| member.id == "dev").unwrap().prompt = "Backend prompt".into();
    t.members.iter_mut().find(|member| member.id == "dev").unwrap().prompt_snapshot = Some(crate::state::teams::model::PromptSnapshot {
        prompt_order: vec!["protocol".into()],
        protocol_extra: "SNAPSHOT EXTRA".into(),
        ..Default::default()
    });
    let task = t.task(1).unwrap().clone();
    let attempt = Attempt { n: 1, id: "a1".into(), member: "dev".into(), ..Default::default() };
    let text = hand_off_text(&t, &task, &attempt, "", "");
    assert!(text.contains("SNAPSHOT EXTRA"), "{text}");
    // An old member without a snapshot keeps today's live catalog text.
    t.members.iter_mut().find(|member| member.id == "dev").unwrap().prompt_snapshot = None;
    let text = hand_off_text(&t, &task, &attempt, "", "");
    assert!(!text.contains("SNAPSHOT EXTRA"), "{text}");
}

#[test]
fn a_stage_hand_off_names_the_change_the_folder_and_the_verdict() {
    // U36/#536: a review or test hand-off says what to read, where, what a
    // verdict contains and that a stage never edits the task's files.
    let mut t = team();
    {
        let dev = t.members.iter_mut().find(|member| member.id == "dev").unwrap();
        dev.directory = "/work/checkout-dev".into();
        dev.folder = MemberFolder::Worktree;
    }
    {
        let task = t.tasks.iter_mut().find(|task| task.id == 1).unwrap();
        task.description = "Build the cards API".into();
        task.criteria = vec!["Paged list".into(), "No N+1".into()];
        let mut attempt = Attempt { n: 1, id: "a1".into(), member: "dev".into(), state: AttemptState::Reported, ..Default::default() };
        attempt.commits.push(CommitRef { sha: "abc1234".into(), files: 2, ..Default::default() });
        attempt.report = Some(Report { text: "Changed src/cards.rs and src/db.rs".into(), at: 1.0, ..Default::default() });
        task.attempts.push(attempt);
        task.pipeline_run = Some(PipelineRun {
            id: "code".into(),
            name: "Code: review, test, lead".into(),
            max_rounds: 3,
            round: 1,
            folder: "hold".into(),
            accept: "lead".into(),
            ..Default::default()
        });
    }
    let task = t.task(1).unwrap().clone();
    let review = StageRun {
        id: "stage-run-1".into(),
        stage: 0,
        kind: "review".into(),
        round: 1,
        attempt: 1,
        member: "qa".into(),
        state: StageState::Delivered,
        ..Default::default()
    };
    let text = stage_hand_off_text(&t, &task, &review);
    for part in [
        "from: Lead",
        "task: #1",
        "stage: review",
        "round 1 of 3",
        "Your role: Tester",
        "Build the cards API",
        "Acceptance criteria:",
        "Paged list",
        "No N+1",
        "Changed src/cards.rs and src/db.rs",
        "/work/checkout-dev",
        "abc1234",
        "git show",
        "review stage",
        "Change nothing in the task's files",
        "\"verdict\":\"pass|return\"",
        "\"attempt\":\"stage-run-1\"",
        "return sends your findings to the implementer",
        "hgs team report --json",
        "hgs team ask --json",
        "hgs team current --json",
    ] {
        assert!(text.contains(part), "{part} missing in\n{text}");
    }
    assert!(!text.contains("shared folder"), "{text}");
    // A test stage says what to run and that the folder is held.
    let test = StageRun { kind: "test".into(), ..review.clone() };
    let text = stage_hand_off_text(&t, &task, &test);
    for part in ["stage: test", "only the suites of the files", "take the criteria's shots", "folder is held"] {
        assert!(text.contains(part), "{part} missing in\n{text}");
    }
    // A shared folder says which files are the task's, and no commits names
    // the report's changed-file list instead.
    t.members.iter_mut().find(|member| member.id == "dev").unwrap().folder = MemberFolder::Shared;
    let text = stage_hand_off_text(&t, &t.task(1).unwrap().clone(), &review);
    assert!(text.contains("shared folder") && text.contains("only the files the report names"), "{text}");
    let mut without = t.clone();
    without.tasks.iter_mut().find(|task| task.id == 1).unwrap().attempts[0].commits.clear();
    let text = stage_hand_off_text(&without, &without.task(1).unwrap().clone(), &review);
    assert!(text.contains("No commits were recorded"), "{text}");
}

/// #728 (live, 2026-10-31): frontend waited in one long Bash call polling
/// its own test run and was marked stuck. A member inside a running tool
/// call is working until the much longer tool bound, counted from the
/// call's start; a tool phase with no running call has the usual bound.
#[test]
fn a_member_waiting_in_its_own_running_tool_call_is_not_stuck() {
    let t = team();
    let at = 100_000.0;
    let lead = session("claude/x/lead", "r0", true);
    let mut dev = session("codex/x/dev", "r1", false);
    dev.activity = "busy".into();
    dev.phase = "tool".into();
    dev.turn_started = Some(at - 50.0 * 60.0);
    dev.tool = "Bash".into();
    dev.tool_started = Some(at - 40.0 * 60.0);
    dev.last_event_at = Some(at - 40.0 * 60.0);
    let marked = |seen: &Seen| steps(&t, &[lead.clone(), seen.clone()], at).into_iter().find_map(|step| match step {
        Step::MarkStuck { since, .. } => Some(since),
        _ => None,
    });
    assert_eq!(marked(&dev), None, "40 min in one running Bash call is work");
    assert_eq!(tool_stuck_minutes(&t), 60.0);
    // Past the tool bound it is stuck, since the call's start.
    let mut long = dev.clone();
    long.tool_started = Some(at - 61.0 * 60.0);
    long.last_event_at = Some(at - 61.0 * 60.0);
    assert_eq!(marked(&long), Some(at - 61.0 * 60.0));
    // A tool phase with no running call: the usual 10-minute bound.
    let mut idle_tool = dev.clone();
    idle_tool.tool.clear();
    idle_tool.tool_started = None;
    assert_eq!(marked(&idle_tool), Some(at - 40.0 * 60.0));
    // A tool started before this turn is not this turn's running call.
    let mut old = dev.clone();
    old.tool_started = Some(at - 55.0 * 60.0);
    assert!(marked(&old).is_some());
}
