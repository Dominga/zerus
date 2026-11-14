use super::*;
use zerus_client_core::{
    launch::{self, Launch},
    ComputerId,
};
pub(super) fn worktree(plan: &Plan) -> Vec<String> {
    let p = &plan.placement;
    [
        "worktrees",
        "create",
        "--path",
        &p.source,
        "--branch",
        &p.branch,
        "--destination",
        &p.destination,
        "--base",
        &plan.base_commit,
        "--json",
    ]
    .map(str::to_owned)
    .into()
}
pub(super) fn launch(plan: &Plan, tree: &Worktree) -> Result<Vec<String>> {
    let mut args = launch::launch_invocation(
        &ComputerId::local(),
        &Launch {
            agent: plan.agent.clone(),
            target: tree.path.clone(),
            name: if plan.session_name.is_empty() {
                // A saved pre-upgrade original keeps its exact launch arguments.
                format!("team-{}", plan.launch_id)
            } else {
                plan.session_name.clone()
            },
            account: plan.account.clone(),
            launch_id: plan.launch_id.clone(),
            model: plan.model.clone(),
            effort: plan.effort.clone(),
        },
        &plan.machine,
    )
    .args;
    if plan.agent == "opencode" {
        return Ok(args);
    }
    let mut native = Vec::new();
    if !plan.model.is_empty() {
        native.extend(["--model".into(), plan.model.clone()]);
    }
    if !plan.effort.is_empty() {
        match plan.agent.as_str() {
            "codex" => native.extend([
                "-c".into(),
                format!("model_reasoning_effort={}", json!(plan.effort)),
            ]),
            "claude" => native.extend(["--effort".into(), plan.effort.clone()]),
            _ => {
                return Err(invalid(
                    "This agent has no supported launch effort argument",
                ))
            }
        }
    }
    if plan.agent == "dsh" && !native.is_empty() {
        return Err(invalid("DeepSeek model is managed in its native account"));
    }
    if !native.is_empty() {
        args.push("--".into());
        args.extend(native);
    }
    Ok(args)
}
pub(super) fn usage(data: &Value, agent: &str, id: &str) -> Result<AccountUsage> {
    if data["id"] != id || data["provider"] != agent {
        return Err(invalid("Account inspection identity changed"));
    }
    // The provider's authenticated account reply can confirm an API key
    // even when it exposes no quota windows. A missing usage sample alone
    // must not turn it into a signed-out account.
    let method = data["identity"]["auth_method"].as_str().unwrap_or("");
    let native_auth = match agent {
        "codex" => matches!(method, "apiKey" | "apikey" | "chatgpt"),
        "kimi" => method == "Kimi Code",
        _ => false,
    };
    let signed_in = data["auth_status"] != "signed_out"
        && data["status"] != "signed_out"
        && (data["auth_status"] == "signed_in"
            || data["status"] == "ok"
            || native_auth
            || (matches!(agent, "dsh" | "opencode") && data["status"] == "configured"));
    let windows = data["windows"]
        .as_array()
        .ok_or_else(|| invalid("Account usage windows unavailable"))?;
    if windows.len() > 32 {
        return Err(invalid("Account windows exceed their bound"));
    }
    let windows = windows
        .iter()
        .map(|w| {
            let used_percent = w["used_percent"]
                .as_f64()
                .filter(|n| n.is_finite())
                .ok_or_else(|| invalid("Invalid usage window"))?;
            let resets_at = w["resets_at"].as_i64().or_else(|| {
                w["resets_at"]
                    .as_str()
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|t| t.timestamp())
            });
            Ok(UsageWindow {
                used_percent,
                resets_at,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(AccountUsage {
        id: id.into(),
        agent: agent.into(),
        signed_in,
        windows,
    })
}
