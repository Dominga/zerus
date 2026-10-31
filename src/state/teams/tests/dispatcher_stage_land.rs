use super::*;

#[test]
fn a_command_stage_runs_in_the_stage_folder_and_its_exit_code_decides() {
    // U47: a real Git folder and a check that exits 1, then 0.
    let root = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(root.path(), || {
        crate::state::builds::INLINE_RUNNER.with(|inline| inline.set(true));
        let (lead, dev) = stage_repo(root.path());
        let command = "git grep -q pass -- gate.txt || (echo gate is red && exit 1)".to_owned();
        let team = stage_team(&lead, &dev, "gate", &|_| command_preset("command", command.clone(), &[]));
        let mut store = Store::open_at(&super::super::super::store::path(), true).unwrap();
        store.create(&uuid::Uuid::new_v4().to_string(), &json!({}), team, vec![]).unwrap();
        deliver_all();
        hand_out(&mut store, STAGE_TEAM, 1, "dev", None, "").unwrap();
        stage_report(&mut store, 1);
        let run = run_stage(&mut store);
        let team = store.load(STAGE_TEAM).unwrap().unwrap();
        let task = team.task(1).unwrap();
        let ended = task.pipeline_run.as_ref().unwrap().runs[0].clone();
        assert_eq!((task.status, ended.state), (TaskStatus::Pending, StageState::Returned), "{ended:?}");
        assert_eq!(ended.commit, git_in(&dev, &["rev-parse", "HEAD"]), "the stage ran on the folder's commit");
        assert_eq!(ended.check.as_ref().map(|check| check.exit), Some(1));
        assert!(ended.check.as_ref().unwrap().tail.contains("gate is red"), "{ended:?}");
        // The run log keeps it, in the stage's folder, as an `hgs run`.
        let record = crate::state::builds::record(&run, now()).unwrap();
        assert_eq!((record["status"].as_str(), record["source"].as_str()), (Some("ran"), Some("team_stage")));
        assert!(same_path(record["cwd"].as_str().unwrap(), &dev.display().to_string()), "{record}");
        assert_eq!(record["session"]["name"], "codex/x/dev");
        // The implementer fixes it in a new attempt: the stage passes.
        hand_out(&mut store, STAGE_TEAM, 1, "dev", None, "").unwrap();
        let team = store.load(STAGE_TEAM).unwrap().unwrap();
        let task = team.task(1).unwrap().clone();
        let text = hand_off_text(&team, &task, task.attempts.last().unwrap(), "", "");
        assert!(text.contains("returned by the command of Zerus") && text.contains("gate is red"), "{text}");
        std::fs::write(dev.join("gate.txt"), "pass\n").unwrap();
        git_in(&dev, &["commit", "-q", "-am", "Pass the gate"]);
        stage_report(&mut store, 1);
        run_stage(&mut store);
        set_fake_hgs(None);
        crate::state::builds::INLINE_RUNNER.with(|inline| inline.set(false));
        let team = store.load(STAGE_TEAM).unwrap().unwrap();
        let task = team.task(1).unwrap();
        assert_eq!(task.status, TaskStatus::Submitted, "the lead decides after the stage passed");
        assert_eq!(task.pipeline_run.as_ref().unwrap().runs[1].state, StageState::Passed);
        let kinds = feed_kinds(&store);
        for kind in ["stage_started", "stage_command_started", "stage_returned", "stage_passed", "stages_passed"] {
            assert!(kinds.iter().any(|found| found == kind), "{kind} missing in {kinds:?}");
        }
    });
}

/// #680 C1: a command stage with the checkout rule checks the attempt's
/// commit in a detached checkout, gets the task, branch and commit as
/// environment variables, and holds the implementer's folder meanwhile.
#[test]
fn a_command_stage_with_the_checkout_rule_checks_the_checked_commit_and_holds_the_folder() {
    let root = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(root.path(), || {
        crate::state::builds::INLINE_RUNNER.with(|inline| inline.set(true));
        let (lead, dev) = stage_repo(root.path());
        let mut team = stage_team(&lead, &dev, "gate", &|_| checkout_command_preset("command", env_command()));
        team.settings.branch_per_task = Some(BranchPerTask { template: "task/{id}".into() });
        let mut store = Store::open_at(&super::super::super::store::path(), true).unwrap();
        store.create(&uuid::Uuid::new_v4().to_string(), &json!({}), team, vec![]).unwrap();
        deliver_all();
        hand_out(&mut store, STAGE_TEAM, 1, "dev", None, "").unwrap();
        std::fs::write(dev.join("a.txt"), "one\n").unwrap();
        let team = store.load(STAGE_TEAM).unwrap().unwrap();
        let attempt = team.task(1).unwrap().open_attempt().unwrap().id.clone();
        let committed = super::super::super::commands::commit_in_team(
            &team,
            "dev",
            &json!({"request_id":uuid::Uuid::new_v4().to_string(),"team":STAGE_TEAM,"attempt":attempt,"message":"Task 1"}),
        )
        .unwrap();
        let sha = committed["sha"].as_str().unwrap().to_owned();
        stage_report(&mut store, 1);
        let team = store.load(STAGE_TEAM).unwrap().unwrap();
        assert_eq!(super::super::super::stages::held(&team, "dev"), Some(1), "a command stage holds the implementer from the report on");
        let seen = [session("claude/x/lead", "r0", true), session("codex/x/dev", "r1", true)];
        let found = steps(&team, &seen, now());
        assert!(
            !found.iter().any(|step| matches!(step, Step::HandOut { task: 2, .. } | Step::Rebalance { task: 2, .. })),
            "no next task in the held folder: {found:?}"
        );
        // The old bug: the next task moves the live folder. The stage still
        // checks the attempt's commit, in its own detached checkout.
        git_in(&dev, &["checkout", "-q", "-b", "task/2", "main"]);
        std::fs::write(dev.join("b.txt"), "two\n").unwrap();
        git_in(&dev, &["add", "-A"]);
        git_in(&dev, &["commit", "-q", "-m", "Task 2 work"]);
        let run = run_stage(&mut store);
        let team = store.load(STAGE_TEAM).unwrap().unwrap();
        let ended = team.task(1).unwrap().pipeline_run.as_ref().unwrap().runs[0].clone();
        assert!(!ended.folder.is_empty() && !same_path(&ended.folder, &dev.display().to_string()), "the stage ran in a checkout: {ended:?}");
        let checkout = PathBuf::from(&ended.folder);
        assert_eq!(git_in(&checkout, &["rev-parse", "HEAD"]), sha, "the checked commit, not task/2's");
        assert!(!checkout.join("b.txt").exists(), "the next task's work is not in the checkout");
        let env = std::fs::read_to_string(checkout.join("env.txt")).unwrap();
        assert_eq!(env.trim(), format!("1|task/1|{sha}"), "task id, its branch and the checked commit");
        assert_eq!(ended.check.as_ref().map(|check| check.exit), Some(0));
        let record = crate::state::builds::record(&run, now()).unwrap();
        assert_eq!(record["source"], "team_stage");
        assert!(same_path(record["cwd"].as_str().unwrap(), &checkout.display().to_string()), "{record}");
        // The hold ends with the stage: the next task goes ahead.
        assert_eq!(super::super::super::stages::held(&store.load(STAGE_TEAM).unwrap().unwrap(), "dev"), None);
        hand_out(&mut store, STAGE_TEAM, 2, "dev", None, "").unwrap();
        set_fake_hgs(None);
        crate::state::builds::INLINE_RUNNER.with(|inline| inline.set(false));
    });
}

/// #683: with the hold rule, the review before the command and land stage
/// holds the implementer's folder from the report on, so #2 waits.
#[test]
fn a_review_before_a_command_or_land_stage_holds_the_folder_under_the_hold_rule() {
    let root = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(root.path(), || {
        let (lead, dev) = stage_repo(root.path());
        let team = stage_team(&lead, &dev, "deliver", &|_| deliver_preset());
        let mut store = Store::open_at(&super::super::super::store::path(), true).unwrap();
        store.create(&uuid::Uuid::new_v4().to_string(), &json!({}), team, vec![]).unwrap();
        deliver_all();
        hand_out(&mut store, STAGE_TEAM, 1, "dev", None, "").unwrap();
        stage_report(&mut store, 1);
        let team = store.load(STAGE_TEAM).unwrap().unwrap();
        assert_eq!(super::super::super::stages::held(&team, "dev"), Some(1), "the review before a command and land stage holds the folder");
        let seen = [session("claude/x/lead", "r0", true), session("codex/x/dev", "r1", true)];
        let found = steps(&team, &seen, now());
        assert!(!found.iter().any(|step| matches!(step, Step::HandOut { task: 2, .. })), "no second task while the review is open: {found:?}");
        set_fake_hgs(None);
    });
}

/// #683: under the checkout rule the later command and land read their own
/// detached checkout, so the review does not hold the implementer.
#[test]
fn a_review_before_a_command_or_land_stage_does_not_hold_under_the_checkout_rule() {
    let root = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(root.path(), || {
        let (lead, dev) = stage_repo(root.path());
        let mut catalog = deliver_preset();
        catalog.pipelines.last_mut().unwrap().folder = "checkout".into();
        let team = stage_team(&lead, &dev, "deliver", &|_| catalog.clone());
        let mut store = Store::open_at(&super::super::super::store::path(), true).unwrap();
        store.create(&uuid::Uuid::new_v4().to_string(), &json!({}), team, vec![]).unwrap();
        deliver_all();
        hand_out(&mut store, STAGE_TEAM, 1, "dev", None, "").unwrap();
        stage_report(&mut store, 1);
        let team = store.load(STAGE_TEAM).unwrap().unwrap();
        assert_eq!(super::super::super::stages::held(&team, "dev"), None, "the checkout rule holds nothing for a review");
        let seen = [session("claude/x/lead", "r0", true), session("codex/x/dev", "r1", true)];
        let found = steps(&team, &seen, now());
        assert!(found.contains(&Step::HandOut { task: 2, member: "dev".into() }), "{found:?}");
        set_fake_hgs(None);
    });
}

/// #680: a land stage refuses a folder moved off the checked commit, and
/// runs nothing.
#[test]
fn a_land_stage_refuses_a_folder_moved_from_the_checked_commit() {
    let root = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(root.path(), || {
        crate::state::builds::INLINE_RUNNER.with(|inline| inline.set(true));
        let (lead, dev) = stage_repo(root.path());
        let mut team = stage_team(&lead, &dev, "gate", &|_| command_preset("land", "git status --porcelain".into(), &[("0", "accepted")]));
        team.settings.land_allowed = true;
        team.settings.branch_per_task = Some(BranchPerTask { template: "task/{id}".into() });
        let mut store = Store::open_at(&super::super::super::store::path(), true).unwrap();
        store.create(&uuid::Uuid::new_v4().to_string(), &json!({}), team, vec![]).unwrap();
        deliver_all();
        hand_out(&mut store, STAGE_TEAM, 1, "dev", None, "").unwrap();
        std::fs::write(dev.join("a.txt"), "one\n").unwrap();
        let team = store.load(STAGE_TEAM).unwrap().unwrap();
        let attempt = team.task(1).unwrap().open_attempt().unwrap().id.clone();
        super::super::super::commands::commit_in_team(
            &team,
            "dev",
            &json!({"request_id":uuid::Uuid::new_v4().to_string(),"team":STAGE_TEAM,"attempt":attempt,"message":"Task 1"}),
        )
        .unwrap();
        stage_report(&mut store, 1);
        // The next task moves the live folder before the land stage runs.
        git_in(&dev, &["checkout", "-q", "-b", "task/2", "main"]);
        std::fs::write(dev.join("b.txt"), "two\n").unwrap();
        git_in(&dev, &["add", "-A"]);
        git_in(&dev, &["commit", "-q", "-m", "Task 2 work"]);
        let run = run_stage(&mut store);
        let team = store.load(STAGE_TEAM).unwrap().unwrap();
        let task = team.task(1).unwrap();
        let ended = task.pipeline_run.as_ref().unwrap().runs[0].clone();
        assert_eq!(ended.outcome, "refused", "{ended:?}");
        assert!(ended.commit.is_empty(), "the land command never started");
        assert!(task.hold.as_ref().unwrap().reason.contains("checked commit"), "{:?}", task.hold);
        assert!(feed_kinds(&store).iter().any(|kind| kind == "stage_not_run"));
        assert!(crate::state::builds::record(&run, now()).is_none(), "the land command did not run");
        set_fake_hgs(None);
        crate::state::builds::INLINE_RUNNER.with(|inline| inline.set(false));
    });
}

/// #680 N11: a member's run waits while the stage's own `team_stage` run is
/// live in the same folder.
#[test]
fn a_members_run_waits_for_the_stages_own_team_stage_run() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut t = team();
    t.status = TeamStatus::Running;
    let folder = dir.path().join("work");
    std::fs::create_dir_all(&folder).unwrap();
    t.folder = folder.display().to_string();
    let task = t.tasks.iter_mut().find(|task| task.assignee == "dev").unwrap();
    task.attempts.push(Attempt { n: 1, id: "A1".into(), member: "dev".into(), state: AttemptState::Delivered, ..Default::default() });
    task.status = TaskStatus::InProgress;
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
    let settings = dir.path().join("templates.json");
    crate::state::teams::templates::settings_override(Some(settings));
    crate::state::teams::templates::set_additions(&[json!({"id":"say","program":"cmd","args":["/c","echo run-ok"]})]).unwrap();
    let claim = crate::state::builds::test_running_claim("88888888-8888-4888-8888-888888888888", &folder, "team_stage");
    let id = "77777777-7777-4777-8777-777777777771";
    let request = json!({"op":"run","request_id":id,"team":t.id,"member":"dev","attempt":"A1","template":"say","args":{},
        "input":{"request_id":id,"team":t.id,"attempt":"A1","template":"say","args":{}}});
    super::super::super::outbox::push(&folder, &request).unwrap();
    let mut dev = session("codex/x/dev", "r1", true);
    dev.cwd = folder.display().to_string();
    let started = std::cell::RefCell::new(Vec::<String>::new());
    let spawn = |_: &Path, request: &Value| {
        started.borrow_mut().push(request["request_id"].as_str().unwrap_or_default().to_owned());
        Ok(())
    };
    let current = store.load(&t.id).unwrap().unwrap();
    take_outbox_with(&mut store, &current, &[dev.clone()], &spawn).unwrap();
    assert!(started.borrow().is_empty(), "the stage's run holds the folder");
    assert!(super::super::super::outbox::request_path(&folder, id).exists(), "the member's run waits");
    // The stage's run ends; the member's run starts on the next tick.
    std::fs::remove_file(&claim).unwrap();
    let current = store.load(&t.id).unwrap().unwrap();
    take_outbox_with(&mut store, &current, &[dev], &spawn).unwrap();
    assert_eq!(started.borrow().as_slice(), [id.to_owned()]);
    crate::state::teams::templates::settings_override(None);
    });
}

/// #538 N4: a stage's runs are serialized by the stage's folder, not only by
/// the requester's: the claim sits in the tester's folder while the run goes
/// in the implementer's.
#[test]
fn a_stage_run_is_serialized_by_the_stage_folder() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let lead = dir.path().join("lead");
        let dev = dir.path().join("dev");
        let qa = dir.path().join("qa");
        for folder in [&lead, &dev, &qa] {
            std::fs::create_dir_all(folder).unwrap();
        }
        let mut t = staged(&lead.display().to_string());
        // Distinct folders: dev's worktree is where a test builds; qa's own
        // folder only holds its outbox.
        for member in &mut t.members {
            member.directory = match member.id.as_str() {
                "dev" => dev.display().to_string(),
                "qa" => qa.display().to_string(),
                _ => lead.display().to_string(),
            };
        }
        // The review passes and the test stage is delivered to qa.
        super::super::super::pipeline::give_stage(&mut t, 1, "rev").unwrap();
        let review = t.task(1).unwrap().pipeline_run.as_ref().unwrap().open_run().unwrap().id.clone();
        let mut events = Vec::new();
        ops::apply(&mut t, "report", &json!({"task":1,"attempt":review,"verdict":"pass","text":"Fine"}), &Actor::Member("rev".into()), &BTreeSet::new(), &mut events).unwrap();
        let prepared = super::super::super::stages::prepare(&t, 1).unwrap();
        let stage_id = super::super::super::pipeline::give_stage(&mut t, 1, "qa").unwrap();
        super::super::super::stages::record(&mut t, 1, &stage_id, &prepared);
        assert_eq!(t.task(1).unwrap().status, TaskStatus::InTest);
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
        // A run is going in the stage's folder (dev's worktree).
        let other = "88888888-8888-4888-8888-888888888888";
        let marker = json!({"request_id":other,"started":crate::state::now(),"timeout_secs":3600.0,"label":"cargo-test","command":"cargo test"});
        super::super::super::outbox::write_running(&dev, other, &marker).unwrap();
        // qa's own folder is free, but the stage's is not: its claim waits.
        let id = "77777777-7777-4777-8777-777777777777";
        let request = json!({"op":"run","request_id":id,"team":t.id,"member":"qa","attempt":stage_id,"template":"cargo-test","args":{},
            "input":{"request_id":id,"team":t.id,"member":"qa","attempt":stage_id,"template":"cargo-test","args":{}}});
        let started = std::cell::RefCell::new(Vec::<String>::new());
        let spawn = |_: &Path, request: &Value| {
            started.borrow_mut().push(request["request_id"].as_str().unwrap_or_default().to_owned());
            Ok(())
        };
        let member = t.member("qa").unwrap().clone();
        start_run(&mut store, &t, &member, &qa, &request, &spawn).unwrap();
        assert!(started.borrow().is_empty(), "the stage folder's run holds the claim");
        // It ends: the same claim starts.
        std::fs::remove_file(super::super::super::outbox::running_path(&dev, other)).unwrap();
        start_run(&mut store, &t, &member, &qa, &request, &spawn).unwrap();
        assert_eq!(started.borrow().as_slice(), [id.to_owned()]);
    });
}

#[test]
fn a_stage_run_is_the_stage_members_attempt() {
    let mut t = staged("C:\\proj");
    super::super::super::pipeline::give_stage(&mut t, 1, "rev").unwrap();
    let review = t.task(1).unwrap().pipeline_run.as_ref().unwrap().open_run().unwrap().id.clone();
    assert!(attempt_of(&t, &review, "rev"), "the stage member's open stage run");
    assert!(!attempt_of(&t, &review, "qa"), "another member's stage run");
    assert!(attempt_of(&t, "a1", "dev"), "the implementer's own attempt");
    assert!(!attempt_of(&t, "nope", "dev"));
}

/// #635 N5: a real report replacing a captured one records the folder's
/// current HEAD again, so the land stage lands B, not the capture's A.
#[test]
fn a_replaced_captured_report_refreshes_the_folder_head_for_the_land_stage() {
    let root = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(root.path(), || {
        crate::state::builds::INLINE_RUNNER.with(|inline| inline.set(true));
        let (lead, dev) = stage_repo(root.path());
        let mut team = stage_team(&lead, &dev, "gate", &|_| command_preset("land", "git rev-parse HEAD > landed.txt".into(), &[("0", "accepted")]));
        team.settings.land_allowed = true;
        let mut store = Store::open_at(&super::super::super::store::path(), true).unwrap();
        store.create(&uuid::Uuid::new_v4().to_string(), &json!({}), team, vec![]).unwrap();
        deliver_all();
        hand_out(&mut store, STAGE_TEAM, 1, "dev", None, "").unwrap();
        let team = store.load(STAGE_TEAM).unwrap().unwrap();
        let attempt = team.task(1).unwrap().open_attempt().unwrap().id.clone();
        // A captured reply at HEAD A.
        store
            .mutate(None, STAGE_TEAM, None, |team, events| {
                ops::apply(
                    team,
                    "capture-report",
                    &json!({"task":1,"attempt":attempt,"text":"Queued, after #12"}),
                    &Actor::Member("dev".into()),
                    &BTreeSet::new(),
                    events,
                )
            })
            .unwrap();
        // The member then commits with plain git (HEAD B) and sends the real report.
        std::fs::write(dev.join("a.txt"), "one\n").unwrap();
        git_in(&dev, &["add", "-A"]);
        git_in(&dev, &["commit", "-q", "-m", "Task 1"]);
        let head = git_in(&dev, &["rev-parse", "HEAD"]);
        store
            .mutate(None, STAGE_TEAM, None, |team, events| {
                ops::apply(
                    team,
                    "report",
                    &json!({"task":1,"attempt":attempt,"text":"Done"}),
                    &Actor::Member("dev".into()),
                    &BTreeSet::new(),
                    events,
                )
            })
            .unwrap();
        let team = store.load(STAGE_TEAM).unwrap().unwrap();
        assert_eq!(team.task(1).unwrap().attempts[0].folder_head, head, "the replacement records HEAD B");
        run_stage(&mut store);
        let team = store.load(STAGE_TEAM).unwrap().unwrap();
        assert_eq!(team.task(1).unwrap().status, TaskStatus::Accepted, "{:?}", team.task(1).unwrap().pipeline_run);
        assert_eq!(std::fs::read_to_string(dev.join("landed.txt")).unwrap().trim(), head, "the land stage landed B");
        set_fake_hgs(None);
        crate::state::builds::INLINE_RUNNER.with(|inline| inline.set(false));
    });
}

/// #635 L1: a command or land stage of a task does not start while an
/// earlier run of the task is awaiting_run and live.
#[test]
fn a_command_stage_waits_for_an_earlier_run_of_the_task_that_is_awaiting() {
    let root = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(root.path(), || {
        crate::state::builds::INLINE_RUNNER.with(|inline| inline.set(true));
        let (lead, dev) = stage_repo(root.path());
        let team = stage_team(&lead, &dev, "gate", &|_| command_preset("command", "echo ok".into(), &[]));
        let mut store = Store::open_at(&super::super::super::store::path(), true).unwrap();
        store.create(&uuid::Uuid::new_v4().to_string(), &json!({}), team, vec![]).unwrap();
        deliver_all();
        hand_out(&mut store, STAGE_TEAM, 1, "dev", None, "").unwrap();
        stage_report(&mut store, 1);
        let previous = "99999999-9999-4999-8999-999999999999";
        let dev_path = dev.display().to_string();
        store
            .mutate(None, STAGE_TEAM, None, |team, _| {
                let task = team.task_mut(1).unwrap();
                let pipeline = task.pipeline_run.as_mut().unwrap();
                pipeline.runs.insert(
                    0,
                    StageRun {
                        id: previous.into(),
                        stage: 0,
                        kind: "test".into(),
                        round: 0,
                        attempt: 1,
                        state: StageState::Passed,
                        awaiting_run: true,
                        folder: dev_path.clone(),
                        ..Default::default()
                    },
                );
                Ok(json!({}))
            })
            .unwrap();
        let claim = crate::state::builds::test_running_claim(previous, Path::new(&dev_path), "team_stage");
        let team = store.load(STAGE_TEAM).unwrap().unwrap();
        let run_id = team.task(1).unwrap().pipeline_run.as_ref().unwrap().open_run().unwrap().id.clone();
        start_stage_command(&mut store, &team, 1, &run_id).unwrap();
        assert!(crate::state::builds::record(&run_id, now()).is_none(), "the stage did not start beside the live run");
        assert_ne!(team.task(1).unwrap().status, TaskStatus::Accepted, "{:?}", team.task(1).unwrap().pipeline_run);
        // The earlier run ends: the stage starts on the next tick.
        std::fs::remove_file(&claim).unwrap();
        run_stage(&mut store);
        let team = store.load(STAGE_TEAM).unwrap().unwrap();
        assert_eq!(team.task(1).unwrap().status, TaskStatus::Submitted, "{:?}", team.task(1).unwrap().pipeline_run);
        set_fake_hgs(None);
        crate::state::builds::INLINE_RUNNER.with(|inline| inline.set(false));
    });
}

/// #684 N1: a plain git commit (not `hgs team commit`) is still the commit
/// the attempt reported, so a folder moved afterwards is refused.
#[test]
fn a_land_stage_refuses_a_folder_moved_after_a_plain_git_commit() {
    let root = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(root.path(), || {
        crate::state::builds::INLINE_RUNNER.with(|inline| inline.set(true));
        let (lead, dev) = stage_repo(root.path());
        let mut team = stage_team(&lead, &dev, "gate", &|_| command_preset("land", "git status --porcelain".into(), &[("0", "accepted")]));
        team.settings.land_allowed = true;
        let mut store = Store::open_at(&super::super::super::store::path(), true).unwrap();
        store.create(&uuid::Uuid::new_v4().to_string(), &json!({}), team, vec![]).unwrap();
        deliver_all();
        hand_out(&mut store, STAGE_TEAM, 1, "dev", None, "").unwrap();
        std::fs::write(dev.join("a.txt"), "one\n").unwrap();
        git_in(&dev, &["add", "-A"]);
        git_in(&dev, &["commit", "-q", "-m", "Task 1"]);
        stage_report(&mut store, 1);
        // The next task moves the live folder before the land stage runs.
        git_in(&dev, &["checkout", "-q", "-b", "task/2", "main"]);
        std::fs::write(dev.join("b.txt"), "two\n").unwrap();
        git_in(&dev, &["add", "-A"]);
        git_in(&dev, &["commit", "-q", "-m", "Task 2 work"]);
        let run = run_stage(&mut store);
        let team = store.load(STAGE_TEAM).unwrap().unwrap();
        let task = team.task(1).unwrap();
        let ended = task.pipeline_run.as_ref().unwrap().runs[0].clone();
        assert_eq!(ended.outcome, "refused", "{ended:?}");
        assert!(task.hold.as_ref().unwrap().reason.contains("checked commit"), "{:?}", task.hold);
        assert!(crate::state::builds::record(&run, now()).is_none(), "the land command did not run");
        set_fake_hgs(None);
        crate::state::builds::INLINE_RUNNER.with(|inline| inline.set(false));
    });
}

/// #684 N2: a land stage in a folder without Git runs; there is no HEAD to
/// check, and the hold rule keeps the folder.
#[test]
fn a_land_stage_in_a_folder_without_git_runs() {
    let root = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(root.path(), || {
        crate::state::builds::INLINE_RUNNER.with(|inline| inline.set(true));
        let plain = root.path().join("plain");
        std::fs::create_dir_all(&plain).unwrap();
        let mut team = team();
        team.id = STAGE_TEAM.into();
        team.folder = plain.display().to_string();
        for member in &mut team.members {
            member.folder = MemberFolder::Shared;
        }
        team.settings.land_allowed = true;
        let catalog = command_preset("land", "echo landed".into(), &[("0", "accepted")]);
        team.task_mut(1).unwrap().pipeline = "gate".into();
        super::super::super::pipeline::snapshot_with(&mut team, 1, &catalog);
        let mut store = Store::open_at(&super::super::super::store::path(), true).unwrap();
        store.create(&uuid::Uuid::new_v4().to_string(), &json!({}), team, vec![]).unwrap();
        deliver_all();
        hand_out(&mut store, STAGE_TEAM, 1, "dev", None, "").unwrap();
        stage_report(&mut store, 1);
        run_stage(&mut store);
        let team = store.load(STAGE_TEAM).unwrap().unwrap();
        let task = team.task(1).unwrap();
        assert_eq!((task.status, task.decided_by), (TaskStatus::Accepted, Some(Decider::Zerus)), "{:?}", task.pipeline_run);
        set_fake_hgs(None);
        crate::state::builds::INLINE_RUNNER.with(|inline| inline.set(false));
    });
}

/// #684 L1: a checkout prepare() made is removed when the start cannot be
/// recorded (a store error), so the next tick is not refused by it.
#[test]
fn a_checkout_prepared_for_a_start_that_is_not_recorded_is_removed() {
    let root = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(root.path(), || {
        let (lead, dev) = stage_repo(root.path());
        let team = stage_team(&lead, &dev, "gate", &|_| checkout_command_preset("command", "echo ok".into()));
        let mut store = Store::open_at(&super::super::super::store::path(), true).unwrap();
        store.create(&uuid::Uuid::new_v4().to_string(), &json!({}), team, vec![]).unwrap();
        deliver_all();
        hand_out(&mut store, STAGE_TEAM, 1, "dev", None, "").unwrap();
        std::fs::write(dev.join("a.txt"), "one\n").unwrap();
        let team = store.load(STAGE_TEAM).unwrap().unwrap();
        let attempt = team.task(1).unwrap().open_attempt().unwrap().id.clone();
        super::super::super::commands::commit_in_team(
            &team,
            "dev",
            &json!({"request_id":uuid::Uuid::new_v4().to_string(),"team":STAGE_TEAM,"attempt":attempt,"message":"Task 1"}),
        )
        .unwrap();
        stage_report(&mut store, 1);
        let team = store.load(STAGE_TEAM).unwrap().unwrap();
        let run_id = team.task(1).unwrap().pipeline_run.as_ref().unwrap().open_run().unwrap().id.clone();
        // A read-only store makes command_started fail after prepare made
        // the checkout; the prepared checkout must go.
        let mut read_only = Store::open_at(&super::super::super::store::path(), false).unwrap();
        let error = start_stage_command(&mut read_only, &team, 1, &run_id).unwrap_err().to_string();
        let leftovers = std::fs::read_dir(root.path().join("teams/stages"))
            .map(|entries| entries.flatten().filter(|entry| entry.path().is_dir()).count())
            .unwrap_or(0);
        assert_eq!(leftovers, 0, "the prepared checkout is cleaned: {error}");
        set_fake_hgs(None);
    });
}

/// #581 N1: a stage hand-out runs the context step before the stage, and
/// waits for its confirmation as a task hand-out does.
#[test]
fn a_stage_hand_out_runs_the_context_action_before_the_stage() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let folder = dir.path().join("proj");
        std::fs::create_dir_all(&folder).unwrap();
        let mut t = staged(&folder.display().to_string());
        t.members.iter_mut().find(|member| member.id == "rev").unwrap().context_before_task = "compact".into();
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        store.create("11111111-1111-4111-8111-111111111701", &json!({}), t.clone(), vec![]).unwrap();
        let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let request = std::rc::Rc::new(std::cell::RefCell::new(String::new()));
        let (seen, seen_request) = (calls.clone(), request.clone());
        let observed = request.clone();
        set_fake_hgs(Some(Box::new(move |_, args: &[&str], input: Option<&Value>| {
            let call = args.first().copied().unwrap_or_default().to_owned();
            seen.borrow_mut().push(call.clone());
            match call.as_str() {
                "compact-context" => {
                    let id = input.unwrap()["request_id"].as_str().unwrap().to_owned();
                    *seen_request.borrow_mut() = id.clone();
                    Ok(fake_output(0, &json!({"request_id":id,"status":"submitted"}).to_string(), ""))
                }
                "send" => Ok(fake_output(0, r#"{"status":"submitted"}"#, "")),
                other => panic!("unexpected hgs {other}"),
            }
        })));
        // The first tick starts the compaction and hands nothing out.
        stage_hand_out(&mut store, &t.id, 1, "rev", "").unwrap();
        assert_eq!(*calls.borrow(), ["compact-context"], "the compaction starts before the stage");
        let saved = store.load(&t.id).unwrap().unwrap();
        let run = saved.task(1).unwrap().pipeline_run.as_ref().unwrap().open_run().unwrap();
        assert!(run.context.is_some() && run.delivered_at.is_none(), "{run:?}");
        assert_eq!(run.member, "rev", "the stage is reserved for its member");
        // It still compacts: the stage waits.
        inspect_override(Some(json!({"run_id":"r2","conversation_id":"","activity":"idle",
            "compact_context_request":{"request_id":observed.borrow().clone(),"run_id":"r2","status":"compacting"}})));
        stage_hand_out(&mut store, &t.id, 1, "rev", "").unwrap();
        assert_eq!(*calls.borrow(), ["compact-context"], "still waiting");
        // It completed: the stage is handed out, with the context step.
        inspect_override(Some(json!({"run_id":"r2","conversation_id":"","activity":"idle",
            "compact_context_request":{"request_id":observed.borrow().clone(),"run_id":"r2","status":"completed"}})));
        stage_hand_out(&mut store, &t.id, 1, "rev", "").unwrap();
        assert_eq!(*calls.borrow(), ["compact-context", "send"], "{:?}", calls.borrow());
        let saved = store.load(&t.id).unwrap().unwrap();
        let run = saved.task(1).unwrap().pipeline_run.as_ref().unwrap().open_run().unwrap();
        assert_eq!(run.state, StageState::Delivered);
        assert!(run.context.is_none());
        assert_eq!(
            run.prepare.iter().map(|step| (step.step.as_str(), step.result.as_str())).collect::<Vec<_>>(),
            [("context", "done")]
        );
        inspect_override(None);
        set_fake_hgs(None);
    });
}

/// #581 N1: a stage hand-out switches the member to a model the stage's role
/// lists, and waits for the session record before the stage.
#[test]
fn a_stage_hand_out_switches_to_a_model_the_stage_role_lists() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let folder = dir.path().join("proj");
        std::fs::create_dir_all(&folder).unwrap();
        // The catalog's reviewer role lists one model.
        let mut catalog = super::super::super::catalog::load().unwrap_or_else(|_| super::super::super::catalog::builtin());
        let version = catalog.version;
        let reviewer = catalog.roles.iter_mut().find(|role| role.id == "reviewer").unwrap();
        reviewer.models = vec![crate::state::teams::model::ModelChoice { model: "gpt-5.5".into(), effort: "high".into() }];
        super::super::super::catalog::run("set", &json!({"catalog":catalog,"version":version}), true).unwrap();
        let mut t = staged(&folder.display().to_string());
        {
            let rev = t.members.iter_mut().find(|member| member.id == "rev").unwrap();
            rev.agent = "claude".into();
            rev.model = "gpt-5".into();
            rev.effort = "medium".into();
            rev.models = vec![
                crate::state::teams::model::ModelChoice { model: "gpt-5".into(), effort: "medium".into() },
                crate::state::teams::model::ModelChoice { model: "gpt-5.5".into(), effort: "high".into() },
            ];
        }
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        store.create("11111111-1111-4111-8111-111111111702", &json!({}), t.clone(), vec![]).unwrap();
        let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let record = calls.clone();
        set_fake_hgs(Some(Box::new(move |_, args: &[&str], input: Option<&Value>| {
            let call = args.first().copied().unwrap_or_default().to_owned();
            record.borrow_mut().push(call.clone());
            match call.as_str() {
                "settings" => {
                    assert_eq!(input.unwrap()["model"], "gpt-5.5");
                    Ok(fake_output(0, r#"{"status":"applied"}"#, ""))
                }
                "send" => Ok(fake_output(0, r#"{"status":"submitted"}"#, "")),
                other => panic!("unexpected hgs {other}"),
            }
        })));
        inspect_override(Some(json!({"run_id":"r2","conversation_id":"","activity":"idle","model":"gpt-5","effort":"medium"})));
        // The first tick switches the member and hands nothing out.
        stage_hand_out(&mut store, &t.id, 1, "rev", "").unwrap();
        assert_eq!(*calls.borrow(), ["settings"], "the switch runs before the stage");
        let saved = store.load(&t.id).unwrap().unwrap();
        let run = saved.task(1).unwrap().pipeline_run.as_ref().unwrap().open_run().unwrap();
        assert_eq!(run.member, "rev", "the stage is reserved for its member");
        assert_eq!(
            run.prepare.iter().map(|step| (step.step.as_str(), step.result.as_str(), step.to.as_str())).collect::<Vec<_>>(),
            [("model", "pending", "gpt-5.5@high")]
        );
        // The session still shows the old model: the stage waits.
        stage_hand_out(&mut store, &t.id, 1, "rev", "").unwrap();
        assert_eq!(*calls.borrow(), ["settings"], "still waiting for the record");
        // It shows the new model: the stage is handed out with the step done.
        inspect_override(Some(json!({"run_id":"r2","conversation_id":"","activity":"idle","model":"gpt-5.5","effort":"high"})));
        stage_hand_out(&mut store, &t.id, 1, "rev", "").unwrap();
        assert_eq!(*calls.borrow(), ["settings", "send"]);
        let saved = store.load(&t.id).unwrap().unwrap();
        let run = saved.task(1).unwrap().pipeline_run.as_ref().unwrap().open_run().unwrap();
        assert_eq!(run.state, StageState::Delivered);
        assert_eq!(
            run.prepare.iter().map(|step| (step.step.as_str(), step.result.as_str())).collect::<Vec<_>>(),
            [("model", "done")]
        );
        inspect_override(None);
        set_fake_hgs(None);
    });
}

#[test]
fn a_branch_per_task_lands_two_tasks_as_squash_without_the_first_ones_commits() {
    // U48: a real repository; each task gets its own branch from main, and
    // the land stage squashes it onto main.
    let root = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(root.path(), || {
        crate::state::builds::INLINE_RUNNER.with(|inline| inline.set(true));
        let (lead, dev) = stage_repo(root.path());
        let land = |n: u32| {
            let lead = lead.display().to_string();
            command_preset("land", format!("git -C {lead} merge -q --squash task/{n} && git -C {lead} commit -q -m land-{n}"), &[("0", "accepted")])
        };
        let mut team = stage_team(&lead, &dev, "gate", &land);
        team.settings.land_allowed = true;
        team.settings.branch_per_task = Some(BranchPerTask { template: "task/{id}".into() });
        let mut store = Store::open_at(&super::super::super::store::path(), true).unwrap();
        store.create(&uuid::Uuid::new_v4().to_string(), &json!({}), team, vec![]).unwrap();
        deliver_all();
        let mut first_commit = String::new();
        for n in [1u32, 2] {
            hand_out(&mut store, STAGE_TEAM, n, "dev", None, "").unwrap();
            let team = store.load(STAGE_TEAM).unwrap().unwrap();
            let attempt = team.task(n).unwrap().attempts.last().unwrap().clone();
            assert_eq!(attempt.state, AttemptState::Delivered);
            let step = &attempt.prepare[0];
            assert_eq!((step.step.as_str(), step.to.as_str(), step.result.as_str()), ("branch", format!("task/{n}").as_str(), "done"));
            assert_eq!(step.reason, "created from main");
            assert_eq!(git_in(&dev, &["rev-parse", "--abbrev-ref", "HEAD"]), format!("task/{n}"));
            if n == 2 {
                // The second task's branch starts from main after the first
                // landed: it never carries the first task's own commit.
                assert_eq!(step.from, "task/1");
                assert!(git_fails(&dev, &["merge-base", "--is-ancestor", &first_commit, "task/2"]));
                assert_eq!(git_in(&dev, &["log", "--oneline", "main..task/2"]), "");
                assert!(dev.join("a.txt").exists(), "the first task's squash is on main");
            }
            // `hgs team commit` takes the task's branch while the attempt is open.
            std::fs::write(dev.join(format!("{}.txt", if n == 1 { "a" } else { "b" })), format!("task {n}\n")).unwrap();
            let committed = super::super::super::commands::commit_in_team(
                &team,
                "dev",
                &json!({"request_id":uuid::Uuid::new_v4().to_string(),"team":STAGE_TEAM,"attempt":attempt.id,"message":format!("Task {n}")}),
            )
            .unwrap();
            assert_eq!(committed["status"], "committed", "{committed}");
            if n == 1 {
                first_commit = committed["sha"].as_str().unwrap().to_owned();
            }
            stage_report(&mut store, n);
            run_stage(&mut store);
            let team = store.load(STAGE_TEAM).unwrap().unwrap();
            let task = team.task(n).unwrap();
            assert_eq!((task.status, task.decided_by), (TaskStatus::Accepted, Some(Decider::Zerus)), "{:?}", task.pipeline_run);
        }
        // Both landed on main as one squash each, linear.
        assert_eq!(git_in(&lead, &["log", "--format=%s", "main"]), "land-2\nland-1\ninit");
        assert!(lead.join("a.txt").exists() && lead.join("b.txt").exists());
        // A reopened task gets its existing branch back.
        let team = store.load(STAGE_TEAM).unwrap().unwrap();
        let step = prepare_branch(&team, 1, "dev").unwrap();
        assert_eq!((step.result.as_str(), step.reason.as_str(), step.from.as_str()), ("done", "checked out", "task/2"));
        assert!(git_in(&lead, &["branch", "--list", "task/*"]).contains("task/2"), "branches are never deleted");
        set_fake_hgs(None);
        crate::state::builds::INLINE_RUNNER.with(|inline| inline.set(false));
    });
}

#[test]
fn a_land_stage_holds_for_the_lead_without_land_allowed_or_on_a_held_exit() {
    let root = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(root.path(), || {
        crate::state::builds::INLINE_RUNNER.with(|inline| inline.set(true));
        let (lead, dev) = stage_repo(root.path());
        let team = stage_team(&lead, &dev, "gate", &|_| command_preset("land", "exit 3".into(), &[("0", "accepted"), ("2", "returned"), ("3", "held")]));
        let mut store = Store::open_at(&super::super::super::store::path(), true).unwrap();
        store.create(&uuid::Uuid::new_v4().to_string(), &json!({}), team, vec![]).unwrap();
        deliver_all();
        hand_out(&mut store, STAGE_TEAM, 1, "dev", None, "").unwrap();
        stage_report(&mut store, 1);
        // Without the owner's land_allowed nothing runs: held for the lead.
        let run = run_stage(&mut store);
        assert!(crate::state::builds::record(&run, now()).is_none(), "the land command never ran");
        let team = store.load(STAGE_TEAM).unwrap().unwrap();
        let task = team.task(1).unwrap();
        assert_eq!(task.status, TaskStatus::Submitted);
        assert!(task.hold.as_ref().unwrap().reason.contains("land_allowed"), "{:?}", task.hold);
        // With it, exit 3 (busy) holds the task for the lead with the tail.
        store
            .mutate(None, STAGE_TEAM, None, |team, events| {
                team.settings.land_allowed = true;
                ops::apply(team, "reject", &json!({"task":1,"reason":"Land again"}), &Actor::Member("lead".into()), &BTreeSet::new(), events)
            })
            .unwrap();
        let team = store.load(STAGE_TEAM).unwrap().unwrap();
        assert!(!plan::ready(&team).contains(&1), "a held landing waits for the lead's release");
        store
            .mutate(None, STAGE_TEAM, None, |team, events| ops::apply(team, "release", &json!({"task":1}), &Actor::Member("lead".into()), &BTreeSet::new(), events))
            .unwrap();
        hand_out(&mut store, STAGE_TEAM, 1, "dev", None, "").unwrap();
        stage_report(&mut store, 1);
        let run = run_stage(&mut store);
        set_fake_hgs(None);
        crate::state::builds::INLINE_RUNNER.with(|inline| inline.set(false));
        assert_eq!(crate::state::builds::record(&run, now()).unwrap()["exit"], 3);
        let team = store.load(STAGE_TEAM).unwrap().unwrap();
        let task = team.task(1).unwrap();
        assert_eq!(task.status, TaskStatus::Submitted);
        assert!(task.hold.as_ref().unwrap().reason.contains("exited 3"), "{:?}", task.hold);
        assert_eq!(task.pipeline_run.as_ref().unwrap().runs.last().unwrap().outcome, "held");
        assert!(feed_kinds(&store).iter().any(|kind| kind == "land_held"));
    });
}

#[test]
fn a_dirty_folder_waits_for_the_lead_and_the_default_keeps_the_members_branch() {
    let root = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(root.path(), || {
        let (lead, dev) = stage_repo(root.path());
        let mut team = stage_team(&lead, &dev, "none", &|_| super::super::super::catalog::builtin());
        team.settings.branch_per_task = Some(BranchPerTask { template: "task/{id}-{member}".into() });
        let mut store = Store::open_at(&super::super::super::store::path(), true).unwrap();
        store.create(&uuid::Uuid::new_v4().to_string(), &json!({}), team, vec![]).unwrap();
        // Uncommitted changes: nothing is sent, the task waits with its reason.
        std::fs::write(dev.join("gate.txt"), "half done\n").unwrap();
        set_fake_hgs(Some(Box::new(|_, args: &[&str], _| panic!("nothing is sent to a folder with changes: {args:?}"))));
        hand_out(&mut store, STAGE_TEAM, 1, "dev", None, "").unwrap();
        let team = store.load(STAGE_TEAM).unwrap().unwrap();
        let task = team.task(1).unwrap();
        assert!(task.attempts.is_empty());
        let waits = task.waits.as_ref().unwrap();
        assert_eq!((waits.waits_for.as_str(), waits.on.as_slice(), waits.reason.as_str(), waits.state.as_str()), ("zerus", &["branch".to_owned()][..], "folder has changes", "refused"));
        assert_eq!(git_in(&dev, &["rev-parse", "--abbrev-ref", "HEAD"]), format!("team/{}/dev", &STAGE_TEAM[..8]));
        assert!(!plan::ready(&team).contains(&1), "never retried on its own");
        let item = inbox::items(&team).into_iter().find(|item| item.kind == "task_waits").expect("one inbox item for the lead");
        assert!(item.summary.contains("folder has changes") && item.command.contains("hgs team retry"), "{item:?}");
        assert!(feed_kinds(&store).iter().any(|kind| kind == "task_waits"));
        // The lead retries once the folder is clean: the branch is made.
        git_in(&dev, &["checkout", "--", "gate.txt"]);
        store
            .mutate(None, STAGE_TEAM, None, |team, events| ops::apply(team, "retry", &json!({"task":1}), &Actor::Member("lead".into()), &BTreeSet::new(), events))
            .unwrap();
        assert!(store.load(STAGE_TEAM).unwrap().unwrap().task(1).unwrap().waits.is_none());
        deliver_all();
        hand_out(&mut store, STAGE_TEAM, 1, "dev", None, "").unwrap();
        assert_eq!(git_in(&dev, &["rev-parse", "--abbrev-ref", "HEAD"]), "task/1-dev");
        // Default: no branch_per_task keeps the member's own team branch.
        let mut team = store.load(STAGE_TEAM).unwrap().unwrap();
        team.settings.branch_per_task = None;
        assert!(prepare_branch(&team, 2, "dev").is_none() && task_branch(&team, 2, "dev").is_none());
        git_in(&dev, &["checkout", "-q", &format!("team/{}/dev", &STAGE_TEAM[..8])]);
        store.mutate(None, STAGE_TEAM, None, |stored, _| {
            stored.settings.branch_per_task = None;
            Ok(Value::Null)
        })
        .unwrap();
        hand_out(&mut store, STAGE_TEAM, 2, "dev", None, "").unwrap();
        set_fake_hgs(None);
        let mut team = store.load(STAGE_TEAM).unwrap().unwrap();
        assert!(team.task(2).unwrap().attempts[0].prepare.is_empty());
        assert_eq!(git_in(&dev, &["rev-parse", "--abbrev-ref", "HEAD"]), format!("team/{}/dev", &STAGE_TEAM[..8]));
        // A template that gives no valid branch is refused when saved.
        let error = ops::apply(&mut team, "settings", &json!({"settings":{"branch_per_task":{"template":"task {id}"}}}), &Actor::Human, &BTreeSet::new(), &mut Vec::new()).unwrap_err();
        assert!(error.to_string().contains("valid branch name"), "{error}");
        let error = ops::apply(&mut team, "settings", &json!({"settings":{"branch_per_task":{"template":"task/{id}"}}}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut Vec::new()).unwrap_err();
        assert!(error.to_string().contains("Only you change the team's branch_per_task"), "{error}");
    });
}
