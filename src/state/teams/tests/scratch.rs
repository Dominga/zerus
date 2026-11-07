use super::*;
use crate::state::teams::commands;
use crate::state::teams::model::{
    Attempt, AttemptState, MemberFolder, ScratchChild, ScratchParent, SessionRef, Task, TaskStatus,
};
use crate::state::teams::{ops, store::Store, templates};

fn work() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

/// A running team with a folder, a lead, a QA member (the operate profile) and
/// a dev, with task #1 open for the QA member (the creating task).
fn team(folder: &str) -> Team {
    let session = SessionRef { name: "dsh/x/lead".into(), run_id: "r0".into(), conversation_id: "lead".into(), ..Default::default() };
    let qa = SessionRef { name: "kimi/x/qa".into(), run_id: "r1".into(), conversation_id: "qa".into(), ..Default::default() };
    let mut team = Team {
        id: "parent".into(),
        name: "Core".into(),
        folder: folder.into(),
        lead: "lead".into(),
        status: TeamStatus::Running,
        ..Default::default()
    };
    team.settings.max_members = 4;
    team.members.push(super::super::model::Member { id: "lead".into(), role: "Lead".into(), agent: "dsh".into(), session: Some(session), ..Default::default() });
    team.members.push(super::super::model::Member {
        id: "qa".into(),
        role: "QA".into(),
        agent: "kimi".into(),
        permissions_profile: "operate".into(),
        session: Some(qa),
        ..Default::default()
    });
    team.members.push(super::super::model::Member { id: "dev".into(), role: "Backend".into(), agent: "codex".into(), ..Default::default() });
    team.tasks.push(Task { id: 1, title: "Live check".into(), assignee: "qa".into(), status: TaskStatus::InProgress, ..Default::default() });
    team
}

fn grant(team: &Team) {
    let rule = owner_steps::rule_from(&json!({"folder":root(team),"agents":["kimi","codex"]})).unwrap();
    let mut rule = rule;
    rule.team = team.id.clone();
    owner_steps::add_standing(rule).unwrap();
}

#[test]
fn a_qa_member_prepares_a_scratch_team_at_the_team_root() {
    let dir = work();
    templates::settings_override(Some(dir.path().join("templates.json")));
    let project = dir.path().join("project");
    let team = team(&project.to_string_lossy());
    grant(&team);
    let qa = team.member("qa").unwrap();
    let inside = format!("{}/check-1", root(&team));
    let filled = prepare(&team, qa, &json!({
        "name": "Live check",
        "folder": inside,
        "members": [{"id":"probe","role":"Backend","agent":"codex"}],
        "settings": {"max_members": 9},
    }))
    .unwrap();
    // The QA member's own session leads the scratch team.
    assert_eq!(filled["lead"]["id"], json!("qa"));
    assert_eq!(filled["lead"]["session"]["name"], json!("kimi/x/qa"));
    // The other member works in the scratch folder, which the standing rule trusts.
    assert_eq!(filled["members"][0]["folder"], json!("shared"));
    // The plan needs no card, and the parent's caps hold.
    assert_eq!(filled["settings"]["approval"], json!("auto"));
    assert_eq!(filled["settings"]["phase_approval"], json!("auto"));
    assert_eq!(filled["settings"]["max_members"], json!(4));
    // The creating task and the rule are recorded for the parent lead and cleanup.
    assert_eq!(filled["scratch_parent"]["team"], json!("parent"));
    assert_eq!(filled["scratch_parent"]["task"], json!(1));
    assert_eq!(filled["scratch_parent"]["member"], json!("qa"));
    assert_eq!(filled["scratch_rule"], json!(standing(&team).unwrap().id));
    // The team is created from it: approval auto, the scratch link and rule set.
    let created = ops::create("scratch-1", &filled).unwrap().0;
    assert_eq!(created.settings.approval, super::super::model::Approval::Auto);
    assert_eq!(created.scratch_parent, Some(ScratchParent { team: "parent".into(), task: 1, member: "qa".into() }));
    assert_eq!(created.scratch_rule, standing(&team).unwrap().id);
    assert_eq!(created.lead, "qa");
    assert!(created.members.iter().any(|member| member.id == "probe" && member.folder == MemberFolder::Shared));
    templates::settings_override(None);
}

#[test]
fn a_member_without_the_qa_right_and_a_folder_outside_the_root_are_refused() {
    let dir = work();
    templates::settings_override(Some(dir.path().join("templates.json")));
    let project = dir.path().join("project");
    let mut team = team(&project.to_string_lossy());
    let qa = team.member("qa").unwrap().clone();
    // Before the owner grants the standing rule, even a QA member is refused.
    let error = authorize(&team, &qa, &format!("{}/check", root(&team))).unwrap_err().to_string();
    assert!(error.contains("grants the team's scratch rule"), "{error}");
    grant(&team);
    // A member without the QA right (write, not operate) never creates one.
    let dev = team.member("dev").unwrap().clone();
    let error = authorize(&team, &dev, &format!("{}/check", root(&team))).unwrap_err().to_string();
    assert!(error.contains("QA right"), "{error}");
    // Outside the root (a sibling with the same prefix is not inside) is refused.
    for outside in [dir.path().join("elsewhere"), project.join(".qa-scratch-2")] {
        let error = authorize(&team, &qa, &outside.to_string_lossy()).unwrap_err().to_string();
        assert!(error.contains("scratch root"), "{outside:?}: {error}");
    }
    // The root follows the owner's setting; empty is the team folder's default.
    team.settings.scratch_root = project.to_string_lossy().into_owned();
    assert_eq!(root(&team), project.to_string_lossy().replace('\\', "/"));
    team.settings.scratch_root.clear();
    assert!(root(&team).ends_with(".qa-scratch"));
    templates::settings_override(None);
}

#[test]
fn the_parent_lead_reads_its_scratch_teams_in_the_inbox() {
    let mut team = team("D:/work/core");
    team.scratch_teams.push(ScratchChild {
        id: "s1".into(),
        name: "Live check".into(),
        task: 1,
        member: "qa".into(),
        at: crate::state::now(),
        state: "created".into(),
        stopped_at: None,
    });
    let items = super::super::inbox::items(&team);
    assert!(
        items.iter().any(|item| item.kind == "scratch_team" && item.summary.contains("Live check")),
        "{:?}",
        items.iter().map(|item| item.kind).collect::<Vec<_>>()
    );
}

#[test]
fn the_creating_tasks_decision_stops_the_scratch_team_and_its_sessions() {
    let dir = work();
    crate::state::with_test_state_root(dir.path(), || {
        commands::record_lead_stops(true);
        let mut store = Store::open(true).unwrap();
        let qa_session = SessionRef { name: "kimi/x/qa".into(), run_id: "r1".into(), conversation_id: "qa".into(), ..Default::default() };
        let probe = SessionRef { name: "codex/x/probe".into(), run_id: "r2".into(), conversation_id: "probe".into(), ..Default::default() };
        let mut parent = team("D:/work/core");
        parent.tasks[0].status = TaskStatus::Accepted;
        parent.scratch_teams.push(ScratchChild {
            id: "scratch-1".into(),
            name: "Live check".into(),
            task: 1,
            member: "qa".into(),
            at: 100.0,
            state: "created".into(),
            stopped_at: None,
        });
        let mut child = Team {
            id: "scratch-1".into(),
            name: "Live check".into(),
            lead: "qa".into(),
            status: TeamStatus::Running,
            scratch_parent: Some(ScratchParent { team: "parent".into(), task: 1, member: "qa".into() }),
            ..Default::default()
        };
        child.members.push(super::super::model::Member { id: "qa".into(), role: "QA".into(), agent: "kimi".into(), session: Some(qa_session), ..Default::default() });
        child.members.push(super::super::model::Member { id: "probe".into(), role: "Backend".into(), agent: "codex".into(), session: Some(probe), ..Default::default() });
        child.tasks.push(Task { id: 1, title: "Probe".into(), assignee: "probe".into(), status: TaskStatus::InProgress, attempts: vec![Attempt { n: 1, id: "a".into(), member: "probe".into(), state: AttemptState::Delivered, ..Default::default() }], ..Default::default() });
        store.create(&uuid::Uuid::new_v4().to_string(), &json!({}), parent, vec![]).unwrap();
        store.create(&uuid::Uuid::new_v4().to_string(), &json!({}), child, vec![]).unwrap();
        commands::close_scratch_teams(&mut store, "parent", 1).unwrap();
        let back = store.load("scratch-1").unwrap().unwrap();
        assert_eq!(back.status, TeamStatus::Stopped);
        let parent = store.load("parent").unwrap().unwrap();
        assert_eq!(parent.scratch_teams[0].state, "stopped");
        assert!(parent.scratch_teams[0].stopped_at.is_some());
        let stops = commands::lead_stops();
        assert!(stops.iter().any(|(_, name)| name == "codex/x/probe"), "the scratch member's session stopped: {stops:?}");
        assert!(!stops.iter().any(|(_, name)| name == "kimi/x/qa"), "the creating member's own session stays: {stops:?}");
        // Deciding again (or for another task) does not stop it twice.
        commands::close_scratch_teams(&mut store, "parent", 1).unwrap();
        assert_eq!(commands::lead_stops().len(), 1);
        commands::record_lead_stops(false);
    });
}
