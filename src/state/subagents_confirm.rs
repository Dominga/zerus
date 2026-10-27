//! U19: the owner confirms subagents before an agent starts them. One global
//! setting, on by default, and the Claude PreToolUse reason that names each
//! subagent's type, model, effort and description.
use super::*;
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
    root().join(format!("subagents-turn-{session}.json"))
}

/// Appends one call to the current turn's group and returns the group.
pub(crate) fn group_add(session: &str, input: &Value) -> Vec<Value> {
    let path = group_path(session);
    let mut calls: Vec<Value> = fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    calls.push(input.clone());
    let _ = fs::write(&path, serde_json::to_string(&calls).unwrap_or_default());
    calls
}

/// A new owner prompt starts a new turn.
pub(crate) fn group_reset(session: &str) {
    let _ = fs::remove_file(group_path(session));
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
