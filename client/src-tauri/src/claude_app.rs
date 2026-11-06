//! WhiteWh fork: the Claude desktop app's sessions in the Tauri client
//! (`tray/src/ClaudeAppSessionsDialog.h`). The reads and the resume call go
//! through `zerus_client_core::claude_app`; the webview only shows them.
use zerus_client_core::{claude_app, ComputerId};

/// A replay without an explicit `ZERUS_CLIENT_HGS` never reaches the real CLI;
/// these commands are reads and a launch, so they obey the actions' rule.
fn refusal(replay: Option<&str>, hgs: Option<&str>) -> Option<&'static str> {
    (!crate::actions::actions_allowed(replay, hgs)).then_some(crate::actions::REPLAY_NEEDS_HGS)
}

fn allowed() -> Result<(), String> {
    match refusal(
        std::env::var("ZERUS_CLIENT_REPLAY").ok().as_deref(),
        std::env::var("ZERUS_CLIENT_HGS").ok().as_deref(),
    ) {
        Some(reason) => Err(reason.into()),
        None => Ok(()),
    }
}

/// `hgs [@host] claude-app ls`: the app's newest sessions, their State and
/// whether each folder is `folder` (Qt's "This folder only").
#[tauri::command]
pub async fn claude_app_sessions(host: String, folder: String) -> Result<claude_app::View, String> {
    allowed()?;
    let computer = ComputerId::from(host.as_str());
    let invocation = claude_app::list_invocation(&computer);
    let hgs = zerus_client_core::default_hgs();
    tauri::async_runtime::spawn_blocking(move || {
        let exit = zerus_client_core::run_limited(&hgs, &invocation, &[], claude_app::LIST_LIMIT)
            .unwrap_or_else(|_| zerus_client_core::Exit::StartFailed(claude_app::INVALID.into()));
        claude_app::listed(exit).map(|list| claude_app::view(&list, &folder))
    })
    .await
    .map_err(|error| error.to_string())?
}

/// `hgs [@host] claude-app resume ID -n NAME --launch-id UUID -d [--force]`.
/// The launch id is this call's request identity: it is returned so the
/// webview can wait for the exact session the launch creates.
#[tauri::command]
pub async fn claude_app_resume(
    host: String,
    session: String,
    name: String,
    force: bool,
) -> Result<String, String> {
    allowed()?;
    let computer = ComputerId::from(host.as_str());
    let launch_id = uuid::Uuid::new_v4().to_string();
    let invocation = claude_app::resume_invocation(&computer, &session, &name, &launch_id, force)?;
    let hgs = zerus_client_core::default_hgs();
    tauri::async_runtime::spawn_blocking(move || {
        let exit = zerus_client_core::run_limited(&hgs, &invocation, &[], 64 * 1024)
            .unwrap_or_else(|_| zerus_client_core::Exit::StartFailed(claude_app::INVALID.into()));
        claude_app::resumed(exit).map(|()| launch_id)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_replay_without_an_explicit_hgs_is_refused() {
        assert_eq!(
            refusal(Some("replay.ndjson"), None),
            Some(crate::actions::REPLAY_NEEDS_HGS)
        );
        assert_eq!(
            refusal(Some("replay.ndjson"), Some("")),
            Some(crate::actions::REPLAY_NEEDS_HGS)
        );
        assert_eq!(refusal(Some("replay.ndjson"), Some("/tmp/hgs")), None);
        assert_eq!(refusal(None, None), None);
    }
}
