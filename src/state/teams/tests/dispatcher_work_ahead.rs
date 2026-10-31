use super::*;

#[test]
fn a_team_without_the_new_fields_hands_out_reports_stages_and_accepts_as_today() {
    // Spec 0: none of the U47/U48 fields set: today's hand-out, report,
    // review stage and accept, today's events, and the stored shape keeps
    // none of the new keys.
    let root = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(root.path(), || {
        let (lead, dev) = stage_repo(root.path());
        let team = stage_team(&lead, &dev, "docs", &|_| super::super::super::catalog::builtin());
        let stored = serde_json::to_string(&team).unwrap();
        for key in ["land_allowed", "branch_per_task", "base_branch", "\"prepare\"", "\"waits\"", "\"exits\"", "\"command\"", "\"flaky\"", "\"outcome\""] {
            assert!(!stored.contains(key), "{key} in {stored}");
        }
        let mut store = Store::open_at(&super::super::super::store::path(), true).unwrap();
        store.create(&uuid::Uuid::new_v4().to_string(), &json!({}), team, vec![]).unwrap();
        deliver_all();
        hand_out(&mut store, STAGE_TEAM, 1, "dev", None, "").unwrap();
        stage_report(&mut store, 1);
        let team = store.load(STAGE_TEAM).unwrap().unwrap();
        assert!(!steps(&team, &[], now()).iter().any(|step| matches!(step, Step::StageCommand { .. } | Step::StageCommandEnded { .. })));
        assert!(team.task(1).unwrap().attempts[0].prepare.is_empty());
        assert_eq!(git_in(&dev, &["rev-parse", "--abbrev-ref", "HEAD"]), format!("team/{}/dev", &STAGE_TEAM[..8]));
        store
            .mutate(None, STAGE_TEAM, None, |team, events| {
                let stage = super::super::super::pipeline::give_stage(team, 1, "rev")?;
                ops::apply(team, "report", &json!({"task":1,"attempt":stage,"verdict":"pass","text":"Read it"}), &Actor::Member("rev".into()), &BTreeSet::new(), events)
            })
            .unwrap();
        set_fake_hgs(None);
        let team = store.load(STAGE_TEAM).unwrap().unwrap();
        let task = team.task(1).unwrap();
        assert_eq!((task.status, task.decided_by), (TaskStatus::Accepted, None), "the docs preset accepts on its own, as today");
        assert_eq!(feed_kinds(&store), ["handed_out", "delivered", "reported", "stage_started", "stage_passed", "accepted"]);
        let stored = serde_json::to_string(&team).unwrap();
        for key in ["land_allowed", "branch_per_task", "\"prepare\"", "\"waits\"", "\"check\":{", "\"flaky\"", "\"outcome\""] {
            assert!(!stored.contains(key), "{key} in the stored team");
        }
    });
}

#[test]
fn a_zerus_build_keeps_the_turn_open_and_reaches_the_lead_as_a_block() {
    // #511 N1: a member that ends its turn with `hgs run` or `hgs team run`
    // still building is Building: its reply is not the report, and a long
    // build reaches the lead like Background work.
    let listing = json!([{"sessions":[
        {"name":"codex/x/dev","run_id":"r1","state":"running","activity":"idle","phase":"idle",
            "build":{"source":"team_run","request_id":"b1","label":"cargo-test","command":"cargo test","since":40.0}},
        {"name":"claude/x/lead","run_id":"r0","state":"running","activity":"idle","phase":"idle","build":null}]}]);
    let listed = seen(&listing);
    assert!(listed[0].building && !listed[0].background && listed[0].free, "a build is work, not Background, and the member takes messages");
    assert!(!listed[1].building);
    let mut team = team();
    delivered(&mut team);
    let mut building = session("codex/x/dev", "r1", true);
    building.reply_at = Some(50.0);
    building.reply_id = "r7".into();
    building.building = true;
    // The finished turn is not captured; the block starts instead.
    let found = steps(&team, &[building.clone()], 60.0);
    assert!(!found.iter().any(|step| matches!(step, Step::TurnEnded { .. })), "{found:?}");
    assert!(found.contains(&Step::GateClosed { task: 1 }), "{found:?}");
    let mut done = building.clone();
    done.building = false;
    assert!(steps(&team, &[done.clone()], 60.0).contains(&Step::TurnEnded { task: 1, at: 50.0, reply: "r7".into() }));
    // A recorded turn end waits too, and never times out into No report.
    let timeout = team.settings.report_timeout_seconds as f64;
    {
        let attempt = &mut team.task_mut(1).unwrap().attempts[0];
        attempt.turn_ended_at = Some(50.0);
        attempt.turn_reply = "r7".into();
        attempt.report_ready_at = Some(50.0);
    }
    assert!(!steps(&team, &[building.clone()], 50.0 + timeout).iter().any(|step| matches!(step, Step::NoReport { .. })));
    assert!(steps(&team, &[done], 50.0 + timeout).contains(&Step::NoReport { task: 1 }));
    // The blocked reminder counts the build as well.
    team.task_mut(1).unwrap().attempts[0].blocked_since = Some(50.0);
    let lead = session("claude/x/lead", "r0", true);
    let blocked = team.settings.blocked_timeout_seconds as f64;
    assert!(steps(&team, &[lead, building], 50.0 + blocked).contains(&Step::BlockedLate { task: 1 }));
    let task = team.task(1).unwrap();
    assert!(blocked_text(&team, task, &task.attempts[0], 50.0 + blocked).contains("a Zerus build still runs"));
}

#[test]
fn work_ahead_waits_for_every_dependency_even_inside_the_draft_phase() {
    // #511 N2: #5 depends on #4 of the same draft phase; #6 on a running task
    // that is not accepted, even by a soft dependency.
    let mut t = work_ahead_team();
    let seen = vec![session("claude/x/lead", "r0", true), session("codex/x/dev", "r1", true), session("kimi/x/qa", "r3", true)];
    assert_eq!(work_ahead_tasks(&t), [4]);
    let found = steps(&t, &seen, 100.0);
    assert!(found.contains(&Step::WorkAhead { task: 4, member: "dev".into() }), "{found:?}");
    assert!(!found.iter().any(|step| matches!(step, Step::WorkAhead { task: 5 | 6, .. })), "qa waits for #4: {found:?}");
    // Once #4 is accepted, #5 may go ahead.
    t.task_mut(4).unwrap().status = TaskStatus::Accepted;
    assert!(steps(&t, &seen, 100.0).contains(&Step::WorkAhead { task: 5, member: "qa".into() }));
    t.task_mut(1).unwrap().status = TaskStatus::Accepted;
    assert_eq!(work_ahead_tasks(&t), [5, 6]);
    t.settings.work_ahead = false;
    assert!(work_ahead_tasks(&t).is_empty());
}

#[test]
fn a_work_ahead_step_hands_the_draft_task_out() {
    // U31: a draft phase's task is Draft, which plan::ready never lists; the
    // hand-out takes it all the same, and only while it may go ahead.
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut t = work_ahead_team();
        t.id = "88888888-8888-4888-8888-888888888888".into();
        assert_eq!(t.task(4).unwrap().status, TaskStatus::Draft);
        let mut store = Store::open_at(&super::super::super::store::path(), true).unwrap();
        store.create(&uuid::Uuid::new_v4().to_string(), &json!({}), t.clone(), vec![]).unwrap();
        set_fake_hgs(Some(Box::new(|_, args: &[&str], _| {
            assert_eq!(args.first().copied(), Some("send"));
            Ok(fake_output(0, r#"{"status":"submitted"}"#, ""))
        })));
        run_step(&mut store, &t, &Step::WorkAhead { task: 4, member: "dev".into() }, "").unwrap();
        // #5 waits for #4: its hand-out is refused, nothing is sent.
        let refused = hand_out(&mut store, &t.id, 5, "qa", None, "");
        set_fake_hgs(None);
        assert!(refused.unwrap_err().to_string().contains("no longer ready"));
        let saved = store.load(&t.id).unwrap().unwrap();
        let task = saved.task(4).unwrap();
        assert_eq!((task.status, task.attempts.len()), (TaskStatus::InProgress, 1));
        assert!(task.attempts[0].worked_ahead && task.attempts[0].state == AttemptState::Delivered);
        assert!(saved.task(5).unwrap().attempts.is_empty());
    });
}

#[test]
fn a_phase_sent_back_tells_the_member_working_ahead_to_stop() {
    // #511 N3: the revoked attempt's member hears it once, under its own
    // request id; a refusal is tried again, an uncertain send never.
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut t = work_ahead_team();
        t.id = "99999999-9999-4999-8999-999999999999".into();
        {
            let task = t.task_mut(4).unwrap();
            task.status = TaskStatus::InProgress;
            task.attempts.push(Attempt { n: 1, id: "a4".into(), member: "dev".into(), state: AttemptState::Delivered, worked_ahead: true, ..Default::default() });
        }
        let mut events = Vec::new();
        ops::apply(&mut t, "reject-phase", &json!({"phase":2,"comment":"Split the docs"}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
        let task = t.task(4).unwrap();
        assert_eq!((task.status, task.attempts.len()), (TaskStatus::Draft, 0));
        assert_eq!(t.stops.len(), 1);
        let stop = t.stops[0].clone();
        assert_eq!((stop.task, stop.member.as_str(), stop.attempt, stop.notice), (4, "dev", 1, Notice::Pending));
        assert!(events.iter().any(|event| event.kind == "work_ahead_revoked" && event.payload["member"] == "Backend"));
        let text = stop_text(&t, &stop);
        assert!(text.contains("Stop working on task #4 Independent docs: phase 2 was sent back") && text.contains("Do not report it"), "{text}");
        // A busy member's session still gets it (it waits behind the turn);
        // a stopped session does not.
        let mut busy = session("codex/x/dev", "r1", false);
        busy.idle = false;
        assert!(steps(&t, &[busy], 100.0).contains(&Step::TellStop { stop: stop.id.clone() }));
        let mut gone = session("codex/x/dev", "r1", false);
        gone.live = false;
        assert!(!steps(&t, &[gone], 100.0).iter().any(|step| matches!(step, Step::TellStop { .. })));
        let mut store = Store::open_at(&super::super::super::store::path(), true).unwrap();
        store.create(&uuid::Uuid::new_v4().to_string(), &json!({}), t.clone(), vec![]).unwrap();
        let sent = std::rc::Rc::new(std::cell::RefCell::new(Vec::<Value>::new()));
        let record = sent.clone();
        let answer = std::rc::Rc::new(std::cell::Cell::new(1));
        let answering = answer.clone();
        set_fake_hgs(Some(Box::new(move |_, args: &[&str], input: Option<&Value>| {
            assert_eq!(args.first().copied(), Some("send"));
            record.borrow_mut().push(input.cloned().unwrap_or_default());
            Ok(match answering.get() {
                1 => fake_output(1, "", "hgs: the session is busy"),
                2 => fake_output(0, r#"{"status":"submitted"}"#, ""),
                _ => fake_output(1, "", "hgs: delivery uncertain"),
            })
        })));
        tell_stop(&mut store, &t, &stop.id).unwrap();
        assert_eq!(store.load(&t.id).unwrap().unwrap().stops[0].notice, Notice::Pending, "a refusal is tried again");
        answer.set(2);
        let current = store.load(&t.id).unwrap().unwrap();
        tell_stop(&mut store, &current, &stop.id).unwrap();
        let saved = store.load(&t.id).unwrap().unwrap();
        assert_eq!(saved.stops[0].notice, Notice::Sent);
        // Sent once: nothing more goes out.
        tell_stop(&mut store, &saved, &stop.id).unwrap();
        assert!(!steps(&saved, &[session("codex/x/dev", "r1", true)], 100.0).iter().any(|step| matches!(step, Step::TellStop { .. })));
        // An uncertain send is left unconfirmed and never repeated.
        store
            .mutate(None, &t.id, None, |team, _| {
                team.stops[0].notice = Notice::Pending;
                Ok(Value::Null)
            })
            .unwrap();
        answer.set(3);
        let current = store.load(&t.id).unwrap().unwrap();
        tell_stop(&mut store, &current, &stop.id).unwrap();
        set_fake_hgs(None);
        assert_eq!(store.load(&t.id).unwrap().unwrap().stops[0].notice, Notice::Unconfirmed);
        let sent = sent.borrow();
        assert_eq!(sent.len(), 3);
        assert!(sent.iter().all(|input| input["request_id"] == stop.id.as_str()), "one request id for the notice");
        // A team that never worked ahead stores no stops.
        assert!(!serde_json::to_string(&team()).unwrap().contains("\"stops\""));
    });
}

#[test]
fn edit_draft_refuses_a_task_with_a_live_worked_ahead_attempt() {
    // #511 N3, second half: a worked-ahead task is handed out, so it is no
    // longer Draft; edit-draft changes and removes only Draft tasks
    // (ops.rs edit_draft). Its text never changes under the member.
    let mut t = work_ahead_team();
    {
        let task = t.task_mut(4).unwrap();
        task.status = TaskStatus::InProgress;
        task.attempts.push(Attempt { n: 1, id: "a4".into(), member: "dev".into(), state: AttemptState::Delivered, worked_ahead: true, ..Default::default() });
    }
    let phase = t.task(4).unwrap().phase;
    let mut events = Vec::new();
    let lead = Actor::Member("lead".into());
    let error = ops::apply(&mut t, "edit-draft", &json!({"phase":phase,"tasks_update":[{"id":4,"title":"Other docs"}]}), &lead, &BTreeSet::new(), &mut events).unwrap_err();
    assert!(error.to_string().contains("Task #4 is not a draft"), "{error}");
    let error = ops::apply(&mut t, "edit-draft", &json!({"phase":phase,"tasks_remove":[4]}), &lead, &BTreeSet::new(), &mut events).unwrap_err();
    assert!(error.to_string().contains("Task #4 is not a draft"), "{error}");
    assert_eq!(t.task(4).unwrap().title, "Independent docs");
}
