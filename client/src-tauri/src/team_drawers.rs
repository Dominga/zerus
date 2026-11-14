//! Drawer reads share the core's bounded CLI runner. Each request returns one
//! projected page; no service, action, or unbounded journal is opened here.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use zerus_client_core::teams::drawers::{DrawerPage, DrawerRead};

/// At most one drawer child in the application, even across discarded views.
#[derive(Default)]
pub struct DrawerReader(Arc<AtomicBool>);
struct Lease(Arc<AtomicBool>);
impl DrawerReader {
    fn begin(&self) -> Result<Arc<Lease>, String> {
        self.0
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| {
                "drawer_busy: A team drawer read is still finishing. Use Refresh.".to_owned()
            })?;
        Ok(Arc::new(Lease(Arc::clone(&self.0))))
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

#[tauri::command]
pub async fn teams_drawer_page(
    reader: tauri::State<'_, DrawerReader>,
    read: DrawerRead,
) -> Result<DrawerPage, String> {
    let lease = reader.begin()?;
    let hgs = zerus_client_core::default_hgs();
    tauri::async_runtime::spawn_blocking(move || {
        zerus_client_core::teams::drawers::read(&hgs, &read, lease)
            .map_err(|error| format!("{}: {}", error.code(), error.message()))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn admission_waits_for_all_accounted_resource_owners() {
        let reader = DrawerReader::default();
        let lease = reader.begin().unwrap();
        // The runner's retained resources outlive its bounded return until
        // physical child/pipe retirement, not merely the command future.
        let retained = Arc::clone(&lease);
        assert!(reader.begin().is_err());
        drop(lease);
        assert!(reader.begin().is_err());
        drop(retained);
        assert!(reader.begin().is_ok());
    }
}
