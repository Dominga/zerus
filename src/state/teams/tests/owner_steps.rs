use super::*;
use crate::state::teams::model::{Task, TaskStatus};

fn team() -> Team {
    let mut team = Team { id: "t".into(), name: "Core".into(), lead: "lead".into(), status: TeamStatus::Running, ..Default::default() };
    team.tasks.push(Task { id: 1, title: "Scratch team check".into(), assignee: "dev".into(), status: TaskStatus::Pending, ..Default::default() });
    team.tasks.push(Task { id: 2, title: "Docs".into(), assignee: "dev".into(), status: TaskStatus::Pending, ..Default::default() });
    team.tasks.push(Task { id: 3, title: "Done".into(), assignee: "dev".into(), status: TaskStatus::Accepted, ..Default::default() });
    team
}

fn folder(name: &str) -> String {
    if cfg!(windows) { format!("D:\\scratch\\{name}") } else { format!("/scratch/{name}") }
}

#[test]
fn a_rule_names_an_absolute_folder_and_known_agents() {
    let rule = rule_from(&json!({"folder":folder("teams"),"agents":["codex","claude","codex"]})).unwrap();
    assert_eq!(rule.agents, ["claude", "codex"], "sorted, once each");
    for (input, said) in [
        (json!({"folder":"relative","agents":["claude"]}), "absolute path"),
        (json!({"folder":folder("x"),"agents":[]}), "1 to 5 agents"),
        (json!({"folder":folder("x"),"agents":["gpt"]}), "gpt is no agent"),
    ] {
        let error = rule_from(&input).unwrap_err().to_string();
        assert!(error.contains(said), "{input}: {error}");
    }
    assert_eq!(trusted(&rule_from(&json!({"folder":folder("x"),"agents":["claude","kimi","dsh"]})).unwrap()).into_iter().collect::<Vec<_>>(), ["claude"]);
}

#[test]
fn rules_are_kept_in_settings_listed_and_revoked() {
    let dir = tempfile::tempdir().unwrap();
    super::super::templates::settings_override(Some(dir.path().join("templates.json")));
    assert!(rules().is_empty());
    let mut rule = rule_from(&json!({"folder":folder("teams"),"agents":["claude"]})).unwrap();
    rule.team = "t".into();
    let added = add(rule.clone()).unwrap();
    assert!(added.id.starts_with('s') && added.granted_at > 0.0);
    assert_eq!(add(rule).unwrap().id, added.id, "the same grant is kept once");
    assert_eq!(rules().len(), 1);
    // The settings' other keys stay.
    let settings = super::super::templates::read_settings();
    assert_eq!(settings["enabled"], json!(true));
    assert!(covering(&rules(), &folder("teams/140-check")).is_some());
    assert!(covering(&rules(), &folder("teams")).is_some());
    assert!(covering(&rules(), &folder("teams2")).is_none(), "a sibling with the same prefix is not inside");
    assert!(covering(&rules(), &folder("other")).is_none());
    assert!(revoke(&added.id).unwrap());
    assert!(!revoke(&added.id).unwrap());
    assert!(rules().is_empty());
    super::super::templates::settings_override(None);
}

#[test]
fn the_owner_is_away_when_locked_or_idle_and_unknown_without_a_desktop() {
    assert_eq!(presence(None), None);
    assert_eq!(presence(Some(&json!({"idle_seconds":30.0,"locked":false}))), Some(true));
    assert_eq!(presence(Some(&json!({"idle_seconds":AWAY_AFTER + 1.0,"locked":false}))), Some(false));
    assert_eq!(presence(Some(&json!({"idle_seconds":0.0,"locked":true}))), Some(false));
}

fn waiting_items() -> Vec<Value> {
    vec![
        json!({"id":"t/owner_word_asked/proposal-p1","team":"t","kind":"owner_word_asked","proposal":"p1","title":"The lead proposes: rights"}),
        json!({"id":"t/owner_word_asked/proposal-p2","team":"t","kind":"owner_word_asked","proposal":"p2","title":"The lead proposes: scratch rule"}),
        json!({"id":"t/phase_draft/2","team":"t","kind":"phase_draft","title":"Phase 2 waits"}),
    ]
}

#[test]
fn cards_wait_for_the_owner_and_come_as_one_batch_with_what_waits_on_them() {
    let mut team = team();
    team.proposals.push(json!({"id":"p1","state":"open","kind":"rights","for_tasks":[1]}));
    team.proposals.push(json!({"id":"p2","state":"open","kind":"scratch-rule"}));
    let mut away = waiting_items();
    mark_cards(&mut away, std::slice::from_ref(&team), Some(false));
    assert_eq!((away[0]["notify"].as_bool(), away[0]["held_for_presence"].as_bool()), (Some(false), Some(true)));
    assert!(away[2].get("notify").is_none(), "only cards are held");
    let mut back = waiting_items();
    mark_cards(&mut back, std::slice::from_ref(&team), Some(true));
    assert_eq!(back[0]["notify"], true);
    assert_eq!(back[0]["batch"], back[1]["batch"], "one batch");
    assert_eq!(back[0]["batch_size"], 2);
    assert_eq!(back[0]["title"], "The lead proposes: rights (#1 wait for it)");
    assert_eq!(back[0]["waiting_tasks"], json!([{"task":1,"title":"Scratch team check"}]));
    // Without a desktop's word nothing is held.
    let mut unknown = waiting_items();
    mark_cards(&mut unknown, std::slice::from_ref(&team), None);
    assert_eq!(unknown[1]["notify"], true);
}

#[test]
fn a_card_holds_its_tasks_back_and_names_only_open_ones() {
    let mut team = team();
    assert_eq!(for_tasks(&team, &json!({"for_tasks":[1,2,1]})).unwrap(), [1, 2]);
    assert!(for_tasks(&team, &json!({})).unwrap().is_empty());
    assert!(for_tasks(&team, &json!({"for_tasks":[9]})).unwrap_err().to_string().contains("Unknown task #9"));
    assert!(for_tasks(&team, &json!({"for_tasks":[3]})).unwrap_err().to_string().contains("already decided"));
    assert!(for_tasks(&team, &json!({"for_tasks":"1"})).is_err());
    team.proposals.push(json!({"id":"p1","state":"open","for_tasks":[1]}));
    assert_eq!(waiting_on_cards(&team).get(&1), Some(&vec!["p1".to_owned()]));
    assert_eq!(super::super::plan::ready(&team), [2], "#1 waits for the owner's step");
    // Granted: it is ready.
    team.proposals[0]["state"] = json!("accepted");
    assert_eq!(super::super::plan::ready(&team), [1, 2]);
    // Denied: held for the lead with your reason, never handed out.
    team.proposals[0]["state"] = json!("rejected");
    let mut events = Vec::new();
    super::super::ops::hold_for_denied(&mut team, "p1", "not tonight", &mut events);
    let held = team.task(1).unwrap().hold.clone().expect("held");
    assert_eq!(held.reason, "you denied card p1: not tonight");
    assert_eq!(super::super::plan::ready(&team), [2]);
}

#[test]
fn a_team_keeps_one_standing_scratch_rule() {
    let dir = tempfile::tempdir().unwrap();
    super::super::templates::settings_override(Some(dir.path().join("templates.json")));
    let mut first = rule_from(&json!({"folder":folder("teams"),"agents":["kimi"]})).unwrap();
    first.team = "t".into();
    let first = add_standing(first).unwrap();
    // The same grant is kept once; a different grant replaces the team's rule.
    let mut again = rule_from(&json!({"folder":folder("teams"),"agents":["kimi"]})).unwrap();
    again.team = "t".into();
    assert_eq!(add_standing(again).unwrap().id, first.id);
    let mut moved = rule_from(&json!({"folder":folder("teams-2"),"agents":["kimi"]})).unwrap();
    moved.team = "t".into();
    let moved = add_standing(moved).unwrap();
    assert_ne!(moved.id, first.id);
    assert_eq!(rules().len(), 1, "one standing rule per team");
    assert_eq!(standing("t", &folder("teams-2")).map(|rule| rule.id), Some(moved.id));
    assert!(standing("t", &folder("teams")).is_none(), "the old root kept no rule");
    // Another team keeps its own rule.
    let mut other = rule_from(&json!({"folder":folder("teams"),"agents":["kimi"]})).unwrap();
    other.team = "u".into();
    add_standing(other).unwrap();
    assert_eq!(rules().len(), 2);
    super::super::templates::settings_override(None);
}

#[test]
fn codex_trust_is_added_once_and_keeps_the_config() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "cli_auth_credentials_store = \"file\"\nmodel = \"gpt-5\"\n").unwrap();
    let scratch = folder("teams");
    assert!(crate::accounts::trust_codex_folder(&path, &scratch).unwrap());
    assert!(!crate::accounts::trust_codex_folder(&path, &scratch).unwrap(), "already trusted");
    let document: toml_edit::DocumentMut = std::fs::read_to_string(&path).unwrap().parse().unwrap();
    assert_eq!(document["projects"][scratch.as_str()]["trust_level"].as_str(), Some("trusted"));
    assert_eq!(document["model"].as_str(), Some("gpt-5"));
    assert_eq!(document["cli_auth_credentials_store"].as_str(), Some("file"));
}
