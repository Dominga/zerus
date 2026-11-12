use super::*;

#[test]
fn a_team_without_the_new_fields_hands_out_reports_stages_and_accepts_as_today() {
    // Spec 0: none of the U47/U48 fields set: today's hand-out, report,
    // review stage and accept, today's events, and the stored shape keeps
    // none of the new keys.
    let root = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(root.path(), || {
        let (lead, dev) = stage_repo(root.path());
        let mut team = stage_team(&lead, &dev, "docs", &|_| super::super::super::catalog::builtin());
        // DCB-svn report (2026-11-12): with the merge queue on (the default for a
        // member in its own worktree) an automatic accept queues the landing;
        // this team runs without the queue, as these teams did.
        team.settings.tests = Some(json!({"merge_queue":{"enabled":false}}));
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

fn all_free() -> Vec<Seen> {
    vec![session("claude/x/lead", "r0", true), session("codex/x/dev", "r1", true), session("kimi/x/qa", "r3", true)]
}

fn handed(found: &[Step], task: u32) -> bool {
    found.iter().any(|step| matches!(step, Step::HandOut { task: id, .. } if *id == task))
}

#[test]
fn a_draft_phase_runs_nothing_before_its_approval() {
    // Task 158 (owner, 2026-11-07): there is no work ahead or start ahead: a
    // draft's tasks are Draft and wait for the phase's approval, however idle
    // the members and however independent the tasks.
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut t = draft_phase_team();
        t.id = "88888888-8888-4888-8888-888888888888".into();
        assert_eq!(t.task(4).unwrap().status, TaskStatus::Draft);
        let found = steps(&t, &all_free(), 100.0);
        assert!(!(4..=6).any(|id| handed(&found, id)), "{found:?}");
        let mut store = Store::open_at(&super::super::super::store::path(), true).unwrap();
        store.create(&uuid::Uuid::new_v4().to_string(), &json!({}), t.clone(), vec![]).unwrap();
        set_fake_hgs(Some(Box::new(|_, _: &[&str], _| panic!("nothing is sent for a draft's task"))));
        let refused = hand_out(&mut store, &t.id, 4, "dev", None, "");
        set_fake_hgs(None);
        assert!(refused.unwrap_err().to_string().contains("no longer ready"));
        assert!(store.load(&t.id).unwrap().unwrap().task(4).unwrap().attempts.is_empty());
    });
}

#[test]
fn an_approved_phase_runs_alongside_the_open_phase_before_it() {
    // Task 158: approving phase 2 while phase 1 still has open work no longer
    // queues it: its independent task goes out at once, a task waits only for
    // its own dependencies.
    let mut t = draft_phase_team();
    let mut events = Vec::new();
    let receipt = ops::apply(&mut t, "approve-phase", &json!({"phase":2}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    assert_eq!(receipt["phase_state"], "running", "{receipt}");
    assert_eq!(t.phases.iter().find(|phase| phase.n == 2).unwrap().state, PhaseState::Approved);
    assert!(t.tasks.iter().any(|task| task.phase == 1 && !task.status.finished()), "phase 1 is still open");
    let ready = plan::ready(&t);
    assert!(ready.contains(&4), "{ready:?}");
    assert!(!ready.contains(&5), "#5 waits for #4's acceptance: {ready:?}");
    let found = steps(&t, &all_free(), 100.0);
    assert!(found.contains(&Step::HandOut { task: 4, member: "dev".into() }), "{found:?}");
    assert!(!handed(&found, 5), "{found:?}");
    // The phase reads running, not queued, while phase 1 is open.
    let summary = plan::phase_summary(&t);
    let two = summary.iter().find(|phase| phase["n"] == 2).unwrap();
    assert_eq!(two["state"], "running", "{two}");
    assert!(two.get("start_ahead").is_none(), "{two}");
}

#[test]
fn a_phase_queued_behind_a_running_one_starts_alongside_it() {
    // The case of the owner's analysis: phase 4 was approved while phase 3
    // ran and sat queued with nothing to start it. An older record's
    // "queued" reads as approved, so its ready task goes out now; the old
    // start-ahead and work-ahead fields are dropped.
    let mut t = draft_phase_team();
    let mut events = Vec::new();
    ops::apply(&mut t, "approve-phase", &json!({"phase":2}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    let mut stored = serde_json::to_value(&t).unwrap();
    let two = stored["phases"].as_array_mut().unwrap().iter_mut().find(|phase| phase["n"] == 2).unwrap();
    two["state"] = json!("queued");
    two["start_ahead"] = json!(true);
    stored["settings"]["work_ahead"] = json!(true);
    let loaded: Team = serde_json::from_value(stored).unwrap();
    assert_eq!(loaded.phases.iter().find(|phase| phase.n == 2).unwrap().state, PhaseState::Approved);
    assert!(steps(&loaded, &all_free(), 100.0).contains(&Step::HandOut { task: 4, member: "dev".into() }));
    let saved = serde_json::to_string(&loaded).unwrap();
    for gone in ["\"queued\"", "start_ahead", "work_ahead", "worked_ahead"] {
        assert!(!saved.contains(gone), "{gone} in {saved}");
    }
}

#[test]
fn the_file_overlap_rule_still_holds_across_phases() {
    // Task 155's rule is kept: a phase that writes what an open task of an
    // earlier phase writes is refused until the tasks are chained, and a
    // chained task waits for its predecessor's acceptance.
    let mut t = draft_phase_team();
    t.task_mut(1).unwrap().output = vec!["src/api".into()];
    t.task_mut(4).unwrap().output = vec!["src/api/docs.md".into()];
    let mut events = Vec::new();
    let error = ops::apply(&mut t, "approve-phase", &json!({"phase":2}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap_err();
    assert!(error.to_string().contains("both write"), "{error}");
    t.task_mut(4).unwrap().deps.push(Dependency { task: 1, kind: DependencyKind::Hard });
    ops::apply(&mut t, "approve-phase", &json!({"phase":2}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    assert!(!plan::ready(&t).contains(&4), "#4 waits for #1's acceptance");
    assert!(!handed(&steps(&t, &all_free(), 100.0), 4));
    // An open task holding the outputs keeps an unchained task out at the
    // hand-out too (a record from before the planning check).
    t.task_mut(4).unwrap().deps.clear();
    assert_eq!(plan::output_holders(&t, t.task(4).unwrap()), [1]);
    assert!(!handed(&steps(&t, &all_free(), 100.0), 4));
    t.task_mut(1).unwrap().status = TaskStatus::Accepted;
    assert!(steps(&t, &all_free(), 100.0).contains(&Step::HandOut { task: 4, member: "dev".into() }));
}

#[test]
fn a_barrier_phase_still_waits_for_the_phases_before_it() {
    // Phases are no scheduling barrier unless marked one.
    let mut t = draft_phase_team();
    t.phases.iter_mut().find(|phase| phase.n == 2).unwrap().barrier = true;
    let mut events = Vec::new();
    ops::apply(&mut t, "approve-phase", &json!({"phase":2}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    assert_eq!(plan::barrier_blockers(&t, t.task(4).unwrap()), [1, 2, 3]);
    assert!(!plan::ready(&t).contains(&4));
    for task in t.tasks.iter_mut().filter(|task| task.phase == 1) {
        task.status = TaskStatus::Accepted;
    }
    assert!(plan::ready(&t).contains(&4));
}

#[test]
fn an_auto_draft_is_accepted_and_runs_alongside_without_start_ahead() {
    // U61 and task 158: with Auto-accept an auto draft whose plan is clean is
    // accepted by Auto and runs beside the open phase; start-ahead is gone.
    let mut t = draft_phase_team();
    t.settings.auto_accept = true;
    t.phases.iter_mut().find(|phase| phase.n == 2).unwrap().approval = Approval::Auto;
    assert_eq!(super::super::super::ops::drafts_ready_to_auto_accept(&t, &BTreeSet::new()), [2]);
    let mut events = Vec::new();
    assert!(super::super::super::ops::accept_auto_draft(&mut t, 2, &BTreeSet::new(), &mut events));
    assert_eq!(t.phases.iter().find(|phase| phase.n == 2).unwrap().state, PhaseState::Approved);
    assert_eq!(events.last().unwrap().payload["state"], "running");
    assert!(steps(&t, &all_free(), 100.0).contains(&Step::HandOut { task: 4, member: "dev".into() }));
    let error = ops::apply(&mut t, "start-ahead", &json!({"phase":2}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut events).unwrap_err();
    assert!(error.to_string().contains("Start ahead is gone"), "{error}");
    let error = ops::apply(&mut t, "settings", &json!({"settings":{"work_ahead":true}}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap_err();
    assert!(error.to_string().contains("Work ahead is gone"), "{error}");
    // An older client that sends it switched off changes nothing.
    ops::apply(&mut t, "settings", &json!({"settings":{"work_ahead":false,"auto_accept":true}}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
}

#[test]
fn an_owner_acceptance_gate_still_holds_the_phases_after_it() {
    // U63 is kept: no phase after one that waits for the owner's acceptance
    // starts, alongside or not.
    let mut t = draft_phase_team();
    let mut events = Vec::new();
    ops::apply(&mut t, "approve-phase", &json!({"phase":2}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    assert!(plan::ready(&t).contains(&4));
    let mut gated = t.clone();
    gated.phases.insert(0, Phase { n: 7, order: 1, title: "Gate".into(), state: PhaseState::AwaitingAcceptance, ..Default::default() });
    assert_eq!(plan::acceptance_block(&gated, 2), Some(7));
    assert!(!plan::ready(&gated).contains(&4));
}

#[test]
fn a_phase_sent_back_tells_the_member_working_ahead_to_stop() {
    // Task 158: no draft runs any more; an older record's work ahead on a
    // draft is still revoked when the owner sends the draft back. #511 N3: the revoked attempt's member hears it once, under its own
    // request id; a refusal is tried again, an uncertain send never.
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut t = draft_phase_team();
        t.id = "99999999-9999-4999-8999-999999999999".into();
        {
            let task = t.task_mut(4).unwrap();
            task.status = TaskStatus::InProgress;
            task.attempts.push(Attempt { n: 1, id: "a4".into(), member: "dev".into(), state: AttemptState::Delivered, ..Default::default() });
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
    // #511 N3, second half (an older record's work ahead): a worked-ahead task is handed out, so it is no
    // longer Draft; edit-draft changes and removes only Draft tasks
    // (ops.rs edit_draft). Its text never changes under the member.
    let mut t = draft_phase_team();
    {
        let task = t.task_mut(4).unwrap();
        task.status = TaskStatus::InProgress;
        task.attempts.push(Attempt { n: 1, id: "a4".into(), member: "dev".into(), state: AttemptState::Delivered, ..Default::default() });
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
