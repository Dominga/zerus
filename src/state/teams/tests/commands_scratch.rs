use super::*;
use crate::state::teams::model::{Attempt, AttemptState};
use crate::state::teams::{owner_steps, scratch, templates};

/// A running team with a folder, a lead, a QA member (the operate profile with
/// a dsh conversation) and a dev, with task #1 open for the QA member.
fn parent(project: &std::path::Path) -> Team {
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
    team.members.push(Member { id: "qa".into(), role: "QA".into(), agent: "dsh".into(), permissions_profile: "operate".into(), session: Some(qa), ..Default::default() });
    team.members.push(Member { id: "dev".into(), role: "Backend".into(), agent: "dsh".into(), session: Some(dev), ..Default::default() });
    team.tasks.push(Task { id: 1, title: "Live check".into(), assignee: "qa".into(), status: TaskStatus::InProgress, ..Default::default() });
    team
}

#[test]
fn a_qa_member_creates_a_scratch_team_and_others_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        templates::settings_override(Some(dir.path().join("templates.json")));
        set_test_caller_identities(Some((None, None, None)));
        let project = dir.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let team = parent(&project);
        Store::open(true).unwrap().create(&uuid::Uuid::new_v4().to_string(), &json!({}), team.clone(), vec![]).unwrap();
        // The owner grants the team's one standing rule once (Settings > Teams,
        // or a card whose folder is the team's root).
        let grant = json!({"request_id": uuid::Uuid::new_v4().to_string(), "team": "parent", "by": "human",
            "standing": true, "agents": ["dsh"], "reason": "live checks"});
        let first = scratch_rule(&grant).unwrap();
        let mut second = grant.clone();
        second["request_id"] = json!(uuid::Uuid::new_v4().to_string());
        let again = scratch_rule(&second).unwrap();
        assert_eq!(first["rule"]["id"], again["rule"]["id"], "the same grant is kept once");
        assert_eq!(owner_steps::rules().len(), 1, "one standing rule per team");
        assert_eq!(first["folder"], json!(scratch::root(&team)));
        // A card that names the root without "standing" grants the same rule.
        let card = json!({"request_id": uuid::Uuid::new_v4().to_string(), "team": "parent", "by": "human",
            "folder": scratch::root(&team), "agents": ["dsh"], "reason": "live checks"});
        let from_card = scratch_rule(&card).unwrap();
        assert_eq!(from_card["standing"], json!(true));
        assert_eq!(from_card["rule"]["id"], first["rule"]["id"]);

        // The QA member creates a scratch team from its own session inside the root.
        set_test_caller_identities(Some((Some("qa-conversation".into()), None, None)));
        let receipt = create(&json!({
            "request_id": uuid::Uuid::new_v4().to_string(),
            "name": "Live check",
            "folder": format!("{}/check", scratch::root(&team)),
            "members": [{"id":"probe","role":"Probe","agent":"dsh"}],
        }))
        .unwrap();
        let child = Store::open(false).unwrap().load(receipt["team"].as_str().unwrap()).unwrap().unwrap();
        assert_eq!(child.lead, "qa", "the QA member leads its scratch team");
        assert_eq!(child.scratch_parent.as_ref().map(|link| (link.team.as_str(), link.task, link.member.as_str())), Some(("parent", 1, "qa")));
        assert_eq!(child.settings.approval, Approval::Auto, "the standing rule approves the plan");
        assert_eq!(child.scratch_rule, owner_steps::standing("parent", &scratch::root(&team)).unwrap().id);
        let stored = Store::open(false).unwrap().load("parent").unwrap().unwrap();
        assert!(stored.scratch_teams.iter().any(|record| record.id == child.id && record.task == 1), "the lead sees it");

        // Outside the root is refused.
        let outside = dir.path().join("elsewhere");
        let error = create(&json!({
            "request_id": uuid::Uuid::new_v4().to_string(),
            "name": "Outside",
            "folder": outside.to_string_lossy(),
            "members": [{"id":"probe","role":"Probe","agent":"dsh"}],
        }))
        .unwrap_err()
        .to_string();
        assert!(error.contains("scratch root"), "{error}");

        // A member without the QA right is refused, even inside the root.
        set_test_caller_identities(Some((Some("dev-conversation".into()), None, None)));
        let error = create(&json!({
            "request_id": uuid::Uuid::new_v4().to_string(),
            "name": "No",
            "folder": format!("{}/check-2", scratch::root(&team)),
        }))
        .unwrap_err()
        .to_string();
        assert!(error.contains("QA right"), "{error}");

        // The creating task's decision stops the scratch team (cleanup on decide).
        Store::open(true).unwrap().mutate(None, "parent", None, |team, events| {
            let task = team.task_mut(1).unwrap();
            task.status = TaskStatus::Submitted;
            task.attempts.push(Attempt { n: 1, id: "00000000-0000-4000-8000-000000000001".into(), member: "qa".into(), state: AttemptState::Reported, ..Default::default() });
            events.push(Event::new("reported", json!({"task": 1})));
            Ok(Value::Null)
        })
        .unwrap();
        set_test_caller_identities(Some((None, None, None)));
        run("accept", &json!({"request_id": uuid::Uuid::new_v4().to_string(), "team": "parent", "task": 1, "by": "human"}), false).unwrap();
        let parent = Store::open(false).unwrap().load("parent").unwrap().unwrap();
        assert!(parent.scratch_teams.iter().any(|record| record.id == child.id && record.state == "stopped"), "the decision stops it");
        let child = Store::open(false).unwrap().load(&child.id).unwrap().unwrap();
        assert_eq!(child.status, TeamStatus::Stopped);

        set_test_caller_identities(None);
        templates::settings_override(None);
    });
}
