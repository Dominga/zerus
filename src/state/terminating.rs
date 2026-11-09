//! A reserved archive/cleanup fence for the future Unix Terminate controller.
//! This leaf does not create markers, send signals or authorize finalization.
use serde_json::Value;

// Inactive durable-writer support. No marker/controller/effect entry calls it;
// saved rows and native locks supply no Terminate or cleanup authority.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[allow(dead_code)]
mod writer_records;

pub(super) const WAIT: &str =
    "Session termination is unfinished; inspect its original result before changing the session";

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

/// Admission fence, not proof that an asynchronous writer has physically
/// settled. Callers retain their existing locking and native identity checks;
/// the future controller must also own the original mutation/IO custody.
pub(super) fn check_mutation(record: &Value) -> super::Result<()> {
    if pending(record) {
        return Err(WAIT.into());
    }
    Ok(())
}

/// Only completion observations of the already verified original run may
/// continue through the hook reducer. In particular, SessionStart cannot
/// replace the binding while a termination marker is present.
pub(super) fn completion_hook(record: &Value, kind: &str) -> bool {
    !pending(record)
        || matches!(
            kind,
            "SessionEnd"
                | "PostToolUse"
                | "PostToolUseFailure"
                | "PostCompact"
                | "PostModelSwitch"
                | "Stop"
                | "StopFailure"
                | "SubagentStop"
                | "Interrupt"
                | "PermissionResult"
                | "SessionHeartbeat"
        )
}

#[cfg(test)]
mod tests;
