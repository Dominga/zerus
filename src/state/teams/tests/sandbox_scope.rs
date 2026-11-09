// Task #346 (owner, 2026-11-08): a sandboxed member's own-scope team calls
// need no approval. The scope names its own work and its scratch teams under
// the owner's standing rule; a single `hgs team <op> --json-file PATH` call in
// that scope is answered by Zerus itself, while a script, a wrapper, another
// team or a path outside the member's folder still asks the lead.
use super::*;
use crate::state::teams::model::{Member, ScratchParent, SessionRef, Team, TeamStatus};
use crate::state::teams::store::Store;
use crate::state::teams::{owner_steps, scratch, templates};
use std::path::Path;

/// A running team with a lead, a QA member carrying the operate profile (the QA
/// right), and a dev, each in its own folder, with task #1 open for QA.
fn parent(project: &Path, qa_folder: &Path) -> Team {
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
    team
}

/// The child scratch team a QA member leads, as the create writes it.
fn child(parent: &Team) -> Team {
    let mut child = Team {
        id: "child1".into(),
        name: "Live check".into(),
        folder: format!("{}/check", scratch::root(parent)),
        lead: "qa".into(),
        status: TeamStatus::Running,
        ..Default::default()
    };
    child.scratch_parent = Some(ScratchParent { team: "parent".into(), task: 1, member: "qa".into() });
    child.members.push(parent.member("qa").unwrap().clone());
    child
}

fn setup(dir: &Path) -> (Team, Member, Member, std::path::PathBuf) {
    templates::settings_override(Some(dir.join("templates.json")));
    let project = dir.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let qa_folder = dir.join("qa");
    std::fs::create_dir_all(&qa_folder).unwrap();
    let team = parent(&project, &qa_folder);
    owner_steps::add_standing(owner_steps::ScratchRule {
        team: team.id.clone(),
        folder: scratch::root(&team),
        agents: vec!["dsh".into()],
        ..Default::default()
    })
    .unwrap();
    let qa = team.member("qa").unwrap().clone();
    let dev = team.member("dev").unwrap().clone();
    (team, qa, dev, qa_folder)
}

#[test]
fn the_members_own_work_and_its_scratch_team_are_in_scope() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let (team, qa, dev, _) = setup(dir.path());
        for op in ["report", "ask", "run", "commit", "reply", "expect"] {
            assert!(own_scope(&team, &qa, op).is_some(), "{op}");
        }
        assert!(own_scope(&team, &qa, "settings").is_none());
        assert!(own_scope(&team, &team.member("lead").unwrap(), "report").is_none(), "the lead's own requests are not the member's");
        // A create inside the scratch root, with the QA right and the rule.
        let inside = json!({"folder": format!("{}/check", scratch::root(&team))});
        assert!(scratch_create(&team, &qa, &inside).is_some());
        let outside = json!({"folder": dir.path().join("elsewhere").to_string_lossy()});
        assert!(scratch_create(&team, &qa, &outside).is_none(), "outside the root");
        assert!(scratch_create(&team, &dev, &inside).is_none(), "no QA right");
        assert!(scratch_create(&team, &qa, &json!({"folder":format!("{}/x", scratch::root(&team)),"parent":{"team":"t","task":1}})).is_none(), "a sub-team is not scratch");
        // The child the member leads: plan, approve and relaunch are its own.
        let child = child(&team);
        for op in ["plan", "approve", "relaunch"] {
            assert!(scratch_child(&team, &qa, &child, op).is_some(), "{op}");
        }
        assert!(scratch_child(&team, &dev, &child, "plan").is_none(), "another member");
        assert!(scratch_child(&team, &qa, &child, "shell").is_none(), "not a team operation");
        let mut foreign = child.clone();
        foreign.scratch_parent = Some(ScratchParent { team: "other".into(), task: 1, member: "qa".into() });
        assert!(scratch_child(&team, &qa, &foreign, "plan").is_none(), "another parent team");
        // The rule gone: the same call is outside the scope.
        owner_steps::revoke(&scratch::standing(&team).unwrap().id).unwrap();
        assert!(scratch_child(&team, &qa, &child, "plan").is_none(), "no standing rule");
    });
}

/// Writes a request file into the QA member's folder.
fn request(folder: &Path, name: &str, input: &Value) -> String {
    let path = folder.join(name);
    std::fs::write(&path, serde_json::to_vec(input).unwrap()).unwrap();
    path.display().to_string()
}

#[test]
fn a_single_team_call_in_scope_is_classified_and_a_script_or_another_scope_is_not() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let (team, qa, _, folder) = setup(dir.path());
        let child = child(&team);
        // Both teams are stored: a call for the child is checked against them.
        let mut store = Store::open(true).unwrap();
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), team.clone(), vec![]).unwrap();
        store.create("11111111-1111-4111-8111-111111111112", &json!({}), child.clone(), vec![]).unwrap();
        let plan = request(&folder, "plan.json", &json!({"team":"child1","tasks":[{"id":1,"title":"Check","assignee":"qa"}]}));
        let call = format!("hgs team plan --json-file {plan}");
        let action = match_call(&team, &qa, &call).expect("an in-scope single call");
        assert_eq!(action["op"], json!("plan"));
        assert!(action["reason"].as_str().unwrap().contains("scratch team"), "{action}");
        // The same call with anything appended, wrapped or piped is a script.
        assert!(match_call(&team, &qa, &format!("{call}; rm -rf /")).is_none(), "a chain");
        assert!(match_call(&team, &qa, &format!("pwsh -Command \"{call}\"")).is_none(), "a wrapper");
        assert!(match_call(&team, &qa, &format!("type plan.json | {call}")).is_none(), "a pipe");
        assert!(match_call(&team, &qa, "hgs team plan --json").is_none(), "stdin is no single call");
        // The same call for another team, or a file outside the member's folder.
        let elsewhere = request(dir.path(), "other.json", &json!({"team":"elsewhere"}));
        assert!(match_call(&team, &qa, &format!("hgs team plan --json-file {elsewhere}")).is_none(), "outside the folder");
        let other = request(&folder, "other-team.json", &json!({"team":"parent"}));
        assert!(match_call(&team, &qa, &format!("hgs team plan --json-file {other}")).is_none(), "another team");
        // The member's own work is in scope; a create inside the root too.
        let report = request(&folder, "report.json", &json!({"team":"parent","task":1,"attempt":"a1","text":"done"}));
        assert!(match_call(&team, &qa, &format!("hgs team report --json-file {report}")).is_some());
        let create = request(&folder, "create.json", &json!({"folder":format!("{}/check", scratch::root(&team)),"name":"Live check"}));
        assert!(match_call(&team, &qa, &format!("hgs team create --json-file {create}")).is_some());
        let outside_create = request(&folder, "outside.json", &json!({"folder":dir.path().join("elsewhere").to_string_lossy()}));
        assert!(match_call(&team, &qa, &format!("hgs team create --json-file {outside_create}")).is_none());
        // The stored records are what the escalation is checked against too.
        let stored = store.load("parent").unwrap().unwrap();
        let qa = stored.member("qa").unwrap().clone();
        assert!(match_call(&stored, &qa, &call).is_some(), "the stored child is the member's");
    });
}

