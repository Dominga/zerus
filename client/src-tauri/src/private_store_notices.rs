//! Task 75: the private-store notices already raised on this device, kept
//! for the next start so a notice is raised once.
use zerus_client_core::private_notices;

/// The notices already raised on this device (at most 512).
#[tauri::command]
pub fn private_store_notices_known(app: tauri::AppHandle) -> Vec<String> {
    private_notices::load(&crate::data_dir(&app).join(private_notices::FILE))
}

/// Keeps the notices raised so far for the next start.
#[tauri::command]
pub async fn private_store_notices_save(
    app: tauri::AppHandle,
    known: Vec<String>,
) -> Result<(), String> {
    let path = crate::data_dir(&app).join(private_notices::FILE);
    tauri::async_runtime::spawn_blocking(move || private_notices::save(&path, &known))
        .await
        .map_err(|error| error.to_string())?
}
