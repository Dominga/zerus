// Task #346 (owner, 2026-11-08): a sandboxed member's own-scope team calls wait
// in its folder and the team worker applies them: the scratch team's create
// under the owner's standing rule, and its calls for the scratch team it leads
// (plan, approve, relaunch, ...). A call outside that scope keeps the sandbox's
// error, so only it asks the lead.
use super::*;
use super::support::*;
use crate::state::teams::model::Member;
use crate::state::teams::{outbox, owner_steps, scratch, templates};

/// A running team whose QA member carries the QA right (operate), with task #1
/// open, the standing scratch rule granted, and every member in its own folder.
fn parent(project: &std::path::Path, qa_folder: &std::path::Path) -> Team {
    let lead = SessionRef { name: "dsh/x/lead".into(), run_id: "r0".into(), conversation_id: "lead-conversation".into(), ..Default::default() };
    let qa = SessionRef { name: "dsh/x/qa".into(), run_id: "r1".into(), conversation_id: "qa-conversation".into(), ..Default::default() };
    let dev = SessionRef { name: "dsh/x/dev".into(), run_id: "r2".into(), conversation_id: "dev-conversation".into(), ..Default::default() };
    let mut team = Team {
        id: "parent".into(),
        name: "Core".into(),
        folder: project.to_string_lossy().into_owned(),
        lead: "lead".into(),
        status: TeamStatus::Running,
        ..Default::default()
    };
    team.members.push(Member { id: "lead".into(), role: "Lead".into(), agent: "dsh".into(), session: Some(lead), ..Default::default() });
    team.members.push(Member {
        id: "qa".into(),
        role: "QA".into(),
        agent: "dsh".into(),
        permissions_profile: "operate".into(),
        session: Some(qa),
        directory: qa_folder.to_string_lossy().into_owned(),
        ..Default::default()
    });
    team.members.push(Member {
        id: "dev".into(),
        role: "Backend".into(),
        agent: "dsh".into(),
        session: Some(dev),
        directory: qa_folder.to_string_lossy().into_owned(),
        ..Default::default()
    });
    team.tasks.push(Task { id: 1, title: "Live check".into(), assignee: "qa".into(), status: TaskStatus::InProgress, ..Default::default() });
    team
}

#[test]
fn a_sandboxed_member_queues_its_scratch_calls_and_the_worker_applies_them() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let _worker = hold_worker_lock();
        templates::settings_override(Some(dir.path().join("templates.json")));
        set_test_caller_identities(Some((Some("qa-conversation".into()), None, None)));
        let project = dir.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let qa_folder = dir.path().join("qa");
        std::fs::create_dir_all(&qa_folder).unwrap();
        let mut store = Store::open(true).unwrap();
        let team = parent(&project, &qa_folder);
        // The owner grants the team's one standing rule once.
        owner_steps::add_standing(owner_steps::ScratchRule {
            team: team.id.clone(),
            folder: scratch::root(&team),
            agents: vec!["dsh".into()],
            ..Default::default()
        })
        .unwrap();
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), team.clone(), vec![]).unwrap();
        let qa = team.member("qa").unwrap().clone();
        let folder = member_outbox_folder(&team, &qa).unwrap();
        // The create of its scratch team inside the root queues, not errors.
        let create_input = json!({"request_id":uuid::Uuid::new_v4().to_string(),"name":"Live check",
            "folder":format!("{}/check", scratch::root(&team)),
            "members":[{"id":"probe","role":"Probe","agent":"dsh"}]});
        let queued = queue_team_call("create", &create_input, false).unwrap().expect("its own scratch create");
        assert_eq!(queued["status"], json!("queued"));
        let requests = outbox::pending(&folder, 10).unwrap();
        assert_eq!(requests.len(), 1);
        let request = requests[0].1.clone();
        assert_eq!((request["op"].as_str(), request["command"].as_str()), (Some("team"), Some("create")));
        // The worker applies it with the member's identity: the child exists and
        // the parent lead sees it.
        let value = apply_team_outbox(&mut store, &team, &qa, &request).unwrap();
        let child_id = value["team"].as_str().unwrap().to_owned();
        let child = store.load(&child_id).unwrap().unwrap();
        assert_eq!(child.lead, "qa");
        assert_eq!(child.scratch_parent.as_ref().map(|link| (link.team.as_str(), link.task, link.member.as_str())), Some(("parent", 1, "qa")));
        let stored = store.load(&team.id).unwrap().unwrap();
        assert!(stored.scratch_teams.iter().any(|record| record.id == child_id), "the lead sees it");
        // A plan of that scratch team queues too and the worker plans it.
        let plan = json!({"request_id":uuid::Uuid::new_v4().to_string(),"team":child_id,
            "tasks":[{"id":1,"title":"Check the page","assignee":"probe"}]});
        let queued = queue_team_call("plan", &plan, false).unwrap().expect("its own scratch plan");
        assert_eq!(queued["status"], json!("queued"));
        let request = outbox::pending(&folder, 10).unwrap().into_iter().map(|(_, request)| request)
            .find(|request| request["command"] == "plan").expect("the plan request");
        let stored = store.load(&team.id).unwrap().unwrap();
        let qa = stored.member("qa").unwrap().clone();
        apply_team_outbox(&mut store, &stored, &qa, &request).unwrap();
        let child = store.load(&child_id).unwrap().unwrap();
        assert_eq!(child.tasks.iter().map(|task| task.id).collect::<Vec<_>>(), vec![1]);
        assert_eq!(child.tasks[0].title, "Check the page");
        // Outside the scope: another team's call, a non-QA member, a folder
        // outside the root. Each keeps the sandbox's error (None here).
        assert!(queue_team_call("plan", &json!({"team":"parent","tasks":[]}), false).unwrap().is_none(), "another team");
        set_test_caller_identities(Some((Some("dev-conversation".into()), None, None)));
        assert!(queue_team_call("plan", &plan, false).unwrap().is_none(), "no QA right");
        set_test_caller_identities(Some((Some("qa-conversation".into()), None, None)));
        let outside = json!({"request_id":uuid::Uuid::new_v4().to_string(),"name":"Outside",
            "folder":dir.path().join("elsewhere").to_string_lossy()});
        assert!(queue_team_call("create", &outside, false).unwrap().is_none(), "outside the root");
        // The queued requests in the folder are only the two in-scope calls.
        assert_eq!(outbox::pending(&folder, 10).unwrap().len(), 2);
        set_test_caller_identities(None);
        templates::settings_override(None);
    });
}
