//! Task #232 (owner, 2026-11-07: finding F3 of live check 170): a Codex
//! member's per-command approval is a pending question, routed like Claude's:
//! the lead's inbox, the owner's Waiting and team look, and the scratch rule's
//! launch trust covers the team's own CLI.
use super::support::*;
use super::*;
use std::cell::RefCell;
use std::rc::Rc;

fn codex_card(id: &str, command: &str) -> Value {
    json!({"question_id":format!("codex-approval:{id}"),"question_hash":format!("h-{id}"),
        "tool_call_id":"codex_tool_approval","agent_id":"main","run_id":"r1","conversation_id":"c",
        "created_at":0,"source":"codex_tool_approval","approval":true,
        "answer_transport":"codex_tui","answer_unavailable_reason":"","tool":"Bash",
        "tool_input":{"command":command,"cwd":"D:/scratch/live"},
        "questions":[{"id":"approval","header":"Bash","question":"Run this command?","body":"Folder: D:/scratch/live",
            "multi_select":false,"allow_other":false,"options":[
                {"id":"exec_0","label":"Yes, proceed (y)","description":"","scope":"once"},
                {"id":"exec_1","label":"Yes, and don't ask again for this command (a)","description":"","scope":"native"},
                {"id":"exec_2","label":"No, and tell Codex what to do differently (esc)","description":"","scope":"once"}]}],
        "can_answer":true})
}

/// The dispatcher's own read of one approval: an `hgs inspect` with the card,
/// and (when something allows it) an `hgs answer` that is kept.
fn ask(store: &mut Store, team: &Team, cards: Value, answers: &Rc<RefCell<Vec<Value>>>) {
    let sent = answers.clone();
    let inspected = json!({"pending_questions":cards}).to_string();
    set_fake_hgs(Some(Box::new(move |_, args: &[&str], input: Option<&Value>| {
        if args.first() == Some(&"answer") {
            sent.borrow_mut().push(input.cloned().unwrap_or_default());
            return Ok(fake_output(0, r#"{"status":"answered"}"#, ""));
        }
        Ok(fake_output(0, &inspected, ""))
    })));
    sync_permission(store, team, "dev").unwrap();
    set_fake_hgs(None);
}

#[test]
fn a_codex_exec_approval_reaches_the_lead_waiting_and_look() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut t = team();
    t.id = "t-approval".into();
    t.settings.remind_owner_permission_minutes = 5;
    t.folder = dir.path().join("project").display().to_string();
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
    let card = codex_card("q1", "hgs guide team; hgs team inbox");
    let answers = Rc::new(RefCell::new(Vec::new()));
    ask(&mut store, &t, json!([card.clone()]), &answers);
    assert!(answers.borrow().is_empty(), "the lead's to answer, never Zerus's");
    let saved = store.load("t-approval").unwrap().unwrap();
    let pending = saved.member("dev").unwrap().pending.clone().expect("a pending question");
    assert_eq!(pending.question_id, card["question_id"].as_str().unwrap());
    assert!(pending.approval);
    // The lead's inbox item names the tool, the command and the answer command.
    let item = inbox::items(&saved).into_iter().find(|item| item.kind == "permission").expect("the lead's item");
    assert!(item.summary.contains("Bash: Run this command?"), "{}", item.summary);
    assert!(item.command.contains("hgs team answer"), "{}", item.command);
    assert!(item.detail.contains("hgs guide team; hgs team inbox"), "{}", item.detail);
    // team look names the request it waits on.
    let dev = saved.member("dev").unwrap();
    let look = super::super::look::snapshot(&saved, dev, &MemberCheck::default(), pending.at + 1.0);
    assert!(look["next"].as_str().unwrap().contains("request waits for the lead's answer"), "{look}");
    assert!(look["request"].as_str().unwrap().contains("Bash: Run this command?"), "{look}");
    // After the owner reminder time the owner's Waiting carries it.
    let mut waited = saved.clone();
    waited.members.iter_mut().find(|member| member.id == "dev").unwrap().pending.as_mut().unwrap().at = crate::state::now() - 600.0;
    let at = crate::state::now();
    let items = super::super::owner_waiting::items(std::slice::from_ref(&waited), at);
    let found = items.iter().find(|item| item["kind"] == "permission_escalated").expect("the owner's item");
    assert!(found["title"].as_str().unwrap().contains("Bash"), "{found}");
}

#[test]
fn the_scratch_rules_trust_allows_the_teams_own_cli_and_keeps_the_rest_for_the_lead() {
    let dir = tempfile::tempdir().unwrap();
    super::super::templates::settings_override(Some(dir.path().join("templates.json")));
    let folder = dir.path().join("scratch/live-check");
    std::fs::create_dir_all(&folder).unwrap();
    let mut rule = super::super::owner_steps::rule_from(&json!({"folder":folder.display().to_string(),"agents":["codex"]})).unwrap();
    rule.team = "t-scratch".into();
    let rule = super::super::owner_steps::add(rule).unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut t = team();
    t.id = "t-scratch".into();
    t.folder = folder.display().to_string();
    t.scratch_rule = rule.id.clone();
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
    let answers = Rc::new(RefCell::new(Vec::new()));
    // The team's own CLI: the rule allows it once, without the lead.
    ask(&mut store, &t, json!([codex_card("h1", "hgs guide team; hgs team inbox")]), &answers);
    assert_eq!(answers.borrow().len(), 1, "allowed by the rule");
    assert_eq!(answers.borrow()[0]["answers"], json!([{"question_id":"approval","selected_option_ids":["exec_0"]}]));
    let saved = store.load("t-scratch").unwrap().unwrap();
    let dev = saved.member("dev").unwrap();
    assert!(dev.pending.is_none() && dev.permission.is_none());
    let last = dev.permission_answers.last().unwrap();
    assert_eq!(last["by"], "safe action scratch-hgs", "{last}");
    assert!(last["reason"].as_str().unwrap().contains("trusts the team's own CLI"), "{last}");
    assert!(!inbox::items(&saved).iter().any(|item| item.kind == "permission"), "nothing waits for the lead");
    // Any other command still reaches the lead.
    sync_permission_forget("t-scratch", "dev");
    ask(&mut store, &t, json!([codex_card("q2", "cargo build --release")]), &answers);
    let saved = store.load("t-scratch").unwrap().unwrap();
    assert_eq!(saved.member("dev").unwrap().pending.as_ref().unwrap().question_id, "codex-approval:q2");
    assert!(inbox::items(&saved).iter().any(|item| item.kind == "permission"));
    super::super::templates::settings_override(None);
}
