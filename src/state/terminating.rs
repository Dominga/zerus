//! A reserved archive/cleanup fence for the future Unix Terminate controller.
//! This leaf does not create markers, send signals or authorize finalization.
use serde_json::Value;

/// Presence is deliberately enough: a damaged or interrupted marker cannot
/// turn a root exit into permission to archive or clean up. Later typed claim
/// validation belongs to the live controller, never to this saved-data guard.
pub(super) fn pending(record: &Value) -> bool {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        record.get("terminating").is_some()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = record;
        false
    }
}

#[cfg(test)]
mod tests;
