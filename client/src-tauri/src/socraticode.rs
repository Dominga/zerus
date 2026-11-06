//! WhiteWh fork: the SocratiCode panel's reads and actions in the Tauri client
//! (`tray/src/SocratiCodePanel.h`). The calls go through
//! `zerus_client_core::socraticode`; the webview only shows the projection.
use zerus_client_core::{socraticode, ComputerId};

/// A replay without an explicit `ZERUS_CLIENT_HGS` never reaches the real CLI.
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

/// `hgs [@host] socraticode status PATH --qdrant native|docker`: the folder's
/// index, Qdrant, Ollama and runner as Qt's panel renders them.
#[tauri::command]
pub async fn socraticode_status(
    host: String,
    path: String,
    native: bool,
) -> Result<socraticode::Panel, String> {
    allowed()?;
    let computer = ComputerId::from(host.as_str());
    let invocation = socraticode::invocation(&computer, socraticode::Act::Status, &path, native)?;
    let hgs = zerus_client_core::default_hgs();
    tauri::async_runtime::spawn_blocking(move || {
        let exit =
            zerus_client_core::run_limited(&hgs, &invocation, &[], socraticode::STATUS_LIMIT)
                .unwrap_or_else(|_| {
                    zerus_client_core::Exit::StartFailed(socraticode::INVALID.into())
                });
        socraticode::status(exit).map(|status| socraticode::panel(&status))
    })
    .await
    .map_err(|error| error.to_string())?
}

/// One of `install`, `install-qdrant`, `start`, `stop` or `connect`, with Qt's
/// arguments and timeout. The answer only says whether the step ran; the
/// panel reads the new status afterwards.
#[tauri::command]
pub async fn socraticode_act(
    host: String,
    action: String,
    path: String,
    native: bool,
) -> Result<socraticode::Acted, String> {
    allowed()?;
    let act = socraticode::Act::parse(&action)
        .ok_or_else(|| format!("Unknown SocratiCode action {action}."))?;
    let computer = ComputerId::from(host.as_str());
    let invocation = socraticode::invocation(&computer, act, &path, native)?;
    let hgs = zerus_client_core::default_hgs();
    tauri::async_runtime::spawn_blocking(move || {
        let exit = zerus_client_core::run_limited(&hgs, &invocation, &[], 1024 * 1024)
            .unwrap_or_else(|_| zerus_client_core::Exit::StartFailed(socraticode::INVALID.into()));
        socraticode::acted(exit)
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
        assert_eq!(refusal(Some("replay.ndjson"), Some("/tmp/hgs")), None);
        assert_eq!(refusal(None, None), None);
    }

    #[test]
    fn only_the_cli_verbs_are_actions() {
        assert!(socraticode::Act::parse("install-qdrant").is_some());
        assert!(socraticode::Act::parse("status").is_some());
        assert!(socraticode::Act::parse("index").is_none());
        assert!(socraticode::Act::parse("--force").is_none());
    }
}
