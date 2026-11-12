//! Task 50: the models a catalog role's account offers, for the catalog
//! editor's model and effort menus (Qt TeamCatalogEditor::requestModels):
//! `hgs [@computer] account model-settings <account> --json`, where a role
//! without an account names `native-<agent>`. Read only; bounded.
use crate::actions::{failure_detail, Exit, Invocation};
use crate::fleet::valid_alias;
use serde_json::Value;
use std::path::Path;
use std::time::Duration;

/// The agents whose accounts report a model catalog (Qt's list).
pub const AGENTS: &[&str] = &["claude", "codex", "kimi", "dsh"];

pub type RoleModel = crate::model_catalog::Model;

/// The account a role's models come from: its own, or the agent's native one.
pub fn account_of(agent: &str, account: &str) -> Result<String, String> {
    let agent = if agent.is_empty() { "claude" } else { agent };
    if !AGENTS.contains(&agent) {
        return Err(format!("{agent} reports no model catalog"));
    }
    let account = if account.is_empty() {
        format!("native-{agent}")
    } else {
        account.to_owned()
    };
    if account.len() > 128
        || account.starts_with('-')
        || account.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return Err("Not an account.".into());
    }
    Ok(account)
}

/// The command: `hgs [@alias] account model-settings ACCOUNT --json`.
pub fn invocation(computer: &str, account: &str) -> Result<Invocation, String> {
    if !computer.is_empty() && !valid_alias(computer) {
        return Err("Not a computer alias.".into());
    }
    let mut args: Vec<String> = (!computer.is_empty())
        .then(|| format!("@{computer}"))
        .into_iter()
        .collect();
    args.extend([
        "account".into(),
        "model-settings".into(),
        account.to_owned(),
        "--json".into(),
    ]);
    Ok(Invocation {
        args,
        stdin: None,
        timeout: Duration::from_secs(if computer.is_empty() { 20 } else { 45 }),
        request_id: None,
    })
}

/// The reply's models: at most 256, each once, with at most 64 efforts.
pub fn parse(value: &Value) -> Result<Vec<RoleModel>, String> {
    crate::model_catalog::parse_models(value)
}

/// Reads one role's models.
pub fn read(
    hgs: &Path,
    computer: &str,
    agent: &str,
    account: &str,
) -> Result<Vec<RoleModel>, String> {
    let account = account_of(agent, account)?;
    let call = invocation(computer, &account)?;
    let exit = crate::run::run_limited(hgs, &call, &[], 4 * 1024 * 1024)
        .map_err(|_| "The model list is too large.".to_owned())?;
    match exit {
        Exit::Code {
            code: 0, stdout, ..
        } => parse(
            &serde_json::from_slice(&stdout)
                .map_err(|_| "Invalid account model settings response".to_owned())?,
        ),
        other => Err(failure_detail(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_role_reads_its_account_or_its_agents_native_models() {
        assert_eq!(account_of("codex", "").unwrap(), "native-codex");
        assert_eq!(account_of("", "work").unwrap(), "work");
        assert!(
            account_of("opencode", "").is_err(),
            "OpenCode has no account model catalog"
        );
        assert!(account_of("claude", "-x").is_err() && account_of("claude", "a b").is_err());
        assert_eq!(
            invocation("", "work").unwrap().args,
            ["account", "model-settings", "work", "--json"]
        );
        assert_eq!(invocation("studio", "work").unwrap().args[0], "@studio");
        assert!(invocation("-oProxy=x", "work").is_err());
        let models = parse(&json!({"models":[
            {"id":"gpt-5.5","label":"GPT-5.5","effort_options":["low","high","high"]},
            {"id":"gpt-5.5","label":"again"},{"id":""},{"id":"o4"}]}))
        .unwrap();
        assert_eq!(
            models,
            vec![
                RoleModel {
                    id: "gpt-5.5".into(),
                    label: "GPT-5.5".into(),
                    effort_options: vec!["low".into(), "high".into()],
                    effort_labels: vec!["low".into(), "high".into()],
                    default_effort: String::new(),
                },
                RoleModel {
                    id: "o4".into(),
                    label: "o4".into(),
                    effort_options: vec![],
                    effort_labels: vec![],
                    default_effort: String::new(),
                },
            ]
        );
        assert!(parse(&json!({})).is_err());
    }
}
