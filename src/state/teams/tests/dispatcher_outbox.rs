use super::*;
use super::support::*;

/// Task #239: a queued run whose runner died is answered where the member
/// polls, with the worker log's tail - never "running" forever, never silence.
#[test]
fn a_run_whose_runner_died_is_an_outcome_with_the_worker_log() {
    use crate::state::teams::outbox;
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let folder = dir.path().join("member");
        std::fs::create_dir_all(&folder).unwrap();
        let log = worker_dir().join("worker.log");
        std::fs::create_dir_all(log.parent().unwrap()).unwrap();
        std::fs::write(&log, "start pid=1\nhgs team worker: run 2222: the runner ended while vitest started\n").unwrap();
        let id = "22222222-2222-4222-8222-222222222222";
        let marker = serde_json::json!({"request_id":id,"started":crate::state::now(),"timeout_secs":1800,"pid":4294967290u32});
        outbox::write_running(&folder, id, &marker).unwrap();
        let team = Team { id: "t".into(), name: "T".into(), ..Default::default() };
        settle_stale_runs(&folder, &team);
        let result = outbox::result(&folder, id).expect("the run is answered");
        assert_eq!(result["status"], "refused", "{result}");
        let error = result["error"].as_str().unwrap();
        assert!(error.contains("Zerus's worker log") && error.contains("the runner ended while vitest started"), "{error}");
        assert!(outbox::running(&folder, id).is_none());
        // A runner that still lives is not answered.
        let live = "33333333-3333-4333-8333-333333333333";
        let marker = serde_json::json!({"request_id":live,"started":crate::state::now(),"timeout_secs":1800,"pid":std::process::id()});
        outbox::write_running(&folder, live, &marker).unwrap();
        settle_stale_runs(&folder, &team);
        assert!(outbox::result(&folder, live).is_none() && outbox::running(&folder, live).is_some());
    });
}

#[test]
fn terminal_members_start_with_a_short_first_turn() {
    let team = team();
    let codex = Member { id: "dev".into(), role: "Backend".into(), agent: "codex".into(), launch_id: "L".into(),
        permissions: "bypass".into(), ..Default::default() };
    let args = launch_args(&team, &codex, "D:/w");
    assert_eq!(args[..8], ["codex", "D:/w", "--new", "-n", "checkout-dev", "--launch-id", "L", "-d"]);
    assert!(args.windows(2).any(|w| w == ["--permissions", "bypass"]));
    assert!(args.last().unwrap().contains("You are Backend in the Zerus agent team Checkout"));
    // The role's model and effort go on the agent's own command line.
    let tuned = Member { model: "gpt-5.5".into(), effort: "low".into(), ..codex.clone() };
    let args = launch_args(&team, &tuned, "D:/w");
    let after = &args[args.iter().position(|a| a == "--").unwrap() + 1..];
    assert_eq!(after[..4], ["--model", "gpt-5.5", "-c", "model_reasoning_effort=\"low\""]);
    let claude = Member { agent: "claude".into(), model: "opus".into(), effort: "high".into(), ..codex.clone() };
    assert!(launch_args(&team, &claude, "D:/w").windows(4).any(|w| w == ["--model", "opus", "--effort", "high"]));
    let dsh = Member { agent: "dsh".into(), ..codex };
    assert!(!launch_args(&team, &dsh, "D:/w").iter().any(|a| a == "--" || a == "-d"));
}

#[test]
fn a_queued_report_from_the_members_folder_is_applied_and_the_request_leaves() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut t = team();
    t.status = TeamStatus::Running;
    let task = t.tasks.iter_mut().find(|t| t.assignee == "dev").unwrap();
    let attempt = Attempt { n: 1, id: "A1".into(), member: "dev".into(), state: AttemptState::Delivered, ..Default::default() };
    task.attempts.push(attempt);
    task.status = TaskStatus::InProgress;
    let id = task.id;
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
    let folder = dir.path().join("work");
    let request = json!({"op":"report","request_id":"33333333-3333-4333-8333-333333333333",
        "team":t.id,"member":"dev","attempt":"A1",
        "input":{"request_id":"33333333-3333-4333-8333-333333333333","team":t.id,"task":id,"attempt":"A1","text":"Done and checked"}});
    let path = super::super::outbox::push(&folder, &request).unwrap();
    let mut member = session("codex/x/dev", "r1", true);
    member.cwd = folder.to_string_lossy().into_owned();
    let current = store.load(&t.id).unwrap().unwrap();
    take_outbox(&mut store, &current, &[member]).unwrap();
    let saved = store.load(&t.id).unwrap().unwrap();
    let attempt = saved.task(id).unwrap().attempts.last().unwrap();
    assert_eq!((attempt.state, attempt.report.as_ref().map(|r| r.text.as_str())), (AttemptState::Reported, Some("Done and checked")));
    // The member's request is gone and its outcome waits in the same folder.
    assert!(!path.exists());
    let result = super::super::outbox::result(&folder, "33333333-3333-4333-8333-333333333333").unwrap();
    assert_eq!(result["status"], "submitted");
    assert_eq!(result["request_id"], "33333333-3333-4333-8333-333333333333");
}

#[test]
fn a_sub_lead_hands_out_its_reports_tasks_and_hears_its_own_inbox() {
    let mut t = team();
    t.status = TeamStatus::Running;
    let qa = t.tasks.iter().find(|task| task.assignee == "dev").unwrap().id;
    t.members.push(Member { id: "qalead".into(), role: "QA lead".into(), agent: "codex".into(), sub: true,
        session: Some(SessionRef { name: "codex/x/qalead".into(), run_id: "r9".into(), ..Default::default() }),
        state: MemberState::Active, ..Default::default() });
    t.members.iter_mut().find(|member| member.id == "dev").unwrap().parent = "qalead".into();
    let task = t.task(qa).unwrap().clone();
    let attempt = Attempt { n: 1, id: "A1".into(), member: "dev".into(), ..Default::default() };
    let text = hand_off_text(&t, &task, &attempt, "", "");
    assert!(text.starts_with(&format!("[Zerus team \u{b7} from: QA lead \u{b7} task: #{qa} \u{b7} attempt: 1]")), "{text}");
    assert!(text.contains("Questions for QA lead:"), "{text}");
    assert!(text.contains("A report is an explicit `hgs "), "{text}");
    assert!(text.contains("No report"), "{text}");
    assert!(text.contains("A blocker goes to QA lead too"), "{text}");
    // U30: every hand-out says how a build shows as work.
    assert!(text.contains("(hgs team run, or hgs run: hgs guide run) so your session shows as Building"), "{text}");
    // The lead's own reports keep the old words.
    t.members.iter_mut().find(|member| member.id == "dev").unwrap().parent.clear();
    assert!(hand_off_text(&t, &task, &attempt, "", "").contains("Questions for the lead:"));
    t.members.iter_mut().find(|member| member.id == "dev").unwrap().parent = "qalead".into();
    // The Tester's report waits in the free sub-lead's inbox, not the lead's.
    let task = t.task_mut(qa).unwrap();
    task.status = TaskStatus::Submitted;
    task.attempts.push(Attempt { n: 1, id: "A1".into(), member: "dev".into(), state: AttemptState::Reported,
        report: Some(Report { text: "Done".into(), at: 1.0, ..Default::default() }), ..Default::default() });
    let seen = [session("claude/x/lead", "r0", true), session("codex/x/qalead", "r9", true)];
    let found = steps(&t, &seen, 10.0);
    assert!(found.contains(&Step::SubInbox { member: "qalead".into() }), "{found:?}");
    assert!(!found.contains(&Step::Inbox), "{found:?}");
    // Once told, it is quiet until a new item.
    let told: Vec<String> = super::inbox::items_for(&t, "qalead").into_iter().map(|item| item.key).collect();
    t.members.iter_mut().find(|member| member.id == "qalead").unwrap().inbox = Inbox { told_at: 10.0, told, ..Default::default() };
    assert!(!steps(&t, &seen, 20.0).iter().any(|step| matches!(step, Step::SubInbox { .. })));
    // N8: a sub-lead with its own Background work still takes a message.
    t.members.iter_mut().find(|member| member.id == "qalead").unwrap().inbox = Inbox::default();
    let mut background = session("codex/x/qalead", "r9", false);
    background.live = true;
    background.activity = "background".into();
    background.phase = "idle".into();
    assert!(steps(&t, &[session("claude/x/lead", "r0", true), background], 20.0)
        .contains(&Step::SubInbox { member: "qalead".into() }));
    // N9: the sub-lead is reminded first; the lead only after a second
    // interval.
    let after = t.settings.remind_lead_minutes as f64 * 60.0;
    let report_at = t.task(qa).unwrap().attempts.last().unwrap().report.as_ref().unwrap().at;
    let found = steps(&t, &seen, report_at + after + 1.0);
    assert!(found.iter().any(|step| matches!(step, Step::RemindReviewer { member, .. } if member == "qalead")), "{found:?}");
    assert!(!found.iter().any(|step| matches!(step, Step::RemindReport { .. })), "{found:?}");
    let found = steps(&t, &seen, report_at + after * 2.0 + 1.0);
    assert!(found.iter().any(|step| matches!(step, Step::RemindReport { .. })), "{found:?}");
    // N4: a lead in a 40-minute turn does not hold the reviewer's reminder
    // back; only the lead's own report reminder waits for its turn to end.
    let mut busy_lead = session("claude/x/lead", "r0", false);
    busy_lead.live = true;
    busy_lead.activity = "busy".into();
    busy_lead.phase = "turn".into();
    let found = steps(&t, &[busy_lead, session("codex/x/qalead", "r9", true)], report_at + after + 1.0);
    assert!(
        found.iter().any(|step| matches!(step, Step::RemindReviewer { member, .. } if member == "qalead")),
        "{found:?}"
    );
    assert!(!found.iter().any(|step| matches!(step, Step::RemindReport { .. })), "{found:?}");
    // N10: a new sub-lead gets its brief once.
    t.members.iter_mut().find(|member| member.id == "qalead").unwrap().sub_brief = true;
    assert!(steps(&t, &seen, 20.0).contains(&Step::SubBrief { member: "qalead".into() }));
    t.members.iter_mut().find(|member| member.id == "qalead").unwrap().sub_brief = false;
    assert!(!steps(&t, &seen, 21.0).iter().any(|step| matches!(step, Step::SubBrief { .. })));
}

#[test]
fn malformed_outbox_files_never_hold_a_valid_report_back() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut t = team();
    t.status = TeamStatus::Running;
    let task = t.tasks.iter_mut().find(|t| t.assignee == "dev").unwrap();
    let attempt = Attempt { n: 1, id: "A1".into(), member: "dev".into(), state: AttemptState::Delivered, ..Default::default() };
    task.attempts.push(attempt);
    task.status = TaskStatus::InProgress;
    let id = task.id;
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
    let folder = dir.path().join("work");
    // 25 malformed requests, older than the valid one: more than one
    // tick's budget of PER_TICK.
    let outbox = super::super::outbox::dir(&folder);
    std::fs::create_dir_all(&outbox).unwrap();
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(600);
    for n in 0..25 {
        let path = outbox.join(format!("{:08x}-0000-4000-8000-000000000000.json", n));
        std::fs::write(&path, b"{ not json").unwrap();
        std::fs::File::options().write(true).open(&path).unwrap().set_modified(old).unwrap();
    }
    let request = json!({"op":"report","request_id":"33333333-3333-4333-8333-333333333333",
        "team":t.id,"member":"dev","attempt":"A1",
        "input":{"request_id":"33333333-3333-4333-8333-333333333333","team":t.id,"task":id,"attempt":"A1","text":"Done after the junk"}});
    super::super::outbox::push(&folder, &request).unwrap();
    let mut member = session("codex/x/dev", "r1", true);
    member.cwd = folder.to_string_lossy().into_owned();
    for _ in 0..2 {
        let current = store.load(&t.id).unwrap().unwrap();
        take_outbox(&mut store, &current, std::slice::from_ref(&member)).unwrap();
    }
    let saved = store.load(&t.id).unwrap().unwrap();
    let attempt = saved.task(id).unwrap().attempts.last().unwrap();
    assert_eq!(attempt.report.as_ref().map(|r| r.text.as_str()), Some("Done after the junk"));
    // Each malformed file was answered and moved out of the queue.
    let refused = super::super::outbox::result(&folder, "00000000-0000-4000-8000-000000000000").unwrap();
    assert_eq!(refused["status"], "refused");
    assert!(refused["error"].as_str().unwrap().contains("not valid JSON"), "{refused}");
    assert!(!outbox.join("00000000-0000-4000-8000-000000000000.json").exists());
    assert_eq!(std::fs::read_dir(super::super::outbox::rejected_dir(&folder)).unwrap().count(), 25);
}

#[test]
fn a_queued_request_naming_another_member_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut t = team();
    t.status = TeamStatus::Running;
    let task = t.tasks.iter_mut().find(|t| t.assignee == "dev").unwrap();
    task.attempts.push(Attempt { n: 1, id: "A1".into(), member: "dev".into(), state: AttemptState::Delivered, ..Default::default() });
    task.status = TaskStatus::InProgress;
    let id = task.id;
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
    let folder = dir.path().join("work");
    // A file in dev's folder that claims to be qa's report: the folder is the
    // proof of who wrote it, so this is refused and nothing is recorded.
    let request = json!({"op":"report","request_id":"44444444-4444-4444-8444-444444444444",
        "team":t.id,"member":"qa","attempt":"A1",
        "input":{"request_id":"44444444-4444-4444-8444-444444444444","team":t.id,"task":id,"attempt":"A1","text":"Not mine"}});
    super::super::outbox::push(&folder, &request).unwrap();
    let mut member = session("codex/x/dev", "r1", true);
    member.cwd = folder.to_string_lossy().into_owned();
    let current = store.load(&t.id).unwrap().unwrap();
    take_outbox(&mut store, &current, &[member]).unwrap();
    let saved = store.load(&t.id).unwrap().unwrap();
    assert!(saved.task(id).unwrap().attempts.last().unwrap().report.is_none());
    let result = super::super::outbox::result(&folder, "44444444-4444-4444-8444-444444444444").unwrap();
    assert_eq!(result["status"], "refused");
    assert!(result["error"].as_str().unwrap_or_default().contains("folder belongs to dev (Backend)"), "{result}");
}

#[test]
fn a_queued_template_run_is_claimed_off_the_tick_and_runs_in_the_runner() {
    // C1: the tick only claims the run and starts a runner; the runner (here
    // called directly) executes it and records exit and output tail.
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut t = team();
    t.status = TeamStatus::Running;
    let folder = dir.path().join("work");
    std::fs::create_dir_all(&folder).unwrap();
    t.folder = folder.display().to_string();
    let task = t.tasks.iter_mut().find(|t| t.assignee == "dev").unwrap();
    task.attempts.push(Attempt { n: 1, id: "A1".into(), member: "dev".into(), state: AttemptState::Delivered, ..Default::default() });
    task.status = TaskStatus::InProgress;
    let id = task.id;
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
    // U21, attempt 2: additions come from Zerus state (the owner's), never
    // from the member's folder; the test points the state at its own file.
    let settings = dir.path().join("templates.json");
    crate::state::teams::templates::settings_override(Some(settings));
    let (program, flag) = if cfg!(windows) { ("cmd", "/c") } else { ("sh", "-c") };
    crate::state::teams::templates::set_additions(&[json!({"id":"say","program":program,"args":[flag,"echo run-ok"]})]).unwrap();
    let run_id = "77777777-7777-4777-8777-777777777777";
    let request = json!({"op":"run","request_id":run_id,
        "team":t.id,"member":"dev","attempt":"A1","template":"say","args":{},
        "input":{"request_id":run_id,"team":t.id,"attempt":"A1","template":"say","args":{}}});
    let path = super::super::outbox::push(&folder, &request).unwrap();
    let mut member = session("codex/x/dev", "r1", true);
    member.cwd = folder.to_string_lossy().into_owned();
    let started = std::cell::RefCell::new(Vec::<(std::path::PathBuf, Value)>::new());
    let spawn = |folder: &Path, request: &Value| {
        started.borrow_mut().push((folder.to_path_buf(), request.clone()));
        Ok(())
    };
    let current = store.load(&t.id).unwrap().unwrap();
    take_outbox_with(&mut store, &current, &[member.clone()], &spawn).unwrap();
    // The tick claimed the request and returned; nothing ran yet.
    let claims = started.borrow().clone();
    assert_eq!(claims.len(), 1);
    assert_eq!(claims[0].1["request_id"], run_id);
    assert_eq!(claims[0].0, folder);
    assert!(super::super::outbox::running_path(&folder, run_id).exists(), "the claim is not recorded");
    assert!(!path.exists(), "the request left the queue");
    assert!(super::super::outbox::result(&folder, run_id).is_none(), "the tick ran the command");
    assert!(store.load(&t.id).unwrap().unwrap().task(id).unwrap().attempts.last().unwrap().runs.is_empty());
    // A second tick starts nothing again: the claim is not a request.
    let current = store.load(&t.id).unwrap().unwrap();
    take_outbox_with(&mut store, &current, &[member], &spawn).unwrap();
    assert_eq!(started.borrow().len(), 1);
    // The runner executes the claim and settles it.
    run_request(&mut store, &folder, &claims[0].1);
    assert!(super::super::outbox::running(&folder, run_id).is_none(), "the claim stays after the result");
    let saved = store.load(&t.id).unwrap().unwrap();
    let attempt = saved.task(id).unwrap().attempts.last().unwrap();
    assert_eq!(attempt.runs.len(), 1);
    assert_eq!((attempt.runs[0].template.as_str(), attempt.runs[0].exit), ("say", Some(0)));
    let result = super::super::outbox::result(&folder, run_id).unwrap();
    assert_eq!(result["status"], "ran");
    assert_eq!(result["tail"].as_str().unwrap_or_default().trim(), "run-ok", "{result}");
    crate::state::teams::templates::settings_override(None);
}

#[test]
fn a_run_whose_attempt_ended_keeps_its_exit_code_and_tail() {
    // C1: the attempt changing while the command runs must not lose the
    // result; the run is recorded on the attempt it belonged to.
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut t = team();
    t.status = TeamStatus::Running;
    let folder = dir.path().join("work");
    std::fs::create_dir_all(&folder).unwrap();
    t.folder = folder.display().to_string();
    let task = t.tasks.iter_mut().find(|t| t.assignee == "dev").unwrap();
    task.attempts.push(Attempt { n: 1, id: "A1".into(), member: "dev".into(), state: AttemptState::Delivered, ..Default::default() });
    task.status = TaskStatus::InProgress;
    let id = task.id;
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
    let settings = dir.path().join("templates.json");
    crate::state::teams::templates::settings_override(Some(settings));
    let (program, flag) = if cfg!(windows) { ("cmd", "/c") } else { ("sh", "-c") };
    crate::state::teams::templates::set_additions(&[json!({"id":"say","program":program,"args":[flag,"echo late-ok"]})]).unwrap();
    let run_id = "77777777-7777-4777-8777-777777777778";
    let request = json!({"op":"run","request_id":run_id,
        "team":t.id,"member":"dev","attempt":"A1","template":"say","args":{},
        "input":{"request_id":run_id,"team":t.id,"attempt":"A1","template":"say","args":{}}});
    super::super::outbox::push(&folder, &request).unwrap();
    let mut member = session("codex/x/dev", "r1", true);
    member.cwd = folder.to_string_lossy().into_owned();
    let started = std::cell::RefCell::new(Vec::<Value>::new());
    let spawn = |_: &Path, request: &Value| {
        started.borrow_mut().push(request.clone());
        Ok(())
    };
    let current = store.load(&t.id).unwrap().unwrap();
    take_outbox_with(&mut store, &current, &[member], &spawn).unwrap();
    // The attempt is decided while the runner works.
    let before = started.borrow().clone();
    let mut ended = store.load(&t.id).unwrap().unwrap();
    {
        let task = ended.task_mut(id).unwrap();
        task.attempts.last_mut().unwrap().state = AttemptState::Rejected;
        task.status = TaskStatus::Pending;
    }
    store.mutate(None, &t.id, None, |team, _| {
        *team = ended;
        Ok(Value::Null)
    })
    .unwrap();
    run_request(&mut store, &folder, &before[0]);
    let saved = store.load(&t.id).unwrap().unwrap();
    let attempt = saved.task(id).unwrap().attempts.last().unwrap();
    assert_eq!((attempt.state, attempt.runs.len()), (AttemptState::Rejected, 1), "{attempt:?}");
    assert_eq!(attempt.runs[0].exit, Some(0));
    let result = super::super::outbox::result(&folder, run_id).unwrap();
    assert_eq!(result["status"], "ran", "{result}");
    assert_eq!(result["tail"].as_str().unwrap_or_default().trim(), "late-ok");
    crate::state::teams::templates::settings_override(None);
}

#[test]
fn a_claim_whose_runner_died_is_answered_on_a_later_tick() {
    // C1: the worker settles a claim no live runner holds, once its bound
    // has long passed; the member stops waiting for a result.
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut t = team();
    t.status = TeamStatus::Running;
    let folder = dir.path().join("work");
    std::fs::create_dir_all(&folder).unwrap();
    t.folder = folder.display().to_string();
    let task = t.tasks.iter_mut().find(|t| t.assignee == "dev").unwrap();
    task.attempts.push(Attempt { n: 1, id: "A1".into(), member: "dev".into(), state: AttemptState::Delivered, ..Default::default() });
    task.status = TaskStatus::InProgress;
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
    let run_id = "77777777-7777-4777-8777-777777777779";
    let marker = json!({"request_id":run_id,"started":now()-7200.0,"timeout_secs":300});
    super::super::outbox::write_running(&folder, run_id, &marker).unwrap();
    let mut member = session("codex/x/dev", "r1", true);
    member.cwd = folder.to_string_lossy().into_owned();
    let current = store.load(&t.id).unwrap().unwrap();
    take_outbox_with(&mut store, &current, &[member], &|_, _| panic!("a stale claim must not start a runner")).unwrap();
    let result = super::super::outbox::result(&folder, run_id).unwrap();
    assert_eq!(result["status"], "refused");
    assert!(result["error"].as_str().unwrap_or_default().contains("stopped without a result"), "{result}");
    assert!(super::super::outbox::running(&folder, run_id).is_none());
}

/// N10: the runner takes only the worker's own claim, from a process with
/// no agent marker. Before this gate an unsandboxed member could cd into a
/// colleague's folder, pipe a request in and have it run and recorded there.
#[test]
fn the_runner_takes_only_the_workers_claim() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path();
    let id = "11111111-1111-4111-8111-111111111111";
    // No claim at all: a session that piped the request in.
    let error = ensure_the_workers_run(folder, id, false, false).unwrap_err().to_string();
    assert!(error.contains("no claim"), "{error}");
    // A claim naming another request is not this one's.
    super::super::outbox::write_running(folder, "22222222-2222-4222-8222-222222222222", &json!({"request_id":"22222222-2222-4222-8222-222222222222"})).unwrap();
    let error = ensure_the_workers_run(folder, id, false, false).unwrap_err().to_string();
    assert!(error.contains("no claim"), "{error}");
    // The worker's claim for this request passes...
    super::super::outbox::write_running(folder, id, &json!({"request_id":id,"started":1.0})).unwrap();
    ensure_the_workers_run(folder, id, false, false).unwrap();
    // ...but never from an agent session, whatever the claim says.
    let error = ensure_the_workers_run(folder, id, true, false).unwrap_err().to_string();
    assert!(error.contains("not from an agent session"), "{error}");
    let error = ensure_the_workers_run(folder, id, false, true).unwrap_err().to_string();
    assert!(error.contains("not from an agent session"), "{error}");
}

/// Task #229: a Crit in the team holds the release: a queued release-candidate
/// run is refused, naming the crit, until it is decided.
#[test]
fn the_release_candidate_is_refused_while_a_crit_is_open() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut t = team();
    t.status = TeamStatus::Running;
    let mut crit = t.tasks.iter().find(|t| t.assignee == "dev").unwrap().clone();
    crit.id = 9;
    crit.title = "Fix the crash".into();
    crit.severity = "crit".into();
    crit.origin = Some(2);
    crit.status = TaskStatus::Pending;
    t.tasks.push(crit);
    let task = t.tasks.iter_mut().find(|t| t.assignee == "dev").unwrap();
    task.attempts.push(Attempt { n: 1, id: "A1".into(), member: "dev".into(), state: AttemptState::Delivered, ..Default::default() });
    task.status = TaskStatus::InProgress;
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
    let folder = dir.path().join("work");
    let request = json!({"op":"run","request_id":"99999999-9999-4999-8999-999999999999",
        "team":t.id,"member":"dev","attempt":"A1","template":"release-candidate","args":{"commit":"abc"},
        "input":{"request_id":"99999999-9999-4999-8999-999999999999","team":t.id,"attempt":"A1","template":"release-candidate","args":{"commit":"abc"}}});
    super::super::outbox::push(&folder, &request).unwrap();
    let mut member = session("codex/x/dev", "r1", true);
    member.cwd = folder.to_string_lossy().into_owned();
    let current = store.load(&t.id).unwrap().unwrap();
    take_outbox(&mut store, &current, &[member]).unwrap();
    let result = super::super::outbox::result(&folder, "99999999-9999-4999-8999-999999999999").unwrap();
    assert_eq!(result["status"], "refused", "{result}");
    let error = result["error"].as_str().unwrap_or_default();
    assert!(error.contains("critical task") && error.contains("#9 in phase 1") && error.contains("release candidate"), "{error}");
}

#[test]
fn a_queued_run_of_an_unknown_template_is_refused() {    // U21: the same argument validation guards the worker's queue.
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut t = team();
    t.status = TeamStatus::Running;
    let task = t.tasks.iter_mut().find(|t| t.assignee == "dev").unwrap();
    task.attempts.push(Attempt { n: 1, id: "A1".into(), member: "dev".into(), state: AttemptState::Delivered, ..Default::default() });
    task.status = TaskStatus::InProgress;
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
    let folder = dir.path().join("work");
    let request = json!({"op":"run","request_id":"88888888-8888-4888-8888-888888888888",
        "team":t.id,"member":"dev","attempt":"A1","template":"rm-rf","args":{},
        "input":{"request_id":"88888888-8888-4888-8888-888888888888","team":t.id,"attempt":"A1","template":"rm-rf","args":{}}});
    super::super::outbox::push(&folder, &request).unwrap();
    let mut member = session("codex/x/dev", "r1", true);
    member.cwd = folder.to_string_lossy().into_owned();
    let current = store.load(&t.id).unwrap().unwrap();
    take_outbox(&mut store, &current, &[member]).unwrap();
    let result = super::super::outbox::result(&folder, "88888888-8888-4888-8888-888888888888").unwrap();
    assert_eq!(result["status"], "refused");
    assert!(result["error"].as_str().unwrap_or_default().contains("Unknown template"), "{result}");
}

#[test]
fn a_templates_file_in_the_members_folder_is_never_a_template_source() {
    // U21, attempt 2: a sandboxed member can write its own folder; a queued
    // run may name only the built-ins and the owner's additions, so a
    // templates.json in the folder is ignored and its program never runs.
    let dir = tempfile::tempdir().unwrap();
    let settings = dir.path().join("templates.json");
    crate::state::teams::templates::settings_override(Some(settings));
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut t = team();
    t.status = TeamStatus::Running;
    let task = t.tasks.iter_mut().find(|t| t.assignee == "dev").unwrap();
    task.attempts.push(Attempt { n: 1, id: "A1".into(), member: "dev".into(), state: AttemptState::Delivered, ..Default::default() });
    task.status = TaskStatus::InProgress;
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
    let folder = dir.path().join("work");
    std::fs::create_dir_all(folder.join(".zerus-team")).unwrap();
    let marker = folder.join("pwned.txt");
    std::fs::write(
        folder.join(".zerus-team").join("templates.json"),
        serde_json::to_vec(&json!({"templates":[{"id":"pwn","program":"cmd",
            "args":["/c",format!("echo x > \"{}\"", marker.display())]}]})).unwrap(),
    )
    .unwrap();
    let request = json!({"op":"run","request_id":"99999999-9999-4999-8999-999999999999",
        "team":t.id,"member":"dev","attempt":"A1","template":"pwn","args":{},
        "input":{"request_id":"99999999-9999-4999-8999-999999999999","team":t.id,"attempt":"A1","template":"pwn","args":{}}});
    super::super::outbox::push(&folder, &request).unwrap();
    let mut member = session("codex/x/dev", "r1", true);
    member.cwd = folder.to_string_lossy().into_owned();
    let current = store.load(&t.id).unwrap().unwrap();
    take_outbox(&mut store, &current, &[member]).unwrap();
    let result = super::super::outbox::result(&folder, "99999999-9999-4999-8999-999999999999").unwrap();
    assert_eq!(result["status"], "refused");
    assert!(result["error"].as_str().unwrap_or_default().contains("Unknown template"), "{result}");
    assert!(!marker.exists(), "the folder's program ran");
    crate::state::teams::templates::settings_override(None);
}

/// Task #28 (1, 2, 3): the worker's side of what a sandboxed member cannot do
/// itself. A host read (`hgs ls --json`, the read team 33fbafeb's DeepSeek
/// member asked the lead to run unsandboxed) is answered from outside the
/// sandbox; anything but a read is refused, never run; and the member's
/// `hgs run` is recorded as its session's work and started at Low integrity.
#[test]
fn the_worker_answers_a_sandboxed_members_read_and_starts_its_run_at_low_integrity() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open(true).unwrap();
        let mut t = team();
        t.status = TeamStatus::Running;
        // A DeepSeek member: its run keeps Low integrity (a Codex member's
        // keeps Codex's sandbox, task #48).
        t.members.iter_mut().find(|member| member.id == "dev").unwrap().agent = "dsh".into();
        let folder = dir.path().join("work");
        std::fs::create_dir_all(&folder).unwrap();
        t.folder = folder.display().to_string();
        // Task 173: dev's run belongs to its handed task.
        delivered(&mut t);
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
        let read = "88888888-8888-4888-8888-888888888888";
        let send = "99999999-9999-4999-8999-999999999999";
        let run = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
        for (id, op, input) in [
            (read, "read", json!({"argv":["ls", "--json"]})),
            (send, "read", json!({"argv":["send", "claude/x/lead", "--json"]})),
            (run, "session-run", json!({"label":"cargo build","command":"cargo build","cwd":folder.display().to_string()})),
        ] {
            super::super::outbox::push(&folder, &json!({"op":op,"request_id":id,"team":t.id,"member":"dev","input":input})).unwrap();
        }
        let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::<Vec<String>>::new()));
        let seen = calls.clone();
        set_fake_hgs(Some(Box::new(move |_, args: &[&str], _: Option<&Value>| {
            seen.borrow_mut().push(args.iter().map(|arg| (*arg).to_owned()).collect());
            Ok(fake_output(0, r#"[{"name":"claude/x/lead"}]"#, ""))
        })));
        crate::state::builds::HELD_RUNNER.with(|held| held.set(true));
        let mut member = session("codex/x/dev", "r1", true);
        member.cwd = folder.to_string_lossy().into_owned();
        let current = store.load(&t.id).unwrap().unwrap();
        let spawn = |_: &Path, _: &Value| -> anyhow::Result<()> { anyhow::bail!("no template run was asked for") };
        let taken = take_outbox_with(&mut store, &current, &[member], &spawn);
        crate::state::builds::HELD_RUNNER.with(|held| held.set(false));
        set_fake_hgs(None);
        taken.unwrap();
        let answered = super::super::outbox::result(&folder, read).unwrap();
        assert_eq!((answered["status"].as_str(), answered["exit"].as_i64()), (Some("ran"), Some(0)), "{answered}");
        assert!(answered["stdout"].as_str().unwrap().contains("claude/x/lead"), "{answered}");
        assert_eq!(calls.borrow().first(), Some(&vec!["ls".to_owned(), "--json".to_owned()]), "the read ran outside the sandbox");
        // Task #275: the refused queued read reaches the member as a message.
        assert!(
            calls.borrow().iter().any(|call| call == &vec!["send".to_owned(), "codex/x/dev".to_owned(), "--json".to_owned()]),
            "the refused queued read reached the member: {:?}",
            calls.borrow()
        );
        let refused = super::super::outbox::result(&folder, send).unwrap();
        assert_eq!(refused["status"], "refused", "{refused}");
        assert!(refused["error"].as_str().unwrap().contains("only for hgs ls, inspect and task reads"), "{refused}");
        assert_eq!(super::super::outbox::result(&folder, run).unwrap()["status"], "running");
        let claim = crate::state::builds::record(run, now()).unwrap();
        assert_eq!((claim["integrity"].as_str(), claim["session"]["name"].as_str()), (Some("low"), Some("codex/x/dev")), "{claim}");
        assert!(super::super::outbox::request(&folder, run).is_none(), "the request left the queue");
    });
}

/// Task #275: a sandboxed member's refused queued call is a team message at its
/// next safe point, with the reason, the limit and how to send a long text, and
/// the lead's inbox reads it too; a failed outcome is told the same way.
#[test]
fn a_refused_queued_question_reaches_the_member_and_the_lead() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut t = team();
    t.status = TeamStatus::Running;
    let task = t.tasks.iter_mut().find(|task| task.assignee == "dev").unwrap();
    task.attempts.push(Attempt { n: 1, id: "A1".into(), member: "dev".into(), state: AttemptState::Delivered, ..Default::default() });
    task.status = TaskStatus::InProgress;
    let _id = task.id;
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
    let folder = dir.path().join("work");
    let request_id = "44444444-4444-4444-8444-444444444444";
    let request = json!({"op":"ask","request_id":request_id,"team":t.id,"member":"dev","attempt":"A1",
        "input":{"request_id":request_id,"team":t.id,"text":"x".repeat(5000),"attempt":"A1"}});
    super::super::outbox::push(&folder, &request).unwrap();
    let mut member = session("codex/x/dev", "r1", true);
    member.cwd = folder.to_string_lossy().into_owned();
    let told: std::rc::Rc<std::cell::RefCell<Vec<String>>> = Default::default();
    let heard = told.clone();
    set_fake_hgs(Some(Box::new(move |_, args, input| {
        if args == ["--help"] {
            return Ok(fake_output(0, "", ""));
        }
        if let Some(input) = input {
            heard.borrow_mut().push(crate::state::teams::doorbell::posted_text(input));
        }
        Ok(fake_output(0, r#"{"status":"submitted"}"#, ""))
    })));
    let current = store.load(&t.id).unwrap().unwrap();
    take_outbox(&mut store, &current, &[member]).unwrap();
    // A failed outcome is told the same way; the report is urgent.
    let failed_id = "55555555-5555-4555-8555-555555555555";
    let failed = json!({"status":"failed","request_id":failed_id,"error":"the runner died"});
    let team_now = store.load(&t.id).unwrap().unwrap();
    let member = team_now.member("dev").unwrap().clone();
    announce_outbox(&mut store, &team_now, &member, &folder,
        &json!({"op":"report","request_id":failed_id}), &failed);
    set_fake_hgs(None);
    // The member heard the refusal with the limit and the file hint, then the failed report.
    let messages = told.borrow();
    assert!(messages.iter().any(|text| text.contains("Your queued ask") && text.contains("NOT applied")), "{messages:?}");
    let ask = messages.iter().find(|text| text.contains("Your queued ask")).unwrap();
    assert!(ask.contains(request_id) && ask.contains("4000"), "{ask}");
    assert!(ask.to_lowercase().contains("file"), "{ask}");
    let report = messages.iter().find(|text| text.contains("Your queued report")).unwrap();
    assert!(report.contains("failed") && report.contains(failed_id), "{report}");
    // The result file keeps the refusal, and the lead's inbox names both.
    let result = super::super::outbox::result(&folder, request_id).unwrap();
    assert_eq!(result["status"], "refused");
    assert!(result["error"].as_str().unwrap().contains("A question is too long"), "{result}");
    let saved = store.load(&t.id).unwrap().unwrap();
    let record = saved.outbox_failures.iter().find(|failure| failure["request_id"] == request_id).unwrap();
    assert_eq!(record["urgent"], json!(true));
    assert!(saved.outbox_failures.iter().any(|failure| failure["request_id"] == failed_id));
    let items = super::inbox::items(&saved);
    assert!(items.iter().any(|item| item.kind == "refused_queued_call"), "{:?}", items.iter().map(|item| item.kind).collect::<Vec<_>>());
}