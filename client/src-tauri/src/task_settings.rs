//! The Tasks settings dialog reads once on open and writes one partial change.
use serde_json::{Map, Value};
use tauri::State;
use zerus_client_core::{task_settings, tasks_write};

#[tauri::command]
pub async fn task_settings(
    reads: State<'_, crate::task_details::TaskReads>,
    writes: State<'_, crate::task_details::TaskWrites>,
    project: String,
    changes: Option<Map<String, Value>>,
) -> Result<task_settings::Settings, String> {
    if crate::actions::blocked() {
        return Err(crate::screens::REPLAY_NEEDS_HGS.into());
    }
    let call = task_settings::call(&project, changes.as_ref())?;
    let permit = if changes.is_some() {
        writes.begin()?
    } else {
        reads.begin()?
    };
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        let exit = zerus_client_core::run_limited(
            &zerus_client_core::default_hgs(),
            &call.invocation(),
            &[],
            256 * 1024,
        )
        .map_err(|_| "Task settings response exceeds 256 KiB.".to_owned())?;
        let reply = tasks_write::decode(exit)?;
        task_settings::checked(&reply, &project, changes.as_ref())
    })
    .await
    .map_err(|e| e.to_string())?
}
