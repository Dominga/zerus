//! U19: the owner confirms subagents before an agent starts them. One global
//! setting, on by default, and the Claude PreToolUse reason that names each
//! subagent's type, model, effort and description. The ask becomes Claude's
//! permission request, which the approval channel (claude_approval::channel)
//! turns into a card for that exact tool use; a team member's card goes to
//! its lead like any permission request. The calls one assistant message
//! makes are one group: the card names every open call, and its answer
//! decides them all. Automatic tool approvals never answer it.
use super::*;
use sha2::Digest;
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn path() -> PathBuf {
    root().join("subagents-confirm.json")
}

/// Default on for every install, including one without the file.
pub(crate) fn confirm() -> bool {
    fs::read_to_string(path())
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| value["confirm"].as_bool())
        .unwrap_or(true)
}

fn write(confirm: bool) -> Result<()> {
    let Some(dir) = path().parent().map(Path::to_path_buf) else { return Err("subagent setting has no parent".into()) };
    private_dir(&dir)?;
    let text = serde_json::to_string_pretty(&json!({"confirm":confirm})).map_err(|error| error.to_string())?;
    atomic(&path(), &text)
}

/// Is this a Claude PreToolUse call that starts a subagent?
pub(crate) fn subagent_call(event: &Value) -> bool {
    string(event, "hook_event_name") == "PreToolUse"
        && string(event, "agent_id").is_empty()
        && matches!(string(event, "tool_name"), "Agent" | "Task")
}

/// The same tool, on any event (PostToolUse removes the answered call).
pub(crate) fn subagent_tool(event: &Value) -> bool {
    string(event, "agent_id").is_empty() && matches!(string(event, "tool_name"), "Agent" | "Task")
}

/// A definition model from `.claude/agents/<type>.md` below `folder`.
fn definition_model(folder: &str, kind: &str) -> Option<String> {
    let path = Path::new(folder).join(".claude").join("agents").join(format!("{kind}.md"));
    let text = fs::read_to_string(path).ok()?;
    let front = text.strip_prefix("---")?;
    let end = front.find("\n---")?;
    front[..end].lines().find_map(|line| {
        line.trim().strip_prefix("model:").map(|model| model.trim().trim_matches('"').to_owned())
    })
}

fn group_path(session: &str) -> PathBuf {
    // Claude's session id is a UUID; anything else is named by its digest so
    // it can never leave the state folder.
    let safe = !session.is_empty() && session.len() <= 128 && session.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    let name = if safe { session.to_owned() } else { format!("{:x}", sha2::Sha256::digest(session.as_bytes())) };
    root().join(format!("subagents-turn-{name}.json"))
}

fn read_group(session: &str) -> Vec<Value> {
    fs::read_to_string(group_path(session))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Adds one unanswered call of this turn and returns the still-unanswered
/// calls, so the reason names only what this answer decides.
pub(crate) fn group_add(session: &str, tool_use_id: &str, input: &Value) -> Vec<Value> {
    let mut group = read_group(session);
    group.push(json!({"id":tool_use_id,"input":input}));
    let _ = atomic(&group_path(session), &serde_json::to_string(&group).unwrap_or_default());
    group.iter().map(|entry| entry["input"].clone()).collect()
}

/// A call answered or denied no longer belongs to the open group.
pub(crate) fn group_remove(session: &str, tool_use_id: &str) {
    let mut group = read_group(session);
    let before = group.len();
    group.retain(|entry| entry["id"].as_str() != Some(tool_use_id));
    if group.len() != before {
        let _ = atomic(&group_path(session), &serde_json::to_string(&group).unwrap_or_default());
    }
}

/// A new owner prompt, Stop or SessionEnd ends the turn.
pub(crate) fn group_reset(session: &str) {
    let _ = fs::remove_file(group_path(session));
}

/// The calls of the turn nobody decided yet: (tool use id, input).
pub(crate) fn open_calls(session: &str) -> Vec<(String, Value)> {
    read_group(session)
        .into_iter()
        .filter(|entry| entry["decision"].is_null())
        .map(|entry| (string(&entry, "id").to_owned(), entry["input"].clone()))
        .collect()
}

/// U19 (3): one answer decides the whole group it showed: `ids` are allowed
/// or denied together. Only calls still open take the decision.
pub(crate) fn decide(session: &str, ids: &[String], allow: bool) {
    let mut group = read_group(session);
    let mut changed = false;
    for entry in group.iter_mut() {
        if entry["decision"].is_null() && ids.iter().any(|id| entry["id"].as_str() == Some(id.as_str())) {
            entry["decision"] = json!(if allow { "allow" } else { "deny" });
            changed = true;
        }
    }
    if changed {
        let _ = atomic(&group_path(session), &serde_json::to_string(&group).unwrap_or_default());
    }
}

/// The group's decision for this call: "allow", "deny" or none yet.
pub(crate) fn decided(session: &str, tool_use_id: &str) -> Option<String> {
    read_group(session)
        .into_iter()
        .find(|entry| entry["id"].as_str() == Some(tool_use_id))
        .and_then(|entry| entry["decision"].as_str().map(str::to_owned))
}

/// The card text and the calls it decides, for the call `tool_use_id`: every
/// open call of its group. None for a call that is not an open subagent call.
pub(crate) fn card(session: &str, tool_use_id: &str, folder: &str, session_model: &str) -> Option<(Vec<String>, String)> {
    let open = open_calls(session);
    if !open.iter().any(|(id, _)| id == tool_use_id) {
        return None;
    }
    let ids = open.iter().map(|(id, _)| id.clone()).collect();
    let inputs: Vec<Value> = open.into_iter().map(|(_, input)| input).collect();
    Some((ids, reason(&inputs, folder, session_model)))
}

/// U19: what Claude's PreToolUse hook answers for a main-agent subagent call,
/// with the note it leaves on the record. A call its group already decided
/// gets that decision; otherwise Claude is asked, which raises the permission
/// request the approval channel carries to the owner or the member's lead.
pub(crate) fn pre_tool(record: &mut Value, event: &Value, session: &str) -> Option<Value> {
    if !confirm() || !subagent_call(event) {
        return None;
    }
    let tool = ["tool_use_id", "tool_call_id"].iter().map(|key| string(event, key)).find(|id| !id.is_empty()).unwrap_or_default();
    if let Some(decision) = decided(session, tool) {
        let allow = decision == "allow";
        record["subagent_confirm"] = json!(if allow { "allowed with its group" } else { "denied with its group" });
        return Some(json!({"hookSpecificOutput":{"hookEventName":"PreToolUse",
            "permissionDecision":if allow { "allow" } else { "deny" },
            "permissionDecisionReason":if allow { "Allowed with the other subagents of this message" }
                else { "Denied with the other subagents of this message" }}}));
    }
    let calls = group_add(session, tool, &event["tool_input"]);
    let folder = string(record, "cwd").to_owned();
    let model = string(record, "model");
    let model = if model.is_empty() { "the session model".to_owned() } else { model.to_owned() };
    if !can_ask() {
        // No one can answer a native ask in print mode; never block it.
        record["subagent_confirm"] = json!("ask skipped in a non-interactive Claude run");
        return None;
    }
    record["subagent_confirm"] = json!(if team_member(string(record, "name"), string(record, "run_id")) {
        "asked; the team's lead answers"
    } else {
        "asked"
    });
    Some(decision(&calls, &folder, &model))
}

/// A session that is a member (or the lead) of an active team. The teams store
/// is indexed by status, so this is a bounded lookup, not a full scan.
pub(crate) fn team_member(session: &str, run_id: &str) -> bool {
    if session.is_empty() {
        return false;
    }
    let Ok(store) = super::teams::store::Store::open(false) else { return false };
    let Ok(teams) = store.active_teams(1000) else { return false };
    teams.iter().any(|team| {
        team.members.iter().any(|member| {
            member.session.as_ref().is_some_and(|found| {
                found.host.is_empty()
                    && found.name == session
                    && (found.run_id.is_empty() || found.run_id == run_id)
            })
        })
    })
}

/// Claude's print mode has nobody to answer a native ask.
pub(crate) fn can_ask() -> bool {
    can_ask_with(std::env::var("CLAUDE_CODE_ENTRYPOINT").ok().as_deref())
}

fn can_ask_with(entry: Option<&str>) -> bool {
    entry.is_none_or(|entry| entry == "cli")
}

pub(crate) fn describe(input: &Value, folder: &str, session_model: &str) -> String {
    let kind = string(input, "subagent_type");
    let kind = if kind.is_empty() { "subagent".to_owned() } else { kind.to_owned() };
    let model = match string(input, "model") {
        model if !model.is_empty() => model.to_owned(),
        _ => definition_model(folder, &kind).unwrap_or_else(|| format!("inherits {session_model}")),
    };
    let effort = string(input, "effort");
    let mut text = format!("{kind} (model {model}");
    if !effort.is_empty() {
        text += &format!(", effort {effort}");
    }
    text += ")";
    let description = string(input, "description");
    if !description.is_empty() {
        text += &format!(": {description}");
    }
    text
}

pub(crate) fn reason(calls: &[Value], folder: &str, session_model: &str) -> String {
    let described: Vec<String> = calls.iter().map(|call| describe(call, folder, session_model)).collect();
    match described.len() {
        0 => "Start a subagent?".to_owned(),
        1 => format!("Start 1 subagent: {}", described[0]),
        n => format!("Start {n} subagents: {}", described.join("; ")),
    }
}

/// The Claude PreToolUse answer: always ask, never allow.
pub(crate) fn decision(calls: &[Value], folder: &str, session_model: &str) -> Value {
    json!({"hookSpecificOutput":{"hookEventName":"PreToolUse",
        "permissionDecision":"ask",
        "permissionDecisionReason":reason(calls, folder, session_model)}})
}

/// `hgs subagents-confirm get|set --json`.
pub(crate) fn dispatch(args: &[String]) -> Result<i32> {
    if args.len() != 2 || args[1] != "--json" {
        return Err("usage: hgs subagents-confirm get|set --json".into());
    }
    match args[0].as_str() {
        "get" => println!("{}", json!({"confirm":confirm()})),
        "set" => {
            let input: Value = stdin_json()?;
            if !super::teams::store::canonical_uuid(string(&input, "request_id")) {
                return Err("A canonical request_id is required".into());
            }
            let value = input["confirm"].as_bool().ok_or("confirm is true or false")?;
            write(value)?;
            println!("{}", json!({"confirm":value}));
        }
        other => return Err(format!("Unknown subagents-confirm operation {other}")),
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(kind: &str, model: &str, effort: &str, description: &str) -> Value {
        json!({"subagent_type":kind,"model":model,"effort":effort,"description":description})
    }

    #[test]
    fn one_and_three_subagents_are_named_in_the_reason() {
        let one = [call("Explore", "", "", "Find the callers")];
        assert_eq!(
            reason(&one, "/tmp", "Opus 4.1"),
            "Start 1 subagent: Explore (model inherits Opus 4.1): Find the callers"
        );
        let three = [
            call("Explore", "", "", "Find the callers"),
            call("general", "Sonnet", "high", "Write the fix"),
            call("test", "Haiku", "", "Run the suite"),
        ];
        let text = reason(&three, "/tmp", "Opus 4.1");
        assert!(text.starts_with("Start 3 subagents: "), "{text}");
        assert!(text.contains("Explore (model inherits Opus 4.1)"), "{text}");
        assert!(text.contains("Sonnet, effort high") && text.contains("Haiku"), "{text}");
        let decision = decision(&three, "/tmp", "Opus 4.1");
        assert_eq!(decision["hookSpecificOutput"]["permissionDecision"], "ask");
        assert!(decision["hookSpecificOutput"]["permissionDecisionReason"]
            .as_str()
            .unwrap()
            .starts_with("Start 3 subagents"));
    }

    #[test]
    fn confirmation_defaults_on_and_can_be_turned_off() {
        let _guard = crate::state::ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let old = std::env::var("HGS_STATE_DIR").ok();
        std::env::set_var("HGS_STATE_DIR", dir.path());
        assert!(confirm(), "the default is on");
        write(false).unwrap();
        assert!(!confirm());
        write(true).unwrap();
        assert!(confirm());
        match old {
            Some(value) => std::env::set_var("HGS_STATE_DIR", value),
            None => std::env::remove_var("HGS_STATE_DIR"),
        }
    }

    #[test]
    fn a_team_member_is_known_and_the_owner_is_not() {
        let _guard = crate::state::ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let old = std::env::var("HGS_STATE_DIR").ok();
        std::env::set_var("HGS_STATE_DIR", dir.path());
        assert!(!team_member("claude/x/lead", "r0"));
        let mut team = super::teams::model::Team {
            id: "11111111-1111-4111-8111-111111111111".into(),
            name: "T".into(),
            lead: "lead".into(),
            status: super::teams::model::TeamStatus::Running,
            ..Default::default()
        };
        team.members.push(super::teams::model::Member {
            id: "lead".into(), role: "Lead".into(), agent: "claude".into(),
            session: Some(super::teams::model::SessionRef { name: "claude/x/lead".into(), run_id: "r0".into(), ..Default::default() }),
            ..Default::default()
        });
        let mut store = super::teams::store::Store::open(true).unwrap();
        store.create("22222222-2222-4222-8222-222222222222", &json!({}), team, Vec::new()).unwrap();
        assert!(team_member("claude/x/lead", "r0"));
        assert!(!team_member("claude/x/owner", "r1"));
        match old {
            Some(value) => std::env::set_var("HGS_STATE_DIR", value),
            None => std::env::remove_var("HGS_STATE_DIR"),
        }
    }

    fn agent_call(id: &str, kind: &str, model: &str, description: &str) -> Value {
        json!({"hook_event_name":"PreToolUse","tool_name":"Agent","tool_use_id":id,"session_id":"conversation",
            "tool_input":{"subagent_type":kind,"model":model,"description":description}})
    }

    /// U19 through the hook's own decision (`pre_tool`, which hooks.rs prints):
    /// the owner's session and a team member's both ask, so Claude raises the
    /// permission request the approval channel carries; the setting off does
    /// nothing; a call its group already decided takes that decision.
    #[test]
    fn the_hook_asks_for_the_owner_and_a_member_and_follows_the_group() {
        let dir = tempfile::tempdir().unwrap();
        crate::state::with_test_state_root(dir.path(), || {
            let mut owner = json!({"agent":"claude","name":"claude/x/owner","run_id":"r9","cwd":"/work","model":"claude-opus-4-6"});
            let output = pre_tool(&mut owner, &agent_call("toolu_1", "Explore", "haiku", "Find the callers"), "conversation").unwrap();
            assert_eq!(output["hookSpecificOutput"]["permissionDecision"], "ask");
            assert_eq!(output["hookSpecificOutput"]["permissionDecisionReason"], "Start 1 subagent: Explore (model haiku): Find the callers");
            assert_eq!(owner["subagent_confirm"], "asked");
            // A second call of the same message joins the group the card shows.
            pre_tool(&mut owner, &agent_call("toolu_2", "general", "", "Write the fix"), "conversation").unwrap();
            let (calls, text) = card("conversation", "toolu_1", "/work", "claude-opus-4-6").unwrap();
            assert_eq!(calls, vec!["toolu_1".to_owned(), "toolu_2".to_owned()]);
            assert_eq!(text, "Start 2 subagents: Explore (model haiku): Find the callers; general (model inherits claude-opus-4-6): Write the fix");
            // The answer to that card decides both; a later hook for either
            // takes it without asking again.
            decide("conversation", &calls, false);
            let denied = pre_tool(&mut owner, &agent_call("toolu_2", "general", "", "Write the fix"), "conversation").unwrap();
            assert_eq!(denied["hookSpecificOutput"]["permissionDecision"], "deny");
            assert!(card("conversation", "toolu_1", "/work", "m").is_none(), "nothing open is left to show");
            group_reset("conversation");
            // A team member asks too (its lead answers): no skip any more.
            let mut store = super::super::teams::store::Store::open(true).unwrap();
            let mut team = super::super::teams::model::Team {
                id: "11111111-1111-4111-8111-111111111111".into(),
                name: "T".into(),
                lead: "lead".into(),
                status: super::super::teams::model::TeamStatus::Running,
                ..Default::default()
            };
            team.members.push(super::super::teams::model::Member {
                id: "dev".into(),
                role: "Backend".into(),
                agent: "claude".into(),
                session: Some(super::super::teams::model::SessionRef { name: "claude/x/dev".into(), run_id: "r1".into(), ..Default::default() }),
                ..Default::default()
            });
            store.create("22222222-2222-4222-8222-222222222222", &json!({}), team, Vec::new()).unwrap();
            let mut member = json!({"agent":"claude","name":"claude/x/dev","run_id":"r1","cwd":"/work"});
            let output = pre_tool(&mut member, &agent_call("toolu_3", "Explore", "", "Map the API"), "member-conversation").unwrap();
            assert_eq!(output["hookSpecificOutput"]["permissionDecision"], "ask");
            assert_eq!(member["subagent_confirm"], "asked; the team's lead answers");
            // The setting off: nothing is asked, recorded or grouped.
            write(false).unwrap();
            let mut off = json!({"agent":"claude","name":"claude/x/owner","run_id":"r9"});
            assert!(pre_tool(&mut off, &agent_call("toolu_4", "Explore", "", "x"), "off-conversation").is_none());
            assert!(off.get("subagent_confirm").is_none());
            assert!(open_calls("off-conversation").is_empty());
            write(true).unwrap();
            // Not a subagent call, or a subagent's own tool use: nothing.
            let mut bash = agent_call("toolu_5", "Explore", "", "x");
            bash["tool_name"] = json!("Bash");
            assert!(pre_tool(&mut owner, &bash, "conversation").is_none());
            let mut child = agent_call("toolu_6", "Explore", "", "x");
            child["agent_id"] = json!("child-1");
            assert!(pre_tool(&mut owner, &child, "conversation").is_none());
        });
    }

    #[test]
    fn the_turn_group_drops_answered_calls_and_resets() {
        let _guard = crate::state::ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let old = std::env::var("HGS_STATE_DIR").ok();
        std::env::set_var("HGS_STATE_DIR", dir.path());
        let first = call("Explore", "", "", "One");
        let second = call("general", "", "", "Two");
        assert_eq!(group_add("s", "t1", &first).len(), 1);
        assert_eq!(group_add("s", "t2", &second).len(), 2);
        group_remove("s", "t1");
        assert_eq!(read_group("s").len(), 1);
        assert_eq!(read_group("s")[0]["id"], "t2");
        group_reset("s");
        assert!(read_group("s").is_empty());
        match old {
            Some(value) => std::env::set_var("HGS_STATE_DIR", value),
            None => std::env::remove_var("HGS_STATE_DIR"),
        }
    }

    #[test]
    fn print_mode_cannot_be_asked() {
        assert!(can_ask_with(None));
        assert!(!can_ask_with(Some("sdk-cli")), "a print-mode run has nobody to answer");
        assert!(can_ask_with(Some("cli")));
    }

    #[test]
    fn a_definition_model_is_used_when_the_call_has_none() {
        let dir = tempfile::tempdir().unwrap();
        let agents = dir.path().join(".claude/agents");
        fs::create_dir_all(&agents).unwrap();
        fs::write(agents.join("reviewer.md"), "---\nmodel: Opus 4\n---\nReview.").unwrap();
        let text = reason(
            &[json!({"subagent_type":"reviewer","description":"Check"})],
            &dir.path().to_string_lossy(),
            "Sonnet",
        );
        assert_eq!(text, "Start 1 subagent: reviewer (model Opus 4): Check");
    }
}
