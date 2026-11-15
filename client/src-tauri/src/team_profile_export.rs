//! A selected data page only; the destination comes from the native Save dialog.
use serde_json::Value;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tauri::AppHandle;

#[derive(Default)]
pub struct ProfileExporter(Arc<AtomicBool>);
struct Lease(Arc<AtomicBool>);
impl ProfileExporter {
    fn begin(&self) -> Result<Lease, String> {
        self.0
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| "A page export is still finishing.".to_owned())?;
        Ok(Lease(Arc::clone(&self.0)))
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

#[tauri::command]
pub async fn team_profile_export(
    app: AppHandle,
    exporter: tauri::State<'_, ProfileExporter>,
    value: Value,
) -> Result<bool, String> {
    use tauri_plugin_dialog::DialogExt;
    let lease = exporter.begin()?;
    let window = crate::main_window(&app);
    tauri::async_runtime::spawn_blocking(move || {
        // Held by the physical worker even if the invoking view disappears.
        let _lease = lease;
        let bytes = zerus_client_core::teams::profile_export::encode(value)?;
        let mut dialog = app
            .dialog()
            .file()
            .set_file_name("zerus-recorded-team-events.json")
            .add_filter("JSON", &["json"]);
        if let Some(window) = &window {
            dialog = dialog.set_parent(window);
        }
        let Some(file) = dialog.blocking_save_file() else {
            return Ok(false);
        };
        let path = file.into_path().map_err(|e| e.to_string())?;
        zerus_client_core::fsx::write_private_atomic(&path, &bytes, true)
            .map_err(|e| e.to_string())?;
        Ok(true)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn another_view_cannot_export_until_the_physical_worker_settles() {
        let state = ProfileExporter::default();
        let lease = state.begin().unwrap();
        let (started, ready) = std::sync::mpsc::channel();
        let (finish, finished) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let _lease = lease;
            started.send(()).unwrap();
            finished
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
        });
        ready
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        assert!(state.begin().is_err());
        finish.send(()).unwrap();
        worker.join().unwrap();
        assert!(state.begin().is_ok());
    }
}
