//! Bounded settings inspection off the UI thread. No write or runner here.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use zerus_client_core::teams::providers::{ConfigRead, Inspection};
#[derive(Default)]
pub struct ProviderReader(Arc<AtomicBool>);
struct Lease(Arc<AtomicBool>);
impl ProviderReader {
    fn begin(&self) -> Result<Arc<Lease>, String> {
        self.0
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| "Provider inspection is still finishing. Check again.".to_owned())?;
        Ok(Arc::new(Lease(Arc::clone(&self.0))))
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}
#[tauri::command]
pub async fn teams_provider_config(
    reader: tauri::State<'_, ProviderReader>,
    read: ConfigRead,
) -> Result<Inspection, String> {
    let lease = reader.begin()?;
    let hgs = zerus_client_core::default_hgs();
    tauri::async_runtime::spawn_blocking(move || {
        zerus_client_core::teams::providers::read(&hgs, &read, lease)
            .map_err(|e| format!("{}: {}", e.code(), e.message()))
    })
    .await
    .map_err(|e| e.to_string())?
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn provider_read_admission_waits_for_physical_resources() {
        let reader = ProviderReader::default();
        let lease = reader.begin().unwrap();
        let retained = Arc::clone(&lease);
        assert!(reader.begin().is_err());
        drop(lease);
        assert!(reader.begin().is_err());
        drop(retained);
        assert!(reader.begin().is_ok());
    }
}
