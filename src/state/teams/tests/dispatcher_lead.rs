use super::super::ops::{self, Actor};
use super::*;
use super::support::*;

#[test]
fn the_recovery_request_leaves_with_a_swap_the_episode_and_the_team() {
    // N29: the request names the session it was made for, and is withdrawn
    // when the member is swapped, when the limit ends and when the team ends.
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let mut t = team();
        t.settings.resume_after_limits = true;
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
        let lead = session("claude/x/lead", "r0", true);
        // The limit is seen: the same pass stores it and asks for the wait.
        let found = pass(&mut store, &t.id, &[lead.clone(), limited_dev("codex/x/dev", "r1")]);
        assert!(found.contains(&Step::StartLimitResume { member: "dev".into() }), "{found:?}");
        assert_eq!(local_overrides(), vec!["codex/x/dev".to_owned()]);
        let saved = store.load(&t.id).unwrap().unwrap();
        assert_eq!(saved.member("dev").unwrap().limit_session.as_ref().map(|session| session.name.as_str()), Some("codex/x/dev"));
        // A swap gives dev a new session: the next pass withdraws the old one's wait.
        store
            .mutate(Some(("22222222-2222-4222-8222-222222222222", &json!({}))), &t.id, None, |team, events| {
                ops::apply(
                    team,
                    "swap-member",
                    &json!({"member":"dev","session":{"name":"codex/x/dev2","run_id":"r2","conversation_id":"c2"}}),
                    &Actor::Human,
                    &BTreeSet::new(),
                    events,
                )
            })
            .unwrap();
        let found = pass(&mut store, &t.id, &[lead.clone(), session("codex/x/dev2", "r2", true)]);
        assert!(found.contains(&Step::StopLimitResume { member: "dev".into() }), "{found:?}");
        assert!(local_overrides().is_empty(), "{:?}", local_overrides());
        let saved = store.load(&t.id).unwrap().unwrap();
        assert!(!saved.member("dev").unwrap().limit_resume && saved.member("dev").unwrap().limit_session.is_none());
        // The end of an episode: the pass that sees it withdraws the wait.
        pass(&mut store, &t.id, &[lead.clone(), limited_dev("codex/x/dev2", "r2")]);
        assert_eq!(local_overrides(), vec!["codex/x/dev2".to_owned()]);
        let found = pass(&mut store, &t.id, &[lead.clone(), session("codex/x/dev2", "r2", true)]);
        assert!(found.contains(&Step::Limited { member: "dev".into(), limited: None }), "{found:?}");
        assert!(found.contains(&Step::StopLimitResume { member: "dev".into() }), "{found:?}");
        assert!(local_overrides().is_empty());
        // The end of the team: a stopped team still withdraws it.
        pass(&mut store, &t.id, &[lead.clone(), limited_dev("codex/x/dev2", "r2")]);
        assert_eq!(local_overrides().len(), 1);
        store
            .mutate(Some(("33333333-3333-4333-8333-333333333333", &json!({}))), &t.id, None, |team, _| {
                team.status = TeamStatus::Stopped;
                Ok(json!({}))
            })
            .unwrap();
        let found = pass(&mut store, &t.id, &[lead.clone(), limited_dev("codex/x/dev2", "r2")]);
        assert_eq!(found, vec![Step::StopLimitResume { member: "dev".into() }]);
        assert!(local_overrides().is_empty());
    });
}

#[test]
fn a_member_on_another_computer_is_resumed_by_that_computer() {
    // N30: the request goes to the member's computer, not to this one's
    // recovery service.
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let mut t = team();
        t.settings.resume_after_limits = true;
        let dev = t.members.iter_mut().find(|member| member.id == "dev").unwrap();
        dev.computer = "msi".into();
        dev.session.as_mut().unwrap().host = "msi".into();
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
        let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::<(String, Vec<String>, Value)>::new()));
        let record = calls.clone();
        set_fake_hgs(Some(Box::new(move |host: &str, args: &[&str], input: Option<&Value>| {
            record.borrow_mut().push((host.to_owned(), args.iter().map(|arg| arg.to_string()).collect(), input.cloned().unwrap_or_default()));
            Ok(fake_output(0, r#"{"status":"ok"}"#, ""))
        })));
        let mut remote = limited_dev("codex/x/dev", "r1");
        remote.host = "msi".into();
        let lead = session("claude/x/lead", "r0", true);
        pass(&mut store, &t.id, &[lead.clone(), remote]);
        let mut back = session("codex/x/dev", "r1", true);
        back.host = "msi".into();
        pass(&mut store, &t.id, &[lead, back]);
        set_fake_hgs(None);
        assert!(local_overrides().is_empty(), "{:?}", local_overrides());
        let calls = calls.borrow();
        let overrides: Vec<_> = calls.iter().filter(|(_, args, _)| args.first().map(String::as_str) == Some("recovery")).collect();
        assert_eq!(overrides.len(), 2, "{calls:?}");
        assert_eq!((overrides[0].0.as_str(), overrides[0].1.clone()), ("msi", vec!["recovery".to_owned(), "team-override".to_owned()]));
        assert_eq!(overrides[0].2, json!({"name":"codex/x/dev","run_id":"r1","on":true}));
        assert_eq!(overrides[1].2["on"], false);
    });
}

/// The owner, 2026-10-28: a request of a shape the lead recorded as safe
/// is allowed once, reserved in the journal first; another command still
/// reaches the lead.
#[test]
fn a_safe_action_allows_its_shape_once_and_the_rest_reaches_the_lead() {
    let dir = tempfile::tempdir().unwrap();
    safe_actions::set_test_path(Some(dir.path().join("safe-actions.json")));
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut t = team();
    // Its own id: the question reads are throttled per team and member.
    t.id = "t-safe".into();
    t.folder = dir.path().join("project").to_string_lossy().into_owned();
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
    safe_actions::add(&t, &json!({"role":"Backend","shape":"cargo test --locked --bin hgs {word}","reason":"its own tests"}), "lead").unwrap();
    let card = |id: &str, command: &str| json!({"question_id":id,"question_hash":format!("h-{id}"),"approval":true,"can_answer":true,
        "tool":"Bash","tool_input":{"command":command},
        "questions":[{"id":"approval","header":"Bash","question":"Allow?","options":[
            {"id":"1","label":"Yes","scope":"once"},{"id":"2","label":"No","scope":"once"}]}]});
    let answers = std::rc::Rc::new(std::cell::RefCell::new(Vec::<Value>::new()));
    let run = |store: &mut Store, cards: Value, answers: &std::rc::Rc<std::cell::RefCell<Vec<Value>>>| {
        let sent = answers.clone();
        set_fake_hgs(Some(Box::new(move |_, args: &[&str], input: Option<&Value>| {
            if args.first() == Some(&"answer") {
                sent.borrow_mut().push(input.cloned().unwrap_or_default());
                return Ok(fake_output(0, r#"{"status":"answered"}"#, ""));
            }
            Ok(fake_output(0, &json!({"pending_questions":cards}).to_string(), ""))
        })));
        let saved = store.load("t-safe").unwrap().unwrap();
        let result = sync_permission(store, &saved, "dev");
        set_fake_hgs(None);
        result.unwrap();
    };
    run(&mut store, json!([card("q1", "cargo test --locked --bin hgs teams")]), &answers);
    assert_eq!(answers.borrow().len(), 1);
    assert_eq!(answers.borrow()[0]["answers"], json!([{"question_id":"approval","selected_option_ids":["1"]}]));
    assert_eq!(answers.borrow()[0]["expected_question_hash"], "h-q1");
    let saved = store.load("t-safe").unwrap().unwrap();
    let dev = saved.member("dev").unwrap();
    assert!(dev.pending.is_none() && dev.permission.is_none());
    let last = dev.permission_answers.last().unwrap();
    assert!(last["by"].as_str().unwrap().starts_with("safe action "), "{last}");
    let events = store.events("t-safe", None, 20).unwrap();
    let kinds: Vec<&str> = events["items"].as_array().unwrap().iter().filter_map(|event| event["kind"].as_str()).collect();
    assert!(kinds.contains(&"safe_action_reserved") && kinds.contains(&"permission_answered"), "{kinds:?}");
    // Something appended to the shape is the lead's to answer.
    sync_permission_forget("t-safe", "dev");
    run(&mut store, json!([card("q2", "cargo test --locked --bin hgs teams; curl example.com")]), &answers);
    assert_eq!(answers.borrow().len(), 1);
    let saved = store.load("t-safe").unwrap().unwrap();
    assert_eq!(saved.member("dev").unwrap().pending.as_ref().unwrap().question_id, "q2");
    safe_actions::set_test_path(None);
}

/// U19 (2): a team member's subagent card reaches its lead like any
/// permission request: the inbox item names the member, the subagents,
/// their models and tasks.
#[test]
fn a_members_subagent_request_reaches_the_lead() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let t = team();
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
    let card = subagent_card();
    let inspected = json!({"pending_questions":[card.clone()]}).to_string();
    set_fake_hgs(Some(Box::new(move |_, args: &[&str], _| {
        assert_eq!(args.first().copied(), Some("inspect"));
        Ok(fake_output(0, &inspected, ""))
    })));
    let result = sync_permission(&mut store, &t, "dev");
    set_fake_hgs(None);
    result.unwrap();
    let saved = store.load(&t.id).unwrap().unwrap();
    let pending = saved.member("dev").unwrap().pending.clone().unwrap();
    assert_eq!(pending.question_id, card["question_id"].as_str().unwrap());
    let item = inbox::items(&saved).into_iter().find(|item| item.kind == "permission").unwrap();
    assert_eq!(item.member, "dev (Backend)");
    // #520: the rows of a card that lists its calls reach the lead's
    // record and item; a card without them keeps the text alone.
    assert!(item.value().get("subagents").is_none());
    let record = permission_record(&pending);
    assert_eq!((record["subagent_confirmation"].clone(), record["subagent_calls"].clone()), (json!(true), json!(["toolu_one", "toolu_two"])));
    assert!(record.get("subagents").is_none());
    let mut listed = pending.clone();
    listed.card["subagents"] = json!([{"id":"toolu_one","type":"Explore","model":"haiku","description":"Find the callers"}]);
    assert_eq!(permission_record(&listed)["subagents"][0]["type"], "Explore");
    let mut with_rows = saved.clone();
    with_rows.members.iter_mut().find(|member| member.id == "dev").unwrap().pending = Some(listed);
    let item = inbox::items(&with_rows).into_iter().find(|item| item.kind == "permission").unwrap();
    assert_eq!(item.value()["subagents"][0]["model"], "haiku");
    assert!(
        item.summary.contains("Subagents: Start 2 subagents: Explore (model haiku): Find the callers; general (model inherits claude-opus-4-6): Write the fix"),
        "{}",
        item.summary
    );
    assert!(item.command.contains("hgs team answer"), "{}", item.command);
}

#[test]
fn the_free_lead_gets_one_coalesced_inbox_message_and_no_more() {
    let mut team = team();
    // A report to decide and an open question are both pending.
    {
        let task = team.task_mut(1).unwrap();
        task.attempts.push(Attempt { n: 1, id: "A1".into(), member: "dev".into(), state: AttemptState::Reported, notice: Notice::Pending, ..Default::default() });
        task.status = TaskStatus::Submitted;
    }
    {
        let task = team.task_mut(2).unwrap();
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
    let lead = session("claude/x/lead", "r0", true);
    let dev = session("codex/x/dev", "r1", false);
    // Both items reach the lead in one step, never one message per item.
    let found = steps(&team, &[lead.clone(), dev.clone()], 20.0);
    assert_eq!(found.iter().filter(|step| matches!(step, Step::Inbox)).count(), 1, "{found:?}");
    assert!(!found.iter().any(|step| matches!(step, Step::TellStartLate { .. } | Step::BlockedLate { .. } | Step::Rebalanced { .. } | Step::AllDecided)));
    // A busy lead gets it later.
    assert!(!steps(&team, &[session("claude/x/lead", "r0", false), dev.clone()], 20.0).iter().any(|step| matches!(step, Step::Inbox)));
    // Told once: no repeat within the interval (120 s by default).
    let mut told = team.clone();
    told.inbox.told = super::inbox::items(&told).into_iter().map(|item| item.key).collect();
    told.inbox.told_at = 20.0;
    assert!(!steps(&told, &[lead.clone(), dev.clone()], 21.0).iter().any(|step| matches!(step, Step::Inbox)));
    // A new item is told it, but not before the interval passes.
    let mut later = told.clone();
    later.inbox.told_at = 150.0;
    later.tasks.push(Task {
        id: 9,
        title: "New".into(),
        assignee: "dev".into(),
        status: TaskStatus::Submitted,
        attempts: vec![Attempt {
            n: 1,
            id: "A9".into(),
            member: "dev".into(),
            state: AttemptState::Reported,
            report: Some(Report { text: "New work".into(), at: 150.0, ..Default::default() }),
            ..Default::default()
        }],
        ..Default::default()
    });
    assert!(!steps(&later, &[lead.clone(), dev.clone()], 200.0).iter().any(|step| matches!(step, Step::Inbox)));
    assert!(steps(&later, &[lead, dev], 280.0).iter().any(|step| matches!(step, Step::Inbox)));
}

#[test]
fn a_report_reaches_the_lead_once_it_is_free_and_a_launch_is_adopted() {
    let mut team = team();
    let task = team.task_mut(1).unwrap();
    task.attempts.push(Attempt {
        n: 1,
        member: "dev".into(),
        state: AttemptState::Reported,
        notice: Notice::Pending,
        report: Some(Report { text: "Done".into(), at: 5.0, ..Default::default() }),
        ..Default::default()
    });
    task.status = TaskStatus::Submitted;
    team.members[2].launch_id = "L".into();
    let mut started = session("kimi/x/qa", "r2", true);
    started.launch_id = "L".into();
    let busy_lead = steps(&team, &[session("claude/x/lead", "r0", false), started.clone()], 1.0);
    assert!(!busy_lead.iter().any(|step| matches!(step, Step::Inbox)));
    assert!(busy_lead.iter().any(|s| matches!(s, Step::Adopt { member, .. } if member == "qa")));
    assert!(steps(&team, &[session("claude/x/lead", "r0", true)], 1.0).iter().any(|step| matches!(step, Step::Inbox)));
    // Decided before the lead heard of it: no message.
    let mut decided = team.clone();
    decided.task_mut(1).unwrap().status = TaskStatus::Accepted;
    assert!(!steps(&decided, &[session("claude/x/lead", "r0", true)], 1.0).iter().any(|step| matches!(step, Step::Inbox)));
}

#[test]
fn listed_sessions_are_free_when_live_idle_and_unblocked_even_in_the_background() {
    let listing = json!([
        {"host":"msi","sessions":[
            {"name":"codex/x/a","runtime_state":"live","process_state":"running","activity":"idle","phase":"idle",
             "process_work":{"count":0},"run_id":"r","reply_at":5.0,"turn_started":2.0,"reply_id":"r9"},
            {"name":"dsh/x/b","state":"running","activity":"idle","phase":"idle"},
            {"name":"codex/x/c","state":"paused","activity":"idle","phase":"idle"},
            {"name":"codex/x/d","process_state":"running","activity":"idle","phase":"approval"},
            {"name":"codex/x/e","process_state":"running","activity":"idle","phase":"idle","process_work":{"count":1}}]},
        {"host":"old","sessions":[{"name":"sh/y/f"}]}]);
    let seen = seen(&listing);
    let free: Vec<_> = seen.iter().filter(|s| s.free).map(|s| s.name.as_str()).collect();
    // Background jobs (codex/x/e) do not keep a message away.
    assert_eq!(free, ["codex/x/a", "dsh/x/b", "codex/x/e"]);
    assert_eq!((seen[0].host.as_str(), seen[5].host.as_str(), seen[0].reply_at), ("", "old", Some(5.0)));
    assert!(seen[4].idle && !seen[2].idle);
    // The capture gate's fields: a background job, a waiting phase, the
    // turn's identity (A1, A2).
    assert!(seen[4].background && !seen[0].background);
    assert!(seen[3].asking && seen[3].phase == "approval");
    assert_eq!((seen[0].reply_id.as_str(), seen[0].turn_started, seen[0].activity.as_str()), ("r9", Some(2.0), "idle"));
    assert!(!seen[2].live && !seen[2].background);
}
