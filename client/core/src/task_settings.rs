//! On-demand project task settings. Only named changes cross the CLI boundary;
//! the existing project writer preserves every other setting under its lock.
use crate::tasks_write::TaskCall;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct Settings {
    pub task_template: String,
    pub cancel_template: String,
    pub agents_other: bool,
    pub agent_limit: i64,
}

pub fn call(project: &str, changes: Option<&Map<String, Value>>) -> Result<TaskCall, String> {
    if project.is_empty() || project.len() > 128 || project.chars().any(char::is_control) {
        return Err("Invalid project task settings request.".into());
    }
    let mut args = vec![
        "settings".into(),
        "--project".into(),
        project.into(),
        "--json".into(),
    ];
    let mut input = String::new();
    if let Some(changes) = changes {
        for (key, value) in changes {
            let valid = match key.as_str() {
                "task_template" | "cancel_template" => value
                    .as_str()
                    .is_some_and(|s| s.len() <= 16 * 1024 && !s.contains('\0')),
                "agents_other" => value.is_boolean(),
                "agent_limit" => value.as_i64().is_some_and(|n| (0..=1000).contains(&n)),
                _ => false,
            };
            if !valid {
                return Err("Invalid project task settings change.".into());
            }
        }
        args.push("--set".into());
        input = Value::Object(changes.clone()).to_string();
    }
    Ok(TaskCall { args, input })
}

pub fn checked(
    reply: &Map<String, Value>,
    project: &str,
    changes: Option<&Map<String, Value>>,
) -> Result<Settings, String> {
    if reply.get("project").and_then(|p| p["id"].as_str()) != Some(project) {
        return Err("The task settings reply did not match the selected project.".into());
    }
    let settings: Settings =
        serde_json::from_value(reply.get("settings").cloned().unwrap_or(Value::Null))
            .map_err(|_| "The project task settings could not be read.".to_owned())?;
    if settings.task_template.len() > 16 * 1024
        || settings.cancel_template.len() > 16 * 1024
        || settings.agent_limit < 0
    {
        return Err("The project task settings could not be read.".into());
    }
    let saved = serde_json::to_value(&settings).map_err(|e| e.to_string())?;
    if changes.is_some_and(|changes| {
        changes
            .iter()
            .any(|(key, value)| saved.get(key) != Some(value))
    }) {
        return Err(
            "Task settings save unconfirmed. Open the project settings again before changing them."
                .into(),
        );
    }
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn bounded_reads_and_partial_changes_keep_their_project() {
        assert_eq!(
            call("example", None).unwrap(),
            TaskCall {
                args: vec![
                    "settings".into(),
                    "--project".into(),
                    "example".into(),
                    "--json".into()
                ],
                input: String::new()
            }
        );
        let changes = json!({"agent_limit":5,"task_template":"Task {ref}: {prompt}"});
        let save = call("example", changes.as_object()).unwrap();
        assert_eq!(save.args.last().unwrap(), "--set");
        assert_eq!(serde_json::from_str::<Value>(&save.input).unwrap(), changes);
        for bad in [
            json!({"agent_limit":-1}),
            json!({"agent_limit":1001}),
            json!({"agents_other":"yes"}),
            json!({"other":"unknown"}),
        ] {
            assert!(call("example", bad.as_object()).is_err());
        }
        assert!(call("", None).is_err());
    }
    #[test]
    fn foreign_or_incomplete_replies_never_seed_the_form() {
        let mut reply = json!({"project":{"id":"example"},"settings":{"task_template":"Task {prompt}","cancel_template":"Cancel {ref}","agents_other":true,"agent_limit":0,"future":"preserved by the CLI"}}).as_object().unwrap().clone();
        assert_eq!(checked(&reply, "example", None).unwrap().agent_limit, 0);
        assert!(checked(&reply, "other", None).is_err());
        assert!(checked(&reply, "example", json!({"agent_limit": 3}).as_object()).is_err());
        reply
            .get_mut("settings")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove("agents_other");
        assert!(checked(&reply, "example", None).is_err());
    }
}
