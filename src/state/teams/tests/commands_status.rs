use super::*;
use super::support::*;

/// N37/F1.6: status reads each member's own folder - a worktree member's
/// worktree, not the team folder - one read per distinct folder, and flags
/// a member in another repository. N36: the team's project label is a GUI
/// group name and never decides the flag.
#[test]
fn status_reads_each_members_own_folder_once_and_flags_another_repository() {
    use crate::state::teams::dispatcher;
    use crate::state::teams::model::{Member, MemberFolder, Team, TeamStatus};
    let dir = tempfile::tempdir().unwrap();
    // A Windows runner's TEMP is an 8.3 short path (RUNNER~1); status names
    // folders in the long form git reports, so the test works in that form.
    let base = std::fs::canonicalize(dir.path()).unwrap();
    let base = std::path::PathBuf::from(base.to_string_lossy().trim_start_matches(r"\\?\").to_owned());
    dispatcher::set_git_ceiling(&base.display().to_string());
    // Two different repositories: distinct paths and distinct initial
    // commits. Identical content committed in the same second makes the same
    // root commit, and clone_of would rightly call them one repository, so
    // the foreign repo must carry its own history (asserted below).
    let make_repo = |path: &std::path::Path, name: &str| {
        std::fs::create_dir(path).unwrap();
        let git = |args: &[&str]| std::process::Command::new("git").arg("-C").arg(path).args(args).status().unwrap();
        assert!(git(&["init", "-q"]).success());
        assert!(git(&["config", "user.email", "t@example.com"]).success());
        assert!(git(&["config", "user.name", "T"]).success());
        std::fs::write(path.join(format!("{name}.txt")), name).unwrap();
        assert!(git(&["add", "."]).success());
        assert!(git(&["commit", "-qm", name]).success());
    };
    let root = |path: &std::path::Path| {
        String::from_utf8(
            std::process::Command::new("git").arg("-C").arg(path).args(["rev-list", "--max-parents=0", "HEAD"]).output().unwrap().stdout,
        )
        .unwrap()
        .trim()
        .to_owned()
    };
    let repo = base.join("project");
    make_repo(&repo, "project");
    let main = String::from_utf8(std::process::Command::new("git").arg("-C").arg(&repo).args(["rev-parse", "--abbrev-ref", "HEAD"]).output().unwrap().stdout).unwrap().trim().to_owned();
    let folder = repo.display().to_string();
    let mut team = Team {
        id: "abcdef12-0000-4000-8000-000000000000".into(),
        name: "T".into(),
        lead: "lead".into(),
        status: TeamStatus::Running,
        folder: folder.clone(),
        // The GUI's group name, not a project id or a path (N36).
        project: "Checkout".into(),
        ..Default::default()
    };
    team.members.push(Member { id: "lead".into(), role: "Lead".into(), agent: "claude".into(), ..Default::default() });
    team.members.push(Member { id: "dev".into(), role: "Backend".into(), agent: "dsh".into(), folder: MemberFolder::Worktree, ..Default::default() });
    team.members.push(Member { id: "qa".into(), role: "QA".into(), agent: "codex".into(), ..Default::default() });
    let foreign = base.join("sibling");
    make_repo(&foreign, "sibling");
    assert_ne!(root(&repo), root(&foreign), "the foreign repository must have its own history");
    team.members.push(Member { id: "out".into(), role: "Ops".into(), agent: "codex".into(), directory: foreign.display().to_string(), ..Default::default() });
    // A member on another computer works in a folder this one cannot read.
    team.members.push(Member {
        id: "far".into(),
        role: "Docs".into(),
        agent: "codex".into(),
        computer: "msi".into(),
        directory: foreign.display().to_string(),
        ..Default::default()
    });
    // The worktree the dispatcher would create for dev.
    let worktree = dispatcher::member_folder_path(&team, team.member("dev").unwrap()).unwrap();
    assert!(std::process::Command::new("git").arg("-C").arg(&repo).args(["worktree", "add", "--quiet", "-b", "team/abcdef12/dev", &worktree, "HEAD"]).status().unwrap().success());
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open(true).unwrap();
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), team.clone(), vec![]).unwrap();
        dispatcher::reset_folder_facts_reads();
        let value = run("status", &json!({"team":team.id}), false).unwrap();
        assert_eq!(value["members"].as_array().unwrap().len(), 5);
        let member = |id: &str| value["members"].as_array().unwrap().iter().find(|member| member["id"] == id).cloned().unwrap();
        // dev shows its own worktree's branch and folder, not the team folder's.
        let dev = member("dev");
        assert_eq!(dev["branch"], "team/abcdef12/dev");
        assert_eq!(dev["folder_path"], worktree);
        assert_eq!(dev["outside_project"], false);
        assert_eq!(dev["project"], member("lead")["project"]);
        assert_eq!(dev["project"].as_str(), Some(folder.replace('\\', "/").as_str()));
        // The shared members show the team folder's branch; the foreign one
        // is flagged and the group name plays no part.
        let lead = member("lead");
        assert_eq!(lead["branch"], main);
        assert_eq!(lead["outside_project"], false);
        let out = member("out");
        assert_eq!(out["outside_project"], true);
        assert_eq!(out["folder_path"], foreign.display().to_string());
        assert!(!out["branch"].as_str().unwrap_or_default().is_empty());
        // The remote member's folder is not read here (C11): its facts stay
        // empty even though a local folder has that path.
        let far = member("far");
        assert_eq!((far["project"].as_str(), far["branch"].as_str()), (Some(""), Some("")));
        assert_eq!(far["outside_project"], false);
        // One git read per distinct folder: the team's, dev's worktree and
        // the foreign one, for five members - not one per member.
        assert_eq!(dispatcher::folder_facts_reads(), 3);
    });
}

/// Task #5 (team ede724c4): the cheap status answers as before. On a fixture
/// team in real repositories (a shared member, a worktree member, a detached
/// worktree, another repository) with an estimate history, the status reads
/// the folder facts from the repositories' files and estimates from one
/// grouped history, and its answer equals the old way's (git for every folder,
/// an estimate per task over the whole history). It never writes the estimate
/// history; a write that accepts a task records it. A status slower than the
/// threshold leaves one slow-calls line naming its phases.
#[test]
fn the_cheap_status_answers_as_before_and_writes_no_history() {
    use crate::state::teams::dispatcher;
    use crate::state::teams::model::{Attempt, Member, MemberFolder, Report, Task, Team, TeamStatus};
    let dir = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(dir.path()).unwrap();
    let base = std::path::PathBuf::from(base.to_string_lossy().trim_start_matches(r"\\?\").to_owned());
    dispatcher::set_git_ceiling(&base.display().to_string());
    let git = |path: &std::path::Path, args: &[&str]| {
        assert!(std::process::Command::new("git").arg("-C").arg(path).args(args).status().unwrap().success(), "git {args:?}");
    };
    let make_repo = |path: &std::path::Path, name: &str| {
        std::fs::create_dir(path).unwrap();
        git(path, &["init", "-q"]);
        git(path, &["config", "user.email", "t@example.com"]);
        git(path, &["config", "user.name", "T"]);
        std::fs::write(path.join(format!("{name}.txt")), name).unwrap();
        git(path, &["add", "."]);
        git(path, &["commit", "-qm", name]);
    };
    let repo = base.join("project");
    make_repo(&repo, "project");
    let foreign = base.join("sibling");
    make_repo(&foreign, "sibling");
    let detached = base.join("detached");
    git(&repo, &["worktree", "add", "--quiet", "--detach", &detached.display().to_string(), "HEAD"]);
    // A clone of the team's repository (its origin is the team folder).
    let clone = base.join("clone");
    git(&base, &["clone", "-q", &repo.display().to_string(), &clone.display().to_string()]);
    let origin = String::from_utf8(std::process::Command::new("git").arg("-C").arg(&clone).args(["config", "--get", "remote.origin.url"]).output().unwrap().stdout).unwrap();
    assert_eq!(dispatcher::origin_url_from_files(&clone.display().to_string()).as_deref(), Some(origin.trim()), "the clone's origin from its config file");
    assert_eq!(dispatcher::origin_url_from_files(&foreign.display().to_string()), None, "no origin: git answers");
    let mut team = Team {
        id: "abcdef12-0000-4000-8000-000000000005".into(),
        name: "T".into(),
        lead: "lead".into(),
        status: TeamStatus::Running,
        folder: repo.display().to_string(),
        ..Default::default()
    };
    team.members.push(Member { id: "lead".into(), role: "Lead".into(), agent: "claude".into(), ..Default::default() });
    team.members.push(Member { id: "dev".into(), role: "Backend".into(), agent: "codex".into(), effort: "high".into(), folder: MemberFolder::Worktree, ..Default::default() });
    team.members.push(Member { id: "qa".into(), role: "QA".into(), agent: "codex".into(), directory: detached.display().to_string(), ..Default::default() });
    team.members.push(Member { id: "out".into(), role: "Ops".into(), agent: "kimi".into(), directory: foreign.display().to_string(), ..Default::default() });
    team.members.push(Member { id: "cl".into(), role: "Ops".into(), agent: "kimi".into(), directory: clone.display().to_string(), ..Default::default() });
    let worktree = dispatcher::member_folder_path(&team, team.member("dev").unwrap()).unwrap();
    git(&repo, &["worktree", "add", "--quiet", "-b", "team/abcdef12/dev", &worktree, "HEAD"]);
    let reported = |member: &str, at: f64| Attempt {
        n: 1,
        id: format!("00000000-0000-4000-8000-{:012}", at as u64),
        member: member.into(),
        sent_at: at - 600.0,
        delivered_at: Some(at - 600.0),
        report: Some(Report { text: "done".into(), at, ..Default::default() }),
        ..Default::default()
    };
    team.tasks = vec![
        Task { id: 1, title: "Save endpoint".into(), assignee: "dev".into(), status: TaskStatus::Accepted, attempts: vec![reported("dev", 1000.0)], ..Default::default() },
        Task { id: 2, title: "Tests pass".into(), assignee: "qa".into(), status: TaskStatus::InProgress, attempts: vec![reported("qa", 2000.0)], ..Default::default() },
        Task { id: 3, title: "Deploy".into(), assignee: "out".into(), ..Default::default() },
        Task { id: 4, title: "Docs".into(), role: "Writer".into(), ..Default::default() },
    ];
    // Every member folder: the files answer what git answers.
    for folder in [repo.display().to_string(), worktree.clone(), detached.display().to_string(), foreign.display().to_string()] {
        let fast = dispatcher::folder_facts_from_files(&folder);
        dispatcher::set_old_status(true);
        let by_git = dispatcher::folder_facts(&folder);
        dispatcher::set_old_status(false);
        assert_eq!(fast, Some(by_git), "{folder}");
    }
    crate::state::with_test_state_root(dir.path(), || {
        let entries: Vec<Value> = (0..30usize)
            .map(|n| {
                let (role, agent, effort) = (["Backend", "QA", "backend", "Ops"][n % 4], ["codex", "CODEX", "kimi"][n % 3], ["high", "", "High"][n % 3]);
                json!({"role":role,"agent":agent,"effort":effort,"minutes":(n * 7 % 50) + 1,"task":1000 + n,"team":"other"})
            })
            .collect();
        let history = json!({"entries":entries,"imported":["other"]});
        let path = crate::state::teams::estimates::path().unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, history.to_string()).unwrap();
        let mut store = Store::open(true).unwrap();
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), team.clone(), vec![]).unwrap();
        let written = std::fs::read_to_string(&path).unwrap();
        let new = run("status", &json!({"team":team.id}), false).unwrap();
        dispatcher::set_old_status(true);
        let old = run("status", &json!({"team":team.id}), false).unwrap();
        dispatcher::set_old_status(false);
        assert_eq!(new, old, "the cheap status answers as before");
        let member = |id: &str| new["members"].as_array().unwrap().iter().find(|member| member["id"] == id).cloned().unwrap();
        assert_eq!((member("dev")["branch"].as_str(), member("qa")["branch"].as_str()), (Some("team/abcdef12/dev"), Some("HEAD")));
        assert_eq!(member("out")["outside_project"], true);
        assert_eq!(member("cl")["outside_project"], false, "a clone of the team's repository");
        assert!(new["tasks"].as_array().unwrap().iter().filter(|task| task["status"] != "accepted").all(|task| task["estimate_source"].is_string()), "{}", new["tasks"]);
        // A read writes nothing: task #1 is accepted and not in the history yet.
        assert_eq!(std::fs::read_to_string(&path).unwrap(), written, "status never writes the estimate history");
        // A write that accepts a task records the team's accepted tasks.
        store
            .mutate(None, &team.id, None, |team, _| {
                team.task_mut(2).unwrap().status = TaskStatus::Accepted;
                Ok(Value::Null)
            })
            .unwrap();
        let recorded: crate::state::teams::estimates::History = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let ours: Vec<u32> = recorded.entries.iter().filter(|entry| entry.team == team.id).map(|entry| entry.task).collect();
        assert_eq!(ours, vec![1, 2], "{:?}", recorded.entries.len());
        // A status over the threshold leaves one slow-calls line naming its phases.
        let log = crate::state::slow_calls::log_path();
        let _ = std::fs::remove_file(&log);
        super::set_status_slow_ms(Some(0));
        run("status", &json!({"team":team.id}), false).unwrap();
        super::set_status_slow_ms(None);
        let text = std::fs::read_to_string(&log).unwrap();
        let line: Value = serde_json::from_str(text.lines().last().unwrap()).unwrap();
        assert_eq!((line["command"].as_str(), line["subject"].as_str()), (Some("team status"), Some(team.id.as_str())));
        for phase in ["open", "load", "view_serialize", "view_estimates", "view_folders", "bound"] {
            assert!(line["timings_ms"][phase].is_u64(), "{phase}: {line}");
        }
        // Not slow: no line.
        std::fs::remove_file(&log).unwrap();
        run("status", &json!({"team":team.id,"timings":true}), false).map(|value| assert!(value["timings_ms"]["total"].is_u64())).unwrap();
        assert!(!log.exists() || std::fs::read_to_string(&log).unwrap().is_empty());
    });
}

/// U36 through `hgs team`: the implementer's report enters review, status
/// shows the stage, its candidates and its span, the reviewer's verdict is
/// proven by its stage id alone, and the owner's overrides are mutations.
/// U38: `spans` gives one task's timeline, or a phase's or the plan's in
/// pages by task id, with one bounded read of the journal.
#[test]
fn spans_give_a_task_or_a_page_of_timelines() {
    let (team, _, _) = review_team();
    assert!(USAGE.contains("|spans|") && !MUTATIONS.contains(&"spans"));
    with_review_team(&team, || {
        let one = run("spans", &json!({"team":"rv","task":1}), false).unwrap();
        assert_eq!(one["tasks"].as_array().unwrap().len(), 1, "{one}");
        assert_eq!(one["tasks"][0]["task"], 1);
        assert!(one["tasks"][0]["spans"].is_array() && one["tasks"][0]["totals"].is_object(), "{one}");
        assert_eq!((one["events"]["truncated"].clone(), one["next_cursor"].clone()), (json!(false), Value::Null));
        let page = run("spans", &json!({"team":"rv","limit":2}), false).unwrap();
        let ids: Vec<Value> = page["tasks"].as_array().unwrap().iter().map(|task| task["task"].clone()).collect();
        assert_eq!((ids, page["next_cursor"].clone()), (vec![json!(1), json!(2)], json!(2)));
        let rest = run("spans", &json!({"team":"rv","limit":2,"after":2}), false).unwrap();
        assert_eq!((rest["tasks"][0]["task"].clone(), rest["next_cursor"].clone()), (json!(3), Value::Null));
        let phase = run("spans", &json!({"team":"rv","phase":2}), false).unwrap();
        assert_eq!(phase["tasks"].as_array().unwrap().len(), 1);
        assert!(run("spans", &json!({"team":"rv","task":99}), false).is_err());
    });
}

/// U31: the lead's inbox names a report made ahead of its draft phase (an
/// older record's work ahead; task 158 runs no draft) as "worked ahead of
/// phase N", through `hgs team inbox`.
#[test]
fn a_worked_ahead_report_says_so_in_the_leads_inbox() {
    use crate::state::teams::model::{Attempt, AttemptState, Report, TaskStatus};
    let (mut team, _, _) = review_team();
    team.phases.iter_mut().find(|phase| phase.n == 2).unwrap().state = crate::state::teams::model::PhaseState::Draft;
    let task = team.task_mut(3).unwrap();
    task.status = TaskStatus::Submitted;
    task.attempts.push(Attempt {
        n: 1,
        id: "00000000-0000-4000-8000-000000000301".into(),
        member: "dev".into(),
        state: AttemptState::Reported,
        report: Some(Report { text: "Docs written".into(), at: 1.0, ..Default::default() }),
        ..Default::default()
    });
    with_review_team(&team, || {
        let inbox = run("inbox", &json!({"team":"rv"}), false).unwrap();
        let items = inbox["items"].as_array().cloned().unwrap_or_default();
        let item = items.iter().find(|item| item["task"] == 3).cloned().unwrap_or_else(|| panic!("{inbox}"));
        assert_eq!(item["summary"], "worked ahead of phase 2: Docs written", "{item}");
        // An ordinary report keeps its text alone.
        let mut team = stored();
        team.phases.iter_mut().find(|phase| phase.n == 2).unwrap().state = crate::state::teams::model::PhaseState::Approved;
        let ordinary = super::inbox_view_for(&team, &team.lead);
        assert!(!ordinary.to_string().contains("worked ahead"), "{ordinary}");
    });
}

#[test]
fn evidence_folders_are_a_setting_status_carries() {
    // N19
    let (team, _, _) = review_team();
    with_review_team(&team, || {
        call("settings", json!({"settings":{"evidence_folders":["shots","D:/work/team-phases/shots-v2"]},"by":"human"})).unwrap();
        assert_eq!(stored().settings.evidence_folders, vec!["shots".to_owned(), "D:/work/team-phases/shots-v2".to_owned()]);
        let status = run("status", &json!({"team":"rv"}), false).unwrap();
        assert_eq!(status["settings"]["evidence_folders"][1], "D:/work/team-phases/shots-v2");
        let error = call("settings", json!({"settings":{"evidence_folders":["  "]},"by":"human"})).unwrap_err().to_string();
        assert!(error.contains("An evidence folder"), "{error}");
    });
}

#[test]
fn findings_pages_are_stable_with_a_before_cursor() {
    use crate::state::teams::model::{Finding, Team};
    let mut team = Team { ..Default::default() };
    for n in 1..=120 {
        team.findings.push(Finding { id: format!("f{n}"), severity: "normal".into(), title: format!("F{n}"), scenario: "s".into(), state: "open".into(), ..Default::default() });
    }
    let first = super::findings_page(&team, None);
    assert_eq!(first["items"].as_array().unwrap().len(), 50);
    assert_eq!(first["items"][0]["id"], "f120");
    assert_eq!(first["next_cursor"], 71);
    let second = super::findings_page(&team, Some(71));
    assert_eq!(second["items"].as_array().unwrap().len(), 50);
    assert_eq!(second["items"][0]["id"], "f70");
    assert_eq!(second["next_cursor"], 21);
    let third = super::findings_page(&team, Some(21));
    assert_eq!(third["items"].as_array().unwrap().len(), 20);
    assert_eq!(third["next_cursor"], serde_json::Value::Null);
    // A new finding does not shift the second page.
    team.findings.push(Finding { id: "f121".into(), severity: "normal".into(), title: "New".into(), scenario: "s".into(), state: "open".into(), ..Default::default() });
    assert_eq!(super::findings_page(&team, Some(71))["items"][0]["id"], "f70");
}

#[test]
fn leftovers_lists_running_sessions_of_finished_teams() {
    use crate::state::teams::dispatcher::Seen;
    use crate::state::teams::model::{Member, MemberSeen, SessionRef, Team, TeamStatus};
    let mut done = Team { id: "t1".into(), name: "T".into(), lead: "lead".into(), status: TeamStatus::Done, ..Default::default() };
    done.members.push(Member { id: "lead".into(), role: "Lead".into(), agent: "claude".into(), ..Default::default() });
    let mut dev = Member { id: "dev".into(), role: "Backend".into(), agent: "codex".into(), ..Default::default() };
    dev.session = Some(SessionRef { name: "claude/x/dev".into(), ..Default::default() });
    dev.seen = Some(MemberSeen { at: 30.0, live: true });
    done.members.push(dev);
    let sessions = vec![Seen { name: "claude/x/dev".into(), live: true, ..Default::default() }];
    let items = super::leftovers(std::slice::from_ref(&done), &sessions);
    assert_eq!(items.len(), 1);
    assert_eq!(
        (items[0]["team"].as_str(), items[0]["member"].as_str(), items[0]["since"].as_f64()),
        (Some("t1"), Some("dev"), Some(30.0))
    );
    // A running team's session is not a leftover; an archived team's is.
    let mut running = done.clone();
    running.status = TeamStatus::Running;
    assert!(super::leftovers(&[running], &sessions).is_empty());
    let mut archived = done.clone();
    archived.status = TeamStatus::Archived;
    assert_eq!(super::leftovers(&[archived], &sessions).len(), 1);
}

#[test]
fn a_shared_dsh_host_lists_its_members_and_marks_them() {
    let members = vec![
        ("a".to_owned(), Some(json!({"pid":7,"started":10.0}))),
        ("b".to_owned(), Some(json!({"pid":7,"started":10.0}))),
        ("c".to_owned(), Some(json!({"pid":9,"started":20.0}))),
        ("d".to_owned(), None),
    ];
    let (hosts, shared) = hosts_value(&members);
    assert_eq!(
        hosts,
        json!([
            {"agent":"dsh","pid":7,"started":10.0,"members":["a","b"]},
            {"agent":"dsh","pid":9,"started":20.0,"members":["c"]}
        ])
    );
    assert_eq!(shared, BTreeSet::from(["a".to_owned(), "b".to_owned()]));
}

#[test]
fn status_carries_each_phases_health() {
    let mut team = running_team();
    team.tasks.push(Task { id: 2, title: "Next".into(), assignee: "dev".into(), phase: 2, status: TaskStatus::Draft, ..Default::default() });
    team.phases.push(Phase { n: 2, title: "Next".into(), state: PhaseState::Draft, tasks: vec![2], ..Default::default() });
    let value = view_with(&team, &[], None);
    // C7: the plan block, the phase map and each phase's own health.
    assert_eq!(value["phases_health"]["1"]["tasks"], 1);
    assert_eq!(value["phases_health"]["2"]["tasks"], 1);
    let phase = value["phases"].as_array().unwrap().iter().find(|phase| phase["n"] == 2).unwrap();
    assert_eq!(phase["health"]["tasks"], 1);
    // Old fields stay: the plan health still names tasks and a path.
    assert_eq!(value["health"]["tasks"], 2);
    assert!(value["health"].get("critical_path").is_some());
    // Section 10.2: the workspace header rows.
    let summary = value["phase_summary"].as_array().unwrap();
    assert_eq!(summary.len(), 2);
    assert_eq!(summary[0]["n"], 1);
    assert_eq!(summary[0]["status"], "in_work");
    assert_eq!(summary[1]["status"], "draft");
}

/// Task #6 (team ede724c4): the polled list carries the counts and the phase
/// strip, never the tasks, the phase records, attention items or proposals.
#[test]
fn the_list_summary_carries_counts_and_the_phase_strip_not_the_tasks() {
    let mut team = crate::state::teams::model::Team::default();
    team.tasks.push(crate::state::teams::model::Task { id: 1, phase: 1, status: TaskStatus::Accepted, ..Default::default() });
    team.tasks.push(crate::state::teams::model::Task { id: 2, phase: 2, ..Default::default() });
    team.tasks.push(crate::state::teams::model::Task { id: 3, phase: 2, status: TaskStatus::Cancelled, ..Default::default() });
    team.phases.push(crate::state::teams::model::Phase { n: 2, title: "Fixes".into(), tasks: vec![2, 3], ..Default::default() });
    let value = super::summary(&team, true);
    for absent in ["tasks", "phases", "attention", "proposals"] {
        assert!(value.get(absent).is_none(), "{absent} is status's: {value}");
    }
    assert_eq!(value["task_count"], 3);
    // Phase 1 is done; phase 2 is in work with its cancelled task finished.
    assert_eq!(value["progress"], json!({"phase":2,"phase_done":1,"phase_total":2,"phases_done":1,"phases_total":2}));
    // Without the cancelled ones (the dashboard's default) they are not counted.
    let shown = super::summary(&team, false);
    assert_eq!(shown["task_count"], 2);
    assert_eq!(shown["progress"], json!({"phase":2,"phase_done":0,"phase_total":1,"phases_done":1,"phases_total":2}));
    assert_eq!(super::summary(&Team::default(), true)["progress"], json!({"phase":0,"phase_done":0,"phase_total":0,"phases_done":0,"phases_total":0}));
    team.phases[0].archived = Some(super::super::model::ArchivedPhase { tasks: 50, accepted: 50, ..Default::default() });
    team.archive_totals.add(team.phases[0].archived.as_ref().unwrap());
    team.tasks[1].status = TaskStatus::Accepted;
    team.tasks[1].archived = Some(Default::default());
    let archived = super::summary(&team, true);
    assert_eq!(archived["accepted"], 51);
}

#[test]
fn the_phase_commands_are_mutations_named_in_the_usage() {
    for op in ["extend", "approve-phase", "discard-phase", "finish"] {
        assert!(MUTATIONS.contains(&op), "{op} is not a mutation");
        assert!(USAGE.contains(op), "{op} is not in the usage");
    }
}

#[test]
fn status_shows_the_plan_mode_and_the_free_phases_state() {
    use super::super::model::{PlanMode, Task, TaskStatus};
    let team = Team {
        id: "t".into(),
        name: "T".into(),
        lead: "lead".into(),
        plan_mode: PlanMode::Free,
        tasks: vec![
            Task { id: 1, title: "A".into(), assignee: "dev".into(), phase: 1, status: TaskStatus::InProgress, ..Default::default() },
            Task { id: 2, title: "B".into(), assignee: "dev".into(), phase: 1, status: TaskStatus::Accepted, ..Default::default() },
        ],
        ..Default::default()
    };
    let value = view_with(&team, &[], None);
    assert_eq!(value["plan_mode"], "free");
    assert_eq!(value["free_phase"]["phase"], 1);
    assert_eq!(value["free_phase"]["state"], "open");
    assert_eq!(value["free_phase"]["running"], json!([1]));
    assert_eq!(value["free_phase"]["accepted"], 1);
    // U59: the header's count since the last build and a pending close.
    let mut with_build = team.clone();
    with_build.free_accepted_since_build = 2;
    with_build.free_close_pending = Some(1);
    let value = view_with(&with_build, &[], None);
    assert_eq!(value["free_phase"]["accepted_since"], 2);
    assert_eq!(value["free_phase"]["close_pending"], 1);
    // Standard teams carry plan_mode and no free phase.
    let standard = Team { plan_mode: PlanMode::Standard, ..team.clone() };
    let value = view_with(&standard, &[], None);
    assert_eq!(value["plan_mode"], "standard");
    assert!(value["free_phase"].is_null());
}

/// Task #58: status and the list carry each phase's mode: its own, else the
/// team's; the free phase is the one whose own mode is free.
#[test]
fn status_and_the_list_carry_each_phases_mode() {
    use super::super::model::{Phase, PhaseState, PlanMode, Task, TaskStatus};
    let mut team = Team {
        id: "t".into(),
        name: "T".into(),
        lead: "lead".into(),
        status: TeamStatus::Running,
        tasks: vec![
            Task { id: 1, title: "A".into(), assignee: "dev".into(), phase: 1, status: TaskStatus::Accepted, ..Default::default() },
            Task { id: 2, title: "B".into(), assignee: "dev".into(), phase: 2, status: TaskStatus::InProgress, ..Default::default() },
        ],
        phases: vec![Phase { n: 2, order: 2, state: PhaseState::Approved, tasks: vec![2], ..Default::default() }],
        ..Default::default()
    };
    team.phase_modes.insert(2, PlanMode::Free);
    let value = view_with(&team, &[], None);
    assert_eq!(value["plan_mode"], "standard", "the team's own mode stays the default");
    assert_eq!(value["phase_modes"], json!({"1":"standard","2":"free"}));
    assert_eq!(value["phases"][0]["mode"], "free");
    assert_eq!((value["free_phase"]["phase"].as_u64(), value["free_phase"]["running"].clone()), (Some(2), json!([2])));
    let listed = summary(&team, true);
    assert_eq!(listed["phase_modes"], json!({"1":"standard","2":"free"}));
    // An old record without phase modes reads the team's mode on every phase.
    team.phase_modes.clear();
    team.plan_mode = PlanMode::Free;
    assert_eq!(view_with(&team, &[], None)["phase_modes"], json!({"1":"free","2":"free"}));
    assert_eq!(summary(&team, true)["phase_modes"], json!({"1":"free","2":"free"}));
}

#[test]
fn status_shows_the_owners_phase_states_and_the_stuck_scope() {
    // U25: Build runs, Polish was sent back, Ship waits for the owner.
    let task = |id: u32, phase: u32, status: TaskStatus| Task { id, phase, status, title: format!("T{id}"), assignee: "lead".into(), ..Default::default() };
    let mut team = Team {
        id: "t".into(),
        name: "T".into(),
        lead: "lead".into(),
        status: TeamStatus::Running,
        tasks: vec![task(1, 1, TaskStatus::Accepted), task(2, 2, TaskStatus::Pending), task(3, 3, TaskStatus::Draft), task(4, 4, TaskStatus::Pending)],
        phases: vec![
            Phase { n: 2, title: "Build".into(), state: PhaseState::Approved, tasks: vec![2], ..Default::default() },
            Phase {
                n: 3,
                title: "Polish".into(),
                state: PhaseState::Rejected,
                tasks: vec![3],
                rejection: Some(PhaseRejection { comment: "Smaller".into(), by: "You".into(), at: 5.0, reworked_at: None }),
                ..Default::default()
            },
            // Task 158: only a barrier phase waits for the phases before it.
            Phase { n: 4, title: "Ship".into(), state: PhaseState::Approved, barrier: true, tasks: vec![4], ..Default::default() },
        ],
        ..Default::default()
    };
    let value = view_with(&team, &[], None);
    let states: Vec<&str> = value["phase_summary"].as_array().unwrap().iter().map(|phase| phase["state"].as_str().unwrap()).collect();
    assert_eq!(states, vec!["done", "running", "rejected", "queued"]);
    assert_eq!(value["phases"][1]["state"], "rejected");
    assert_eq!(value["phases"][1]["rejection"]["comment"], "Smaller");
    assert_eq!(value["phases"][2]["waiting"]["text"], "Starts when phase 2 is finished");
    // Two approved phases ahead: it names the nearer one, the one it
    // queues behind (phase 43 said "after phase 40" with 41 and 42 ahead).
    {
        let mut ahead = team.clone();
        ahead.tasks.push(task(5, 5, TaskStatus::Pending));
        ahead.phases.push(Phase { n: 5, title: "Docs".into(), state: PhaseState::Approved, barrier: true, tasks: vec![5], ..Default::default() });
        let value = view_with(&ahead, &[], None);
        assert_eq!(value["phases"][3]["waiting"]["text"], "Starts when phase 4 is finished");
    }
    // Only the running phase can make the team Stuck.
    assert_eq!(value["stuck_scope"], json!([2]));
    // A sent-back draft waits for the lead, not the owner.
    assert!(value["pending_phase"].is_null());
    assert_eq!(value["attention"], json!([]));
    // Reworked, the draft waits for the owner's Accept.
    team.phases[1].state = PhaseState::Draft;
    let value = view_with(&team, &[], None);
    assert_eq!(value["attention"][0]["kind"], "phase_draft");
    assert_eq!(value["attention"][0]["summary"], "Phase 3 \u{b7} Polish \u{2014} draft waits for your Accept");
    // Even with the report reminder off, and the team list carries it too.
    team.settings.remind_owner_minutes = 0;
    team.settings.auto_accept = true;
    let listed = summary(&team, true);
    let expected = view_with(&team, &[], None)["attention"].as_array().unwrap().len();
    assert!(expected >= 1);
    assert_eq!(listed["attention_count"], expected);
    assert_eq!((listed["stuck_scope"].clone(), listed["auto_accept"].clone()), (json!([2]), json!(true)));
    // While planning every phase is in the plan.
    team.status = TeamStatus::Planning;
    let planned = plan::phase_summary(&team);
    assert!(planned.iter().all(|phase| phase["state"] == "planned"));
}

#[test]
fn the_send_back_commands_are_mutations_that_may_start_work() {
    for op in ["reject-phase", "undo-reject-phase", "rework-phase"] {
        assert!(MUTATIONS.contains(&op), "{op} is not a mutation");
        assert!(USAGE.contains(op), "{op} is not in the usage");
    }
    // Auto-accept may take a draft on enabling or after a rework.
    assert!(starts_worker("settings") && starts_worker("rework-phase"));
    assert!(!starts_worker("reject-phase") && !starts_worker("undo-reject-phase"));
}

#[test]
fn status_shows_waiting_for_lead_and_the_pending_phase() {
    let team = Team {
        id: "t".into(),
        name: "T".into(),
        lead: "lead".into(),
        status: TeamStatus::Running,
        tasks: vec![Task { id: 1, title: "Done".into(), status: TaskStatus::Accepted, assignee: "lead".into(), ..Default::default() }],
        phases: vec![Phase { n: 2, state: PhaseState::Draft, ..Default::default() }],
        ..Default::default()
    };
    let value = view_with(&team, &[], None);
    assert_eq!(value["waiting_for_lead"], true);
    assert_eq!(value["pending_phase"], 2);
    // A paused team with every task decided is Paused, not waiting.
    let mut paused = team.clone();
    paused.status = TeamStatus::Paused;
    assert_eq!(view_with(&paused, &[], None)["waiting_for_lead"], false);
    // The session list carries both too.
    let listed = summary(&paused, true);
    assert_eq!((listed["waiting_for_lead"].as_bool(), listed["pending_phase"].as_u64()), (Some(false), Some(2)));
    // A team that is over shows no pending phase, even with a stale record.
    let mut done = team.clone();
    done.status = TeamStatus::Done;
    assert!(view_with(&done, &[], None)["pending_phase"].is_null());
    assert_eq!(pending_phase(&done), None);
}

#[test]
fn ask_reply_hold_and_release_are_mutations_and_current_is_a_read_in_the_usage() {
    for op in ["ask", "reply", "hold", "release"] {
        assert!(MUTATIONS.contains(&op), "{op} is not a mutation");
        assert!(USAGE.contains(op), "{op} is not in the usage");
    }
    assert!(USAGE.contains("current"));
    assert!(!MUTATIONS.contains(&"current"));
}

#[test]
fn a_question_shows_on_its_attempt_in_status() {
    let mut team = running_team();
    let mut events = Vec::new();
    let asked = ops::apply(
        &mut team,
        "ask",
        &json!({"text":"Must I keep the old field?"}),
        &Actor::Member("dev".into()),
        &BTreeSet::new(),
        &mut events,
    )
    .unwrap();
    assert_eq!((asked["status"].as_str(), asked["question"].as_str()), (Some("asked"), Some("q1")));
    // The attempt stays open and status shows it waiting for an answer.
    assert!(team.task(1).unwrap().open_attempt().is_some());
    let value = view_with(&team, &[], None);
    assert_eq!(value["tasks"][0]["attempts"][0]["phase"], "asking");
    assert_eq!(value["tasks"][0]["attempts"][0]["questions"][0]["text"], "Must I keep the old field?");
    assert_eq!(value["tasks"][0]["attempts"][0]["questions"][0]["answer"], "");
    // The lead's answer closes it; the attempt is still open and working.
    ops::apply(&mut team, "reply", &json!({"question":"q1","text":"Yes"}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    assert!(team.task(1).unwrap().open_attempt().is_some());
    assert_eq!(view_with(&team, &[], None)["tasks"][0]["attempts"][0]["phase"], "working");
    assert_eq!(view_with(&team, &[], None)["tasks"][0]["attempts"][0]["questions"][0]["answer"], "Yes");
}

#[test]
fn current_names_the_live_attempt_and_its_report_command() {
    let mut team = running_team();
    let mut events = Vec::new();
    ops::apply(&mut team, "ask", &json!({"text":"Must I keep?"}), &Actor::Member("dev".into()), &BTreeSet::new(), &mut events).unwrap();
    let value = current_view(&team, "dev").unwrap();
    assert_eq!((value["team"].as_str(), value["task"].as_u64(), value["n"].as_u64()), (Some("t"), Some(1), Some(1)));
    assert_eq!((value["title"].as_str(), value["phase"].as_str()), (Some("API"), Some("asking")));
    assert_eq!(value["attempt"], "00000000-0000-4000-8000-000000000001");
    assert_eq!(value["open_questions"], json!(["q1"]));
    // The report command the member fills in and pipes to hgs team report.
    let report = &value["report"];
    assert_eq!((report["team"].as_str(), report["task"].as_u64()), (Some("t"), Some(1)));
    assert_eq!(report["attempt"], "00000000-0000-4000-8000-000000000001");
    assert_eq!(report["text"], "...");
    assert_eq!(report["interim"], false);
    assert!(canonical_uuid(report["request_id"].as_str().unwrap()));
    // Every call offers a fresh request id.
    assert_ne!(current_view(&team, "dev").unwrap()["report"]["request_id"], report["request_id"]);
}

#[test]
fn current_names_a_stage_members_run_and_its_verdict_route() {
    // U36: a stage member's current attempt is the open stage run given to
    // it; the answer it fills in is a verdict, not a report.
    let mut team = running_team();
    {
        let task = team.task_mut(1).unwrap();
        task.attempts[0].state = AttemptState::Reported;
        task.attempts[0].report = Some(Report { text: "Done".into(), at: 12.0, ..Default::default() });
        task.status = TaskStatus::InReview;
        task.pipeline_run = Some(PipelineRun {
            id: "code".into(),
            name: "Code: review, test, lead".into(),
            stages: vec![PipelineStage { kind: "review".into(), role: "Tester".into(), ..Default::default() }],
            max_rounds: 3,
            folder: "hold".into(),
            accept: "lead".into(),
            round: 1,
            hold_from_report: false,
            trimmed_returns: 0,
            runs: vec![StageRun {
                id: "stage-run-1".into(),
                stage: 0,
                kind: "review".into(),
                round: 1,
                attempt: 1,
                member: "qa".into(),
                state: StageState::Delivered,
                ..Default::default()
            }],
        });
    }
    let value = current_view(&team, "qa").unwrap();
    assert_eq!(
        (value["team"].as_str(), value["task"].as_u64(), value["attempt"].as_str()),
        (Some("t"), Some(1), Some("stage-run-1"))
    );
    assert_eq!(
        (value["stage"].as_str(), value["round"].as_u64(), value["phase"].as_str()),
        (Some("review"), Some(1), Some("in_review"))
    );
    let report = &value["report"];
    assert_eq!(
        (report["attempt"].as_str(), report["verdict"].as_str()),
        (Some("stage-run-1"), Some("pass|return"))
    );
    assert_eq!(report["findings"], json!([]));
    assert!(canonical_uuid(report["request_id"].as_str().unwrap()));
    // The implementer's own attempt is not the stage member's current one.
    assert_eq!(current_view(&team, "dev").unwrap_err().to_string(), "#1 has a report in its review stage");
}

#[test]
fn current_refuses_a_member_without_an_open_attempt() {
    let mut team = running_team();
    {
        let task = team.task_mut(1).unwrap();
        task.attempts[0].state = AttemptState::Rejected;
        task.status = TaskStatus::Pending;
        task.hold = Some(Hold { by: "Lead".into(), at: 12.0, reason: "Later".into() });
    }
    assert_eq!(current_view(&team, "dev").unwrap_err().to_string(), "#1 was rejected and is held by the lead");
    // A member without a task, and the lead with none.
    assert_eq!(current_view(&team, "qa").unwrap_err().to_string(), "You have no open attempt");
    assert_eq!(current_view(&team, "lead").unwrap_err().to_string(), "You have no open attempt");
    // A reported attempt waits for the lead, not for the member.
    let mut reported = running_team();
    {
        let attempt = &mut reported.task_mut(1).unwrap().attempts[0];
        attempt.state = AttemptState::Reported;
        attempt.report = Some(Report { text: "Done".into(), at: 20.0, ..Default::default() });
    }
    reported.task_mut(1).unwrap().status = TaskStatus::Submitted;
    assert_eq!(current_view(&reported, "dev").unwrap_err().to_string(), "#1 has a report waiting for the lead's decision");
}

#[test]
fn an_attempt_id_names_a_member_away_from_its_computer() {
    let team = running_team();
    let id = team.task(1).unwrap().attempts[0].id.clone();
    // ask and current accept the attempt id where report already does (N2).
    assert_eq!(attempt_member(&team, &id).as_deref(), Some("dev"));
    assert_eq!(attempt_member(&team, "unknown"), None);
    assert_eq!(attempt_member(&team, ""), None);
    // U36: a stage run id names its member the same way.
    let mut staged = team.clone();
    staged.task_mut(1).unwrap().pipeline_run = Some(PipelineRun {
        runs: vec![StageRun { id: "stage-run-1".into(), member: "qa".into(), ..Default::default() }],
        ..Default::default()
    });
    assert_eq!(attempt_member(&staged, "stage-run-1").as_deref(), Some("qa"));
}

#[test]
fn a_recorded_stopped_session_makes_the_phase_stopped() {
    let mut team = running_team();
    assert_eq!(view_with(&team, &[], None)["tasks"][0]["attempts"][0]["phase"], "working");
    // The dispatcher saw the session stop: status derives the phase from
    // the record, without a listing of its own (L3).
    team.members.iter_mut().find(|member| member.id == "dev").unwrap().seen = Some(MemberSeen { at: 30.0, live: false });
    assert_eq!(view_with(&team, &[], None)["tasks"][0]["attempts"][0]["phase"], "stopped");
    assert_eq!(view_with(&team, &[], None)["members"][1]["queue"][0]["phase"], json!("stopped"));
    // Seen live again: working.
    team.members.iter_mut().find(|member| member.id == "dev").unwrap().seen = Some(MemberSeen { at: 40.0, live: true });
    assert_eq!(view_with(&team, &[], None)["tasks"][0]["attempts"][0]["phase"], "working");
}

#[test]
fn status_of_a_running_team_reads_no_live_listing() {
    // The desktop polls the watched team's status every 3 s; a listing
    // there is the C1 load and process churn again (L3 regression).
    for status in [TeamStatus::Planning, TeamStatus::Running, TeamStatus::Paused] {
        assert!(!status_lists_sessions(status), "{status:?}");
    }
    for status in [TeamStatus::Done, TeamStatus::Stopped] {
        assert!(status_lists_sessions(status), "{status:?}");
    }
}

#[test]
fn a_members_permission_and_answers_show_in_status() {
    let mut team = running_team();
    let member = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
    member.permission = Some(json!({"question_id":"toolu_1","tool":"Bash","command":"rm -rf build",
        "command_source":"tool_input","reason":"clean the build","asked_at":30.0}));
    // U17: an answer keeps the tool and command it was asked about; the one
    // from this attempt's window belongs to the attempt.
    member.permission_answers = vec![
        json!({"question_id":"q0","decision":"deny","by":"lead","at":25.0,"reason":"not now",
            "tool":"Bash","command":"rm -rf build"}),
        json!({"question_id":"q1","decision":"allow","by":"owner","at":5.0,"reason":"","tool":"Bash","command":"ls"}),
    ];
    let value = view_with(&team, &[], None);
    let question = &value["members"][1]["question"];
    assert_eq!(
        (question["tool"].as_str(), question["command"].as_str(), question["command_source"].as_str()),
        (Some("Bash"), Some("rm -rf build"), Some("tool_input"))
    );
    assert_eq!(value["members"][1]["permission_answers"][0]["by"], "lead");
    assert_eq!(value["members"][1]["permission_answers"][0]["decision"], "deny");
    assert_eq!(value["members"][1]["permission_answers"][0]["command"], "rm -rf build");
    // The attempt (sent at 10.0) carries the answer at 25.0, not the one at 5.0.
    let attempt = &value["tasks"][0]["attempts"][0];
    assert_eq!(attempt["permissions"].as_array().unwrap().len(), 1, "{attempt}");
    assert_eq!(attempt["permissions"][0]["command"], "rm -rf build");
    assert_eq!(attempt["permissions"][0]["at"], 25.0);
    // U37: every task carries its timing, computed by status.
    let timing = &value["tasks"][0]["timing"];
    assert!(timing["worked_s"].is_i64() && timing["waits"]["member_s"].is_i64(), "{timing}");
    // N31: the answers belong to the member the attempt ran for, not to the
    // task's current assignee after a reassignment.
    team.task_mut(1).unwrap().assignee = "qa".into();
    let value = view_with(&team, &[], None);
    assert_eq!(value["tasks"][0]["attempts"][0]["permissions"].as_array().unwrap().len(), 1, "{value}");
    // N31: a closed attempt's window ends with its report; an answer recorded
    // after it is not that attempt's any more.
    {
        let attempt = team.task_mut(1).unwrap().attempts.last_mut().unwrap();
        attempt.state = AttemptState::Reported;
        attempt.report = Some(crate::state::teams::model::Report { text: "Done".into(), at: 30.0, ..Default::default() });
    }
    team.members.iter_mut().find(|member| member.id == "dev").unwrap().permission_answers.push(json!(
        {"question_id":"q2","decision":"allow","by":"lead","at":60.0,"reason":"late","tool":"Bash","command":"npm test"}
    ));
    let value = view_with(&team, &[], None);
    let permissions = value["tasks"][0]["attempts"][0]["permissions"].as_array().unwrap();
    assert_eq!(permissions.len(), 1, "{permissions:?}");
    assert_eq!(permissions[0]["at"], 25.0);
    assert!(denies("No") && denies("Deny") && !denies("Allow"));
}

#[test]
fn a_dsh_session_is_known_by_the_conversation_its_harness_exports() {
    let mut team = running_team();
    {
        let dev = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
        dev.session = Some(SessionRef {
            name: "dsh/x/dev".into(),
            run_id: "r1".into(),
            conversation_id: "session-1".into(),
            ..Default::default()
        });
    }
    assert_eq!(dsh_member(&team, "session-1").as_deref(), Some("dev"));
    assert_eq!(dsh_member(&team, "session-2"), None);
    assert_eq!(dsh_member(&team, ""), None);
    // A member's session on another computer is not proven by this one.
    team.members.iter_mut().find(|member| member.id == "dev").unwrap().session.as_mut().unwrap().host = "msi".into();
    assert_eq!(dsh_member(&team, "session-1"), None);
}

#[test]
fn status_shows_each_member_queue() {
    let mut team = running_team();
    // Task 2 came back to dev and the lead holds it.
    team.tasks.push(Task {
        id: 2,
        title: "Retry".into(),
        assignee: "dev".into(),
        status: TaskStatus::Pending,
        hold: Some(Hold { by: "Lead".into(), at: 12.0, reason: "Later".into() }),
        attempts: vec![Attempt { n: 1, id: "A2".into(), member: "dev".into(), state: AttemptState::Rejected, ..Default::default() }],
        ..Default::default()
    });
    // Assigned but never handed out, and a finished task: neither is queued.
    team.tasks.push(Task { id: 3, title: "Later".into(), assignee: "dev".into(), status: TaskStatus::Pending, ..Default::default() });
    team.tasks.push(Task {
        id: 4,
        title: "Done".into(),
        assignee: "dev".into(),
        status: TaskStatus::Accepted,
        attempts: vec![Attempt { n: 1, id: "A4".into(), member: "dev".into(), state: AttemptState::Reported, ..Default::default() }],
        ..Default::default()
    });
    let expected = json!([{"task":1,"phase":"working"},{"task":2,"phase":"held"}]);
    let value = view_with(&team, &[], None);
    let members = value["members"].as_array().unwrap();
    assert_eq!(members.iter().find(|member| member["id"] == "dev").unwrap()["queue"], expected);
    assert_eq!(members.iter().find(|member| member["id"] == "lead").unwrap()["queue"], json!([]));
    // Task #6: the polled list leaves the queue to status.
    let listed = summary(&team, true);
    assert!(listed["members"].as_array().unwrap().iter().all(|member| member.get("queue").is_none()));
}

/// so clients need not derive the hold from inbox text.
#[test]
fn status_gives_each_member_the_task_and_stage_that_hold_its_folder() {
    use crate::state::teams::model::{PipelineRun, PipelineStage, StageRun, StageState};
    let mut team = running_team();
    let member = |value: &Value, id: &str| value["members"].as_array().unwrap().iter().find(|member| member["id"] == id).unwrap().clone();
    assert!(member(&view_with(&team, &[], None), "dev").get("held_by").is_none(), "nothing holds dev yet");
    let task = team.task_mut(1).unwrap();
    task.status = TaskStatus::InTest;
    task.attempts = vec![Attempt { n: 1, id: "A1".into(), member: "dev".into(), state: AttemptState::Reported, ..Default::default() }];
    task.pipeline_run = Some(PipelineRun {
        id: "code".into(),
        name: "Code".into(),
        stages: vec![PipelineStage { kind: "test".into(), role: "tester".into(), ..Default::default() }],
        max_rounds: 3,
        folder: "hold".into(),
        accept: "lead".into(),
        round: 1,
        runs: vec![StageRun {
            id: "s".into(),
            stage: 0,
            kind: "test".into(),
            round: 1,
            attempt: 1,
            member: "qa".into(),
            state: StageState::Delivered,
            holds: true,
            at: 1.0,
            delivered_at: Some(2.0),
            ..Default::default()
        }],
        ..Default::default()
    });
    let value = view_with(&team, &[], None);
    assert_eq!(member(&value, "dev")["held_by"], json!({"task":1,"stage":"test"}));
    assert!(member(&value, "lead").get("held_by").is_none());
}

#[test]
fn status_shows_the_captured_report_that_holds_a_member() {
    let mut team = running_team();
    let dev = |value: &Value| value["members"].as_array().unwrap().iter().find(|member| member["id"] == "dev").unwrap().clone();
    // Nothing holds dev: the field is absent, so the JSON is unchanged.
    assert!(dev(&view_with(&team, &[], None)).get("captured_hold").is_none());
    // A7: its report on #1 was captured from its last reply.
    let task = team.task_mut(1).unwrap();
    task.status = TaskStatus::Submitted;
    let attempt = task.attempts.last_mut().unwrap();
    attempt.state = AttemptState::Reported;
    attempt.report = Some(Report { text: "Queued, after FU-trees".into(), at: 20.0, captured: true, ..Default::default() });
    let expected = json!({"task":1,"text":"No new work: the captured report on #1 awaits a decision"});
    assert_eq!(dev(&view_with(&team, &[], None))["captured_hold"], expected);
    // An explicit report holds nothing.
    team.task_mut(1).unwrap().attempts.last_mut().unwrap().report.as_mut().unwrap().captured = false;
    assert!(dev(&view_with(&team, &[], None)).get("captured_hold").is_none());
}

#[test]
fn a_finished_team_lists_the_members_whose_sessions_still_run() {
    let mut team = running_team();
    team.status = TeamStatus::Done;
    let dev = team.member("dev").unwrap().session.clone().unwrap();
    let lead = team.member("lead").unwrap().session.clone().unwrap();
    // Even the lead's live session is not left running: the owner must not
    // stop a working lead from this list.
    let sessions = vec![live_session(&dev), live_session(&lead)];
    let value = view_with(&team, &[], Some(&sessions));
    assert_eq!(value["left_running"], json!([{"member":"dev","role":"Backend","session":dev}]));
    // A running team has none, and no live session is an empty list.
    team.status = TeamStatus::Running;
    assert!(view_with(&team, &[], Some(&sessions)).get("left_running").is_none());
    team.status = TeamStatus::Done;
    assert_eq!(view_with(&team, &[], Some(&[]))["left_running"], json!([]));
    // The session list never carries a live listing: only status does (C1).
    assert!(summary(&team, true).get("left_running").is_none());
}

#[test]
fn left_running_leaves_out_sessions_of_active_teams() {
    let mut done = running_team();
    done.status = TeamStatus::Done;
    let dev = done.member("dev").unwrap().session.clone().unwrap();
    let sessions = vec![live_session(&dev)];
    // The session still runs, but now leads another active team.
    let mut working = running_team();
    working.id = "working".into();
    working.name = "Working".into();
    working.members.iter_mut().find(|member| member.id == "lead").unwrap().session = Some(dev.clone());
    let value = view_with(&done, &[working.clone()], Some(&sessions));
    assert_eq!(value["left_running"], json!([]));
    // The same as a member of an active team.
    working.members.iter_mut().find(|member| member.id == "lead").unwrap().session =
        Some(SessionRef { name: "claude/x/other".into(), ..Default::default() });
    working.members.iter_mut().find(|member| member.id == "dev").unwrap().session = Some(dev.clone());
    let value = view_with(&done, &[working.clone()], Some(&sessions));
    assert_eq!(value["left_running"], json!([]));
    // A paused team keeps the session out too.
    working.status = TeamStatus::Paused;
    assert_eq!(view_with(&done, &[working.clone()], Some(&sessions))["left_running"], json!([]));
    // Once that team is over, the session is left running again.
    working.status = TeamStatus::Done;
    assert_eq!(view_with(&done, &[working], Some(&sessions))["left_running"], json!([{"member":"dev","role":"Backend","session":dev}]));
}

#[test]
fn current_ignores_finished_teams() {
    let mut done = running_team();
    done.id = "done".into();
    done.status = TeamStatus::Done;
    let mut running = running_team();
    running.id = "running".into();
    running.name = "Running".into();
    let dev = |team: &Team| team.members.iter().find(|member| member.id == "dev").map(|member| member.id.clone());
    // Only a finished team is not "a running team" any more (N3).
    let error = current_team(&[done.clone()], "", dev).unwrap_err().to_string();
    assert_eq!(error, "This session is not a member of a running team");
    // The same session in a running team is found; the finished one is not.
    let teams = [done.clone(), running.clone()];
    let (team, member) = current_team(&teams, "", dev).unwrap();
    assert_eq!((team.id.as_str(), member.as_str()), ("running", "dev"));
    assert!(current_team(&[done], "done", dev).is_err());
}

#[test]
fn status_of_a_finished_team_reads_only_active_teams() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut done = running_team();
    done.id = "done".into();
    done.status = TeamStatus::Done;
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), done, vec![]).unwrap();
    for n in 0..20u32 {
        let mut old = running_team();
        old.id = format!("old{n}");
        old.status = TeamStatus::Done;
        store.create(&format!("22222222-2222-4222-8222-{n:012}"), &json!({}), old, vec![]).unwrap();
    }
    let mut active = running_team();
    active.id = "active".into();
    active.status = TeamStatus::Paused;
    store.create("33333333-3333-4333-8333-333333333333", &json!({}), active, vec![]).unwrap();
    // The status path reads the indexed active query, not the 21 finished
    // teams (C1).
    let teams = active_team_records(&store).unwrap();
    assert_eq!(teams.iter().map(|team| team.id.as_str()).collect::<Vec<_>>(), vec!["active"]);
}

#[test]
fn the_team_list_is_stored_only() {
    let mut done = running_team();
    done.status = TeamStatus::Done;
    // A list item comes from the stored record alone: no left_running, no
    // live session listing (C1).
    let items = list_items(&[done], true);
    assert_eq!(items["items"].as_array().unwrap().len(), 1);
    assert!(items["items"][0].get("left_running").is_none());
    // Task #6: a listed member is who it is, its state and its session.
    let member = items["items"][0]["members"][0].as_object().unwrap();
    assert_eq!(member.keys().map(String::as_str).collect::<BTreeSet<_>>(), BTreeSet::from(["id", "role", "agent", "state", "session", "session_state", "state_reason"]));
}

#[test]
fn the_inbox_and_status_name_the_leads_next_actions() {
    let mut team = running_team();
    // The report waits for a decision: one inbox item, one wait.
    {
        let attempt = &mut team.task_mut(1).unwrap().attempts[0];
        attempt.state = AttemptState::Reported;
        attempt.report = Some(Report { text: "Done and checked".into(), at: 10.0, ..Default::default() });
    }
    team.task_mut(1).unwrap().status = TaskStatus::Submitted;
    let inbox = inbox_view_for(&team, &team.lead);
    assert_eq!(inbox["items"][0]["kind"], "report");
    assert_eq!((inbox["items"][0]["task"].as_u64(), inbox["items"][0]["member"].as_str()), (Some(1), Some("dev (Backend)")));
    assert!(inbox["items"][0]["command"].as_str().unwrap().contains("hgs team accept --json"));
    let value = view_with(&team, &[], None);
    assert_eq!(value["tasks"][0]["waits"], json!({"for":"lead","on":["Lead"],"unblock":"accept #1"}));
    // An old report shows on the owner's attention list; 0 turns it off.
    let mut old = team.clone();
    old.task_mut(1).unwrap().attempts[0].report.as_mut().unwrap().at = super::super::super::now() - 61.0 * 60.0;
    let value = view_with(&old, &[], None);
    assert_eq!(value["attention"][0]["kind"], "report");
    assert_eq!(value["attention"][0]["task"], 1);
    // The polled team list counts the same attention items.
    assert_eq!(summary(&old, true)["attention_count"], value["attention"].as_array().unwrap().len());
    old.settings.remind_owner_minutes = 0;
    assert_eq!(view_with(&old, &[], None)["attention"], json!([]));
    // The settings are optional with the spec's defaults (C2, C4, U18).
    assert_eq!(
        (
            team.settings.inbox_interval_seconds,
            team.settings.remind_lead_minutes,
            team.settings.remind_owner_minutes,
            team.settings.remind_owner_permission_minutes
        ),
        (120, 15, 60, 10)
    );
    // The inbox is a read, named in the usage.
    assert!(USAGE.contains("inbox"));
    assert!(!MUTATIONS.contains(&"inbox"));
}

#[test]
fn the_status_lists_every_check_command() {
    let mut team = running_team();
    team.tasks.push(Task {
        id: 7,
        title: "Build".into(),
        assignee: "dev".into(),
        kind: TaskKind::Check,
        check: Some(Check { command: "cargo test".into(), expect_exit: 0, timeout_minutes: 15 }),
        ..Default::default()
    });
    let value = view_with(&team, &[], None);
    assert_eq!(
        value["checks"],
        json!([{"task":7,"title":"Build","command":"cargo test","expect_exit":0,"timeout_minutes":15}])
    );
    // A plan without checks says so with an empty list.
    assert_eq!(view_with(&running_team(), &[], None)["checks"], json!([]));
}

#[test]
fn a_parent_task_shows_its_subteam_and_the_team_lists_its_children() {
    let parent = running_team();
    let mut child = Team { id: "child".into(), name: "Child".into(), lead: "dev".into(), status: TeamStatus::Running, ..Default::default() };
    child.parent = Some(Parent { team: parent.id.clone(), task: 1, lead: "lead".into(), member: "dev".into() });
    child.members.push(Member { id: "dev".into(), role: "Backend".into(), agent: "codex".into(), ..Default::default() });
    child.tasks.push(Task { id: 1, title: "Sub work".into(), assignee: "dev".into(), status: TaskStatus::Accepted, ..Default::default() });
    child.tasks.push(Task { id: 2, title: "More".into(), assignee: "dev".into(), ..Default::default() });
    let view = view_with(&parent, &[parent.clone(), child.clone()], None);
    // Section 9.4: the delegated task shows its sub-team and counts.
    let task = view["tasks"].as_array().unwrap().iter().find(|task| task["id"] == 1).unwrap();
    assert_eq!(task["subteam"], json!({"team":"child","status":"running","done":1,"total":2}));
    assert_eq!(view["children"], json!([{"team":"child","name":"Child","status":"running","done":1,"total":2}]));
    // The parent's plan health counts only its own tasks.
    assert_eq!(view["health"]["tasks"], 1);
    // Task 52: the list names each team's parent task, so a client walks the
    // tree from the list; a team with no parent says null.
    assert_eq!(summary(&child, true)["parent"], json!({"team":parent.id,"task":1}));
    assert_eq!(summary(&parent, true)["parent"], json!(null));
}

#[test]
fn a_flat_team_tree_check_reads_no_teams_and_a_child_reads_by_id() {
    let flat = running_team();
    assert!(flat.parent.is_none() && flat.children.is_empty());
    let mut reads = 0;
    let loaded = tree_around(&flat, &mut |_| {
        reads += 1;
        Ok(None)
    })
    .unwrap();
    assert_eq!(reads, 0, "a flat team reads nothing extra");
    assert_eq!(loaded.len(), 1);
    // A parent with one child reads that child by id, never the full list.
    let mut parent = flat.clone();
    parent.children = vec!["child".into()];
    let mut child = Team { id: "child".into(), name: "Child".into(), lead: "dev".into(), ..Default::default() };
    child.parent = Some(Parent { team: parent.id.clone(), task: 1, lead: "lead".into(), member: "dev".into() });
    let mut reads = 0;
    let loaded = tree_around(&parent, &mut |id| {
        reads += 1;
        Ok((id == "child").then(|| child.clone()))
    })
    .unwrap();
    assert_eq!(reads, 1);
    assert_eq!(loaded.len(), 2);
}

#[test]
fn status_shows_the_budget_state_on_the_member() {
    let mut team = running_team();
    let now = super::super::super::now();
    let since = now - 40.0 * 60.0;
    team.members.iter_mut().find(|member| member.id == "dev").unwrap().budget = Some(Budget {
        label: "install".into(),
        minutes: 15,
        red_minutes: 30,
        declared_at: since,
        job: Some(JobRef { command: "npm install".into(), since }),
        ..Default::default()
    });
    let value = view_with(&team, &[], None);
    let dev = value["members"].as_array().unwrap().iter().find(|member| member["id"] == "dev").unwrap();
    assert_eq!(dev["budget"]["state"], "red");
    assert_eq!((dev["budget"]["label"].as_str(), dev["budget"]["elapsed_minutes"].as_u64()), (Some("install"), Some(40)));
    assert_eq!(dev["budget"]["command"], "npm install");
    // Amber between the estimate and the red time.
    team.members.iter_mut().find(|member| member.id == "dev").unwrap().budget.as_mut().unwrap().job.as_mut().unwrap().since =
        now - 20.0 * 60.0;
    let value = view_with(&team, &[], None);
    let dev = value["members"].as_array().unwrap().iter().find(|member| member["id"] == "dev").unwrap();
    assert_eq!(dev["budget"]["state"], "amber");
}

/// U56: status shows each member's phase, activity and minutes without
/// progress, and whether it is stuck, from the dispatcher's last view.
#[test]
fn status_shows_each_members_phase_activity_and_minutes_without_progress() {
    use super::super::model::{MemberSeen, MemberStuck};
    let mut team = running_team();
    let dev = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
    dev.seen = Some(MemberSeen { at: crate::state::now() - 25.0 * 60.0, live: true });
    dev.seen_phase = "tool".into();
    dev.seen_activity = "busy".into();
    dev.seen_event_at = Some(crate::state::now() - 25.0 * 60.0);
    dev.stuck = Some(MemberStuck { at: crate::state::now() - 15.0 * 60.0, reason: "no_progress".into(), phase: "tool".into(), ..Default::default() });
    let value = view_with(&team, &[], None);
    let dev = value["members"].as_array().unwrap().iter().find(|member| member["id"] == "dev").unwrap();
    assert_eq!((dev["phase"].as_str(), dev["activity"].as_str()), (Some("tool"), Some("busy")));
    assert_eq!(dev["minutes_without_progress"], 25);
    assert_eq!(dev["stuck"]["reason"], "no_progress");
    // Task 115: a busy tool reads Working in the same contract.
    assert_eq!(dev["session_state"], "working");
}

/// Task 209: status carries the member's native context reading, and reads its
/// state `compacting` while a compaction is being confirmed.
#[test]
fn status_shows_a_members_context_and_compacting_state() {
    use super::super::model::{MemberCompact, MemberContext};
    let mut team = running_team();
    {
        let dev = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
        dev.context = Some(MemberContext::of(960_000, 1_000_000, 1000.0));
    }
    let value = view_with(&team, &[], None);
    let dev = value["members"].as_array().unwrap().iter().find(|member| member["id"] == "dev").unwrap();
    assert_eq!((dev["context"]["used"].as_u64(), dev["context"]["limit"].as_u64()), (Some(960_000), Some(1_000_000)));
    assert_eq!(dev["context"]["percent"], 96);
    assert!(dev.get("compacting").is_none(), "no compaction: today's member shape");
    {
        let dev = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
        dev.context_compact = Some(MemberCompact {
            action: "compact".into(),
            started_at: 1000.0,
            task: 1,
            reason: "threshold".into(),
            ..Default::default()
        });
    }
    let value = view_with(&team, &[], None);
    let dev = value["members"].as_array().unwrap().iter().find(|member| member["id"] == "dev").unwrap();
    assert_eq!(dev["state"], "compacting");
    assert_eq!(dev["compacting"]["action"], "compact");
    assert_eq!(dev["compacting"]["reason"], "threshold");
    assert_eq!(dev["member_state"], "active", "the stored roster state stays beside it");
}

/// Task 115: the Adventure team's Integrator (Kimi) sat on a native Bash
/// permission while the last tick still stored it busy/working, so the lead
/// read "active" from `hgs team status`. The member entry now carries the
/// session state contract's own state, and the session list's team row too.
#[test]
fn a_member_with_an_open_approval_reads_waiting_for_you_in_status() {
    use crate::state::teams::model::{Member, MemberSeen, PendingQuestion, Team, TeamStatus};
    let mut team = Team {
        id: "abcdef12-0000-4000-8000-000000000000".into(),
        name: "Adventure".into(),
        lead: "lead".into(),
        status: TeamStatus::Running,
        ..Default::default()
    };
    team.members.push(Member { id: "lead".into(), role: "Lead".into(), agent: "claude".into(), ..Default::default() });
    let mut integrator = Member { id: "integrator".into(), role: "Integrator".into(), agent: "kimi".into(), ..Default::default() };
    integrator.seen = Some(MemberSeen { at: crate::state::now() - 120.0, live: true });
    integrator.seen_phase = "working".into();
    integrator.seen_activity = "busy".into();
    integrator.seen_event_at = Some(crate::state::now() - 120.0);
    integrator.pending = Some(PendingQuestion {
        question_id: "q1".into(),
        question_hash: "h1".into(),
        approval: true,
        at: crate::state::now(),
        ..Default::default()
    });
    team.members.push(integrator);
    let value = view_with(&team, &[], None);
    let member = value["members"].as_array().unwrap().iter().find(|member| member["id"] == "integrator").unwrap();
    assert_eq!(member["session_state"], "user", "{member}");
    assert_eq!(member["state_reason"]["kind"], "approval", "{member}");
    assert_eq!(member["state_reason"]["text"], "Needs approval", "{member}");
    // The session list's own team row carries the same state.
    let listed = summary(&team, true);
    let row = listed["members"].as_array().unwrap().iter().find(|member| member["id"] == "integrator").unwrap();
    assert_eq!((row["session_state"].clone(), row["state_reason"]["kind"].clone()), (json!("user"), json!("approval")));
    // Answered, the stored phase decides again: it was working, not waiting.
    let mut answered = team.clone();
    answered.members.iter_mut().find(|member| member.id == "integrator").unwrap().pending = None;
    let value = view_with(&answered, &[], None);
    let member = value["members"].as_array().unwrap().iter().find(|member| member["id"] == "integrator").unwrap();
    assert_eq!(member["session_state"], "working", "{member}");
    assert!(member["state_reason"].is_null(), "{member}");
}

/// Task #6 (team ede724c4): the polled list's size and time on a copy of a real
/// store, as the desktop asks for it (cancelled tasks included). It runs only
/// when HGS_TEST_STORE_COPY names the copy's teams.sqlite3; HGS_TEST_LIST_OUT,
/// when set, receives the answer for comparing the desktop's decoding.
#[test]
fn measure_the_list_on_a_store_copy() {
    let Some(path) = std::env::var_os("HGS_TEST_STORE_COPY") else { return };
    let store = Store::open_at(std::path::Path::new(&path), false).unwrap();
    let mut best = f64::MAX;
    let mut answer = Value::Null;
    for _ in 0..5 {
        let started = std::time::Instant::now();
        let teams = store.teams(false, 200).unwrap();
        answer = list_items(&teams, true);
        let text = serde_json::to_string(&answer).unwrap();
        best = best.min(started.elapsed().as_secs_f64() * 1000.0);
        assert!(!text.is_empty());
    }
    let text = serde_json::to_string(&answer).unwrap();
    let mut sizes: Vec<(usize, String)> = answer["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| (serde_json::to_string(item).unwrap().len(), item["name"].as_str().unwrap_or_default().to_owned()))
        .collect();
    sizes.sort_by_key(|size| std::cmp::Reverse(size.0));
    println!("list teams={} bytes={} min_ms={best:.1} largest={:?}", sizes.len(), text.len(), &sizes[..sizes.len().min(3)]);
    if let Some(out) = std::env::var_os("HGS_TEST_LIST_OUT") {
        std::fs::write(out, &text).unwrap();
    }
}

/// Task #6 (team ede724c4): `hgs team inbox` time on a copy of a real state
/// folder, the min of 5: the owner's read of a team (as the desktop asks) and
/// the calling member's own (`peek`, so nothing is marked). It runs only when
/// HGS_TEST_STATE_COPY names the copy's state folder (teams/teams.sqlite3 and
/// the calling session's record) and HGS_TEST_INBOX_TEAM the team.
#[test]
fn measure_the_inbox_on_a_store_copy() {
    let (Some(root), Ok(team)) = (std::env::var_os("HGS_TEST_STATE_COPY"), std::env::var("HGS_TEST_INBOX_TEAM")) else { return };
    let markers: Vec<(&str, Option<std::ffi::OsString>)> =
        ["HGS_SESSION", "HGS_RUN_ID", "DSH_SESSION_ID"].into_iter().map(|name| (name, std::env::var_os(name))).collect();
    let best = |input: &Value| {
        let mut best = f64::MAX;
        let mut answer = Value::Null;
        for _ in 0..5 {
            let started = std::time::Instant::now();
            answer = inbox_command(input).unwrap();
            best = best.min(started.elapsed().as_secs_f64() * 1000.0);
        }
        (best, serde_json::to_string(&answer).unwrap().len(), answer["for"].clone())
    };
    crate::state::with_test_state_root(std::path::Path::new(&root), || {
        for (name, _) in &markers {
            std::env::remove_var(name);
        }
        let (owner_ms, owner_bytes, owner_for) = best(&json!({"team":team}));
        println!("inbox owner team={team} min_ms={owner_ms:.1} bytes={owner_bytes} for={owner_for}");
        for (name, value) in &markers {
            if let Some(value) = value {
                std::env::set_var(name, value);
            }
        }
        set_test_caller_identities(Some((None, std::env::var("HGS_SESSION").ok(), std::env::var("HGS_RUN_ID").ok())));
        let (member_ms, member_bytes, member_for) = best(&json!({"peek":true}));
        println!("inbox member min_ms={member_ms:.1} bytes={member_bytes} for={member_for}");
    });
}
