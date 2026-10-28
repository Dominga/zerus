//! Settings > Worktrees (WorktreeSettings.h): a computer's cleanup rules,
//! read and changed through its own `hgs` on the blocking pool. Nothing
//! here removes a checkout or starts a maintenance pass. A replay never
//! drives the CLI.

use serde_json::{Map, Value};
use tauri::{AppHandle, Manager, State};
use zerus_client_core::default_hgs;
use zerus_client_core::worktree_policy::{self, Policy, Rules};

use crate::commands::AppState;

/// Whether this is a replay.
pub struct WorktreePolicy {
    blocked: bool,
}

pub fn start() -> WorktreePolicy {
    WorktreePolicy {
        blocked: crate::actions::blocked(),
    }
}

/// The computers the page offers besides this one: at most 32 peers.
#[tauri::command(async)]
pub fn worktree_policy_peers(app: AppHandle) -> Vec<String> {
    app.state::<AppState>()
        .peers()
        .unwrap_or_default()
        .into_iter()
        .filter(|peer| !peer.is_empty())
        .take(32)
        .collect()
}

async fn run(
    state: &WorktreePolicy,
    route: String,
    change: Option<Map<String, Value>>,
) -> Result<Policy, String> {
    if state.blocked {
        return Err("Cleanup settings are not read in a replay.".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        worktree_policy::call(&default_hgs(), &route, change.as_ref())
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Refresh, opening the section, and choosing a computer: its policy.
#[tauri::command]
pub async fn worktree_policy_read(
    state: State<'_, WorktreePolicy>,
    route: String,
) -> Result<Policy, String> {
    run(&state, route, None).await
}

/// Apply to this computer: the policy read, with these rules; the CLI
/// refuses it when the policy changed meanwhile.
#[tauri::command]
pub async fn worktree_policy_apply(
    state: State<'_, WorktreePolicy>,
    route: String,
    read: Map<String, Value>,
    rules: Rules,
) -> Result<Policy, String> {
    let change = worktree_policy::change(&read, &rules)?;
    run(&state, route, Some(change)).await
}
