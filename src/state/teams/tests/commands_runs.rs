use super::*;
use super::support::*;

/// N4/F1: `hgs team run` answers from the member's folder: a result already
/// there never queues the run again, the wait is short, and the same
/// request_id reads the outcome instead of running the command twice.
#[test]
fn a_run_answers_without_queueing_it_twice_and_waits_briefly() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("work");
    std::fs::create_dir_all(&folder).unwrap();
    crate::state::teams::templates::settings_override(Some(dir.path().join("templates.json")));
    crate::state::with_test_state_root(dir.path(), || {
        super::set_test_caller_identities(Some((Some("conv-dev".into()), None, None)));
        let mut store = Store::open(true).unwrap();
        let mut team = running_team();
        team.folder = folder.display().to_string();
        let member = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
        member.session = Some(SessionRef { name: "dsh/x/dev".into(), run_id: "r1".into(), conversation_id: "conv-dev".into(), ..Default::default() });
        let attempt = team.task(1).unwrap().open_attempt().unwrap().id.clone();
        let id = team.id.clone();
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
        let input = |request: &str| json!({"request_id":request,"team":id,"attempt":attempt,"template":"cargo-test","args":{"filter":"teams"}});
        // An answer already in the folder is the answer: no push, no wait.
        let request = "22222222-2222-4222-8222-222222222222";
        std::fs::create_dir_all(super::super::outbox::dir(&folder)).unwrap();
        let answer = json!({"status":"ran","request_id":request,"exit":0,"tail":"old"});
        std::fs::write(super::super::outbox::result_path(&folder, request), serde_json::to_vec(&answer).unwrap()).unwrap();
        let value = super::team_run_in(&folder, &input(request), false, 0.1).unwrap();
        assert_eq!((value["status"].as_str(), value["tail"].as_str()), (Some("ran"), Some("old")));
        assert!(!super::super::outbox::request_path(&folder, request).exists(), "the result queued the run again");
        // A short wait, not the template's 1800 s.
        let request = "33333333-3333-4333-8333-333333333333";
        let started = std::time::Instant::now();
        let value = super::team_run_in(&folder, &input(request), true, 0.2).unwrap();
        assert!(started.elapsed() < std::time::Duration::from_secs(10), "the wait took {:?}", started.elapsed());
        assert_eq!(value["status"], "running", "{value}");
        assert!(super::super::outbox::request(&folder, request).is_some(), "the queued request left");
        // Asking again with the same id reads the queue; another run cannot reuse it.
        let value = super::team_run_in(&folder, &input(request), false, 0.1).unwrap();
        assert_eq!(value["status"], "queued", "{value}");
        let mut other = input(request);
        other["template"] = json!("cargo-clippy");
        let error = super::team_run_in(&folder, &other, false, 0.1).unwrap_err().to_string();
        assert!(error.contains("already used by another run"), "{error}");
        // A result written later comes back without a wait.
        let done = json!({"status":"ran","request_id":request,"exit":0,"tail":"done"});
        super::super::outbox::settle(&folder, &json!({"request_id":request}), &done).unwrap();
        assert_eq!(super::team_run_in(&folder, &input(request), false, 0.1).unwrap()["tail"], "done");
    });
    crate::state::teams::templates::settings_override(None);
}

/// N5: a sandboxed write queues in the member's own folder, never in the
/// process's directory: a member that ran `cd tray` before reporting would
/// otherwise queue where the worker never looks.
#[test]
fn a_sandboxed_write_queues_in_the_members_folder_not_the_process_directory() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    crate::state::teams::templates::settings_override(Some(dir.path().join("templates.json")));
    crate::state::with_test_state_root(dir.path(), || {
        super::set_test_caller_identities(Some((Some("conv-dev".into()), None, None)));
        let mut store = Store::open(true).unwrap();
        let mut team = running_team();
        team.folder = project.display().to_string();
        let member = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
        member.session = Some(SessionRef { name: "dsh/x/dev".into(), run_id: "r1".into(), conversation_id: "conv-dev".into(), ..Default::default() });
        let attempt = team.task(1).unwrap().open_attempt().unwrap().id.clone();
        let id = team.id.clone();
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), team.clone(), vec![]).unwrap();
        let member_folder = std::path::PathBuf::from(
            super::super::dispatcher::member_folder_path(&team, team.member("dev").unwrap()).unwrap(),
        );
        assert_eq!(member_folder, project);
        let report = "22222222-2222-4222-8222-222222222222";
        let input = json!({"request_id":report,"team":id,"attempt":attempt,"text":"Done and checked"});
        let value = super::queue_write("report", &input, false).unwrap();
        assert_eq!(value["status"], "queued", "{value}");
        assert!(super::super::outbox::request_path(&member_folder, report).exists(), "the report queued outside the member's folder");
        let run = "33333333-3333-4333-8333-333333333333";
        let input = json!({"request_id":run,"team":id,"attempt":attempt,"template":"cargo-test","args":{"filter":"teams"}});
        let value = super::team_run(&input, false).unwrap();
        assert_eq!(value["status"], "queued", "{value}");
        assert!(super::super::outbox::request_path(&member_folder, run).exists(), "the run queued outside the member's folder");
        // Not in the directory this test process happens to run in.
        let here = std::env::current_dir().unwrap();
        assert!(!super::super::outbox::request_path(&here, report).exists());
        assert!(!super::super::outbox::request_path(&here, run).exists());
    });
    crate::state::teams::templates::settings_override(None);
}

#[test]
fn a_pipeline_runs_through_hgs_team() {
    use crate::state::teams::model::{Member, MemberState, TaskStatus};
    let (mut team, attempt, _) = review_team();
    team.members.push(Member { id: "rev".into(), role: "Reviewer".into(), agent: "claude".into(), state: MemberState::Active, ..Default::default() });
    team.task_mut(1).unwrap().pipeline_run = super::super::pipeline::resolve(
        &super::super::catalog::builtin(),
        &team,
        &crate::state::teams::model::Task { pipeline: "code".into(), ..team.task(1).unwrap().clone() },
    );
    with_review_team(&team, || {
        for op in ["skip-stage", "send-back"] {
            assert!(MUTATIONS.contains(&op) && USAGE.contains(op), "{op}");
        }
        let value = call("report", json!({"task":1,"attempt":attempt,"text":"Done"})).unwrap();
        assert_eq!(value["status"], "in_review", "{value}");
        let status = run("status", &json!({"team":"rv"}), false).unwrap();
        let task = status["tasks"].as_array().unwrap().iter().find(|task| task["id"] == 1).unwrap().clone();
        assert_eq!(task["status"], "in_review");
        assert_eq!(task["stage_candidates"], json!(["rev"]), "never the implementer dev");
        assert_eq!(task["waits"]["for"], "stage", "{task}");
        assert_eq!(task["stage_spans"][0]["kind"], "review");
        // The polled team list (without `list`, which may start the worker).
        let list = list_items(&[stored()], true);
        let summary = list["items"][0].clone();
        assert_eq!(summary["in_review"], 1, "{summary}");
        // Stage delivery (#532) gives the review to rev.
        let stage = Store::open(true)
            .unwrap()
            .mutate(None, "rv", None, |team, _| Ok(json!(super::super::pipeline::give_stage(team, 1, "rev")?)))
            .unwrap();
        let stage = stage.as_str().unwrap().to_owned();
        let value = call("report", json!({"task":1,"attempt":stage,"verdict":"pass","text":"Reads well"})).unwrap();
        assert_eq!(value["status"], "in_test", "{value}");
        // The owner skips the test, then sends the task back with a reason.
        let value = call("skip-stage", json!({"task":1,"reason":"No suites changed","by":"human"})).unwrap();
        assert_eq!(value["status"], "submitted", "{value}");
        let value = call("send-back", json!({"task":1,"reason":"Rename the endpoint","by":"human"})).unwrap();
        assert_eq!(value["status"], "pending", "{value}");
        let saved = stored();
        assert_eq!(saved.task(1).unwrap().status, TaskStatus::Pending);
        let events = run("events", &json!({"team":"rv"}), false).unwrap();
        let kinds: Vec<String> = events["items"].as_array().unwrap().iter().map(|event| event["kind"].as_str().unwrap_or_default().to_owned()).collect();
        for kind in ["stage_started", "stage_passed", "stage_skipped", "sent_back"] {
            assert!(kinds.iter().any(|found| found == kind), "{kind}: {kinds:?}");
        }
    });
}

#[test]
fn a_member_prompt_snapshot_survives_a_catalog_edit_through_run() {
    let _env = crate::state::ENV_LOCK.lock().unwrap();
    let old = ["HGS_SESSION", "HGS_RUN_ID", "DSH_SESSION_ID"].map(|key| (key, std::env::var(key).ok()));
    for key in ["HGS_SESSION", "HGS_RUN_ID", "DSH_SESSION_ID"] { std::env::remove_var(key); }
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        super::set_test_caller_identities(Some((None, None, None)));
        let mut catalog = crate::state::teams::catalog::builtin();
        let role = catalog.roles.iter_mut().find(|role| role.id == "backend").unwrap();
        role.prompt_order = vec!["protocol".into()];
        role.protocol_extra = "OLD BLOCK".into();
        crate::state::teams::catalog::run("set", &json!({"version":1,"catalog":catalog}), true).unwrap();
        let _ = Store::open(true).unwrap();
        let created = super::run("create", &json!({"request_id":"10000000-0000-4000-8000-000000000001","name":"T","by":"human",
            "lead":{"id":"lead","role":"Lead","agent":"codex"},
            "members":[{"id":"dev","role":"Backend","agent":"codex","prompt":"Backend prompt"}],
            "tasks":[{"id":1,"title":"Work","assignee":"dev"}]}), false).unwrap();
        let store = Store::open(false).unwrap();
        let team_id = created["team"].as_str().unwrap_or("t").to_owned();
        let mut next = crate::state::teams::catalog::load().unwrap();
        next.roles.iter_mut().find(|role| role.id == "backend").unwrap().protocol_extra = "NEW BLOCK".into();
        crate::state::teams::catalog::run("set", &json!({"version":next.version,"catalog":next}), true).unwrap();
        assert_eq!(crate::state::teams::catalog::load().unwrap().roles.iter().find(|role| role.id == "backend").unwrap().protocol_extra, "NEW BLOCK");
        let team = store.load(&team_id).unwrap().unwrap();
        let task = team.task(1).unwrap().clone();
        let attempt = Attempt { n: 1, id: "a1".into(), member: "dev".into(), ..Default::default() };
        let text = crate::state::teams::dispatcher::hand_off_text(&team, &task, &attempt, "", "");
        assert!(text.contains("OLD BLOCK") && !text.contains("NEW BLOCK"), "{text}");
        let mut old_team = team.clone();
        old_team.members.iter_mut().find(|member| member.id == "dev").unwrap().prompt_snapshot = None;
        let text = crate::state::teams::dispatcher::hand_off_text(&old_team, &task, &attempt, "", "");
        assert!(text.contains("NEW BLOCK"), "{text}");
        super::set_test_caller_identities(None);
    });
    for (key, value) in old {
        match value { Some(value) => std::env::set_var(key, value), None => std::env::remove_var(key) }
    }
}

#[test]
fn phase35_findings_work_through_run() {
    let _env = crate::state::ENV_LOCK.lock().unwrap();
    let old = ["HGS_SESSION", "HGS_RUN_ID", "DSH_SESSION_ID"].map(|key| (key, std::env::var(key).ok()));
    for key in ["HGS_SESSION", "HGS_RUN_ID", "DSH_SESSION_ID"] { std::env::remove_var(key); }
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        super::set_test_caller_identities(Some((None, None, None)));
        let mut store = Store::open(true).unwrap();
        let req = |n: u8| format!("00000000-0000-4000-8000-0000000000{n:02}");
        let mut parent = Team { id: "p".into(), name: "Parent".into(), lead: "lead".into(), status: TeamStatus::Running,
            settings: Settings { tree_sessions: 16, ..Default::default() }, ..Default::default() };
        parent.tasks.push(Task { id: 1, title: "Delegated".into(), assignee: "dev".into(), status: TaskStatus::InProgress,
            subteam: Some("c".into()), ..Default::default() });
        parent.tasks[0].attempts.push(Attempt { n: 1, id: "pa".into(), member: "dev".into(), state: AttemptState::Delivered,
            questions: vec![Question { id: "pq".into(), text: "Which DB?".into(), at: 1.0, forwarded: "q1".into(), ..Default::default() }], ..Default::default() });
        let mut child = Team { id: "c".into(), name: "Child".into(), lead: "dev".into(), status: TeamStatus::Running,
            parent: Some(Parent { team: "p".into(), task: 1, lead: "lead".into(), member: "dev".into() }), ..Default::default() };
        child.tasks.push(Task { id: 1, title: "Work".into(), assignee: "dev".into(), status: TaskStatus::Accepted, ..Default::default() });
        child.tasks[0].attempts.push(Attempt { n: 1, id: "ca".into(), member: "dev".into(), state: AttemptState::Reported,
            report: Some(Report { text: "Done".into(), at: 1.0, ..Default::default() }),
            questions: vec![Question { id: "q1".into(), text: "Which DB?".into(), at: 1.0, forwarded: "pq".into(), ..Default::default() }], ..Default::default() });
        store.create(&req(1), &json!({}), parent.clone(), Vec::new()).unwrap();
        store.create(&req(2), &json!({}), child.clone(), Vec::new()).unwrap();

        // N21: the parent's answer leaves the child question Pending.
        super::run("reply", &json!({"request_id":req(3),"team":"p","task":1,"question":"pq","text":"Postgres","by":"human"}), false).unwrap();
        let saved = store.load("c").unwrap().unwrap();
        let question = &saved.tasks[0].attempts[0].questions[0];
        assert_eq!((question.answer.as_str(), question.notice), ("Postgres", Notice::Pending));

        // N22/N23: finish files a Reported attempt the parent inbox lists,
        // and a replay adds no second one.
        let finish = json!({"request_id":req(4),"team":"c","summary":"Sub done","by":"human"});
        super::run("finish", &finish, false).unwrap();
        let parent_now = store.load("p").unwrap().unwrap();
        let attempt = parent_now.tasks[0].attempts.last().unwrap();
        assert_eq!(attempt.state, AttemptState::Reported);
        let inbox = super::run("inbox", &json!({"team":"p","by":"human"}), false).unwrap();
        assert!(inbox["items"].as_array().unwrap().iter().any(|item| item["task"] == 1), "{inbox}");
        let count = parent_now.tasks[0].attempts.len();
        super::run("finish", &finish, false).unwrap();
        assert_eq!(store.load("p").unwrap().unwrap().tasks[0].attempts.len(), count);

        // N24: a reject does not reopen a Stopped sub-team, and the stored
        // reject is not turned into an error by the tree hook (N28).
        let mut stopped = store.load("c").unwrap().unwrap();
        stopped.status = TeamStatus::Stopped;
        store.mutate(None, "c", None, |team, _| { *team = stopped; Ok(Value::Null) }).unwrap();
        super::run("reject", &json!({"request_id":req(5),"team":"p","task":1,"reason":"Rework","by":"human"}), false).unwrap();
        assert_eq!(store.load("c").unwrap().unwrap().status, TeamStatus::Stopped);

        // N25: a second create on the same parent task is refused; a
        // replay after the parent link was lost repairs it.
        let mut parent2 = Team { id: "p2".into(), name: "Parent2".into(), lead: "dev".into(), status: TeamStatus::Running,
            settings: Settings { tree_sessions: 16, ..Default::default() }, ..Default::default() };
        parent2.members.push(Member { id: "dev".into(), role: "Lead".into(), agent: "codex".into(),
            session: Some(SessionRef { name: "codex/x/dev".into(), run_id: "r0".into(), ..Default::default() }), ..Default::default() });
        parent2.tasks.push(Task { id: 2, title: "Delegated".into(), assignee: "dev".into(), status: TaskStatus::InProgress, ..Default::default() });
        store.create(&req(6), &json!({}), parent2.clone(), Vec::new()).unwrap();
        let create = json!({"request_id":req(7),"team":"p2","name":"Sub","by":"human",
            "parent":{"team":"p2","task":2},"lead":{"id":"dev","agent":"codex","role":"Lead"}});
        super::run("create", &create, false).unwrap();
        let second = super::run("create", &json!({"request_id":req(8),"team":"p2","name":"Other","by":"human",
            "parent":{"team":"p2","task":2},"lead":{"id":"dev","agent":"codex","role":"Lead"}}), false);
        assert!(second.unwrap_err().to_string().contains("already has a sub-team"));
        let child_id = store.load("p2").unwrap().unwrap().tasks[0].subteam.clone().unwrap();
        store.mutate(None, "p2", None, |team, _| { team.tasks[0].subteam = None; Ok(Value::Null) }).unwrap();
        super::run("create", &create, false).unwrap();
        assert_eq!(store.load("p2").unwrap().unwrap().tasks[0].subteam.as_deref(), Some(child_id.as_str()));

        // N26: a 20-member plan under a 16-session budget is refused.
        let mut sub = Team { id: "c3".into(), name: "Sub3".into(), lead: "lead".into(), status: TeamStatus::Planning,
            settings: Settings { tree_sessions: 16, ..Default::default() },
            parent: Some(Parent { team: "p2".into(), task: 2, lead: "dev".into(), member: "dev".into() }), ..Default::default() };
        sub.members.push(Member { id: "lead".into(), role: "Lead".into(), agent: "codex".into(), ..Default::default() });
        store.create(&req(9), &json!({}), sub, Vec::new()).unwrap();
        let members: Vec<Value> = (1..=20).map(|n| json!({"id":format!("m{n}"),"role":"Dev","agent":"codex"})).collect();
        let error = super::run("plan", &json!({"request_id":req(10),"team":"c3","by":"human","members":members}), false)
            .unwrap_err().to_string();
        assert!(error.contains("at most 16 sessions"), "{error}");

        // N28: a replay returns the first receipt; a failed upstream
        // forward clears the child's forwarded mark.
        let mut p3 = Team { id: "p3".into(), name: "Parent3".into(), lead: "dev".into(), status: TeamStatus::Running, ..Default::default() };
        p3.members.push(Member { id: "dev".into(), role: "Lead".into(), agent: "codex".into(),
            session: Some(SessionRef { name: "codex/x/dev".into(), run_id: "r0".into(), ..Default::default() }), ..Default::default() });
        p3.tasks.push(Task { id: 3, title: "Delegated".into(), assignee: "dev".into(), status: TaskStatus::InProgress,
            subteam: Some("c4".into()), ..Default::default() });
        p3.tasks[0].attempts.push(Attempt { n: 1, id: "p3a".into(), member: "dev".into(), state: AttemptState::Delivered,
            questions: vec![Question { id: "pq4".into(), text: "Q".into(), at: 1.0, forwarded: "q9".into(), ..Default::default() }], ..Default::default() });
        let mut c4 = Team { id: "c4".into(), name: "Child4".into(), lead: "dev".into(), status: TeamStatus::Running,
            parent: Some(Parent { team: "p3".into(), task: 3, lead: "dev".into(), member: "dev".into() }), ..Default::default() };
        c4.tasks.push(Task { id: 1, title: "W".into(), assignee: "dev".into(), status: TaskStatus::InProgress, ..Default::default() });
        c4.tasks[0].attempts.push(Attempt { n: 1, id: "c4a".into(), member: "dev".into(), state: AttemptState::Delivered,
            questions: vec![Question { id: "q9".into(), text: "Q".into(), at: 1.0, ..Default::default() }], ..Default::default() });
        store.create(&req(11), &json!({}), p3, Vec::new()).unwrap();
        store.create(&req(12), &json!({}), c4, Vec::new()).unwrap();
        let escalate = json!({"request_id":req(13),"team":"c4","question":"q9","by":"human"});
        super::run("escalate", &escalate, false).unwrap();
        let count = store.load("p3").unwrap().unwrap().tasks[0].attempts[0].questions.len();
        super::run("escalate", &escalate, false).unwrap();
        assert_eq!(store.load("p3").unwrap().unwrap().tasks[0].attempts[0].questions.len(), count, "a replay does not duplicate");
        // N24: a question goes up one level at a time: c6 -> c4 -> p3, and
        // the parent's answer travels back down to the member that asked.
        let mut c6 = Team { id: "c6".into(), name: "Child6".into(), lead: "dev".into(), status: TeamStatus::Running,
            parent: Some(Parent { team: "c4".into(), task: 1, lead: "dev".into(), member: "dev".into() }), ..Default::default() };
        c6.tasks.push(Task { id: 1, title: "Deep work".into(), assignee: "dev".into(), status: TaskStatus::InProgress, ..Default::default() });
        c6.tasks[0].attempts.push(Attempt { n: 1, id: "c6a".into(), member: "dev".into(), state: AttemptState::Delivered,
            questions: vec![Question { id: "q11".into(), text: "Which cache?".into(), at: 1.0, ..Default::default() }], ..Default::default() });
        store.create(&req(16), &json!({}), c6, Vec::new()).unwrap();
        let mut c4_deep = store.load("c4").unwrap().unwrap();
        c4_deep.children.push("c6".into());
        c4_deep.tasks[0].subteam = Some("c6".into());
        store.mutate(None, "c4", None, |team, _| { *team = c4_deep; Ok(Value::Null) }).unwrap();
        super::run("escalate", &json!({"request_id":req(17),"team":"c6","question":"q11","by":"human"}), false).unwrap();
        let c4_level = store.load("c4").unwrap().unwrap();
        let carrier = c4_level.tasks[0].attempts[0].questions.iter().find(|q| q.source == "q11").cloned();
        let carrier = carrier.unwrap_or_else(|| panic!("no carrier in c4: {:?}", c4_level.tasks[0].attempts[0].questions));
        // The middle level passes the same question one level up.
        super::run("escalate", &json!({"request_id":req(18),"team":"c4","question":carrier.id,"by":"human"}), false).unwrap();
        let top = store.load("p3").unwrap().unwrap();
        let top_question = top.tasks[0].attempts[0].questions.iter().find(|q| q.source == carrier.id).cloned();
        let top_question = top_question.unwrap_or_else(|| panic!("no question in p3: {:?}", top.tasks[0].attempts[0].questions));
        // The top lead answers by the question only (N23), and the answer
        // reaches q11 two levels down.
        super::run("reply", &json!({"request_id":req(19),"team":"p3","question":top_question.id,"text":"Redis","by":"human"}), false).unwrap();
        let deep = store.load("c6").unwrap().unwrap();
        assert_eq!(deep.tasks[0].attempts[0].questions[0].answer, "Redis", "{:?}", deep.tasks[0].attempts[0].questions);
        let middle = store.load("c4").unwrap().unwrap();
        assert!(middle.tasks[0].attempts[0].questions.iter().any(|q| q.id == carrier.id && q.answer == "Redis"));
        // N25: a create refused before the child exists releases the parent
        // task's reservation; a corrected create for the same task works.
        let mut p5 = Team { id: "p5".into(), name: "Parent5".into(), lead: "dev".into(), status: TeamStatus::Running, ..Default::default() };
        p5.members.push(Member { id: "dev".into(), role: "Lead".into(), agent: "codex".into(),
            session: Some(SessionRef { name: "codex/x/dev".into(), run_id: "r0".into(), ..Default::default() }), ..Default::default() });
        p5.tasks.push(Task { id: 1, title: "Delegated".into(), assignee: "dev".into(), status: TaskStatus::InProgress, ..Default::default() });
        store.create(&req(20), &json!({}), p5, Vec::new()).unwrap();
        let refused = super::run("create", &json!({"request_id":req(21),"team":"p5","name":"Sub","by":"human",
            "parent":{"team":"p5","task":1},"lead":{"id":"dev","agent":"codex","role":"Lead"},
            "members":[{"id":"dev","role":"Helper","agent":"codex"}]}), false).unwrap_err().to_string();
        assert!(refused.contains("used twice"), "{refused}");
        assert!(store.load("p5").unwrap().unwrap().tasks[0].subteam.is_none(), "a refused create leaves the task undelegated");
        super::run("create", &json!({"request_id":req(22),"team":"p5","name":"Sub","by":"human",
            "parent":{"team":"p5","task":1},"lead":{"id":"dev","agent":"codex","role":"Lead"}}), false).unwrap();
        assert_eq!(store.load("p5").unwrap().unwrap().tasks[0].subteam.as_deref(), Some(req(22).as_str()));
        // N26: an added member without a session still takes a tree session:
        // at 16/16 the 17th is refused (it used to count 0 and launch).
        let mut full = Team { id: "m1".into(), name: "Full".into(), lead: "lead".into(), status: TeamStatus::Planning,
            settings: Settings { tree_sessions: 16, ..Default::default() }, ..Default::default() };
        for n in 0..16 {
            full.members.push(Member { id: if n == 0 { "lead".into() } else { format!("m{n}") }, role: "Dev".into(), agent: "codex".into(), ..Default::default() });
        }
        store.create(&req(23), &json!({}), full, Vec::new()).unwrap();
        let error = super::run("add-member", &json!({"request_id":req(24),"team":"m1","by":"human",
            "member":{"id":"extra","role":"Ops","agent":"codex"}}), false).unwrap_err().to_string();
        assert!(error.contains("at most 16 sessions"), "{error}");
        // N26: a same-size re-plan counts the roster once, not twice.
        let members: Vec<Value> = (1..=15).map(|n| json!({"id":format!("m{n}"),"role":"Dev","agent":"codex"})).collect();
        super::run("plan", &json!({"request_id":req(25),"team":"m1","by":"human",
            "members":members,"tasks":[{"id":1,"title":"A","assignee":"lead"}]}), false)
            .unwrap_or_else(|error| panic!("a same-size re-plan is allowed: {error:#}"));
        // N27: a forward that landed but lost its receipt answers with the
        // receipt instead of refusing the question as already escalated.
        let again = super::run("escalate", &json!({"request_id":req(26),"team":"c4","question":"q9","by":"human"}), false);
        assert_eq!(again.unwrap()["status"], "escalated");
        let mut stopped_parent = store.load("p3").unwrap().unwrap();
        stopped_parent.tasks[0].status = TaskStatus::Accepted;
        store.mutate(None, "p3", None, |team, _| { *team = stopped_parent; Ok(Value::Null) }).unwrap();
        let mut c5 = store.load("c4").unwrap().unwrap();
        c5.tasks[0].attempts[0].questions.push(Question { id: "q10".into(), text: "Q2".into(), at: 2.0, ..Default::default() });
        store.mutate(None, "c4", None, |team, _| { *team = c5; Ok(Value::Null) }).unwrap();
        let _ = super::run("escalate", &json!({"request_id":req(14),"team":"c4","question":"q10","by":"human"}), false);
        let child = store.load("c4").unwrap().unwrap();
        assert!(child.tasks[0].attempts[0].questions.iter().find(|q| q.id == "q10").unwrap().forwarded.is_empty());
        super::set_test_caller_identities(None);
        for (key, value) in old {
            match value { Some(value) => std::env::set_var(key, value), None => std::env::remove_var(key) }
        }
    });
}

#[test]
fn the_tree_changes_work_through_the_command_helpers() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams.sqlite3"), true).unwrap();
    let request = |n: u8| format!("00000000-0000-4000-8000-00000000000{n}");
    let mut parent = Team { id: "p".into(), name: "Parent".into(), lead: "lead".into(), status: TeamStatus::Running, children: vec!["c".into(), "c2".into()], ..Default::default() };
    parent.tasks.push(Task { id: 1, title: "Delegated".into(), assignee: "dev".into(), subteam: Some("c".into()), ..Default::default() });
    parent.tasks.push(Task { id: 2, title: "Other".into(), assignee: "dev".into(), ..Default::default() });
    let mut child = Team { id: "c".into(), name: "Child".into(), lead: "dev".into(), status: TeamStatus::Done, parent: Some(Parent { team: "p".into(), task: 1, lead: "lead".into(), member: "dev".into() }), ..Default::default() };
    child.finish = Some(Finish { by: "Lead".into(), summary: "Sub done".into(), at: 1.0 });
    let mut sibling = Team { id: "c2".into(), name: "Sibling".into(), lead: "dev".into(), status: TeamStatus::Running, children: vec!["g".into()], parent: Some(Parent { team: "p".into(), task: 2, lead: "lead".into(), member: "dev".into() }), ..Default::default() };
    sibling.tasks.push(Task { id: 1, title: "Work".into(), assignee: "dev".into(), ..Default::default() });
    let grand = Team { id: "g".into(), name: "Grand".into(), lead: "dev".into(), status: TeamStatus::Running, parent: Some(Parent { team: "c2".into(), task: 1, lead: "dev".into(), member: "dev".into() }), ..Default::default() };
    for (i, team) in [parent.clone(), child.clone(), sibling.clone(), grand.clone()].into_iter().enumerate() {
        store.create(&request(i as u8 + 1), &json!({}), team, Vec::new()).unwrap();
    }
    // 9.5: finish files the report, reject reopens with Rework.
    file_subteam_report(&mut store, "c").unwrap();
    let parent = store.load("p").unwrap().unwrap();
    let task = parent.task(1).unwrap();
    assert_eq!(task.status, TaskStatus::Submitted);
    assert!(task.attempts.last().unwrap().report.as_ref().unwrap().text.contains("Sub-team c Child"));
    reopen_rejected_subteam(&mut store, "p", &json!({"task":1,"reason":"Fix the migration"})).unwrap();
    let child = store.load("c").unwrap().unwrap();
    assert_eq!(child.status, TeamStatus::Running);
    assert!(child.phases.iter().any(|phase| phase.title == "Rework" && phase.reason == "Fix the migration"));
    // 9.6: escalate forwards and the parent reply returns to the child.
    let mut child = child;
    child.tasks.push(Task { id: 1, title: "Child work".into(), assignee: "dev".into(), ..Default::default() });
    child.tasks[0].attempts.push(Attempt { n: 1, id: "a1".into(), member: "dev".into(), state: AttemptState::Delivered, questions: vec![Question { id: "q1".into(), text: "Which DB?".into(), at: 1.0, ..Default::default() }], ..Default::default() });
    store.mutate(None, "c", None, |team, _| { *team = child.clone(); Ok(Value::Null) }).unwrap();
    let link = child.parent.clone().unwrap();
    let forwarded = ops::forward_question(&child, &child.tasks[0].attempts[0].questions[0]);
    store.mutate(None, "c", None, |team, _| { team.tasks[0].attempts[0].questions[0].forwarded = forwarded.clone(); Ok(Value::Null) }).unwrap();
    forward_to_parent(&mut store, &child, &forwarded, "q1", "Which DB?", &link).unwrap();
    let parent = store.load("p").unwrap().unwrap();
    assert!(parent.task(1).unwrap().attempts.iter().flat_map(|a| &a.questions).any(|q| q.id == forwarded));
    // N23: the inbox's reply names only the question; its task is found.
    route_reply_down(&mut store, "p", &json!({"question":forwarded,"text":"Postgres"})).unwrap();
    let child = store.load("c").unwrap().unwrap();
    assert_eq!(child.tasks[0].attempts[0].questions[0].answer, "Postgres");
    // 9.7: archive needs closed children, cancel asks, stop cascades two levels.
    let parent = store.load("p").unwrap().unwrap();
    assert!(ensure_children_closed(&store, &parent).is_err());
    let cancel = json!({"task":1});
    assert!(choose_subteam(&mut store, &parent, &cancel).is_err());
    let stop = json!({"task":1,"subteam":"stop"});
    choose_subteam(&mut store, &parent, &stop).unwrap();
    assert_eq!(store.load("c").unwrap().unwrap().status, TeamStatus::Stopped);
    cascade_status(&mut store, "p", "stop", 0).unwrap();
    for id in ["c", "c2", "g"] {
        assert_eq!(store.load(id).unwrap().unwrap().status, TeamStatus::Stopped, "{id}");
    }
    let parent = store.load("p").unwrap().unwrap();
    ensure_children_closed(&store, &parent).unwrap();
}

#[test]
fn a_rejected_subteam_report_reopens_it_but_a_final_reject_does_not() {
    // N28: the tree hooks run after the reject is stored; they never turn
    // it into an error, reopen only a non-final reject of a finished
    // sub-team, and leave the parent task with the sub-team.
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        super::set_test_caller_identities(Some((None, None, None)));
        let mut store = Store::open(true).unwrap();
        let req = |n: u8| format!("00000000-0000-4000-8000-0000000000{n:02}");
        let pair = |parent: &str, child: &str, child_status: TeamStatus| -> (Team, Team) {
            let mut parent_team = Team { id: parent.into(), name: parent.into(), lead: "lead".into(), status: TeamStatus::Running, ..Default::default() };
            parent_team.members.push(Member { id: "lead".into(), role: "Lead".into(), agent: "codex".into(),
                session: Some(SessionRef { name: "codex/x/lead".into(), run_id: "r0".into(), ..Default::default() }), ..Default::default() });
            parent_team.members.push(Member { id: "dev".into(), role: "Backend".into(), agent: "codex".into(), ..Default::default() });
            parent_team.tasks.push(Task { id: 1, title: "Delegated".into(), assignee: "dev".into(), status: TaskStatus::Submitted,
                subteam: Some(child.into()), ..Default::default() });
            parent_team.tasks[0].attempts.push(Attempt { n: 1, id: "pa".into(), member: "dev".into(), state: AttemptState::Reported,
                report: Some(Report { text: format!("[Sub-team {child} Child] Done"), at: 1.0, ..Default::default() }), ..Default::default() });
            parent_team.children.push(child.into());
            let child_team = Team { id: child.into(), name: "Child".into(), lead: "dev".into(), status: child_status,
                parent: Some(Parent { team: parent.into(), task: 1, lead: "lead".into(), member: "dev".into() }),
                finish: Some(Finish { by: "Lead".into(), summary: "Done".into(), at: 1.0 }), ..Default::default() };
            (parent_team, child_team)
        };
        let (p1, c1) = pair("p1", "c1", TeamStatus::Done);
        store.create(&req(1), &json!({}), p1, Vec::new()).unwrap();
        store.create(&req(2), &json!({}), c1, Vec::new()).unwrap();
        super::run("reject", &json!({"request_id":req(3),"team":"p1","task":1,"reason":"Fix the migration","by":"human"}), false)
            .unwrap();
        assert_eq!(store.load("c1").unwrap().unwrap().status, TeamStatus::Running);
        let p1_now = store.load("p1").unwrap().unwrap();
        assert_eq!(p1_now.tasks[0].status, TaskStatus::InProgress);
        assert_eq!(p1_now.tasks[0].attempts.last().unwrap().state, AttemptState::Rejected);
        assert!(!crate::state::teams::plan::ready(&p1_now).contains(&1), "the reworking sub-team keeps its task");
        // C5: the reworked sub-team finishes again and its report is filed.
        super::run("finish", &json!({"request_id":req(4),"team":"c1","summary":"The migration is fixed","by":"human"}), false)
            .unwrap();
        let p1_now = store.load("p1").unwrap().unwrap();
        assert_eq!((p1_now.tasks[0].status, p1_now.tasks[0].attempts.len()), (TaskStatus::Submitted, 2));
        assert!(p1_now.tasks[0].attempts.last().unwrap().report.as_ref().unwrap().text.contains("The migration is fixed"));
        // A final reject ends the task and does not reopen a finished child.
        let (p2, c2) = pair("p2", "c2", TeamStatus::Done);
        store.create(&req(5), &json!({}), p2, Vec::new()).unwrap();
        store.create(&req(6), &json!({}), c2, Vec::new()).unwrap();
        super::run("reject", &json!({"request_id":req(7),"team":"p2","task":1,"reason":"Wrong approach","final":true,"by":"human"}), false)
            .unwrap();
        assert_eq!(store.load("c2").unwrap().unwrap().status, TeamStatus::Done, "a final reject leaves the sub-team");
        assert_eq!(store.load("p2").unwrap().unwrap().tasks[0].status, TaskStatus::Rejected);
        // A stopped child does not turn the stored reject into an error.
        let (p3, c3) = pair("p3", "c3", TeamStatus::Stopped);
        store.create(&req(8), &json!({}), p3, Vec::new()).unwrap();
        store.create(&req(9), &json!({}), c3, Vec::new()).unwrap();
        let rejected = super::run("reject", &json!({"request_id":req(10),"team":"p3","task":1,"reason":"Stop it","by":"human"}), false)
            .unwrap();
        assert!(rejected.get("subteam_reopen_error").is_none(), "{rejected}");
        assert_eq!(store.load("c3").unwrap().unwrap().status, TeamStatus::Stopped);
        super::set_test_caller_identities(None);
    });
}

/// #717: submit queues, force runs within the resource rule, results list it.
#[test]
fn test_submit_force_and_results_use_the_projects_suite() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".zerus")).unwrap();
    std::fs::write(
        dir.path().join(".zerus/tests.toml"),
        "version = 1\n[budgets]\nsuite_minutes = 1\n[[provider]]\nname = \"echo\"\nkind = \"command\"\ncommand = \"echo hello\"\n",
    )
    .unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let id = "11111111-1111-4111-8111-111111111111";
        let team = Team { id: id.into(), lead: "lead".into(), folder: dir.path().display().to_string(), ..Default::default() };
        let mut store = Store::open_at(&super::super::store::path(), true).unwrap();
        store.create(id, &json!({}), team, vec![]).unwrap();
        let queued = run("test", &json!({"team":id,"action":"submit","suite":"echo","targets":[]}), false).unwrap();
        assert_eq!(queued["status"], "queued", "{queued}");
        let ran = run("test", &json!({"team":id,"action":"force","suite":"echo","targets":[]}), false).unwrap();
        assert_eq!(ran["results"][0]["status"], "passed", "{ran}");
        let listed = run("test", &json!({"team":id,"action":"results"}), false).unwrap();
        assert!(!listed["items"].as_array().unwrap().is_empty(), "{listed}");
    });
}

#[test]
fn the_commands_that_create_work_start_the_worker() {
    for op in ["approve", "resume", "extend", "approve-phase", "ask", "reply", "release", "expect"] {
        assert!(starts_worker(op), "{op}");
    }
    for op in ["report", "cancel", "finish", "discard-phase", "hold"] {
        assert!(!starts_worker(op), "{op}");
    }
}
